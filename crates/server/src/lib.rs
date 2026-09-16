use std::{collections::BTreeMap, fmt};

pub mod tcp;

use castle_fight_protocol::{
    Checkpoint, CheckpointReport, ClientHello, ClientMessage, CommandAcknowledgement,
    CompatibilityIdentity, HandshakeRejectReason, ProtocolErrorCode, ServerMessage,
    SessionAssignment, WireCanonicalStreamRecord, WireCommandExecution, WireExecutionBatch,
};
use castle_fight_sim::{
    AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION, CANONICAL_CHECKSUM_SCHEMA_VERSION, CanonicalStreamError,
    CanonicalStreamRecord, CastleFightMatch, CastleFightMatchConfig, CastleFightMatchSetupError,
    ClientCommandSequence, MatchDriver, PlayerId, Simulation, create_castle_fight_match,
};

pub const DEFAULT_CHECKPOINT_INTERVAL_TICKS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerMatchOptions {
    pub checkpoint_interval_ticks: u64,
}

impl Default for ServerMatchOptions {
    fn default() -> Self {
        Self {
            checkpoint_interval_ticks: DEFAULT_CHECKPOINT_INTERVAL_TICKS,
        }
    }
}

#[derive(Debug)]
pub enum ServerMatchError {
    Setup(CastleFightMatchSetupError),
    InvalidCheckpointInterval,
    CanonicalStream(CanonicalStreamError),
}

impl fmt::Display for ServerMatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Setup(error) => error.fmt(formatter),
            Self::InvalidCheckpointInterval => {
                formatter.write_str("checkpoint interval must be greater than zero")
            }
            Self::CanonicalStream(error) => {
                write!(formatter, "canonical stream error: {error:?}")
            }
        }
    }
}

impl std::error::Error for ServerMatchError {}

impl From<CastleFightMatchSetupError> for ServerMatchError {
    fn from(error: CastleFightMatchSetupError) -> Self {
        Self::Setup(error)
    }
}

