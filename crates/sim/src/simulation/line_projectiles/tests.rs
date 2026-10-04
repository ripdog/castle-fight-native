use super::*;

fn simulation(workers: usize) -> Simulation {
    Simulation::new(
        SimulationConfig {
            navigation_cell_size: 10,
            spatial_cell_size: 40,
            unit_separation_distance: 0,
            max_separation_per_tick: 0,
            ..SimulationConfig::default()
        },
        workers,
    )
}
fn line() -> AttackDelivery {
    AttackDelivery::Line {
        speed_per_tick: 10,
        minimum_range: 20,
        spill_distance: 40,
        spill_radius: 5,
        damage_retention_per_10k: 5000,
        spill_targets: AttackTargetMask::GROUND_UNITS,
    }
}
fn spawn(
    sim: &mut Simulation,
    team: u8,
    x: i32,
    y: i32,
    delivery: AttackDelivery,
    damage: i32,
) -> SimId {
    sim.spawn_unit(UnitSpawn {
        team: Team(team),
        position: SimPoint::new(x, y),
        health: 1000,
        attack: AttackProfile {
            delivery,
            damage,
            range: 100,
            acquisition_range: 120,
            cooldown_ticks: 1000,
        },
        movement: MovementProfile { speed_per_tick: 0 },
    })
}
fn inert(sim: &mut Simulation, team: u8, x: i32, y: i32) -> SimId {
    spawn(sim, team, x, y, AttackDelivery::Melee, 0)
}
fn advance_to(sim: &mut Simulation, tick: u64) {
    while sim.next_tick <= tick {
        sim.step();
    }
}
fn restore(sim: &Simulation, workers: usize) -> Simulation {
    let content = crate::castle_fight_content_bundle(crate::MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let wire = sim.capture_snapshot().encode_wire().unwrap();
    let snapshot = SimulationSnapshot::decode_wire(&wire, content).unwrap();
    let mut restored = simulation(workers);
    restored.restore_snapshot(&snapshot).unwrap();
    assert_eq!(restored.checksum(), sim.checksum());
    restored
}
fn pending(sim: &mut Simulation) -> LineProjectile {
    let mut query = sim.world.query::<&LineProjectile>();
    query.single(&sim.world).unwrap().clone()
}

#[test]
fn directed_spill_has_separate_masks_timing_collision_order_and_wire_continuation() {
    let mut sim = simulation(1);
    spawn(&mut sim, 0, 20, 20, line(), 80);
    let main = inert(&mut sim, 1, 60, 20);
    sim.step();
    sim.step();
    let primary_tick = pending(&mut sim).primary_impact_tick;
    // Spawn after launch so these fixtures do not replace the selected primary target.
    let before = inert(&mut sim, 1, 55, 20);
    let behind = inert(&mut sim, 1, 75, 20);
    let far = inert(&mut sim, 1, 95, 20);
    let side = inert(&mut sim, 1, 75, 26);
    let past_end = inert(&mut sim, 1, 101, 20);
    let ally = inert(&mut sim, 0, 75, 20);
    let air = sim.spawn_unit_with_properties(
        UnitSpawn {
            team: Team(1),
            position: SimPoint::new(75, 20),
            health: 1000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1000,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        },
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            ..UnitGameplayProperties::default()
        },
    );
    let building = sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(8, 2, 1, 1),
        health: 1000,
        production: None,
        attack: None,
        spellcasting: None,
    });
    let mut restored = restore(&sim, 4);
    advance_to(&mut sim, primary_tick - 1);
    assert_eq!(sim.unit(main).unwrap().health, 1000, "not early");
    advance_to(&mut sim, primary_tick);
    assert_eq!(sim.unit(main).unwrap().health, 920);
    assert_eq!(
        sim.unit(behind).unwrap().health,
        1000,
        "spill is not instantaneous radial splash"
    );
    let mut spill_restored = restore(&sim, 2);
    advance_to(&mut sim, primary_tick + 1);
    assert_eq!(sim.unit(behind).unwrap().health, 1000);
    advance_to(&mut sim, primary_tick + 2);
    assert_eq!(sim.unit(behind).unwrap().health, 960);
    advance_to(&mut sim, primary_tick + 4);
    assert_eq!(sim.unit(far).unwrap().health, 980);
    for id in [before, side, past_end, ally, air] {
        assert_eq!(sim.unit(id).unwrap().health, 1000);
    }
    assert_eq!(sim.building(building).unwrap().health, 1000);
    assert_eq!(sim.projectile_count(), 0);
    advance_to(&mut restored, primary_tick + 4);
    advance_to(&mut spill_restored, primary_tick + 4);
    assert_eq!(sim.checksum(), restored.checksum());
    assert_eq!(sim.checksum(), spill_restored.checksum());
}

