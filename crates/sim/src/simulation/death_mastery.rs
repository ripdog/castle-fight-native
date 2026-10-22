//! The Lich King's single-roll damage listener: devour an eligible target outright or leave a
//! Death and Decay area at the damaged target's position.
use super::*;
use crate::DeathMasteryProfile;
use crate::components::{DeathAndDecayState, NativeAction};

const RANDOM_PURPOSE_DECAY_PHASE: u64 = 0x4445_4341_5950_0001;

pub(super) fn new_decay(
    source: PendingAttackEffectSource,
    position: SimPoint,
    profile: DeathMasteryProfile,
    completed_tick: u64,
) -> DeathAndDecayState {
    DeathAndDecayState {
        source: source.id,
        team: source.team,
        position,
        profile: profile.decay,
        cast_tick: completed_tick,
        expires_tick: completed_tick + u64::from(profile.decay.duration_ticks),
    }
}

/// `devourAttackProc`'s first branch: the bearer regains a share of the target's remaining life
/// before the target takes the retained killing damage (which ignores armor).
pub(super) fn devour(
    profile: DeathMasteryProfile,
    devours: u8,
    source: PendingAttackEffectSource,
    target: usize,
    state: &mut PendingAttackEffectState<'_>,
) {
    // The listener returns early for a target that died to the triggering hit.
    if state.unit_health[target] <= 0 {
        return;
    }
    // Every nested cast heals from the same untouched life: the damage lands only after the chain.
    let restored = i64::from(state.unit_health[target])
        * i64::from(profile.devour_heal_per_10k)
        * i64::from(devours)
        / i64::from(ATTACK_PROC_CHANCE_SCALE);
    if let Some(bearer) = find_unit_index(state.units, source.id)
        && state.unit_health[bearer] > 0
    {
        state.unit_health[bearer] = state.unit_health[bearer]
            .saturating_add(i32::try_from(restored).expect("devour heal fits health"))
            .min(state.units[bearer].health_max);
    }
    if state.units[target].classifications.invulnerable {
        return;
    }
    damage_thresholds::observe_damage_event(
        &mut state.units[target],
        state.unit_health[target],
        profile.devour_damage,
    );
    let after = state.unit_health[target].saturating_sub(profile.devour_damage);
    death_healing::commit_split_health_from(
        state.units,
        state.unit_health,
        None,
        target,
        after,
        Some(source.id),
    );
}

impl Simulation {
    pub(super) fn launch_death_and_decay(&mut self, state: DeathAndDecayState) {
        let id = self.allocate_id();
        self.world.spawn((id, NativeAction::DeathAndDecay(state)));
    }

    /// The tick offset, within the first second after the release, of a body's first pulse.
    ///
    /// The engine attaches the area buff to each body inside the area and arms its pulse timer
    /// with a random delay of up to one second, then re-arms it every second; the phase is a
    /// deterministic draw per body, so bodies inside the area from the start pulse at their own
    /// offset once per second and a body that stays loses its share five times in five seconds.
    fn decay_phase(&self, state: &DeathAndDecayState, target: SimId) -> u64 {
        let second = CASTLE_FIGHT_SIMULATION_HZ as u64;
        1 + deterministic_random(
            self.config.match_seed,
            state.cast_tick,
            state.source,
            RANDOM_PURPOSE_DECAY_PHASE ^ u64::from(state.profile.ability.0),
            target.0,
        ) % (second - 1)
    }

    /// Every hostile body inside the area loses its share of maximum life once per second, each
    /// on its own phase; spell-immune and invulnerable bodies are never buffed.
    pub(super) fn advance_death_and_decay(
        &mut self,
        entity: Entity,
        state: DeathAndDecayState,
        units: &mut [UnitSnapshot],
    ) {
        let limit = square_i32(state.profile.radius);
        let second = CASTLE_FIGHT_SIMULATION_HZ as u64;
        let elapsed = self.next_tick.saturating_sub(state.cast_tick);
        if elapsed > 0 && self.next_tick < state.expires_tick {
            for i in 0..units.len() {
                let target = units[i];
                if target.health <= 0
                    || target.team == state.team
                    || target.classifications.locust
                    || target.classifications.invulnerable
                    || target.classifications.spell_immune
                    || !state
                        .profile
                        .targets
                        .can_target_unit(target.target_movement_class())
                    || state.position.distance_sq(target.position) > limit
                {
                    continue;
                }
                let phase = self.decay_phase(&state, target.id);
                if elapsed < phase || !(elapsed - phase).is_multiple_of(second) {
                    continue;
                }
                let amount = i64::from(target.health_max) * i64::from(state.profile.life_per_10k)
                    / i64::from(ATTACK_PROC_CHANCE_SCALE);
                apply_ability_effect_to_unit(
                    &mut units[i],
                    AbilityEffect::Damage {
                        amount: i32::try_from(amount).expect("Death and Decay damage fits health"),
                    },
                    state.team,
                    self.next_tick,
                    self.combat_rules.damage_rules,
                );
                death_healing::observe_inline_death_from(
                    units,
                    i,
                    target.health,
                    Some(state.source),
                );
            }
        }
        if self.next_tick >= state.expires_tick {
            self.world.despawn(entity);
        }
    }
}
