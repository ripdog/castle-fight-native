//! Source-catalog validation plus synthetic interactions that need retained control profiles.
use super::*;
use crate::MapVersion;
use crate::building_mechanics::{city_spellcasting_for_version, overheat_shield_for_version};

fn control_test_config() -> SimulationConfig {
    SimulationConfig {
        navigation_min: NavCell::new(-128, -128),
        navigation_max: NavCell::new(128, 128),
        ..SimulationConfig::default()
    }
}

#[test]
fn retained_building_catalog_registers_hex_and_preserves_proxy_provenance() {
    let version = MapVersion::CASTLE_FIGHT_9_27;
    let content = crate::castle_fight_content_bundle(version).unwrap();
    let definition = crate::CastleFightTowerKind::CityOfMagic
        .definition_for_version(version)
        .unwrap();
    let spell = city_spellcasting_for_version(version);
    assert_eq!(definition.spellcasting, Some(spell));
    assert_eq!(
        crate::CastleFightTowerKind::CityOfMagic.stable_id().0,
        0x3000_000a
    );
    let bindings = crate::resolve_native_effect_requirements(
        version,
        &[crate::NativeEffectSource::new(
            crate::NativeEffectSourceKind::UnitAbility,
            spell.ability.id.0,
        )],
    )
    .unwrap();
    assert_eq!(bindings.len(), 2);
    assert!(bindings.iter().all(|b| b.implementation == crate::NativeEffectImplementationId::WarcraftBuildingHexV1));
    assert!(
        content
            .building_kind_for_rawcode(definition.rawcode)
            .is_some()
    );
}

#[test]
fn retained_control_deadlines_are_separate_from_native_duration_and_mana_cadence() {
    let version = MapVersion::CASTLE_FIGHT_9_27;
    let spell = city_spellcasting_for_version(version);
    let AbilityEffect::Hex { profile } = spell.ability.effect else {
        panic!("Hex definition");
    };
    let mut sim = Simulation::new(control_test_config(), 1);
    let definition = crate::CastleFightUnitKind::Defender
        .definition_for_version(version)
        .unwrap();
    let target = sim.spawn_unit_with_properties(
        UnitSpawn {
            team: Team(1),
            position: SimPoint::new(0, 0),
            health: 1000,
            attack: AttackProfile {
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
                delivery: AttackDelivery::Melee,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        },
        definition.gameplay_properties(),
    );
    let mut unit = sim.snapshot_units()[0];
    assert!(sim.resolve_building_hex(&mut unit, profile, SimId(999), 0));
    let deadline = u64::from(profile.initial_reengage_ticks);
    sim.next_tick = deadline - 1;
    sim.advance_building_spell_controls();
    assert!(sim.snapshot_units()[0].orders_suspended);
    sim.next_tick = deadline;
    sim.advance_building_spell_controls();
    assert!(!sim.snapshot_units()[0].orders_suspended);
    let defend_deadline = deadline + u64::from(profile.defender_restore_ticks);
    sim.next_tick = defend_deadline - 1;
    sim.advance_building_spell_controls();
    assert!(sim.unit(target).unwrap().active_defend_ability.is_none());
    sim.next_tick = defend_deadline;
    sim.advance_building_spell_controls();
    assert!(sim.unit(target).unwrap().hex.is_none());
    assert!(sim.unit(target).unwrap().active_defend_ability.is_some());
    assert!(sim.snapshot_units()[0].orders_suspended);
    sim.next_tick = defend_deadline + u64::from(profile.defender_resume_ticks) - 1;
    sim.advance_building_spell_controls();
    assert!(sim.snapshot_units()[0].orders_suspended);
    sim.next_tick += 1;
    sim.advance_building_spell_controls();
    assert!(!sim.snapshot_units()[0].orders_suspended);
}

#[test]
fn retained_mana_gate_casts_on_exact_seconds_not_rounded_per_tick_rates() {
    let spell = city_spellcasting_for_version(MapVersion::CASTLE_FIGHT_9_27);
    let mut sim = Simulation::new(control_test_config(), 1);
    sim.spawn_unit(UnitSpawn {
        team: Team(1),
        position: SimPoint::new(20 * SUBUNITS_PER_WORLD_UNIT, 0),
        health: 1000,
        attack: AttackProfile {
            damage: 0,
            range: 0,
            acquisition_range: 0,
            cooldown_ticks: 1,
            delivery: AttackDelivery::Melee,
        },
        movement: MovementProfile { speed_per_tick: 0 },
    });
    let source = sim.spawn_building(BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(0, 10, 1, 1),
        health: 10_000,
        production: None,
        attack: None,
        spellcasting: Some(spell),
    });
    let crate::ManaRegeneration::PerSecondPer10k(rate) = spell.mana.regeneration() else {
        panic!("exact rate");
    };
    let ticks_to_first = (u64::try_from(spell.ability.mana_cost - spell.mana.starting).unwrap()
        * 10_000
        * CASTLE_FIGHT_SIMULATION_HZ as u64)
        .div_ceil(u64::from(rate));
    let cadence = (u64::try_from(spell.ability.mana_cost).unwrap()
        * 10_000
        * CASTLE_FIGHT_SIMULATION_HZ as u64)
        .div_ceil(u64::from(rate));
    let mut cast_ticks = Vec::new();
    for tick in 0..ticks_to_first + cadence * 2 {
        if sim.step().ability_casts > 0 {
            cast_ticks.push(tick);
        }
    }
    assert_eq!(
        cast_ticks,
        vec![
            ticks_to_first - 1,
            ticks_to_first + cadence - 1,
            ticks_to_first + cadence * 2 - 1
        ]
    );
    assert_eq!(sim.building(source).unwrap().mana_current, Some(0));
}

