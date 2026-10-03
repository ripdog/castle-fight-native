//! Synthetic target semantics for native Feedback and Faerie Fire; no entity tuning copies.
use super::*;
use crate::components::{
    CriticalStrikeEffectProfile, EvasionEffectProfile, FeedbackEffectProfile, ManaProfile,
};

fn unit(team: u8, x: i32, damage: i32, delivery: AttackDelivery) -> UnitSpawn {
    UnitSpawn {
        team: Team(team),
        position: SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, 20 * SUBUNITS_PER_WORLD_UNIT),
        health: 1000,
        attack: AttackProfile {
            damage,
            range: 200 * SUBUNITS_PER_WORLD_UNIT,
            acquisition_range: 250 * SUBUNITS_PER_WORLD_UNIT,
            cooldown_ticks: 1000,
            delivery,
        },
        movement: MovementProfile { speed_per_tick: 0 },
    }
}
fn mana_profile(starting: i32) -> SpellcastingProfile {
    SpellcastingProfile {
        mana: ManaProfile {
            maximum: 100,
            starting,
            regen_per_tick_per_10k: 0,
        },
        ability: AutomaticAbilityProfile {
            id: AbilityId(9),
            mana_cost: 0,
            cooldown_ticks: 1000,
            range: 0,
            target_policy: AbilityTargetPolicy::RandomEnemyUnit,
            effect: AbilityEffect::Damage { amount: 0 },
        },
    }
}
fn feedback() -> PassiveUnitEffect {
    PassiveUnitEffect::Feedback(FeedbackEffectProfile {
        ability: AbilityId(1),
        maximum_mana_drained: 6,
        damage_per_mana_per_10k: 5000,
        summoned_damage: 4,
        targets: AttackTargetMask::AIR_AND_GROUND,
    })
}
fn simulation(workers: usize) -> Simulation {
    Simulation::new(
        SimulationConfig {
            navigation_max: NavCell::new(500, 64),
            ..SimulationConfig::default()
        },
        workers,
    )
}
fn entity(sim: &Simulation, id: SimId) -> Entity {
    sim.world
        .iter_entities()
        .find(|entity| entity.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}
fn wire_restored(sim: &Simulation, workers: usize) -> Simulation {
    let content = crate::castle_fight_content_bundle(crate::MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), content)
            .unwrap();
    let mut restored = simulation(workers);
    restored.restore_snapshot(&snapshot).unwrap();
    restored
}

#[test]
fn feedback_is_not_gated_or_multiplied_by_critical_strikes() {
    for chance in [0, 10_000] {
        let mut sim = simulation(1);
        sim.spawn_unit_with_properties(
            unit(0, 20, 10, AttackDelivery::Melee),
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::from_slice(&[
                    feedback(),
                    PassiveUnitEffect::CriticalStrike(CriticalStrikeEffectProfile {
                        ability: AbilityId(2),
                        chance_per_10k: chance,
                        damage_multiplier_per_10k: 20_000,
                        targets: AttackTargetMask::AIR_AND_GROUND,
                    }),
                ]),
                ..UnitGameplayProperties::default()
            },
        );
        let victim = sim
            .spawn_unit_with_spellcasting(unit(1, 60, 0, AttackDelivery::Melee), mana_profile(10));
        sim.step(); // Birth tick has no ordinary attacks.
        sim.step();
        let view = sim.unit(victim).unwrap();
        assert_eq!(view.mana_current, Some(4));
        assert_eq!(view.health, 1000 - if chance == 0 { 10 } else { 20 } - 3);
    }
}

#[test]
fn feedback_reads_live_mana_at_projectile_impact_and_continues_after_wire_restore() {
    let mut sim = simulation(1);
    sim.spawn_unit_with_properties(
        unit(
            0,
            20,
            10,
            AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: 10 * SUBUNITS_PER_WORLD_UNIT,
            },
        ),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(feedback()),
            ..UnitGameplayProperties::default()
        },
    );
    let victim =
        sim.spawn_unit_with_spellcasting(unit(1, 60, 0, AttackDelivery::Melee), mana_profile(10));
    sim.step();
    sim.step();
    assert_eq!(sim.unit(victim).unwrap().mana_current, Some(10));
    let victim_entity = entity(&sim, victim);
    sim.world
        .entity_mut(victim_entity)
        .get_mut::<ManaState>()
        .unwrap()
        .current = 2;
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..5 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert_eq!(sim.unit(victim).unwrap().mana_current, Some(0));
    assert_eq!(sim.unit(victim).unwrap().health, 1000 - 10 - 1);
}

