use std::{
    collections::BTreeMap,
    env,
    time::{Duration, Instant},
};

use castle_fight_sim::{
    AbilityEffect, AbilityId, AbilityTargetPolicy, AttackDelivery, AttackProfile,
    AutomaticAbilityProfile, BuildingFootprint, BuildingSpawn, CollisionRadius, ManaProfile,
    ModifierId, MovementProfile, ProductionProfile, SUBUNITS_PER_WORLD_UNIT, SimId, SimPoint,
    Simulation, SimulationConfig, SpellcastingProfile, Team, TickResult, TickTimings, UnitSpawn,
    UnitTemplate, populate_crossing_crowd, populate_dense_cage_battle, populate_lane_battle,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Scenario {
    Lane,
    Cage,
    Crowd,
    Pathing,
    Topology,
    Production,
    Projectile,
    Ballistic,
    Bounce,
    Tower,
    Ability,
    Stun,
    Slow,
    Radius,
    Traffic,
    TrafficFlow,
    TrafficProduction,
    Mixed,
}

impl Scenario {
    const fn name(self) -> &'static str {
        match self {
            Self::Lane => "lane",
            Self::Cage => "cage",
            Self::Crowd => "crowd",
            Self::Pathing => "pathing",
            Self::Topology => "topology",
            Self::Production => "production",
            Self::Projectile => "projectile",
            Self::Ballistic => "ballistic",
            Self::Bounce => "bounce",
            Self::Tower => "tower",
            Self::Ability => "ability",
            Self::Stun => "stun",
            Self::Slow => "slow",
            Self::Radius => "radius",
            Self::Traffic => "traffic",
            Self::TrafficFlow => "traffic-flow",
            Self::TrafficProduction => "traffic-production",
            Self::Mixed => "mixed",
        }
    }
}

#[derive(Debug)]
struct Args {
    scenarios: Vec<Scenario>,
    units: Vec<usize>,
    workers: Vec<usize>,
    ticks: u64,
    warmup: u64,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            scenarios: vec![Scenario::Lane, Scenario::Cage, Scenario::Crowd],
            units: vec![700, 1_000, 5_000, 10_000],
            workers: vec![1, 2, 4],
            ticks: 200,
            warmup: 20,
        }
    }
}

fn main() {
    let args = parse_args();
    println!("castle-fight deterministic simulation benchmark");
    println!(
        "scenarios={:?} units={:?} workers={:?} ticks={} warmup={}",
        args.scenarios, args.units, args.workers, args.ticks, args.warmup
    );

    let mut expected_checksums = BTreeMap::<(Scenario, usize), u64>::new();
    let mut mismatch = false;

    for &scenario in &args.scenarios {
        println!();
        println!("scenario={}", scenario.name());
        println!(
            "{:>8} {:>7} {:>9} {:>9} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>8} {:>8} {:>10} {:>8} {:>8} {:>18}",
            "units",
            "workers",
            "ms/tick",
            "ticks/s",
            "topo",
            "timer",
            "prod",
            "spatial",
            "ability",
            "target",
            "combat",
            "intent",
            "collide",
            "impact",
            "commit",
            "hash",
            "astar%",
            "cache%",
            "nodes/tick",
            "spawn/t",
            "fail/t",
            "state-hash",
        );

        for &units in &args.units {
            assert!(
                units >= 2 && units.is_multiple_of(2),
                "unit counts must be even and >= 2"
            );

            for &workers in &args.workers {
                assert!(workers > 0, "worker counts must be positive");
                let result = run_case(scenario, units, workers, args.warmup, args.ticks);

                let expected = expected_checksums
                    .entry((scenario, units))
                    .or_insert(result.checksum);
                if *expected != result.checksum {
                    mismatch = true;
                }

                println!(
                    "{:>8} {:>7} {:>9.3} {:>9.1} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.2} {:>7.2} {:>10.1} {:>8.2} {:>8.2} {:>18x}{}",
                    units,
                    workers,
                    result.ms_per_tick,
                    result.ticks_per_second,
                    result.phase_ms.topology,
                    result.phase_ms.timers,
                    result.phase_ms.production,
                    result.phase_ms.snapshot_and_spatial,
                    result.phase_ms.abilities,
                    result.phase_ms.targeting,
                    result.phase_ms.combat,
                    result.phase_ms.movement_intent,
                    result.phase_ms.crowd_and_collision,
                    result.phase_ms.ballistic_impact,
                    result.phase_ms.structural_commit,
                    result.phase_ms.checksum,
                    result.a_star_fallback_percent(),
                    result.a_star_cache_hit_percent(),
                    result.a_star_nodes_per_tick,
                    result.spawns_per_tick,
                    result.spawn_failures_per_tick,
                    result.checksum,
                    if *expected == result.checksum {
                        ""
                    } else {
                        "  MISMATCH"
                    },
                );
                if result.movement_intents_per_tick > 0.0 {
                    println!(
                        "         movement intent/t={:.1} objective/t={:.1} hard-blocked/t={:.1} blocked={:.2}%",
                        result.movement_intents_per_tick,
                        result.objective_move_intents_per_tick,
                        result.movement_blocked_per_tick,
                        result.movement_blocked_percent,
                    );
                }
                if matches!(
                    scenario,
                    Scenario::Traffic | Scenario::TrafficFlow | Scenario::TrafficProduction
                ) {
                    println!(
                        "         traffic lower-half={} upper-half={} center-line={}",
                        result.traffic_lower_half_units,
                        result.traffic_upper_half_units,
                        result.final_units_alive
                            - result.traffic_lower_half_units
                            - result.traffic_upper_half_units,
                    );
                }
                if result.peak_projectiles_alive > 0
                    || result.projectile_launches_per_tick > 0.0
                    || result.projectile_impacts_per_tick > 0.0
                {
                    println!(
                        "         projectiles avg-live={:.1} peak-live={} launch/t={:.1} impact/t={:.1} effect/t={:.1} invalid/t={:.1} ballistic-candidates/impact={:.2} bounce-jump/t={:.1} bounce-candidates/jump={:.2}",
                        result.average_projectiles_alive,
                        result.peak_projectiles_alive,
                        result.projectile_launches_per_tick,
                        result.projectile_impacts_per_tick,
                        result.projectile_effects_per_tick,
                        result.projectile_invalidations_per_tick,
                        result.ballistic_candidates_per_impact,
                        result.bounce_jumps_per_tick,
                        result.bounce_candidates_per_jump,
                    );
                }
                if result.ability_evaluations_per_tick > 0.0 {
                    println!(
                        "         abilities eval/t={:.1} cast/t={:.1} effect/t={:.1} candidates/eval={:.2} stunned avg={:.1} peak={} move-mod avg={:.1} peak={}",
                        result.ability_evaluations_per_tick,
                        result.ability_casts_per_tick,
                        result.ability_effects_per_tick,
                        result.ability_candidates_per_evaluation,
                        result.average_stunned_units,
                        result.peak_stunned_units,
                        result.average_movement_modifiers,
                        result.peak_movement_modifiers,
                    );
                }
                if scenario == Scenario::Mixed {
                    println!(
                        "         mixed final-units={} final-buildings={}",
                        result.final_units_alive, result.final_buildings_alive,
                    );
                }
                if result.ally_defense_queries_per_tick > 0.0
                    || result.target_changes_per_tick > 0.0
                {
                    println!(
                        "         targeting retained/t={:.1} changes/t={:.1} defense-q/t={:.1} victims/q={:.2} attackers/q={:.2}",
                        result.retained_targets_per_tick,
                        result.target_changes_per_tick,
                        result.ally_defense_queries_per_tick,
                        result.ally_defense_victims_per_query,
                        result.ally_defense_attackers_per_query,
                    );
                }
            }
        }
    }

    if mismatch {
        eprintln!("determinism failure: worker counts produced different final checksums");
        std::process::exit(2);
    }
}

