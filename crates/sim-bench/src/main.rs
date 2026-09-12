use std::{
    collections::BTreeMap,
    env,
    time::{Duration, Instant},
};

use castle_fight_sim::{
    AttackDelivery, AttackProfile, BuildingFootprint, BuildingSpawn, MovementProfile,
    ProductionProfile, SUBUNITS_PER_WORLD_UNIT, SimId, SimPoint, Simulation, SimulationConfig,
    Team, TickResult, TickTimings, UnitSpawn, UnitTemplate, populate_crossing_crowd,
    populate_dense_cage_battle, populate_lane_battle,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Scenario {
    Lane,
    Cage,
    Crowd,
    Pathing,
    Topology,
    Production,
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
            units: vec![1_000, 5_000, 10_000],
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
            "{:>8} {:>7} {:>9} {:>9} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>8} {:>8} {:>10} {:>8} {:>8} {:>18}",
            "units",
            "workers",
            "ms/tick",
            "ticks/s",
            "topo",
            "timer",
            "prod",
            "spatial",
            "target",
            "combat",
            "intent",
            "collide",
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
                    "{:>8} {:>7} {:>9.3} {:>9.1} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.3} {:>7.2} {:>7.2} {:>10.1} {:>8.2} {:>8.2} {:>18x}{}",
                    units,
                    workers,
                    result.ms_per_tick,
                    result.ticks_per_second,
                    result.phase_ms.topology,
                    result.phase_ms.timers,
                    result.phase_ms.production,
                    result.phase_ms.snapshot_and_spatial,
                    result.phase_ms.targeting,
                    result.phase_ms.combat,
                    result.phase_ms.movement_intent,
                    result.phase_ms.crowd_and_collision,
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
    targeting: f64,
    combat: f64,
    movement_intent: f64,
    crowd_and_collision: f64,
    structural_commit: f64,
    checksum: f64,
}

#[derive(Debug)]
struct BenchResult {
    ms_per_tick: f64,
    ticks_per_second: f64,
    phase_ms: PhaseMs,
    pursuit_steps: usize,
    a_star_fallbacks: usize,
    a_star_cache_hits: usize,
    a_star_nodes_per_tick: f64,
    spawns_per_tick: f64,
    spawn_failures_per_tick: f64,
    checksum: u64,
}

impl BenchResult {
    fn a_star_fallback_percent(&self) -> f64 {
        if self.pursuit_steps == 0 {
            0.0
        } else {
            self.a_star_fallbacks as f64 * 100.0 / self.pursuit_steps as f64
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
    let mut pursuit_steps = 0usize;
    let mut a_star_fallbacks = 0usize;
    let mut a_star_cache_hits = 0usize;
    let mut a_star_expanded_nodes = 0usize;
    let mut units_spawned = 0usize;
    let mut spawn_failures = 0usize;
    for _ in 0..ticks {
        state.before_tick(&mut simulation);
        let result = simulation.step();
        accumulate_timings(&mut timings, result.timings);
        accumulate_counters(
            &result,
            &mut pursuit_steps,
            &mut a_star_fallbacks,
            &mut a_star_cache_hits,
            &mut a_star_expanded_nodes,
            &mut units_spawned,
            &mut spawn_failures,
        );
    }
    let elapsed = start.elapsed();

    let seconds = elapsed.as_secs_f64();
    BenchResult {
        ms_per_tick: duration_per_tick(elapsed, ticks).as_secs_f64() * 1_000.0,
        ticks_per_second: if seconds == 0.0 {
            f64::INFINITY
        } else {
            ticks as f64 / seconds
        },
        phase_ms: average_phase_ms(timings, ticks),
        pursuit_steps,
        a_star_fallbacks,
        a_star_cache_hits,
        a_star_nodes_per_tick: a_star_expanded_nodes as f64 / ticks as f64,
        spawns_per_tick: units_spawned as f64 / ticks as f64,
        spawn_failures_per_tick: spawn_failures as f64 / ticks as f64,
        checksum: simulation.checksum(),
    }
}

fn scenario_config(scenario: Scenario) -> SimulationConfig {
    let mut config = SimulationConfig::default();
    if scenario == Scenario::Pathing {
        config
            .static_blockers
            .push(BuildingFootprint::new(60, -48, 1, 97));
        config.target_pursuit_extra_range = 64 * SUBUNITS_PER_WORLD_UNIT;
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
    }

    match scenario {
        Scenario::Topology => ScenarioState::TopologyToggle {
            building: None,
            footprint: BuildingFootprint::new(118, 63, 1, 1),
        },
        _ => ScenarioState::Static,
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
            });
        }
    }
}

fn accumulate_counters(
    result: &TickResult,
    pursuit_steps: &mut usize,
    a_star_fallbacks: &mut usize,
    a_star_cache_hits: &mut usize,
    a_star_expanded_nodes: &mut usize,
    units_spawned: &mut usize,
    spawn_failures: &mut usize,
) {
    *pursuit_steps += result.pursuit_steps;
    *a_star_fallbacks += result.a_star_fallbacks;
    *a_star_cache_hits += result.a_star_cache_hits;
    *a_star_expanded_nodes += result.a_star_expanded_nodes;
    *units_spawned += result.units_spawned;
    *spawn_failures += result.spawn_failures;
}

fn accumulate_timings(total: &mut TickTimings, tick: TickTimings) {
    total.topology += tick.topology;
    total.timers += tick.timers;
    total.production += tick.production;
    total.snapshot_and_spatial += tick.snapshot_and_spatial;
    total.targeting += tick.targeting;
    total.combat += tick.combat;
    total.movement_intent += tick.movement_intent;
    total.crowd_and_collision += tick.crowd_and_collision;
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
        targeting: ms_per_tick(total.targeting, ticks),
        combat: ms_per_tick(total.combat, ticks),
        movement_intent: ms_per_tick(total.movement_intent, ticks),
        crowd_and_collision: ms_per_tick(total.crowd_and_collision, ticks),
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
                println!("  --scenario lane,cage,crowd,pathing,topology,production");
                println!("  --units 1000,5000,10000");
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
