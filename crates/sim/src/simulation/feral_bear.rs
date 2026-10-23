//! The Bears' target-damage listener (`handleTargetDamageEffects`): Feral Rage and the one-time
//! hibernation. Damage events are recorded where they happen and resolved once per tick.
use super::*;
use crate::components::{BearPhase, FeralBearState};

const RANDOM_PURPOSE_FERAL_RAGE: u64 = 0x4645_5241_4c52_4147;

pub(super) fn initial_state(effects: PassiveUnitEffects) -> Option<FeralBearState> {
    effects.iter().find_map(|effect| match effect {
        PassiveUnitEffect::FeralBear(profile) => Some(FeralBearState::new(profile)),
        _ => None,
    })
}

/// A damage event on the bear; `life` is its life when the event fires. The listener ignores a body
/// that is already dead.
pub(super) fn observe_target_damage(unit: &mut UnitSnapshot, life: i32) {
    let Some(state) = unit.status.feral_bear.as_mut() else {
        return;
    };
    if life <= 0 {
        return;
    }
    state.pending_events = state.pending_events.saturating_add(1);
    state.pending_lowest_health = state.pending_lowest_health.min(life);
}

/// A sleeping bear cannot act.
pub(super) fn project_control(unit: &mut UnitSnapshot) {
    if let Some(state) = unit.status.feral_bear
        && matches!(state.phase, BearPhase::Asleep { .. })
    {
        unit.orders_suspended = true;
        unit.attacks_disabled = true;
    }
}

/// `SetUnitMoveSpeed(unit, 400.)` while the bear walks home replaces its base speed.
pub(super) fn retreat_speed(unit: &UnitSnapshot) -> Option<i32> {
    match unit.status.feral_bear {
        Some(FeralBearState {
            phase: BearPhase::Retreating { .. },
            profile,
            ..
        }) => Some(profile.retreat_speed_per_tick),
        _ => None,
    }
}

/// The map removes the wake attack bonus in the bearer's first source-damage callback.
pub(super) fn consume_wake_bonus(unit: &mut UnitSnapshot) {
    if let Some(state) = unit.status.feral_bear.as_mut()
        && state.bonus_pending
    {
        state.bonus_pending = false;
        if let Some(bonus) = state.profile.wake_bonus {
            unit.status.native_grants.remove(&[bonus.ability]);
        }
    }
}

fn end_sleep(unit: &mut UnitSnapshot, state: &mut FeralBearState) {
    let regeneration = ModifierId(state.profile.sleep_regeneration_ability.0);
    super::native_carriers::retain_modifiers(
        &mut unit.status.armor_modifiers,
        &mut unit.status.armor_modifier_count,
        |modifier| modifier.id != regeneration,
    );
    state.phase = BearPhase::Spent;
}

impl Simulation {
    pub(super) fn resolve_feral_bears(&mut self, units: &mut [UnitSnapshot]) -> usize {
        let tick = self.next_tick;
        let mut effects = 0;
        for unit in units.iter_mut() {
            let Some(mut state) = unit.status.feral_bear else {
                continue;
            };
            if unit.health <= 0 {
                continue;
            }
            let profile = state.profile;
            // The two `doAfter` callbacks of the hibernation.
            match state.phase {
                BearPhase::Retreating { until_tick } if tick >= until_tick => {
                    // `startBearSleep`: all buffs go, the body is put to sleep and regenerates.
                    super::native_summons::dispel_magic_buffs(&mut unit.status);
                    unit.status.pending_attack = None;
                    unit.status.pending_cast = None;
                    unit.status.action_animation = None;
                    let until_tick = tick + u64::from(profile.sleep_ticks);
                    super::status::apply_timed_armor_modifier(
                        &mut unit.status,
                        TimedArmorModifier {
                            id: ModifierId(profile.sleep_regeneration_ability.0),
                            regeneration_per_second_per_10k: profile
                                .sleep_regeneration_per_second_per_10k,
                            undispellable: true,
                            expires_tick: until_tick,
                            ..TimedArmorModifier::default()
                        },
                    );
                    if let Some(bonus) = profile.wake_bonus {
                        unit.status.native_grants.insert(bonus);
                        state.bonus_pending = true;
                    }
                    state.phase = BearPhase::Asleep { until_tick };
                    effects += 1;
                }
                BearPhase::Asleep { until_tick } if tick >= until_tick => {
                    end_sleep(unit, &mut state);
                    effects += 1;
                }
                _ => {}
            }
            let events = std::mem::take(&mut state.pending_events);
            let lowest = std::mem::replace(&mut state.pending_lowest_health, i32::MAX);
            for _ in 0..events {
                let sequence = state.event_sequence;
                state.event_sequence = sequence
                    .checked_add(1)
                    .expect("feral rage event sequence overflow");
                let roll = deterministic_random(
                    self.config.match_seed,
                    tick,
                    unit.id,
                    RANDOM_PURPOSE_FERAL_RAGE ^ u64::from(profile.ability.0),
                    sequence,
                ) % 10_000;
                if roll >= u64::from(profile.rage_chance_per_10k) {
                    continue;
                }
                let expires_tick = tick + u64::from(profile.rage_duration_ticks);
                // Two free dummies cast Inner Fire and Bloodlust on the bear.
                super::status::apply_timed_armor_modifier(
                    &mut unit.status,
                    TimedArmorModifier {
                        id: ModifierId(profile.rage_damage_ability.0),
                        native_buff: Some(profile.rage_damage_buff),
                        damage_bonus_per_10k: profile.rage_damage_bonus_per_10k,
                        expires_tick,
                        ..TimedArmorModifier::default()
                    },
                );
                super::status::apply_timed_attack_speed_modifier(
                    &mut unit.status,
                    ModifierId(profile.rage_speed_ability.0),
                    profile.rage_attack_speed_percent_delta,
                    expires_tick,
                );
                effects += 1;
            }
            if events > 0 {
                match state.phase {
                    BearPhase::Ready if lowest < profile.hibernate_below_health => {
                        // `startBearHibernate`: the marker is spent and the bear walks home.
                        let until_tick = tick + u64::from(profile.retreat_ticks);
                        state.phase = BearPhase::Retreating { until_tick };
                        unit.status.ability_retreat_start_tick = tick;
                        unit.status.ability_retreat_end_tick = until_tick;
                        effects += 1;
                    }
                    BearPhase::Asleep { .. } => {
                        // Any later damage event removes the sleep and the regeneration.
                        end_sleep(unit, &mut state);
                        effects += 1;
                    }
                    _ => {}
                }
            }
            unit.status.feral_bear = Some(state);
        }
        effects
    }
}
