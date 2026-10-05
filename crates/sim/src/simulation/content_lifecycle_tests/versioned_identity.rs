use super::*;
use crate::{MapVersion, SnapshotWireError};

fn constructing_simulation() -> (Simulation, SimId) {
    let content = castle_fight_content_bundle(CASTLE_FIGHT_DEFAULT_MAP_VERSION).unwrap();
    let definition = content.production_building_definitions().next().unwrap();
    let size = definition.footprint_size_cells;
    let mut simulation = Simulation::new(SimulationConfig::default(), 1);
    let id = simulation
        .try_start_building_construction(
            PlayerId(0),
            definition.spawn(Team(0), BuildingFootprint::new(12, 12, size, size)),
            definition.gameplay_properties(),
        )
        .unwrap();
    (simulation, id)
}

fn building_entity(simulation: &Simulation, id: SimId) -> Entity {
    simulation
        .world
        .iter_entities()
        .find(|entity| entity.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}

#[test]
fn changing_only_a_cold_identity_version_changes_the_checksum_before_activation() {
    for future_child in [false, true] {
        let (mut simulation, id) = constructing_simulation();
        let before = simulation.checksum();
        let entity = building_entity(&simulation, id);
        let mut construction = simulation
            .world
            .get_mut::<BuildingConstruction>(entity)
            .unwrap();
        let identity = if future_child {
            construction
                .properties
                .production_unit
                .content
                .as_mut()
                .unwrap()
        } else {
            construction.properties.content.as_mut().unwrap()
        };
        // Deliberately unsupported synthetic identity; it must not be normalized to the default.
        identity.map_version = MapVersion::new(99, 1);
        assert_ne!(simulation.checksum(), before);
        assert_eq!(simulation.unit_count(), 0);
    }
}

#[test]
fn wire_decoder_rejects_a_different_version_inside_a_cold_identity() {
    let content = castle_fight_content_bundle(CASTLE_FIGHT_DEFAULT_MAP_VERSION).unwrap();
    for future_child in [false, true] {
        let (mut simulation, id) = constructing_simulation();
        let entity = building_entity(&simulation, id);
        let mut construction = simulation
            .world
            .get_mut::<BuildingConstruction>(entity)
            .unwrap();
        let identity = if future_child {
            construction
                .properties
                .production_unit
                .content
                .as_mut()
                .unwrap()
        } else {
            construction.properties.content.as_mut().unwrap()
        };
        identity.map_version = MapVersion::new(99, 1);
        let encoded = simulation.capture_snapshot().encode_wire().unwrap();
        assert!(matches!(
            SimulationSnapshot::decode_wire(&encoded, content),
            Err(SnapshotWireError::ContentVersionMismatch { expected, actual })
                if expected == content.map_version && actual == MapVersion::new(99, 1)
        ));
    }
}

#[test]
fn live_content_version_is_hashed_independently_of_rawcode_and_display_name() {
    let content = castle_fight_content_bundle(CASTLE_FIGHT_DEFAULT_MAP_VERSION).unwrap();
    let definition = content.unit_definitions().next().unwrap();
    let mut simulation = Simulation::new(SimulationConfig::default(), 1);
    let position = SimPoint::new(
        12 * simulation.config.navigation_cell_size,
        12 * simulation.config.navigation_cell_size,
    );
    let id = simulation.spawn_unit_with_properties(
        UnitSpawn {
            team: Team(0),
            position,
            health: 100,
            attack: AttackProfile {
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
                delivery: AttackDelivery::Melee,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        },
        // This contract test needs source identity, not an imported body or its geometry.
        UnitGameplayProperties {
            content: definition.gameplay_properties().content,
            ..Default::default()
        },
    );
    let entity = building_entity(&simulation, id);
    let before = simulation.checksum();
    simulation
        .world
        .get_mut::<ContentIdentity>(entity)
        .unwrap()
        .name = "presentation only";
    assert_eq!(simulation.checksum(), before);
    simulation
        .world
        .get_mut::<ContentIdentity>(entity)
        .unwrap()
        .map_version = MapVersion::new(99, 1);
    assert_ne!(simulation.checksum(), before);
}

#[test]
fn activation_and_restoration_retain_carrier_and_regeneration_source_versions() {
    let content = castle_fight_content_bundle(CASTLE_FIGHT_DEFAULT_MAP_VERSION).unwrap();
    let mut native_states = 0;
    for definition in content.tower_definitions() {
        let size = definition.footprint_size_cells;
        let mut simulation = Simulation::new(SimulationConfig::default(), 1);
        let mut properties = definition.gameplay_properties();
        // Synthetic duration keeps this lifecycle test independent of map tuning.
        properties.construction_time_ticks = Some(1);
        simulation
            .try_start_building_construction(
                PlayerId(0),
                definition.spawn(Team(0), BuildingFootprint::new(12, 12, size, size)),
                properties,
            )
            .unwrap();
        let mut restored = wire_restore(&simulation, 4);
        for _ in 0..3 {
            assert_eq!(simulation.step().checksum, restored.step().checksum);
        }
        for entity in restored.world.iter_entities() {
            if let Some(state) = entity.get::<crate::native_carriers::NativeCarrierState>() {
                let version = match state {
                    crate::native_carriers::NativeCarrierState::Carrier { map_version, .. }
                    | crate::native_carriers::NativeCarrierState::Regeneration {
                        map_version,
                        ..
                    } => *map_version,
                    crate::native_carriers::NativeCarrierState::Bolt(bolt) => bolt.map_version,
                };
                assert_eq!(version, properties.content.unwrap().map_version);
                native_states += 1;
            }
        }
        assert_eq!(wire_restore(&restored, 2).checksum(), restored.checksum());
    }
    assert!(native_states > 0);
}

#[test]
fn standalone_native_timer_and_control_versions_are_checked_without_source_identities() {
    use crate::native_carriers::{NativeCarrierBolt, NativeCarrierState};
    use crate::simulation::building_spells::{
        BuildingSpellTargetState, ControlAction, ControlCallback,
    };
    let content = castle_fight_content_bundle(CASTLE_FIGHT_DEFAULT_MAP_VERSION).unwrap();
    let foreign = MapVersion::new(99, 1);
    for case in 0..5 {
        let mut simulation = Simulation::new(SimulationConfig::default(), 1);
        let id = simulation.allocate_id();
        if case < 3 {
            let state = match case {
                0 => NativeCarrierState::Carrier {
                    building: SimId(100),
                    owner: None,
                    team: Team(0),
                    position: SimPoint::new(0, 0),
                    map_version: foreign,
                    ready_tick: 10,
                    sequence: 0,
                },
                1 => NativeCarrierState::Regeneration {
                    building: SimId(100),
                    map_version: foreign,
                    per_second_per_10k: 0,
                    remainder: 0,
                },
                _ => NativeCarrierState::Bolt(NativeCarrierBolt {
                    source: SimId(100),
                    visual_source: SimId(100),
                    source_team: Team(0),
                    target: SimId(101),
                    ability: AbilityId(1),
                    map_version: foreign,
                    damage: 1,
                    attack_damage_type: None,
                    launch_position: SimPoint::new(0, 0),
                    launch_tick: 0,
                    position: SimPoint::new(0, 0),
                    position_tick: 0,
                    impact_tick: 10,
                }),
            };
            simulation.world.spawn((id, state));
        } else {
            let callbacks = if case == 4 {
                let AbilityEffect::Hex { mut profile } =
                    crate::building_mechanics::city_spellcasting_for_version(content.map_version)
                        .ability
                        .effect
                else {
                    unreachable!()
                };
                profile.map_version = foreign;
                vec![ControlCallback {
                    due_tick: 10,
                    action: ControlAction::Attack,
                    profile,
                }]
            } else {
                Vec::new()
            };
            simulation.world.spawn((
                id,
                BuildingSpellTargetState {
                    target: SimId(100),
                    version: if case == 3 {
                        foreign
                    } else {
                        content.map_version
                    },
                    shield_level: 0,
                    shield_expires_tick: None,
                    anti_negative: false,
                    selector_excluded: false,
                    overheat_level: 0,
                    hex: None,
                    defend_disabled: false,
                    orders_suspended: false,
                    callbacks,
                },
            ));
        }
        // Source handles may legitimately disappear before an autonomous action resolves.
        // Their absence must not conceal a foreign script/native timer version.
        let wire = simulation.capture_snapshot().encode_wire().unwrap();
        assert!(matches!(SimulationSnapshot::decode_wire(&wire, content),
            Err(SnapshotWireError::ContentVersionMismatch { expected, actual })
                if expected == content.map_version && actual == foreign));
    }
}
