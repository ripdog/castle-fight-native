use super::*;

pub(super) fn apply_ability_effect_to_unit(
    target: &mut UnitSnapshot,
    effect: AbilityEffect,
    completed_tick: u64,
    damage_rules: DamageRules,
) -> bool {
    if target.health <= 0 {
        return false;
    }
    match effect {
        AbilityEffect::Damage { amount } => {
            let adjusted = damage_rules.apply_spell(amount, target.armor.armor_type);
            let adjusted = spell_damage_after_defend(*target, adjusted, completed_tick);
            target.health = target
                .health
                .checked_sub(adjusted)
                .expect("ability damage overflowed validated bounds");
        }
        AbilityEffect::Stun { duration_ticks } => {
            let stunned_until_tick = completed_tick
                .checked_add(u64::from(duration_ticks))
                .expect("stun expiry tick overflow");
            target.status.stunned_until_tick =
                target.status.stunned_until_tick.max(stunned_until_tick);
        }
        AbilityEffect::ModifyMovementSpeedPercent {
            modifier,
            percent_delta,
            duration_ticks,
        } => {
            let expires_tick = completed_tick
                .checked_add(u64::from(duration_ticks))
                .expect("movement modifier expiry tick overflow");
            apply_timed_movement_modifier(
                &mut target.status,
                modifier,
                percent_delta,
                expires_tick,
            );
        }
        AbilityEffect::AreaDamage { amount, radius: _ } => {
            let adjusted = damage_rules.apply_spell(amount, target.armor.armor_type);
            let adjusted = spell_damage_after_defend(*target, adjusted, completed_tick);
            target.health = target
                .health
                .checked_sub(adjusted)
                .expect("area ability damage overflowed validated bounds");
        }
        AbilityEffect::FrostArmor {
            modifier,
            armor_bonus_per_100,
            armor_duration_ticks,
            slow_duration_ticks,
            movement_percent_delta,
            attack_speed_percent_delta,
        } => {
            let expires_tick = completed_tick
                .checked_add(u64::from(armor_duration_ticks))
                .expect("Frost Armor expiry tick overflow");
            apply_timed_armor_modifier(
                &mut target.status,
                TimedArmorModifier {
                    id: modifier,
                    armor_bonus_per_100,
                    regeneration_per_second_per_10k: 0,
                    mana_regeneration_per_second_per_10k: 0,
                    damage_bonus_per_10k: 0,
                    expires_tick,
                    reactive_slow_duration_ticks: slow_duration_ticks,
                    reactive_movement_percent_delta: movement_percent_delta,
                    reactive_attack_speed_percent_delta: attack_speed_percent_delta,
                },
            );
        }
        AbilityEffect::HolyAid {
            modifier,
            healing,
            armor_bonus_per_100,
            regeneration_per_second_per_10k,
            duration_ticks,
            permanent_max_health_bonus,
            resurrection_count: _,
            resurrection_radius: _,
        } => {
            if permanent_max_health_bonus > 0 && !target.status.permanent_holy_health_bonus {
                target.status.permanent_holy_health_bonus = true;
                target.health_max = target.health_max.saturating_add(permanent_max_health_bonus);
            }
            target.health = target.health.saturating_add(healing).min(target.health_max);
            apply_timed_armor_modifier(
                &mut target.status,
                TimedArmorModifier {
                    id: modifier,
                    armor_bonus_per_100,
                    regeneration_per_second_per_10k,
                    mana_regeneration_per_second_per_10k: 0,
                    damage_bonus_per_10k: 0,
                    expires_tick: completed_tick + u64::from(duration_ticks),
                    reactive_slow_duration_ticks: 0,
                    reactive_movement_percent_delta: 0,
                    reactive_attack_speed_percent_delta: 0,
                },
            );
        }
        AbilityEffect::Prayer {
            modifier,
            healing,
            mana_restored,
            armor_bonus_per_100,
            damage_bonus_per_10k,
            duration_ticks,
            radius: _,
            resurrection_count: _,
            resurrection_radius: _,
        } => {
            target.health = target.health.saturating_add(healing).min(target.health_max);
            if let (Some(profile), Some(mana)) = (target.spellcasting, target.mana_current.as_mut())
            {
                *mana = mana.saturating_add(mana_restored).min(profile.mana.maximum);
            }
            apply_timed_armor_modifier(
                &mut target.status,
                TimedArmorModifier {
                    id: modifier,
                    armor_bonus_per_100,
                    regeneration_per_second_per_10k: 0,
                    mana_regeneration_per_second_per_10k: 0,
                    damage_bonus_per_10k,
                    expires_tick: completed_tick + u64::from(duration_ticks),
                    reactive_slow_duration_ticks: 0,
                    reactive_movement_percent_delta: 0,
                    reactive_attack_speed_percent_delta: 0,
                },
            );
        }
    }
    true
}

