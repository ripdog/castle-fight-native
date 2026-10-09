use super::*;

fn timed_simulation(rounds_to_win: u8) -> Simulation {
    let config = SimulationConfig {
        match_rules: crate::MatchRules {
            time_limit_minutes: 1,
            time_limit_outcome: crate::TimeLimitOutcome::MoreStrikesThenIncome,
            rounds_to_win,
            rescue_strikes_enabled: true,
        },
        rescue_strike_rules: crate::RescueStrikeRules::for_version(
            crate::MapVersion::CASTLE_FIGHT_9_27,
        ),
        ..Default::default()
    };
    let mut sim = Simulation::new(config, 1);
    sim.prepare_round_restart();
    sim
}

#[test]
fn timed_match_prefers_remaining_strikes() {
    let mut sim = timed_simulation(1);
    sim.player_strikes[0].available = false;
    sim.next_tick = sim.config.match_rules.duration_ticks().unwrap() - 1;
    sim.step();
    assert_eq!(
        sim.lifecycle(),
        MatchLifecycle::Finished {
            outcome: MatchOutcome::Victory(Team(1)),
            finished_tick: sim.tick() - 1,
        }
    );
}

#[test]
fn timed_match_draws_when_strikes_and_income_are_equal() {
    let mut sim = timed_simulation(1);
    sim.next_tick = sim.config.match_rules.duration_ticks().unwrap() - 1;
    sim.step();
    assert!(matches!(
        sim.lifecycle(),
        MatchLifecycle::Finished {
            outcome: MatchOutcome::Draw,
            ..
        }
    ));
}

#[test]
fn timed_match_uses_team_income_when_strike_counts_tie() {
    let mut sim = timed_simulation(1);
    sim.spawn_building_for_player_with_properties(
        PlayerId(0),
        BuildingSpawn {
            team: Team(0),
            footprint: BuildingFootprint::new(2, 2, 2, 2),
            health: 1000,
            production: None,
            attack: None,
            spellcasting: None,
        },
        BuildingGameplayProperties {
            economy: Some(BuildingEconomyProfile {
                income_per_10k: 200_000,
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    assert!(
        sim.player_income_for(PlayerId(0)).unwrap() > sim.player_income_for(PlayerId(1)).unwrap()
    );
    sim.next_tick = sim.config.match_rules.duration_ticks().unwrap() - 1;
    sim.step();
    assert!(matches!(
        sim.lifecycle(),
        MatchLifecycle::Finished {
            outcome: MatchOutcome::Victory(Team(0)),
            ..
        }
    ));
}

#[test]
fn timed_rounds_reset_strikes_and_keep_monotonic_ticks() {
    let mut sim = timed_simulation(2);
    sim.player_strikes[1].available = false;
    sim.next_tick = sim.config.match_rules.duration_ticks().unwrap() - 1;
    sim.step();
    assert_eq!(sim.round_index(), 2);
    assert_eq!(sim.round_wins(), [1, 0]);
    assert_eq!(sim.rescue_strikes_left(Team(1)), 1);
    assert_eq!(sim.round_elapsed_ticks(), 0);
    assert_eq!(sim.lifecycle(), MatchLifecycle::Running);
    sim.player_strikes[1].available = false;
    sim.next_tick = sim.round_start_tick + sim.config.match_rules.duration_ticks().unwrap() - 1;
    sim.step();
    assert_eq!(sim.round_wins(), [2, 0]);
    assert!(matches!(
        sim.lifecycle(),
        MatchLifecycle::Finished {
            outcome: MatchOutcome::Victory(Team(0)),
            ..
        }
    ));
}

#[test]
fn strike_budget_is_canonical_and_restores_from_snapshot() {
    let mut sim = timed_simulation(1);
    let snapshot = sim.capture_snapshot();
    let original = sim.checksum();
    sim.player_strikes[0].available = false;
    assert_ne!(sim.checksum(), original);
    sim.restore_snapshot(&snapshot).unwrap();
    assert_eq!(sim.rescue_strikes_left(Team(0)), 1);
    assert_eq!(sim.checksum(), original);
}
