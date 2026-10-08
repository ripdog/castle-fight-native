use super::*;
use crate::BurningOilEffectProfile;

fn profile() -> BurningOilEffectProfile {
    BurningOilEffectProfile {
        ability: AbilityId(94),
        radius: 100 * SUBUNITS_PER_WORLD_UNIT,
        full_damage: 11,
        full_interval_millis: 1000,
        half_damage: 3,
        half_interval_millis: 1000,
        full_duration_millis: 1000,
        total_duration_millis: 2000,
        target_ground_units: true,
        target_buildings: true,
    }
}

fn zone(sim: &mut Simulation, profile: BurningOilEffectProfile) -> SimId {
    let source = sim.spawn_unit(unit(0, 300, 0, AttackDelivery::Melee));
    let id = sim.allocate_id();
    sim.world.spawn((
        id,
        BurningOilZone {
            source,
            source_team: Team(0),
            center: unit(0, 100, 0, AttackDelivery::Melee).position,
            profile,
            created_tick: sim.tick(),
            pulse_index: 1,
        },
    ));
    sim.world.despawn(entity(sim, source));
    id
}

#[test]
fn burning_oil_samples_live_ground_targets_and_preserves_expiry_pulse_after_recovery() {
    let mut sim = simulation(1);
    let mut targets = Vec::new();
    for (team, x, movement_class, mechanical, immune, invulnerable, affected) in [
        (1, 100, MovementClass::Ground, false, false, false, true),
        (1, 100, MovementClass::Ground, true, false, false, true),
        (1, 200, MovementClass::Ground, false, false, false, true),
        (1, 201, MovementClass::Ground, false, false, false, false),
        (0, 100, MovementClass::Ground, false, false, false, false),
        (1, 100, MovementClass::Air, false, false, false, false),
        (1, 100, MovementClass::Ground, false, true, false, false),
        (1, 100, MovementClass::Ground, false, false, true, false),
    ] {
        let id = sim.spawn_unit_with_properties(
            unit(team, x, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                movement_class,
                mechanical,
                build_time_ticks: mechanical.then_some(13),
                classifications: UnitClassifications {
                    spell_immune: immune,
                    invulnerable,
                    ..Default::default()
                },
                collision_radius: Some(CollisionRadius(0)),
                ..Default::default()
            },
        );
        targets.push((id, affected));
    }
    let zone = zone(&mut sim, profile());
    let affected: Vec<_> = targets
        .iter()
        .filter_map(|&(id, affected)| affected.then_some(id))
        .collect();
    assert_eq!(sim.burning_oil_zones()[0].recipients, affected);
    while sim.tick() < 30 {
        sim.step();
    }
    assert!(
        targets
            .iter()
            .all(|&(id, _)| sim.unit(id).unwrap().health == 1000)
    );
    sim.step();
    for &(id, affected) in &targets {
        assert_eq!(
            sim.unit(id).unwrap().health,
            1000 - if affected { 11 } else { 0 }
        );
    }
    let mut restored = wire_restored(&sim, 4);
    while sim.tick() <= 60 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    for &(id, affected) in &targets {
        assert_eq!(
            sim.unit(id).unwrap().health,
            1000 - if affected { 14 } else { 0 }
        );
    }
    assert!(
        !sim.world
            .iter_entities()
            .any(|entity| entity.get::<SimId>() == Some(&zone))
    );
    assert!(sim.burning_oil_zones().is_empty());
    sim.step();
    restored.step();
    assert_eq!(sim.checksum(), restored.checksum());
}

#[test]
fn burning_oil_structure_mask_and_live_protection_are_independent_of_unit_targets() {
    for target_buildings in [false, true] {
        let mut sim = simulation(1);
        let unit = sim.spawn_unit(unit(1, 100, 0, AttackDelivery::Melee));
        let mut buildings = Vec::new();
        for (index, (team, immune, invulnerable, affected)) in [
            (1, false, false, true),
            (0, false, false, false),
            (1, true, false, false),
            (1, false, true, false),
        ]
        .into_iter()
        .enumerate()
        {
            let id = sim.spawn_building_with_properties(
                BuildingSpawn {
                    team: Team(team),
                    footprint: BuildingFootprint::new(6 + index as i32 * 2, 1, 1, 1),
                    health: 1000,
                    production: None,
                    attack: None,
                    spellcasting: None,
                },
                BuildingGameplayProperties {
                    classifications: UnitClassifications {
                        spell_immune: immune,
                        invulnerable,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
            buildings.push((id, affected));
        }
        zone(
            &mut sim,
            BurningOilEffectProfile {
                target_ground_units: false,
                target_buildings,
                ..profile()
            },
        );
        while sim.tick() <= 60 {
            sim.step();
        }
        assert_eq!(sim.unit(unit).unwrap().health, 1000);
        for (id, affected) in buildings {
            assert_eq!(
                sim.building(id).unwrap().health,
                1000 - if affected && target_buildings { 14 } else { 0 }
            );
        }
    }
}
