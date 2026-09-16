use std::collections::{BTreeMap, BTreeSet};

mod replay;
mod snapshot;

pub use replay::{
    MATCH_REPLAY_SCHEMA_VERSION, MatchReplay, ReplayCheckpoint, ReplayError, ReplayHeader,
    ReplaySeekSnapshot,
};
pub use snapshot::{
    MATCH_DRIVER_SNAPSHOT_SCHEMA_VERSION, MatchDriverSnapshot, MatchSnapshotRestoreError,
};

use crate::CastleFightContentBundle;
use crate::{
    CommandAdmissionError, CommandOutcome, MatchLifecycle, MatchOutcome, PlayerCommand,
    PlayerConnectionStatus, PlayerId, Simulation, TickResult, admit_player_command,
    commands::execute_player_command,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientCommandSequence(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommandOrder(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InputStreamPosition(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledCommand {
    pub tick: u64,
    pub order: CommandOrder,
    pub player: PlayerId,
    pub client_sequence: ClientCommandSequence,
    pub command: PlayerCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalizedTickInputs {
    pub tick: u64,
    pub stream_position: InputStreamPosition,
    pub commands: Vec<ScheduledCommand>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchControlEvent {
    SetPlayerConnection {
        player: PlayerId,
        connection: PlayerConnectionStatus,
    },
    FinishMatch {
        outcome: MatchOutcome,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundaryControlRecord {
    /// The completed simulation tick immediately preceding this control. `None` is the initial
    /// pre-tick boundary.
    pub after_completed_tick: Option<u64>,
    pub stream_position: InputStreamPosition,
    pub event: MatchControlEvent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalStreamRecord {
    Tick(FinalizedTickInputs),
    Control(BoundaryControlRecord),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandExecution {
    pub scheduled: ScheduledCommand,
    pub outcome: CommandOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSubmission {
    Scheduled(ScheduledCommand),
    DuplicateScheduled(ScheduledCommand),
    Rejected(CommandAdmissionError),
    DuplicateRejected(CommandAdmissionError),
}

impl CommandSubmission {
    #[must_use]
    pub const fn scheduled(self) -> Option<ScheduledCommand> {
        match self {
            Self::Scheduled(command) | Self::DuplicateScheduled(command) => Some(command),
            Self::Rejected(_) | Self::DuplicateRejected(_) => None,
        }
    }

    #[must_use]
    pub const fn is_newly_scheduled(self) -> bool {
        matches!(self, Self::Scheduled(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSequenceError {
    Gap {
        player: PlayerId,
        expected: ClientCommandSequence,
        actual: ClientCommandSequence,
    },
    Stale {
        player: PlayerId,
        expected: ClientCommandSequence,
        actual: ClientCommandSequence,
    },
    ConflictingDuplicate {
        player: PlayerId,
        sequence: ClientCommandSequence,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalStreamError {
    UnexpectedStreamPosition {
        expected: InputStreamPosition,
        actual: InputStreamPosition,
    },
    UnexpectedTick {
        expected: u64,
        actual: u64,
    },
    MatchNotRunning(MatchLifecycle),
    PendingLocalCommands,
    CommandTickMismatch {
        expected: u64,
        actual: u64,
    },
    NonCanonicalCommandOrder {
        expected: CommandOrder,
        actual: CommandOrder,
    },
    UnknownCommandPlayer(PlayerId),
    DuplicateCommandSequence {
        player: PlayerId,
        sequence: ClientCommandSequence,
    },
    BoundaryMismatch {
        expected: Option<u64>,
        actual: Option<u64>,
    },
    UnknownControlPlayer(PlayerId),
    MatchAlreadyFinished,
    StreamPositionExhausted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SubmissionRecord {
    command: PlayerCommand,
    disposition: StoredSubmissionDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoredSubmissionDisposition {
    Scheduled(ScheduledCommand),
    Rejected(CommandAdmissionError),
}

#[derive(Debug)]
pub struct DriverTickResult {
    pub finalized: FinalizedTickInputs,
    pub executions: Vec<CommandExecution>,
    pub tick_result: TickResult,
}

/// Owns canonical input ordering/history while the `Simulation` remains the authoritative gameplay
/// state. The local client uses this exact driver; a future server can feed the same finalized
/// records to independent instances.
pub struct MatchDriver {
    content: &'static CastleFightContentBundle,
    next_stream_position: InputStreamPosition,
    pending_commands: Vec<ScheduledCommand>,
    next_client_sequences: BTreeMap<PlayerId, ClientCommandSequence>,
    submissions: BTreeMap<(PlayerId, ClientCommandSequence), SubmissionRecord>,
    applied_sequences: BTreeSet<(PlayerId, ClientCommandSequence)>,
    history: Vec<CanonicalStreamRecord>,
    replay_initial_state: crate::SimulationSnapshot,
    replay_checkpoints: Vec<ReplayCheckpoint>,
    last_executions: Vec<CommandExecution>,
}

impl MatchDriver {
    #[must_use]
    pub fn new(simulation: &Simulation, content: &'static CastleFightContentBundle) -> Self {
        let next_client_sequences = simulation
            .players()
            .into_iter()
            .map(|player| (player.id, ClientCommandSequence(0)))
            .collect();
        let replay_initial_state = simulation.capture_snapshot();
        Self {
            content,
            next_stream_position: InputStreamPosition(0),
            pending_commands: Vec::new(),
            next_client_sequences,
            submissions: BTreeMap::new(),
            applied_sequences: BTreeSet::new(),
            history: Vec::new(),
            replay_initial_state,
            replay_checkpoints: Vec::new(),
            last_executions: Vec::new(),
        }
    }

    #[must_use]
    pub const fn content(&self) -> &'static CastleFightContentBundle {
        self.content
    }

    #[must_use]
    pub const fn next_stream_position(&self) -> InputStreamPosition {
        self.next_stream_position
    }

    #[must_use]
    pub fn pending_commands(&self) -> &[ScheduledCommand] {
        &self.pending_commands
    }

    #[must_use]
    pub fn history(&self) -> &[CanonicalStreamRecord] {
        &self.history
    }

    #[must_use]
    pub fn last_executions(&self) -> &[CommandExecution] {
        &self.last_executions
    }

    #[must_use]
    pub fn next_client_sequence(&self, player: PlayerId) -> Option<ClientCommandSequence> {
        self.next_client_sequences.get(&player).copied()
    }

    pub fn submit_local_command(
        &mut self,
        simulation: &Simulation,
        player: PlayerId,
        command: PlayerCommand,
    ) -> CommandSubmission {
        let Some(sequence) = self.next_client_sequence(player) else {
            return CommandSubmission::Rejected(CommandAdmissionError::UnknownPlayer);
        };
        self.submit_command(simulation, player, sequence, command)
            .expect("driver-generated local sequence must be contiguous and conflict-free")
    }

    pub fn submit_command(
        &mut self,
        simulation: &Simulation,
        player: PlayerId,
        sequence: ClientCommandSequence,
        command: PlayerCommand,
    ) -> Result<CommandSubmission, CommandSequenceError> {
        let Some(expected) = self.next_client_sequence(player) else {
            return Ok(CommandSubmission::Rejected(
                CommandAdmissionError::UnknownPlayer,
            ));
        };

        if let Some(previous) = self.submissions.get(&(player, sequence)) {
            if previous.command != command {
                return Err(CommandSequenceError::ConflictingDuplicate { player, sequence });
            }
            return Ok(match previous.disposition {
                StoredSubmissionDisposition::Scheduled(command) => {
                    CommandSubmission::DuplicateScheduled(command)
                }
                StoredSubmissionDisposition::Rejected(error) => {
                    CommandSubmission::DuplicateRejected(error)
                }
            });
        }
        if sequence.0 > expected.0 {
            return Err(CommandSequenceError::Gap {
                player,
                expected,
                actual: sequence,
            });
        }
        if sequence.0 < expected.0 {
            return Err(CommandSequenceError::Stale {
                player,
                expected,
                actual: sequence,
            });
        }

        let disposition = match admit_player_command(simulation, self.content, player, command) {
            Ok(()) => {
                let order = CommandOrder(
                    self.pending_commands
                        .len()
                        .try_into()
                        .expect("pending command count exceeds u32 command order space"),
                );
                let scheduled = ScheduledCommand {
                    tick: simulation.tick(),
                    order,
                    player,
                    client_sequence: sequence,
                    command,
                };
                self.pending_commands.push(scheduled);
                StoredSubmissionDisposition::Scheduled(scheduled)
            }
            Err(error) => StoredSubmissionDisposition::Rejected(error),
        };
        self.submissions.insert(
            (player, sequence),
            SubmissionRecord {
                command,
                disposition,
            },
        );
        self.next_client_sequences.insert(
            player,
            ClientCommandSequence(
                expected
                    .0
                    .checked_add(1)
                    .expect("client command sequence exhausted"),
            ),
        );

        Ok(match disposition {
            StoredSubmissionDisposition::Scheduled(command) => {
                CommandSubmission::Scheduled(command)
            }
            StoredSubmissionDisposition::Rejected(error) => CommandSubmission::Rejected(error),
        })
    }

    pub fn advance_local_tick(
        &mut self,
        simulation: &mut Simulation,
    ) -> Result<DriverTickResult, CanonicalStreamError> {
        if simulation.lifecycle() != MatchLifecycle::Running {
            return Err(CanonicalStreamError::MatchNotRunning(
                simulation.lifecycle(),
            ));
        }
        let finalized = FinalizedTickInputs {
            tick: simulation.tick(),
            stream_position: self.next_stream_position,
            commands: std::mem::take(&mut self.pending_commands),
        };
        let (executions, tick_result) = self.apply_finalized_tick(simulation, finalized.clone())?;
        Ok(DriverTickResult {
            finalized,
            executions,
            tick_result,
        })
    }

    pub fn apply_stream_record(
        &mut self,
        simulation: &mut Simulation,
        record: CanonicalStreamRecord,
    ) -> Result<Option<DriverTickResult>, CanonicalStreamError> {
        match record {
            CanonicalStreamRecord::Tick(finalized) => {
                let (executions, tick_result) =
                    self.apply_finalized_tick(simulation, finalized.clone())?;
                Ok(Some(DriverTickResult {
                    finalized,
                    executions,
                    tick_result,
                }))
            }
            CanonicalStreamRecord::Control(control) => {
                self.apply_boundary_control(simulation, control)?;
                Ok(None)
            }
        }
    }

    pub fn apply_finalized_tick(
        &mut self,
        simulation: &mut Simulation,
        finalized: FinalizedTickInputs,
    ) -> Result<(Vec<CommandExecution>, TickResult), CanonicalStreamError> {
        if !self.pending_commands.is_empty() {
            return Err(CanonicalStreamError::PendingLocalCommands);
        }
        self.validate_stream_position(finalized.stream_position)?;
        if simulation.lifecycle() != MatchLifecycle::Running {
            return Err(CanonicalStreamError::MatchNotRunning(
                simulation.lifecycle(),
            ));
        }
        if finalized.tick != simulation.tick() {
            return Err(CanonicalStreamError::UnexpectedTick {
                expected: simulation.tick(),
                actual: finalized.tick,
            });
        }
        let mut finalized_sequences = BTreeSet::new();
        for (index, command) in finalized.commands.iter().copied().enumerate() {
            if command.tick != finalized.tick {
                return Err(CanonicalStreamError::CommandTickMismatch {
                    expected: finalized.tick,
                    actual: command.tick,
                });
            }
            if simulation.player(command.player).is_none() {
                return Err(CanonicalStreamError::UnknownCommandPlayer(command.player));
            }
            let expected_order = CommandOrder(
                index
                    .try_into()
                    .expect("finalized command count exceeds u32 command order space"),
            );
            if command.order != expected_order {
                return Err(CanonicalStreamError::NonCanonicalCommandOrder {
                    expected: expected_order,
                    actual: command.order,
                });
            }
            let sequence_key = (command.player, command.client_sequence);
            if self.applied_sequences.contains(&sequence_key)
                || !finalized_sequences.insert(sequence_key)
            {
                return Err(CanonicalStreamError::DuplicateCommandSequence {
                    player: command.player,
                    sequence: command.client_sequence,
                });
            }
        }

        let mut executions = Vec::with_capacity(finalized.commands.len());
        for command in finalized.commands.iter().copied() {
            let outcome =
                execute_player_command(simulation, self.content, command.player, command.command);
            self.applied_sequences
                .insert((command.player, command.client_sequence));
            executions.push(CommandExecution {
                scheduled: command,
                outcome,
            });
        }
        let tick_result = simulation.step();
        self.last_executions.clone_from(&executions);
        self.replay_checkpoints.push(ReplayCheckpoint {
            stream_position: finalized.stream_position,
            completed_tick: Some(tick_result.completed_tick),
            checksum: tick_result.checksum,
        });
        self.history.push(CanonicalStreamRecord::Tick(finalized));
        self.advance_stream_position()?;
        Ok((executions, tick_result))
    }

    pub fn emit_local_control(
        &mut self,
        simulation: &mut Simulation,
        event: MatchControlEvent,
    ) -> Result<BoundaryControlRecord, CanonicalStreamError> {
        let control = BoundaryControlRecord {
            after_completed_tick: completed_tick_boundary(simulation),
            stream_position: self.next_stream_position,
            event,
        };
        self.apply_boundary_control(simulation, control)?;
        Ok(control)
    }

    pub fn apply_boundary_control(
        &mut self,
        simulation: &mut Simulation,
        control: BoundaryControlRecord,
    ) -> Result<(), CanonicalStreamError> {
        self.validate_stream_position(control.stream_position)?;
        let expected_boundary = completed_tick_boundary(simulation);
        if control.after_completed_tick != expected_boundary {
            return Err(CanonicalStreamError::BoundaryMismatch {
                expected: expected_boundary,
                actual: control.after_completed_tick,
            });
        }
        match control.event {
            MatchControlEvent::SetPlayerConnection { player, connection } => {
                if !simulation.set_player_connection_status(player, connection) {
                    if matches!(simulation.lifecycle(), MatchLifecycle::Finished { .. }) {
                        return Err(CanonicalStreamError::MatchAlreadyFinished);
                    }
                    return Err(CanonicalStreamError::UnknownControlPlayer(player));
                }
            }
            MatchControlEvent::FinishMatch { outcome } => {
                if !simulation.finish_match_from_control(outcome) {
                    return Err(CanonicalStreamError::MatchAlreadyFinished);
                }
                self.pending_commands.clear();
            }
        }
        self.last_executions.clear();
        self.replay_checkpoints.push(ReplayCheckpoint {
            stream_position: control.stream_position,
            completed_tick: expected_boundary,
            checksum: simulation.checksum(),
        });
        self.history.push(CanonicalStreamRecord::Control(control));
        self.advance_stream_position()?;
        Ok(())
    }

    fn validate_stream_position(
        &self,
        actual: InputStreamPosition,
    ) -> Result<(), CanonicalStreamError> {
        if actual == self.next_stream_position {
            Ok(())
        } else {
            Err(CanonicalStreamError::UnexpectedStreamPosition {
                expected: self.next_stream_position,
                actual,
            })
        }
    }

    fn advance_stream_position(&mut self) -> Result<(), CanonicalStreamError> {
        self.next_stream_position = InputStreamPosition(
            self.next_stream_position
                .0
                .checked_add(1)
                .ok_or(CanonicalStreamError::StreamPositionExhausted)?,
        );
        Ok(())
    }
}

fn completed_tick_boundary(simulation: &Simulation) -> Option<u64> {
    simulation.tick().checked_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BuilderBuildError, BuildingPlacementError, CastleFightBuilderRace, CastleFightMatchConfig,
        CastleFightParticipantConfig, CastleFightProductionKind, CommandRejectReason, MapVersion,
        Team, create_castle_fight_match,
    };

    fn match_927(workers: usize) -> crate::CastleFightMatch {
        create_castle_fight_match(
            CastleFightMatchConfig::development_subset(
                MapVersion::CASTLE_FIGHT_9_27,
                "r1",
                0x4452_4956_4552,
            )
            .unwrap(),
            workers,
        )
        .unwrap()
    }

    #[test]
    fn empty_tick_is_an_explicit_canonical_stream_record() {
        let mut game = match_927(1);
        let mut driver = MatchDriver::new(&game.simulation, game.content);
        let result = driver.advance_local_tick(&mut game.simulation).unwrap();
        assert_eq!(result.finalized.tick, 0);
        assert_eq!(result.finalized.stream_position, InputStreamPosition(0));
        assert!(result.finalized.commands.is_empty());
        assert!(result.executions.is_empty());
        assert_eq!(game.simulation.tick(), 1);
        assert_eq!(driver.next_stream_position(), InputStreamPosition(1));
        assert_eq!(driver.history().len(), 1);
    }

    #[test]
    fn duplicate_submission_cannot_double_execute_or_double_charge() {
        let mut game = match_927(1);
        let mut driver = MatchDriver::new(&game.simulation, game.content);
        let builder = game
            .simulation
            .builder_for_player(PlayerId(0))
            .expect("western builder");
        let command = PlayerCommand::PlaceBuilding {
            builder: builder.id,
            building: CastleFightProductionKind::Barracks.stable_id(),
            position: crate::BuildPosition::new(-138, 2),
        };
        let sequence = ClientCommandSequence(0);
        let first = driver
            .submit_command(&game.simulation, PlayerId(0), sequence, command)
            .unwrap();
        let duplicate = driver
            .submit_command(&game.simulation, PlayerId(0), sequence, command)
            .unwrap();
        assert!(first.is_newly_scheduled());
        assert!(matches!(
            duplicate,
            CommandSubmission::DuplicateScheduled(_)
        ));
        assert_eq!(driver.pending_commands().len(), 1);

        let result = driver.advance_local_tick(&mut game.simulation).unwrap();
        assert_eq!(result.executions.len(), 1);
        assert_eq!(
            game.simulation
                .player_resources_for(PlayerId(0))
                .unwrap()
                .gold,
            150
        );
        let retry_after_execution = driver
            .submit_command(&game.simulation, PlayerId(0), sequence, command)
            .unwrap();
        assert!(matches!(
            retry_after_execution,
            CommandSubmission::DuplicateScheduled(_)
        ));
        assert!(driver.pending_commands().is_empty());
        assert_eq!(
            game.simulation
                .player_resources_for(PlayerId(0))
                .unwrap()
                .gold,
            150,
            "retrying an already-executed sequence must not charge again"
        );
    }

    #[test]
    fn same_tick_build_contention_is_canonically_ordered_and_atomic() {
        let config = CastleFightMatchConfig::development_subset_with_participants(
            MapVersion::CASTLE_FIGHT_9_27,
            "r1",
            0x0043_4f4e_5445_4e44,
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
        let mut game = create_castle_fight_match(config, 1).unwrap();
        let mut driver = MatchDriver::new(&game.simulation, game.content);
        let first_builder = game.simulation.builder_for_player(PlayerId(0)).unwrap();
        let second_builder = game.simulation.builder_for_player(PlayerId(1)).unwrap();
        let building = CastleFightProductionKind::Barracks.stable_id();
        let position = crate::BuildPosition::new(-138, 2);
        let first = PlayerCommand::PlaceBuilding {
            builder: first_builder.id,
            building,
            position,
        };
        let second = PlayerCommand::PlaceBuilding {
            builder: second_builder.id,
            building,
            position,
        };
        assert!(
            driver
                .submit_local_command(&game.simulation, PlayerId(0), first)
                .is_newly_scheduled()
        );
        assert!(
            driver
                .submit_local_command(&game.simulation, PlayerId(1), second)
                .is_newly_scheduled()
        );
        let second_resources_before = game.simulation.player_resources_for(PlayerId(1)).unwrap();

        let result = driver.advance_local_tick(&mut game.simulation).unwrap();
        assert_eq!(result.executions.len(), 2);
        assert_eq!(
            result.executions[0].outcome,
            CommandOutcome::Executed(crate::CommandExecutionResult::Applied)
        );
        assert_eq!(
            result.executions[1].outcome,
            CommandOutcome::Rejected(CommandRejectReason::Build(BuilderBuildError::Placement(
                BuildingPlacementError::BuildingReserved,
            )))
        );
        assert_eq!(
            game.simulation
                .player_resources_for(PlayerId(0))
                .unwrap()
                .gold,
            150
        );
        assert_eq!(
            game.simulation.player_resources_for(PlayerId(1)).unwrap(),
            second_resources_before,
            "losing contention must not spend any resources"
        );
    }

    #[test]
    fn unknown_player_in_finalized_tick_is_rejected_before_execution() {
        let mut game = match_927(1);
        let mut driver = MatchDriver::new(&game.simulation, game.content);
        let builder = game
            .simulation
            .builder_for_player(PlayerId(0))
            .expect("western builder");
        let finalized = FinalizedTickInputs {
            tick: 0,
            stream_position: InputStreamPosition(0),
            commands: vec![ScheduledCommand {
                tick: 0,
                order: CommandOrder(0),
                player: PlayerId(99),
                client_sequence: ClientCommandSequence(0),
                command: PlayerCommand::MoveBuilder {
                    builder: builder.id,
                    destination: builder.position,
                },
            }],
        };
        assert_eq!(
            driver.apply_finalized_tick(&mut game.simulation, finalized),
            Err(CanonicalStreamError::UnknownCommandPlayer(PlayerId(99)))
        );
        assert_eq!(game.simulation.tick(), 0);
    }

    #[test]
    fn duplicate_sequence_inside_one_finalized_tick_is_rejected_before_any_execution() {
        let mut game = match_927(1);
        let mut driver = MatchDriver::new(&game.simulation, game.content);
        let builder = game
            .simulation
            .builder_for_player(PlayerId(0))
            .expect("western builder");
        let command = PlayerCommand::PlaceBuilding {
            builder: builder.id,
            building: CastleFightProductionKind::Barracks.stable_id(),
            position: crate::BuildPosition::new(-138, 2),
        };
        let sequence = ClientCommandSequence(0);
        let finalized = FinalizedTickInputs {
            tick: 0,
            stream_position: InputStreamPosition(0),
            commands: vec![
                ScheduledCommand {
                    tick: 0,
                    order: CommandOrder(0),
                    player: PlayerId(0),
                    client_sequence: sequence,
                    command,
                },
                ScheduledCommand {
                    tick: 0,
                    order: CommandOrder(1),
                    player: PlayerId(0),
                    client_sequence: sequence,
                    command,
                },
            ],
        };
        let resources_before = game.simulation.player_resources_for(PlayerId(0)).unwrap();
        assert_eq!(
            driver.apply_finalized_tick(&mut game.simulation, finalized),
            Err(CanonicalStreamError::DuplicateCommandSequence {
                player: PlayerId(0),
                sequence,
            })
        );
        assert_eq!(game.simulation.tick(), 0);
        assert_eq!(
            game.simulation.player_resources_for(PlayerId(0)).unwrap(),
            resources_before,
            "bundle validation must fail before the first duplicate mutates state"
        );
    }

    #[test]
    fn conflicting_duplicate_and_sequence_gap_are_rejected_before_scheduling() {
        let game = match_927(1);
        let mut driver = MatchDriver::new(&game.simulation, game.content);
        let builder = game
            .simulation
            .builder_for_player(PlayerId(0))
            .expect("western builder");
        let first = PlayerCommand::MoveBuilder {
            builder: builder.id,
            destination: builder.position,
        };
        driver
            .submit_command(
                &game.simulation,
                PlayerId(0),
                ClientCommandSequence(0),
                first,
            )
            .unwrap();
        let conflicting = PlayerCommand::StopBuilder {
            builder: builder.id,
        };
        assert_eq!(
            driver.submit_command(
                &game.simulation,
                PlayerId(0),
                ClientCommandSequence(0),
                conflicting,
            ),
            Err(CommandSequenceError::ConflictingDuplicate {
                player: PlayerId(0),
                sequence: ClientCommandSequence(0),
            })
        );
        assert_eq!(
            driver.submit_command(
                &game.simulation,
                PlayerId(0),
                ClientCommandSequence(2),
                conflicting,
            ),
            Err(CommandSequenceError::Gap {
                player: PlayerId(0),
                expected: ClientCommandSequence(1),
                actual: ClientCommandSequence(2),
            })
        );
        assert_eq!(driver.pending_commands().len(), 1);
    }

    #[test]
    fn same_finalized_inputs_produce_same_outcomes_and_state_across_workers() {
        let source = match_927(1);
        let mut source_driver = MatchDriver::new(&source.simulation, source.content);
        let builder = source
            .simulation
            .builder_for_player(PlayerId(0))
            .expect("western builder");
        source_driver.submit_local_command(
            &source.simulation,
            PlayerId(0),
            PlayerCommand::MoveBuilder {
                builder: builder.id,
                destination: crate::SimPoint::new(builder.position.x + 64, builder.position.y),
            },
        );
        let finalized = FinalizedTickInputs {
            tick: 0,
            stream_position: InputStreamPosition(0),
            commands: source_driver.pending_commands().to_vec(),
        };

        let mut first = match_927(1);
        let mut second = match_927(4);
        let mut first_driver = MatchDriver::new(&first.simulation, first.content);
        let mut second_driver = MatchDriver::new(&second.simulation, second.content);
        let (first_outcomes, _) = first_driver
            .apply_finalized_tick(&mut first.simulation, finalized.clone())
            .unwrap();
        let (second_outcomes, _) = second_driver
            .apply_finalized_tick(&mut second.simulation, finalized)
            .unwrap();
        assert_eq!(first_outcomes, second_outcomes);
        assert_eq!(first.simulation.checksum(), second.simulation.checksum());
    }

    #[test]
    fn terminal_control_can_finish_a_paused_match_without_advancing_ticks() {
        let config = CastleFightMatchConfig::development_subset_with_participants(
            MapVersion::CASTLE_FIGHT_9_27,
            "r1",
            0x5041_5553_4545_4e44,
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
        let mut game = create_castle_fight_match(config, 1).unwrap();
        let mut driver = MatchDriver::new(&game.simulation, game.content);
        let builder = game.simulation.builder_for_player(PlayerId(0)).unwrap();
        assert!(
            driver
                .submit_local_command(
                    &game.simulation,
                    PlayerId(0),
                    PlayerCommand::MoveBuilder {
                        builder: builder.id,
                        destination: builder.position,
                    },
                )
                .is_newly_scheduled()
        );
        assert_eq!(driver.pending_commands().len(), 1);
        for player in [PlayerId(0), PlayerId(1)] {
            driver
                .emit_local_control(
                    &mut game.simulation,
                    MatchControlEvent::SetPlayerConnection {
                        player,
                        connection: PlayerConnectionStatus::Disconnected,
                    },
                )
                .unwrap();
        }
        assert!(matches!(
            game.simulation.lifecycle(),
            MatchLifecycle::PausedForDisconnect { .. }
        ));
        driver
            .emit_local_control(
                &mut game.simulation,
                MatchControlEvent::FinishMatch {
                    outcome: MatchOutcome::Victory(Team(1)),
                },
            )
            .unwrap();
        assert_eq!(game.simulation.tick(), 0);
        assert!(
            driver.pending_commands().is_empty(),
            "terminal controls discard admitted commands that can no longer execute"
        );
        assert_eq!(
            game.simulation.lifecycle(),
            MatchLifecycle::Finished {
                outcome: MatchOutcome::Victory(Team(1)),
                finished_tick: 0,
            }
        );
    }

    #[test]
    fn boundary_controls_resume_a_paused_match_without_advancing_ticks() {
        let config = CastleFightMatchConfig::development_subset_with_participants(
            MapVersion::CASTLE_FIGHT_9_27,
            "r1",
            99,
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
        let mut game = create_castle_fight_match(config, 1).unwrap();
        let mut driver = MatchDriver::new(&game.simulation, game.content);
        driver
            .emit_local_control(
                &mut game.simulation,
                MatchControlEvent::SetPlayerConnection {
                    player: PlayerId(0),
                    connection: PlayerConnectionStatus::Disconnected,
                },
            )
            .unwrap();
        driver
            .emit_local_control(
                &mut game.simulation,
                MatchControlEvent::SetPlayerConnection {
                    player: PlayerId(1),
                    connection: PlayerConnectionStatus::Disconnected,
                },
            )
            .unwrap();
        assert!(matches!(
            game.simulation.lifecycle(),
            MatchLifecycle::PausedForDisconnect { .. }
        ));
        assert_eq!(game.simulation.tick(), 0);
        assert!(matches!(
            driver.advance_local_tick(&mut game.simulation),
            Err(CanonicalStreamError::MatchNotRunning(
                MatchLifecycle::PausedForDisconnect { .. }
            ))
        ));
        driver
            .emit_local_control(
                &mut game.simulation,
                MatchControlEvent::SetPlayerConnection {
                    player: PlayerId(1),
                    connection: PlayerConnectionStatus::Connected,
                },
            )
            .unwrap();
        assert_eq!(game.simulation.lifecycle(), MatchLifecycle::Running);
        assert_eq!(game.simulation.tick(), 0);
        assert_eq!(driver.next_stream_position(), InputStreamPosition(3));
    }
}
