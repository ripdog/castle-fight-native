//! Script-selected native attack proxies and their in-flight, source-independent breath effect.
use super::*;
#[cfg(test)]
mod tests;
use crate::building_mechanics::HailstoneProfile;
use crate::components::{HailstoneState, NativeAction};

impl Simulation {
    pub(super) fn hailstone_target_eligible(
        &self,
        source: AbilitySourceSnapshot,
        profile: HailstoneProfile,
        entity: bevy_ecs::world::EntityRef<'_>,
    ) -> bool {
        let Some(footprint) = entity.get::<BuildingFootprint>() else {
            return false;
        };
        let point = footprint_center_point(*footprint, self.config.navigation_cell_size);
        let bounds =
            crate::building_mechanics::snowveil_for_version(profile.map_version).battlefield_bounds;
        entity.get::<Health>().is_some_and(|h| h.current > 0)
            && entity.get::<Team>().is_some_and(|t| *t != source.team)
            && entity
                .get::<Owner>()
                .is_some_and(|o| self.player_state(o.0).is_some())
            && !entity
                .get::<ContentIdentity>()
                .is_some_and(|c| profile.excluded_rawcodes.contains(&c.rawcode))
            && !entity.get::<StatusState>().is_some_and(|s| {
                s.armor_modifiers[..usize::from(s.armor_modifier_count)]
                    .iter()
                    .any(|m| m.id.0 == profile.excluded_buff && self.next_tick < m.expires_tick)
            })
            && point.x >= bounds[0]
            && point.y >= bounds[1]
            && point.x <= bounds[2]
            && point.y <= bounds[3]
    }

    pub(super) fn hailstone_trigger_allowed(
        &self,
        source: AbilitySourceSnapshot,
        ability: AutomaticAbilityProfile,
        team: Team,
        health: i32,
        position: SimPoint,
        flags: UnitClassifications,
    ) -> bool {
        let AbilityEffect::Hailstone(profile) = ability.effect else {
            return false;
        };
        health > 0
            && team != source.team
            && (!flags.invulnerable || profile.trigger_invulnerable)
            && (!flags.spell_immune || profile.trigger_spell_immune)
            && self.ability_source_distance_sq(source.origin, position) <= square_i32(ability.range)
    }

    pub(super) fn hailstone_trigger_target(
        &self,
        source: AbilitySourceSnapshot,
        ability: AutomaticAbilityProfile,
        sequence: u64,
        units: &[UnitSnapshot],
        checks: &mut usize,
    ) -> Option<AbilityIntentTarget> {
        let AbilityEffect::Hailstone(profile) = ability.effect else {
            return None;
        };
        let mut candidates = Vec::new();
        for (index, unit) in units.iter().enumerate() {
            *checks += 1;
            if profile.trigger_targets.can_target_unit(unit.movement_class)
                && self.hailstone_trigger_allowed(
                    source,
                    ability,
                    unit.team,
                    unit.health,
                    unit.position,
                    unit.classifications,
                )
                && unit.visible_to(source.team, self.next_tick)
            {
                candidates.push((unit.id, AbilityIntentTarget::Unit { index, id: unit.id }));
            }
        }
        if profile.trigger_targets.can_target_buildings() {
            for entity in self.world.iter_entities() {
                let (Some(id), Some(team), Some(health), Some(footprint)) = (
                    entity.get::<SimId>(),
                    entity.get::<Team>(),
                    entity.get::<Health>(),
                    entity.get::<BuildingFootprint>(),
                ) else {
                    continue;
                };
                *checks += 1;
                let position = footprint_center_point(*footprint, self.config.navigation_cell_size);
                let flags = entity
                    .get::<UnitClassifications>()
                    .copied()
                    .unwrap_or_default();
                if self.hailstone_trigger_allowed(
                    source,
                    ability,
                    *team,
                    health.current,
                    position,
                    flags,
                ) && self.visible_teams(position) & (1 << source.team.0) != 0
                {
                    candidates.push((*id, AbilityIntentTarget::Building { id: *id, position }));
                }
            }
        }
        candidates
            .into_iter()
            .min_by_key(|(id, _)| {
                (
                    deterministic_ability_target_rank(
                        self.config.match_seed,
                        source.id,
                        ability.id,
                        sequence,
                        *id,
                    ),
                    *id,
                )
            })
            .map(|(_, target)| target)
    }

