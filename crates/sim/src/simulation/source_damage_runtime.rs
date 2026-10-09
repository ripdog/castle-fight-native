use super::*;

pub(super) struct SourceDamageState<'a> {
    pub units: &'a mut [UnitSnapshot],
    pub health: &'a mut [i32],
    pub rules: DamageRules,
    pub tick: u64,
}

/// The map registers hhb only on combat sappers. Its source handler precedes
/// the target handler and temporarily disables hhb during this nested hit.
pub(super) fn apply_aftershock(source: SimId, victim: usize, state: SourceDamageState<'_>) {
    if !state.units[victim].classifications.combat_sapper || state.health[victim] <= 0 {
        return;
    }
    let Some(source_index) = find_unit_index(state.units, source) else {
        return;
    };
    let Some(profile) = state.units[source_index]
        .effective_passive_effects()
        .find_map(|effect| {
            if let PassiveUnitEffect::HealthScaledAftershock(profile) = effect {
                Some(profile)
            } else {
                None
            }
        })
    else {
        return;
    };
    let amount = i32::try_from(
        i64::from(profile.maximum_damage) * i64::from(state.health[source_index].max(0))
            / i64::from(state.units[source_index].health_max.max(1)),
    )
    .expect("aftershock damage fits validated health bounds");
    let target = &mut state.units[victim];
    // ATTACK_TYPE_SIEGE with DAMAGE_TYPE_DEMOLITION retains the attack/defense
    // table, bypasses armor points and permits spell-immune recipients.
    let amount = state.rules.apply_attack_with_armor_per_100(
        amount,
        DamageType::Siege,
        target.armor.armor_type,
        0,
    );
    let amount = scale_damage_per_10k(
        native_kill_effects::damage_after_native_incoming(target, amount, state.tick),
        target.snow_damage_taken_per_10k,
    );
    // Independent DamageListener callbacks still run. hhb's source/target
    // controllers (including charge and retaliation) must not recurse here.
    damage_thresholds::observe_damage(target, state.health[victim]);
    state.health[victim] = state.health[victim]
        .checked_sub(amount)
        .expect("aftershock health overflow");
    if state.health[victim] <= 0 && state.health[source_index] > 0 {
        native_kill_effects::activate_kill_berserk(&mut state.units[source_index], state.tick);
    }
}
