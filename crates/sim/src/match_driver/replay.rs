use super::*;
use crate::{
    AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION, CANONICAL_CHECKSUM_SCHEMA_VERSION,
    CastleFightContentIdentity, MapVersion, SimulationSnapshot, SnapshotRestoreError,
};

pub const MATCH_REPLAY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayHeader {
    pub schema_version: u32,
    pub snapshot_schema_version: u32,
    pub checksum_schema_version: u32,
    pub map_version: MapVersion,
    pub release_revision: String,
    pub content_identity: CastleFightContentIdentity,
    pub configuration_identity: u64,
    pub initial_completed_tick: Option<u64>,
    pub initial_checksum: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayCheckpoint {
    pub stream_position: InputStreamPosition,
    pub completed_tick: Option<u64>,
    pub checksum: u64,
}

#[derive(Debug, Clone)]
pub struct ReplaySeekSnapshot {
    snapshot: MatchDriverSnapshot,
}

impl ReplaySeekSnapshot {
    #[must_use]
    pub const fn next_stream_position(&self) -> InputStreamPosition {
        self.snapshot.next_stream_position()
    }

    #[must_use]
    pub const fn snapshot(&self) -> &MatchDriverSnapshot {
        &self.snapshot
    }
}

#[derive(Debug, Clone)]
pub struct MatchReplay {
    header: ReplayHeader,
    initial_state: SimulationSnapshot,
    records: Vec<CanonicalStreamRecord>,
    checkpoints: Vec<ReplayCheckpoint>,
    seek_snapshots: Vec<ReplaySeekSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayError {
    SchemaMismatch {
        expected: u32,
        actual: u32,
    },
    SnapshotSchemaMismatch {
        expected: u32,
        actual: u32,
    },
    ChecksumSchemaMismatch {
        expected: u32,
        actual: u32,
    },
    ContentMismatch {
        expected: CastleFightContentIdentity,
        actual: CastleFightContentIdentity,
    },
    ReleaseMismatch {
        expected_map_version: MapVersion,
        actual_map_version: MapVersion,
        expected_revision: String,
        actual_revision: String,
    },
    ConfigurationMismatch {
        expected: u64,
        actual: u64,
    },
    InitialSnapshotMismatch,
    CheckpointCountMismatch {
        records: usize,
        checkpoints: usize,
    },
    CheckpointPositionMismatch {
        expected: InputStreamPosition,
        actual: InputStreamPosition,
    },
    CheckpointBoundaryMismatch {
        stream_position: InputStreamPosition,
        expected: Option<u64>,
        actual: Option<u64>,
    },
    CheckpointChecksumMismatch {
        stream_position: InputStreamPosition,
        expected: u64,
        actual: u64,
    },
    TargetBeyondHistory {
        target: InputStreamPosition,
        available: InputStreamPosition,
    },
    SeekSnapshotHasPendingCommands,
    SeekSnapshotBeyondHistory {
        position: InputStreamPosition,
        available: InputStreamPosition,
    },
    SeekSnapshotChecksumMismatch {
        position: InputStreamPosition,
        expected: u64,
        actual: u64,
    },
    Snapshot(SnapshotRestoreError),
    DriverSnapshot(MatchSnapshotRestoreError),
    Stream(CanonicalStreamError),
}

impl From<SnapshotRestoreError> for ReplayError {
    fn from(value: SnapshotRestoreError) -> Self {
        Self::Snapshot(value)
    }
}

impl From<MatchSnapshotRestoreError> for ReplayError {
    fn from(value: MatchSnapshotRestoreError) -> Self {
        Self::DriverSnapshot(value)
    }
}

impl From<CanonicalStreamError> for ReplayError {
    fn from(value: CanonicalStreamError) -> Self {
        Self::Stream(value)
    }
}

impl MatchReplay {
    #[must_use]
    pub const fn header(&self) -> &ReplayHeader {
        &self.header
    }

    #[must_use]
    pub fn records(&self) -> &[CanonicalStreamRecord] {
        &self.records
    }

    #[must_use]
    pub fn checkpoints(&self) -> &[ReplayCheckpoint] {
        &self.checkpoints
    }

    #[must_use]
    pub fn seek_snapshots(&self) -> &[ReplaySeekSnapshot] {
        &self.seek_snapshots
    }

    pub fn add_seek_snapshot(&mut self, snapshot: MatchDriverSnapshot) -> Result<(), ReplayError> {
        if !snapshot.pending_commands().is_empty() {
            return Err(ReplayError::SeekSnapshotHasPendingCommands);
        }
        let position = snapshot.next_stream_position();
        let available = self.end_stream_position();
        if position.0 > available.0 {
            return Err(ReplayError::SeekSnapshotBeyondHistory {
                position,
                available,
            });
        }
        let expected_checksum = if position.0 == 0 {
            self.initial_state.checksum()
        } else {
            let index = usize::try_from(position.0 - 1)
                .expect("validated replay stream position must fit record index");
            self.checkpoints[index].checksum
        };
        let actual_checksum = snapshot.simulation().checksum();
        if expected_checksum != actual_checksum {
            return Err(ReplayError::SeekSnapshotChecksumMismatch {
                position,
                expected: expected_checksum,
                actual: actual_checksum,
            });
        }
        self.seek_snapshots.push(ReplaySeekSnapshot { snapshot });
        self.seek_snapshots
            .sort_by_key(ReplaySeekSnapshot::next_stream_position);
        self.seek_snapshots
            .dedup_by_key(|snapshot| snapshot.next_stream_position());
        Ok(())
    }

    pub fn play_to_end(
        &self,
        simulation: &mut Simulation,
        driver: &mut MatchDriver,
    ) -> Result<(), ReplayError> {
        self.play_to_stream_position(self.end_stream_position(), simulation, driver)
    }

    pub fn play_to_stream_position(
        &self,
        target: InputStreamPosition,
        simulation: &mut Simulation,
        driver: &mut MatchDriver,
    ) -> Result<(), ReplayError> {
        self.validate_for(driver)?;
        let available = self.end_stream_position();
        if target.0 > available.0 {
            return Err(ReplayError::TargetBeyondHistory { target, available });
        }

        let seek = self
            .seek_snapshots
            .iter()
            .rev()
            .find(|seek| seek.next_stream_position().0 <= target.0);
        let start_position = if let Some(seek) = seek {
            driver.restore_snapshot(simulation, seek.snapshot())?;
            seek.next_stream_position()
        } else {
            simulation.restore_snapshot(&self.initial_state)?;
            driver.reset_for_replay(simulation, &self.initial_state);
            InputStreamPosition(0)
        };

        for position in start_position.0..target.0 {
            let index = usize::try_from(position)
                .expect("validated replay stream position must fit record index");
            driver.apply_stream_record(simulation, self.records[index].clone())?;
            self.verify_checkpoint(index, simulation)?;
        }
        simulation.clear_presentation_events();
        Ok(())
    }

    #[must_use]
    pub fn end_stream_position(&self) -> InputStreamPosition {
        InputStreamPosition(
            u64::try_from(self.records.len()).expect("replay history length exceeds u64"),
        )
    }

    fn validate_for(&self, driver: &MatchDriver) -> Result<(), ReplayError> {
        if self.header.schema_version != MATCH_REPLAY_SCHEMA_VERSION {
            return Err(ReplayError::SchemaMismatch {
                expected: MATCH_REPLAY_SCHEMA_VERSION,
                actual: self.header.schema_version,
            });
        }
        if self.header.snapshot_schema_version != AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION {
            return Err(ReplayError::SnapshotSchemaMismatch {
                expected: AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION,
                actual: self.header.snapshot_schema_version,
            });
        }
        if self.header.checksum_schema_version != CANONICAL_CHECKSUM_SCHEMA_VERSION {
            return Err(ReplayError::ChecksumSchemaMismatch {
                expected: CANONICAL_CHECKSUM_SCHEMA_VERSION,
                actual: self.header.checksum_schema_version,
            });
        }
        if self.header.content_identity != driver.content.identity {
            return Err(ReplayError::ContentMismatch {
                expected: driver.content.identity,
                actual: self.header.content_identity,
            });
        }
        if self.header.map_version != driver.content.map_version
            || self.header.release_revision != driver.content.revision
        {
            return Err(ReplayError::ReleaseMismatch {
                expected_map_version: driver.content.map_version,
                actual_map_version: self.header.map_version,
                expected_revision: driver.content.revision.to_owned(),
                actual_revision: self.header.release_revision.clone(),
            });
        }
        if self.header.configuration_identity != self.initial_state.configuration_identity() {
            return Err(ReplayError::ConfigurationMismatch {
                expected: self.initial_state.configuration_identity(),
                actual: self.header.configuration_identity,
            });
        }
        if self.header.initial_completed_tick != self.initial_state.completed_tick()
            || self.header.initial_checksum != self.initial_state.checksum()
            || self.header.snapshot_schema_version != self.initial_state.schema_version()
        {
            return Err(ReplayError::InitialSnapshotMismatch);
        }
        if self.records.len() != self.checkpoints.len() {
            return Err(ReplayError::CheckpointCountMismatch {
                records: self.records.len(),
                checkpoints: self.checkpoints.len(),
            });
        }
        for (index, checkpoint) in self.checkpoints.iter().enumerate() {
            let expected = InputStreamPosition(
                u64::try_from(index).expect("replay checkpoint count exceeds u64"),
            );
            if checkpoint.stream_position != expected {
                return Err(ReplayError::CheckpointPositionMismatch {
                    expected,
                    actual: checkpoint.stream_position,
                });
            }
        }
        Ok(())
    }

    fn verify_checkpoint(&self, index: usize, simulation: &Simulation) -> Result<(), ReplayError> {
        let checkpoint = self.checkpoints[index];
        let actual_boundary = simulation.tick().checked_sub(1);
        if actual_boundary != checkpoint.completed_tick {
            return Err(ReplayError::CheckpointBoundaryMismatch {
                stream_position: checkpoint.stream_position,
                expected: checkpoint.completed_tick,
                actual: actual_boundary,
            });
        }
        let actual_checksum = simulation.checksum();
        if actual_checksum != checkpoint.checksum {
            return Err(ReplayError::CheckpointChecksumMismatch {
                stream_position: checkpoint.stream_position,
                expected: checkpoint.checksum,
                actual: actual_checksum,
            });
        }
        Ok(())
    }
}

impl MatchDriver {
    #[must_use]
    pub fn export_replay(&self) -> MatchReplay {
        MatchReplay {
            header: ReplayHeader {
                schema_version: MATCH_REPLAY_SCHEMA_VERSION,
                snapshot_schema_version: AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION,
                checksum_schema_version: CANONICAL_CHECKSUM_SCHEMA_VERSION,
                map_version: self.content.map_version,
                release_revision: self.content.revision.to_owned(),
                content_identity: self.content.identity,
                configuration_identity: self.replay_initial_state.configuration_identity(),
                initial_completed_tick: self.replay_initial_state.completed_tick(),
                initial_checksum: self.replay_initial_state.checksum(),
            },
            initial_state: self.replay_initial_state.clone(),
            records: self.history.clone(),
            checkpoints: self.replay_checkpoints.clone(),
            seek_snapshots: Vec::new(),
        }
    }

    fn reset_for_replay(&mut self, simulation: &Simulation, initial_state: &SimulationSnapshot) {
        self.next_stream_position = InputStreamPosition(0);
        self.pending_commands.clear();
        self.next_client_sequences = simulation
            .players()
            .into_iter()
            .map(|player| (player.id, ClientCommandSequence(0)))
            .collect();
        self.submissions.clear();
        self.applied_sequences.clear();
        self.history.clear();
        self.replay_initial_state = initial_state.clone();
        self.replay_checkpoints.clear();
        self.last_executions.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CastleFightMatchConfig, MapVersion, MatchControlEvent, MatchOutcome,
        PlayerConnectionStatus, create_castle_fight_match,
    };

    fn match_927(workers: usize) -> crate::CastleFightMatch {
        create_castle_fight_match(
            CastleFightMatchConfig::development_subset(
                MapVersion::CASTLE_FIGHT_9_27,
                "r1",
                0x5245_504c_4159_0001,
            )
            .unwrap(),
            workers,
        )
        .unwrap()
    }

    #[test]
    fn replay_from_initial_state_reproduces_controls_ticks_and_final_checksum() {
        let mut original = match_927(1);
        let mut driver = MatchDriver::new(&original.simulation, original.content);
        driver.advance_local_tick(&mut original.simulation).unwrap();
        let seek_snapshot = driver.capture_snapshot(&original.simulation);
        driver
            .emit_local_control(
                &mut original.simulation,
                MatchControlEvent::SetPlayerConnection {
                    player: PlayerId(0),
                    connection: PlayerConnectionStatus::Disconnected,
                },
            )
            .unwrap();
        driver
            .emit_local_control(
                &mut original.simulation,
                MatchControlEvent::SetPlayerConnection {
                    player: PlayerId(0),
                    connection: PlayerConnectionStatus::Connected,
                },
            )
            .unwrap();
        driver.advance_local_tick(&mut original.simulation).unwrap();
        driver
            .emit_local_control(
                &mut original.simulation,
                MatchControlEvent::FinishMatch {
                    outcome: MatchOutcome::Draw,
                },
            )
            .unwrap();

        let expected_checksum = original.simulation.checksum();
        let expected_history = driver.history().to_vec();
        let replay_without_seek = driver.export_replay();

        let mut replayed = match_927(4);
        let mut replay_driver = MatchDriver::new(&replayed.simulation, replayed.content);
        replay_without_seek
            .play_to_end(&mut replayed.simulation, &mut replay_driver)
            .unwrap();
        assert_eq!(replayed.simulation.checksum(), expected_checksum);
        assert_eq!(
            replayed.simulation.lifecycle(),
            original.simulation.lifecycle()
        );
        assert_eq!(replay_driver.history(), expected_history);
        assert!(replayed.simulation.attacks_last_tick().is_empty());
        assert!(replayed.simulation.ability_casts_last_tick().is_empty());
        assert!(replayed.simulation.chain_lightnings_last_tick().is_empty());

        let mut replay_with_seek = driver.export_replay();
        replay_with_seek.add_seek_snapshot(seek_snapshot).unwrap();
        let mut seek_replayed = match_927(3);
        let mut seek_driver = MatchDriver::new(&seek_replayed.simulation, seek_replayed.content);
        replay_with_seek
            .play_to_end(&mut seek_replayed.simulation, &mut seek_driver)
            .unwrap();
        assert_eq!(seek_replayed.simulation.checksum(), expected_checksum);
        assert_eq!(
            seek_replayed.simulation.lifecycle(),
            original.simulation.lifecycle()
        );
        assert_eq!(seek_driver.history(), expected_history);
    }
}
