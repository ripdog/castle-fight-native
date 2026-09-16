use super::*;

impl Simulation {
    #[must_use]
    pub fn can_player_control_builder(&self, controller: PlayerId, builder: SimId) -> bool {
        if self.lifecycle != MatchLifecycle::Running {
            return false;
        }
        let Some(controller_state) = self.player_state(controller) else {
            return false;
        };
        if controller_state.connection != PlayerConnectionStatus::Connected {
            return false;
        }
        let Some((owner, team)) = self.world.iter_entities().find_map(|entity| {
            (entity.get::<SimId>().copied() == Some(builder) && entity.get::<Builder>().is_some())
                .then(|| Some((entity.get::<Owner>()?.0, *entity.get::<Team>()?)))?
        }) else {
            return false;
        };
        if owner == controller {
            return true;
        }
        let Some(owner_state) = self.player_state(owner) else {
            return false;
        };
        owner_state.connection == PlayerConnectionStatus::Disconnected
            && owner_state.team == controller_state.team
            && team == controller_state.team
    }

    #[must_use]
    pub fn can_player_control_building(&self, controller: PlayerId, building: SimId) -> bool {
        if self.lifecycle != MatchLifecycle::Running {
            return false;
        }
        let Some(controller_state) = self.player_state(controller) else {
            return false;
        };
        if controller_state.connection != PlayerConnectionStatus::Connected {
            return false;
        }
        self.world.iter_entities().any(|entity| {
            entity.get::<SimId>().copied() == Some(building)
                && entity.get::<BuildingFootprint>().is_some()
                && entity.get::<Owner>().map(|owner| owner.0) == Some(controller)
        })
    }

    pub(crate) fn order_builder_move_as(
        &mut self,
        controller: PlayerId,
        builder: SimId,
        destination: SimPoint,
    ) -> Result<(), BuilderCommandError> {
        if !self.can_player_control_builder(controller, builder) {
            return Err(BuilderCommandError::NotAuthorized);
        }
        self.order_builder_move(builder, destination)
    }

    pub(crate) fn order_builder_follow_as(
        &mut self,
        controller: PlayerId,
        builder: SimId,
        target: SimId,
    ) -> Result<(), BuilderCommandError> {
        if !self.can_player_control_builder(controller, builder) {
            return Err(BuilderCommandError::NotAuthorized);
        }
        self.order_builder_follow(builder, target)
    }

    pub(crate) fn order_builder_blink_as(
        &mut self,
        controller: PlayerId,
        builder: SimId,
        destination: SimPoint,
    ) -> Result<SimPoint, BuilderCommandError> {
        if !self.can_player_control_builder(controller, builder) {
            return Err(BuilderCommandError::NotAuthorized);
        }
        self.order_builder_blink(builder, destination)
    }

    pub(crate) fn order_builder_repair_as(
        &mut self,
        controller: PlayerId,
        builder: SimId,
        target: SimId,
    ) -> Result<(), BuilderCommandError> {
        if !self.can_player_control_builder(controller, builder) {
            return Err(BuilderCommandError::NotAuthorized);
        }
        self.order_builder_repair(builder, target)
    }

    pub(crate) fn set_builder_repair_autocast_as(
        &mut self,
        controller: PlayerId,
        builder: SimId,
        enabled: bool,
    ) -> Result<(), BuilderCommandError> {
        if !self.can_player_control_builder(controller, builder) {
            return Err(BuilderCommandError::NotAuthorized);
        }
        self.set_builder_repair_autocast(builder, enabled)
    }

    pub(crate) fn stop_builder_as(
        &mut self,
        controller: PlayerId,
        builder: SimId,
    ) -> Result<(), BuilderCommandError> {
        if !self.can_player_control_builder(controller, builder) {
            return Err(BuilderCommandError::NotAuthorized);
        }
        self.stop_builder(builder)
    }

    pub(crate) fn order_building_attack_target_as(
        &mut self,
        controller: PlayerId,
        source: SimId,
        target: SimId,
    ) -> Result<(), BuildingCommandError> {
        if !self.can_player_control_building(controller, source) {
            return Err(BuildingCommandError::NotAuthorized);
        }
        self.order_building_attack_target(source, target)
    }

