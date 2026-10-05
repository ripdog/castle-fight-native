use super::*;
use crate::SnapshotWireError;

fn pending_entity(sim: &Simulation) -> Entity {
    sim.world
        .iter_entities()
        .find(|entity| entity.contains::<DelayedShrineRevival>())
        .unwrap()
        .id()
}

#[test]
fn delayed_support_version_hashes_and_wire_rejects_mismatch_without_replacement_identity() {
    let mut sim = ready_sim();
    let pending = pending_entity(&sim);
    let callback = sim.world.get::<DelayedShrineRevival>(pending).unwrap();
    assert!(callback.definition.properties.content.is_none());
    assert_eq!(
        callback.map_version,
        sim.golden_shrine_support(callback.team)
            .unwrap()
            .map_version
    );
    let before = sim.checksum();
    let foreign = MapVersion::new(99, 1);
    sim.world
        .get_mut::<DelayedShrineRevival>(pending)
        .unwrap()
        .map_version = foreign;
    assert_ne!(before, sim.checksum());
    let content = castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    assert!(
        matches!(SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), content),
        Err(SnapshotWireError::ContentVersionMismatch { expected, actual })
            if expected == content.map_version && actual == foreign)
    );
}

#[test]
fn support_version_survives_building_removal_and_wire_continuation_for_unlabelled_replacement() {
    let mut sim = ready_sim();
    let callback = *sim
        .world
        .get::<DelayedShrineRevival>(pending_entity(&sim))
        .unwrap();
    let supporting = sim
        .buildings()
        .into_iter()
        .find(|building| building.content.is_some())
        .unwrap()
        .id;
    assert!(sim.remove_building(supporting));
    assert_eq!(sim.golden_shrine_revive_chance(callback.team), 0);
    let mut wire_restored = restored(&sim);
    while sim.next_tick <= callback.due_tick {
        assert_eq!(sim.step().checksum, wire_restored.step().checksum);
    }
    assert_eq!(sim.unit_count(), 1);
    assert_eq!(
        sim.shrine_revivals_last_tick()[0].model_path,
        shrine_definition(callback.map_version).resurrection_model
    );
    assert_eq!(
        wire_restored.shrine_revivals_last_tick(),
        sim.shrine_revivals_last_tick()
    );
}

#[test]
fn live_or_original_version_mismatch_cannot_silently_use_support_tuning() {
    for original_only in [false, true] {
        let mut sim = simulation(1, 0);
        shrine(&mut sim, Team(0), 20);
        let id = unit(&mut sim, definition());
        let unit_entity = entity(&sim, id);
        let mut foreign = crate::CastleFightUnitKind::ALL[0]
            .definition()
            .gameplay_properties()
            .content
            .unwrap();
        foreign.map_version = MapVersion::new(99, 1);
        if original_only {
            sim.world
                .get_mut::<ResurrectionProfile>(unit_entity)
                .unwrap()
                .0
                .properties
                .content = Some(foreign);
        } else {
            sim.world.entity_mut(unit_entity).insert(foreign);
        }
        let support = sim.golden_shrine_support(Team(0));
        let next_id = sim.next_id;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            sim.schedule_shrine_revival(
                unit_entity,
                PlayerId(0),
                Team(0),
                SimPoint::new(50, 11),
                0,
                support,
            );
        }));
        assert!(result.is_err());
        assert_eq!(pending(&sim), 0);
        assert_eq!(sim.next_id, next_id);
    }
}

#[test]
fn unsupported_support_identity_is_not_normalized_by_count_or_owner_transfer() {
    for transfer in [false, true] {
        let mut sim = simulation(1, 0);
        let id = shrine(&mut sim, Team(0), 20);
        sim.world
            .get_mut::<ContentIdentity>(entity(&sim, id))
            .unwrap()
            .map_version = MapVersion::new(99, 1);
        let next_id = sim.next_id;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if transfer {
                assert!(sim.transfer_golden_shrine_owner(id, PlayerId(1)));
            } else {
                assert!(sim.golden_shrine_revive_chance(Team(0)) > 0);
            }
        }));
        assert!(result.is_err());
        assert_eq!(sim.next_id, next_id);
        assert_eq!(
            sim.world.get::<Owner>(entity(&sim, id)).unwrap().0,
            PlayerId(0)
        );
    }
}

#[test]
fn native_activation_rejects_foreign_identity_before_any_unqualified_counter_or_regeneration() {
    for kind in [
        CastleFightTowerKind::Gjallarhorn,
        CastleFightTowerKind::GoldenShrineOfJustice,
    ] {
        let definition = kind
            .definition_for_version(MapVersion::CASTLE_FIGHT_9_27)
            .unwrap();
        let mut sim = simulation(1, 0);
        let mut properties = definition.gameplay_properties();
        properties.economy = None;
        properties.construction_time_ticks = Some(1);
        properties.content.as_mut().unwrap().map_version = MapVersion::new(99, 1);
        let size = definition.footprint_size_cells;
        let id = sim
            .try_start_building_construction(
                PlayerId(0),
                definition.spawn(Team(0), BuildingFootprint::new(20, 20, size, size)),
                properties,
            )
            .unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for _ in 0..3 {
                sim.step();
            }
        }));
        assert!(result.is_err());
        assert_eq!(sim.gjallarhorn_constructed_count, [0, 0]);
        assert!(
            sim.world
                .get::<HealthRegeneration>(entity(&sim, id))
                .is_none()
        );
    }
}
