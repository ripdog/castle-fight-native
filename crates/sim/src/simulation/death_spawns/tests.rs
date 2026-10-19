use super::*;
use crate::{CastleFightUnitKind, MapVersion};
use std::collections::BTreeSet;

fn simulation(workers: usize) -> Simulation {
    Simulation::new(SimulationConfig::default(), workers)
}

fn entity(sim: &Simulation, id: SimId) -> Entity {
    sim.world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}

fn restored(sim: &Simulation) -> Simulation {
    let content = crate::castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), content)
            .unwrap();
    let mut restored = simulation(4);
    restored.restore_snapshot(&snapshot).unwrap();
    restored
}

#[test]
fn native_death_children_initialize_full_definitions_and_independent_fresh_state() {
    let mut sim = simulation(1);
    let version = MapVersion::CASTLE_FIGHT_9_27;
    let definition = CastleFightUnitKind::Warlock
        .definition_for_version(version)
        .unwrap()
        .resolved();
    let position = SimPoint::new(40 * SUBUNITS_PER_WORLD_UNIT, 0);
    let profile = NativeDeathSpawn {
        ability: AbilityId(123),
        child_rawcode: definition.properties.content.unwrap().rawcode,
        count: 3,
    };
    assert_eq!(
        sim.spawn_native_death_children(PlayerId(1), Team(1), position, version, profile),
        3
    );
    let ids: BTreeSet<_> = sim.units().iter().map(|u| u.id).collect();
    assert_eq!(ids.len(), 3);
    for child in sim.units() {
        assert_eq!(
            (child.owner, child.team, child.position),
            (PlayerId(1), Team(1), position)
        );
        assert_eq!(child.health, definition.template.health);
        assert_eq!(child.attack, definition.template.attack);
        assert_eq!(child.status.poison, None);
        assert_eq!(child.status.spawn_building, None);
        assert_eq!(child.status.summon_order_refresh_tick, None);
        assert!(child.classifications.summoned);
        assert_eq!(
            child.mana_current,
            definition.spellcasting.map(|p| p.mana.starting)
        );
        assert_eq!(child.ability_cast_sequence, Some(0));
    }
    let mut copy = restored(&sim);
    for _ in 0..4 {
        assert_eq!(
            {
                sim.step();
                sim.checksum()
            },
            {
                copy.step();
                copy.checksum()
            }
        );
    }
}

#[test]
fn promoted_death_spawn_catalog_closes_children_and_splits_once_across_wire() {
    let version = MapVersion::CASTLE_FIGHT_9_27;
    let profiles: Vec<_> = crate::native_effects::death_spawns_for_version(version).collect();
    assert!(!profiles.is_empty());
    for kind in CastleFightUnitKind::ALL {
        let definition = kind.definition_for_version(version).unwrap();
        let matching: Vec<_> = profiles
            .iter()
            .filter(|p| {
                crate::content::unit_has_ability_for_version(definition.rawcode, p.ability, version)
            })
            .collect();
        if matching.is_empty() {
            continue;
        }
        let mut sim = simulation(1);
        let source = sim.spawn_resolved_unit_for_player(
            PlayerId(1),
            Team(1),
            SimPoint::new(40 * SUBUNITS_PER_WORLD_UNIT, 0),
            definition.resolved(),
        );
        sim.world
            .get_mut::<StatusState>(entity(&sim, source))
            .unwrap()
            .spawn_building = Some(SimId(900));
        sim.debug_damage_all_units(1);
        assert_eq!(sim.unit_count(), 1, "nonfatal damage cannot split");
        let mut copy = restored(&sim);
        let damage = definition.health;
        sim.debug_damage_all_units(damage);
        copy.debug_damage_all_units(damage);
        let expected: usize = matching.iter().map(|p| usize::from(p.count)).sum();
        assert_eq!(sim.unit_count(), expected);
        assert!(sim.unit(source).is_none());
        for child in sim.units() {
            assert!(
                matching
                    .iter()
                    .any(|p| Some(p.child_rawcode) == child.content.map(|c| c.rawcode))
            );
            assert!(child.classifications.summoned);
            assert_eq!(child.owner, PlayerId(1));
            assert_eq!(child.status.spawn_building, None);
            assert_eq!(child.status.summon_order_refresh_tick, None);
        }
        for _ in 0..4 {
            assert_eq!(
                {
                    sim.step();
                    sim.checksum()
                },
                {
                    copy.step();
                    copy.checksum()
                }
            );
        }
        let descendants: usize = sim
            .units()
            .iter()
            .map(|u| {
                profiles
                    .iter()
                    .filter(|p| {
                        crate::content::unit_has_ability_for_version(
                            u.content.unwrap().rawcode,
                            p.ability,
                            version,
                        )
                    })
                    .map(|p| usize::from(p.count))
                    .sum::<usize>()
            })
            .sum();
        sim.debug_damage_all_units(i32::MAX / 2);
        copy.debug_damage_all_units(i32::MAX / 2);
        assert_eq!(
            sim.unit_count(),
            descendants,
            "new children are not recursively killed in the same boundary"
        );
        for _ in 0..4 {
            assert_eq!(
                {
                    sim.step();
                    sim.checksum()
                },
                {
                    copy.step();
                    copy.checksum()
                }
            );
        }
    }
}

