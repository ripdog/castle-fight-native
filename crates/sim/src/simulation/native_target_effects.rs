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
    let mut restored = Simulation::new(sim.config.clone(), workers);
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
                    ..UnitClassifications::default()
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

#[test]
fn ranged_bash_uses_intrinsic_hero_duration_at_impact_and_retains_profiles_on_wire() {
    use crate::components::BashEffectProfile;
    for hero in [false, true] {
        let mut sim = simulation(1);
        let source = sim.spawn_unit_with_properties(
            unit(
                0,
                20,
                10,
                AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: 10 * SUBUNITS_PER_WORLD_UNIT,
                },
            ),
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Bash(
                    BashEffectProfile {
                        ability: AbilityId(20),
                        chance_per_10k: 10_000,
                        bonus_damage: 7,
                        stun_duration_ticks: 9,
                        hero_stun_duration_ticks: 3,
                        targets: AttackTargetMask::GROUND_UNITS,
                    },
                )),
                ..UnitGameplayProperties::default()
            },
        );
        let target = sim.spawn_unit_with_properties(
            unit(1, 60, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                classifications: UnitClassifications {
                    hero,
                    ..UnitClassifications::default()
                },
                // Armor category deliberately differs from the intrinsic hero flag.
                armor: ArmorProfile {
                    armor_type: ArmorType::Hero,
                    ..ArmorProfile::default()
                },
                ..UnitGameplayProperties::default()
            },
        );
        sim.step();
        sim.step();
        assert_eq!(sim.unit(target).unwrap().stunned_until_tick, 0);
        let mut restored = wire_restored(&sim, 4);
        assert_eq!(sim.checksum(), restored.checksum());
        let source_entity = entity(&restored, source);
        if let Some(mut effects) = restored
            .world
            .entity_mut(source_entity)
            .get_mut::<PassiveUnitEffects>()
        {
            let PassiveUnitEffect::Bash(mut profile) = effects.iter().next().unwrap() else {
                panic!()
            };
            profile.hero_stun_duration_ticks += 1;
            *effects = PassiveUnitEffects::single(PassiveUnitEffect::Bash(profile));
        }
        assert_ne!(
            sim.checksum(),
            restored.checksum(),
            "hero duration belongs to canonical state"
        );
        restored = wire_restored(&sim, 4);
        for _ in 0..5 {
            sim.step();
            restored.step();
            assert_eq!(sim.checksum(), restored.checksum());
        }
        assert_eq!(
            sim.unit(target).unwrap().stunned_until_tick,
            5 + if hero { 3 } else { 9 }
        );
    }
}

#[test]
fn evasion_uses_only_the_highest_chance_independent_of_inventory_order() {
    fn trace(effects: &[PassiveUnitEffect], workers: usize) -> Vec<bool> {
        let mut sim = simulation(workers);
        let mut attacker = unit(0, 20, 0, AttackDelivery::Melee);
        attacker.attack.cooldown_ticks = 1;
        sim.spawn_unit(attacker);
        sim.spawn_unit_with_properties(
            unit(1, 60, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::from_slice(effects),
                ..UnitGameplayProperties::default()
            },
        );
        (0..128)
            .filter_map(|_| {
                sim.step();
                sim.attacks_last_tick().first().map(|attack| attack.missed)
            })
            .collect()
    }
    let high = PassiveUnitEffect::Evasion(EvasionEffectProfile {
        ability: AbilityId(20),
        chance_per_10k: 6000,
    });
    let low = PassiveUnitEffect::Evasion(EvasionEffectProfile {
        ability: AbilityId(21),
        chance_per_10k: 5000,
    });
    let expected = trace(&[high], 1);
    assert!(expected.contains(&true) && expected.contains(&false));
    assert_eq!(expected, trace(&[high, low], 4));
    assert_eq!(expected, trace(&[low, high], 1));
}

