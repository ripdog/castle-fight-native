use super::*;
use crate::{
    building_mechanics::BuildingAuraProfile,
    components::{AuraCarrierState, NativeAction},
};

fn profile() -> BuildingAuraProfile {
    BuildingAuraProfile {
        map_version: crate::MapVersion::CASTLE_FIGHT_9_27,
        ability: AbilityId(101),
        buff_rawcode: 102,
        trigger_targets: AttackTargetMask::GROUND_AND_BUILDINGS,
        trigger_invulnerable: true,
        trigger_spell_immune: false,
        targets: AttackTargetMask::AIR_AND_GROUND,
        organic_only: true,
        affects_invulnerable: true,
        radius: 40 * SUBUNITS_PER_WORLD_UNIT,
        regeneration_per_second_per_10k: 450_000,
        movement_percent_delta: 0,
        attack_speed_percent_delta: 0,
        lifetime_ticks: 8,
        stand_ticks: 3,
    }
}

fn carrier(sim: &mut Simulation, team: Team, x: i32, profile: BuildingAuraProfile) -> SimId {
    let id = sim.allocate_id();
    sim.world.spawn((
        id,
        NativeAction::AuraCarrier(AuraCarrierState {
            source: SimId(999),
            team,
            position: unit(team.0, x, 0, AttackDelivery::Melee).position,
            profile,
            created_tick: sim.tick(),
            expires_tick: sim.tick() + u64::from(profile.lifetime_ticks),
        }),
    ));
    id
}
fn wound(sim: &mut Simulation, id: SimId, current: i32) {
    sim.world
        .get_mut::<Health>(entity(sim, id))
        .unwrap()
        .current = current;
}
fn rate(sim: &Simulation, id: SimId) -> u32 {
    let view = sim.unit(id).unwrap();
    view.status.armor_modifiers[..usize::from(view.status.armor_modifier_count)]
        .iter()
        .filter(|m| m.expires_tick > sim.tick().saturating_sub(1))
        .map(|m| m.regeneration_per_second_per_10k)
        .sum()
}