    pub(super) fn cast_hailstone(
        &mut self,
        source: AbilitySourceSnapshot,
        ability: AutomaticAbilityProfile,
        sequence: u64,
        buildings: &[BuildingSnapshot],
    ) -> Option<bool> {
        let AbilityEffect::Hailstone(profile) = ability.effect else {
            return None;
        };
        // The callback runs after the native parent has spent its mana. Its independent
        // selector must see live phase health/status, including earlier same-tick spells.
        let target = buildings
            .iter()
            .filter(|b| {
                b.health > 0
                    && self.hailstone_target_eligible(source, profile, self.world.entity(b.entity))
                    && !b.status.is_some_and(|s| {
                        s.armor_modifiers[..usize::from(s.armor_modifier_count)]
                            .iter()
                            .any(|m| {
                                m.id.0 == profile.excluded_buff && self.next_tick < m.expires_tick
                            })
                    })
            })
            .min_by_key(|b| {
                (
                    deterministic_ability_target_rank(
                        self.config.match_seed,
                        source.id,
                        profile.ability,
                        sequence,
                        b.id,
                    ),
                    b.id,
                )
            })?;
        let destination =
            footprint_center_point(target.footprint, self.config.navigation_cell_size);
        Some(self.launch_hailstone(source, target.id, destination, profile, buildings))
    }

