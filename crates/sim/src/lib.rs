mod components;
mod content;
mod damage;
mod economy;
mod match_setup;
mod math;
mod native_effects;
mod simulation;
mod spatial;
mod terrain;
mod topology;
mod version;

pub use components::{
    AbilityEffect, AbilityId, AbilityTargetPolicy, AttackDelivery, AttackProfile, AttackTargetMask,
    AutomaticAbilityProfile, BashEffectProfile, BuilderConfiguration, BuilderLocomotion,
    BuilderProfile, BuilderSpawn, BuildingFootprint, BuildingGameplayProperties, BuildingSpawn,
    BurningOilEffectProfile, ChainLightningEffectProfile, CollisionRadius, ContentIdentity,
    CorpseDefinitionId, CorpseProfile, DefendEffectProfile, EntanglingRootsEffectProfile,
    EvasionEffectProfile, GameplayBundleIdentity, ManaProfile, ModifierId, MovementClass,
    MovementProfile, Owner, PassiveUnitEffect, PassiveUnitEffects, PlayerId, ProductionProfile,
    ResolvedUnitDefinition, SimId, SpellcastingProfile, StatusState, Team, TriggeredAttackEffect,
    TriggeredSpellProcProfile, UnitGameplayProperties, UnitSpawn, UnitTemplate,
};
pub use content::{
    CASTLE_FIGHT_CONTENT_BUNDLE_SCHEMA_VERSION, CASTLE_FIGHT_CONTENT_REVISION_927,
    CASTLE_FIGHT_DEFAULT_MAP_VERSION, CASTLE_FIGHT_SIMULATION_HZ, CastleFightAbilityId,
    CastleFightBuilderDefinition, CastleFightBuilderId, CastleFightBuilderRace,
    CastleFightBuildingId, CastleFightBuildingKind, CastleFightCommandCardLayout,
    CastleFightContentAvailability, CastleFightContentBundle, CastleFightContentError,
    CastleFightContentIdentity, CastleFightProductionDefinition, CastleFightProductionKind,
    CastleFightTowerDefinition, CastleFightTowerKind, CastleFightUnitDefinition, CastleFightUnitId,
    CastleFightUnitKind, CommandCardPosition, ResolvedCastleFightBehavior,
    UnsupportedCastleFightMapVersion, castle_fight_builder_profile,
    castle_fight_builder_profile_for_version, castle_fight_command_card_layout,
    castle_fight_command_card_layout_for_version, castle_fight_content_availability,
    castle_fight_content_bundle, castle_fight_content_bundle_for_revision,
    castle_fight_damage_rules, castle_fight_damage_rules_for_version, castle_fight_economy_rules,
    castle_fight_economy_rules_for_version, castle_fight_main_castle_repair_time_ticks,
    castle_fight_main_castle_repair_time_ticks_for_version,
};
pub use damage::{
    ArmorProfile, ArmorType, DAMAGE_MULTIPLIER_SCALE, DamageRules, DamageRulesLoadError, DamageType,
};
pub use economy::{
    BuildingEconomyProfile, EconomyRules, PlayerEconomyView, PlayerResources, RESOURCE_FIXED_SCALE,
    ResourcePurchaseError,
};
pub use match_setup::{
    CASTLE_FIGHT_REGISTERED_RELEASES, CastleFightMatch, CastleFightMatchConfig,
    CastleFightMatchMode, CastleFightMatchSetupError, CastleFightParticipantConfig,
    CastleFightReleaseDescriptor, CastleFightResolvedMatch, castle_fight_registered_releases,
    castle_fight_release_descriptor, create_castle_fight_match, resolve_castle_fight_match,
};
pub use math::{SUBUNITS_PER_WORLD_UNIT, SimPoint};
pub use native_effects::{
    NativeEffectImplementationId, NativeEffectResolveError, NativeEffectSource,
    NativeEffectSourceKind, ResolvedNativeEffectBinding, native_effect_implementation_for,
    resolve_native_effect_requirements,
};
pub use simulation::{
    AbilityCastEvent, AbilityCastTarget, AttackEvent, BuilderBuildError, BuilderCommandError,
    BuilderSpawnError, BuilderView, BuildingCommandError, BuildingConstructionCancelError,
    BuildingConstructionCancelOutcome, BuildingPlacementError, BuildingUpgradeError, BuildingView,
    CANONICAL_CHECKSUM_SCHEMA_VERSION, ChainLightningEvent, CombatRules, CorpseView,
    MatchLifecycle, MatchOutcome, PlayerConfig, PlayerConnectionStatus, PlayerView, ProjectileView,
    ProjectileViewKind, Simulation, SimulationConfig, TargetlessLane, TeamObjectiveError,
    TickResult, TickTimings, UPHILL_MISS_CHANCE_SCALE, UnitView,
};
pub use terrain::{
    TerrainElevationMap, TerrainElevationSample, TerrainLoadError, WC3_TERRAIN_TILE_WORLD_UNITS,
};
pub use topology::NavCell;
pub use version::{MapVersion, MapVersionParseError, MapVersionRange};

#[cfg(test)]
mod tests {
    use super::*;

    fn duel_unit(team: u8, x: i32, damage: i32, health: i32) -> UnitSpawn {
        UnitSpawn {
            team: Team(team),
            position: SimPoint::new(x, 0),
            health,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage,
                range: 4 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 10,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        }
    }

    fn populate_crossing_test_units(simulation: &mut Simulation, total_units: usize) {
        assert!(total_units >= 2 && total_units.is_multiple_of(2));
        let per_team = total_units / 2;
        let columns = 50usize;
        let spacing = 3 * SUBUNITS_PER_WORLD_UNIT / 4;
        let center_x = 60 * SUBUNITS_PER_WORLD_UNIT;
        let attack = AttackProfile {
            delivery: AttackDelivery::Melee,
            damage: 0,
            range: SUBUNITS_PER_WORLD_UNIT,
            acquisition_range: 4 * SUBUNITS_PER_WORLD_UNIT,
            cooldown_ticks: 30,
        };
        let movement = MovementProfile {
            speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 8,
        };
        for team in 0..2u8 {
            for index in 0..per_team {
                let column = (index % columns) as i32;
                let row = (index / columns) as i32;
                let y = (row - per_team.div_ceil(columns) as i32 / 2) * spacing;
                let x = if team == 0 {
                    center_x - 4 * SUBUNITS_PER_WORLD_UNIT - column * spacing
                } else {
                    center_x + 4 * SUBUNITS_PER_WORLD_UNIT + column * spacing
                };
                simulation.spawn_unit(UnitSpawn {
                    team: Team(team),
                    position: SimPoint::new(x, y),
                    health: 10_000,
                    attack,
                    movement,
                });
            }
        }
    }

    fn populate_lane_test_units(simulation: &mut Simulation, total_units: usize) {
        assert!(total_units >= 2 && total_units.is_multiple_of(2));
        let per_team = total_units / 2;
        let columns = 32usize;
        let spacing = 3 * SUBUNITS_PER_WORLD_UNIT / 4;
        let front_left = 48 * SUBUNITS_PER_WORLD_UNIT;
        let front_right = 72 * SUBUNITS_PER_WORLD_UNIT;
        let attack = AttackProfile {
            delivery: AttackDelivery::Melee,
            damage: 5,
            range: 2 * SUBUNITS_PER_WORLD_UNIT,
            acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
            cooldown_ticks: 10,
        };
        let movement = MovementProfile {
            speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 8,
        };
        for team in 0..2u8 {
            for index in 0..per_team {
                let column = (index % columns) as i32;
                let row = (index / columns) as i32;
                let y = (row - per_team.div_ceil(columns) as i32 / 2) * spacing;
                let x = if team == 0 {
                    front_left - column * spacing
                } else {
                    front_right + column * spacing
                };
                simulation.spawn_unit(UnitSpawn {
                    team: Team(team),
                    position: SimPoint::new(x, y),
                    health: 10_000,
                    attack,
                    movement,
                });
            }
        }
    }

    fn passive_unit(team: u8, x: i32) -> UnitSpawn {
        UnitSpawn {
            team: Team(team),
            position: SimPoint::new(x, 0),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        }
    }

    fn passive_building(team: u8, footprint: BuildingFootprint) -> BuildingSpawn {
        BuildingSpawn {
            team: Team(team),
            footprint,
            health: 10_000,
            production: None,
            attack: None,
            spellcasting: None,
        }
    }

    fn test_builder_configuration(build_catalog: Vec<u32>) -> BuilderConfiguration {
        BuilderConfiguration {
            appearance: ContentIdentity {
                rawcode: u32::from_be_bytes(*b"TEST"),
                name: "Test Builder",
            },
            locomotion: BuilderLocomotion::Foot,
            build_catalog,
        }
    }

    fn test_building_properties(rawcode: u32) -> BuildingGameplayProperties {
        BuildingGameplayProperties {
            content: Some(ContentIdentity {
                rawcode,
                name: "Test Building",
            }),
            ..BuildingGameplayProperties::default()
        }
    }

    fn attack_building(
        team: u8,
        footprint: BuildingFootprint,
        attack: AttackProfile,
    ) -> BuildingSpawn {
        BuildingSpawn {
            team: Team(team),
            footprint,
            health: 10_000,
            production: None,
            attack: Some(attack),
            spellcasting: None,
        }
    }

    fn spell_building(
        team: u8,
        footprint: BuildingFootprint,
        spellcasting: SpellcastingProfile,
    ) -> BuildingSpawn {
        BuildingSpawn {
            team: Team(team),
            footprint,
            health: 10_000,
            production: None,
            attack: None,
            spellcasting: Some(spellcasting),
        }
    }

    fn global_stun_spell(id: u32, duration_ticks: u16, cooldown_ticks: u16) -> SpellcastingProfile {
        SpellcastingProfile {
            mana: ManaProfile {
                maximum: 1_000,
                starting: 1_000,
                regen_per_tick_per_10k: 0,
            },
            ability: AutomaticAbilityProfile {
                id: AbilityId(id),
                mana_cost: 1,
                cooldown_ticks,
                range: 0,
                target_policy: AbilityTargetPolicy::AllEnemyUnits,
                effect: AbilityEffect::Stun { duration_ticks },
            },
        }
    }

    fn global_movement_modifier_spell(
        ability_id: u32,
        modifier_id: u32,
        percent_delta: i16,
        duration_ticks: u16,
        cooldown_ticks: u16,
    ) -> SpellcastingProfile {
        SpellcastingProfile {
            mana: ManaProfile {
                maximum: 1_000,
                starting: 1_000,
                regen_per_tick_per_10k: 0,
            },
            ability: AutomaticAbilityProfile {
                id: AbilityId(ability_id),
                mana_cost: 1,
                cooldown_ticks,
                range: 0,
                target_policy: AbilityTargetPolicy::AllEnemyUnits,
                effect: AbilityEffect::ModifyMovementSpeedPercent {
                    modifier: ModifierId(modifier_id),
                    percent_delta,
                    duration_ticks,
                },
            },
        }
    }

    fn original_map_terrain_config() -> SimulationConfig {
        let tile = WC3_TERRAIN_TILE_WORLD_UNITS * SUBUNITS_PER_WORLD_UNIT;
        SimulationConfig {
            match_seed: 0x5550_4849_4c4c,
            spatial_cell_size: 4 * tile,
            navigation_cell_size: tile,
            navigation_min: NavCell::new(-64, -32),
            navigation_max: NavCell::new(67, 31),
            target_pursuit_extra_range: tile,
            unit_separation_distance: 2 * SUBUNITS_PER_WORLD_UNIT,
            max_separation_per_tick: SUBUNITS_PER_WORLD_UNIT,
            static_blockers: Vec::new(),
            air_static_blockers: Vec::new(),
            build_static_blockers: Vec::new(),
            team_build_regions: [Vec::new(), Vec::new()],
            targetless_lane: None,
            team_objective: [wc3_point(6_000, 0), wc3_point(-6_000, 0)],
            economy: EconomyRules::default(),
        }
    }

    fn original_map_terrain() -> TerrainElevationMap {
        TerrainElevationMap::from_wc3_terrain_json(include_str!(
            "../../../docs/original_map/extracted/terrain.json"
        ))
        .unwrap()
    }

