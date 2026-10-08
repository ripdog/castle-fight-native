use super::*;

pub(super) fn observe_damage(unit: &mut UnitSnapshot, current_health: i32) {
    observe_damage_state(
        unit.passive_effects,
        &mut unit.status.damage_threshold,
        current_health,
        unit.health_max,
    );
}

pub(super) fn observe_damage_state(
    effects: PassiveUnitEffects,
    state: &mut crate::components::DamageThresholdState,
    current_health: i32,
    health_max: i32,
) {
    if current_health <= 0 {
        return;
    }
    for effect in effects.iter() {
        let PassiveUnitEffect::DamageThreshold(profile) = effect else {
            continue;
        };
        let Some((index, threshold)) =
            profile
                .thresholds
                .iter()
                .enumerate()
                .rev()
                .find(|(_, threshold)| {
                    i64::from(current_health) * 10_000
                        < i64::from(health_max) * i64::from(threshold.hp_ratio_below_per_10k)
                })
        else {
            continue;
        };
        let level = u8::try_from(index + 1).expect("bounded threshold level");
        let update = if threshold.adds_if_absent {
            state.ability_level == 0
        } else {
            state.ability_level < level
        };
        if update {
            state.ability = Some(profile.ability);
            // A native level assignment cannot add an absent ability. The script
            // still changes vertex color after that failed assignment.
            if state.ability_level > 0 || threshold.adds_if_absent {
                state.ability_level = level;
                state.attack_bonus = threshold.attack_bonus;
            }
            state.color_level = level;
            state.vertex_rgba = Some(threshold.vertex_rgba);
        }
    }
}
