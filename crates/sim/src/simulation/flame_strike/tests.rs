use super::*;
use crate::{UnitGameplayProperties, building_mechanics::BuildingFlameStrikeProfile};

fn profile() -> BuildingFlameStrikeProfile {
    let spell = crate::building_mechanics::flame_strike_spellcasting_for_version(
        crate::MapVersion::CASTLE_FIGHT_9_27,
    );
    let AbilityEffect::BuildingFlameStrike(mut p) = spell.ability.effect else {
        panic!()
    };
    p.trigger_mechanical_only = false;
    p.dummy_ticks = 10;
    for option in &mut p.options {
        option.radius = 20 * SUBUNITS_PER_WORLD_UNIT;
        option.cast_ticks = 2;
        option.full_damage = 7;
        option.full_interval_millis = 200;
        option.full_duration_millis = 450;
        option.half_damage = 3;
        option.half_interval_millis = 300;
        option.duration_millis = 1500;
        option.maximum_damage = 21;
    }
    p.options[0].building_damage_per_10k = 5000;
    p
}
fn sim() -> Simulation {
    Simulation::new(
        SimulationConfig {
            navigation_max: NavCell::new(500, 500),
            ..Default::default()
        },
        1,
    )
}
fn entity(sim: &Simulation, id: SimId) -> Entity {
    sim.world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}
fn unit(
    sim: &mut Simulation,
    x: i32,
    flags: UnitClassifications,
    movement: MovementClass,
) -> SimId {
    sim.spawn_unit_with_properties(
        UnitSpawn {
            team: Team(1),
            position: SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, 20 * SUBUNITS_PER_WORLD_UNIT),
            health: 1000,
            attack: AttackProfile {
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1000,
                delivery: AttackDelivery::Melee,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        },
        UnitGameplayProperties {
            movement_class: movement,
            collision_radius: Some(CollisionRadius(0)),
            classifications: flags,
            ..Default::default()
        },
    )
}
fn caster(sim: &mut Simulation, p: BuildingFlameStrikeProfile) -> SimId {
    sim.spawn_building(BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(1, 1, 1, 1),
        health: 1000,
        production: None,
        attack: None,
        spellcasting: Some(SpellcastingProfile {
            mana: crate::ManaProfile::per_second(9, 7, 0),
            ability: AutomaticAbilityProfile {
                id: AbilityId(721),
                mana_cost: 7,
                cooldown_ticks: 1,
                range: 1000 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
                effect: AbilityEffect::BuildingFlameStrike(p),
            },
        }),
    })
}
fn restored(sim: &Simulation) -> Simulation {
    let bundle = crate::castle_fight_content_bundle(crate::MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), bundle)
            .unwrap();
    let mut other = Simulation::new(sim.config.clone(), 4);
    other.restore_snapshot(&snapshot).unwrap();
    other
}