    #[must_use]
    pub fn can_builder_afford_building(
        &self,
        builder: SimId,
        economy: BuildingEconomyProfile,
    ) -> bool {
        let Some(entity) = self.world.iter_entities().find(|entity| {
            entity.get::<SimId>().copied() == Some(builder) && entity.get::<Builder>().is_some()
        }) else {
            return false;
        };
        let Some(owner) = entity.get::<Owner>().copied() else {
            return false;
        };
        let Some(resources) = self.player_resources_for(owner.0) else {
            return false;
        };
        let committed = entity
            .get::<BuilderBuildOrder>()
            .and_then(|order| order.properties.economy);
        let available_gold = resources
            .gold
            .saturating_add(committed.map_or(0, |old| old.gold_cost));
        let available_lumber = resources
            .lumber
            .saturating_add(committed.map_or(0, |old| old.lumber_cost));
        available_gold >= economy.gold_cost && available_lumber >= economy.lumber_cost
    }

    pub fn spawn_builder(&mut self, builder: BuilderSpawn) -> SimId {
        let owner = self.inferred_owner_for_team(builder.team);
        self.spawn_builder_for_player(owner, builder)
    }

    pub fn spawn_builder_for_player(&mut self, owner: PlayerId, builder: BuilderSpawn) -> SimId {
        self.try_spawn_builder_for_player(owner, builder)
            .expect("invalid authored builder spawn")
    }

    pub fn try_spawn_builder(&mut self, builder: BuilderSpawn) -> Result<SimId, BuilderSpawnError> {
        let Some(owner) = self.unique_player_for_team(builder.team) else {
            return Err(BuilderSpawnError::UnsupportedPlayer);
        };
        self.try_spawn_builder_for_player(owner, builder)
    }

    pub fn try_spawn_builder_for_player(
        &mut self,
        owner: PlayerId,
        builder: BuilderSpawn,
    ) -> Result<SimId, BuilderSpawnError> {
        if builder.team.0 >= 2 {
            return Err(BuilderSpawnError::UnsupportedTeam);
        }
        let player = self
            .player_state(owner)
            .ok_or(BuilderSpawnError::UnsupportedPlayer)?;
        if player.team != builder.team {
            return Err(BuilderSpawnError::PlayerTeamMismatch);
        }
        assert!(builder.profile.speed_per_tick >= 0);
        assert!(builder.profile.build_range >= 0);
        assert!(builder.profile.repair_range >= 0);
        assert!(builder.profile.repair_autocast_range >= builder.profile.repair_range);
        assert!(builder.profile.repair_time_ratio_numerator > 0);
        assert!(builder.profile.repair_time_ratio_denominator > 0);
        assert!(builder.profile.full_repair_duration_ticks > 0);
        assert!(builder.profile.blink_range >= 0);
        assert!(builder.profile.blink_boundary_inset >= 0);
        if self.world.iter_entities().any(|entity| {
            entity.get::<Builder>().is_some() && entity.get::<Owner>() == Some(&Owner(owner))
        }) {
            return Err(BuilderSpawnError::PlayerAlreadyHasBuilder);
        }
        if !self.point_inside_team_build_region(builder.team, builder.position) {
            return Err(BuilderSpawnError::OutsideBuildRegion);
        }

        let id = self.allocate_id();
        self.world.spawn((
            id,
            builder.team,
            Owner(owner),
            Position(builder.position),
            Builder,
            builder.profile,
            builder.configuration,
            BuilderState {
                repair_autocast_enabled: builder.repair_autocast_enabled,
                ..BuilderState::default()
            },
        ));
        Ok(id)
    }

