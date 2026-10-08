use super::*;
use crate::PoisonProfile;

fn profile() -> PoisonProfile {
    PoisonProfile {
        ability: AbilityId(95),
        damage_per_second: 19,
        movement_percent_delta: -10,
        attack_speed_percent_delta: -15,
        duration_ticks: 61,
        hero_duration_ticks: 10,
        targets: AttackTargetMask::AIR_AND_GROUND,
        stack_damage: true,
        can_kill: false,
    }
}

fn attacker(sim: &mut Simulation, x: i32, delivery: AttackDelivery) -> SimId {
    sim.spawn_unit_with_properties(
        unit(0, x, 1, delivery),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Poison(profile())),
            ..Default::default()
        },
    )
}

fn apply(sim: &mut Simulation, source: SimId, target: SimId) {
    let mut units = sim.snapshot_units();
    let index = find_unit_index(&units, target).unwrap();
    sim.apply_poison(source, &mut units[index], profile(), sim.next_tick);
    *sim.world
        .entity_mut(units[index].entity)
        .get_mut::<StatusState>()
        .unwrap() = units[index].status;
}

fn poison_actions(sim: &Simulation) -> usize {
    sim.world
        .iter_entities()
        .filter(|entity| {
            matches!(
                entity.get::<crate::components::NativeAction>(),
                Some(crate::components::NativeAction::Poison(_))
            )
        })
        .count()
}

#[test]
fn lethal_triggered_weapon_spell_does_not_leave_a_poison_action() {
    let mut sim = simulation(1);
    sim.spawn_unit_with_properties(
        unit(0, 20, 1, AttackDelivery::RangedInstant),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::from_slice(&[
                PassiveUnitEffect::Poison(profile()),
                PassiveUnitEffect::TriggeredSpellProc(crate::TriggeredSpellProcProfile {
                    ability: AbilityId(96),
                    chance_per_10k: 10_000,
                    targets: AttackTargetMask::AIR_AND_GROUND,
                    effect: TriggeredAttackEffect::ChainLightning(
                        crate::ChainLightningEffectProfile {
                            ability: AbilityId(97),
                            initial_damage: 2000,
                            maximum_targets: 1,
                            jump_radius: 0,
                            damage_reduction_per_10k: 0,
                            targets: AttackTargetMask::AIR_AND_GROUND,
                        },
                    ),
                }),
            ]),
            ..Default::default()
        },
    );
    let target = sim.spawn_unit(unit(1, 40, 0, AttackDelivery::Melee));
    sim.step();
    sim.step();
    assert!(sim.unit(target).is_none());
    assert_eq!(poison_actions(&sim), 0);
}

#[test]
fn poison_stacks_per_source_without_stacking_slows_or_delaying_existing_pulses() {
    let mut sim = simulation(1);
    let first = attacker(&mut sim, 20, AttackDelivery::Melee);
    let second = attacker(&mut sim, 25, AttackDelivery::Melee);
    let target = sim.spawn_unit(unit(1, 40, 0, AttackDelivery::Melee));
    sim.step();
    sim.step();
    let status = sim.unit(target).unwrap().status;
    assert_eq!(status.poison.unwrap().sources, 2);
    assert_eq!(status.movement_modifier_count, 1);
    assert_eq!(status.attack_speed_modifier_count, 1);
    assert_eq!(status.movement_modifiers[0].percent_delta, -10);
    let before = sim.unit(target).unwrap().health;
    for _ in 0..10 {
        sim.step();
    }
    apply(&mut sim, first, target);
    apply(&mut sim, first, target);
    assert_eq!(poison_actions(&sim), 2);
    assert_eq!(sim.unit(target).unwrap().status.poison.unwrap().sources, 2);
    let mut restored = wire_restored(&sim, 6);
    while sim.next_tick <= 31 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert_eq!(sim.unit(target).unwrap().health, before - 2 * 19);
    // A dead attacking handle does not cancel an already delivered poison.
    sim.world.despawn(entity(&sim, second));
    restored.world.despawn(entity(&restored, second));
    while sim.next_tick <= 74 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert_eq!(poison_actions(&sim), 0);
    assert!(sim.unit(target).unwrap().status.poison.is_none());
    assert_eq!(sim.unit(target).unwrap().status.movement_modifier_count, 0);
}