#[test]
fn flame_strike_samples_fixed_selected_point_after_cast_delay_and_survives_recycling() {
    let mut sim = sim();
    let p = profile();
    let caster = caster(&mut sim, p);
    let target = unit(
        &mut sim,
        100,
        UnitClassifications {
            combat_sapper: true,
            ..Default::default()
        },
        MovementClass::Ground,
    );
    sim.step();
    assert_eq!(sim.building(caster).unwrap().mana_current, Some(0));
    let state = sim
        .world
        .iter_entities()
        .find_map(|e| match e.get::<NativeAction>() {
            Some(NativeAction::FlameStrike(s)) => Some(*s),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        state.center,
        SimPoint::new(100 * SUBUNITS_PER_WORLD_UNIT, 20 * SUBUNITS_PER_WORLD_UNIT)
    );
    sim.world
        .entity_mut(entity(&sim, target))
        .get_mut::<Position>()
        .unwrap()
        .0
        .x = 200 * SUBUNITS_PER_WORLD_UNIT;
    let newcomer = unit(
        &mut sim,
        100,
        UnitClassifications::default(),
        MovementClass::Ground,
    );
    sim.world.despawn(entity(&sim, caster));
    let mut other = restored(&sim);
    sim.step();
    other.step();
    assert_eq!(sim.unit(newcomer).unwrap().health, 1000);
    sim.step();
    other.step();
    assert_eq!(sim.unit(newcomer).unwrap().health, 993);
    for _ in 0..60 {
        sim.step();
        other.step();
        assert_eq!(sim.checksum(), other.checksum());
    }
    assert_eq!(sim.unit(target).unwrap().health, 1000);
    assert_eq!(sim.unit(newcomer).unwrap().health, 1000 - 3 * 7 - 3 * 3);
    assert!(
        !sim.world
            .iter_entities()
            .any(|e| matches!(e.get::<NativeAction>(), Some(NativeAction::FlameStrike(_))))
    );
}

#[test]
fn full_and_half_pulses_keep_fractional_deadlines_and_the_final_expiry_pulse() {
    let p = profile().options[0];
    assert_eq!(p.pulse(0), Some((0, 7)));
    assert_eq!(p.pulse(2), Some((400, 7)));
    assert_eq!(p.pulse(3), Some((900, 3)));
    assert_eq!(p.pulse(5), Some((1500, 3)));
    assert_eq!(p.pulse(6), None);
}

#[test]
fn flame_strike_cap_and_native_recipient_classes_are_independent_of_selection() {
    let mut sim = sim();
    let p = profile();
    caster(&mut sim, p);
    let mut targets = Vec::new();
    for _ in 0..6 {
        targets.push(unit(
            &mut sim,
            100,
            UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
            MovementClass::Ground,
        ));
    }
    let air = unit(
        &mut sim,
        100,
        UnitClassifications::default(),
        MovementClass::Air,
    );
    let immune = unit(
        &mut sim,
        100,
        UnitClassifications {
            spell_immune: true,
            ..Default::default()
        },
        MovementClass::Ground,
    );
    let protected = unit(
        &mut sim,
        100,
        UnitClassifications {
            invulnerable: true,
            ..Default::default()
        },
        MovementClass::Ground,
    );
    for _ in 0..3 {
        sim.step();
    }
    for id in targets {
        assert_eq!(sim.unit(id).unwrap().health, 997);
    }
    for id in [air, immune, protected] {
        assert_eq!(sim.unit(id).unwrap().health, 1000);
    }
}

#[test]
fn mode_switch_keeps_structure_mask_and_multiplier_separate_from_unit_damage() {
    for enabled in [true, false] {
        let mut sim = sim();
        sim.config.match_rules.artillery_enabled = enabled;
        let p = profile();
        caster(&mut sim, p);
        let target = unit(
            &mut sim,
            100,
            UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
            MovementClass::Ground,
        );
        let building = sim.spawn_building(BuildingSpawn {
            team: Team(1),
            footprint: BuildingFootprint::new(105, 20, 1, 1),
            health: 1000,
            production: None,
            attack: None,
            spellcasting: None,
        });
        for _ in 0..3 {
            sim.step();
        }
        assert_eq!(sim.unit(target).unwrap().health, 993);
        let damage = sim
            .combat_rules
            .damage_rules
            .apply_spell(3, sim.building(building).unwrap().armor.armor_type);
        assert_eq!(
            sim.building(building).unwrap().health,
            1000 - if enabled { damage } else { 0 }
        );
    }
}

#[test]
fn native_buff_families_control_overlap_even_when_both_expire_on_the_same_tick() {
    for different_buff in [false, true] {
        let mut sim = sim();
        let p = profile();
        let target = unit(
            &mut sim,
            100,
            UnitClassifications::default(),
            MovementClass::Ground,
        );
        let center = sim.unit(target).unwrap().position;
        for index in 0..2 {
            let actor = sim.allocate_id();
            let mut profile = p.options[0];
            if index == 1 && different_buff {
                profile.buff = profile.buff.wrapping_add(1);
            }
            let id = sim.allocate_id();
            sim.world.spawn((
                id,
                NativeAction::FlameStrike(FlameStrikeState {
                    map_version: p.map_version,
                    source: actor,
                    team: Team(0),
                    center,
                    profile,
                    released_tick: 1,
                    expires_tick: 1 + ceil_millis_to_ticks(u64::from(profile.duration_millis)),
                    next_pulse: 0,
                }),
            ));
        }
        let mut other = restored(&sim);
        for _ in 0..60 {
            sim.step();
            other.step();
            assert_eq!(sim.checksum(), other.checksum());
        }
        assert_eq!(
            sim.unit(target).unwrap().health,
            1000 - 30 * if different_buff { 2 } else { 1 }
        );
    }
}

#[test]
fn a_native_structure_trigger_spends_mana_without_a_ground_callback_target() {
    let mut sim = sim();
    let p = profile();
    let caster = caster(&mut sim, p);
    let target = sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(10, 5, 1, 1),
        health: 1000,
        production: None,
        attack: None,
        spellcasting: None,
    });
    sim.step();
    assert_eq!(sim.building(caster).unwrap().mana_current, Some(0));
    assert!(sim.world.iter_entities().any(
        |e| matches!(e.get::<NativeAction>(), Some(NativeAction::Parasite(s)) if s.target == target)
    ));
    assert!(
        !sim.world
            .iter_entities()
            .any(|e| matches!(e.get::<NativeAction>(), Some(NativeAction::FlameStrike(_))))
    );
}
