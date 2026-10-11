use super::*;
use crate::{
    ManaProfile, MapVersion, NativeAbilityGrant, NativeAbilityGrantEffect as Effect,
    NativeAbilityGrants,
};
const VERSION: MapVersion = MapVersion::CASTLE_FIGHT_9_27;
fn grant(id: u32, effect: Effect) -> NativeAbilityGrant {
    NativeAbilityGrant {
        ability: AbilityId(id),
        effect,
    }
}
fn profile() -> BuildingPermanentGrantProfile {
    BuildingPermanentGrantProfile {
        map_version: VERSION,
        trigger_targets: AttackTargetMask::GROUND_AND_BUILDINGS,
        trigger_invulnerable: true,
        trigger_spell_immune: false,
        retry_marker: AbilityId(99),
        always: [
            grant(101, Effect::AttackSpeed(25)),
            grant(102, Effect::AttackDamage(7)),
            grant(
                crate::building_mechanics::holy_aid_life_ability_for_version(VERSION).0,
                Effect::MaximumLife(37),
            ),
            grant(104, Effect::Armor(200)),
        ],
        options: [
            [
                grant(
                    105,
                    Effect::CriticalStrike {
                        chance_per_10k: 10_000,
                        multiplier_per_10k: 20_000,
                        targets: AttackTargetMask::AIR_AND_GROUND,
                    },
                ),
                grant(106, Effect::AuraMarker),
            ],
            [
                grant(
                    107,
                    Effect::Evasion {
                        chance_per_10k: 10_000,
                    },
                ),
                grant(108, Effect::AuraMarker),
            ],
            [
                grant(
                    109,
                    Effect::Demolish {
                        chance_per_10k: 10_000,
                        multiplier_per_10k: 15_000,
                        targets: AttackTargetMask::BUILDINGS,
                    },
                ),
                grant(110, Effect::AuraMarker),
            ],
        ],
    }
}
fn simulation(workers: usize) -> Simulation {
    Simulation::new(
        SimulationConfig {
            navigation_cell_size: 16 * SUBUNITS_PER_WORLD_UNIT,
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
fn unit(team: u8, x: i32, damage: i32) -> UnitSpawn {
    UnitSpawn {
        team: Team(team),
        position: SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, 10 * SUBUNITS_PER_WORLD_UNIT),
        health: 1000,
        attack: AttackProfile {
            damage,
            range: 200 * SUBUNITS_PER_WORLD_UNIT,
            acquisition_range: 200 * SUBUNITS_PER_WORLD_UNIT,
            cooldown_ticks: 1000,
            delivery: AttackDelivery::Melee,
        },
        movement: MovementProfile { speed_per_tick: 0 },
    }
}
fn ally(sim: &mut Simulation) -> SimId {
    sim.spawn_unit_with_properties(
        unit(0, 40, 0),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}
fn tower(sim: &mut Simulation) -> SimId {
    sim.spawn_building(BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(1, 1, 1, 1),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: Some(SpellcastingProfile {
            mana: ManaProfile::per_second(100, 100, 0),
            ability: AutomaticAbilityProfile {
                id: AbilityId(98),
                mana_cost: 3,
                cooldown_ticks: 1,
                range: 2000 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
                effect: AbilityEffect::BuildingPermanentGrant(profile()),
            },
        }),
    })
}
fn trigger(sim: &mut Simulation) -> SimId {
    sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(10, 1, 1, 1),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: None,
    })
}
fn restore(sim: &Simulation, workers: usize) -> Simulation {
    let content = crate::castle_fight_content_bundle(VERSION).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), content)
            .unwrap();
    let mut restored = Simulation::new(sim.config.clone(), workers);
    restored.restore_snapshot(&snapshot).unwrap();
    restored
}