#[test]
fn poison_waits_for_a_landed_weapon_and_revalidates_live_native_target_classes() {
    for (mechanical, immune, invulnerable, air) in [
        (false, false, false, false),
        (false, false, false, true),
        (true, false, false, false),
        (false, true, false, false),
        (false, false, true, false),
    ] {
        let mut sim = simulation(1);
        attacker(
            &mut sim,
            20,
            AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: 5 * SUBUNITS_PER_WORLD_UNIT,
            },
        );
        let target = sim.spawn_unit_with_properties(
            unit(1, 40, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                mechanical,
                build_time_ticks: mechanical.then_some(1),
                movement_class: if air {
                    MovementClass::Air
                } else {
                    MovementClass::Ground
                },
                classifications: UnitClassifications {
                    spell_immune: immune,
                    invulnerable,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        sim.step();
        sim.step();
        assert!(sim.unit(target).unwrap().status.poison.is_none());
        let mut restored = wire_restored(&sim, 6);
        for _ in 0..8 {
            sim.step();
            restored.step();
            assert_eq!(sim.checksum(), restored.checksum());
        }
        assert_eq!(
            sim.unit(target).unwrap().status.poison.is_some(),
            !(mechanical || invulnerable)
        );
    }
    let mut sim = simulation(1);
    attacker(&mut sim, 20, AttackDelivery::Melee);
    let building = sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(2, 1, 1, 1),
        health: 1000,
        production: None,
        attack: None,
        spellcasting: None,
    });
    for _ in 0..3 {
        sim.step();
    }
    assert_eq!(poison_actions(&sim), 0);
    assert!(sim.building(building).unwrap().health < 1000);
}

#[test]
fn poison_respects_hero_expiry_nonlethal_damage_and_cleanse_generations() {
    let mut sim = simulation(1);
    let source = attacker(&mut sim, 20, AttackDelivery::Melee);
    let target = sim.spawn_unit(unit(1, 40, 0, AttackDelivery::Melee));
    sim.step();
    sim.step();
    let source_entity = entity(&sim, source);
    sim.world
        .entity_mut(source_entity)
        .get_mut::<AttackCooldown>()
        .unwrap()
        .remaining = 1000;
    let target_entity = entity(&sim, target);
    sim.world
        .entity_mut(target_entity)
        .get_mut::<Health>()
        .unwrap()
        .current = 5;
    while sim.next_tick <= 31 {
        sim.step();
    }
    assert_eq!(sim.unit(target).unwrap().health, 1);
    let old_generation = sim.unit(target).unwrap().status.poison.unwrap().generation;
    super::super::native_actions::cleanse_native_status(
        &mut sim
            .world
            .entity_mut(target_entity)
            .get_mut::<StatusState>()
            .unwrap(),
    );
    apply(&mut sim, source, target);
    assert_ne!(
        sim.unit(target).unwrap().status.poison.unwrap().generation,
        old_generation
    );
    sim.step();
    assert_eq!(poison_actions(&sim), 1);
    assert_eq!(sim.unit(target).unwrap().status.poison.unwrap().sources, 1);

    let mut sim = simulation(1);
    attacker(&mut sim, 20, AttackDelivery::Melee);
    let hero = sim.spawn_unit_with_properties(
        unit(1, 40, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                hero: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    sim.step();
    sim.step();
    assert_eq!(
        sim.unit(hero).unwrap().status.poison.unwrap().expires_tick,
        11
    );
    while sim.next_tick <= 11 {
        sim.step();
    }
    assert!(sim.unit(hero).unwrap().status.poison.is_none());
    assert_eq!(sim.unit(hero).unwrap().health, 999);
}

#[test]
fn poison_acquisition_prefers_an_unpoisoned_unit_without_breaking_retained_targets() {
    let mut sim = simulation(1);
    let source = attacker(&mut sim, 20, AttackDelivery::Melee);
    let near = sim.spawn_unit(unit(1, 40, 0, AttackDelivery::Melee));
    let far = sim.spawn_unit(unit(1, 60, 0, AttackDelivery::Melee));
    apply(&mut sim, source, near);
    sim.step();
    assert_eq!(sim.unit(source).unwrap().target, Some(far));
    apply(&mut sim, source, far);
    sim.step();
    assert_eq!(sim.unit(source).unwrap().target, Some(far));
}
