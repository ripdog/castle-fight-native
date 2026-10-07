use super::*;
#[cfg(test)]
mod tests;
#[cfg(test)]
use crate::MapVersion;
use crate::native_carriers::{NativeCarrierBolt, NativeCarrierState, carrier_for_version};

const RANDOM_PURPOSE_NATIVE_CARRIER: u64 = 0x4341_5252_4945_5201;

#[derive(Default)]
pub(super) struct NativeCarrierResolution {
    pub launches: usize,
    pub impacts: usize,
    pub effects: usize,
    pub invalidations: usize,
}

impl Simulation {
    pub(super) fn start_native_carrier(&mut self, building_entity: Entity) {
        let building = self.world.entity(building_entity);
        if building
            .get::<Health>()
            .is_none_or(|health| health.current <= 0)
        {
            return;
        }
        let Some(content) = building.get::<ContentIdentity>().copied() else {
            return;
        };
        let version = content.map_version;
        let rawcode = content.rawcode;
        let kind = crate::CastleFightTowerKind::from_rawcode_for_version(rawcode, version)
            .expect("building content must reference a supported version");
        if !matches!(
            kind,
            Some(
                crate::CastleFightTowerKind::ArcaneTower
                    | crate::CastleFightTowerKind::ObeliskOfLight
            )
        ) {
            return;
        }
        let profile = carrier_for_version(version).expect("registered carrier version");
        let building_id = *building.get::<SimId>().unwrap();
        let arcane_rawcode = crate::content::CastleFightTowerKind::ArcaneTower
            .definition_for_version(version)
            .unwrap()
            .rawcode;
        let per_second_per_10k = if rawcode == arcane_rawcode {
            profile.arcane_regeneration_per_second_per_10k
        } else if rawcode == profile.building_rawcode {
            profile.obelisk_regeneration_per_second_per_10k
        } else {
            return;
        };
        // Copy the launch origin/ownership before allocating the independent regeneration entity.
        let owner = building.get::<Owner>().map(|owner| owner.0);
        let team = *building.get::<Team>().unwrap();
        let position = footprint_center_point(
            *building.get::<BuildingFootprint>().unwrap(),
            self.config.navigation_cell_size,
        );
        if !self.world.iter_entities().any(|entity| matches!(entity.get::<NativeCarrierState>(), Some(NativeCarrierState::Regeneration { building, .. }) if *building == building_id)) {
            let id = self.allocate_id();
            self.world.spawn((id, NativeCarrierState::Regeneration { building: building_id, map_version: version, per_second_per_10k, remainder: 0 }));
        }
        if rawcode != profile.building_rawcode {
            return;
        }
        if self.world.iter_entities().any(|entity| matches!(entity.get::<NativeCarrierState>(), Some(NativeCarrierState::Carrier { building, .. }) if *building == building_id)) { return }
        let state = NativeCarrierState::Carrier {
            building: building_id,
            owner,
            team,
            position,
            map_version: version,
            ready_tick: self.next_tick,
            sequence: 0,
        };
        let id = self.allocate_id();
        self.world.spawn((id, state));
    }

    pub(super) fn stop_native_carrier(&mut self, building_id: SimId) {
        let removals: Vec<_> = self.world.iter_entities().filter_map(|entity| {
            matches!(entity.get::<NativeCarrierState>(), Some(NativeCarrierState::Carrier { building, .. } | NativeCarrierState::Regeneration { building, .. }) if *building == building_id).then_some(entity.id())
        }).collect();
        for entity in removals {
            self.world.despawn(entity);
        }
    }

