use super::*;
use crate::{CastleFightTowerKind, ManaProfile, MapVersion, UnitTemplate};
const VERSION: MapVersion = MapVersion::CASTLE_FIGHT_9_27;
fn simulation() -> Simulation {
    Simulation::new(SimulationConfig::default(), 1)
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
    team: u8,
    x: i32,
    movement: MovementClass,
    flags: UnitClassifications,
) -> SimId {
    sim.spawn_unit_with_properties(
        UnitSpawn::from_template(
            Team(team),
            SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, 10 * SUBUNITS_PER_WORLD_UNIT),
            UnitTemplate {
                health: 1000,
                attack: AttackProfile {
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    cooldown_ticks: 100,
                    delivery: AttackDelivery::Melee,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
        ),
        UnitGameplayProperties {
            movement_class: movement,
            classifications: flags,
            ..Default::default()
        },
    )
}
fn wave() -> crate::building_mechanics::WeatherWaveProfile {
    crate::building_mechanics::WeatherWaveProfile {
        ability: AbilityId(90),
        radius: 5 * SUBUNITS_PER_WORLD_UNIT,
        damage: 20,
        maximum_damage: 100,
        building_damage_per_10k: 5000,
        waves: 3,
        shards: 2,
        interval_ticks: 4,
        first_impact_ticks: 5,
        damage_per_second: 0,
        duration_ticks: 0,
        hero_duration_ticks: 0,
        buff_rawcode: 91,
        wave_buff_rawcode: 92,
        targets: AttackTargetMask::GROUND_AND_BUILDINGS,
    }
}
fn storm(
    sim: &mut Simulation,
    profile: crate::building_mechanics::WeatherWaveProfile,
    center: SimPoint,
) -> SimId {
    let id = sim.allocate_id();
    sim.world.spawn((
        id,
        crate::components::NativeAction::Weather(WeatherState {
            map_version: VERSION,
            source: SimId(0),
            team: Team(0),
            center,
            profile,
            started_tick: sim.next_tick,
            expires_tick: sim.next_tick + 100,
            next_wave: 0,
            next_visual: 0,
        }),
    ));
    id
}
fn restore(sim: &Simulation) -> Simulation {
    let content = crate::castle_fight_content_bundle(VERSION).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), content)
            .unwrap();
    let mut result = Simulation::new(sim.config.clone(), 4);
    result.restore_snapshot(&snapshot).unwrap();
    result
}
#[test]
fn released_point_waves_sample_live_recipients_and_restore_every_deadline() {
    let mut sim = simulation();
    let flags = UnitClassifications {
        combat_sapper: true,
        ..Default::default()
    };
    let target = unit(&mut sim, 1, 50, MovementClass::Ground, flags);
    let ally = unit(&mut sim, 0, 51, MovementClass::Ground, flags);
    let air = unit(&mut sim, 1, 51, MovementClass::Air, flags);
    let immune = unit(
        &mut sim,
        1,
        51,
        MovementClass::Ground,
        UnitClassifications {
            spell_immune: true,
            ..flags
        },
    );
    let invul = unit(
        &mut sim,
        1,
        51,
        MovementClass::Ground,
        UnitClassifications {
            invulnerable: true,
            ..flags
        },
    );
    let outside = unit(&mut sim, 1, 56, MovementClass::Ground, flags);
    let center = sim.unit(target).unwrap().position;
    storm(&mut sim, wave(), center);
    for tick in 0..5 {
        sim.step();
        assert_eq!(sim.unit(target).unwrap().health, 1000);
        if tick == 0 {
            assert_eq!(
                sim.ability_casts_last_tick().len(),
                usize::from(wave().shards)
            );
            assert!(
                sim.ability_casts_last_tick()
                    .iter()
                    .all(|event| matches!(event.effect, AbilityEffect::NativeWeatherVisual { .. }))
            );
        }
    }
    let mut other = restore(&sim);
    for _ in 0..9 {
        assert_eq!(sim.step().checksum, other.step().checksum);
    }
    assert_eq!(sim.unit(target).unwrap().health, 940);
    for id in [ally, air, immune, invul, outside] {
        assert_eq!(sim.unit(id).unwrap().health, 1000);
    }
    assert!(!sim.world.iter_entities().any(|e| matches!(
        e.get::<crate::components::NativeAction>(),
        Some(crate::components::NativeAction::Weather(_))
    )));
}
#[test]
fn wave_cap_is_shared_proportionally_and_building_factor_is_independent() {
    let mut sim = simulation();
    let flags = UnitClassifications::default();
    let first = unit(&mut sim, 1, 50, MovementClass::Ground, flags);
    let second = unit(&mut sim, 1, 50, MovementClass::Ground, flags);
    let building = sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(53, 10, 1, 1),
        health: 1000,
        production: None,
        attack: None,
        spellcasting: None,
    });
    let center = sim.unit(first).unwrap().position;
    let mut p = wave();
    p.maximum_damage = 25;
    p.waves = 1;
    storm(&mut sim, p, center);
    for _ in 0..6 {
        sim.step();
    }
    assert_eq!(sim.unit(first).unwrap().health, 990);
    assert_eq!(sim.unit(second).unwrap().health, 990);
    assert_eq!(sim.building(building).unwrap().health, 995);
}
#[test]
fn strongest_overlapping_native_family_wins_even_on_its_final_wave() {
    let mut sim = simulation();
    let target = unit(
        &mut sim,
        1,
        50,
        MovementClass::Ground,
        UnitClassifications::default(),
    );
    let center = sim.unit(target).unwrap().position;
    let mut strong = wave();
    strong.damage = 35;
    storm(&mut sim, strong, center);
    storm(&mut sim, wave(), center);
    for _ in 0..14 {
        sim.step();
    }
    assert_eq!(sim.unit(target).unwrap().health, 895);
}
#[test]
fn native_parent_commits_without_an_eligible_point_and_ground_marker_is_excluded() {
    let mut sim = simulation();
    let definition = CastleFightTowerKind::ElementalRain.definition();
    let mut spawn = definition.spawn(Team(0), BuildingFootprint::new(2, 2, 1, 1));
    let casting = spawn.spellcasting.as_mut().unwrap();
    let cost = casting.ability.mana_cost;
    casting.mana = ManaProfile::per_second(cost, cost, 0);
    let AbilityEffect::BuildingWeather(mut p) = casting.ability.effect else {
        panic!()
    };
    p.option_count = 1;
    for mode in &mut p.options {
        mode[0] = wave();
    }
    casting.ability.effect = AbilityEffect::BuildingWeather(p);
    let caster = sim.spawn_building_with_properties(spawn, definition.gameplay_properties());
    let marked = unit(
        &mut sim,
        1,
        55,
        MovementClass::Ground,
        UnitClassifications {
            combat_sapper: true,
            ..Default::default()
        },
    );
    sim.world
        .get_mut::<StatusState>(entity(&sim, marked))
        .unwrap()
        .native_grants
        .insert(crate::NativeAbilityGrant {
            ability: AbilityId(p.selector_excluded_ability),
            effect: crate::NativeAbilityGrantEffect::AuraMarker,
        });
    // Invulnerable enemy can trigger native Parasite but cannot enter the script picker.
    unit(
        &mut sim,
        1,
        50,
        MovementClass::Ground,
        UnitClassifications {
            invulnerable: true,
            combat_sapper: true,
            ..Default::default()
        },
    );
    for _ in 0..60 {
        if sim.building(caster).unwrap().ability_cast_sequence != Some(0) {
            break;
        }
        sim.step();
    }
    assert_eq!(sim.building(caster).unwrap().ability_cast_sequence, Some(1));
    assert_eq!(sim.building(caster).unwrap().mana_current, Some(0));
    assert!(!sim.world.iter_entities().any(|e| matches!(
        e.get::<crate::components::NativeAction>(),
        Some(crate::components::NativeAction::Weather(_))
    )));
    assert_eq!(sim.building(caster).unwrap().ability_cast_sequence, Some(1));
}
#[test]
fn release_uses_selected_enemy_point_and_mode_then_survives_building_removal() {
    for enabled in [true, false] {
        let mut config = SimulationConfig::default();
        config.match_rules.artillery_enabled = enabled;
        let mut sim = Simulation::new(config, 1);
        let definition = CastleFightTowerKind::ElementalRain.definition();
        let mut spawn = definition.spawn(Team(0), BuildingFootprint::new(2, 2, 1, 1));
        let casting = spawn.spellcasting.as_mut().unwrap();
        let cost = casting.ability.mana_cost;
        casting.mana = ManaProfile::per_second(cost, cost, 0);
        let AbilityEffect::BuildingWeather(mut p) = casting.ability.effect else {
            panic!()
        };
        p.option_count = 1;
        p.options = [[wave(); 2]; 2];
        p.options[1][0].damage = 30;
        casting.ability.effect = AbilityEffect::BuildingWeather(p);
        let caster = sim.spawn_building_with_properties(spawn, definition.gameplay_properties());
        let target = unit(
            &mut sim,
            1,
            50,
            MovementClass::Ground,
            UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
        );
        for _ in 0..60 {
            if sim.building(caster).unwrap().ability_cast_sequence != Some(0) {
                break;
            }
            sim.step();
        }
        assert_eq!(sim.building(caster).unwrap().ability_cast_sequence, Some(1));
        sim.world.despawn(entity(&sim, caster));
        let mut other = restore(&sim);
        for _ in 0..13 {
            assert_eq!(sim.step().checksum, other.step().checksum);
        }
        assert_eq!(
            sim.unit(target).unwrap().health,
            1000 - 3 * if enabled { 20 } else { 30 }
        );
    }
}