#[test]
fn collision_order_is_longitudinal_then_stable_id_and_a_victim_is_hit_once() {
    let mut sim = simulation(1);
    spawn(&mut sim, 0, 20, 20, line(), 80);
    inert(&mut sim, 1, 60, 20);
    sim.step();
    sim.step();
    let tick = pending(&mut sim).primary_impact_tick;
    let farther_lower_id = inert(&mut sim, 1, 69, 20);
    let nearer_higher_id = inert(&mut sim, 1, 65, 20);
    let tied_higher_id = inert(&mut sim, 1, 65, 21);
    advance_to(&mut sim, tick + 1);
    assert_eq!(sim.unit(nearer_higher_id).unwrap().health, 960);
    assert_eq!(sim.unit(tied_higher_id).unwrap().health, 980);
    assert_eq!(sim.unit(farther_lower_id).unwrap().health, 990);
    advance_to(&mut sim, tick + 4);
    assert_eq!(sim.unit(nearer_higher_id).unwrap().health, 960);
}

#[test]
fn minimum_range_is_inclusive_and_excludes_acquisition_and_final_intents() {
    assert_eq!(line().stable_tag(), 5);
    for distance in [19, 20, 100, 101] {
        let mut sim = simulation(1);
        spawn(&mut sim, 0, 20, 20, line(), 80);
        inert(&mut sim, 1, 20 + distance, 20);
        sim.step();
        sim.step();
        assert_eq!(
            sim.projectile_count(),
            usize::from((20..=100).contains(&distance))
        );
    }
}

#[test]
fn minimum_range_revalidates_forced_final_intents() {
    for distance in [19, 20] {
        let mut sim = simulation(1);
        let source = spawn(&mut sim, 0, 20, 20, line(), 80);
        let target = inert(&mut sim, 1, 20 + distance, 20);
        sim.step();
        let entity = sim
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&source))
            .unwrap()
            .id();
        sim.world
            .entity_mut(entity)
            .get_mut::<TargetState>()
            .unwrap()
            .current = Some(target);
        let units = sim.snapshot_units();
        let buildings = sim.snapshot_buildings();
        assert_eq!(
            sim.attack_intents(&units, &buildings)
                .iter()
                .any(|intent| intent.source_id == source),
            distance == 20
        );
    }
}

#[test]
fn projectile_sim_id_order_not_ecs_iteration_decides_competing_primary_hits() {
    let mut sim = simulation(1);
    spawn(&mut sim, 0, 20, 20, line(), 100);
    spawn(&mut sim, 0, 20, 20, line(), 80);
    let primary = inert(&mut sim, 1, 60, 20);
    let entity = sim
        .world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&primary))
        .unwrap()
        .id();
    sim.world
        .entity_mut(entity)
        .get_mut::<Health>()
        .unwrap()
        .current = 100;
    sim.step();
    sim.step();
    let mut query = sim.world.query::<(Entity, &SimId, &LineProjectile)>();
    let mut pending: Vec<_> = query
        .iter(&sim.world)
        .map(|(entity, id, p)| (entity, *id, p.clone()))
        .collect();
    assert_eq!(pending.len(), 2);
    let tick = pending[0].2.primary_impact_tick;
    pending.sort_unstable_by_key(|(_, id, _)| std::cmp::Reverse(*id));
    let checksum = sim.checksum();
    for (entity, _, _) in &pending {
        sim.world.despawn(*entity);
    }
    for (_, id, p) in pending {
        sim.world.spawn((id, p));
    }
    assert_eq!(sim.checksum(), checksum);
    let behind = inert(&mut sim, 1, 75, 20);
    let mut restored = restore(&sim, 4);
    advance_to(&mut sim, tick + 4);
    advance_to(&mut restored, tick + 4);
    assert_eq!(
        sim.unit(behind).unwrap().health,
        950,
        "first projectile kills primary; second invalidates instead of spilling"
    );
    assert_eq!(sim.checksum(), restored.checksum());
}