#[derive(Debug, Default)]
struct PhaseMs {
    topology: f64,
    timers: f64,
    production: f64,
    snapshot_and_spatial: f64,
    abilities: f64,
    targeting: f64,
    combat: f64,
    movement_intent: f64,
    crowd_and_collision: f64,
    ballistic_impact: f64,
    structural_commit: f64,
    checksum: f64,
}

#[derive(Debug, Default)]
struct BenchCounters {
    navigation_route_steps: usize,
    movement_intents: usize,
    movement_blocked: usize,
    objective_move_intents: usize,
    a_star_fallbacks: usize,
    a_star_cache_hits: usize,
    a_star_expanded_nodes: usize,
    units_spawned: usize,
    spawn_failures: usize,
    projectile_live_sum: usize,
    peak_projectiles_alive: usize,
    projectiles_launched: usize,
    projectile_impacts: usize,
    projectile_effects: usize,
    projectile_invalidations: usize,
    ballistic_candidate_checks: usize,
    bounce_jumps: usize,
    bounce_candidate_checks: usize,
    ability_evaluations: usize,
    ability_casts: usize,
    ability_candidate_checks: usize,
    ability_effects: usize,
    stunned_unit_sum: usize,
    peak_stunned_units: usize,
    movement_modifier_sum: usize,
    peak_movement_modifiers: usize,
    retained_targets: usize,
    target_changes: usize,
    ally_defense_queries: usize,
    ally_defense_victim_candidates: usize,
    ally_defense_attacker_candidates: usize,
}

#[derive(Debug)]
struct BenchResult {
    ms_per_tick: f64,
    ticks_per_second: f64,
    phase_ms: PhaseMs,
    navigation_route_steps: usize,
    movement_intents_per_tick: f64,
    movement_blocked_per_tick: f64,
    objective_move_intents_per_tick: f64,
    movement_blocked_percent: f64,
    a_star_fallbacks: usize,
    a_star_cache_hits: usize,
    a_star_nodes_per_tick: f64,
    spawns_per_tick: f64,
    spawn_failures_per_tick: f64,
    average_projectiles_alive: f64,
    peak_projectiles_alive: usize,
    projectile_launches_per_tick: f64,
    projectile_impacts_per_tick: f64,
    projectile_effects_per_tick: f64,
    projectile_invalidations_per_tick: f64,
    ballistic_candidates_per_impact: f64,
    bounce_jumps_per_tick: f64,
    bounce_candidates_per_jump: f64,
    ability_evaluations_per_tick: f64,
    ability_casts_per_tick: f64,
    ability_candidates_per_evaluation: f64,
    ability_effects_per_tick: f64,
    average_stunned_units: f64,
    peak_stunned_units: usize,
    average_movement_modifiers: f64,
    peak_movement_modifiers: usize,
    retained_targets_per_tick: f64,
    target_changes_per_tick: f64,
    ally_defense_queries_per_tick: f64,
    ally_defense_victims_per_query: f64,
    ally_defense_attackers_per_query: f64,
    final_units_alive: usize,
    final_buildings_alive: usize,
    traffic_lower_half_units: usize,
    traffic_upper_half_units: usize,
    checksum: u64,
}

impl BenchResult {
    fn a_star_fallback_percent(&self) -> f64 {
        if self.navigation_route_steps == 0 {
            0.0
        } else {
            self.a_star_fallbacks as f64 * 100.0 / self.navigation_route_steps as f64
        }
    }

    fn a_star_cache_hit_percent(&self) -> f64 {
        if self.a_star_fallbacks == 0 {
            0.0
        } else {
            self.a_star_cache_hits as f64 * 100.0 / self.a_star_fallbacks as f64
        }
    }
}

#[derive(Debug)]
enum ScenarioState {
    Static,
    TopologyToggle {
        building: Option<SimId>,
        footprint: BuildingFootprint,
    },
}

impl ScenarioState {
    fn before_tick(&mut self, simulation: &mut Simulation) {
        match self {
            Self::Static => {}
            Self::TopologyToggle {
                building,
                footprint,
            } => {
                if let Some(id) = building.take() {
                    assert!(simulation.remove_building(id));
                } else {
                    *building = Some(simulation.spawn_building(BuildingSpawn {
                        team: Team(0),
                        footprint: *footprint,
                        health: 1_000_000,
                        production: None,
                        attack: None,
                        spellcasting: None,
                    }));
                }
            }
        }
    }
}

