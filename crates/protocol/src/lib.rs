use std::{
    fmt,
    io::{self, Read, Write},
};

use castle_fight_sim::{
    BoundaryControlRecord, BuildPosition, BuilderBuildError, BuilderCommandError,
    BuilderQueuedCommand, BuildingCommandError, BuildingConstructionCancelError,
    BuildingConstructionCancelOutcome, BuildingPlacementError, BuildingUpgradeError,
    CanonicalStreamRecord, CastleFightBuildingId, ClientCommandSequence, CommandAdmissionError,
    CommandExecution, CommandExecutionResult, CommandOrder, CommandOutcome, CommandSubmission,
    FinalizedTickInputs, InputStreamPosition, MapVersion, MatchControlEvent, MatchOutcome,
    PlayerCommand, PlayerConnectionStatus, PlayerId, ResourcePurchaseError, ScheduledCommand,
    SimId, SimPoint, Team,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub const PROTOCOL_SCHEMA_VERSION: u32 = 11;
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_RELEASE_REVISION_BYTES: usize = 64;
pub const RECONNECT_TOKEN_BYTES: usize = 32;
pub const MAX_SNAPSHOT_BYTES: usize = 8 * 1024 * 1024;
pub const SNAPSHOT_CHUNK_BYTES: usize = 48 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolEnvelope<T> {
    pub schema_version: u32,
    pub message: T,
}

impl<T> ProtocolEnvelope<T> {
    #[must_use]
    pub const fn new(message: T) -> Self {
        Self {
            schema_version: PROTOCOL_SCHEMA_VERSION,
            message,
        }
    }

    pub fn into_current(self) -> Result<T, ProtocolSchemaError> {
        if self.schema_version == PROTOCOL_SCHEMA_VERSION {
            Ok(self.message)
        } else {
            Err(ProtocolSchemaError {
                expected: PROTOCOL_SCHEMA_VERSION,
                actual: self.schema_version,
            })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolSchemaError {
    pub expected: u32,
    pub actual: u32,
}

impl fmt::Display for ProtocolSchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "protocol schema mismatch: expected {}, got {}",
            self.expected, self.actual
        )
    }
}

impl std::error::Error for ProtocolSchemaError {}

#[derive(Debug)]
pub enum FrameError {
    Io(io::Error),
    Encode(serde_json::Error),
    Decode(serde_json::Error),
    EmptyFrame,
    TooLarge { length: usize, maximum: usize },
    TrailingBytes { trailing: usize },
}

impl fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "frame I/O error: {error}"),
            Self::Encode(error) => write!(formatter, "frame encoding error: {error}"),
            Self::Decode(error) => write!(formatter, "frame decoding error: {error}"),
            Self::EmptyFrame => formatter.write_str("zero-length protocol frame is invalid"),
            Self::TooLarge { length, maximum } => write!(
                formatter,
                "protocol frame length {length} exceeds configured maximum {maximum}"
            ),
            Self::TrailingBytes { trailing } => {
                write!(
                    formatter,
                    "protocol frame contains {trailing} trailing bytes"
                )
            }
        }
    }
}

impl std::error::Error for FrameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Encode(error) | Self::Decode(error) => Some(error),
            Self::EmptyFrame | Self::TooLarge { .. } | Self::TrailingBytes { .. } => None,
        }
    }
}

pub fn frame_length_prefix(length: usize) -> Result<[u8; 4], FrameError> {
    if length == 0 {
        return Err(FrameError::EmptyFrame);
    }
    if length > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge {
            length,
            maximum: MAX_FRAME_BYTES,
        });
    }
    let length = u32::try_from(length).map_err(|_| FrameError::TooLarge {
        length,
        maximum: MAX_FRAME_BYTES,
    })?;
    Ok(length.to_be_bytes())
}

pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, FrameError> {
    let body = serde_json::to_vec(value).map_err(FrameError::Encode)?;
    let prefix = frame_length_prefix(body.len())?;
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&prefix);
    frame.extend_from_slice(&body);
    Ok(frame)
}

pub fn decode_frame<T: DeserializeOwned>(frame: &[u8]) -> Result<T, FrameError> {
    if frame.len() < 4 {
        return Err(FrameError::Io(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "protocol frame is missing its length prefix",
        )));
    }
    let length = u32::from_be_bytes(frame[..4].try_into().expect("slice length checked")) as usize;
    if length == 0 {
        return Err(FrameError::EmptyFrame);
    }
    if length > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge {
            length,
            maximum: MAX_FRAME_BYTES,
        });
    }
    let expected = 4usize
        .checked_add(length)
        .expect("bounded frame length cannot overflow usize");
    if frame.len() < expected {
        return Err(FrameError::Io(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "protocol frame body is truncated",
        )));
    }
    if frame.len() > expected {
        return Err(FrameError::TrailingBytes {
            trailing: frame.len() - expected,
        });
    }
    serde_json::from_slice(&frame[4..expected]).map_err(FrameError::Decode)
}

pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<(), FrameError> {
    let frame = encode_frame(value)?;
    writer.write_all(&frame).map_err(FrameError::Io)
}

pub fn read_frame<R: Read, T: DeserializeOwned>(reader: &mut R) -> Result<T, FrameError> {
    let mut prefix = [0u8; 4];
    reader.read_exact(&mut prefix).map_err(FrameError::Io)?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 {
        return Err(FrameError::EmptyFrame);
    }
    if length > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge {
            length,
            maximum: MAX_FRAME_BYTES,
        });
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).map_err(FrameError::Io)?;
    serde_json::from_slice(&body).map_err(FrameError::Decode)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityIdentity {
    pub snapshot_schema_version: u32,
    pub checksum_schema_version: u32,
    pub map_version: MapVersion,
    pub release_revision: String,
    pub content_schema_version: u32,
    pub content_gameplay_hash: u64,
    pub configuration_identity: u64,
}

impl CompatibilityIdentity {
    pub fn validate_shape(&self) -> Result<(), MessageValidationError> {
        if self.release_revision.is_empty()
            || self.release_revision.len() > MAX_RELEASE_REVISION_BYTES
            || self.release_revision.contains('\0')
        {
            return Err(MessageValidationError::InvalidReleaseRevision);
        }
        Ok(())
    }