#[test]
fn feedback_has_summoned_bonus_without_mana_but_respects_misses_immunity_and_structures() {
    for (summoned, immune, evasion) in [
        (true, false, 0),
        (true, true, 0),
        (true, false, 10_000),
        (false, false, 0),
    ] {
        let mut sim = simulation(1);
        sim.spawn_unit_with_properties(
            unit(0, 20, 10, AttackDelivery::Melee),
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::single(feedback()),
                ..UnitGameplayProperties::default()
            },
        );
        let target = sim.spawn_unit_with_properties(
            unit(1, 60, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                classifications: UnitClassifications {
                    summoned,
                    spell_immune: immune,
                    hero: false,
                },
                passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Evasion(
                    EvasionEffectProfile {
                        ability: AbilityId(3),
                        chance_per_10k: evasion,
                    },
                )),
                ..UnitGameplayProperties::default()
            },
        );
        sim.step();
        sim.step();
        assert_eq!(
            sim.unit(target).unwrap().health,
            1000 - if evasion == 10_000 {
                0
            } else {
                10 + if summoned && !immune { 4 } else { 0 }
            }
        );
    }
    let mut sim = simulation(1);
    let source = sim.spawn_unit_with_properties(
        unit(0, 20, 10, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(feedback()),
            ..UnitGameplayProperties::default()
        },
    );
    let target = sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(2, 0, 1, 1),
        health: 1000,
        production: None,
        attack: None,
        spellcasting: Some(mana_profile(10)),
    });
    sim.world
        .entity_mut(entity(&sim, source))
        .get_mut::<TargetState>()
        .unwrap()
        .current = Some(target);
    sim.step();
    assert_eq!(sim.building(target).unwrap().mana_current, Some(10));
}

#[test]
fn feedback_live_mana_is_shared_between_same_tick_hits_and_not_burned_on_immune_or_evaded_targets()
{
    for (immune, evade, expected_mana) in [(false, 0, 0), (true, 0, 10), (false, 10_000, 10)] {
        let mut sim = simulation(1);
        for x in [20, 30] {
            sim.spawn_unit_with_properties(
                unit(0, x, 1, AttackDelivery::Melee),
                UnitGameplayProperties {
                    passive_effects: PassiveUnitEffects::single(feedback()),
                    ..UnitGameplayProperties::default()
                },
            );
        }
        let target = sim.spawn_unit_with_properties_and_spellcasting(
            unit(1, 60, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                classifications: UnitClassifications {
                    spell_immune: immune,
                    ..UnitClassifications::default()
                },
                passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Evasion(
                    EvasionEffectProfile {
                        ability: AbilityId(8),
                        chance_per_10k: evade,
                    },
                )),
                ..UnitGameplayProperties::default()
            },
            mana_profile(10),
        );
        sim.step();
        sim.step();
        assert_eq!(sim.unit(target).unwrap().mana_current, Some(expected_mana));
        if !immune && evade == 0 {
            assert_eq!(sim.unit(target).unwrap().health, 1000 - 2 - 5);
        }
    }
}

fn faerie_profile() -> SpellcastingProfile {
    SpellcastingProfile {
        mana: ManaProfile {
            maximum: 20,
            starting: 12,
            regen_per_tick_per_10k: 0,
        },
        ability: AutomaticAbilityProfile {
            id: AbilityId(4),
            mana_cost: 3,
            cooldown_ticks: 2,
            range: 100 * SUBUNITS_PER_WORLD_UNIT,
            target_policy: AbilityTargetPolicy::NearestEnemyInCombat,
            effect: AbilityEffect::FaerieFire {
                modifier: ModifierId(4),
                armor_reduction_per_100: 400,
                duration_ticks: 8,
                hero_duration_ticks: 3,
            },
        },
    }
}
fn battle_target(
    sim: &mut Simulation,
    caster: SimId,
    x: i32,
    classifications: UnitClassifications,
) -> SimId {
    let target = sim.spawn_unit_with_properties(
        unit(1, x, 1, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications,
            ..UnitGameplayProperties::default()
        },
    );
    sim.world
        .entity_mut(entity(sim, target))
        .get_mut::<TargetState>()
        .unwrap()
        .current = Some(caster);
    target
}

