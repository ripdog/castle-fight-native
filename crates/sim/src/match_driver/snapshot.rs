use super::*;
use crate::{CastleFightContentIdentity, SimulationSnapshot, SnapshotRestoreError};

pub const MATCH_DRIVER_SNAPSHOT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct MatchDriverSnapshot {
    schema_version: u32,
    content_identity: CastleFightContentIdentity,
    simulation: SimulationSnapshot,
    next_stream_position: InputStreamPosition,
    pending_commands: Vec<ScheduledCommand>,
    next_client_sequences: BTreeMap<PlayerId, ClientCommandSequence>,
    submissions: BTreeMap<(PlayerId, ClientCommandSequence), SubmissionRecord>,
    applied_sequences: BTreeSet<(PlayerId, ClientCommandSequence)>,
    history: Vec<CanonicalStreamRecord>,
    replay_initial_state: SimulationSnapshot,
    replay_checkpoints: Vec<ReplayCheckpoint>,
}

impl MatchDriverSnapshot {
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    #[must_use]
    pub const fn next_stream_position(&self) -> InputStreamPosition {
        self.next_stream_position
    }

    #[must_use]
    pub const fn simulation(&self) -> &SimulationSnapshot {
        &self.simulation
    }

    #[must_use]
    pub fn pending_commands(&self) -> &[ScheduledCommand] {
        &self.pending_commands
    }