fn run_case(
    scenario: Scenario,
    units: usize,
    workers: usize,
    warmup: u64,
    ticks: u64,
) -> BenchResult {
    let mut simulation = Simulation::new(scenario_config(scenario), workers);
    let mut state = populate_scenario(scenario, &mut simulation, units);

    for _ in 0..warmup {
        state.before_tick(&mut simulation);
        simulation.step();
    }

    let start = Instant::now();
    let mut timings = TickTimings::default();
    let mut counters = BenchCounters::default();
    for _ in 0..ticks {
        state.before_tick(&mut simulation);
        let result = simulation.step();
        accumulate_timings(&mut timings, result.timings);
        accumulate_counters(&result, &mut counters);
    }
    let elapsed = start.elapsed();

    let (traffic_lower_half_units, traffic_upper_half_units) = if matches!(
        scenario,
        Scenario::Traffic | Scenario::TrafficFlow | Scenario::TrafficProduction
    ) {
        let center_y = 375 * SUBUNITS_PER_WORLD_UNIT;
        simulation
            .units()
            .into_iter()
            .fold((0, 0), |(lower, upper), unit| {
                if unit.position.y < center_y {
                    (lower + 1, upper)
                } else if unit.position.y > center_y {
                    (lower, upper + 1)
                } else {
                    (lower, upper)
                }
            })
    } else {
        (0, 0)
    };
    let seconds = elapsed.as_secs_f64();
    BenchResult {
        ms_per_tick: duration_per_tick(elapsed, ticks).as_secs_f64() * 1_000.0,
        ticks_per_second: if seconds == 0.0 {
            f64::INFINITY
        } else {
            ticks as f64 / seconds
        },
        phase_ms: average_phase_ms(timings, ticks),
        navigation_route_steps: counters.navigation_route_steps,
        movement_intents_per_tick: counters.movement_intents as f64 / ticks as f64,
        movement_blocked_per_tick: counters.movement_blocked as f64 / ticks as f64,
        objective_move_intents_per_tick: counters.objective_move_intents as f64 / ticks as f64,
        movement_blocked_percent: if counters.movement_intents == 0 {
            0.0
        } else {
            counters.movement_blocked as f64 * 100.0 / counters.movement_intents as f64
        },
        a_star_fallbacks: counters.a_star_fallbacks,
        a_star_cache_hits: counters.a_star_cache_hits,
        a_star_nodes_per_tick: counters.a_star_expanded_nodes as f64 / ticks as f64,
        spawns_per_tick: counters.units_spawned as f64 / ticks as f64,
        spawn_failures_per_tick: counters.spawn_failures as f64 / ticks as f64,
        average_projectiles_alive: counters.projectile_live_sum as f64 / ticks as f64,
        peak_projectiles_alive: counters.peak_projectiles_alive,
        projectile_launches_per_tick: counters.projectiles_launched as f64 / ticks as f64,
        projectile_impacts_per_tick: counters.projectile_impacts as f64 / ticks as f64,
        projectile_effects_per_tick: counters.projectile_effects as f64 / ticks as f64,
        projectile_invalidations_per_tick: counters.projectile_invalidations as f64 / ticks as f64,
        ballistic_candidates_per_impact: if counters.projectile_impacts == 0 {
            0.0
        } else {
            counters.ballistic_candidate_checks as f64 / counters.projectile_impacts as f64
        },
        bounce_jumps_per_tick: counters.bounce_jumps as f64 / ticks as f64,
        bounce_candidates_per_jump: if counters.bounce_jumps == 0 {
            0.0
        } else {
            counters.bounce_candidate_checks as f64 / counters.bounce_jumps as f64
        },
        ability_evaluations_per_tick: counters.ability_evaluations as f64 / ticks as f64,
        ability_casts_per_tick: counters.ability_casts as f64 / ticks as f64,
        ability_candidates_per_evaluation: if counters.ability_evaluations == 0 {
            0.0
        } else {
            counters.ability_candidate_checks as f64 / counters.ability_evaluations as f64
        },
        ability_effects_per_tick: counters.ability_effects as f64 / ticks as f64,
        average_stunned_units: counters.stunned_unit_sum as f64 / ticks as f64,
        peak_stunned_units: counters.peak_stunned_units,
        average_movement_modifiers: counters.movement_modifier_sum as f64 / ticks as f64,
        peak_movement_modifiers: counters.peak_movement_modifiers,
        retained_targets_per_tick: counters.retained_targets as f64 / ticks as f64,
        target_changes_per_tick: counters.target_changes as f64 / ticks as f64,
        ally_defense_queries_per_tick: counters.ally_defense_queries as f64 / ticks as f64,
        ally_defense_victims_per_query: if counters.ally_defense_queries == 0 {
            0.0
        } else {
            counters.ally_defense_victim_candidates as f64 / counters.ally_defense_queries as f64
        },
        ally_defense_attackers_per_query: if counters.ally_defense_queries == 0 {
            0.0
        } else {
            counters.ally_defense_attacker_candidates as f64 / counters.ally_defense_queries as f64
        },
        final_units_alive: simulation.unit_count(),
        final_buildings_alive: simulation.building_count(),
        traffic_lower_half_units,
        traffic_upper_half_units,
        checksum: simulation.checksum(),
    }
}

fn scenario_config(scenario: Scenario) -> SimulationConfig {
    let mut config = SimulationConfig::default();
    if scenario == Scenario::Bounce {
        config.match_seed = 0x5eed_b0ce_2026_0912;
    }
    if scenario == Scenario::Ability {
        config.match_seed = 0x5eed_ab11_17e5_2026;
    }
    if scenario == Scenario::Mixed {
        config.match_seed = 0x5eed_7000_cafe_2026;
    }
    if scenario == Scenario::Pathing {
        config
            .static_blockers
            .push(BuildingFootprint::new(60, -48, 1, 97));
        config.target_pursuit_extra_range = 64 * SUBUNITS_PER_WORLD_UNIT;
    }
    if scenario == Scenario::Radius {
        let world = SUBUNITS_PER_WORLD_UNIT;
        config.spatial_cell_size = 40 * world;
        config.navigation_cell_size = 10 * world;
        config.navigation_min = castle_fight_sim::NavCell::new(0, -100);
        config.navigation_max = castle_fight_sim::NavCell::new(240, 100);
        config.target_pursuit_extra_range = 300 * world;
        config.max_separation_per_tick = 4 * world;
        config.team_objective = [
            SimPoint::new(2_300 * world, 0),
            SimPoint::new(100 * world, 0),
        ];
    }
    if matches!(
        scenario,
        Scenario::Traffic | Scenario::TrafficFlow | Scenario::TrafficProduction
    ) {
        let world = SUBUNITS_PER_WORLD_UNIT;
        config.spatial_cell_size = 40 * world;
        config.navigation_cell_size = 10 * world;
        config.navigation_min = castle_fight_sim::NavCell::new(0, 0);
        config.navigation_max = castle_fight_sim::NavCell::new(199, 74);
        config.target_pursuit_extra_range = 30 * world;
        config.unit_separation_distance = 8 * world;
        config.max_separation_per_tick = world;
        config.static_blockers = vec![
            BuildingFootprint::new(67, 0, 66, 20),
            BuildingFootprint::new(67, 55, 66, 20),
        ];
        config.team_objective = [
            SimPoint::new(1_625 * world, 375 * world),
            SimPoint::new(375 * world, 375 * world),
        ];
    }
    config
}

