use super::*;
use crate::{
    PoisonBuffStatus, PoisonProfile, PoisonStatus,
    components::{MAX_NATIVE_POISON_BUFFS, PoisonState},
};

pub(in crate::simulation) fn eligible(target: &UnitSnapshot, profile: PoisonProfile) -> bool {
    !target.mechanical
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
        let existing = self.world.iter_entities().find_map(|entity| {
            let NativeAction::Poison(state) = entity.get::<NativeAction>()? else {
                return None;
            };
            (state.target == target.id
                && state.profile.buff_rawcode == profile.buff_rawcode
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
            state.profile = profile;
            // Refresh the lifetime without postponing the already scheduled pulse.
        } else {
            let id = self.allocate_id();
            let status = target.status.poison.get_or_insert(PoisonStatus {
                profile,
                total_damage_per_second: 0,
                buffs: [None; MAX_NATIVE_POISON_BUFFS],
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
        self.refresh_poison_summary(target);
    }

    /// Each attacker/ability owns a damage clock; the family has one nonstacking slow.
    fn refresh_poison_summary(&self, target: &mut UnitSnapshot) {
        let Some(previous) = target.status.poison else {
            return;
        };
        let mut representative = None;
        let mut buffs = [None::<PoisonBuffStatus>; MAX_NATIVE_POISON_BUFFS];
        let mut count = 0_u32;
        let mut damage = 0_i32;
        let mut expiry = 0;
        let mut movement = 0_i16;
        let mut attack_speed = 0_i16;
        for entity in self.world.iter_entities() {
            let Some(NativeAction::Poison(state)) = entity.get::<NativeAction>() else {
                continue;
            };
            if state.target != target.id || state.generation != previous.generation {
                continue;
            }
            if representative
                .is_none_or(|profile: PoisonProfile| state.profile.ability < profile.ability)
            {
                representative = Some(state.profile);
            }
            let slot = buffs
                .iter()
                .position(|buff| {
                    buff.is_some_and(|buff| buff.buff_rawcode == state.profile.buff_rawcode)
                })
                .or_else(|| buffs.iter().position(Option::is_none))
                .expect("native poison buff capacity");
            let buff = buffs[slot].get_or_insert(PoisonBuffStatus {
                ability: state.profile.ability,
                buff_rawcode: state.profile.buff_rawcode,
                damage_per_second: 0,
                expires_tick: 0,
            });
            buff.ability = buff.ability.min(state.profile.ability);
            buff.damage_per_second = buff
                .damage_per_second
                .checked_add(state.profile.damage_per_second)
                .expect("poison buff damage overflow");
            buff.expires_tick = buff.expires_tick.max(state.expires_tick);
            count = count.checked_add(1).expect("poison source count overflow");
            damage = damage
                .checked_add(state.profile.damage_per_second)
                .expect("poison damage overflow");
            expiry = expiry.max(state.expires_tick);
            movement = nonstacking_speed_delta(movement, state.profile.movement_percent_delta);
            attack_speed =
                nonstacking_speed_delta(attack_speed, state.profile.attack_speed_percent_delta);
        }
        let old_id = ModifierId(previous.profile.ability.0);
        super::super::native_carriers::retain_modifiers(
            &mut target.status.movement_modifiers,
            &mut target.status.movement_modifier_count,
            |modifier| modifier.id != old_id,
        );
        super::super::native_carriers::retain_modifiers(
            &mut target.status.attack_speed_modifiers,
            &mut target.status.attack_speed_modifier_count,
            |modifier| modifier.id != old_id,
        );
        buffs.sort_unstable_by_key(|buff| buff.map_or(u32::MAX, |buff| buff.buff_rawcode));
        target.status.poison = representative.map(|profile| PoisonStatus {
            buffs,
            profile,
            total_damage_per_second: damage,
            generation: previous.generation,
            sources: count,
            expires_tick: expiry,
        });
        if let Some(profile) = representative {
            let id = ModifierId(profile.ability.0);
            apply_timed_movement_modifier(&mut target.status, id, movement, expiry);
            apply_timed_attack_speed_modifier(&mut target.status, id, attack_speed, expiry);
        }
    }

    pub(in crate::simulation) fn advance_poison(
        &mut self,
        entity: Entity,
        mut state: PoisonState,
        units: &mut [UnitSnapshot],
    ) {
        let Some(index) = units.iter().position(|unit| {
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
            let target = &units[index];
            if eligible(target, state.profile) {
                let damage = self
                    .combat_rules
                    .damage_rules
                    .apply_spell(state.profile.damage_per_second, target.armor.armor_type);
                let damage = spell_damage_after_defend(*target, damage, self.next_tick);
                let event_damage = damage;
                super::super::source_damage_runtime::dispatch_inline(
                    state.source,
                    index,
                    event_damage,
                    units,
                    self.combat_rules.damage_rules,
                    self.next_tick,
                    &self.retired_damage_sources,
                );
                let target = &mut units[index];
                super::super::damage_thresholds::observe_damage_event(
                    target,
                    target.health,
                    event_damage,
                );
                let floor = i32::from(!state.profile.can_kill);
                if target.health > 0 {
                    target.health = target.health.saturating_sub(event_damage).max(floor);
                }
            }
            state.next_pulse_tick = state
                .next_pulse_tick
                .checked_add(CASTLE_FIGHT_SIMULATION_HZ as u64)
                .expect("poison deadline overflow");
            if units[index].health <= 0 {
                break;
            }
        }
        if self.next_tick >= state.expires_tick || units[index].health <= 0 {
            self.world.despawn(entity);
            self.refresh_poison_summary(&mut units[index]);
        } else {
            *self
                .world
                .entity_mut(entity)
                .get_mut::<NativeAction>()
                .unwrap() = NativeAction::Poison(state);
        }
    }
}

fn nonstacking_speed_delta(previous: i16, incoming: i16) -> i16 {
    if previous < 0 || incoming < 0 {
        previous.min(incoming)
    } else {
        previous.max(incoming)
    }
}