pub(super) fn purge_expired_status_modifiers(status: &mut StatusState, tick: u64) {
    purge_expired_movement_modifiers(status, tick);

    let attack_speed_count = usize::from(status.attack_speed_modifier_count);
    debug_assert!(attack_speed_count <= MAX_TIMED_ATTACK_SPEED_MODIFIERS);
    let mut attack_speed_write = 0usize;
    for read_index in 0..attack_speed_count {
        let modifier = status.attack_speed_modifiers[read_index];
        if tick < modifier.expires_tick {
            status.attack_speed_modifiers[attack_speed_write] = modifier;
            attack_speed_write += 1;
        }
    }
    for slot in &mut status.attack_speed_modifiers[attack_speed_write..attack_speed_count] {
        *slot = Default::default();
    }
    status.attack_speed_modifier_count =
        u8::try_from(attack_speed_write).expect("attack-speed modifier count exceeds u8");

    let armor_count = usize::from(status.armor_modifier_count);
    debug_assert!(armor_count <= MAX_TIMED_ARMOR_MODIFIERS);
    let mut armor_write = 0usize;
    for read_index in 0..armor_count {
        let modifier = status.armor_modifiers[read_index];
        if tick < modifier.expires_tick {
            status.armor_modifiers[armor_write] = modifier;
            armor_write += 1;
        }
    }
    for slot in &mut status.armor_modifiers[armor_write..armor_count] {
        *slot = Default::default();
    }
    status.armor_modifier_count =
        u8::try_from(armor_write).expect("armor modifier count exceeds u8");

    let dot_count = usize::from(status.damage_over_time_count);
    debug_assert!(dot_count <= MAX_TIMED_DAMAGE_OVER_TIME);
    let mut dot_write = 0usize;
    for read_index in 0..dot_count {
        let effect = status.damage_over_time[read_index];
        if tick < effect.expires_tick {
            status.damage_over_time[dot_write] = effect;
            dot_write += 1;
        }
    }
    for slot in &mut status.damage_over_time[dot_write..dot_count] {
        *slot = Default::default();
    }
    status.damage_over_time_count =
        u8::try_from(dot_write).expect("damage-over-time count exceeds u8");
}

fn purge_expired_movement_modifiers(status: &mut StatusState, tick: u64) {
    let count = usize::from(status.movement_modifier_count);
    debug_assert!(count <= MAX_TIMED_MOVEMENT_MODIFIERS);
    let mut write_index = 0usize;
    for read_index in 0..count {
        let modifier = status.movement_modifiers[read_index];
        if tick < modifier.expires_tick {
            status.movement_modifiers[write_index] = modifier;
            write_index += 1;
        }
    }
    for slot in &mut status.movement_modifiers[write_index..count] {
        *slot = Default::default();
    }
    status.movement_modifier_count =
        u8::try_from(write_index).expect("movement modifier count exceeds u8");
}

pub(super) fn apply_timed_movement_modifier(
    status: &mut StatusState,
    modifier_id: ModifierId,
    percent_delta: i16,
    expires_tick: u64,
) {
    let count = usize::from(status.movement_modifier_count);
    debug_assert!(count <= MAX_TIMED_MOVEMENT_MODIFIERS);
    let active = &status.movement_modifiers[..count];
    match active.binary_search_by_key(&modifier_id, |modifier| modifier.id) {
        Ok(index) => {
            let modifier = &mut status.movement_modifiers[index];
            assert_eq!(
                modifier.percent_delta, percent_delta,
                "same ModifierId authored with conflicting movement percentages"
            );
            modifier.expires_tick = modifier.expires_tick.max(expires_tick);
        }
        Err(index) => {
            assert!(
                count < MAX_TIMED_MOVEMENT_MODIFIERS,
                "timed movement modifier capacity exceeded"
            );
            status
                .movement_modifiers
                .copy_within(index..count, index + 1);
            status.movement_modifiers[index] = crate::components::TimedMovementModifier {
                id: modifier_id,
                percent_delta,
                expires_tick,
            };
            status.movement_modifier_count = status
                .movement_modifier_count
                .checked_add(1)
                .expect("movement modifier count overflow");
        }
    }
}