#[test]
fn multiple_critical_strikes_use_highest_successful_multiplier_not_sum() {
    let mut sim = simulation(1);
    let crit = |ability, multiplier| {
        PassiveUnitEffect::CriticalStrike(CriticalStrikeEffectProfile {
            ability: AbilityId(ability),
            chance_per_10k: 10_000,
            damage_multiplier_per_10k: multiplier,
            targets: AttackTargetMask::GROUND_UNITS,
        })
    };
    sim.spawn_unit_with_properties(
        unit(0, 20, 10, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::from_slice(&[crit(20, 20_000), crit(21, 30_000)]),
            ..UnitGameplayProperties::default()
        },
    );
    let target = sim.spawn_unit(unit(1, 60, 0, AttackDelivery::Melee));
    sim.step();
    sim.step();
    assert_eq!(sim.unit(target).unwrap().health, 970);
    assert!(sim.attacks_last_tick().iter().any(|attack| attack.critical));
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

fn always_faerie_profile() -> SpellcastingProfile {
    let mut profile = faerie_profile();
    profile.ability.target_policy = AbilityTargetPolicy::RandomEnemyDebuff;
    profile
}

#[test]
fn always_autocast_faerie_fire_marks_idle_nonattacking_units_but_not_invalid_classes() {
    let mut sim = simulation(1);
    let caster = sim.spawn_unit_with_spellcasting(
        unit(0, 20, 0, AttackDelivery::Melee),
        always_faerie_profile(),
    );
    let allied = sim.spawn_unit(unit(0, 30, 0, AttackDelivery::Melee));
    let immune = sim.spawn_unit_with_properties(
        unit(1, 40, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                spell_immune: true,
                ..UnitClassifications::default()
            },
            ..UnitGameplayProperties::default()
        },
    );
    let mechanical = sim.spawn_unit(unit(1, 50, 0, AttackDelivery::Melee));
    sim.world
        .entity_mut(entity(&sim, mechanical))
        .insert(MechanicalUnit);
    let out_of_range = sim.spawn_unit(unit(1, 121, 0, AttackDelivery::Melee));
    let target = sim.spawn_unit(unit(1, 120, 0, AttackDelivery::Melee));
    sim.step();
    assert!(sim.unit(target).unwrap().status.is_revealed_to(Team(0), 0));
    for id in [allied, immune, mechanical, out_of_range] {
        assert!(!sim.unit(id).unwrap().status.is_revealed_to(Team(0), 0));
    }
    assert_eq!(sim.unit(caster).unwrap().mana_current, Some(9));
    let mut restored = wire_restored(&sim, 4);
    assert_eq!(sim.checksum(), restored.checksum());
    for _ in 0..6 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert_eq!(
        sim.unit(caster).unwrap().mana_current,
        Some(9),
        "marked target must not refresh or cost more mana"
    );
}

#[test]
fn always_autocast_faerie_fire_uses_seeded_viable_selection_not_nearest_combat_priority() {
    let mut seen = std::collections::BTreeSet::new();
    for seed in 0..24 {
        let make = |workers| {
            Simulation::new(
                SimulationConfig {
                    match_seed: seed,
                    navigation_max: NavCell::new(500, 64),
                    ..SimulationConfig::default()
                },
                workers,
            )
        };
        let mut sim = make(1);
        sim.spawn_unit_with_spellcasting(
            unit(0, 20, 0, AttackDelivery::Melee),
            always_faerie_profile(),
        );
        let near = sim.spawn_unit(unit(1, 40, 0, AttackDelivery::Melee));
        let far = sim.spawn_unit(unit(1, 80, 0, AttackDelivery::Melee));
        let mut other = wire_restored(&sim, 4);
        sim.step();
        other.step();
        assert_eq!(sim.checksum(), other.checksum());
        assert_eq!(
            sim.ability_casts_last_tick(),
            other.ability_casts_last_tick()
        );
        let selected = match sim.ability_casts_last_tick()[0].target {
            AbilityCastTarget::Unit(id) => id,
            _ => panic!(),
        };
        assert!(selected == near || selected == far);
        seen.insert(selected == far);
    }
    assert_eq!(
        seen.len(),
        2,
        "random viable selection must not always prefer nearest"
    );
}

