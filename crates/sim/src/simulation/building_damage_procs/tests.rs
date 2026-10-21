use super::*;
use crate::{CastleFightTowerKind, MapVersion, UnitTemplate};
fn entity(sim: &Simulation, id: SimId) -> Entity {
    sim.world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}
fn controller(sim: &Simulation, source: SimId) -> (Entity, BuildingDamageProcState) {
    sim.world
        .iter_entities()
        .find_map(|e| match e.get::<NativeAction>() {
            Some(NativeAction::BuildingDamageProc(p)) if p.source == source => Some((e.id(), *p)),
            _ => None,
        })
        .unwrap()
}
fn setup(weapon: bool) -> (Simulation, SimId, BuildingDamageProcState) {
    let mut sim = Simulation::new(SimulationConfig::default(), 1);
    let definition = CastleFightTowerKind::ElementalGuardTower.definition();
    let mut spawn = definition.spawn(Team(0), BuildingFootprint::new(2, 2, 1, 1));
    spawn.attack = weapon.then_some(AttackProfile {
        damage: 1,
        range: 30 * SUBUNITS_PER_WORLD_UNIT,
        acquisition_range: 30 * SUBUNITS_PER_WORLD_UNIT,
        cooldown_ticks: 100,
        delivery: AttackDelivery::RangedGuaranteedHit {
            speed_per_tick: 5 * SUBUNITS_PER_WORLD_UNIT,
        },
    });
    let mut properties = definition.gameplay_properties();
    properties.action_timing = Default::default();
    let source = sim.spawn_building_with_properties(spawn, properties);
    let (e, mut p) = controller(&sim, source);
    p.profile.chance_per_10k = 10_000;
    p.profile.range = 40 * SUBUNITS_PER_WORLD_UNIT;
    p.profile.bolt.as_mut().unwrap().speed_per_tick = 5 * SUBUNITS_PER_WORLD_UNIT;
    p.profile.bolt.as_mut().unwrap().damage = 13;
    p.profile.bolt.as_mut().unwrap().stun_ticks = 11;
    p.profile.bolt.as_mut().unwrap().hero_stun_ticks = 3;
    sim.world
        .entity_mut(e)
        .insert(NativeAction::BuildingDamageProc(p));
    (sim, source, p)
}
fn target(
    sim: &mut Simulation,
    origin: SimPoint,
    offset: i32,
    flags: UnitClassifications,
) -> SimId {
    sim.spawn_unit_with_properties(
        UnitSpawn::from_template(
            Team(1),
            SimPoint::new(origin.x + offset * SUBUNITS_PER_WORLD_UNIT, origin.y),
            UnitTemplate {
                health: 1000,
                attack: AttackProfile {
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    cooldown_ticks: 100,
                    delivery: AttackDelivery::Melee,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
        ),
        UnitGameplayProperties {
            classifications: flags,
            ..Default::default()
        },
    )
}
fn observe(sim: &mut Simulation, source: SimId, victim: SimId) {
    let unit = sim
        .snapshot_units()
        .into_iter()
        .find(|u| u.id == victim)
        .unwrap();
    sim.observe_building_damage_proc(source, &unit, unit.position, sim.next_tick);
}
fn restore(sim: &Simulation) -> Simulation {
    let content = crate::castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), content)
            .unwrap();
    let mut other = Simulation::new(sim.config.clone(), 4);
    other.restore_snapshot(&snapshot).unwrap();
    other
}
#[test]
fn fresh_dummy_orders_have_independent_cooldowns_and_survive_source_removal_and_wire() {
    let (mut sim, source, p) = setup(false);
    let victim = target(
        &mut sim,
        p.origin,
        20,
        UnitClassifications {
            combat_sapper: true,
            ..Default::default()
        },
    );
    observe(&mut sim, source, victim);
    observe(&mut sim, source, victim);
    assert_eq!(controller(&sim, source).1.event_sequence, 2);
    assert_eq!(
        sim.world
            .iter_entities()
            .filter(|e| matches!(e.get::<NativeAction>(), Some(NativeAction::Bolt(_))))
            .count(),
        2
    );
    sim.world.despawn(entity(&sim, source));
    let mut other = restore(&sim);
    for _ in 0..5 {
        assert_eq!(
            {
                sim.step();
                sim.checksum()
            },
            {
                other.step();
                other.checksum()
            }
        );
    }
    assert_eq!(
        sim.unit(victim).unwrap().health,
        1000 - 2 * p.profile.bolt.unwrap().damage
    );
    assert!(
        sim.world
            .get::<StatusState>(entity(&sim, victim))
            .unwrap()
            .native_stun_until_tick
            > sim.next_tick
    );
}
#[test]
fn callback_rolls_before_failed_native_order_and_ignores_non_combat_recipients() {
    let (mut sim, source, p) = setup(false);
    let flags = UnitClassifications {
        combat_sapper: true,
        ..Default::default()
    };
    let outside = target(&mut sim, p.origin, 41, flags);
    let immune = target(
        &mut sim,
        p.origin,
        20,
        UnitClassifications {
            spell_immune: true,
            ..flags
        },
    );
    let invulnerable = target(
        &mut sim,
        p.origin,
        20,
        UnitClassifications {
            invulnerable: true,
            ..flags
        },
    );
    let non_sapper = target(&mut sim, p.origin, 20, Default::default());
    for victim in [outside, immune, invulnerable, non_sapper] {
        observe(&mut sim, source, victim);
    }
    assert_eq!(controller(&sim, source).1.event_sequence, 3);
    assert!(
        !sim.world
            .iter_entities()
            .any(|e| matches!(e.get::<NativeAction>(), Some(NativeAction::Bolt(_))))
    );
}
#[test]
fn released_weapon_retains_callback_until_impact_even_after_tower_death() {
    let (mut sim, source, p) = setup(true);
    let victim = target(
        &mut sim,
        p.origin,
        20,
        UnitClassifications {
            combat_sapper: true,
            hero: true,
            ..Default::default()
        },
    );
    for _ in 0..5 {
        sim.step();
        if sim.world.iter_entities().any(|e| {
            e.get::<GuaranteedHitProjectile>()
                .is_some_and(|p| p.source == source)
        }) {
            break;
        }
    }
    assert!(sim.world.iter_entities().any(|e| {
        e.get::<GuaranteedHitProjectile>()
            .is_some_and(|p| p.source == source)
    }));
    sim.world.despawn(entity(&sim, source));
    let mut other = restore(&sim);
    let mut saw_boulder = false;
    for _ in 0..15 {
        assert_eq!(
            {
                sim.step();
                sim.checksum()
            },
            {
                other.step();
                other.checksum()
            }
        );
        saw_boulder |= sim
            .world
            .iter_entities()
            .any(|e| matches!(e.get::<NativeAction>(), Some(NativeAction::Bolt(_))));
    }
    assert!(saw_boulder);
    assert_eq!(
        sim.unit(victim).unwrap().health,
        1000 - 1 - p.profile.bolt.unwrap().damage
    );
    assert!(!sim.world.iter_entities().any(|e| matches!(
        e.get::<NativeAction>(),
        Some(NativeAction::BuildingDamageProc(_))
    )));
}

