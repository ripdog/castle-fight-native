use super::*;
use crate::{EntanglingRootsEffectProfile, ManaProfile, NativeBoltProfile};

fn world(n: i32) -> i32 {
    n * SUBUNITS_PER_WORLD_UNIT
}
fn profile() -> WorldFreezerProfile {
    let bolt = NativeBoltProfile {
        ability: AbilityId(103),
        damage: 17,
        stun_ticks: 8,
        hero_stun_ticks: 2,
        damage_per_second: 0,
        duration_ticks: 0,
        speed_per_tick: world(5),
        cleanse: false,
        targets: AttackTargetMask::AIR_AND_GROUND,
    };
    WorldFreezerProfile {
        map_version: crate::MapVersion::CASTLE_FIGHT_9_27,
        parent: AbilityId(101),
        dummy_rawcode: u32::from_be_bytes(*b"h08U"),
        trigger_targets: AttackTargetMask::GROUND_AND_BUILDINGS,
        trigger_invulnerable: true,
        trigger_spell_immune: false,
        delay_ticks: 3,
        angle_offsets: [-45, 0, 45],
        interval_millis: 40,
        step: world(4),
        bounce_step: world(8),
        horizontal_bounds: [world(-100), world(300)],
        vertical_bounds: [world(-50), world(50)],
        counter_limit: 2,
        target_radius: world(200),
        aura: AbilityId(102),
        aura_radius: world(100),
        aura_targets: AttackTargetMask::AIR_AND_GROUND,
        movement_percent_delta: -20,
        attack_speed_percent_delta: -10,
        fire_buff: 100,
        air_buff: 100,
        ground_buff: 100,
        fire: NativeBoltProfile {
            damage: 0,
            stun_ticks: 0,
            hero_stun_ticks: 0,
            duration_ticks: 1,
            ..bolt
        },
        alternate_fire: NativeBoltProfile {
            targets: AttackTargetMask::AIR_AND_GROUND,
            ..bolt
        },
        fire_radius: 0,
        fire_cooldown_millis: 250,
        air: bolt,
        ground: NativeBoltProfile {
            ability: AbilityId(104),
            damage: 0,
            stun_ticks: 3,
            hero_stun_ticks: 3,
            ..bolt
        },
        roots: EntanglingRootsEffectProfile {
            ability: AbilityId(105),
            damage_per_second: 7,
            duration_ticks: 30,
            hero_duration_ticks: 5,
            nonhero_only: true,
            targets: AttackTargetMask::AIR_AND_GROUND,
        },
        vision_radius: world(10),
        vision_ticks: 3,
    }
}
fn simulation() -> Simulation {
    Simulation::new(
        SimulationConfig {
            navigation_max: NavCell::new(500, 100),
            ..SimulationConfig::default()
        },
        1,
    )
}
fn caster(sim: &mut Simulation, p: WorldFreezerProfile) -> SimId {
    caster_at(sim, p, 0)
}
fn caster_at(sim: &mut Simulation, p: WorldFreezerProfile, x: i32) -> SimId {
    sim.spawn_building(BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(x, 0, 1, 1),
        health: 1000,
        production: None,
        attack: None,
        spellcasting: Some(SpellcastingProfile {
            mana: ManaProfile {
                maximum: 10,
                starting: 10,
                regen_per_tick_per_10k: 0,
            },
            ability: AutomaticAbilityProfile {
                id: p.parent,
                mana_cost: 6,
                cooldown_ticks: 1000,
                range: world(1000),
                target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
                effect: AbilityEffect::WorldFreezer(p),
            },
        }),
    })
}
fn victim(
    sim: &mut Simulation,
    x: i32,
    movement_class: MovementClass,
    flags: UnitClassifications,
) -> SimId {
    sim.spawn_unit_with_properties(
        UnitSpawn {
            team: Team(1),
            position: SimPoint::new(world(x), world(20)),
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
            movement_class,
            classifications: flags,
            ..UnitGameplayProperties::default()
        },
    )
}
fn trigger(sim: &mut Simulation) {
    sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(8, 0, 1, 1),
        health: 1000,
        production: None,
        attack: None,
        spellcasting: None,
    });
}
fn entity(sim: &Simulation, id: SimId) -> Entity {
    sim.world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}
fn state(sim: &Simulation) -> &WorldFreezerState {
    sim.world
        .iter_entities()
        .find_map(|e| e.get::<WorldFreezerState>())
        .unwrap()
}
fn restore(sim: &Simulation, workers: usize) -> Simulation {
    let content = crate::castle_fight_content_bundle(crate::MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), content)
            .unwrap();
    let mut restored = Simulation::new(sim.config.clone(), workers);
    restored.restore_snapshot(&snapshot).unwrap();
    restored
}

