use super::*;
use crate::{
    DamageRetaliationProfile, ForkedLightningProfile, NativeBoltProfile, TriggeredAttackEffect,
    TriggeredSpellProcProfile,
};

pub(super) fn shield(chance: u16) -> PassiveUnitEffects {
    PassiveUnitEffects::single(PassiveUnitEffect::DamageRetaliation(
        DamageRetaliationProfile {
            marker: AbilityId(701),
            chance_per_10k: chance,
            range: 100 * SUBUNITS_PER_WORLD_UNIT,
            organic_only: true,
            projectile: NativeBoltProfile {
                ability: AbilityId(702),
                damage: 30,
                stun_ticks: 9,
                hero_stun_ticks: 3,
                damage_per_second: 0,
                duration_ticks: 0,
                speed_per_tick: 10 * SUBUNITS_PER_WORLD_UNIT,
                cleanse: false,
                targets: AttackTargetMask::AIR_AND_GROUND,
            },
        },
    ))
}

#[test]
fn damage_retaliation_uses_native_melee_classification_before_fatal_health_commit_and_restores_flight()
 {
    let mut sim = simulation(1);
    let attacker = sim.spawn_unit_with_properties(
        unit(0, 20, 2000, AttackDelivery::RangedInstant),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                melee_attacker: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let victim = sim.spawn_unit_with_properties(
        unit(1, 60, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: shield(10_000),
            ..Default::default()
        },
    );
    sim.step();
    sim.step();
    assert!(sim.unit(victim).is_none());
    assert_eq!(sim.projectile_count(), 1);
    assert_eq!(sim.unit(attacker).unwrap().health, 1000);
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..4 {
        assert_eq!(
            {
                sim.step();
                sim.checksum()
            },
            {
                restored.step();
                restored.checksum()
            }
        );
    }
    assert_eq!(sim.unit(attacker).unwrap().health, 970);
    assert!(sim.unit(attacker).unwrap().status.stunned_until_tick > sim.next_tick);
}

#[test]
fn damage_retaliation_rejects_non_melee_and_spell_events_but_rolls_before_native_order_failure() {
    for (melee, mechanical, immune, event_type, expected_sequence, expected_missiles) in [
        (false, false, false, DamageType::Normal, 0, 0),
        (true, true, false, DamageType::Normal, 1, 0),
        (true, false, true, DamageType::Normal, 1, 0),
        (true, false, false, DamageType::Normal, 1, 1),
        (true, false, false, DamageType::Spells, 0, 0),
    ] {
        let mut sim = simulation(1);
        let attacker = sim.spawn_unit_with_properties(
            unit(0, 20, 0, AttackDelivery::RangedInstant),
            UnitGameplayProperties {
                mechanical,
                damage_type: event_type,
                build_time_ticks: Some(30),
                classifications: UnitClassifications {
                    melee_attacker: melee,
                    spell_immune: immune,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let victim = sim.spawn_unit_with_properties(
            unit(1, 60, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                passive_effects: shield(10_000),
                ..Default::default()
            },
        );
        sim.step();
        sim.step();
        assert_eq!(
            sim.unit(victim).unwrap().status.retaliation_sequence,
            expected_sequence
        );
        assert_eq!(sim.projectile_count(), expected_missiles);
        let profile = NativeBoltProfile {
            ability: AbilityId(703),
            damage: 10,
            stun_ticks: 0,
            hero_stun_ticks: 0,
            damage_per_second: 0,
            duration_ticks: 0,
            speed_per_tick: 100 * SUBUNITS_PER_WORLD_UNIT,
            cleanse: false,
            targets: AttackTargetMask::AIR_AND_GROUND,
        };
        sim.launch_native_bolt(
            attacker,
            Team(0),
            sim.unit(attacker).unwrap().position,
            victim,
            sim.unit(victim).unwrap().position,
            profile,
        );
        sim.step();
        assert_eq!(
            sim.unit(victim).unwrap().status.retaliation_sequence,
            expected_sequence,
            "native spell attack events do not enter the melee response"
        );
    }
}

#[test]
fn damage_retaliation_inclusive_percent_roll_has_distinct_per_event_sequence() {
    for roll in [15, 16] {
        let purpose = RANDOM_PURPOSE_ATTACK_PROC ^ 701 ^ 1u64.rotate_left(13);
        let seed = (0..100_000)
            .find(|&seed| deterministic_random(seed, 1, SimId(2), purpose, 0) % 100 == roll)
            .unwrap();
        let config = SimulationConfig {
            match_seed: seed,
            ..simulation(1).config
        };
        let mut sim = Simulation::new(config, 1);
        let attacker = sim.spawn_unit_with_properties(
            unit(0, 20, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                classifications: UnitClassifications {
                    melee_attacker: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let victim = sim.spawn_unit_with_properties(
            unit(1, 60, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                passive_effects: shield(1600),
                ..Default::default()
            },
        );
        assert_eq!((attacker, victim), (SimId(1), SimId(2)));
        sim.step();
        sim.step();
        assert_eq!(sim.projectile_count(), usize::from(roll == 15));
        assert_eq!(sim.unit(victim).unwrap().status.retaliation_sequence, 1);
        let mut restored = wire_restored(&sim, 4);
        for _ in 0..6 {
            assert_eq!(
                {
                    sim.step();
                    sim.checksum()
                },
                {
                    restored.step();
                    restored.checksum()
                }
            );
        }
    }
}

fn fork(mask: AttackTargetMask) -> ForkedLightningProfile {
    ForkedLightningProfile {
        ability: AbilityId(801),
        damage: 50,
        maximum_targets: 3,
        distance: 100 * SUBUNITS_PER_WORLD_UNIT,
        initial_radius: 2 * SUBUNITS_PER_WORLD_UNIT,
        final_radius: 25 * SUBUNITS_PER_WORLD_UNIT,
        targets: mask,
    }
}

#[test]
fn forked_lightning_is_simultaneous_caster_cone_primary_first_and_keeps_air_immunity_and_target_limit()
 {
    let mut sim = simulation(1);
    let source = sim.spawn_unit(unit(0, 10, 0, AttackDelivery::Melee));
    let primary = sim.spawn_unit(unit(1, 40, 0, AttackDelivery::Melee));
    let air = sim.spawn_unit_with_properties(
        unit(1, 60, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            ..Default::default()
        },
    );
    let far = sim.spawn_unit(unit(1, 100, 0, AttackDelivery::Melee));
    let capped = sim.spawn_unit(unit(1, 109, 0, AttackDelivery::Melee));
    let immune = sim.spawn_unit_with_properties(
        unit(1, 45, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                spell_immune: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let behind = sim.spawn_unit(unit(1, 5, 0, AttackDelivery::Melee));
    let mut side = unit(1, 40, 0, AttackDelivery::Melee);
    side.position.y += 40 * SUBUNITS_PER_WORLD_UNIT;
    let outside = sim.spawn_unit(side);
    let ally = sim.spawn_unit(unit(0, 50, 0, AttackDelivery::Melee));
    let mut units = sim.snapshot_units();
    let mut health: Vec<_> = units.iter().map(|u| u.health).collect();
    let mut buildings = Vec::new();
    let mut building_health = Vec::new();
    let primary_index = find_unit_index(&units, primary).unwrap();
    let result = apply_pending_attack_effects(
        TargetIndex::Unit(primary_index),
        PendingAttackEffects {
            triggered_spell: Some(TriggeredAttackEffect::ForkedLightning(fork(
                AttackTargetMask::AIR_AND_GROUND,
            ))),
            stun_duration_ticks: 7,
            ..Default::default()
        },
        PendingAttackEffectSource {
            id: source,
            team: Team(0),
            position: units[0].position,
        },
        PendingAttackEffectState {
            retired_sources: &BTreeMap::new(),
            completed_tick: 1,
            navigation_cell_size: sim.config.navigation_cell_size,
            units: &mut units,
            unit_health: &mut health,
            buildings: &mut buildings,
            building_health: &mut building_health,
            damage_rules: sim.combat_rules.damage_rules,
        },
    );
    let events = result.fork_events;
    assert_eq!(units[primary_index].status.stunned_until_tick, 8);
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].points[1], sim.unit(primary).unwrap().position);
    assert!(
        events
            .iter()
            .all(|e| e.points[0] == sim.unit(source).unwrap().position && e.bounce_index == 0)
    );
    for id in [primary, air, far] {
        assert_eq!(health[find_unit_index(&units, id).unwrap()], 950);
    }
    for id in [capped, immune, behind, outside, ally] {
        assert_eq!(health[find_unit_index(&units, id).unwrap()], 1000);
    }
}

#[test]
fn mutually_exclusive_orbs_apply_structure_fork_on_weapon_projectile_impact() {
    let mut sim = simulation(1);
    let mut building_fork = fork(AttackTargetMask::BUILDINGS);
    building_fork.damage = 25;
    building_fork.maximum_targets = 1;
    sim.spawn_unit_with_properties(
        unit(
            0,
            20,
            1,
            AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: 20 * SUBUNITS_PER_WORLD_UNIT,
            },
        ),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::from_slice(&[
                PassiveUnitEffect::TriggeredSpellProc(TriggeredSpellProcProfile {
                    cooldown_ticks: 0,
                    hero_chance_per_10k: None,
                    summoned_chance_per_10k: None,
                    ability: AbilityId(804),
                    chance_per_10k: 10_000,
                    targets: AttackTargetMask::AIR_AND_GROUND,
                    effect: TriggeredAttackEffect::ForkedLightning(fork(
                        AttackTargetMask::AIR_AND_GROUND,
                    )),
                }),
                PassiveUnitEffect::TriggeredSpellProc(TriggeredSpellProcProfile {
                    cooldown_ticks: 0,
                    hero_chance_per_10k: None,
                    summoned_chance_per_10k: None,
                    ability: AbilityId(805),
                    chance_per_10k: 10_000,
                    targets: AttackTargetMask::BUILDINGS,
                    effect: TriggeredAttackEffect::ForkedLightning(building_fork),
                }),
            ]),
            ..Default::default()
        },
    );
    let target = sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(60, 20, 1, 1),
        health: 1000,
        production: None,
        attack: None,
        spellcasting: None,
    });
    sim.step();
    sim.step();
    assert_eq!(sim.building(target).unwrap().health, 1000);
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..4 {
        assert_eq!(
            {
                sim.step();
                sim.checksum()
            },
            {
                restored.step();
                restored.checksum()
            }
        );
    }
    assert_eq!(sim.building(target).unwrap().health, 974);
}
