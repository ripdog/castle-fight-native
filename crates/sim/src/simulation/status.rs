use super::*;

pub(super) fn timed_armor_effect_active(
    status: &StatusState,
    id: ModifierId,
    family: Option<crate::NativeBuffIdentity>,
    tick: u64,
) -> bool {
    status.armor_modifiers[..usize::from(status.armor_modifier_count)]
        .iter()
        .any(|active| {
            tick < active.expires_tick
                && (active.id == id
                    || family
                        .zip(active.native_buff)
                        .is_some_and(|(expected, current)| expected.rawcode == current.rawcode))
        })
}

pub(super) fn ability_damage_for_unit(
    target: &UnitSnapshot,
    effect: AbilityEffect,
    tick: u64,
    rules: DamageRules,
) -> Option<i32> {
    let amount = match effect {
        AbilityEffect::Damage { amount } | AbilityEffect::AreaDamage { amount, .. } => amount,
        AbilityEffect::FrostNova {
            area_damage,
            targets,
            ..
        }
        | AbilityEffect::AreaStun {
            damage: area_damage,
            targets,
            ..
        } if !target.classifications.invulnerable
            && !target.classifications.spell_immune
            && targets.can_target_unit(target.target_movement_class()) =>
        {
            area_damage
        }
        _ => return None,
    };
    Some(spell_damage_after_defend(
        *target,
        rules.apply_spell(amount, target.armor.armor_type),
        tick,
    ))
}

