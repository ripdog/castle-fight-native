use std::{
    collections::BTreeMap,
    env,
    time::{Duration, Instant},
};

use castle_fight_sim::{
    AbilityEffect, AbilityId, AbilityTargetPolicy, AttackDelivery, AttackProfile,
    AutomaticAbilityProfile, BuildingFootprint, BuildingSpawn, CASTLE_FIGHT_SIMULATION_HZ,
    CastleFightBuilderRace, CastleFightMatchConfig, CastleFightParticipantConfig,
    CastleFightProductionKind, CastleFightUnitKind, CollisionRadius, CombatRules,
    DebugStartingBuildings, ManaProfile, MapVersion, ModifierId, MovementProfile, PlayerId,
    ProductionProfile, SUBUNITS_PER_WORLD_UNIT, SimId, SimPoint, Simulation, SimulationConfig,
    Span, SpanTimings, SpellcastingProfile, Team, TickResult, TickTimings, UnitSpawn, UnitTemplate,
    castle_fight_damage_rules, create_castle_fight_match,
};

/// Fixed seed so the all-buildings match is reproducible across runs.
const ALL_BUILDINGS_MATCH_SEED: u64 = 1;
/// Fixed seed for de-synchronizing spellcaster mana in the all-buildings scenario.
const ALL_BUILDINGS_MANA_SEED: u64 = 0x5eed_0000_c0de_0001;
/// Default game length for the all-buildings scenario when `--ticks` is not given.
const ALL_BUILDINGS_GAME_SECONDS: u64 = 120;
/// Width of each row in the all-buildings window breakdown.
const TICK_WINDOW_SECONDS: u64 = 10;
/// Number of slowest individual ticks listed after an all-buildings run.
const SLOWEST_TICK_COUNT: usize = 10;

const PHASE_COUNT: usize = 12;
const PHASE_NAMES: [&str; PHASE_COUNT] = [
    "topology",
    "timers",
    "production",
    "snapshot_and_spatial",
    "abilities",
    "targeting",
    "combat",
    "movement_intent",
    "crowd_and_collision",
    "ballistic_impact",
    "structural_commit",
    "checksum",
];
const PHASE_SHORT_NAMES: [&str; PHASE_COUNT] = [
    "topo", "timer", "prod", "spatial", "ability", "target", "combat", "intent", "collide",
    "impact", "commit", "hash",
];

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
    MultiAbility,
    Stun,
    Slow,
    Radius,
    Traffic,
    TrafficFlow,
    TrafficProduction,
    Mixed,
    AllBuildings,
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
            Self::MultiAbility => "multi-ability",
            Self::Stun => "stun",
            Self::Slow => "slow",
            Self::Radius => "radius",
            Self::Traffic => "traffic",
            Self::TrafficFlow => "traffic-flow",
            Self::TrafficProduction => "traffic-production",
            Self::Mixed => "mixed",
            Self::AllBuildings => "all-buildings",
        }
    }
}

/// Developer-tool choices for the all-buildings scenario. Other scenarios ignore them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AllBuildingsOptions {
    /// Players per team, 1 through 3. Team 0 uses ids `0..N` and team 1 uses ids `6..6+N`.
    players_per_team: usize,
    /// Suppresses building health loss so castles and production survive a sustained run.
    invulnerable_buildings: bool,
    /// Randomizes spellcaster mana so spell buildings do not cast in lockstep.
    randomize_mana: bool,
}

impl Default for AllBuildingsOptions {
    fn default() -> Self {
        Self {
            players_per_team: 3,
            invulnerable_buildings: true,
            randomize_mana: true,
        }
    }
}

#[derive(Debug)]
struct Args {
    scenarios: Vec<Scenario>,
    units: Vec<usize>,
    workers: Vec<usize>,
    /// `None` means the scenario's default; see `Args::run_length`.
    ticks: Option<u64>,
    /// `None` means the scenario's default; see `Args::run_length`.
    warmup: Option<u64>,
    all_buildings: AllBuildingsOptions,
    /// Prints the sub-step span breakdown for every scenario. All-buildings always prints it.
    spans: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            scenarios: vec![Scenario::Lane, Scenario::Cage, Scenario::Crowd],
            units: vec![700, 1_000, 5_000, 10_000],
            workers: vec![1, 2, 4],
            ticks: None,
            warmup: None,
            all_buildings: AllBuildingsOptions::default(),
            spans: false,
        }
    }
}

