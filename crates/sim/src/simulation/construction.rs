use super::*;

impl Simulation {
    pub fn spawn_building(&mut self, building: BuildingSpawn) -> SimId {
        self.spawn_building_with_properties(building, BuildingGameplayProperties::default())
    }

    pub fn spawn_building_with_properties(
        &mut self,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> SimId {
        let owner = self.inferred_owner_for_team(building.team);
        self.spawn_building_for_player_with_properties(owner, building, properties)
    }

    pub fn spawn_building_for_player_with_properties(
        &mut self,
        owner: PlayerId,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> SimId {
        self.assert_player_team(owner, building.team);
        self.try_spawn_building_internal(Some(owner), building, properties)
            .expect("invalid authored building placement")
    }

    /// Debug-only population helper that preserves authoritative building state while deliberately
    /// bypassing ordinary pathing/build-blocker placement checks. The caller must still keep the
    /// footprint inside the team's build region; this is used for deterministic stress rosters
    /// that can be denser than legal player construction around the authored castle doodads.
    pub fn debug_spawn_building_for_player_with_properties(
        &mut self,
        owner: PlayerId,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> SimId {
        self.assert_player_team(owner, building.team);
        self.validate_building_definition(building, properties);
        assert!(
            self.footprint_inside_team_build_region(building.team, building.footprint),
            "debug building footprint must remain inside the owning team's build region"
        );
        let (id, entity) = self.spawn_building_shell(Some(owner), building, properties);
        self.activate_building_entity(entity, building, properties);
        self.topology_dirty = true;
        id
    }

    pub fn spawn_shared_building_with_properties(
        &mut self,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> SimId {
        self.try_spawn_building_internal(None, building, properties)
            .expect("invalid authored shared building placement")
    }

    pub fn spawn_building_with_attack_targets(
        &mut self,
        building: BuildingSpawn,
        attack_targets: AttackTargetMask,
    ) -> SimId {
        self.spawn_building_with_properties(
            building,
            BuildingGameplayProperties {
                attack_targets,
                ..BuildingGameplayProperties::default()
            },
        )
    }

    pub fn spawn_building_with_production_corpse(
        &mut self,
        building: BuildingSpawn,
        corpse: CorpseProfile,
    ) -> SimId {
        self.spawn_building_with_production_properties(
            building,
            UnitGameplayProperties {
                corpse: Some(corpse),
                ..UnitGameplayProperties::default()
            },
        )
    }

    pub fn try_spawn_building(
        &mut self,
        building: BuildingSpawn,
    ) -> Result<SimId, BuildingPlacementError> {
        self.try_spawn_building_with_properties(building, BuildingGameplayProperties::default())
    }

    pub fn try_spawn_building_with_properties(
        &mut self,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> Result<SimId, BuildingPlacementError> {
        let owner = self.inferred_owner_for_team(building.team);
        self.try_spawn_building_internal(Some(owner), building, properties)
    }

    pub fn try_spawn_building_with_production_corpse(
        &mut self,
        building: BuildingSpawn,
        corpse: CorpseProfile,
    ) -> Result<SimId, BuildingPlacementError> {
        self.try_spawn_building_with_production_properties(
            building,
            UnitGameplayProperties {
                corpse: Some(corpse),
                ..UnitGameplayProperties::default()
            },
        )
    }

    pub fn spawn_building_with_production_collision_radius(
        &mut self,
        building: BuildingSpawn,
        collision_radius: CollisionRadius,
    ) -> SimId {
        self.spawn_building_with_production_properties(
            building,
            UnitGameplayProperties {
                collision_radius: Some(collision_radius),
                ..UnitGameplayProperties::default()
            },
        )
    }

    pub fn spawn_building_with_production_properties(
        &mut self,
        building: BuildingSpawn,
        properties: UnitGameplayProperties,
    ) -> SimId {
        self.try_spawn_building_with_production_properties(building, properties)
            .expect("invalid authored building placement")
    }

    pub fn try_spawn_building_with_production_properties(
        &mut self,
        building: BuildingSpawn,
        properties: UnitGameplayProperties,
    ) -> Result<SimId, BuildingPlacementError> {
        assert!(
            building.production.is_some(),
            "production unit properties require a production building"
        );
        if let Some(corpse) = properties.corpse {
            validate_corpse_profile(corpse);
        }
        if let Some(collision_radius) = properties.collision_radius {
            validate_collision_radius(collision_radius);
        }
        let owner = self.inferred_owner_for_team(building.team);
        self.try_spawn_building_internal(
            Some(owner),
            building,
            BuildingGameplayProperties {
                production_unit: properties,
                ..BuildingGameplayProperties::default()
            },
        )
    }

    pub fn spawn_building_with_production_spellcasting(
        &mut self,
        building: BuildingSpawn,
        spellcasting: SpellcastingProfile,
    ) -> SimId {
        self.try_spawn_building_with_production_spellcasting(building, spellcasting)
            .expect("invalid authored building placement")
    }

    pub fn try_spawn_building_with_production_spellcasting(
        &mut self,
        building: BuildingSpawn,
        spellcasting: SpellcastingProfile,
    ) -> Result<SimId, BuildingPlacementError> {
        assert!(
            building.production.is_some(),
            "production spellcasting profile requires a production building"
        );
        validate_spellcasting_profile(spellcasting);
        let owner = self.inferred_owner_for_team(building.team);
        self.try_spawn_building_internal(
            Some(owner),
            building,
            BuildingGameplayProperties {
                production_spellcasting: Some(spellcasting),
                ..BuildingGameplayProperties::default()
            },
        )
    }

    pub(super) fn try_spawn_building_internal(
        &mut self,
        owner: Option<PlayerId>,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> Result<SimId, BuildingPlacementError> {
        self.validate_building_definition(building, properties);
        self.validate_building_placement(building.team, building.footprint)?;
        let (id, entity) = self.spawn_building_shell(owner, building, properties);
        self.activate_building_entity(entity, building, properties);
        self.topology_dirty = true;
        Ok(id)
    }

    pub(super) fn try_start_building_construction(
        &mut self,
        owner: PlayerId,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> Result<SimId, BuildingPlacementError> {
        let duration_ticks = properties
            .construction_time_ticks
            .expect("construction start requires authored construction duration");
        assert!(
            duration_ticks > 0,
            "building construction time must be positive"
        );
        self.validate_building_definition(building, properties);
        self.validate_building_placement(building.team, building.footprint)?;

        let complete_tick = self
            .next_tick
            .checked_add(u64::from(duration_ticks))
            .expect("building construction tick overflow");
        let (id, entity) = self.spawn_building_shell(Some(owner), building, properties);
        self.world.entity_mut(entity).insert(BuildingConstruction {
            started_tick: self.next_tick,
            complete_tick,
            building,
            properties,
            upgrade_from: None,
        });
        self.topology_dirty = true;
        Ok(id)
    }

    fn validate_building_definition(
        &self,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) {
        assert!(building.health > 0);
        assert!(building.team.0 < 2, "verification slice supports two teams");
        assert!(building.footprint.width > 0 && building.footprint.height > 0);
        if let Some(construction_time_ticks) = properties.construction_time_ticks {
            assert!(
                construction_time_ticks > 0,
                "building construction time must be positive"
            );
        }
        if let Some(production) = building.production {
            assert!(production.interval_ticks > 0);
            validate_unit_template(production.unit);
            if let Some(corpse) = properties.production_unit.corpse {
                validate_corpse_profile(corpse);
            }
            if let Some(collision_radius) = properties.production_unit.collision_radius {
                validate_collision_radius(collision_radius);
            }
            if properties.production_unit.mechanical {
                assert!(
                    properties
                        .production_unit
                        .repair_time_ticks
                        .is_some_and(|ticks| ticks > 0),
                    "mechanical production units require positive repair-time metadata"
                );
            }
            if let Some(spellcasting) = properties.production_spellcasting {
                validate_spellcasting_profile(spellcasting);
            }
            super::automatic_abilities::validate_additional_automatic_definitions(
                properties.production_spellcasting,
                properties.production_additional_abilities,
            )
            .expect("production ability definitions require a primary and distinct IDs");
        } else {
            assert!(
                properties.production_additional_abilities.is_none(),
                "additional production ability definitions require a production profile"
            );
        }
        if let Some(attack) = building.attack {
            validate_attack_profile(attack);
        }
        if let Some(spellcasting) = building.spellcasting {
            validate_spellcasting_profile(spellcasting);
        }
        if let Some(repair_time_ticks) = properties.repair_time_ticks {
            assert!(
                repair_time_ticks > 0,
                "building repair time must be positive"
            );
        }
    }

    fn spawn_building_shell(
        &mut self,
        owner: Option<PlayerId>,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> (SimId, Entity) {
        let id = self.allocate_id();
        let mut entity = self.world.spawn((
            id,
            building.team,
            building.footprint,
            Health {
                current: building.health,
                max: building.health,
            },
            properties.damage_type,
            properties.armor,
        ));
        if properties.classifications != UnitClassifications::default() {
            entity.insert(properties.classifications);
        }
        if let Some(owner) = owner {
            entity.insert(Owner(owner));
        }
        if let Some(content) = properties.content {
            entity.insert(content);
        }
        let entity_id = entity.id();
        (id, entity_id)
    }

    fn activate_building_entity(
        &mut self,
        entity: Entity,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) {
        let tower_kind = properties.content.and_then(|content| {
            crate::CastleFightTowerKind::from_rawcode_for_version(
                content.rawcode,
                content.map_version,
            )
            .expect("registered building activation version")
        });
        if tower_kind == Some(crate::CastleFightTowerKind::Gjallarhorn) {
            let count = &mut self.gjallarhorn_constructed_count[usize::from(building.team.0)];
            *count = count
                .checked_add(1)
                .expect("Gjallarhorn construction counter overflow");
        }

        let mut entity = self.world.entity_mut(entity);
        if properties.classifications == UnitClassifications::default() {
            entity.remove::<UnitClassifications>();
        } else {
            entity.insert(properties.classifications);
        }
        if let Some(economy) = properties.economy {
            entity.insert(economy);
        }
        if let Some(repair_time_ticks) = properties.repair_time_ticks {
            entity.insert(RepairTimeTicks(repair_time_ticks));
        }
        if let Some(production) = building.production {
            let next_spawn_tick = self
                .next_tick
                .checked_add(u64::from(production.initial_delay_ticks))
                .expect("initial production tick overflow");
            entity.insert((
                production,
                ProductionState {
                    next_spawn_tick,
                    queued: 2,
                },
                ProductionMovementClass(properties.production_unit.movement_class),
                ProductionActionTiming(properties.production_unit.action_timing),
                ProductionUnitRepairMetadata {
                    mechanical: properties.production_unit.mechanical,
                    build_time_ticks: properties.production_unit.build_time_ticks,
                    repair_time_ticks: properties.production_unit.repair_time_ticks,
                },
                ProductionAttackTargets(properties.production_unit.attack_targets),
                ProductionHealthRegeneration(
                    properties.production_unit.health_regen_per_second_per_10k,
                ),
            ));
            if let Some(content) = properties.production_unit.content {
                entity.insert(ProductionContentIdentity(content));
            }
            if let Some(corpse) = properties.production_unit.corpse {
                entity.insert(ProductionCorpseProfile(corpse));
            }
            if let Some(collision_radius) = properties.production_unit.collision_radius {
                entity.insert(ProductionCollisionRadius(collision_radius));
            }
            if let Some(secondary_attack) = properties.production_unit.secondary_attack {
                entity.insert(ProductionSecondaryAttack(secondary_attack));
            }
            entity.insert((
                ProductionDamageType(properties.production_unit.damage_type),
                ProductionArmorProfile(properties.production_unit.armor),
                ProductionPassiveEffects(properties.production_unit.passive_effects),
            ));
            if let Some(spellcasting) = properties.production_spellcasting {
                entity.insert(ProductionSpellcastingProfile(spellcasting));
            }
            if properties.production_unit.classifications != UnitClassifications::default() {
                entity.insert(ProductionUnitClassifications(
                    properties.production_unit.classifications,
                ));
            }
            if let Some(definitions) = properties.production_additional_abilities {
                entity.insert(ProductionAdditionalAutomaticAbilities(definitions));
            }
        }
        if let Some(attack) = building.attack {
            entity.insert((
                attack,
                properties.attack_targets,
                AttackCooldown::default(),
                TargetState::default(),
                SpawnTick(self.next_tick),
            ));
        }
        if let Some(spellcasting) = building.spellcasting {
            entity.insert((
                spellcasting,
                ManaState {
                    current: spellcasting.mana.starting,
                    regen_remainder_per_10k: 0,
                },
                AutomaticAbilityState {
                    ready_tick: self.next_tick,
                    cast_sequence: 0,
                    autocast_enabled: true,
                    manual_cast_requested: false,
                },
            ));
        }
        if building.attack.is_some() || building.spellcasting.is_some() {
            entity.insert(StatusState::default());
        }
        if tower_kind == Some(crate::CastleFightTowerKind::GoldenShrineOfJustice) {
            let version = properties
                .content
                .expect("retained shrine identity")
                .map_version;
            let shrine = crate::golden_shrine_definition_for_version(version)
                .expect("registered shrine building baseline");
            entity.insert(HealthRegeneration {
                per_second_per_10k: shrine.building_health_regen_per_second_per_10k,
                remainder_per_10k_hz: 0,
            });
        }
        let entity_id = entity.id();
        self.start_native_carrier(entity_id);
    }

    pub(super) fn deactivate_building_entity(&mut self, entity: Entity) {
        let id = *self.world.get::<SimId>(entity).expect("building id");
        self.stop_native_carrier(id);
        let mut entity = self.world.entity_mut(entity);
        entity.remove::<BuildingEconomyProfile>();
        entity.remove::<HealthRegeneration>();
        entity.remove::<RepairTimeTicks>();
        entity.remove::<ProductionProfile>();
        entity.remove::<ProductionState>();
        entity.remove::<ProductionContentIdentity>();
        entity.remove::<ProductionCorpseProfile>();
        entity.remove::<ProductionCollisionRadius>();
        entity.remove::<ProductionMovementClass>();
        entity.remove::<ProductionUnitRepairMetadata>();
        entity.remove::<ProductionAttackTargets>();
        entity.remove::<ProductionSecondaryAttack>();
        entity.remove::<ProductionActionTiming>();
        entity.remove::<ProductionHealthRegeneration>();
        entity.remove::<ProductionDamageType>();
        entity.remove::<ProductionArmorProfile>();
        entity.remove::<ProductionPassiveEffects>();
        entity.remove::<ProductionSpellcastingProfile>();
        entity.remove::<ProductionAdditionalAutomaticAbilities>();
        entity.remove::<ProductionUnitClassifications>();
        entity.remove::<AttackProfile>();
        entity.remove::<AttackTargetMask>();
        entity.remove::<AttackCooldown>();
        entity.remove::<TargetState>();
        entity.remove::<SpawnTick>();
        entity.remove::<SpellcastingProfile>();
        entity.remove::<ManaState>();
        entity.remove::<AutomaticAbilityState>();
        entity.remove::<AdditionalAutomaticAbilities>();
        entity.remove::<StatusState>();
    }

    fn restore_building_runtime_state(&mut self, entity: Entity, runtime: BuildingRuntimeState) {
        let mut entity = self.world.entity_mut(entity);
        if runtime.classifications == UnitClassifications::default() {
            entity.remove::<UnitClassifications>();
        } else {
            entity.insert(runtime.classifications);
        }
        match runtime.production {
            Some(state) => {
                entity.insert(state);
            }
            None => {
                entity.remove::<ProductionState>();
            }
        }
        match runtime.attack_cooldown {
            Some(state) => {
                entity.insert(state);
            }
            None => {
                entity.remove::<AttackCooldown>();
            }
        }
        match runtime.target {
            Some(state) => {
                entity.insert(state);
            }
            None => {
                entity.remove::<TargetState>();
            }
        }
        match runtime.spawn_tick {
            Some(state) => {
                entity.insert(state);
            }
            None => {
                entity.remove::<SpawnTick>();
            }
        }
        match runtime.mana {
            Some(state) => {
                entity.insert(state);
            }
            None => {
                entity.remove::<ManaState>();
            }
        }
        match runtime.ability_state {
            Some(state) => {
                entity.insert(state);
            }
            None => {
                entity.remove::<AutomaticAbilityState>();
            }
        }
        match runtime.additional_abilities {
            Some(abilities) => {
                entity.insert(abilities);
            }
            None => {
                entity.remove::<AdditionalAutomaticAbilities>();
            }
        }
        match runtime.status {
            Some(state) => {
                entity.insert(state);
            }
            None => {
                entity.remove::<StatusState>();
            }
        }
    }

    pub(crate) fn start_building_upgrade_as(
        &mut self,
        controller: PlayerId,
        source_id: SimId,
        source_building: BuildingSpawn,
        source_properties: BuildingGameplayProperties,
        target_building: BuildingSpawn,
        target_properties: BuildingGameplayProperties,
    ) -> Result<(), BuildingUpgradeError> {
        if !self.can_player_control_building(controller, source_id) {
            return Err(BuildingUpgradeError::NotOwner);
        }
        self.start_building_upgrade(
            source_id,
            source_building,
            source_properties,
            target_building,
            target_properties,
        )
    }

    pub(crate) fn start_building_upgrade(
        &mut self,
        source_id: SimId,
        source_building: BuildingSpawn,
        source_properties: BuildingGameplayProperties,
        target_building: BuildingSpawn,
        target_properties: BuildingGameplayProperties,
    ) -> Result<(), BuildingUpgradeError> {
        self.validate_building_definition(source_building, source_properties);
        self.validate_building_definition(target_building, target_properties);
        let duration_ticks = target_properties
            .construction_time_ticks
            .expect("Castle Fight building upgrades require authored construction time");
        let target_economy = target_properties
            .economy
            .ok_or(BuildingUpgradeError::MissingEconomyProfile)?;

        let Some((
            entity,
            actual_owner,
            actual_team,
            actual_footprint,
            actual_health,
            actual_content,
            runtime,
        )) = self.world.iter_entities().find_map(|entity| {
            (entity.get::<SimId>().copied() == Some(source_id)
                && entity.get::<BuildingFootprint>().is_some())
            .then(|| {
                Some((
                    entity.id(),
                    entity.get::<Owner>()?.0,
                    *entity.get::<Team>()?,
                    *entity.get::<BuildingFootprint>()?,
                    *entity.get::<Health>()?,
                    entity.get::<ContentIdentity>().copied(),
                    BuildingRuntimeState {
                        classifications: entity
                            .get::<UnitClassifications>()
                            .copied()
                            .unwrap_or_default(),
                        production: entity.get::<ProductionState>().copied(),
                        attack_cooldown: entity.get::<AttackCooldown>().copied(),
                        target: entity.get::<TargetState>().copied(),
                        spawn_tick: entity.get::<SpawnTick>().copied(),
                        mana: entity.get::<ManaState>().copied(),
                        ability_state: entity.get::<AutomaticAbilityState>().copied(),
                        additional_abilities: entity.get::<AdditionalAutomaticAbilities>().copied(),
                        status: entity.get::<StatusState>().copied(),
                    },
                ))
            })?
        })
        else {
            return Err(BuildingUpgradeError::SourceNotFound);
        };
        if self
            .world
            .entity(entity)
            .get::<BuildingConstruction>()
            .is_some()
        {
            return Err(BuildingUpgradeError::SourceUnderConstruction);
        }
        if source_building.team != actual_team || target_building.team != actual_team {
            return Err(BuildingUpgradeError::TeamMismatch);
        }
        if source_building.footprint != actual_footprint
            || target_building.footprint != actual_footprint
        {
            return Err(BuildingUpgradeError::FootprintMismatch);
        }
        if actual_content != source_properties.content {
            return Err(BuildingUpgradeError::SourceDefinitionMismatch);
        }
        if runtime.production.is_some_and(|state| state.queued != 0) {
            return Err(BuildingUpgradeError::ProductionQueueNotEmpty);
        }

        let resources = &mut self
            .player_state_mut(actual_owner)
            .expect("building owner must exist")
            .resources;
        if resources.gold < target_economy.gold_cost {
            return Err(BuildingUpgradeError::Resources(
                ResourcePurchaseError::InsufficientGold {
                    required: target_economy.gold_cost,
                    available: resources.gold,
                },
            ));
        }
        if resources.lumber < target_economy.lumber_cost {
            return Err(BuildingUpgradeError::Resources(
                ResourcePurchaseError::InsufficientLumber {
                    required: target_economy.lumber_cost,
                    available: resources.lumber,
                },
            ));
        }
        let source_points = source_properties
            .economy
            .map_or(0, |economy| economy.legendary_points_cost);
        let available_points = resources
            .legendary_points_available()
            .saturating_add(source_points);
        if available_points < target_economy.legendary_points_cost {
            return Err(BuildingUpgradeError::Resources(
                ResourcePurchaseError::InsufficientLegendaryPoints {
                    available: available_points,
                    required: target_economy.legendary_points_cost,
                },
            ));
        }
        resources.gold -= target_economy.gold_cost;
        resources.lumber -= target_economy.lumber_cost;
        resources.legendary_points_used =
            resources.legendary_points_used - source_points + target_economy.legendary_points_cost;

        let complete_tick = self
            .next_tick
            .checked_add(u64::from(duration_ticks))
            .expect("building upgrade completion tick overflow");
        let upgrade_from = BuildingUpgradeSource {
            building: source_building,
            properties: source_properties,
            health: actual_health,
            runtime,
        };

        self.deactivate_building_entity(entity);
        let target_health = scale_building_health(
            actual_health.current,
            actual_health.max,
            target_building.health,
        );
        let mut entity_mut = self.world.entity_mut(entity);
        *entity_mut
            .get_mut::<Health>()
            .expect("upgrade source building missing health") = Health {
            current: target_health,
            max: target_building.health,
        };
        *entity_mut
            .get_mut::<DamageType>()
            .expect("upgrade source building missing damage type") = target_properties.damage_type;
        *entity_mut
            .get_mut::<ArmorProfile>()
            .expect("upgrade source building missing armor profile") = target_properties.armor;
        if target_properties.classifications == UnitClassifications::default() {
            entity_mut.remove::<UnitClassifications>();
        } else {
            entity_mut.insert(target_properties.classifications);
        }
        match target_properties.content {
            Some(content) => {
                entity_mut.insert(content);
            }
            None => {
                entity_mut.remove::<ContentIdentity>();
            }
        }
        // An upgrading Castle Fight production building still represents its precursor's
        // completed economic investment until the upgrade completes.
        if let Some(economy) = source_properties.economy {
            entity_mut.insert(economy);
        }
        entity_mut.insert(BuildingConstruction {
            started_tick: self.next_tick,
            complete_tick,
            building: target_building,
            properties: target_properties,
            upgrade_from: Some(upgrade_from),
        });
        Ok(())
    }

    #[must_use]
    pub fn remove_building(&mut self, id: SimId) -> bool {
        let entity = self.world.iter_entities().find_map(|entity| {
            (entity.get::<SimId>().copied() == Some(id)
                && entity.get::<BuildingFootprint>().is_some())
            .then_some(entity.id())
        });
        let Some(entity) = entity else {
            return false;
        };
        self.release_building_legendary_points(entity);
        self.stop_native_carrier(id);
        self.world.despawn(entity);
        self.topology_dirty = true;
        true
    }

    pub(super) fn release_building_legendary_points(&mut self, entity: Entity) {
        let building = self.world.entity(entity);
        let owner = building.get::<Owner>().map(|owner| owner.0);
        let points = building
            .get::<BuildingConstruction>()
            .and_then(|construction| construction.properties.economy)
            .or_else(|| building.get::<BuildingEconomyProfile>().copied())
            .map_or(0, |economy| economy.legendary_points_cost);
        if let Some(owner) = owner {
            self.player_state_mut(owner)
                .expect("building owner must exist")
                .resources
                .legendary_points_used -= points;
        }
    }

    #[cfg(test)]
    pub(crate) fn cancel_building_construction(
        &mut self,
        team: Team,
        id: SimId,
    ) -> Result<BuildingConstructionCancelOutcome, BuildingConstructionCancelError> {
        let player = self
            .unique_player_for_team(team)
            .ok_or(BuildingConstructionCancelError::NotOwner)?;
        self.cancel_building_construction_for_player(player, id)
    }

    pub(crate) fn cancel_building_construction_for_player(
        &mut self,
        player: PlayerId,
        id: SimId,
    ) -> Result<BuildingConstructionCancelOutcome, BuildingConstructionCancelError> {
        let Some((entity, owner, construction, current_health)) =
            self.world.iter_entities().find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(id)).then(|| {
                    Some((
                        entity.id(),
                        entity.get::<Owner>()?.0,
                        *entity.get::<BuildingConstruction>()?,
                        *entity.get::<Health>()?,
                    ))
                })?
            })
        else {
            return Err(BuildingConstructionCancelError::ConstructionNotFound);
        };
        if owner != player {
            return Err(BuildingConstructionCancelError::NotOwner);
        }

        if let Some(economy) = construction.properties.economy {
            let resources = &mut self
                .player_state_mut(player)
                .expect("building owner must exist")
                .resources;
            resources.gold = resources
                .gold
                .checked_add(economy.gold_cost)
                .expect("player gold refund overflow");
            resources.lumber = resources
                .lumber
                .checked_add(economy.lumber_cost)
                .expect("player lumber refund overflow");
            let source_points = construction
                .upgrade_from
                .and_then(|source| source.properties.economy)
                .map_or(0, |source| source.legendary_points_cost);
            resources.legendary_points_used =
                resources.legendary_points_used - economy.legendary_points_cost + source_points;
        }
        let outcome = if let Some(source) = construction.upgrade_from {
            self.world
                .entity_mut(entity)
                .remove::<BuildingConstruction>();
            self.deactivate_building_entity(entity);
            let restored_health = scale_building_health(
                current_health.current,
                current_health.max,
                source.health.max,
            );
            {
                let mut entity_mut = self.world.entity_mut(entity);
                *entity_mut
                    .get_mut::<Health>()
                    .expect("upgrade cancellation source missing health") = Health {
                    current: restored_health,
                    max: source.health.max,
                };
                *entity_mut
                    .get_mut::<DamageType>()
                    .expect("upgrade cancellation source missing damage type") =
                    source.properties.damage_type;
                *entity_mut
                    .get_mut::<ArmorProfile>()
                    .expect("upgrade cancellation source missing armor profile") =
                    source.properties.armor;
                match source.properties.content {
                    Some(content) => {
                        entity_mut.insert(content);
                    }
                    None => {
                        entity_mut.remove::<ContentIdentity>();
                    }
                }
            }
            self.activate_building_entity(entity, source.building, source.properties);
            self.restore_building_runtime_state(entity, source.runtime);
            BuildingConstructionCancelOutcome::RevertedUpgrade
        } else {
            self.world.despawn(entity);
            self.topology_dirty = true;
            BuildingConstructionCancelOutcome::RemovedNewBuilding
        };
        Ok(outcome)
    }

    pub(super) fn advance_building_construction(&mut self) {
        let mut completing: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                let construction = *entity.get::<BuildingConstruction>()?;
                (construction.complete_tick <= self.next_tick).then_some((
                    *entity.get::<SimId>()?,
                    entity.id(),
                    entity.get::<Owner>()?.0,
                    construction,
                ))
            })
            .collect();
        completing.sort_unstable_by_key(|(id, ..)| *id);

        for (_, entity, owner, construction) in completing {
            self.world
                .entity_mut(entity)
                .remove::<BuildingConstruction>();
            self.activate_building_entity(entity, construction.building, construction.properties);
            if let Some(economy) = construction.properties.economy {
                let resources = &mut self
                    .player_state_mut(owner)
                    .expect("building owner must exist")
                    .resources;
                resources.lumber = resources
                    .lumber
                    .checked_add(economy.lumber_refund)
                    .expect("player lumber reward overflow");
            }
        }
    }

    pub(super) fn validate_building_placement(
        &self,
        team: Team,
        footprint: BuildingFootprint,
    ) -> Result<(), BuildingPlacementError> {
        if !self.footprint_inside_navigation(footprint) {
            return Err(BuildingPlacementError::OutsideNavigation);
        }
        if !self.footprint_inside_team_build_region(team, footprint) {
            return Err(BuildingPlacementError::OutsideBuildRegion);
        }
        if self
            .config
            .static_blockers
            .iter()
            .chain(self.config.build_static_blockers.iter())
            .copied()
            .any(|blocker| footprints_overlap(blocker, footprint))
        {
            return Err(BuildingPlacementError::StaticObstacle);
        }
        if self
            .world
            .iter_entities()
            .filter_map(|entity| entity.get::<BuildingFootprint>().copied())
            .any(|existing| footprints_overlap(existing, footprint))
        {
            return Err(BuildingPlacementError::BuildingOverlap);
        }
        if self.footprint_contains_live_unit(footprint) {
            return Err(BuildingPlacementError::UnitOccupied);
        }
        Ok(())
    }

    #[must_use]
    pub fn can_place_building(&self, footprint: BuildingFootprint) -> bool {
        self.footprint_inside_navigation(footprint)
            && !self
                .config
                .static_blockers
                .iter()
                .chain(self.config.build_static_blockers.iter())
                .copied()
                .any(|blocker| footprints_overlap(blocker, footprint))
            && !self
                .world
                .iter_entities()
                .filter_map(|entity| entity.get::<BuildingFootprint>().copied())
                .any(|existing| footprints_overlap(existing, footprint))
            && !self.footprint_overlaps_pending_build_order(footprint, None)
            && !self.footprint_contains_live_unit(footprint)
    }

    #[must_use]
    pub fn can_place_building_for_team(&self, team: Team, footprint: BuildingFootprint) -> bool {
        self.can_place_building(footprint)
            && self.footprint_inside_team_build_region(team, footprint)
    }

    /// Returns whether one navigation cell is individually legal for building placement.
    ///
    /// This is presentation-facing diagnostic data for footprint previews. Whole-building
    /// placement must still use [`Self::can_place_building_for_team`], which remains authoritative
    /// for rules that apply to the footprint as a whole.
    #[must_use]
    pub fn can_place_building_cell_for_team(&self, team: Team, cell: NavCell) -> bool {
        let footprint = BuildingFootprint::new(cell.x, cell.y, 1, 1);
        self.topology.contains(cell)
            && self.cell_inside_team_build_region(team, cell)
            && !self
                .config
                .static_blockers
                .iter()
                .chain(self.config.build_static_blockers.iter())
                .copied()
                .any(|blocker| footprint_contains_cell(blocker, cell))
            && !self
                .world
                .iter_entities()
                .filter_map(|entity| entity.get::<BuildingFootprint>().copied())
                .any(|existing| footprint_contains_cell(existing, cell))
            && !self
                .world
                .iter_entities()
                .filter_map(|entity| entity.get::<BuilderBuildOrder>())
                .any(|order| footprint_contains_cell(order.building.footprint, cell))
            && !self.footprint_contains_live_unit(footprint)
    }
}
