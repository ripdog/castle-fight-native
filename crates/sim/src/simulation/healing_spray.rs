//! Script-released Healing Spray: a recycled dummy channels waves of healing at the healed
//! unit's position, independently of the casting body.
use super::*;
use crate::components::{HealingSprayProfile, HealingSprayState, NativeAction};

impl Simulation {
    pub(super) fn launch_healing_spray(
        &mut self,
        source: SimId,
        team: Team,
        center: SimPoint,
        profile: HealingSprayProfile,
    ) {
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::HealingSpray(HealingSprayState {
                profile,
                source,
                team,
                center,
                started_tick: self.next_tick,
                next_wave: 0,
            }),
        ));
    }

    pub(super) fn advance_healing_spray(
        &mut self,
        entity: Entity,
        mut state: HealingSprayState,
        units: &mut [UnitSnapshot],
    ) {
        let profile = state.profile;
        let wave_tick = |wave: u16| {
            state.started_tick
                + u64::from(profile.first_wave_ticks)
                + u64::from(wave) * u64::from(profile.interval_ticks)
        };
        let expires_tick = state.started_tick + u64::from(profile.lifetime_ticks);
        if state.next_wave >= profile.waves
            || wave_tick(state.next_wave) >= expires_tick
            || self.next_tick >= expires_tick
        {
            self.world.despawn(entity);
            return;
        }
        if self.next_tick >= wave_tick(state.next_wave) {
            let radius_sq = square_i32(profile.radius);
            let mut recipients: Vec<usize> = units
                .iter()
                .enumerate()
                .filter(|(_, unit)| {
                    unit.health > 0
                        && unit.team == state.team
                        && !unit.classifications.locust
                        && !unit.classifications.invulnerable
                        && profile.targets.can_target_unit(unit.target_movement_class())
                        && state.center.distance_sq(unit.position) <= radius_sq
                })
                .map(|(index, _)| index)
                .collect();
            recipients.sort_unstable_by_key(|&index| units[index].id);
            // The native per-wave cap is shared proportionally when it would be exceeded.
            let total = i64::from(profile.amount) * recipients.len() as i64;
            let maximum = i64::from(profile.maximum_total);
            for index in recipients {
                let share = if total > maximum {
                    i64::from(profile.amount) * maximum / total
                } else {
                    i64::from(profile.amount)
                } as i32;
                let unit = &mut units[index];
                let healing = native_healing_amount(&unit.status, share, self.next_tick);
                unit.health = unit.health.saturating_add(healing).min(unit.health_max);
            }
            state.next_wave += 1;
        }
        if state.next_wave >= profile.waves {
            self.world.despawn(entity);
        } else if let Some(mut action) = self.world.get_mut::<NativeAction>(entity) {
            *action = NativeAction::HealingSpray(state);
        }
    }
}