    pub(super) fn launch_hailstone(
        &mut self,
        source: AbilitySourceSnapshot,
        target: SimId,
        destination: SimPoint,
        profile: HailstoneProfile,
        buildings: &[BuildingSnapshot],
    ) -> bool {
        let Some(victim) = buildings.iter().find(|b| b.id == target && b.health > 0) else {
            return false;
        };
        let origin = match source.origin {
            AbilitySourceOrigin::Unit(p) => p,
            AbilitySourceOrigin::Building(f) => {
                footprint_center_point(f, self.config.navigation_cell_size)
            }
        };
        if origin.distance_sq(destination) > square_i32(profile.range) {
            return false;
        }
        self.reveal_area(
            source.team,
            destination,
            profile.vision_radius,
            u64::from(profile.vision_ticks),
            false,
        );
        // Vision is issued even when the following native attack order fails.
        if victim.classifications.invulnerable {
            return false;
        }
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::Hailstone(HailstoneState {
                source: source.id,
                team: source.team,
                target,
                profile,
                origin,
                destination,
                launch_tick: self.next_tick,
                impact_tick: self.next_tick
                    + projectile_travel_ticks(
                        origin.distance_sq(destination),
                        profile.speed_per_tick,
                    ),
            }),
        ));
        true
    }

    pub(super) fn resolve_hailstone(
        &mut self,
        state: HailstoneState,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
    ) {
        let p = state.profile;
        for target in units {
            if target.health <= 0
                || target.team == state.team
                || target.classifications.invulnerable
                || !p.splash_targets.can_target_unit(target.movement_class)
                || state.destination.distance_sq(target.position) > square_i32(p.full_radius)
            {
                continue;
            }
            let damage = self
                .combat_rules
                .damage_rules
                .apply_attack_with_armor_per_100(
                    p.damage,
                    DamageType::Chaos,
                    target.armor.armor_type,
                    effective_armor_points_per_100(target),
                );
            target.health = target.health.saturating_sub(scale_damage_per_10k(
                damage,
                target.snow_damage_taken_per_10k,
            ));
            if target.health > 0
                && !target.classifications.spell_immune
                && p.freeze_targets.can_target_unit(target.movement_class)
            {
                freeze(
                    &mut target.status,
                    self.next_tick,
                    if target.classifications.hero {
                        p.hero_freeze_ticks
                    } else {
                        p.freeze_ticks
                    },
                    p.ability,
                );
            }
        }
        for target in buildings {
            let distance_sq = point_to_footprint_distance_sq(
                state.destination,
                target.footprint,
                self.config.navigation_cell_size,
            );
            if target.health <= 0
                || target.team == state.team
                || target.classifications.invulnerable
                || (target.id != state.target
                    && (!p.splash_targets.can_target_buildings()
                        || distance_sq > square_i32(p.full_radius)))
            {
                continue;
            }
            if !self.debug_buildings_invulnerable {
                let damage = self.combat_rules.damage_rules.apply_attack(
                    p.damage,
                    DamageType::Chaos,
                    target.armor,
                );
                target.health = target.health.saturating_sub(scale_damage_per_10k(
                    damage,
                    target.snow_damage_taken_per_10k,
                ));
            }
            if target.health > 0
                && !target.classifications.spell_immune
                && p.freeze_targets.can_target_buildings()
            {
                freeze(
                    target.status.get_or_insert_default(),
                    self.next_tick,
                    if target.classifications.hero {
                        p.hero_freeze_ticks
                    } else {
                        p.freeze_ticks
                    },
                    p.ability,
                );
            }
        }
    }

    pub(super) fn resolve_due_hailstone_impacts(
        &mut self,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
        positions: &[SimPoint],
        unit_health: &mut [i32],
        building_health: &mut [i32],
    ) -> (usize, usize) {
        let mut due = self
            .world
            .iter_entities()
            .filter_map(|e| {
                let NativeAction::Hailstone(state) = e.get::<NativeAction>()? else {
                    return None;
                };
                (state.impact_tick <= self.next_tick)
                    .then(|| (*e.get::<SimId>().unwrap(), e.id(), state.clone()))
            })
            .collect::<Vec<_>>();
        if due.is_empty() {
            return (0, 0);
        }
        due.sort_unstable_by_key(|(id, _, _)| *id);
        for ((unit, &position), &health) in units.iter_mut().zip(positions).zip(unit_health.iter())
        {
            unit.position = position;
            unit.health = health;
        }
        for (building, &health) in buildings.iter_mut().zip(building_health.iter()) {
            building.health = health;
        }
        let impacts = due.len();
        for (_, entity, state) in due {
            self.world.despawn(entity);
            self.resolve_hailstone(state, units, buildings);
        }
        let mut effects = 0;
        for (unit, health) in units.iter().zip(unit_health) {
            effects += usize::from(unit.health != *health);
            *health = unit.health;
        }
        for (building, health) in buildings.iter().zip(building_health) {
            effects += usize::from(building.health != *health);
            *health = building.health;
        }
        (impacts, effects)
    }

    pub(super) fn pause_frozen_building_activities(&mut self) {
        let mut production = self.world.query::<(&StatusState, &mut ProductionState)>();
        for (status, mut state) in production.iter_mut(&mut self.world) {
            if self.next_tick < status.frozen_until_tick && state.queued != 0 {
                state.next_spawn_tick = state
                    .next_spawn_tick
                    .checked_add(1)
                    .expect("frozen production clock overflow");
            }
        }
        let mut construction = self
            .world
            .query::<(&StatusState, &mut BuildingConstruction)>();
        for (status, mut state) in construction.iter_mut(&mut self.world) {
            if self.next_tick < status.frozen_until_tick {
                state.started_tick += 1;
                state.complete_tick += 1;
            }
        }
    }
}

fn freeze(status: &mut StatusState, tick: u64, duration: u16, ability: AbilityId) {
    let expiry = tick + u64::from(duration);
    if expiry >= status.frozen_until_tick {
        status.frozen_ability = Some(ability);
    }
    status.frozen_until_tick = status.frozen_until_tick.max(expiry);
    status.stunned_until_tick = status.stunned_until_tick.max(expiry);
    status.action_animation = None;
    status.pending_attack = None;
    status.pending_cast = None;
}