#[test]
fn delayed_movers_survive_source_death_and_share_an_exact_fractional_timer_after_rejoin() {
    let mut sim = simulation();
    let source = caster(&mut sim, profile());
    trigger(&mut sim);
    sim.step();
    assert_eq!(sim.building(source).unwrap().mana_current, Some(4));
    assert!(state(&sim).orbs.is_empty());
    sim.world.despawn(entity(&sim, source));
    let mut restored = restore(&sim, 4);
    for _ in 0..2 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
        assert!(state(&sim).orbs.is_empty());
    }
    sim.step();
    restored.step();
    assert_eq!(sim.checksum(), restored.checksum());
    assert_eq!(state(&sim).orbs.len(), 3);
    assert_eq!(sim.projectile_count(), sim.projectiles().len());
    let start = state(&sim).orbs[1].position();
    assert_eq!(state(&sim).timer_origin_tick, 3);
    for _ in 0..6 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert_eq!(state(&sim).timer_step, 6);
    assert_eq!(state(&sim).orbs[1].position().x, start.x + world(20));
    assert_eq!(
        sim.projectiles()
            .iter()
            .filter(|p| matches!(p.kind, ProjectileViewKind::NativeMover { .. }))
            .count(),
        3
    );
}

#[test]
fn native_trigger_spends_resources_without_an_effect_candidate_but_air_cannot_replace_ground_trigger()
 {
    for ground_trigger in [false, true] {
        let mut sim = simulation();
        let source = caster(&mut sim, profile());
        victim(
            &mut sim,
            60,
            MovementClass::Air,
            UnitClassifications {
                combat_sapper: true,
                ..UnitClassifications::default()
            },
        );
        if ground_trigger {
            trigger(&mut sim);
        }
        sim.step();
        assert_eq!(
            sim.building(source).unwrap().mana_current,
            Some(if ground_trigger { 4 } else { 10 })
        );
        assert_eq!(
            sim.world
                .iter_entities()
                .any(|e| e.get::<WorldFreezerState>().is_some()),
            ground_trigger
        );
    }
}

#[test]
fn effect_selector_and_native_children_separate_air_hero_roots_immunity_and_shields() {
    for (movement, hero, immune, invulnerable, sapper, shield) in [
        (MovementClass::Air, false, false, false, true, 0),
        (MovementClass::Air, true, false, false, true, 0),
        (MovementClass::Ground, false, false, false, true, 2),
        (MovementClass::Ground, true, false, false, true, 0),
        (MovementClass::Ground, false, true, false, true, 0),
        (MovementClass::Ground, false, false, true, true, 0),
        (MovementClass::Ground, false, false, false, false, 0),
    ] {
        let mut sim = simulation();
        caster(&mut sim, profile());
        trigger(&mut sim);
        let id = victim(
            &mut sim,
            70,
            movement,
            UnitClassifications {
                combat_sapper: sapper,
                hero,
                spell_immune: immune,
                invulnerable,
                ..UnitClassifications::default()
            },
        );
        if shield > 0 {
            assert!(sim.set_negative_building_shield(id, shield, None));
        }
        while sim.tick() < 8 {
            sim.step();
        }
        let u = sim.unit(id).unwrap();
        let selected = sapper && !invulnerable && !immune;
        if selected && movement == MovementClass::Air {
            assert_eq!(u.health, 949);
            assert_eq!(
                u.status.native_stun_until_tick,
                7 + if hero { 2 } else { 8 }
            );
            assert_eq!(u.status.native_stun_ability, Some(AbilityId(103)));
        } else {
            assert_eq!(u.health, 1000);
        }
        assert_eq!(
            u.status.rooted_until_tick > 0,
            selected && movement == MovementClass::Ground && !hero
        );
        if selected && movement == MovementClass::Ground {
            assert_eq!(u.status.native_stun_until_tick, 10);
        }
        assert_eq!(u.status.movement_modifier_count > 0, !invulnerable); // The hostile aura includes non-sappers and magic immunity.
        if shield > 0 {
            assert_eq!(u.negative_building_shield_level, shield);
        }
        let mut restored = restore(&sim, 4);
        for _ in 0..4 {
            sim.step();
            restored.step();
            assert_eq!(sim.checksum(), restored.checksum());
        }
    }
}

#[test]
fn mover_reflects_with_authored_bounce_step_and_removal_processes_swapped_successor() {
    let mut sim = simulation();
    let mut p = profile();
    let initial = footprint_center_point(
        BuildingFootprint::new(0, 0, 1, 1),
        sim.config.navigation_cell_size,
    );
    p.vertical_bounds = [initial.y - world(20), initial.y + world(4)];
    p.horizontal_bounds = [initial.x - world(20), initial.x + world(10)];
    p.counter_limit = 1000;
    caster(&mut sim, p);
    trigger(&mut sim);
    while sim.tick() < 7 {
        sim.step();
    }
    let s = state(&sim);
    assert_eq!(s.orbs.len(), 3);
    assert_eq!(s.orbs[2].facing_degrees, 315);
    let initial = footprint_center_point(
        BuildingFootprint::new(0, 0, 1, 1),
        sim.config.navigation_cell_size,
    );
    assert!(s.orbs[2].position().y < initial.y); // Reflection advances by bounce_step, never clamps to the wall.
    while sim.tick() < 8 {
        sim.step();
    }
    let s = state(&sim);
    assert_eq!(s.orbs.len(), 2);
    assert!(s.orbs.iter().all(|o| o.counter == 3));
}

