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
    assert_eq!(sim.round_index(), 1);
    assert_eq!(sim.round_wins(), [1, 0]);
    let until = match sim.lifecycle() {
        MatchLifecycle::RoundReview {
            outcome: MatchOutcome::Victory(Team(0)),
            next_round_tick,
            ..
        } => next_round_tick,
        state => panic!("expected round review, got {state:?}"),
    };
    assert_eq!(sim.review_seconds_remaining(), Some(15));
    assert_eq!(sim.last_round_scoreboard().unwrap().round, 1);
    let during_review = sim.checksum();
    sim.step();
    assert_ne!(sim.checksum(), during_review);
    assert_eq!(sim.round_index(), 1);
    while sim.tick() < until {
        sim.step();
    }
    assert_eq!(sim.round_index(), 2);
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
fn review_snapshot_restores_scoreboard_and_countdown() {
    let mut sim = timed_simulation(2);
    sim.player_strikes[1].available = false;
    sim.next_tick = sim.config.match_rules.duration_ticks().unwrap() - 1;
    sim.step();
    let saved = sim.capture_snapshot();
    let hash = sim.checksum();
    sim.step();
    sim.restore_snapshot(&saved).unwrap();
    assert_eq!(sim.checksum(), hash);
    assert_eq!(sim.review_seconds_remaining(), Some(15));
    assert_eq!(sim.last_round_scoreboard().unwrap().round_wins, [1, 0]);
}

#[test]
fn disconnect_pauses_round_review_without_losing_countdown() {
    let mut sim = timed_simulation(2);
    sim.player_strikes[1].available = false;
    sim.next_tick = sim.config.match_rules.duration_ticks().unwrap() - 1;
    sim.step();
    let remaining = sim.review_seconds_remaining();
    let tick = sim.tick();
    assert!(sim.set_player_connection_status(PlayerId(1), PlayerConnectionStatus::Disconnected));
    assert!(matches!(
        sim.lifecycle(),
        MatchLifecycle::PausedForDisconnect { .. }
    ));
    sim.step();
    assert_eq!(sim.tick(), tick);
    assert_eq!(sim.review_seconds_remaining(), remaining);
    assert!(sim.set_player_connection_status(PlayerId(1), PlayerConnectionStatus::Connected));
    assert!(matches!(
        sim.lifecycle(),
        MatchLifecycle::RoundReview { .. }
    ));
}

#[test]
fn round_review_ticks_replay_identically_across_authoritative_stream() {
    use crate::{CanonicalStreamRecord, MatchDriver, castle_fight_content_bundle};
    let content = castle_fight_content_bundle(crate::MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let mut host = timed_simulation(2);
    let mut replica = timed_simulation(2);
    host.player_strikes[1].available = false;
    replica.player_strikes[1].available = false;
    let last_tick = host.config.match_rules.duration_ticks().unwrap() - 1;
    host.next_tick = last_tick;
    replica.next_tick = last_tick;
    let mut host_driver = MatchDriver::new(&host, content);
    let mut replica_driver = MatchDriver::new(&replica, content);
    for _ in 0..(15 * crate::CASTLE_FIGHT_SIMULATION_HZ as usize + 2) {
        let record = host_driver.advance_local_tick(&mut host).unwrap();
        replica_driver
            .apply_stream_record(&mut replica, CanonicalStreamRecord::Tick(record.finalized))
            .unwrap();
        assert_eq!(host.checksum(), replica.checksum());
        assert_eq!(host.tick(), replica.tick());
    }
    assert_eq!(host.round_index(), 2);
    assert_eq!(replica.round_index(), 2);
}

#[test]
fn rescue_strike_uses_separate_chaos_death_and_normal_magic_damage_packets() {
    let mut sim = timed_simulation(1);
    let armor = ArmorProfile::new(crate::ArmorType::Fortified, 25);
    let target = sim.spawn_unit_with_properties(
        UnitSpawn {
            team: Team(1),
            position: SimPoint::new(0, 0),
            health: 50_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 100,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        },
        UnitGameplayProperties {
            armor,
            ..Default::default()
        },
    );
    sim.pending_rescue_strikes.push(PendingRescueStrike {
        player: PlayerId(0),
        builder: SimId(999),
        center: SimPoint::new(0, 0),
        resolves_at: 0,
        finishes_at: 45,
        kills: None,
    });
    sim.advance_rescue_strikes(0);
    let rules = sim.config.rescue_strike_rules.unwrap();
    let expected = sim.combat_rules.damage_rules.apply_attack(
        rules.damage_each_packet,
        DamageType::Chaos,
        armor,
    ) + sim
        .combat_rules
        .damage_rules
        .apply_attack_with_armor_per_100(
            rules.damage_each_packet,
            DamageType::Normal,
            armor.armor_type,
            0,
        );
    assert_ne!(expected, rules.damage_each_packet * 2);
    assert_eq!(sim.unit(target).unwrap().health, 50_000 - expected);
    assert_eq!(sim.pending_rescue_strikes[0].kills, Some(1));
    assert_eq!(sim.last_rescue_strike_visuals[0].targets, vec![target]);
}

#[test]
fn empty_rescue_strike_refunds_only_after_finish_delay() {
    let mut sim = timed_simulation(1);
    sim.player_strikes[0].available = false;
    sim.pending_rescue_strikes.push(PendingRescueStrike {
        player: PlayerId(0),
        builder: SimId(999),
        center: SimPoint::new(0, 0),
        resolves_at: 0,
        finishes_at: 45,
        kills: None,
    });
    sim.advance_rescue_strikes(0);
    assert!(!sim.player_strikes[0].available);
    sim.advance_rescue_strikes(44);
    assert!(!sim.player_strikes[0].available);
    sim.advance_rescue_strikes(45);
    assert!(sim.player_strikes[0].available);
    assert_eq!(sim.player_strikes[0].cooldown_until_tick, 45 + 180 * 30);
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
