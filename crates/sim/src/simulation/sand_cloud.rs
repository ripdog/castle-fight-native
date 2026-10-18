//! Script-owned Sandcloud: a fixed carrier re-casting a miss-chance area at a fixed period.
use super::*;
use crate::components::{NativeAction, SandCloudProfile, SandCloudState};

impl Simulation {
    pub(super) fn launch_sand_cloud(
        &mut self,
        source: AbilitySourceSnapshot,
        owner: PlayerId,
        caster_rawcode: u32,
        center: SimPoint,
        profile: SandCloudProfile,
    ) {
        let variant = if caster_rawcode == profile.special_caster_rawcode {
            profile.special
        } else {
            profile.standard
        };
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::SandCloud(SandCloudState {
                variant,
                source: source.id,
                owner,
                team: source.team,
                center,
                expires_tick: self.next_tick + u64::from(profile.carrier_ticks),
                // doPeriodically fires its first callback one period after the cast.
                next_pulse_tick: self.next_tick + u64::from(profile.period_ticks),
                period_ticks: profile.period_ticks,
                pulses_remaining: profile.pulses,
            }),
        ));
    }

    pub(super) fn advance_sand_cloud(
        &mut self,
        entity: Entity,
        mut state: SandCloudState,
        units: &mut [UnitSnapshot],
    ) {
        let tick = self.next_tick;
        if state.pulses_remaining > 0 && tick >= state.next_pulse_tick {
            // The carrier issues the cast while it lives; each cast affects the area at once.
            if state.next_pulse_tick < state.expires_tick {
                let radius_sq = square_i32(state.variant.radius);
                for unit in units.iter_mut() {
                    if state.center.distance_sq(unit.position) <= radius_sq {
                        apply_ability_effect_to_unit(
                            unit,
                            AbilityEffect::NativeDebuff(state.variant.debuff),
                            state.team,
                            tick,
                            self.combat_rules.damage_rules,
                        );
                    }
                }
            }
            state.pulses_remaining -= 1;
            state.next_pulse_tick += u64::from(state.period_ticks);
        }
        if state.pulses_remaining == 0 || tick >= state.expires_tick {
            self.world.despawn(entity);
        } else if let Some(mut action) = self.world.get_mut::<NativeAction>(entity) {
            *action = NativeAction::SandCloud(state);
        }
    }
}