fn apply_timed_attack_speed_modifier(
    status: &mut StatusState,
    modifier_id: ModifierId,
    percent_delta: i16,
    expires_tick: u64,
) {
    let count = usize::from(status.attack_speed_modifier_count);
    debug_assert!(count <= MAX_TIMED_ATTACK_SPEED_MODIFIERS);
    let active = &status.attack_speed_modifiers[..count];
    match active.binary_search_by_key(&modifier_id, |modifier| modifier.id) {
        Ok(index) => {
            let modifier = &mut status.attack_speed_modifiers[index];
            assert_eq!(
                modifier.percent_delta, percent_delta,
                "same ModifierId authored with conflicting attack-speed percentages"
            );
            modifier.expires_tick = modifier.expires_tick.max(expires_tick);
        }
        Err(index) => {
            assert!(
                count < MAX_TIMED_ATTACK_SPEED_MODIFIERS,
                "timed attack-speed modifier capacity exceeded"
            );
            status
                .attack_speed_modifiers
                .copy_within(index..count, index + 1);
            status.attack_speed_modifiers[index] = TimedAttackSpeedModifier {
                id: modifier_id,
                percent_delta,
                expires_tick,
            };
            status.attack_speed_modifier_count = status
                .attack_speed_modifier_count
                .checked_add(1)
                .expect("attack-speed modifier count overflow");
        }
    }
}

pub(super) fn apply_timed_armor_modifier(status: &mut StatusState, incoming: TimedArmorModifier) {
    let count = usize::from(status.armor_modifier_count);
    debug_assert!(count <= MAX_TIMED_ARMOR_MODIFIERS);
    let active = &status.armor_modifiers[..count];
    match active.binary_search_by_key(&incoming.id, |modifier| modifier.id) {
        Ok(index) => {
            let modifier = &mut status.armor_modifiers[index];
            *modifier = incoming;
        }
        Err(index) => {
            assert!(
                count < MAX_TIMED_ARMOR_MODIFIERS,
                "timed armor modifier capacity exceeded"
            );
            status.armor_modifiers.copy_within(index..count, index + 1);
            status.armor_modifiers[index] = incoming;
            status.armor_modifier_count = status
                .armor_modifier_count
                .checked_add(1)
                .expect("armor modifier count overflow");
        }
    }
}

pub(super) fn apply_timed_damage_over_time(
    status: &mut StatusState,
    modifier_id: ModifierId,
    damage_per_pulse: i32,
    pulse_interval_ticks: u16,
    applied_tick: u64,
    expires_tick: u64,
) {
    assert!(damage_per_pulse >= 0);
    assert!(pulse_interval_ticks > 0);
    let count = usize::from(status.damage_over_time_count);
    debug_assert!(count <= MAX_TIMED_DAMAGE_OVER_TIME);
    let active = &status.damage_over_time[..count];
    let next_pulse_tick = applied_tick
        .checked_add(u64::from(pulse_interval_ticks))
        .expect("damage-over-time pulse tick overflow");
    match active.binary_search_by_key(&modifier_id, |effect| effect.id) {
        Ok(index) => {
            let effect = &mut status.damage_over_time[index];
            assert_eq!(
                (effect.damage_per_pulse, effect.pulse_interval_ticks),
                (damage_per_pulse, pulse_interval_ticks),
                "same ModifierId authored with conflicting damage-over-time parameters"
            );
            effect.expires_tick = effect.expires_tick.max(expires_tick);
            effect.next_pulse_tick = next_pulse_tick;
        }
        Err(index) => {
            assert!(
                count < MAX_TIMED_DAMAGE_OVER_TIME,
                "damage-over-time capacity exceeded"
            );
            status.damage_over_time.copy_within(index..count, index + 1);
            status.damage_over_time[index] = TimedDamageOverTime {
                id: modifier_id,
                damage_per_pulse,
                pulse_interval_ticks,
                next_pulse_tick,
                expires_tick,
            };
            status.damage_over_time_count = status
                .damage_over_time_count
                .checked_add(1)
                .expect("damage-over-time count overflow");
        }
    }
}

