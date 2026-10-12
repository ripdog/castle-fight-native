//! Fixed-heading expanding native spell wave. Trigger eligibility and cone recipients are separate.
use super::*;
use crate::components::{ConeWaveProfile, ConeWaveState};

pub(in crate::simulation) fn new_state(
    source: SimId,
    team: Team,
    origin: SimPoint,
    target: SimPoint,
    profile: ConeWaveProfile,
    tick: u64,
) -> ConeWaveState {
    let dx = i64::from(target.x) - i64::from(origin.x);
    let dy = i64::from(target.y) - i64::from(origin.y);
    let length = ((i128::from(dx) * i128::from(dx) + i128::from(dy) * i128::from(dy)) as u128)
        .isqrt()
        .max(1) as i64;
    let destination = SimPoint::new(
        origin
            .x
            .saturating_add((dx * i64::from(profile.distance) / length) as i32),
        origin
            .y
            .saturating_add((dy * i64::from(profile.distance) / length) as i32),
    );
    ConeWaveState {
        source,
        team,
        profile,
        origin,
        destination,
        launch_tick: tick,
        traveled: 0,
        remaining_damage: profile.maximum_damage,
        hit_targets: Vec::new(),
    }
}

pub(in crate::simulation) fn hit_distance(
    state: &ConeWaveState,
    point: SimPoint,
    from: i32,
    to: i32,
) -> Option<i64> {
    let dx = i128::from(state.destination.x) - i128::from(state.origin.x);
    let dy = i128::from(state.destination.y) - i128::from(state.origin.y);
    let px = i128::from(point.x) - i128::from(state.origin.x);
    let py = i128::from(point.y) - i128::from(state.origin.y);
    let length = ((dx * dx + dy * dy) as u128).isqrt() as i128;
    if length == 0 {
        return None;
    }
    let dot = dx * px + dy * py;
    if dot < i128::from(from) * length || dot > i128::from(to) * length {
        return None;
    }
    let distance = dot / length;
    let width = i128::from(state.profile.initial_radius)
        + (i128::from(state.profile.final_radius) - i128::from(state.profile.initial_radius))
            * distance
            / i128::from(state.profile.distance.max(1));
    let cross = dx * py - dy * px;
    (cross * cross <= width * width * length * length).then_some(distance as i64)
}

impl Simulation {
    /// Every admitted orb wave has a one-tick native child cooldown. The saved wave's
    /// launch identity supplies that clock, including repeated impacts in one phase.
    pub(in crate::simulation) fn launch_attack_cone_wave(&mut self, state: ConeWaveState) {
        if self.world.iter_entities().any(|e| matches!(e.get::<NativeAction>(),
            Some(NativeAction::ConeWave(existing)) if existing.source == state.source
                && existing.profile.ability == state.profile.ability && existing.launch_tick == state.launch_tick)) {
            return;
        }
        let id = self.allocate_id();
        self.world.spawn((id, NativeAction::ConeWave(state)));
    }

    pub(in crate::simulation) fn advance_cone_wave(
        &mut self,
        entity: Entity,
        mut state: ConeWaveState,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
    ) {
        let traveled = i32::try_from(
            (self.next_tick.saturating_sub(state.launch_tick)
                * state.profile.speed_per_tick as u64)
                .min(state.profile.distance as u64),
        )
        .unwrap();
        let mut hits: Vec<_> = units
            .iter()
            .enumerate()
            .filter_map(|(index, unit)| {
                (unit.health > 0
                    && unit.team != state.team
                    && !unit.classifications.locust
                    && !unit.classifications.spell_immune
                    && !unit.classifications.invulnerable
                    && state
                        .profile
                        .targets
                        .can_target_unit(unit.target_movement_class())
                    && !state.hit_targets.contains(&unit.id))
                .then(|| {
                    hit_distance(&state, unit.position, state.traveled, traveled)
                        .map(|distance| (distance, unit.id, TargetIndex::Unit(index)))
                })
                .flatten()
            })
            .collect();
        if state.profile.targets.can_target_buildings() && !self.debug_buildings_invulnerable {
            hits.extend(
                buildings
                    .iter()
                    .enumerate()
                    .filter_map(|(index, building)| {
                        if building.health <= 0
                            || building.team == state.team
                            || building.classifications.invulnerable
                            || building.classifications.spell_immune
                            || state.hit_targets.contains(&building.id)
                        {
                            return None;
                        }
                        let center = footprint_center_point(
                            building.footprint,
                            self.config.navigation_cell_size,
                        );
                        hit_distance(&state, center, state.traveled, traveled)
                            .map(|distance| (distance, building.id, TargetIndex::Building(index)))
                    }),
            );
        }
        hits.sort_unstable_by_key(|(distance, id, _)| (*distance, *id));
        for (_, id, target) in hits {
            if state.remaining_damage <= 0 {
                break;
            }
            let raw = state.profile.damage.min(state.remaining_damage);
            state.remaining_damage -= raw;
            let position = match target {
                TargetIndex::Unit(index) => {
                    let damage = self
                        .combat_rules
                        .damage_rules
                        .apply_spell(raw, units[index].armor.armor_type);
                    let damage = spell_damage_after_defend(units[index], damage, self.next_tick);
                    super::super::source_damage_runtime::dispatch_inline(
                        state.source,
                        index,
                        damage,
                        units,
                        self.combat_rules.damage_rules,
                        self.next_tick,
                        &self.retired_damage_sources,
                    );
                    let health = units[index].health;
                    damage_thresholds::observe_damage_event(&mut units[index], health, damage);
                    super::super::death_healing::commit_health_from(
                        units,
                        index,
                        health.saturating_sub(damage),
                        Some(state.source),
                    );
                    units[index].position
                }
                TargetIndex::Building(index) => {
                    let building = &mut buildings[index];
                    let damage = self
                        .combat_rules
                        .damage_rules
                        .apply_spell(raw, building.armor.armor_type);
                    building.health = building
                        .health
                        .checked_sub(scale_damage_per_10k(
                            damage,
                            building.snow_damage_taken_per_10k,
                        ))
                        .expect("native wave building health overflow");
                    footprint_center_point(building.footprint, self.config.navigation_cell_size)
                }
            };
            if matches!(target, TargetIndex::Building(_)) {
                super::super::death_healing::refresh_support(units, buildings, None);
            }
            state.hit_targets.push(id);
            self.last_ability_casts.push(AbilityCastEvent {
                source: state.source,
                ability: state.profile.ability,
                target: AbilityCastTarget::Unit(id),
                target_position: Some(position),
                effect: AbilityEffect::Damage { amount: raw },
            });
        }
        state.traveled = traveled;
        if traveled >= state.profile.distance {
            self.world.despawn(entity);
        } else {
            self.world
                .entity_mut(entity)
                .insert(NativeAction::ConeWave(state));
        }
    }
}
