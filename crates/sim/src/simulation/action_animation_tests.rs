use super::*;

fn configuration() -> SimulationConfig {
    SimulationConfig {
        navigation_cell_size: 32,
        navigation_min: NavCell::new(-16, -16),
        navigation_max: NavCell::new(16, 16),
        team_objective: [SimPoint::new(400, 0), SimPoint::new(-400, 0)],
        max_separation_per_tick: 2,
        ..SimulationConfig::default()
    }
}

fn spawn(team: Team, position: SimPoint) -> UnitSpawn {
    UnitSpawn {
        team,
        position,
        health: 5,
        attack: AttackProfile {
            delivery: AttackDelivery::Melee,
            damage: 10,
            range: 48,
            acquisition_range: 160,
            cooldown_ticks: 10,
        },
        movement: MovementProfile { speed_per_tick: 8 },
    }
}

fn properties(movement_class: MovementClass) -> UnitGameplayProperties {
    UnitGameplayProperties {
        movement_class,
        collision_radius: Some(CollisionRadius(8)),
        action_timing: ActionTimingProfile {
            primary_attack_ticks: 3,
            secondary_attack_ticks: 5,
            cast_ticks: 4,
        },
        ..UnitGameplayProperties::default()
    }
}

fn target(sim: &mut Simulation, movement_class: MovementClass) -> SimId {
    let mut unit = spawn(Team(1), SimPoint::new(40, 0));
    unit.movement.speed_per_tick = 0;
    unit.attack.acquisition_range = 0;
    unit.attack.range = 0;
    unit.attack.damage = 0;
    sim.spawn_unit_with_properties(unit, properties(movement_class))
}

fn wire_restore(sim: &Simulation) -> Simulation {
    let content =
        crate::castle_fight_content_bundle(crate::CASTLE_FIGHT_DEFAULT_MAP_VERSION).unwrap();
    let wire = sim.capture_snapshot().encode_wire().unwrap();
    let snapshot = SimulationSnapshot::decode_wire(&wire, content).unwrap();
    let mut restored = Simulation::new(configuration(), 4);
    restored.restore_snapshot(&snapshot).unwrap();
    assert_eq!(sim.checksum(), restored.checksum());
    restored
}

#[test]
fn attack_animation_anchors_ground_and_air_units_after_target_death_through_wire_restore() {
    for class in [MovementClass::Ground, MovementClass::Air] {
        let mut sim = Simulation::new(configuration(), 1);
        let source =
            sim.spawn_unit_with_properties(spawn(Team(0), SimPoint::new(0, 0)), properties(class));
        let victim = target(&mut sim, MovementClass::Ground);
        assert_eq!(sim.step().attacks_resolved, 0);
        let origin = sim.unit(source).unwrap().position;
        assert_eq!(sim.step().attacks_resolved, 1);
        assert!(sim.unit(victim).is_none());
        let action = sim.unit(source).unwrap().status.action_animation.unwrap();
        assert_eq!(action.kind, ActionAnimationKind::Attack);
        assert_eq!(action.started_tick, 1);
        assert_eq!(action.until_tick, 4);
        assert_eq!(
            sim.unit(source).unwrap().position,
            origin,
            "the killing attack tick must not slide"
        );
        let mut restored = wire_restore(&sim);
        for _ in 2..4 {
            assert_eq!(sim.step().checksum, restored.step().checksum);
            assert_eq!(sim.unit(source).unwrap().position, origin);
        }
        assert_eq!(sim.step().checksum, restored.step().checksum);
        assert!(
            sim.unit(source).unwrap().position.x > origin.x,
            "movement must resume on the exclusive expiry tick"
        );
        assert!(sim.unit(source).unwrap().status.action_animation.is_none());
    }
}

#[test]
fn secondary_attack_uses_its_own_animation_duration() {
    let mut sim = Simulation::new(configuration(), 1);
    let source_spawn = spawn(Team(0), SimPoint::new(0, 0));
    let mut props = properties(MovementClass::Ground);
    props.secondary_attack = Some(SecondaryAttackProfile {
        primary_targets: AttackTargetMask::GROUND_AND_BUILDINGS,
        attack: source_spawn.attack,
        targets: AttackTargetMask::AIR_UNITS,
        damage_type: DamageType::Normal,
    });
    let source = sim.spawn_unit_with_properties(source_spawn, props);
    target(&mut sim, MovementClass::Air);
    sim.step();
    sim.step();
    let action = sim.unit(source).unwrap().status.action_animation.unwrap();
    assert_eq!(
        action.until_tick - action.started_tick,
        5,
        "identical attack primitives must still retain attack-slot timing"
    );
}

fn spellcasting(starting_mana: i32) -> SpellcastingProfile {
    SpellcastingProfile {
        mana: crate::ManaProfile {
            maximum: 10,
            starting: starting_mana,
            regen_per_tick_per_10k: 0,
        },
        ability: AutomaticAbilityProfile {
            id: AbilityId(1),
            mana_cost: 1,
            cooldown_ticks: 10,
            range: 160,
            target_policy: AbilityTargetPolicy::RandomEnemyUnit,
            effect: AbilityEffect::Damage { amount: 10 },
        },
    }
}

