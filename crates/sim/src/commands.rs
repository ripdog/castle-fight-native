use crate::{
    BuilderBuildError, BuilderCommandError, BuildingCommandError, BuildingConstructionCancelError,
    BuildingConstructionCancelOutcome, BuildingFootprint, BuildingUpgradeError,
    CastleFightBuildingId, CastleFightBuildingKind, CastleFightContentBundle, MatchLifecycle,
    PlayerId, SimId, SimPoint, Simulation,
};

/// A gameplay action authored by one player.
///
/// Commands intentionally contain only stable entity/content identifiers and deterministic
/// coordinates. Authored stats, costs, cooldowns, and component payloads are resolved from the
/// selected content bundle by the authoritative executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildPosition {
    pub min_x: i32,
    pub min_y: i32,
}

impl BuildPosition {
    #[must_use]
    pub const fn new(min_x: i32, min_y: i32) -> Self {
        Self { min_x, min_y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerCommand {
    MoveBuilder {
        builder: SimId,
        destination: SimPoint,
    },
    FollowWithBuilder {
        builder: SimId,
        target: SimId,
    },
    StopBuilder {
        builder: SimId,
    },
    BlinkBuilder {
        builder: SimId,
        destination: SimPoint,
    },
    RepairWithBuilder {
        builder: SimId,
        target: SimId,
    },
    SetBuilderRepairAutocast {
        builder: SimId,
        enabled: bool,
    },
    PlaceBuilding {
        builder: SimId,
        building: CastleFightBuildingId,
        position: BuildPosition,
    },
    CancelBuildingConstruction {
        building: SimId,
    },
    QueueProductionUnit {
        building: SimId,
    },
    CancelProductionUnit {
        building: SimId,
    },
    UpgradeBuilding {
        building: SimId,
        target: CastleFightBuildingId,
    },
    AttackWithBuilding {
        building: SimId,
        target: SimId,
    },
    CastBuildingSpell {
        building: SimId,
    },
    SetBuildingSpellAutocast {
        building: SimId,
        enabled: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandAdmissionError {
    MatchNotRunning,
    UnknownPlayer,
    UnknownBuildingDefinition(CastleFightBuildingId),
    InvalidBuildPosition(BuildPosition),
    BuilderNotControllable(SimId),
    BuildingNotControllable(SimId),
    BuildingNotInBuilderCatalog {
        builder: SimId,
        building: CastleFightBuildingId,
    },
    InvalidUpgradeTarget {
        building: SimId,
        target: CastleFightBuildingId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandRejectReason {
    Builder(BuilderCommandError),
    Build(BuilderBuildError),
    CancelConstruction(BuildingConstructionCancelError),
    Upgrade(BuildingUpgradeError),
    Building(BuildingCommandError),
    UnknownBuildingDefinition(CastleFightBuildingId),
    InvalidBuildPosition(BuildPosition),
    SourceDefinitionMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandExecutionResult {
    Applied,
    BuilderBlinkedTo(SimPoint),
    BuildingConstructionCancelled(BuildingConstructionCancelOutcome),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutcome {
    Executed(CommandExecutionResult),
    Rejected(CommandRejectReason),
}

pub fn admit_player_command(
    simulation: &Simulation,
    content: &CastleFightContentBundle,
    player: PlayerId,
    command: PlayerCommand,
) -> Result<(), CommandAdmissionError> {
    if simulation.lifecycle() != MatchLifecycle::Running {
        return Err(CommandAdmissionError::MatchNotRunning);
    }
    if simulation.player(player).is_none() {
        return Err(CommandAdmissionError::UnknownPlayer);
    }

    match command {
        PlayerCommand::MoveBuilder { builder, .. }
        | PlayerCommand::FollowWithBuilder { builder, .. }
        | PlayerCommand::StopBuilder { builder }
        | PlayerCommand::BlinkBuilder { builder, .. }
        | PlayerCommand::RepairWithBuilder { builder, .. }
        | PlayerCommand::SetBuilderRepairAutocast { builder, .. } => {
            if simulation.can_player_control_builder(player, builder) {
                Ok(())
            } else {
                Err(CommandAdmissionError::BuilderNotControllable(builder))
            }
        }
        PlayerCommand::PlaceBuilding {
            builder,
            building,
            position,
        } => {
            if !simulation.can_player_control_builder(player, builder) {
                return Err(CommandAdmissionError::BuilderNotControllable(builder));
            }
            let kind = content
                .building_kind(building)
                .ok_or(CommandAdmissionError::UnknownBuildingDefinition(building))?;
            if resolved_build_footprint(kind, content, position).is_none() {
                return Err(CommandAdmissionError::InvalidBuildPosition(position));
            }
            let rawcode = kind
                .rawcode(content)
                .expect("resolved building kind must belong to selected content bundle");
            let builder_view = simulation
                .builder(builder)
                .ok_or(CommandAdmissionError::BuilderNotControllable(builder))?;
            if !builder_view.configuration.allows_building(rawcode) {
                return Err(CommandAdmissionError::BuildingNotInBuilderCatalog {
                    builder,
                    building,
                });
            }
            Ok(())
        }
        PlayerCommand::CancelBuildingConstruction { building }
        | PlayerCommand::QueueProductionUnit { building }
        | PlayerCommand::CancelProductionUnit { building }
        | PlayerCommand::AttackWithBuilding { building, .. }
        | PlayerCommand::CastBuildingSpell { building }
        | PlayerCommand::SetBuildingSpellAutocast { building, .. } => {
            if simulation.can_player_control_building(player, building) {
                Ok(())
            } else {
                Err(CommandAdmissionError::BuildingNotControllable(building))
            }
        }
        PlayerCommand::UpgradeBuilding { building, target } => {
            if !simulation.can_player_control_building(player, building) {
                return Err(CommandAdmissionError::BuildingNotControllable(building));
            }
            let Some(target_kind) = content.building_kind(target) else {
                return Err(CommandAdmissionError::UnknownBuildingDefinition(target));
            };
            let Some(source) = simulation.building(building) else {
                return Err(CommandAdmissionError::BuildingNotControllable(building));
            };
            let Some(source_kind) = source
                .content
                .and_then(|identity| content.building_kind_for_rawcode(identity.rawcode))
            else {
                return Err(CommandAdmissionError::InvalidUpgradeTarget { building, target });
            };
            let valid = source_kind
                .upgrade_targets_for_version(content.map_version)
                .expect("selected content bundle must support its own upgrade graph")
                .contains(&target_kind);
            if valid {
                Ok(())
            } else {
                Err(CommandAdmissionError::InvalidUpgradeTarget { building, target })
            }
        }
    }
}

pub(crate) fn execute_player_command(
    simulation: &mut Simulation,
    content: &CastleFightContentBundle,
    player: PlayerId,
    command: PlayerCommand,
) -> CommandOutcome {
    let result = match command {
        PlayerCommand::MoveBuilder {
            builder,
            destination,
        } => simulation
            .order_builder_move_as(player, builder, destination)
            .map(|()| CommandExecutionResult::Applied)
            .map_err(CommandRejectReason::Builder),
        PlayerCommand::FollowWithBuilder { builder, target } => simulation
            .order_builder_follow_as(player, builder, target)
            .map(|()| CommandExecutionResult::Applied)
            .map_err(CommandRejectReason::Builder),
        PlayerCommand::StopBuilder { builder } => simulation
            .stop_builder_as(player, builder)
            .map(|()| CommandExecutionResult::Applied)
            .map_err(CommandRejectReason::Builder),
        PlayerCommand::BlinkBuilder {
            builder,
            destination,
        } => simulation
            .order_builder_blink_as(player, builder, destination)
            .map(CommandExecutionResult::BuilderBlinkedTo)
            .map_err(CommandRejectReason::Builder),
        PlayerCommand::RepairWithBuilder { builder, target } => simulation
            .order_builder_repair_as(player, builder, target)
            .map(|()| CommandExecutionResult::Applied)
            .map_err(CommandRejectReason::Builder),
        PlayerCommand::SetBuilderRepairAutocast { builder, enabled } => simulation
            .set_builder_repair_autocast_as(player, builder, enabled)
            .map(|()| CommandExecutionResult::Applied)
            .map_err(CommandRejectReason::Builder),
        PlayerCommand::PlaceBuilding {
            builder,
            building,
            position,
        } => execute_place_building(simulation, content, player, builder, building, position),
        PlayerCommand::CancelBuildingConstruction { building } => simulation
            .cancel_building_construction_for_player(player, building)
            .map(CommandExecutionResult::BuildingConstructionCancelled)
            .map_err(CommandRejectReason::CancelConstruction),
        PlayerCommand::QueueProductionUnit { building } => simulation
            .queue_production_unit_for_player(player, building)
            .map(|()| CommandExecutionResult::Applied)
            .map_err(CommandRejectReason::Building),
        PlayerCommand::CancelProductionUnit { building } => simulation
            .cancel_production_unit_for_player(player, building)
            .map(|()| CommandExecutionResult::Applied)
            .map_err(CommandRejectReason::Building),
        PlayerCommand::UpgradeBuilding { building, target } => {
            execute_upgrade_building(simulation, content, player, building, target)
        }
        PlayerCommand::AttackWithBuilding { building, target } => simulation
            .order_building_attack_target_as(player, building, target)
            .map(|()| CommandExecutionResult::Applied)
            .map_err(CommandRejectReason::Building),
        PlayerCommand::CastBuildingSpell { building } => simulation
            .cast_building_spell_for_player(player, building)
            .map(|()| CommandExecutionResult::Applied)
            .map_err(CommandRejectReason::Building),
        PlayerCommand::SetBuildingSpellAutocast { building, enabled } => simulation
            .set_building_spell_autocast_for_player(player, building, enabled)
            .map(|()| CommandExecutionResult::Applied)
            .map_err(CommandRejectReason::Building),
    };

    match result {
        Ok(result) => CommandOutcome::Executed(result),
        Err(reason) => CommandOutcome::Rejected(reason),
    }
}

fn execute_place_building(
    simulation: &mut Simulation,
    content: &CastleFightContentBundle,
    player: PlayerId,
    builder: SimId,
    building: CastleFightBuildingId,
    position: BuildPosition,
) -> Result<CommandExecutionResult, CommandRejectReason> {
    let builder_view = simulation
        .builder(builder)
        .ok_or(CommandRejectReason::Builder(
            BuilderCommandError::BuilderNotFound,
        ))?;
    let team = builder_view.team;
    let kind = content
        .building_kind(building)
        .ok_or(CommandRejectReason::UnknownBuildingDefinition(building))?;
    let footprint = resolved_build_footprint(kind, content, position)
        .ok_or(CommandRejectReason::InvalidBuildPosition(position))?;
    let result = match kind {
        CastleFightBuildingKind::Production(kind) => {
            let definition = content
                .production_building(kind)
                .expect("resolved building kind must belong to selected content bundle");
            simulation.order_builder_purchase_building_with_properties_as(
                player,
                builder,
                definition.spawn(team, footprint),
                definition.gameplay_properties(),
            )
        }
        CastleFightBuildingKind::Tower(kind) => {
            let definition = content
                .tower(kind)
                .expect("resolved building kind must belong to selected content bundle");
            simulation.order_builder_purchase_building_with_properties_as(
                player,
                builder,
                definition.spawn(team, footprint),
                definition.gameplay_properties(),
            )
        }
    };
    result
        .map(|()| CommandExecutionResult::Applied)
        .map_err(CommandRejectReason::Build)
}

fn resolved_build_footprint(
    kind: CastleFightBuildingKind,
    content: &CastleFightContentBundle,
    position: BuildPosition,
) -> Option<BuildingFootprint> {
    let size = kind.footprint_size_cells(content)?;
    let extent = i32::from(size).checked_sub(1)?;
    position.min_x.checked_add(extent)?;
    position.min_y.checked_add(extent)?;
    Some(BuildingFootprint::new(
        position.min_x,
        position.min_y,
        size,
        size,
    ))
}

fn execute_upgrade_building(
    simulation: &mut Simulation,
    content: &CastleFightContentBundle,
    player: PlayerId,
    building: SimId,
    target: CastleFightBuildingId,
) -> Result<CommandExecutionResult, CommandRejectReason> {
    let source = simulation
        .building(building)
        .ok_or(CommandRejectReason::Upgrade(
            BuildingUpgradeError::SourceNotFound,
        ))?;
    let source_kind = source
        .content
        .and_then(|identity| content.building_kind_for_rawcode(identity.rawcode))
        .ok_or(CommandRejectReason::SourceDefinitionMismatch)?;
    let target_kind = content
        .building_kind(target)
        .ok_or(CommandRejectReason::UnknownBuildingDefinition(target))?;
    if !source_kind
        .upgrade_targets_for_version(content.map_version)
        .expect("selected content bundle must support its own upgrade graph")
        .contains(&target_kind)
    {
        return Err(CommandRejectReason::SourceDefinitionMismatch);
    }

    let resolve = |kind| match kind {
        CastleFightBuildingKind::Production(kind) => {
            let definition = content.production_building(kind)?;
            Some((
                definition.spawn(source.team, source.footprint),
                definition.gameplay_properties(),
            ))
        }
        CastleFightBuildingKind::Tower(kind) => {
            let definition = content.tower(kind)?;
            Some((
                definition.spawn(source.team, source.footprint),
                definition.gameplay_properties(),
            ))
        }
    };
    let (source_spawn, source_properties) =
        resolve(source_kind).ok_or(CommandRejectReason::SourceDefinitionMismatch)?;
    let (target_spawn, target_properties) =
        resolve(target_kind).ok_or(CommandRejectReason::UnknownBuildingDefinition(target))?;
    simulation
        .start_building_upgrade_as(
            player,
            building,
            source_spawn,
            source_properties,
            target_spawn,
            target_properties,
        )
        .map(|()| CommandExecutionResult::Applied)
        .map_err(CommandRejectReason::Upgrade)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CastleFightBuilderRace, CastleFightMatchConfig, CastleFightParticipantConfig,
        CastleFightProductionKind, CastleFightTowerKind, MapVersion, ResourcePurchaseError, Team,
        create_castle_fight_match,
    };

    fn match_927() -> crate::CastleFightMatch {
        create_castle_fight_match(
            CastleFightMatchConfig::development_subset(
                MapVersion::CASTLE_FIGHT_9_27,
                "r1",
                0x0043_4f4d_4d41_4e44,
            )
            .unwrap(),
            1,
        )
        .unwrap()
    }

    #[test]
    fn placement_command_resolves_definition_from_stable_content_id() {
        let mut game = match_927();
        let builder = game
            .simulation
            .builder_for_player(PlayerId(0))
            .expect("western builder");
        let barracks = CastleFightProductionKind::Barracks.stable_id();
        let footprint = BuildingFootprint::new(-138, 16, 4, 4);
        let command = PlayerCommand::PlaceBuilding {
            builder: builder.id,
            building: barracks,
            position: BuildPosition::new(footprint.min_x, footprint.min_y),
        };
        assert_eq!(
            admit_player_command(&game.simulation, game.content, PlayerId(0), command),
            Ok(())
        );
        assert_eq!(
            execute_player_command(&mut game.simulation, game.content, PlayerId(0), command),
            CommandOutcome::Executed(CommandExecutionResult::Applied)
        );
        assert_eq!(
            game.simulation
                .player_resources_for(PlayerId(0))
                .unwrap()
                .gold,
            150
        );
    }

    #[test]
    fn tower_upgrade_command_accepts_tiny_watch_to_multishot_edge() {
        let mut game = match_927();
        let source = CastleFightTowerKind::TinyWatchTower.definition();
        let target = CastleFightTowerKind::TinyMultishotTower.definition();
        let footprint = BuildingFootprint::new(
            -138,
            16,
            source.footprint_size_cells,
            source.footprint_size_cells,
        );
        let building = game.simulation.spawn_building_with_properties(
            source.spawn(Team(0), footprint),
            source.gameplay_properties(),
        );
        game.simulation
            .debug_grant_player_resources(Team(0), 1_000, 1_000);
        let command = PlayerCommand::UpgradeBuilding {
            building,
            target: CastleFightTowerKind::TinyMultishotTower.stable_id(),
        };

        assert_eq!(
            admit_player_command(&game.simulation, game.content, PlayerId(0), command),
            Ok(())
        );
        assert_eq!(
            execute_player_command(&mut game.simulation, game.content, PlayerId(0), command),
            CommandOutcome::Executed(CommandExecutionResult::Applied)
        );
        let upgrading = game.simulation.building(building).unwrap();
        assert_eq!(upgrading.content.unwrap().rawcode, target.rawcode);
        assert!(upgrading.construction_complete_tick.is_some());
    }

    #[test]
    fn allied_player_cannot_admit_someone_elses_commands() {
        let config = CastleFightMatchConfig::development_subset_with_participants(
            MapVersion::CASTLE_FIGHT_9_27,
            "r1",
            7,
            vec![
                CastleFightParticipantConfig {
                    id: PlayerId(0),
                    team: Team(0),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(1),
                    team: Team(0),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(6),
                    team: Team(1),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(7),
                    team: Team(1),
                    builder_race: CastleFightBuilderRace::Human,
                },
            ],
        )
        .unwrap();
        let game = create_castle_fight_match(config, 1).unwrap();
        let owner_builder = game
            .simulation
            .builder_for_player(PlayerId(0))
            .expect("player 0 builder");
        let move_command = PlayerCommand::MoveBuilder {
            builder: owner_builder.id,
            destination: owner_builder.position,
        };
        assert_eq!(
            admit_player_command(&game.simulation, game.content, PlayerId(1), move_command),
            Err(CommandAdmissionError::BuilderNotControllable(
                owner_builder.id
            ))
        );

        let western_castle = game.simulation.team_objective(Team(0)).unwrap();
        let building_command = PlayerCommand::AttackWithBuilding {
            building: western_castle,
            target: game.simulation.team_objective(Team(1)).unwrap(),
        };
        assert_eq!(
            admit_player_command(
                &game.simulation,
                game.content,
                PlayerId(1),
                building_command,
            ),
            Err(CommandAdmissionError::BuildingNotControllable(
                western_castle
            ))
        );
    }

    #[test]
    fn malformed_build_position_is_rejected_during_admission_and_execution() {
        let mut game = match_927();
        let builder = game
            .simulation
            .builder_for_player(PlayerId(0))
            .expect("western builder");
        let position = BuildPosition::new(i32::MAX, 0);
        let command = PlayerCommand::PlaceBuilding {
            builder: builder.id,
            building: CastleFightProductionKind::Barracks.stable_id(),
            position,
        };
        assert_eq!(
            admit_player_command(&game.simulation, game.content, PlayerId(0), command),
            Err(CommandAdmissionError::InvalidBuildPosition(position))
        );
        let resources_before = game.simulation.player_resources_for(PlayerId(0)).unwrap();
        assert_eq!(
            execute_player_command(&mut game.simulation, game.content, PlayerId(0), command),
            CommandOutcome::Rejected(CommandRejectReason::InvalidBuildPosition(position))
        );
        assert_eq!(
            game.simulation.player_resources_for(PlayerId(0)).unwrap(),
            resources_before
        );
    }

    #[test]
    fn mutable_resource_failure_is_rechecked_atomically_at_execution() {
        let mut game = match_927();
        let builder = game
            .simulation
            .builder_for_player(PlayerId(0))
            .expect("western builder");
        let mut configuration = builder.configuration.clone();
        configuration
            .build_catalog
            .push(CastleFightTowerKind::WatchTower.definition().rawcode);
        game.simulation
            .configure_builder(builder.id, builder.profile, configuration)
            .unwrap();
        let before = game.simulation.player_resources_for(PlayerId(0)).unwrap();
        let command = PlayerCommand::PlaceBuilding {
            builder: builder.id,
            building: CastleFightTowerKind::WatchTower.stable_id(),
            position: BuildPosition::new(-138, 16),
        };
        assert_eq!(
            admit_player_command(&game.simulation, game.content, PlayerId(0), command),
            Ok(()),
            "admission deliberately does not trust a mutable resource snapshot"
        );
        assert_eq!(
            execute_player_command(&mut game.simulation, game.content, PlayerId(0), command),
            CommandOutcome::Rejected(CommandRejectReason::Build(BuilderBuildError::Resources(
                ResourcePurchaseError::InsufficientLumber {
                    available: 125,
                    required: 300,
                }
            )))
        );
        assert_eq!(
            game.simulation.player_resources_for(PlayerId(0)).unwrap(),
            before,
            "failed execution must not partially spend resources"
        );
    }
}
