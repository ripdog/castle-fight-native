mod components;
mod fixture;
mod math;
mod simulation;
mod spatial;
mod topology;

pub use components::{
    AttackProfile, BuildingFootprint, BuildingSpawn, MovementProfile, ProductionProfile, SimId,
    Team, UnitSpawn, UnitTemplate,
};
pub use fixture::{populate_crossing_crowd, populate_dense_cage_battle, populate_lane_battle};
pub use math::{SUBUNITS_PER_WORLD_UNIT, SimPoint};
pub use simulation::{
    BuildingView, Simulation, SimulationConfig, TickResult, TickTimings, UnitView,
};
pub use topology::NavCell;

#[cfg(test)]
mod tests {
    use super::*;

    fn duel_unit(team: u8, x: i32, damage: i32, health: i32) -> UnitSpawn {
        UnitSpawn {
            team: Team(team),
            position: SimPoint::new(x, 0),
            health,
            attack: AttackProfile {
                damage,
                range: 4 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 10,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        }
    }

    fn passive_unit(team: u8, x: i32) -> UnitSpawn {
        UnitSpawn {
            team: Team(team),
            position: SimPoint::new(x, 0),
            health: 10_000,
            attack: AttackProfile {
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        }
    }

    fn passive_building(team: u8, footprint: BuildingFootprint) -> BuildingSpawn {
        BuildingSpawn {
            team: Team(team),
            footprint,
            health: 10_000,
            production: None,
        }
    }

    fn production_building(
        team: u8,
        footprint: BuildingFootprint,
        search_radius_cells: u16,
    ) -> BuildingSpawn {
        BuildingSpawn {
            team: Team(team),
            footprint,
            health: 10_000,
            production: Some(ProductionProfile {
                interval_ticks: 10,
                search_radius_cells,
                unit: UnitTemplate {
                    health: 100,
                    attack: AttackProfile {
                        damage: 1,
                        range: SUBUNITS_PER_WORLD_UNIT,
                        acquisition_range: 3 * SUBUNITS_PER_WORLD_UNIT,
                        cooldown_ticks: 10,
                    },
                    movement: MovementProfile { speed_per_tick: 0 },
                },
            }),
        }
    }

    #[test]
    fn units_do_not_attack_on_spawn_tick() {
        let mut sim = Simulation::new(SimulationConfig::default(), 1);
        let a = sim.spawn_unit(duel_unit(0, 0, 10, 100));
        let b = sim.spawn_unit(duel_unit(1, SUBUNITS_PER_WORLD_UNIT, 10, 100));

        let first = sim.step();
        assert_eq!(first.attacks_resolved, 0);
        assert_eq!(sim.unit(a).unwrap().health, 100);
        assert_eq!(sim.unit(b).unwrap().health, 100);

        let second = sim.step();
        assert_eq!(second.attacks_resolved, 2);
        assert_eq!(sim.unit(a).unwrap().health, 90);
        assert_eq!(sim.unit(b).unwrap().health, 90);
    }

    #[test]
    fn death_cancels_later_attack_in_canonical_order() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let first = sim.spawn_unit(duel_unit(0, 0, 100, 100));
        let second = sim.spawn_unit(duel_unit(1, SUBUNITS_PER_WORLD_UNIT, 100, 100));

        sim.step();
        let result = sim.step();

        assert_eq!(result.attacks_resolved, 1);
        assert_eq!(result.deaths, 1);
        assert_eq!(sim.unit(first).unwrap().health, 100);
        assert!(sim.unit(second).is_none());
    }

    #[test]
    fn exact_target_tie_uses_lowest_sim_id() {
        let mut sim = Simulation::new(SimulationConfig::default(), 3);
        let source = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                damage: 0,
                range: 0,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1,
            },
            movement: MovementProfile {
                speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 8,
            },
        });
        let lower = sim.spawn_unit(duel_unit(1, 9 * SUBUNITS_PER_WORLD_UNIT, 0, 100));
        let _higher = sim.spawn_unit(duel_unit(1, 11 * SUBUNITS_PER_WORLD_UNIT, 0, 100));

        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(lower));
    }

    #[test]
    fn sticky_target_does_not_switch_to_new_non_attacker() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let source = sim.spawn_unit(duel_unit(0, 0, 0, 100));
        let first = sim.spawn_unit(passive_unit(1, 3 * SUBUNITS_PER_WORLD_UNIT));

        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(first));

        let _closer = sim.spawn_unit(passive_unit(1, SUBUNITS_PER_WORLD_UNIT));
        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(first));
    }

    #[test]
    fn fresh_attacker_preempts_target_that_is_not_fighting_back() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let source = sim.spawn_unit(duel_unit(0, 0, 0, 1_000));
        let passive = sim.spawn_unit(passive_unit(1, 3 * SUBUNITS_PER_WORLD_UNIT));
        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(passive));

        let attacker = sim.spawn_unit(duel_unit(1, SUBUNITS_PER_WORLD_UNIT, 1, 1_000));
        sim.step(); // attacker is spawn-tick suppressed
        sim.step(); // attacker hits source
        assert_eq!(sim.unit(source).unwrap().target, Some(passive));
        assert_eq!(sim.unit(source).unwrap().last_attacker, Some(attacker));
        assert_eq!(sim.unit(passive).unwrap().target, None);
        assert_eq!(sim.unit(attacker).unwrap().target, Some(source));
        sim.step(); // source reacts on the next targeting phase
        assert_eq!(sim.unit(source).unwrap().target, Some(attacker));
    }

    #[test]
    fn fresh_attacker_does_not_break_mutual_engagement() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let source = sim.spawn_unit(duel_unit(0, 0, 0, 1_000));
        let mutual = sim.spawn_unit(duel_unit(1, 2 * SUBUNITS_PER_WORLD_UNIT, 0, 1_000));
        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(mutual));
        assert_eq!(sim.unit(mutual).unwrap().target, Some(source));

        let attacker = sim.spawn_unit(duel_unit(1, SUBUNITS_PER_WORLD_UNIT, 1, 1_000));
        sim.step();
        sim.step();
        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(mutual));
        assert_ne!(sim.unit(source).unwrap().target, Some(attacker));
    }

    #[test]
    fn unreachable_caged_unit_is_ignored_in_favor_of_cage_building() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let source = sim.spawn_unit(duel_unit(0, 6 * SUBUNITS_PER_WORLD_UNIT, 0, 1_000));
        let _caged = sim.spawn_unit(passive_unit(1, 10 * SUBUNITS_PER_WORLD_UNIT));
        let walls = [
            BuildingFootprint::new(9, -1, 1, 3),
            BuildingFootprint::new(11, -1, 1, 3),
            BuildingFootprint::new(10, -1, 1, 1),
            BuildingFootprint::new(10, 1, 1, 1),
        ];
        let first_wall = sim.spawn_building(passive_building(1, walls[0]));
        for footprint in walls.into_iter().skip(1) {
            sim.spawn_building(passive_building(1, footprint));
        }

        sim.step();
        let target = sim.unit(source).unwrap().target;
        assert!(target.is_some());
        assert_ne!(
            target,
            sim.units()
                .into_iter()
                .find(|unit| unit.team == Team(1))
                .map(|u| u.id)
        );
        assert!(sim.building(target.unwrap()).is_some());
        assert!(sim.building(first_wall).is_some());
    }

    #[test]
    fn reachable_enemy_unit_beats_passive_cage_building() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let source = sim.spawn_unit(duel_unit(0, 6 * SUBUNITS_PER_WORLD_UNIT, 0, 1_000));
        let reachable = sim.spawn_unit(passive_unit(1, 7 * SUBUNITS_PER_WORLD_UNIT));
        let _wall = sim.spawn_building(passive_building(1, BuildingFootprint::new(8, 0, 1, 1)));

        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(reachable));
    }

    #[test]
    fn bounded_spawn_search_fails_without_backlog_when_full() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let footprint = BuildingFootprint::new(20, 0, 1, 1);
        let preferred = SimPoint::new(
            21 * SUBUNITS_PER_WORLD_UNIT + SUBUNITS_PER_WORLD_UNIT / 2,
            SUBUNITS_PER_WORLD_UNIT / 2,
        );
        sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: preferred,
            health: 100,
            attack: AttackProfile {
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.spawn_building(production_building(0, footprint, 0));

        let first = sim.step();
        assert_eq!(first.units_spawned, 0);
        assert_eq!(first.spawn_failures, 1);
        assert_eq!(sim.unit_count(), 1);

        for _ in 0..9 {
            let tick = sim.step();
            assert_eq!(tick.units_spawned, 0);
            assert_eq!(tick.spawn_failures, 0);
        }
        let retry = sim.step();
        assert_eq!(retry.units_spawned, 0);
        assert_eq!(retry.spawn_failures, 1);
        assert_eq!(sim.unit_count(), 1);
    }

    #[test]
    fn spawn_search_uses_deterministic_expanding_spiral() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let footprint = BuildingFootprint::new(20, 0, 1, 1);
        let preferred = SimPoint::new(
            21 * SUBUNITS_PER_WORLD_UNIT + SUBUNITS_PER_WORLD_UNIT / 2,
            SUBUNITS_PER_WORLD_UNIT / 2,
        );
        let blocker = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: preferred,
            health: 100,
            attack: AttackProfile {
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.spawn_building(production_building(0, footprint, 1));

        let result = sim.step();
        assert_eq!(result.units_spawned, 1);
        assert_eq!(result.spawn_failures, 0);
        let spawned = sim
            .units()
            .into_iter()
            .find(|unit| unit.id != blocker)
            .expect("production unit missing");
        assert_eq!(
            spawned.position,
            SimPoint::new(
                20 * SUBUNITS_PER_WORLD_UNIT + SUBUNITS_PER_WORLD_UNIT / 2,
                -SUBUNITS_PER_WORLD_UNIT / 2,
            )
        );
    }

    #[test]
    fn exact_overlap_separates_units() {
        let mut sim = Simulation::new(SimulationConfig::default(), 4);
        let moving = |team| UnitSpawn {
            team: Team(team),
            position: SimPoint::new(20 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 8,
            },
        };
        let first = sim.spawn_unit(moving(0));
        let second = sim.spawn_unit(moving(0));

        sim.step();
        assert_ne!(
            sim.unit(first).unwrap().position,
            sim.unit(second).unwrap().position
        );
    }

    #[test]
    fn crowd_separation_is_worker_count_independent() {
        let mut expected = None;
        for workers in [1, 2, 4, 8] {
            let mut sim = Simulation::new(SimulationConfig::default(), workers);
            populate_crossing_crowd(&mut sim, 2_048);
            for _ in 0..100 {
                sim.step();
            }

            match expected {
                Some(checksum) => assert_eq!(sim.checksum(), checksum, "workers={workers}"),
                None => expected = Some(sim.checksum()),
            }
        }
    }

    #[test]
    fn worker_count_does_not_change_battle_checksum() {
        let mut expected = None;
        for workers in [1, 2, 4] {
            let mut sim = Simulation::new(SimulationConfig::default(), workers);
            populate_lane_battle(&mut sim, 512);
            for _ in 0..200 {
                sim.step();
            }

            match expected {
                Some(checksum) => assert_eq!(sim.checksum(), checksum, "workers={workers}"),
                None => expected = Some(sim.checksum()),
            }
        }
    }
}
