use super::*;
use crate::components::{
    MasterFan, MasterMover, MasterMoverProfile, MasterProjectileProfile, MasterProjectileState,
    NativeAction, NativeNovaProfile,
};
fn point(x: i32, y: i32) -> SimPoint {
    SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, y * SUBUNITS_PER_WORLD_UNIT)
}
fn profile() -> MasterProjectileProfile {
    let mover = MasterMoverProfile {
        rawcode: u32::from_be_bytes(*b"h03H"),
        step: 10 * SUBUNITS_PER_WORLD_UNIT,
        radius: 8 * SUBUNITS_PER_WORLD_UNIT,
        budget: 20 * SUBUNITS_PER_WORLD_UNIT,
        decrement: 10 * SUBUNITS_PER_WORLD_UNIT,
        interval_millis: 30,
        height: 20,
    };
    MasterProjectileProfile {
        ability: AbilityId(44),
        buildings: [
            [11, 12, 0],
            [21, 22, 0],
            [31, 32, 0],
            [41, 42, 43],
            [51, 52, 53],
        ],
        lightning: mover,
        frost: MasterMoverProfile {
            rawcode: u32::from_be_bytes(*b"h068"),
            ..mover
        },
        damage: 10,
        damage_count_cap: 3,
        branch_roll_size: 100,
        lightning_roll_below: 100,
        lightning_initial_count: 2,
        lightning_count_divisor: 2,
        split_initial_count: 1,
        split_count_divisor: 2,
        lane_spacing: 10 * SUBUNITS_PER_WORLD_UNIT,
        forward_offset: 10 * SUBUNITS_PER_WORLD_UNIT,
        split_angle_millidegrees: 30_000,
        stub_budget: 10 * SUBUNITS_PER_WORLD_UNIT,
        frost_initial_count: 3,
        frost_count_factor: 2,
        fan_spread_millidegrees: 0,
        fan_interval_millis: 40,
        nova: NativeNovaProfile {
            ability: AbilityId(55),
            radius: 15 * SUBUNITS_PER_WORLD_UNIT,
            primary_damage: 7,
            area_damage: 5,
            duration_ticks: 10,
            hero_duration_ticks: 4,
            movement_percent_delta: -10,
            attack_speed_percent_delta: -10,
        },
        nova_reveal_radius: 10 * SUBUNITS_PER_WORLD_UNIT,
        nova_reveal_ticks: 2,
        parasite_speed: 100 * SUBUNITS_PER_WORLD_UNIT,
        parasite_duration_ticks: 1,
        parasite_child_rawcode: u32::from_be_bytes(*b"nfbr"),
        parasite_child_duration_ticks: 1,
    }
}
fn state(p: MasterProjectileProfile) -> MasterProjectileState {
    MasterProjectileState {
        profile: Box::new(p),
        map_version: crate::MapVersion::CASTLE_FIGHT_9_27,
        lightning: Vec::new(),
        frost: Vec::new(),
        fans: Vec::new(),
        parasites: Vec::new(),
        minions: Vec::new(),
        lightning_due_millis: 30,
        frost_due_millis: 30,
    }
}
fn moving(id: u64, position: SimPoint, last_lane: bool) -> MasterMover {
    MasterMover {
        id: SimId(id),
        source: SimId(99),
        owner: PlayerId(0),
        team: Team(0),
        position_fractional: [
            i64::from(position.x) * 1_000_000,
            i64::from(position.y) * 1_000_000,
        ],
        direction: SimPoint::new(1_000_000, 0),
        damage: 10,
        budget: 20 * SUBUNITS_PER_WORLD_UNIT,
        splits: 1,
        stage: 0,
        last_hit: None,
        last_original_lane: last_lane,
        born_tick: 0,
    }
}
fn victim(sim: &mut Simulation, team: u8, x: i32, y: i32, immune: bool) -> SimId {
    let mut spawn = unit(team, x, 0, AttackDelivery::Melee);
    spawn.position = point(x, y);
    sim.spawn_unit_with_properties(
        spawn,
        UnitGameplayProperties {
            classifications: UnitClassifications {
                combat_sapper: true,
                spell_immune: immune,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}
fn run_action(sim: &mut Simulation, state: MasterProjectileState, units: &mut [UnitSnapshot]) {
    let id = sim.allocate_id();
    let entity = sim.world.spawn((id,)).id();
    sim.advance_master_projectiles(entity, state, units);
}
fn saved(sim: &Simulation) -> MasterProjectileState {
    sim.world
        .iter_entities()
        .find_map(|e| match e.get::<NativeAction>()? {
            NativeAction::MasterProjectiles(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap()
}
#[test]
fn master_lightning_remembers_only_last_victim_and_preserves_asymmetric_lane_splits() {
    let mut sim = simulation(1);
    let first = victim(&mut sim, 1, 21, 20, false);
    let second = victim(&mut sim, 1, 21, 22, false);
    let mut units = sim.snapshot_units();
    let mut p = profile();
    p.lightning.step = 1;
    p.lightning.budget = 100;
    p.lightning.decrement = 1;
    let mut s = state(p);
    let mut m = moving(101, point(20, 20), true);
    m.budget = 100;
    s.lightning.push(m);
    let id = sim.allocate_id();
    let entity = sim.world.spawn((id,)).id();
    for tick in [1, 2, 3] {
        sim.next_tick = tick;
        sim.advance_master_projectiles(entity, s, &mut units);
        s = saved(&sim);
    }
    assert_eq!(units[find_unit_index(&units, first).unwrap()].health, 980);
    assert_eq!(units[find_unit_index(&units, second).unwrap()].health, 990);
    let mut sim = simulation(1);
    let mut s = state(profile());
    s.lightning.extend([
        moving(201, point(0, 0), false),
        moving(202, point(0, 50), true),
    ]);
    let id = sim.allocate_id();
    let entity = sim.world.spawn((id,)).id();
    sim.next_tick = 2;
    sim.advance_master_projectiles(entity, s, &mut []);
    let s = saved(&sim);
    assert_eq!(s.lightning.len(), 4);
    assert_eq!(s.lightning.iter().filter(|m| m.stage == 1).count(), 2);
    // New right branches are appended and processed during this same native callback.
    assert_eq!(s.lightning.iter().filter(|m| m.stage == 0).count(), 2);
    sim.next_tick = 3;
    sim.advance_master_projectiles(entity, s, &mut []);
    let s = saved(&sim);
    assert_eq!(s.lightning.len(), 3);
    assert!(s.lightning.iter().all(|m| m.stage == 0));
    assert!(s.lightning.iter().any(|m| m.id == SimId(202)));
    assert!(s.lightning.iter().all(|m| m.id != SimId(201)));
}
#[test]
fn master_frost_stops_on_immune_hit_and_secondary_nova_uses_surviving_victim_center() {
    for immune in [false, true] {
        let mut sim = simulation(1);
        let primary = victim(&mut sim, 1, 30, 20, immune);
        let splash = victim(&mut sim, 1, 42, 20, false);
        let ally = victim(&mut sim, 0, 30, 21, false);
        let mut units = sim.snapshot_units();
        let mut s = state(profile());
        s.frost.push(moving(101, point(20, 20), false));
        sim.next_tick = 1;
        run_action(&mut sim, s, &mut units);
        assert!(saved(&sim).frost.is_empty());
        assert_eq!(
            units[find_unit_index(&units, primary).unwrap()].health,
            if immune { 1000 } else { 978 }
        );
        assert_eq!(
            units[find_unit_index(&units, splash).unwrap()].health,
            if immune { 1000 } else { 995 }
        );
        assert_eq!(units[find_unit_index(&units, ally).unwrap()].health, 1000);
        assert_eq!(
            sim.last_ability_casts
                .iter()
                .filter(|c| c.ability == AbilityId(55))
                .count(),
            usize::from(!immune)
        );
    }
    let mut sim = simulation(1);
    let primary = victim(&mut sim, 1, 30, 20, false);
    let splash = victim(&mut sim, 1, 42, 20, false);
    let mut units = sim.snapshot_units();
    let i = find_unit_index(&units, primary).unwrap();
    units[i].health = 5;
    let mut s = state(profile());
    s.frost.push(moving(101, point(20, 20), false));
    sim.next_tick = 1;
    run_action(&mut sim, s, &mut units);
    assert!(units[i].health <= 0);
    assert_eq!(units[find_unit_index(&units, splash).unwrap()].health, 1000);
    assert!(sim.last_ability_casts.is_empty());
}
#[test]
fn master_fan_samples_live_source_position_then_stops_new_missiles_on_source_death_and_recovers() {
    let mut sim = simulation(1);
    let source = victim(&mut sim, 0, 10, 20, false);
    let mut s = state(profile());
    s.fans.push(MasterFan {
        source,
        owner: PlayerId(0),
        team: Team(0),
        direction: SimPoint::new(1_000_000, 0),
        damage: 10,
        remaining: 2,
        sequence: 1,
        due_millis: 40,
    });
    let id = sim.allocate_id();
    sim.world.spawn((id, NativeAction::MasterProjectiles(s)));
    sim.step();
    sim.step();
    assert_eq!(saved(&sim).frost.len(), 0);
    let entity = sim
        .world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&source))
        .unwrap()
        .id();
    sim.world
        .entity_mut(entity)
        .get_mut::<Position>()
        .unwrap()
        .0 = point(50, 20);
    let mut restored = wire_restored(&sim, 4);
    assert_eq!(sim.step().checksum, restored.step().checksum);
    let s = saved(&sim);
    assert_eq!(s.frost.len(), 1);
    assert_eq!(s.frost[0].position(), point(40, 20));
    sim.world.despawn(entity);
    let e = restored
        .world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&source))
        .unwrap()
        .id();
    restored.world.despawn(e);
    for _ in 0..5 {
        assert_eq!(sim.step().checksum, restored.step().checksum);
    }
    assert!(saved(&sim).fans.is_empty());
    assert!(saved(&sim).frost.is_empty());
}

#[test]
fn master_counts_owned_completed_buckets_and_launches_on_release_heading() {
    let mut sim = simulation(1);
    let caster = victim(&mut sim, 0, 10, 20, false);
    victim(&mut sim, 1, 50, 20, false);
    for (n, rawcode, owner) in [
        (0, 11, 0),
        (1, 12, 0),
        (2, 11, 1),
        (3, 31, 0),
        (4, 32, 0),
        (5, 51, 0),
        (6, 51, 0),
        (7, 51, 0),
        (8, 51, 0),
    ] {
        let id = sim.spawn_building_with_properties(
            BuildingSpawn {
                team: Team(0),
                footprint: BuildingFootprint::new(n * 2, 50, 1, 1),
                health: 100,
                production: None,
                attack: None,
                spellcasting: None,
            },
            BuildingGameplayProperties {
                content: Some(ContentIdentity {
                    map_version: crate::MapVersion::CASTLE_FIGHT_9_27,
                    rawcode,
                    name: "synthetic",
                }),
                ..Default::default()
            },
        );
        let entity = sim
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&id))
            .unwrap()
            .id();
        sim.world.entity_mut(entity).insert(Owner(PlayerId(owner)));
    }
    let mut units = sim.snapshot_units();
    let index = find_unit_index(&units, caster).unwrap();
    units[index].status.facing_direction = SimPoint::new(0, 1);
    let target = units.iter().position(|u| u.team == Team(1)).unwrap();
    let buildings = sim.snapshot_buildings();
    let source = AbilitySourceSnapshot {
        map_version: Some(crate::MapVersion::CASTLE_FIGHT_9_27),
        source: AbilitySourceIndex::Unit(index),
        id: caster,
        team: Team(0),
        origin: AbilitySourceOrigin::Unit(units[index].position),
        health: units[index].health,
        stunned_until_tick: 0,
        spellcasting: None,
        mana_current: None,
        ability_state: None,
    };
    let mut p = profile();
    p.damage_count_cap = 10;
    sim.start_master_projectiles(source, p, target, 1, &mut units, &buildings);
    let s = saved(&sim);
    assert_eq!(s.lightning.len(), 3);
    assert!(
        s.lightning
            .iter()
            .all(|m| m.splits == 3 && m.damage == 30 && m.direction.x == 0 && m.direction.y > 0)
    );
    assert!(
        s.lightning
            .iter()
            .all(|m| m.position().y > units[index].position.y)
    );
    assert_eq!(
        s.lightning.iter().filter(|m| m.last_original_lane).count(),
        1
    );
}
#[test]
fn native_parasite_death_window_keeps_inert_parent_payload_and_short_lived_noncombat_child() {
    let mut sim = simulation(1);
    let target = victim(&mut sim, 1, 30, 20, false);
    let mut s = state(profile());
    s.parasites.push(crate::components::MasterParasite {
        source: SimId(99),
        owner: PlayerId(0),
        team: Team(0),
        target,
        position: point(20, 20),
        buff_until_tick: None,
    });
    let mut units = sim.snapshot_units();
    sim.next_tick = 1;
    run_action(&mut sim, s, &mut units);
    assert_eq!(units[0].health, 1000);
    assert!(saved(&sim).parasites[0].buff_until_tick.is_some());
    let positions = vec![units[0].position];
    sim.observe_master_parasite_deaths(&units, &[0], &positions);
    let s = saved(&sim);
    assert_eq!(s.minions.len(), 1);
    assert!(s.parasites.is_empty());
    assert_eq!(sim.unit_count(), 1);
    let mut restored = wire_restored(&sim, 4);
    assert_eq!(sim.checksum(), restored.checksum());
    assert_eq!(sim.step().checksum, restored.step().checksum);
    assert_eq!(sim.step().checksum, restored.step().checksum);
    assert!(saved(&sim).minions.is_empty());
}
