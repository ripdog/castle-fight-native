use super::*;
use crate::{BuildingGameplayProperties, MapVersion, castle_fight_content_bundle};

fn simulation() -> Simulation {
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
fn profile() -> crate::building_mechanics::BuildingAttackProxyProfile {
    let spell = crate::building_mechanics::attack_proxy_spellcasting_for_version(
        MapVersion::CASTLE_FIGHT_9_27,
    );
    let AbilityEffect::BuildingAttackProxy(mut p) = spell.ability.effect else {
        panic!()
    };
    p.damage = 73;
    p.speed_per_tick = 2 * SUBUNITS_PER_WORLD_UNIT;
    p.dummy_ticks = 3;
    p.level_after_minutes = [2, 4, 7];
    for (i, level) in p.levels.iter_mut().enumerate() {
        level.freeze_ticks = 41 + i as u16;
        level.poison.duration_ticks = 95;
        level.poison.damage_per_second = 11 + i as i32;
    }
    p
}
fn target_building(sim: &mut Simulation, x: i32, flags: UnitClassifications) -> SimId {
    sim.spawn_building_with_properties(
        BuildingSpawn {
            team: Team(1),
            footprint: BuildingFootprint::new(x, 5, 1, 1),
            health: 500,
            production: None,
            attack: None,
            spellcasting: None,
        },
        BuildingGameplayProperties {
            classifications: flags,
            ..Default::default()
        },
    )
}
fn caster(sim: &mut Simulation, p: crate::building_mechanics::BuildingAttackProxyProfile) -> SimId {
    sim.spawn_building(BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(2, 5, 1, 1),
        health: 500,
        production: None,
        attack: None,
        spellcasting: Some(SpellcastingProfile {
            mana: crate::ManaProfile::per_second(9, 7, 0),
            ability: AutomaticAbilityProfile {
                id: AbilityId(713),
                mana_cost: 7,
                cooldown_ticks: 1,
                range: p.range,
                target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
                effect: AbilityEffect::BuildingAttackProxy(p),
            },
        }),
    })
}
fn restored(sim: &Simulation) -> Simulation {
    let bundle = castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), bundle)
            .unwrap();
    let mut other = Simulation::new(sim.config.clone(), 4);
    other.restore_snapshot(&snapshot).unwrap();
    other
}
fn proxy_actions(sim: &Simulation) -> usize {
    sim.world
        .iter_entities()
        .filter(|e| {
            matches!(
                e.get::<NativeAction>(),
                Some(NativeAction::BuildingAttackProxy(_))
            )
        })
        .count()
}