    fn wc3_point(x: i32, y: i32) -> SimPoint {
        SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, y * SUBUNITS_PER_WORLD_UNIT)
    }

    fn terrain_attacker(team: u8, position: SimPoint, delivery: AttackDelivery) -> UnitSpawn {
        UnitSpawn {
            team: Team(team),
            position,
            health: 10_000,
            attack: AttackProfile {
                delivery,
                damage: 7,
                range: 7_000 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 7_000 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        }
    }

    fn unit_properties(
        movement_class: MovementClass,
        attack_targets: AttackTargetMask,
    ) -> UnitGameplayProperties {
        UnitGameplayProperties {
            movement_class,
            attack_targets,
            ..UnitGameplayProperties::default()
        }
    }

    #[test]
    fn imported_content_identity_survives_building_and_production_spawn() {
        let mut sim = Simulation::new(SimulationConfig::default(), 1);
        let barracks = CastleFightProductionKind::Barracks.definition();
        let mut barracks_spawn = barracks.spawn(Team(0), BuildingFootprint::new(20, 0, 4, 4));
        let production = barracks_spawn
            .production
            .as_mut()
            .expect("Barracks must remain a production building");
        production.initial_delay_ticks = 0;
        production.interval_ticks = 1;
        let barracks_id =
            sim.spawn_building_with_properties(barracks_spawn, barracks.gameplay_properties());

        let building_content = sim.building(barracks_id).unwrap().content.unwrap();
        assert_eq!(building_content.rawcode, barracks.rawcode);
        assert_eq!(building_content.name, "Barracks");

        sim.step();
        let produced = sim
            .units()
            .into_iter()
            .find(|unit| unit.team == Team(0))
            .expect("Barracks should produce immediately in this fixture");
        let unit_content = produced.content.unwrap();
        assert_eq!(
            unit_content.rawcode,
            CastleFightUnitKind::Footman.definition().rawcode
        );
        assert_eq!(unit_content.name, "Footman");

        let tower = CastleFightTowerKind::WatchTower.definition();
        let tower_id = sim.spawn_building_with_properties(
            tower.spawn(Team(1), BuildingFootprint::new(100, 0, 4, 4)),
            tower.gameplay_properties(),
        );
        assert_eq!(
            sim.building(tower_id).unwrap().content.unwrap().name,
            "Watch Tower"
        );
    }

    #[test]
    fn imported_melee_damage_uses_castle_fight_type_and_armor_rules() {
        let mut sim = Simulation::new_with_combat_rules(
            SimulationConfig::default(),
            2,
            CombatRules {
                damage_rules: castle_fight_damage_rules(),
                ..CombatRules::default()
            },
        );
        let footman = CastleFightUnitKind::Footman.definition();
        let catapult = CastleFightUnitKind::Catapult.definition();
        sim.spawn_unit_with_properties(
            UnitSpawn::from_template(
                Team(0),
                SimPoint::new(40 * SUBUNITS_PER_WORLD_UNIT, 0),
                footman.template(),
            ),
            footman.gameplay_properties(),
        );
        let target = sim.spawn_unit_with_properties(
            UnitSpawn::from_template(
                Team(1),
                SimPoint::new(80 * SUBUNITS_PER_WORLD_UNIT, 0),
                catapult.template(),
            ),
            catapult.gameplay_properties(),
        );

        sim.step();
        sim.step();
        // 26 Normal * 175% vs Medium, then 5 armor => exactly 35 after rounding.
        assert_eq!(sim.unit(target).unwrap().health, catapult.health - 35);
    }

    #[test]
    fn imported_projectile_preserves_damage_type_until_impact() {
        let mut sim = Simulation::new_with_combat_rules(
            SimulationConfig::default(),
            2,
            CombatRules {
                damage_rules: castle_fight_damage_rules(),
                ..CombatRules::default()
            },
        );
        let ranger = CastleFightUnitKind::Ranger.definition();
        let footman = CastleFightUnitKind::Footman.definition();
        sim.spawn_unit_with_properties(
            UnitSpawn::from_template(
                Team(0),
                SimPoint::new(40 * SUBUNITS_PER_WORLD_UNIT, 0),
                ranger.template(),
            ),
            ranger.gameplay_properties(),
        );
        let target = sim.spawn_unit_with_properties(
            UnitSpawn::from_template(
                Team(1),
                SimPoint::new(80 * SUBUNITS_PER_WORLD_UNIT, 0),
                footman.template(),
            ),
            footman.gameplay_properties(),
        );

        for _ in 0..5 {
            sim.step();
        }
        // 65 Pierce * 70% vs Large, then 4 armor => 37 damage after rounding.
        assert_eq!(sim.unit(target).unwrap().health, footman.health - 37);
    }

    #[test]
    fn imported_gryphon_bash_is_versioned_and_data_driven() {
        let gryphon = CastleFightUnitKind::GryphonRider
            .definition_for_version(MapVersion::new(9, 27))
            .unwrap();
        let effects: Vec<_> = gryphon.passive_effects.iter().collect();
        assert_eq!(effects.len(), 2);
        assert!(
            effects.contains(&PassiveUnitEffect::Bash(BashEffectProfile {
                ability: AbilityId(u32::from_be_bytes(*b"A05K")),
                chance_per_10k: 1_500,
                bonus_damage: 25,
                stun_duration_ticks: 60,
                targets: AttackTargetMask::GROUND_UNITS,
            }))
        );
        assert!(effects.iter().any(|effect| matches!(
            effect,
            PassiveUnitEffect::TriggeredSpellProc(TriggeredSpellProcProfile {
                ability: AbilityId(ability),
                chance_per_10k: 1_000,
                effect: TriggeredAttackEffect::ChainLightning(ChainLightningEffectProfile {
                    ability: AbilityId(chain_ability),
                    initial_damage: 150,
                    maximum_targets: 5,
                    damage_reduction_per_10k: 2_500,
                    ..
                }),
                ..
            }) if *ability == u32::from_be_bytes(*b"A01B")
                && *chain_ability == u32::from_be_bytes(*b"A05X")
        )));
        assert!(
            CastleFightUnitKind::GryphonRider
                .definition_for_version(MapVersion::new(9, 28))
                .is_err()
        );

        let rock = CastleFightProductionKind::GryphonRock
            .definition_for_version(MapVersion::new(9, 27))
            .unwrap();
        assert_eq!(rock.map_version, MapVersion::new(9, 27));
        assert_eq!(
            rock.gameplay_properties().production_unit.passive_effects,
            gryphon.passive_effects
        );
    }

    #[test]
    fn bash_proc_adds_damage_and_stuns_melee_target() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let attack = AttackProfile {
            delivery: AttackDelivery::Melee,
            damage: 10,
            range: 2 * world,
            acquisition_range: 8 * world,
            cooldown_ticks: 100,
        };
        sim.spawn_unit_with_properties(
            UnitSpawn {
                team: Team(0),
                position: SimPoint::new(40 * world, 0),
                health: 100,
                attack,
                movement: MovementProfile { speed_per_tick: 0 },
            },
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Bash(
                    BashEffectProfile {
                        ability: AbilityId(u32::from_be_bytes(*b"TEST")),
                        chance_per_10k: 10_000,
                        bonus_damage: 25,
                        stun_duration_ticks: 2,
                        targets: AttackTargetMask::GROUND_UNITS,
                    },
                )),
                ..UnitGameplayProperties::default()
            },
        );
        let target = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(41 * world, 0),
            health: 100,
            attack: AttackProfile {
                damage: 0,
                ..attack
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step();
        let resolved = sim.step();
        assert_eq!(resolved.completed_tick, 1);
        let target = sim.unit(target).unwrap();
        assert_eq!(target.health, 65);
        assert_eq!(target.stunned_until_tick, 3);
    }

    #[test]
    fn ranged_bash_proc_is_carried_by_projectile_until_impact() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let attack = AttackProfile {
            delivery: AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: 2 * world,
            },
            damage: 10,
            range: 20 * world,
            acquisition_range: 20 * world,
            cooldown_ticks: 100,
        };
        sim.spawn_unit_with_properties(
            UnitSpawn {
                team: Team(0),
                position: SimPoint::new(40 * world, 0),
                health: 100,
                attack,
                movement: MovementProfile { speed_per_tick: 0 },
            },
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Bash(
                    BashEffectProfile {
                        ability: AbilityId(u32::from_be_bytes(*b"TEST")),
                        chance_per_10k: 10_000,
                        bonus_damage: 25,
                        stun_duration_ticks: 2,
                        targets: AttackTargetMask::GROUND_UNITS,
                    },
                )),
                ..UnitGameplayProperties::default()
            },
        );
        let target = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(50 * world, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: world,
                acquisition_range: 8 * world,
                cooldown_ticks: 100,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step();
        let launch = sim.step();
        assert_eq!(launch.projectiles_launched, 1);
        assert_eq!(sim.unit(target).unwrap().health, 100);
        assert_eq!(sim.unit(target).unwrap().stunned_until_tick, 0);
        let impact_tick = sim.projectiles()[0].impact_tick;
        while sim.tick() <= impact_tick {
            sim.step();
        }
        let target = sim.unit(target).unwrap();
        assert_eq!(target.health, 65);
        assert_eq!(target.stunned_until_tick, impact_tick + 2);
    }

    #[test]
    fn evasion_marks_the_attack_missed_and_prevents_damage() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let attack = AttackProfile {
            delivery: AttackDelivery::Melee,
            damage: 25,
            range: 4 * world,
            acquisition_range: 8 * world,
            cooldown_ticks: 100,
        };
        sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(40 * world, 0),
            health: 100,
            attack,
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let target = sim.spawn_unit_with_properties(
            UnitSpawn {
                team: Team(1),
                position: SimPoint::new(41 * world, 0),
                health: 100,
                attack: AttackProfile {
                    damage: 0,
                    ..attack
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Evasion(
                    EvasionEffectProfile {
                        ability: AbilityId(u32::from_be_bytes(*b"EVAD")),
                        chance_per_10k: 10_000,
                    },
                )),
                ..UnitGameplayProperties::default()
            },
        );

        sim.step();
        sim.step();
        assert_eq!(sim.unit(target).unwrap().health, 100);
        assert_eq!(sim.attacks_last_tick().len(), 2);
        assert!(
            sim.attacks_last_tick()
                .iter()
                .any(|event| event.target == target && event.missed)
        );
    }

    fn always_on_defend() -> PassiveUnitEffects {
        PassiveUnitEffects::single(PassiveUnitEffect::Defend(DefendEffectProfile {
            ability: AbilityId(u32::from_be_bytes(*b"DEFD")),
            ranged_damage_taken_per_10k: 4_000,
            spell_damage_taken_per_10k: 5_000,
            deflect_chance_per_10k: 10_000,
            deflected_pierce_damage_taken_per_10k: 0,
            activation_delay_ticks: 0,
        }))
    }

    #[test]
    fn defend_reduces_directed_ranged_damage_at_projectile_impact() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let attacker = sim.spawn_unit_with_properties(
            UnitSpawn {
                team: Team(0),
                position: SimPoint::new(40 * world, 0),
                health: 100,
                attack: AttackProfile {
                    delivery: AttackDelivery::RangedGuaranteedHit {
                        speed_per_tick: world,
                    },
                    damage: 100,
                    range: 4 * world,
                    acquisition_range: 8 * world,
                    cooldown_ticks: 100,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
            UnitGameplayProperties {
                damage_type: DamageType::Normal,
                ..UnitGameplayProperties::default()
            },
        );
        let defender = sim.spawn_unit_with_properties(
            UnitSpawn {
                team: Team(1),
                position: SimPoint::new(42 * world, 0),
                health: 100,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    cooldown_ticks: 100,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
            UnitGameplayProperties {
                passive_effects: always_on_defend(),
                ..UnitGameplayProperties::default()
            },
        );

        sim.step();
        let launch = sim.step();
        assert_eq!(launch.projectiles_launched, 1);
        let impact_tick = sim.projectiles()[0].impact_tick;
        while sim.tick() <= impact_tick {
            sim.step();
        }
        assert_eq!(sim.unit(defender).unwrap().health, 60);
        assert_eq!(sim.unit(attacker).unwrap().health, 100);
    }

    #[test]
    fn defend_reflects_pierce_as_persistent_return_projectile_without_ping_pong() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let attack = AttackProfile {
            delivery: AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: world,
            },
            damage: 40,
            range: 4 * world,
            acquisition_range: 8 * world,
            cooldown_ticks: 100,
        };
        let attacker = sim.spawn_unit_with_properties(
            UnitSpawn {
                team: Team(0),
                position: SimPoint::new(40 * world, 0),
                health: 100,
                attack,
                movement: MovementProfile { speed_per_tick: 0 },
            },
            UnitGameplayProperties {
                damage_type: DamageType::Pierce,
                // If returned missiles re-enter Defend interception this would ping-pong forever.
                passive_effects: always_on_defend(),
                ..UnitGameplayProperties::default()
            },
        );
        let defender = sim.spawn_unit_with_properties(
            UnitSpawn {
                team: Team(1),
                position: SimPoint::new(42 * world, 0),
                health: 100,
                attack: AttackProfile {
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    ..attack
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
            UnitGameplayProperties {
                passive_effects: always_on_defend(),
                ..UnitGameplayProperties::default()
            },
        );

        sim.step();
        sim.step();
        let outgoing = sim.projectiles()[0];
        let first_impact_tick = outgoing.impact_tick;
        while sim.tick() <= first_impact_tick {
            sim.step();
        }
        assert_eq!(sim.unit(defender).unwrap().health, 100);
        let reflected = sim
            .projectiles()
            .into_iter()
            .find(|projectile| matches!(projectile.kind, ProjectileViewKind::Reflected { .. }))
            .expect("Defend must create a persistent reflected projectile");
        assert_eq!(
            reflected.source, attacker,
            "return missile keeps original source art"
        );
        assert!(matches!(
            reflected.kind,
            ProjectileViewKind::Reflected {
                target,
                reflector
            } if target == attacker && reflector == defender
        ));

        let return_impact_tick = reflected.impact_tick;
        while sim.tick() <= return_impact_tick {
            sim.step();
        }
        // The reflected missile retains Pierce attack type. Against the synthetic attacker's
        // default Unarmored defense, Warcraft's damage table scales 40 raw Pierce to 60 damage.
        assert_eq!(sim.unit(attacker).unwrap().health, 40);
        assert_eq!(sim.unit(defender).unwrap().health, 100);
        assert!(
            sim.projectiles().is_empty(),
            "reflected missiles cannot reflect again"
        );
    }

    #[test]
    fn fractional_health_regeneration_accumulates_deterministically() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let attack = AttackProfile {
            delivery: AttackDelivery::Melee,
            damage: 30,
            range: 4 * world,
            acquisition_range: 8 * world,
            cooldown_ticks: 100,
        };
        sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(40 * world, 0),
            health: 100,
            attack,
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let target = sim.spawn_unit_with_properties(
            UnitSpawn {
                team: Team(1),
                position: SimPoint::new(41 * world, 0),
                health: 100,
                attack: AttackProfile {
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    ..attack
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
            UnitGameplayProperties {
                // Defender's extracted 1.75 HP/s. At 30 Hz the first whole HP arrives after
                // 18 regeneration ticks; fixed-point remainder must make this exact and stable.
                health_regen_per_second_per_10k: 17_500,
                ..UnitGameplayProperties::default()
            },
        );

        sim.step();
        sim.step();
        assert_eq!(sim.unit(target).unwrap().health, 70);
        for _ in 0..17 {
            sim.step();
        }
        assert_eq!(sim.unit(target).unwrap().health, 70);
        sim.step();
        assert_eq!(sim.unit(target).unwrap().health, 71);
    }

    #[test]
    fn defend_halves_spell_damage() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let attack = AttackProfile {
            delivery: AttackDelivery::Melee,
            damage: 0,
            range: 4 * world,
            acquisition_range: 8 * world,
            cooldown_ticks: 100,
        };
        sim.spawn_unit_with_properties(
            UnitSpawn {
                team: Team(0),
                position: SimPoint::new(40 * world, 0),
                health: 100,
                attack,
                movement: MovementProfile { speed_per_tick: 0 },
            },
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::TriggeredSpellProc(
                    TriggeredSpellProcProfile {
                        ability: AbilityId(u32::from_be_bytes(*b"ORBP")),
                        chance_per_10k: 10_000,
                        targets: AttackTargetMask::GROUND_UNITS,
                        effect: TriggeredAttackEffect::ChainLightning(
                            ChainLightningEffectProfile {
                                ability: AbilityId(u32::from_be_bytes(*b"CHLN")),
                                initial_damage: 100,
                                maximum_targets: 1,
                                jump_radius: 0,
                                damage_reduction_per_10k: 0,
                                targets: AttackTargetMask::GROUND_UNITS,
                            },
                        ),
                    },
                )),
                ..UnitGameplayProperties::default()
            },
        );
        let defender = sim.spawn_unit_with_properties(
            UnitSpawn {
                team: Team(1),
                position: SimPoint::new(41 * world, 0),
                health: 100,
                attack: AttackProfile {
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    ..attack
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
            UnitGameplayProperties {
                passive_effects: always_on_defend(),
                ..UnitGameplayProperties::default()
            },
        );

        sim.step();
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().health, 50);
    }

    #[test]
    fn triggered_chain_lightning_hits_nearest_valid_jump_targets() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let attack = AttackProfile {
            delivery: AttackDelivery::Melee,
            damage: 0,
            range: 4 * world,
            acquisition_range: 8 * world,
            cooldown_ticks: 100,
        };
        let source = sim.spawn_unit_with_properties(
            UnitSpawn {
                team: Team(0),
                position: SimPoint::new(40 * world, 0),
                health: 100,
                attack,
                movement: MovementProfile { speed_per_tick: 0 },
            },
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::TriggeredSpellProc(
                    TriggeredSpellProcProfile {
                        ability: AbilityId(u32::from_be_bytes(*b"ORBP")),
                        chance_per_10k: 10_000,
                        targets: AttackTargetMask::GROUND_UNITS,
                        effect: TriggeredAttackEffect::ChainLightning(
                            ChainLightningEffectProfile {
                                ability: AbilityId(u32::from_be_bytes(*b"CHLN")),
                                initial_damage: 100,
                                maximum_targets: 3,
                                jump_radius: 3 * world,
                                damage_reduction_per_10k: 5_000,
                                targets: AttackTargetMask::GROUND_UNITS,
                            },
                        ),
                    },
                )),
                ..UnitGameplayProperties::default()
            },
        );
        let first = sim.spawn_unit(duel_unit(1, 41 * world, 0, 1_000));
        let second = sim.spawn_unit(duel_unit(1, 43 * world, 0, 1_000));
        let third = sim.spawn_unit(duel_unit(1, 45 * world, 0, 1_000));

        sim.step();
        sim.step();
        assert_eq!(sim.unit(first).unwrap().health, 900);
        assert_eq!(sim.unit(second).unwrap().health, 1_000);
        assert_eq!(sim.unit(third).unwrap().health, 1_000);
        let events = sim.chain_lightnings_last_tick();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].source, source);
        assert_eq!(events[0].ability, AbilityId(u32::from_be_bytes(*b"CHLN")));
        assert_eq!(events[0].bounce_index, 0);
        assert_eq!(
            events[0].points(),
            &[SimPoint::new(40 * world, 0), SimPoint::new(41 * world, 0)]
        );

        while sim.tick() <= 8 {
            sim.step();
        }
        assert_eq!(sim.unit(second).unwrap().health, 1_000);
        sim.step();
        assert_eq!(sim.unit(second).unwrap().health, 950);
        assert_eq!(sim.unit(third).unwrap().health, 1_000);
        assert_eq!(sim.chain_lightnings_last_tick()[0].bounce_index, 1);
        assert_eq!(
            sim.chain_lightnings_last_tick()[0].points(),
            &[SimPoint::new(41 * world, 0), SimPoint::new(43 * world, 0)]
        );

        while sim.tick() <= 15 {
            sim.step();
        }
        assert_eq!(sim.unit(third).unwrap().health, 1_000);
        sim.step();
        assert_eq!(sim.unit(third).unwrap().health, 975);
        assert_eq!(sim.chain_lightnings_last_tick()[0].bounce_index, 2);
        assert_eq!(
            sim.chain_lightnings_last_tick()[0].points(),
            &[SimPoint::new(43 * world, 0), SimPoint::new(45 * world, 0)]
        );
    }

    #[test]
    fn staged_chain_lightning_is_worker_count_independent() {
        let build_sim = |workers| {
            let world = SUBUNITS_PER_WORLD_UNIT;
            let mut sim = Simulation::new(SimulationConfig::default(), workers);
            let attack = AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 4 * world,
                acquisition_range: 8 * world,
                cooldown_ticks: 100,
            };
            sim.spawn_unit_with_properties(
                UnitSpawn {
                    team: Team(0),
                    position: SimPoint::new(40 * world, 0),
                    health: 100,
                    attack,
                    movement: MovementProfile { speed_per_tick: 0 },
                },
                UnitGameplayProperties {
                    passive_effects: PassiveUnitEffects::single(
                        PassiveUnitEffect::TriggeredSpellProc(TriggeredSpellProcProfile {
                            ability: AbilityId(u32::from_be_bytes(*b"ORBP")),
                            chance_per_10k: 10_000,
                            targets: AttackTargetMask::GROUND_UNITS,
                            effect: TriggeredAttackEffect::ChainLightning(
                                ChainLightningEffectProfile {
                                    ability: AbilityId(u32::from_be_bytes(*b"CHLN")),
                                    initial_damage: 100,
                                    maximum_targets: 3,
                                    jump_radius: 3 * world,
                                    damage_reduction_per_10k: 5_000,
                                    targets: AttackTargetMask::GROUND_UNITS,
                                },
                            ),
                        }),
                    ),
                    ..UnitGameplayProperties::default()
                },
            );
            sim.spawn_unit(duel_unit(1, 41 * world, 0, 1_000));
            sim.spawn_unit(duel_unit(1, 43 * world, 0, 1_000));
            sim.spawn_unit(duel_unit(1, 45 * world, 0, 1_000));
            sim
        };

        let mut single = build_sim(1);
        let mut parallel = build_sim(4);
        for tick in 0..20 {
            let single_result = single.step();
            let parallel_result = parallel.step();
            assert_eq!(
                single_result.checksum, parallel_result.checksum,
                "staged Chain Lightning diverged on tick {tick}"
            );
            assert_eq!(
                single.chain_lightnings_last_tick(),
                parallel.chain_lightnings_last_tick(),
                "Chain Lightning events diverged on tick {tick}"
            );
        }
    }

    #[test]
    fn entangling_roots_immobilizes_and_deals_one_pulse_per_second() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let attack = AttackProfile {
            delivery: AttackDelivery::Melee,
            damage: 0,
            range: 30 * world,
            acquisition_range: 30 * world,
            cooldown_ticks: 1_000,
        };
        sim.spawn_unit_with_properties(
            UnitSpawn {
                team: Team(0),
                position: SimPoint::new(40 * world, 0),
                health: 100,
                attack,
                movement: MovementProfile { speed_per_tick: 0 },
            },
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::TriggeredSpellProc(
                    TriggeredSpellProcProfile {
                        ability: AbilityId(u32::from_be_bytes(*b"ORBR")),
                        chance_per_10k: 10_000,
                        targets: AttackTargetMask::GROUND_UNITS,
                        effect: TriggeredAttackEffect::EntanglingRoots(
                            EntanglingRootsEffectProfile {
                                ability: AbilityId(u32::from_be_bytes(*b"ROOT")),
                                damage_per_second: 30,
                                duration_ticks: 60,
                                targets: AttackTargetMask::GROUND_UNITS,
                            },
                        ),
                    },
                )),
                ..UnitGameplayProperties::default()
            },
        );
        let target = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(60 * world, 0),
            health: 1_000,
            attack: AttackProfile {
                damage: 0,
                ..attack
            },
            movement: MovementProfile {
                speed_per_tick: world,
            },
        });

        sim.step();
        sim.step();
        let rooted = sim.unit(target).unwrap();
        let rooted_position = rooted.position;
        assert!(rooted.stunned_until_tick > sim.tick());
        let frozen_tick = sim.step();
        assert_eq!(frozen_tick.attacks_resolved, 0);
        for _ in 0..29 {
            sim.step();
        }
        assert_eq!(sim.unit(target).unwrap().position, rooted_position);
        assert_eq!(sim.unit(target).unwrap().health, 970);
        for _ in 0..30 {
            sim.step();
        }
        assert_eq!(sim.unit(target).unwrap().health, 940);
    }

    #[test]
    fn burning_oil_creates_persistent_ground_damage_after_ballistic_impact() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let attack = AttackProfile {
            delivery: AttackDelivery::RangedBallistic {
                speed_per_tick: 10 * world,
                impact_radius: 0,
            },
            damage: 0,
            range: 30 * world,
            acquisition_range: 30 * world,
            cooldown_ticks: 1_000,
        };
        sim.spawn_unit_with_properties(
            UnitSpawn {
                team: Team(0),
                position: SimPoint::new(40 * world, 0),
                health: 100,
                attack,
                movement: MovementProfile { speed_per_tick: 0 },
            },
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::BurningOil(
                    BurningOilEffectProfile {
                        ability: AbilityId(u32::from_be_bytes(*b"BOIL")),
                        radius: 5 * world,
                        full_damage: 12,
                        full_interval_millis: 250,
                        half_damage: 3,
                        half_interval_millis: 1_000,
                        full_duration_millis: 1_010,
                        total_duration_millis: 2_510,
                        target_ground_units: true,
                        target_buildings: true,
                    },
                )),
                ..UnitGameplayProperties::default()
            },
        );
        let target = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(60 * world, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: world,
                acquisition_range: world,
                cooldown_ticks: 1_000,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step();
        sim.step();
        let impact_tick = sim
            .projectiles()
            .iter()
            .find(|projectile| matches!(projectile.kind, ProjectileViewKind::Ballistic { .. }))
            .expect("ballistic projectile must launch")
            .impact_tick;
        while sim.tick() <= impact_tick + 62 {
            sim.step();
        }
        assert_eq!(sim.unit(target).unwrap().health, 949);
    }

    #[test]
    fn imported_ice_troll_frost_armor_autocasts_and_slows_melee_attackers() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let troll = CastleFightUnitKind::IceTrollShadowPriest.definition();
        assert!(
            troll.spellcasting.is_some(),
            "Ice Troll Shadow Priest must have Frost Armor autocast"
        );
        let caster =
            sim.spawn_resolved_unit(Team(0), SimPoint::new(30 * world, 0), troll.resolved());
        let ally = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(60 * world, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: world,
                acquisition_range: world,
                cooldown_ticks: 100,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(85 * world, 0),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 10,
                range: 30 * world,
                acquisition_range: 30 * world,
                cooldown_ticks: 100,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step();
        sim.step();
        assert_eq!(sim.unit(ally).unwrap().last_attacked_tick, Some(1));
        sim.step();
        assert_eq!(sim.unit(caster).unwrap().mana_current, Some(115));
        assert!(sim.ability_casts_last_tick().iter().any(|event| {
            event.source == caster
                && event.ability == AbilityId(u32::from_be_bytes(*b"A03Z"))
                && event.target == AbilityCastTarget::Unit(ally)
        }));

        let slowed_attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(90 * world, 0),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 10,
                range: 30 * world,
                acquisition_range: 30 * world,
                cooldown_ticks: 40,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert_eq!(sim.unit(slowed_attacker).unwrap().cooldown_remaining, 54);
    }

    #[test]
    fn imported_siege_damage_uses_fortified_multiplier_on_buildings() {
        let mut sim = Simulation::new_with_combat_rules(
            SimulationConfig::default(),
            2,
            CombatRules {
                damage_rules: castle_fight_damage_rules(),
                ..CombatRules::default()
            },
        );
        let catapult = CastleFightUnitKind::Catapult.definition();
        sim.spawn_unit_with_properties(
            UnitSpawn::from_template(
                Team(0),
                SimPoint::new(40 * SUBUNITS_PER_WORLD_UNIT, 0),
                catapult.template(),
            ),
            catapult.gameplay_properties(),
        );
        let tower = CastleFightTowerKind::WatchTower.definition();
        let target = sim.spawn_building_with_properties(
            tower.spawn(Team(1), BuildingFootprint::new(80, 0, 4, 4)),
            tower.gameplay_properties(),
        );

        for _ in 0..10 {
            sim.step();
        }
        // 135 Siege * 160% vs Fortified, then 5 armor => 166 damage after rounding.
        assert_eq!(sim.building(target).unwrap().health, tower.health - 166);
    }

    #[test]
    fn castle_fight_spell_damage_uses_spell_row_but_ignores_numeric_armor() {
        let mut sim = Simulation::new_with_combat_rules(
            SimulationConfig::default(),
            2,
            CombatRules {
                damage_rules: castle_fight_damage_rules(),
                ..CombatRules::default()
            },
        );
        sim.spawn_building(spell_building(
            0,
            BuildingFootprint::new(10, 0, 1, 1),
            SpellcastingProfile {
                mana: ManaProfile {
                    maximum: 1,
                    starting: 1,
                    regen_per_tick_per_10k: 0,
                },
                ability: AutomaticAbilityProfile {
                    id: AbilityId(0x4441_4d47),
                    mana_cost: 1,
                    cooldown_ticks: 30,
                    range: 8 * SUBUNITS_PER_WORLD_UNIT,
                    target_policy: AbilityTargetPolicy::RandomEnemyUnit,
                    effect: AbilityEffect::Damage { amount: 100 },
                },
            },
        ));
        let target = sim.spawn_unit_with_properties(
            passive_unit(1, 15 * SUBUNITS_PER_WORLD_UNIT),
            UnitGameplayProperties {
                armor: ArmorProfile::new(ArmorType::Large, 4),
                ..UnitGameplayProperties::default()
            },
        );

        sim.step();
        assert_eq!(sim.unit(target).unwrap().health, 9_900);
    }

    #[test]
    fn imported_typed_battle_is_worker_count_independent() {
        fn run(workers: usize) -> u64 {
            let world = SUBUNITS_PER_WORLD_UNIT;
            let config = SimulationConfig {
                navigation_max: NavCell::new(500, 64),
                team_objective: [SimPoint::new(500 * world, 0), SimPoint::new(0, 0)],
                ..SimulationConfig::default()
            };
            let mut sim = Simulation::new_with_combat_rules(
                config,
                workers,
                CombatRules {
                    damage_rules: castle_fight_damage_rules(),
                    ..CombatRules::default()
                },
            );
            for team in 0..2u8 {
                for (index, kind) in CastleFightUnitKind::ALL.into_iter().enumerate() {
                    let definition = kind.definition();
                    let x = if team == 0 {
                        40 + index as i32 * 36
                    } else {
                        460 - index as i32 * 36
                    };
                    sim.spawn_unit_with_properties(
                        UnitSpawn::from_template(
                            Team(team),
                            SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, 0),
                            definition.template(),
                        ),
                        definition.gameplay_properties(),
                    );
                }
            }
            for _ in 0..120 {
                sim.step();
            }
            sim.checksum()
        }

        assert_eq!(run(1), run(8));
    }

    fn air_unit(team: u8, position: SimPoint, speed_per_tick: i32) -> UnitSpawn {
        UnitSpawn {
            team: Team(team),
            position,
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 4 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick },
        }
    }

    fn production_building(
        team: u8,
        footprint: BuildingFootprint,
        search_radius_cells: u16,
    ) -> BuildingSpawn {
        BuildingSpawn {
            team: Team(team),
            footprint,
            health: 10_000,
            production: Some(ProductionProfile {
                initial_delay_ticks: 0,
                interval_ticks: 10,
                search_radius_cells,
                unit: UnitTemplate {
                    health: 100,
                    attack: AttackProfile {
                        delivery: AttackDelivery::Melee,
                        damage: 1,
                        range: SUBUNITS_PER_WORLD_UNIT,
                        acquisition_range: 3 * SUBUNITS_PER_WORLD_UNIT,
                        cooldown_ticks: 10,
                    },
                    movement: MovementProfile { speed_per_tick: 0 },
                },
            }),
            attack: None,
            spellcasting: None,
        }
    }

    #[test]
    fn attack_target_masks_filter_air_and_ground_units() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(30, 10),
            team_objective: [
                SimPoint::new(30 * cell, 5 * cell),
                SimPoint::new(0, 5 * cell),
            ],
            ..SimulationConfig::default()
        };

        let mut ground_only = Simulation::new(config.clone(), 2);
        let attacker = ground_only.spawn_unit_with_properties(
            UnitSpawn {
                team: Team(0),
                position: SimPoint::new(5 * cell, 5 * cell),
                health: 100,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 1,
                    range: 12 * cell,
                    acquisition_range: 12 * cell,
                    cooldown_ticks: 10,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
            unit_properties(MovementClass::Ground, AttackTargetMask::GROUND_UNITS),
        );
        ground_only.spawn_unit_with_properties(
            air_unit(1, SimPoint::new(7 * cell, 5 * cell), 0),
            unit_properties(MovementClass::Air, AttackTargetMask::ALL),
        );
        let mut ground_spawn = passive_unit(1, 9 * cell);
        ground_spawn.position.y = 5 * cell;
        let ground = ground_only.spawn_unit(ground_spawn);
        ground_only.step();
        assert_eq!(ground_only.unit(attacker).unwrap().target, Some(ground));

        let mut air_only = Simulation::new(config, 2);
        let attacker = air_only.spawn_unit_with_properties(
            UnitSpawn {
                team: Team(0),
                position: SimPoint::new(5 * cell, 5 * cell),
                health: 100,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 1,
                    range: 12 * cell,
                    acquisition_range: 12 * cell,
                    cooldown_ticks: 10,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
            unit_properties(MovementClass::Ground, AttackTargetMask::AIR_UNITS),
        );
        let air = air_only.spawn_unit_with_properties(
            air_unit(1, SimPoint::new(7 * cell, 5 * cell), 0),
            unit_properties(MovementClass::Air, AttackTargetMask::ALL),
        );
        let mut ground_spawn = passive_unit(1, 9 * cell);
        ground_spawn.position.y = 5 * cell;
        air_only.spawn_unit(ground_spawn);
        air_only.step();
        assert_eq!(air_only.unit(attacker).unwrap().target, Some(air));
    }

    #[test]
    fn air_units_fly_across_ground_blockers() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(20, 10),
            static_blockers: vec![BuildingFootprint::new(10, 0, 1, 11)],
            team_objective: [
                SimPoint::new(20 * cell + cell / 2, 5 * cell + cell / 2),
                SimPoint::new(cell / 2, 5 * cell + cell / 2),
            ],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        let air = sim.spawn_unit_with_properties(
            air_unit(
                0,
                SimPoint::new(5 * cell + cell / 2, 5 * cell + cell / 2),
                cell / 2,
            ),
            unit_properties(MovementClass::Air, AttackTargetMask::ALL),
        );

        for _ in 0..16 {
            sim.step();
        }
        assert!(sim.unit(air).unwrap().position.x > 11 * cell);
    }

    #[test]
    fn air_units_route_around_air_static_blockers() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(20, 10),
            air_static_blockers: vec![BuildingFootprint::new(10, 4, 1, 7)],
            team_objective: [
                SimPoint::new(20 * cell + cell / 2, 8 * cell + cell / 2),
                SimPoint::new(cell / 2, 8 * cell + cell / 2),
            ],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        let air = sim.spawn_unit_with_properties(
            air_unit(
                0,
                SimPoint::new(5 * cell + cell / 2, 8 * cell + cell / 2),
                cell / 2,
            ),
            unit_properties(MovementClass::Air, AttackTargetMask::ALL),
        );

        let mut minimum_y = i32::MAX;
        for _ in 0..100 {
            sim.step();
            minimum_y = minimum_y.min(sim.unit(air).unwrap().position.y);
        }
        let position = sim.unit(air).unwrap().position;
        assert!(
            position.x > 11 * cell,
            "flyer never cleared no-fly barrier: {position:?}"
        );
        assert!(
            minimum_y < 4 * cell,
            "flyer did not route through the open lane"
        );
    }

    #[test]
    fn build_only_static_blockers_do_not_change_unit_navigation() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let footprint = BuildingFootprint::new(10, 5, 1, 1);
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(20, 10),
            build_static_blockers: vec![footprint],
            team_objective: [
                SimPoint::new(20 * cell + cell / 2, 5 * cell + cell / 2),
                SimPoint::new(cell / 2, 5 * cell + cell / 2),
            ],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        assert!(!sim.can_place_building(footprint));
        let ground = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(5 * cell + cell / 2, 5 * cell + cell / 2),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: cell,
                acquisition_range: cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile {
                speed_per_tick: cell / 2,
            },
        });
        for _ in 0..16 {
            sim.step();
        }
        assert!(sim.unit(ground).unwrap().position.x > 11 * cell);
    }

    #[test]
    fn air_and_ground_use_separate_collision_layers_and_air_does_not_block_building_placement() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(10, 10),
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        let position = SimPoint::new(4 * cell + cell / 2, 4 * cell + cell / 2);
        let air = sim.spawn_unit_with_properties(
            air_unit(0, position, 0),
            unit_properties(MovementClass::Air, AttackTargetMask::ALL),
        );
        let footprint = BuildingFootprint::new(4, 4, 1, 1);
        assert!(sim.can_place_building(footprint));

        let ground = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position,
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        assert_eq!(sim.unit(air).unwrap().position, position);
        assert_eq!(sim.unit(ground).unwrap().position, position);
        assert!(!sim.can_place_building(footprint));
    }

    #[test]
    fn air_units_still_collide_with_other_air_units() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(10, 10),
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        let position = SimPoint::new(5 * cell, 5 * cell);
        let a = sim.spawn_unit_with_properties(
            air_unit(0, position, 0),
            unit_properties(MovementClass::Air, AttackTargetMask::ALL),
        );
        let b = sim.spawn_unit_with_properties(
            air_unit(0, position, 0),
            unit_properties(MovementClass::Air, AttackTargetMask::ALL),
        );
        sim.step();
        let a = sim.unit(a).unwrap();
        let b = sim.unit(b).unwrap();
        let clearance = a.collision_radius + b.collision_radius;
        assert!(a.position.distance_sq(b.position) >= (clearance as i64 * clearance as i64) as u64);
    }

    #[test]
    fn ballistic_splash_preserves_attack_target_mask() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(30, 10),
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        sim.spawn_unit_with_properties(
            UnitSpawn {
                team: Team(0),
                position: SimPoint::new(5 * cell, 5 * cell),
                health: 100,
                attack: AttackProfile {
                    delivery: AttackDelivery::RangedBallistic {
                        speed_per_tick: 100 * cell,
                        impact_radius: 3 * cell,
                    },
                    damage: 10,
                    range: 12 * cell,
                    acquisition_range: 12 * cell,
                    cooldown_ticks: 100,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
            unit_properties(MovementClass::Ground, AttackTargetMask::GROUND_UNITS),
        );
        let mut ground_spawn = passive_unit(1, 10 * cell);
        ground_spawn.position.y = 5 * cell;
        let ground = sim.spawn_unit(ground_spawn);
        let air = sim.spawn_unit_with_properties(
            air_unit(1, SimPoint::new(11 * cell, 5 * cell), 0),
            unit_properties(MovementClass::Air, AttackTargetMask::ALL),
        );

        sim.step();
        sim.step();
        sim.step();
        assert_eq!(sim.unit(ground).unwrap().health, 9_990);
        assert_eq!(sim.unit(air).unwrap().health, 100);
    }

    #[test]
    fn production_inherits_air_movement_and_attack_targets() {
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(20, 10),
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        sim.spawn_building_with_properties(
            production_building(0, BuildingFootprint::new(4, 4, 2, 2), 4),
            BuildingGameplayProperties {
                production_unit: unit_properties(MovementClass::Air, AttackTargetMask::AIR_UNITS),
                ..BuildingGameplayProperties::default()
            },
        );
        sim.step();
        let unit = sim
            .units()
            .into_iter()
            .next()
            .expect("production unit missing");
        assert_eq!(unit.movement_class, MovementClass::Air);
        assert_eq!(unit.attack_targets, AttackTargetMask::AIR_UNITS);
    }

    #[test]
    fn air_units_ignore_uphill_miss() {
        let rules = CombatRules {
            terrain_elevation: Some(original_map_terrain()),
            uphill_miss_chance_per_10k: UPHILL_MISS_CHANCE_SCALE,
            ..CombatRules::default()
        };
        let mut sim = Simulation::new_with_combat_rules(original_map_terrain_config(), 2, rules);
        sim.spawn_unit_with_properties(
            terrain_attacker(0, wc3_point(0, 0), AttackDelivery::Melee),
            unit_properties(MovementClass::Air, AttackTargetMask::GROUND_UNITS),
        );
        let target = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: wc3_point(-6_000, 0),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert_eq!(sim.unit(target).unwrap().health, 9_993);
        assert!(!sim.attacks_last_tick()[0].missed);
    }

    #[test]
    fn mixed_air_ground_battle_is_worker_count_independent() {
        fn run(workers: usize) -> u64 {
            let cell = SUBUNITS_PER_WORLD_UNIT;
            let config = SimulationConfig {
                match_seed: 0x4149_522d_4752_4f55,
                navigation_min: NavCell::new(0, 0),
                navigation_max: NavCell::new(60, 20),
                static_blockers: vec![BuildingFootprint::new(30, 0, 1, 21)],
                team_objective: [
                    SimPoint::new(60 * cell, 10 * cell),
                    SimPoint::new(0, 10 * cell),
                ],
                ..SimulationConfig::default()
            };
            let mut sim = Simulation::new(config, workers);
            for team in 0..=1 {
                for index in 0..18 {
                    let air = index % 3 == 0;
                    let x = if team == 0 {
                        8 + index / 6
                    } else {
                        52 - index / 6
                    };
                    let y = 2 + index % 6 * 3;
                    let spawn = UnitSpawn {
                        team: Team(team),
                        position: SimPoint::new(x * cell + cell / 2, y * cell + cell / 2),
                        health: 100,
                        attack: AttackProfile {
                            delivery: AttackDelivery::RangedGuaranteedHit {
                                speed_per_tick: 4 * cell,
                            },
                            damage: 3,
                            range: 5 * cell,
                            acquisition_range: 10 * cell,
                            cooldown_ticks: 4,
                        },
                        movement: MovementProfile {
                            speed_per_tick: cell / 3,
                        },
                    };
                    sim.spawn_unit_with_properties(
                        spawn,
                        unit_properties(
                            if air {
                                MovementClass::Air
                            } else {
                                MovementClass::Ground
                            },
                            if index % 2 == 0 {
                                AttackTargetMask::ALL
                            } else {
                                AttackTargetMask::GROUND_UNITS
                            },
                        ),
                    );
                }
            }
            for _ in 0..120 {
                sim.step();
            }
            sim.checksum()
        }

        assert_eq!(run(1), run(8));
    }

    #[test]
    fn uphill_miss_chance_is_configurable_and_uses_extracted_cliff_levels() {
        fn run(chance: u16, attacker: SimPoint, target: SimPoint) -> (i32, AttackEvent) {
            let rules = CombatRules {
                terrain_elevation: Some(original_map_terrain()),
                uphill_miss_chance_per_10k: chance,
                ..CombatRules::default()
            };
            let mut sim =
                Simulation::new_with_combat_rules(original_map_terrain_config(), 2, rules);
            sim.spawn_unit(terrain_attacker(0, attacker, AttackDelivery::Melee));
            let target_id = sim.spawn_unit(UnitSpawn {
                team: Team(1),
                position: target,
                health: 10_000,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    cooldown_ticks: 1,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            });

            sim.step();
            sim.step();
            let event = *sim
                .attacks_last_tick()
                .first()
                .expect("attack attempt missing");
            (sim.unit(target_id).unwrap().health, event)
        }

        let lane = wc3_point(0, 0);
        let left_base = wc3_point(-6_000, 0);

        let (uphill_health, uphill_event) = run(UPHILL_MISS_CHANCE_SCALE, lane, left_base);
        assert_eq!(uphill_health, 10_000);
        assert!(uphill_event.missed);

        let (disabled_health, disabled_event) = run(0, lane, left_base);
        assert_eq!(disabled_health, 9_993);
        assert!(!disabled_event.missed);

        let (downhill_health, downhill_event) = run(UPHILL_MISS_CHANCE_SCALE, left_base, lane);
        assert_eq!(downhill_health, 9_993);
        assert!(!downhill_event.missed);
    }

    #[test]
    fn uphill_miss_prevents_guaranteed_hit_projectile_launch() {
        let rules = CombatRules {
            terrain_elevation: Some(original_map_terrain()),
            uphill_miss_chance_per_10k: UPHILL_MISS_CHANCE_SCALE,
            ..CombatRules::default()
        };
        let mut sim = Simulation::new_with_combat_rules(original_map_terrain_config(), 2, rules);
        sim.spawn_unit(terrain_attacker(
            0,
            wc3_point(0, 0),
            AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: 100 * SUBUNITS_PER_WORLD_UNIT,
            },
        ));
        sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: wc3_point(-6_000, 0),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step();
        let result = sim.step();
        assert_eq!(result.attacks_resolved, 1);
        assert_eq!(result.projectiles_launched, 0);
        assert!(sim.attacks_last_tick()[0].missed);
    }

    #[test]
    fn attack_buildings_do_not_use_unit_uphill_miss_rule() {
        let rules = CombatRules {
            terrain_elevation: Some(original_map_terrain()),
            uphill_miss_chance_per_10k: UPHILL_MISS_CHANCE_SCALE,
            ..CombatRules::default()
        };
        let mut sim = Simulation::new_with_combat_rules(original_map_terrain_config(), 2, rules);
        sim.spawn_building(attack_building(
            0,
            BuildingFootprint::new(0, 0, 1, 1),
            AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: 100 * SUBUNITS_PER_WORLD_UNIT,
                },
                damage: 7,
                range: 7_000 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 7_000 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1,
            },
        ));
        sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: wc3_point(-6_000, 0),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step();
        let result = sim.step();
        assert_eq!(result.projectiles_launched, 1);
        assert!(!sim.attacks_last_tick()[0].missed);
    }

    #[test]
    fn uphill_miss_is_worker_count_independent() {
        fn run(workers: usize) -> u64 {
            let rules = CombatRules {
                terrain_elevation: Some(original_map_terrain()),
                uphill_miss_chance_per_10k: 2_500,
                ..CombatRules::default()
            };
            let mut sim =
                Simulation::new_with_combat_rules(original_map_terrain_config(), workers, rules);
            sim.spawn_unit(terrain_attacker(0, wc3_point(0, 0), AttackDelivery::Melee));
            sim.spawn_unit(UnitSpawn {
                team: Team(1),
                position: wc3_point(-6_000, 0),
                health: 100_000,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    cooldown_ticks: 1,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            });

            let mut checksum = 0;
            for _ in 0..64 {
                checksum = sim.step().checksum;
            }
            checksum
        }

        assert_eq!(run(1), run(8));
    }

    #[test]
    fn topology_metrics_report_cage_opening_rebuild_and_group_release() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(15, 11),
            team_objective: [
                SimPoint::new(15 * cell + cell / 2, 5 * cell + cell / 2),
                SimPoint::new(cell / 2, 5 * cell + cell / 2),
            ],
            ..SimulationConfig::default()
        };
        config.static_blockers.clear();
        let mut sim = Simulation::new(config, 2);
        sim.spawn_building(passive_building(1, BuildingFootprint::new(8, 0, 1, 5)));
        let gate = sim.spawn_building(passive_building(1, BuildingFootprint::new(8, 5, 1, 1)));
        sim.spawn_building(passive_building(1, BuildingFootprint::new(8, 6, 1, 6)));

        let mut trapped = Vec::new();
        for y in 2..8 {
            for x in 2..6 {
                trapped.push(sim.spawn_unit(UnitSpawn {
                    team: Team(0),
                    position: SimPoint::new(x * cell + cell / 2, y * cell + cell / 2),
                    health: 100,
                    attack: AttackProfile {
                        delivery: AttackDelivery::Melee,
                        damage: 0,
                        range: 0,
                        acquisition_range: 0,
                        cooldown_ticks: 1,
                    },
                    movement: MovementProfile {
                        speed_per_tick: cell / 4,
                    },
                }));
            }
        }

        let initial_positions: Vec<_> = trapped
            .iter()
            .map(|id| sim.unit(*id).unwrap().position)
            .collect();
        let closed = sim.step();
        assert_eq!(closed.topology_rebuilds, 1);
        let pressing_toward_wall = trapped
            .iter()
            .zip(&initial_positions)
            .filter(|(id, position)| {
                sim.unit(**id)
                    .is_some_and(|unit| unit.position.x > position.x)
            })
            .count();
        assert!(
            pressing_toward_wall >= trapped.len() / 2,
            "closed cage should still press the crowd toward the objective-side wall"
        );
        assert!(
            trapped
                .iter()
                .all(|id| { sim.unit(*id).is_some_and(|unit| unit.position.x < 8 * cell) })
        );

        assert!(sim.remove_building(gate));
        let opened = sim.step();
        assert_eq!(opened.topology_rebuilds, 1);
        let moved = trapped
            .iter()
            .zip(&initial_positions)
            .filter(|(id, position)| {
                sim.unit(**id)
                    .is_some_and(|unit| unit.position != **position)
            })
            .count();
        assert!(
            moved >= trapped.len() / 2,
            "opened cage should release the crowd"
        );
    }

    #[test]
    fn pursuit_metrics_count_a_star_fallback_work() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(6, 4),
            static_blockers: vec![BuildingFootprint::new(3, 0, 1, 3)],
            team_objective: [
                SimPoint::new(6 * cell, 2 * cell),
                SimPoint::new(0, 2 * cell),
            ],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(2 * cell + cell / 2, cell + cell / 2),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: cell / 2,
                acquisition_range: 4 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile {
                speed_per_tick: cell / 4,
            },
        });
        sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(4 * cell + cell / 2, cell + cell / 2),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        let result = sim.step();
        assert_eq!(result.pursuit_steps, 1);
        assert_eq!(result.a_star_fallbacks, 1);
        assert!(result.a_star_expanded_nodes > 0);
    }

    #[test]
    fn units_do_not_attack_on_spawn_tick() {
        let mut sim = Simulation::new(SimulationConfig::default(), 1);
        let a = sim.spawn_unit(duel_unit(0, 0, 10, 100));
        let b = sim.spawn_unit(duel_unit(1, SUBUNITS_PER_WORLD_UNIT, 10, 100));

        let first = sim.step();
        assert_eq!(first.attacks_resolved, 0);
        assert_eq!(sim.unit(a).unwrap().health, 100);
        assert_eq!(sim.unit(b).unwrap().health, 100);

        let second = sim.step();
        assert_eq!(second.attacks_resolved, 2);
        assert_eq!(sim.unit(a).unwrap().health, 90);
        assert_eq!(sim.unit(b).unwrap().health, 90);
    }

    #[test]
    fn death_cancels_later_attack_in_canonical_order() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let first = sim.spawn_unit(duel_unit(0, 0, 100, 100));
        let second = sim.spawn_unit(duel_unit(1, SUBUNITS_PER_WORLD_UNIT, 100, 100));

        sim.step();
        let result = sim.step();

        assert_eq!(result.attacks_resolved, 1);
        assert_eq!(result.deaths, 1);
        assert_eq!(sim.unit(first).unwrap().health, 100);
        assert!(sim.unit(second).is_none());
    }

    #[test]
    fn corpse_lifecycle_is_tick_exact_and_worker_count_independent() {
        fn run(workers: usize) -> ([u64; 3], CorpseView) {
            let mut sim = Simulation::new(SimulationConfig::default(), workers);
            let attacker = sim.spawn_unit(duel_unit(0, 0, 100, 100));
            let victim_position = SimPoint::new(SUBUNITS_PER_WORLD_UNIT, 0);
            let victim = sim.spawn_unit_with_corpse(
                duel_unit(1, victim_position.x, 0, 100),
                CorpseProfile {
                    definition: CorpseDefinitionId(42),
                    lifetime_ticks: Some(2),
                },
            );

            sim.step();
            let death = sim.step();
            assert_eq!(death.deaths, 1);
            assert_eq!(death.corpses_spawned, 1);
            assert_eq!(death.corpses_alive, 1);
            assert_eq!(sim.unit_count(), 1);
            assert_eq!(sim.corpse_count(), 1);
            let corpse = sim.corpses()[0];
            assert!(corpse.id > victim);
            assert_eq!(corpse.position, victim_position);
            assert_eq!(corpse.source_unit, victim);
            assert_eq!(corpse.source_team, Team(1));
            assert_eq!(corpse.definition, CorpseDefinitionId(42));
            assert_eq!(corpse.created_tick, 1);
            assert_eq!(corpse.expires_tick, Some(3));
            assert_eq!(sim.unit(attacker).unwrap().health, 100);
            let after_death = sim.checksum();

            let retained = sim.step();
            assert_eq!(retained.corpses_expired, 0);
            assert_eq!(retained.corpses_alive, 1);
            let before_expiry = sim.checksum();

            let expired = sim.step();
            assert_eq!(expired.corpses_expired, 1);
            assert_eq!(expired.corpses_alive, 0);
            assert_eq!(sim.corpse_count(), 0);
            let after_expiry = sim.checksum();
            ([after_death, before_expiry, after_expiry], corpse)
        }

        let expected = run(1);
        assert_eq!(run(8), expected);
    }

    #[test]
    fn corpse_does_not_count_as_unit_or_block_building_placement() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * world, world / 2),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 100,
                range: 12 * world,
                acquisition_range: 20 * world,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let victim = sim.spawn_unit_with_corpse(
            UnitSpawn {
                team: Team(1),
                position: SimPoint::new(20 * world + world / 2, world / 2),
                health: 100,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    cooldown_ticks: 1,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
            CorpseProfile {
                definition: CorpseDefinitionId(7),
                lifetime_ticks: None,
            },
        );

        sim.step();
        sim.step();
        assert!(sim.unit(victim).is_none());
        assert_eq!(sim.unit_count(), 1);
        assert_eq!(sim.corpse_count(), 1);

        let building = sim.try_spawn_building(BuildingSpawn {
            team: Team(0),
            footprint: BuildingFootprint::new(20, 0, 1, 1),
            health: 100,
            production: None,
            attack: None,
            spellcasting: None,
        });
        assert!(
            building.is_ok(),
            "authoritative corpse blocked building placement"
        );
        assert_eq!(sim.corpse_count(), 1);
    }

    #[test]
    fn production_corpse_profile_is_inherited_by_spawned_units() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        sim.spawn_building_with_production_corpse(
            production_building(1, BuildingFootprint::new(10, 0, 1, 1), 2),
            CorpseProfile {
                definition: CorpseDefinitionId(19),
                lifetime_ticks: None,
            },
        );

        let production = sim.step();
        assert_eq!(production.units_spawned, 1);
        let produced = sim.units()[0].id;
        sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(12 * world + world / 2, world / 2),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 100,
                range: 4 * world,
                acquisition_range: 8 * world,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step();
        let death = sim.step();
        assert!(sim.unit(produced).is_none());
        assert_eq!(death.corpses_spawned, 1);
        let corpse = sim.corpses()[0];
        assert_eq!(corpse.source_unit, produced);
        assert_eq!(corpse.definition, CorpseDefinitionId(19));
        assert_eq!(corpse.expires_tick, None);
    }

    #[test]
    fn unit_gameplay_properties_combine_collision_and_corpse_profiles() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        sim.spawn_unit(duel_unit(0, 0, 100, 100));
        let victim = sim.spawn_unit_with_properties(
            duel_unit(1, world, 0, 100),
            UnitGameplayProperties {
                corpse: Some(CorpseProfile {
                    definition: CorpseDefinitionId(23),
                    lifetime_ticks: None,
                }),
                collision_radius: Some(CollisionRadius(world / 8)),
                ..UnitGameplayProperties::default()
            },
        );
        assert_eq!(sim.unit(victim).unwrap().collision_radius, world / 8);

        sim.step();
        sim.step();
        assert!(sim.unit(victim).is_none());
        let corpse = sim.corpses()[0];
        assert_eq!(corpse.source_unit, victim);
        assert_eq!(corpse.definition, CorpseDefinitionId(23));
    }

    #[test]
    fn ordinary_unit_death_does_not_create_corpse_without_profile() {
        let mut sim = Simulation::new(SimulationConfig::default(), 1);
        sim.spawn_unit(duel_unit(0, 0, 100, 100));
        sim.spawn_unit(duel_unit(1, SUBUNITS_PER_WORLD_UNIT, 0, 100));
        sim.step();
        let result = sim.step();
        assert_eq!(result.deaths, 1);
        assert_eq!(result.corpses_spawned, 0);
        assert_eq!(result.corpses_alive, 0);
    }

    #[test]
    fn exact_target_tie_uses_lowest_sim_id() {
        let mut sim = Simulation::new(SimulationConfig::default(), 3);
        let source = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1,
            },
            movement: MovementProfile {
                speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 8,
            },
        });
        let lower = sim.spawn_unit(duel_unit(1, 9 * SUBUNITS_PER_WORLD_UNIT, 0, 100));
        let _higher = sim.spawn_unit(duel_unit(1, 11 * SUBUNITS_PER_WORLD_UNIT, 0, 100));

        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(lower));
    }

    #[test]
    fn sticky_target_does_not_switch_to_new_non_attacker() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let source = sim.spawn_unit(duel_unit(0, 0, 0, 100));
        let first = sim.spawn_unit(passive_unit(1, 3 * SUBUNITS_PER_WORLD_UNIT));

        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(first));

        let _closer = sim.spawn_unit(passive_unit(1, SUBUNITS_PER_WORLD_UNIT));
        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(first));
    }

    #[test]
    fn fresh_attacker_preempts_target_that_is_not_fighting_back() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let source = sim.spawn_unit(duel_unit(0, 0, 0, 1_000));
        let passive = sim.spawn_unit(passive_unit(1, 3 * SUBUNITS_PER_WORLD_UNIT));
        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(passive));

        let attacker = sim.spawn_unit(duel_unit(1, SUBUNITS_PER_WORLD_UNIT, 1, 1_000));
        sim.step(); // attacker is spawn-tick suppressed
        sim.step(); // attacker hits source
        assert_eq!(sim.unit(source).unwrap().target, Some(passive));
        assert_eq!(sim.unit(source).unwrap().last_attacker, Some(attacker));
        assert_eq!(sim.unit(passive).unwrap().target, None);
        assert_eq!(sim.unit(attacker).unwrap().target, Some(source));
        sim.step(); // source reacts on the next targeting phase
        assert_eq!(sim.unit(source).unwrap().target, Some(attacker));
    }

    #[test]
    fn fresh_attacker_does_not_break_mutual_engagement() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let source = sim.spawn_unit(duel_unit(0, 0, 0, 1_000));
        let mutual = sim.spawn_unit(duel_unit(1, 2 * SUBUNITS_PER_WORLD_UNIT, 0, 1_000));
        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(mutual));
        assert_eq!(sim.unit(mutual).unwrap().target, Some(source));

        let attacker = sim.spawn_unit(duel_unit(1, SUBUNITS_PER_WORLD_UNIT, 1, 1_000));
        sim.step();
        sim.step();
        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(mutual));
        assert_ne!(sim.unit(source).unwrap().target, Some(attacker));
    }

    #[test]
    fn direct_retaliation_reaches_three_times_acquisition_range_and_stays_locked() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let source = sim.spawn_unit(duel_unit(0, 0, 0, 1_000));
        let decoy = sim.spawn_unit(passive_unit(1, 3 * cell));
        let attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(23 * cell, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: 100 * cell,
                },
                damage: 1,
                range: 24 * cell,
                acquisition_range: 24 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(decoy));
        sim.step();
        assert_eq!(sim.projectile_count(), 1);
        sim.step();
        let source_view = sim.unit(source).unwrap();
        assert_eq!(source_view.last_attacker, Some(attacker));
        assert!(source_view.last_attacked_tick.is_some());
        assert!(!source_view.direct_retaliation_lock);
        sim.step();
        let source_view = sim.unit(source).unwrap();
        assert_eq!(source_view.target, Some(attacker));
        assert!(source_view.direct_retaliation_lock);
        sim.step();
        let source_view = sim.unit(source).unwrap();
        assert_eq!(source_view.target, Some(attacker));
        assert!(source_view.direct_retaliation_lock);
    }

    #[test]
    fn direct_retaliation_does_not_extend_beyond_three_times_acquisition_range() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let source = sim.spawn_unit(duel_unit(0, 0, 0, 1_000));
        let decoy = sim.spawn_unit(passive_unit(1, 3 * cell));
        let _attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(25 * cell, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: 100 * cell,
                },
                damage: 1,
                range: 26 * cell,
                acquisition_range: 26 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(decoy));
        sim.step();
        sim.step();
        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(decoy));
    }

    #[test]
    fn unreachable_caged_unit_is_ignored_in_favor_of_cage_building() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let source = sim.spawn_unit(duel_unit(0, 6 * SUBUNITS_PER_WORLD_UNIT, 0, 1_000));
        let _caged = sim.spawn_unit(passive_unit(1, 10 * SUBUNITS_PER_WORLD_UNIT));
        let walls = [
            BuildingFootprint::new(9, -1, 1, 3),
            BuildingFootprint::new(11, -1, 1, 3),
            BuildingFootprint::new(10, -1, 1, 1),
            BuildingFootprint::new(10, 1, 1, 1),
        ];
        let first_wall = sim.spawn_building(passive_building(1, walls[0]));
        for footprint in walls.into_iter().skip(1) {
            sim.spawn_building(passive_building(1, footprint));
        }

        sim.step();
        let target = sim.unit(source).unwrap().target;
        assert!(target.is_some());
        assert_ne!(
            target,
            sim.units()
                .into_iter()
                .find(|unit| unit.team == Team(1))
                .map(|u| u.id)
        );
        assert!(sim.building(target.unwrap()).is_some());
        assert!(sim.building(first_wall).is_some());
    }

    #[test]
    fn reachable_enemy_unit_beats_passive_cage_building() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let source = sim.spawn_unit(duel_unit(0, 6 * SUBUNITS_PER_WORLD_UNIT, 0, 1_000));
        let reachable = sim.spawn_unit(passive_unit(1, 7 * SUBUNITS_PER_WORLD_UNIT));
        let _wall = sim.spawn_building(passive_building(1, BuildingFootprint::new(8, 0, 1, 1)));

        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(reachable));
    }

    #[test]
    fn bounded_spawn_search_fails_without_backlog_when_full() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let footprint = BuildingFootprint::new(20, 0, 1, 1);
        let preferred = SimPoint::new(
            21 * SUBUNITS_PER_WORLD_UNIT + SUBUNITS_PER_WORLD_UNIT / 2,
            SUBUNITS_PER_WORLD_UNIT / 2,
        );
        sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: preferred,
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.spawn_building(production_building(0, footprint, 0));

        let first = sim.step();
        assert_eq!(first.units_spawned, 0);
        assert_eq!(first.spawn_failures, 1);
        assert_eq!(sim.unit_count(), 1);

        for _ in 0..9 {
            let tick = sim.step();
            assert_eq!(tick.units_spawned, 0);
            assert_eq!(tick.spawn_failures, 0);
        }
        let retry = sim.step();
        assert_eq!(retry.units_spawned, 0);
        assert_eq!(retry.spawn_failures, 1);
        assert_eq!(sim.unit_count(), 1);
    }

    #[test]
    fn spawn_search_uses_deterministic_expanding_spiral() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let footprint = BuildingFootprint::new(20, 0, 1, 1);
        let preferred = SimPoint::new(
            21 * SUBUNITS_PER_WORLD_UNIT + SUBUNITS_PER_WORLD_UNIT / 2,
            SUBUNITS_PER_WORLD_UNIT / 2,
        );
        let blocker = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: preferred,
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.spawn_building(production_building(0, footprint, 1));

        let result = sim.step();
        assert_eq!(result.units_spawned, 1);
        assert_eq!(result.spawn_failures, 0);
        let spawned = sim
            .units()
            .into_iter()
            .find(|unit| unit.id != blocker)
            .expect("production unit missing");
        assert_eq!(
            spawned.position,
            SimPoint::new(
                20 * SUBUNITS_PER_WORLD_UNIT + SUBUNITS_PER_WORLD_UNIT / 2,
                -SUBUNITS_PER_WORLD_UNIT / 2,
            )
        );
    }

    #[test]
    fn production_spawn_respects_collision_radius_across_neighbor_cells() {
        let config = SimulationConfig::default();
        let minimum_distance = config.unit_separation_distance;
        let mut sim = Simulation::new(config, 2);
        let footprint = BuildingFootprint::new(20, 0, 1, 1);
        let preferred = SimPoint::new(
            21 * SUBUNITS_PER_WORLD_UNIT + SUBUNITS_PER_WORLD_UNIT / 2,
            SUBUNITS_PER_WORLD_UNIT / 2,
        );
        let nearby = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(
                22 * SUBUNITS_PER_WORLD_UNIT + SUBUNITS_PER_WORLD_UNIT / 100,
                SUBUNITS_PER_WORLD_UNIT / 2,
            ),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.spawn_building(production_building(0, footprint, 1));

        let result = sim.step();
        assert_eq!(result.units_spawned, 1);
        let spawned = sim
            .units()
            .into_iter()
            .find(|unit| unit.id != nearby)
            .expect("production unit missing");
        assert_ne!(spawned.position, preferred);
        assert!(
            spawned
                .position
                .distance_sq(sim.unit(nearby).unwrap().position)
                >= (i64::from(minimum_distance) * i64::from(minimum_distance)) as u64
        );
    }

    #[test]
    fn ranged_attack_building_targets_caged_unit_without_navigation_route() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let tower = sim.spawn_building(attack_building(
            0,
            BuildingFootprint::new(4, 0, 2, 2),
            AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: 2 * cell,
                },
                damage: 3,
                range: 20 * cell,
                acquisition_range: 20 * cell,
                cooldown_ticks: 30,
            },
        ));
        let target = sim.spawn_unit(passive_unit(1, 12 * cell));
        for footprint in [
            BuildingFootprint::new(11, -1, 1, 3),
            BuildingFootprint::new(13, -1, 1, 3),
            BuildingFootprint::new(12, -1, 1, 1),
            BuildingFootprint::new(12, 1, 1, 1),
        ] {
            sim.spawn_building(passive_building(1, footprint));
        }

        let acquire = sim.step();
        assert_eq!(acquire.attacks_resolved, 0);
        assert_eq!(sim.building(tower).unwrap().target, Some(target));

        let launch = sim.step();
        assert_eq!(launch.attacks_resolved, 1);
        assert_eq!(launch.projectiles_launched, 1);
        assert_eq!(sim.projectiles()[0].source, tower);
        assert_eq!(sim.unit(target).unwrap().health, 10_000);

        while sim.projectile_count() != 0 {
            sim.step();
        }
        assert_eq!(sim.unit(target).unwrap().health, 9_997);
    }

    #[test]
    fn attack_building_manual_order_overrides_nearer_automatic_target() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 1);
        let tower = sim.spawn_building(attack_building(
            0,
            BuildingFootprint::new(4, 0, 2, 2),
            AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: 4 * cell,
                },
                damage: 3,
                range: 30 * cell,
                acquisition_range: 30 * cell,
                cooldown_ticks: 30,
            },
        ));
        let nearer = sim.spawn_unit(passive_unit(1, 8 * cell));
        let ordered = sim.spawn_unit(passive_unit(1, 14 * cell));

        sim.order_building_attack_target(tower, ordered).unwrap();
        sim.step();
        assert_eq!(sim.building(tower).unwrap().target, Some(ordered));
        assert_ne!(sim.building(tower).unwrap().target, Some(nearer));
    }

    #[test]
    fn attack_buildings_choose_targets_independently() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 4);
        let attack = AttackProfile {
            delivery: AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: 4 * cell,
            },
            damage: 1,
            range: 30 * cell,
            acquisition_range: 30 * cell,
            cooldown_ticks: 30,
        };
        let left_tower = sim.spawn_building(attack_building(
            0,
            BuildingFootprint::new(10, 0, 1, 1),
            attack,
        ));
        let right_tower = sim.spawn_building(attack_building(
            0,
            BuildingFootprint::new(30, 0, 1, 1),
            attack,
        ));
        let left_target = sim.spawn_unit(passive_unit(1, 13 * cell));
        let right_target = sim.spawn_unit(passive_unit(1, 33 * cell));

        sim.step();
        assert_eq!(sim.building(left_tower).unwrap().target, Some(left_target));
        assert_eq!(
            sim.building(right_tower).unwrap().target,
            Some(right_target)
        );
    }

    #[test]
    fn attack_building_worker_count_is_deterministic() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut expected = None;
        for workers in [1, 2, 8] {
            let mut sim = Simulation::new(SimulationConfig::default(), workers);
            let attack = AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: 8 * cell,
                },
                damage: 1,
                range: 100 * cell,
                acquisition_range: 100 * cell,
                cooldown_ticks: 2,
            };
            for (team, x) in [(0, 10), (0, 20), (1, 100), (1, 110)] {
                sim.spawn_building(attack_building(
                    team,
                    BuildingFootprint::new(x, -10, 1, 1),
                    attack,
                ));
            }
            for index in 0..20 {
                let team = (index % 2) as u8;
                let x = if team == 0 { 45 } else { 75 };
                sim.spawn_unit(UnitSpawn {
                    team: Team(team),
                    position: SimPoint::new(x * cell, (index - 10) * cell),
                    ..passive_unit(team, x * cell)
                });
            }
            for _ in 0..20 {
                sim.step();
            }
            match expected {
                None => expected = Some(sim.checksum()),
                Some(checksum) => assert_eq!(sim.checksum(), checksum),
            }
        }
    }

    #[test]
    fn automatic_spell_mana_regen_cost_and_cooldown_are_tick_exact() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let caster = sim.spawn_building(spell_building(
            0,
            BuildingFootprint::new(10, 0, 1, 1),
            SpellcastingProfile {
                mana: ManaProfile {
                    maximum: 6,
                    starting: 0,
                    regen_per_tick_per_10k: 20_000,
                },
                ability: AutomaticAbilityProfile {
                    id: AbilityId(7),
                    mana_cost: 6,
                    cooldown_ticks: 3,
                    range: 8 * cell,
                    target_policy: AbilityTargetPolicy::RandomEnemyUnit,
                    effect: AbilityEffect::Damage { amount: 1 },
                },
            },
        ));
        let target = sim.spawn_unit(passive_unit(1, 15 * cell));

        let tick0 = sim.step();
        assert_eq!(tick0.ability_evaluations, 1);
        assert_eq!(tick0.ability_casts, 0);
        assert_eq!(sim.building(caster).unwrap().mana_current, Some(2));

        let tick1 = sim.step();
        assert_eq!(tick1.ability_casts, 0);
        assert_eq!(sim.building(caster).unwrap().mana_current, Some(4));

        let tick2 = sim.step();
        assert_eq!(tick2.ability_casts, 1);
        assert_eq!(tick2.ability_effects, 1);
        assert_eq!(sim.unit(target).unwrap().health, 9_999);
        let view = sim.building(caster).unwrap();
        assert_eq!(view.mana_current, Some(0));
        assert_eq!(view.ability_ready_tick, Some(5));
        assert_eq!(view.ability_cast_sequence, Some(1));
        assert_eq!(
            sim.ability_casts_last_tick(),
            &[AbilityCastEvent {
                source: caster,
                ability: AbilityId(7),
                target: AbilityCastTarget::Unit(target),
                target_position: Some(SimPoint::new(15 * cell, 0)),
                effect: AbilityEffect::Damage { amount: 1 },
            }]
        );

        sim.step();
        sim.step();
        assert_eq!(sim.unit(target).unwrap().health, 9_999);
        let tick5 = sim.step();
        assert_eq!(tick5.completed_tick, 5);
        assert_eq!(tick5.ability_casts, 1);
        assert_eq!(sim.unit(target).unwrap().health, 9_998);
        assert_eq!(sim.building(caster).unwrap().ability_cast_sequence, Some(2));
    }

    #[test]
    fn global_random_area_spell_casts_at_full_mana_and_hits_live_cluster() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let caster = sim.spawn_building(spell_building(
            0,
            BuildingFootprint::new(2, 0, 1, 1),
            SpellcastingProfile {
                mana: ManaProfile {
                    maximum: 2,
                    starting: 0,
                    regen_per_tick_per_10k: 10_000,
                },
                ability: AutomaticAbilityProfile {
                    id: AbilityId(8),
                    mana_cost: 2,
                    cooldown_ticks: 1,
                    range: 0,
                    target_policy: AbilityTargetPolicy::RandomEnemyUnitGlobal,
                    effect: AbilityEffect::AreaDamage {
                        amount: 3,
                        radius: 2 * cell,
                    },
                },
            },
        ));
        let targets = [
            sim.spawn_unit(passive_unit(1, 60 * cell)),
            sim.spawn_unit(passive_unit(1, 61 * cell)),
            sim.spawn_unit(passive_unit(1, 80 * cell)),
        ];

        let charging = sim.step();
        assert_eq!(charging.ability_casts, 0);
        assert_eq!(sim.building(caster).unwrap().mana_current, Some(1));

        let cast = sim.step();
        assert_eq!(cast.ability_casts, 1);
        assert_eq!(sim.building(caster).unwrap().mana_current, Some(0));
        let event = sim.ability_casts_last_tick()[0];
        let AbilityCastTarget::Unit(selected) = event.target else {
            panic!("global random AOE should select one enemy unit as its center");
        };
        assert!(targets.contains(&selected));
        let center = sim.unit(selected).unwrap().position;
        assert_eq!(event.target_position, Some(center));
        let area_radius_sq = u64::try_from(2 * cell).unwrap().pow(2);
        let expected_hits = targets
            .iter()
            .filter(|id| sim.unit(**id).unwrap().position.distance_sq(center) <= area_radius_sq)
            .count();
        assert_eq!(cast.ability_effects, expected_hits);
        for target in targets {
            let view = sim.unit(target).unwrap();
            let expected_health = if view.position.distance_sq(center) <= area_radius_sq {
                9_997
            } else {
                10_000
            };
            assert_eq!(view.health, expected_health);
        }
    }

    #[test]
    fn combat_unit_can_cast_short_range_random_area_spell() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let caster = sim.spawn_unit_with_spellcasting(
            UnitSpawn {
                team: Team(0),
                position: SimPoint::new(10 * cell, 0),
                health: 100,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    cooldown_ticks: 1,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
            SpellcastingProfile {
                mana: ManaProfile {
                    maximum: 1,
                    starting: 1,
                    regen_per_tick_per_10k: 0,
                },
                ability: AutomaticAbilityProfile {
                    id: AbilityId(9),
                    mana_cost: 1,
                    cooldown_ticks: 10,
                    range: 4 * cell,
                    target_policy: AbilityTargetPolicy::RandomEnemyUnit,
                    effect: AbilityEffect::AreaDamage {
                        amount: 2,
                        radius: cell,
                    },
                },
            },
        );
        let near = sim.spawn_unit(passive_unit(1, 13 * cell));
        let far = sim.spawn_unit(passive_unit(1, 25 * cell));

        let result = sim.step();
        assert_eq!(result.ability_evaluations, 1);
        assert_eq!(result.ability_casts, 1);
        assert_eq!(result.ability_effects, 1);
        assert_eq!(
            sim.ability_casts_last_tick(),
            &[AbilityCastEvent {
                source: caster,
                ability: AbilityId(9),
                target: AbilityCastTarget::Unit(near),
                target_position: Some(SimPoint::new(13 * cell, 0)),
                effect: AbilityEffect::AreaDamage {
                    amount: 2,
                    radius: cell,
                },
            }]
        );
        assert_eq!(sim.unit(near).unwrap().health, 9_998);
        assert_eq!(sim.unit(far).unwrap().health, 10_000);
        let caster_view = sim.unit(caster).unwrap();
        assert_eq!(caster_view.mana_current, Some(0));
        assert_eq!(caster_view.mana_maximum, Some(1));
        assert_eq!(caster_view.ability_cast_sequence, Some(1));
    }

    #[test]
    fn production_spellcasting_profile_is_inherited_by_spawned_units() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let profile = SpellcastingProfile {
            mana: ManaProfile {
                maximum: 10,
                starting: 0,
                regen_per_tick_per_10k: 10_000,
            },
            ability: AutomaticAbilityProfile {
                id: AbilityId(10),
                mana_cost: 10,
                cooldown_ticks: 1,
                range: 4 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::RandomEnemyUnit,
                effect: AbilityEffect::AreaDamage {
                    amount: 1,
                    radius: SUBUNITS_PER_WORLD_UNIT,
                },
            },
        };
        sim.spawn_building_with_production_spellcasting(
            production_building(0, BuildingFootprint::new(10, 0, 1, 1), 4),
            profile,
        );

        let result = sim.step();
        assert_eq!(result.units_spawned, 1);
        let spawned = sim.units().into_iter().next().unwrap();
        assert_eq!(spawned.mana_maximum, Some(10));
        assert_eq!(spawned.mana_current, Some(0));
        assert_eq!(spawned.ability_cast_sequence, Some(0));

        sim.step();
        let regenerated = sim.unit(spawned.id).unwrap();
        assert_eq!(regenerated.mana_current, Some(1));
        assert_eq!(regenerated.ability_cast_sequence, Some(0));
    }

    #[test]
    fn building_spell_damage_does_not_create_retaliation_or_ally_defense() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let victim = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 2 * cell,
                acquisition_range: 8 * cell,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let nearby_ally = sim.spawn_unit(passive_unit(0, 9 * cell));
        let decoy = sim.spawn_unit(passive_unit(1, 12 * cell));
        let caster = sim.spawn_building(spell_building(
            1,
            BuildingFootprint::new(14, 0, 1, 1),
            SpellcastingProfile {
                mana: ManaProfile {
                    maximum: 10,
                    starting: 10,
                    regen_per_tick_per_10k: 0,
                },
                ability: AutomaticAbilityProfile {
                    id: AbilityId(11),
                    mana_cost: 1,
                    cooldown_ticks: 30,
                    range: 8 * cell,
                    target_policy: AbilityTargetPolicy::RandomEnemyUnit,
                    effect: AbilityEffect::Damage { amount: 1 },
                },
            },
        ));

        let cast = sim.step();
        assert_eq!(cast.ability_casts, 1);
        assert_eq!(sim.ability_casts_last_tick()[0].source, caster);
        assert_eq!(
            sim.ability_casts_last_tick()[0].target,
            AbilityCastTarget::Unit(victim)
        );
        assert_eq!(sim.unit(victim).unwrap().health, 99);
        assert_eq!(sim.unit(victim).unwrap().last_attacker, None);
        assert_eq!(sim.unit(victim).unwrap().target, Some(decoy));
        assert_eq!(sim.unit(nearby_ally).unwrap().target, None);

        let next = sim.step();
        assert_eq!(next.ally_defense_queries, 0);
        assert_eq!(sim.unit(victim).unwrap().target, Some(decoy));
        assert_eq!(sim.unit(nearby_ally).unwrap().target, None);
    }

    #[test]
    fn automatic_spell_kill_suppresses_later_ordinary_attack() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let victim = sim.spawn_unit(passive_unit(0, 10 * cell));
        let attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(12 * cell, 0),
            health: 5,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 10,
                range: 4 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step();
        assert_eq!(sim.unit(attacker).unwrap().target, Some(victim));
        sim.spawn_building(spell_building(
            0,
            BuildingFootprint::new(16, 0, 1, 1),
            SpellcastingProfile {
                mana: ManaProfile {
                    maximum: 10,
                    starting: 10,
                    regen_per_tick_per_10k: 0,
                },
                ability: AutomaticAbilityProfile {
                    id: AbilityId(13),
                    mana_cost: 1,
                    cooldown_ticks: 30,
                    range: 8 * cell,
                    target_policy: AbilityTargetPolicy::RandomEnemyUnit,
                    effect: AbilityEffect::Damage { amount: 5 },
                },
            },
        ));

        let result = sim.step();
        assert_eq!(result.ability_casts, 1);
        assert_eq!(result.attacks_resolved, 0);
        assert!(sim.unit(attacker).is_none());
        assert_eq!(sim.unit(victim).unwrap().health, 10_000);
    }

    #[test]
    fn one_tick_global_stun_cancels_existing_attack_then_expires() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let victim = sim.spawn_unit(passive_unit(0, 10 * cell));
        let attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(12 * cell, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 5,
                range: 4 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step();
        assert_eq!(sim.unit(attacker).unwrap().target, Some(victim));
        sim.spawn_building(spell_building(
            0,
            BuildingFootprint::new(20, 0, 1, 1),
            global_stun_spell(21, 1, 30),
        ));

        let stunned = sim.step();
        assert_eq!(stunned.completed_tick, 1);
        assert_eq!(stunned.ability_casts, 1);
        assert_eq!(stunned.ability_effects, 1);
        assert_eq!(stunned.attacks_resolved, 0);
        let attacker_view = sim.unit(attacker).unwrap();
        assert_eq!(attacker_view.stunned_until_tick, 2);
        assert_eq!(attacker_view.target, Some(victim));
        assert_eq!(sim.unit(victim).unwrap().health, 10_000);
        assert_eq!(
            sim.ability_casts_last_tick()[0].target,
            AbilityCastTarget::AllEnemyUnits
        );

        let recovered = sim.step();
        assert_eq!(recovered.completed_tick, 2);
        assert_eq!(recovered.attacks_resolved, 1);
        assert_eq!(sim.unit(victim).unwrap().health, 9_995);
    }

    #[test]
    fn two_tick_global_stun_suppresses_acquisition_and_movement_exactly() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let target = sim.spawn_unit(passive_unit(0, 10 * cell));
        let mover = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(20 * cell, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: cell,
                acquisition_range: 20 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile {
                speed_per_tick: cell,
            },
        });
        sim.spawn_building(spell_building(
            0,
            BuildingFootprint::new(30, 0, 1, 1),
            global_stun_spell(22, 2, 30),
        ));
        let initial = sim.unit(mover).unwrap().position;

        let tick0 = sim.step();
        assert_eq!(tick0.ability_casts, 1);
        assert_eq!(sim.unit(mover).unwrap().stunned_until_tick, 2);
        assert_eq!(sim.unit(mover).unwrap().target, None);
        assert_eq!(sim.unit(mover).unwrap().position, initial);

        sim.step();
        assert_eq!(sim.unit(mover).unwrap().target, None);
        assert_eq!(sim.unit(mover).unwrap().position, initial);

        let recovered = sim.step();
        assert_eq!(recovered.completed_tick, 2);
        assert_eq!(sim.unit(mover).unwrap().target, Some(target));
        assert_ne!(sim.unit(mover).unwrap().position, initial);
    }

    #[test]
    fn shorter_stun_does_not_truncate_longer_stun() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let victim = sim.spawn_unit(passive_unit(1, 20 * cell));
        sim.spawn_building(spell_building(
            0,
            BuildingFootprint::new(4, 0, 1, 1),
            global_stun_spell(23, 4, 30),
        ));
        sim.spawn_building(spell_building(
            0,
            BuildingFootprint::new(6, 0, 1, 1),
            global_stun_spell(24, 1, 30),
        ));

        let result = sim.step();
        assert_eq!(result.ability_casts, 2);
        assert_eq!(result.ability_effects, 2);
        assert_eq!(sim.unit(victim).unwrap().stunned_until_tick, 4);
    }

    #[test]
    fn global_stun_is_worker_count_independent() {
        fn run(workers: usize) -> u64 {
            let cell = SUBUNITS_PER_WORLD_UNIT;
            let mut sim = Simulation::new(SimulationConfig::default(), workers);
            for index in 0..4 {
                sim.spawn_building(spell_building(
                    0,
                    BuildingFootprint::new(4 + index * 2, -20, 1, 1),
                    global_stun_spell(30 + index as u32, 2, 3),
                ));
            }
            for index in 0..100 {
                sim.spawn_unit(UnitSpawn {
                    team: Team(1),
                    position: SimPoint::new((40 + index % 20) * cell, (index / 20 - 2) * cell),
                    health: 100,
                    attack: AttackProfile {
                        delivery: AttackDelivery::Melee,
                        damage: 1,
                        range: 2 * cell,
                        acquisition_range: 12 * cell,
                        cooldown_ticks: 3,
                    },
                    movement: MovementProfile {
                        speed_per_tick: cell / 2,
                    },
                });
            }
            for _ in 0..30 {
                sim.step();
            }
            sim.checksum()
        }

        let expected = run(1);
        assert_eq!(run(2), expected);
        assert_eq!(run(8), expected);
    }

    #[test]
    fn timed_movement_slow_applies_and_expires_exactly() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let start = SimPoint::new(20 * cell + cell / 2, cell / 2);
        let mover = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: start,
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile {
                speed_per_tick: cell / 2,
            },
        });
        sim.spawn_building(spell_building(
            0,
            BuildingFootprint::new(30, 10, 1, 1),
            global_movement_modifier_spell(40, 1, -50, 2, 30),
        ));

        sim.step();
        assert_eq!(sim.unit(mover).unwrap().position.x, start.x - cell / 4);
        sim.step();
        assert_eq!(sim.unit(mover).unwrap().position.x, start.x - cell / 2);
        sim.step();
        assert_eq!(sim.unit(mover).unwrap().position.x, start.x - cell);
    }

    #[test]
    fn same_movement_modifier_id_refreshes_without_stacking() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let start = SimPoint::new(20 * cell + cell / 2, cell / 2);
        let mover = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: start,
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile {
                speed_per_tick: cell / 2,
            },
        });
        sim.spawn_building(spell_building(
            0,
            BuildingFootprint::new(30, 10, 1, 1),
            global_movement_modifier_spell(41, 9, -50, 4, 30),
        ));
        sim.spawn_building(spell_building(
            0,
            BuildingFootprint::new(32, 10, 1, 1),
            global_movement_modifier_spell(42, 9, -50, 1, 30),
        ));

        for expected_quarters in 1..=4 {
            sim.step();
            assert_eq!(
                sim.unit(mover).unwrap().position.x,
                start.x - expected_quarters * (cell / 4)
            );
        }
        sim.step();
        assert_eq!(sim.unit(mover).unwrap().position.x, start.x - 3 * cell / 2);
    }

    #[test]
    fn distinct_movement_modifier_ids_stack_additively() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let start = SimPoint::new(20 * cell + cell / 2, cell / 2);
        let mover = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: start,
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile {
                speed_per_tick: cell / 2,
            },
        });
        sim.spawn_building(spell_building(
            0,
            BuildingFootprint::new(30, 10, 1, 1),
            global_movement_modifier_spell(43, 10, -30, 2, 30),
        ));
        sim.spawn_building(spell_building(
            0,
            BuildingFootprint::new(32, 10, 1, 1),
            global_movement_modifier_spell(44, 11, -30, 2, 30),
        ));

        sim.step();
        assert_eq!(
            sim.unit(mover).unwrap().position.x,
            start.x - (cell / 2) * 40 / 100
        );
    }

    #[test]
    fn timed_movement_modifiers_are_worker_count_independent() {
        fn run(workers: usize) -> u64 {
            let cell = SUBUNITS_PER_WORLD_UNIT;
            let mut sim = Simulation::new(SimulationConfig::default(), workers);
            for index in 0..4 {
                sim.spawn_building(spell_building(
                    0,
                    BuildingFootprint::new(4 + index * 2, -20, 1, 1),
                    global_movement_modifier_spell(
                        50 + index as u32,
                        20 + (index % 2) as u32,
                        -15,
                        3,
                        4,
                    ),
                ));
            }
            for index in 0..100 {
                sim.spawn_unit(UnitSpawn {
                    team: Team(1),
                    position: SimPoint::new(
                        (40 + index % 20) * cell + cell / 2,
                        (index / 20 - 2) * cell + cell / 2,
                    ),
                    health: 100,
                    attack: AttackProfile {
                        delivery: AttackDelivery::Melee,
                        damage: 0,
                        range: 0,
                        acquisition_range: 0,
                        cooldown_ticks: 1,
                    },
                    movement: MovementProfile {
                        speed_per_tick: cell / 2,
                    },
                });
            }
            for _ in 0..30 {
                sim.step();
            }
            sim.checksum()
        }

        let expected = run(1);
        assert_eq!(run(2), expected);
        assert_eq!(run(8), expected);
    }

    #[test]
    fn automatic_spellcasting_is_worker_count_independent() {
        fn run(workers: usize) -> u64 {
            let cell = SUBUNITS_PER_WORLD_UNIT;
            let mut config = SimulationConfig {
                match_seed: 0x5eed_ab11_17e5_2026,
                ..SimulationConfig::default()
            };
            config.static_blockers.clear();
            let mut sim = Simulation::new(config, workers);
            let spellcasting = SpellcastingProfile {
                mana: ManaProfile {
                    maximum: 1_000,
                    starting: 1_000,
                    regen_per_tick_per_10k: 10_000,
                },
                ability: AutomaticAbilityProfile {
                    id: AbilityId(17),
                    mana_cost: 1,
                    cooldown_ticks: 1,
                    range: 100 * cell,
                    target_policy: AbilityTargetPolicy::RandomEnemyUnit,
                    effect: AbilityEffect::Damage { amount: 1 },
                },
            };
            for index in 0..16 {
                sim.spawn_building(spell_building(
                    0,
                    BuildingFootprint::new(4 + index, -20, 1, 1),
                    spellcasting,
                ));
            }
            for index in 0..40 {
                sim.spawn_unit(UnitSpawn {
                    health: 10_000,
                    ..passive_unit(1, (40 + index) * cell)
                });
            }
            for _ in 0..50 {
                sim.step();
            }
            assert_eq!(
                sim.buildings()
                    .iter()
                    .filter_map(|building| building.ability_cast_sequence)
                    .sum::<u64>(),
                800
            );
            sim.checksum()
        }

        let expected = run(1);
        assert_eq!(run(2), expected);
        assert_eq!(run(8), expected);
    }

    #[test]
    fn unit_and_global_area_spellcasting_are_worker_count_independent() {
        fn run(workers: usize) -> u64 {
            let cell = SUBUNITS_PER_WORLD_UNIT;
            let config = SimulationConfig {
                match_seed: 0xaea0_2026_0913,
                ..SimulationConfig::default()
            };
            let mut sim = Simulation::new(config, workers);
            let global_spell = SpellcastingProfile {
                mana: ManaProfile {
                    maximum: 1_000,
                    starting: 1_000,
                    regen_per_tick_per_10k: 10_000,
                },
                ability: AutomaticAbilityProfile {
                    id: AbilityId(71),
                    mana_cost: 1,
                    cooldown_ticks: 1,
                    range: 0,
                    target_policy: AbilityTargetPolicy::RandomEnemyUnitGlobal,
                    effect: AbilityEffect::AreaDamage {
                        amount: 1,
                        radius: 2 * cell,
                    },
                },
            };
            let local_spell = SpellcastingProfile {
                mana: ManaProfile {
                    maximum: 1_000,
                    starting: 1_000,
                    regen_per_tick_per_10k: 10_000,
                },
                ability: AutomaticAbilityProfile {
                    id: AbilityId(72),
                    mana_cost: 1,
                    cooldown_ticks: 1,
                    range: 12 * cell,
                    target_policy: AbilityTargetPolicy::RandomEnemyUnit,
                    effect: AbilityEffect::AreaDamage {
                        amount: 1,
                        radius: 2 * cell,
                    },
                },
            };
            for index in 0..4 {
                sim.spawn_building(spell_building(
                    0,
                    BuildingFootprint::new(4 + index * 2, -20, 1, 1),
                    global_spell,
                ));
            }
            for index in 0..8 {
                sim.spawn_unit_with_spellcasting(
                    UnitSpawn {
                        team: Team(0),
                        position: SimPoint::new(40 * cell, (index - 4) * 2 * cell),
                        health: 10_000,
                        attack: AttackProfile {
                            delivery: AttackDelivery::Melee,
                            damage: 0,
                            range: 0,
                            acquisition_range: 0,
                            cooldown_ticks: 1,
                        },
                        movement: MovementProfile { speed_per_tick: 0 },
                    },
                    local_spell,
                );
            }
            for index in 0..40 {
                sim.spawn_unit(UnitSpawn {
                    team: Team(1),
                    position: SimPoint::new((44 + index % 8) * cell, (index / 8 - 2) * 2 * cell),
                    health: 10_000,
                    ..passive_unit(1, 0)
                });
            }
            for _ in 0..20 {
                sim.step();
            }
            assert_eq!(
                sim.buildings()
                    .iter()
                    .filter_map(|building| building.ability_cast_sequence)
                    .sum::<u64>(),
                80
            );
            assert_eq!(
                sim.units()
                    .iter()
                    .filter(|unit| unit.mana_maximum.is_some())
                    .filter_map(|unit| unit.ability_cast_sequence)
                    .sum::<u64>(),
                160
            );
            sim.checksum()
        }

        let expected = run(1);
        assert_eq!(run(2), expected);
        assert_eq!(run(8), expected);
    }

    #[test]
    fn attack_building_does_not_preempt_valid_unit_target() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 2 * cell,
                acquisition_range: 8 * cell,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: cell / 8,
            },
        });
        let decoy = sim.spawn_unit(passive_unit(1, 12 * cell));
        let tower = sim.spawn_building(attack_building(
            1,
            BuildingFootprint::new(20, 0, 1, 1),
            AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: 10 * cell,
                },
                damage: 1,
                range: 12 * cell,
                acquisition_range: 12 * cell,
                cooldown_ticks: 30,
            },
        ));

        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(decoy));
        sim.step();
        assert_eq!(sim.projectile_count(), 1);
        let impact = sim.step();
        assert_eq!(impact.projectile_impacts, 1);
        assert_eq!(sim.unit(defender).unwrap().health, 99);
        assert_eq!(sim.unit(defender).unwrap().last_attacker, Some(tower));
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(decoy));
    }

    #[test]
    fn unit_retaliates_against_attack_building_when_no_unit_target_exists() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 2 * cell,
                acquisition_range: 8 * cell,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: cell / 8,
            },
        });
        let tower = sim.spawn_building(attack_building(
            1,
            BuildingFootprint::new(20, 0, 1, 1),
            AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: 10 * cell,
                },
                damage: 1,
                range: 12 * cell,
                acquisition_range: 12 * cell,
                cooldown_ticks: 30,
            },
        ));

        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, None);
        sim.step();
        assert_eq!(sim.projectile_count(), 1);
        let impact = sim.step();
        assert_eq!(impact.projectile_impacts, 1);
        assert_eq!(sim.unit(defender).unwrap().health, 99);
        assert_eq!(sim.unit(defender).unwrap().last_attacker, Some(tower));
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(tower));
        assert!(sim.unit(defender).unwrap().direct_retaliation_lock);
    }

    #[test]
    fn earlier_unit_kill_cancels_later_attack_building_action() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let attacker = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 3 * cell,
                acquisition_range: 3 * cell,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let tower = sim.spawn_building(BuildingSpawn {
            team: Team(1),
            footprint: BuildingFootprint::new(11, 0, 1, 1),
            health: 1,
            production: None,
            attack: Some(AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: 4 * cell,
                },
                damage: 100,
                range: 3 * cell,
                acquisition_range: 3 * cell,
                cooldown_ticks: 30,
            }),
            spellcasting: None,
        });

        sim.step();
        assert_eq!(sim.unit(attacker).unwrap().target, Some(tower));
        assert_eq!(sim.building(tower).unwrap().target, Some(attacker));
        let combat = sim.step();
        assert_eq!(combat.attacks_resolved, 1);
        assert_eq!(combat.projectiles_launched, 0);
        assert!(sim.building(tower).is_none());
        assert_eq!(sim.unit(attacker).unwrap().health, 100);
    }

    #[test]
    fn ranged_guaranteed_hit_crosses_cage_with_authoritative_travel_time() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let source = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(6 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
                },
                damage: 1,
                range: 8 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let caged = sim.spawn_unit(passive_unit(1, 10 * SUBUNITS_PER_WORLD_UNIT));
        for footprint in [
            BuildingFootprint::new(9, -1, 1, 3),
            BuildingFootprint::new(11, -1, 1, 3),
            BuildingFootprint::new(10, -1, 1, 1),
            BuildingFootprint::new(10, 1, 1, 1),
        ] {
            sim.spawn_building(passive_building(1, footprint));
        }

        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(caged));
        let launch = sim.step();
        assert_eq!(launch.completed_tick, 1);
        assert_eq!(launch.projectiles_launched, 1);
        assert_eq!(launch.projectiles_alive, 1);
        assert_eq!(launch.projectile_impacts, 0);
        assert_eq!(sim.unit(caged).unwrap().health, 10_000);
        assert_eq!(sim.attacks_last_tick().len(), 1);
        let projectile = sim.projectiles()[0];
        assert_eq!(projectile.launch_tick, 1);
        assert_eq!(projectile.impact_tick, 5);
        assert_eq!(projectile.source, source);
        assert_eq!(
            projectile.kind,
            ProjectileViewKind::GuaranteedHit { target: caged }
        );

        for expected_tick in 2..5 {
            let in_flight = sim.step();
            assert_eq!(in_flight.completed_tick, expected_tick);
            assert_eq!(in_flight.projectile_impacts, 0);
            assert_eq!(in_flight.projectiles_alive, 1);
            assert_eq!(sim.unit(caged).unwrap().health, 10_000);
        }

        let impact = sim.step();
        assert_eq!(impact.completed_tick, 5);
        assert_eq!(impact.projectile_impacts, 1);
        assert_eq!(impact.projectile_invalidations, 0);
        assert_eq!(impact.projectiles_alive, 0);
        assert_eq!(sim.unit(caged).unwrap().health, 9_999);
    }

    #[test]
    fn ballistic_projectile_misses_original_target_after_it_moves_out_of_zone() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            team_objective: [SimPoint::new(0, 0), SimPoint::new(20 * cell, 0)],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        let source = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(0, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedBallistic {
                    speed_per_tick: 2 * cell,
                    impact_radius: cell / 2,
                },
                damage: 7,
                range: 10 * cell,
                acquisition_range: 10 * cell,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let target = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(4 * cell, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile {
                speed_per_tick: cell,
            },
        });

        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(target));
        let target_position_at_launch = sim.unit(target).unwrap().position;
        let launch = sim.step();
        assert_eq!(launch.completed_tick, 1);
        assert_eq!(launch.projectiles_launched, 1);
        let projectile = sim.projectiles()[0];
        let ProjectileViewKind::Ballistic {
            destination,
            impact_radius,
        } = projectile.kind
        else {
            panic!("expected ballistic projectile");
        };
        assert_eq!(destination, target_position_at_launch);
        assert_eq!(impact_radius, cell / 2);
        assert_eq!(projectile.impact_tick, 4);

        while sim.tick() < 4 {
            sim.step();
        }
        let impact = sim.step();
        assert_eq!(impact.completed_tick, 4);
        assert_eq!(impact.projectile_impacts, 1);
        assert_eq!(impact.projectile_effects, 0);
        assert_eq!(sim.projectile_count(), 0);
        assert_eq!(sim.unit(target).unwrap().health, 100);
        assert!(sim.unit(target).unwrap().position.x > destination.x + impact_radius);
    }

    #[test]
    fn ballistic_projectile_hits_unit_that_moves_into_captured_zone() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            team_objective: [SimPoint::new(0, 0), SimPoint::new(5 * cell, 0)],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(0, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedBallistic {
                    speed_per_tick: 2 * cell,
                    impact_radius: 2 * cell,
                },
                damage: 7,
                range: 10 * cell,
                acquisition_range: 10 * cell,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let target = sim.spawn_unit(UnitSpawn {
            health: 100,
            ..passive_unit(1, 5 * cell)
        });
        let bystander = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(9 * cell, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile {
                speed_per_tick: cell,
            },
        });

        sim.step();
        let before_launch = sim.unit(bystander).unwrap().position;
        let launch = sim.step();
        assert_eq!(launch.projectiles_launched, 1);
        let destination = match sim.projectiles()[0].kind {
            ProjectileViewKind::Ballistic { destination, .. } => destination,
            ProjectileViewKind::GuaranteedHit { .. }
            | ProjectileViewKind::Reflected { .. }
            | ProjectileViewKind::Bounce { .. } => {
                panic!("expected ballistic projectile")
            }
        };
        assert_eq!(destination, SimPoint::new(5 * cell, 0));
        assert!(sim.unit(bystander).unwrap().position.x < before_launch.x);

        while sim.tick() < 4 {
            sim.step();
        }
        let impact = sim.step();
        assert_eq!(impact.completed_tick, 4);
        assert_eq!(impact.projectile_impacts, 1);
        assert_eq!(impact.projectile_effects, 2);
        assert!(impact.ballistic_candidate_checks >= 2);
        assert_eq!(sim.unit(target).unwrap().health, 93);
        assert_eq!(sim.unit(bystander).unwrap().health, 93);
        assert!(
            destination.distance_sq(sim.unit(bystander).unwrap().position)
                <= (i64::from(2 * cell) * i64::from(2 * cell)) as u64
        );
    }

    #[test]
    fn bounce_chain_keeps_projectile_identity_avoids_repeats_and_scales_damage() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            match_seed: 0x1234_5678_9abc_def0,
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        let source = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(0, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Bounce {
                    speed_per_tick: 10 * cell,
                    bounce_range: 5 * cell,
                    max_bounces: 2,
                    damage_percent_per_bounce: 50,
                    allow_repeat_targets: false,
                },
                damage: 8,
                range: 10 * cell,
                acquisition_range: 10 * cell,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let first = sim.spawn_unit(UnitSpawn {
            health: 100,
            ..passive_unit(1, 4 * cell)
        });
        let second_candidate = sim.spawn_unit(UnitSpawn {
            health: 100,
            ..passive_unit(1, 6 * cell)
        });
        let third_candidate = sim.spawn_unit(UnitSpawn {
            health: 100,
            ..passive_unit(1, 8 * cell)
        });

        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(first));
        let launch = sim.step();
        assert_eq!(launch.projectiles_launched, 1);
        let projectile = sim.projectiles()[0];
        let projectile_id = projectile.id;
        assert!(matches!(
            projectile.kind,
            ProjectileViewKind::Bounce {
                target,
                bounce_index: 0,
                remaining_bounces: 2,
            } if target == first
        ));

        let first_impact = sim.step();
        assert_eq!(first_impact.projectile_impacts, 1);
        assert_eq!(first_impact.projectile_effects, 1);
        assert_eq!(first_impact.bounce_jumps, 1);
        assert!(first_impact.bounce_candidate_checks >= 2);
        assert_eq!(sim.unit(first).unwrap().health, 92);
        let first_hop = sim.projectiles()[0];
        assert_eq!(first_hop.id, projectile_id);
        let second_target = match first_hop.kind {
            ProjectileViewKind::Bounce {
                target,
                bounce_index: 1,
                remaining_bounces: 1,
            } => target,
            _ => panic!("expected first bounce hop"),
        };
        assert!(second_target == second_candidate || second_target == third_candidate);

        let second_impact = sim.step();
        assert_eq!(second_impact.projectile_impacts, 1);
        assert_eq!(second_impact.projectile_effects, 1);
        assert_eq!(second_impact.bounce_jumps, 1);
        assert_eq!(sim.unit(second_target).unwrap().health, 96);
        let second_hop = sim.projectiles()[0];
        assert_eq!(second_hop.id, projectile_id);
        let third_target = match second_hop.kind {
            ProjectileViewKind::Bounce {
                target,
                bounce_index: 2,
                remaining_bounces: 0,
            } => target,
            _ => panic!("expected second bounce hop"),
        };
        assert_ne!(third_target, first);
        assert_ne!(third_target, second_target);
        assert!(third_target == second_candidate || third_target == third_candidate);

        let third_impact = sim.step();
        assert_eq!(third_impact.projectile_impacts, 1);
        assert_eq!(third_impact.projectile_effects, 1);
        assert_eq!(third_impact.bounce_jumps, 0);
        assert_eq!(sim.unit(third_target).unwrap().health, 98);
        assert_eq!(sim.projectile_count(), 0);
    }

    #[test]
    fn bounce_chain_is_worker_count_independent() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut expected = None;
        for workers in [1, 2, 8] {
            let config = SimulationConfig {
                match_seed: 0x0ddc_0ffe_e15e_beef,
                ..SimulationConfig::default()
            };
            let mut sim = Simulation::new(config, workers);
            sim.spawn_unit(UnitSpawn {
                team: Team(0),
                position: SimPoint::new(0, 0),
                health: 100_000,
                attack: AttackProfile {
                    delivery: AttackDelivery::Bounce {
                        speed_per_tick: 4 * cell,
                        bounce_range: 8 * cell,
                        max_bounces: 3,
                        damage_percent_per_bounce: 75,
                        allow_repeat_targets: false,
                    },
                    damage: 8,
                    range: 20 * cell,
                    acquisition_range: 20 * cell,
                    cooldown_ticks: 1,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            });
            for index in 0..12 {
                sim.spawn_unit(UnitSpawn {
                    team: Team(1),
                    position: SimPoint::new((5 + index % 4) * cell, (index / 4 - 1) * cell),
                    health: 100_000,
                    attack: AttackProfile {
                        delivery: AttackDelivery::Melee,
                        damage: 0,
                        range: 0,
                        acquisition_range: 0,
                        cooldown_ticks: 1,
                    },
                    movement: MovementProfile { speed_per_tick: 0 },
                });
            }
            for _ in 0..30 {
                sim.step();
            }
            match expected {
                Some(checksum) => assert_eq!(sim.checksum(), checksum, "workers={workers}"),
                None => expected = Some(sim.checksum()),
            }
        }
    }

    #[test]
    fn guaranteed_hit_projectile_survives_source_death() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let source = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(0, 0),
            health: 10,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
                },
                damage: 1,
                range: 8 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let target = sim.spawn_unit(passive_unit(1, 6 * SUBUNITS_PER_WORLD_UNIT));

        sim.step();
        let launch = sim.step();
        assert_eq!(launch.projectiles_launched, 1);
        assert_eq!(sim.projectiles()[0].impact_tick, 7);

        sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 10,
                range: 2 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 2 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert!(sim.unit(source).is_none());
        assert_eq!(sim.projectile_count(), 1);

        while sim.tick() <= 7 {
            let result = sim.step();
            if result.completed_tick == 7 {
                assert_eq!(result.projectile_impacts, 1);
            }
        }
        assert_eq!(sim.unit(target).unwrap().health, 9_999);
        assert_eq!(sim.projectile_count(), 0);
    }

    #[test]
    fn guaranteed_hit_projectile_invalidates_if_target_dies_before_impact() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(0, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
                },
                damage: 1,
                range: 8 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let target = sim.spawn_unit(UnitSpawn {
            health: 1,
            ..passive_unit(1, 4 * SUBUNITS_PER_WORLD_UNIT)
        });

        sim.step();
        let launch = sim.step();
        assert_eq!(launch.projectiles_launched, 1);
        assert_eq!(sim.projectiles()[0].impact_tick, 5);

        sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(3 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 2 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert!(sim.unit(target).is_none());
        assert_eq!(sim.projectile_count(), 1);

        sim.step();
        let invalidated = sim.step();
        assert_eq!(invalidated.completed_tick, 5);
        assert_eq!(invalidated.projectile_impacts, 0);
        assert_eq!(invalidated.projectile_invalidations, 1);
        assert_eq!(invalidated.projectiles_alive, 0);
    }

    #[test]
    fn static_blockers_reject_building_placement() {
        let config = SimulationConfig {
            static_blockers: vec![BuildingFootprint::new(20, 0, 2, 2)],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 1);
        let result =
            sim.try_spawn_building(passive_building(0, BuildingFootprint::new(21, 1, 1, 1)));
        assert_eq!(result, Err(BuildingPlacementError::StaticObstacle));
    }

    #[test]
    fn building_placement_cell_preview_reports_individual_blockers() {
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(19, 9),
            static_blockers: vec![BuildingFootprint::new(3, 3, 1, 1)],
            build_static_blockers: vec![BuildingFootprint::new(4, 3, 1, 1)],
            team_build_regions: [
                vec![BuildingFootprint::new(0, 0, 10, 10)],
                vec![BuildingFootprint::new(10, 0, 10, 10)],
            ],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 1);
        sim.try_spawn_building(passive_building(0, BuildingFootprint::new(5, 3, 1, 1)))
            .unwrap();

        assert!(sim.can_place_building_cell_for_team(Team(0), NavCell::new(2, 3)));
        assert!(!sim.can_place_building_cell_for_team(Team(0), NavCell::new(3, 3)));
        assert!(!sim.can_place_building_cell_for_team(Team(0), NavCell::new(4, 3)));
        assert!(!sim.can_place_building_cell_for_team(Team(0), NavCell::new(5, 3)));
        assert!(!sim.can_place_building_cell_for_team(Team(0), NavCell::new(10, 3)));
        assert!(!sim.can_place_building_cell_for_team(Team(0), NavCell::new(-1, 3)));
    }

    #[test]
    fn team_build_regions_reject_middle_and_enemy_territory() {
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(29, 9),
            team_build_regions: [
                vec![BuildingFootprint::new(0, 0, 10, 10)],
                vec![BuildingFootprint::new(20, 0, 10, 10)],
            ],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 1);
        let own = BuildingFootprint::new(2, 2, 2, 2);
        let middle = BuildingFootprint::new(12, 2, 2, 2);
        let enemy = BuildingFootprint::new(22, 2, 2, 2);

        assert!(sim.can_place_building_for_team(Team(0), own));
        assert!(!sim.can_place_building_for_team(Team(0), middle));
        assert!(!sim.can_place_building_for_team(Team(0), enemy));
        assert_eq!(
            sim.try_spawn_building(passive_building(0, middle)),
            Err(BuildingPlacementError::OutsideBuildRegion)
        );
        assert_eq!(
            sim.try_spawn_building(passive_building(0, enemy)),
            Err(BuildingPlacementError::OutsideBuildRegion)
        );
        assert!(sim.try_spawn_building(passive_building(0, own)).is_ok());
    }

    #[test]
    fn builder_race_configuration_changes_in_place_without_replacing_entity() {
        let mut sim = Simulation::new(SimulationConfig::default(), 1);
        let human = CastleFightBuilderRace::Human.definition();
        let builder = sim.spawn_builder(human.spawn(Team(0), SimPoint::new(0, 0)));
        let corrupted = CastleFightBuilderRace::Corrupted.definition();

        sim.configure_builder(builder, corrupted.profile, corrupted.configuration())
            .unwrap();

        let view = sim.builder(builder).unwrap();
        assert_eq!(view.id, builder);
        assert_eq!(view.profile, corrupted.profile);
        assert_eq!(view.configuration.appearance.rawcode, corrupted.rawcode);
        assert_eq!(view.configuration.locomotion, BuilderLocomotion::Hover);
        assert_eq!(view.configuration.build_catalog, corrupted.build_catalog);
    }

    #[test]
    fn builder_moves_through_blockers_does_not_occupy_space_and_stays_in_base() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(29, 9),
            static_blockers: vec![BuildingFootprint::new(4, 0, 2, 10)],
            team_build_regions: [
                vec![BuildingFootprint::new(0, 0, 10, 10)],
                vec![BuildingFootprint::new(20, 0, 10, 10)],
            ],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 1);
        let builder = sim.spawn_builder(BuilderSpawn {
            team: Team(0),
            position: SimPoint::new(2 * cell, 5 * cell),
            profile: BuilderProfile {
                speed_per_tick: cell,
                build_range: cell,
                repair_range: cell,
                repair_autocast_range: cell,
                repair_time_ratio_numerator: 1,
                repair_time_ratio_denominator: 1,
                full_repair_duration_ticks: 10,
                blink_range: 20 * cell,
                blink_boundary_inset: 0,
            },
            configuration: test_builder_configuration(vec![]),
            repair_autocast_enabled: false,
        });

        assert_eq!(sim.unit_count(), 0, "builder must not be a combat unit");
        let existing_builder = sim.builder(builder).unwrap();
        assert_eq!(
            sim.try_spawn_builder(BuilderSpawn {
                team: Team(0),
                position: SimPoint::new(3 * cell, 5 * cell),
                profile: existing_builder.profile,
                configuration: existing_builder.configuration,
                repair_autocast_enabled: existing_builder.repair_autocast_enabled,
            }),
            Err(BuilderSpawnError::PlayerAlreadyHasBuilder)
        );
        assert_eq!(
            sim.order_builder_move(builder, SimPoint::new(12 * cell, 5 * cell)),
            Err(BuilderCommandError::OutsideBuildRegion)
        );

        sim.order_builder_move(builder, SimPoint::new(8 * cell, 5 * cell))
            .unwrap();
        for _ in 0..6 {
            sim.step();
        }
        assert_eq!(
            sim.builder(builder).unwrap().position,
            SimPoint::new(8 * cell, 5 * cell),
            "static blocker must not affect builder movement"
        );

        let footprint = BuildingFootprint::new(8, 5, 1, 1);
        assert!(
            sim.try_spawn_building(passive_building(0, footprint))
                .is_ok(),
            "builder itself must not occupy or block building placement"
        );
    }

    #[test]
    fn builder_follow_stops_at_target_envelope_and_is_replaced_by_move() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(19, 9),
            team_build_regions: [
                vec![BuildingFootprint::new(0, 0, 10, 10)],
                vec![BuildingFootprint::new(10, 0, 10, 10)],
            ],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 1);
        let builder = sim.spawn_builder(BuilderSpawn {
            team: Team(0),
            position: SimPoint::new(cell, 5 * cell),
            profile: BuilderProfile {
                speed_per_tick: cell,
                build_range: cell,
                repair_range: cell,
                repair_autocast_range: cell,
                repair_time_ratio_numerator: 1,
                repair_time_ratio_denominator: 1,
                full_repair_duration_ticks: 10,
                blink_range: 20 * cell,
                blink_boundary_inset: 0,
            },
            configuration: test_builder_configuration(vec![]),
            repair_autocast_enabled: false,
        });
        let target = sim.spawn_building(passive_building(0, BuildingFootprint::new(6, 4, 2, 2)));

        sim.order_builder_follow(builder, target).unwrap();
        for _ in 0..8 {
            sim.step();
        }
        let following = sim.builder(builder).unwrap();
        assert_eq!(following.follow_target, Some(target));
        assert_eq!(following.position.x, 6 * cell);
        assert!(following.position.y >= 4 * cell && following.position.y <= 6 * cell);

        sim.order_builder_move(builder, SimPoint::new(2 * cell, 5 * cell))
            .unwrap();
        let moving = sim.builder(builder).unwrap();
        assert_eq!(moving.follow_target, None);
        assert_eq!(moving.destination, Some(SimPoint::new(2 * cell, 5 * cell)));
    }

    #[test]
    fn builder_blink_clamps_to_inset_base_rect_and_stops_current_order() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(29, 9),
            team_build_regions: [
                vec![BuildingFootprint::new(0, 0, 10, 10)],
                vec![BuildingFootprint::new(20, 0, 10, 10)],
            ],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 1);
        let builder = sim.spawn_builder(BuilderSpawn {
            team: Team(0),
            position: SimPoint::new(5 * cell, 5 * cell),
            profile: BuilderProfile {
                speed_per_tick: cell,
                build_range: cell,
                repair_range: cell,
                repair_autocast_range: 10 * cell,
                repair_time_ratio_numerator: 1,
                repair_time_ratio_denominator: 1,
                full_repair_duration_ticks: 10,
                blink_range: 100 * cell,
                blink_boundary_inset: 2 * cell,
            },
            configuration: test_builder_configuration(vec![]),
            repair_autocast_enabled: false,
        });
        sim.order_builder_move(builder, SimPoint::new(7 * cell, 5 * cell))
            .unwrap();

        let resolved = sim
            .order_builder_blink(builder, SimPoint::new(-10 * cell, 9 * cell))
            .unwrap();
        assert_eq!(resolved.x, 2 * cell);
        assert_eq!(resolved.y, 8 * cell - 1);
        let view = sim.builder(builder).unwrap();
        assert_eq!(view.position, resolved);
        assert_eq!(view.destination, None);
        assert_eq!(view.repair_target, None);

        assert_eq!(
            sim.order_builder_blink(builder, SimPoint::new(200 * cell, 5 * cell)),
            Err(BuilderCommandError::BlinkOutOfRange)
        );
        assert_eq!(sim.builder(builder).unwrap().position, resolved);
    }

    #[test]
    fn builder_summon_uses_owning_team_placement_rules() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(29, 9),
            team_build_regions: [
                vec![BuildingFootprint::new(0, 0, 10, 10)],
                vec![BuildingFootprint::new(20, 0, 10, 10)],
            ],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 1);
        let allowed_rawcode = u32::from_be_bytes(*b"BLD1");
        let builder = sim.spawn_builder(BuilderSpawn {
            team: Team(0),
            position: SimPoint::new(2 * cell, 2 * cell),
            profile: BuilderProfile {
                speed_per_tick: cell,
                build_range: cell,
                repair_range: cell,
                repair_autocast_range: cell,
                repair_time_ratio_numerator: 1,
                repair_time_ratio_denominator: 1,
                full_repair_duration_ticks: 10,
                blink_range: 20 * cell,
                blink_boundary_inset: 0,
            },
            configuration: test_builder_configuration(vec![allowed_rawcode]),
            repair_autocast_enabled: false,
        });

        assert!(
            sim.try_builder_summon_building_with_properties(
                builder,
                passive_building(0, BuildingFootprint::new(3, 3, 1, 1)),
                test_building_properties(allowed_rawcode),
            )
            .is_ok()
        );
        assert_eq!(
            sim.try_builder_summon_building_with_properties(
                builder,
                passive_building(1, BuildingFootprint::new(22, 3, 1, 1)),
                test_building_properties(allowed_rawcode),
            ),
            Err(BuilderBuildError::TeamMismatch)
        );
        assert_eq!(
            sim.try_builder_summon_building_with_properties(
                builder,
                passive_building(0, BuildingFootprint::new(12, 3, 1, 1)),
                test_building_properties(allowed_rawcode),
            ),
            Err(BuilderBuildError::Placement(
                BuildingPlacementError::OutsideBuildRegion
            ))
        );

        let drafted_rawcode = u32::from_be_bytes(*b"DRF1");
        assert_eq!(
            sim.try_builder_summon_building_with_properties(
                builder,
                passive_building(0, BuildingFootprint::new(5, 3, 1, 1)),
                test_building_properties(drafted_rawcode),
            ),
            Err(BuilderBuildError::BuildingNotInCatalog)
        );
        let current_builder = sim.builder(builder).unwrap();
        let mut draft_configuration = current_builder.configuration;
        draft_configuration.build_catalog = vec![drafted_rawcode];
        sim.configure_builder(builder, current_builder.profile, draft_configuration)
            .unwrap();
        assert!(
            sim.try_builder_summon_building_with_properties(
                builder,
                passive_building(0, BuildingFootprint::new(5, 3, 1, 1)),
                test_building_properties(drafted_rawcode),
            )
            .is_ok(),
            "draft-style catalog replacement should change build legality without replacing builder"
        );
    }

    #[test]
    fn debug_resource_grant_is_authoritative_saturating_and_team_scoped() {
        let mut sim = Simulation::new(
            SimulationConfig {
                economy: EconomyRules {
                    starting_gold: 10,
                    starting_lumber: 20,
                    ..EconomyRules::default()
                },
                ..SimulationConfig::default()
            },
            1,
        );

        assert!(sim.debug_grant_player_resources(Team(0), 30, 40));
        assert_eq!(
            sim.player_resources(Team(0)).unwrap(),
            PlayerResources {
                gold: 40,
                lumber: 60,
                legendary_points_used: 0,
                legendary_points_cap: 0,
            }
        );
        assert_eq!(sim.player_resources(Team(1)).unwrap().gold, 10);
        assert!(!sim.debug_grant_player_resources(Team(2), 30, 40));

        assert!(sim.debug_grant_player_resources(Team(0), u32::MAX, u32::MAX));
        let saturated = sim.player_resources(Team(0)).unwrap();
        assert_eq!(saturated.gold, u32::MAX);
        assert_eq!(saturated.lumber, u32::MAX);
    }

    #[test]
    fn debug_damage_all_units_applies_exact_damage_and_ignores_builders_and_buildings() {
        let mut sim = Simulation::new(SimulationConfig::default(), 1);
        let doomed = sim.spawn_unit_with_corpse(
            UnitSpawn {
                health: 100,
                ..passive_unit(0, 0)
            },
            CorpseProfile {
                definition: CorpseDefinitionId(42),
                lifetime_ticks: Some(60),
            },
        );
        let survivor = sim.spawn_unit(UnitSpawn {
            health: 10_000,
            ..passive_unit(1, 4 * SUBUNITS_PER_WORLD_UNIT)
        });
        let builder = sim.spawn_builder(BuilderSpawn {
            team: Team(0),
            position: SimPoint::new(8 * SUBUNITS_PER_WORLD_UNIT, 0),
            profile: castle_fight_builder_profile(),
            configuration: test_builder_configuration(vec![]),
            repair_autocast_enabled: false,
        });
        let building = sim.spawn_building(passive_building(0, BuildingFootprint::new(12, 0, 1, 1)));
        let building_health = sim.building(building).unwrap().health;

        assert_eq!(sim.debug_damage_all_units(9_999), 2);
        assert!(sim.unit(doomed).is_none());
        assert_eq!(sim.unit(survivor).unwrap().health, 1);
        let corpses = sim.corpses();
        assert_eq!(corpses.len(), 1);
        assert_eq!(corpses[0].source_unit, doomed);
        assert_eq!(corpses[0].definition, CorpseDefinitionId(42));
        assert!(sim.builder(builder).is_some());
        assert_eq!(sim.building(building).unwrap().health, building_health);
    }

    #[test]
    fn purchased_buildings_spend_resources_award_lumber_and_pay_tick_income() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(29, 9),
            team_build_regions: [
                vec![BuildingFootprint::new(0, 0, 15, 10)],
                vec![BuildingFootprint::new(15, 0, 15, 10)],
            ],
            economy: castle_fight_economy_rules(),
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 1);
        let barracks = CastleFightProductionKind::Barracks.definition();
        let watch_tower = CastleFightTowerKind::WatchTower.definition();
        let builder = sim.spawn_builder(BuilderSpawn {
            team: Team(0),
            position: SimPoint::new(2 * cell, 2 * cell),
            profile: castle_fight_builder_profile(),
            configuration: test_builder_configuration(vec![barracks.rawcode, watch_tower.rawcode]),
            repair_autocast_enabled: true,
        });

        let initial = sim.player_economy(Team(0)).unwrap();
        assert_eq!(initial.resources.gold, 250);
        assert_eq!(initial.resources.lumber, 125);
        assert_eq!(initial.resources.legendary_points_used, 0);
        assert_eq!(initial.resources.legendary_points_cap, 1);
        assert_eq!(initial.income, 5);
        assert_eq!(initial.ticks_until_income, 300);

        sim.try_builder_purchase_building_with_properties(
            builder,
            barracks.spawn(Team(0), BuildingFootprint::new(4, 3, 4, 4)),
            barracks.gameplay_properties(),
        )
        .unwrap();
        let after_barracks = sim.player_economy(Team(0)).unwrap();
        assert_eq!(after_barracks.resources.gold, 150);
        assert_eq!(after_barracks.resources.lumber, 225);
        assert_eq!(after_barracks.income, 7);

        assert_eq!(
            sim.try_builder_purchase_building_with_properties(
                builder,
                watch_tower.spawn(Team(0), BuildingFootprint::new(9, 3, 4, 4)),
                watch_tower.gameplay_properties(),
            ),
            Err(BuilderBuildError::Resources(
                ResourcePurchaseError::InsufficientLumber {
                    available: 225,
                    required: 300,
                }
            ))
        );
        assert_eq!(
            sim.player_resources(Team(0)).unwrap(),
            after_barracks.resources
        );
        assert_eq!(sim.building_count(), 1);

        for _ in 0..299 {
            sim.step();
        }
        assert_eq!(sim.player_resources(Team(0)).unwrap().gold, 150);
        assert_eq!(
            sim.player_economy(Team(0)).unwrap().income_progress_per_10k,
            9_966
        );
        sim.step();
        assert_eq!(sim.player_resources(Team(0)).unwrap().gold, 157);
        assert_eq!(
            sim.player_economy(Team(0)).unwrap().income_progress_per_10k,
            0
        );
    }

    #[test]
    fn builder_build_order_walks_into_range_and_refunds_when_replaced() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(259, 19),
            team_build_regions: [
                vec![BuildingFootprint::new(0, 0, 130, 20)],
                vec![BuildingFootprint::new(130, 0, 130, 20)],
            ],
            economy: castle_fight_economy_rules(),
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 1);
        let barracks = CastleFightProductionKind::Barracks.definition();
        let builder = sim.spawn_builder(BuilderSpawn {
            team: Team(0),
            position: SimPoint::new(2 * cell, 2 * cell),
            profile: castle_fight_builder_profile(),
            configuration: test_builder_configuration(vec![barracks.rawcode]),
            repair_autocast_enabled: false,
        });
        let footprint = BuildingFootprint::new(80, 2, 4, 4);

        sim.order_builder_purchase_building_with_properties(
            builder,
            barracks.spawn(Team(0), footprint),
            barracks.gameplay_properties(),
        )
        .unwrap();
        assert_eq!(
            sim.building_count(),
            0,
            "build order must not construct remotely"
        );
        assert_eq!(sim.player_resources(Team(0)).unwrap().gold, 150);
        assert_eq!(sim.player_resources(Team(0)).unwrap().lumber, 125);
        assert_eq!(
            sim.builder(builder).unwrap().build_footprint,
            Some(footprint)
        );

        sim.step();
        assert_eq!(
            sim.building_count(),
            0,
            "builder is still outside 50-unit build range"
        );
        sim.step();
        assert_eq!(
            sim.building_count(),
            1,
            "builder should start construction after entering range"
        );
        let site = sim.buildings()[0];
        assert_eq!(site.construction_started_tick, Some(1));
        assert_eq!(site.construction_complete_tick, Some(61));
        assert!(
            site.production.is_none(),
            "unfinished building must not produce"
        );
        assert_eq!(
            sim.player_resources(Team(0)).unwrap().lumber,
            125,
            "construction lumber reward belongs to completion, not construction start"
        );
        assert!(sim.builder(builder).unwrap().position.x > 2 * cell);
        assert_eq!(sim.builder(builder).unwrap().build_footprint, None);

        for _ in 0..59 {
            sim.step();
        }
        let site = sim.buildings()[0];
        assert_eq!(site.construction_complete_tick, Some(61));
        assert!(site.production.is_none());
        assert_eq!(sim.player_resources(Team(0)).unwrap().lumber, 125);
        sim.step();
        let completed = sim.buildings()[0];
        assert_eq!(completed.construction_started_tick, None);
        assert_eq!(completed.construction_complete_tick, None);
        assert!(completed.production.is_some());
        assert_eq!(sim.player_resources(Team(0)).unwrap().lumber, 225);

        let second_footprint = BuildingFootprint::new(110, 2, 4, 4);
        sim.order_builder_purchase_building_with_properties(
            builder,
            barracks.spawn(Team(0), second_footprint),
            barracks.gameplay_properties(),
        )
        .unwrap();
        assert_eq!(sim.player_resources(Team(0)).unwrap().gold, 50);
        sim.order_builder_move(builder, SimPoint::new(10 * cell, 2 * cell))
            .unwrap();
        assert_eq!(
            sim.player_resources(Team(0)).unwrap().gold,
            150,
            "ConstructionRefundRate=1 must fully refund a build that has not started"
        );
        for _ in 0..10 {
            sim.step();
        }
        assert_eq!(sim.building_count(), 1, "cancelled build must never appear");
    }

    #[test]
    fn in_progress_building_can_be_cancelled_for_full_committed_cost() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(39, 9),
            team_build_regions: [
                vec![BuildingFootprint::new(0, 0, 20, 10)],
                vec![BuildingFootprint::new(20, 0, 20, 10)],
            ],
            economy: castle_fight_economy_rules(),
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 1);
        let barracks = CastleFightProductionKind::Barracks.definition();
        let builder = sim.spawn_builder(BuilderSpawn {
            team: Team(0),
            position: SimPoint::new(2 * cell, 2 * cell),
            profile: BuilderProfile {
                speed_per_tick: 0,
                build_range: 20 * cell,
                ..castle_fight_builder_profile()
            },
            configuration: test_builder_configuration(vec![barracks.rawcode]),
            repair_autocast_enabled: false,
        });

        sim.order_builder_purchase_building_with_properties(
            builder,
            barracks.spawn(Team(0), BuildingFootprint::new(4, 2, 4, 4)),
            barracks.gameplay_properties(),
        )
        .unwrap();
        sim.step();
        let site = sim.buildings()[0];
        assert!(site.construction_complete_tick.is_some());
        assert_eq!(sim.player_resources(Team(0)).unwrap().gold, 150);
        assert_eq!(sim.player_resources(Team(0)).unwrap().lumber, 125);
        assert_eq!(sim.player_economy(Team(0)).unwrap().income, 5);

        assert_eq!(
            sim.cancel_building_construction(Team(1), site.id),
            Err(BuildingConstructionCancelError::NotOwner)
        );
        assert_eq!(
            sim.cancel_building_construction(Team(0), site.id).unwrap(),
            BuildingConstructionCancelOutcome::RemovedNewBuilding
        );
        assert_eq!(sim.building_count(), 0);
        assert_eq!(sim.player_resources(Team(0)).unwrap().gold, 250);
        assert_eq!(sim.player_resources(Team(0)).unwrap().lumber, 125);
        assert_eq!(sim.player_economy(Team(0)).unwrap().income, 5);
        for _ in 0..barracks.construction_time_ticks + 1 {
            assert_eq!(sim.step().units_spawned, 0);
        }
    }

    #[test]
    fn production_upgrade_reuses_identity_pauses_production_and_cancels_to_precursor() {
        let mut sim = Simulation::new(
            SimulationConfig {
                economy: castle_fight_economy_rules(),
                ..SimulationConfig::default()
            },
            1,
        );
        let barracks = CastleFightProductionKind::Barracks.definition();
        let stronghold = CastleFightProductionKind::Stronghold.definition();
        let footprint = BuildingFootprint::new(0, 0, 4, 4);
        let mut source_spawn = barracks.spawn(Team(0), footprint);
        source_spawn
            .production
            .as_mut()
            .expect("Barracks must produce")
            .initial_delay_ticks = 5;
        let building =
            sim.spawn_building_with_properties(source_spawn, barracks.gameplay_properties());
        let source_income = sim.player_income(Team(0)).unwrap();
        assert_eq!(sim.building(building).unwrap().next_spawn_tick, Some(5));

        sim.start_building_upgrade(
            building,
            source_spawn,
            barracks.gameplay_properties(),
            stronghold.spawn(Team(0), footprint),
            stronghold.gameplay_properties(),
        )
        .unwrap();

        let upgrading = sim.building(building).unwrap();
        assert_eq!(upgrading.id, building);
        assert_eq!(upgrading.content.unwrap().rawcode, stronghold.rawcode);
        assert_eq!(upgrading.footprint, footprint);
        assert_eq!(upgrading.construction_started_tick, Some(0));
        assert_eq!(
            upgrading.construction_complete_tick,
            Some(u64::from(stronghold.construction_time_ticks))
        );
        assert!(upgrading.production.is_none());
        assert_eq!(upgrading.next_spawn_tick, None);
        assert_eq!(sim.player_resources(Team(0)).unwrap().gold, 75);
        assert_eq!(sim.player_income(Team(0)).unwrap(), source_income);

        for _ in 0..3 {
            assert_eq!(sim.step().units_spawned, 0);
        }
        assert_eq!(
            sim.cancel_building_construction(Team(0), building).unwrap(),
            BuildingConstructionCancelOutcome::RevertedUpgrade
        );
        let restored = sim.building(building).unwrap();
        assert_eq!(restored.id, building);
        assert_eq!(restored.content.unwrap().rawcode, barracks.rawcode);
        assert_eq!(restored.footprint, footprint);
        assert_eq!(restored.construction_started_tick, None);
        assert_eq!(restored.construction_complete_tick, None);
        assert!(restored.production.is_some());
        assert_eq!(restored.next_spawn_tick, Some(5));
        assert_eq!(sim.player_resources(Team(0)).unwrap().gold, 250);
        assert_eq!(sim.player_income(Team(0)).unwrap(), source_income);

        assert_eq!(sim.step().units_spawned, 0); // tick 3
        assert_eq!(sim.step().units_spawned, 0); // tick 4
        assert_eq!(sim.step().units_spawned, 1); // original tick-5 deadline survives cancellation
    }

    #[test]
    fn upgrade_checksum_includes_saved_precursor_runtime() {
        fn upgrading_checksum(source_delay_ticks: u16) -> u64 {
            let mut sim = Simulation::new(
                SimulationConfig {
                    economy: castle_fight_economy_rules(),
                    ..SimulationConfig::default()
                },
                1,
            );
            let barracks = CastleFightProductionKind::Barracks.definition();
            let stronghold = CastleFightProductionKind::Stronghold.definition();
            let footprint = BuildingFootprint::new(0, 0, 4, 4);
            let mut source_spawn = barracks.spawn(Team(0), footprint);
            source_spawn
                .production
                .as_mut()
                .expect("Barracks must produce")
                .initial_delay_ticks = source_delay_ticks;
            let building =
                sim.spawn_building_with_properties(source_spawn, barracks.gameplay_properties());
            sim.start_building_upgrade(
                building,
                source_spawn,
                barracks.gameplay_properties(),
                stronghold.spawn(Team(0), footprint),
                stronghold.gameplay_properties(),
            )
            .unwrap();
            sim.step().checksum
        }

        assert_ne!(
            upgrading_checksum(5),
            upgrading_checksum(6),
            "saved precursor runtime can change cancellation outcome and must be canonical state"
        );
    }

    #[test]
    fn production_upgrade_completion_switches_definition_and_starts_target_timer() {
        let mut sim = Simulation::new(
            SimulationConfig {
                economy: castle_fight_economy_rules(),
                ..SimulationConfig::default()
            },
            1,
        );
        let barracks = CastleFightProductionKind::Barracks.definition();
        let stronghold = CastleFightProductionKind::Stronghold.definition();
        let footprint = BuildingFootprint::new(0, 0, 4, 4);
        let mut source_spawn = barracks.spawn(Team(0), footprint);
        source_spawn
            .production
            .as_mut()
            .expect("Barracks must produce")
            .initial_delay_ticks = 1;
        let building =
            sim.spawn_building_with_properties(source_spawn, barracks.gameplay_properties());
        let source_income = sim.player_income(Team(0)).unwrap();

        sim.start_building_upgrade(
            building,
            source_spawn,
            barracks.gameplay_properties(),
            stronghold.spawn(Team(0), footprint),
            stronghold.gameplay_properties(),
        )
        .unwrap();

        let mut steps = 0;
        while sim
            .building(building)
            .is_some_and(|view| view.construction_complete_tick.is_some())
        {
            assert_eq!(sim.step().units_spawned, 0);
            steps += 1;
            assert!(steps <= stronghold.construction_time_ticks + 1);
        }
        assert_eq!(steps, stronghold.construction_time_ticks + 1);

        let completed = sim.building(building).unwrap();
        assert_eq!(completed.id, building);
        assert_eq!(completed.content.unwrap().rawcode, stronghold.rawcode);
        assert_eq!(completed.footprint, footprint);
        assert!(completed.production.is_some());
        assert_eq!(
            completed.production.unwrap().unit,
            stronghold
                .spawn(Team(0), footprint)
                .production
                .expect("Stronghold must produce")
                .unit
        );
        assert_eq!(
            completed.next_spawn_tick,
            Some(
                u64::from(stronghold.construction_time_ticks)
                    + u64::from(stronghold.spawn_interval_ticks)
            )
        );
        assert_eq!(
            sim.player_resources(Team(0)).unwrap().lumber,
            125 + stronghold.economy.lumber_refund
        );
        assert!(sim.player_income(Team(0)).unwrap() > source_income);
    }

    #[test]
    fn builder_moves_to_friendly_building_and_repairs_at_tick_exact_rate() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(39, 9),
            static_blockers: vec![BuildingFootprint::new(5, 0, 2, 10)],
            team_build_regions: [
                vec![BuildingFootprint::new(0, 0, 20, 10)],
                vec![BuildingFootprint::new(20, 0, 20, 10)],
            ],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        let builder = sim.spawn_builder(BuilderSpawn {
            team: Team(0),
            position: SimPoint::new(cell, 4 * cell),
            profile: BuilderProfile {
                speed_per_tick: 2 * cell,
                build_range: 2 * cell,
                repair_range: 2 * cell,
                repair_autocast_range: 2 * cell,
                repair_time_ratio_numerator: 1,
                repair_time_ratio_denominator: 1,
                full_repair_duration_ticks: 99,
                blink_range: 20 * cell,
                blink_boundary_inset: 0,
            },
            configuration: test_builder_configuration(vec![]),
            repair_autocast_enabled: false,
        });
        let mut target_spawn = passive_building(0, BuildingFootprint::new(10, 4, 1, 1));
        target_spawn.health = 900;
        let target = sim.spawn_building_with_properties(
            target_spawn,
            BuildingGameplayProperties {
                repair_time_ticks: Some(9),
                ..BuildingGameplayProperties::default()
            },
        );
        sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(13 * cell, 4 * cell),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 300,
                range: 4 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 1_000,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        for _ in 0..3 {
            sim.step();
            if sim.building(target).unwrap().health < 900 {
                break;
            }
        }
        let damaged = sim.building(target).unwrap().health;
        assert!(damaged < 900, "test attacker must damage repair target");
        assert_eq!(
            sim.order_builder_repair(builder, SimId(u64::MAX)),
            Err(BuilderCommandError::RepairTargetNotFound)
        );
        sim.order_builder_repair(builder, target).unwrap();

        let mut first_repair_tick = None;
        for tick in 1..=20 {
            sim.step();
            if sim.building(target).unwrap().health > damaged && first_repair_tick.is_none() {
                first_repair_tick = Some(tick);
            }
            if sim.building(target).unwrap().health == 900 {
                break;
            }
        }
        assert!(
            first_repair_tick.is_some_and(|tick| tick >= 3),
            "builder must move into repair range before healing"
        );
        assert_eq!(sim.building(target).unwrap().health, 900);
        assert_eq!(sim.builder(builder).unwrap().repair_target, None);
        assert!(
            sim.builder(builder).unwrap().position.x > 5 * cell,
            "builder must cross the static blocker while moving to repair"
        );
    }

    #[test]
    fn builder_repair_autocast_is_toggleable_and_repairs_mechanical_units() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(29, 9),
            team_build_regions: [
                vec![BuildingFootprint::new(0, 0, 20, 10)],
                vec![BuildingFootprint::new(20, 0, 10, 10)],
            ],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 1);
        let builder = sim.spawn_builder(BuilderSpawn {
            team: Team(0),
            position: SimPoint::new(cell, 4 * cell),
            profile: BuilderProfile {
                speed_per_tick: 0,
                build_range: 10 * cell,
                repair_range: 10 * cell,
                repair_autocast_range: 20 * cell,
                repair_time_ratio_numerator: 3,
                repair_time_ratio_denominator: 2,
                full_repair_duration_ticks: 3,
                blink_range: 20 * cell,
                blink_boundary_inset: 0,
            },
            configuration: test_builder_configuration(vec![]),
            repair_autocast_enabled: true,
        });
        assert!(sim.builder(builder).unwrap().repair_autocast_enabled);
        sim.set_builder_repair_autocast(builder, false).unwrap();
        assert!(!sim.builder(builder).unwrap().repair_autocast_enabled);

        let mechanical = sim.spawn_unit_with_properties(
            UnitSpawn {
                team: Team(0),
                position: SimPoint::new(3 * cell, 4 * cell),
                health: 600,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 0,
                    range: cell,
                    acquisition_range: cell,
                    cooldown_ticks: 30,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
            UnitGameplayProperties {
                mechanical: true,
                // Four ticks of target Repair Time at a 1.5x Repair Time Ratio gives a six-tick
                // full repair duration: 100 HP/tick for this 600-HP test unit.
                build_time_ticks: Some(4),
                repair_time_ticks: Some(4),
                ..UnitGameplayProperties::default()
            },
        );
        assert!(sim.unit(mechanical).unwrap().mechanical);

        let organic = sim.spawn_unit(passive_unit(0, 5 * cell));
        assert_eq!(
            sim.order_builder_repair(builder, organic),
            Err(BuilderCommandError::RepairTargetNotRepairable)
        );

        sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(4 * cell, 4 * cell),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 300,
                range: 4 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 1_000,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        for _ in 0..4 {
            sim.step();
            if sim.unit(mechanical).unwrap().health < 600 {
                break;
            }
        }
        assert_eq!(sim.unit(mechanical).unwrap().health, 300);
        assert_eq!(sim.builder(builder).unwrap().repair_target, None);

        sim.set_builder_repair_autocast(builder, true).unwrap();
        sim.step();
        assert_eq!(
            sim.builder(builder).unwrap().repair_target,
            Some(mechanical)
        );
        assert_eq!(sim.unit(mechanical).unwrap().health, 400);
        sim.step();
        sim.step();
        assert_eq!(sim.unit(mechanical).unwrap().health, 600);
        assert_eq!(sim.builder(builder).unwrap().repair_target, None);
    }

    #[test]
    fn builder_repair_autocast_does_not_override_explicit_move() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(29, 9),
            team_build_regions: [
                vec![BuildingFootprint::new(0, 0, 20, 10)],
                vec![BuildingFootprint::new(20, 0, 10, 10)],
            ],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 1);
        let builder = sim.spawn_builder(BuilderSpawn {
            team: Team(0),
            position: SimPoint::new(cell, 4 * cell),
            profile: BuilderProfile {
                speed_per_tick: cell,
                build_range: 2 * cell,
                repair_range: 2 * cell,
                repair_autocast_range: 10 * cell,
                repair_time_ratio_numerator: 1,
                repair_time_ratio_denominator: 1,
                full_repair_duration_ticks: 10,
                blink_range: 20 * cell,
                blink_boundary_inset: 0,
            },
            configuration: test_builder_configuration(vec![]),
            repair_autocast_enabled: true,
        });
        let target = sim.spawn_building(passive_building(0, BuildingFootprint::new(3, 4, 1, 1)));
        sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(6 * cell, 4 * cell),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 100,
                range: 4 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 1_000,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        for _ in 0..4 {
            sim.step();
            if sim.building(target).unwrap().health < 10_000 {
                break;
            }
        }
        assert!(sim.building(target).unwrap().health < 10_000);

        let destination = SimPoint::new(8 * cell, 4 * cell);
        sim.order_builder_move(builder, destination).unwrap();
        sim.step();
        let view = sim.builder(builder).unwrap();
        assert_eq!(view.repair_target, None);
        assert_eq!(view.destination, Some(destination));
        assert_eq!(view.position, SimPoint::new(2 * cell, 4 * cell));
    }

    #[test]
    fn live_unit_blocks_building_placement() {
        let mut sim = Simulation::new(SimulationConfig::default(), 1);
        sim.spawn_unit(passive_unit(0, 20 * SUBUNITS_PER_WORLD_UNIT));
        let result =
            sim.try_spawn_building(passive_building(0, BuildingFootprint::new(20, 0, 1, 1)));
        assert_eq!(result, Err(BuildingPlacementError::UnitOccupied));
    }

    #[test]
    fn custom_collision_radius_blocks_building_edge() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 1);
        sim.spawn_unit_with_collision_radius(
            passive_unit(0, 19 * world),
            CollisionRadius(2 * world),
        );

        let result =
            sim.try_spawn_building(passive_building(0, BuildingFootprint::new(20, 0, 1, 1)));
        assert_eq!(result, Err(BuildingPlacementError::UnitOccupied));
    }

    #[test]
    fn production_units_inherit_custom_collision_radius() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 1);
        sim.spawn_building_with_production_collision_radius(
            production_building(0, BuildingFootprint::new(20, 0, 1, 1), 2),
            CollisionRadius(world / 8),
        );

        assert_eq!(sim.step().units_spawned, 1);
        assert_eq!(sim.units()[0].collision_radius, world / 8);
    }

    #[test]
    fn production_can_delay_its_first_spawn() {
        let mut sim = Simulation::new(SimulationConfig::default(), 1);
        let mut building = production_building(0, BuildingFootprint::new(20, 0, 1, 1), 1);
        building
            .production
            .as_mut()
            .expect("production profile missing")
            .initial_delay_ticks = 3;
        sim.spawn_building(building);

        for _ in 0..3 {
            assert_eq!(sim.step().units_spawned, 0);
        }
        assert_eq!(sim.step().units_spawned, 1);
    }

    #[test]
    fn exact_overlap_separates_units() {
        let mut sim = Simulation::new(SimulationConfig::default(), 4);
        let moving = |team| UnitSpawn {
            team: Team(team),
            position: SimPoint::new(20 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 8,
            },
        };
        let first = sim.spawn_unit(moving(0));
        let second = sim.spawn_unit(moving(0));

        sim.step();
        assert_ne!(
            sim.unit(first).unwrap().position,
            sim.unit(second).unwrap().position
        );
    }

    #[test]
    fn custom_collision_radii_use_pairwise_sum_not_global_spacing() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 1);
        let first = sim.spawn_unit_with_collision_radius(
            passive_unit(0, 20 * world),
            CollisionRadius(world / 8),
        );
        let second = sim.spawn_unit_with_collision_radius(
            passive_unit(0, 20 * world + world / 2),
            CollisionRadius(world / 8),
        );
        let before = (
            sim.unit(first).unwrap().position,
            sim.unit(second).unwrap().position,
        );
        sim.step();
        assert_eq!(
            (
                sim.unit(first).unwrap().position,
                sim.unit(second).unwrap().position
            ),
            before,
            "small units should be allowed closer than the legacy global spacing"
        );

        let mut overlap = Simulation::new(SimulationConfig::default(), 1);
        let small = overlap.spawn_unit_with_collision_radius(
            passive_unit(0, 20 * world),
            CollisionRadius(world / 4),
        );
        let large = overlap.spawn_unit_with_collision_radius(
            passive_unit(0, 20 * world + world / 2),
            CollisionRadius(world / 2),
        );
        overlap.step();
        let distance_sq = overlap
            .unit(small)
            .unwrap()
            .position
            .distance_sq(overlap.unit(large).unwrap().position);
        let required = 3 * world / 4;
        assert!(distance_sq >= (i64::from(required) * i64::from(required)) as u64);
    }

    #[test]
    fn radius_aware_unit_pursuit_finds_alternate_attack_position() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_cell_size: 10 * world,
            navigation_min: NavCell::new(0, -10),
            navigation_max: NavCell::new(50, 10),
            static_blockers: vec![BuildingFootprint::new(28, 0, 1, 1)],
            team_objective: [
                SimPoint::new(45 * 10 * world, 5 * world),
                SimPoint::new(5 * 10 * world, 5 * world),
            ],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        let attacker = sim.spawn_unit_with_collision_radius(
            UnitSpawn {
                team: Team(0),
                position: SimPoint::new(205 * world, 5 * world),
                health: 10_000,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 0,
                    range: 20 * world,
                    acquisition_range: 150 * world,
                    cooldown_ticks: 5,
                },
                movement: MovementProfile {
                    speed_per_tick: 5 * world,
                },
            },
            CollisionRadius(4 * world),
        );
        let target = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(305 * world, 5 * world),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        let mut attacked = false;
        for _ in 0..80 {
            sim.step();
            attacked |= sim
                .attacks_last_tick()
                .iter()
                .any(|event| event.source == attacker && event.target == target);
            if attacked {
                break;
            }
        }
        assert!(
            attacked,
            "radius-aware pursuer stalled at a blocked nearest attack-envelope point"
        );
    }

    #[test]
    fn radius_aware_building_pursuit_finds_alternate_attack_position() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_cell_size: 10 * world,
            navigation_min: NavCell::new(0, -10),
            navigation_max: NavCell::new(50, 10),
            static_blockers: vec![BuildingFootprint::new(28, 0, 1, 1)],
            team_objective: [
                SimPoint::new(45 * 10 * world, 5 * world),
                SimPoint::new(5 * 10 * world, 5 * world),
            ],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        let attacker = sim.spawn_unit_with_collision_radius(
            UnitSpawn {
                team: Team(0),
                position: SimPoint::new(205 * world, 5 * world),
                health: 10_000,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 0,
                    range: 20 * world,
                    acquisition_range: 180 * world,
                    cooldown_ticks: 5,
                },
                movement: MovementProfile {
                    speed_per_tick: 5 * world,
                },
            },
            CollisionRadius(4 * world),
        );
        let target = sim.spawn_building(passive_building(1, BuildingFootprint::new(30, 0, 2, 2)));

        let mut attacked = false;
        for _ in 0..80 {
            sim.step();
            attacked |= sim
                .attacks_last_tick()
                .iter()
                .any(|event| event.source == attacker && event.target == target);
            if attacked {
                break;
            }
        }
        assert!(
            attacked,
            "radius-aware pursuer stalled at a blocked nearest building attack-envelope point"
        );
    }

    #[test]
    fn mixed_collision_radii_are_worker_count_independent() {
        fn run(workers: usize) -> u64 {
            let world = SUBUNITS_PER_WORLD_UNIT;
            let config = SimulationConfig {
                navigation_min: NavCell::new(0, 0),
                navigation_max: NavCell::new(80, 30),
                team_objective: [
                    SimPoint::new(75 * world, 15 * world),
                    SimPoint::new(5 * world, 15 * world),
                ],
                ..SimulationConfig::default()
            };
            let mut sim = Simulation::new(config, workers);
            for team in 0..=1 {
                for index in 0..40 {
                    let x = if team == 0 { 10 } else { 70 };
                    let y = 2 + index % 20;
                    let mut unit = passive_unit(team, x * world);
                    unit.position = SimPoint::new(x * world, y * world);
                    unit.movement.speed_per_tick = world / 4;
                    let radius = world / 8 + (index % 3) * world / 16;
                    sim.spawn_unit_with_collision_radius(unit, CollisionRadius(radius));
                }
            }
            for _ in 0..160 {
                sim.step();
            }
            sim.checksum()
        }

        assert_eq!(run(1), run(8));
    }

    #[test]
    fn collision_is_global_across_disconnected_navigation_components() {
        let config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(2, 2),
            static_blockers: vec![
                BuildingFootprint::new(1, 0, 1, 1),
                BuildingFootprint::new(0, 1, 1, 1),
            ],
            team_objective: [
                SimPoint::new(2 * SUBUNITS_PER_WORLD_UNIT, 2 * SUBUNITS_PER_WORLD_UNIT),
                SimPoint::new(0, 0),
            ],
            ..SimulationConfig::default()
        };
        let minimum_distance = config.unit_separation_distance;
        let mut sim = Simulation::new(config, 2);
        let spawn = |position| UnitSpawn {
            team: Team(0),
            position,
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        };
        let first = sim.spawn_unit(spawn(SimPoint::new(
            SUBUNITS_PER_WORLD_UNIT - 16,
            SUBUNITS_PER_WORLD_UNIT - 16,
        )));
        let second = sim.spawn_unit(spawn(SimPoint::new(
            SUBUNITS_PER_WORLD_UNIT + 16,
            SUBUNITS_PER_WORLD_UNIT + 16,
        )));

        sim.step();
        let distance_sq = sim
            .unit(first)
            .unwrap()
            .position
            .distance_sq(sim.unit(second).unwrap().position);
        assert!(distance_sq >= (i64::from(minimum_distance) * i64::from(minimum_distance)) as u64);
    }

    #[test]
    fn converging_crowd_never_commits_overlapping_units() {
        let mut config = SimulationConfig {
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(40, 20),
            team_objective: [
                SimPoint::new(40 * SUBUNITS_PER_WORLD_UNIT, 10 * SUBUNITS_PER_WORLD_UNIT),
                SimPoint::new(0, 10 * SUBUNITS_PER_WORLD_UNIT),
            ],
            ..SimulationConfig::default()
        };
        config.unit_separation_distance = 3 * SUBUNITS_PER_WORLD_UNIT / 4;
        let minimum_distance_sq = {
            let distance = i64::from(config.unit_separation_distance);
            (distance * distance) as u64
        };
        let mut sim = Simulation::new(config, 4);
        let template = UnitTemplate {
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 4,
            },
        };

        for y in 1..=10 {
            for x in 1..=10 {
                sim.spawn_unit(UnitSpawn::from_template(
                    Team(0),
                    SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, y * SUBUNITS_PER_WORLD_UNIT),
                    template,
                ));
            }
        }

        for _ in 0..300 {
            sim.step();
            let units = sim.units();
            for (index, unit) in units.iter().enumerate() {
                for other in &units[index + 1..] {
                    assert!(
                        unit.position.distance_sq(other.position) >= minimum_distance_sq,
                        "units {:?} and {:?} overlap at tick {}: {:?} vs {:?}",
                        unit.id,
                        other.id,
                        sim.tick(),
                        unit.position,
                        other.position,
                    );
                }
            }
        }
    }

    #[test]
    fn crowd_separation_is_worker_count_independent() {
        let mut expected = None;
        for workers in [1, 2, 4, 8] {
            let mut sim = Simulation::new(SimulationConfig::default(), workers);
            populate_crossing_test_units(&mut sim, 2_048);
            for _ in 0..100 {
                sim.step();
            }

            match expected {
                Some(checksum) => assert_eq!(sim.checksum(), checksum, "workers={workers}"),
                None => expected = Some(sim.checksum()),
            }
        }
    }

    #[test]
    fn attacker_preempts_passive_building_target() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let castle = sim.spawn_building(passive_building(1, BuildingFootprint::new(11, 0, 1, 1)));
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(castle));

        let attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(9 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(castle));
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(attacker));
    }

    #[test]
    fn nearby_ally_attack_preempts_building_target() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 8,
            },
        });
        let ally = sim.spawn_unit(passive_unit(0, 12 * SUBUNITS_PER_WORLD_UNIT));
        let castle = sim.spawn_building(passive_building(1, BuildingFootprint::new(11, 0, 1, 1)));
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(castle));

        let attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(12 * SUBUNITS_PER_WORLD_UNIT, SUBUNITS_PER_WORLD_UNIT),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert_eq!(sim.unit(attacker).unwrap().target, Some(ally));
        assert_eq!(sim.unit(defender).unwrap().target, Some(castle));
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(attacker));
    }

    #[test]
    fn castle_attackers_peel_to_arriving_defender_after_attack() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let castle = sim.spawn_building(passive_building(0, BuildingFootprint::new(10, -1, 2, 3)));
        let castle_attacker = |y: i32| UnitSpawn {
            team: Team(1),
            position: SimPoint::new(8 * cell, y),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 3 * cell,
                acquisition_range: 8 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        };
        let directly_attacked = sim.spawn_unit(castle_attacker(0));
        let nearby_attacker = sim.spawn_unit(castle_attacker(2 * cell));

        sim.step();
        sim.step();
        assert_eq!(sim.unit(directly_attacked).unwrap().target, Some(castle));
        assert_eq!(sim.unit(nearby_attacker).unwrap().target, Some(castle));

        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(7 * cell, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step(); // defender is spawn-tick suppressed
        assert_eq!(sim.unit(defender).unwrap().target, Some(directly_attacked));
        assert_eq!(sim.unit(directly_attacked).unwrap().target, Some(castle));
        assert_eq!(sim.unit(nearby_attacker).unwrap().target, Some(castle));

        sim.step(); // defender actually attacks the first castle attacker
        assert_eq!(
            sim.unit(directly_attacked).unwrap().last_attacker,
            Some(defender)
        );
        assert_eq!(sim.unit(directly_attacked).unwrap().target, Some(castle));
        assert_eq!(sim.unit(nearby_attacker).unwrap().target, Some(castle));

        sim.step(); // direct retaliation + nearby ally defense both peel off the castle
        assert_eq!(sim.unit(directly_attacked).unwrap().target, Some(defender));
        assert_eq!(sim.unit(nearby_attacker).unwrap().target, Some(defender));
    }

    #[test]
    fn castle_attackers_answer_nearby_ally_defense_against_distant_tower() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let castle = sim.spawn_building(passive_building(0, BuildingFootprint::new(20, -1, 2, 3)));
        let castle_attacker = |y: i32| UnitSpawn {
            team: Team(1),
            position: SimPoint::new(18 * cell, y),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 3 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile {
                speed_per_tick: cell / 8,
            },
        };
        let directly_attacked = sim.spawn_unit(castle_attacker(0));
        let nearby_attacker = sim.spawn_unit(castle_attacker(2 * cell));

        sim.step();
        sim.step();
        assert_eq!(sim.unit(directly_attacked).unwrap().target, Some(castle));
        assert_eq!(sim.unit(nearby_attacker).unwrap().target, Some(castle));

        let tower = sim.spawn_building(attack_building(
            0,
            BuildingFootprint::new(8, 0, 1, 1),
            AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: 20 * cell,
                },
                damage: 1,
                range: 12 * cell,
                acquisition_range: 12 * cell,
                cooldown_ticks: 30,
            },
        ));

        // The attacked gryphon is nearby enough to call for help, but the tower's right edge at
        // x=9 is beyond the other gryphon's ordinary 4-tile acquisition + 3-tile pursuit allowance.
        let tower_right_edge_x = 9 * cell;
        assert!(sim.unit(nearby_attacker).unwrap().position.x - tower_right_edge_x > 7 * cell);

        let mut tower_hit = false;
        for _ in 0..4 {
            sim.step();
            if sim.unit(directly_attacked).unwrap().last_attacker == Some(tower) {
                tower_hit = true;
                break;
            }
        }
        assert!(tower_hit, "tower never attacked the castle attacker");
        assert_eq!(sim.unit(nearby_attacker).unwrap().target, Some(castle));

        sim.step();
        assert_eq!(sim.unit(directly_attacked).unwrap().target, Some(tower));
        assert!(sim.unit(directly_attacked).unwrap().direct_retaliation_lock);
        assert_eq!(sim.unit(nearby_attacker).unwrap().target, Some(tower));
        assert!(sim.unit(nearby_attacker).unwrap().ally_defense_lock);

        let directly_attacked_position = sim.unit(directly_attacked).unwrap().position;
        let arriving_unit = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(
                directly_attacked_position.x - cell,
                directly_attacked_position.y,
            ),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step(); // arriving unit is spawn-tick suppressed
        sim.step(); // arriving unit attacks the directly tower-locked unit
        assert_eq!(
            sim.unit(directly_attacked).unwrap().last_attacker,
            Some(arriving_unit)
        );
        sim.step(); // unit priority peels both direct- and ally-defense tower locks
        assert_eq!(
            sim.unit(directly_attacked).unwrap().target,
            Some(arriving_unit)
        );
        assert_eq!(
            sim.unit(nearby_attacker).unwrap().target,
            Some(arriving_unit)
        );
    }

    #[test]
    fn ally_defense_orders_nearest_ally_then_nearest_attacker_when_idle() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            team_objective: [SimPoint::new(10 * cell, 0), SimPoint::new(0, 0)],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 4);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 3 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: cell / 8,
            },
        });
        let near_ally = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 3 * cell),
            ..passive_unit(0, 0)
        });
        let far_ally = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(14 * cell, 0),
            ..passive_unit(0, 0)
        });
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, None);

        let attacker_for_near_ally_farther = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(10 * cell, 5 * cell),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 3 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let attacker_for_near_ally_closer = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(11 * cell, 4 * cell),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 3 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let attacker_for_far_ally = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(15 * cell, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 3 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, None);
        sim.step();
        assert_eq!(
            sim.unit(attacker_for_near_ally_farther).unwrap().target,
            Some(near_ally)
        );
        assert_eq!(
            sim.unit(attacker_for_near_ally_closer).unwrap().target,
            Some(near_ally)
        );
        assert_eq!(
            sim.unit(attacker_for_far_ally).unwrap().target,
            Some(far_ally)
        );
        assert_eq!(sim.unit(defender).unwrap().target, None);
        sim.step();
        assert_eq!(
            sim.unit(defender).unwrap().target,
            Some(attacker_for_near_ally_closer)
        );
    }

    #[test]
    fn ally_defense_prefers_unit_attacker_over_nearer_building_attacker() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 2 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: cell / 8,
            },
        });
        let near_ally = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(11 * cell, 0),
            ..passive_unit(0, 0)
        });
        let far_ally = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(14 * cell, 0),
            ..passive_unit(0, 0)
        });
        let tower = sim.spawn_building(attack_building(
            1,
            BuildingFootprint::new(20, 0, 1, 1),
            AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: 20 * cell,
                },
                damage: 1,
                range: 12 * cell,
                acquisition_range: 12 * cell,
                cooldown_ticks: 1,
            },
        ));
        sim.order_building_attack_target(tower, near_ally).unwrap();
        let unit_attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(16 * cell, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 3 * cell,
                acquisition_range: 3 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, None);

        let mut simultaneous_alert_tick = None;
        for _ in 0..4 {
            sim.step();
            let near = sim.unit(near_ally).unwrap();
            let far = sim.unit(far_ally).unwrap();
            if near.last_attacker == Some(tower)
                && far.last_attacker == Some(unit_attacker)
                && near.last_attacked_tick == far.last_attacked_tick
            {
                simultaneous_alert_tick = near.last_attacked_tick;
                break;
            }
        }
        assert!(
            simultaneous_alert_tick.is_some(),
            "tower and unit never produced simultaneous ally-defense alerts"
        );

        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(unit_attacker));
    }

    #[test]
    fn ally_defense_target_stays_sticky_when_other_allies_are_attacked() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            team_objective: [SimPoint::new(10 * cell, 0), SimPoint::new(0, 0)],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 3 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: cell / 8,
            },
        });
        let right_ally = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 3 * cell),
            ..passive_unit(0, 0)
        });
        let left_ally = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(6 * cell, 0),
            ..passive_unit(0, 0)
        });
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, None);

        let right_attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(10 * cell, 5 * cell),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 3 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert_eq!(sim.unit(right_attacker).unwrap().target, Some(right_ally));
        assert_eq!(sim.unit(defender).unwrap().target, None);
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(right_attacker));

        let left_attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(4 * cell, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 3 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert_eq!(sim.unit(left_attacker).unwrap().target, Some(left_ally));
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(right_attacker));
        assert_ne!(sim.unit(defender).unwrap().target, Some(left_attacker));
    }

    #[test]
    fn unit_ally_defense_target_preempts_locked_building_target() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 4 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: cell / 8,
            },
        });
        let first_ally = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(13 * cell, 0),
            health: 1_000,
            ..passive_unit(0, 0)
        });
        let first_attacker = sim.spawn_building(attack_building(
            1,
            BuildingFootprint::new(17, 0, 1, 1),
            AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 5 * cell,
                acquisition_range: 5 * cell,
                cooldown_ticks: 1,
            },
        ));

        sim.step();
        sim.step();
        assert_eq!(
            sim.unit(first_ally).unwrap().last_attacker,
            Some(first_attacker)
        );
        sim.step();
        let defender_view = sim.unit(defender).unwrap();
        assert_eq!(defender_view.target, Some(first_attacker));
        assert!(defender_view.ally_defense_lock);
        assert!(!defender_view.direct_retaliation_lock);

        let second_ally = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 2 * cell),
            health: 1_000,
            ..passive_unit(0, 0)
        });
        let second_attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(10 * cell, 3 * cell),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: cell,
                acquisition_range: 2 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step();
        sim.step();
        assert_eq!(
            sim.unit(second_ally).unwrap().last_attacker,
            Some(second_attacker)
        );
        sim.step();
        let defender_view = sim.unit(defender).unwrap();
        assert_eq!(defender_view.target, Some(second_attacker));
        assert!(defender_view.ally_defense_lock);
        assert!(!defender_view.direct_retaliation_lock);

        let self_attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(10 * cell, -cell),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 2 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert_eq!(
            sim.unit(defender).unwrap().last_attacker,
            Some(self_attacker)
        );
        sim.step();
        let defender_view = sim.unit(defender).unwrap();
        assert_eq!(defender_view.target, Some(self_attacker));
        assert!(defender_view.direct_retaliation_lock);
        assert!(!defender_view.ally_defense_lock);
    }

    #[test]
    fn ally_defense_lock_ignores_ordinary_pursuit_leash() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            team_objective: [SimPoint::new(120 * cell, 0), SimPoint::new(30 * cell, 0)],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(30 * cell, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 3 * cell,
                acquisition_range: 3 * cell,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: cell / 4,
            },
        });
        let ally = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(33 * cell, 0),
            health: 1,
            ..passive_unit(1, 0)
        });
        let attacker = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(34 * cell, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: cell,
                acquisition_range: cell,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: 2 * cell,
            },
        });

        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, None);
        sim.step();
        assert!(sim.unit(ally).is_none());
        assert_eq!(sim.unit(defender).unwrap().target, None);

        sim.step();
        let defender_view = sim.unit(defender).unwrap();
        assert_eq!(defender_view.target, Some(attacker));
        assert!(defender_view.ally_defense_lock);

        for _ in 0..4 {
            sim.step();
        }
        let defender_view = sim.unit(defender).unwrap();
        let attacker_view = sim.unit(attacker).unwrap();
        assert!(
            (attacker_view.position.x - defender_view.position.x).abs() > 6 * cell,
            "fixture did not move the defense target beyond the ordinary pursuit leash"
        );
        assert_eq!(defender_view.target, Some(attacker));
        assert!(defender_view.ally_defense_lock);
        assert!(!defender_view.direct_retaliation_lock);
    }

    #[test]
    fn first_personal_attacker_stays_locked_despite_later_attackers() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 0),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 2 * cell,
                acquisition_range: 8 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let passive = sim.spawn_unit(passive_unit(1, 12 * cell));
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(passive));

        let first_attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(9 * cell, 0),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 100,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let later_attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(11 * cell, 0),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step();
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(passive));
        assert_eq!(
            sim.unit(defender).unwrap().last_attacker,
            Some(first_attacker)
        );
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(first_attacker));
        assert_eq!(
            sim.unit(defender).unwrap().last_attacker,
            Some(later_attacker)
        );
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(first_attacker));
    }

    #[test]
    fn lethal_hit_still_alerts_idle_nearby_ally() {
        let cell = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 3 * cell,
                acquisition_range: 4 * cell,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: cell / 8,
            },
        });
        let ally = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * cell, 3 * cell),
            health: 1,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, None);

        let attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(10 * cell, 5 * cell),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * cell,
                acquisition_range: 3 * cell,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert!(sim.unit(ally).is_none());
        assert_eq!(sim.unit(defender).unwrap().target, None);
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(attacker));
    }

    #[test]
    fn mutual_fight_ignores_nearby_ally_defense_alert() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let defender = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(10 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 2 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let mutual = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(11 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 2 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let ally = sim.spawn_unit(passive_unit(0, 13 * SUBUNITS_PER_WORLD_UNIT));
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(mutual));
        assert_eq!(sim.unit(mutual).unwrap().target, Some(defender));

        let attacker = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(13 * SUBUNITS_PER_WORLD_UNIT, SUBUNITS_PER_WORLD_UNIT),
            health: 1_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 2 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        sim.step();
        sim.step();
        assert_eq!(sim.unit(attacker).unwrap().target, Some(ally));
        sim.step();
        assert_eq!(sim.unit(defender).unwrap().target, Some(mutual));
    }

    #[test]
    fn trailing_melee_units_sidestep_around_engaged_frontline() {
        let mut sim = Simulation::new(SimulationConfig::default(), 4);
        let moving_melee = |team: u8, x: i32| UnitSpawn {
            team: Team(team),
            position: SimPoint::new(x, 0),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 8,
            },
        };
        let spacing = 3 * SUBUNITS_PER_WORLD_UNIT / 4;
        let front = sim.spawn_unit(moving_melee(0, 20 * SUBUNITS_PER_WORLD_UNIT));
        let rear_a = sim.spawn_unit(moving_melee(0, 20 * SUBUNITS_PER_WORLD_UNIT - spacing));
        let rear_b = sim.spawn_unit(moving_melee(0, 20 * SUBUNITS_PER_WORLD_UNIT - 2 * spacing));
        let enemy_front = sim.spawn_unit(moving_melee(1, 24 * SUBUNITS_PER_WORLD_UNIT));
        sim.spawn_unit(moving_melee(1, 24 * SUBUNITS_PER_WORLD_UNIT + spacing));
        sim.spawn_unit(moving_melee(1, 24 * SUBUNITS_PER_WORLD_UNIT + 2 * spacing));

        let mut rear_attacked = false;
        let mut lateral_displacement = 0_i32;
        for _ in 0..240 {
            sim.step();
            for rear in [rear_a, rear_b] {
                lateral_displacement =
                    lateral_displacement.max(sim.unit(rear).unwrap().position.y.abs());
            }
            rear_attacked |= sim
                .attacks_last_tick()
                .iter()
                .any(|attack| attack.source == rear_a || attack.source == rear_b);
            if rear_attacked {
                break;
            }
        }

        assert_eq!(sim.unit(front).unwrap().target, Some(enemy_front));
        assert!(
            lateral_displacement >= SUBUNITS_PER_WORLD_UNIT / 8,
            "rear units never sidestepped around the engaged front"
        );
        assert!(rear_attacked, "rear units never reached an attack position");
    }

    #[test]
    fn trailing_ranged_unit_routes_around_occupied_firing_clump() {
        fn run(workers: usize) -> (u64, i32, Option<usize>, usize, usize) {
            let world = SUBUNITS_PER_WORLD_UNIT;
            let mut config = SimulationConfig {
                spatial_cell_size: 40 * world,
                navigation_cell_size: 10 * world,
                navigation_min: NavCell::new(0, -40),
                navigation_max: NavCell::new(100, 40),
                target_pursuit_extra_range: 30 * world,
                unit_separation_distance: 8 * world,
                max_separation_per_tick: world,
                team_objective: [SimPoint::new(900 * world, 0), SimPoint::new(100 * world, 0)],
                ..SimulationConfig::default()
            };
            config.static_blockers.clear();
            let mut sim = Simulation::new(config, workers);
            let ranged = |x_world: i32, y_world: i32| UnitSpawn {
                team: Team(0),
                position: SimPoint::new(x_world * world, y_world * world),
                health: 10_000,
                attack: AttackProfile {
                    delivery: AttackDelivery::RangedGuaranteedHit {
                        speed_per_tick: 10 * world,
                    },
                    damage: 0,
                    range: 120 * world,
                    acquisition_range: 180 * world,
                    cooldown_ticks: 30,
                },
                movement: MovementProfile {
                    speed_per_tick: 40 * world / 30,
                },
            };
            for row in -2..=2 {
                sim.spawn_unit(ranged(180, row * 8));
            }
            let rear = sim.spawn_unit(ranged(172, 0));
            let target = sim.spawn_unit(UnitSpawn {
                team: Team(1),
                position: SimPoint::new(300 * world, 0),
                health: 10_000,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    cooldown_ticks: 30,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            });

            let mut max_lateral = 0;
            let mut attack_tick = None;
            let mut previous = sim.unit(rear).unwrap().position;
            let mut previous_dy: i32 = 0;
            let mut vertical_reversals = 0;
            let mut stationary_ticks = 0;
            for tick in 0..120 {
                sim.step();
                let rear_view = sim.unit(rear).unwrap();
                max_lateral = max_lateral.max(rear_view.position.y.abs());
                let dx = rear_view.position.x - previous.x;
                let dy = rear_view.position.y - previous.y;
                stationary_ticks += usize::from(dx == 0 && dy == 0);
                if dy != 0 && previous_dy != 0 && dy.signum() != previous_dy.signum() {
                    vertical_reversals += 1;
                }
                if dy != 0 {
                    previous_dy = dy;
                }
                previous = rear_view.position;
                if sim
                    .attacks_last_tick()
                    .iter()
                    .any(|attack| attack.source == rear && attack.target == target)
                {
                    attack_tick = Some(tick);
                    break;
                }
            }
            (
                sim.checksum(),
                max_lateral,
                attack_tick,
                vertical_reversals,
                stationary_ticks,
            )
        }

        let expected = run(1);
        let parallel = run(8);
        assert_eq!(
            parallel, expected,
            "worker count changed ranged-clump bypass"
        );
        let (_, max_lateral, attack_tick, vertical_reversals, stationary_ticks) = expected;
        let world = SUBUNITS_PER_WORLD_UNIT;
        assert!(
            max_lateral >= 8 * world,
            "rear ranged unit lateral bypass was too small: {max_lateral}"
        );
        assert!(
            attack_tick.is_some_and(|tick| tick < 40),
            "rear ranged unit took too long to reach a firing position: {attack_tick:?}"
        );
        assert!(
            vertical_reversals <= 2,
            "rear ranged unit jittered between bypass directions {vertical_reversals} times"
        );
        assert!(
            stationary_ticks <= 2,
            "rear ranged unit stalled for {stationary_ticks} ticks while pursuing"
        );
    }

    #[test]
    fn ranged_unit_already_inside_max_range_does_not_back_away() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let attacker = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(20 * world, 0),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: 10 * world,
                },
                damage: 0,
                range: 120 * world,
                acquisition_range: 180 * world,
                cooldown_ticks: 1,
            },
            movement: MovementProfile {
                speed_per_tick: 40 * world / 30,
            },
        });
        let target = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(80 * world, 0),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let start = sim.unit(attacker).unwrap().position;
        let mut attacked = false;
        for _ in 0..5 {
            sim.step();
            attacked |= sim
                .attacks_last_tick()
                .iter()
                .any(|event| event.source == attacker && event.target == target);
            assert_eq!(sim.unit(attacker).unwrap().position, start);
        }
        assert!(attacked, "in-range ranged unit never attacked");
    }

    #[test]
    fn objective_march_preserves_current_horizontal_line() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_cell_size: 10 * world,
            navigation_min: NavCell::new(0, -20),
            navigation_max: NavCell::new(100, 20),
            team_objective: [
                SimPoint::new(900 * world, 100 * world),
                SimPoint::new(100 * world, -100 * world),
            ],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        let start = SimPoint::new(100 * world, 17 * world + world / 3);
        let unit = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: start,
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: world,
            },
        });

        for _ in 0..20 {
            sim.step();
            let position = sim.unit(unit).unwrap().position;
            assert_eq!(position.y, start.y, "objective movement drifted vertically");
        }
        assert!(sim.unit(unit).unwrap().position.x > start.x);
    }

    #[test]
    fn targetless_lane_ingress_moves_diagonally_then_keeps_entry_line() {
        fn run(team: Team, start: SimPoint) {
            let world = SUBUNITS_PER_WORLD_UNIT;
            let lane = TargetlessLane::new(-40 * world, 40 * world);
            let config = SimulationConfig {
                navigation_cell_size: 10 * world,
                navigation_min: NavCell::new(0, -20),
                navigation_max: NavCell::new(100, 20),
                targetless_lane: Some(lane),
                team_objective: [SimPoint::new(900 * world, 0), SimPoint::new(100 * world, 0)],
                ..SimulationConfig::default()
            };
            let mut sim = Simulation::new(config, 1);
            let footman = CastleFightUnitKind::Footman.definition();
            let unit = sim.spawn_unit_with_properties(
                UnitSpawn::from_template(team, start, footman.template()),
                footman.gameplay_properties(),
            );

            sim.step();
            let first = sim.unit(unit).unwrap().position;
            let forward = if team.0 == 0 {
                first.x > start.x
            } else {
                first.x < start.x
            };
            let inward = if start.y > 0 {
                first.y < start.y
            } else {
                first.y > start.y
            };
            assert!(
                forward && inward,
                "off-lane unit did not approach the lane diagonally: {start:?} -> {first:?}"
            );

            let safe_min_y = lane.min_y + footman.collision_radius.0;
            let safe_max_y = lane.max_y - footman.collision_radius.0;
            for _ in 0..80 {
                let position = sim.unit(unit).unwrap().position;
                if position.y >= safe_min_y && position.y <= safe_max_y {
                    break;
                }
                sim.step();
            }
            let entered = sim.unit(unit).unwrap().position;
            assert!(entered.y >= safe_min_y && entered.y <= safe_max_y);
            let forward_distance = (i64::from(entered.x) - i64::from(start.x)).abs();
            let inward_distance = (i64::from(entered.y) - i64::from(start.y)).abs();
            assert!(
                (forward_distance - inward_distance).abs() <= i64::from(20 * world),
                "clear ingress did not follow the preferred diagonal: {start:?} -> {entered:?}"
            );

            sim.step();
            let marching = sim.unit(unit).unwrap().position;
            assert_eq!(
                marching.y, entered.y,
                "unit recalibrated vertically after reaching a valid lane line"
            );
            if team.0 == 0 {
                assert!(marching.x > entered.x);
            } else {
                assert!(marching.x < entered.x);
            }
        }

        let world = SUBUNITS_PER_WORLD_UNIT;
        run(Team(0), SimPoint::new(100 * world, 120 * world));
        run(Team(1), SimPoint::new(900 * world, -120 * world));
    }

    #[test]
    fn objective_march_uses_new_horizontal_line_after_combat_displacement() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_cell_size: 10 * world,
            navigation_min: NavCell::new(0, -40),
            navigation_max: NavCell::new(100, 40),
            team_objective: [SimPoint::new(900 * world, 0), SimPoint::new(100 * world, 0)],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        let attacker = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(100 * world, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 100,
                range: 10 * world,
                acquisition_range: 200 * world,
                cooldown_ticks: 1,
            },
            movement: MovementProfile {
                speed_per_tick: 10 * world,
            },
        });
        let victim = sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(170 * world, 40 * world),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        for _ in 0..30 {
            sim.step();
            if sim.unit(victim).is_none() {
                break;
            }
        }
        assert!(sim.unit(victim).is_none(), "combat target never died");
        let displaced = sim.unit(attacker).unwrap().position;
        assert!(
            displaced.y > 0,
            "combat never pulled the attacker off its spawn line"
        );

        sim.step();
        let resumed = sim.unit(attacker).unwrap().position;
        assert!(
            resumed.x > displaced.x,
            "unit did not resume objective march"
        );
        assert_eq!(
            resumed.y, displaced.y,
            "unit attempted to restore a remembered pre-combat lane"
        );
    }

    #[test]
    fn horizontal_objective_march_detours_without_restoring_old_line() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_cell_size: 10 * world,
            navigation_min: NavCell::new(0, -10),
            navigation_max: NavCell::new(30, 10),
            static_blockers: vec![BuildingFootprint::new(10, 0, 1, 1)],
            targetless_lane: Some(TargetlessLane::new(-80 * world, 80 * world)),
            team_objective: [SimPoint::new(290 * world, 0), SimPoint::new(10 * world, 0)],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 1);
        let unit = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(50 * world, 5 * world),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: 10 * world,
            },
        });

        for _ in 0..20 {
            sim.step();
        }
        let after_detour = sim.unit(unit).unwrap().position;
        assert!(
            after_detour.x > 120 * world,
            "unit never cleared the blocker"
        );
        assert_ne!(
            after_detour.y,
            5 * world,
            "unit never took the required detour"
        );
        let detour_y = after_detour.y;

        for _ in 0..5 {
            sim.step();
            assert_eq!(
                sim.unit(unit).unwrap().position.y,
                detour_y,
                "unit tried to return to its pre-detour horizontal line"
            );
        }
    }

    #[test]
    fn imported_ranger_does_not_corner_lock_beside_adjacent_towers() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut config = SimulationConfig {
            spatial_cell_size: 256 * world,
            navigation_cell_size: 32 * world,
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(40, 30),
            target_pursuit_extra_range: 30 * world,
            unit_separation_distance: 8 * world,
            max_separation_per_tick: world,
            team_objective: [
                SimPoint::new(1_100 * world, 315 * world),
                SimPoint::new(100 * world, 315 * world),
            ],
            ..SimulationConfig::default()
        };
        config.static_blockers.clear();
        let mut sim = Simulation::new(config, 1);
        let tower = CastleFightTowerKind::WatchTower.definition();
        for footprint in [
            BuildingFootprint::new(10, 10, 4, 4),
            BuildingFootprint::new(14, 10, 4, 4),
        ] {
            sim.spawn_building_with_properties(
                tower.spawn(Team(0), footprint),
                tower.gameplay_properties(),
            );
        }
        let ranger = CastleFightUnitKind::Ranger.definition();
        let unit = sim.spawn_unit_with_properties(
            UnitSpawn::from_template(
                Team(0),
                SimPoint::new(294 * world, 315 * world),
                ranger.template(),
            ),
            ranger.gameplay_properties(),
        );

        let mut blocked_ticks = 0usize;
        let mut previous = sim.unit(unit).unwrap().position;
        for _ in 0..80 {
            sim.step();
            let current = sim.unit(unit).unwrap().position;
            blocked_ticks += usize::from(current == previous);
            previous = current;
            if current.x > 600 * world {
                break;
            }
        }

        let final_position = sim.unit(unit).unwrap().position;
        assert!(
            final_position.x > 600 * world,
            "ranger remained corner-locked beside towers at {final_position:?}"
        );
        assert!(
            final_position.y <= 304 * world,
            "ranger did not leave the snagged horizontal line at the tower corner: {final_position:?}"
        );
        assert!(
            blocked_ticks <= 4,
            "ranger remained stationary for {blocked_ticks} ticks while an open detour existed"
        );
    }

    #[test]
    fn imported_catapult_does_not_corner_lock_on_single_tower() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut config = SimulationConfig {
            spatial_cell_size: 256 * world,
            navigation_cell_size: 32 * world,
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(40, 30),
            target_pursuit_extra_range: 30 * world,
            unit_separation_distance: 8 * world,
            max_separation_per_tick: world,
            team_objective: [
                SimPoint::new(1_100 * world, 315 * world),
                SimPoint::new(100 * world, 315 * world),
            ],
            ..SimulationConfig::default()
        };
        config.static_blockers.clear();
        let mut sim = Simulation::new(config, 1);
        let tower = CastleFightTowerKind::WatchTower.definition();
        sim.spawn_building_with_properties(
            tower.spawn(Team(0), BuildingFootprint::new(10, 10, 4, 4)),
            tower.gameplay_properties(),
        );
        let catapult = CastleFightUnitKind::Catapult.definition();
        let unit = sim.spawn_unit_with_properties(
            UnitSpawn::from_template(
                Team(0),
                SimPoint::new(304 * world, 316 * world),
                catapult.template(),
            ),
            catapult.gameplay_properties(),
        );

        let mut stationary_run = 0usize;
        let mut longest_stationary_run = 0usize;
        let mut previous = sim.unit(unit).unwrap().position;
        for _ in 0..100 {
            sim.step();
            let current = sim.unit(unit).unwrap().position;
            if current == previous {
                stationary_run += 1;
                longest_stationary_run = longest_stationary_run.max(stationary_run);
            } else {
                stationary_run = 0;
            }
            previous = current;
            if current.x > 520 * world {
                break;
            }
        }

        let final_position = sim.unit(unit).unwrap().position;
        assert!(
            final_position.x > 520 * world,
            "catapult remained corner-locked beside the tower at {final_position:?}"
        );
        assert!(
            longest_stationary_run <= 2,
            "catapult stalled for {longest_stationary_run} consecutive ticks at the tower corner"
        );
    }

    #[test]
    fn imported_ground_pursuit_moves_diagonally_on_open_topology() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let mut config = SimulationConfig {
            navigation_cell_size: 32 * world,
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(20, 20),
            team_objective: [
                SimPoint::new(19 * 32 * world, 10 * 32 * world),
                SimPoint::new(32 * world, 10 * 32 * world),
            ],
            ..SimulationConfig::default()
        };
        config.static_blockers.clear();
        let mut sim = Simulation::new(config, 1);
        let footman = CastleFightUnitKind::Footman.definition();
        let start = SimPoint::new(3 * 32 * world + 16 * world, 3 * 32 * world + 16 * world);
        let attacker = sim.spawn_unit_with_properties(
            UnitSpawn::from_template(Team(0), start, footman.template()),
            footman.gameplay_properties(),
        );
        sim.spawn_unit(UnitSpawn {
            team: Team(1),
            position: SimPoint::new(10 * 32 * world + 16 * world, 10 * 32 * world + 16 * world),
            health: 10_000,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });

        sim.step();
        let position = sim.unit(attacker).unwrap().position;
        assert!(
            position.x > start.x && position.y > start.y,
            "pursuit did not take a diagonal step: {start:?} -> {position:?}"
        );
        assert_eq!(
            position.x - start.x,
            position.y - start.y,
            "open diagonal pursuit should not alternate cardinal headings"
        );
    }

    #[test]
    fn horizontal_march_can_bypass_enemy_mass_outside_acquisition_range() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_cell_size: 10 * world,
            navigation_min: NavCell::new(0, -30),
            navigation_max: NavCell::new(100, 30),
            team_objective: [SimPoint::new(900 * world, 0), SimPoint::new(100 * world, 0)],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        let bypass = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(100 * world, -100 * world),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 10 * world,
                acquisition_range: 60 * world,
                cooldown_ticks: 30,
            },
            movement: MovementProfile {
                speed_per_tick: 10 * world,
            },
        });
        for offset in -2..=2 {
            sim.spawn_unit(UnitSpawn {
                team: Team(1),
                position: SimPoint::new((250 + offset * 8) * world, 20 * world),
                health: 1_000,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    cooldown_ticks: 30,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            });
        }

        for _ in 0..25 {
            sim.step();
        }
        let view = sim.unit(bypass).unwrap();
        assert!(
            view.position.x > 300 * world,
            "unit failed to bypass the enemy clump"
        );
        assert_eq!(view.position.y, -100 * world);
        assert_eq!(
            view.target, None,
            "distant enemy clump incorrectly pulled the unit off-line"
        );
    }

    #[test]
    fn produced_unit_marches_from_its_spawned_horizontal_line() {
        let world = SUBUNITS_PER_WORLD_UNIT;
        let config = SimulationConfig {
            navigation_cell_size: 10 * world,
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(100, 60),
            team_objective: [
                SimPoint::new(900 * world, 300 * world),
                SimPoint::new(100 * world, 300 * world),
            ],
            ..SimulationConfig::default()
        };
        let mut sim = Simulation::new(config, 2);
        sim.spawn_building(BuildingSpawn {
            team: Team(0),
            footprint: BuildingFootprint::new(10, 4, 4, 4),
            health: 1_000,
            production: Some(ProductionProfile {
                initial_delay_ticks: 0,
                interval_ticks: 60,
                search_radius_cells: 4,
                unit: UnitTemplate {
                    health: 100,
                    attack: AttackProfile {
                        delivery: AttackDelivery::Melee,
                        damage: 0,
                        range: 0,
                        acquisition_range: 0,
                        cooldown_ticks: 30,
                    },
                    movement: MovementProfile {
                        speed_per_tick: world,
                    },
                },
            }),
            attack: None,
            spellcasting: None,
        });

        let spawn = sim.step();
        assert_eq!(spawn.units_spawned, 1);
        let produced = sim.units()[0];
        let spawn_y = produced.position.y;
        assert!(
            spawn_y < 100 * world,
            "low production building spawned too high"
        );

        for _ in 0..20 {
            sim.step();
            assert_eq!(
                sim.unit(produced.id).unwrap().position.y,
                spawn_y,
                "produced unit drifted toward objective center"
            );
        }
        assert!(sim.unit(produced.id).unwrap().position.x > produced.position.x);
    }

    #[test]
    fn packed_convoy_uses_simultaneously_vacated_space() {
        fn run(workers: usize) -> (u64, Vec<SimPoint>, TickResult) {
            let world = SUBUNITS_PER_WORLD_UNIT;
            let config = SimulationConfig {
                spatial_cell_size: 40 * world,
                navigation_cell_size: 10 * world,
                navigation_min: NavCell::new(0, -10),
                navigation_max: NavCell::new(100, 10),
                unit_separation_distance: 8 * world,
                max_separation_per_tick: world,
                team_objective: [SimPoint::new(900 * world, 0), SimPoint::new(100 * world, 0)],
                ..SimulationConfig::default()
            };
            let mut sim = Simulation::new(config, workers);
            let mover = |x_world| UnitSpawn {
                team: Team(0),
                position: SimPoint::new(x_world * world, 0),
                health: 100,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    cooldown_ticks: 30,
                },
                movement: MovementProfile {
                    speed_per_tick: world / 2,
                },
            };
            let ids = [
                sim.spawn_unit(mover(200)),
                sim.spawn_unit(mover(208)),
                sim.spawn_unit(mover(216)),
            ];
            let tick = sim.step();
            let positions = ids
                .into_iter()
                .map(|id| sim.unit(id).unwrap().position)
                .collect();
            (sim.checksum(), positions, tick)
        }

        let expected = run(1);
        let parallel = run(8);
        assert_eq!(parallel.0, expected.0);
        assert_eq!(parallel.1, expected.1);
        assert_eq!(parallel.2.movement_intents, expected.2.movement_intents);
        assert_eq!(parallel.2.movement_blocked, expected.2.movement_blocked);
        assert_eq!(expected.2.movement_intents, 3);
        assert_eq!(expected.2.movement_blocked, 0);
        for (index, position) in expected.1.iter().enumerate() {
            let start = (200 + i32::try_from(index).unwrap() * 8) * SUBUNITS_PER_WORLD_UNIT;
            assert!(
                position.x > start,
                "packed convoy unit {index} did not advance into vacated space"
            );
        }
    }

    #[test]
    fn objective_mover_flows_around_stationary_allied_clump() {
        fn run(workers: usize) -> (u64, SimPoint, usize, usize) {
            let world = SUBUNITS_PER_WORLD_UNIT;
            let config = SimulationConfig {
                spatial_cell_size: 40 * world,
                navigation_cell_size: 10 * world,
                navigation_min: NavCell::new(0, -40),
                navigation_max: NavCell::new(100, 40),
                target_pursuit_extra_range: 30 * world,
                unit_separation_distance: 8 * world,
                max_separation_per_tick: world,
                team_objective: [SimPoint::new(900 * world, 0), SimPoint::new(100 * world, 0)],
                ..SimulationConfig::default()
            };
            let mut sim = Simulation::new(config, workers);
            for y in [-16, -8, 0, 8, 16] {
                let mut blocker = passive_unit(0, 237 * world);
                blocker.position = SimPoint::new(237 * world, y * world);
                sim.spawn_unit(blocker);
            }
            let rear = sim.spawn_unit(UnitSpawn {
                team: Team(0),
                position: SimPoint::new(229 * world, 0),
                health: 10_000,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    cooldown_ticks: 30,
                },
                movement: MovementProfile {
                    speed_per_tick: 40 * world / 30,
                },
            });

            let mut previous = sim.unit(rear).unwrap().position;
            let mut previous_dy = 0_i32;
            let mut reversals = 0usize;
            let mut blocked_ticks = 0usize;
            for _ in 0..120 {
                let tick = sim.step();
                blocked_ticks += usize::from(tick.movement_blocked > 0);
                let current = sim.unit(rear).unwrap().position;
                let dy = current.y - previous.y;
                if dy != 0 && previous_dy != 0 && dy.signum() != previous_dy.signum() {
                    reversals += 1;
                }
                if dy != 0 {
                    previous_dy = dy;
                }
                previous = current;
                if current.x > 260 * world {
                    break;
                }
            }
            (
                sim.checksum(),
                sim.unit(rear).unwrap().position,
                reversals,
                blocked_ticks,
            )
        }

        let expected = run(1);
        assert_eq!(
            run(8),
            expected,
            "worker count changed objective crowd flow"
        );
        assert!(
            expected.1.x > 260 * SUBUNITS_PER_WORLD_UNIT,
            "objective mover never flowed past stationary allies: {:?}",
            expected.1
        );
        assert!(
            expected.2 <= 4,
            "objective mover repeatedly oscillated between bypass directions {} times",
            expected.2
        );
        assert!(
            expected.3 <= 8,
            "objective mover remained hard-blocked for {} ticks despite lateral space",
            expected.3
        );
    }

    #[test]
    fn trailing_client_scale_melee_flows_around_engaged_units_without_jitter() {
        fn run(workers: usize) -> (u64, Option<usize>, usize, i32) {
            let world = SUBUNITS_PER_WORLD_UNIT;
            let mut config = SimulationConfig {
                spatial_cell_size: 40 * world,
                navigation_cell_size: 10 * world,
                navigation_min: NavCell::new(0, -40),
                navigation_max: NavCell::new(100, 40),
                target_pursuit_extra_range: 30 * world,
                unit_separation_distance: 8 * world,
                max_separation_per_tick: world,
                team_objective: [SimPoint::new(900 * world, 0), SimPoint::new(100 * world, 0)],
                ..SimulationConfig::default()
            };
            config.static_blockers.clear();
            let mut sim = Simulation::new(config, workers);
            let melee = |x_world: i32, y_world: i32| UnitSpawn {
                team: Team(0),
                position: SimPoint::new(x_world * world, y_world * world),
                health: 10_000,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 0,
                    range: 14 * world,
                    acquisition_range: 80 * world,
                    cooldown_ticks: 30,
                },
                movement: MovementProfile {
                    speed_per_tick: 40 * world / 30,
                },
            };
            for (x, y) in [(237, 0), (241, -10), (241, 10)] {
                sim.spawn_unit(melee(x, y));
            }
            let rear = sim.spawn_unit(melee(229, 0));
            let target = sim.spawn_unit(UnitSpawn {
                team: Team(1),
                position: SimPoint::new(250 * world, 0),
                health: 10_000,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 0,
                    range: 14 * world,
                    acquisition_range: 80 * world,
                    cooldown_ticks: 30,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            });

            let mut previous = sim.unit(rear).unwrap().position;
            let mut previous_dy = 0_i32;
            let mut reversals = 0usize;
            let mut max_lateral = 0_i32;
            let mut attack_tick = None;
            for tick in 0..120 {
                sim.step();
                let view = sim.unit(rear).unwrap();
                let dy = view.position.y - previous.y;
                if dy != 0 && previous_dy != 0 && dy.signum() != previous_dy.signum() {
                    reversals += 1;
                }
                if dy != 0 {
                    previous_dy = dy;
                }
                previous = view.position;
                max_lateral = max_lateral.max(view.position.y.abs());
                if sim
                    .attacks_last_tick()
                    .iter()
                    .any(|event| event.source == rear && event.target == target)
                {
                    attack_tick = Some(tick);
                    break;
                }
            }
            (sim.checksum(), attack_tick, reversals, max_lateral)
        }

        let expected = run(1);
        assert_eq!(run(8), expected, "worker count changed melee crowd flow");
        let (_, attack_tick, reversals, max_lateral) = expected;
        let world = SUBUNITS_PER_WORLD_UNIT;
        assert!(
            attack_tick.is_some_and(|tick| tick < 60),
            "rear melee unit never flowed into attack range: {attack_tick:?}"
        );
        assert!(
            reversals <= 2,
            "rear melee unit jittered between flow directions {reversals} times"
        );
        assert!(
            max_lateral >= 4 * world,
            "rear melee unit lateral bypass was too small: {max_lateral}"
        );
    }

    #[test]
    fn melee_group_flows_around_single_unit_attack_envelope() {
        fn run(workers: usize) -> (u64, usize, u8) {
            let world = SUBUNITS_PER_WORLD_UNIT;
            let mut config = SimulationConfig {
                spatial_cell_size: 40 * world,
                navigation_cell_size: 10 * world,
                navigation_min: NavCell::new(0, -40),
                navigation_max: NavCell::new(100, 40),
                target_pursuit_extra_range: 30 * world,
                unit_separation_distance: 8 * world,
                max_separation_per_tick: world,
                team_objective: [SimPoint::new(900 * world, 0), SimPoint::new(100 * world, 0)],
                ..SimulationConfig::default()
            };
            config.static_blockers.clear();
            let mut sim = Simulation::new(config, workers);
            let target_position = SimPoint::new(250 * world, 0);
            let target = sim.spawn_unit(UnitSpawn {
                team: Team(0),
                position: target_position,
                health: 1_000_000,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    cooldown_ticks: 30,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            });
            let attack = AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 14 * world,
                acquisition_range: 100 * world,
                cooldown_ticks: 10,
            };
            for index in 0..12 {
                let row = index % 6;
                let column = index / 6;
                sim.spawn_unit(UnitSpawn {
                    team: Team(1),
                    position: SimPoint::new((300 + column * 8) * world, (-20 + row * 8) * world),
                    health: 10_000,
                    attack,
                    movement: MovementProfile {
                        speed_per_tick: 40 * world / 30,
                    },
                });
            }

            let mut attackers = Vec::new();
            let mut side_mask = 0_u8;
            for _ in 0..240 {
                sim.step();
                for event in sim
                    .attacks_last_tick()
                    .iter()
                    .filter(|event| event.target == target)
                {
                    if !attackers.contains(&event.source) {
                        attackers.push(event.source);
                    }
                    let dx = event.source_position.x - target_position.x;
                    let dy = event.source_position.y - target_position.y;
                    if dx < 0 {
                        side_mask |= 1;
                    }
                    if dx > 0 {
                        side_mask |= 2;
                    }
                    if dy < 0 {
                        side_mask |= 4;
                    }
                    if dy > 0 {
                        side_mask |= 8;
                    }
                }
            }
            (sim.checksum(), attackers.len(), side_mask)
        }

        let expected = run(1);
        assert_eq!(run(8), expected, "worker count changed unit surround flow");
        assert!(
            expected.1 >= 9,
            "only {} of 12 melee units reached the unit attack envelope",
            expected.1
        );
        assert!(
            expected.2.count_ones() >= 3,
            "melee group did not flow around the unit target: {:04b}",
            expected.2
        );
    }

    #[test]
    fn client_scale_melee_group_flows_around_large_building_attack_envelope() {
        fn run(workers: usize) -> (u64, usize, u8) {
            let world = SUBUNITS_PER_WORLD_UNIT;
            let mut config = SimulationConfig {
                spatial_cell_size: 40 * world,
                navigation_cell_size: 10 * world,
                navigation_min: NavCell::new(0, 0),
                navigation_max: NavCell::new(100, 74),
                target_pursuit_extra_range: 30 * world,
                unit_separation_distance: 8 * world,
                max_separation_per_tick: world,
                team_objective: [
                    SimPoint::new(900 * world, 370 * world),
                    SimPoint::new(100 * world, 370 * world),
                ],
                ..SimulationConfig::default()
            };
            config.static_blockers.clear();
            let mut sim = Simulation::new(config, workers);
            let footprint = BuildingFootprint::new(30, 34, 7, 7);
            let building = sim.spawn_building(BuildingSpawn {
                team: Team(0),
                footprint,
                health: 1_000_000,
                production: None,
                attack: None,
                spellcasting: None,
            });
            let attack = AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 14 * world,
                acquisition_range: 180 * world,
                cooldown_ticks: 10,
            };
            for index in 0..24 {
                let row = index % 8;
                let column = index / 8;
                sim.spawn_unit(UnitSpawn {
                    team: Team(1),
                    position: SimPoint::new((430 + column * 8) * world, (342 + row * 8) * world),
                    health: 10_000,
                    attack,
                    movement: MovementProfile {
                        speed_per_tick: 40 * world / 30,
                    },
                });
            }

            let min_x = footprint.min_x * 10 * world;
            let max_x = (footprint.max_x() + 1) * 10 * world;
            let min_y = footprint.min_y * 10 * world;
            let max_y = (footprint.max_y() + 1) * 10 * world;
            let mut attackers = Vec::new();
            let mut side_mask = 0_u8;
            for _ in 0..300 {
                sim.step();
                for event in sim
                    .attacks_last_tick()
                    .iter()
                    .filter(|event| event.target == building)
                {
                    if !attackers.contains(&event.source) {
                        attackers.push(event.source);
                    }
                    let position = event.source_position;
                    if position.x < min_x {
                        side_mask |= 1;
                    }
                    if position.x > max_x {
                        side_mask |= 2;
                    }
                    if position.y < min_y {
                        side_mask |= 4;
                    }
                    if position.y > max_y {
                        side_mask |= 8;
                    }
                }
            }
            (sim.checksum(), attackers.len(), side_mask)
        }

        let expected = run(1);
        assert_eq!(
            run(8),
            expected,
            "worker count changed building surround flow"
        );
        assert!(
            expected.1 >= 20,
            "only {} of 24 melee units reached the building attack envelope",
            expected.1
        );
        assert!(
            expected.2.count_ones() >= 3,
            "melee group reached too few building faces: {:04b}",
            expected.2
        );
    }

    #[test]
    fn targetless_topology_detour_does_not_retarget_rows_and_oscillate() {
        fn run(workers: usize) -> (u64, SimPoint, usize) {
            let world = SUBUNITS_PER_WORLD_UNIT;
            let config = SimulationConfig {
                spatial_cell_size: 64 * world,
                navigation_cell_size: 32 * world,
                navigation_min: NavCell::new(0, -20),
                navigation_max: NavCell::new(40, 20),
                target_pursuit_extra_range: 30 * world,
                unit_separation_distance: 8 * world,
                max_separation_per_tick: world,
                team_objective: [SimPoint::new(1_000 * world, 0), SimPoint::new(0, 0)],
                static_blockers: vec![
                    BuildingFootprint::new(7, -6, 4, 4),
                    BuildingFootprint::new(12, -3, 4, 4),
                ],
                ..SimulationConfig::default()
            };
            let mut sim = Simulation::new(config, workers);
            let definition = CastleFightUnitKind::Footman.definition();
            let mover = sim.spawn_unit_with_properties(
                UnitSpawn::from_template(
                    Team(0),
                    SimPoint::new(176 * world, -112 * world),
                    definition.template(),
                ),
                definition.gameplay_properties(),
            );

            let mut previous = sim.unit(mover).unwrap().position;
            let mut previous_dx = 0_i32;
            let mut previous_dy = 0_i32;
            let mut heading_reversals = 0usize;
            for _ in 0..60 {
                sim.step();
                let current = sim.unit(mover).unwrap().position;
                let dx = current.x - previous.x;
                let dy = current.y - previous.y;
                if dx != 0 && previous_dx != 0 && dx.signum() != previous_dx.signum() {
                    heading_reversals += 1;
                }
                if dy != 0 && previous_dy != 0 && dy.signum() != previous_dy.signum() {
                    heading_reversals += 1;
                }
                if dx != 0 {
                    previous_dx = dx;
                }
                if dy != 0 {
                    previous_dy = dy;
                }
                previous = current;
            }
            (
                sim.checksum(),
                sim.unit(mover).unwrap().position,
                heading_reversals,
            )
        }

        let expected = run(1);
        assert_eq!(run(8), expected, "worker count changed objective detour");
        assert!(
            expected.1.x > 560 * SUBUNITS_PER_WORLD_UNIT,
            "footman failed to make steady objective progress: {:?}",
            expected.1
        );
        assert!(
            expected.2 <= 2,
            "footman repeatedly reversed heading during a targetless detour: {} reversals",
            expected.2
        );
    }

    #[test]
    fn worker_count_does_not_change_battle_checksum() {
        let mut expected = None;
        for workers in [1, 2, 4] {
            let mut sim = Simulation::new(SimulationConfig::default(), workers);
            populate_lane_test_units(&mut sim, 512);
            for _ in 0..200 {
                sim.step();
            }

            match expected {
                Some(checksum) => assert_eq!(sim.checksum(), checksum, "workers={workers}"),
                None => expected = Some(sim.checksum()),
            }
        }
    }
}