#[test]
fn carrier_healing_uses_fixed_origin_live_organic_allies_and_exact_closing_interval() {
    let mut sim = simulation(1);
    let aura = carrier(&mut sim, Team(0), 50, profile());
    let ground = sim.spawn_unit(unit(0, 90, 0, AttackDelivery::Melee));
    let protected_air = sim.spawn_unit_with_properties(
        unit(0, 80, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            classifications: UnitClassifications {
                hero: true,
                spell_immune: true,
                invulnerable: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let mechanical = sim.spawn_unit_with_properties(
        unit(0, 70, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            mechanical: true,
            build_time_ticks: Some(1),
            ..Default::default()
        },
    );
    let outside = sim.spawn_unit(unit(0, 91, 0, AttackDelivery::Melee));
    let enemy = sim.spawn_unit(unit(1, 70, 0, AttackDelivery::Melee));
    let dead = sim.spawn_unit(unit(0, 70, 0, AttackDelivery::Melee));
    for id in [ground, protected_air, mechanical, outside, enemy] {
        wound(&mut sim, id, 900);
    }
    wound(&mut sim, dead, 0);
    sim.set_negative_building_shield(ground, 2, None);
    let building = sim.spawn_building(BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(60, 40, 1, 1),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: None,
    });
    wound(&mut sim, building, 900);
    sim.step();
    assert_eq!(sim.unit(ground).unwrap().health, 900); // No elapsed interval on creation.
    assert_eq!(
        rate(&sim, ground),
        profile().regeneration_per_second_per_10k
    );
    let mut restored = wire_restored(&sim, 4);
    while sim.tick() <= u64::from(profile().lifetime_ticks) {
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
    // Eight completed intervals at 1.5 HP/tick, including the closing interval at expiry.
    for id in [ground, protected_air] {
        assert_eq!(sim.unit(id).unwrap().health, 912);
        assert_eq!(rate(&sim, id), 0);
    }
    for id in [mechanical, outside, enemy] {
        assert_eq!(sim.unit(id).unwrap().health, 900);
    }
    assert_eq!(sim.building(building).unwrap().health, 900);
    assert!(sim.unit(dead).is_none_or(|unit| unit.health == 0));
    assert_eq!(sim.unit(ground).unwrap().negative_building_shield_level, 2);
    assert!(
        !sim.world
            .iter_entities()
            .any(|e| e.get::<SimId>() == Some(&aura))
    );
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
    assert_eq!(sim.unit(ground).unwrap().health, 912);
}

#[test]
fn carrier_healing_resamples_membership_without_linger_and_clamps_full_health() {
    let mut sim = simulation(1);
    carrier(&mut sim, Team(0), 50, profile());
    let mover = sim.spawn_unit(unit(0, 90, 0, AttackDelivery::Melee));
    let full = sim.spawn_unit(unit(0, 80, 0, AttackDelivery::Melee));
    wound(&mut sim, mover, 900);
    sim.step();
    sim.step();
    assert_eq!(sim.unit(mover).unwrap().health, 901);
    sim.world
        .entity_mut(entity(&sim, mover))
        .insert(Position(unit(0, 91, 0, AttackDelivery::Melee).position));
    sim.step();
    assert_eq!(sim.unit(mover).unwrap().health, 901);
    assert_eq!(rate(&sim, mover), 0);
    assert_eq!(sim.unit(full).unwrap().health, 1000);
    sim.world
        .entity_mut(entity(&sim, mover))
        .insert(Position(unit(0, 90, 0, AttackDelivery::Melee).position));
    wound(&mut sim, full, 998);
    let mut restored = wire_restored(&sim, 3);
    for _ in 0..3 {
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
    assert_eq!(sim.unit(full).unwrap().health, 1000);
    assert_eq!(sim.unit(mover).unwrap().health, 905);
    assert_eq!(
        sim.world
            .get::<HealthRegeneration>(entity(&sim, full))
            .unwrap()
            .remainder_per_10k_hz,
        0
    );
}

#[test]
fn overlapping_carriers_share_one_native_buff_and_fall_back_after_stronger_expiry() {
    let mut sim = simulation(1);
    let weaker = BuildingAuraProfile {
        regeneration_per_second_per_10k: 300_000,
        lifetime_ticks: 6,
        ..profile()
    };
    carrier(&mut sim, Team(0), 50, weaker);
    carrier(&mut sim, Team(0), 50, weaker);
    let stronger = BuildingAuraProfile {
        ability: AbilityId(103),
        lifetime_ticks: 2,
        ..profile()
    };
    carrier(&mut sim, Team(0), 50, stronger);
    let target = sim.spawn_unit(unit(0, 90, 0, AttackDelivery::Melee));
    wound(&mut sim, target, 900);
    sim.step();
    assert_eq!(rate(&sim, target), stronger.regeneration_per_second_per_10k);
    assert_eq!(sim.unit(target).unwrap().status.armor_modifier_count, 1);
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..2 {
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
    assert_eq!(sim.unit(target).unwrap().health, 903);
    assert_eq!(rate(&sim, target), weaker.regeneration_per_second_per_10k);
    while sim.tick() <= 6 {
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
    assert_eq!(sim.unit(target).unwrap().health, 907);
    assert_eq!(rate(&sim, target), 0);
}

#[test]
fn carrier_native_aura_reapplies_after_removal_and_cannot_be_stolen() {
    for aura_profile in [profile(), speed_profile()] {
        let mut sim = simulation(1);
        carrier(&mut sim, Team(0), 50, aura_profile);
        let donor = sim.spawn_unit(unit(0, 60, 0, AttackDelivery::Melee));
        wound(&mut sim, donor, 900);
        let caster = sim.spawn_unit_with_spellcasting(
            unit(1, 80, 0, AttackDelivery::Melee),
            SpellcastingProfile {
                mana: ManaProfile::per_second(10, 10, 0),
                ability: AutomaticAbilityProfile {
                    id: AbilityId(200),
                    mana_cost: 3,
                    cooldown_ticks: 100,
                    range: 100 * SUBUNITS_PER_WORLD_UNIT,
                    target_policy: AbilityTargetPolicy::NativeBuffDonor,
                    effect: AbilityEffect::SpellSteal {
                        recipient_radius: 100 * SUBUNITS_PER_WORLD_UNIT,
                    },
                },
            },
        );
        sim.step();
        let mut status = sim
            .world
            .get_mut::<StatusState>(entity(&sim, donor))
            .unwrap();
        status.armor_modifiers = Default::default();
        status.armor_modifier_count = 0;
        status.movement_modifiers = Default::default();
        status.movement_modifier_count = 0;
        status.attack_speed_modifiers = Default::default();
        status.attack_speed_modifier_count = 0;
        let mut restored = wire_restored(&sim, 4);
        for _ in 0..2 {
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
        assert_eq!(
            sim.unit(donor).unwrap().health,
            if aura_profile.regeneration_per_second_per_10k > 0 {
                903
            } else {
                900
            }
        );
        assert_eq!(
            rate(&sim, donor),
            aura_profile.regeneration_per_second_per_10k
        );
        assert_eq!(sim.unit(caster).unwrap().mana_current, Some(10));
        assert_eq!(sim.unit(caster).unwrap().ability_ready_tick, Some(0));
        assert_eq!(sim.unit(caster).unwrap().status.armor_modifier_count, 0);
    }
}

#[test]
fn hostile_trigger_and_friendly_carrier_effect_commit_independently_and_survive_source_death() {
    for aura_profile in [profile(), speed_profile()] {
        for (structure_trigger, eligible_ally) in [(false, true), (true, true), (true, false)] {
            let mut sim = simulation(1);
            let caster = sim.spawn_building(BuildingSpawn {
                team: Team(0),
                footprint: BuildingFootprint::new(1, 1, 1, 1),
                health: 1000,
                attack: None,
                production: None,
                spellcasting: Some(SpellcastingProfile {
                    mana: ManaProfile::per_second(10, 10, 0),
                    ability: AutomaticAbilityProfile {
                        id: AbilityId(100),
                        mana_cost: 3,
                        cooldown_ticks: 100,
                        range: 500 * SUBUNITS_PER_WORLD_UNIT,
                        target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
                        effect: AbilityEffect::BuildingAura(aura_profile),
                    },
                }),
            });
            let origin = footprint_center_point(
                sim.building(caster).unwrap().footprint,
                sim.config.navigation_cell_size,
            );
            let target = sim.spawn_unit_with_properties(
                UnitSpawn {
                    position: SimPoint::new(
                        origin.x + aura_profile.radius + i32::from(!eligible_ally),
                        origin.y,
                    ),
                    ..unit(0, 0, 0, AttackDelivery::Melee)
                },
                UnitGameplayProperties {
                    movement_class: MovementClass::Air,
                    mechanical: !eligible_ally,
                    build_time_ticks: Some(1),
                    ..Default::default()
                },
            );
            wound(&mut sim, target, 900);
            if structure_trigger {
                sim.spawn_building_with_properties(
                    BuildingSpawn {
                        team: Team(1),
                        footprint: BuildingFootprint::new(200, 20, 1, 1),
                        health: 1000,
                        attack: None,
                        production: None,
                        spellcasting: None,
                    },
                    BuildingGameplayProperties {
                        classifications: UnitClassifications {
                            invulnerable: true,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                );
            } else {
                sim.spawn_unit_with_properties(
                    unit(1, 200, 0, AttackDelivery::Melee),
                    UnitGameplayProperties {
                        movement_class: MovementClass::Air,
                        ..Default::default()
                    },
                );
            }
            sim.step();
            let view = sim.building(caster).unwrap();
            assert_eq!(
                view.mana_current,
                Some(if structure_trigger { 7 } else { 10 })
            );
            if !structure_trigger {
                assert!(sim.aura_carriers().is_empty());
                assert_eq!(rate(&sim, target), 0);
                continue;
            }
            assert_eq!(view.ability_ready_tick, Some(100));
            assert_eq!(
                view.status.action_animation.unwrap().until_tick,
                u64::from(aura_profile.stand_ticks)
            );
            assert_eq!(sim.last_ability_casts[0].ability, aura_profile.ability);
            assert_eq!(sim.last_ability_casts[0].target_position, Some(origin));
            if eligible_ally {
                sim.world.despawn(entity(&sim, caster));
            }
            let mut restored = wire_restored(&sim, 4);
            while sim.tick() <= u64::from(aura_profile.lifetime_ticks) {
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
            assert_eq!(
                sim.unit(target).unwrap().health,
                if eligible_ally && aura_profile.regeneration_per_second_per_10k > 0 {
                    912
                } else {
                    900
                }
            );
            if !eligible_ally {
                assert!(
                    sim.building(caster)
                        .unwrap()
                        .status
                        .action_animation
                        .is_none()
                );
            }
        }
    }
}

fn speed_profile() -> BuildingAuraProfile {
    BuildingAuraProfile {
        regeneration_per_second_per_10k: 0,
        organic_only: false,
        movement_percent_delta: 50,
        attack_speed_percent_delta: 100,
        lifetime_ticks: 6,
        ..profile()
    }
}

#[test]
fn speed_carrier_samples_air_mechanical_protected_allies_and_expires_without_linger() {
    for affects_invulnerable in [false, true] {
        let mut sim = simulation(1);
        let aura = carrier(
            &mut sim,
            Team(0),
            50,
            BuildingAuraProfile {
                affects_invulnerable,
                ..speed_profile()
            },
        );
        let ordinary = sim.spawn_unit(unit(0, 90, 0, AttackDelivery::Melee));
        let protected = sim.spawn_unit_with_properties(
            unit(0, 80, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                mechanical: true,
                build_time_ticks: Some(1),
                movement_class: MovementClass::Air,
                classifications: UnitClassifications {
                    spell_immune: true,
                    invulnerable: true,
                    hero: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let enemy = sim.spawn_unit(unit(1, 70, 0, AttackDelivery::Melee));
        let outside = sim.spawn_unit(unit(0, 91, 0, AttackDelivery::Melee));
        wound(&mut sim, ordinary, 900);
        sim.step();
        assert_eq!(sim.aura_carriers()[0].id, aura);
        assert_eq!(
            sim.aura_carriers()[0].position,
            unit(0, 50, 0, AttackDelivery::Melee).position
        );
        assert!(sim.units().iter().all(|unit| unit.id != aura));
        for (id, eligible) in [
            (ordinary, true),
            (protected, affects_invulnerable),
            (enemy, false),
            (outside, false),
        ] {
            let status = sim.unit(id).unwrap().status;
            assert_eq!(status.movement_modifier_count, u8::from(eligible));
            assert_eq!(status.attack_speed_modifier_count, u8::from(eligible));
            if eligible {
                assert_eq!(
                    status.movement_modifiers[0].percent_delta,
                    speed_profile().movement_percent_delta
                );
                assert_eq!(
                    status.attack_speed_modifiers[0].percent_delta,
                    speed_profile().attack_speed_percent_delta
                );
                assert!(!status.armor_modifiers[0].native_buff.unwrap().stealable);
            }
        }
        let mut restored = wire_restored(&sim, 4);
        while sim.tick() <= 6 {
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
        assert!(sim.aura_carriers().is_empty());
        assert_eq!(sim.unit(ordinary).unwrap().health, 900);
        for id in [ordinary, protected] {
            let status = sim.unit(id).unwrap().status;
            assert_eq!(status.movement_modifier_count, 0);
            assert_eq!(status.attack_speed_modifier_count, 0);
            assert_eq!(status.armor_modifier_count, 0);
        }
    }
}

#[test]
fn speed_carrier_changes_real_attack_cadence_and_live_movement_speed() {
    let mut sim = simulation(1);
    carrier(&mut sim, Team(0), 50, speed_profile());
    let mut template = unit(0, 90, 5, AttackDelivery::Melee);
    template.attack.cooldown_ticks = 4;
    template.movement.speed_per_tick = 10 * SUBUNITS_PER_WORLD_UNIT;
    let attacker = sim.spawn_unit(template);
    sim.spawn_unit(unit(1, 120, 0, AttackDelivery::Melee));
    let mut attacks = 0;
    for tick in 0..8 {
        sim.step();
        attacks += sim
            .attacks_last_tick()
            .iter()
            .filter(|event| event.source == attacker)
            .count();
        let snapshot = sim
            .snapshot_units()
            .into_iter()
            .find(|unit| unit.id == attacker)
            .unwrap();
        assert_eq!(
            super::super::movement::effective_movement_speed(&snapshot),
            if tick < 6 { 15 } else { 10 } * SUBUNITS_PER_WORLD_UNIT
        );
        assert_eq!(
            super::super::status::effective_attack_cooldown_ticks(4, snapshot.status),
            if tick < 6 { 2 } else { 4 }
        );
    }
    assert_eq!(attacks, 4);
}

#[test]
fn native_speed_family_selects_each_stat_once_and_rekeys_after_representative_source_expires() {
    let mut sim = simulation(1);
    let short = BuildingAuraProfile {
        movement_percent_delta: 20,
        lifetime_ticks: 2,
        ..speed_profile()
    };
    let long = BuildingAuraProfile {
        ability: AbilityId(103),
        movement_percent_delta: 60,
        attack_speed_percent_delta: 50,
        ..speed_profile()
    };
    carrier(&mut sim, Team(0), 50, short);
    carrier(&mut sim, Team(0), 50, long);
    carrier(&mut sim, Team(0), 50, long);
    let target = sim.spawn_unit(unit(0, 90, 0, AttackDelivery::Melee));
    sim.step();
    let status = sim.unit(target).unwrap().status;
    assert_eq!(status.movement_modifier_count, 1);
    assert_eq!(status.attack_speed_modifier_count, 1);
    assert_eq!(status.movement_modifiers[0].percent_delta, 60);
    assert_eq!(status.movement_modifiers[0].expires_tick, 6);
    assert_eq!(status.attack_speed_modifiers[0].percent_delta, 100);
    assert_eq!(status.attack_speed_modifiers[0].expires_tick, 2);
    assert_eq!(status.movement_modifiers[0].id.0, short.ability.0);
    let mut restored = wire_restored(&sim, 3);
    for _ in 0..3 {
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
    let status = sim.unit(target).unwrap().status;
    assert_eq!(status.movement_modifier_count, 1);
    assert_eq!(status.attack_speed_modifier_count, 1);
    assert_eq!(status.movement_modifiers[0].percent_delta, 60);
    assert_eq!(status.attack_speed_modifiers[0].percent_delta, 50);
    assert_eq!(status.movement_modifiers[0].id.0, long.ability.0);
    // Departure removes the whole composite family, and reentry supplies only live sources.
    for simulation in [&mut sim, &mut restored] {
        simulation
            .world
            .entity_mut(entity(simulation, target))
            .insert(Position(unit(0, 91, 0, AttackDelivery::Melee).position));
    }
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
    assert_eq!(sim.unit(target).unwrap().status.movement_modifier_count, 0);
    assert_eq!(
        sim.unit(target).unwrap().status.attack_speed_modifier_count,
        0
    );
    for simulation in [&mut sim, &mut restored] {
        simulation
            .world
            .entity_mut(entity(simulation, target))
            .insert(Position(unit(0, 90, 0, AttackDelivery::Melee).position));
    }
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
    assert_eq!(
        sim.unit(target).unwrap().status.movement_modifiers[0].percent_delta,
        60
    );
    assert_eq!(
        sim.unit(target).unwrap().status.attack_speed_modifiers[0].percent_delta,
        50
    );
    while sim.tick() <= 6 {
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
    assert_eq!(sim.unit(target).unwrap().status.armor_modifier_count, 0);
}

#[test]
fn separate_speed_aura_buff_families_and_signed_payloads_keep_independent_modifiers() {
    let mut sim = simulation(1);
    carrier(&mut sim, Team(0), 50, speed_profile());
    carrier(
        &mut sim,
        Team(0),
        50,
        BuildingAuraProfile {
            ability: AbilityId(103),
            buff_rawcode: 104,
            movement_percent_delta: -20,
            attack_speed_percent_delta: -50,
            ..speed_profile()
        },
    );
    let target = sim.spawn_unit(unit(0, 90, 0, AttackDelivery::Melee));
    sim.step();
    let status = sim.unit(target).unwrap().status;
    assert_eq!(status.armor_modifier_count, 2);
    assert_eq!(status.movement_modifier_count, 2);
    assert_eq!(status.attack_speed_modifier_count, 2);
    assert_eq!(
        super::super::status::effective_attack_cooldown_ticks(6, status),
        4
    );
    let mut snapshot = sim
        .snapshot_units()
        .into_iter()
        .find(|unit| unit.id == target)
        .unwrap();
    snapshot.movement.speed_per_tick = 10 * SUBUNITS_PER_WORLD_UNIT;
    assert_eq!(
        super::super::movement::effective_movement_speed(&snapshot),
        13 * SUBUNITS_PER_WORLD_UNIT
    );
}