#[test]
fn faerie_fire_simultaneous_casters_revalidate_marks_before_spending_resources() {
    let mut sim = simulation(4);
    let first = sim.spawn_unit_with_spellcasting(
        unit(0, 20, 0, AttackDelivery::Melee),
        always_faerie_profile(),
    );
    let second = sim.spawn_unit_with_spellcasting(
        unit(0, 30, 0, AttackDelivery::Melee),
        always_faerie_profile(),
    );
    let target = sim.spawn_unit(unit(1, 60, 0, AttackDelivery::Melee));
    sim.step();
    assert_eq!(sim.ability_casts_last_tick().len(), 1);
    assert_eq!(sim.unit(first).unwrap().mana_current, Some(9));
    assert_eq!(sim.unit(second).unwrap().mana_current, Some(12));
    assert_eq!(sim.unit(target).unwrap().status.armor_modifier_count, 1);
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

#[test]
fn cleave_uses_primary_center_mask_and_critical_damage_only_on_landed_melee_hits() {
    for targets in [
        AttackTargetMask::GROUND_UNITS,
        AttackTargetMask::GROUND_AND_BUILDINGS,
    ] {
        for delivery in [AttackDelivery::Melee, AttackDelivery::RangedInstant] {
            for evade in [0, 10_000] {
                let mut sim = simulation(1);
                sim.spawn_unit_with_properties(
                    unit(0, 20, 40, delivery),
                    UnitGameplayProperties {
                        passive_effects: PassiveUnitEffects::from_slice(&[
                            PassiveUnitEffect::Cleave(crate::components::CleaveEffectProfile {
                                ability: AbilityId(101),
                                radius: 60 * SUBUNITS_PER_WORLD_UNIT,
                                damage_per_10k: 5000,
                                targets,
                            }),
                            PassiveUnitEffect::CriticalStrike(CriticalStrikeEffectProfile {
                                ability: AbilityId(102),
                                chance_per_10k: 10_000,
                                damage_multiplier_per_10k: 20_000,
                                targets: AttackTargetMask::GROUND_UNITS,
                            }),
                        ]),
                        ..UnitGameplayProperties::default()
                    },
                );
                let primary = sim.spawn_unit_with_properties(
                    unit(1, 60, 0, AttackDelivery::Melee),
                    UnitGameplayProperties {
                        passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Evasion(
                            EvasionEffectProfile {
                                ability: AbilityId(103),
                                chance_per_10k: evade,
                            },
                        )),
                        ..UnitGameplayProperties::default()
                    },
                );
                let secondary = sim.spawn_unit(unit(1, 115, 0, AttackDelivery::Melee));
                let outside = sim.spawn_unit(unit(1, 125, 0, AttackDelivery::Melee));
                let ally = sim.spawn_unit(unit(0, 105, 0, AttackDelivery::Melee));
                let air = sim.spawn_unit_with_properties(
                    unit(1, 105, 0, AttackDelivery::Melee),
                    UnitGameplayProperties {
                        movement_class: MovementClass::Air,
                        ..UnitGameplayProperties::default()
                    },
                );
                let structure = sim.spawn_building(BuildingSpawn {
                    team: Team(1),
                    footprint: BuildingFootprint::new(3, 0, 1, 1),
                    health: 1000,
                    production: None,
                    attack: None,
                    spellcasting: None,
                });
                sim.step();
                let mut restored = wire_restored(&sim, 4);
                sim.step();
                restored.step();
                assert_eq!(sim.checksum(), restored.checksum());
                let landed = evade == 0;
                let cleaved = landed && delivery == AttackDelivery::Melee;
                assert_eq!(
                    sim.unit(primary).unwrap().health,
                    1000 - if landed { 80 } else { 0 }
                );
                assert_eq!(
                    sim.unit(secondary).unwrap().health,
                    1000 - if cleaved { 40 } else { 0 }
                );
                for id in [outside, ally, air] {
                    assert_eq!(sim.unit(id).unwrap().health, 1000);
                }
                assert_eq!(
                    sim.building(structure).unwrap().health,
                    1000 - if cleaved && targets.can_target_buildings() {
                        40
                    } else {
                        0
                    }
                );
            }
        }
    }
}