#[test]
fn illusions_and_handle_removal_do_not_run_native_death_spawn() {
    let version = MapVersion::CASTLE_FIGHT_9_27;
    for kind in CastleFightUnitKind::ALL {
        let definition = kind.definition_for_version(version).unwrap();
        if !crate::native_effects::death_spawns_for_version(version).any(|p| {
            crate::content::unit_has_ability_for_version(definition.rawcode, p.ability, version)
        }) {
            continue;
        }
        for illusion in [false, true] {
            let mut sim = simulation(1);
            let mut body = definition.resolved();
            body.properties.classifications.illusion = illusion;
            let id = sim.spawn_resolved_unit(
                Team(0),
                SimPoint::new(40 * SUBUNITS_PER_WORLD_UNIT, 0),
                body,
            );
            if illusion {
                sim.debug_damage_all_units(definition.health);
            } else {
                sim.world.despawn(entity(&sim, id));
            }
            assert_eq!(sim.unit_count(), 0);
            sim.step();
            assert_eq!(sim.unit_count(), 0);
        }
    }
}

#[test]
fn combat_death_creates_children_in_the_commit_and_counts_them_once() {
    let version = MapVersion::CASTLE_FIGHT_9_27;
    for kind in CastleFightUnitKind::ALL {
        let definition = kind.definition_for_version(version).unwrap();
        let expected: usize = crate::native_effects::death_spawns_for_version(version)
            .filter(|p| {
                crate::content::unit_has_ability_for_version(definition.rawcode, p.ability, version)
            })
            .map(|p| usize::from(p.count))
            .sum();
        if expected == 0 {
            continue;
        }
        let mut sim = simulation(1);
        let mut body = definition.resolved();
        body.template.health = 1;
        body.template.movement.speed_per_tick = 0;
        body.template.attack.damage = 0;
        let source = sim.spawn_resolved_unit(
            Team(1),
            SimPoint::new(40 * SUBUNITS_PER_WORLD_UNIT, 0),
            body,
        );
        sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(20 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 1000,
            attack: AttackProfile {
                damage: 10000,
                range: 100 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 100 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1000,
                delivery: AttackDelivery::RangedInstant,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        let mut copy = restored(&sim);
        let tick = sim.step();
        assert_eq!(sim.checksum(), {
            copy.step();
            copy.checksum()
        });
        assert!(sim.unit(source).is_none());
        assert_eq!(tick.deaths, 1);
        assert_eq!(tick.units_spawned, expected);
        assert_eq!(tick.units_alive, expected + 1);
        assert_eq!(sim.unit_count(), expected + 1);
        let next = sim.step();
        assert_eq!(sim.checksum(), {
            copy.step();
            copy.checksum()
        });
        assert_eq!(next.units_spawned, 0);
    }
}
