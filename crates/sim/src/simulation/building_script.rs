//! Desert building scripts: Beacon replenish, Peyote illusions and the global Sandstorm.
use super::*;
use crate::MapVersion;
use crate::components::{
    BuildingChild, BuildingScriptProfile, IllusionState, NativeAction, SandstormState,
};

impl Simulation {
    pub(super) fn cast_building_script(
        &mut self,
        source: AbilitySourceSnapshot,
        profile: BuildingScriptProfile,
        sequence: u64,
        units: &mut [UnitSnapshot],
    ) -> usize {
        match profile.child {
            BuildingChild::Replenish {
                life,
                mana,
                severe_missing_life,
                mana_below_per_10k,
            } => self.cast_replenish(
                source,
                profile.ability,
                sequence,
                (life, mana, severe_missing_life, mana_below_per_10k),
                units,
            ),
            BuildingChild::Hallucination {
                illusion_ability,
                illusion_ticks,
                damage_dealt_per_10k,
                damage_taken_per_10k,
                excluded_ability,
            } => self.cast_hallucination(
                source,
                profile.ability,
                sequence,
                (
                    illusion_ability,
                    illusion_ticks,
                    damage_dealt_per_10k,
                    damage_taken_per_10k,
                    excluded_ability,
                ),
                units,
            ),
            BuildingChild::ThunderCharge(thunder) => {
                self.add_thunder_charge(source.team, thunder);
                1
            }
            BuildingChild::ManaBurn {
                cap,
                priority_above,
                damage_per_mana,
                excluded_ability,
            } => self.cast_mana_burn(
                source,
                profile.ability,
                sequence,
                (cap, priority_above, damage_per_mana, excluded_ability),
                units,
            ),
            BuildingChild::Sandstorm {
                debuff,
                pulse_ability,
                pulse_damage,
                pulse_interval_ticks,
                duration_ticks,
                targets,
            } => {
                let mut applied = 0;
                for unit in units.iter_mut() {
                    if unit.team != source.team
                        && !unit.classifications.invulnerable
                        && debuff.targets.can_target_unit(unit.target_movement_class())
                        && apply_ability_effect_to_unit(
                            unit,
                            AbilityEffect::NativeDebuff(debuff),
                            source.team,
                            self.next_tick,
                            self.combat_rules.damage_rules,
                        )
                    {
                        applied += 1;
                    }
                }
                let id = self.allocate_id();
                self.world.spawn((
                    id,
                    NativeAction::Sandstorm(SandstormState {
                        source: source.id,
                        team: source.team,
                        ability: pulse_ability,
                        damage: pulse_damage,
                        targets,
                        interval_ticks: pulse_interval_ticks,
                        next_pulse_tick: self.next_tick + u64::from(pulse_interval_ticks),
                        expires_tick: self.next_tick + u64::from(duration_ticks),
                    }),
                ));
                applied + 1
            }
        }
    }

    /// Withering Touch: a random hostile combat body, preferring high mana, loses mana and is
    /// combusted for a multiple of the mana actually burned (sonic damage, no spell defenses).
    fn cast_mana_burn(
        &self,
        source: AbilitySourceSnapshot,
        ability: AbilityId,
        sequence: u64,
        (cap, priority_above, damage_per_mana, excluded): (i32, i32, i32, AbilityId),
        units: &mut [UnitSnapshot],
    ) -> usize {
        let version = source.map_version.unwrap_or(MapVersion::CASTLE_FIGHT_9_27);
        let eligible = |unit: &UnitSnapshot, above: i32| {
            unit.team != source.team
                && unit.health > 0
                && unit.classifications.combat_sapper
                && !unit.classifications.invulnerable
                && unit.mana_current.is_some_and(|mana| mana > above)
                && !crate::content::unit_has_ability_for_version(unit.rawcode, excluded, version)
        };
        let pick = |above: i32| {
            units
                .iter()
                .enumerate()
                .filter(|(_, unit)| eligible(unit, above))
                .min_by_key(|(_, unit)| {
                    (
                        deterministic_ability_target_rank(
                            self.config.match_seed,
                            source.id,
                            ability,
                            sequence,
                            unit.id,
                        ),
                        unit.id,
                    )
                })
                .map(|(index, _)| index)
        };
        let Some(index) = pick(priority_above).or_else(|| pick(0)) else {
            return 0;
        };
        let unit = &mut units[index];
        let burned = unit.mana_current.unwrap_or(0).min(cap);
        unit.mana_current = unit.mana_current.map(|mana| mana - burned);
        let damage = self.combat_rules.damage_rules.apply_attack_with_armor_per_100(
            burned.saturating_mul(damage_per_mana),
            DamageType::Normal,
            unit.armor.armor_type,
            0,
        );
        let damage = scale_damage_per_10k(
            native_kill_effects::damage_after_native_incoming(unit, damage, self.next_tick),
            unit.snow_damage_taken_per_10k,
        );
        let before = unit.health;
        damage_thresholds::observe_damage_event(unit, before, damage);
        death_healing::commit_health_from(units, index, before.saturating_sub(damage), Some(source.id));
        1
    }

