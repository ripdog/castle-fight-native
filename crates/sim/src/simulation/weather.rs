//! Released point channels remain independent of their logical building.
use super::*;
use crate::building_mechanics::{BuildingWeatherProfile, WeatherState};
const RANDOM_PURPOSE_WEATHER: u64 = 0x5745_4154_4845_5221;

impl Simulation {
    pub(super) fn cast_building_weather(
        &mut self,
        source: AbilitySourceSnapshot,
        ability: AbilityId,
        profile: BuildingWeatherProfile,
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
                    && !u.classifications.invulnerable
                    && (u.movement_class == MovementClass::Ground) == profile.select_ground
                    && (!profile.select_ground
                        || (self.hex_trigger_eligible(u, profile.map_version)
                            && !u
                                .status
                                .native_grants
                                .contains(AbilityId(profile.selector_excluded_ability))))
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
        let option = (deterministic_random(
            self.config.match_seed,
            self.next_tick,
            source.id,
            RANDOM_PURPOSE_WEATHER,
            sequence,
        ) % u64::from(profile.option_count)) as usize;
        let wave = profile.options[usize::from(!self.config.match_rules.artillery_enabled)][option];
        let id = self.allocate_id();
        let mut state = WeatherState {
            map_version: profile.map_version,
            source: source.id,
            team: source.team,
            center: target.position,
            profile: wave,
            started_tick: self.next_tick,
            expires_tick: self.next_tick + u64::from(profile.dummy_lifetime_ticks),
            next_wave: 0,
            next_visual: 0,
        };
        self.emit_weather_wave(&state);
        state.next_visual = if wave.shards == 0 { wave.waves } else { 1 };
        self.world
            .spawn((id, crate::components::NativeAction::Weather(state)));
        true
    }

    fn emit_weather_wave(&mut self, state: &WeatherState) {
        let p = state.profile;
        for shard in 0..p.shards.max(1) {
            let mut point = state.center;
            if p.shards > 0 {
                let width = 2 * p.radius as u64 + 1;
                for attempt in 0..16 {
                    let sequence = (u64::from(state.next_visual) * u64::from(p.shards)
                        + u64::from(shard))
                        * 32
                        + attempt * 2;
                    let x = (deterministic_random(
                        self.config.match_seed,
                        state.started_tick,
                        state.source,
                        RANDOM_PURPOSE_WEATHER + 1,
                        sequence,
                    ) % width) as i32
                        - p.radius;
                    let y = (deterministic_random(
                        self.config.match_seed,
                        state.started_tick,
                        state.source,
                        RANDOM_PURPOSE_WEATHER + 1,
                        sequence + 1,
                    ) % width) as i32
                        - p.radius;
                    if SimPoint::default().distance_sq(SimPoint::new(x, y)) <= square_i32(p.radius)
                    {
                        point = SimPoint::new(state.center.x + x, state.center.y + y);
                        break;
                    }
                }
            }
            self.last_ability_casts.push(AbilityCastEvent {
                source: state.source,
                ability: p.ability,
                target: AbilityCastTarget::Point(point),
                target_position: Some(point),
                effect: AbilityEffect::NativeWeatherVisual {
                    duration_ticks: if p.shards == 0 {
                        p.duration_ticks
                    } else {
                        p.first_impact_ticks
                    },
                },
            });
        }
    }