    pub fn configure_builder(
        &mut self,
        builder: SimId,
        profile: BuilderProfile,
        configuration: BuilderConfiguration,
    ) -> Result<(), BuilderCommandError> {
        assert!(profile.speed_per_tick >= 0);
        assert!(profile.build_range >= 0);
        assert!(profile.repair_range >= 0);
        assert!(profile.repair_autocast_range >= profile.repair_range);
        assert!(profile.repair_time_ratio_numerator > 0);
        assert!(profile.repair_time_ratio_denominator > 0);
        assert!(profile.full_repair_duration_ticks > 0);
        assert!(profile.blink_range >= 0);
        assert!(profile.blink_boundary_inset >= 0);
        let entity = self
            .world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(builder)
                    && entity.get::<Builder>().is_some())
                .then_some(entity.id())
            })
            .ok_or(BuilderCommandError::BuilderNotFound)?;
        let mut builder_entity = self.world.entity_mut(entity);
        *builder_entity
            .get_mut::<BuilderProfile>()
            .expect("builder missing profile") = profile;
        *builder_entity
            .get_mut::<BuilderConfiguration>()
            .expect("builder missing configuration") = configuration;
        Ok(())
    }

    pub(crate) fn order_builder_move(
        &mut self,
        builder: SimId,
        destination: SimPoint,
    ) -> Result<(), BuilderCommandError> {
        let (entity, team) = self
            .world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(builder)
                    && entity.get::<Builder>().is_some())
                .then(|| {
                    (
                        entity.id(),
                        *entity.get::<Team>().expect("builder missing team"),
                    )
                })
            })
            .ok_or(BuilderCommandError::BuilderNotFound)?;
        if !self.point_inside_team_build_region(team, destination) {
            return Err(BuilderCommandError::OutsideBuildRegion);
        }

        self.cancel_builder_build_order_internal(entity);
        let mut entity = self.world.entity_mut(entity);
        let mut state = entity
            .get_mut::<BuilderState>()
            .expect("builder missing command state");
        state.destination = Some(destination);
        state.follow_target = None;
        state.repair_target = None;
        state.repair_progress_remainder = 0;
        Ok(())
    }

    pub(crate) fn order_builder_follow(
        &mut self,
        builder: SimId,
        target: SimId,
    ) -> Result<(), BuilderCommandError> {
        let builder_entity = self
            .world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(builder)
                    && entity.get::<Builder>().is_some())
                .then_some(entity.id())
            })
            .ok_or(BuilderCommandError::BuilderNotFound)?;
        if target == builder || self.builder_follow_target(target).is_none() {
            return Err(BuilderCommandError::FollowTargetNotFound);
        }

        self.cancel_builder_build_order_internal(builder_entity);
        let mut entity = self.world.entity_mut(builder_entity);
        let mut state = entity
            .get_mut::<BuilderState>()
            .expect("builder missing command state");
        state.destination = None;
        state.follow_target = Some(target);
        state.repair_target = None;
        state.repair_progress_remainder = 0;
        Ok(())
    }

    pub(crate) fn order_builder_blink(
        &mut self,
        builder: SimId,
        destination: SimPoint,
    ) -> Result<SimPoint, BuilderCommandError> {
        let (entity, team, position, profile) = self
            .world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(builder)
                    && entity.get::<Builder>().is_some())
                .then(|| {
                    (
                        entity.id(),
                        *entity.get::<Team>().expect("builder missing team"),
                        entity
                            .get::<Position>()
                            .expect("builder missing position")
                            .0,
                        *entity
                            .get::<BuilderProfile>()
                            .expect("builder missing profile"),
                    )
                })
            })
            .ok_or(BuilderCommandError::BuilderNotFound)?;
        if position.distance_sq(destination) > square_i32(profile.blink_range) {
            return Err(BuilderCommandError::BlinkOutOfRange);
        }
        let destination = self
            .clamp_builder_blink_destination(team, destination, profile.blink_boundary_inset)
            .expect("spawned builder team must have a legal movement region");

        self.cancel_builder_build_order_internal(entity);
        let mut builder = self.world.entity_mut(entity);
        builder
            .get_mut::<Position>()
            .expect("builder missing position")
            .0 = destination;
        let mut state = builder
            .get_mut::<BuilderState>()
            .expect("builder missing command state");
        let repair_autocast_enabled = state.repair_autocast_enabled;
        *state = BuilderState {
            repair_autocast_enabled,
            ..BuilderState::default()
        };
        Ok(destination)
    }

    pub(crate) fn order_builder_repair(
        &mut self,
        builder: SimId,
        target: SimId,
    ) -> Result<(), BuilderCommandError> {
        let (builder_entity, builder_team) = self
            .world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(builder)
                    && entity.get::<Builder>().is_some())
                .then(|| {
                    (
                        entity.id(),
                        *entity.get::<Team>().expect("builder missing team"),
                    )
                })
            })
            .ok_or(BuilderCommandError::BuilderNotFound)?;
        let (target_team, repairable) = self
            .world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(target)
                    && entity
                        .get::<Health>()
                        .is_some_and(|health| health.current > 0))
                .then(|| {
                    (
                        *entity.get::<Team>().expect("repair target missing team"),
                        entity.get::<BuildingFootprint>().is_some()
                            || entity.get::<MechanicalUnit>().is_some(),
                    )
                })
            })
            .ok_or(BuilderCommandError::RepairTargetNotFound)?;
        if builder_team != target_team {
            return Err(BuilderCommandError::NotFriendlyRepairTarget);
        }
        if !repairable {
            return Err(BuilderCommandError::RepairTargetNotRepairable);
        }

        self.cancel_builder_build_order_internal(builder_entity);
        let mut entity = self.world.entity_mut(builder_entity);
        let mut state = entity
            .get_mut::<BuilderState>()
            .expect("builder missing command state");
        state.destination = None;
        state.follow_target = None;
        state.repair_target = Some(target);
        state.repair_progress_remainder = 0;
        Ok(())
    }

    pub(crate) fn set_builder_repair_autocast(
        &mut self,
        builder: SimId,
        enabled: bool,
    ) -> Result<(), BuilderCommandError> {
        let entity = self
            .world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(builder)
                    && entity.get::<Builder>().is_some())
                .then_some(entity.id())
            })
            .ok_or(BuilderCommandError::BuilderNotFound)?;
        self.world
            .entity_mut(entity)
            .get_mut::<BuilderState>()
            .expect("builder missing command state")
            .repair_autocast_enabled = enabled;
        Ok(())
    }

    pub(crate) fn stop_builder(&mut self, builder: SimId) -> Result<(), BuilderCommandError> {
        let entity = self
            .world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(builder)
                    && entity.get::<Builder>().is_some())
                .then_some(entity.id())
            })
            .ok_or(BuilderCommandError::BuilderNotFound)?;
        self.cancel_builder_build_order_internal(entity);
        let mut builder = self.world.entity_mut(entity);
        let mut state = builder
            .get_mut::<BuilderState>()
            .expect("builder missing command state");
        let repair_autocast_enabled = state.repair_autocast_enabled;
        *state = BuilderState {
            repair_autocast_enabled,
            ..BuilderState::default()
        };
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn try_builder_summon_building_with_properties(
        &mut self,
        builder: SimId,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> Result<SimId, BuilderBuildError> {
        let (_, owner) = self.validate_builder_summon(builder, building, properties)?;
        self.try_spawn_building_internal(Some(owner), building, properties)
            .map_err(BuilderBuildError::Placement)
    }

    #[cfg(test)]
    pub(crate) fn try_builder_purchase_building_with_properties(
        &mut self,
        builder: SimId,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> Result<SimId, BuilderBuildError> {
        let (_, owner) = self.validate_builder_summon(builder, building, properties)?;
        let economy = properties
            .economy
            .ok_or(BuilderBuildError::MissingEconomyProfile)?;
        let resources = self
            .player_resources_for(owner)
            .expect("builder owner must have player resources");
        if resources.gold < economy.gold_cost {
            return Err(BuilderBuildError::Resources(
                ResourcePurchaseError::InsufficientGold {
                    available: resources.gold,
                    required: economy.gold_cost,
                },
            ));
        }
        if resources.lumber < economy.lumber_cost {
            return Err(BuilderBuildError::Resources(
                ResourcePurchaseError::InsufficientLumber {
                    available: resources.lumber,
                    required: economy.lumber_cost,
                },
            ));
        }

        let id = self
            .try_spawn_building_internal(Some(owner), building, properties)
            .map_err(BuilderBuildError::Placement)?;
        let resources = &mut self
            .player_state_mut(owner)
            .expect("builder owner must exist")
            .resources;
        resources.gold -= economy.gold_cost;
        resources.lumber -= economy.lumber_cost;
        resources.lumber = resources
            .lumber
            .checked_add(economy.lumber_refund)
            .expect("player lumber overflow");
        Ok(id)
    }

    pub(crate) fn order_builder_purchase_building_with_properties_as(
        &mut self,
        controller: PlayerId,
        builder: SimId,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> Result<(), BuilderBuildError> {
        if !self.can_player_control_builder(controller, builder) {
            return Err(BuilderBuildError::Builder(
                BuilderCommandError::NotAuthorized,
            ));
        }
        self.order_builder_purchase_building_with_properties(builder, building, properties)
    }

    pub(crate) fn order_builder_purchase_building_with_properties(
        &mut self,
        builder: SimId,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> Result<(), BuilderBuildError> {
        let (builder_entity, owner) =
            self.validate_builder_summon(builder, building, properties)?;
        self.validate_building_placement(building.team, building.footprint)
            .map_err(BuilderBuildError::Placement)?;
        if self.footprint_overlaps_pending_build_order(building.footprint, Some(builder_entity)) {
            return Err(BuilderBuildError::Placement(
                BuildingPlacementError::BuildingReserved,
            ));
        }
        let economy = properties
            .economy
            .ok_or(BuilderBuildError::MissingEconomyProfile)?;
        let current_order = self
            .world
            .entity(builder_entity)
            .get::<BuilderBuildOrder>()
            .copied();
        let current_economy = current_order.and_then(|order| order.properties.economy);
        let resources = self
            .player_resources_for(owner)
            .expect("builder owner must have player resources");
        let available_gold = resources
            .gold
            .checked_add(current_economy.map_or(0, |old| old.gold_cost))
            .expect("player gold availability overflow");
        let available_lumber = resources
            .lumber
            .checked_add(current_economy.map_or(0, |old| old.lumber_cost))
            .expect("player lumber availability overflow");
        if available_gold < economy.gold_cost {
            return Err(BuilderBuildError::Resources(
                ResourcePurchaseError::InsufficientGold {
                    available: available_gold,
                    required: economy.gold_cost,
                },
            ));
        }
        if available_lumber < economy.lumber_cost {
            return Err(BuilderBuildError::Resources(
                ResourcePurchaseError::InsufficientLumber {
                    available: available_lumber,
                    required: economy.lumber_cost,
                },
            ));
        }

        self.cancel_builder_build_order_internal(builder_entity);
        let resources = &mut self
            .player_state_mut(owner)
            .expect("builder owner must exist")
            .resources;
        resources.gold -= economy.gold_cost;
        resources.lumber -= economy.lumber_cost;
        self.world
            .entity_mut(builder_entity)
            .insert(BuilderBuildOrder {
                building,
                properties,
            });
        let mut builder_entity_mut = self.world.entity_mut(builder_entity);
        let mut builder_state = builder_entity_mut
            .get_mut::<BuilderState>()
            .expect("builder missing command state");
        builder_state.destination = None;
        builder_state.follow_target = None;
        builder_state.repair_target = None;
        builder_state.repair_progress_remainder = 0;
        Ok(())
    }

    fn cancel_builder_build_order_internal(&mut self, builder_entity: Entity) -> bool {
        let order = self
            .world
            .entity(builder_entity)
            .get::<BuilderBuildOrder>()
            .copied();
        let Some(order) = order else {
            return false;
        };
        self.world
            .entity_mut(builder_entity)
            .remove::<BuilderBuildOrder>();
        if let Some(economy) = order.properties.economy {
            let owner = self
                .world
                .entity(builder_entity)
                .get::<Owner>()
                .copied()
                .expect("builder missing owner")
                .0;
            let resources = &mut self
                .player_state_mut(owner)
                .expect("builder owner must exist")
                .resources;
            resources.gold = resources
                .gold
                .checked_add(economy.gold_cost)
                .expect("player gold refund overflow");
            resources.lumber = resources
                .lumber
                .checked_add(economy.lumber_cost)
                .expect("player lumber refund overflow");
        }
        true
    }

    fn validate_builder_summon(
        &self,
        builder: SimId,
        building: BuildingSpawn,
        properties: BuildingGameplayProperties,
    ) -> Result<(Entity, PlayerId), BuilderBuildError> {
        let (builder_entity, builder_team, builder_owner) = self
            .world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(builder)
                    && entity.get::<Builder>().is_some())
                .then(|| {
                    (
                        entity.id(),
                        *entity.get::<Team>().expect("builder missing team"),
                        entity.get::<Owner>().expect("builder missing owner").0,
                    )
                })
            })
            .ok_or(BuilderBuildError::Builder(
                BuilderCommandError::BuilderNotFound,
            ))?;
        if builder_team != building.team {
            return Err(BuilderBuildError::TeamMismatch);
        }
        let building_rawcode = properties
            .content
            .ok_or(BuilderBuildError::MissingBuildingIdentity)?
            .rawcode;
        let building_allowed = self
            .world
            .entity(builder_entity)
            .get::<BuilderConfiguration>()
            .expect("builder missing configuration")
            .allows_building(building_rawcode);
        if !building_allowed {
            return Err(BuilderBuildError::BuildingNotInCatalog);
        }
        Ok((builder_entity, builder_owner))
    }

    fn builder_follow_target(&self, target_id: SimId) -> Option<BuilderFollowTargetSnapshot> {
        let default_collision_radius = self.default_collision_radius();
        self.world
            .iter_entities()
            .find(|entity| entity.get::<SimId>().copied() == Some(target_id))
            .and_then(|entity| builder_follow_target_from_entity(entity, default_collision_radius))
    }

    fn builder_repair_target(&self, target_id: SimId) -> Option<BuilderRepairTargetSnapshot> {
        self.world
            .iter_entities()
            .find(|entity| entity.get::<SimId>().copied() == Some(target_id))
            .and_then(builder_repair_target_from_entity)
    }

    fn select_builder_autocast_repair_target(
        &self,
        team: Team,
        position: SimPoint,
        acquisition_range: i32,
    ) -> Option<SimId> {
        let max_distance_sq = square_i32(acquisition_range);
        self.world
            .iter_entities()
            .filter_map(builder_repair_target_from_entity)
            .filter(|target| {
                target.team == team
                    && target.health.current > 0
                    && target.health.current < target.health.max
                    && match target.geometry {
                        BuilderRepairGeometry::Building(_) => true,
                        BuilderRepairGeometry::Unit(target_position) => {
                            self.point_inside_team_build_region(team, target_position)
                        }
                    }
            })
            .filter_map(|target| {
                let distance_sq = target.distance_sq(position, self.config.navigation_cell_size);
                (distance_sq <= max_distance_sq).then_some((distance_sq, target.id))
            })
            .min_by_key(|(distance_sq, id)| (*distance_sq, *id))
            .map(|(_, id)| id)
    }

    pub(super) fn advance_builders(&mut self) {
        let mut builders: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                entity.get::<Builder>()?;
                Some((
                    *entity.get::<SimId>()?,
                    entity.id(),
                    entity.get::<Owner>()?.0,
                    *entity.get::<Team>()?,
                    entity.get::<Position>()?.0,
                    *entity.get::<BuilderProfile>()?,
                    *entity.get::<BuilderState>()?,
                    entity.get::<BuilderBuildOrder>().copied(),
                ))
            })
            .collect();
        builders.sort_unstable_by_key(|(id, ..)| *id);

        for (_, builder_entity, owner, team, position, profile, mut state, build_order) in builders
        {
            let mut next_position = position;

            if let Some(order) = build_order {
                let mut distance_sq = point_to_footprint_distance_sq(
                    next_position,
                    order.building.footprint,
                    self.config.navigation_cell_size,
                );
                if distance_sq > square_i32(profile.build_range) {
                    let candidate = next_position.step_towards(
                        footprint_center_point(
                            order.building.footprint,
                            self.config.navigation_cell_size,
                        ),
                        profile.speed_per_tick,
                    );
                    if self.point_inside_team_build_region(team, candidate) {
                        next_position = candidate;
                        distance_sq = point_to_footprint_distance_sq(
                            next_position,
                            order.building.footprint,
                            self.config.navigation_cell_size,
                        );
                    } else {
                        self.cancel_builder_build_order_internal(builder_entity);
                    }
                }

                if self
                    .world
                    .entity(builder_entity)
                    .get::<BuilderBuildOrder>()
                    .is_some()
                    && distance_sq <= square_i32(profile.build_range)
                {
                    let starts_construction = order.properties.construction_time_ticks.is_some();
                    let result = if starts_construction {
                        self.try_start_building_construction(
                            owner,
                            order.building,
                            order.properties,
                        )
                    } else {
                        self.try_spawn_building_internal(
                            Some(owner),
                            order.building,
                            order.properties,
                        )
                    };
                    match result {
                        Ok(_) => {
                            self.world
                                .entity_mut(builder_entity)
                                .remove::<BuilderBuildOrder>();
                            if !starts_construction && let Some(economy) = order.properties.economy
                            {
                                let resources = &mut self
                                    .player_state_mut(owner)
                                    .expect("builder owner must exist")
                                    .resources;
                                resources.lumber = resources
                                    .lumber
                                    .checked_add(economy.lumber_refund)
                                    .expect("player lumber reward overflow");
                            }
                        }
                        Err(_) => {
                            // Construction has not begun yet, so Castle Fight's
                            // ConstructionRefundRate=1 returns the full committed cost.
                            self.cancel_builder_build_order_internal(builder_entity);
                        }
                    }
                }
                self.apply_builder_state(builder_entity, next_position, state);
                continue;
            }

            if state.repair_target.is_none()
                && state.follow_target.is_none()
                && state.destination.is_none()
                && state.repair_autocast_enabled
            {
                state.repair_target = self.select_builder_autocast_repair_target(
                    team,
                    next_position,
                    profile.repair_autocast_range,
                );
                state.repair_progress_remainder = 0;
            }

            if let Some(target_id) = state.repair_target {
                let Some(target) = self.builder_repair_target(target_id) else {
                    state.repair_target = None;
                    state.repair_progress_remainder = 0;
                    self.apply_builder_state(builder_entity, next_position, state);
                    continue;
                };
                if target.team != team
                    || target.health.current <= 0
                    || target.health.current >= target.health.max
                {
                    state.repair_target = None;
                    state.repair_progress_remainder = 0;
                    self.apply_builder_state(builder_entity, next_position, state);
                    continue;
                }

                let mut distance_sq =
                    target.distance_sq(next_position, self.config.navigation_cell_size);
                if distance_sq > square_i32(profile.repair_range) {
                    let candidate = next_position.step_towards(
                        target.approach_position(self.config.navigation_cell_size),
                        profile.speed_per_tick,
                    );
                    if self.point_inside_team_build_region(team, candidate) {
                        next_position = candidate;
                        distance_sq =
                            target.distance_sq(next_position, self.config.navigation_cell_size);
                    } else {
                        state.repair_target = None;
                        state.repair_progress_remainder = 0;
                    }
                }

                if state.repair_target.is_some() && distance_sq <= square_i32(profile.repair_range)
                {
                    let duration = builder_repair_duration_ticks(profile, target);
                    let accumulated = u64::from(state.repair_progress_remainder)
                        + u64::try_from(target.health.max)
                            .expect("repair target max health must be positive");
                    let repaired = accumulated / duration;
                    state.repair_progress_remainder = u32::try_from(accumulated % duration)
                        .expect("repair remainder fits builder state");

                    if repaired > 0 {
                        let mut target_entity = self.world.entity_mut(target.entity);
                        let mut target_health = target_entity
                            .get_mut::<Health>()
                            .expect("repair target lost health component");
                        let repaired = i32::try_from(repaired).unwrap_or(i32::MAX);
                        target_health.current = target_health
                            .current
                            .saturating_add(repaired)
                            .min(target_health.max);
                        if target_health.current >= target_health.max {
                            state.repair_target = None;
                            state.repair_progress_remainder = 0;
                        }
                    }
                }
            } else if let Some(target_id) = state.follow_target {
                let Some(target) = self.builder_follow_target(target_id) else {
                    state.follow_target = None;
                    self.apply_builder_state(builder_entity, next_position, state);
                    continue;
                };
                if !target.reached(next_position, self.config.navigation_cell_size) {
                    let approach =
                        target.approach_position(next_position, self.config.navigation_cell_size);
                    let candidate = next_position.step_towards(approach, profile.speed_per_tick);
                    if self.point_inside_team_build_region(team, candidate) {
                        next_position = candidate;
                    } else {
                        state.follow_target = None;
                    }
                }
            } else if let Some(destination) = state.destination {
                let candidate = next_position.step_towards(destination, profile.speed_per_tick);
                if self.point_inside_team_build_region(team, candidate) {
                    next_position = candidate;
                    if next_position == destination {
                        state.destination = None;
                    }
                } else {
                    state.destination = None;
                }
            }

            self.apply_builder_state(builder_entity, next_position, state);
        }
    }

    fn apply_builder_state(&mut self, entity: Entity, position: SimPoint, state: BuilderState) {
        let mut builder = self.world.entity_mut(entity);
        builder
            .get_mut::<Position>()
            .expect("builder missing position")
            .0 = position;
        *builder
            .get_mut::<BuilderState>()
            .expect("builder missing command state") = state;
    }
}

