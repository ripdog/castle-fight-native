use super::*;
use crate::components::{EvasionEffectProfile, SpellResistanceEffectProfile};

mod cleanse;
mod homing;

fn simulation(workers: usize) -> Simulation {
    Simulation::new(
        SimulationConfig {
            navigation_max: NavCell::new(20_000, 64),
            ..SimulationConfig::default()
        },
        workers,
    )
}
fn entity(sim: &Simulation, id: SimId) -> Entity {
    sim.world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}
fn victim(sim: &mut Simulation, x: i32, properties: UnitGameplayProperties) -> SimId {
    sim.spawn_unit_with_properties(
        UnitSpawn {
            team: Team(1),
            position: SimPoint::new(x, SUBUNITS_PER_WORLD_UNIT),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1000,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        },
        properties,
    )
}
fn tower(
    sim: &mut Simulation,
    kind: crate::CastleFightTowerKind,
    attack: Option<AttackProfile>,
) -> SimId {
    let definition = kind.definition();
    let mut spawn = definition.spawn(
        Team(0),
        BuildingFootprint::new(sim.building_count() as i32 * 4, 0, 2, 2),
    );
    spawn.attack = attack;
    sim.spawn_building_with_properties(spawn, definition.gameplay_properties())
}
fn status(sim: &mut Simulation, id: SimId) {
    let e = entity(sim, id);
    let tick = sim.next_tick;
    let mut s = sim.world.get_mut::<StatusState>(e).unwrap();
    apply_timed_armor_modifier(
        &mut s,
        TimedArmorModifier {
            id: ModifierId(900),
            armor_bonus_per_100: 100,
            expires_tick: tick + 1000,
            ..Default::default()
        },
    );
    apply_timed_movement_modifier(&mut s, ModifierId(901), -20, tick + 1000);
    s.ability_retreat_start_tick = tick + 20;
    s.ability_retreat_end_tick = tick + 30;
}
fn restore(sim: &Simulation, workers: usize) -> Simulation {
    let mut restored = simulation(workers);
    restored.restore_snapshot(&sim.capture_snapshot()).unwrap();
    restored
}

