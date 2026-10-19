//! Catalog-wide lifecycle coverage: identities must survive before any runtime child exists.
//! Expected definitions are read from the selected bundle, never duplicated map tuning.

use super::*;
use crate::{
    CASTLE_FIGHT_DEFAULT_MAP_VERSION, CastleFightProductionKind, castle_fight_content_bundle,
};

mod building_classifications;
mod versioned_identity;

fn wire_restore(original: &Simulation, workers: usize) -> Simulation {
    let content = castle_fight_content_bundle(CASTLE_FIGHT_DEFAULT_MAP_VERSION).unwrap();
    let encoded = original.capture_snapshot().encode_wire().unwrap();
    let decoded = SimulationSnapshot::decode_wire(&encoded, content).unwrap();
    let mut restored = Simulation::new(original.config.clone(), workers);
    restored.restore_snapshot(&decoded).unwrap();
    assert_eq!(original.checksum(), restored.checksum());
    restored
}

#[test]
fn catalog_construction_round_trips_before_activation_and_continues_across_workers() {
    let content = castle_fight_content_bundle(CASTLE_FIGHT_DEFAULT_MAP_VERSION).unwrap();
    let definitions = content
        .production_building_definitions()
        .map(|definition| {
            let size = definition.footprint_size_cells;
            (
                definition.spawn(Team(0), BuildingFootprint::new(12, 12, size, size)),
                definition.gameplay_properties(),
            )
        })
        .chain(content.tower_definitions().map(|definition| {
            let size = definition.footprint_size_cells;
            (
                definition.spawn(Team(0), BuildingFootprint::new(12, 12, size, size)),
                definition.gameplay_properties(),
            )
        }));
    for (building, properties) in definitions {
        let mut original = Simulation::new(SimulationConfig::default(), 1);
        let id = original
            .try_start_building_construction(PlayerId(0), building, properties)
            .unwrap();
        let mut restored = wire_restore(&original, 4);
        assert_eq!(restored.building(id).unwrap().content, properties.content);
        let entity = restored
            .world
            .iter_entities()
            .find(|entity| entity.get::<SimId>() == Some(&id))
            .unwrap();
        assert_eq!(
            entity
                .get::<UnitClassifications>()
                .copied()
                .unwrap_or_default(),
            properties.classifications
        );
        let construction = entity.get::<BuildingConstruction>().unwrap();
        assert_eq!(construction.properties, properties);
        assert_eq!(
            serde_json::to_value(construction.building).unwrap(),
            serde_json::to_value(building).unwrap()
        );

        let duration = properties.construction_time_ticks.unwrap();
        for _ in 0..=duration + 2 {
            assert_eq!(
                {
                    original.step();
                    original.checksum()
                },
                {
                    restored.step();
                    restored.checksum()
                }
            );
        }
        assert!(
            restored
                .building(id)
                .unwrap()
                .construction_complete_tick
                .is_none()
        );
        let entity = restored
            .world
            .iter_entities()
            .find(|entity| entity.get::<SimId>() == Some(&id))
            .unwrap();
        assert_eq!(
            entity
                .get::<UnitClassifications>()
                .copied()
                .unwrap_or_default(),
            properties.classifications
        );
        let after_activation = wire_restore(&restored, 2);
        assert_eq!(restored.checksum(), after_activation.checksum());
    }
}

#[test]
fn catalog_upgrade_wire_restore_and_cancellation_preserve_the_precursor_definition() {
    let content = castle_fight_content_bundle(CASTLE_FIGHT_DEFAULT_MAP_VERSION).unwrap();
    for source_kind in CastleFightProductionKind::ALL
        .into_iter()
        .map(crate::CastleFightBuildingKind::Production)
        .chain(
            crate::CastleFightTowerKind::ALL
                .into_iter()
                .map(crate::CastleFightBuildingKind::Tower),
        )
    {
        for target_kind in source_kind
            .upgrade_targets_for_version(content.map_version)
            .unwrap()
        {
            let size = source_kind.footprint_size_cells(content).unwrap();
            let footprint = BuildingFootprint::new(12, 12, size, size);
            let (source_building, source_properties) =
                building_fixture(content, source_kind, footprint);
            let (target_building, target_properties) =
                building_fixture(content, target_kind, footprint);
            let mut original = Simulation::new(SimulationConfig::default(), 1);
            original.debug_grant_player_resources_for(PlayerId(0), u32::MAX / 2, u32::MAX / 2);
            let id = original.spawn_building_for_player_with_properties(
                PlayerId(0),
                source_building,
                source_properties,
            );
            // This lifecycle fixture is about an idle, completed precursor, not queue timing.
            let entity = original
                .world
                .iter_entities()
                .find(|entity| entity.get::<SimId>() == Some(&id))
                .unwrap()
                .id();
            if let Some(mut state) = original.world.get_mut::<ProductionState>(entity) {
                state.queued = 0;
            }
            let resources = &mut original.player_state_mut(PlayerId(0)).unwrap().resources;
            resources.legendary_points_cap = u16::MAX;
            resources.legendary_points_used =
                source_properties.economy.unwrap().legendary_points_cost;
            let precursor_checksum = original.checksum();
            let precursor_resources = original.player_resources_for(PlayerId(0)).unwrap();
            original
                .start_building_upgrade_as(
                    PlayerId(0),
                    id,
                    source_building,
                    source_properties,
                    target_building,
                    target_properties,
                )
                .unwrap();
            let mut restored = wire_restore(&original, 4);
            assert_eq!(
                restored.building(id).unwrap().content.unwrap().rawcode,
                target_properties.content.unwrap().rawcode
            );
            original
                .cancel_building_construction_for_player(PlayerId(0), id)
                .unwrap();
            restored
                .cancel_building_construction_for_player(PlayerId(0), id)
                .unwrap();
            assert_eq!(original.checksum(), restored.checksum());
            assert_eq!(restored.checksum(), precursor_checksum);
            assert_eq!(
                restored.player_resources_for(PlayerId(0)),
                Some(precursor_resources)
            );
            assert_eq!(
                restored.building(id).unwrap().content,
                source_properties.content
            );
            for _ in 0..3 {
                assert_eq!(
                    {
                        original.step();
                        original.checksum()
                    },
                    {
                        restored.step();
                        restored.checksum()
                    }
                );
            }
        }
    }
}