#[test]
fn ambient_fire_is_independent_homing_spell_damage_and_preserves_fractional_cooldown() {
    let mut sim = simulation();
    let mut p = profile();
    p.fire_radius = world(60);
    p.fire.damage = 11;
    p.fire.targets = AttackTargetMask::AIR_AND_GROUND;
    p.counter_limit = 1000;
    caster(&mut sim, p);
    trigger(&mut sim);
    let target = victim(
        &mut sim,
        40,
        MovementClass::Ground,
        UnitClassifications::default(),
    );
    while sim.tick() < 4 {
        sim.step();
    }
    assert_eq!(
        state(&sim)
            .orbs
            .iter()
            .map(|o| o.fire_sequence)
            .sum::<u64>(),
        3
    );
    let mut restored = restore(&sim, 4);
    for _ in 0..16 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert!(sim.unit(target).unwrap().health < 1000);
    assert!(
        sim.unit(target)
            .unwrap()
            .status
            .native_stun_ability
            .is_none()
    );
    assert!(state(&sim).orbs.iter().all(|o| o.fire_sequence == 3));
    assert!(state(&sim).orbs.iter().all(|o| o.fire_due_time == 25_500));
}

#[test]
fn overlapping_auras_refresh_one_identity_and_expire_after_the_movers_leave() {
    let mut sim = simulation();
    caster(&mut sim, profile());
    trigger(&mut sim);
    let id = victim(
        &mut sim,
        70,
        MovementClass::Ground,
        UnitClassifications::default(),
    );
    while sim.tick() < 5 {
        sim.step();
    }
    let u = sim.unit(id).unwrap();
    assert_eq!(u.status.movement_modifier_count, 1);
    assert_eq!(u.status.attack_speed_modifier_count, 1);
    assert_eq!(u.status.movement_modifiers[0].percent_delta, -20);
    sim.clear_world_freezer();
    sim.step();
    sim.step();
    assert_eq!(sim.unit(id).unwrap().status.movement_modifier_count, 0);
}

#[test]
fn native_fire_buff_identity_blocks_ambient_orders_until_expiry_and_restores() {
    for shared in [true, false] {
        let mut sim = simulation();
        let mut p = profile();
        p.fire.ability = AbilityId(106);
        p.fire_radius = world(60);
        p.counter_limit = 1000;
        if !shared {
            p.air_buff += 1;
        }
        caster(&mut sim, p);
        trigger(&mut sim);
        let target = victim(
            &mut sim,
            40,
            MovementClass::Ground,
            UnitClassifications::default(),
        );
        let e = entity(&sim, target);
        {
            let mut status = sim.world.get_mut::<StatusState>(e).unwrap();
            status.native_stun_ability = Some(p.air.ability);
            status.native_stun_until_tick = 12;
        }
        while sim.tick() < 6 {
            sim.step();
        }
        let shots = state(&sim)
            .orbs
            .iter()
            .map(|o| o.fire_sequence)
            .sum::<u64>();
        assert_eq!(shots == 0, shared);
        let mut restored = restore(&sim, 4);
        for _ in 0..10 {
            sim.step();
            restored.step();
            assert_eq!(sim.checksum(), restored.checksum());
        }
        assert!(state(&sim).orbs.iter().any(|o| o.fire_sequence > 0));
    }
}

#[test]
fn mover_wire_rejects_a_foreign_source_version() {
    let mut sim = simulation();
    caster(&mut sim, profile());
    trigger(&mut sim);
    sim.step();
    let e = sim
        .world
        .iter_entities()
        .find(|e| e.get::<WorldFreezerState>().is_some())
        .unwrap()
        .id();
    sim.world
        .get_mut::<WorldFreezerState>(e)
        .unwrap()
        .profile
        .map_version
        .minor += 1;
    let content = crate::castle_fight_content_bundle(crate::MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let error =
        SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), content)
            .unwrap_err();
    assert!(matches!(
        error,
        SnapshotWireError::ContentVersionMismatch { .. }
    ));
}

#[test]
fn a_second_cast_joins_the_existing_timer_and_preserves_array_order_after_rejoin() {
    let mut sim = simulation();
    let p = profile();
    caster(&mut sim, p);
    trigger(&mut sim);
    while sim.tick() < 6 {
        sim.step();
    }
    let origin = state(&sim).timer_origin_tick;
    caster_at(&mut sim, p, 2);
    sim.step();
    let due = state(&sim).pending.last().unwrap().due_tick;
    let mut restored = restore(&sim, 4);
    while sim.tick() <= due {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    let s = state(&sim);
    assert_eq!(s.timer_origin_tick, origin);
    assert_eq!(s.orbs.len(), 6);
    assert!(s.orbs[..3].iter().all(|o| o.born_tick < due));
    assert!(
        s.orbs[3..]
            .iter()
            .all(|o| o.born_tick == due && o.counter == 1)
    );
    assert_eq!(sim.projectile_count(), sim.projectiles().len());
}
