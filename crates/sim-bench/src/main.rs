use std::{
    collections::BTreeMap,
    env,
    time::{Duration, Instant},
};

use castle_fight_sim::{
    Simulation, SimulationConfig, TickTimings, populate_dense_cage_battle, populate_lane_battle,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Scenario {
    Lane,
    Cage,
}

impl Scenario {
    const fn name(self) -> &'static str {
        match self {
            Self::Lane => "lane",
            Self::Cage => "cage",
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
            scenarios: vec![Scenario::Lane, Scenario::Cage],
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
            "{:>8} {:>7} {:>9} {:>9} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>18}",
            "units",
            "workers",
            "ms/tick",
            "ticks/s",
            "topo",
            "spatial",
            "target",
            "combat",
            "move",
            "checksum",
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
                    "{:>8} {:>7} {:>9.3} {:>9.1} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>18x}{}",
                    units,
                    workers,
                    result.ms_per_tick,
                    result.ticks_per_second,
                    result.phase_ms.topology_and_timers,
                    result.phase_ms.snapshot_and_spatial,
                    result.phase_ms.targeting,
                    result.phase_ms.combat,
                    result.phase_ms.movement_and_commit,
                    result.phase_ms.checksum,
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
    topology_and_timers: f64,
    snapshot_and_spatial: f64,
    targeting: f64,
    combat: f64,
    movement_and_commit: f64,
    checksum: f64,
}

#[derive(Debug)]
struct BenchResult {
    ms_per_tick: f64,
    ticks_per_second: f64,
    phase_ms: PhaseMs,
    checksum: u64,
}

fn run_case(
    scenario: Scenario,
    units: usize,
    workers: usize,
    warmup: u64,
    ticks: u64,
) -> BenchResult {
    let mut simulation = Simulation::new(SimulationConfig::default(), workers);
    match scenario {
        Scenario::Lane => populate_lane_battle(&mut simulation, units),
        Scenario::Cage => populate_dense_cage_battle(&mut simulation, units),
    }

    for _ in 0..warmup {
        simulation.step();
    }

    let start = Instant::now();
    let mut timings = TickTimings::default();
    for _ in 0..ticks {
        let result = simulation.step();
        accumulate_timings(&mut timings, result.timings);
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
        checksum: simulation.checksum(),
    }
}

fn accumulate_timings(total: &mut TickTimings, tick: TickTimings) {
    total.topology_and_timers += tick.topology_and_timers;
    total.production += tick.production;
    total.snapshot_and_spatial += tick.snapshot_and_spatial;
    total.targeting += tick.targeting;
    total.combat += tick.combat;
    total.movement_and_commit += tick.movement_and_commit;
    total.checksum += tick.checksum;
    total.total += tick.total;
}

fn average_phase_ms(total: TickTimings, ticks: u64) -> PhaseMs {
    PhaseMs {
        topology_and_timers: ms_per_tick(total.topology_and_timers, ticks),
        snapshot_and_spatial: ms_per_tick(total.snapshot_and_spatial, ticks),
        targeting: ms_per_tick(total.targeting, ticks),
        combat: ms_per_tick(total.combat, ticks),
        movement_and_commit: ms_per_tick(total.movement_and_commit, ticks),
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
                println!("  --scenario lane,cage");
                println!("  --units 1000,5000,10000");
                println!("  --workers 1,2,4,8");
                println!("  --ticks 200");
                println!("  --warmup 20");
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