    pub(super) fn advance_weather(
        &mut self,
        entity: Entity,
        mut state: WeatherState,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
        active: &[(SimId, WeatherState)],
    ) {
        let due = state.started_tick
            + u64::from(state.profile.first_impact_ticks)
            + u64::from(state.next_wave) * u64::from(state.profile.interval_ticks);
        if self.next_tick >= state.expires_tick {
            self.world.despawn(entity);
            return;
        }
        if state.next_visual < state.profile.waves
            && self.next_tick
                >= state.started_tick
                    + u64::from(state.next_visual) * u64::from(state.profile.interval_ticks)
        {
            self.emit_weather_wave(&state);
            state.next_visual = if state.profile.shards == 0 {
                state.profile.waves
            } else {
                state.next_visual + 1
            };
        }
        if self.next_tick < due {
            self.world
                .entity_mut(entity)
                .insert(crate::components::NativeAction::Weather(state));
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
                    && !u.classifications.spell_immune
                    && p.targets.can_target_unit(u.target_movement_class())
                    && state.center.distance_sq(u.position) <= square_i32(p.radius)
            })
            .map(|(i, u)| (u.id, false, i))
            .collect();
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
                            && state.center.distance_sq(footprint_center_point(
                                b.footprint,
                                self.config.navigation_cell_size,
                            )) <= square_i32(p.radius)
                    })
                    .map(|(i, b)| (b.id, true, i)),
            );
        }
        recipients.sort_unstable_by_key(|(id, _, _)| *id);
        let action_id = *self.world.get::<SimId>(entity).unwrap();
        let recipients: Vec<_> = recipients
            .into_iter()
            .map(|(id, building, i)| {
                let (point, movement, hero) = if building {
                    (
                        footprint_center_point(
                            buildings[i].footprint,
                            self.config.navigation_cell_size,
                        ),
                        None,
                        false,
                    )
                } else {
                    (
                        units[i].position,
                        Some(units[i].target_movement_class()),
                        units[i].classifications.hero,
                    )
                };
                let winner = |family| {
                    weather_family_winner(
                        action_id,
                        &state,
                        (point, movement),
                        self.next_tick,
                        active,
                        family,
                    )
                };
                (
                    id,
                    building,
                    i,
                    winner(WeatherFamily::Wave),
                    winner(WeatherFamily::Burn { hero }),
                )
            })
            .collect();
        // Native per-wave caps reduce all recipients proportionally, independent of iteration order.
        let total: i64 = recipients
            .iter()
            .map(|(_, building, _, wave_winner, _)| {
                if !wave_winner {
                    return 0;
                }
                if *building {
                    i64::from(p.damage) * i64::from(p.building_damage_per_10k) / 10000
                } else {
                    i64::from(p.damage)
                }
            })
            .sum();
        for (_, building, i, wave_winner, burn_winner) in recipients {
            let raw = if !wave_winner {
                0
            } else if building {
                i64::from(p.damage) * i64::from(p.building_damage_per_10k) / 10000
            } else {
                i64::from(p.damage)
            };
            let raw = if total > i64::from(p.maximum_damage) {
                raw * i64::from(p.maximum_damage) / total
            } else {
                raw
            } as i32;
            if building {
                let b = &mut buildings[i];
                b.health = b.health.saturating_sub(
                    self.combat_rules
                        .damage_rules
                        .apply_spell(raw, b.armor.armor_type),
                );
                if b.health > 0 && burn_winner {
                    let burn = apply_weather_burn(
                        b.status.get_or_insert_default(),
                        p,
                        false,
                        self.next_tick,
                    );
                    b.health = b.health.saturating_sub(
                        self.combat_rules
                            .damage_rules
                            .apply_spell(burn, b.armor.armor_type),
                    );
                }
                death_healing::refresh_support(units, buildings, None);
            } else {
                let target = &mut units[i];
                let damage = self
                    .combat_rules
                    .damage_rules
                    .apply_spell(raw, target.armor.armor_type);
                let damage = spell_damage_after_defend(*target, damage, self.next_tick);
                // This damage belongs to the recycled native dummy, not the enchanted logical building.
                if raw > 0 {
                    damage_thresholds::observe_damage_event(target, target.health, damage);
                }
                let after = target.health.saturating_sub(damage);
                death_healing::commit_health(units, i, after);
                let target = &mut units[i];
                if target.health > 0 && burn_winner {
                    let burn = apply_weather_burn(
                        &mut target.status,
                        p,
                        target.classifications.hero,
                        self.next_tick,
                    );
                    let burn = self
                        .combat_rules
                        .damage_rules
                        .apply_spell(burn, target.armor.armor_type);
                    let burn = spell_damage_after_defend(*target, burn, self.next_tick);
                    if burn > 0 {
                        damage_thresholds::observe_damage_event(target, target.health, burn);
                    }
                    let after = target.health.saturating_sub(burn);
                    death_healing::commit_health(units, i, after);
                }
                let target = &units[i];
                if p.shards == 0 {
                    self.last_ability_casts.push(AbilityCastEvent {
                        source: state.source,
                        ability: AbilityId(p.buff_rawcode),
                        target: AbilityCastTarget::Unit(target.id),
                        target_position: Some(target.position),
                        effect: AbilityEffect::NativeWeatherVisual {
                            duration_ticks: p.interval_ticks,
                        },
                    });
                }
            }
        }
        state.next_wave += 1;
        if state.next_wave >= p.waves {
            self.world.despawn(entity);
        } else {
            self.world
                .entity_mut(entity)
                .insert(crate::components::NativeAction::Weather(state));
        }
    }
}

