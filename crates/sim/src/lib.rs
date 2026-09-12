mod components;
mod fixture;
mod math;
mod simulation;
mod spatial;
mod topology;

pub use components::{
    AttackDelivery, AttackProfile, BuildingFootprint, BuildingSpawn, MovementProfile,
    ProductionProfile, SimId, Team, UnitSpawn, UnitTemplate,
};
pub use fixture::{populate_crossing_crowd, populate_dense_cage_battle, populate_lane_battle};
pub use math::{SUBUNITS_PER_WORLD_UNIT, SimPoint};
pub use simulation::{
    AttackEvent, BuildingPlacementError, BuildingView, ProjectileView, Simulation,
    SimulationConfig, TickResult, TickTimings, UnitView,
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
                delivery: AttackDelivery::Melee,
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
                delivery: AttackDelivery::Melee,
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
                initial_delay_ticks: 0,
                interval_ticks: 10,
                search_radius_cells,
                unit: UnitTemplate {
                    health: 100,
                    attack: AttackProfile {
                        delivery: AttackDelivery::Melee,
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
    fn topology_metrics_report_cage_opening_rebuild_and_group_release() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(15, 11),
            team_objective: [
                SimPoint::new(15 * cell + cell / 2, 5 * cell + cell / 2),
                SimPoint::new(cell / 2, 5 * cell + cell / 2),
            ],
            ..SimulationConfig::default()
        };
        config.static_blockers.clear();
        let mut sim = Simulation::new(config, 2);
        sim.spawn_building(passive_building(1, BuildingFootprint::new(8, 0, 1, 5)));
        let gate = sim.spawn_building(passive_building(1, BuildingFootprint::new(8, 5, 1, 1)));
        sim.spawn_building(passive_building(1, BuildingFootprint::new(8, 6, 1, 6)));

        let mut trapped = Vec::new();
        for y in 2..8 {
            for x in 2..6 {
                trapped.push(sim.spawn_unit(UnitSpawn {
                    team: Team(0),
                    position: SimPoint::new(x * cell + cell / 2, y * cell + cell / 2),
                    health: 100,
                    attack: AttackProfile {
                        delivery: AttackDelivery::Melee,
                        damage: 0,
                        range: 0,
                        acquisition_range: 0,
                        cooldown_ticks: 1,
                    },
                    movement: MovementProfile {
                        speed_per_tick: cell / 4,
                    },
                }));
            }
        }

        let initial_positions: Vec<_> = trapped
            .iter()
            .map(|id| sim.unit(*id).unwrap().position)
            .collect();
        let closed = sim.step();
        assert_eq!(closed.topology_rebuilds, 1);
        assert!(
            trapped
                .iter()
                .zip(&initial_positions)
                .all(|(id, position)| {
                    sim.unit(*id).is_some_and(|unit| unit.position == *position)
                })
        );

        assert!(sim.remove_building(gate));
        let opened = sim.step();
        assert_eq!(opened.topology_rebuilds, 1);
        let moved = trapped
            .iter()
            .zip(&initial_positions)
            .filter(|(id, position)| {
                sim.unit(**id)
                    .is_some_and(|unit| unit.position != **position)
            })
            .count();
        assert!(
            moved >= trapped.len() / 2,
            "opened cage should release the crowd"
        );
    }

    #[test]
    fn pursuit_metrics_count_a_star_fallback_work() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(6, 4),
            static_blockers: vec![BuildingFootprint::new(3, 0, 1, 3)],
            team_objective: [
                SimPoint::new(6 * cell, 2 * cell),
                SimPoint::new(0, 2 * cell),
            ],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(2 * cell + cell / 2, cell + cell / 2),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: cell / 2,
                acquisition_range: 4 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile {
                speed_per_tick: cell / 4,
            },
        });
        sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(4 * cell + cell / 2, cell + cell / 2),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        let result = sim.step();
        assert_eq!(result.pursuit_steps, 1);
        assert_eq!(result.a_star_fallbacks, 1);
        assert!(result.a_star_expanded_nodes > 0);
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
                delivery: AttackDelivery::Melee,
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
                delivery: AttackDelivery::Melee,
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
                delivery: AttackDelivery::Melee,
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
    fn production_spawn_respects_collision_radius_across_neighbor_cells() {
        let config = SimulationConfig::default();
        let minimum_distance = config.unit_separation_distance;
        let mut sim = Simulation::new(config, 2);
        let footprint = BuildingFootprint::new(20, 0, 1, 1);
        let preferred = SimPoint::new(
            21 * SUBUNITS_PER_WORLD_UNIT + SUBUNITS_PER_WORLD_UNIT / 2,
            SUBUNITS_PER_WORLD_UNIT / 2,
        );
        let nearby = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(
                22 * SUBUNITS_PER_WORLD_UNIT + SUBUNITS_PER_WORLD_UNIT / 100,
                SUBUNITS_PER_WORLD_UNIT / 2,
            ),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
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
        let spawned = sim
            .units()
            .into_iter()
            .find(|unit| unit.id != nearby)
            .expect("production unit missing");
        assert_ne!(spawned.position, preferred);
        assert!(
            spawned
                .position
                .distance_sq(sim.unit(nearby).unwrap().position)
                >= (i64::from(minimum_distance) * i64::from(minimum_distance)) as u64
        );
    }

    #[test]
    fn ranged_guaranteed_hit_crosses_cage_with_authoritative_travel_time() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let source = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(6 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
                },
                damage: 1,
                range: 8 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let caged = sim.spawn_unit(passive_unit(1, 10 * SUBUNITS_PER_WORLD_UNIT));
        for footprint in [
            BuildingFootprint::new(9, -1, 1, 3),
            BuildingFootprint::new(11, -1, 1, 3),
            BuildingFootprint::new(10, -1, 1, 1),
            BuildingFootprint::new(10, 1, 1, 1),
        ] {
            sim.spawn_building(passive_building(1, footprint));
        }

        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(caged));
        let launch = sim.step();
        assert_eq!(launch.completed_tick, 1);
        assert_eq!(launch.projectiles_launched, 1);
        assert_eq!(launch.projectiles_alive, 1);
        assert_eq!(launch.projectile_impacts, 0);
        assert_eq!(sim.unit(caged).unwrap().health, 10_000);
        assert_eq!(sim.attacks_last_tick().len(), 1);
        let projectile = sim.projectiles()[0];
        assert_eq!(projectile.launch_tick, 1);
        assert_eq!(projectile.impact_tick, 5);
        assert_eq!(projectile.source, source);
        assert_eq!(projectile.target, caged);

        for expected_tick in 2..5 {
            let in_flight = sim.step();
            assert_eq!(in_flight.completed_tick, expected_tick);
            assert_eq!(in_flight.projectile_impacts, 0);
            assert_eq!(in_flight.projectiles_alive, 1);
            assert_eq!(sim.unit(caged).unwrap().health, 10_000);
        }

        let impact = sim.step();
        assert_eq!(impact.completed_tick, 5);
        assert_eq!(impact.projectile_impacts, 1);
        assert_eq!(impact.projectile_invalidations, 0);
        assert_eq!(impact.projectiles_alive, 0);
        assert_eq!(sim.unit(caged).unwrap().health, 9_999);
    }

    #[test]
    fn guaranteed_hit_projectile_survives_source_death() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let source = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(0, 0),
            health: 10,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
                },
                damage: 1,
                range: 8 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let target = sim.spawn_unit(passive_unit(1, 6 * SUBUNITS_PER_WORLD_UNIT));

        sim.step();
        let launch = sim.step();
        assert_eq!(launch.projectiles_launched, 1);
        assert_eq!(sim.projectiles()[0].impact_tick, 7);

        sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 10,
                range: 2 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 2 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert!(sim.unit(source).is_none());
        assert_eq!(sim.projectile_count(), 1);

        while sim.tick() <= 7 {
            let result = sim.step();
            if result.completed_tick == 7 {
                assert_eq!(result.projectile_impacts, 1);
            }
        }
        assert_eq!(sim.unit(target).unwrap().health, 9_999);
        assert_eq!(sim.projectile_count(), 0);
    }

    #[test]
    fn guaranteed_hit_projectile_invalidates_if_target_dies_before_impact() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(0, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
                },
                damage: 1,
                range: 8 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let target = sim.spawn_unit(UnitSpawn {
            health: 1,
            ..passive_unit(1, 4 * SUBUNITS_PER_WORLD_UNIT)
        });

        sim.step();
        let launch = sim.step();
        assert_eq!(launch.projectiles_launched, 1);
        assert_eq!(sim.projectiles()[0].impact_tick, 5);

        sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(3 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 2 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert!(sim.unit(target).is_none());
        assert_eq!(sim.projectile_count(), 1);

        sim.step();
        let invalidated = sim.step();
        assert_eq!(invalidated.completed_tick, 5);
        assert_eq!(invalidated.projectile_impacts, 0);
        assert_eq!(invalidated.projectile_invalidations, 1);
        assert_eq!(invalidated.projectiles_alive, 0);
    }

    #[test]
    fn static_blockers_reject_building_placement() {
        let config = SimulationConfig {
            static_blockers: vec![BuildingFootprint::new(20, 0, 2, 2)],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 1);
        let result =
            sim.try_spawn_building(passive_building(0, BuildingFootprint::new(21, 1, 1, 1)));
        assert_eq!(result, Err(BuildingPlacementError::StaticObstacle));
    }

    #[test]
    fn live_unit_blocks_building_placement() {
        let mut sim = Simulation::new(SimulationConfig::default(), 1);
        sim.spawn_unit(passive_unit(0, 20 * SUBUNITS_PER_WORLD_UNIT));
        let result =
            sim.try_spawn_building(passive_building(0, BuildingFootprint::new(20, 0, 1, 1)));
        assert_eq!(result, Err(BuildingPlacementError::UnitOccupied));
    }

    #[test]
    fn production_can_delay_its_first_spawn() {
        let mut sim = Simulation::new(SimulationConfig::default(), 1);
        let mut building = production_building(0, BuildingFootprint::new(20, 0, 1, 1), 1);
        building
            .production
            .as_mut()
            .expect("production profile missing")
            .initial_delay_ticks = 3;
        sim.spawn_building(building);

        for _ in 0..3 {
            assert_eq!(sim.step().units_spawned, 0);
        }
        assert_eq!(sim.step().units_spawned, 1);
    }

    #[test]
    fn exact_overlap_separates_units() {
        let mut sim = Simulation::new(SimulationConfig::default(), 4);
        let moving = |team| UnitSpawn {
            team: Team(team),
            position: SimPoint::new(20 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
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
    fn collision_is_global_across_disconnected_navigation_components() {
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(2, 2),
            static_blockers: vec![
                BuildingFootprint::new(1, 0, 1, 1),
                BuildingFootprint::new(0, 1, 1, 1),
            ],
            team_objective: [
                SimPoint::new(2 * SUBUNITS_PER_WORLD_UNIT, 2 * SUBUNITS_PER_WORLD_UNIT),
                SimPoint::new(0, 0),
            ],
            ..SimulationConfig::default()
        };
        let minimum_distance = config.unit_separation_distance;
        let mut sim = Simulation::new(config, 2);
        let spawn = |position| UnitSpawn {
            team: Team(0),
            position,
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        };
        let first = sim.spawn_unit(spawn(SimPoint::new(
            SUBUNITS_PER_WORLD_UNIT - 16,
            SUBUNITS_PER_WORLD_UNIT - 16,
        )));
        let second = sim.spawn_unit(spawn(SimPoint::new(
            SUBUNITS_PER_WORLD_UNIT + 16,
            SUBUNITS_PER_WORLD_UNIT + 16,
        )));

        sim.step();
        let distance_sq = sim
            .unit(first)
            .unwrap()
            .position
            .distance_sq(sim.unit(second).unwrap().position);
        assert!(distance_sq >= (i64::from(minimum_distance) * i64::from(minimum_distance)) as u64);
    }

    #[test]
    fn converging_crowd_never_commits_overlapping_units() {
        let mut config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(40, 20),
            team_objective: [
                SimPoint::new(40 * SUBUNITS_PER_WORLD_UNIT, 10 * SUBUNITS_PER_WORLD_UNIT),
                SimPoint::new(0, 10 * SUBUNITS_PER_WORLD_UNIT),
            ],
            ..SimulationConfig::default()
        };
        config.unit_separation_distance = 3 * SUBUNITS_PER_WORLD_UNIT / 4;
        let minimum_distance_sq = {
            let distance = i64::from(config.unit_separation_distance);
            (distance * distance) as u64
        };
        let mut sim = Simulation::new(config, 4);
        let template = UnitTemplate {
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 4,
            },
        };

        for y in 1..=10 {
            for x in 1..=10 {
                sim.spawn_unit(UnitSpawn::from_template(
                    Team(0),
                    SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, y * SUBUNITS_PER_WORLD_UNIT),
                    template,
                ));
            }
        }

        for _ in 0..300 {
            sim.step();
            let units = sim.units();
            for (index, unit) in units.iter().enumerate() {
                for other in &units[index + 1..] {
                    assert!(
                        unit.position.distance_sq(other.position) >= minimum_distance_sq,
                        "units {:?} and {:?} overlap at tick {}: {:?} vs {:?}",
                        unit.id,
                        other.id,
                        sim.tick(),
                        unit.position,
                        other.position,
                    );
                }
            }
        }
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
    fn attacker_preempts_passive_building_target() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let castle = sim.spawn_building(passive_building(1, BuildingFootprint::new(11, 0, 1, 1)));
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(castle));

        let attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(9 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(castle));
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(attacker));
    }

    #[test]
    fn nearby_ally_attack_preempts_building_target() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 8,
            },
        });
        let ally = sim.spawn_unit(passive_unit(0, 12 * SUBUNITS_PER_WORLD_UNIT));
        let castle = sim.spawn_building(passive_building(1, BuildingFootprint::new(11, 0, 1, 1)));
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(castle));

        let attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(12 * SUBUNITS_PER_WORLD_UNIT, SUBUNITS_PER_WORLD_UNIT),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert_eq!(sim.unit(attacker).unwrap().target, Some(ally));
        assert_eq!(sim.unit(defender).unwrap().target, Some(castle));
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(attacker));
    }

    #[test]
    fn castle_attackers_peel_to_arriving_defender_after_attack() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let castle = sim.spawn_building(passive_building(0, BuildingFootprint::new(10, -1, 2, 3)));
        let castle_attacker = |y: i32| UnitSpawn {
            team: Team(1),
            position: SimPoint::new(8 * cell, y),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 3 * cell,
                acquisition_range: 8 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        };
        let directly_attacked = sim.spawn_unit(castle_attacker(0));
        let nearby_attacker = sim.spawn_unit(castle_attacker(2 * cell));

        sim.step();
        sim.step();
        assert_eq!(sim.unit(directly_attacked).unwrap().target, Some(castle));
        assert_eq!(sim.unit(nearby_attacker).unwrap().target, Some(castle));

        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(7 * cell, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step(); // defender is spawn-tick suppressed
        assert_eq!(sim.unit(defender).unwrap().target, Some(directly_attacked));
        assert_eq!(sim.unit(directly_attacked).unwrap().target, Some(castle));
        assert_eq!(sim.unit(nearby_attacker).unwrap().target, Some(castle));

        sim.step(); // defender actually attacks the first castle attacker
        assert_eq!(
            sim.unit(directly_attacked).unwrap().last_attacker,
            Some(defender)
        );
        assert_eq!(sim.unit(directly_attacked).unwrap().target, Some(castle));
        assert_eq!(sim.unit(nearby_attacker).unwrap().target, Some(castle));

        sim.step(); // direct retaliation + nearby ally defense both peel off the castle
        assert_eq!(sim.unit(directly_attacked).unwrap().target, Some(defender));
        assert_eq!(sim.unit(nearby_attacker).unwrap().target, Some(defender));
    }

    #[test]
    fn ally_defense_orders_nearest_ally_then_nearest_attacker_when_idle() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            team_objective: [SimPoint::new(10 * cell, 0), SimPoint::new(0, 0)],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 4);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 3 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: cell / 8,
            },
        });
        let near_ally = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 3 * cell),
            ..passive_unit(0, 0)
        });
        let far_ally = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(14 * cell, 0),
            ..passive_unit(0, 0)
        });
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, None);

        let attacker_for_near_ally_farther = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(10 * cell, 5 * cell),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 3 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let attacker_for_near_ally_closer = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(11 * cell, 4 * cell),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 3 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let attacker_for_far_ally = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(15 * cell, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 3 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, None);
        sim.step();
        assert_eq!(
            sim.unit(attacker_for_near_ally_farther).unwrap().target,
            Some(near_ally)
        );
        assert_eq!(
            sim.unit(attacker_for_near_ally_closer).unwrap().target,
            Some(near_ally)
        );
        assert_eq!(
            sim.unit(attacker_for_far_ally).unwrap().target,
            Some(far_ally)
        );
        assert_eq!(sim.unit(defender).unwrap().target, None);
        sim.step();
        assert_eq!(
            sim.unit(defender).unwrap().target,
            Some(attacker_for_near_ally_closer)
        );
    }

    #[test]
    fn ally_defense_target_stays_sticky_when_other_allies_are_attacked() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            team_objective: [SimPoint::new(10 * cell, 0), SimPoint::new(0, 0)],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 3 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: cell / 8,
            },
        });
        let right_ally = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 3 * cell),
            ..passive_unit(0, 0)
        });
        let left_ally = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(6 * cell, 0),
            ..passive_unit(0, 0)
        });
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, None);

        let right_attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(10 * cell, 5 * cell),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 3 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert_eq!(sim.unit(right_attacker).unwrap().target, Some(right_ally));
        assert_eq!(sim.unit(defender).unwrap().target, None);
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(right_attacker));

        let left_attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(4 * cell, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 3 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert_eq!(sim.unit(left_attacker).unwrap().target, Some(left_ally));
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(right_attacker));
        assert_ne!(sim.unit(defender).unwrap().target, Some(left_attacker));
    }

    #[test]
    fn first_personal_attacker_stays_locked_despite_later_attackers() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 0),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 2 * cell,
                acquisition_range: 8 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let passive = sim.spawn_unit(passive_unit(1, 12 * cell));
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(passive));

        let first_attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(9 * cell, 0),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 100,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let later_attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(11 * cell, 0),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step();
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(passive));
        assert_eq!(
            sim.unit(defender).unwrap().last_attacker,
            Some(first_attacker)
        );
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(first_attacker));
        assert_eq!(
            sim.unit(defender).unwrap().last_attacker,
            Some(later_attacker)
        );
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(first_attacker));
    }

    #[test]
    fn lethal_hit_still_alerts_idle_nearby_ally() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 3 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: cell / 8,
            },
        });
        let ally = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 3 * cell),
            health: 1,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, None);

        let attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(10 * cell, 5 * cell),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 3 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert!(sim.unit(ally).is_none());
        assert_eq!(sim.unit(defender).unwrap().target, None);
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(attacker));
    }

    #[test]
    fn mutual_fight_ignores_nearby_ally_defense_alert() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 2 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let mutual = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(11 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 2 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let ally = sim.spawn_unit(passive_unit(0, 13 * SUBUNITS_PER_WORLD_UNIT));
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(mutual));
        assert_eq!(sim.unit(mutual).unwrap().target, Some(defender));

        let attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(13 * SUBUNITS_PER_WORLD_UNIT, SUBUNITS_PER_WORLD_UNIT),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert_eq!(sim.unit(attacker).unwrap().target, Some(ally));
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(mutual));
    }

    #[test]
    fn trailing_melee_units_sidestep_around_engaged_frontline() {
        let mut sim = Simulation::new(SimulationConfig::default(), 4);
        let moving_melee = |team: u8, x: i32| UnitSpawn {
            team: Team(team),
            position: SimPoint::new(x, 0),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 8,
            },
        };
        let spacing = 3 * SUBUNITS_PER_WORLD_UNIT / 4;
        let front = sim.spawn_unit(moving_melee(0, 20 * SUBUNITS_PER_WORLD_UNIT));
        let rear_a = sim.spawn_unit(moving_melee(0, 20 * SUBUNITS_PER_WORLD_UNIT - spacing));
        let rear_b = sim.spawn_unit(moving_melee(0, 20 * SUBUNITS_PER_WORLD_UNIT - 2 * spacing));
        let enemy_front = sim.spawn_unit(moving_melee(1, 24 * SUBUNITS_PER_WORLD_UNIT));
        sim.spawn_unit(moving_melee(1, 24 * SUBUNITS_PER_WORLD_UNIT + spacing));
        sim.spawn_unit(moving_melee(1, 24 * SUBUNITS_PER_WORLD_UNIT + 2 * spacing));

        let mut rear_attacked = false;
        let mut lateral_displacement = 0_i32;
        for _ in 0..240 {
            sim.step();
            for rear in [rear_a, rear_b] {
                lateral_displacement =
                    lateral_displacement.max(sim.unit(rear).unwrap().position.y.abs());
            }
            rear_attacked |= sim
                .attacks_last_tick()
                .iter()
                .any(|attack| attack.source == rear_a || attack.source == rear_b);
            if rear_attacked {
                break;
            }
        }

        assert_eq!(sim.unit(front).unwrap().target, Some(enemy_front));
        assert!(
            lateral_displacement >= SUBUNITS_PER_WORLD_UNIT / 8,
            "rear units never sidestepped around the engaged front"
        );
        assert!(rear_attacked, "rear units never reached an attack position");
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