fn populate_scenario(
    scenario: Scenario,
    simulation: &mut Simulation,
    units: usize,
) -> ScenarioState {
    match scenario {
        Scenario::Lane => populate_lane_battle(simulation, units),
        Scenario::Cage => populate_dense_cage_battle(simulation, units),
        Scenario::Crowd => populate_crossing_crowd(simulation, units),
        Scenario::Pathing => populate_pathing_wall_battle(simulation, units),
        Scenario::Topology => populate_lane_battle(simulation, units),
        Scenario::Production => populate_production_churn(simulation, units),
        Scenario::Projectile => populate_projectile_density_battle(simulation, units),
        Scenario::Ballistic => populate_ballistic_density_battle(simulation, units),
        Scenario::Bounce => populate_bounce_density_battle(simulation, units),
        Scenario::Tower => populate_attack_building_density(simulation, units),
        Scenario::Ability => populate_automatic_ability_density(simulation, units),
        Scenario::Stun => populate_global_stun_density(simulation, units),
        Scenario::Slow => populate_timed_movement_modifier_density(simulation, units),
        Scenario::Radius => populate_mixed_radius_battle(simulation, units),
        Scenario::Traffic => populate_traffic_jam(simulation, units),
        Scenario::TrafficFlow => populate_traffic_flow(simulation, units, false),
        Scenario::TrafficProduction => populate_production_fed_traffic(simulation, units),
        Scenario::Mixed => populate_mixed_battle(simulation, units),
    }

    match scenario {
        Scenario::Topology => ScenarioState::TopologyToggle {
            building: None,
            footprint: BuildingFootprint::new(118, 63, 1, 1),
        },
        _ => ScenarioState::Static,
    }
}

fn populate_traffic_jam(simulation: &mut Simulation, total_units: usize) {
    populate_traffic_flow(simulation, total_units, true);
}

fn populate_traffic_flow(simulation: &mut Simulation, total_units: usize, combat: bool) {
    let world = SUBUNITS_PER_WORLD_UNIT;
    let per_team = total_units / 2;
    const ROWS: usize = 40;
    assert!(
        per_team <= ROWS * 40,
        "traffic fixture supports at most 3,200 units on the client-scale lane"
    );
    let spacing = 8 * world;
    let attack = AttackProfile {
        delivery: AttackDelivery::Melee,
        damage: 0,
        range: if combat { 14 * world } else { 0 },
        acquisition_range: if combat { 80 * world } else { 0 },
        cooldown_ticks: 30,
    };
    let movement = MovementProfile {
        speed_per_tick: 40 * world / 30,
    };

    for team in 0..2u8 {
        for index in 0..per_team {
            let row = index % ROWS;
            let column = index / ROWS;
            let x = if team == 0 {
                960 * world - i32::try_from(column).expect("traffic column fits i32") * spacing
            } else {
                1_040 * world + i32::try_from(column).expect("traffic column fits i32") * spacing
            };
            let y = (219 + i32::try_from(row).expect("traffic row fits i32") * 8) * world;
            simulation.spawn_unit(UnitSpawn {
                team: Team(team),
                position: SimPoint::new(x, y),
                health: 1_000_000,
                attack,
                movement,
            });
        }
    }
}

fn populate_production_fed_traffic(simulation: &mut Simulation, requested_units: usize) {
    let world = SUBUNITS_PER_WORLD_UNIT;
    let mut initial_units = requested_units.saturating_mul(4) / 5;
    initial_units -= initial_units % 2;
    populate_traffic_jam(simulation, initial_units.max(2));

    let production = ProductionProfile {
        initial_delay_ticks: 0,
        interval_ticks: 30,
        search_radius_cells: 12,
        unit: UnitTemplate {
            health: 1_000_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 14 * world,
                acquisition_range: 80 * world,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: 40 * world / 30,
            },
        },
    };

    for team in 0..2u8 {
        for index in 0..20 {
            let column = index % 5;
            let row = index / 5;
            let x = if team == 0 {
                42 + column * 5
            } else {
                154 - column * 5
            };
            let y = 22 + row * 9;
            simulation.spawn_building(BuildingSpawn {
                team: Team(team),
                footprint: BuildingFootprint::new(x, y, 4, 4),
                health: 1_000_000,
                production: Some(production),
                attack: None,
                spellcasting: None,
            });
        }
    }
}

fn populate_mixed_radius_battle(simulation: &mut Simulation, total_units: usize) {
    let world = SUBUNITS_PER_WORLD_UNIT;
    let per_team = total_units / 2;
    let rows = 30usize;
    let spacing = 64 * world;
    let radii = [8 * world, 16 * world, 24 * world, 31 * world];
    let attack = AttackProfile {
        delivery: AttackDelivery::Melee,
        damage: 0,
        range: 0,
        acquisition_range: 0,
        cooldown_ticks: 30,
    };
    let movement = MovementProfile {
        speed_per_tick: 4 * world,
    };

    for team in 0..2u8 {
        for index in 0..per_team {
            let row = index % rows;
            let column = index / rows;
            let x = if team == 0 {
                900 * world - i32::try_from(column).expect("radius column fits i32") * spacing
            } else {
                1_500 * world + i32::try_from(column).expect("radius column fits i32") * spacing
            };
            let y = (i32::try_from(row).expect("radius row fits i32") - 14) * spacing;
            simulation.spawn_unit_with_collision_radius(
                UnitSpawn {
                    team: Team(team),
                    position: SimPoint::new(x, y),
                    health: 100_000,
                    attack,
                    movement,
                },
                CollisionRadius(radii[index % radii.len()]),
            );
        }
    }
}

fn populate_pathing_wall_battle(simulation: &mut Simulation, total_units: usize) {
    let per_team = total_units / 2;
    let rows = 100usize.min(per_team.max(1));
    let spacing = 3 * SUBUNITS_PER_WORLD_UNIT / 4;
    let attack = AttackProfile {
        delivery: AttackDelivery::Melee,
        damage: 0,
        range: SUBUNITS_PER_WORLD_UNIT / 2,
        acquisition_range: 64 * SUBUNITS_PER_WORLD_UNIT,
        cooldown_ticks: 30,
    };
    let movement = MovementProfile {
        speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 8,
    };

    for team in 0..2u8 {
        for index in 0..per_team {
            let row = (index % rows) as i32;
            let column = (index / rows) as i32;
            let y = (row - rows as i32 / 2) * spacing;
            let x = if team == 0 {
                59 * SUBUNITS_PER_WORLD_UNIT + SUBUNITS_PER_WORLD_UNIT / 2 - column * spacing
            } else {
                61 * SUBUNITS_PER_WORLD_UNIT + SUBUNITS_PER_WORLD_UNIT / 2 + column * spacing
            };
            simulation.spawn_unit(UnitSpawn {
                team: Team(team),
                position: SimPoint::new(x, y),
                health: 100_000,
                attack,
                movement,
            });
        }
    }
}

