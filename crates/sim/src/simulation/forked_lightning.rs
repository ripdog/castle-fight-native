//! Native Forked Lightning resolves all branches together; it has no bounce clock.
use super::*;

pub(super) fn resolve(
    target: TargetIndex,
    profile: crate::ForkedLightningProfile,
    source: PendingAttackEffectSource,
    state: &mut PendingAttackEffectState<'_>,
) -> Vec<ChainLightningEvent> {
    let eligible = |index: TargetIndex| match index {
        TargetIndex::Unit(i) => {
            state.unit_health[i] > 0
                && state.units[i].team != source.team
                && !state.units[i].classifications.spell_immune
                && !state.units[i].classifications.invulnerable
                && profile
                    .targets
                    .can_target_unit(state.units[i].target_movement_class())
        }
        TargetIndex::Building(i) => {
            state.building_health[i] > 0
                && state.buildings[i].team != source.team
                && !state.buildings[i].classifications.spell_immune
                && !state.buildings[i].classifications.invulnerable
                && profile.targets.can_target_buildings()
        }
    };
    if profile.maximum_targets == 0 || !eligible(target) {
        return Vec::new();
    }
    let point = |index: TargetIndex| match index {
        TargetIndex::Unit(i) => state.units[i].position,
        TargetIndex::Building(i) => {
            footprint_center_point(state.buildings[i].footprint, state.navigation_cell_size)
        }
    };
    let origin = find_unit_index(state.units, source.id)
        .map_or(source.position, |i| state.units[i].position);
    let cone = native_actions::cone_wave::new_state(
        source.id,
        source.team,
        origin,
        point(target),
        crate::ConeWaveProfile {
            ability: profile.ability,
            damage: 0,
            maximum_damage: 0,
            distance: profile.distance,
            initial_radius: profile.initial_radius,
            final_radius: profile.final_radius,
            speed_per_tick: 1,
            targets: profile.targets,
        },
        state.completed_tick,
    );
    let mut candidates = Vec::new();
    for i in 0..state.units.len() + state.buildings.len() {
        let index = if i < state.units.len() {
            TargetIndex::Unit(i)
        } else {
            TargetIndex::Building(i - state.units.len())
        };
        if index == target || !eligible(index) {
            continue;
        }
        if native_actions::cone_wave::hit_distance(&cone, point(index), 0, profile.distance)
            .is_some()
        {
            let id = match index {
                TargetIndex::Unit(i) => state.units[i].id,
                TargetIndex::Building(i) => state.buildings[i].id,
            };
            candidates.push((origin.distance_sq(point(index)), id, index));
        }
    }
    candidates.sort_unstable_by_key(|(distance, id, _)| (*distance, *id));
    let targets: Vec<_> = std::iter::once(target)
        .chain(candidates.into_iter().map(|(_, _, index)| index))
        .take(usize::from(profile.maximum_targets))
        .map(|index| (index, point(index)))
        .collect();
    let mut events = Vec::new();
    for (target, destination) in targets {
        match target {
            TargetIndex::Unit(i) => {
                let damage = state
                    .damage_rules
                    .apply_spell(profile.damage, state.units[i].armor.armor_type);
                let damage =
                    spell_damage_after_defend(state.units[i], damage, state.completed_tick);
                damage_thresholds::observe_damage_event(
                    &mut state.units[i],
                    state.unit_health[i],
                    damage,
                );
                state.unit_health[i] = state.unit_health[i].saturating_sub(damage);
            }
            TargetIndex::Building(i) => {
                let damage = scale_damage_per_10k(
                    state
                        .damage_rules
                        .apply_spell(profile.damage, state.buildings[i].armor.armor_type),
                    state.buildings[i].snow_damage_taken_per_10k,
                );
                state.building_health[i] = state.building_health[i].saturating_sub(damage);
            }
        }
        let mut points = [SimPoint::default(); MAX_BOUNCE_HITS + 1];
        points[0] = origin;
        points[1] = destination;
        events.push(ChainLightningEvent {
            source: source.id,
            ability: profile.ability,
            bounce_index: 0,
            points,
            point_count: 2,
        });
    }
    events
}
