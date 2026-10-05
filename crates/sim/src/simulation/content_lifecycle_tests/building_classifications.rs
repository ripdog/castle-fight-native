use super::*;

fn flags() -> [UnitClassifications; 9] {
    [
        UnitClassifications {
            hero: true,
            ..Default::default()
        },
        UnitClassifications {
            summoned: true,
            ..Default::default()
        },
        UnitClassifications {
            spell_immune: true,
            ..Default::default()
        },
        UnitClassifications {
            combat_sapper: true,
            ..Default::default()
        },
        UnitClassifications {
            invulnerable: true,
            ..Default::default()
        },
        UnitClassifications {
            legendary: true,
            ..Default::default()
        },
        UnitClassifications {
            summoned_marker: true,
            ..Default::default()
        },
        UnitClassifications {
            illusion: true,
            ..Default::default()
        },
        UnitClassifications {
            invisible: true,
            ..Default::default()
        },
    ]
}

fn building() -> BuildingSpawn {
    BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(10, 10, 1, 1),
        health: 100,
        production: None,
        attack: None,
        spellcasting: None,
    }
}

fn entity(sim: &Simulation, id: SimId) -> Entity {
    sim.world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}

fn live_flags(sim: &Simulation, id: SimId) -> UnitClassifications {
    sim.world
        .get::<UnitClassifications>(entity(sim, id))
        .copied()
        .unwrap_or_default()
}

#[test]
fn each_building_classification_is_hashed_and_restored_in_live_and_cold_state() {
    let mut baseline = Simulation::new(SimulationConfig::default(), 1);
    baseline.spawn_building(building());
    for flags in flags() {
        let mut live = Simulation::new(SimulationConfig::default(), 1);
        let id = live.spawn_building_with_properties(
            building(),
            BuildingGameplayProperties {
                classifications: flags,
                ..Default::default()
            },
        );
        assert_ne!(live.checksum(), baseline.checksum(), "{flags:?}");
        let mut restored = wire_restore(&live, 4);
        assert_eq!(live_flags(&restored, id), flags);
        assert_eq!(live.step().checksum, restored.step().checksum);

        let mut cold = Simulation::new(SimulationConfig::default(), 1);
        let id = cold
            .try_start_building_construction(
                PlayerId(0),
                building(),
                BuildingGameplayProperties {
                    construction_time_ticks: Some(3),
                    ..Default::default()
                },
            )
            .unwrap();
        let before = cold.checksum();
        let e = entity(&cold, id);
        cold.world
            .get_mut::<BuildingConstruction>(e)
            .unwrap()
            .properties
            .classifications = flags;
        assert_eq!(live_flags(&cold, id), UnitClassifications::default());
        assert_ne!(cold.checksum(), before, "cold-only flag {flags:?}");
        let mut restored = wire_restore(&cold, 4);
        for _ in 0..4 {
            assert_eq!(cold.step().checksum, restored.step().checksum);
        }
        assert_eq!(live_flags(&restored, id), flags);
    }
}

#[test]
fn upgrade_replaces_flags_but_cancellation_restores_the_actual_precursor_runtime() {
    let declared_source = UnitClassifications {
        legendary: true,
        ..Default::default()
    };
    let acquired_source = UnitClassifications {
        invisible: true,
        ..declared_source
    };
    let target_flags = UnitClassifications {
        invulnerable: true,
        ..Default::default()
    };
    let source_properties = BuildingGameplayProperties {
        classifications: declared_source,
        ..Default::default()
    };
    let target_properties = BuildingGameplayProperties {
        classifications: target_flags,
        construction_time_ticks: Some(3),
        economy: Some(BuildingEconomyProfile {
            gold_cost: 0,
            lumber_cost: 0,
            lumber_refund: 0,
            legendary_points_cost: 0,
            income_per_10k: 0,
        }),
        ..Default::default()
    };
    for cancel in [false, true] {
        let mut sim = Simulation::new(SimulationConfig::default(), 1);
        let id = sim.spawn_building_with_properties(building(), source_properties);
        let e = entity(&sim, id);
        sim.world.entity_mut(e).insert(acquired_source);
        sim.start_building_upgrade_as(
            PlayerId(0),
            id,
            building(),
            source_properties,
            building(),
            target_properties,
        )
        .unwrap();
        assert_eq!(live_flags(&sim, id), target_flags);
        let before = sim.checksum();
        // Both the saved cold baseline and independently changed live state matter.
        sim.world
            .get_mut::<BuildingConstruction>(e)
            .unwrap()
            .upgrade_from
            .as_mut()
            .unwrap()
            .runtime
            .classifications
            .hero = true;
        assert_ne!(sim.checksum(), before);
        sim.world
            .get_mut::<BuildingConstruction>(e)
            .unwrap()
            .upgrade_from
            .as_mut()
            .unwrap()
            .runtime
            .classifications
            .hero = false;
        assert_eq!(sim.checksum(), before);
        sim.world
            .get_mut::<BuildingConstruction>(e)
            .unwrap()
            .upgrade_from
            .as_mut()
            .unwrap()
            .properties
            .classifications
            .hero = true;
        assert_ne!(sim.checksum(), before);
        sim.world
            .get_mut::<BuildingConstruction>(e)
            .unwrap()
            .upgrade_from
            .as_mut()
            .unwrap()
            .properties
            .classifications
            .hero = false;
        assert_eq!(sim.checksum(), before);
        let mut restored = wire_restore(&sim, 4);
        if cancel {
            sim.cancel_building_construction_for_player(PlayerId(0), id)
                .unwrap();
            restored
                .cancel_building_construction_for_player(PlayerId(0), id)
                .unwrap();
            assert_eq!(sim.checksum(), restored.checksum());
            assert_eq!(live_flags(&restored, id), acquired_source);
        } else {
            for _ in 0..4 {
                assert_eq!(sim.step().checksum, restored.step().checksum);
            }
            assert_eq!(live_flags(&restored, id), target_flags);
        }
        let rejoined = wire_restore(&restored, 2);
        assert_eq!(live_flags(&rejoined, id), live_flags(&restored, id));
    }
}