fn populate_projectile_density_battle(simulation: &mut Simulation, total_units: usize) {
    let per_team = total_units / 2;
    let rows = 100usize.min(per_team.max(1));
    let spacing = 3 * SUBUNITS_PER_WORLD_UNIT / 4;
    let attack = AttackProfile {
        delivery: AttackDelivery::RangedGuaranteedHit {
            speed_per_tick: 4 * SUBUNITS_PER_WORLD_UNIT,
        },
        damage: 1,
        range: 100 * SUBUNITS_PER_WORLD_UNIT,
        acquisition_range: 100 * SUBUNITS_PER_WORLD_UNIT,
        cooldown_ticks: 1,
    };
    let movement = MovementProfile { speed_per_tick: 0 };

    for team in 0..2u8 {
        for index in 0..per_team {
            let row = (index % rows) as i32;
            let column = (index / rows) as i32;
            let y = (row - rows as i32 / 2) * spacing;
            let x = if team == 0 {
                50 * SUBUNITS_PER_WORLD_UNIT - column * spacing
            } else {
                70 * SUBUNITS_PER_WORLD_UNIT + column * spacing
            };
            simulation.spawn_unit(UnitSpawn {
                team: Team(team),
                position: SimPoint::new(x, y),
                health: 1_000_000,
                attack,
                movement,
            });
        }
    }
}

fn populate_ballistic_density_battle(simulation: &mut Simulation, total_units: usize) {
    let per_team = total_units / 2;
    let rows = 100usize.min(per_team.max(1));
    let spacing = 3 * SUBUNITS_PER_WORLD_UNIT / 4;
    let attack = AttackProfile {
        delivery: AttackDelivery::RangedBallistic {
            speed_per_tick: 4 * SUBUNITS_PER_WORLD_UNIT,
            impact_radius: 2 * SUBUNITS_PER_WORLD_UNIT,
        },
        damage: 1,
        range: 100 * SUBUNITS_PER_WORLD_UNIT,
        acquisition_range: 100 * SUBUNITS_PER_WORLD_UNIT,
        cooldown_ticks: 1,
    };
    let movement = MovementProfile { speed_per_tick: 0 };

    for team in 0..2u8 {
        for index in 0..per_team {
            let row = (index % rows) as i32;
            let column = (index / rows) as i32;
            let y = (row - rows as i32 / 2) * spacing;
            let x = if team == 0 {
                50 * SUBUNITS_PER_WORLD_UNIT - column * spacing
            } else {
                70 * SUBUNITS_PER_WORLD_UNIT + column * spacing
            };
            simulation.spawn_unit(UnitSpawn {
                team: Team(team),
                position: SimPoint::new(x, y),
                health: 1_000_000_000,
                attack,
                movement,
            });
        }
    }
}

fn populate_bounce_density_battle(simulation: &mut Simulation, total_units: usize) {
    let per_team = total_units / 2;
    let rows = 100usize.min(per_team.max(1));
    let spacing = 3 * SUBUNITS_PER_WORLD_UNIT / 4;
    let attack = AttackProfile {
        delivery: AttackDelivery::Bounce {
            speed_per_tick: 4 * SUBUNITS_PER_WORLD_UNIT,
            bounce_range: 8 * SUBUNITS_PER_WORLD_UNIT,
            max_bounces: 3,
            damage_percent_per_bounce: 75,
            allow_repeat_targets: false,
        },
        damage: 8,
        range: 100 * SUBUNITS_PER_WORLD_UNIT,
        acquisition_range: 100 * SUBUNITS_PER_WORLD_UNIT,
        cooldown_ticks: 1,
    };
    let movement = MovementProfile { speed_per_tick: 0 };

    for team in 0..2u8 {
        for index in 0..per_team {
            let row = (index % rows) as i32;
            let column = (index / rows) as i32;
            let y = (row - rows as i32 / 2) * spacing;
            let x = if team == 0 {
                50 * SUBUNITS_PER_WORLD_UNIT - column * spacing
            } else {
                70 * SUBUNITS_PER_WORLD_UNIT + column * spacing
            };
            simulation.spawn_unit(UnitSpawn {
                team: Team(team),
                position: SimPoint::new(x, y),
                health: 1_000_000_000,
                attack,
                movement,
            });
        }
    }
}

fn populate_attack_building_density(simulation: &mut Simulation, total_units: usize) {
    let tower_count = (total_units / 4).clamp(2, 500);
    let per_team_towers = tower_count.div_ceil(2);
    let tower_attack = AttackProfile {
        delivery: AttackDelivery::RangedGuaranteedHit {
            speed_per_tick: 10 * SUBUNITS_PER_WORLD_UNIT,
        },
        damage: 1,
        range: 100 * SUBUNITS_PER_WORLD_UNIT,
        acquisition_range: 100 * SUBUNITS_PER_WORLD_UNIT,
        cooldown_ticks: 1,
    };
    for team in 0..2u8 {
        for index in 0..per_team_towers {
            if team == 1 && per_team_towers + index >= tower_count {
                break;
            }
            let column = (index % 50) as i32;
            let row = (index / 50) as i32;
            let x = if team == 0 { 5 + column } else { 115 - column };
            let y = -60 + row * 2;
            simulation.spawn_building(BuildingSpawn {
                team: Team(team),
                footprint: BuildingFootprint::new(x, y, 1, 1),
                health: 1_000_000_000,
                production: None,
                attack: Some(tower_attack),
                spellcasting: None,
            });
        }
    }

    let per_team_units = total_units / 2;
    let rows = 100usize.min(per_team_units.max(1));
    let spacing = 3 * SUBUNITS_PER_WORLD_UNIT / 4;
    let passive_attack = AttackProfile {
        delivery: AttackDelivery::Melee,
        damage: 0,
        range: 0,
        acquisition_range: 0,
        cooldown_ticks: 1,
    };
    for team in 0..2u8 {
        for index in 0..per_team_units {
            let row = (index % rows) as i32;
            let column = (index / rows) as i32;
            let y = (row - rows as i32 / 2) * spacing;
            let x = if team == 0 {
                45 * SUBUNITS_PER_WORLD_UNIT - column * spacing
            } else {
                75 * SUBUNITS_PER_WORLD_UNIT + column * spacing
            };
            simulation.spawn_unit(UnitSpawn {
                team: Team(team),
                position: SimPoint::new(x, y),
                health: 1_000_000_000,
                attack: passive_attack,
                movement: MovementProfile { speed_per_tick: 0 },
            });
        }
    }
}

