use super::*;
use crate::simulation::building_spells::{
    BuildingSpellTargetState, refresh_building_spell_controls,
};

fn ordered_spell() -> SpellcastingProfile {
    SpellcastingProfile {
        mana: fire().mana,
        ability: AutomaticAbilityProfile {
            id: AbilityId(30),
            mana_cost: 1,
            cooldown_ticks: 1000,
            range: 200 * SUBUNITS_PER_WORLD_UNIT,
            target_policy: AbilityTargetPolicy::RandomEnemyUnit,
            effect: AbilityEffect::Damage { amount: 1 },
        },
    }
}

fn hex() -> crate::building_mechanics::HexEffectProfile {
    let AbilityEffect::Hex { mut profile } =
        crate::building_mechanics::city_spellcasting_for_version(
            crate::MapVersion::CASTLE_FIGHT_9_27,
        )
        .ability
        .effect
    else {
        unreachable!()
    };
    profile.duration_ticks = 1000;
    profile.hero_duration_ticks = 1000;
    profile.initial_reengage_ticks = 1000;
    profile.ground.collision_radius = 1;
    profile.ground.speed_per_tick = 0;
    profile
}

fn wire_restore(original: &Simulation) -> Simulation {
    let content =
        crate::castle_fight_content_bundle(crate::CASTLE_FIGHT_DEFAULT_MAP_VERSION).unwrap();
    let wire = original.capture_snapshot().encode_wire().unwrap();
    let snapshot = SimulationSnapshot::decode_wire(&wire, content).unwrap();
    let mut restored = sim(4);
    restored.restore_snapshot(&snapshot).unwrap();
    assert_eq!(original.checksum(), restored.checksum());
    restored
}

#[test]
fn healing_recovery_blocks_ordered_intents_but_not_pending_native_fire_and_survives_wire() {
    let mut original = sim(1);
    let wave_profile = SpellcastingProfile {
        mana: fire().mana,
        ability: AutomaticAbilityProfile {
            id: AbilityId(10),
            mana_cost: 1,
            cooldown_ticks: 1000,
            range: 200 * SUBUNITS_PER_WORLD_UNIT,
            target_policy: AbilityTargetPolicy::WoundedFriendlyUnit,
            effect: AbilityEffect::HealingWave(wave()),
        },
    };
    let source = caster(&mut original, wave_profile);
    unit(&mut original, 0, 24, MovementClass::Ground, true);
    unit(&mut original, 1, 25, MovementClass::Ground, false);
    original
        .configure_additional_automatic_abilities(
            source,
            &[fire().ability, ordered_spell().ability],
        )
        .unwrap();
    let metrics = original.step();
    assert_eq!(
        metrics.ability_casts, 2,
        "native fire commits after the earlier healing cast's recovery starts"
    );
    let status = original.unit(source).unwrap().status;
    assert_eq!(status.stunned_until_tick, 0);
    assert_eq!(
        status.order_recovery_until_tick,
        u64::from(wave().recovery_ticks)
    );
    assert_eq!(
        original.unit(source).unwrap().mana_current,
        Some(fire().mana.starting - 1)
    );
    let source_entity = entity(&original, source);
    let slots = original
        .world
        .get::<AdditionalAutomaticAbilities>(source_entity)
        .unwrap();
    assert_eq!(slots.get(fire().ability.id).unwrap().state.cast_sequence, 1);
    assert_eq!(
        slots
            .get(ordered_spell().ability.id)
            .unwrap()
            .state
            .cast_sequence,
        0,
        "the already evaluated ordered intent revalidates the live recovery state"
    );
    let mut restored = wire_restore(&original);
    let recovery_end = status.order_recovery_until_tick;
    while original.next_tick <= recovery_end {
        assert_eq!(original.step().checksum, restored.step().checksum);
        let count = original
            .world
            .get::<AdditionalAutomaticAbilities>(source_entity)
            .unwrap()
            .get(ordered_spell().ability.id)
            .unwrap()
            .state
            .cast_sequence;
        assert_eq!(count, u64::from(original.next_tick > recovery_end));
    }
    assert_eq!(
        original.unit(source).unwrap().mana_current,
        Some(fire().mana.starting - 2)
    );
}