impl Args {
    /// Returns `(ticks, warmup)` for one scenario. Explicit flags always win. The
    /// all-buildings match defaults to `ALL_BUILDINGS_GAME_SECONDS` of game time with
    /// no warmup; every other scenario defaults to 200 ticks after 20 warmup ticks.
    fn run_length(&self, scenario: Scenario) -> (u64, u64) {
        match scenario {
            Scenario::AllBuildings => (
                self.ticks
                    .unwrap_or(ALL_BUILDINGS_GAME_SECONDS * simulation_hz()),
                self.warmup.unwrap_or(0),
            ),
            _ => (self.ticks.unwrap_or(200), self.warmup.unwrap_or(20)),
        }
    }
}

fn main() {
    let args = parse_args();
    println!("castle-fight deterministic simulation benchmark");
    println!(
        "scenarios={:?} units={:?} workers={:?} ticks={} warmup={}",
        args.scenarios,
        args.units,
        args.workers,
        optional_label(args.ticks),
        optional_label(args.warmup),
    );

    let mut expected_checksums = BTreeMap::<(Scenario, usize), u64>::new();
    let mut mismatch = false;

    for &scenario in &args.scenarios {
        let (ticks, warmup) = args.run_length(scenario);
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
                let result = run_case(scenario, units, workers, warmup, ticks, args.all_buildings);

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
                if scenario == Scenario::AllBuildings {
                    print_tick_profile(&result.tick_samples, warmup);
                }
                if args.spans || scenario == Scenario::AllBuildings {
                    print_span_breakdown(&result, scenario.name(), units, workers);
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
    /// Measured ticks, the divisor for `spans` averages.
    ticks: u64,
    /// Sub-step span totals over the measured ticks.
    spans: SpanTimings,
    /// Per-tick samples, recorded only for the all-buildings scenario.
    tick_samples: Vec<TickSample>,
}

/// One measured `Simulation::step` call: wall time plus the tick's reported state.
#[derive(Debug, Clone, Copy)]
struct TickSample {
    completed_tick: u64,
    wall: Duration,
    timings: TickTimings,
    units_alive: usize,
    buildings_alive: usize,
    projectiles_alive: usize,
    a_star_expanded_nodes: usize,
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

fn build_scenario(
    scenario: Scenario,
    units: usize,
    workers: usize,
    all_buildings: AllBuildingsOptions,
) -> (Simulation, ScenarioState) {
    if scenario == Scenario::AllBuildings {
        return (
            all_buildings_simulation(workers, all_buildings),
            ScenarioState::Static,
        );
    }
    let mut simulation = Simulation::new_with_combat_rules(
        scenario_config(scenario),
        workers,
        CombatRules {
            damage_rules: castle_fight_damage_rules(),
            ..CombatRules::default()
        },
    );
    let state = populate_scenario(scenario, &mut simulation, units);
    (simulation, state)
}

/// Team 0 takes ids `0..N` and team 1 takes ids `6..6+N`, as the match validator requires.
fn all_buildings_participants(players_per_team: usize) -> Vec<CastleFightParticipantConfig> {
    let participant = |id: u8, team: u8| CastleFightParticipantConfig {
        id: PlayerId(id),
        team: Team(team),
        builder_race: CastleFightBuilderRace::Human,
    };
    (0..players_per_team)
        .map(|slot| participant(u8::try_from(slot).expect("player slot fits in u8"), 0))
        .chain(
            (0..players_per_team).map(|slot| {
                participant(6 + u8::try_from(slot).expect("player slot fits in u8"), 1)
            }),
        )
        .collect()
}

/// Builds the real Castle Fight 9.27 match with one completed copy of every implemented
/// building for each player. The match is fully determined by `ALL_BUILDINGS_MATCH_SEED`, and
/// the developer-tool options are applied in a fixed order after creation.
fn all_buildings_simulation(workers: usize, options: AllBuildingsOptions) -> Simulation {
    let mut config = CastleFightMatchConfig::development_subset_with_participants(
        MapVersion::CASTLE_FIGHT_9_27,
        "r1",
        ALL_BUILDINGS_MATCH_SEED,
        all_buildings_participants(options.players_per_team),
    )
    .expect("valid Castle Fight 9.27 participants");
    config.rules.debug_starting_buildings = DebugStartingBuildings::All;
    let mut simulation = create_castle_fight_match(config, workers)
        .expect("registered Castle Fight 9.27 match")
        .simulation;
    if options.invulnerable_buildings {
        simulation.debug_set_buildings_invulnerable(true);
    }
    let mana_randomized = if options.randomize_mana {
        simulation.debug_randomize_spellcaster_mana(ALL_BUILDINGS_MANA_SEED)
    } else {
        0
    };
    println!(
        "all-buildings setup: players-per-team={} invulnerable={} mana-randomized={} initial-units={} initial-buildings={}",
        options.players_per_team,
        if options.invulnerable_buildings {
            "on"
        } else {
            "off"
        },
        mana_randomized,
        simulation.unit_count(),
        simulation.building_count(),
    );
    simulation
}

fn run_case(
    scenario: Scenario,
    units: usize,
    workers: usize,
    warmup: u64,
    ticks: u64,
    all_buildings: AllBuildingsOptions,
) -> BenchResult {
    let (mut simulation, mut state) = build_scenario(scenario, units, workers, all_buildings);

    for _ in 0..warmup {
        state.before_tick(&mut simulation);
        simulation.step();
    }

    let record_samples = scenario == Scenario::AllBuildings;
    let mut tick_samples = Vec::with_capacity(if record_samples {
        usize::try_from(ticks).expect("tick count fits in usize")
    } else {
        0
    });
    let start = Instant::now();
    let mut timings = TickTimings::default();
    let mut counters = BenchCounters::default();
    for _ in 0..ticks {
        state.before_tick(&mut simulation);
        let tick_start = Instant::now();
        let result = simulation.step();
        let wall = tick_start.elapsed();
        accumulate_timings(&mut timings, result.timings);
        accumulate_counters(&result, &mut counters);
        if record_samples {
            tick_samples.push(TickSample {
                completed_tick: result.completed_tick,
                wall,
                timings: result.timings,
                units_alive: result.units_alive,
                buildings_alive: result.buildings_alive,
                projectiles_alive: result.projectiles_alive,
                a_star_expanded_nodes: result.a_star_expanded_nodes,
            });
        }
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
        ticks,
        spans: timings.spans,
        tick_samples,
    }
}

fn simulation_hz() -> u64 {
    u64::try_from(CASTLE_FIGHT_SIMULATION_HZ).expect("simulation tick rate is positive")
}

fn optional_label(value: Option<u64>) -> String {
    value.map_or_else(|| "default".to_owned(), |value| value.to_string())
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn phase_durations(timings: &TickTimings) -> [Duration; PHASE_COUNT] {
    [
        timings.topology,
        timings.timers,
        timings.production,
        timings.snapshot_and_spatial,
        timings.abilities,
        timings.targeting,
        timings.combat,
        timings.movement_intent,
        timings.crowd_and_collision,
        timings.ballistic_impact,
        timings.structural_commit,
        timings.checksum,
    ]
}

/// Prints the sub-step breakdown: one row per nonzero span sorted by average ms per tick, then an
/// `untracked` row for the wall time not covered by any span.
fn print_span_breakdown(result: &BenchResult, scenario: &str, units: usize, workers: usize) {
    let wall_ms = result.ms_per_tick;
    let mut rows: Vec<(Span, Duration, f64)> = Span::ALL
        .into_iter()
        .filter_map(|span| {
            let total = result.spans.get(span);
            (total > Duration::ZERO).then(|| (span, total, ms_per_tick(total, result.ticks)))
        })
        .collect();
    rows.sort_by(|a, b| b.2.total_cmp(&a.2));
    let tracked_ms: f64 = rows.iter().map(|(_, _, ms)| ms).sum();
    let percent = |ms: f64| {
        if wall_ms > 0.0 {
            ms * 100.0 / wall_ms
        } else {
            0.0
        }
    };

    println!();
    println!(
        "span breakdown: scenario={scenario} units={units} workers={workers} ticks={} wall-ms/tick={wall_ms:.3} tracked-ms/tick={tracked_ms:.3}",
        result.ticks,
    );
    println!(
        "{:<28} {:<8} {:>10} {:>8}",
        "span", "phase", "avg ms/tick", "% wall"
    );
    for (span, _, ms) in &rows {
        println!(
            "{:<28} {:<8} {:>10.3} {:>7.2}%",
            span.name(),
            span.phase(),
            ms,
            percent(*ms),
        );
    }
    let untracked_ms = wall_ms - tracked_ms;
    println!(
        "{:<28} {:<8} {:>10.3} {:>7.2}%",
        "untracked",
        "-",
        untracked_ms,
        percent(untracked_ms),
    );
}

/// Prints per-window (10 game-second) timing and state, then the slowest individual ticks.
fn print_tick_profile(samples: &[TickSample], warmup: u64) {
    if samples.is_empty() {
        return;
    }
    let hz = simulation_hz();
    let window_ticks =
        usize::try_from(TICK_WINDOW_SECONDS * hz).expect("window tick count fits in usize");

    println!();
    println!(
        "all-buildings window breakdown: {TICK_WINDOW_SECONDS}s game-time windows ({window_ticks} ticks), warmup={warmup}"
    );
    let phase_header: String = PHASE_SHORT_NAMES
        .iter()
        .map(|name| format!(" {name:>7}"))
        .collect();
    println!(
        "{:>11} {:>6} {:>9} {:>9}{phase_header} {:>7} {:>7} {:>7} {:>7} {:>9}",
        "game-s", "ticks", "avg-ms", "max-ms", "units", "peak-u", "bldgs", "proj", "nodes/t",
    );

    for window in samples.chunks(window_ticks) {
        let count = window.len();
        let first = window[0].completed_tick;
        let last = window[count - 1].completed_tick;
        let wall_sum: Duration = window.iter().map(|sample| sample.wall).sum();
        let max_wall = window
            .iter()
            .map(|sample| sample.wall)
            .max()
            .unwrap_or_default();
        let mut phase_sums = [Duration::ZERO; PHASE_COUNT];
        for sample in window {
            for (sum, phase) in phase_sums.iter_mut().zip(phase_durations(&sample.timings)) {
                *sum += phase;
            }
        }
        let phase_cells: String = phase_sums
            .iter()
            .map(|sum| format!(" {:>7.3}", duration_ms(*sum) / count as f64))
            .collect();
        let nodes: usize = window
            .iter()
            .map(|sample| sample.a_star_expanded_nodes)
            .sum();
        let peak_units = window
            .iter()
            .map(|sample| sample.units_alive)
            .max()
            .unwrap_or_default();
        let end = &window[count - 1];
        println!(
            "{:>11} {:>6} {:>9.3} {:>9.3}{phase_cells} {:>7} {:>7} {:>7} {:>7} {:>9.1}",
            format!("{}-{}", first / hz, (last + 1) / hz),
            count,
            duration_ms(wall_sum) / count as f64,
            duration_ms(max_wall),
            end.units_alive,
            peak_units,
            end.buildings_alive,
            end.projectiles_alive,
            nodes as f64 / count as f64,
        );
    }

    let mut slowest: Vec<&TickSample> = samples.iter().collect();
    slowest.sort_by(|a, b| {
        b.wall
            .cmp(&a.wall)
            .then(a.completed_tick.cmp(&b.completed_tick))
    });
    println!();
    println!(
        "slowest {} ticks (wall-clock per Simulation::step, top 3 phases by time)",
        slowest.len().min(SLOWEST_TICK_COUNT)
    );
    println!(
        "{:>4} {:>10} {:>10}  top phases (ms)",
        "rank", "tick", "total-ms"
    );
    for (rank, sample) in slowest.iter().take(SLOWEST_TICK_COUNT).enumerate() {
        let mut phases: Vec<(&str, Duration)> = PHASE_NAMES
            .into_iter()
            .zip(phase_durations(&sample.timings))
            .collect();
        phases.sort_by_key(|(_, duration)| std::cmp::Reverse(*duration));
        let top: Vec<String> = phases
            .iter()
            .take(3)
            .map(|(name, duration)| format!("{name}={:.3}", duration_ms(*duration)))
            .collect();
        println!(
            "{:>4} {:>10} {:>10.3}  {}",
            rank + 1,
            sample.completed_tick,
            duration_ms(sample.wall),
            top.join(", "),
        );
    }
}

fn scenario_config(scenario: Scenario) -> SimulationConfig {
    let mut config = SimulationConfig::default();
    if scenario == Scenario::Bounce {
        config.match_seed = 0x5eed_b0ce_2026_0912;
    }
    if matches!(scenario, Scenario::Ability | Scenario::MultiAbility) {
        config.match_seed = 0x5eed_ab11_17e5_2026;
    }
    if scenario == Scenario::Mixed {
        config.match_seed = 0x5eed_7000_cafe_2026;
    }
    if matches!(
        scenario,
        Scenario::Lane | Scenario::Cage | Scenario::Crowd | Scenario::Topology
    ) {
        let world = SUBUNITS_PER_WORLD_UNIT;
        config.spatial_cell_size = 256 * world;
        config.navigation_cell_size = 32 * world;
        config.navigation_min = castle_fight_sim::NavCell::new(-256, -128);
        config.navigation_max = castle_fight_sim::NavCell::new(256, 128);
        config.target_pursuit_extra_range = 96 * world;
        config.unit_separation_distance = 32 * world;
        config.max_separation_per_tick = world;
        config.team_objective = [
            SimPoint::new(6_000 * world, 0),
            SimPoint::new(-6_000 * world, 0),
        ];
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

fn spawn_imported_unit(
    simulation: &mut Simulation,
    kind: CastleFightUnitKind,
    team: Team,
    position: SimPoint,
) {
    simulation.spawn_resolved_unit(team, position, kind.definition().resolved());
}

fn populate_imported_lane_battle(simulation: &mut Simulation, total_units: usize) {
    let world = SUBUNITS_PER_WORLD_UNIT;
    let per_team = total_units / 2;
    const COLUMNS: usize = 100;
    let spacing = 40 * world;
    for team in 0..2u8 {
        for index in 0..per_team {
            let column = index % COLUMNS;
            let row = index / COLUMNS;
            let x = if team == 0 {
                -512 * world - i32::try_from(column).expect("column fits i32") * spacing
            } else {
                512 * world + i32::try_from(column).expect("column fits i32") * spacing
            };
            let y = (i32::try_from(row).expect("row fits i32")
                - i32::try_from(per_team.div_ceil(COLUMNS) / 2).expect("row count fits i32"))
                * spacing;
            let kind = CastleFightUnitKind::ALL[index % CastleFightUnitKind::ALL.len()];
            spawn_imported_unit(simulation, kind, Team(team), SimPoint::new(x, y));
        }
    }
}

fn populate_imported_crossing_crowd(simulation: &mut Simulation, total_units: usize) {
    let world = SUBUNITS_PER_WORLD_UNIT;
    let per_team = total_units / 2;
    const COLUMNS: usize = 100;
    let spacing = 40 * world;
    for team in 0..2u8 {
        for index in 0..per_team {
            let column = index % COLUMNS;
            let row = index / COLUMNS;
            let x = if team == 0 {
                -320 * world - i32::try_from(column).expect("column fits i32") * spacing
            } else {
                320 * world + i32::try_from(column).expect("column fits i32") * spacing
            };
            let y = (i32::try_from(row).expect("row fits i32")
                - i32::try_from(per_team.div_ceil(COLUMNS) / 2).expect("row count fits i32"))
                * spacing;
            spawn_imported_unit(
                simulation,
                CastleFightUnitKind::Footman,
                Team(team),
                SimPoint::new(x, y),
            );
        }
    }
}

fn populate_imported_cage_battle(simulation: &mut Simulation, total_units: usize) {
    let barracks = CastleFightProductionKind::Barracks.definition();
    let cage_team = Team(1);
    for y in (-32..=28).step_by(4) {
        for x in [-24, 20] {
            let footprint = BuildingFootprint::new(x, y, 4, 4);
            simulation.spawn_building_with_properties(
                barracks.spawn(cage_team, footprint),
                barracks.gameplay_properties(),
            );
        }
    }
    for x in (-20..=16).step_by(4) {
        for y in [-32, 28] {
            let footprint = BuildingFootprint::new(x, y, 4, 4);
            simulation.spawn_building_with_properties(
                barracks.spawn(cage_team, footprint),
                barracks.gameplay_properties(),
            );
        }
    }

    let world = SUBUNITS_PER_WORLD_UNIT;
    let per_team = total_units / 2;
    const COLUMNS: usize = 40;
    let spacing = 40 * world;
    for index in 0..per_team {
        let column = index % COLUMNS;
        let row = index / COLUMNS;
        let y = (i32::try_from(row).expect("row fits i32")
            - i32::try_from(per_team.div_ceil(COLUMNS) / 2).expect("row count fits i32"))
            * spacing;
        let outside_x = -1_200 * world - i32::try_from(column).expect("column fits i32") * spacing;
        spawn_imported_unit(
            simulation,
            CastleFightUnitKind::Footman,
            Team(0),
            SimPoint::new(outside_x, y),
        );

        let inside_column = i32::try_from(column % 18).expect("column fits i32");
        let inside_x = (-340 + inside_column * 40) * world;
        let inside_y = ((i32::try_from(index / 18).expect("row fits i32") % 40) - 20) * 40 * world;
        spawn_imported_unit(
            simulation,
            CastleFightUnitKind::Footman,
            cage_team,
            SimPoint::new(inside_x, inside_y),
        );
    }
}

fn populate_scenario(
    scenario: Scenario,
    simulation: &mut Simulation,
    units: usize,
) -> ScenarioState {
    match scenario {
        Scenario::Lane => populate_imported_lane_battle(simulation, units),
        Scenario::Cage => populate_imported_cage_battle(simulation, units),
        Scenario::Crowd => populate_imported_crossing_crowd(simulation, units),
        Scenario::Pathing => populate_pathing_wall_battle(simulation, units),
        Scenario::Topology => populate_imported_lane_battle(simulation, units),
        Scenario::Production => populate_production_churn(simulation, units),
        Scenario::Projectile => populate_projectile_density_battle(simulation, units),
        Scenario::Ballistic => populate_ballistic_density_battle(simulation, units),
        Scenario::Bounce => populate_bounce_density_battle(simulation, units),
        Scenario::Tower => populate_attack_building_density(simulation, units),
        Scenario::Ability => populate_automatic_ability_density(simulation, units, false),
        Scenario::MultiAbility => populate_automatic_ability_density(simulation, units, true),
        Scenario::Stun => populate_global_stun_density(simulation, units),
        Scenario::Slow => populate_timed_movement_modifier_density(simulation, units),
        Scenario::Radius => populate_mixed_radius_battle(simulation, units),
        Scenario::Traffic => populate_traffic_jam(simulation, units),
        Scenario::TrafficFlow => populate_traffic_flow(simulation, units, false),
        Scenario::TrafficProduction => populate_production_fed_traffic(simulation, units),
        Scenario::Mixed => populate_mixed_battle(simulation, units),
        Scenario::AllBuildings => {
            unreachable!("all-buildings is built by the real match constructor")
        }
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

fn populate_automatic_ability_density(
    simulation: &mut Simulation,
    total_units: usize,
    multi: bool,
) {
    let caster_count = (total_units / 4).clamp(2, 500);
    let per_team_casters = caster_count.div_ceil(2);
    let spellcasting = SpellcastingProfile {
        mana: ManaProfile {
            maximum: 1_000_000_000,
            starting: 1_000_000_000,
            regen_per_tick_per_10k: 10_000,
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
            let source = simulation.spawn_building(BuildingSpawn {
                team: Team(team),
                footprint: BuildingFootprint::new(x, y, 1, 1),
                health: 1_000_000_000,
                production: None,
                attack: None,
                spellcasting: Some(spellcasting),
            });
            if multi {
                simulation
                    .configure_additional_automatic_abilities(
                        source,
                        &[
                            AutomaticAbilityProfile {
                                id: AbilityId(2),
                                cooldown_ticks: 3,
                                effect: AbilityEffect::Damage { amount: 2 },
                                ..spellcasting.ability
                            },
                            AutomaticAbilityProfile {
                                id: AbilityId(3),
                                cooldown_ticks: 7,
                                effect: AbilityEffect::Stun { duration_ticks: 2 },
                                ..spellcasting.ability
                            },
                        ],
                    )
                    .expect("synthetic multi-ability benchmark profiles are valid");
            }
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
            regen_per_tick_per_10k: 0,
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
            regen_per_tick_per_10k: 0,
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
            regen_per_tick_per_10k: 10_000,
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
            regen_per_tick_per_10k: 10_000,
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
    total.spans.accumulate(&tick.spans);
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
            "--ticks" => {
                args.ticks = Some(parse_number(&next_value(&mut iter, "--ticks"), "--ticks"));
            }
            "--warmup" => {
                args.warmup = Some(parse_number(&next_value(&mut iter, "--warmup"), "--warmup"));
            }
            "--players-per-team" => {
                let players: usize = parse_number(
                    &next_value(&mut iter, "--players-per-team"),
                    "--players-per-team",
                );
                assert!(
                    (1..=3).contains(&players),
                    "--players-per-team must be 1, 2, or 3"
                );
                args.all_buildings.players_per_team = players;
            }
            "--mortal-buildings" => args.all_buildings.invulnerable_buildings = false,
            "--spans" => args.spans = true,
            "--synced-mana" => args.all_buildings.randomize_mana = false,
            "-h" | "--help" => {
                println!("Usage: cargo run --release -p castle-fight-sim-bench -- [options]");
                println!(
                    "  --scenario lane,cage,crowd,pathing,topology,production,projectile,ballistic,bounce,tower,ability,stun,slow,radius,traffic,traffic-flow,traffic-production,mixed,all-buildings"
                );
                println!("  --units 700,1000,5000,10000");
                println!(
                    "  --spans (print the per-sub-step span breakdown for every scenario; all-buildings always prints it)"
                );
                println!("  --workers 1,2,4,8");
                println!("  --ticks 200 (all-buildings: 120 seconds of game time)");
                println!("  --warmup 20 (all-buildings: 0)");
                println!("Pathing is intentionally adversarial; start with smaller unit counts.");
                println!(
                    "all-buildings runs the real Castle Fight 9.27 match with every implemented building; --units is ignored."
                );
                println!("  --players-per-team 1|2|3 (all-buildings only; default 3, i.e. 3v3)");
                println!(
                    "  --mortal-buildings (all-buildings: buildings are invulnerable by default so production stays sustained)"
                );
                println!(
                    "  --synced-mana (all-buildings: keep every spellcaster at its starting mana instead of a seeded random value)"
                );
                std::process::exit(0);
            }
            other => panic!("unknown argument: {other}"),
        }
    }

    if let Some(ticks) = args.ticks {
        assert!(ticks > 0, "--ticks must be positive");
    }
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
            "multi-ability" => Scenario::MultiAbility,
            "stun" => Scenario::Stun,
            "slow" => Scenario::Slow,
            "radius" => Scenario::Radius,
            "traffic" => Scenario::Traffic,
            "traffic-flow" => Scenario::TrafficFlow,
            "traffic-production" => Scenario::TrafficProduction,
            "mixed" => Scenario::Mixed,
            "all-buildings" => Scenario::AllBuildings,
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