    /// Random ally from the first non-empty tier: badly wounded, wounded, then partial mana.
    fn cast_replenish(
        &self,
        source: AbilitySourceSnapshot,
        ability: AbilityId,
        sequence: u64,
        (life, mana, severe_missing_life, mana_below_per_10k): (i32, i32, i32, u16),
        units: &mut [UnitSnapshot],
    ) -> usize {
        let version = source.map_version.unwrap_or(MapVersion::CASTLE_FIGHT_9_27);
        let mana_fraction = |unit: &UnitSnapshot| -> Option<(i32, i32)> {
            let current = unit.mana_current?;
            let maximum = unit
                .spellcasting
                .map(|s| s.mana.maximum)
                .filter(|m| *m > 0)
                .or_else(|| {
                    crate::content::native_mana_maximum_for_version(version, unit.rawcode)
                })?;
            (maximum > 0).then_some((current, maximum))
        };
        let tier = |unit: &UnitSnapshot| -> Option<u8> {
            if unit.team != source.team || unit.health <= 0 || !unit.classifications.combat_sapper {
                return None;
            }
            if unit.health_max - severe_missing_life > unit.health {
                Some(0)
            } else if unit.health_max > unit.health {
                Some(1)
            } else if mana_fraction(unit).is_some_and(|(current, maximum)| {
                current > 0
                    && i64::from(current) * 10_000
                        < i64::from(maximum) * i64::from(mana_below_per_10k)
            }) {
                Some(2)
            } else {
                None
            }
        };
        let best = units
            .iter()
            .enumerate()
            .filter_map(|(index, unit)| Some((tier(unit)?, index)))
            .min_by_key(|(tier, _)| *tier)
            .map(|(tier, _)| tier);
        let Some(best) = best else {
            return 0;
        };
        let Some(index) = units
            .iter()
            .enumerate()
            .filter(|(_, unit)| tier(unit) == Some(best))
            .min_by_key(|(_, unit)| {
                (
                    deterministic_ability_target_rank(
                        self.config.match_seed,
                        source.id,
                        ability,
                        sequence,
                        unit.id,
                    ),
                    unit.id,
                )
            })
            .map(|(index, _)| index)
        else {
            return 0;
        };
        let maximum = mana_fraction(&units[index]).map(|(_, maximum)| maximum);
        let unit = &mut units[index];
        unit.health = unit.health.saturating_add(life).min(unit.health_max);
        if let (Some(current), Some(maximum)) = (unit.mana_current, maximum) {
            unit.mana_current = Some(current.saturating_add(mana).min(maximum));
        }
        1
    }

