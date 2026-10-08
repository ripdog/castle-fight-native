use super::*;
use crate::{DamageThreshold, DamageThresholdProfile, DamageThresholdState};

fn profile() -> DamageThresholdProfile {
    DamageThresholdProfile {
        ability: AbilityId(92),
        thresholds: [
            DamageThreshold {
                hp_ratio_below_per_10k: 8000,
                attack_bonus: 7,
                vertex_rgba: [11, 22, 33, 255],
                adds_if_absent: true,
            },
            DamageThreshold {
                hp_ratio_below_per_10k: 6000,
                attack_bonus: 13,
                vertex_rgba: [44, 55, 66, 255],
                adds_if_absent: false,
            },
            DamageThreshold {
                hp_ratio_below_per_10k: 4000,
                attack_bonus: 29,
                vertex_rgba: [77, 88, 99, 255],
                adds_if_absent: false,
            },
        ],
    }
}

fn effects() -> PassiveUnitEffects {
    PassiveUnitEffects::single(PassiveUnitEffect::DamageThreshold(profile()))
}

fn observe(state: &mut DamageThresholdState, health: i32) {
    super::super::damage_thresholds::observe_damage_state(effects(), state, health, 1000);
}

#[test]
fn damage_threshold_strict_bands_skip_absent_grants_but_keep_script_color() {
    let mut state = DamageThresholdState::default();
    observe(&mut state, 800);
    assert_eq!(state, DamageThresholdState::default());
    observe(&mut state, 300);
    assert_eq!(
        (state.ability_level, state.attack_bonus, state.color_level),
        (0, 0, 3)
    );
    assert_eq!(state.vertex_rgba, Some(profile().thresholds[2].vertex_rgba));
    observe(&mut state, 600);
    assert_eq!(
        (state.ability_level, state.attack_bonus, state.color_level),
        (1, 7, 1)
    );
    observe(&mut state, 400);
    assert_eq!((state.ability_level, state.attack_bonus), (2, 13));
    observe(&mut state, 399);
    let reached = state;
    assert_eq!((state.ability_level, state.attack_bonus), (3, 29));
    observe(&mut state, 700);
    observe(&mut state, 1000);
    observe(&mut state, 0);
    assert_eq!(state, reached);
}

fn engage(sim: &mut Simulation, source: SimId, target: SimId) {
    sim.world
        .entity_mut(entity(sim, source))
        .get_mut::<TargetState>()
        .unwrap()
        .current = Some(target);
}

fn set_health(sim: &mut Simulation, id: SimId, current: i32) {
    sim.world
        .entity_mut(entity(sim, id))
        .get_mut::<Health>()
        .unwrap()
        .current = current;
}

#[test]
fn damage_threshold_observes_pre_hit_live_health_in_canonical_attack_order() {
    let mut sim = simulation(1);
    let first = sim.spawn_unit(unit(1, 20, 250, AttackDelivery::Melee));
    let second = sim.spawn_unit(unit(1, 25, 150, AttackDelivery::Melee));
    let troll = sim.spawn_unit_with_properties(
        unit(0, 40, 10, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::from_slice(&[
                PassiveUnitEffect::DamageThreshold(profile()),
                PassiveUnitEffect::CriticalStrike(CriticalStrikeEffectProfile {
                    ability: AbilityId(93),
                    chance_per_10k: 10_000,
                    damage_multiplier_per_10k: 20_000,
                    targets: AttackTargetMask::AIR_AND_GROUND,
                }),
            ]),
            ..Default::default()
        },
    );
    set_health(&mut sim, troll, 720);
    engage(&mut sim, first, troll);
    engage(&mut sim, second, troll);
    engage(&mut sim, troll, first);
    sim.step();
    sim.step();
    assert_eq!(sim.unit(troll).unwrap().health, 320);
    // The second event sees the first event's committed HP. The final hit does
    // not advance to the lowest band until another damage event is delivered.
    assert_eq!(
        sim.unit(troll)
            .unwrap()
            .status
            .damage_threshold
            .ability_level,
        2
    );
    assert_eq!(sim.unit(first).unwrap().health, 1000 - (10 + 13) * 2);
    let mut restored = wire_restored(&sim, 6);
    for _ in 0..5 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
}