#[test]
fn primary_and_additional_native_fire_ignore_stun_recovery_and_script_orders_but_not_hex() {
    for primary_fire in [false, true] {
        let mut original = sim(1);
        let source = caster(
            &mut original,
            if primary_fire {
                fire()
            } else {
                ordered_spell()
            },
        );
        if !primary_fire {
            original
                .configure_additional_automatic_abilities(source, &[fire().ability])
                .unwrap();
        }
        let source_entity = entity(&original, source);
        {
            let mut status = original
                .world
                .get_mut::<StatusState>(source_entity)
                .unwrap();
            status.stunned_until_tick = 1000;
            status.order_recovery_until_tick = 1000;
        }
        original.set_negative_building_markers(source, false, false);
        let control_entity = original
            .world
            .iter_entities()
            .find_map(|entity| {
                entity
                    .get::<BuildingSpellTargetState>()
                    .filter(|state| state.target == source)
                    .map(|_| entity.id())
            })
            .unwrap();
        original
            .world
            .get_mut::<BuildingSpellTargetState>(control_entity)
            .unwrap()
            .orders_suspended = true;
        refresh_building_spell_controls(&mut original.world, original.next_tick);
        let target = unit(&mut original, 1, 25, MovementClass::Ground, false);
        assert_eq!(original.step().ability_casts, 1);
        assert_eq!(original.projectiles().len(), 1);
        let mut projected = original
            .snapshot_units()
            .into_iter()
            .find(|unit| unit.id == source)
            .unwrap();
        assert!(original.resolve_building_hex(&mut projected, hex(), SimId(9000), 0));
        // Force readiness and supply a second unmarked target: cooldown/burn exclusion must
        // not accidentally explain the missing cast while native Hex disables abilities.
        if primary_fire {
            original
                .world
                .get_mut::<AutomaticAbilityState>(source_entity)
                .unwrap()
                .ready_tick = 0;
        } else {
            original
                .world
                .get_mut::<AdditionalAutomaticAbilities>(source_entity)
                .unwrap()
                .get_mut(fire().ability.id)
                .unwrap()
                .state
                .ready_tick = 0;
        }
        let untouched = unit(&mut original, 1, 30, MovementClass::Ground, false);
        let mut restored = wire_restore(&original);
        let metrics = original.step();
        assert_eq!(metrics.checksum, restored.step().checksum);
        assert_eq!(metrics.ability_casts, 0);
        assert_eq!(
            original.unit(target).unwrap().health,
            93,
            "the committed missile survives source ability disable"
        );
        assert_eq!(original.unit(untouched).unwrap().health, 100);
    }
}

#[test]
fn a_prior_committed_hex_cancels_evaluated_native_fire_in_primary_and_additional_slots() {
    for primary_fire in [false, true] {
        let mut original = sim(1);
        original.spawn_building(BuildingSpawn {
            team: Team(0),
            footprint: BuildingFootprint::new(0, 0, 1, 1),
            health: 100,
            production: None,
            attack: None,
            spellcasting: Some(SpellcastingProfile {
                mana: ManaProfile::per_second(1, 1, 0),
                ability: AutomaticAbilityProfile {
                    id: AbilityId(1),
                    mana_cost: 1,
                    cooldown_ticks: 1000,
                    range: 0,
                    target_policy: AbilityTargetPolicy::RandomEnemyUnitGlobal,
                    effect: AbilityEffect::Hex { profile: hex() },
                },
            }),
        });
        let source = original.spawn_unit_with_properties_and_spellcasting(
            UnitSpawn {
                team: Team(1),
                position: SimPoint::new(20 * SUBUNITS_PER_WORLD_UNIT, 0),
                health: 100,
                attack: AttackProfile {
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    cooldown_ticks: 30,
                    delivery: AttackDelivery::Melee,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
            UnitGameplayProperties {
                classifications: UnitClassifications {
                    combat_sapper: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            if primary_fire {
                fire()
            } else {
                ordered_spell()
            },
        );
        if !primary_fire {
            original
                .configure_additional_automatic_abilities(source, &[fire().ability])
                .unwrap();
        }
        let source_entity = entity(&original, source);
        original
            .world
            .get_mut::<StatusState>(source_entity)
            .unwrap()
            .stunned_until_tick = 1000;
        unit(&mut original, 0, 25, MovementClass::Ground, false);
        let metrics = original.step();
        assert_eq!(
            metrics.ability_casts, 1,
            "earlier Hex cancels the already evaluated passive intent"
        );
        assert!(original.projectiles().is_empty());
        let sequence = if primary_fire {
            original
                .world
                .get::<AutomaticAbilityState>(source_entity)
                .unwrap()
                .cast_sequence
        } else {
            original
                .world
                .get::<AdditionalAutomaticAbilities>(source_entity)
                .unwrap()
                .get(fire().ability.id)
                .unwrap()
                .state
                .cast_sequence
        };
        assert_eq!(sequence, 0);
        assert_eq!(
            original.unit(source).unwrap().mana_current,
            Some(fire().mana.starting)
        );
    }
}

#[test]
fn an_order_recovery_deadline_is_authoritative_even_without_a_native_stun() {
    let mut original = sim(1);
    let source = caster(&mut original, ordered_spell());
    let before = original.checksum();
    original
        .world
        .get_mut::<StatusState>(entity(&original, source))
        .unwrap()
        .order_recovery_until_tick = 17;
    assert_ne!(original.checksum(), before);
    assert_eq!(original.unit(source).unwrap().status.stunned_until_tick, 0);
    let mut restored = wire_restore(&original);
    for _ in 0..20 {
        assert_eq!(original.step().checksum, restored.step().checksum);
    }
}