#[test]
fn construction_grant_activates_once_at_completion_after_wire_restore() {
    let mut sim = Simulation::new(SimulationConfig::default(), 1);
    let definition = CastleFightTowerKind::ElementalGuardTower.definition();
    let mut properties = definition.gameplay_properties();
    properties.construction_time_ticks = Some(3);
    let source = sim
        .try_start_building_construction(
            PlayerId(0),
            definition.spawn(Team(0), BuildingFootprint::new(2, 2, 1, 1)),
            properties,
        )
        .unwrap();
    let mut other = restore(&sim);
    for _ in 0..3 {
        assert!(!sim.world.iter_entities().any(|e| matches!(
            e.get::<NativeAction>(),
            Some(NativeAction::BuildingDamageProc(_))
        )));
        assert_eq!(
            {
                sim.step();
                sim.checksum()
            },
            {
                other.step();
                other.checksum()
            }
        );
    }
    assert_eq!(
        {
            sim.step();
            sim.checksum()
        },
        {
            other.step();
            other.checksum()
        }
    );
    assert_eq!(controller(&sim, source).1.event_sequence, 0);
    sim.start_building_damage_proc(entity(&sim, source));
    assert_eq!(
        sim.world
            .iter_entities()
            .filter(|e| matches!(
                e.get::<NativeAction>(),
                Some(NativeAction::BuildingDamageProc(_))
            ))
            .count(),
        1
    );
}
