//! Multi-ability source configuration and scheduling adapters. All slots share one mana pool.
use super::*;

impl Simulation {
    /// Configure additional automatic abilities from validated content. This is an engine
    /// setup API, not a player command. Reconfiguration preserves cooldowns/sequences by ID;
    /// changing an existing ID's immutable definition is rejected.
    pub fn configure_additional_automatic_abilities(
        &mut self,
        source: SimId,
        profiles: &[AutomaticAbilityProfile],
    ) -> Result<(), AbilityConfigurationError> {
        let entity = self
            .world
            .iter_entities()
            .find_map(|entity| (entity.get::<SimId>() == Some(&source)).then_some(entity.id()))
            .ok_or(AbilityConfigurationError::MissingSpellcaster)?;
        let primary = self
            .world
            .entity(entity)
            .get::<SpellcastingProfile>()
            .copied()
            .ok_or(AbilityConfigurationError::MissingSpellcaster)?;
        let previous = self
            .world
            .entity(entity)
            .get::<AdditionalAutomaticAbilities>()
            .copied();
        if profiles.len() >= crate::MAX_AUTOMATIC_ABILITIES {
            return Err(AbilityConfigurationError::CapacityExceeded);
        }
        let is_building = self.world.entity(entity).contains::<BuildingFootprint>();
        let mut entries = Vec::with_capacity(profiles.len());
        for profile in profiles {
            if is_building
                && matches!(
                    profile.effect,
                    AbilityEffect::HolyAid {
                        resurrection_count: 1..,
                        ..
                    }
                )
            {
                return Err(AbilityConfigurationError::UnsupportedDelayedSource);
            }
            if profile.id == primary.ability.id {
                return Err(AbilityConfigurationError::DuplicateAbility(profile.id));
            }
            if previous
                .as_ref()
                .and_then(|set| set.get(profile.id))
                .is_some_and(|entry| entry.profile != *profile)
            {
                return Err(AbilityConfigurationError::DefinitionChanged(profile.id));
            }
            validate_spellcasting_profile(SpellcastingProfile {
                mana: primary.mana,
                ability: *profile,
            });
            let state = previous
                .as_ref()
                .and_then(|set| set.get(profile.id))
                .map_or(
                    AutomaticAbilityState {
                        ready_tick: self.next_tick,
                        cast_sequence: 0,
                        autocast_enabled: true,
                        manual_cast_requested: false,
                    },
                    |entry| entry.state,
                );
            let secondary_resurrection = previous
                .as_ref()
                .and_then(|set| set.get(profile.id))
                .map_or_else(SecondaryResurrectionState::default, |entry| {
                    entry.secondary_resurrection
                });
            entries.push(AutomaticAbilityInstance {
                profile: *profile,
                state,
                secondary_resurrection,
            });
        }
        // Construct/validate before mutating the source so rejection is atomic.
        let set = AdditionalAutomaticAbilities::try_from(entries)?;
        if let Some(mut definition) = self
            .world
            .entity_mut(entity)
            .get_mut::<ResurrectionProfile>()
        {
            definition.0.additional_abilities = (!profiles.is_empty())
                .then(|| AdditionalAutomaticAbilityDefinitions::from_runtime(set));
        }
        if profiles.is_empty() {
            self.world
                .entity_mut(entity)
                .remove::<AdditionalAutomaticAbilities>();
        } else {
            self.world.entity_mut(entity).insert(set);
        }
        Ok(())
    }

    pub(super) fn additional_ability_sources(
        &self,
        buildings: &[BuildingSnapshot],
        units: &[UnitSnapshot],
    ) -> Vec<AbilitySourceSnapshot> {
        let mut result = Vec::new();
        for (index, unit) in units
            .iter()
            .enumerate()
            .filter(|(_, unit)| unit.spellcasting.is_some() && !unit.abilities_disabled)
        {
            self.append_additional_ability_sources(
                &mut result,
                unit.entity,
                AbilitySourceSnapshot {
                    map_version: unit.map_version,
                    source: AbilitySourceIndex::Unit(index),
                    id: unit.id,
                    team: unit.team,
                    origin: AbilitySourceOrigin::Unit(unit.position),
                    health: unit.health,
                    stunned_until_tick: unit.status.stunned_until_tick,
                    spellcasting: unit.spellcasting,
                    mana_current: unit.mana_current,
                    ability_state: unit.ability_state,
                },
                unit.orders_suspended || unit.status.is_casting(self.next_tick),
            );
        }
        for (index, building) in buildings
            .iter()
            .enumerate()
            .filter(|(_, building)| building.spellcasting.is_some())
        {
            self.append_additional_ability_sources(
                &mut result,
                building.entity,
                AbilitySourceSnapshot {
                    map_version: building.map_version,
                    source: AbilitySourceIndex::Building(index),
                    id: building.id,
                    team: building.team,
                    origin: AbilitySourceOrigin::Building(building.footprint),
                    health: building.health,
                    stunned_until_tick: building
                        .status
                        .map_or(0, |status| status.stunned_until_tick),
                    spellcasting: building.spellcasting,
                    mana_current: building.mana_current,
                    ability_state: building.ability_state,
                },
                false,
            );
        }
        result
    }