#[test]
fn attack_proxy_commits_parent_then_delivers_one_weapon_after_actor_and_building_expire() {
    let mut sim = simulation();
    let p = profile();
    let caster = caster(&mut sim, p);
    let target = target_building(&mut sim, 20, UnitClassifications::default());
    let neighbor = target_building(
        &mut sim,
        21,
        UnitClassifications {
            invulnerable: true,
            ..Default::default()
        },
    );
    // The script can select invulnerable buildings, so remove the neighbor from selection.
    sim.world
        .entity_mut(entity(&sim, neighbor))
        .insert(Owner(PlayerId(254)));
    sim.step();
    assert_eq!(sim.building(caster).unwrap().mana_current, Some(0));
    assert_eq!(proxy_actions(&sim), 1);
    let state = sim
        .world
        .iter_entities()
        .find_map(|e| match e.get::<NativeAction>() {
            Some(NativeAction::BuildingAttackProxy(s)) => Some(s.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        state.origin,
        footprint_center_point(
            BuildingFootprint::new(2, 5, 1, 1),
            sim.config.navigation_cell_size
        )
    );
    assert_eq!(sim.building(target).unwrap().health, 500);
    sim.world.despawn(entity(&sim, caster));
    let mut other = restored(&sim);
    while sim.next_tick <= state.impact_tick {
        sim.step();
        other.step();
        assert_eq!(sim.checksum(), other.checksum());
    }
    assert!(
        !sim.world
            .iter_entities()
            .any(|e| e.get::<SimId>() == Some(&state.source))
    );
    assert_eq!(sim.building(target).unwrap().health, 500 - p.damage);
    assert_eq!(sim.building(neighbor).unwrap().health, 500);
    let status = sim.building(target).unwrap().status;
    assert_eq!(
        status.frozen_until_tick,
        state.impact_tick + u64::from(p.levels[0].freeze_ticks)
    );
    assert_eq!(status.poison.unwrap().sources, 1);
    while sim.next_tick <= state.impact_tick + CASTLE_FIGHT_SIMULATION_HZ as u64 {
        sim.step();
        other.step();
        assert_eq!(sim.checksum(), other.checksum());
    }
    assert_eq!(
        sim.building(target).unwrap().health,
        500 - p.damage - p.levels[0].poison.damage_per_second
    );
    assert_eq!(proxy_actions(&sim), 0);
}

#[test]
fn failed_proxy_attack_keeps_mark_fog_parent_and_resource_commitment() {
    let mut sim = simulation();
    let p = profile();
    let caster = caster(&mut sim, p);
    let target = target_building(
        &mut sim,
        8,
        UnitClassifications {
            invulnerable: true,
            ..Default::default()
        },
    );
    sim.step();
    assert_eq!(sim.building(caster).unwrap().mana_current, Some(0));
    assert_eq!(proxy_actions(&sim), 0);
    let cast = sim
        .ability_casts_last_tick()
        .iter()
        .find(|c| c.ability == AbilityId(713))
        .unwrap();
    assert_eq!(
        cast.target,
        AbilityCastTarget::Point(footprint_center_point(
            sim.building(target).unwrap().footprint,
            sim.config.navigation_cell_size
        ))
    );
    assert!(sim.world.iter_entities().any(
        |e| matches!(e.get::<NativeAction>(), Some(NativeAction::Parasite(s)) if s.target == target)
    ));
    assert_eq!(sim.building(target).unwrap().health, 500);
}

#[test]
fn native_parent_can_trigger_on_an_excluded_building_without_a_script_effect() {
    let mut sim = simulation();
    let mut p = profile();
    let definition = crate::CastleFightTowerKind::WatchTower.definition();
    p.excluded_rawcodes[0] = definition.rawcode;
    let caster = caster(&mut sim, p);
    let target = sim.spawn_building_with_properties(
        definition.spawn(Team(1), BuildingFootprint::new(8, 5, 1, 1)),
        definition.gameplay_properties(),
    );
    sim.step();
    assert_eq!(sim.building(caster).unwrap().mana_current, Some(0));
    assert_eq!(proxy_actions(&sim), 0);
    assert!(sim.ability_casts_last_tick().is_empty());
    assert!(sim.world.iter_entities().any(
        |e| matches!(e.get::<NativeAction>(), Some(NativeAction::Parasite(s)) if s.target == target)
    ));
}

#[test]
fn round_scaling_uses_whole_minutes_and_is_snapshotted_at_launch() {
    let p = profile();
    let minute = CASTLE_FIGHT_SIMULATION_HZ as u64 * 60;
    for (index, &at) in p.level_after_minutes.iter().enumerate() {
        assert_eq!(p.level(u64::from(at) * minute - 1), index as u8);
        assert_eq!(p.level(u64::from(at) * minute), index as u8 + 1);
    }
    let mut sim = simulation();
    sim.next_tick = 4 * minute;
    sim.round_start_tick = 2 * minute;
    caster(&mut sim, p);
    target_building(&mut sim, 20, UnitClassifications::default());
    sim.step();
    assert!(sim.world.iter_entities().any(|e| matches!(e.get::<NativeAction>(), Some(NativeAction::BuildingAttackProxy(s)) if s.level == 1)));
}

#[test]
fn weapon_poison_can_stack_on_immune_structures_while_freezing_cannot() {
    let mut sim = simulation();
    let p = profile();
    let caster = caster(&mut sim, p);
    let target = target_building(
        &mut sim,
        8,
        UnitClassifications {
            spell_immune: true,
            ..Default::default()
        },
    );
    // The native parent cannot target spell immunity. Exercise the already committed callback.
    let buildings = sim.snapshot_buildings();
    let b = buildings.iter().find(|b| b.id == caster).unwrap();
    let source = AbilitySourceSnapshot {
        source: AbilitySourceIndex::Building(0),
        map_version: Some(p.map_version),
        id: caster,
        team: b.team,
        origin: AbilitySourceOrigin::Building(b.footprint),
        health: b.health,
        stunned_until_tick: 0,
        spellcasting: b.spellcasting,
        mana_current: b.mana_current,
        ability_state: b.ability_state,
    };
    assert!(sim.cast_building_attack_proxy(
        source,
        source.spellcasting.unwrap().ability,
        target,
        0,
        &buildings
    ));
    assert!(sim.cast_building_attack_proxy(
        source,
        source.spellcasting.unwrap().ability,
        target,
        1,
        &buildings
    ));
    for _ in 0..70 {
        sim.step();
    }
    let status = sim.building(target).unwrap().status;
    assert_eq!(status.frozen_until_tick, 0);
    assert_eq!(status.poison.unwrap().sources, 2);
    assert_eq!(
        status.poison.unwrap().total_damage_per_second,
        2 * p.levels[0].poison.damage_per_second
    );
}

#[test]
fn callback_selection_is_independent_of_the_native_trigger_draw() {
    let mut different = 0;
    for seed in 0..16 {
        let mut sim = simulation();
        sim.config.match_seed = seed;
        caster(&mut sim, profile());
        target_building(&mut sim, 8, UnitClassifications::default());
        target_building(&mut sim, 9, UnitClassifications::default());
        sim.step();
        let trigger = sim
            .world
            .iter_entities()
            .find_map(|e| match e.get::<NativeAction>() {
                Some(NativeAction::Parasite(s)) => Some(s.target),
                _ => None,
            })
            .unwrap();
        let effect = sim
            .world
            .iter_entities()
            .find_map(|e| match e.get::<NativeAction>() {
                Some(NativeAction::BuildingAttackProxy(s)) => Some(s.target),
                _ => None,
            })
            .unwrap();
        different += usize::from(trigger != effect);
    }
    assert!(different > 0 && different < 16);
}

#[test]
fn building_poison_remains_nonlethal_and_clears_after_its_final_pulse() {
    let mut sim = simulation();
    let mut p = profile();
    p.damage = 0;
    let caster = caster(&mut sim, p);
    let target = target_building(&mut sim, 8, UnitClassifications::default());
    sim.world
        .entity_mut(entity(&sim, target))
        .get_mut::<Health>()
        .unwrap()
        .current = 20;
    sim.step();
    sim.world.despawn(entity(&sim, caster));
    for _ in 0..60 {
        sim.step();
    }
    assert!(sim.building(target).unwrap().status.poison.is_some());
    let mut other = restored(&sim);
    for _ in 0..100 {
        sim.step();
        other.step();
        assert_eq!(sim.checksum(), other.checksum());
    }
    let building = sim.building(target).unwrap();
    assert_eq!(building.health, 1);
    assert!(building.status.poison.is_none());
}

#[test]
fn invisible_native_actor_has_flying_day_night_sight_until_its_exclusive_expiry() {
    let p = profile();
    let sight = crate::castle_fight_sight_for_version(p.map_version, p.dummy_rawcode).unwrap();
    for night in [false, true] {
        let mut config = SimulationConfig {
            navigation_max: NavCell::new(
                sight.day.max(sight.night) / SUBUNITS_PER_WORLD_UNIT + 100,
                500,
            ),
            ..Default::default()
        };
        config.fog = Some(crate::FogRules {
            cell_size: 16 * SUBUNITS_PER_WORLD_UNIT,
            fallback_sight: crate::SightProfile::default(),
            initially_explored: false,
            night,
            clock: None,
            attack_reveal: None,
            permanent_rectangles: [vec![], vec![]],
            sight_blockers: vec![(
                SimPoint::new(20 * SUBUNITS_PER_WORLD_UNIT, -10 * SUBUNITS_PER_WORLD_UNIT),
                SimPoint::new(30 * SUBUNITS_PER_WORLD_UNIT, 10 * SUBUNITS_PER_WORLD_UNIT),
            )],
        });
        let mut sim = Simulation::new(config, 1);
        let radius = sight.radius(night);
        let probe = SimPoint::new(radius / 2, 0);
        let id = sim.allocate_id();
        sim.world.spawn((
            id,
            NativeAction::NativeSpellActor {
                flying: true,
                owner: PlayerId(0),
                team: Team(0),
                map_version: p.map_version,
                rawcode: p.dummy_rawcode,
                position: SimPoint::new(0, 0),
                expires_tick: 3,
            },
        ));
        sim.refresh_vision();
        assert!(sim.fog.as_ref().unwrap().is_visible(Team(0), probe));
        assert!(!sim.fog.as_ref().unwrap().is_visible(Team(1), probe));
        assert!(!sim.fog.as_ref().unwrap().is_visible(
            Team(0),
            SimPoint::new(radius + 64 * SUBUNITS_PER_WORLD_UNIT, 0)
        ));
        let mut other = restored(&sim);
        for _ in 0..4 {
            sim.step();
            other.step();
            assert_eq!(sim.checksum(), other.checksum());
        }
        assert!(!sim.fog.as_ref().unwrap().is_visible(Team(0), probe));
    }
}
