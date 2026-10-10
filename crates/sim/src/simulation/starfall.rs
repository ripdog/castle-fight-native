//! Fresh native dummy channels own their point and deadlines independently of the building.
use super::*;
use crate::building_mechanics::{StarfallProfile, StarfallState};
#[cfg(test)]
mod tests;

impl Simulation {
    pub(super) fn cast_starfall(
        &mut self,
        source: AbilitySourceSnapshot,
        ability: AbilityId,
        profile: StarfallProfile,
        sequence: u64,
        units: &[UnitSnapshot],
    ) -> bool {
        let bounds =
            crate::building_mechanics::snowveil_for_version(profile.map_version).battlefield_bounds;
        let target = units
            .iter()
            .filter(|u| {
                u.health > 0
                    && u.team != source.team
                    && u.classifications.combat_sapper
                    && u.rawcode != profile.selector_excluded_unit
                    && !profile
                        .selector_exclusions
                        .into_iter()
                        .any(|a| self.native_ability_installed(u, a))
                    && u.position.x >= bounds[0]
                    && u.position.y >= bounds[1]
                    && u.position.x <= bounds[2]
                    && u.position.y <= bounds[3]
            })
            .min_by_key(|u| {
                (
                    deterministic_ability_target_rank(
                        self.config.match_seed,
                        source.id,
                        ability,
                        sequence,
                        u.id,
                    ),
                    u.id,
                )
            });
        let Some(target) = target else {
            return false;
        };
        let center = target.position;
        // NGb is the startup reveal flag. Functional casts run in the active round;
        // the source's startup full-map modifier has already been removed.
        self.reveal_area(
            source.team,
            center,
            profile.vision_radius,
            u64::from(profile.visual_duration_ticks),
            false,
        );
        self.last_ability_casts.push(AbilityCastEvent {
            source: source.id,
            ability,
            target: AbilityCastTarget::AllEnemyUnits,
            target_position: None,
            effect: AbilityEffect::Starfall(profile),
        });
        let wave = profile.options[usize::from(!self.config.match_rules.artillery_enabled)];
        if wave.mana_cost > profile.dummy_mana {
            return false;
        }
        let duration = profile.dummy_lifetime_ticks.min(wave.duration_ticks);
        let id = self.allocate_id();
        self.world.spawn((
            id,
            crate::components::NativeAction::Starfall(StarfallState {
                map_version: profile.map_version,
                source: source.id,
                team: source.team,
                center,
                profile: wave,
                mana_after_cast: profile.dummy_mana - wave.mana_cost,
                cooldown_until_tick: self.next_tick + u64::from(wave.cooldown_ticks),
                expires_tick: self.next_tick + u64::from(duration),
                next_pulse_tick: self.next_tick + u64::from(wave.interval_ticks),
            }),
        ));
        self.last_ability_casts.push(AbilityCastEvent {
            source: source.id,
            ability: wave.ability,
            target: AbilityCastTarget::Point(center),
            target_position: Some(center),
            effect: AbilityEffect::NativeWeatherVisual {
                duration_ticks: duration,
            },
        });
        true
    }
    pub(super) fn advance_starfall(
        &mut self,
        entity: Entity,
        mut state: StarfallState,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
    ) {
        if self.next_tick >= state.expires_tick {
            self.world.despawn(entity);
            return;
        }
        if self.next_tick < state.next_pulse_tick {
            return;
        }
        let p = state.profile;
        let mut recipients: Vec<_> = units
            .iter()
            .enumerate()
            .filter(|(_, u)| {
                u.health > 0
                    && u.team != state.team
                    && !u.classifications.invulnerable
                    && p.targets.can_target_unit(u.target_movement_class())
                    && state.center.distance_sq(u.position) <= square_i32(p.radius)
            })
            .map(|(i, u)| (u.id, false, i))
            .collect();
        if p.targets.can_target_buildings() && !self.debug_buildings_invulnerable {
            recipients.extend(
                buildings
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| {
                        b.health > 0
                            && b.team != state.team
                            && !b.classifications.invulnerable
                            && state.center.distance_sq(footprint_center_point(
                                b.footprint,
                                self.config.navigation_cell_size,
                            )) <= square_i32(p.radius)
                    })
                    .map(|(i, b)| (b.id, true, i)),
            );
        }
        recipients.sort_unstable_by_key(|(id, _, _)| *id);
        for (id, structure, i) in recipients {
            let position;
            if structure {
                let b = &mut buildings[i];
                position = footprint_center_point(b.footprint, self.config.navigation_cell_size);
                let damage = scale_damage_per_10k(p.damage, p.building_damage_per_10k);
                let damage = self
                    .combat_rules
                    .damage_rules
                    .apply_spell(damage, b.armor.armor_type);
                b.health = b
                    .health
                    .saturating_sub(scale_damage_per_10k(damage, b.snow_damage_taken_per_10k));
                death_healing::refresh_support(units, buildings, None);
            } else {
                if units[i].health <= 0 {
                    continue;
                }
                let u = &mut units[i];
                position = u.position;
                // Native AEsf is a Spell/Universal ultimate: its mask permits immune bodies.
                // Native resistance/Defend do not reduce universal damage; scripted incoming factors still do.
                let damage = self
                    .combat_rules
                    .damage_rules
                    .apply_spell(p.damage, u.armor.armor_type);
                let damage = scale_damage_per_10k(
                    native_kill_effects::damage_after_native_incoming(u, damage, self.next_tick),
                    u.snow_damage_taken_per_10k,
                );
                if damage > 0 {
                    damage_thresholds::observe_damage_event(u, u.health, damage);
                }
                let after = u.health.saturating_sub(damage);
                // Actual caster is the invulnerable recycled dummy, never the logical building.
                death_healing::commit_health_from(units, i, after, None);
            }
            self.last_ability_casts.push(AbilityCastEvent {
                source: state.source,
                ability: AbilityId(p.buff),
                target: AbilityCastTarget::Unit(id),
                target_position: Some(position),
                effect: AbilityEffect::NativeWeatherVisual {
                    duration_ticks: p.interval_ticks,
                },
            });
        }
        state.next_pulse_tick = state
            .next_pulse_tick
            .checked_add(u64::from(p.interval_ticks))
            .expect("native Starfall pulse overflow");
        self.world
            .entity_mut(entity)
            .insert(crate::components::NativeAction::Starfall(state));
    }
}
