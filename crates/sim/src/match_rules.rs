//! Version-scoped Castle Fight match modes and Rescue Strike mechanics.
//! Source: 9.27 war3map.lua mode registry (glw/gld/nrs) and
//! docs/original_map/extracted/script/runtime-system-mechanics.tsv (rescue-strike).
use crate::{CASTLE_FIGHT_SIMULATION_HZ, MapVersion};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeLimitOutcome {
    MoreStrikesThenIncome,
    Draw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchRules {
    /// glw/gld: zero disables the timer; other values are minutes.
    pub time_limit_minutes: u16,
    pub time_limit_outcome: TimeLimitOutcome,
    /// Number of round victories required to win the match.
    pub rounds_to_win: u8,
    /// nrs disables Rescue Strike on builders.
    pub rescue_strikes_enabled: bool,
    /// Source artillery mode controls alternative native weather objects.
    pub artillery_enabled: bool,
}

impl Default for MatchRules {
    fn default() -> Self {
        Self {
            time_limit_minutes: 0,
            time_limit_outcome: TimeLimitOutcome::MoreStrikesThenIncome,
            rounds_to_win: 1,
            rescue_strikes_enabled: false,
            artillery_enabled: true,
        }
    }
}

impl MatchRules {
    #[must_use]
    pub fn for_version(version: MapVersion) -> Self {
        if version == MapVersion::CASTLE_FIGHT_9_27 {
            Self {
                time_limit_minutes: 0,
                time_limit_outcome: TimeLimitOutcome::MoreStrikesThenIncome,
                rounds_to_win: 1,
                rescue_strikes_enabled: true,
                artillery_enabled: true,
            }
        } else {
            Self {
                time_limit_minutes: 0,
                time_limit_outcome: TimeLimitOutcome::MoreStrikesThenIncome,
                rounds_to_win: 1,
                rescue_strikes_enabled: false,
                artillery_enabled: true,
            }
        }
    }

    #[must_use]
    pub fn valid(self) -> bool {
        (1..=9).contains(&self.rounds_to_win)
            && (self.time_limit_minutes == 0 || (1..=999).contains(&self.time_limit_minutes))
    }

    #[must_use]
    pub fn duration_ticks(self) -> Option<u64> {
        (self.time_limit_minutes > 0)
            .then_some(u64::from(self.time_limit_minutes) * 60 * CASTLE_FIGHT_SIMULATION_HZ as u64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RescueStrikeRules {
    pub radius_subunits: i32,
    pub cast_delay_ticks: u64,
    pub finish_delay_ticks: u64,
    pub team_cooldown_ticks: u64,
    pub zero_kill_cooldown_ticks: u64,
    pub damage_each_packet: i32,
}

impl RescueStrikeRules {
    #[must_use]
    pub fn for_version(version: MapVersion) -> Option<Self> {
        if version == MapVersion::CASTLE_FIGHT_9_27 {
            Some(Self {
                radius_subunits: 700 * crate::SUBUNITS_PER_WORLD_UNIT,
                cast_delay_ticks: (CASTLE_FIGHT_SIMULATION_HZ as u64 * 35).div_ceil(100),
                finish_delay_ticks: (CASTLE_FIGHT_SIMULATION_HZ as u64 * 150).div_ceil(100),
                team_cooldown_ticks: CASTLE_FIGHT_SIMULATION_HZ as u64 * 2,
                zero_kill_cooldown_ticks: CASTLE_FIGHT_SIMULATION_HZ as u64 * 180,
                damage_each_packet: 4444,
            })
        } else {
            None
        }
    }
}