#[test]
fn barrage_capacity_range_mask_damage_and_independent_persistent_missiles() {
    let profile = barrage_for_version(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let mut sim = simulation(1);
    let attack = AttackProfile {
        damage: 17,
        range: profile.range + 100 * SUBUNITS_PER_WORLD_UNIT,
        acquisition_range: profile.range + 100 * SUBUNITS_PER_WORLD_UNIT,
        cooldown_ticks: 1000,
        delivery: AttackDelivery::RangedGuaranteedHit {
            speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
        },
    };
    let source = tower(
        &mut sim,
        crate::CastleFightTowerKind::ArcaneTower,
        Some(attack),
    );
    // The ordinary target is deliberately outside the Barrage radius.
    let primary = victim(
        &mut sim,
        profile.range + 20 * SUBUNITS_PER_WORLD_UNIT,
        UnitGameplayProperties::default(),
    );
    let targets: Vec<_> = (0..usize::from(profile.maximum_targets) + 2)
        .map(|i| {
            victim(
                &mut sim,
                (50 + i as i32 * 10) * SUBUNITS_PER_WORLD_UNIT,
                UnitGameplayProperties {
                    movement_class: if i % 2 == 0 {
                        MovementClass::Air
                    } else {
                        MovementClass::Ground
                    },
                    ..Default::default()
                },
            )
        })
        .collect();
    let source_entity = entity(&sim, source);
    sim.step();
    sim.world
        .get_mut::<TargetState>(source_entity)
        .unwrap()
        .current = Some(primary);
    sim.step();
    let bolts: Vec<_> = sim
        .projectiles()
        .into_iter()
        .filter(|p| matches!(p.kind, ProjectileViewKind::NativeCarrierBolt { .. }))
        .collect();
    assert_eq!(bolts.len(), profile.additional_targets());
    assert!(
        bolts
            .iter()
            .all(|p| p.source == source && p.impact_tick > p.launch_tick)
    );
    assert!(sim.projectiles().iter().any(
        |p| matches!(p.kind, ProjectileViewKind::GuaranteedHit { target } if target == primary)
    ));
    let mut restored = restore(&sim, 4);
    // No new arrows after removal; every already launched arrow survives source removal.
    assert!(sim.remove_building(source));
    assert!(restored.remove_building(source));
    for _ in 0..1100 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    let damaged = targets
        .iter()
        .filter(|id| sim.unit(**id).unwrap().health < 10_000)
        .count();
    assert_eq!(damaged, profile.additional_targets());
    for id in targets.iter().take(profile.additional_targets()) {
        assert_eq!(sim.unit(*id).unwrap().health, 10_000 - attack.damage);
    }
}

#[test]
fn carrier_construct_finish_idempotence_random_unit_selection_cooldown_and_removal() {
    let profile = carrier_for_version(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let mut sim = simulation(1);
    let source = tower(&mut sim, crate::CastleFightTowerKind::ObeliskOfLight, None);
    sim.start_native_carrier(entity(&sim, source));
    let carriers = sim
        .world
        .iter_entities()
        .filter(|e| {
            matches!(
                e.get::<NativeCarrierState>(),
                Some(NativeCarrierState::Carrier { .. })
            )
        })
        .count();
    assert_eq!(carriers, 1);
    let targets: Vec<_> = [1000, 2000, 3000]
        .into_iter()
        .map(|x| {
            victim(
                &mut sim,
                x * SUBUNITS_PER_WORLD_UNIT,
                UnitGameplayProperties::default(),
            )
        })
        .collect();
    sim.step();
    let bolts = sim.projectiles();
    assert_eq!(bolts.len(), 1);
    let target = match bolts[0].kind {
        ProjectileViewKind::NativeCarrierBolt { target, ability } => {
            assert_eq!(ability, profile.ability);
            target
        }
        _ => panic!("carrier missile"),
    };
    assert!(targets.contains(&target));
    let mut restored = restore(&sim, 4);
    for _ in 1..profile.cooldown_ticks {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert_eq!(
        targets
            .iter()
            .filter(|id| sim.unit(**id).unwrap().health < 10_000)
            .count(),
        1
    );
    sim.step();
    restored.step();
    assert_eq!(sim.projectiles().len(), 1);
    assert_eq!(sim.checksum(), restored.checksum());
    assert!(sim.remove_building(source));
    assert!(restored.remove_building(source));
    for _ in 0..profile.cooldown_ticks + 20 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert!(sim.projectiles().is_empty());
}

#[test]
fn cleanse_requires_positive_damage_and_live_source_ability_not_building_identity() {
    let profile = carrier_for_version(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    for (immune, zero_damage, remove_source) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let mut sim = simulation(1);
        let source = tower(&mut sim, crate::CastleFightTowerKind::ObeliskOfLight, None);
        let target = victim(
            &mut sim,
            2000 * SUBUNITS_PER_WORLD_UNIT,
            UnitGameplayProperties {
                classifications: UnitClassifications {
                    spell_immune: immune,
                    ..Default::default()
                },
                passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::SpellResistance(
                    SpellResistanceEffectProfile {
                        ability: AbilityId(800),
                        damage_taken_per_10k: if zero_damage { 0 } else { 10_000 },
                    },
                )),
                ..Default::default()
            },
        );
        status(&mut sim, target);
        if immune {
            // Native auto-targeting rejects immunity; an in-flight target may acquire immunity.
            sim.world
                .get_mut::<UnitClassifications>(entity(&sim, target))
                .unwrap()
                .spell_immune = false;
        }
        sim.step();
        assert_eq!(sim.projectiles().len(), 1);
        if immune {
            sim.world
                .get_mut::<UnitClassifications>(entity(&sim, target))
                .unwrap()
                .spell_immune = true;
        }
        if remove_source {
            assert!(sim.remove_building(source));
        }
        let mut restored = restore(&sim, 4);
        for _ in 0..15 {
            sim.step();
            restored.step();
            assert_eq!(sim.checksum(), restored.checksum());
        }
        let e = entity(&sim, target);
        let s = sim.world.get::<StatusState>(e).unwrap();
        assert_eq!(
            s.armor_modifier_count == 0,
            !immune && !zero_damage && !remove_source
        );
        assert_eq!(
            s.movement_modifier_count == 0,
            !immune && !zero_damage && !remove_source
        );
        assert!(
            s.ability_retreat_end_tick > 0,
            "cleanse cannot erase script AI state"
        );
        assert_eq!(
            sim.unit(target).unwrap().health,
            10_000
                - if immune || zero_damage {
                    0
                } else {
                    profile.initial_damage
                }
        );
    }
}

#[test]
fn barrage_arrows_are_not_spell_damage_and_can_miss_independently() {
    let mut sim = simulation(1);
    let attack = AttackProfile {
        delivery: AttackDelivery::RangedGuaranteedHit {
            speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
        },
        damage: 10,
        range: 300 * SUBUNITS_PER_WORLD_UNIT,
        acquisition_range: 300 * SUBUNITS_PER_WORLD_UNIT,
        cooldown_ticks: 1000,
    };
    tower(
        &mut sim,
        crate::CastleFightTowerKind::ArcaneTower,
        Some(attack),
    );
    let primary = victim(
        &mut sim,
        30 * SUBUNITS_PER_WORLD_UNIT,
        UnitGameplayProperties::default(),
    );
    let immune = victim(
        &mut sim,
        40 * SUBUNITS_PER_WORLD_UNIT,
        UnitGameplayProperties {
            classifications: UnitClassifications {
                spell_immune: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let evader = victim(
        &mut sim,
        50 * SUBUNITS_PER_WORLD_UNIT,
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Evasion(
                EvasionEffectProfile {
                    ability: AbilityId(20),
                    chance_per_10k: 10_000,
                },
            )),
            ..Default::default()
        },
    );
    for _ in 0..60 {
        sim.step();
    }
    assert_eq!(sim.unit(primary).unwrap().health, 9990);
    assert_eq!(sim.unit(immune).unwrap().health, 9990);
    assert_eq!(sim.unit(evader).unwrap().health, 10_000);
}

#[test]
fn wire_continuation_preserves_bolt_carrier_cooldown_and_regeneration_remainder() {
    let mut sim = simulation(1);
    tower(&mut sim, crate::CastleFightTowerKind::ArcaneTower, None);
    tower(&mut sim, crate::CastleFightTowerKind::ObeliskOfLight, None);
    victim(
        &mut sim,
        2500 * SUBUNITS_PER_WORLD_UNIT,
        UnitGameplayProperties::default(),
    );
    for _ in 0..4 {
        sim.step();
    }
    let content = crate::castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let encoded = sim.capture_snapshot().encode_wire().unwrap();
    let decoded = SimulationSnapshot::decode_wire(&encoded, content).unwrap();
    let mut restored = simulation(4);
    restored.restore_snapshot(&decoded).unwrap();
    for _ in 0..200 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
}

#[test]
fn permanent_ability_removal_is_exact_and_native_buff_identity_is_not_a_grant() {
    let profile = carrier_for_version(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let mut sim = simulation(1);
    tower(&mut sim, crate::CastleFightTowerKind::ObeliskOfLight, None);
    let removable = AbilityId(u32::from_be_bytes(*b"A08M"));
    assert!(profile.removed_persistent_abilities.contains(&removable));
    let target = victim(
        &mut sim,
        2000 * SUBUNITS_PER_WORLD_UNIT,
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::from_slice(&[
                PassiveUnitEffect::Evasion(EvasionEffectProfile {
                    ability: removable,
                    chance_per_10k: 10_000,
                }),
                PassiveUnitEffect::SpellResistance(SpellResistanceEffectProfile {
                    ability: AbilityId(810),
                    damage_taken_per_10k: 10_000,
                }),
            ]),
            ..Default::default()
        },
    );
    let e = entity(&sim, target);
    sim.world.get_mut::<Health>(e).unwrap().max += profile.holy_health_bonus;
    let mut s = sim.world.get_mut::<StatusState>(e).unwrap();
    s.permanent_holy_health_bonus = true;
    s.stunned_until_tick = 1000;
    s.secondary_resurrection_due_tick = 1000;
    // Equal lifetimes and equal signs must not conflate buffs with permanent abilities.
    for id in [
        ModifierId(profile.buff.0),
        ModifierId(u32::from_be_bytes(*b"A08L")),
        ModifierId(811),
    ] {
        apply_timed_armor_modifier(
            &mut s,
            TimedArmorModifier {
                id,
                armor_bonus_per_100: 100,
                expires_tick: u64::MAX,
                ..Default::default()
            },
        );
    }
    apply_timed_movement_modifier(&mut s, ModifierId(812), -20, u64::MAX);
    apply_timed_attack_speed_modifier(&mut s, ModifierId(813), 20, 1000);
    apply_timed_damage_over_time(&mut s, ModifierId(814), 0, 20, 0, 1000);
    sim.step();
    let mut restored = restore(&sim, 4);
    for _ in 0..15 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    let s = sim.unit(target).unwrap().status;
    assert_eq!(s.stunned_until_tick, 0);
    assert_eq!(s.secondary_resurrection_due_tick, 1000);
    assert!(!s.permanent_holy_health_bonus);
    assert_eq!(sim.unit(target).unwrap().health_max, 10_000);
    assert_eq!(s.armor_modifier_count, 1);
    assert_eq!(s.armor_modifiers[0].id, ModifierId(811));
    assert_eq!(s.movement_modifier_count, 1);
    assert_eq!(s.movement_modifiers[0].id, ModifierId(812));
    assert_eq!(s.attack_speed_modifier_count, 0);
    assert_eq!(s.damage_over_time_count, 0);
    let passives = sim.world.get::<PassiveUnitEffects>(e).unwrap();
    assert_eq!(passives.iter().count(), 1);
    assert!(
        matches!(passives.iter().next(), Some(PassiveUnitEffect::SpellResistance(p)) if p.ability == AbilityId(810))
    );
}

#[test]
fn carrier_rejects_allies_structures_immunity_and_invisibility_until_revealed() {
    let mut sim = simulation(1);
    tower(&mut sim, crate::CastleFightTowerKind::ObeliskOfLight, None);
    let ally = victim(
        &mut sim,
        1000 * SUBUNITS_PER_WORLD_UNIT,
        UnitGameplayProperties::default(),
    );
    sim.world.entity_mut(entity(&sim, ally)).insert(Team(0));
    victim(
        &mut sim,
        1100 * SUBUNITS_PER_WORLD_UNIT,
        UnitGameplayProperties {
            classifications: UnitClassifications {
                spell_immune: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let hidden = victim(
        &mut sim,
        1200 * SUBUNITS_PER_WORLD_UNIT,
        UnitGameplayProperties {
            classifications: UnitClassifications {
                invisible: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let mut building = crate::CastleFightTowerKind::ObeliskOfLight
        .definition()
        .spawn(Team(1), BuildingFootprint::new(16, 0, 2, 2));
    building.attack = None;
    sim.spawn_building(building);
    assert_eq!(sim.step().projectiles_launched, 0);
    assert!(sim.projectiles().is_empty());
    let e = entity(&sim, hidden);
    let mut status = sim.world.get_mut::<StatusState>(e).unwrap();
    apply_timed_armor_modifier(
        &mut status,
        TimedArmorModifier {
            id: ModifierId(815),
            revealed_to: Some(Team(0)),
            expires_tick: 1000,
            ..Default::default()
        },
    );
    assert_eq!(sim.step().projectiles_launched, 1);
    assert!(
        matches!(sim.projectiles()[0].kind, ProjectileViewKind::NativeCarrierBolt { target, .. } if target == hidden)
    );
    // Once airborne, native missiles do not re-run launch visibility eligibility.
    let mut restored = restore(&sim, 4);
    for _ in 0..10 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert!(sim.unit(hidden).unwrap().health < 10_000);
}

#[test]
fn construction_does_not_create_carrier_until_completion_and_deactivation_stops_it() {
    let mut sim = simulation(1);
    let definition = crate::CastleFightTowerKind::ObeliskOfLight.definition();
    let mut properties = definition.gameplay_properties();
    properties.construction_time_ticks = Some(3);
    let source = sim
        .try_start_building_construction(
            PlayerId(0),
            definition.spawn(Team(0), BuildingFootprint::new(0, 0, 2, 2)),
            properties,
        )
        .unwrap();
    victim(
        &mut sim,
        2000 * SUBUNITS_PER_WORLD_UNIT,
        UnitGameplayProperties::default(),
    );
    for _ in 0..3 {
        assert_eq!(sim.step().projectiles_launched, 0);
    }
    assert_eq!(sim.step().projectiles_launched, 1);
    let mut restored = restore(&sim, 4);
    sim.deactivate_building_entity(entity(&sim, source));
    restored.deactivate_building_entity(entity(&restored, source));
    for _ in 0..120 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert!(sim.projectiles().is_empty());
    assert!(sim.world.iter_entities().all(|e| !matches!(
        e.get::<NativeCarrierState>(),
        Some(NativeCarrierState::Carrier { .. })
    )));
}

#[test]
fn positive_native_barrage_count_is_offset_and_damage_fields_are_not_a_damage_budget() {
    let mut profile = barrage_for_version(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    profile.maximum_targets = 7;
    profile.damage_per_target = 0;
    profile.maximum_total_damage = 1;
    assert_eq!(
        profile.additional_targets(),
        usize::from(profile.maximum_targets) + 1
    );
    profile.maximum_targets = 1;
    profile.maximum_total_damage = 2;
    assert_eq!(profile.additional_targets(), 1);
}