fn area_stun() -> SpellcastingProfile {
    SpellcastingProfile {
        mana: ManaProfile {
            maximum: 50,
            starting: 30,
            regen_per_tick_per_10k: 0,
        },
        ability: AutomaticAbilityProfile {
            id: AbilityId(201),
            mana_cost: 10,
            cooldown_ticks: 20,
            range: 50 * SUBUNITS_PER_WORLD_UNIT,
            target_policy: AbilityTargetPolicy::RandomGroundEnemyUnit,
            effect: AbilityEffect::AreaStun {
                ability: AbilityId(202),
                damage: 13,
                radius: 70 * SUBUNITS_PER_WORLD_UNIT,
                stun_ticks: 9,
                hero_stun_ticks: 3,
                targets: AttackTargetMask::GROUND_UNITS,
            },
        },
    }
}

#[test]
fn proxy_area_stun_separates_ground_sapper_trigger_from_caster_centered_native_effect() {
    let mut sim = simulation(1);
    let source =
        sim.spawn_unit_with_spellcasting(unit(0, 100, 0, AttackDelivery::Melee), area_stun());
    let primary = sim.spawn_unit_with_properties(
        unit(1, 150, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                combat_sapper: true,
                ..UnitClassifications::default()
            },
            ..UnitGameplayProperties::default()
        },
    );
    let secondary = sim.spawn_unit(unit(1, 40, 0, AttackDelivery::Melee));
    let outside = sim.spawn_unit(unit(1, 200, 0, AttackDelivery::Melee));
    let hero = sim.spawn_unit_with_properties(
        unit(1, 70, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                hero: true,
                ..UnitClassifications::default()
            },
            ..UnitGameplayProperties::default()
        },
    );
    let air = sim.spawn_unit_with_properties(
        unit(1, 110, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            ..UnitGameplayProperties::default()
        },
    );
    let ally = sim.spawn_unit(unit(0, 105, 0, AttackDelivery::Melee));
    let immune = sim.spawn_unit_with_properties(
        unit(1, 110, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                spell_immune: true,
                ..UnitClassifications::default()
            },
            ..UnitGameplayProperties::default()
        },
    );
    sim.step();
    for id in [primary, secondary, hero] {
        assert_eq!(sim.unit(id).unwrap().health, 987);
    }
    for id in [outside, air, ally, immune] {
        assert_eq!(sim.unit(id).unwrap().health, 1000);
    }
    assert_eq!(sim.unit(source).unwrap().mana_current, Some(20));
    assert_eq!(sim.unit(hero).unwrap().status.stunned_until_tick, 3);
    assert_eq!(sim.unit(secondary).unwrap().status.stunned_until_tick, 9);
    let cast = sim
        .ability_casts_last_tick()
        .iter()
        .find(|cast| cast.source == source)
        .unwrap();
    assert_eq!(cast.ability, AbilityId(202));
    assert_eq!(
        cast.target_position,
        Some(sim.unit(source).unwrap().position)
    );
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..12 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert_eq!(sim.unit(source).unwrap().mana_current, Some(20));
}

#[test]
fn proxy_area_stun_rejects_invalid_trigger_classes_range_and_insufficient_mana() {
    for (movement, flags, x, mana) in [
        (
            MovementClass::Air,
            UnitClassifications {
                combat_sapper: true,
                ..UnitClassifications::default()
            },
            140,
            30,
        ),
        (
            MovementClass::Ground,
            UnitClassifications::default(),
            140,
            30,
        ),
        (
            MovementClass::Ground,
            UnitClassifications {
                combat_sapper: true,
                hero: true,
                ..UnitClassifications::default()
            },
            140,
            30,
        ),
        (
            MovementClass::Ground,
            UnitClassifications {
                combat_sapper: true,
                invulnerable: true,
                ..UnitClassifications::default()
            },
            140,
            30,
        ),
        (
            MovementClass::Ground,
            UnitClassifications {
                combat_sapper: true,
                spell_immune: true,
                ..UnitClassifications::default()
            },
            140,
            30,
        ),
        (
            MovementClass::Ground,
            UnitClassifications {
                combat_sapper: true,
                ..UnitClassifications::default()
            },
            151,
            30,
        ),
        (
            MovementClass::Ground,
            UnitClassifications {
                combat_sapper: true,
                ..UnitClassifications::default()
            },
            140,
            9,
        ),
    ] {
        let mut sim = simulation(1);
        let mut profile = area_stun();
        profile.mana.starting = mana;
        let source =
            sim.spawn_unit_with_spellcasting(unit(0, 100, 0, AttackDelivery::Melee), profile);
        sim.spawn_unit_with_properties(
            unit(1, x, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                movement_class: movement,
                classifications: flags,
                ..UnitGameplayProperties::default()
            },
        );
        sim.step();
        assert!(sim.ability_casts_last_tick().is_empty());
        assert_eq!(sim.unit(source).unwrap().mana_current, Some(mana));
    }
}

