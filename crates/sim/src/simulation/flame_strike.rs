use super::*;
use crate::{building_mechanics::FlameStrikeState, components::NativeAction};
#[cfg(test)]
mod tests;

impl Simulation {
    pub(super) fn cast_building_flame_strike(
        &mut self,
        source: AbilitySourceSnapshot,
        ability: AutomaticAbilityProfile,
        trigger: SimId,
        sequence: u64,
        units: &[UnitSnapshot],
    ) -> bool {
        let AbilityEffect::BuildingFlameStrike(p) = ability.effect else {
            unreachable!()
        };
        let owner = self
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&source.id))
            .and_then(|e| e.get::<Owner>())
            .map_or(PlayerId(source.team.0), |o| o.0);
        self.launch_native_parasite(source, owner, trigger, p.parent);
        let option = p.options[usize::from(!self.config.match_rules.artillery_enabled)];
        let bounds =
            crate::building_mechanics::snowveil_for_version(p.map_version).battlefield_bounds;
        let target = units
            .iter()
            .filter(|u| {
                u.health > 0
                    && u.team != source.team
                    && u.classifications.combat_sapper
                    && u.target_movement_class() == MovementClass::Ground
                    && !p
                        .selector_exclusions
                        .into_iter()
                        .any(|a| self.native_ability_installed(u, a))
                    && u.position.x >= bounds[0]
                    && u.position.x <= bounds[2]
                    && u.position.y >= bounds[1]
                    && u.position.y <= bounds[3]
            })
            .min_by_key(|u| {
                (
                    deterministic_ability_target_rank(
                        self.config.match_seed,
                        source.id,
                        option.ability,
                        sequence,
                        u.id,
                    ),
                    u.id,
                )
            });
        let Some(center) = target.map(|u| u.position) else {
            return false;
        };
        let actor = self.allocate_id();
        self.world.spawn((
            actor,
            NativeAction::NativeSpellActor {
                owner,
                team: source.team,
                flying: p.dummy_flying,
                map_version: p.map_version,
                rawcode: p.dummy_rawcode,
                position: center,
                expires_tick: self.next_tick + u64::from(p.dummy_ticks),
            },
        ));
        let released_tick = self.next_tick + u64::from(option.cast_ticks);
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::FlameStrike(FlameStrikeState {
                map_version: p.map_version,
                source: actor,
                team: source.team,
                center,
                profile: option,
                released_tick,
                expires_tick: released_tick
                    + ceil_millis_to_ticks(u64::from(option.duration_millis)),
                next_pulse: 0,
            }),
        ));
        true
    }

    pub(super) fn advance_flame_strike(
        &mut self,
        entity: Entity,
        mut state: FlameStrikeState,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
        flames: &[(SimId, FlameStrikeState)],
    ) {
        if self.next_tick < state.released_tick {
            return;
        }
        if state.next_pulse == 0 {
            self.last_ability_casts.push(AbilityCastEvent {
                source: state.source,
                ability: state.profile.ability,
                target: AbilityCastTarget::Point(state.center),
                target_position: Some(state.center),
                effect: AbilityEffect::NativeWeatherVisual {
                    duration_ticks: ceil_millis_to_ticks(u64::from(state.profile.duration_millis))
                        as u16,
                },
            });
        }
        while let Some((millis, damage)) = state.profile.pulse(state.next_pulse) {
            let due = state.released_tick + ceil_millis_to_ticks(millis);
            if due > self.next_tick {
                break;
            }
            self.apply_flame_strike_pulse(entity, &state, damage, units, buildings, flames);
            state.next_pulse = state
                .next_pulse
                .checked_add(1)
                .expect("Flame Strike pulse count overflow");
        }
        if self.next_tick >= state.expires_tick {
            self.world.despawn(entity);
        } else {
            *self
                .world
                .entity_mut(entity)
                .get_mut::<NativeAction>()
                .unwrap() = NativeAction::FlameStrike(state);
        }
    }

    fn flame_strike_wins(
        &self,
        id: SimId,
        state: &FlameStrikeState,
        point: SimPoint,
        movement: Option<MovementClass>,
        flames: &[(SimId, FlameStrikeState)],
    ) -> bool {
        let strength = state
            .profile
            .strength(self.next_tick.saturating_sub(state.released_tick));
        !flames.iter().any(|(other_id, other)| {
            if *other_id == id
                || other.team != state.team
                || other.profile.buff != state.profile.buff
                || self.next_tick < other.released_tick
                || self.next_tick > other.expires_tick
                || other.center.distance_sq(point) > square_i32(other.profile.radius)
                || !movement.map_or_else(
                    || other.profile.targets.can_target_buildings(),
                    |m| other.profile.targets.can_target_unit(m),
                )
            {
                return false;
            }
            let other_strength = other.profile.strength(self.next_tick - other.released_tick);
            other_strength > strength || (other_strength == strength && *other_id < id)
        })
    }

    fn apply_flame_strike_pulse(
        &mut self,
        entity: Entity,
        state: &FlameStrikeState,
        damage: i32,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
        flames: &[(SimId, FlameStrikeState)],
    ) {
        let p = state.profile;
        let id = *self.world.get::<SimId>(entity).unwrap();
        let mut recipients = units
            .iter()
            .enumerate()
            .filter(|(_, u)| {
                u.health > 0
                    && u.team != state.team
                    && !u.classifications.invulnerable
                    && !u.classifications.spell_immune
                    && !u.classifications.locust
                    && p.targets.can_target_unit(u.target_movement_class())
                    && state.center.distance_sq(u.position) <= square_i32(p.radius)
                    && self.flame_strike_wins(
                        id,
                        state,
                        u.position,
                        Some(u.target_movement_class()),
                        flames,
                    )
            })
            .map(|(i, u)| (u.id, false, i, i64::from(damage)))
            .collect::<Vec<_>>();
        if p.targets.can_target_buildings() {
            recipients.extend(
                buildings
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| {
                        b.health > 0
                            && b.team != state.team
                            && !b.classifications.invulnerable
                            && !b.classifications.spell_immune
                            && !self.debug_buildings_invulnerable
                            && state.center.distance_sq(footprint_center_point(
                                b.footprint,
                                self.config.navigation_cell_size,
                            )) <= square_i32(p.radius)
                            && self.flame_strike_wins(
                                id,
                                state,
                                footprint_center_point(
                                    b.footprint,
                                    self.config.navigation_cell_size,
                                ),
                                None,
                                flames,
                            )
                    })
                    .map(|(i, b)| {
                        (
                            b.id,
                            true,
                            i,
                            i64::from(damage) * i64::from(p.building_damage_per_10k) / 10000,
                        )
                    }),
            );
        }
        recipients.sort_unstable_by_key(|(id, _, _, _)| *id);
        let total: i64 = recipients.iter().map(|(_, _, _, damage)| damage).sum();
        for (_, building, i, raw) in recipients {
            let raw = if total > i64::from(p.maximum_damage) {
                raw * i64::from(p.maximum_damage) / total
            } else {
                raw
            } as i32;
            if building {
                let b = &mut buildings[i];
                let damage = self
                    .combat_rules
                    .damage_rules
                    .apply_spell(raw, b.armor.armor_type);
                b.health = b
                    .health
                    .saturating_sub(scale_damage_per_10k(damage, b.snow_damage_taken_per_10k));
            } else {
                let event = spell_damage_after_defend(
                    units[i],
                    self.combat_rules
                        .damage_rules
                        .apply_spell(raw, units[i].armor.armor_type),
                    self.next_tick,
                );
                source_damage_runtime::dispatch_inline(
                    state.source,
                    i,
                    event,
                    units,
                    self.combat_rules.damage_rules,
                    self.next_tick,
                    &self.retired_damage_sources,
                );
                let health = units[i].health;
                damage_thresholds::observe_damage_event(&mut units[i], health, event);
                if units[i].health > 0 {
                    death_healing::commit_health_from(
                        units,
                        i,
                        units[i].health.saturating_sub(event),
                        Some(state.source),
                    );
                }
            }
        }
        death_healing::refresh_support(units, buildings, None);
    }
}