    pub(super) fn advance_native_regeneration(&mut self) {
        let states: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                let NativeCarrierState::Regeneration {
                    building,
                    map_version,
                    per_second_per_10k,
                    remainder,
                } = *entity.get::<NativeCarrierState>()?
                else {
                    return None;
                };
                Some((
                    entity.id(),
                    building,
                    map_version,
                    per_second_per_10k,
                    remainder,
                ))
            })
            .collect();
        for (entity, building, map_version, per_second_per_10k, remainder) in states {
            let target = self.world.iter_entities().find_map(|entity| {
                (entity.get::<SimId>() == Some(&building)
                    && entity.get::<Health>().is_some_and(|h| h.current > 0))
                .then_some(entity.id())
            });
            let Some(target) = target else {
                self.world.despawn(entity);
                continue;
            };
            let denominator = 10_000 * CASTLE_FIGHT_SIMULATION_HZ as u32;
            let total = remainder + per_second_per_10k;
            let mut health = self.world.get_mut::<Health>(target).unwrap();
            health.current = health
                .current
                .saturating_add((total / denominator) as i32)
                .min(health.max);
            self.world
                .entity_mut(entity)
                .insert(NativeCarrierState::Regeneration {
                    building,
                    map_version,
                    per_second_per_10k,
                    remainder: total % denominator,
                });
        }
    }

    /// Native Barrage has a separate mask/radius/capacity; it is not splash,
    /// and its missiles do not inherit ordinary-attack on-hit procs or missile art.
    pub(super) fn launch_native_barrage(
        &mut self,
        intent: &AttackIntent,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        health: &[i32],
        tick: u64,
    ) -> usize {
        let entity = match intent.source {
            AttackSourceIndex::Building(index) => buildings[index].entity,
            AttackSourceIndex::Unit(index) => units[index].entity,
        };
        let Some(content) = self.world.get::<ContentIdentity>(entity) else {
            return 0;
        };
        let version = content.map_version;
        let Some(profile) = crate::native_carriers::barrage_for_source(version, content.rawcode)
            .expect("registered Barrage version")
        else {
            return 0;
        };
        let mut candidates: Vec<_> = units
            .iter()
            .enumerate()
            .filter(|(index, unit)| {
                health[*index] > 0
                    && unit.id != intent.target_id
                    && unit.team != intent.source_team
                    && unit.visible_to(intent.source_team, tick)
                    && !unit.classifications.invulnerable
                    && profile.targets.can_target_unit(unit.movement_class)
                    && intent.source_position.distance_sq(unit.position)
                        <= square_i32(profile.range)
            })
            .collect();
        candidates.sort_unstable_by_key(|(_, unit)| {
            (intent.source_position.distance_sq(unit.position), unit.id)
        });
        // Modern native Aroc uses the actual weapon damage, not Efk1/Efk2 as a
        // damage amount/budget. In particular, zero Efk1 never makes neutral arrows.
        let damage = intent.attack.damage;
        if damage <= 0 || profile.range <= 0 {
            return 0;
        }
        let count = candidates.len().min(profile.additional_targets());
        for (_, target) in candidates.into_iter().take(count) {
            self.spawn_native_bolt(NativeCarrierBolt {
                source: intent.source_id,
                visual_source: intent.source_id,
                source_team: intent.source_team,
                target: target.id,
                ability: profile.ability,
                map_version: version,
                damage,
                attack_damage_type: Some(intent.damage_type),
                launch_position: intent.source_position,
                launch_tick: tick,
                position: intent.source_position,
                position_tick: tick,
                impact_tick: tick
                    + projectile_travel_ticks(
                        intent.source_position.distance_sq(target.position),
                        profile.speed_per_tick,
                    ),
            });
        }
        count
    }

    fn spawn_native_bolt(&mut self, bolt: NativeCarrierBolt) {
        let id = self.allocate_id();
        self.world.spawn((id, NativeCarrierState::Bolt(bolt)));
    }

    pub(super) fn resolve_native_carriers(
        &mut self,
        context: TargetProjectileContext<'_>,
    ) -> NativeCarrierResolution {
        let mut result = NativeCarrierResolution::default();
        let TargetProjectileContext {
            units,
            buildings,
            unit_health,
            building_health,
            positions,
            attackers_this_tick,
            next_defense_alerts,
            completed_tick,
            ..
        } = context;
        let mut states: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                Some((
                    entity.id(),
                    *entity.get::<SimId>()?,
                    *entity.get::<NativeCarrierState>()?,
                ))
            })
            .collect();
        states.sort_unstable_by_key(|(_, id, _)| *id);
        // Remove dead sources before any impact. In-flight spell damage still resolves, but the
        // damage listener cannot find A000 on a removed dummy. A surviving building is not enough.
        for (entity, _, state) in &states {
            if let NativeCarrierState::Carrier { building, .. } = state
                && find_building_index(buildings, *building).is_none_or(|i| building_health[i] <= 0)
            {
                self.world.despawn(*entity);
            }
        }
        for (entity, id, state) in states {
            match state {
                NativeCarrierState::Regeneration { .. } => {}
                NativeCarrierState::Carrier {
                    building,
                    owner,
                    team,
                    position,
                    map_version,
                    ready_tick,
                    sequence,
                } => {
                    if self.world.get_entity(entity).is_err() || completed_tick < ready_tick {
                        continue;
                    }
                    let profile = carrier_for_version(map_version).unwrap();
                    let candidates: Vec<_> = units
                        .iter()
                        .enumerate()
                        .filter(|(i, unit)| {
                            unit_health[*i] > 0
                                && unit.team != team
                                && unit.visible_to(team, completed_tick)
                                && !unit.classifications.spell_immune
                                && !unit.classifications.invulnerable
                                && profile.targets.can_target_unit(unit.movement_class)
                                && !unit.status.damage_over_time
                                    [..usize::from(unit.status.damage_over_time_count)]
                                    .iter()
                                    .any(|buff| {
                                        buff.id.0 == profile.buff.0
                                            && completed_tick < buff.expires_tick
                                    })
                                && position.distance_sq(unit.position) <= square_i32(profile.range)
                        })
                        .collect();
                    if candidates.is_empty() {
                        continue;
                    }
                    let selected = deterministic_random(
                        self.config.match_seed,
                        completed_tick,
                        id,
                        RANDOM_PURPOSE_NATIVE_CARRIER,
                        sequence,
                    ) % candidates.len() as u64;
                    let (_, target) = candidates[selected as usize];
                    self.spawn_native_bolt(NativeCarrierBolt {
                        source: id,
                        visual_source: building,
                        source_team: team,
                        target: target.id,
                        ability: profile.ability,
                        map_version,
                        damage: profile.initial_damage,
                        attack_damage_type: None,
                        launch_position: position,
                        launch_tick: completed_tick,
                        position,
                        position_tick: completed_tick,
                        impact_tick: completed_tick
                            + projectile_travel_ticks(
                                position.distance_sq(target.position),
                                profile.speed_per_tick,
                            ),
                    });
                    result.launches += 1;
                    self.world
                        .entity_mut(entity)
                        .insert(NativeCarrierState::Carrier {
                            building,
                            owner,
                            team,
                            position,
                            map_version,
                            ready_tick: completed_tick + profile.cooldown_ticks,
                            sequence: sequence.checked_add(1).expect("carrier sequence overflow"),
                        });
                }
                NativeCarrierState::Bolt(mut bolt) => {
                    let Some(index) =
                        find_unit_index(units, bolt.target).filter(|i| unit_health[*i] > 0)
                    else {
                        self.world.despawn(entity);
                        result.invalidations += 1;
                        continue;
                    };
                    let speed = if bolt.attack_damage_type.is_some() {
                        crate::native_carriers::barrage_for_ability(bolt.map_version, bolt.ability)
                            .unwrap()
                            .expect("registered in-flight Barrage ability")
                            .speed_per_tick
                    } else {
                        carrier_for_version(bolt.map_version)
                            .unwrap()
                            .speed_per_tick
                    };
                    let elapsed = completed_tick
                        .checked_sub(bolt.position_tick)
                        .expect("native homing position must not lie in the future");
                    let budget = i32::try_from(
                        u64::from(u32::try_from(speed).expect("positive native missile speed"))
                            .saturating_mul(elapsed),
                    )
                    .unwrap_or(i32::MAX);
                    let destination = positions[index];
                    if bolt.position.distance_sq(destination) > square_i32(budget) {
                        bolt.position = bolt.position.step_towards(destination, budget);
                        bolt.position_tick = completed_tick;
                        bolt.impact_tick = completed_tick
                            + projectile_travel_ticks(
                                bolt.position.distance_sq(destination),
                                speed,
                            );
                        self.world
                            .entity_mut(entity)
                            .insert(NativeCarrierState::Bolt(bolt));
                        continue;
                    }
                    self.world.despawn(entity);
                    result.impacts += 1;
                    // Avul acquired during flight blocks both ordinary arrows and native
                    // damage/buff/cleanse; spell immunity blocks only the spell family below.
                    if units[index].classifications.invulnerable {
                        continue;
                    }
                    if let Some(damage_type) = bolt.attack_damage_type {
                        // Independent arrows are ordinary attack damage, not spell damage.
                        let evaded = units[index].passive_effects.iter().any(|effect| {
                            let PassiveUnitEffect::Evasion(profile) = effect else {
                                return false;
                            };
                            deterministic_random(
                                self.config.match_seed,
                                completed_tick,
                                id,
                                RANDOM_PURPOSE_ATTACK_PROC ^ u64::from(profile.ability.0),
                                0,
                            ) % u64::from(ATTACK_PROC_CHANCE_SCALE)
                                < u64::from(profile.chance_per_10k)
                        });
                        if !evaded {
                            let damage = resolve_directed_projectile_defense(
                                TargetIndex::Unit(index),
                                id,
                                bolt.damage,
                                damage_type,
                                completed_tick,
                                self.config.match_seed,
                                units,
                            )
                            .damage;
                            if apply_damage_to_target(
                                TargetIndex::Unit(index),
                                bolt.source,
                                damage,
                                damage_type,
                                completed_tick,
                                self.debug_buildings_invulnerable,
                                DamageTargetState {
                                    damage_rules: self.combat_rules.damage_rules,
                                    units,
                                    buildings,
                                    unit_positions: positions,
                                    unit_health,
                                    building_health,
                                    attackers_this_tick,
                                    next_defense_alerts,
                                    navigation_cell_size: self.config.navigation_cell_size,
                                },
                            )
                            .is_some()
                            {
                                result.effects += 1;
                            }
                        }
                    } else if !units[index].classifications.spell_immune {
                        let damage = spell_damage_after_defend(
                            units[index],
                            self.combat_rules
                                .damage_rules
                                .apply_spell(bolt.damage, units[index].armor.armor_type),
                            completed_tick,
                        );
                        unit_health[index] = unit_health[index].saturating_sub(damage.max(0));
                        if damage > 0 {
                            result.effects += 1;
                        }
                        let source_has_ability = self.world.iter_entities().any(|entity| {
                            entity.get::<SimId>() == Some(&bolt.source)
                                && matches!(
                                    entity.get::<NativeCarrierState>(),
                                    Some(NativeCarrierState::Carrier { .. })
                                )
                        });
                        if damage > 0 && source_has_ability {
                            let profile = carrier_for_version(bolt.map_version).unwrap();
                            // Hex/Defend projections suppress passives; they are not the
                            // mutable baseline from which permanent grants may be removed.
                            {
                                let mut baseline = self
                                    .world
                                    .get_mut::<PassiveUnitEffects>(units[index].entity)
                                    .expect("unit passive baseline");
                                cleanse_native_effects(
                                    &mut units[index],
                                    &mut unit_health[index],
                                    &mut baseline,
                                    profile,
                                );
                            }
                            self.cleanse_building_spell_controls(
                                units[index].id,
                                units[index].position,
                                &profile.removed_persistent_abilities,
                            );
                            self.project_building_spell_control(&mut units[index]);
                        }
                        // Phoenix Fire applies its native buff after dealing initial damage;
                        // the script cleanse occurs inside that damage event. Zero DPS still has
                        // a native identity/lifetime, independent of permanently granted abilities.
                        let profile = carrier_for_version(bolt.map_version).unwrap();
                        apply_timed_damage_over_time(
                            &mut units[index].status,
                            ModifierId(profile.buff.0),
                            0,
                            1,
                            completed_tick,
                            completed_tick + profile.buff_duration_ticks,
                        );
                    }
                }
            }
        }
        result
    }
}

