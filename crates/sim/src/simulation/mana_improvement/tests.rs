use super::*;
use crate::{CastleFightProductionKind, CastleFightTowerKind, ManaProfile};

fn simulation(workers: usize) -> Simulation {
    Simulation::new(
        SimulationConfig {
            navigation_cell_size: 32 * SUBUNITS_PER_WORLD_UNIT,
            navigation_max: NavCell::new(512, 128),
            ..Default::default()
        },
        workers,
    )
}
fn tower(sim: &mut Simulation, kind: CastleFightTowerKind, x: i32, team: u8) -> SimId {
    let definition = kind.definition();
    sim.spawn_building_with_properties(
        definition.spawn(Team(team), BuildingFootprint::new(x, 0, 2, 2)),
        definition.gameplay_properties(),
    )
}
fn set_mana(sim: &mut Simulation, id: SimId, value: i32) {
    let entity = sim
        .world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id();
    sim.world.get_mut::<ManaState>(entity).unwrap().current = value;
    sim.world
        .get_mut::<AutomaticAbilityState>(entity)
        .unwrap()
        .autocast_enabled = false;
}
fn restored(sim: &Simulation) -> Simulation {
    let bundle = crate::castle_fight_content_bundle(crate::MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let encoded = sim.capture_snapshot().encode_wire().unwrap();
    let decoded = SimulationSnapshot::decode_wire(&encoded, bundle).unwrap();
    let mut restored = simulation(4);
    restored.restore_snapshot(&decoded).unwrap();
    restored
}

#[test]
fn manual_improvement_is_identity_scoped_permanent_and_survives_generator_death() {
    let mut sim = simulation(1);
    let source = tower(&mut sim, CastleFightTowerKind::ManaGenerator, 0, 0);
    let second = tower(&mut sim, CastleFightTowerKind::ManaGenerator, 4, 0);
    let target = tower(&mut sim, CastleFightTowerKind::ChillingMushroom, 8, 0);
    set_mana(&mut sim, target, 0);
    assert_eq!(
        sim.improve_building_for_player(PlayerId(1), source, target),
        Err(BuildingCommandError::NotAuthorized)
    );
    sim.improve_building_for_player(PlayerId(0), source, target)
        .unwrap();
    assert!(!sim.can_improve_building(source));
    assert_eq!(
        sim.improve_building_for_player(PlayerId(0), second, target),
        Err(BuildingCommandError::InvalidTargetType)
    );
    assert!(sim.can_improve_building(second));
    let grant = sim.building(target).unwrap().status.native_grants;
    assert_eq!(grant.iter().count(), 1);
    let link = sim
        .building(target)
        .unwrap()
        .status
        .mana_improvement_link
        .unwrap();
    assert!(sim.remove_building(source));
    assert_eq!(sim.building(target).unwrap().status.native_grants, grant);
    assert_eq!(
        sim.building(target).unwrap().status.mana_improvement_link,
        Some(link)
    );
    let mut copy = restored(&sim);
    for _ in 0..600 {
        assert_eq!(
            {
                sim.step();
                sim.checksum()
            },
            {
                copy.step();
                copy.checksum()
            }
        );
    }
    assert!(sim.building(target).unwrap().mana_current.unwrap() > 0);
}

#[test]
fn invalid_targets_do_not_consume_the_manual_ability_and_target_death_restores_it() {
    let mut sim = simulation(1);
    let source = tower(&mut sim, CastleFightTowerKind::ManaGenerator, 0, 0);
    let no_mana = tower(&mut sim, CastleFightTowerKind::WatchTower, 4, 0);
    let hostile = tower(&mut sim, CastleFightTowerKind::ChillingMushroom, 8, 1);
    let far = tower(&mut sim, CastleFightTowerKind::ChillingMushroom, 100, 0);
    let production = CastleFightProductionKind::RockPile.definition();
    let produced = sim.spawn_building_with_properties(
        production.spawn(Team(0), BuildingFootprint::new(12, 0, 2, 2)),
        production.gameplay_properties(),
    );
    for target in [source, no_mana, hostile, produced] {
        assert_eq!(
            sim.improve_building_for_player(PlayerId(0), source, target),
            Err(BuildingCommandError::InvalidTargetType)
        );
        assert!(sim.can_improve_building(source));
    }
    assert_eq!(
        sim.improve_building_for_player(PlayerId(0), source, far),
        Err(BuildingCommandError::TargetOutOfRange)
    );
    let target = tower(&mut sim, CastleFightTowerKind::ChillingMushroom, 14, 0);
    sim.improve_building_for_player(PlayerId(0), source, target)
        .unwrap();
    assert!(sim.remove_building(target));
    assert!(sim.can_improve_building(source));
    assert_eq!(
        sim.building(source).unwrap().status.mana_improvement_target,
        None
    );
    sim.improve_building_for_player(PlayerId(0), source, no_mana)
        .unwrap_err();
    assert!(sim.can_improve_building(source));
}

#[test]
fn permanent_improvement_is_retained_across_upgrade_completion_and_cancellation() {
    for (grant_during_upgrade, cancel) in
        [(false, false), (false, true), (true, false), (true, true)]
    {
        let mut sim = simulation(1);
        let source = tower(&mut sim, CastleFightTowerKind::ManaGenerator, 0, 0);
        let definition = CastleFightTowerKind::ChillingMushroom.definition();
        let footprint = BuildingFootprint::new(4, 0, 2, 2);
        let building = definition.spawn(Team(0), footprint);
        let properties = BuildingGameplayProperties {
            content: None,
            construction_time_ticks: Some(3),
            economy: Some(BuildingEconomyProfile {
                gold_cost: 0,
                lumber_cost: 0,
                lumber_refund: 0,
                legendary_points_cost: 0,
                income_per_10k: 0,
            }),
            ..definition.gameplay_properties()
        };
        let target = sim.spawn_building_with_properties(building, properties);
        if !grant_during_upgrade {
            sim.improve_building_for_player(PlayerId(0), source, target)
                .unwrap();
        }
        sim.start_building_upgrade_as(
            PlayerId(0),
            target,
            building,
            properties,
            building,
            properties,
        )
        .unwrap();
        if grant_during_upgrade {
            // Native max mana exists while the runtime spell component is suspended.
            let entity = sim
                .world
                .iter_entities()
                .find(|e| e.get::<SimId>() == Some(&target))
                .unwrap()
                .id();
            sim.world
                .entity_mut(entity)
                .insert(definition.gameplay_properties().content.unwrap());
            sim.improve_building_for_player(PlayerId(0), source, target)
                .unwrap();
            sim.world.entity_mut(entity).remove::<ContentIdentity>();
        }
        let status = sim.building(target).unwrap().status;
        let mut other = restored(&sim);
        if cancel {
            sim.cancel_building_construction_for_player(PlayerId(0), target)
                .unwrap();
            other
                .cancel_building_construction_for_player(PlayerId(0), target)
                .unwrap();
        }
        for _ in 0..5 {
            assert_eq!(
                {
                    sim.step();
                    sim.checksum()
                },
                {
                    other.step();
                    other.checksum()
                }
            );
        }
        let after = sim.building(target).unwrap();
        assert!(after.construction_complete_tick.is_none());
        assert_eq!(after.status.native_grants, status.native_grants);
        assert_eq!(
            after.status.mana_improvement_link,
            status.mana_improvement_link
        );
        assert!(!sim.can_improve_building(source));
    }
}

#[test]
fn permanent_regeneration_accumulates_fractional_mana_exactly_after_restore() {
    let mut sim = simulation(1);
    let definition = CastleFightTowerKind::ChillingMushroom.definition();
    let mut building = definition.spawn(Team(0), BuildingFootprint::new(0, 0, 2, 2));
    building.spellcasting.as_mut().unwrap().mana = ManaProfile::per_second(1000, 0, 0);
    let id = sim.spawn_building(building);
    set_mana(&mut sim, id, 0);
    let entity = sim
        .world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id();
    let mut status = StatusState::default();
    status.native_grants.insert(crate::NativeAbilityGrant {
        ability: AbilityId(91),
        effect: crate::NativeAbilityGrantEffect::ManaRegeneration(17_500),
    });
    sim.world.entity_mut(entity).insert(status);
    for _ in 0..30 {
        sim.step();
    }
    assert_eq!(sim.building(id).unwrap().mana_current, Some(1));
    let mut other = restored(&sim);
    for _ in 0..30 {
        assert_eq!(
            {
                sim.step();
                sim.checksum()
            },
            {
                other.step();
                other.checksum()
            }
        );
    }
    assert_eq!(sim.building(id).unwrap().mana_current, Some(3));
    assert_eq!(
        sim.world
            .get::<ManaState>(entity)
            .unwrap()
            .regen_remainder_per_10k,
        5000
    );
}
