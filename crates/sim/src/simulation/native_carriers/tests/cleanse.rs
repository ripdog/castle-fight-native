use super::*;
use crate::simulation::building_spells::{
    BuildingSpellTargetState, ControlAction, refresh_building_spell_controls,
};

fn control(simulation: &Simulation, target: SimId) -> &BuildingSpellTargetState {
    simulation
        .world
        .iter_entities()
        .find_map(|entity| {
            entity
                .get::<BuildingSpellTargetState>()
                .filter(|state| state.target == target)
        })
        .unwrap()
}

fn prepare(simulation: &mut Simulation) -> (SimId, SimId, PassiveUnitEffects, PassiveUnitEffects) {
    let profile = carrier_for_version(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let source = tower(
        simulation,
        crate::CastleFightTowerKind::ObeliskOfLight,
        None,
    );
    let removed = PassiveUnitEffect::SpellResistance(SpellResistanceEffectProfile {
        ability: profile.removed_persistent_abilities[0],
        damage_taken_per_10k: 8000,
    });
    let unrelated = PassiveUnitEffect::Evasion(EvasionEffectProfile {
        ability: AbilityId(9876),
        chance_per_10k: 1000,
    });
    let defend = PassiveUnitEffect::Defend(DefendEffectProfile {
        ability: AbilityId(9877),
        ranged_damage_taken_per_10k: 5000,
        spell_damage_taken_per_10k: 5000,
        deflect_chance_per_10k: 0,
        deflected_pierce_damage_taken_per_10k: 5000,
        activation_delay_ticks: 0,
    });
    let baseline = PassiveUnitEffects::from_slice(&[removed, unrelated, defend]);
    let retained = PassiveUnitEffects::from_slice(&[unrelated, defend]);
    let definition = crate::CastleFightUnitKind::Defender.definition();
    let target = victim(
        simulation,
        200 * SUBUNITS_PER_WORLD_UNIT,
        UnitGameplayProperties {
            content: definition.gameplay_properties().content,
            classifications: UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
            passive_effects: baseline,
            ..Default::default()
        },
    );
    status(simulation, target);
    let AbilityEffect::Hex { mut profile } =
        crate::building_mechanics::city_spellcasting_for_version(MapVersion::CASTLE_FIGHT_9_27)
            .ability
            .effect
    else {
        unreachable!()
    };
    // Synthetic body/timing isolate removal from source collision footprints and expiry.
    profile.duration_ticks = 1000;
    profile.hero_duration_ticks = 1000;
    profile.initial_reengage_ticks = 100;
    profile.defender_restore_ticks = 100;
    profile.defender_resume_ticks = 1;
    profile.ground.collision_radius = 1;
    profile.ground.speed_per_tick = 0;
    let mut projected = simulation
        .snapshot_units()
        .into_iter()
        .find(|unit| unit.id == target)
        .unwrap();
    assert!(simulation.resolve_building_hex(&mut projected, profile, SimId(9000), 0));
    assert!(projected.passive_effects.is_empty());
    // A shield applied after morph must not retroactively block that already committed Hex.
    assert!(simulation.set_negative_building_shield(target, 2, Some(1000)));
    let control_entity = simulation
        .world
        .iter_entities()
        .find_map(|entity| {
            entity
                .get::<BuildingSpellTargetState>()
                .filter(|state| state.target == target)
                .map(|_| entity.id())
        })
        .unwrap();
    simulation
        .world
        .get_mut::<BuildingSpellTargetState>(control_entity)
        .unwrap()
        .overheat_level = 2;
    refresh_building_spell_controls(&mut simulation.world, simulation.next_tick);
    (source, target, baseline, retained)
}

#[test]
fn positive_cleanse_restores_baseline_and_removes_shield_without_erasing_overheat_or_callbacks() {
    let mut original = simulation(1);
    let (_, target, _, retained) = prepare(&mut original);
    original.step();
    assert_eq!(original.projectiles().len(), 1);
    let callbacks = control(&original, target).callbacks.clone();
    let content = crate::castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let wire = original.capture_snapshot().encode_wire().unwrap();
    let snapshot = SimulationSnapshot::decode_wire(&wire, content).unwrap();
    let mut restored = simulation(4);
    restored.restore_snapshot(&snapshot).unwrap();
    let mut restore_visual = false;
    for _ in 0..40 {
        assert_eq!(original.step().checksum, restored.step().checksum);
        restore_visual |= original.last_building_spell_visuals.iter().any(|event| {
            event.target == target && event.kind == BuildingSpellVisualKind::HexRestore
        });
        if control(&original, target).hex.is_none() {
            break;
        }
    }
    assert!(restore_visual);
    let state = control(&original, target);
    assert!(state.hex.is_none());
    assert_eq!(state.shield_level, 0);
    assert_eq!(state.shield_expires_tick, None);
    assert_eq!(state.overheat_level, 2, "A09C is not in the removal list");
    assert_eq!(state.callbacks, callbacks);
    assert!(state.defend_disabled && state.orders_suspended);
    let target_entity = entity(&original, target);
    assert_eq!(
        *original
            .world
            .get::<PassiveUnitEffects>(target_entity)
            .unwrap(),
        retained
    );
    let projected = original
        .snapshot_units()
        .into_iter()
        .find(|unit| unit.id == target)
        .unwrap();
    assert!(!projected.abilities_disabled && !projected.attacks_disabled);
    assert!(projected.orders_suspended);
    assert_eq!(projected.passive_effects, retained.without_defend());
    assert_eq!(projected.status.movement_modifier_count, 0);
    assert_eq!(projected.status.armor_modifier_count, 0);
    assert_eq!(projected.status.ability_retreat_start_tick, 20);
    assert_eq!(projected.status.ability_retreat_end_tick, 30);
    assert!(
        projected.status.attack_speed_modifiers
            [..usize::from(projected.status.attack_speed_modifier_count)]
            .iter()
            .any(|modifier| modifier.expires_tick == u64::MAX)
    );
    while original.next_tick <= 201 {
        assert_eq!(original.step().checksum, restored.step().checksum);
        if original.next_tick == 101 {
            let state = control(&original, target);
            assert!(!state.orders_suspended);
            assert!(
                state
                    .callbacks
                    .iter()
                    .any(|callback| callback.action == ControlAction::DefenderDefend)
            );
        }
    }
    let state = control(&original, target);
    assert!(!state.defend_disabled && !state.orders_suspended);
    assert!(state.callbacks.is_empty());
    assert_eq!(
        *original
            .world
            .get::<PassiveUnitEffects>(target_entity)
            .unwrap(),
        retained
    );
}

#[test]
fn zero_damage_or_removed_source_does_not_cleanse_morph_shield_or_passive_baseline() {
    for remove_source in [false, true] {
        let mut original = simulation(1);
        let (source, target, baseline, _) = prepare(&mut original);
        original.step();
        if remove_source {
            assert!(original.remove_building(source));
        } else {
            let mut query = original.world.query::<&mut NativeCarrierState>();
            for mut state in query.iter_mut(&mut original.world) {
                if let NativeCarrierState::Bolt(bolt) = &mut *state {
                    bolt.damage = 0;
                }
            }
        }
        let callbacks = control(&original, target).callbacks.clone();
        let mut restored = restore(&original, 4);
        for _ in 0..40 {
            assert_eq!(original.step().checksum, restored.step().checksum);
        }
        assert!(original.projectiles().is_empty());
        let state = control(&original, target);
        assert!(state.hex.is_some());
        assert_eq!(state.shield_level, 2);
        assert_eq!(state.overheat_level, 2);
        assert_eq!(state.callbacks, callbacks);
        assert_eq!(
            *original
                .world
                .get::<PassiveUnitEffects>(entity(&original, target))
                .unwrap(),
            baseline
        );
    }
}