fn populate_automatic_ability_density(simulation: &mut Simulation, total_units: usize) {
    let caster_count = (total_units / 4).clamp(2, 500);
    let per_team_casters = caster_count.div_ceil(2);
    let spellcasting = SpellcastingProfile {
        mana: ManaProfile {
            maximum: 1_000_000_000,
            starting: 1_000_000_000,
            regen_per_tick: 1,
        },
        ability: AutomaticAbilityProfile {
            id: AbilityId(1),
            mana_cost: 1,
            cooldown_ticks: 1,
            range: 100 * SUBUNITS_PER_WORLD_UNIT,
            target_policy: AbilityTargetPolicy::RandomEnemyUnit,
            effect: AbilityEffect::Damage { amount: 1 },
        },
    };
    for team in 0..2u8 {
        for index in 0..per_team_casters {
            if team == 1 && per_team_casters + index >= caster_count {
                break;
            }
            let column = (index % 10) as i32;
            let row = (index / 10) as i32;
            let x = if team == 0 {
                5 + column * 2
            } else {
                115 - column * 2
            };
            let y = -50 + row * 2;
            simulation.spawn_building(BuildingSpawn {
                team: Team(team),
                footprint: BuildingFootprint::new(x, y, 1, 1),
                health: 1_000_000_000,
                production: None,
                attack: None,
                spellcasting: Some(spellcasting),
            });
        }
    }

    let per_team_units = total_units / 2;
    let rows = 100usize.min(per_team_units.max(1));
    let spacing = 3 * SUBUNITS_PER_WORLD_UNIT / 4;
    let passive_attack = AttackProfile {
        delivery: AttackDelivery::Melee,
        damage: 0,
        range: 0,
        acquisition_range: 0,
        cooldown_ticks: 1,
    };
    for team in 0..2u8 {
        for index in 0..per_team_units {
            let row = (index % rows) as i32;
            let column = (index / rows) as i32;
            let y = (row - rows as i32 / 2) * spacing;
            let x = if team == 0 {
                45 * SUBUNITS_PER_WORLD_UNIT - column * spacing
            } else {
                75 * SUBUNITS_PER_WORLD_UNIT + column * spacing
            };
            simulation.spawn_unit(UnitSpawn {
                team: Team(team),
                position: SimPoint::new(x, y),
                health: 1_000_000_000,
                attack: passive_attack,
                movement: MovementProfile { speed_per_tick: 0 },
            });
        }
    }
}

fn populate_global_stun_density(simulation: &mut Simulation, total_units: usize) {
    let caster_count = (total_units / 40).clamp(2, 64);
    let per_team_casters = caster_count.div_ceil(2);
    let spellcasting = SpellcastingProfile {
        mana: ManaProfile {
            maximum: 1_000_000_000,
            starting: 1_000_000_000,
            regen_per_tick: 0,
        },
        ability: AutomaticAbilityProfile {
            id: AbilityId(2),
            mana_cost: 1,
            cooldown_ticks: 3,
            range: 0,
            target_policy: AbilityTargetPolicy::AllEnemyUnits,
            effect: AbilityEffect::Stun { duration_ticks: 2 },
        },
    };
    for team in 0..2u8 {
        for index in 0..per_team_casters {
            if team == 1 && per_team_casters + index >= caster_count {
                break;
            }
            let column = (index % 10) as i32;
            let row = (index / 10) as i32;
            let x = if team == 0 {
                5 + column * 2
            } else {
                115 - column * 2
            };
            simulation.spawn_building(BuildingSpawn {
                team: Team(team),
                footprint: BuildingFootprint::new(x, -55 + row * 2, 1, 1),
                health: 1_000_000_000,
                production: None,
                attack: None,
                spellcasting: Some(spellcasting),
            });
        }
    }

    let per_team_units = total_units / 2;
    let rows = 100usize.min(per_team_units.max(1));
    let spacing = 3 * SUBUNITS_PER_WORLD_UNIT / 4;
    let attack = AttackProfile {
        delivery: AttackDelivery::Melee,
        damage: 0,
        range: 2 * SUBUNITS_PER_WORLD_UNIT,
        acquisition_range: 100 * SUBUNITS_PER_WORLD_UNIT,
        cooldown_ticks: 3,
    };
    for team in 0..2u8 {
        for index in 0..per_team_units {
            let row = (index % rows) as i32;
            let column = (index / rows) as i32;
            let y = (row - rows as i32 / 2) * spacing;
            let x = if team == 0 {
                40 * SUBUNITS_PER_WORLD_UNIT - column * spacing
            } else {
                80 * SUBUNITS_PER_WORLD_UNIT + column * spacing
            };
            simulation.spawn_unit(UnitSpawn {
                team: Team(team),
                position: SimPoint::new(x, y),
                health: 1_000_000,
                attack,
                movement: MovementProfile {
                    speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 2,
                },
            });
        }
    }
}

fn populate_timed_movement_modifier_density(simulation: &mut Simulation, total_units: usize) {
    let caster_count = (total_units / 40).clamp(2, 64);
    let per_team_casters = caster_count.div_ceil(2);
    let make_spell = |ability_id: u32, modifier_id: u32, percent_delta: i16| SpellcastingProfile {
        mana: ManaProfile {
            maximum: 1_000_000_000,
            starting: 1_000_000_000,
            regen_per_tick: 0,
        },
        ability: AutomaticAbilityProfile {
            id: AbilityId(ability_id),
            mana_cost: 1,
            cooldown_ticks: 3,
            range: 0,
            target_policy: AbilityTargetPolicy::AllEnemyUnits,
            effect: AbilityEffect::ModifyMovementSpeedPercent {
                modifier: ModifierId(modifier_id),
                percent_delta,
                duration_ticks: 5,
            },
        },
    };
    for team in 0..2u8 {
        for index in 0..per_team_casters {
            if team == 1 && per_team_casters + index >= caster_count {
                break;
            }
            let column = (index % 10) as i32;
            let row = (index / 10) as i32;
            let x = if team == 0 {
                5 + column * 2
            } else {
                115 - column * 2
            };
            let spellcasting = if index % 2 == 0 {
                make_spell(3, 100, -20)
            } else {
                make_spell(4, 101, -15)
            };
            simulation.spawn_building(BuildingSpawn {
                team: Team(team),
                footprint: BuildingFootprint::new(x, -55 + row * 2, 1, 1),
                health: 1_000_000_000,
                production: None,
                attack: None,
                spellcasting: Some(spellcasting),
            });
        }
    }

    let per_team_units = total_units / 2;
    let rows = 100usize.min(per_team_units.max(1));
    let spacing = 3 * SUBUNITS_PER_WORLD_UNIT / 4;
    let attack = AttackProfile {
        delivery: AttackDelivery::Melee,
        damage: 0,
        range: 2 * SUBUNITS_PER_WORLD_UNIT,
        acquisition_range: 100 * SUBUNITS_PER_WORLD_UNIT,
        cooldown_ticks: 3,
    };
    for team in 0..2u8 {
        for index in 0..per_team_units {
            let row = (index % rows) as i32;
            let column = (index / rows) as i32;
            let y = (row - rows as i32 / 2) * spacing;
            let x = if team == 0 {
                40 * SUBUNITS_PER_WORLD_UNIT - column * spacing
            } else {
                80 * SUBUNITS_PER_WORLD_UNIT + column * spacing
            };
            simulation.spawn_unit(UnitSpawn {
                team: Team(team),
                position: SimPoint::new(x, y),
                health: 1_000_000,
                attack,
                movement: MovementProfile {
                    speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 2,
                },
            });
        }
    }
}

