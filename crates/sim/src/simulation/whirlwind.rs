//! Script-authored Whirlwind: a damage listener spins around the damaged target.
use super::*;

/// Universal damage to every eligible unit within the profile radius of `center`.
pub(super) fn resolve(
    center: SimPoint,
    profile: crate::WhirlwindProfile,
    source: PendingAttackEffectSource,
    state: &mut PendingAttackEffectState<'_>,
) -> AbilityCastEvent {
    let radius_sq = square_i32(profile.radius);
    for i in 0..state.units.len() {
        let unit = &state.units[i];
        if state.unit_health[i] <= 0
            || unit.team == source.team
            || unit.classifications.invulnerable
            || (profile.combat_sappers_only && !unit.classifications.combat_sapper)
            || !profile
                .targets
                .can_target_unit(unit.target_movement_class())
            || center.distance_sq(unit.position) > radius_sq
        {
            continue;
        }
        let damage = state
            .damage_rules
            .apply_spell(profile.damage, unit.armor.armor_type);
        let damage = spell_damage_after_defend(*unit, damage, state.completed_tick);
        super::source_damage_runtime::dispatch(
            source.id,
            i,
            damage,
            super::source_damage_runtime::SourceDamageState {
                units: state.units,
                health: state.unit_health,
                rules: state.damage_rules,
                tick: state.completed_tick,
            },
            state.retired_sources,
        );
        damage_thresholds::observe_damage_event(&mut state.units[i], state.unit_health[i], damage);
        let after = state.unit_health[i].saturating_sub(damage);
        death_healing::commit_split_health_from(
            state.units,
            state.unit_health,
            None,
            i,
            after,
            Some(source.id),
        );
    }
    AbilityCastEvent {
        source: source.id,
        ability: profile.ability,
        target: AbilityCastTarget::Point(center),
        target_position: Some(center),
        effect: AbilityEffect::AreaDamage {
            amount: profile.damage,
            radius: profile.radius,
            origin: AreaDamageOrigin::Target,
        },
    }
}