#[test]
fn permanent_grants_separate_native_order_trigger_from_friendly_script_selection() {
    let mut sim = simulation(1);
    let caster = tower(&mut sim);
    let target = ally(&mut sim);
    let air = sim.spawn_unit_with_properties(
        unit(1, 80, 0),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            ..Default::default()
        },
    );
    sim.step();
    assert_eq!(sim.building(caster).unwrap().ability_cast_sequence, Some(0));
    assert!(
        sim.unit(target)
            .unwrap()
            .status
            .native_grants
            .iter()
            .next()
            .is_none()
    );
    sim.world.despawn(entity(&sim, air));
    trigger(&mut sim);
    sim.step();
    let view = sim.unit(target).unwrap();
    assert_eq!(view.health_max, 1037);
    assert_eq!(view.health, 1000);
    assert_eq!(view.attack.damage, 7);
    assert_eq!(view.status.native_grants.armor_bonus_per_100(), 200);
    assert_eq!(view.status.native_grants.attack_speed_percent(), 25);
    assert!(!view.status.native_grants.contains(profile().retry_marker));
    assert_eq!(sim.building(caster).unwrap().mana_current, Some(97));
    assert_eq!(
        sim.ability_casts_last_tick()[0].target,
        AbilityCastTarget::Unit(target)
    );
    // Parent commitment does not require a successful script selection.
    sim.world.despawn(entity(&sim, target));
    sim.step();
    assert_eq!(sim.building(caster).unwrap().mana_current, Some(94));
    assert_eq!(sim.building(caster).unwrap().ability_cast_sequence, Some(2));
    assert!(sim.ability_casts_last_tick().is_empty());
}