    #[must_use]
    pub fn mismatch(&self, expected: &Self) -> Option<CompatibilityMismatch> {
        if self.snapshot_schema_version != expected.snapshot_schema_version {
            return Some(CompatibilityMismatch::SnapshotSchema {
                expected: expected.snapshot_schema_version,
                actual: self.snapshot_schema_version,
            });
        }
        if self.checksum_schema_version != expected.checksum_schema_version {
            return Some(CompatibilityMismatch::ChecksumSchema {
                expected: expected.checksum_schema_version,
                actual: self.checksum_schema_version,
            });
        }
        if self.map_version != expected.map_version
            || self.release_revision != expected.release_revision
        {
            return Some(CompatibilityMismatch::MapRelease {
                expected_version: expected.map_version,
                expected_revision: expected.release_revision.clone(),
                actual_version: self.map_version,
                actual_revision: self.release_revision.clone(),
            });
        }
        if self.content_schema_version != expected.content_schema_version
            || self.content_gameplay_hash != expected.content_gameplay_hash
        {
            return Some(CompatibilityMismatch::Content {
                expected_schema: expected.content_schema_version,
                expected_hash: expected.content_gameplay_hash,
                actual_schema: self.content_schema_version,
                actual_hash: self.content_gameplay_hash,
            });
        }
        if self.configuration_identity != expected.configuration_identity {
            return Some(CompatibilityMismatch::Configuration {
                expected: expected.configuration_identity,
                actual: self.configuration_identity,
            });
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompatibilityMismatch {
    SnapshotSchema {
        expected: u32,
        actual: u32,
    },
    ChecksumSchema {
        expected: u32,
        actual: u32,
    },
    MapRelease {
        expected_version: MapVersion,
        expected_revision: String,
        actual_version: MapVersion,
        actual_revision: String,
    },
    Content {
        expected_schema: u32,
        expected_hash: u64,
        actual_schema: u32,
        actual_hash: u64,
    },
    Configuration {
        expected: u64,
        actual: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageValidationError {
    InvalidReleaseRevision,
}

impl fmt::Display for MessageValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidReleaseRevision => formatter
                .write_str("release revision must be non-empty, bounded, and contain no NUL bytes"),
        }
    }
}

impl std::error::Error for MessageValidationError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientHello {
    pub compatibility: CompatibilityIdentity,
    pub client_nonce: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconnectToken {
    pub bytes: [u8; RECONNECT_TOKEN_BYTES],
}

impl std::fmt::Debug for ReconnectToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReconnectToken([REDACTED])")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconnectHello {
    pub compatibility: CompatibilityIdentity,
    pub session_id: u64,
    pub reconnect_token: ReconnectToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionAssignment {
    pub session_id: u64,
    pub reconnect_token: ReconnectToken,
    pub player_id: u8,
    pub team: u8,
    pub next_stream_position: u64,
    pub next_client_sequence: u64,
    pub completed_tick: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotTransferBegin {
    pub transfer_id: u64,
    pub snapshot_stream_position: u64,
    pub snapshot_completed_tick: Option<u64>,
    pub snapshot_checksum: u64,
    pub snapshot_bytes: u32,
    pub chunk_count: u32,
    pub handoff_stream_position: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotChunk {
    pub transfer_id: u64,
    pub chunk_index: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatchUpComplete {
    pub transfer_id: u64,
    pub handoff_stream_position: u64,
    pub completed_tick: Option<u64>,
    pub checksum: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HandshakeRejectReason {
    ProtocolSchema { expected: u32, actual: u32 },
    Incompatible { mismatch: CompatibilityMismatch },
    InvalidHello,
    InvalidReconnect,
    MatchFull,
    MatchUnavailable,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebugSpeed {
    Quarter,
    Half,
    #[default]
    Normal,
    Double,
    Quadruple,
}

impl DebugSpeed {
    pub const ALL: [Self; 5] = [
        Self::Quarter,
        Self::Half,
        Self::Normal,
        Self::Double,
        Self::Quadruple,
    ];

    pub const fn multiplier(self) -> f64 {
        match self {
            Self::Quarter => 0.25,
            Self::Half => 0.5,
            Self::Normal => 1.0,
            Self::Double => 2.0,
            Self::Quadruple => 4.0,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Quarter => "0.25x",
            Self::Half => "0.5x",
            Self::Normal => "1x",
            Self::Double => "2x",
            Self::Quadruple => "4x",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WireDebugCommand {
    GrantResources,
    KillAllUnits,
    SetBuildingsInvulnerable {
        enabled: bool,
    },
    PopulateBuildings,
    PlayerCommand {
        player: u8,
        command: WirePlayerCommand,
    },
}

impl From<WireDebugCommand> for castle_fight_sim::debug::DebugCommand {
    fn from(value: WireDebugCommand) -> Self {
        match value {
            WireDebugCommand::GrantResources => Self::GrantResources,
            WireDebugCommand::KillAllUnits => Self::KillAllUnits,
            WireDebugCommand::SetBuildingsInvulnerable { enabled } => {
                Self::SetBuildingsInvulnerable { enabled }
            }
            WireDebugCommand::PopulateBuildings => Self::PopulateBuildings,
            WireDebugCommand::PlayerCommand { player, command } => Self::PlayerCommand {
                player: PlayerId(player),
                command: command.into(),
            },
        }
    }
}
impl From<castle_fight_sim::debug::DebugCommand> for WireDebugCommand {
    fn from(value: castle_fight_sim::debug::DebugCommand) -> Self {
        use castle_fight_sim::debug::DebugCommand;
        match value {
            DebugCommand::GrantResources => Self::GrantResources,
            DebugCommand::KillAllUnits => Self::KillAllUnits,
            DebugCommand::SetBuildingsInvulnerable { enabled } => {
                Self::SetBuildingsInvulnerable { enabled }
            }
            DebugCommand::PopulateBuildings => Self::PopulateBuildings,
            DebugCommand::PlayerCommand { player, command } => Self::PlayerCommand {
                player: player.0,
                command: command.into(),
            },
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DebugRequest {
    Command { command: WireDebugCommand },
    SetPlayback { paused: bool, speed: DebugSpeed },
    StepOneTick,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LobbyStatus {
    pub host_player_id: u8,
    pub debug_paused: bool,
    pub debug_speed: DebugSpeed,
    pub connected_player_ids: Vec<u8>,
    pub required_players: u8,
    pub participants: Vec<LobbyParticipant>,
    pub started: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LobbyParticipant {
    pub player_id: u8,
    pub builder_rawcode: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LobbyRaceRejectReason {
    AlreadyStarted,
    RaceUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LobbyStartRejectReason {
    NotHost,
    WaitingForPlayers,
    AlreadyStarted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientMessage {
    Hello { hello: ClientHello },
    Reconnect { reconnect: ReconnectHello },
    StartMatch,
    Debug { request: DebugRequest },
    SelectRace { builder_rawcode: u32 },
    SubmitCommand { request: CommandRequest },
    CheckpointReport { report: CheckpointReport },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerMessage {
    HelloAccepted {
        assignment: SessionAssignment,
    },
    HelloRejected {
        reason: HandshakeRejectReason,
    },
    LobbyStatus {
        status: LobbyStatus,
    },
    LobbyStartRejected {
        reason: LobbyStartRejectReason,
    },
    LobbyRaceRejected {
        reason: LobbyRaceRejectReason,
    },
    CommandAcknowledged {
        acknowledgement: CommandAcknowledgement,
    },
    StreamRecord {
        record: WireCanonicalStreamRecord,
    },
    TickExecutions {
        batch: WireExecutionBatch,
    },
    Checkpoint {
        checkpoint: Checkpoint,
    },
    SnapshotBegin {
        begin: SnapshotTransferBegin,
    },
    SnapshotChunk {
        chunk: SnapshotChunk,
    },
    CatchUpComplete {
        complete: CatchUpComplete,
    },
    ProtocolError {
        code: ProtocolErrorCode,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolErrorCode {
    ExpectedHello,
    AlreadyAuthenticated,
    MalformedMessage,
    Unauthorized,
    SequenceViolation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandRequest {
    pub client_sequence: u64,
    pub observed_completed_tick: Option<u64>,
    pub command: WirePlayerCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandAcknowledgement {
    Scheduled {
        client_sequence: u64,
        tick: u64,
        order: u32,
        duplicate: bool,
    },
    Rejected {
        client_sequence: u64,
        reason: WireAdmissionError,
        duplicate: bool,
    },
}

impl CommandAcknowledgement {
    #[must_use]
    pub fn from_submission(client_sequence: u64, submission: CommandSubmission) -> Self {
        match submission {
            CommandSubmission::Scheduled(scheduled) => Self::Scheduled {
                client_sequence,
                tick: scheduled.tick,
                order: scheduled.order.0,
                duplicate: false,
            },
            CommandSubmission::DuplicateScheduled(scheduled) => Self::Scheduled {
                client_sequence,
                tick: scheduled.tick,
                order: scheduled.order.0,
                duplicate: true,
            },
            CommandSubmission::Rejected(reason) => Self::Rejected {
                client_sequence,
                reason: reason.into(),
                duplicate: false,
            },
            CommandSubmission::DuplicateRejected(reason) => Self::Rejected {
                client_sequence,
                reason: reason.into(),
                duplicate: true,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub completed_tick: u64,
    pub checksum: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointReport {
    pub completed_tick: u64,
    pub checksum: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WirePoint {
    pub x: i32,
    pub y: i32,
}

impl From<SimPoint> for WirePoint {
    fn from(value: SimPoint) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

impl From<WirePoint> for SimPoint {
    fn from(value: WirePoint) -> Self {
        Self::new(value.x, value.y)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBuildPosition {
    pub min_x: i32,
    pub min_y: i32,
}

impl From<BuildPosition> for WireBuildPosition {
    fn from(value: BuildPosition) -> Self {
        Self {
            min_x: value.min_x,
            min_y: value.min_y,
        }
    }
}

impl From<WireBuildPosition> for BuildPosition {
    fn from(value: WireBuildPosition) -> Self {
        Self::new(value.min_x, value.min_y)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WireBuilderQueuedCommand {
    Build {
        building: u32,
        position: WireBuildPosition,
    },
    Move {
        destination: WirePoint,
    },
    Follow {
        target: u64,
    },
    Repair {
        target: u64,
    },
    Blink {
        destination: WirePoint,
    },
    Stop,
}

impl From<BuilderQueuedCommand> for WireBuilderQueuedCommand {
    fn from(value: BuilderQueuedCommand) -> Self {
        match value {
            BuilderQueuedCommand::Build { building, position } => Self::Build {
                building: building.0,
                position: position.into(),
            },
            BuilderQueuedCommand::Move { destination } => Self::Move {
                destination: destination.into(),
            },
            BuilderQueuedCommand::Follow { target } => Self::Follow { target: target.0 },
            BuilderQueuedCommand::Repair { target } => Self::Repair { target: target.0 },
            BuilderQueuedCommand::Blink { destination } => Self::Blink {
                destination: destination.into(),
            },
            BuilderQueuedCommand::Stop => Self::Stop,
        }
    }
}

impl From<WireBuilderQueuedCommand> for BuilderQueuedCommand {
    fn from(value: WireBuilderQueuedCommand) -> Self {
        match value {
            WireBuilderQueuedCommand::Build { building, position } => Self::Build {
                building: CastleFightBuildingId(building),
                position: position.into(),
            },
            WireBuilderQueuedCommand::Move { destination } => Self::Move {
                destination: destination.into(),
            },
            WireBuilderQueuedCommand::Follow { target } => Self::Follow {
                target: SimId(target),
            },
            WireBuilderQueuedCommand::Repair { target } => Self::Repair {
                target: SimId(target),
            },
            WireBuilderQueuedCommand::Blink { destination } => Self::Blink {
                destination: destination.into(),
            },
            WireBuilderQueuedCommand::Stop => Self::Stop,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WirePlayerCommand {
    QueueBuilderCommand {
        builder: u64,
        command: WireBuilderQueuedCommand,
    },
    MoveBuilder {
        builder: u64,
        destination: WirePoint,
    },
    FollowWithBuilder {
        builder: u64,
        target: u64,
    },
    StopBuilder {
        builder: u64,
    },
    BlinkBuilder {
        builder: u64,
        destination: WirePoint,
    },
    RepairWithBuilder {
        builder: u64,
        target: u64,
    },
    SetBuilderRepairAutocast {
        builder: u64,
        enabled: bool,
    },
    PlaceBuilding {
        builder: u64,
        building: u32,
        position: WireBuildPosition,
    },
    CancelBuildingConstruction {
        building: u64,
    },
    QueueProductionUnit {
        building: u64,
    },
    CancelProductionUnit {
        building: u64,
    },
    UpgradeBuilding {
        building: u64,
        target: u32,
    },
    AttackWithBuilding {
        building: u64,
        target: u64,
    },
    CastBuildingSpellAt {
        building: u64,
        position: [i32; 2],
    },
    CastBuildingSpell {
        building: u64,
    },
    SetBuildingSpellAutocast {
        building: u64,
        enabled: bool,
    },
}

impl From<PlayerCommand> for WirePlayerCommand {
    fn from(value: PlayerCommand) -> Self {
        match value {
            PlayerCommand::QueueBuilderCommand { builder, command } => Self::QueueBuilderCommand {
                builder: builder.0,
                command: command.into(),
            },
            PlayerCommand::MoveBuilder {
                builder,
                destination,
            } => Self::MoveBuilder {
                builder: builder.0,
                destination: destination.into(),
            },
            PlayerCommand::FollowWithBuilder { builder, target } => Self::FollowWithBuilder {
                builder: builder.0,
                target: target.0,
            },
            PlayerCommand::StopBuilder { builder } => Self::StopBuilder { builder: builder.0 },
            PlayerCommand::BlinkBuilder {
                builder,
                destination,
            } => Self::BlinkBuilder {
                builder: builder.0,
                destination: destination.into(),
            },
            PlayerCommand::RepairWithBuilder { builder, target } => Self::RepairWithBuilder {
                builder: builder.0,
                target: target.0,
            },
            PlayerCommand::SetBuilderRepairAutocast { builder, enabled } => {
                Self::SetBuilderRepairAutocast {
                    builder: builder.0,
                    enabled,
                }
            }
            PlayerCommand::PlaceBuilding {
                builder,
                building,
                position,
            } => Self::PlaceBuilding {
                builder: builder.0,
                building: building.0,
                position: position.into(),
            },
            PlayerCommand::CancelBuildingConstruction { building } => {
                Self::CancelBuildingConstruction {
                    building: building.0,
                }
            }
            PlayerCommand::QueueProductionUnit { building } => Self::QueueProductionUnit {
                building: building.0,
            },
            PlayerCommand::CancelProductionUnit { building } => Self::CancelProductionUnit {
                building: building.0,
            },
            PlayerCommand::UpgradeBuilding { building, target } => Self::UpgradeBuilding {
                building: building.0,
                target: target.0,
            },
            PlayerCommand::AttackWithBuilding { building, target } => Self::AttackWithBuilding {
                building: building.0,
                target: target.0,
            },
            PlayerCommand::CastBuildingSpellAt { building, position } => {
                Self::CastBuildingSpellAt {
                    building: building.0,
                    position: [position.x, position.y],
                }
            }
            PlayerCommand::CastBuildingSpell { building } => Self::CastBuildingSpell {
                building: building.0,
            },
            PlayerCommand::SetBuildingSpellAutocast { building, enabled } => {
                Self::SetBuildingSpellAutocast {
                    building: building.0,
                    enabled,
                }
            }
        }
    }
}

impl From<WirePlayerCommand> for PlayerCommand {
    fn from(value: WirePlayerCommand) -> Self {
        match value {
            WirePlayerCommand::QueueBuilderCommand { builder, command } => {
                Self::QueueBuilderCommand {
                    builder: SimId(builder),
                    command: command.into(),
                }
            }
            WirePlayerCommand::MoveBuilder {
                builder,
                destination,
            } => Self::MoveBuilder {
                builder: SimId(builder),
                destination: destination.into(),
            },
            WirePlayerCommand::FollowWithBuilder { builder, target } => Self::FollowWithBuilder {
                builder: SimId(builder),
                target: SimId(target),
            },
            WirePlayerCommand::StopBuilder { builder } => Self::StopBuilder {
                builder: SimId(builder),
            },
            WirePlayerCommand::BlinkBuilder {
                builder,
                destination,
            } => Self::BlinkBuilder {
                builder: SimId(builder),
                destination: destination.into(),
            },
            WirePlayerCommand::RepairWithBuilder { builder, target } => Self::RepairWithBuilder {
                builder: SimId(builder),
                target: SimId(target),
            },
            WirePlayerCommand::SetBuilderRepairAutocast { builder, enabled } => {
                Self::SetBuilderRepairAutocast {
                    builder: SimId(builder),
                    enabled,
                }
            }
            WirePlayerCommand::PlaceBuilding {
                builder,
                building,
                position,
            } => Self::PlaceBuilding {
                builder: SimId(builder),
                building: CastleFightBuildingId(building),
                position: position.into(),
            },
            WirePlayerCommand::CancelBuildingConstruction { building } => {
                Self::CancelBuildingConstruction {
                    building: SimId(building),
                }
            }
            WirePlayerCommand::QueueProductionUnit { building } => Self::QueueProductionUnit {
                building: SimId(building),
            },
            WirePlayerCommand::CancelProductionUnit { building } => Self::CancelProductionUnit {
                building: SimId(building),
            },
            WirePlayerCommand::UpgradeBuilding { building, target } => Self::UpgradeBuilding {
                building: SimId(building),
                target: CastleFightBuildingId(target),
            },
            WirePlayerCommand::AttackWithBuilding { building, target } => {
                Self::AttackWithBuilding {
                    building: SimId(building),
                    target: SimId(target),
                }
            }
            WirePlayerCommand::CastBuildingSpellAt { building, position } => {
                Self::CastBuildingSpellAt {
                    building: SimId(building),
                    position: SimPoint::new(position[0], position[1]),
                }
            }
            WirePlayerCommand::CastBuildingSpell { building } => Self::CastBuildingSpell {
                building: SimId(building),
            },
            WirePlayerCommand::SetBuildingSpellAutocast { building, enabled } => {
                Self::SetBuildingSpellAutocast {
                    building: SimId(building),
                    enabled,
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireScheduledCommand {
    pub tick: u64,
    pub order: u32,
    pub player: u8,
    pub client_sequence: u64,
    pub command: WirePlayerCommand,
}

impl From<ScheduledCommand> for WireScheduledCommand {
    fn from(value: ScheduledCommand) -> Self {
        Self {
            tick: value.tick,
            order: value.order.0,
            player: value.player.0,
            client_sequence: value.client_sequence.0,
            command: value.command.into(),
        }
    }
}

impl From<WireScheduledCommand> for ScheduledCommand {
    fn from(value: WireScheduledCommand) -> Self {
        Self {
            tick: value.tick,
            order: CommandOrder(value.order),
            player: PlayerId(value.player),
            client_sequence: ClientCommandSequence(value.client_sequence),
            command: value.command.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireCanonicalStreamRecord {
    Tick {
        tick: u64,
        stream_position: u64,
        commands: Vec<WireScheduledCommand>,
    },
    Control {
        after_completed_tick: Option<u64>,
        stream_position: u64,
        event: WireMatchControlEvent,
    },
}

impl From<&CanonicalStreamRecord> for WireCanonicalStreamRecord {
    fn from(value: &CanonicalStreamRecord) -> Self {
        match value {
            CanonicalStreamRecord::Tick(tick) => Self::Tick {
                tick: tick.tick,
                stream_position: tick.stream_position.0,
                commands: tick.commands.iter().copied().map(Into::into).collect(),
            },
            CanonicalStreamRecord::Control(control) => Self::Control {
                after_completed_tick: control.after_completed_tick,
                stream_position: control.stream_position.0,
                event: control.event.into(),
            },
        }
    }
}

impl From<WireCanonicalStreamRecord> for CanonicalStreamRecord {
    fn from(value: WireCanonicalStreamRecord) -> Self {
        match value {
            WireCanonicalStreamRecord::Tick {
                tick,
                stream_position,
                commands,
            } => Self::Tick(FinalizedTickInputs {
                tick,
                stream_position: InputStreamPosition(stream_position),
                commands: commands.into_iter().map(Into::into).collect(),
            }),
            WireCanonicalStreamRecord::Control {
                after_completed_tick,
                stream_position,
                event,
            } => Self::Control(BoundaryControlRecord {
                after_completed_tick,
                stream_position: InputStreamPosition(stream_position),
                event: event.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireMatchControlEvent {
    Debug { command: WireDebugCommand },
    SetPlayerConnection { player: u8, connected: bool },
    FinishMatch { outcome: WireMatchOutcome },
}

impl From<MatchControlEvent> for WireMatchControlEvent {
    fn from(value: MatchControlEvent) -> Self {
        match value {
            MatchControlEvent::Debug(command) => Self::Debug {
                command: command.into(),
            },
            MatchControlEvent::SetPlayerConnection { player, connection } => {
                Self::SetPlayerConnection {
                    player: player.0,
                    connected: connection == PlayerConnectionStatus::Connected,
                }
            }
            MatchControlEvent::FinishMatch { outcome } => Self::FinishMatch {
                outcome: outcome.into(),
            },
        }
    }
}

impl From<WireMatchControlEvent> for MatchControlEvent {
    fn from(value: WireMatchControlEvent) -> Self {
        match value {
            WireMatchControlEvent::Debug { command } => Self::Debug(command.into()),
            WireMatchControlEvent::SetPlayerConnection { player, connected } => {
                Self::SetPlayerConnection {
                    player: PlayerId(player),
                    connection: if connected {
                        PlayerConnectionStatus::Connected
                    } else {
                        PlayerConnectionStatus::Disconnected
                    },
                }
            }
            WireMatchControlEvent::FinishMatch { outcome } => Self::FinishMatch {
                outcome: outcome.into(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireMatchOutcome {
    Victory { team: u8 },
    Draw,
}

impl From<MatchOutcome> for WireMatchOutcome {
    fn from(value: MatchOutcome) -> Self {
        match value {
            MatchOutcome::Victory(team) => Self::Victory { team: team.0 },
            MatchOutcome::Draw => Self::Draw,
        }
    }
}

impl From<WireMatchOutcome> for MatchOutcome {
    fn from(value: WireMatchOutcome) -> Self {
        match value {
            WireMatchOutcome::Victory { team } => Self::Victory(Team(team)),
            WireMatchOutcome::Draw => Self::Draw,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireExecutionBatch {
    pub tick: u64,
    pub executions: Vec<WireCommandExecution>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireCommandExecution {
    pub scheduled: WireScheduledCommand,
    pub outcome: WireCommandOutcome,
}

impl From<CommandExecution> for WireCommandExecution {
    fn from(value: CommandExecution) -> Self {
        Self {
            scheduled: value.scheduled.into(),
            outcome: value.outcome.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireCommandOutcome {
    Executed { result: WireExecutionResult },
    Rejected { reason: WireRejectReason },
}

impl From<CommandOutcome> for WireCommandOutcome {
    fn from(value: CommandOutcome) -> Self {
        match value {
            CommandOutcome::Executed(result) => Self::Executed {
                result: result.into(),
            },
            CommandOutcome::Rejected(reason) => Self::Rejected {
                reason: reason.into(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireExecutionResult {
    Applied,
    BuilderBlinkedTo { destination: WirePoint },
    BuildingConstructionCancelled { reverted_upgrade: bool },
}

impl From<CommandExecutionResult> for WireExecutionResult {
    fn from(value: CommandExecutionResult) -> Self {
        match value {
            CommandExecutionResult::Applied => Self::Applied,
            CommandExecutionResult::BuilderBlinkedTo(point) => Self::BuilderBlinkedTo {
                destination: point.into(),
            },
            CommandExecutionResult::BuildingConstructionCancelled(outcome) => {
                Self::BuildingConstructionCancelled {
                    reverted_upgrade: outcome == BuildingConstructionCancelOutcome::RevertedUpgrade,
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireAdmissionError {
    MatchNotRunning,
    UnknownPlayer,
    UnknownBuildingDefinition { building: u32 },
    InvalidBuildPosition { position: WireBuildPosition },
    BuilderNotControllable { builder: u64 },
    BuildingNotControllable { building: u64 },
    BuildingNotInBuilderCatalog { builder: u64, building: u32 },
    InvalidUpgradeTarget { building: u64, target: u32 },
}

impl From<CommandAdmissionError> for WireAdmissionError {
    fn from(value: CommandAdmissionError) -> Self {
        match value {
            CommandAdmissionError::MatchNotRunning => Self::MatchNotRunning,
            CommandAdmissionError::UnknownPlayer => Self::UnknownPlayer,
            CommandAdmissionError::UnknownBuildingDefinition(building) => {
                Self::UnknownBuildingDefinition {
                    building: building.0,
                }
            }
            CommandAdmissionError::InvalidBuildPosition(position) => Self::InvalidBuildPosition {
                position: position.into(),
            },
            CommandAdmissionError::BuilderNotControllable(builder) => {
                Self::BuilderNotControllable { builder: builder.0 }
            }
            CommandAdmissionError::BuildingNotControllable(building) => {
                Self::BuildingNotControllable {
                    building: building.0,
                }
            }
            CommandAdmissionError::BuildingNotInBuilderCatalog { builder, building } => {
                Self::BuildingNotInBuilderCatalog {
                    builder: builder.0,
                    building: building.0,
                }
            }
            CommandAdmissionError::InvalidUpgradeTarget { building, target } => {
                Self::InvalidUpgradeTarget {
                    building: building.0,
                    target: target.0,
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireRejectReason {
    Builder { reason: WireBuilderCommandError },
    Build { reason: WireBuilderBuildError },
    CancelConstruction { reason: WireConstructionCancelError },
    Upgrade { reason: WireBuildingUpgradeError },
    Building { reason: WireBuildingCommandError },
    UnknownBuildingDefinition { building: u32 },
    InvalidBuildPosition { position: WireBuildPosition },
    SourceDefinitionMismatch,
}

impl From<castle_fight_sim::CommandRejectReason> for WireRejectReason {
    fn from(value: castle_fight_sim::CommandRejectReason) -> Self {
        use castle_fight_sim::CommandRejectReason;
        match value {
            CommandRejectReason::Builder(reason) => Self::Builder {
                reason: reason.into(),
            },
            CommandRejectReason::Build(reason) => Self::Build {
                reason: reason.into(),
            },
            CommandRejectReason::CancelConstruction(reason) => Self::CancelConstruction {
                reason: reason.into(),
            },
            CommandRejectReason::Upgrade(reason) => Self::Upgrade {
                reason: reason.into(),
            },
            CommandRejectReason::Building(reason) => Self::Building {
                reason: reason.into(),
            },
            CommandRejectReason::UnknownBuildingDefinition(building) => {
                Self::UnknownBuildingDefinition {
                    building: building.0,
                }
            }
            CommandRejectReason::InvalidBuildPosition(position) => Self::InvalidBuildPosition {
                position: position.into(),
            },
            CommandRejectReason::SourceDefinitionMismatch => Self::SourceDefinitionMismatch,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireBuilderCommandError {
    OrderQueueFull,
    BuilderNotFound,
    NotAuthorized,
    OutsideBuildRegion,
    BlinkOutOfRange,
    FollowTargetNotFound,
    RepairTargetNotFound,
    NotFriendlyRepairTarget,
    RepairTargetNotRepairable,
}

impl From<BuilderCommandError> for WireBuilderCommandError {
    fn from(value: BuilderCommandError) -> Self {
        match value {
            BuilderCommandError::OrderQueueFull => Self::OrderQueueFull,
            BuilderCommandError::BuilderNotFound => Self::BuilderNotFound,
            BuilderCommandError::NotAuthorized => Self::NotAuthorized,
            BuilderCommandError::OutsideBuildRegion => Self::OutsideBuildRegion,
            BuilderCommandError::BlinkOutOfRange => Self::BlinkOutOfRange,
            BuilderCommandError::FollowTargetNotFound => Self::FollowTargetNotFound,
            BuilderCommandError::RepairTargetNotFound => Self::RepairTargetNotFound,
            BuilderCommandError::NotFriendlyRepairTarget => Self::NotFriendlyRepairTarget,
            BuilderCommandError::RepairTargetNotRepairable => Self::RepairTargetNotRepairable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireBuilderBuildError {
    Builder { reason: WireBuilderCommandError },
    TeamMismatch,
    MissingBuildingIdentity,
    MissingEconomyProfile,
    BuildingNotInCatalog,
    Resources { reason: WireResourcePurchaseError },
    Placement { reason: WireBuildingPlacementError },
}

impl From<BuilderBuildError> for WireBuilderBuildError {
    fn from(value: BuilderBuildError) -> Self {
        match value {
            BuilderBuildError::Builder(reason) => Self::Builder {
                reason: reason.into(),
            },
            BuilderBuildError::TeamMismatch => Self::TeamMismatch,
            BuilderBuildError::MissingBuildingIdentity => Self::MissingBuildingIdentity,
            BuilderBuildError::MissingEconomyProfile => Self::MissingEconomyProfile,
            BuilderBuildError::BuildingNotInCatalog => Self::BuildingNotInCatalog,
            BuilderBuildError::Resources(reason) => Self::Resources {
                reason: reason.into(),
            },
            BuilderBuildError::Placement(reason) => Self::Placement {
                reason: reason.into(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireConstructionCancelError {
    ConstructionNotFound,
    NotOwner,
}

impl From<BuildingConstructionCancelError> for WireConstructionCancelError {
    fn from(value: BuildingConstructionCancelError) -> Self {
        match value {
            BuildingConstructionCancelError::ConstructionNotFound => Self::ConstructionNotFound,
            BuildingConstructionCancelError::NotOwner => Self::NotOwner,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireBuildingUpgradeError {
    SourceNotFound,
    NotOwner,
    SourceUnderConstruction,
    SourceDisabled,
    ProductionQueueNotEmpty,
    SourceDefinitionMismatch,
    TeamMismatch,
    FootprintMismatch,
    MissingEconomyProfile,
    Resources { reason: WireResourcePurchaseError },
}

impl From<BuildingUpgradeError> for WireBuildingUpgradeError {
    fn from(value: BuildingUpgradeError) -> Self {
        match value {
            BuildingUpgradeError::SourceNotFound => Self::SourceNotFound,
            BuildingUpgradeError::NotOwner => Self::NotOwner,
            BuildingUpgradeError::SourceUnderConstruction => Self::SourceUnderConstruction,
            BuildingUpgradeError::SourceDisabled => Self::SourceDisabled,
            BuildingUpgradeError::ProductionQueueNotEmpty => Self::ProductionQueueNotEmpty,
            BuildingUpgradeError::SourceDefinitionMismatch => Self::SourceDefinitionMismatch,
            BuildingUpgradeError::TeamMismatch => Self::TeamMismatch,
            BuildingUpgradeError::FootprintMismatch => Self::FootprintMismatch,
            BuildingUpgradeError::MissingEconomyProfile => Self::MissingEconomyProfile,
            BuildingUpgradeError::Resources(reason) => Self::Resources {
                reason: reason.into(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireBuildingCommandError {
    SourceNotFound,
    NotAuthorized,
    SourceCannotAttack,
    SourceCannotProduce,
    SourceCannotCast,
    InsufficientMana,
    AbilityOnCooldown,
    ProductionQueueEmpty,
    ProductionQueueFull,
    TargetNotFound,
    FriendlyTarget,
    InvalidTargetType,
    TargetOutOfRange,
}

impl From<BuildingCommandError> for WireBuildingCommandError {
    fn from(value: BuildingCommandError) -> Self {
        match value {
            BuildingCommandError::SourceNotFound => Self::SourceNotFound,
            BuildingCommandError::NotAuthorized => Self::NotAuthorized,
            BuildingCommandError::SourceCannotAttack => Self::SourceCannotAttack,
            BuildingCommandError::SourceCannotProduce => Self::SourceCannotProduce,
            BuildingCommandError::SourceCannotCast => Self::SourceCannotCast,
            BuildingCommandError::InsufficientMana => Self::InsufficientMana,
            BuildingCommandError::AbilityOnCooldown => Self::AbilityOnCooldown,
            BuildingCommandError::ProductionQueueEmpty => Self::ProductionQueueEmpty,
            BuildingCommandError::ProductionQueueFull => Self::ProductionQueueFull,
            BuildingCommandError::TargetNotFound => Self::TargetNotFound,
            BuildingCommandError::FriendlyTarget => Self::FriendlyTarget,
            BuildingCommandError::InvalidTargetType => Self::InvalidTargetType,
            BuildingCommandError::TargetOutOfRange => Self::TargetOutOfRange,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireResourcePurchaseError {
    InsufficientGold { available: u32, required: u32 },
    InsufficientLumber { available: u32, required: u32 },
    InsufficientLegendaryPoints { available: u16, required: u16 },
}

impl From<ResourcePurchaseError> for WireResourcePurchaseError {
    fn from(value: ResourcePurchaseError) -> Self {
        match value {
            ResourcePurchaseError::InsufficientGold {
                available,
                required,
            } => Self::InsufficientGold {
                available,
                required,
            },
            ResourcePurchaseError::InsufficientLumber {
                available,
                required,
            } => Self::InsufficientLumber {
                available,
                required,
            },
            ResourcePurchaseError::InsufficientLegendaryPoints {
                available,
                required,
            } => Self::InsufficientLegendaryPoints {
                available,
                required,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireBuildingPlacementError {
    OutsideNavigation,
    OutsideBuildRegion,
    StaticObstacle,
    BuildingOverlap,
    BuildingReserved,
    UnitOccupied,
}

impl From<BuildingPlacementError> for WireBuildingPlacementError {
    fn from(value: BuildingPlacementError) -> Self {
        match value {
            BuildingPlacementError::OutsideNavigation => Self::OutsideNavigation,
            BuildingPlacementError::OutsideBuildRegion => Self::OutsideBuildRegion,
            BuildingPlacementError::StaticObstacle => Self::StaticObstacle,
            BuildingPlacementError::BuildingOverlap => Self::BuildingOverlap,
            BuildingPlacementError::BuildingReserved => Self::BuildingReserved,
            BuildingPlacementError::UnitOccupied => Self::UnitOccupied,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use castle_fight_sim::{
        AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION, CANONICAL_CHECKSUM_SCHEMA_VERSION,
    };

    fn compatibility() -> CompatibilityIdentity {
        CompatibilityIdentity {
            snapshot_schema_version: AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION,
            checksum_schema_version: CANONICAL_CHECKSUM_SCHEMA_VERSION,
            map_version: MapVersion::CASTLE_FIGHT_9_27,
            release_revision: "r1".to_owned(),
            content_schema_version: 1,
            content_gameplay_hash: 0x1234,
            configuration_identity: 0x5678,
        }
    }

    #[test]
    fn bounded_frame_round_trip_preserves_envelope() {
        let value = ProtocolEnvelope::new(ClientMessage::Hello {
            hello: ClientHello {
                compatibility: compatibility(),
                client_nonce: 7,
            },
        });
        let frame = encode_frame(&value).unwrap();
        let decoded: ProtocolEnvelope<ClientMessage> = decode_frame(&frame).unwrap();
        assert_eq!(decoded, value);
        assert_eq!(decoded.into_current().unwrap(), value.message);
    }

    #[test]
    fn bounded_reader_rejects_oversized_frame_before_allocating_body() {
        let oversized = u32::try_from(MAX_FRAME_BYTES + 1).unwrap().to_be_bytes();
        let error = read_frame::<_, ProtocolEnvelope<ClientMessage>>(&mut oversized.as_slice())
            .unwrap_err();
        assert!(matches!(error, FrameError::TooLarge { .. }));
    }

    #[test]
    fn decode_rejects_trailing_and_truncated_frames() {
        let value = ProtocolEnvelope::new(ClientMessage::CheckpointReport {
            report: CheckpointReport {
                completed_tick: 4,
                checksum: 9,
            },
        });
        let mut frame = encode_frame(&value).unwrap();
        let truncated = &frame[..frame.len() - 1];
        assert!(matches!(
            decode_frame::<ProtocolEnvelope<ClientMessage>>(truncated),
            Err(FrameError::Io(error)) if error.kind() == io::ErrorKind::UnexpectedEof
        ));
        frame.push(0);
        assert!(matches!(
            decode_frame::<ProtocolEnvelope<ClientMessage>>(&frame),
            Err(FrameError::TrailingBytes { trailing: 1 })
        ));
    }

    #[test]
    fn schema_and_compatibility_mismatch_are_explicit() {
        let envelope = ProtocolEnvelope {
            schema_version: PROTOCOL_SCHEMA_VERSION + 1,
            message: (),
        };
        assert_eq!(
            envelope.into_current(),
            Err(ProtocolSchemaError {
                expected: PROTOCOL_SCHEMA_VERSION,
                actual: PROTOCOL_SCHEMA_VERSION + 1,
            })
        );

        let expected = compatibility();
        let mut actual = expected.clone();
        actual.content_gameplay_hash ^= 1;
        assert!(matches!(
            actual.mismatch(&expected),
            Some(CompatibilityMismatch::Content { .. })
        ));
    }

    #[test]
    fn reconnect_credentials_round_trip_without_claiming_player_identity() {
        let token = ReconnectToken {
            bytes: [0x5a; RECONNECT_TOKEN_BYTES],
        };
        let value = ProtocolEnvelope::new(ClientMessage::Reconnect {
            reconnect: ReconnectHello {
                compatibility: compatibility(),
                session_id: 42,
                reconnect_token: token,
            },
        });
        let frame = encode_frame(&value).unwrap();
        let decoded: ProtocolEnvelope<ClientMessage> = decode_frame(&frame).unwrap();
        assert_eq!(decoded, value);

        let json = String::from_utf8(frame[4..].to_vec()).unwrap();
        assert!(json.contains("\"session_id\":42"));
        assert!(!json.contains("player_id"));
        assert!(!json.contains("\"player\""));
        assert_eq!(format!("{token:?}"), "ReconnectToken([REDACTED])");
    }

    #[test]
    fn player_commands_round_trip_through_wire_without_player_identity() {
        let commands = [
            PlayerCommand::MoveBuilder {
                builder: SimId(1),
                destination: SimPoint::new(-2, 3),
            },
            PlayerCommand::FollowWithBuilder {
                builder: SimId(1),
                target: SimId(2),
            },
            PlayerCommand::StopBuilder { builder: SimId(1) },
            PlayerCommand::BlinkBuilder {
                builder: SimId(1),
                destination: SimPoint::new(4, 5),
            },
            PlayerCommand::RepairWithBuilder {
                builder: SimId(1),
                target: SimId(3),
            },
            PlayerCommand::SetBuilderRepairAutocast {
                builder: SimId(1),
                enabled: false,
            },
            PlayerCommand::PlaceBuilding {
                builder: SimId(1),
                building: CastleFightBuildingId(12),
                position: BuildPosition::new(7, 8),
            },
            PlayerCommand::QueueBuilderCommand {
                builder: SimId(1),
                command: BuilderQueuedCommand::Build {
                    building: CastleFightBuildingId(12),
                    position: BuildPosition::new(7, 8),
                },
            },
            PlayerCommand::QueueBuilderCommand {
                builder: SimId(1),
                command: BuilderQueuedCommand::Move {
                    destination: SimPoint::new(4, 5),
                },
            },
            PlayerCommand::QueueBuilderCommand {
                builder: SimId(1),
                command: BuilderQueuedCommand::Follow { target: SimId(2) },
            },
            PlayerCommand::QueueBuilderCommand {
                builder: SimId(1),
                command: BuilderQueuedCommand::Repair { target: SimId(3) },
            },
            PlayerCommand::QueueBuilderCommand {
                builder: SimId(1),
                command: BuilderQueuedCommand::Blink {
                    destination: SimPoint::new(4, 5),
                },
            },
            PlayerCommand::QueueBuilderCommand {
                builder: SimId(1),
                command: BuilderQueuedCommand::Stop,
            },
            PlayerCommand::CancelBuildingConstruction { building: SimId(9) },
            PlayerCommand::QueueProductionUnit { building: SimId(9) },
            PlayerCommand::CancelProductionUnit { building: SimId(9) },
            PlayerCommand::UpgradeBuilding {
                building: SimId(9),
                target: CastleFightBuildingId(13),
            },
            PlayerCommand::AttackWithBuilding {
                building: SimId(9),
                target: SimId(10),
            },
            PlayerCommand::CastBuildingSpellAt {
                building: SimId(9),
                position: SimPoint::new(-123, 456),
            },
            PlayerCommand::CastBuildingSpell { building: SimId(9) },
            PlayerCommand::SetBuildingSpellAutocast {
                building: SimId(9),
                enabled: false,
            },
        ];
        for command in commands {
            let wire: WirePlayerCommand = command.into();
            assert_eq!(PlayerCommand::from(wire), command);
        }
    }

    #[test]
    fn canonical_stream_record_round_trip_preserves_ordering_fields() {
        let record = CanonicalStreamRecord::Tick(FinalizedTickInputs {
            tick: 11,
            stream_position: InputStreamPosition(12),
            commands: vec![ScheduledCommand {
                tick: 11,
                order: CommandOrder(0),
                player: PlayerId(3),
                client_sequence: ClientCommandSequence(4),
                command: PlayerCommand::StopBuilder { builder: SimId(5) },
            }],
        });
        let wire = WireCanonicalStreamRecord::from(&record);
        assert_eq!(CanonicalStreamRecord::from(wire), record);
    }

    #[test]
    fn inbound_messages_reject_unknown_fields() {
        let json = r#"{"kind":"checkpoint_report","report":{"completed_tick":4,"checksum":9},"unexpected":1}"#;
        assert!(serde_json::from_str::<ClientMessage>(json).is_err());
    }

    #[test]
    fn compatibility_identity_rejects_invalid_release_revision_shape() {
        let mut identity = compatibility();
        identity.release_revision.clear();
        assert_eq!(
            identity.validate_shape(),
            Err(MessageValidationError::InvalidReleaseRevision)
        );

        identity.release_revision = "x".repeat(MAX_RELEASE_REVISION_BYTES + 1);
        assert_eq!(
            identity.validate_shape(),
            Err(MessageValidationError::InvalidReleaseRevision)
        );
    }
}
