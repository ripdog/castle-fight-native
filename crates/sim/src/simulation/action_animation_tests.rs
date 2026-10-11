use super::*;
use crate::ActionAnimationState;

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
            cast_point_ticks: 0,
            ..ActionTimingProfile::default()
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
fn cast_point_delays_effect_and_survives_wire_restore() {
    let mut sim = Simulation::new(configuration(), 1);
    let mut unit = spawn(Team(0), SimPoint::new(0, 0));
    unit.attack.range = 0;
    unit.attack.acquisition_range = 0;
    let mut props = properties(MovementClass::Ground);
    props.action_timing.cast_ticks = 4;
    props.action_timing.cast_point_ticks = 2;
    let source = sim.spawn_unit_with_properties_and_spellcasting(unit, props, spellcasting(10));
    let victim = target(&mut sim, MovementClass::Ground);

    let start = sim.step();
    assert_eq!(start.ability_casts, 0);
    assert!(
        sim.unit(victim).is_some(),
        "spell damage must wait for the cast point"
    );
    let source_state = sim.unit(source).unwrap();
    assert_eq!(source_state.position, SimPoint::new(0, 0));
    assert_eq!(source_state.status.pending_cast.unwrap().release_tick, 2);
    assert_eq!(
        source_state.status.action_animation.unwrap(),
        ActionAnimationState {
            kind: ActionAnimationKind::Cast,
            started_tick: 0,
            until_tick: 4,
        }
    );

    let mut restored = wire_restore(&sim);
    let windup = sim.step();
    assert_eq!(windup.checksum, restored.step().checksum);
    assert_eq!(windup.ability_casts, 0);
    assert!(sim.unit(victim).is_some());
    assert_eq!(sim.unit(source).unwrap().position, SimPoint::new(0, 0));

    let release = sim.step();
    assert_eq!(release.checksum, restored.step().checksum);
    assert_eq!(release.ability_casts, 1);
    assert!(
        sim.unit(victim).is_none(),
        "spell damage must resolve at the cast point"
    );
    assert!(sim.unit(source).unwrap().status.pending_cast.is_none());
    assert_eq!(sim.unit(source).unwrap().position, SimPoint::new(0, 0));

    assert_eq!(sim.step().checksum, restored.step().checksum);
    assert_eq!(sim.unit(source).unwrap().position, SimPoint::new(0, 0));
    assert_eq!(sim.step().checksum, restored.step().checksum);
    assert!(sim.unit(source).unwrap().position.x > 0);
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

#[test]
fn attack_windup_precedes_damage_and_projectiles_without_extending_cadence() {
    for class in [MovementClass::Ground, MovementClass::Air] {
        for delivery in [
            AttackDelivery::Melee,
            AttackDelivery::RangedGuaranteedHit { speed_per_tick: 4 },
        ] {
            let mut sim = Simulation::new(configuration(), 1);
            let mut attacker = spawn(Team(0), SimPoint::new(0, 0));
            attacker.attack.cooldown_ticks = 6;
            let mut props = properties(class);
            props.action_timing.primary_attack_ticks = 4;
            props.action_timing.primary_attack_point_ticks = 2;
            attacker.attack.delivery = delivery;
            let source = sim.spawn_unit_with_properties(attacker, props);
            let mut victim = spawn(Team(1), SimPoint::new(40, 0));
            victim.health = 1_000;
            victim.attack.damage = 0;
            victim.attack.acquisition_range = 0;
            victim.attack.range = 0;
            victim.movement.speed_per_tick = 0;
            let target = sim.spawn_unit_with_properties(victim, properties(MovementClass::Ground));
            sim.step();
            let origin = sim.unit(source).unwrap().position;
            let start = sim.step();
            assert_eq!(start.attacks_resolved, 0);
            assert_eq!(start.projectiles_launched, 0);
            assert_eq!(sim.unit(target).unwrap().health, victim.health);
            let action = sim.unit(source).unwrap().status.action_animation.unwrap();
            assert_eq!((action.started_tick, action.until_tick), (1, 5));
            assert_eq!(
                sim.unit(source)
                    .unwrap()
                    .status
                    .pending_attack
                    .unwrap()
                    .release_tick,
                3
            );
            let mut restored = wire_restore(&sim);
            assert_eq!(sim.step().checksum, restored.step().checksum);
            assert_eq!(sim.unit(source).unwrap().position, origin);
            assert_eq!(sim.unit(target).unwrap().health, victim.health);
            assert!(sim.attacks_last_tick().is_empty());
            let release = sim.step();
            assert_eq!(release.checksum, restored.step().checksum);
            assert_eq!(release.attacks_resolved, 1);
            assert_eq!(
                sim.unit(source).unwrap().status.action_animation,
                Some(action)
            );
            assert!(sim.unit(source).unwrap().status.pending_attack.is_none());
            assert_eq!(sim.unit(source).unwrap().cooldown_remaining, 4);
            if delivery == AttackDelivery::Melee {
                assert!(sim.unit(target).unwrap().health < victim.health);
            } else {
                assert_eq!(release.projectiles_launched, 1);
            }
            let mut releases = vec![release.completed_tick];
            while sim.tick() <= 15 {
                let result = sim.step();
                assert_eq!(result.checksum, restored.step().checksum);
                if sim
                    .attacks_last_tick()
                    .iter()
                    .any(|event| event.source == source)
                {
                    releases.push(result.completed_tick);
                }
            }
            assert_eq!(releases, [3, 9, 15]);
        }
    }
}

#[test]
fn secondary_attack_and_speed_modifiers_scale_windup_inside_the_existing_cycle() {
    for (secondary, speed_delta) in [(false, 0), (true, 0), (true, 100), (true, -50)] {
        let mut sim = Simulation::new(configuration(), 1);
        let mut props = properties(MovementClass::Ground);
        props.action_timing.primary_attack_point_ticks = 1;
        props.action_timing.secondary_attack_point_ticks = 3;
        let attacker = spawn(Team(0), SimPoint::new(0, 0));
        props.secondary_attack = Some(SecondaryAttackProfile {
            primary_targets: AttackTargetMask::GROUND_AND_BUILDINGS,
            attack: attacker.attack,
            targets: AttackTargetMask::AIR_UNITS,
            damage_type: DamageType::Normal,
        });
        let source = sim.spawn_unit_with_properties(attacker, props);
        target(
            &mut sim,
            if secondary {
                MovementClass::Air
            } else {
                MovementClass::Ground
            },
        );
        let entity = sim
            .snapshot_units()
            .into_iter()
            .find(|unit| unit.id == source)
            .unwrap()
            .entity;
        let mut status = sim.world.get_mut::<StatusState>(entity).unwrap();
        status.attack_speed_modifiers[0] = TimedAttackSpeedModifier {
            undispellable: false,
            id: ModifierId(123),
            percent_delta: speed_delta,
            expires_tick: 100,
        };
        status.attack_speed_modifier_count = 1;
        let expected_point =
            effective_attack_cooldown_ticks(if secondary { 3 } else { 1 }, *status);
        let expected_cycle =
            effective_attack_cooldown_ticks(attacker.attack.cooldown_ticks, *status);
        sim.step();
        assert_eq!(sim.step().attacks_resolved, 0);
        let state = sim.unit(source).unwrap();
        assert_eq!(state.cooldown_remaining, expected_cycle);
        assert_eq!(
            state.status.pending_attack.unwrap().release_tick,
            1 + u64::from(expected_point)
        );
        while sim.tick() < 1 + u64::from(expected_point) {
            assert_eq!(sim.step().attacks_resolved, 0);
        }
        assert_eq!(sim.step().attacks_resolved, 1);
    }
}

#[test]
fn interrupted_windup_does_not_release_or_switch_to_a_new_target() {
    for interruption in 0..4 {
        let mut sim = Simulation::new(configuration(), 1);
        let mut props = properties(MovementClass::Ground);
        props.action_timing.primary_attack_point_ticks = 2;
        let source = sim.spawn_unit_with_properties(spawn(Team(0), SimPoint::new(0, 0)), props);
        let victim = target(&mut sim, MovementClass::Ground);
        sim.step();
        sim.step();
        assert!(sim.unit(source).unwrap().status.pending_attack.is_some());
        let units = sim.snapshot_units();
        let source_entity = units.iter().find(|unit| unit.id == source).unwrap().entity;
        let victim_entity = units.iter().find(|unit| unit.id == victim).unwrap().entity;
        match interruption {
            0 => {
                sim.world
                    .get_mut::<StatusState>(source_entity)
                    .unwrap()
                    .stunned_until_tick = 5;
            }
            1 => {
                sim.world.get_mut::<Health>(victim_entity).unwrap().current = 0;
                target(&mut sim, MovementClass::Ground);
            }
            2 => {
                sim.world.get_mut::<Position>(victim_entity).unwrap().0 = SimPoint::new(200, 0);
            }
            3 => {
                sim.world
                    .get_mut::<StatusState>(source_entity)
                    .unwrap()
                    .begin_action_animation(ActionAnimationKind::Cast, 2, 4);
            }
            _ => unreachable!(),
        }
        for _ in 2..5 {
            sim.step();
            assert!(
                !sim.attacks_last_tick()
                    .iter()
                    .any(|attack| attack.source == source)
            );
        }
        assert!(sim.unit(source).unwrap().status.pending_attack.is_none());
    }
}

fn dual_weapon_tower(sim: &mut Simulation) -> SimId {
    let primary = AttackProfile {
        delivery: AttackDelivery::RangedInstant,
        damage: 7,
        range: 96,
        acquisition_range: 128,
        cooldown_ticks: 12,
    };
    sim.spawn_building_with_properties(
        BuildingSpawn {
            team: Team(0),
            footprint: BuildingFootprint::new(0, 0, 1, 1),
            health: 500,
            production: None,
            attack: Some(primary),
            spellcasting: None,
        },
        BuildingGameplayProperties {
            attack_targets: AttackTargetMask::AIR_AND_GROUND,
            secondary_attack: Some(SecondaryAttackProfile {
                primary_targets: AttackTargetMask::AIR_UNITS,
                attack: AttackProfile {
                    delivery: AttackDelivery::RangedGuaranteedHit {
                        speed_per_tick: 256,
                    },
                    damage: 11,
                    range: 32,
                    acquisition_range: 64,
                    cooldown_ticks: 17,
                },
                targets: AttackTargetMask::GROUND_UNITS,
                damage_type: DamageType::Pierce,
            }),
            action_timing: ActionTimingProfile {
                primary_attack_ticks: 5,
                primary_attack_point_ticks: 2,
                secondary_attack_ticks: 7,
                secondary_attack_point_ticks: 4,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}

#[test]
fn building_weapons_keep_separate_delivery_damage_and_windup_through_rejoin() {
    for (class, damage, release_tick, cooldown) in [
        (MovementClass::Air, 7, 3, 10),
        (MovementClass::Ground, 11, 5, 13),
    ] {
        let mut sim = Simulation::new(configuration(), 1);
        let tower = dual_weapon_tower(&mut sim);
        let mut victim = spawn(Team(1), SimPoint::new(48, 16));
        victim.health = 500;
        victim.movement.speed_per_tick = 0;
        victim.attack.damage = 0;
        victim.attack.acquisition_range = 0;
        victim.attack.range = 0;
        let victim = sim.spawn_unit_with_properties(victim, properties(class));
        sim.step();
        sim.step();
        assert_eq!(sim.unit(victim).unwrap().health, 500);
        let view = sim.building(tower).unwrap();
        assert!(view.secondary_attack.is_some());
        assert_eq!(
            view.status.pending_attack.unwrap().release_tick,
            release_tick
        );
        assert_eq!(view.status.action_animation.unwrap().started_tick, 1);
        let mut restored = wire_restore(&sim);
        while sim.next_tick <= release_tick {
            let tick = sim.next_tick;
            let step = sim.step();
            assert_eq!(step.checksum, restored.step().checksum);
            assert_eq!(step.attacks_resolved, usize::from(tick == release_tick));
        }
        assert_eq!(
            sim.building(tower).unwrap().cooldown_remaining,
            Some(cooldown)
        );
        if class == MovementClass::Ground {
            assert_eq!(
                sim.unit(victim).unwrap().health,
                500,
                "the ground weapon has missile travel"
            );
            assert_eq!(sim.step().checksum, restored.step().checksum);
        }
        let damage_type = if class == MovementClass::Air {
            DamageType::Normal
        } else {
            DamageType::Pierce
        };
        let expected_damage = sim.combat_rules.damage_rules.apply_attack(
            damage,
            damage_type,
            sim.unit(victim).unwrap().armor,
        );
        assert_eq!(sim.unit(victim).unwrap().health, 500 - expected_damage);
    }
}

#[test]
fn building_secondary_weapon_rejects_structures_and_uses_its_own_ranges() {
    let mut sim = Simulation::new(configuration(), 1);
    let tower = dual_weapon_tower(&mut sim);
    let structure = sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(2, 0, 1, 1),
        health: 500,
        production: None,
        attack: None,
        spellcasting: None,
    });
    assert_eq!(
        sim.order_building_attack_target(tower, structure),
        Err(BuildingCommandError::InvalidTargetType)
    );
    let mut ground = spawn(Team(1), SimPoint::new(112, 16));
    ground.health = 500;
    ground.attack.damage = 0;
    ground.attack.acquisition_range = 0;
    ground.attack.range = 0;
    ground.movement.speed_per_tick = 0;
    let ground = sim.spawn_unit_with_properties(ground, properties(MovementClass::Ground));
    assert_eq!(
        sim.order_building_attack_target(tower, ground),
        Err(BuildingCommandError::TargetOutOfRange)
    );
    for _ in 0..4 {
        sim.step();
    }
    assert!(sim.building(tower).unwrap().target.is_none());
    let victim = sim
        .snapshot_units()
        .into_iter()
        .find(|unit| unit.id == ground)
        .unwrap()
        .entity;
    sim.world
        .entity_mut(victim)
        .get_mut::<Position>()
        .unwrap()
        .0 = SimPoint::new(80, 16);
    sim.order_building_attack_target(tower, ground).unwrap();
    sim.step();
    assert_eq!(sim.building(tower).unwrap().target, Some(ground));
    assert!(
        sim.building(tower).unwrap().status.pending_attack.is_none(),
        "acquisition does not extend weapon range"
    );
    sim.world
        .entity_mut(victim)
        .get_mut::<Position>()
        .unwrap()
        .0 = SimPoint::new(48, 16);
    sim.step();
    assert!(sim.building(tower).unwrap().status.pending_attack.is_some());
    sim.world
        .entity_mut(victim)
        .get_mut::<Position>()
        .unwrap()
        .0 = SimPoint::new(160, 16);
    sim.step();
    assert!(sim.building(tower).unwrap().status.pending_attack.is_none());
    assert!(
        sim.building(tower)
            .unwrap()
            .status
            .action_animation
            .is_none()
    );
    assert_eq!(sim.unit(ground).unwrap().health, 500);
}

#[test]
fn building_windup_cancels_when_an_earlier_released_attack_kills_its_target() {
    let mut sim = Simulation::new(configuration(), 1);
    let mut earlier = spawn(Team(0), SimPoint::new(-16, 16));
    earlier.health = 500;
    earlier.movement.speed_per_tick = 0;
    earlier.attack.range = 0;
    earlier.attack.acquisition_range = 0;
    let earlier = sim.spawn_unit(earlier);
    let tower = dual_weapon_tower(&mut sim);
    let mut victim = spawn(Team(1), SimPoint::new(48, 16));
    victim.movement.speed_per_tick = 0;
    victim.attack.range = 0;
    victim.attack.acquisition_range = 0;
    let victim = sim.spawn_unit_with_properties(victim, properties(MovementClass::Air));
    sim.step();
    sim.step();
    let release_tick = sim
        .building(tower)
        .unwrap()
        .status
        .pending_attack
        .unwrap()
        .release_tick;
    while sim.next_tick < release_tick {
        sim.step();
    }
    let entity = sim
        .snapshot_units()
        .into_iter()
        .find(|unit| unit.id == earlier)
        .unwrap()
        .entity;
    *sim.world
        .entity_mut(entity)
        .get_mut::<AttackProfile>()
        .unwrap() = AttackProfile {
        delivery: AttackDelivery::RangedInstant,
        damage: 100,
        range: 96,
        acquisition_range: 128,
        cooldown_ticks: 10,
    };
    let step = sim.step();
    assert_eq!(step.attacks_resolved, 1);
    assert!(sim.unit(victim).is_none());
    let status = sim.building(tower).unwrap().status;
    assert!(status.pending_attack.is_none());
    assert!(status.action_animation.is_none());
}
