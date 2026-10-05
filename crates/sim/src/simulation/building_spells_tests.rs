//! Synthetic contracts for native morph, shields and independent script control callbacks.
use super::*;
use crate::building_mechanics::{HexEffectProfile, HexFormProfile};
use crate::{ManaProfile, MapVersion};

fn simulation(workers: usize) -> Simulation {
    Simulation::new(SimulationConfig::default(), workers)
}
fn spawn(team: u8) -> UnitSpawn {
    UnitSpawn {
        team: Team(team),
        position: SimPoint::new(20 * SUBUNITS_PER_WORLD_UNIT, 0),
        health: 1000,
        attack: AttackProfile {
            damage: 0,
            range: 0,
            acquisition_range: 0,
            cooldown_ticks: 10,
            delivery: AttackDelivery::Melee,
        },
        movement: MovementProfile { speed_per_tick: 6 },
    }
}
fn combat_properties() -> UnitGameplayProperties {
    UnitGameplayProperties {
        classifications: UnitClassifications {
            combat_sapper: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn profile() -> HexEffectProfile {
    let form = |rawcode, speed_per_tick, collision_radius| HexFormProfile {
        rawcode,
        speed_per_tick,
        collision_radius,
        armor: ArmorProfile {
            armor_points: 0,
            armor_type: ArmorType::Medium,
        },
    };
    HexEffectProfile {
        map_version: crate::CASTLE_FIGHT_DEFAULT_MAP_VERSION,
        duration_ticks: 10,
        hero_duration_ticks: 4,
        ground: form(1, 2, 1),
        air: form(2, 4, 3),
        initial_reengage_ticks: 2,
        defender_restore_ticks: 11,
        defender_resume_ticks: 1,
        defender_rawcode: crate::CastleFightUnitKind::Defender.definition().rawcode,
    }
}
fn entity(sim: &Simulation, id: SimId) -> Entity {
    sim.world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}
fn state(sim: &Simulation, id: SimId) -> &BuildingSpellTargetState {
    sim.world
        .iter_entities()
        .find_map(|e| {
            e.get::<BuildingSpellTargetState>()
                .filter(|s| s.target == id)
        })
        .unwrap()
}
fn apply(sim: &mut Simulation, id: SimId, profile: HexEffectProfile) -> bool {
    let mut target = sim
        .snapshot_units()
        .into_iter()
        .find(|u| u.id == id)
        .unwrap();
    let applied = sim.resolve_building_hex(&mut target, profile, SimId(999), 0);
    sim.world
        .entity_mut(target.entity)
        .get_mut::<Health>()
        .unwrap()
        .current = target.health;
    applied
}
fn advance_to(sim: &mut Simulation, tick: u64) {
    sim.next_tick = tick;
    sim.advance_building_spell_controls();
}
fn defend() -> PassiveUnitEffects {
    PassiveUnitEffects::single(PassiveUnitEffect::Defend(DefendEffectProfile {
        ability: AbilityId(1),
        ranged_damage_taken_per_10k: 5000,
        spell_damage_taken_per_10k: 6000,
        deflect_chance_per_10k: 0,
        deflected_pierce_damage_taken_per_10k: 5000,
        activation_delay_ticks: 0,
    }))
}
fn defender_properties() -> UnitGameplayProperties {
    let source = crate::CastleFightUnitKind::Defender.definition();
    UnitGameplayProperties {
        content: Some(ContentIdentity {
            map_version: source.map_version,
            rawcode: source.rawcode,
            name: source.name,
        }),
        passive_effects: defend(),
        armor: ArmorProfile {
            armor_points: 9,
            armor_type: ArmorType::Large,
        },
        collision_radius: Some(CollisionRadius(7)),
        ..combat_properties()
    }
}
fn caster(
    sim: &mut Simulation,
    x: i32,
    effect: AbilityEffect,
    mana: ManaProfile,
    cost: i32,
) -> SimId {
    sim.spawn_building(BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(x, 10, 1, 1),
        health: 10_000,
        production: None,
        attack: None,
        spellcasting: Some(SpellcastingProfile {
            mana,
            ability: AutomaticAbilityProfile {
                id: AbilityId(9),
                mana_cost: cost,
                cooldown_ticks: 1000,
                range: 0,
                target_policy: AbilityTargetPolicy::RandomEnemyUnitGlobal,
                effect,
            },
        }),
    })
}

#[test]
fn shields_intercept_after_spending_and_heal_only_the_first_greater_charge() {
    let mut sim = simulation(1);
    let target = sim.spawn_unit_with_properties(spawn(1), combat_properties());
    sim.world
        .entity_mut(entity(&sim, target))
        .get_mut::<Health>()
        .unwrap()
        .current = 400;
    assert!(sim.set_negative_building_shield(target, 2, None));
    let mana = ManaProfile::per_second(5, 5, 0);
    let first = caster(
        &mut sim,
        0,
        AbilityEffect::Hex { profile: profile() },
        mana,
        5,
    );
    let second = caster(
        &mut sim,
        2,
        AbilityEffect::Hex { profile: profile() },
        mana,
        5,
    );
    let third = caster(
        &mut sim,
        4,
        AbilityEffect::Hex { profile: profile() },
        mana,
        5,
    );
    let metrics = sim.step();
    assert_eq!(metrics.ability_casts, 3);
    for id in [first, second, third] {
        assert_eq!(sim.building(id).unwrap().mana_current, Some(0));
    }
    assert_eq!(sim.unit(target).unwrap().health, 1000);
    assert_eq!(state(&sim, target).shield_level, 0);
    assert!(state(&sim, target).hex.is_some());
    assert_eq!(
        state(&sim, target).callbacks.len(),
        1,
        "blocked casts do not reengage"
    );
    assert_eq!(
        sim.last_building_spell_visuals
            .iter()
            .filter(|e| e.kind == BuildingSpellVisualKind::ShieldConsumed)
            .count(),
        2
    );
    // A one-charge shield never heals, including when reapplied over an existing morph.
    sim.world
        .entity_mut(entity(&sim, target))
        .get_mut::<Health>()
        .unwrap()
        .current = 300;
    sim.set_negative_building_shield(target, 1, None);
    assert!(!apply(&mut sim, target, profile()));
    assert_eq!(sim.unit(target).unwrap().health, 300);
}

#[test]
fn shield_expiry_dispel_and_marker_precedence_are_independent_of_native_immunity() {
    let mut sim = simulation(1);
    let target = sim.spawn_unit_with_properties(
        spawn(1),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                spell_immune: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    sim.set_negative_building_markers(target, true, false);
    sim.set_negative_building_shield(target, 2, Some(4));
    assert!(!apply(&mut sim, target, profile()));
    assert_eq!(
        state(&sim, target).shield_level,
        1,
        "A09L is consumed before A070"
    );
    assert!(state(&sim, target).callbacks.is_empty());
    advance_to(&mut sim, 4);
    assert_eq!(state(&sim, target).shield_level, 0);
    assert!(!apply(&mut sim, target, profile()));
    assert!(
        state(&sim, target).callbacks.is_empty(),
        "A070 blocks without consumption"
    );
    sim.set_negative_building_markers(target, false, false);
    assert!(!apply(&mut sim, target, profile()));
    assert_eq!(
        state(&sim, target).callbacks.len(),
        1,
        "native immune failure still schedules script callback"
    );
    sim.set_negative_building_shield(target, 1, None);
    assert!(sim.dispel_negative_building_shield(target));
    assert_eq!(state(&sim, target).shield_level, 0);
    assert!(!sim.dispel_negative_building_shield(target));
}

#[test]
fn native_hex_projects_ground_air_and_hero_forms_without_destroying_baselines() {
    for (movement_class, hero, mechanical) in [
        (MovementClass::Ground, false, true),
        (MovementClass::Air, true, false),
    ] {
        let mut sim = simulation(1);
        let properties = UnitGameplayProperties {
            movement_class,
            mechanical,
            build_time_ticks: mechanical.then_some(30),
            classifications: UnitClassifications {
                hero,
                ..Default::default()
            },
            ..defender_properties()
        };
        let target = sim.spawn_unit_with_properties(spawn(1), properties);
        assert!(apply(&mut sim, target, profile()));
        let projected = sim.snapshot_units()[0];
        let expected = if movement_class == MovementClass::Air {
            profile().air
        } else {
            profile().ground
        };
        assert_eq!(projected.armor, expected.armor);
        assert_eq!(projected.movement.speed_per_tick, expected.speed_per_tick);
        assert_eq!(
            projected.collision_radius_override,
            Some(expected.collision_radius)
        );
        assert!(
            projected.attacks_disabled
                && projected.abilities_disabled
                && projected.orders_suspended
        );
        assert!(projected.passive_effects.is_empty());
        assert_eq!(
            projected.status.stunned_until_tick, 0,
            "native Hex is not generic stun"
        );
        assert_eq!(sim.unit(target).unwrap().content, properties.content);
        assert_eq!(sim.unit(target).unwrap().movement_class, movement_class);
        advance_to(&mut sim, 2);
        let projected = sim.snapshot_units()[0];
        assert!(
            !projected.orders_suspended,
            "reengagement does not wait for Hex expiry"
        );
        assert!(projected.attacks_disabled && projected.abilities_disabled);
        let expiry = if hero { 4 } else { 10 };
        advance_to(&mut sim, expiry - 1);
        assert!(state(&sim, target).hex.is_some());
        advance_to(&mut sim, expiry);
        let restored = sim.snapshot_units()[0];
        assert!(!restored.attacks_disabled && !restored.abilities_disabled);
        assert_eq!(restored.armor, properties.armor);
        assert_eq!(
            restored.movement.speed_per_tick,
            spawn(1).movement.speed_per_tick
        );
        assert_eq!(restored.collision_radius_override, Some(7));
        assert_eq!(
            sim.world
                .entity(entity(&sim, target))
                .get::<PassiveUnitEffects>(),
            Some(&defend())
        );
    }
}

#[test]
fn early_dispel_does_not_cancel_defender_callbacks_and_failed_hex_is_not_undefend() {
    let mut sim = simulation(1);
    let target = sim.spawn_unit_with_properties(spawn(1), defender_properties());
    assert!(apply(&mut sim, target, profile()));
    assert!(sim.dispel_building_hex(target));
    assert!(!sim.snapshot_units()[0].abilities_disabled);
    advance_to(&mut sim, 2);
    assert!(state(&sim, target).defend_disabled);
    advance_to(&mut sim, 12);
    assert!(sim.unit(target).unwrap().active_defend_ability.is_none());
    advance_to(&mut sim, 13);
    assert!(!state(&sim, target).defend_disabled);
    assert!(state(&sim, target).orders_suspended);
    assert!(sim.unit(target).unwrap().active_defend_ability.is_some());
    advance_to(&mut sim, 14);
    assert!(!state(&sim, target).orders_suspended);
    // The source schedules Defender's branch even if the native cast is rejected.
    sim.world
        .entity_mut(entity(&sim, target))
        .insert(UnitClassifications {
            spell_immune: true,
            ..Default::default()
        });
    assert!(!apply(&mut sim, target, profile()));
    advance_to(&mut sim, 16);
    assert!(
        !state(&sim, target).defend_disabled,
        "attack order isn't an undefend toggle"
    );
    assert!(sim.unit(target).unwrap().active_defend_ability.is_some());
}

#[test]
fn hex_selector_requires_combat_sapper_and_excludes_avul_without_flattening_native_failure() {
    for (classifications, selected) in [
        (UnitClassifications::default(), false),
        (
            UnitClassifications {
                combat_sapper: true,
                invulnerable: true,
                ..Default::default()
            },
            false,
        ),
        (
            UnitClassifications {
                combat_sapper: true,
                spell_immune: true,
                ..Default::default()
            },
            true,
        ),
        (
            UnitClassifications {
                combat_sapper: true,
                hero: true,
                ..Default::default()
            },
            true,
        ),
    ] {
        let mut sim = simulation(1);
        let target = sim.spawn_unit_with_properties(
            spawn(1),
            UnitGameplayProperties {
                classifications,
                ..Default::default()
            },
        );
        let source = caster(
            &mut sim,
            0,
            AbilityEffect::Hex { profile: profile() },
            ManaProfile::per_second(1, 1, 0),
            1,
        );
        assert_eq!(sim.step().ability_casts, usize::from(selected));
        assert_eq!(
            sim.building(source).unwrap().mana_current,
            Some(i32::from(!selected))
        );
        if selected {
            assert_eq!(state(&sim, target).callbacks.len(), 1);
            assert_eq!(
                state(&sim, target).hex.is_some(),
                !classifications.spell_immune
            );
        } else {
            assert!(!sim.world.iter_entities().any(|entity| {
                entity
                    .get::<BuildingSpellTargetState>()
                    .is_some_and(|state| state.target == target)
            }));
        }
        // The same predicate used by live commitment must reject a class change
        // even if an earlier evaluation observed a valid combat sapper.
        let mut snapshot = sim
            .snapshot_units()
            .into_iter()
            .find(|unit| unit.id == target)
            .unwrap();
        snapshot.classifications = UnitClassifications {
            combat_sapper: true,
            ..Default::default()
        };
        assert!(sim.hex_trigger_eligible(&snapshot, profile().map_version));
        snapshot.classifications.invulnerable = true;
        assert!(!sim.hex_trigger_eligible(&snapshot, profile().map_version));
        snapshot.classifications.invulnerable = false;
        snapshot.classifications.combat_sapper = false;
        assert!(!sim.hex_trigger_eligible(&snapshot, profile().map_version));
    }
}

#[test]
fn hex_selection_excludes_hidden_markers_but_not_air_mechanical_or_native_immunity() {
    let mut sim = simulation(1);
    let target = sim.spawn_unit_with_properties(
        spawn(1),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            mechanical: true,
            build_time_ticks: Some(30),
            classifications: UnitClassifications {
                combat_sapper: true,
                spell_immune: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let ally = sim.spawn_unit_with_properties(spawn(0), combat_properties());
    sim.set_negative_building_markers(target, false, true);
    let source = caster(
        &mut sim,
        0,
        AbilityEffect::Hex { profile: profile() },
        ManaProfile::per_second(5, 5, 0),
        5,
    );
    assert_eq!(sim.step().ability_casts, 0);
    assert_eq!(sim.building(source).unwrap().mana_current, Some(5));
    sim.set_negative_building_markers(target, false, false);
    assert_eq!(sim.step().ability_casts, 1);
    assert!(state(&sim, target).hex.is_none());
    assert_eq!(state(&sim, target).callbacks.len(), 1);
    assert!(!sim.world.iter_entities().any(|e| {
        e.get::<BuildingSpellTargetState>()
            .is_some_and(|s| s.target == ally)
    }));
}

#[test]
fn hex_suppresses_pending_primary_and_additional_casts_and_restores_after_dispel() {
    let mut sim = simulation(1);
    caster(
        &mut sim,
        0,
        AbilityEffect::Hex { profile: profile() },
        ManaProfile::per_second(1, 1, 0),
        1,
    );
    let spell = SpellcastingProfile {
        mana: ManaProfile::per_second(5, 5, 0),
        ability: AutomaticAbilityProfile {
            id: AbilityId(20),
            mana_cost: 1,
            cooldown_ticks: 1,
            range: 0,
            target_policy: AbilityTargetPolicy::RandomEnemyUnitGlobal,
            effect: AbilityEffect::Damage { amount: 1 },
        },
    };
    let target =
        sim.spawn_unit_with_properties_and_spellcasting(spawn(1), defender_properties(), spell);
    sim.spawn_unit(spawn(0));
    let mut additional = spell;
    additional.ability.id = AbilityId(21);
    sim.configure_additional_automatic_abilities(target, &[additional.ability])
        .unwrap();
    assert_eq!(
        sim.step().ability_casts,
        1,
        "canonical earlier Hex cancels already queued casts"
    );
    assert_eq!(sim.unit(target).unwrap().mana_current, Some(5));
    sim.dispel_building_hex(target);
    advance_to(&mut sim, 2);
    assert_eq!(sim.step().ability_casts, 2);
    assert_eq!(sim.unit(target).unwrap().mana_current, Some(3));
}

#[test]
fn exact_per_second_mana_survives_wire_restore_without_changing_legacy_profiles() {
    let content = crate::castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    for rate in [10_000, 35_000] {
        let mut sim = simulation(1);
        let target = sim.spawn_unit(spawn(1));
        let id = caster(
            &mut sim,
            0,
            AbilityEffect::Damage { amount: 0 },
            ManaProfile::per_second(100, 0, rate),
            100,
        );
        for _ in 0..17 {
            sim.step();
        }
        let wire = sim.capture_snapshot().encode_wire().unwrap();
        let snapshot = SimulationSnapshot::decode_wire(&wire, content).unwrap();
        let mut restored = simulation(3);
        restored.restore_snapshot(&snapshot).unwrap();
        assert_eq!(sim.checksum(), restored.checksum());
        for _ in 17..300 {
            sim.step();
            restored.step();
            assert_eq!(sim.checksum(), restored.checksum());
        }
        assert_eq!(
            sim.building(id).unwrap().mana_current,
            Some((rate / 1000) as i32)
        );
        assert!(sim.unit(target).is_some());
    }
}

#[test]
fn pending_hex_shield_and_defender_state_restores_across_workers() {
    let mut sim = simulation(1);
    let target = sim.spawn_unit_with_properties(spawn(1), defender_properties());
    apply(&mut sim, target, profile());
    sim.set_negative_building_shield(target, 2, Some(9));
    sim.step();
    let content = crate::castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), content)
            .unwrap();
    let mut restored = simulation(4);
    restored.restore_snapshot(&snapshot).unwrap();
    assert_eq!(sim.checksum(), restored.checksum());
    for _ in 0..20 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
        assert_eq!(sim.unit(target), restored.unit(target));
    }
}