#[derive(Debug, Clone, Copy)]
enum BuilderFollowGeometry {
    Building(BuildingFootprint),
    Point { position: SimPoint, stop_range: i32 },
}

#[derive(Debug, Clone, Copy)]
struct BuilderFollowTargetSnapshot {
    geometry: BuilderFollowGeometry,
}

impl BuilderFollowTargetSnapshot {
    fn reached(self, position: SimPoint, navigation_cell_size: i32) -> bool {
        match self.geometry {
            BuilderFollowGeometry::Building(footprint) => {
                point_to_footprint_distance_sq(position, footprint, navigation_cell_size) == 0
            }
            BuilderFollowGeometry::Point {
                position: target,
                stop_range,
            } => position.distance_sq(target) <= square_i32(stop_range),
        }
    }

    fn approach_position(self, source: SimPoint, navigation_cell_size: i32) -> SimPoint {
        match self.geometry {
            BuilderFollowGeometry::Building(footprint) => {
                closest_point_on_footprint(source, footprint, navigation_cell_size)
            }
            BuilderFollowGeometry::Point {
                position: target,
                stop_range,
            } => point_attack_envelope_goal(source, target, stop_range),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum BuilderRepairGeometry {
    Building(BuildingFootprint),
    Unit(SimPoint),
}

#[derive(Debug, Clone, Copy)]
struct BuilderRepairTargetSnapshot {
    entity: Entity,
    id: SimId,
    team: Team,
    health: Health,
    geometry: BuilderRepairGeometry,
    repair_time_ticks: Option<u32>,
}

impl BuilderRepairTargetSnapshot {
    fn distance_sq(self, position: SimPoint, navigation_cell_size: i32) -> u64 {
        match self.geometry {
            BuilderRepairGeometry::Building(footprint) => {
                point_to_footprint_distance_sq(position, footprint, navigation_cell_size)
            }
            BuilderRepairGeometry::Unit(target) => position.distance_sq(target),
        }
    }

    fn approach_position(self, navigation_cell_size: i32) -> SimPoint {
        match self.geometry {
            BuilderRepairGeometry::Building(footprint) => {
                footprint_center_point(footprint, navigation_cell_size)
            }
            BuilderRepairGeometry::Unit(position) => position,
        }
    }
}

fn builder_follow_target_from_entity(
    entity: bevy_ecs::world::EntityRef<'_>,
    default_collision_radius: i32,
) -> Option<BuilderFollowTargetSnapshot> {
    if entity
        .get::<Health>()
        .is_some_and(|health| health.current <= 0)
    {
        return None;
    }
    if let Some(footprint) = entity.get::<BuildingFootprint>().copied() {
        return Some(BuilderFollowTargetSnapshot {
            geometry: BuilderFollowGeometry::Building(footprint),
        });
    }
    let position = entity.get::<Position>()?.0;
    let stop_range = entity
        .get::<CollisionRadius>()
        .map_or(default_collision_radius, |radius| radius.0);
    Some(BuilderFollowTargetSnapshot {
        geometry: BuilderFollowGeometry::Point {
            position,
            stop_range,
        },
    })
}

fn builder_repair_target_from_entity(
    entity: bevy_ecs::world::EntityRef<'_>,
) -> Option<BuilderRepairTargetSnapshot> {
    let id = *entity.get::<SimId>()?;
    let team = *entity.get::<Team>()?;
    let health = *entity.get::<Health>()?;
    if let Some(footprint) = entity.get::<BuildingFootprint>().copied() {
        return Some(BuilderRepairTargetSnapshot {
            entity: entity.id(),
            id,
            team,
            health,
            geometry: BuilderRepairGeometry::Building(footprint),
            repair_time_ticks: entity.get::<RepairTimeTicks>().map(|ticks| ticks.0),
        });
    }
    entity.get::<MechanicalUnit>()?;
    let position = entity.get::<Position>()?.0;
    let repair_time_ticks = entity.get::<RepairTimeTicks>()?.0;
    Some(BuilderRepairTargetSnapshot {
        entity: entity.id(),
        id,
        team,
        health,
        geometry: BuilderRepairGeometry::Unit(position),
        repair_time_ticks: Some(repair_time_ticks),
    })
}

fn builder_repair_duration_ticks(
    profile: BuilderProfile,
    target: BuilderRepairTargetSnapshot,
) -> u64 {
    let Some(repair_time_ticks) = target.repair_time_ticks else {
        return u64::from(profile.full_repair_duration_ticks);
    };
    let scaled = u64::from(repair_time_ticks)
        .checked_mul(u64::from(profile.repair_time_ratio_numerator))
        .expect("mechanical-unit repair duration overflow");
    let denominator = u64::from(profile.repair_time_ratio_denominator);
    scaled.div_ceil(denominator).max(1)
}
