//! Native Grab Tree: the spawn-time tree grab and the club weapon's limited attack count.
use super::*;
use crate::components::{ClubPhase, WarClubState};

pub(super) fn initial_state(effects: PassiveUnitEffects, spawn_tick: u64) -> Option<WarClubState> {
    effects.iter().find_map(|effect| match effect {
        PassiveUnitEffect::WarClub(profile) => Some(WarClubState {
            profile,
            resume_tick: spawn_tick + u64::from(profile.resume_delay_ticks),
            phase: ClubPhase::Gripping,
        }),
        _ => None,
    })
}

/// Applies the club to the unit's per-tick projection. Idempotent within a tick.
pub(super) fn project(unit: &mut UnitSnapshot, tick: u64) {
    let Some(mut state) = unit.status.war_club else {
        return;
    };
    let profile = state.profile;
    if state.phase == ClubPhase::Gripping {
        if tick < state.resume_tick {
            // The grab order replaces the lane order until the script resumes it.
            unit.orders_suspended = true;
            unit.attacks_disabled = true;
            return;
        }
        state.phase = ClubPhase::Armed {
            start_sequence: unit.attack_sequence,
        };
    }
    if let ClubPhase::Armed { start_sequence } = state.phase
        && unit.attack_sequence.saturating_sub(start_sequence) >= u64::from(profile.max_attacks)
    {
        state.phase = ClubPhase::Spent;
    }
    if matches!(state.phase, ClubPhase::Armed { .. }) {
        // Enabled Attack Index 1, Disabled Attack Index 0.
        unit.attack.damage = profile.damage;
        unit.attack.range = profile.range;
        unit.attack.cooldown_ticks = profile.cooldown_ticks;
        unit.attack_targets = profile.targets;
        unit.damage_type = profile.damage_type;
        unit.secondary_attack = None;
    }
    unit.status.war_club = Some(state);
}