impl From<CanonicalStreamError> for ServerMatchError {
    fn from(error: CanonicalStreamError) -> Self {
        Self::CanonicalStream(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundTarget {
    Session(SessionId),
    Broadcast,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundMessage {
    pub target: OutboundTarget,
    pub message: ServerMessage,
}

impl OutboundMessage {
    #[must_use]
    pub const fn to_session(session: SessionId, message: ServerMessage) -> Self {
        Self {
            target: OutboundTarget::Session(session),
            message,
        }
    }

    #[must_use]
    pub const fn broadcast(message: ServerMessage) -> Self {
        Self {
            target: OutboundTarget::Broadcast,
            message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeResult {
    Accepted {
        session_id: SessionId,
        assignment: SessionAssignment,
    },
    Rejected {
        reason: HandshakeRejectReason,
    },
}

impl HandshakeResult {
    #[must_use]
    pub fn message(&self) -> ServerMessage {
        match self {
            Self::Accepted { assignment, .. } => ServerMessage::HelloAccepted {
                assignment: *assignment,
            },
            Self::Rejected { reason } => ServerMessage::HelloRejected {
                reason: reason.clone(),
            },
        }
    }

    #[must_use]
    pub const fn session_id(&self) -> Option<SessionId> {
        match self {
            Self::Accepted { session_id, .. } => Some(*session_id),
            Self::Rejected { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointReportStatus {
    Matched,
    Mismatch {
        expected_checksum: u64,
        actual_checksum: u64,
    },
    UnknownCheckpoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SessionState {
    player: PlayerId,
    connected: bool,
    checkpoint_status: Option<CheckpointReportStatus>,
}

/// Headless authoritative match/session state.
///
/// Input scheduling policy for Step 9 is deliberately simple: `Simulation::tick()` is the one open,
/// unfinalized tick. Every newly admitted command is assigned to that tick in server arrival order.
/// Once `finalize_next_tick` runs, that tick and order are immutable; commands arriving afterward
/// can only enter the next open tick. Transport silence is never interpreted as an empty tick: an
/// explicit broadcast `StreamRecord::Tick` is the only proof that a tick's input set is complete.
pub struct AuthoritativeMatch {
    game: CastleFightMatch,
    driver: MatchDriver,
    compatibility: CompatibilityIdentity,
    options: ServerMatchOptions,
    sessions: BTreeMap<SessionId, SessionState>,
    player_claims: BTreeMap<PlayerId, SessionId>,
    next_session_id: u64,
    last_checkpoint: Option<Checkpoint>,
}

impl AuthoritativeMatch {
    pub fn new(
        config: CastleFightMatchConfig,
        workers: usize,
        options: ServerMatchOptions,
    ) -> Result<Self, ServerMatchError> {
        if options.checkpoint_interval_ticks == 0 {
            return Err(ServerMatchError::InvalidCheckpointInterval);
        }
        let game = create_castle_fight_match(config, workers)?;
        let snapshot = game.simulation.capture_snapshot();
        let compatibility = CompatibilityIdentity {
            snapshot_schema_version: AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION,
            checksum_schema_version: CANONICAL_CHECKSUM_SCHEMA_VERSION,
            map_version: game.match_config.release.map_version,
            release_revision: game.match_config.release.release_revision.to_owned(),
            content_schema_version: game.content.identity.schema_version,
            content_gameplay_hash: game.content.identity.gameplay_hash,
            configuration_identity: snapshot.configuration_identity(),
        };
        let driver = MatchDriver::new(&game.simulation, game.content);
        Ok(Self {
            game,
            driver,
            compatibility,
            options,
            sessions: BTreeMap::new(),
            player_claims: BTreeMap::new(),
            next_session_id: 1,
            last_checkpoint: None,
        })
    }

    #[must_use]
    pub const fn simulation(&self) -> &Simulation {
        &self.game.simulation
    }

    #[must_use]
    pub const fn driver(&self) -> &MatchDriver {
        &self.driver
    }

    #[must_use]
    pub const fn compatibility(&self) -> &CompatibilityIdentity {
        &self.compatibility
    }

    #[must_use]
    pub const fn last_checkpoint(&self) -> Option<Checkpoint> {
        self.last_checkpoint
    }

    #[must_use]
    pub fn session_player(&self, session_id: SessionId) -> Option<PlayerId> {
        self.sessions.get(&session_id).map(|session| session.player)
    }

    #[must_use]
    pub fn session_is_connected(&self, session_id: SessionId) -> bool {
        self.sessions
            .get(&session_id)
            .is_some_and(|session| session.connected)
    }

    #[must_use]
    pub fn all_players_connected(&self) -> bool {
        self.player_claims.len() == self.game.match_config.participants.len()
            && self
                .player_claims
                .values()
                .all(|session_id| self.session_is_connected(*session_id))
    }

    #[must_use]
    pub fn checkpoint_report_status(
        &self,
        session_id: SessionId,
    ) -> Option<CheckpointReportStatus> {
        self.sessions
            .get(&session_id)
            .and_then(|session| session.checkpoint_status)
    }

    pub fn accept_hello(&mut self, hello: ClientHello) -> HandshakeResult {
        if hello.compatibility.validate_shape().is_err() {
            return HandshakeResult::Rejected {
                reason: HandshakeRejectReason::InvalidHello,
            };
        }
        if let Some(mismatch) = hello.compatibility.mismatch(&self.compatibility) {
            return HandshakeResult::Rejected {
                reason: HandshakeRejectReason::Incompatible { mismatch },
            };
        }

        let Some(participant) = self
            .game
            .match_config
            .participants
            .iter()
            .find(|participant| !self.player_claims.contains_key(&participant.id))
            .copied()
        else {
            return HandshakeResult::Rejected {
                reason: HandshakeRejectReason::MatchFull,
            };
        };

        let Some(next_session_id) = self.next_session_id.checked_add(1) else {
            return HandshakeResult::Rejected {
                reason: HandshakeRejectReason::MatchUnavailable,
            };
        };
        let session_id = SessionId(self.next_session_id);
        self.next_session_id = next_session_id;
        let assignment = SessionAssignment {
            session_id: session_id.0,
            player_id: participant.id.0,
            team: participant.team.0,
            next_stream_position: self.driver.next_stream_position().0,
            completed_tick: self.game.simulation.tick().checked_sub(1),
        };
        self.sessions.insert(
            session_id,
            SessionState {
                player: participant.id,
                connected: true,
                checkpoint_status: None,
            },
        );
        self.player_claims.insert(participant.id, session_id);
        HandshakeResult::Accepted {
            session_id,
            assignment,
        }
    }

    /// Marks a transport/session as unavailable without changing canonical gameplay state.
    ///
    /// Step 10 is responsible for translating disconnect/reconnect into canonical delegation and
    /// pause controls. Step 9 intentionally keeps the claimed player slot reserved so a fresh
    /// unauthenticated connection cannot hijack it after a socket drop.
    pub fn disconnect_session(&mut self, session_id: SessionId) {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.connected = false;
        }
    }

    pub fn handle_client_message(
        &mut self,
        session_id: SessionId,
        message: ClientMessage,
    ) -> Vec<OutboundMessage> {
        let Some(session) = self.sessions.get(&session_id).copied() else {
            return vec![OutboundMessage::to_session(
                session_id,
                ServerMessage::ProtocolError {
                    code: ProtocolErrorCode::Unauthorized,
                },
            )];
        };
        if !session.connected {
            return vec![OutboundMessage::to_session(
                session_id,
                ServerMessage::ProtocolError {
                    code: ProtocolErrorCode::Unauthorized,
                },
            )];
        }

        match message {
            ClientMessage::Hello { .. } => vec![OutboundMessage::to_session(
                session_id,
                ServerMessage::ProtocolError {
                    code: ProtocolErrorCode::AlreadyAuthenticated,
                },
            )],
            ClientMessage::SubmitCommand { request } => {
                let client_sequence = request.client_sequence;
                match self.driver.submit_command(
                    &self.game.simulation,
                    session.player,
                    ClientCommandSequence(client_sequence),
                    request.command.into(),
                ) {
                    Ok(submission) => vec![OutboundMessage::to_session(
                        session_id,
                        ServerMessage::CommandAcknowledged {
                            acknowledgement: CommandAcknowledgement::from_submission(
                                client_sequence,
                                submission,
                            ),
                        },
                    )],
                    Err(_) => vec![OutboundMessage::to_session(
                        session_id,
                        ServerMessage::ProtocolError {
                            code: ProtocolErrorCode::SequenceViolation,
                        },
                    )],
                }
            }
            ClientMessage::CheckpointReport { report } => {
                self.record_checkpoint_report(session_id, report);
                Vec::new()
            }
        }
    }

    pub fn finalize_next_tick(&mut self) -> Result<Vec<OutboundMessage>, ServerMatchError> {
        let tick = self.driver.advance_local_tick(&mut self.game.simulation)?;
        let record = CanonicalStreamRecord::Tick(tick.finalized.clone());
        let mut outbound = vec![
            OutboundMessage::broadcast(ServerMessage::StreamRecord {
                record: WireCanonicalStreamRecord::from(&record),
            }),
            OutboundMessage::broadcast(ServerMessage::TickExecutions {
                batch: WireExecutionBatch {
                    tick: tick.finalized.tick,
                    executions: tick
                        .executions
                        .iter()
                        .copied()
                        .map(WireCommandExecution::from)
                        .collect(),
                },
            }),
        ];

        if tick.finalized.tick % self.options.checkpoint_interval_ticks
            == self.options.checkpoint_interval_ticks - 1
        {
            let checkpoint = Checkpoint {
                completed_tick: tick.finalized.tick,
                checksum: self.game.simulation.checksum(),
            };
            self.last_checkpoint = Some(checkpoint);
            outbound.push(OutboundMessage::broadcast(ServerMessage::Checkpoint {
                checkpoint,
            }));
        }

        Ok(outbound)
    }

    fn record_checkpoint_report(&mut self, session_id: SessionId, report: CheckpointReport) {
        let status = match self.last_checkpoint {
            Some(checkpoint) if checkpoint.completed_tick == report.completed_tick => {
                if checkpoint.checksum == report.checksum {
                    CheckpointReportStatus::Matched
                } else {
                    CheckpointReportStatus::Mismatch {
                        expected_checksum: checkpoint.checksum,
                        actual_checksum: report.checksum,
                    }
                }
            }
            _ => CheckpointReportStatus::UnknownCheckpoint,
        };
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.checkpoint_status = Some(status);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use castle_fight_protocol::{
        CommandRequest, CompatibilityMismatch, WireAdmissionError, WirePlayerCommand,
    };
    use castle_fight_sim::{
        CastleFightBuilderRace, CastleFightParticipantConfig, MapVersion, MatchDriver,
        PlayerCommand, SimPoint, Team,
    };

    fn config(seed: u64) -> CastleFightMatchConfig {
        CastleFightMatchConfig::development_subset(MapVersion::CASTLE_FIGHT_9_27, "r1", seed)
            .unwrap()
    }

    fn config_2v2(seed: u64) -> CastleFightMatchConfig {
        CastleFightMatchConfig::development_subset_with_participants(
            MapVersion::CASTLE_FIGHT_9_27,
            "r1",
            seed,
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
        .unwrap()
    }

    fn hello(server: &AuthoritativeMatch, nonce: u64) -> ClientHello {
        ClientHello {
            compatibility: server.compatibility().clone(),
            client_nonce: nonce,
        }
    }

    fn accepted_session(result: HandshakeResult) -> SessionId {
        match result {
            HandshakeResult::Accepted { session_id, .. } => session_id,
            HandshakeResult::Rejected { reason } => panic!("handshake rejected: {reason:?}"),
        }
    }

    fn command_message(sequence: u64, command: PlayerCommand) -> ClientMessage {
        ClientMessage::SubmitCommand {
            request: CommandRequest {
                client_sequence: sequence,
                observed_completed_tick: None,
                command: command.into(),
            },
        }
    }

    fn stream_record(messages: &[OutboundMessage]) -> CanonicalStreamRecord {
        messages
            .iter()
            .find_map(|outbound| match &outbound.message {
                ServerMessage::StreamRecord { record } => Some(record.clone().into()),
                _ => None,
            })
            .expect("tick finalization must broadcast a stream record")
    }

    #[test]
    fn handshake_assigns_authored_players_and_rejects_incompatible_clients() {
        let mut server =
            AuthoritativeMatch::new(config(11), 1, ServerMatchOptions::default()).unwrap();
        let first_hello = hello(&server, 1);
        let first = server.accept_hello(first_hello);
        assert!(matches!(
            first,
            HandshakeResult::Accepted {
                assignment: SessionAssignment {
                    player_id: 0,
                    team: 0,
                    completed_tick: None,
                    ..
                },
                ..
            }
        ));

        let second_hello = hello(&server, 2);
        let second = server.accept_hello(second_hello);
        assert!(matches!(
            second,
            HandshakeResult::Accepted {
                assignment: SessionAssignment {
                    player_id: 6,
                    team: 1,
                    ..
                },
                ..
            }
        ));

        let mut incompatible = hello(&server, 3);
        incompatible.compatibility.configuration_identity ^= 1;
        assert!(matches!(
            server.accept_hello(incompatible),
            HandshakeResult::Rejected {
                reason: HandshakeRejectReason::Incompatible {
                    mismatch: CompatibilityMismatch::Configuration { .. }
                }
            }
        ));
    }

    #[test]
    fn bound_session_cannot_command_another_players_builder() {
        let mut server =
            AuthoritativeMatch::new(config(12), 1, ServerMatchOptions::default()).unwrap();
        let session = accepted_session(server.accept_hello(hello(&server, 1)));
        let foreign_builder = server
            .simulation()
            .builder_for_player(PlayerId(6))
            .unwrap()
            .id;
        let responses = server.handle_client_message(
            session,
            command_message(
                0,
                PlayerCommand::MoveBuilder {
                    builder: foreign_builder,
                    destination: SimPoint::new(0, 0),
                },
            ),
        );
        assert!(matches!(
            responses.as_slice(),
            [OutboundMessage {
                message: ServerMessage::CommandAcknowledged {
                    acknowledgement: CommandAcknowledgement::Rejected {
                        reason: WireAdmissionError::BuilderNotControllable { builder },
                        duplicate: false,
                        ..
                    }
                },
                ..
            }] if *builder == foreign_builder.0
        ));
    }

    #[test]
    fn duplicate_submission_is_acknowledged_without_double_scheduling() {
        let mut server =
            AuthoritativeMatch::new(config(13), 1, ServerMatchOptions::default()).unwrap();
        let session = accepted_session(server.accept_hello(hello(&server, 1)));
        let builder = server
            .simulation()
            .builder_for_player(PlayerId(0))
            .unwrap()
            .id;
        let request = command_message(0, PlayerCommand::StopBuilder { builder });

        let first = server.handle_client_message(session, request.clone());
        let duplicate = server.handle_client_message(session, request);
        assert!(matches!(
            first[0].message,
            ServerMessage::CommandAcknowledged {
                acknowledgement: CommandAcknowledgement::Scheduled {
                    duplicate: false,
                    ..
                }
            }
        ));
        assert!(matches!(
            duplicate[0].message,
            ServerMessage::CommandAcknowledged {
                acknowledgement: CommandAcknowledgement::Scheduled {
                    duplicate: true,
                    ..
                }
            }
        ));

        let finalized = server.finalize_next_tick().unwrap();
        let CanonicalStreamRecord::Tick(tick) = stream_record(&finalized) else {
            panic!("expected finalized tick")
        };
        assert_eq!(tick.commands.len(), 1);
    }

    #[test]
    fn empty_tick_is_explicitly_broadcast_and_checkpointed() {
        let mut server = AuthoritativeMatch::new(
            config(14),
            1,
            ServerMatchOptions {
                checkpoint_interval_ticks: 1,
            },
        )
        .unwrap();
        let finalized = server.finalize_next_tick().unwrap();
        let CanonicalStreamRecord::Tick(tick) = stream_record(&finalized) else {
            panic!("expected finalized tick")
        };
        assert_eq!(tick.tick, 0);
        assert!(tick.commands.is_empty());
        assert!(finalized.iter().any(|outbound| matches!(
            outbound.message,
            ServerMessage::Checkpoint {
                checkpoint: Checkpoint {
                    completed_tick: 0,
                    ..
                }
            }
        )));
    }

    #[test]
    fn two_independent_clients_apply_server_stream_and_keep_checksum() {
        let match_config = config(15);
        let mut server = AuthoritativeMatch::new(
            match_config.clone(),
            1,
            ServerMatchOptions {
                checkpoint_interval_ticks: 1,
            },
        )
        .unwrap();
        let session_a = accepted_session(server.accept_hello(hello(&server, 1)));
        let session_b = accepted_session(server.accept_hello(hello(&server, 2)));

        let mut client_a = create_castle_fight_match(match_config.clone(), 1).unwrap();
        let mut client_b = create_castle_fight_match(match_config, 2).unwrap();
        let mut driver_a = MatchDriver::new(&client_a.simulation, client_a.content);
        let mut driver_b = MatchDriver::new(&client_b.simulation, client_b.content);

        let builder_a = server
            .simulation()
            .builder_for_player(PlayerId(0))
            .unwrap()
            .id;
        let builder_b = server
            .simulation()
            .builder_for_player(PlayerId(6))
            .unwrap()
            .id;
        server.handle_client_message(
            session_a,
            command_message(0, PlayerCommand::StopBuilder { builder: builder_a }),
        );
        server.handle_client_message(
            session_b,
            command_message(0, PlayerCommand::StopBuilder { builder: builder_b }),
        );

        for _ in 0..3 {
            let outbound = server.finalize_next_tick().unwrap();
            let record = stream_record(&outbound);
            driver_a
                .apply_stream_record(&mut client_a.simulation, record.clone())
                .unwrap();
            driver_b
                .apply_stream_record(&mut client_b.simulation, record)
                .unwrap();
            assert_eq!(
                client_a.simulation.checksum(),
                server.simulation().checksum()
            );
            assert_eq!(
                client_b.simulation.checksum(),
                server.simulation().checksum()
            );
        }
    }

    #[test]
    fn two_vs_two_assignment_and_replication_stay_synchronized() {
        let match_config = config_2v2(16);
        let mut server = AuthoritativeMatch::new(
            match_config.clone(),
            1,
            ServerMatchOptions {
                checkpoint_interval_ticks: 2,
            },
        )
        .unwrap();
        let mut sessions = Vec::new();
        for nonce in 0..4 {
            sessions.push(accepted_session(server.accept_hello(hello(&server, nonce))));
        }
        assert_eq!(
            sessions
                .iter()
                .map(|session| server.session_player(*session).unwrap().0)
                .collect::<Vec<_>>(),
            vec![0, 1, 6, 7]
        );

        let mut client = create_castle_fight_match(match_config, 3).unwrap();
        let mut client_driver = MatchDriver::new(&client.simulation, client.content);
        for session in sessions.iter().copied() {
            let player = server.session_player(session).unwrap();
            let builder = server.simulation().builder_for_player(player).unwrap().id;
            let response = server.handle_client_message(
                session,
                command_message(0, PlayerCommand::StopBuilder { builder }),
            );
            assert!(matches!(
                response[0].message,
                ServerMessage::CommandAcknowledged {
                    acknowledgement: CommandAcknowledgement::Scheduled { .. }
                }
            ));
        }
        let outbound = server.finalize_next_tick().unwrap();
        let record = stream_record(&outbound);
        client_driver
            .apply_stream_record(&mut client.simulation, record)
            .unwrap();
        assert_eq!(client.simulation.checksum(), server.simulation().checksum());
    }

    #[test]
    fn disconnect_keeps_slot_claimed_and_cannot_change_finalized_order() {
        let mut server =
            AuthoritativeMatch::new(config(17), 1, ServerMatchOptions::default()).unwrap();
        let session_a = accepted_session(server.accept_hello(hello(&server, 1)));
        let _session_b = accepted_session(server.accept_hello(hello(&server, 2)));
        let builder = server
            .simulation()
            .builder_for_player(PlayerId(0))
            .unwrap()
            .id;
        server.handle_client_message(
            session_a,
            command_message(0, PlayerCommand::StopBuilder { builder }),
        );
        let first_tick = server.finalize_next_tick().unwrap();
        let first_record = stream_record(&first_tick);

        server.disconnect_session(session_a);
        let response = server.handle_client_message(
            session_a,
            command_message(1, PlayerCommand::StopBuilder { builder }),
        );
        assert!(matches!(
            response[0].message,
            ServerMessage::ProtocolError {
                code: ProtocolErrorCode::Unauthorized
            }
        ));
        assert!(matches!(
            server.accept_hello(hello(&server, 3)),
            HandshakeResult::Rejected {
                reason: HandshakeRejectReason::MatchFull
            }
        ));
        assert_eq!(first_record, stream_record(&first_tick));
    }

    #[test]
    fn checkpoint_reports_are_compared_to_the_latest_authoritative_checkpoint() {
        let mut server = AuthoritativeMatch::new(
            config(18),
            1,
            ServerMatchOptions {
                checkpoint_interval_ticks: 1,
            },
        )
        .unwrap();
        let session = accepted_session(server.accept_hello(hello(&server, 1)));
        server.finalize_next_tick().unwrap();
        let checkpoint = server.last_checkpoint().unwrap();

        server.handle_client_message(
            session,
            ClientMessage::CheckpointReport {
                report: CheckpointReport {
                    completed_tick: checkpoint.completed_tick,
                    checksum: checkpoint.checksum,
                },
            },
        );
        assert_eq!(
            server.checkpoint_report_status(session),
            Some(CheckpointReportStatus::Matched)
        );

        server.handle_client_message(
            session,
            ClientMessage::CheckpointReport {
                report: CheckpointReport {
                    completed_tick: checkpoint.completed_tick,
                    checksum: checkpoint.checksum ^ 1,
                },
            },
        );
        assert!(matches!(
            server.checkpoint_report_status(session),
            Some(CheckpointReportStatus::Mismatch { .. })
        ));
    }

    #[test]
    fn command_request_wire_shape_has_no_player_identity() {
        let request = CommandRequest {
            client_sequence: 0,
            observed_completed_tick: None,
            command: WirePlayerCommand::StopBuilder { builder: 1 },
        };
        let json = castle_fight_protocol::encode_frame(
            &castle_fight_protocol::ProtocolEnvelope::new(ClientMessage::SubmitCommand { request }),
        )
        .unwrap();
        let text = String::from_utf8(json[4..].to_vec()).unwrap();
        assert!(!text.contains("player_id"));
        assert!(!text.contains("\"player\""));
    }
}
