use super::*;

pub(super) fn activate_kill_berserk(source: &mut UnitSnapshot, tick: u64) {
    if source.health <= 0 || source.attacks_disabled || source.status.attacks_disabled(tick) {
        return;
    }
    for effect in source.passive_effects.iter() {
        let PassiveUnitEffect::KillBerserk(profile) = effect else {
            continue;
        };
        let id = ModifierId(profile.ability.0);
        let expires_tick = tick
            .checked_add(u64::from(profile.duration_ticks))
            .expect("Berserk expiry overflow");
        apply_timed_movement_modifier(
            &mut source.status,
            id,
            profile.movement_percent_delta,
            expires_tick,
        );
        apply_timed_attack_speed_modifier(
            &mut source.status,
            id,
            profile.attack_speed_percent_delta,
            expires_tick,
        );
        apply_timed_armor_modifier(
            &mut source.status,
            TimedArmorModifier {
                undispellable: false,
                aura_membership: false,
                id,
                damage_taken_bonus_per_10k: profile.damage_taken_bonus_per_10k,
                expires_tick,
                ..TimedArmorModifier::default()
            },
        );
        // fJ issues the native instant order, then immediately resumes attack.
        // An already released missile remains independent of the new order.
        source.status.pending_cast = None;
        source.status.pending_attack = None;
        source.status.action_animation = None;
        source.target = None;
        source.direct_retaliation_lock = false;
        source.ally_defense_lock = false;
    }
}

pub(super) fn damage_after_native_incoming(unit: &UnitSnapshot, damage: i32, tick: u64) -> i32 {
    let damage = scale_damage_per_10k(damage, native_incoming_damage_factor(&unit.status, tick));
    unit.status.illusion.map_or(damage, |illusion| {
        scale_damage_per_10k(damage, illusion.damage_taken_per_10k)
    })
}

pub(super) fn native_incoming_damage_factor(status: &StatusState, tick: u64) -> u16 {
    let bonus = status.armor_modifiers[..usize::from(status.armor_modifier_count)]
        .iter()
        .filter(|modifier| tick < modifier.expires_tick)
        .map(|modifier| i32::from(modifier.damage_taken_bonus_per_10k))
        .sum::<i32>();
    u16::try_from((10_000 + bonus).max(0)).expect("native incoming damage factor fits u16")
}

/// Non-attack incoming-damage factor from native aura buffs. Same-buff sources never stack because
/// aura membership keeps one modifier per family.
pub(super) fn native_spell_vulnerability_factor(status: &StatusState, tick: u64) -> u16 {
    let bonus = status.armor_modifiers[..usize::from(status.armor_modifier_count)]
        .iter()
        .filter(|modifier| tick < modifier.expires_tick)
        .map(|modifier| u32::from(modifier.spell_damage_taken_bonus_per_10k))
        .sum::<u32>();
    u16::try_from(10_000 + bonus).expect("native spell vulnerability factor fits u16")
}