pub(super) fn apply_ability_effect_to_unit(
    target: &mut UnitSnapshot,
    effect: AbilityEffect,
    source_team: Team,
    completed_tick: u64,
    damage_rules: DamageRules,
) -> bool {
    if target.health <= 0 {
        return false;
    }
    match effect {
        AbilityEffect::NagaTidalWave(_)
        | AbilityEffect::NagaCharm(_)
        | AbilityEffect::NagaEarthquake(_)
        | AbilityEffect::SilverGlade(_)
        | AbilityEffect::Wilderness(_)
        | AbilityEffect::Starfall(_) => return false,
        AbilityEffect::Heal(p) => {
            if (p.organic_only && target.mechanical)
                || (p.nonancient_only && target.classifications.ancient)
                || (!p.affects_invulnerable && target.classifications.invulnerable)
                || target.team != source_team
                || !p.targets.can_target_unit(target.target_movement_class())
                || target.health >= target.health_max
            {
                return false;
            }
            target.health = target
                .health
                .saturating_add(p.amount)
                .min(target.health_max);
        }
        AbilityEffect::StatBuff {
            modifier,
            buff,
            armor_bonus_per_100,
            damage_bonus_per_10k,
            regeneration_per_second_per_10k,
            duration_ticks,
            hero_duration_ticks,
            ..
        } => {
            if target.team != source_team || target.classifications.invulnerable {
                return false;
            }
            let duration = if target.classifications.hero {
                hero_duration_ticks
            } else {
                duration_ticks
            };
            apply_timed_armor_modifier(
                &mut target.status,
                TimedArmorModifier {
                    undispellable: false,
                    aura_membership: false,
                    id: modifier,
                    native_buff: Some(buff),
                    armor_bonus_per_100,
                    damage_bonus_per_10k,
                    regeneration_per_second_per_10k,
                    expires_tick: completed_tick + u64::from(duration),
                    ..TimedArmorModifier::default()
                },
            );
        }
        AbilityEffect::Damage { .. } => {
            let event_damage =
                ability_damage_for_unit(target, effect, completed_tick, damage_rules)
                    .expect("damage effect");
            super::damage_thresholds::observe_damage_event(target, target.health, event_damage);
            target.health = target
                .health
                .checked_sub(event_damage)
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
        AbilityEffect::FrostNova {
            ability,
            area_damage: _,
            duration_ticks,
            hero_duration_ticks,
            movement_percent_delta,
            attack_speed_percent_delta,
            targets,
            ..
        } => {
            if target.classifications.invulnerable
                || target.classifications.spell_immune
                || !targets.can_target_unit(target.target_movement_class())
            {
                return false;
            }
            let event_damage =
                ability_damage_for_unit(target, effect, completed_tick, damage_rules)
                    .expect("eligible Nova");
            super::damage_thresholds::observe_damage_event(target, target.health, event_damage);
            target.health = target
                .health
                .checked_sub(event_damage)
                .expect("Frost Nova damage overflow");
            if target.health > 0 {
                apply_frost_attack(
                    target,
                    crate::FrostAttackEffectProfile {
                        ability,
                        duration_ticks,
                        hero_duration_ticks,
                        movement_percent_delta,
                        attack_speed_percent_delta,
                        targets,
                    },
                    completed_tick,
                );
            }
        }
        AbilityEffect::AreaDebuff {
            ability,
            armor_delta_per_100,
            damage_delta_per_10k,
            duration_ticks,
            hero_duration_ticks,
            targets,
            ..
        } => {
            if target.classifications.invulnerable
                || target.classifications.spell_immune
                || !targets.can_target_unit(target.target_movement_class())
            {
                return false;
            }
            let duration = if target.classifications.uses_hero_spell_rules() {
                hero_duration_ticks
            } else {
                duration_ticks
            };
            apply_timed_armor_modifier(
                &mut target.status,
                TimedArmorModifier {
                    undispellable: false,
                    aura_membership: false,
                    id: ModifierId(ability.0),
                    armor_bonus_per_100: armor_delta_per_100,
                    damage_bonus_per_10k: damage_delta_per_10k,
                    damage_taken_bonus_per_10k: 0,
                    expires_tick: completed_tick
                        .checked_add(u64::from(duration))
                        .expect("area debuff expiry overflow"),
                    ..TimedArmorModifier::default()
                },
            );
        }
        AbilityEffect::AreaStun {
            damage: _,
            stun_ticks,
            hero_stun_ticks,
            targets,
            ..
        } => {
            if target.classifications.invulnerable
                || target.classifications.spell_immune
                || !targets.can_target_unit(target.target_movement_class())
            {
                return false;
            }
            let event_damage =
                ability_damage_for_unit(target, effect, completed_tick, damage_rules)
                    .expect("eligible area stun");
            super::damage_thresholds::observe_damage_event(target, target.health, event_damage);
            target.health = target
                .health
                .checked_sub(event_damage)
                .expect("area stun damage overflow");
            let duration = if target.classifications.uses_hero_spell_rules() {
                hero_stun_ticks
            } else {
                stun_ticks
            };
            target.status.stunned_until_tick = target.status.stunned_until_tick.max(
                completed_tick
                    .checked_add(u64::from(duration))
                    .expect("area stun expiry overflow"),
            );
        }
        AbilityEffect::AreaDamage { .. } => {
            let event_damage =
                ability_damage_for_unit(target, effect, completed_tick, damage_rules)
                    .expect("damage effect");
            super::damage_thresholds::observe_damage_event(target, target.health, event_damage);
            target.health = target
                .health
                .checked_sub(event_damage)
                .expect("area ability damage overflowed validated bounds");
        }
        AbilityEffect::HealingWave(_)
        | AbilityEffect::PhoenixFire(_)
        | AbilityEffect::SolarStrike { .. } => {
            unreachable!("native delivery requires authoritative action state")
        }
        AbilityEffect::FaerieFire {
            native_buff,
            modifier,
            armor_reduction_per_100,
            duration_ticks,
            hero_duration_ticks,
        } => {
            if target.classifications.spell_immune || target.mechanical {
                return false;
            }
            let duration = if target.classifications.uses_hero_spell_rules() {
                hero_duration_ticks
            } else {
                duration_ticks
            };
            apply_timed_armor_modifier(
                &mut target.status,
                TimedArmorModifier {
                    undispellable: false,
                    aura_membership: false,
                    id: modifier,
                    native_buff,
                    armor_bonus_per_100: -armor_reduction_per_100,
                    expires_tick: completed_tick
                        .checked_add(u64::from(duration))
                        .expect("Faerie Fire expiry overflow"),
                    revealed_to: Some(source_team),
                    ..TimedArmorModifier::default()
                },
            );
        }
        AbilityEffect::FrostArmor {
            native_buff,
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
                    undispellable: false,
                    aura_membership: false,
                    id: modifier,
                    native_buff,
                    armor_bonus_per_100,
                    regeneration_per_second_per_10k: 0,
                    mana_regeneration_per_second_per_10k: 0,
                    damage_bonus_per_10k: 0,
                    damage_taken_bonus_per_10k: 0,
                    expires_tick,
                    reactive_slow_duration_ticks: slow_duration_ticks,
                    reactive_movement_percent_delta: movement_percent_delta,
                    reactive_attack_speed_percent_delta: attack_speed_percent_delta,
                    revealed_to: None,
                },
            );
        }
        AbilityEffect::HolyAid {
            native_buff,
            modifier,
            healing,
            armor_bonus_per_100,
            regeneration_per_second_per_10k,
            duration_ticks,
            permanent_max_health_bonus,
            resurrection_count: _,
            resurrection_radius: _,
            resurrection_mana_cost: _,
            resurrection_cooldown_ticks: _,
            resurrection_delay_ticks: _,
        } => {
            if permanent_max_health_bonus > 0 {
                let ability = crate::building_mechanics::holy_aid_life_ability_for_version(
                    target
                        .map_version
                        .unwrap_or(crate::CASTLE_FIGHT_DEFAULT_MAP_VERSION),
                );
                if target
                    .status
                    .native_grants
                    .insert(crate::NativeAbilityGrant {
                        ability,
                        effect: crate::NativeAbilityGrantEffect::MaximumLife(
                            permanent_max_health_bonus,
                        ),
                    })
                {
                    target.health_max =
                        target.health_max.saturating_add(permanent_max_health_bonus);
                }
            }
            target.health = target.health.saturating_add(healing).min(target.health_max);
            apply_timed_armor_modifier(
                &mut target.status,
                TimedArmorModifier {
                    undispellable: false,
                    aura_membership: false,
                    id: modifier,
                    native_buff,
                    armor_bonus_per_100,
                    regeneration_per_second_per_10k,
                    mana_regeneration_per_second_per_10k: 0,
                    damage_bonus_per_10k: 0,
                    damage_taken_bonus_per_10k: 0,
                    revealed_to: None,
                    expires_tick: completed_tick + u64::from(duration_ticks),
                    reactive_slow_duration_ticks: 0,
                    reactive_movement_percent_delta: 0,
                    reactive_attack_speed_percent_delta: 0,
                },
            );
        }
        AbilityEffect::Prayer {
            native_buff,
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
            if let Some(charge) = target.status.damage_charge.as_mut() {
                super::damage_charge::restore_mana(charge, mana_restored);
            }
            if let (Some(profile), Some(mana)) = (target.spellcasting, target.mana_current.as_mut())
            {
                *mana = mana.saturating_add(mana_restored).min(profile.mana.maximum);
            }
            apply_timed_armor_modifier(
                &mut target.status,
                TimedArmorModifier {
                    undispellable: false,
                    aura_membership: false,
                    id: modifier,
                    native_buff,
                    armor_bonus_per_100,
                    regeneration_per_second_per_10k: 0,
                    mana_regeneration_per_second_per_10k: 0,
                    damage_bonus_per_10k: i16::try_from(damage_bonus_per_10k)
                        .expect("Prayer damage modifier fits i16"),
                    damage_taken_bonus_per_10k: 0,
                    revealed_to: None,
                    expires_tick: completed_tick + u64::from(duration_ticks),
                    reactive_slow_duration_ticks: 0,
                    reactive_movement_percent_delta: 0,
                    reactive_attack_speed_percent_delta: 0,
                },
            );
        }
        AbilityEffect::NativeSummon(_)
        | AbilityEffect::SirenPurge(_)
        | AbilityEffect::WindWalk(_)
        | AbilityEffect::MasterProjectiles(_)
        | AbilityEffect::ScriptedRoots(_)
        | AbilityEffect::Ensnare { .. }
        | AbilityEffect::SpellSteal { .. }
        | AbilityEffect::WorldFreezer(_)
        | AbilityEffect::BuildingAura(_)
        | AbilityEffect::BuildingStasis(_)
        | AbilityEffect::NativeWeatherVisual { .. }
        | AbilityEffect::BuildingWeather(_)
        | AbilityEffect::BuildingEnchantment(_)
        | AbilityEffect::BuildingPermanentGrant(_)
        | AbilityEffect::BuildingAcidBomb(_)
        | AbilityEffect::BuildingBolt(_)
        | AbilityEffect::Hailstone(_)
        | AbilityEffect::Snowfall { .. }
        | AbilityEffect::Hex { .. }
        | AbilityEffect::HolyFervour { .. }
        | AbilityEffect::Purification { .. }
        | AbilityEffect::ArtilleryBombardment { .. } => return false,
    }
    true
}