    fn append_additional_ability_sources(
        &self,
        result: &mut Vec<AbilitySourceSnapshot>,
        entity: Entity,
        source: AbilitySourceSnapshot,
        orders_suspended: bool,
    ) {
        let Some(set) = self
            .world
            .entity(entity)
            .get::<AdditionalAutomaticAbilities>()
        else {
            return;
        };
        let primary = source
            .spellcasting
            .expect("additional ability source needs a mana profile");
        for entry in set.iter() {
            if orders_suspended && !entry.profile.effect.ignores_order_interruptions() {
                continue;
            }
            result.push(AbilitySourceSnapshot {
                spellcasting: Some(SpellcastingProfile {
                    mana: primary.mana,
                    ability: entry.profile,
                }),
                ability_state: Some(entry.state),
                ..source
            });
        }
    }

    pub(super) fn select_ability_source_slot(
        &self,
        mut source: AbilitySourceSnapshot,
        entity: Entity,
        ability: AutomaticAbilityProfile,
    ) -> Option<AbilitySourceSnapshot> {
        let primary = source.spellcasting?;
        if primary.ability == ability {
            return Some(source);
        }
        let entity = self.world.entity(entity);
        let entry = entity
            .get::<AdditionalAutomaticAbilities>()?
            .get(ability.id)?;
        if entry.profile != ability {
            return None;
        }
        source.spellcasting = Some(SpellcastingProfile {
            mana: primary.mana,
            ability,
        });
        source.ability_state = Some(entry.state);
        Some(source)
    }

    pub(super) fn commit_additional_ability_state(
        &mut self,
        entity: Entity,
        ability: AbilityId,
        state: AutomaticAbilityState,
    ) {
        let mut entity = self.world.entity_mut(entity);
        let mut set = entity
            .get_mut::<AdditionalAutomaticAbilities>()
            .expect("validated additional ability source");
        set.get_mut(ability)
            .expect("validated additional ability slot")
            .state = state;
    }
}