fn populate_mixed_battle(simulation: &mut Simulation, total_units: usize) {
    const RESERVED_PRODUCTION_SPAWNS: usize = 4;
    assert!(
        total_units >= RESERVED_PRODUCTION_SPAWNS + 2,
        "mixed fixture needs room for production reserve"
    );
    let initial_units = total_units - RESERVED_PRODUCTION_SPAWNS;
    let per_team = initial_units / 2;
    let rows = 100usize.min(per_team.max(1));
    let spacing = 3 * SUBUNITS_PER_WORLD_UNIT / 4;

    for team in 0..2u8 {
        for index in 0..per_team {
            let row = (index % rows) as i32;
            let column = (index / rows) as i32;
            let y = (row - rows as i32 / 2) * spacing;
            let x = if team == 0 {
                32 * SUBUNITS_PER_WORLD_UNIT - column * spacing
            } else {
                88 * SUBUNITS_PER_WORLD_UNIT + column * spacing
            };
            let (delivery, damage, range) = match index % 4 {
                0 => (AttackDelivery::Melee, 4, 2 * SUBUNITS_PER_WORLD_UNIT),
                1 => (
                    AttackDelivery::RangedGuaranteedHit {
                        speed_per_tick: 3 * SUBUNITS_PER_WORLD_UNIT,
                    },
                    3,
                    12 * SUBUNITS_PER_WORLD_UNIT,
                ),
                2 => (
                    AttackDelivery::RangedBallistic {
                        speed_per_tick: 3 * SUBUNITS_PER_WORLD_UNIT,
                        impact_radius: 3 * SUBUNITS_PER_WORLD_UNIT / 2,
                    },
                    2,
                    12 * SUBUNITS_PER_WORLD_UNIT,
                ),
                _ => (
                    AttackDelivery::Bounce {
                        speed_per_tick: 3 * SUBUNITS_PER_WORLD_UNIT,
                        bounce_range: 4 * SUBUNITS_PER_WORLD_UNIT,
                        max_bounces: 2,
                        damage_percent_per_bounce: 75,
                        allow_repeat_targets: false,
                    },
                    4,
                    12 * SUBUNITS_PER_WORLD_UNIT,
                ),
            };
            simulation.spawn_unit(UnitSpawn {
                team: Team(team),
                position: SimPoint::new(x, y),
                health: 1_000,
                attack: AttackProfile {
                    delivery,
                    damage,
                    range,
                    acquisition_range: 18 * SUBUNITS_PER_WORLD_UNIT,
                    cooldown_ticks: 15,
                },
                movement: MovementProfile {
                    speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 4,
                },
            });
        }
    }

    let tower_attack = AttackProfile {
        delivery: AttackDelivery::RangedGuaranteedHit {
            speed_per_tick: 5 * SUBUNITS_PER_WORLD_UNIT,
        },
        damage: 3,
        range: 90 * SUBUNITS_PER_WORLD_UNIT,
        acquisition_range: 90 * SUBUNITS_PER_WORLD_UNIT,
        cooldown_ticks: 20,
    };
    let spellcasting = SpellcastingProfile {
        mana: ManaProfile {
            maximum: 100,
            starting: 100,
            regen_per_tick: 1,
        },
        ability: AutomaticAbilityProfile {
            id: AbilityId(2),
            mana_cost: 10,
            cooldown_ticks: 30,
            range: 90 * SUBUNITS_PER_WORLD_UNIT,
            target_policy: AbilityTargetPolicy::RandomEnemyUnit,
            effect: AbilityEffect::Damage { amount: 5 },
        },
    };
    let global_stun = SpellcastingProfile {
        mana: ManaProfile {
            maximum: 100,
            starting: 100,
            regen_per_tick: 1,
        },
        ability: AutomaticAbilityProfile {
            id: AbilityId(3),
            mana_cost: 20,
            cooldown_ticks: 150,
            range: 0,
            target_policy: AbilityTargetPolicy::AllEnemyUnits,
            effect: AbilityEffect::Stun { duration_ticks: 15 },
        },
    };
    let production = ProductionProfile {
        initial_delay_ticks: 300,
        interval_ticks: u16::MAX,
        search_radius_cells: 10,
        unit: UnitTemplate {
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 4,
                range: 2 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 18 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 15,
            },
            movement: MovementProfile {
                speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 4,
            },
        },
    };
    for team in 0..2u8 {
        for index in 0..6 {
            let x = if team == 0 { 8 } else { 112 };
            let y = -30 + index * 5;
            simulation.spawn_building(BuildingSpawn {
                team: Team(team),
                footprint: BuildingFootprint::new(x, y, 1, 1),
                health: 100_000,
                production: None,
                attack: Some(tower_attack),
                spellcasting: None,
            });
        }
        for index in 0..4 {
            let x = if team == 0 { 12 } else { 108 };
            let y = 10 + index * 5;
            simulation.spawn_building(BuildingSpawn {
                team: Team(team),
                footprint: BuildingFootprint::new(x, y, 1, 1),
                health: 100_000,
                production: None,
                attack: None,
                spellcasting: Some(if index == 0 {
                    global_stun
                } else {
                    spellcasting
                }),
            });
        }
        for index in 0..2 {
            let x = if team == 0 { 6 } else { 114 };
            let y = 35 + index * 5;
            simulation.spawn_building(BuildingSpawn {
                team: Team(team),
                footprint: BuildingFootprint::new(x, y, 1, 1),
                health: 100_000,
                production: Some(production),
                attack: None,
                spellcasting: None,
            });
        }
    }
}