pub(super) fn purge_expired_status_modifiers(status: &mut StatusState, tick: u64) {
    super::native_summons::advance_purge(status, tick);
    if status
        .acid_bomb
        .is_some_and(|acid| tick >= acid.expires_tick)
    {
        status.acid_bomb = None;
    }
    if status.ensnare.is_some_and(|net| tick >= net.expires_tick) {
        status.ensnare = None;
    }
    if status
        .action_animation
        .is_some_and(|action| tick >= action.until_tick)
    {
        status.action_animation = None;
    }
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
        if tick < effect.expires_tick
            || (effect.final_pulse_at_expiry
                && effect.next_pulse_tick <= effect.expires_tick
                && tick == effect.expires_tick)
        {
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
                undispellable: false,
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

pub(super) fn apply_timed_attack_speed_modifier(
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
            modifier.percent_delta = percent_delta;
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
                undispellable: false,
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
    if let Some(buff) = incoming.native_buff {
        let old_count = usize::from(status.armor_modifier_count);
        let mut kept = 0;
        for index in 0..old_count {
            let active = status.armor_modifiers[index];
            if active
                .native_buff
                .is_some_and(|active| active.rawcode == buff.rawcode)
            {
                continue;
            }
            status.armor_modifiers[kept] = active;
            kept += 1;
        }
        status.armor_modifiers[kept..old_count].fill(TimedArmorModifier::default());
        status.armor_modifier_count = u8::try_from(kept).expect("bounded modifier count");
    }
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
                source: None,
                id: modifier_id,
                damage_per_pulse,
                pulse_interval_ticks,
                next_pulse_tick,
                expires_tick,
                final_pulse_at_expiry: false,
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
    retired_sources: &BTreeMap<SimId, super::source_damage_runtime::RetiredDamageSource>,
) {
    for unit_index in 0..units.len() {
        let unit = &units[unit_index];
        if unit.health <= 0 {
            continue;
        }
        super::native_actions::acid_bomb::resolve_periodic_acid_bomb(
            units,
            unit_index,
            completed_tick,
            damage_rules,
            retired_sources,
        );
        let unit = &mut units[unit_index];
        let count = usize::from(unit.status.damage_over_time_count);
        debug_assert!(count <= MAX_TIMED_DAMAGE_OVER_TIME);
        let resistance = unit
            .effective_passive_effects()
            .filter_map(|effect| match effect {
                PassiveUnitEffect::SpellResistance(profile) => Some(profile.damage_taken_per_10k),
                _ => None,
            })
            .min()
            .unwrap_or(10_000);
        let spell_damage_taken_per_10k =
            active_defend_profile(unit.passive_effects, unit.spawn_tick, completed_tick)
                .map(|profile| profile.spell_damage_taken_per_10k);
        let incoming_factor =
            super::native_kill_effects::native_incoming_damage_factor(&unit.status, completed_tick);
        for index in 0..count {
            let mut effect = units[unit_index].status.damage_over_time[index];
            while completed_tick >= effect.next_pulse_tick
                && (effect.next_pulse_tick < effect.expires_tick
                    || (effect.final_pulse_at_expiry
                        && effect.next_pulse_tick == effect.expires_tick))
                && units[unit_index].health > 0
            {
                let unit = &units[unit_index];
                let adjusted = if unit.classifications.spell_immune
                    || unit.classifications.invulnerable
                {
                    0
                } else {
                    scale_damage_per_10k(
                        damage_rules.apply_spell(effect.damage_per_pulse, unit.armor.armor_type),
                        resistance,
                    )
                };
                let adjusted = spell_damage_taken_per_10k
                    .map_or(adjusted, |factor| scale_damage_per_10k(adjusted, factor));
                let event_damage = scale_damage_per_10k(
                    scale_damage_per_10k(adjusted, incoming_factor),
                    unit.snow_damage_taken_per_10k,
                );
                if let Some(source) = effect.source {
                    super::source_damage_runtime::dispatch_inline(
                        source,
                        unit_index,
                        event_damage,
                        units,
                        damage_rules,
                        completed_tick,
                        retired_sources,
                    );
                }
                let unit = &mut units[unit_index];
                super::damage_thresholds::observe_damage_state(
                    unit.passive_effects,
                    &mut unit.status.damage_threshold,
                    unit.health,
                    unit.health_max,
                );
                if super::damage_charge::observe_damage_state(
                    &mut unit.status.damage_charge,
                    unit.health,
                    event_damage,
                ) {
                    unit.status.pending_attack = None;
                    unit.status.action_animation = None;
                }
                let after = unit
                    .health
                    .checked_sub(event_damage)
                    .expect("damage-over-time health arithmetic overflow");
                death_healing::commit_health_from(units, unit_index, after, effect.source);
                effect.next_pulse_tick = effect
                    .next_pulse_tick
                    .checked_add(u64::from(effect.pulse_interval_ticks))
                    .expect("damage-over-time pulse tick overflow");
            }
            units[unit_index].status.damage_over_time[index] = effect;
        }
        purge_expired_status_modifiers(&mut units[unit_index].status, completed_tick);
    }
}

pub(super) fn effective_armor_points_per_100(unit: &UnitSnapshot) -> i32 {
    unit.passive_effects
        .iter()
        .filter_map(|effect| match effect {
            PassiveUnitEffect::SpikedShell(profile) => Some(profile.armor_bonus_per_100),
            _ => None,
        })
        .fold(
            unit.status.effective_armor_points_per_100(unit.armor),
            |armor, bonus| {
                armor
                    .checked_add(bonus)
                    .expect("native armor bonus exceeds i32")
            },
        )
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
        .fold(
            100_i32
                + status.native_grants.attack_speed_percent()
                + status
                    .acid_bomb
                    .map_or(0, |acid| i32::from(acid.profile.attack_speed_percent_delta)),
            |total, modifier| {
                total
                    .checked_add(i32::from(modifier.percent_delta))
                    .expect("attack-speed percentage overflow")
            },
        )
        .clamp(1, 1_000);
    let scaled = u64::from(base_ticks) * 100;
    u16::try_from(scaled.div_ceil(u64::try_from(percent).expect("positive attack speed")))
        .expect("effective attack cooldown exceeds u16")
        .max(1)
}

/// Native attack orb lands before damage and refreshes one shared armor family.
pub(super) fn apply_corruption(
    target: TargetIndex,
    profile: Option<crate::components::CorruptionProfile>,
    tick: u64,
    units: &mut [UnitSnapshot],
) -> bool {
    let (TargetIndex::Unit(i), Some(p)) = (target, profile) else {
        return false;
    };
    let u = &mut units[i];
    if u.health <= 0
        || u.classifications.invulnerable
        || !p.targets.can_target_unit(u.target_movement_class())
    {
        return false;
    }
    let duration = if u.classifications.uses_hero_spell_rules() {
        p.hero_duration_ticks
    } else {
        p.duration_ticks
    };
    apply_timed_armor_modifier(
        &mut u.status,
        TimedArmorModifier {
            undispellable: true,
            id: ModifierId(p.ability.0),
            native_buff: Some(p.buff),
            armor_bonus_per_100: -p.armor_reduction_per_100,
            expires_tick: tick + u64::from(duration),
            ..TimedArmorModifier::default()
        },
    );
    true
}

/// Presentation uses the existing negative armor effect shape; this event is not a native cast.
pub(super) fn corruption_hit_event(
    source: SimId,
    target: TargetIndex,
    p: Option<crate::components::CorruptionProfile>,
    units: &[UnitSnapshot],
) -> AbilityCastEvent {
    let (TargetIndex::Unit(i), Some(p)) = (target, p) else {
        unreachable!("successful native unit orb");
    };
    AbilityCastEvent {
        source,
        ability: p.ability,
        target: AbilityCastTarget::Unit(units[i].id),
        target_position: Some(units[i].position),
        effect: AbilityEffect::FaerieFire {
            native_buff: Some(p.buff),
            modifier: ModifierId(p.ability.0),
            armor_reduction_per_100: p.armor_reduction_per_100,
            duration_ticks: p.duration_ticks,
            hero_duration_ticks: p.hero_duration_ticks,
        },
    }
}

pub(super) fn mark_modifier_undispellable(status: &mut StatusState, id: ModifierId) {
    for modifier in &mut status.movement_modifiers[..usize::from(status.movement_modifier_count)] {
        if modifier.id == id {
            modifier.undispellable = true;
        }
    }
    for modifier in
        &mut status.attack_speed_modifiers[..usize::from(status.attack_speed_modifier_count)]
    {
        if modifier.id == id {
            modifier.undispellable = true;
        }
    }
}