    #[must_use]
    pub fn history(&self) -> &[CanonicalStreamRecord] {
        &self.history
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchSnapshotRestoreError {
    SchemaMismatch {
        expected: u32,
        actual: u32,
    },
    ContentMismatch {
        expected: CastleFightContentIdentity,
        actual: CastleFightContentIdentity,
    },
    Simulation(SnapshotRestoreError),
    HistoryLengthMismatch {
        expected: u64,
        actual: usize,
    },
    UnexpectedHistoryPosition {
        expected: InputStreamPosition,
        actual: InputStreamPosition,
    },
    PlayerSequenceStateMismatch,
    PendingCommandTickMismatch {
        expected: u64,
        actual: u64,
    },
    NonCanonicalPendingOrder {
        expected: CommandOrder,
        actual: CommandOrder,
    },
    MissingPendingSubmission {
        player: PlayerId,
        sequence: ClientCommandSequence,
    },
    AppliedSequenceMismatch,
    ReplayInitialConfigurationMismatch,
    ReplayCheckpointCountMismatch {
        history: usize,
        checkpoints: usize,
    },
    ReplayCheckpointMismatch {
        expected_position: InputStreamPosition,
        actual_position: InputStreamPosition,
    },
    ReplayBoundaryStateMismatch,
}

impl From<SnapshotRestoreError> for MatchSnapshotRestoreError {
    fn from(value: SnapshotRestoreError) -> Self {
        Self::Simulation(value)
    }
}

impl MatchDriver {
    #[must_use]
    pub fn capture_snapshot(&self, simulation: &Simulation) -> MatchDriverSnapshot {
        MatchDriverSnapshot {
            schema_version: MATCH_DRIVER_SNAPSHOT_SCHEMA_VERSION,
            content_identity: self.content.identity,
            simulation: simulation.capture_snapshot(),
            next_stream_position: self.next_stream_position,
            pending_commands: self.pending_commands.clone(),
            next_client_sequences: self.next_client_sequences.clone(),
            submissions: self.submissions.clone(),
            applied_sequences: self.applied_sequences.clone(),
            history: self.history.clone(),
            replay_initial_state: self.replay_initial_state.clone(),
            replay_checkpoints: self.replay_checkpoints.clone(),
        }
    }

    pub fn restore_snapshot(
        &mut self,
        simulation: &mut Simulation,
        snapshot: &MatchDriverSnapshot,
    ) -> Result<(), MatchSnapshotRestoreError> {
        validate_driver_snapshot(self, simulation, snapshot)?;
        simulation.restore_snapshot(&snapshot.simulation)?;

        self.next_stream_position = snapshot.next_stream_position;
        self.pending_commands.clone_from(&snapshot.pending_commands);
        self.next_client_sequences
            .clone_from(&snapshot.next_client_sequences);
        self.submissions.clone_from(&snapshot.submissions);
        self.applied_sequences
            .clone_from(&snapshot.applied_sequences);
        self.history.clone_from(&snapshot.history);
        self.replay_initial_state
            .clone_from(&snapshot.replay_initial_state);
        self.replay_checkpoints
            .clone_from(&snapshot.replay_checkpoints);
        self.last_executions.clear();
        Ok(())
    }
}

fn validate_driver_snapshot(
    driver: &MatchDriver,
    simulation: &Simulation,
    snapshot: &MatchDriverSnapshot,
) -> Result<(), MatchSnapshotRestoreError> {
    if snapshot.schema_version != MATCH_DRIVER_SNAPSHOT_SCHEMA_VERSION {
        return Err(MatchSnapshotRestoreError::SchemaMismatch {
            expected: MATCH_DRIVER_SNAPSHOT_SCHEMA_VERSION,
            actual: snapshot.schema_version,
        });
    }
    if snapshot.content_identity != driver.content.identity {
        return Err(MatchSnapshotRestoreError::ContentMismatch {
            expected: driver.content.identity,
            actual: snapshot.content_identity,
        });
    }
    let expected_history_len = snapshot.next_stream_position.0;
    if u64::try_from(snapshot.history.len()).ok() != Some(expected_history_len) {
        return Err(MatchSnapshotRestoreError::HistoryLengthMismatch {
            expected: expected_history_len,
            actual: snapshot.history.len(),
        });
    }
    for (index, record) in snapshot.history.iter().enumerate() {
        let expected = InputStreamPosition(
            u64::try_from(index).expect("history length was already validated against u64"),
        );
        let actual = stream_position(record);
        if actual != expected {
            return Err(MatchSnapshotRestoreError::UnexpectedHistoryPosition { expected, actual });
        }
    }
    if snapshot.replay_initial_state.configuration_identity()
        != snapshot.simulation.configuration_identity()
    {
        return Err(MatchSnapshotRestoreError::ReplayInitialConfigurationMismatch);
    }
    if snapshot.replay_checkpoints.len() != snapshot.history.len() {
        return Err(MatchSnapshotRestoreError::ReplayCheckpointCountMismatch {
            history: snapshot.history.len(),
            checkpoints: snapshot.replay_checkpoints.len(),
        });
    }
    for (index, (record, checkpoint)) in snapshot
        .history
        .iter()
        .zip(&snapshot.replay_checkpoints)
        .enumerate()
    {
        let expected_position = InputStreamPosition(
            u64::try_from(index).expect("checkpoint count was already validated against u64"),
        );
        let expected_boundary = match record {
            CanonicalStreamRecord::Tick(tick) => Some(tick.tick),
            CanonicalStreamRecord::Control(control) => control.after_completed_tick,
        };
        if checkpoint.stream_position != expected_position
            || checkpoint.completed_tick != expected_boundary
        {
            return Err(MatchSnapshotRestoreError::ReplayCheckpointMismatch {
                expected_position,
                actual_position: checkpoint.stream_position,
            });
        }
    }
    match snapshot.replay_checkpoints.last() {
        Some(checkpoint)
            if checkpoint.completed_tick != snapshot.simulation.completed_tick()
                || checkpoint.checksum != snapshot.simulation.checksum() =>
        {
            return Err(MatchSnapshotRestoreError::ReplayBoundaryStateMismatch);
        }
        None if snapshot.replay_initial_state.completed_tick()
            != snapshot.simulation.completed_tick()
            || snapshot.replay_initial_state.checksum() != snapshot.simulation.checksum() =>
        {
            return Err(MatchSnapshotRestoreError::ReplayBoundaryStateMismatch);
        }
        Some(_) | None => {}
    }

    let current_players: BTreeSet<_> = simulation
        .players()
        .into_iter()
        .map(|player| player.id)
        .collect();
    let sequence_players: BTreeSet<_> = snapshot.next_client_sequences.keys().copied().collect();
    if current_players != sequence_players {
        return Err(MatchSnapshotRestoreError::PlayerSequenceStateMismatch);
    }
    for &(player, sequence) in snapshot.submissions.keys() {
        let Some(next) = snapshot.next_client_sequences.get(&player) else {
            return Err(MatchSnapshotRestoreError::PlayerSequenceStateMismatch);
        };
        if sequence.0 >= next.0 {
            return Err(MatchSnapshotRestoreError::PlayerSequenceStateMismatch);
        }
    }

    let pending_tick = snapshot.simulation.next_tick();
    for (index, command) in snapshot.pending_commands.iter().copied().enumerate() {
        if command.tick != pending_tick {
            return Err(MatchSnapshotRestoreError::PendingCommandTickMismatch {
                expected: pending_tick,
                actual: command.tick,
            });
        }
        let expected = CommandOrder(
            u32::try_from(index).expect("pending command count exceeds u32 command order space"),
        );
        if command.order != expected {
            return Err(MatchSnapshotRestoreError::NonCanonicalPendingOrder {
                expected,
                actual: command.order,
            });
        }
        let Some(submission) = snapshot
            .submissions
            .get(&(command.player, command.client_sequence))
        else {
            return Err(MatchSnapshotRestoreError::MissingPendingSubmission {
                player: command.player,
                sequence: command.client_sequence,
            });
        };
        if submission.command != command.command
            || submission.disposition != StoredSubmissionDisposition::Scheduled(command)
        {
            return Err(MatchSnapshotRestoreError::MissingPendingSubmission {
                player: command.player,
                sequence: command.client_sequence,
            });
        }
        if snapshot
            .applied_sequences
            .contains(&(command.player, command.client_sequence))
        {
            return Err(MatchSnapshotRestoreError::AppliedSequenceMismatch);
        }
    }

    let history_sequence_count = snapshot
        .history
        .iter()
        .filter_map(|record| match record {
            CanonicalStreamRecord::Tick(tick) => Some(tick.commands.len()),
            CanonicalStreamRecord::Control(_) => None,
        })
        .sum::<usize>();
    let history_sequences: BTreeSet<_> = snapshot
        .history
        .iter()
        .filter_map(|record| match record {
            CanonicalStreamRecord::Tick(tick) => Some(tick.commands.iter()),
            CanonicalStreamRecord::Control(_) => None,
        })
        .flatten()
        .map(|command| (command.player, command.client_sequence))
        .collect();
    if history_sequences.len() != history_sequence_count
        || history_sequences != snapshot.applied_sequences
    {
        return Err(MatchSnapshotRestoreError::AppliedSequenceMismatch);
    }
    Ok(())
}

fn stream_position(record: &CanonicalStreamRecord) -> InputStreamPosition {
    match record {
        CanonicalStreamRecord::Tick(tick) => tick.stream_position,
        CanonicalStreamRecord::Control(control) => control.stream_position,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BuildPosition, CastleFightMatchConfig, CastleFightProductionKind, MapVersion,
        PlayerCommand, PlayerConnectionStatus, create_castle_fight_match,
    };

    fn match_927(workers: usize) -> crate::CastleFightMatch {
        create_castle_fight_match(
            CastleFightMatchConfig::development_subset(
                MapVersion::CASTLE_FIGHT_9_27,
                "r1",
                0x534e_4150_4452_4956,
            )
            .unwrap(),
            workers,
        )
        .unwrap()
    }

    #[test]
    fn snapshot_preserves_admitted_future_command_and_duplicate_state() {
        let mut original = match_927(1);
        let mut original_driver = MatchDriver::new(&original.simulation, original.content);
        let builder = original
            .simulation
            .builder_for_player(PlayerId(0))
            .expect("western builder");
        let command = PlayerCommand::PlaceBuilding {
            builder: builder.id,
            building: CastleFightProductionKind::Barracks.stable_id(),
            position: BuildPosition::new(-138, 2),
        };
        let sequence = ClientCommandSequence(0);
        let scheduled = original_driver
            .submit_command(&original.simulation, PlayerId(0), sequence, command)
            .unwrap();
        assert!(scheduled.is_newly_scheduled());
        let snapshot = original_driver.capture_snapshot(&original.simulation);
        assert_eq!(snapshot.pending_commands().len(), 1);

        let mut restored = match_927(4);
        let mut restored_driver = MatchDriver::new(&restored.simulation, restored.content);
        restored_driver
            .restore_snapshot(&mut restored.simulation, &snapshot)
            .unwrap();
        assert_eq!(
            restored.simulation.checksum(),
            original.simulation.checksum()
        );
        assert_eq!(
            restored_driver.next_stream_position(),
            InputStreamPosition(0)
        );

        let retry = restored_driver
            .submit_command(&restored.simulation, PlayerId(0), sequence, command)
            .unwrap();
        assert!(matches!(retry, CommandSubmission::DuplicateScheduled(_)));
        assert_eq!(restored_driver.pending_commands().len(), 1);

        let original_result = original_driver
            .advance_local_tick(&mut original.simulation)
            .unwrap();
        let restored_result = restored_driver
            .advance_local_tick(&mut restored.simulation)
            .unwrap();
        assert_eq!(original_result.executions, restored_result.executions);
        assert_eq!(
            original_result.tick_result.checksum,
            restored_result.tick_result.checksum
        );
        assert_eq!(
            original.simulation.player_resources_for(PlayerId(0)),
            restored.simulation.player_resources_for(PlayerId(0))
        );

        let retry_after_execution = restored_driver
            .submit_command(&restored.simulation, PlayerId(0), sequence, command)
            .unwrap();
        assert!(matches!(
            retry_after_execution,
            CommandSubmission::DuplicateScheduled(_)
        ));
        assert!(restored_driver.pending_commands().is_empty());
    }

    #[test]
    fn snapshot_restores_paused_boundary_and_control_stream_position() {
        let mut original = match_927(1);
        let mut original_driver = MatchDriver::new(&original.simulation, original.content);
        original_driver
            .emit_local_control(
                &mut original.simulation,
                MatchControlEvent::SetPlayerConnection {
                    player: PlayerId(0),
                    connection: PlayerConnectionStatus::Disconnected,
                },
            )
            .unwrap();
        assert!(matches!(
            original.simulation.lifecycle(),
            MatchLifecycle::PausedForDisconnect { .. }
        ));
        let snapshot = original_driver.capture_snapshot(&original.simulation);

        let mut restored = match_927(3);
        let mut restored_driver = MatchDriver::new(&restored.simulation, restored.content);
        restored_driver
            .restore_snapshot(&mut restored.simulation, &snapshot)
            .unwrap();
        assert_eq!(
            restored.simulation.lifecycle(),
            original.simulation.lifecycle()
        );
        assert_eq!(
            restored_driver.next_stream_position(),
            InputStreamPosition(1)
        );
        assert_eq!(restored_driver.history(), original_driver.history());

        let resume = MatchControlEvent::SetPlayerConnection {
            player: PlayerId(0),
            connection: PlayerConnectionStatus::Connected,
        };
        let original_control = original_driver
            .emit_local_control(&mut original.simulation, resume)
            .unwrap();
        let restored_control = restored_driver
            .emit_local_control(&mut restored.simulation, resume)
            .unwrap();
        assert_eq!(original_control, restored_control);
        assert_eq!(restored.simulation.lifecycle(), MatchLifecycle::Running);
        assert_eq!(
            restored.simulation.checksum(),
            original.simulation.checksum()
        );
    }
}