fn building_fixture(
    content: &crate::CastleFightContentBundle,
    kind: crate::CastleFightBuildingKind,
    footprint: BuildingFootprint,
) -> (BuildingSpawn, BuildingGameplayProperties) {
    match kind {
        crate::CastleFightBuildingKind::Production(kind) => {
            let definition = content.production_building(kind).unwrap();
            (
                definition.spawn(Team(0), footprint),
                definition.gameplay_properties(),
            )
        }
        crate::CastleFightBuildingKind::Tower(kind) => {
            let definition = content.tower(kind).unwrap();
            (
                definition.spawn(Team(0), footprint),
                definition.gameplay_properties(),
            )
        }
    }
}

#[test]
fn catalog_type_caps_reserve_pending_upgrades_and_release_on_cancellation_or_death() {
    let content = castle_fight_content_bundle(CASTLE_FIGHT_DEFAULT_MAP_VERSION).unwrap();
    for source_kind in crate::CastleFightTowerKind::ALL {
        for target_kind in source_kind
            .upgrade_targets_for_version(content.map_version)
            .unwrap()
        {
            let source = content.tower(source_kind).unwrap();
            let target = content.tower(target_kind).unwrap();
            let Some(limit) = crate::building_mechanics::building_type_limit_for_version(
                content.map_version,
                target.rawcode,
            ) else {
                continue;
            };
            let mut sim = Simulation::new(SimulationConfig::default(), 1);
            sim.debug_grant_player_resources_for(PlayerId(0), u32::MAX / 2, u32::MAX / 2);
            let size = source.footprint_size_cells;
            let first_footprint = BuildingFootprint::new(12, 12, size, size);
            let second_footprint = BuildingFootprint::new(20, 12, size, size);
            let first = sim.spawn_building_for_player_with_properties(
                PlayerId(0),
                source.spawn(Team(0), first_footprint),
                source.gameplay_properties(),
            );
            let second = sim.spawn_building_for_player_with_properties(
                PlayerId(0),
                source.spawn(Team(0), second_footprint),
                source.gameplay_properties(),
            );
            for n in 1..limit {
                let footprint =
                    BuildingFootprint::new(12 + i32::from(n) * i32::from(size + 2), 20, size, size);
                sim.spawn_building_for_player_with_properties(
                    PlayerId(0),
                    target.spawn(Team(0), footprint),
                    target.gameplay_properties(),
                );
            }
            let resources = &mut sim.player_state_mut(PlayerId(0)).unwrap().resources;
            resources.legendary_points_used = source.economy.legendary_points_cost * 2
                + target.economy.legendary_points_cost * (limit - 1);
            resources.legendary_points_cap = resources.legendary_points_used;
            assert!(sim.can_afford_building_upgrade_for_player(PlayerId(0), first, target.economy));
            assert!(!sim.can_afford_building_for_player(PlayerId(0), target.economy));
            sim.start_building_upgrade_as(
                PlayerId(0),
                first,
                source.spawn(Team(0), first_footprint),
                source.gameplay_properties(),
                target.spawn(Team(0), first_footprint),
                target.gameplay_properties(),
            )
            .unwrap();
            assert!(!sim.building_type_limit_available_for_player(
                PlayerId(0),
                content.map_version,
                target.rawcode
            ));
            assert!(sim.building_type_limit_available_for_player(
                PlayerId(1),
                content.map_version,
                target.rawcode
            ));
            let mut restored = wire_restore(&sim, 4);
            let before = sim.checksum();
            assert_eq!(
                sim.start_building_upgrade_as(
                    PlayerId(0),
                    second,
                    source.spawn(Team(0), second_footprint),
                    source.gameplay_properties(),
                    target.spawn(Team(0), second_footprint),
                    target.gameplay_properties()
                ),
                Err(BuildingUpgradeError::TypeLimitReached)
            );
            assert_eq!(sim.checksum(), before);
            for simulation in [&mut sim, &mut restored] {
                simulation
                    .cancel_building_construction_for_player(PlayerId(0), first)
                    .unwrap();
                assert!(simulation.building_type_limit_available_for_player(
                    PlayerId(0),
                    content.map_version,
                    target.rawcode
                ));
                simulation
                    .start_building_upgrade_as(
                        PlayerId(0),
                        second,
                        source.spawn(Team(0), second_footprint),
                        source.gameplay_properties(),
                        target.spawn(Team(0), second_footprint),
                        target.gameplay_properties(),
                    )
                    .unwrap();
            }
            for _ in 0..=target.construction_time_ticks {
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
            assert!(!sim.building_type_limit_available_for_player(
                PlayerId(0),
                content.map_version,
                target.rawcode
            ));
            let entity = sim
                .world
                .iter_entities()
                .find(|e| e.get::<SimId>() == Some(&second))
                .unwrap()
                .id();
            sim.world.get_mut::<Health>(entity).unwrap().current = 0;
            assert!(sim.building_type_limit_available_for_player(
                PlayerId(0),
                content.map_version,
                target.rawcode
            ));
        }
    }
}