pub(super) fn validate_additional_automatic_definitions(
    primary: Option<SpellcastingProfile>,
    definitions: Option<AdditionalAutomaticAbilityDefinitions>,
) -> Result<(), AbilityConfigurationError> {
    let Some(definitions) = definitions else {
        return Ok(());
    };
    for (index, profile) in definitions.iter().enumerate() {
        let primary = primary.ok_or(AbilityConfigurationError::MissingSpellcaster)?;
        if profile.id == primary.ability.id
            || definitions
                .iter()
                .take(index)
                .any(|previous| previous.id == profile.id)
        {
            return Err(AbilityConfigurationError::DuplicateAbility(profile.id));
        }
        validate_spellcasting_profile(SpellcastingProfile {
            mana: primary.mana,
            ability: profile,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ability(id: u32, cost: i32, damage: i32, cooldown: u16) -> AutomaticAbilityProfile {
        AutomaticAbilityProfile {
            id: AbilityId(id),
            mana_cost: cost,
            cooldown_ticks: cooldown,
            range: 10 * SUBUNITS_PER_WORLD_UNIT,
            target_policy: AbilityTargetPolicy::RandomEnemyUnit,
            effect: AbilityEffect::Damage { amount: damage },
        }
    }
    fn unit(team: u8, x: i32) -> UnitSpawn {
        UnitSpawn {
            team: Team(team),
            position: SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 1000,
            attack: AttackProfile {
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 100,
                delivery: AttackDelivery::Melee,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        }
    }
    fn setup(
        workers: usize,
        mana: i32,
        additional: &[AutomaticAbilityProfile],
    ) -> (Simulation, SimId, SimId) {
        let mut sim = Simulation::new(SimulationConfig::default(), workers);
        let source = sim.spawn_unit_with_spellcasting(
            unit(0, 40),
            SpellcastingProfile {
                mana: crate::components::ManaProfile {
                    maximum: mana,
                    starting: mana,
                    regen_per_tick_per_10k: 0,
                },
                ability: ability(30, 4, 3, 4),
            },
        );
        sim.configure_additional_automatic_abilities(source, additional)
            .unwrap();
        let target = sim.spawn_unit(unit(1, 45));
        (sim, source, target)
    }

    #[test]
    fn spell_resistance_mitigates_spells_not_ordinary_attacks_even_with_magic_attack_damage() {
        for damage_type in [DamageType::Normal, DamageType::Magic] {
            for (factor, spell_damage) in [(10_000, 20), (5_000, 10)] {
                let mut sim = Simulation::new(SimulationConfig::default(), 1);
                let mut source = unit(0, 40);
                source.attack.damage = 20;
                source.attack.range = 100 * SUBUNITS_PER_WORLD_UNIT;
                source.attack.acquisition_range = source.attack.range;
                sim.spawn_unit_with_properties_and_spellcasting(
                    source,
                    UnitGameplayProperties {
                        damage_type,
                        ..UnitGameplayProperties::default()
                    },
                    SpellcastingProfile {
                        mana: crate::components::ManaProfile {
                            maximum: 100,
                            starting: 100,
                            regen_per_tick_per_10k: 0,
                        },
                        ability: ability(1, 1, 20, 100),
                    },
                );
                let target = sim.spawn_unit_with_properties(
                    unit(1, 45),
                    UnitGameplayProperties {
                        passive_effects: PassiveUnitEffects::single(
                            PassiveUnitEffect::SpellResistance(
                                crate::components::SpellResistanceEffectProfile {
                                    ability: AbilityId(2),
                                    damage_taken_per_10k: factor,
                                },
                            ),
                        ),
                        ..UnitGameplayProperties::default()
                    },
                );
                sim.step();
                assert_eq!(sim.unit(target).unwrap().health, 1000 - spell_damage);
                sim.step();
                assert_eq!(sim.unit(target).unwrap().health, 1000 - spell_damage - 20);
            }
        }
    }

    #[test]
    fn competing_abilities_commit_in_id_order_against_one_revalidated_mana_pool() {
        let (mut sim, source, target) = setup(2, 5, &[ability(20, 4, 7, 6), ability(10, 5, 11, 8)]);
        sim.step();
        assert_eq!(sim.unit(target).unwrap().health, 989);
        assert_eq!(sim.unit(source).unwrap().mana_current, Some(0));
        assert_eq!(sim.ability_casts_last_tick().len(), 1);
        assert_eq!(sim.ability_casts_last_tick()[0].ability, AbilityId(10));
        // The rejected primary did not acquire a cooldown or cast sequence.
        let snapshot = sim.capture_snapshot();
        let entity = sim
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&source))
            .unwrap();
        let state = entity.get::<AutomaticAbilityState>().unwrap();
        assert_eq!(state.cast_sequence, 0);
        assert_eq!(state.ready_tick, 0);
        assert_eq!(snapshot.checksum(), sim.checksum());
    }

    #[test]
    fn independent_cooldowns_and_snapshot_continuation_are_order_and_worker_independent() {
        let profiles = [ability(20, 1, 7, 6), ability(10, 1, 11, 8)];
        let (mut first, source, _) = setup(1, 100, &profiles);
        let (mut second, _, _) = setup(3, 100, &[profiles[1], profiles[0]]);
        let mut ticks = Vec::new();
        for tick in 0..12 {
            first.step();
            second.step();
            assert_eq!(first.checksum(), second.checksum());
            for event in first.ability_casts_last_tick() {
                ticks.push((tick, event.ability));
            }
            if tick == 4 {
                let before = first.checksum();
                first
                    .configure_additional_automatic_abilities(source, &[profiles[1], profiles[0]])
                    .unwrap();
                assert_eq!(
                    before,
                    first.checksum(),
                    "reconfiguration must preserve per-ID state"
                );
                let bytes = first.capture_snapshot().encode_wire().unwrap();
                let content =
                    crate::castle_fight_content_bundle(crate::MapVersion::CASTLE_FIGHT_9_27)
                        .unwrap();
                let snapshot = SimulationSnapshot::decode_wire(&bytes, content).unwrap();
                second.restore_snapshot(&snapshot).unwrap();
            }
        }
        assert_eq!(
            ticks
                .iter()
                .filter(|(_, id)| *id == AbilityId(30))
                .map(|(tick, _)| *tick)
                .collect::<Vec<_>>(),
            [0, 4, 8]
        );
        assert_eq!(
            ticks
                .iter()
                .filter(|(_, id)| *id == AbilityId(20))
                .map(|(tick, _)| *tick)
                .collect::<Vec<_>>(),
            [0, 6]
        );
        assert_eq!(
            ticks
                .iter()
                .filter(|(_, id)| *id == AbilityId(10))
                .map(|(tick, _)| *tick)
                .collect::<Vec<_>>(),
            [0, 8]
        );
    }

    #[test]
    fn delayed_secondary_actions_belong_to_their_ability_slot_and_survive_wire_restore() {
        let support = |id, delay| AutomaticAbilityProfile {
            id: AbilityId(id),
            mana_cost: 1,
            cooldown_ticks: 100,
            range: 10 * SUBUNITS_PER_WORLD_UNIT,
            target_policy: AbilityTargetPolicy::WoundedFriendlyUnit,
            effect: AbilityEffect::HolyAid {
                native_buff: None,
                modifier: ModifierId(id),
                healing: 0,
                armor_bonus_per_100: 0,
                regeneration_per_second_per_10k: 0,
                duration_ticks: 1,
                permanent_max_health_bonus: 0,
                resurrection_count: 1,
                resurrection_radius: 10 * SUBUNITS_PER_WORLD_UNIT,
                resurrection_mana_cost: 1,
                resurrection_cooldown_ticks: 5,
                resurrection_delay_ticks: delay,
            },
        };
        let profiles = [support(10, 2), support(20, 3)];
        let (mut sim, source, _) = setup(1, 100, &profiles);
        let entity = sim
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&source))
            .unwrap()
            .id();
        sim.world
            .entity_mut(entity)
            .get_mut::<Health>()
            .unwrap()
            .current -= 1;
        sim.step();
        let pending = |sim: &Simulation| {
            let entity = sim
                .world
                .iter_entities()
                .find(|e| e.get::<SimId>() == Some(&source))
                .unwrap();
            entity
                .get::<AdditionalAutomaticAbilities>()
                .unwrap()
                .iter()
                .map(|entry| (entry.profile.id, entry.secondary_resurrection.due_tick))
                .collect::<Vec<_>>()
        };
        assert_eq!(pending(&sim), [(AbilityId(10), 2), (AbilityId(20), 3)]);
        let content =
            crate::castle_fight_content_bundle(crate::MapVersion::CASTLE_FIGHT_9_27).unwrap();
        let snapshot = SimulationSnapshot::decode_wire(
            &sim.capture_snapshot().encode_wire().unwrap(),
            content,
        )
        .unwrap();
        let (mut restored, _, _) = setup(3, 100, &profiles);
        restored.restore_snapshot(&snapshot).unwrap();
        for _ in 0..3 {
            sim.step();
            restored.step();
            assert_eq!(sim.checksum(), restored.checksum());
        }
        assert_eq!(pending(&sim), [(AbilityId(10), 0), (AbilityId(20), 0)]);
        assert_eq!(
            sim.unit(source)
                .unwrap()
                .status
                .secondary_resurrection_due_tick,
            0,
            "secondary slots must not overwrite the primary slot's pending state"
        );
    }

    #[test]
    fn resurrection_retains_additional_definitions_but_resets_runtime_state() {
        let mut sim = Simulation::new(SimulationConfig::default(), 1);
        let source = sim.spawn_unit_with_properties_and_spellcasting(
            unit(0, 40),
            UnitGameplayProperties {
                corpse: Some(CorpseProfile {
                    definition: CorpseDefinitionId(1),
                    decay_start_ticks: 1,
                    lifetime_ticks: Some(100),
                }),
                ..UnitGameplayProperties::default()
            },
            SpellcastingProfile {
                mana: crate::components::ManaProfile {
                    maximum: 100,
                    starting: 100,
                    regen_per_tick_per_10k: 0,
                },
                ability: ability(1, 1, 1, 20),
            },
        );
        let additional = ability(2, 1, 1, 30);
        sim.configure_additional_automatic_abilities(source, &[additional])
            .unwrap();
        let mut killer = unit(1, 45);
        killer.attack.damage = 2000;
        killer.attack.range = 100 * SUBUNITS_PER_WORLD_UNIT;
        killer.attack.acquisition_range = killer.attack.range;
        sim.spawn_unit(killer);
        sim.step();
        let entity = sim
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&source))
            .unwrap()
            .id();
        assert_eq!(
            sim.world
                .entity(entity)
                .get::<AdditionalAutomaticAbilities>()
                .unwrap()
                .get(AbilityId(2))
                .unwrap()
                .state
                .cast_sequence,
            1
        );
        // A permanent live stat change must not redefine the original resurrection template
        // when a snapshot is restored before the death (the old reconstruction did so).
        sim.world
            .entity_mut(entity)
            .get_mut::<Health>()
            .unwrap()
            .max += 5;
        let content =
            crate::castle_fight_content_bundle(crate::MapVersion::CASTLE_FIGHT_9_27).unwrap();
        let live = SimulationSnapshot::decode_wire(
            &sim.capture_snapshot().encode_wire().unwrap(),
            content,
        )
        .unwrap();
        let mut from_live = Simulation::new(SimulationConfig::default(), 2);
        from_live.restore_snapshot(&live).unwrap();
        sim.step();
        from_live.step();
        assert_eq!(sim.corpses().len(), 1);
        assert_eq!(
            sim.checksum(),
            from_live.checksum(),
            "live restoration must preserve the future corpse definition"
        );
        let snapshot = SimulationSnapshot::decode_wire(
            &sim.capture_snapshot().encode_wire().unwrap(),
            content,
        )
        .unwrap();
        let mut restored = Simulation::new(SimulationConfig::default(), 3);
        restored.restore_snapshot(&snapshot).unwrap();
        for sim in [&mut sim, &mut restored] {
            assert_eq!(
                sim.resurrect_friendly_corpses(
                    Team(0),
                    AbilitySourceOrigin::Unit(SimPoint::new(40 * SUBUNITS_PER_WORLD_UNIT, 0)),
                    100 * SUBUNITS_PER_WORLD_UNIT,
                    1
                ),
                1
            );
            let revived = sim
                .world
                .iter_entities()
                .find(|e| {
                    e.get::<Team>() == Some(&Team(0))
                        && e.get::<AdditionalAutomaticAbilities>().is_some()
                })
                .unwrap();
            let slot = revived
                .get::<AdditionalAutomaticAbilities>()
                .unwrap()
                .get(AbilityId(2))
                .unwrap();
            assert_eq!(revived.get::<Health>().unwrap().max, 1000);
            assert_eq!(slot.profile, additional);
            assert_eq!(slot.state.cast_sequence, 0);
            assert_eq!(slot.state.ready_tick, sim.tick());
            assert_eq!(
                slot.secondary_resurrection,
                SecondaryResurrectionState::default()
            );
        }
        assert_eq!(sim.checksum(), restored.checksum());
    }

    #[test]
    fn duplicate_slots_are_rejected_without_changing_authoritative_state() {
        let profile = ability(10, 1, 2, 3);
        let (mut sim, source, _) = setup(1, 100, &[profile]);
        let before = sim.checksum();
        assert_eq!(
            sim.configure_additional_automatic_abilities(source, &[profile, profile]),
            Err(AbilityConfigurationError::DuplicateAbility(profile.id))
        );
        assert_eq!(
            sim.configure_additional_automatic_abilities(source, &[ability(30, 1, 2, 3)]),
            Err(AbilityConfigurationError::DuplicateAbility(AbilityId(30)))
        );
        assert_eq!(
            sim.configure_additional_automatic_abilities(
                source,
                &[AutomaticAbilityProfile {
                    cooldown_ticks: 4,
                    ..profile
                }]
            ),
            Err(AbilityConfigurationError::DefinitionChanged(profile.id))
        );
        let overflowing = (0..crate::MAX_AUTOMATIC_ABILITIES)
            .map(|index| ability(100 + index as u32, 1, 2, 3))
            .collect::<Vec<_>>();
        assert_eq!(
            sim.configure_additional_automatic_abilities(source, &overflowing),
            Err(AbilityConfigurationError::CapacityExceeded)
        );
        assert_eq!(sim.checksum(), before);
    }
}
