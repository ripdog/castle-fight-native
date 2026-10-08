use super::*;
use crate::{PoisonProfile, PoisonStatus, components::PoisonState};

pub(in crate::simulation) fn eligible(target: &UnitSnapshot, profile: PoisonProfile) -> bool {
    !target.mechanical
        && !target.classifications.spell_immune
        && !target.classifications.invulnerable
        && profile
            .targets
            .can_target_unit(target.target_movement_class())
}

impl Simulation {
    pub(in crate::simulation) fn apply_poison(
        &mut self,
        source: SimId,
        target: &mut UnitSnapshot,
        profile: PoisonProfile,
        tick: u64,
    ) {
        let duration = if target.classifications.hero {
            profile.hero_duration_ticks
        } else {
            profile.duration_ticks
        };
        if duration == 0 {
            return;
        }
        let expires_tick = tick + u64::from(duration);
        if let Some(status) = target.status.poison {
            assert_eq!(
                status.profile, profile,
                "one native poison family per recipient"
            );
        }
        let existing = self.world.iter_entities().find_map(|entity| {
            let NativeAction::Poison(state) = entity.get::<NativeAction>()? else {
                return None;
            };
            (state.target == target.id
                && (state.source == source || !profile.stack_damage)
                && target
                    .status
                    .poison
                    .is_some_and(|status| status.generation == state.generation))
            .then_some(entity.id())
        });
        if let Some(entity) = existing {
            let mut entity = self.world.entity_mut(entity);
            let mut action = entity.get_mut::<NativeAction>().unwrap();
            let NativeAction::Poison(state) = &mut *action else {
                unreachable!()
            };
            state.expires_tick = expires_tick;
            state.source = source;
            // Refresh the lifetime without postponing the already scheduled pulse.
        } else {
            let id = self.allocate_id();
            let status = target.status.poison.get_or_insert(PoisonStatus {
                profile,
                generation: id,
                sources: 0,
                expires_tick,
            });
            status.sources = status
                .sources
                .checked_add(1)
                .expect("poison source count overflow");
            self.world.spawn((
                id,
                NativeAction::Poison(PoisonState {
                    source,
                    target: target.id,
                    generation: status.generation,
                    profile,
                    next_pulse_tick: tick + CASTLE_FIGHT_SIMULATION_HZ as u64,
                    expires_tick,
                }),
            ));
        }
        let status = target.status.poison.as_mut().unwrap();
        status.expires_tick = status.expires_tick.max(expires_tick);
        let id = ModifierId(profile.ability.0);
        apply_timed_movement_modifier(
            &mut target.status,
            id,
            profile.movement_percent_delta,
            expires_tick,
        );
        apply_timed_attack_speed_modifier(
            &mut target.status,
            id,
            profile.attack_speed_percent_delta,
            expires_tick,
        );
    }

    pub(in crate::simulation) fn advance_poison(
        &mut self,
        entity: Entity,
        mut state: PoisonState,
        units: &mut [UnitSnapshot],
    ) {
        let Some(target) = units.iter_mut().find(|unit| {
            unit.id == state.target
                && unit.health > 0
                && unit
                    .status
                    .poison
                    .is_some_and(|status| status.generation == state.generation)
        }) else {
            self.world.despawn(entity);
            return;
        };
        while self.next_tick >= state.next_pulse_tick && state.next_pulse_tick <= state.expires_tick
        {
            if eligible(target, state.profile) {
                let damage = self
                    .combat_rules
                    .damage_rules
                    .apply_spell(state.profile.damage_per_second, target.armor.armor_type);
                let damage = spell_damage_after_defend(*target, damage, self.next_tick);
                super::super::damage_thresholds::observe_damage(target, target.health);
                let floor = i32::from(!state.profile.can_kill);
                target.health = target.health.saturating_sub(damage).max(floor);
            }
            state.next_pulse_tick = state
                .next_pulse_tick
                .checked_add(CASTLE_FIGHT_SIMULATION_HZ as u64)
                .expect("poison deadline overflow");
            if target.health <= 0 {
                break;
            }
        }
        if self.next_tick >= state.expires_tick || target.health <= 0 {
            self.world.despawn(entity);
            let status = target.status.poison.as_mut().unwrap();
            status.sources -= 1;
            if status.sources == 0 {
                target.status.poison = None;
            }
        } else {
            *self
                .world
                .entity_mut(entity)
                .get_mut::<NativeAction>()
                .unwrap() = NativeAction::Poison(state);
        }
    }
}