#[test]
fn missing_primary_cancels_spill_but_dead_source_does_not() {
    for remove_primary in [false, true] {
        let mut sim = simulation(1);
        let source = spawn(&mut sim, 0, 20, 20, line(), 80);
        let primary = inert(&mut sim, 1, 60, 20);
        sim.step();
        sim.step();
        let tick = pending(&mut sim).primary_impact_tick;
        let behind = inert(&mut sim, 1, 75, 20);
        let removed = if remove_primary { primary } else { source };
        let entity = sim
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&removed))
            .unwrap()
            .id();
        sim.world.despawn(entity);
        advance_to(&mut sim, tick + 4);
        assert_eq!(
            sim.unit(behind).unwrap().health,
            if remove_primary { 1000 } else { 960 }
        );
    }
}

#[test]
fn spill_building_mask_uses_footprint_intersection_not_center_or_primary_mask() {
    let mut sim = simulation(1);
    let AttackDelivery::Line {
        speed_per_tick,
        minimum_range,
        spill_distance,
        spill_radius,
        damage_retention_per_10k,
        ..
    } = line()
    else {
        unreachable!()
    };
    spawn(
        &mut sim,
        0,
        20,
        20,
        AttackDelivery::Line {
            speed_per_tick,
            minimum_range,
            spill_distance,
            spill_radius,
            damage_retention_per_10k,
            spill_targets: AttackTargetMask::BUILDINGS,
        },
        80,
    );
    inert(&mut sim, 1, 60, 20);
    sim.step();
    sim.step();
    let tick = pending(&mut sim).primary_impact_tick;
    let ground = inert(&mut sim, 1, 75, 14);
    let building = sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(7, 2, 1, 2),
        health: 1000,
        production: None,
        attack: None,
        spellcasting: None,
    });
    advance_to(&mut sim, tick + 1);
    assert_eq!(sim.building(building).unwrap().health, 960);
    assert_eq!(sim.unit(ground).unwrap().health, 1000);
}

#[test]
fn primary_tracks_its_target_but_spill_does_not_track_later_movement() {
    let mut sim = simulation(1);
    spawn(&mut sim, 0, 20, 20, line(), 80);
    let primary = inert(&mut sim, 1, 60, 20);
    sim.step();
    sim.step();
    let tick = pending(&mut sim).primary_impact_tick;
    let entity = sim
        .world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&primary))
        .unwrap()
        .id();
    sim.world
        .entity_mut(entity)
        .get_mut::<Position>()
        .unwrap()
        .0 = SimPoint::new(60, 60);
    advance_to(&mut sim, tick);
    let state = pending(&mut sim);
    assert_eq!(state.spill_origin, Some(SimPoint::new(60, 60)));
    let on_ray = inert(&mut sim, 1, 70, 70);
    let stale_aim = inert(&mut sim, 1, 75, 20);
    sim.world
        .entity_mut(entity)
        .get_mut::<Position>()
        .unwrap()
        .0 = SimPoint::new(60, 20);
    advance_to(&mut sim, tick + 2);
    assert_eq!(sim.unit(on_ray).unwrap().health, 960);
    assert_eq!(sim.unit(stale_aim).unwrap().health, 1000);
}