#[derive(Clone, Copy)]
enum WeatherFamily {
    Wave,
    Burn { hero: bool },
}

fn weather_family_winner(
    id: SimId,
    state: &WeatherState,
    (point, movement): (SimPoint, Option<MovementClass>),
    tick: u64,
    active: &[(SimId, WeatherState)],
    family: WeatherFamily,
) -> bool {
    // Native caster buff families select the strongest overlapping channel. Sample the
    // entire phase before mutation so a completing action cannot expose a weaker duplicate.
    let eligible = |p: crate::building_mechanics::WeatherWaveProfile| match family {
        WeatherFamily::Wave => p.shards > 0,
        WeatherFamily::Burn { hero } => {
            p.shards > 0
                && p.damage > 0
                && p.damage_per_second > 0
                && p.buff_rawcode != 0
                && if hero {
                    p.hero_duration_ticks > 0
                } else {
                    p.duration_ticks > 0
                }
        }
    };
    if !eligible(state.profile) {
        return matches!(family, WeatherFamily::Wave);
    }
    let buff = |p: crate::building_mechanics::WeatherWaveProfile| match family {
        WeatherFamily::Wave => p.wave_buff_rawcode,
        WeatherFamily::Burn { .. } => p.buff_rawcode,
    };
    let damage = |profile: crate::building_mechanics::WeatherWaveProfile| match family {
        WeatherFamily::Burn { .. } => i64::from(profile.damage_per_second),
        WeatherFamily::Wave if movement.is_none() => {
            i64::from(profile.damage) * i64::from(profile.building_damage_per_10k) / 10000
        }
        WeatherFamily::Wave => i64::from(profile.damage),
    };
    !active.iter().any(|(other, w)| {
        w.team == state.team
            && eligible(w.profile)
            && buff(w.profile) == buff(state.profile)
            && tick >= w.started_tick + u64::from(w.profile.first_impact_ticks)
            && tick < w.expires_tick
            && movement.map_or_else(
                || w.profile.targets.can_target_buildings(),
                |class| w.profile.targets.can_target_unit(class),
            )
            && w.center.distance_sq(point) <= square_i32(w.profile.radius)
            && (damage(w.profile) > damage(state.profile)
                || (damage(w.profile) == damage(state.profile) && *other < id))
    })
}

// The native DPS family applies its first pulse at wave impact. Refreshes keep
// the already committed periodic clock; matching recipient buffs never stack.
fn apply_weather_burn(
    status: &mut StatusState,
    p: crate::building_mechanics::WeatherWaveProfile,
    hero: bool,
    tick: u64,
) -> i32 {
    let duration = if hero {
        p.hero_duration_ticks
    } else {
        p.duration_ticks
    };
    if p.damage == 0 || p.damage_per_second <= 0 || duration == 0 || p.buff_rawcode == 0 {
        return 0;
    }
    let id = ModifierId(p.buff_rawcode);
    let expiry = tick + u64::from(duration);
    if let Some(effect) = status.damage_over_time[..usize::from(status.damage_over_time_count)]
        .iter_mut()
        .find(|e| e.id == id && tick < e.expires_tick)
    {
        effect.damage_per_pulse = effect.damage_per_pulse.max(p.damage_per_second);
        effect.expires_tick = effect.expires_tick.max(expiry);
        return 0;
    }
    // A due expiry pulse has already resolved in the periodic phase.
    if let Some(effect) = status.damage_over_time[..usize::from(status.damage_over_time_count)]
        .iter_mut()
        .find(|e| e.id == id)
    {
        effect.damage_per_pulse = p.damage_per_second;
        effect.pulse_interval_ticks = CASTLE_FIGHT_SIMULATION_HZ as u16;
        effect.next_pulse_tick = tick + CASTLE_FIGHT_SIMULATION_HZ as u64;
        effect.expires_tick = expiry;
        effect.source = None;
        effect.final_pulse_at_expiry = false;
    } else {
        apply_timed_damage_over_time(
            status,
            id,
            p.damage_per_second,
            CASTLE_FIGHT_SIMULATION_HZ as u16,
            tick,
            expiry,
        );
        let effect = status.damage_over_time[..usize::from(status.damage_over_time_count)]
            .iter_mut()
            .find(|e| e.id == id)
            .unwrap();
        effect.final_pulse_at_expiry = false;
    }
    p.damage_per_second
}

#[cfg(test)]
mod tests;