pub(super) fn resolve_periodic_unit_statuses(
    units: &mut [UnitSnapshot],
    completed_tick: u64,
    damage_rules: DamageRules,
) {
    for unit in units {
        if unit.health <= 0 {
            continue;
        }
        let count = usize::from(unit.status.damage_over_time_count);
        debug_assert!(count <= MAX_TIMED_DAMAGE_OVER_TIME);
        let spell_damage_taken_per_10k =
            active_defend_profile(unit.passive_effects, unit.spawn_tick, completed_tick)
                .map(|profile| profile.spell_damage_taken_per_10k);
        for index in 0..count {
            let effect = &mut unit.status.damage_over_time[index];
            while completed_tick >= effect.next_pulse_tick
                && effect.next_pulse_tick < effect.expires_tick
                && unit.health > 0
            {
                let adjusted =
                    damage_rules.apply_spell(effect.damage_per_pulse, unit.armor.armor_type);
                let adjusted = spell_damage_taken_per_10k
                    .map_or(adjusted, |factor| scale_damage_per_10k(adjusted, factor));
                unit.health = unit
                    .health
                    .checked_sub(adjusted)
                    .expect("damage-over-time health arithmetic overflow");
                effect.next_pulse_tick = effect
                    .next_pulse_tick
                    .checked_add(u64::from(effect.pulse_interval_ticks))
                    .expect("damage-over-time pulse tick overflow");
            }
        }
    }
}

pub(super) fn effective_armor_points_per_100(unit: &UnitSnapshot) -> i32 {
    unit.status.effective_armor_points_per_100(unit.armor)
}

pub(super) fn apply_melee_reactive_armor_effects(
    source_index: usize,
    target_index: usize,
    completed_tick: u64,
    units: &mut [UnitSnapshot],
    unit_health: &[i32],
) {
    if unit_health[source_index] <= 0 {
        return;
    }
    let target_status = units[target_index].status;
    let count = usize::from(target_status.armor_modifier_count);
    debug_assert!(count <= MAX_TIMED_ARMOR_MODIFIERS);
    for armor in target_status.armor_modifiers[..count].iter().copied() {
        if armor.reactive_slow_duration_ticks == 0 || completed_tick >= armor.expires_tick {
            continue;
        }
        let expires_tick = completed_tick
            .checked_add(u64::from(armor.reactive_slow_duration_ticks))
            .expect("reactive Frost Armor slow expiry overflow");
        if armor.reactive_movement_percent_delta != 0 {
            apply_timed_movement_modifier(
                &mut units[source_index].status,
                armor.id,
                armor.reactive_movement_percent_delta,
                expires_tick,
            );
        }
        if armor.reactive_attack_speed_percent_delta != 0 {
            apply_timed_attack_speed_modifier(
                &mut units[source_index].status,
                armor.id,
                armor.reactive_attack_speed_percent_delta,
                expires_tick,
            );
        }
    }
}

pub(super) fn effective_attack_cooldown_ticks(base_ticks: u16, status: StatusState) -> u16 {
    let count = usize::from(status.attack_speed_modifier_count);
    debug_assert!(count <= MAX_TIMED_ATTACK_SPEED_MODIFIERS);
    let percent = status.attack_speed_modifiers[..count]
        .iter()
        .fold(100_i32, |total, modifier| {
            total
                .checked_add(i32::from(modifier.percent_delta))
                .expect("attack-speed percentage overflow")
        })
        .clamp(1, 1_000);
    let scaled = u64::from(base_ticks) * 100;
    u16::try_from(scaled.div_ceil(u64::try_from(percent).expect("positive attack speed")))
        .expect("effective attack cooldown exceeds u16")
        .max(1)
}