#[test]
fn orb_child_nonhero_restriction_does_not_cancel_primary_hit_and_retains_hero_duration() {
    for nonhero_only in [false, true] {
        let mut sim = simulation(1);
        sim.spawn_unit_with_properties(
            unit(0, 20, 10, AttackDelivery::Melee),
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::TriggeredSpellProc(
                    crate::TriggeredSpellProcProfile {
                        ability: AbilityId(301),
                        chance_per_10k: 10_000,
                        targets: AttackTargetMask::AIR_AND_GROUND,
                        effect: crate::TriggeredAttackEffect::EntanglingRoots(
                            crate::EntanglingRootsEffectProfile {
                                ability: AbilityId(302),
                                damage_per_second: 1,
                                duration_ticks: 9,
                                hero_duration_ticks: 3,
                                nonhero_only,
                                targets: AttackTargetMask::AIR_AND_GROUND,
                            },
                        ),
                    },
                )),
                ..UnitGameplayProperties::default()
            },
        );
        let hero = sim.spawn_unit_with_properties(
            unit(1, 60, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                classifications: UnitClassifications {
                    hero: true,
                    ..UnitClassifications::default()
                },
                ..UnitGameplayProperties::default()
            },
        );
        sim.step();
        sim.step();
        assert_eq!(sim.unit(hero).unwrap().health, 990);
        assert_eq!(
            sim.unit(hero).unwrap().status.stunned_until_tick,
            if nonhero_only { 0 } else { 4 }
        );
        let mut restored = wire_restored(&sim, 4);
        for _ in 0..12 {
            sim.step();
            restored.step();
            assert_eq!(sim.checksum(), restored.checksum());
        }
    }
}

#[test]
fn frost_attack_retains_independent_orb_payload_and_live_hero_immunity_after_restore() {
    use crate::components::{FrostAttackEffectProfile, TriggeredSpellProcProfile};
    for (hero, immune, evades) in [
        (false, false, false),
        (true, false, false),
        (true, true, false),
        (false, true, false),
        (false, false, true),
    ] {
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
                passive_effects: PassiveUnitEffects::from_slice(&[
                    PassiveUnitEffect::FrostAttack(FrostAttackEffectProfile {
                        ability: AbilityId(21),
                        duration_ticks: 12,
                        hero_duration_ticks: 3,
                        movement_percent_delta: -40,
                        attack_speed_percent_delta: -20,
                        targets: AttackTargetMask::AIR_AND_GROUND,
                    }),
                    PassiveUnitEffect::TriggeredSpellProc(TriggeredSpellProcProfile {
                        ability: AbilityId(22),
                        chance_per_10k: 10_000,
                        targets: AttackTargetMask::AIR_AND_GROUND,
                        effect: TriggeredAttackEffect::EntanglingRoots(
                            crate::components::EntanglingRootsEffectProfile {
                                ability: AbilityId(23),
                                damage_per_second: 1,
                                duration_ticks: 2,
                                hero_duration_ticks: 2,
                                nonhero_only: true,
                                targets: AttackTargetMask::AIR_AND_GROUND,
                            },
                        ),
                    }),
                ]),
                ..UnitGameplayProperties::default()
            },
        );
        let victim = sim.spawn_unit_with_properties(
            unit(1, 60, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                classifications: UnitClassifications {
                    hero,
                    ..UnitClassifications::default()
                },
                passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Evasion(
                    EvasionEffectProfile {
                        ability: AbilityId(24),
                        chance_per_10k: if evades { 10_000 } else { 0 },
                    },
                )),
                ..UnitGameplayProperties::default()
            },
        );
        sim.step();
        sim.step();
        assert_eq!(sim.unit(victim).unwrap().health, 1000);
        sim.world
            .entity_mut(entity(&sim, victim))
            .insert(UnitClassifications {
                hero,
                spell_immune: immune,
                ..UnitClassifications::default()
            });
        let mut restored = wire_restored(&sim, 4);
        for _ in 0..4 {
            sim.step();
            restored.step();
            assert_eq!(sim.checksum(), restored.checksum());
        }
        let target = sim.world.entity(entity(&sim, victim));
        let status = target.get::<StatusState>().unwrap();
        let frost = status
            .attack_speed_modifiers
            .iter()
            .find(|p| p.id == ModifierId(21));
        if immune || evades {
            assert!(frost.is_none());
            assert_eq!(status.stunned_until_tick, 0);
        } else {
            assert_eq!(frost.unwrap().percent_delta, -20);
            assert_eq!(
                frost.unwrap().expires_tick,
                sim.tick() - 1 + if hero { 3 } else { 12 }
            );
            assert_eq!(status.stunned_until_tick > sim.tick(), !hero);
        }
        for _ in 0..14 {
            sim.step();
            restored.step();
            assert_eq!(sim.checksum(), restored.checksum());
        }
    }
}