#[test]
fn faerie_fire_prefers_nearest_unmarked_combat_enemy_not_passive_idle_or_immune_units() {
    let mut sim = simulation(1);
    let caster =
        sim.spawn_unit_with_spellcasting(unit(0, 20, 0, AttackDelivery::Melee), faerie_profile());
    let passive = sim.spawn_unit(unit(1, 40, 0, AttackDelivery::Melee));
    let mut idle_spawn = unit(1, 50, 1, AttackDelivery::Melee);
    idle_spawn.attack.range = 0;
    idle_spawn.attack.acquisition_range = 0;
    let idle = sim.spawn_unit(idle_spawn);
    let immune = battle_target(
        &mut sim,
        caster,
        60,
        UnitClassifications {
            spell_immune: true,
            ..UnitClassifications::default()
        },
    );
    let mechanical = battle_target(&mut sim, caster, 70, UnitClassifications::default());
    let mechanical_entity = entity(&sim, mechanical);
    sim.world
        .entity_mut(mechanical_entity)
        .insert(MechanicalUnit);
    let closest = battle_target(&mut sim, caster, 80, UnitClassifications::default());
    let farther = battle_target(&mut sim, caster, 100, UnitClassifications::default());
    sim.step();
    assert!(sim.unit(closest).unwrap().status.is_revealed_to(Team(0), 0));
    for target in [passive, idle, immune, mechanical, farther] {
        assert!(!sim.unit(target).unwrap().status.is_revealed_to(Team(0), 0));
    }
    assert_eq!(sim.unit(caster).unwrap().mana_current, Some(9));
    // Existing marks must not spend mana or refresh the same target at every cooldown.
    sim.step();
    sim.step();
    assert!(sim.unit(farther).unwrap().status.is_revealed_to(Team(0), 2));
    assert_eq!(
        sim.unit(closest).unwrap().status.armor_modifiers[0].expires_tick,
        8
    );
}

#[test]
fn faerie_fire_hero_duration_reveal_and_armor_survive_mid_buff_wire_restore() {
    let mut sim = simulation(1);
    let caster =
        sim.spawn_unit_with_spellcasting(unit(0, 20, 0, AttackDelivery::Melee), faerie_profile());
    let target = battle_target(
        &mut sim,
        caster,
        60,
        UnitClassifications {
            hero: true,
            ..UnitClassifications::default()
        },
    );
    sim.step();
    let status = sim.unit(target).unwrap().status;
    assert_eq!(status.armor_modifiers[0].armor_bonus_per_100, -400);
    assert_eq!(status.armor_modifiers[0].expires_tick, 3);
    assert!(status.is_revealed_to(Team(0), 2));
    assert!(!status.is_revealed_to(Team(1), 2));
    assert!(!status.is_revealed_to(Team(0), 3));
    // Keep the source from immediately reapplying after expiry.
    let caster_entity = entity(&sim, caster);
    sim.world
        .entity_mut(caster_entity)
        .get_mut::<AutomaticAbilityState>()
        .unwrap()
        .autocast_enabled = false;
    let mut other = wire_restored(&sim, 4);
    for _ in 0..5 {
        sim.step();
        other.step();
        assert_eq!(sim.checksum(), other.checksum());
    }
    assert_eq!(sim.unit(target).unwrap().status.armor_modifier_count, 0);
    assert!(other.unit(target).unwrap().classifications.hero);
}

#[test]
fn faerie_fire_inclusive_range_boundary_and_insufficient_mana_are_revalidated() {
    for starting in [2, 3] {
        let mut sim = simulation(1);
        let profile = SpellcastingProfile {
            mana: ManaProfile {
                starting,
                ..faerie_profile().mana
            },
            ..faerie_profile()
        };
        let caster =
            sim.spawn_unit_with_spellcasting(unit(0, 20, 0, AttackDelivery::Melee), profile);
        let target = battle_target(&mut sim, caster, 120, UnitClassifications::default());
        sim.step();
        assert_eq!(
            sim.unit(target).unwrap().status.is_revealed_to(Team(0), 0),
            starting == 3
        );
        assert_eq!(
            sim.unit(caster).unwrap().mana_current,
            Some(if starting == 3 { 0 } else { 2 })
        );
    }
}

#[test]
fn faerie_fire_does_not_consume_resources_for_no_combat_or_out_of_range_targets() {
    let mut sim = simulation(1);
    let caster =
        sim.spawn_unit_with_spellcasting(unit(0, 20, 0, AttackDelivery::Melee), faerie_profile());
    sim.spawn_unit(unit(1, 60, 0, AttackDelivery::Melee));
    battle_target(&mut sim, caster, 121, UnitClassifications::default());
    sim.step();
    assert_eq!(sim.unit(caster).unwrap().mana_current, Some(12));
    assert_eq!(sim.unit(caster).unwrap().ability_cast_sequence, Some(0));
}