/// Full native buff removal must not reset AI/cast delays, or unrelated permanent grants.
/// Permanent ability modifiers are identity-scoped, rather than inferred from their sign.
fn cleanse_native_effects(
    unit: &mut UnitSnapshot,
    health: &mut i32,
    passive_baseline: &mut PassiveUnitEffects,
    profile: &crate::native_carriers::NativeCarrierProfile,
) {
    let status = &mut unit.status;
    status.stunned_until_tick = 0;
    // Finite-lifetime modifiers and retained native buff identities are buffs.
    // Permanent grants carry the granting ability identity and use the exact removal list.
    let keep_modifier = |id: ModifierId, expires: u64| {
        expires == u64::MAX
            && profile.native_buff_ids.binary_search(&id.0).is_err()
            && !profile
                .removed_persistent_abilities
                .contains(&AbilityId(id.0))
    };
    retain_modifiers(
        &mut status.movement_modifiers,
        &mut status.movement_modifier_count,
        |modifier| keep_modifier(modifier.id, modifier.expires_tick),
    );
    retain_modifiers(
        &mut status.attack_speed_modifiers,
        &mut status.attack_speed_modifier_count,
        |modifier| keep_modifier(modifier.id, modifier.expires_tick),
    );
    retain_modifiers(
        &mut status.armor_modifiers,
        &mut status.armor_modifier_count,
        |modifier| keep_modifier(modifier.id, modifier.expires_tick),
    );
    status.damage_over_time = Default::default();
    status.damage_over_time_count = 0;
    passive_baseline.retain(|effect| {
        let ability = match effect {
            PassiveUnitEffect::Bash(p) => Some(p.ability),
            PassiveUnitEffect::CriticalStrike(p) => Some(p.ability),
            PassiveUnitEffect::Evasion(p) => Some(p.ability),
            PassiveUnitEffect::Defend(p) => Some(p.ability),
            PassiveUnitEffect::TriggeredSpellProc(p) => Some(p.ability),
            PassiveUnitEffect::BurningOil(p) => Some(p.ability),
            PassiveUnitEffect::Cleave(p) => Some(p.ability),
            PassiveUnitEffect::Aura(p) => Some(p.ability),
            PassiveUnitEffect::SpellResistance(p) => Some(p.ability),
            PassiveUnitEffect::Feedback(p) => Some(p.ability),
            PassiveUnitEffect::SplashFalloff(_) => None,
        };
        ability.is_none_or(|id| !profile.removed_persistent_abilities.contains(&id))
    });
    if status.permanent_holy_health_bonus
        && profile
            .removed_persistent_abilities
            .contains(&AbilityId(u32::from_be_bytes(*b"A03D")))
    {
        status.permanent_holy_health_bonus = false;
        unit.health_max = unit
            .health_max
            .saturating_sub(profile.holy_health_bonus)
            .max(1);
        *health = (*health).min(unit.health_max);
    }
}

fn retain_modifiers<T: Copy + Default>(
    modifiers: &mut [T],
    count: &mut u8,
    mut keep: impl FnMut(&T) -> bool,
) {
    let active = usize::from(*count);
    let mut write = 0;
    for read in 0..active {
        if keep(&modifiers[read]) {
            modifiers[write] = modifiers[read];
            write += 1;
        }
    }
    modifiers[write..active].fill(T::default());
    *count = u8::try_from(write).expect("modifier count fits u8");
}