#[test]
fn permanent_grant_picker_preserves_native_retry_gate_and_explicit_negative_cases() {
    let mut sim = simulation(1);
    tower(&mut sim);
    trigger(&mut sim);
    let marked = ally(&mut sim);
    let unmarked = ally(&mut sim);
    sim.world
        .get_mut::<StatusState>(entity(&sim, marked))
        .unwrap()
        .native_grants
        .insert(grant(99, Effect::AuraMarker));
    sim.step();
    assert_eq!(
        sim.ability_casts_last_tick()[0].target,
        AbilityCastTarget::Unit(unmarked)
    );
    assert_eq!(
        sim.unit(marked)
            .unwrap()
            .status
            .native_grants
            .iter()
            .count(),
        1
    );
    sim.world.despawn(entity(&sim, unmarked));
    sim.step();
    assert_eq!(
        sim.ability_casts_last_tick()[0].target,
        AbilityCastTarget::Unit(marked)
    );
    assert_eq!(
        sim.unit(marked)
            .unwrap()
            .status
            .native_grants
            .iter()
            .count(),
        7
    );
    for (team, sapper, invulnerable, immune, invisible, mechanical, air, hero, outside, eligible) in [
        (0, true, false, true, true, true, true, true, false, true),
        (
            1, true, false, false, false, false, false, false, false, false,
        ),
        (
            0, false, false, false, false, false, false, false, false, false,
        ),
        (
            0, true, true, false, false, false, false, false, false, false,
        ),
        (
            0, true, false, false, false, false, false, false, true, false,
        ),
    ] {
        let mut sim = simulation(1);
        tower(&mut sim);
        trigger(&mut sim);
        let target = sim.spawn_unit_with_properties(
            unit(team, 40, 0),
            UnitGameplayProperties {
                mechanical,
                build_time_ticks: mechanical.then_some(3),
                movement_class: if air {
                    MovementClass::Air
                } else {
                    MovementClass::Ground
                },
                classifications: UnitClassifications {
                    combat_sapper: sapper,
                    invulnerable,
                    spell_immune: immune,
                    invisible,
                    hero,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        if outside {
            let bounds =
                crate::building_mechanics::snowveil_for_version(VERSION).battlefield_bounds;
            sim.world
                .get_mut::<Position>(entity(&sim, target))
                .unwrap()
                .0
                .x = bounds[2] + SUBUNITS_PER_WORLD_UNIT;
        }
        sim.step();
        assert_eq!(
            sim.unit(target)
                .unwrap()
                .status
                .native_grants
                .iter()
                .count(),
            if eligible { 6 } else { 0 }
        );
    }
}

#[test]
fn repeated_permanent_grants_do_not_stack_and_recover_independently_of_their_source() {
    let mut sim = simulation(1);
    let caster = tower(&mut sim);
    trigger(&mut sim);
    let target = ally(&mut sim);
    for _ in 0..30 {
        sim.step();
    }
    let view = sim.unit(target).unwrap();
    assert_eq!(view.status.native_grants.iter().count(), 10);
    assert_eq!(view.health_max, 1037);
    assert_eq!(view.health, 1000);
    assert_eq!(view.attack.damage, 7);
    assert_eq!(view.status.native_grants.attack_speed_percent(), 25);
    sim.world.despawn(entity(&sim, caster));
    let mut restored = restore(&sim, 4);
    for _ in 0..20 {
        assert_eq!(sim.step().checksum, restored.step().checksum);
    }
    assert_eq!(
        sim.unit(target).unwrap().status.native_grants,
        view.status.native_grants
    );
}

#[test]
fn identity_scoped_grants_share_native_life_and_preserve_acquisition_order() {
    let grants = profile().always;
    let forward = NativeAbilityGrants::from_slice(&grants);
    let reverse = NativeAbilityGrants::from_slice(&grants.into_iter().rev().collect::<Vec<_>>());
    assert_ne!(forward, reverse);
    assert_eq!(forward.iter().collect::<Vec<_>>(), grants);
    assert_eq!(
        reverse.iter().collect::<Vec<_>>(),
        grants.into_iter().rev().collect::<Vec<_>>()
    );
    let mut sim = simulation(1);
    let target = ally(&mut sim);
    let mut units = sim.snapshot_units();
    let target = units.iter_mut().find(|unit| unit.id == target).unwrap();
    target.status.native_grants = forward;
    target.health_max += 37;
    let aid = AbilityEffect::HolyAid {
        native_buff: None,
        modifier: ModifierId(111),
        healing: 4,
        armor_bonus_per_100: 0,
        regeneration_per_second_per_10k: 0,
        duration_ticks: 2,
        permanent_max_health_bonus: 37,
        resurrection_count: 0,
        resurrection_radius: 0,
        resurrection_mana_cost: 0,
        resurrection_cooldown_ticks: 0,
        resurrection_delay_ticks: 0,
    };
    assert!(apply_ability_effect_to_unit(
        target,
        aid,
        Team(0),
        0,
        sim.combat_rules.damage_rules
    ));
    assert_eq!(target.health_max, 1037);
    assert_eq!(target.health, 1004);
    assert_eq!(target.status.native_grants, forward);
    let revoked = target
        .status
        .native_grants
        .remove(&[grants[2].ability, grants[3].ability]);
    assert_eq!(revoked, 37);
    assert_eq!(target.status.native_grants.armor_bonus_per_100(), 0);
    assert_eq!(target.status.native_grants.attack_damage(), 7);
}

#[test]
fn granted_critical_and_evasion_use_native_passive_resolution_while_demolish_is_not_critical() {
    for (effect, building, expected, critical) in [
        (profile().options[0][0], false, 40, true),
        (profile().options[2][0], false, 20, false),
        (profile().options[2][0], true, 30, false),
    ] {
        let mut sim = simulation(1);
        let source = sim.spawn_unit(unit(0, 20, 20));
        sim.world
            .get_mut::<StatusState>(entity(&sim, source))
            .unwrap()
            .native_grants
            .insert(effect);
        let target = if building {
            trigger(&mut sim)
        } else {
            sim.spawn_unit(unit(1, 40, 0))
        };
        let units = sim.snapshot_units();
        let index = units.iter().position(|unit| unit.id == source).unwrap();
        let target_index = units
            .iter()
            .position(|unit| unit.id == target)
            .unwrap_or_default();
        let intent = AttackIntent {
            source: AttackSourceIndex::Unit(index),
            target: if building {
                TargetIndex::Building(0)
            } else {
                TargetIndex::Unit(target_index)
            },
            source_id: source,
            source_team: Team(0),
            source_position: units[index].position,
            target_id: target,
            attack: units[index].attack,
            attack_targets: AttackTargetMask::ALL,
            damage_type: DamageType::Normal,
            passive_effects: PassiveUnitEffects::default(),
            attack_sequence: 0,
            distance_sq: 0,
            completing_windup: false,
        };
        let (bonus, _, is_critical) = sim.resolve_passive_attack_effects(&intent, &units, 0);
        assert_eq!(20 + bonus, expected);
        assert_eq!(is_critical, critical);
        let mut disabled = units.clone();
        disabled[index].abilities_disabled = true;
        assert_eq!(
            sim.resolve_passive_attack_effects(&intent, &disabled, 0).0,
            0
        );
        if !building {
            let mut protected = units.clone();
            protected[target_index]
                .status
                .native_grants
                .insert(profile().options[1][0]);
            assert!(sim.attack_is_evaded(&intent, &protected, 0));
            protected[target_index].abilities_disabled = true;
            assert!(!sim.attack_is_evaded(&intent, &protected, 0));
        }
    }
    // A ranged release commits its granted damage; later ability removal cannot rewrite flight.
    let mut sim = simulation(1);
    let mut spawn = unit(0, 20, 20);
    spawn.attack.delivery = AttackDelivery::RangedGuaranteedHit {
        speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
    };
    let source = sim.spawn_unit(spawn);
    let target = sim.spawn_unit(unit(1, 30, 0));
    sim.world
        .get_mut::<StatusState>(entity(&sim, source))
        .unwrap()
        .native_grants =
        NativeAbilityGrants::from_slice(&[profile().always[1], profile().options[0][0]]);
    for _ in 0..3 {
        sim.step();
        if !sim.projectiles().is_empty() {
            break;
        }
    }
    assert!(!sim.projectiles().is_empty());
    sim.world
        .get_mut::<StatusState>(entity(&sim, source))
        .unwrap()
        .native_grants = NativeAbilityGrants::default();
    let mut restored = restore(&sim, 3);
    for _ in 0..15 {
        assert_eq!(sim.step().checksum, restored.step().checksum);
    }
    assert_eq!(sim.unit(target).unwrap().health, 946);
}

#[test]
fn native_grant_wire_inventory_rejects_duplicate_invalid_and_over_capacity_payloads() {
    let grants = profile().always;
    let encoded = serde_json::to_string(&NativeAbilityGrants::from_slice(&grants)).unwrap();
    assert_eq!(
        serde_json::from_str::<NativeAbilityGrants>(&encoded).unwrap(),
        NativeAbilityGrants::from_slice(&grants)
    );
    let reverse = grants.into_iter().rev().collect::<Vec<_>>();
    let reversed = serde_json::to_string(&reverse).unwrap();
    assert_eq!(
        serde_json::from_str::<NativeAbilityGrants>(&reversed).unwrap(),
        NativeAbilityGrants::from_slice(&reverse)
    );
    for invalid in [
        vec![grants[0], grants[0]],
        (0..=crate::MAX_PERMANENT_NATIVE_ABILITIES)
            .map(|id| grant(id as u32, Effect::AuraMarker))
            .collect(),
        vec![grant(
            1,
            Effect::Evasion {
                chance_per_10k: 10_001,
            },
        )],
    ] {
        assert!(
            serde_json::from_str::<NativeAbilityGrants>(&serde_json::to_string(&invalid).unwrap())
                .is_err()
        );
    }
}