#[test]
fn overheat_shield_roll_precedence_progression_and_ground_death_splash_follow_evidence() {
    let version = MapVersion::CASTLE_FIGHT_9_27;
    let spell = city_spellcasting_for_version(version);
    let AbilityEffect::Hex { profile } = spell.ability.effect else {
        panic!()
    };
    let shield = overheat_shield_for_version(version);
    let mut sim = Simulation::new(control_test_config(), 1);
    let spawn = |team| UnitSpawn {
        team: Team(team),
        position: SimPoint::new(0, 0),
        health: 10_000,
        attack: AttackProfile {
            damage: 0,
            range: 0,
            acquisition_range: 0,
            cooldown_ticks: 1,
            delivery: AttackDelivery::Melee,
        },
        movement: MovementProfile { speed_per_tick: 0 },
    };
    let target = sim.spawn_unit_with_properties(
        spawn(1),
        UnitGameplayProperties {
            content: Some(ContentIdentity {
                rawcode: shield.rawcode,
                name: "Shield fixture",
            }),
            ..Default::default()
        },
    );
    sim.set_negative_building_markers(target, true, false);
    let state_entity = sim
        .world
        .iter_entities()
        .find(|e| {
            e.get::<BuildingSpellTargetState>()
                .is_some_and(|s| s.target == target)
        })
        .unwrap()
        .id();
    sim.set_negative_building_shield(target, 1, None);
    let mut unit = sim.snapshot_units()[0];
    assert!(!sim.resolve_building_hex(&mut unit, profile, SimId(999), 0));
    assert_eq!(
        sim.world
            .entity(state_entity)
            .get::<BuildingSpellTargetState>()
            .unwrap()
            .overheat_level,
        shield.initial_level,
        "active shield precedes the overheat roll"
    );
    let mut blocked = 0;
    let mut passed = 0;
    for sequence in 0..100 {
        let mut unit = sim.snapshot_units()[0];
        if sim.resolve_building_hex(&mut unit, profile, SimId(999), sequence) {
            passed += 1;
            sim.dispel_building_hex(target);
        } else {
            blocked += 1;
        }
    }
    assert!(
        blocked > 0 && passed > 0,
        "failed roll must not fall through to A070"
    );
    let level = sim
        .world
        .entity(state_entity)
        .get::<BuildingSpellTargetState>()
        .unwrap()
        .overheat_level;
    assert_eq!(level, shield.maximum_level);
    let units = sim.snapshot_units();
    assert_eq!(
        units[0].status.attack_speed_modifiers[0].percent_delta,
        shield.attack_speed_percent[usize::from(level - 1)]
    );
    let ground = sim.spawn_unit(spawn(0));
    let air = sim.spawn_unit_with_properties(
        spawn(0),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            ..Default::default()
        },
    );
    let units = sim.snapshot_units();
    let mut health: Vec<_> = units
        .iter()
        .map(|u| if u.id == target { 0 } else { u.health })
        .collect();
    let positions: Vec<_> = units.iter().map(|u| u.position).collect();
    sim.resolve_overheat_deaths(&units, &[], &positions, &mut health, &mut []);
    let damage = sim.damage_rules().apply_spell(
        shield.explosions[usize::from(level - 1)].full_damage,
        units[1].armor.armor_type,
    );
    assert_eq!(
        health[find_unit_index(&units, ground).unwrap()],
        10_000 - damage
    );
    assert_eq!(health[find_unit_index(&units, air).unwrap()], 10_000);
}