#[test]
fn source_art_identity_survives_death_wire_restore_and_canonical_hashing() {
    let mut sim = simulation(1);
    let source = spawn(&mut sim, 0, 20, 20, line(), 80);
    let entity = sim
        .world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&source))
        .unwrap()
        .id();
    let rawcode = crate::CastleFightUnitKind::Ballista.definition().rawcode;
    sim.world.entity_mut(entity).insert(ContentIdentity {
        rawcode,
        name: "synthetic source",
    });
    inert(&mut sim, 1, 60, 20);
    sim.step();
    sim.step();
    let tick = pending(&mut sim).primary_impact_tick;
    assert_eq!(pending(&mut sim).source_rawcode, Some(rawcode));
    sim.world.despawn(entity);
    let mut restored = restore(&sim, 4);
    assert_eq!(pending(&mut restored).source_rawcode, Some(rawcode));
    assert!(matches!(restored.projectiles()[0].kind,
        ProjectileViewKind::Line { source_rawcode: Some(code), .. } if code == rawcode));
    let original = restored.checksum();
    let mut query = restored.world.query::<&mut LineProjectile>();
    query
        .single_mut(&mut restored.world)
        .unwrap()
        .source_rawcode = None;
    assert_ne!(original, restored.checksum());
    query
        .single_mut(&mut restored.world)
        .unwrap()
        .source_rawcode = Some(rawcode);
    advance_to(&mut sim, tick + 4);
    advance_to(&mut restored, tick + 4);
    assert_eq!(sim.checksum(), restored.checksum());
}

#[test]
fn moving_victims_cannot_reenter_a_passed_sweep_or_take_duplicate_damage() {
    let mut sim = simulation(1);
    spawn(&mut sim, 0, 20, 20, line(), 80);
    inert(&mut sim, 1, 60, 20);
    sim.step();
    sim.step();
    let tick = pending(&mut sim).primary_impact_tick;
    let early = inert(&mut sim, 1, 65, 20);
    let late = inert(&mut sim, 1, 95, 40);
    advance_to(&mut sim, tick + 1);
    assert_eq!(sim.unit(early).unwrap().health, 960);
    for (id, point) in [
        (early, SimPoint::new(85, 20)),
        (late, SimPoint::new(65, 20)),
    ] {
        let entity = sim
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&id))
            .unwrap()
            .id();
        sim.world
            .entity_mut(entity)
            .get_mut::<Position>()
            .unwrap()
            .0 = point;
    }
    advance_to(&mut sim, tick + 4);
    assert_eq!(sim.unit(early).unwrap().health, 960);
    assert_eq!(
        sim.unit(late).unwrap().health,
        1000,
        "passed strip is not a lingering zone"
    );
}

#[test]
fn diagonal_strip_and_footprint_intersections_do_not_become_circles() {
    let source = SimPoint::new(0, 0);
    let origin = SimPoint::new(100, 100);
    assert!(line_point_hit(source, origin, SimPoint::new(107, 107), 0, 10, 2).is_some());
    assert!(line_point_hit(source, origin, SimPoint::new(108, 108), 0, 10, 2).is_none());
    assert!(line_point_hit(source, origin, SimPoint::new(105, 100), 0, 10, 2).is_none());
    assert!(line_point_hit(source, origin, SimPoint::new(99, 99), 0, 10, 10).is_none());
    assert!(
        line_footprint_hit(
            source,
            origin,
            BuildingFootprint::new(10, 10, 1, 1),
            10,
            0,
            10,
            2
        )
        .is_some()
    );
    assert!(
        line_footprint_hit(
            source,
            origin,
            BuildingFootprint::new(11, 10, 1, 1),
            10,
            0,
            10,
            2
        )
        .is_none()
    );
    assert!(
        line_footprint_hit(
            source,
            origin,
            BuildingFootprint::new(9, 9, 1, 1),
            10,
            1,
            10,
            2
        )
        .is_none()
    );
}