fn populate_production_churn(simulation: &mut Simulation, scale: usize) {
    let total_buildings = (scale / 100).clamp(2, 100);
    let per_team = total_buildings.div_ceil(2);
    let template = UnitTemplate {
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
    let production = ProductionProfile {
        initial_delay_ticks: 0,
        interval_ticks: 1,
        search_radius_cells: 2,
        unit: template,
    };

    for team in 0..2u8 {
        for index in 0..per_team {
            if team == 1 && per_team + index >= total_buildings {
                break;
            }
            let column = (index % 10) as i32;
            let row = (index / 10) as i32;
            let x = if team == 0 {
                8 + column * 3
            } else {
                112 - column * 3
            };
            let y = -30 + row * 3;
            simulation.spawn_building(BuildingSpawn {
                team: Team(team),
                footprint: BuildingFootprint::new(x, y, 1, 1),
                health: 1_000_000,
                production: Some(production),
                attack: None,
                spellcasting: None,
            });
        }
    }
}

fn accumulate_counters(result: &TickResult, counters: &mut BenchCounters) {
    counters.navigation_route_steps += result.navigation_route_steps;
    counters.movement_intents += result.movement_intents;
    counters.movement_blocked += result.movement_blocked;
    counters.objective_move_intents += result.objective_move_intents;
    counters.a_star_fallbacks += result.a_star_fallbacks;
    counters.a_star_cache_hits += result.a_star_cache_hits;
    counters.a_star_expanded_nodes += result.a_star_expanded_nodes;
    counters.units_spawned += result.units_spawned;
    counters.spawn_failures += result.spawn_failures;
    counters.projectile_live_sum += result.projectiles_alive;
    counters.peak_projectiles_alive = counters
        .peak_projectiles_alive
        .max(result.projectiles_alive);
    counters.projectiles_launched += result.projectiles_launched;
    counters.projectile_impacts += result.projectile_impacts;
    counters.projectile_effects += result.projectile_effects;
    counters.projectile_invalidations += result.projectile_invalidations;
    counters.ballistic_candidate_checks += result.ballistic_candidate_checks;
    counters.bounce_jumps += result.bounce_jumps;
    counters.bounce_candidate_checks += result.bounce_candidate_checks;
    counters.ability_evaluations += result.ability_evaluations;
    counters.ability_casts += result.ability_casts;
    counters.ability_candidate_checks += result.ability_candidate_checks;
    counters.ability_effects += result.ability_effects;
    counters.stunned_unit_sum += result.stunned_units;
    counters.peak_stunned_units = counters.peak_stunned_units.max(result.stunned_units);
    counters.movement_modifier_sum += result.timed_movement_modifiers;
    counters.peak_movement_modifiers = counters
        .peak_movement_modifiers
        .max(result.timed_movement_modifiers);
    counters.retained_targets += result.retained_targets;
    counters.target_changes += result.target_changes;
    counters.ally_defense_queries += result.ally_defense_queries;
    counters.ally_defense_victim_candidates += result.ally_defense_victim_candidates;
    counters.ally_defense_attacker_candidates += result.ally_defense_attacker_candidates;
}

fn accumulate_timings(total: &mut TickTimings, tick: TickTimings) {
    total.topology += tick.topology;
    total.timers += tick.timers;
    total.production += tick.production;
    total.snapshot_and_spatial += tick.snapshot_and_spatial;
    total.abilities += tick.abilities;
    total.targeting += tick.targeting;
    total.combat += tick.combat;
    total.movement_intent += tick.movement_intent;
    total.crowd_and_collision += tick.crowd_and_collision;
    total.ballistic_impact += tick.ballistic_impact;
    total.structural_commit += tick.structural_commit;
    total.checksum += tick.checksum;
    total.total += tick.total;
}

fn average_phase_ms(total: TickTimings, ticks: u64) -> PhaseMs {
    PhaseMs {
        topology: ms_per_tick(total.topology, ticks),
        timers: ms_per_tick(total.timers, ticks),
        production: ms_per_tick(total.production, ticks),
        snapshot_and_spatial: ms_per_tick(total.snapshot_and_spatial, ticks),
        abilities: ms_per_tick(total.abilities, ticks),
        targeting: ms_per_tick(total.targeting, ticks),
        combat: ms_per_tick(total.combat, ticks),
        movement_intent: ms_per_tick(total.movement_intent, ticks),
        crowd_and_collision: ms_per_tick(total.crowd_and_collision, ticks),
        ballistic_impact: ms_per_tick(total.ballistic_impact, ticks),
        structural_commit: ms_per_tick(total.structural_commit, ticks),
        checksum: ms_per_tick(total.checksum, ticks),
    }
}

fn ms_per_tick(total: Duration, ticks: u64) -> f64 {
    duration_per_tick(total, ticks).as_secs_f64() * 1_000.0
}

fn duration_per_tick(elapsed: Duration, ticks: u64) -> Duration {
    assert!(ticks > 0, "benchmark ticks must be positive");
    elapsed.div_f64(ticks as f64)
}

fn parse_args() -> Args {
    let mut args = Args::default();
    let mut iter = env::args().skip(1);

    while let Some(flag) = iter.next() {
        match flag.as_str() {
            "--scenario" | "--scenarios" => {
                args.scenarios = parse_scenarios(&next_value(&mut iter, &flag));
            }
            "--units" => args.units = parse_list(&next_value(&mut iter, "--units")),
            "--workers" => args.workers = parse_list(&next_value(&mut iter, "--workers")),
            "--ticks" => args.ticks = parse_number(&next_value(&mut iter, "--ticks"), "--ticks"),
            "--warmup" => {
                args.warmup = parse_number(&next_value(&mut iter, "--warmup"), "--warmup");
            }
            "-h" | "--help" => {
                println!("Usage: cargo run --release -p castle-fight-sim-bench -- [options]");
                println!(
                    "  --scenario lane,cage,crowd,pathing,topology,production,projectile,ballistic,bounce,tower,ability,stun,slow,radius,traffic,traffic-flow,traffic-production,mixed"
                );
                println!("  --units 700,1000,5000,10000");
                println!("  --workers 1,2,4,8");
                println!("  --ticks 200");
                println!("  --warmup 20");
                println!("Pathing is intentionally adversarial; start with smaller unit counts.");
                std::process::exit(0);
            }
            other => panic!("unknown argument: {other}"),
        }
    }

    assert!(args.ticks > 0, "--ticks must be positive");
    assert!(
        !args.scenarios.is_empty(),
        "at least one scenario is required"
    );
    args
}

fn parse_scenarios(value: &str) -> Vec<Scenario> {
    value
        .split(',')
        .map(|part| match part {
            "lane" => Scenario::Lane,
            "cage" => Scenario::Cage,
            "crowd" => Scenario::Crowd,
            "pathing" => Scenario::Pathing,
            "topology" => Scenario::Topology,
            "production" => Scenario::Production,
            "projectile" => Scenario::Projectile,
            "ballistic" => Scenario::Ballistic,
            "bounce" => Scenario::Bounce,
            "tower" => Scenario::Tower,
            "ability" => Scenario::Ability,
            "stun" => Scenario::Stun,
            "slow" => Scenario::Slow,
            "radius" => Scenario::Radius,
            "traffic" => Scenario::Traffic,
            "traffic-flow" => Scenario::TrafficFlow,
            "traffic-production" => Scenario::TrafficProduction,
            "mixed" => Scenario::Mixed,
            other => panic!("unknown scenario: {other}"),
        })
        .collect()
}

fn next_value(iter: &mut impl Iterator<Item = String>, flag: &str) -> String {
    iter.next()
        .unwrap_or_else(|| panic!("missing value for {flag}"))
}

fn parse_list<T>(value: &str) -> Vec<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Debug,
{
    value
        .split(',')
        .map(|part| {
            part.parse()
                .unwrap_or_else(|error| panic!("invalid list value {part:?}: {error:?}"))
        })
        .collect()
}

fn parse_number<T>(value: &str, flag: &str) -> T
where
    T: std::str::FromStr,
    T::Err: std::fmt::Debug,
{
    value
        .parse()
        .unwrap_or_else(|error| panic!("invalid {flag} value {value:?}: {error:?}"))
}