#[test]
fn successful_cast_anchors_until_expiry_but_failed_cast_does_not() {
    for mana in [0, 10] {
        let mut sim = Simulation::new(configuration(), 1);
        let mut unit = spawn(Team(0), SimPoint::new(0, 0));
        unit.attack.range = 0;
        unit.attack.acquisition_range = 0;
        let source = sim.spawn_unit_with_properties_and_spellcasting(
            unit,
            properties(MovementClass::Ground),
            spellcasting(mana),
        );
        target(&mut sim, MovementClass::Ground);
        let tick = sim.step();
        if mana == 0 {
            assert_eq!(tick.ability_casts, 0);
            assert!(sim.unit(source).unwrap().position.x > 0);
            assert!(sim.unit(source).unwrap().status.action_animation.is_none());
            continue;
        }
        assert_eq!(tick.ability_casts, 1);
        assert_eq!(sim.unit(source).unwrap().position, SimPoint::new(0, 0));
        assert_eq!(
            sim.unit(source)
                .unwrap()
                .status
                .action_animation
                .unwrap()
                .kind,
            ActionAnimationKind::Cast
        );
        let mut restored = wire_restore(&sim);
        for _ in 1..4 {
            assert_eq!(sim.step().checksum, restored.step().checksum);
            assert_eq!(sim.unit(source).unwrap().position, SimPoint::new(0, 0));
        }
        assert_eq!(sim.step().checksum, restored.step().checksum);
        assert!(sim.unit(source).unwrap().position.x > 0);
    }
}

#[test]
fn ordered_spell_preempts_attack_recovery_without_opening_a_movement_tick() {
    let mut sim = Simulation::new(configuration(), 1);
    let source = sim.spawn_unit_with_properties_and_spellcasting(
        spawn(Team(0), SimPoint::new(0, 0)),
        properties(MovementClass::Ground),
        SpellcastingProfile {
            ability: AutomaticAbilityProfile {
                effect: AbilityEffect::Damage { amount: 1 },
                ..spellcasting(10).ability
            },
            ..spellcasting(10)
        },
    );
    let victim = target(&mut sim, MovementClass::Ground);
    let units = sim.snapshot_units();
    let source_entity = units.iter().find(|unit| unit.id == source).unwrap().entity;
    let victim_entity = units.iter().find(|unit| unit.id == victim).unwrap().entity;
    sim.world.get_mut::<Health>(victim_entity).unwrap().current = 100;
    sim.world.get_mut::<Health>(victim_entity).unwrap().max = 100;
    sim.world
        .get_mut::<AutomaticAbilityState>(source_entity)
        .unwrap()
        .ready_tick = 2;
    sim.step();
    assert_eq!(sim.step().attacks_resolved, 1);
    assert_eq!(
        sim.unit(source)
            .unwrap()
            .status
            .action_animation
            .unwrap()
            .kind,
        ActionAnimationKind::Attack
    );
    let cast = sim.step();
    assert_eq!(
        cast.ability_casts, 1,
        "responsive autocast must not wait for attack backswing"
    );
    assert_eq!(cast.attacks_resolved, 0);
    assert_eq!(sim.unit(source).unwrap().position, SimPoint::new(0, 0));
    assert_eq!(
        sim.unit(source)
            .unwrap()
            .status
            .action_animation
            .unwrap()
            .kind,
        ActionAnimationKind::Cast
    );
}

#[test]
fn crowd_steering_reserves_acting_units_before_movers_regardless_of_id() {
    for actor_first in [false, true] {
        let mut sim = Simulation::new(configuration(), 1);
        let actor = spawn(Team(0), SimPoint::new(0, 0));
        let mover = spawn(Team(0), SimPoint::new(-20, 0));
        let (actor_id, mover_id) = if actor_first {
            (
                sim.spawn_unit_with_properties(actor, properties(MovementClass::Ground)),
                sim.spawn_unit_with_properties(mover, properties(MovementClass::Ground)),
            )
        } else {
            let mover_id = sim.spawn_unit_with_properties(mover, properties(MovementClass::Ground));
            (
                sim.spawn_unit_with_properties(actor, properties(MovementClass::Ground)),
                mover_id,
            )
        };
        let entity = sim
            .snapshot_units()
            .iter()
            .find(|unit| unit.id == actor_id)
            .unwrap()
            .entity;
        sim.world
            .get_mut::<StatusState>(entity)
            .unwrap()
            .begin_action_animation(ActionAnimationKind::Cast, 0, 4);
        let mut restored = wire_restore(&sim);
        for _ in 0..4 {
            assert_eq!(sim.step().checksum, restored.step().checksum);
            let actor = sim.unit(actor_id).unwrap().position;
            assert_eq!(
                actor,
                SimPoint::new(0, 0),
                "separation/collision must not slide an actor"
            );
            assert!(actor.distance_sq(sim.unit(mover_id).unwrap().position) >= 16 * 16);
        }
    }
}
