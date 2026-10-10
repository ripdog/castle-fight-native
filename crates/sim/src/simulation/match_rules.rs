use super::*;

/// Per-player figures displayed live and snapshotted at the end of every round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerRoundScore {
    pub player: PlayerId,
    pub team: Team,
    pub gold: u32,
    pub lumber: u32,
    pub income: u32,
    pub units: u32,
    pub buildings: u32,
    pub rescue_strikes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundScoreboard {
    pub round: u16,
    pub outcome: MatchOutcome,
    pub ended_tick: u64,
    pub elapsed_ticks: u64,
    pub players: Vec<PlayerRoundScore>,
    pub round_wins: [u8; 2],
}

/// Castle Fight 9.27's between-round statistics review is 15 seconds.
const ROUND_REVIEW_SECONDS_927: u64 = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PlayerStrikeState {
    pub player: PlayerId,
    pub available: bool,
    pub cooldown_until_tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PendingRescueStrike {
    pub player: PlayerId,
    pub builder: SimId,
    pub center: SimPoint,
    pub resolves_at: u64,
    pub finishes_at: u64,
    pub kills: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RescueStrikeVisualKind {
    Impact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RescueStrikeVisualEvent {
    pub kind: RescueStrikeVisualKind,
    pub center: SimPoint,
    pub radius: i32,
    pub targets: Vec<SimId>,
}

impl Simulation {
    pub fn prepare_round_restart(&mut self) {
        assert_eq!(
            self.next_tick, 0,
            "round template must be captured before the first tick"
        );
        self.initial_round = Some(Box::new(self.capture_snapshot()));
    }

    #[must_use]
    pub fn match_rules(&self) -> crate::MatchRules {
        self.config.match_rules
    }

    #[must_use]
    pub fn round_index(&self) -> u16 {
        self.round_index
    }

    #[must_use]
    pub fn current_round_scores(&self) -> Vec<PlayerRoundScore> {
        let units = self.units();
        let buildings = self.buildings();
        self.players
            .iter()
            .map(|player| PlayerRoundScore {
                player: player.id,
                team: player.team,
                gold: player.resources.gold,
                lumber: player.resources.lumber,
                income: self.player_income_for(player.id).unwrap_or(0),
                units: units.iter().filter(|unit| unit.owner == player.id).count() as u32,
                buildings: buildings
                    .iter()
                    .filter(|building| building.owner == Some(player.id))
                    .count() as u32,
                rescue_strikes: u32::from(
                    self.player_strikes
                        .iter()
                        .any(|strike| strike.player == player.id && strike.available),
                ),
            })
            .collect()
    }

    #[must_use]
    pub fn last_round_scoreboard(&self) -> Option<&RoundScoreboard> {
        self.last_round_scoreboard.as_ref()
    }

    #[must_use]
    pub fn review_seconds_remaining(&self) -> Option<u64> {
        self.review_until_tick.map(|ends| {
            ends.saturating_sub(self.next_tick)
                .div_ceil(crate::CASTLE_FIGHT_SIMULATION_HZ as u64)
        })
    }

    #[must_use]
    pub fn round_wins(&self) -> [u8; 2] {
        self.round_wins
    }

    #[must_use]
    pub fn round_elapsed_ticks(&self) -> u64 {
        self.next_tick.saturating_sub(self.round_start_tick)
    }

    #[must_use]
    pub fn rescue_strikes_left(&self, team: Team) -> u32 {
        self.player_strikes
            .iter()
            .filter(|strike| {
                strike.available
                    && self
                        .player_state(strike.player)
                        .is_some_and(|player| player.team == team)
            })
            .count() as u32
    }

    #[must_use]
    pub fn can_cast_rescue_strike(&self, player: PlayerId, builder: SimId) -> bool {
        if !self.config.match_rules.rescue_strikes_enabled
            || self.config.rescue_strike_rules.is_none()
            || self.lifecycle != MatchLifecycle::Running
        {
            return false;
        }
        let Some(builder_view) = self.builder(builder) else {
            return false;
        };
        let Some(owner) = self.player_state(builder_view.owner) else {
            return false;
        };
        let Some(state) = self
            .player_strikes
            .iter()
            .find(|state| state.player == builder_view.owner)
        else {
            return false;
        };
        state.available
            && state.cooldown_until_tick <= self.next_tick
            && self.team_rescue_ready_ticks[owner.team.0 as usize] <= self.next_tick
            && !self.pending_rescue_strikes.iter().any(|pending| {
                self.player_state(pending.player)
                    .is_some_and(|p| p.team == owner.team)
            })
            && self.can_player_control_builder(player, builder)
    }

    pub fn cast_rescue_strike_for_player(
        &mut self,
        player: PlayerId,
        builder: SimId,
        center: SimPoint,
    ) -> Result<(), BuilderCommandError> {
        if !self.can_cast_rescue_strike(player, builder) {
            return Err(BuilderCommandError::NotAuthorized);
        }
        let rules = self
            .config
            .rescue_strike_rules
            .expect("validated strike rules");
        let owner = self.builder(builder).expect("validated builder").owner;
        let resolves_at = self.next_tick.saturating_add(rules.cast_delay_ticks);
        self.pending_rescue_strikes.push(PendingRescueStrike {
            player: owner,
            builder,
            center,
            resolves_at,
            finishes_at: resolves_at.saturating_add(rules.finish_delay_ticks),
            kills: None,
        });
        self.player_strikes
            .iter_mut()
            .find(|state| state.player == owner)
            .expect("validated player strike")
            .available = false;
        Ok(())
    }

    /// Targets are determined at the authored impact tick, not at cursor click time.
    pub(super) fn advance_rescue_strikes(&mut self, completed_tick: u64) {
        let Some(rules) = self.config.rescue_strike_rules else {
            return;
        };
        for index in 0..self.pending_rescue_strikes.len() {
            let pending = self.pending_rescue_strikes[index];
            if pending.kills.is_none() && completed_tick >= pending.resolves_at {
                let source_team = self
                    .player_state(pending.player)
                    .expect("strike caster persists")
                    .team;
                let radius_squared = i64::from(rules.radius_subunits).pow(2);
                let mut targets = Vec::new();
                let mut kills = 0;
                let mut query = self.world.query::<(
                    Entity,
                    &SimId,
                    &Team,
                    Option<&Position>,
                    Option<&BuildingFootprint>,
                    Option<&UnitClassifications>,
                    Option<&Builder>,
                    Option<&ArmorProfile>,
                    &Health,
                )>();
                let mut eligible = Vec::new();
                for (
                    entity,
                    id,
                    team,
                    position,
                    footprint,
                    classifications,
                    builder,
                    armor,
                    health,
                ) in query.iter(&self.world)
                {
                    if *team == source_team || builder.is_some() {
                        continue;
                    }
                    if classifications.is_some_and(|classifications| classifications.invulnerable) {
                        continue;
                    }
                    let position = position.map(|point| point.0).or_else(|| {
                        footprint.map(|footprint| {
                            SimPoint::new(
                                (footprint.min_x * 2 + i32::from(footprint.width))
                                    * self.config.navigation_cell_size
                                    / 2,
                                (footprint.min_y * 2 + i32::from(footprint.height))
                                    * self.config.navigation_cell_size
                                    / 2,
                            )
                        })
                    });
                    let Some(position) = position else {
                        continue;
                    };
                    let dx = i64::from(position.x) - i64::from(pending.center.x);
                    let dy = i64::from(position.y) - i64::from(pending.center.y);
                    if dx * dx + dy * dy > radius_squared {
                        continue;
                    }
                    if health.current <= 0 {
                        continue;
                    }
                    eligible.push((*id, entity, armor.copied().unwrap_or_default()));
                }
                eligible.sort_unstable_by_key(|(id, _, _)| *id);
                for (id, entity, armor) in eligible {
                    if self.world.get::<Health>(entity).unwrap().current <= 0 {
                        continue;
                    }
                    // Source map invokes UnitDamageTarget twice: CHAOS/DEATH and
                    // NORMAL/MAGIC. The first uses physical armor; magic damage
                    // preserves normal attack-vs-armor-type scaling but ignores armor
                    // points. Keep the packets separate for deterministic mitigation.
                    let damage_rules = self.combat_rules.damage_rules;
                    let death_packet = damage_rules.apply_attack(
                        rules.damage_each_packet,
                        DamageType::Chaos,
                        armor,
                    );
                    let magic_packet = damage_rules.apply_attack_with_armor_per_100(
                        rules.damage_each_packet,
                        DamageType::Normal,
                        armor.armor_type,
                        0,
                    );
                    for packet in [death_packet, magic_packet] {
                        let mut health = self.world.get_mut::<Health>(entity).unwrap();
                        let before = health.current;
                        health.current = health.current.saturating_sub(packet);
                        let died = before > 0 && health.current <= 0;
                        if died {
                            self.observe_world_death_heal(id);
                        }
                    }
                    // The original map counts eligible victims, not confirmed deaths,
                    // for deciding whether to refund an unused strike.
                    kills += 1;
                    targets.push(id);
                }
                let team_index = usize::from(source_team.0);
                self.team_rescue_ready_ticks[team_index] =
                    completed_tick.saturating_add(rules.team_cooldown_ticks);
                self.pending_rescue_strikes[index].kills = Some(kills);
                self.last_rescue_strike_visuals
                    .push(RescueStrikeVisualEvent {
                        kind: RescueStrikeVisualKind::Impact,
                        center: pending.center,
                        radius: rules.radius_subunits,
                        targets,
                    });
            }
        }
        let mut index = 0;
        while index < self.pending_rescue_strikes.len() {
            let pending = self.pending_rescue_strikes[index];
            if pending.kills.is_some() && completed_tick >= pending.finishes_at {
                if pending.kills == Some(0)
                    && let Some(state) = self
                        .player_strikes
                        .iter_mut()
                        .find(|state| state.player == pending.player)
                {
                    state.available = true;
                    state.cooldown_until_tick =
                        completed_tick.saturating_add(rules.zero_kill_cooldown_ticks);
                }
                self.pending_rescue_strikes.remove(index);
            } else {
                index += 1;
            }
        }
    }

    #[must_use]
    pub fn rescue_strike_visuals_last_tick(&self) -> &[RescueStrikeVisualEvent] {
        &self.last_rescue_strike_visuals
    }

    #[must_use]
    pub fn pending_rescue_strike_centers(&self) -> Vec<SimPoint> {
        self.pending_rescue_strikes
            .iter()
            .map(|strike| strike.center)
            .collect()
    }

    #[must_use]
    pub fn pending_rescue_strikes(&self) -> Vec<(SimId, SimPoint)> {
        self.pending_rescue_strikes
            .iter()
            .map(|strike| (strike.builder, strike.center))
            .collect()
    }

    fn decide_time_limit_outcome(&self) -> MatchOutcome {
        if self.config.match_rules.time_limit_outcome == crate::TimeLimitOutcome::Draw {
            return MatchOutcome::Draw;
        }
        let strikes = [
            self.rescue_strikes_left(Team(0)),
            self.rescue_strikes_left(Team(1)),
        ];
        match strikes[0].cmp(&strikes[1]) {
            std::cmp::Ordering::Greater => MatchOutcome::Victory(Team(0)),
            std::cmp::Ordering::Less => MatchOutcome::Victory(Team(1)),
            std::cmp::Ordering::Equal => {
                let mut income = [0u64; 2];
                for player in &self.players {
                    income[usize::from(player.team.0)] +=
                        u64::from(self.player_income_for(player.id).unwrap_or(0));
                }
                match income[0].cmp(&income[1]) {
                    std::cmp::Ordering::Greater => MatchOutcome::Victory(Team(0)),
                    std::cmp::Ordering::Less => MatchOutcome::Victory(Team(1)),
                    std::cmp::Ordering::Equal => MatchOutcome::Draw,
                }
            }
        }
    }

    pub(super) fn finish_round(&mut self, outcome: MatchOutcome, completed_tick: u64) {
        if let MatchOutcome::Victory(team) = outcome {
            self.round_wins[usize::from(team.0)] += 1;
            if self.round_wins[usize::from(team.0)] >= self.config.match_rules.rounds_to_win {
                self.finish_round_match(outcome, completed_tick);
                return;
            }
        } else if self.config.match_rules.rounds_to_win == 1 {
            self.finish_round_match(outcome, completed_tick);
            return;
        }
        if self.initial_round.is_some() {
            self.capture_round_scoreboard(outcome, completed_tick);
            let next_round_tick = completed_tick
                + 1
                + ROUND_REVIEW_SECONDS_927 * crate::CASTLE_FIGHT_SIMULATION_HZ as u64;
            self.review_until_tick = Some(next_round_tick);
            self.lifecycle = MatchLifecycle::RoundReview {
                outcome,
                ended_tick: completed_tick,
                next_round_tick,
            };
        } else {
            self.finish_round_match(outcome, completed_tick);
        }
    }

    pub(super) fn capture_round_scoreboard(&mut self, outcome: MatchOutcome, completed_tick: u64) {
        self.last_round_scoreboard = Some(RoundScoreboard {
            round: self.round_index,
            outcome,
            ended_tick: completed_tick,
            elapsed_ticks: completed_tick + 1 - self.round_start_tick,
            players: self.current_round_scores(),
            round_wins: self.round_wins,
        });
    }

    fn finish_round_match(&mut self, outcome: MatchOutcome, completed_tick: u64) {
        self.capture_round_scoreboard(outcome, completed_tick);
        self.review_until_tick = None;
        self.invalidate_pending_golden_shrine_revivals();
        self.clear_snow();
        self.clear_world_freezer();
        self.lifecycle = MatchLifecycle::Finished {
            outcome,
            finished_tick: completed_tick,
        };
    }

    pub(super) fn evaluate_match_time_limit(&mut self, completed_tick: u64) {
        if self.lifecycle != MatchLifecycle::Running {
            return;
        }
        if self
            .config
            .match_rules
            .duration_ticks()
            .is_some_and(|limit| completed_tick + 1 - self.round_start_tick >= limit)
        {
            let outcome = self.decide_time_limit_outcome();
            self.finish_round(outcome, completed_tick);
        }
    }
}