#[test]
fn pulverize_uses_caster_radii_independent_physical_proc_and_restored_rng() {
    for chance in [0, 10_000] {
        for evade in [0, 10_000] {
            let mut sim = simulation(1);
            let caster = sim.spawn_unit_with_properties(
                unit(0, 20, 10, AttackDelivery::Melee),
                UnitGameplayProperties {
                    passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Pulverize(
                        crate::PulverizeEffectProfile {
                            ability: AbilityId(41),
                            chance_per_10k: chance,
                            damage: 20,
                            full_radius: 40 * SUBUNITS_PER_WORLD_UNIT,
                            half_radius: 70 * SUBUNITS_PER_WORLD_UNIT,
                            targets: AttackTargetMask::GROUND_UNITS,
                        },
                    )),
                    ..UnitGameplayProperties::default()
                },
            );
            let primary = sim.spawn_unit_with_properties(
                unit(1, 60, 0, AttackDelivery::Melee),
                UnitGameplayProperties {
                    classifications: UnitClassifications {
                        spell_immune: true,
                        ..UnitClassifications::default()
                    },
                    passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Evasion(
                        EvasionEffectProfile {
                            ability: AbilityId(42),
                            chance_per_10k: evade,
                        },
                    )),
                    ..UnitGameplayProperties::default()
                },
            );
            let half = sim.spawn_unit(unit(1, 90, 0, AttackDelivery::Melee));
            let outside = sim.spawn_unit(unit(1, 91, 0, AttackDelivery::Melee));
            let ally = sim.spawn_unit(unit(0, 90, 0, AttackDelivery::Melee));
            let air = sim.spawn_unit_with_properties(
                unit(1, 90, 0, AttackDelivery::Melee),
                UnitGameplayProperties {
                    movement_class: MovementClass::Air,
                    ..UnitGameplayProperties::default()
                },
            );
            let structure = sim.spawn_building(BuildingSpawn {
                team: Team(1),
                footprint: BuildingFootprint::new(2, 0, 1, 1),
                health: 1000,
                production: None,
                attack: None,
                spellcasting: None,
            });
            sim.step();
            let mut restored = wire_restored(&sim, 4);
            sim.step();
            restored.step();
            assert_eq!(sim.checksum(), restored.checksum());
            assert_eq!(
                sim.unit(primary).unwrap().health,
                1000 - if evade == 0 { 10 } else { 0 } - if chance > 0 { 20 } else { 0 }
            );
            assert_eq!(
                sim.unit(half).unwrap().health,
                1000 - if chance > 0 { 10 } else { 0 }
            );
            for id in [outside, ally, air] {
                assert_eq!(sim.unit(id).unwrap().health, 1000);
            }
            assert_eq!(sim.building(structure).unwrap().health, 1000);
            if chance > 0 {
                let effect = sim
                    .ability_casts_last_tick()
                    .iter()
                    .find(|e| e.source == caster)
                    .unwrap();
                assert_eq!(
                    effect.target_position,
                    Some(sim.unit(caster).unwrap().position)
                );
            }
        }
    }
}