    /// Item-class illusion of a random vulnerable enemy combat body, owned by the casting player.
    fn cast_hallucination(
        &mut self,
        source: AbilitySourceSnapshot,
        ability: AbilityId,
        sequence: u64,
        (illusion_ability, illusion_ticks, damage_dealt, damage_taken, excluded): (
            AbilityId,
            u16,
            u16,
            u16,
            AbilityId,
        ),
        units: &mut [UnitSnapshot],
    ) -> usize {
        let version = source.map_version.unwrap_or(MapVersion::CASTLE_FIGHT_9_27);
        let Some(index) = units
            .iter()
            .enumerate()
            .filter(|(_, unit)| {
                unit.team != source.team
                    && unit.health > 0
                    && unit.classifications.combat_sapper
                    && !unit.classifications.invulnerable
                    && !crate::content::unit_has_ability_for_version(
                        unit.rawcode,
                        excluded,
                        version,
                    )
            })
            .min_by_key(|(_, unit)| {
                (
                    deterministic_ability_target_rank(
                        self.config.match_seed,
                        source.id,
                        ability,
                        sequence,
                        unit.id,
                    ),
                    unit.id,
                )
            })
            .map(|(index, _)| index)
        else {
            return 0;
        };
        // The script does nothing at all when a shield absorbs the cast.
        if self.check_negative_building_shield(&mut units[index], version, source.id, sequence) {
            return 0;
        }
        let target = units[index];
        let Some(owner) = self
            .world
            .get::<Owner>(self.source_entity(source, units))
            .map(|owner| owner.0)
        else {
            return 0;
        };
        let Some(mut definition) = self
            .world
            .get::<ResurrectionProfile>(target.entity)
            .map(|profile| profile.0)
        else {
            return 0;
        };
        definition.template.attack = *self.world.get::<AttackProfile>(target.entity).unwrap();
        definition.properties.classifications.illusion = true;
        definition.properties.classifications.summoned = false;
        definition.properties.corpse = None;
        definition.properties.passive_effects.retain(|effect| {
            !matches!(
                effect,
                PassiveUnitEffect::LavaSpawn(_)
                    | PassiveUnitEffect::MirrorImage(_)
                    | PassiveUnitEffect::TriggeredSpellProc(_)
                    | PassiveUnitEffect::Feedback(_)
                    | PassiveUnitEffect::Bash(_)
            )
        });
        definition.spellcasting = None;
        definition.additional_abilities = None;
        let id = self.spawn_resolved_unit_unchecked(
            Some(owner),
            UnitSpawn::from_template(source.team, target.position, definition.template),
            definition,
        );
        let entity = self
            .world
            .iter_entities()
            .find(|entity| entity.get::<SimId>() == Some(&id))
            .expect("spawned illusion")
            .id();
        self.world.entity_mut(entity).insert((
            Health {
                current: target.health,
                max: target.health_max,
            },
            StatusState {
                illusion: Some(IllusionState {
                    source: target.id,
                    ability: illusion_ability,
                    expires_tick: self.next_tick + u64::from(illusion_ticks),
                    damage_dealt_per_10k: damage_dealt,
                    damage_taken_per_10k: damage_taken,
                }),
                ..Default::default()
            },
        ));
        1
    }

    fn source_entity(&self, source: AbilitySourceSnapshot, units: &[UnitSnapshot]) -> Entity {
        match source.source {
            AbilitySourceIndex::Unit(index) => units[index].entity,
            AbilitySourceIndex::Building(_) => self
                .world
                .iter_entities()
                .find(|entity| entity.get::<SimId>() == Some(&source.id))
                .expect("building ability source")
                .id(),
        }
    }

    pub(super) fn advance_sandstorm(
        &mut self,
        entity: Entity,
        mut state: SandstormState,
        units: &mut [UnitSnapshot],
    ) {
        let tick = self.next_tick;
        if tick >= state.next_pulse_tick && state.next_pulse_tick < state.expires_tick {
            for index in 0..units.len() {
                let unit = &units[index];
                if unit.team != state.team
                    && unit.health > 0
                    && !unit.classifications.invulnerable
                    && state.targets.can_target_unit(unit.target_movement_class())
                {
                    self.apply_source_ability_to_unit(
                        state.source,
                        state.team,
                        index,
                        AbilityEffect::Damage {
                            amount: state.damage,
                        },
                        units,
                    );
                }
            }
            state.next_pulse_tick += u64::from(state.interval_ticks);
        }
        if state.next_pulse_tick >= state.expires_tick {
            self.world.despawn(entity);
        } else if let Some(mut action) = self.world.get_mut::<NativeAction>(entity) {
            *action = NativeAction::Sandstorm(state);
        }
    }
}
