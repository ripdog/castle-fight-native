use std::{
    collections::BTreeMap,
    env,
    time::{Duration, Instant},
};

use castle_fight_sim::{Simulation, SimulationConfig, populate_lane_battle};

#[derive(Debug)]
struct Args {
    units: Vec<usize>,
    workers: Vec<usize>,
    ticks: u64,
    warmup: u64,
}

impl Default for Args {
    fn default() -> Self {
        Self {
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
        "units={:?} workers={:?} ticks={} warmup={}",
        args.units, args.workers, args.ticks, args.warmup
    );
    println!();
    println!(
        "{:>8} {:>7} {:>10} {:>12} {:>12} {:>12} {:>18}",
        "units", "workers", "ms/tick", "ticks/sec", "attacks", "alive", "checksum"
    );

    let mut expected_checksums = BTreeMap::<usize, u64>::new();
    let mut mismatch = false;

    for &units in &args.units {
        assert!(
            units >= 2 && units.is_multiple_of(2),
            "unit counts must be even and >= 2"
        );

        for &workers in &args.workers {
            assert!(workers > 0, "worker counts must be positive");
            let result = run_case(units, workers, args.warmup, args.ticks);

            let expected = expected_checksums.entry(units).or_insert(result.checksum);
            if *expected != result.checksum {
                mismatch = true;
            }

            println!(
                "{:>8} {:>7} {:>10.3} {:>12.1} {:>12} {:>12} {:>18x}{}",
                units,
                workers,
                result.ms_per_tick,
                result.ticks_per_second,
                result.attacks,
                result.alive,
                result.checksum,
                if *expected == result.checksum {
                    ""
                } else {
                    "  MISMATCH"
                },
            );
        }
    }

    if mismatch {
        eprintln!("determinism failure: worker counts produced different final checksums");
        std::process::exit(2);
    }
}

#[derive(Debug)]
struct BenchResult {
    ms_per_tick: f64,
    ticks_per_second: f64,
    attacks: usize,
    alive: usize,
    checksum: u64,
}

fn run_case(units: usize, workers: usize, warmup: u64, ticks: u64) -> BenchResult {
    let mut simulation = Simulation::new(SimulationConfig::default(), workers);
    populate_lane_battle(&mut simulation, units);

    for _ in 0..warmup {
        simulation.step();
    }

    let start = Instant::now();
    let mut attacks = 0usize;
    let mut alive = simulation.unit_count();
    for _ in 0..ticks {
        let result = simulation.step();
        attacks += result.attacks_resolved;
        alive = result.units_alive;
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
        attacks,
        alive,
        checksum: simulation.checksum(),
    }
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
            "--units" => args.units = parse_list(&next_value(&mut iter, "--units")),
            "--workers" => args.workers = parse_list(&next_value(&mut iter, "--workers")),
            "--ticks" => args.ticks = parse_number(&next_value(&mut iter, "--ticks"), "--ticks"),
            "--warmup" => {
                args.warmup = parse_number(&next_value(&mut iter, "--warmup"), "--warmup")
            }
            "-h" | "--help" => {
                println!("Usage: cargo run --release -p castle-fight-sim-bench -- [options]");
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
    args
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