#[test]
fn damage_threshold_waits_for_projectile_impact_and_survives_healing_and_cleanse() {
    let mut sim = simulation(1);
    let source = sim.spawn_unit(unit(
        1,
        20,
        400,
        AttackDelivery::RangedGuaranteedHit {
            speed_per_tick: 5 * SUBUNITS_PER_WORLD_UNIT,
        },
    ));
    let troll = sim.spawn_unit_with_properties(
        unit(0, 40, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            attack_targets: AttackTargetMask::BUILDINGS,
            passive_effects: effects(),
            ..Default::default()
        },
    );
    set_health(&mut sim, troll, 790);
    engage(&mut sim, source, troll);
    sim.step();
    sim.step();
    assert_eq!(
        sim.unit(troll)
            .unwrap()
            .status
            .damage_threshold
            .ability_level,
        0
    );
    let mut restored = wire_restored(&sim, 6);
    for _ in 0..8 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
        if sim.unit(troll).unwrap().health < 790 {
            break;
        }
    }
    assert_eq!(sim.unit(troll).unwrap().health, 390);
    assert_eq!(
        sim.unit(troll)
            .unwrap()
            .status
            .damage_threshold
            .ability_level,
        1
    );
    set_health(&mut sim, troll, 1000);
    let troll_entity = entity(&sim, troll);
    super::super::native_actions::cleanse_native_status(
        &mut sim
            .world
            .entity_mut(troll_entity)
            .get_mut::<StatusState>()
            .unwrap(),
    );
    for _ in 0..3 {
        sim.step();
    }
    assert_eq!(
        sim.unit(troll)
            .unwrap()
            .status
            .damage_threshold
            .ability_level,
        1
    );
    assert_eq!(sim.unit(troll).unwrap().attack.damage, 7);
}

#[test]
fn damage_threshold_periodic_damage_is_an_event_and_healing_alone_is_not() {
    let mut sim = simulation(1);
    let troll = sim.spawn_unit_with_properties(
        unit(0, 40, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: effects(),
            corpse: Some(crate::CorpseProfile {
                definition: crate::CorpseDefinitionId(99),
                decay_start_ticks: 2,
                lifetime_ticks: Some(100),
            }),
            ..Default::default()
        },
    );
    set_health(&mut sim, troll, 700);
    sim.step();
    assert_eq!(
        sim.unit(troll)
            .unwrap()
            .status
            .damage_threshold
            .ability_level,
        0
    );
    let tick = sim.next_tick;
    let troll_entity = entity(&sim, troll);
    apply_timed_damage_over_time(
        &mut sim
            .world
            .entity_mut(troll_entity)
            .get_mut::<StatusState>()
            .unwrap(),
        ModifierId(94),
        250,
        1,
        tick,
        tick + 4,
    );
    sim.step();
    sim.step();
    assert_eq!(
        sim.unit(troll)
            .unwrap()
            .status
            .damage_threshold
            .ability_level,
        1
    );
    let mut restored = wire_restored(&sim, 6);
    sim.step();
    restored.step();
    assert_eq!(
        sim.unit(troll)
            .unwrap()
            .status
            .damage_threshold
            .ability_level,
        2
    );
    assert_eq!(sim.checksum(), restored.checksum());
    for _ in 0..2 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert!(sim.unit(troll).is_none());
    assert_eq!(
        sim.corpses()[0].vertex_rgba,
        Some(profile().thresholds[2].vertex_rgba)
    );
    let resumed = wire_restored(&sim, 3);
    assert_eq!(resumed.corpses(), sim.corpses());
}
