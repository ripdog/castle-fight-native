use super::*;
use crate::{ActionTimingProfile, ManaProfile, SupportOrderRules, WindWalkProfile};
fn p() -> WindWalkProfile {
    let point = |x, y| SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, y * SUBUNITS_PER_WORLD_UNIT);
    WindWalkProfile {
        ability: AbilityId(123),
        rules: SupportOrderRules {
            map_version: crate::MapVersion::CASTLE_FIGHT_9_27,
            interval_ticks: 6,
            budget: 2,
            cache_ticks: 3,
            retention_limit: 2,
            preferred_mana_above: 4,
            fallback_health_below: [40, 80],
            local_rectangles: [
                [point(-100, -100), point(30, 100)],
                [point(-30, -100), point(100, 100)],
            ],
            battlefield: [point(-100, -100), point(100, 100)],
            excluded_ability: 0,
            excluded_buffs: [999, 998],
        },
        fade_ticks: 3,
        duration_ticks: 100,
        movement_percent_delta: 25,
        backstab_damage: 7,
        fallback_health_below: 40,
    }
}
fn spawn(s: &mut Simulation, team: u8, x: i32, hp: i32, caster: bool, mana: bool) -> SimId {
    let properties = UnitGameplayProperties {
        collision_radius: Some(CollisionRadius(0)),
        classifications: UnitClassifications {
            combat_sapper: true,
            ..Default::default()
        },
        action_timing: ActionTimingProfile {
            primary_attack_point_ticks: 2,
            primary_attack_ticks: 3,
            ..Default::default()
        },
        ..Default::default()
    };
    let spawn = UnitSpawn {
        team: Team(team),
        position: SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, 0),
        health: hp,
        attack: AttackProfile {
            damage: if caster { 2 } else { 0 },
            range: 4 * SUBUNITS_PER_WORLD_UNIT,
            acquisition_range: 5 * SUBUNITS_PER_WORLD_UNIT,
            cooldown_ticks: 100,
            delivery: AttackDelivery::Melee,
        },
        movement: MovementProfile { speed_per_tick: 0 },
    };
    if caster || mana {
        s.spawn_unit_with_properties_and_spellcasting(
            spawn,
            properties,
            SpellcastingProfile {
                mana: ManaProfile::per_second(6, 6, 0),
                ability: AutomaticAbilityProfile {
                    id: AbilityId(if caster { 123 } else { 124 }),
                    mana_cost: if caster { 5 } else { 0 },
                    cooldown_ticks: 0,
                    range: 0,
                    target_policy: AbilityTargetPolicy::ScriptedController,
                    effect: if caster {
                        AbilityEffect::WindWalk(p())
                    } else {
                        AbilityEffect::Damage { amount: 0 }
                    },
                },
            },
        )
    } else {
        s.spawn_unit_with_properties(spawn, properties)
    }
}
fn simulation() -> Simulation {
    Simulation::new(SimulationConfig::default(), 1)
}
#[test]
fn support_selection_prefers_local_fallback_before_global_mana_and_retains_native_failures() {
    let mut s = simulation();
    let actor = spawn(&mut s, 0, 0, 100, true, false);
    let fallback = spawn(&mut s, 1, 1, 20, false, false);
    spawn(&mut s, 1, 80, 100, false, true);
    let mut units = s.snapshot_units();
    s.resolve_support_orders(&mut units);
    let i = find_unit_index(&units, actor).unwrap();
    assert_eq!(
        units[i].status.support_order.unwrap().selected_target,
        Some(fallback)
    );
    assert_eq!(units[i].mana_current, Some(1));
    let mut q = SupportOrderQueue {
        rules: p().rules,
        next_tick: 0,
        actors: Default::default(),
        caches: Vec::new(),
    };
    units[i].status.stunned_until_tick = 100;
    for count in 1..=2 {
        s.issue_support_order(i, &mut q, &mut units);
        assert_eq!(units[i].status.support_order.unwrap().iterations, count);
    }
    s.issue_support_order(i, &mut q, &mut units);
    assert_eq!(units[i].status.support_order.unwrap().selected_target, None);
    assert_eq!(units[i].mana_current, Some(1));
}
#[test]
fn support_queue_caps_service_rotates_fifo_and_halves_interval_under_load() {
    let mut s = simulation();
    let ids: Vec<_> = (0..9)
        .map(|_| spawn(&mut s, 0, 0, 100, true, false))
        .collect();
    let mut units = s.snapshot_units();
    s.resolve_support_orders(&mut units);
    assert_eq!(
        units
            .iter()
            .filter(|u| u.status.wind_walk.is_some())
            .count(),
        2
    );
    let (_, q) = s
        .world
        .iter_entities()
        .find_map(|e| match e.get::<NativeAction>()? {
            NativeAction::SupportOrders(q) => Some((e.id(), q.clone())),
            _ => None,
        })
        .unwrap();
    assert_eq!(q.next_tick, 3);
    assert_eq!(q.actors.front(), Some(&ids[2]));
    let entity = s
        .world
        .iter_entities()
        .find(|e| {
            matches!(
                e.get::<NativeAction>(),
                Some(NativeAction::SupportOrders(_))
            )
        })
        .unwrap()
        .id();
    if let NativeAction::SupportOrders(q) = &mut *s.world.get_mut::<NativeAction>(entity).unwrap() {
        q.actors.push_front(SimId(99999));
    }
    s.next_tick = 3;
    s.resolve_support_orders(&mut units);
    assert_eq!(
        units
            .iter()
            .filter(|u| u.status.wind_walk.is_some())
            .count(),
        4
    );
    assert!(units[..4].iter().all(|u| u.mana_current == Some(1)));
}
#[test]
fn wind_walk_fade_breaks_on_attack_start_and_backstab_survives_windup_wire_restore() {
    let mut s = simulation();
    let actor = spawn(&mut s, 0, 0, 100, true, false);
    let victim = spawn(&mut s, 1, 1, 100, false, true);
    for _ in 0..4 {
        s.step();
    }
    assert!(s.unit(actor).unwrap().classifications.invisible);
    for _ in 0..3 {
        s.step();
    }
    let actor_view = s.unit(actor).unwrap();
    assert!(!actor_view.classifications.invisible);
    assert_eq!(actor_view.mana_current, Some(1));
    assert_eq!(actor_view.status.support_order.unwrap().backstab_pending, 7);
    assert!(actor_view.status.pending_attack.is_some());
    let bundle = crate::castle_fight_content_bundle(crate::MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&s.capture_snapshot().encode_wire().unwrap(), bundle)
            .unwrap();
    let mut restored = Simulation::new(SimulationConfig::default(), 4);
    restored.restore_snapshot(&snapshot).unwrap();
    for _ in 0..3 {
        assert_eq!(s.step().checksum, restored.step().checksum);
    }
    assert_eq!(s.unit(victim).unwrap().health, 91);
    assert_eq!(
        s.unit(actor)
            .unwrap()
            .status
            .support_order
            .unwrap()
            .backstab_pending,
        0
    );
}
#[test]
fn wind_walk_phases_through_units_without_changing_terrain_clearance() {
    let mut s = simulation();
    let source = spawn(&mut s, 0, 0, 100, false, false);
    spawn(&mut s, 0, 1, 100, false, false);
    let mut units = s.snapshot_units();
    let i = find_unit_index(&units, source).unwrap();
    units[i].movement.speed_per_tick = 2 * SUBUNITS_PER_WORLD_UNIT;
    units[i].collision_radius = SUBUNITS_PER_WORLD_UNIT;
    units[1].collision_radius = SUBUNITS_PER_WORLD_UNIT;
    units[i].status.support_order = Some(crate::SupportOrderState {
        move_point: Some(SimPoint::new(3 * SUBUNITS_PER_WORLD_UNIT, 0)),
        ..Default::default()
    });
    units[i].status.wind_walk = Some(crate::WindWalkState {
        ability: p().ability,
        cloaked_tick: 0,
        expires_tick: 100,
        movement_percent_delta: 0,
        backstab_damage: 0,
    });
    let mut positions: Vec<_> = units.iter().map(|u| u.position).collect();
    let mut navigation: Vec<_> = units.iter().map(|u| u.navigation).collect();
    s.resolve_movement(
        &units,
        &[],
        &[100, 100],
        &[],
        &mut positions,
        &mut navigation,
    );
    assert!(positions[i].x > units[1].position.x);
    assert!(
        positions[i].distance_sq(positions[1])
            < square_i32(units[i].collision_radius + units[1].collision_radius)
    );
    assert_eq!(units[i].collision_radius, SUBUNITS_PER_WORLD_UNIT);
}
