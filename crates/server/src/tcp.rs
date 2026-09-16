use std::{
    collections::BTreeMap,
    fmt, io,
    net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    thread,
    time::{Duration, Instant},
};

use castle_fight_protocol::{
    CatchUpComplete, ClientMessage, FrameError, HandshakeRejectReason, MAX_SNAPSHOT_BYTES,
    ProtocolEnvelope, ProtocolErrorCode, ProtocolSchemaError, SNAPSHOT_CHUNK_BYTES, ServerMessage,
    SnapshotChunk, SnapshotTransferBegin, WireCanonicalStreamRecord, read_frame, write_frame,
};
use castle_fight_sim::{
    CASTLE_FIGHT_SIMULATION_HZ, CanonicalStreamRecord, InputStreamPosition, MatchLifecycle,
};

use crate::{
    AuthoritativeMatch, HandshakeResult, OutboundMessage, OutboundTarget, ServerMatchError,
    SessionId,
};

const OUTBOUND_QUEUE_CAPACITY: usize = 256;
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(1);
const MAX_RECONNECT_SNAPSHOT_CHUNKS: usize = MAX_SNAPSHOT_BYTES.div_ceil(SNAPSHOT_CHUNK_BYTES);
const MAX_RECONNECT_HISTORY_RECORDS: usize = 8;
const MAX_RECONNECT_HANDOFF_MESSAGES: usize =
    3 + MAX_RECONNECT_SNAPSHOT_CHUNKS + MAX_RECONNECT_HISTORY_RECORDS;
const _: () = assert!(MAX_RECONNECT_HANDOFF_MESSAGES <= OUTBOUND_QUEUE_CAPACITY);

#[derive(Debug)]
pub enum TcpServerError {
    Io(io::Error),
    Match(ServerMatchError),
    ConnectionIdExhausted,
    ReconnectHistoryUnavailable,
    ReconnectHistoryOverflow,
}

impl fmt::Display for TcpServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "TCP server I/O error: {error}"),
            Self::Match(error) => error.fmt(formatter),
            Self::ConnectionIdExhausted => formatter.write_str("TCP connection ID space exhausted"),
            Self::ReconnectHistoryUnavailable => formatter
                .write_str("reconnect snapshot boundary is no longer present in canonical history"),
            Self::ReconnectHistoryOverflow => {
                formatter.write_str("reconnect history tail exceeds the bounded handoff allowance")
            }
        }
    }
}

impl std::error::Error for TcpServerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Match(error) => Some(error),
            Self::ConnectionIdExhausted
            | Self::ReconnectHistoryUnavailable
            | Self::ReconnectHistoryOverflow => None,
        }
    }
}

impl From<io::Error> for TcpServerError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ServerMatchError> for TcpServerError {
    fn from(error: ServerMatchError) -> Self {
        Self::Match(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnectionId(pub u64);

struct ConnectionState {
    session_id: Option<SessionId>,
    outbound: SyncSender<ServerMessage>,
    shutdown: TcpStream,
}

enum InboundEvent {
    Frame(ConnectionId, ProtocolEnvelope<ClientMessage>),
    Malformed(ConnectionId),
    Disconnected(ConnectionId),
}

/// TCP transport shell around one authoritative match.
///
/// Socket reader/writer threads only decode/encode protocol frames and move messages through
/// bounded channels. All session assignment, command admission, canonical ordering and simulation
/// mutation remain on the thread calling `poll_network` / `finalize_next_tick` / `run_until_match_end`.
pub struct TcpAuthoritativeServer {
    listener: TcpListener,
    authoritative: AuthoritativeMatch,
    inbound_tx: mpsc::Sender<InboundEvent>,
    inbound_rx: Receiver<InboundEvent>,
    connections: BTreeMap<ConnectionId, ConnectionState>,
    session_connections: BTreeMap<SessionId, ConnectionId>,
    next_connection_id: u64,
    started: bool,
    team_disconnect_since: [Option<Instant>; 2],
}

impl TcpAuthoritativeServer {
    pub fn bind<A: ToSocketAddrs>(
        address: A,
        authoritative: AuthoritativeMatch,
    ) -> Result<Self, TcpServerError> {
        let listener = TcpListener::bind(address)?;
        listener.set_nonblocking(true)?;
        let (inbound_tx, inbound_rx) = mpsc::channel();
        Ok(Self {
            listener,
            authoritative,
            inbound_tx,
            inbound_rx,
            connections: BTreeMap::new(),
            session_connections: BTreeMap::new(),
            next_connection_id: 1,
            started: false,
            team_disconnect_since: [None; 2],
        })
    }

    #[must_use]
    pub const fn authoritative(&self) -> &AuthoritativeMatch {
        &self.authoritative
    }

    pub fn local_addr(&self) -> Result<SocketAddr, io::Error> {
        self.listener.local_addr()
    }

    #[must_use]
    pub const fn is_started(&self) -> bool {
        self.started
    }

    #[must_use]
    pub fn authenticated_connection_count(&self) -> usize {
        self.session_connections.len()
    }

    pub fn poll_network(&mut self) -> Result<(), TcpServerError> {
        self.poll_network_at(Instant::now())
    }

    fn poll_network_at(&mut self, now: Instant) -> Result<(), TcpServerError> {
        self.accept_pending_connections()?;
        loop {
            match self.inbound_rx.try_recv() {
                Ok(event) => self.process_inbound(event)?,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        if !self.started && self.authoritative.all_players_connected() {
            self.started = true;
        }
        self.update_disconnect_timeouts(now)
    }

    fn update_disconnect_timeouts(&mut self, now: Instant) -> Result<(), TcpServerError> {
        if !self.started {
            self.team_disconnect_since = [None; 2];
            return Ok(());
        }
        let disconnected_teams_mask = match self.authoritative.simulation().lifecycle() {
            MatchLifecycle::PausedForDisconnect {
                disconnected_teams_mask,
            } => disconnected_teams_mask,
            MatchLifecycle::Running | MatchLifecycle::Finished { .. } => {
                self.team_disconnect_since = [None; 2];
                return Ok(());
            }
        };

        for (team, disconnected_since) in self.team_disconnect_since.iter_mut().enumerate() {
            let bit = 1_u8 << team;
            if disconnected_teams_mask & bit == 0 {
                *disconnected_since = None;
            } else if disconnected_since.is_none() {
                *disconnected_since = Some(now);
            }
        }

        let timeout = self.authoritative.disconnect_timeout();
        let mut timed_out_teams_mask = 0_u8;
        for (team, disconnected_since) in self.team_disconnect_since.iter().enumerate() {
            if disconnected_since
                .is_some_and(|since| now.saturating_duration_since(since) >= timeout)
            {
                timed_out_teams_mask |= 1_u8 << team;
            }
        }
        if timed_out_teams_mask == 0 {
            return Ok(());
        }

        let outbound = self
            .authoritative
            .expire_disconnect_timeout(timed_out_teams_mask)?;
        self.dispatch_all(outbound)?;
        if matches!(
            self.authoritative.simulation().lifecycle(),
            MatchLifecycle::Finished { .. }
        ) {
            self.team_disconnect_since = [None; 2];
        }
        Ok(())
    }

    pub fn finalize_next_tick(&mut self) -> Result<(), TcpServerError> {
        let outbound = self.authoritative.finalize_next_tick()?;
        self.dispatch_all(outbound)
    }

    pub fn run_until_match_end(mut self) -> Result<(), TcpServerError> {
        let tick_hz = f64::from(CASTLE_FIGHT_SIMULATION_HZ);
        let tick_period = Duration::from_secs_f64(1.0 / tick_hz);
        let mut next_tick_deadline = Instant::now() + tick_period;

        loop {
            self.poll_network()?;
            if matches!(
                self.authoritative.simulation().lifecycle(),
                MatchLifecycle::Finished { .. }
            ) {
                return Ok(());
            }
            if !self.started
                || matches!(
                    self.authoritative.simulation().lifecycle(),
                    MatchLifecycle::PausedForDisconnect { .. }
                )
            {
                thread::sleep(IDLE_POLL_INTERVAL);
                next_tick_deadline = Instant::now() + tick_period;
                continue;
            }

            let now = Instant::now();
            if now >= next_tick_deadline {
                self.finalize_next_tick()?;
                next_tick_deadline += tick_period;
                continue;
            }
            thread::sleep((next_tick_deadline - now).min(IDLE_POLL_INTERVAL));
        }
    }

    fn accept_pending_connections(&mut self) -> Result<(), TcpServerError> {
        loop {
            match self.listener.accept() {
                Ok((stream, _peer)) => self.register_connection(stream)?,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn register_connection(&mut self, stream: TcpStream) -> Result<(), TcpServerError> {
        let Some(next_connection_id) = self.next_connection_id.checked_add(1) else {
            return Err(TcpServerError::ConnectionIdExhausted);
        };
        let connection_id = ConnectionId(self.next_connection_id);
        self.next_connection_id = next_connection_id;

        stream.set_nodelay(true)?;
        let reader = stream.try_clone()?;
        let writer = stream.try_clone()?;
        let shutdown = stream;
        let (outbound, outbound_rx) = mpsc::sync_channel(OUTBOUND_QUEUE_CAPACITY);
        spawn_reader(connection_id, reader, self.inbound_tx.clone());
        spawn_writer(writer, outbound_rx);
        self.connections.insert(
            connection_id,
            ConnectionState {
                session_id: None,
                outbound,
                shutdown,
            },
        );
        Ok(())
    }

    fn process_inbound(&mut self, event: InboundEvent) -> Result<(), TcpServerError> {
        match event {
            InboundEvent::Frame(connection_id, envelope) => {
                self.process_frame(connection_id, envelope)
            }
            InboundEvent::Malformed(connection_id) | InboundEvent::Disconnected(connection_id) => {
                self.disconnect_connection(connection_id)
            }
        }
    }

    fn process_frame(
        &mut self,
        connection_id: ConnectionId,
        envelope: ProtocolEnvelope<ClientMessage>,
    ) -> Result<(), TcpServerError> {
        let Some(session_id) = self
            .connections
            .get(&connection_id)
            .and_then(|connection| connection.session_id)
        else {
            return self.process_unbound_frame(connection_id, envelope);
        };

        let message = match envelope.into_current() {
            Ok(message) => message,
            Err(_) => {
                self.send_to_connection(
                    connection_id,
                    ServerMessage::ProtocolError {
                        code: ProtocolErrorCode::MalformedMessage,
                    },
                )?;
                return Ok(());
            }
        };
        let outbound = self
            .authoritative
            .handle_client_message(session_id, message);
        self.dispatch_all(outbound)
    }

    fn process_unbound_frame(
        &mut self,
        connection_id: ConnectionId,
        envelope: ProtocolEnvelope<ClientMessage>,
    ) -> Result<(), TcpServerError> {
        let message = match envelope.into_current() {
            Ok(message) => message,
            Err(ProtocolSchemaError { expected, actual }) => {
                self.send_to_connection(
                    connection_id,
                    ServerMessage::HelloRejected {
                        reason: HandshakeRejectReason::ProtocolSchema { expected, actual },
                    },
                )?;
                return Ok(());
            }
        };
        match message {
            ClientMessage::Hello { hello } => {
                let result = self.authoritative.accept_hello(hello);
                if let HandshakeResult::Accepted { session_id, .. } = &result {
                    self.bind_session_connection(connection_id, *session_id);
                }
                self.send_to_connection(connection_id, result.message())
            }
            ClientMessage::Reconnect { reconnect } => {
                let session_id = match self.authoritative.authenticate_reconnect(&reconnect) {
                    Ok(session_id) => session_id,
                    Err(reason) => {
                        self.send_to_connection(
                            connection_id,
                            ServerMessage::HelloRejected { reason },
                        )?;
                        return Ok(());
                    }
                };

                // Pin a fresh authoritative snapshot while this session is still canonically
                // disconnected. The Connected control is then the first history record after the
                // snapshot boundary and is delivered exactly once through this catch-up tail.
                let snapshot_stream_position = self.authoritative.driver().next_stream_position();
                let snapshot = self.authoritative.simulation().capture_snapshot();
                let snapshot_completed_tick = snapshot.completed_tick();
                let snapshot_checksum = snapshot.checksum();
                let snapshot_bytes = match snapshot.encode_wire() {
                    Ok(bytes) if bytes.len() <= MAX_SNAPSHOT_BYTES => bytes,
                    Ok(_) | Err(_) => {
                        self.send_to_connection(
                            connection_id,
                            ServerMessage::HelloRejected {
                                reason: HandshakeRejectReason::MatchUnavailable,
                            },
                        )?;
                        return Ok(());
                    }
                };

                // The replacement socket is deliberately still unbound here, so this canonical
                // reconnect record broadcasts only to already-live peers. Subsequent live records
                // cannot race the handoff because all authoritative mutation is serialized on this
                // server-loop thread.
                let outbound = self.authoritative.reconnect_session(session_id)?;
                self.dispatch_all(outbound)?;
                let handoff_stream_position = self.authoritative.driver().next_stream_position();
                let history_tail = reconnect_history_tail(
                    self.authoritative.driver().history(),
                    snapshot_stream_position,
                    handoff_stream_position,
                )?
                .iter()
                .map(WireCanonicalStreamRecord::from)
                .collect::<Vec<_>>();
                if history_tail.len() > MAX_RECONNECT_HISTORY_RECORDS {
                    return Err(TcpServerError::ReconnectHistoryOverflow);
                }

                let assignment = self
                    .authoritative
                    .session_assignment(session_id)
                    .expect("authenticated reconnect session must still exist");
                let chunk_count = snapshot_bytes.len().div_ceil(SNAPSHOT_CHUNK_BYTES);
                let chunk_count = u32::try_from(chunk_count)
                    .expect("bounded reconnect snapshot chunk count fits u32");
                let begin = SnapshotTransferBegin {
                    transfer_id: connection_id.0,
                    snapshot_stream_position: snapshot_stream_position.0,
                    snapshot_completed_tick,
                    snapshot_checksum,
                    snapshot_bytes: u32::try_from(snapshot_bytes.len())
                        .expect("bounded reconnect snapshot length fits u32"),
                    chunk_count,
                    handoff_stream_position: handoff_stream_position.0,
                };

                self.send_to_connection(
                    connection_id,
                    ServerMessage::HelloAccepted { assignment },
                )?;
                self.send_to_connection(connection_id, ServerMessage::SnapshotBegin { begin })?;
                for (chunk_index, bytes) in snapshot_bytes.chunks(SNAPSHOT_CHUNK_BYTES).enumerate()
                {
                    self.send_to_connection(
                        connection_id,
                        ServerMessage::SnapshotChunk {
                            chunk: SnapshotChunk {
                                transfer_id: connection_id.0,
                                chunk_index: u32::try_from(chunk_index)
                                    .expect("bounded reconnect chunk index fits u32"),
                                bytes: bytes.to_vec(),
                            },
                        },
                    )?;
                }
                for record in history_tail {
                    self.send_to_connection(connection_id, ServerMessage::StreamRecord { record })?;
                }
                self.send_to_connection(
                    connection_id,
                    ServerMessage::CatchUpComplete {
                        complete: CatchUpComplete {
                            transfer_id: connection_id.0,
                            handoff_stream_position: handoff_stream_position.0,
                            completed_tick: self.authoritative.simulation().tick().checked_sub(1),
                            checksum: self.authoritative.simulation().checksum(),
                        },
                    },
                )?;

                // Every handoff message is already queued on this connection before it becomes a
                // live broadcast target. FIFO channel/socket ordering makes later canonical records
                // follow the catch-up completion marker without overlap or a gap.
                self.bind_session_connection(connection_id, session_id);
                Ok(())
            }
            ClientMessage::SubmitCommand { .. } | ClientMessage::CheckpointReport { .. } => self
                .send_to_connection(
                    connection_id,
                    ServerMessage::ProtocolError {
                        code: ProtocolErrorCode::ExpectedHello,
                    },
                ),
        }
    }

    fn bind_session_connection(&mut self, connection_id: ConnectionId, session_id: SessionId) {
        if let Some(connection) = self.connections.get_mut(&connection_id) {
            connection.session_id = Some(session_id);
        }
        self.session_connections.insert(session_id, connection_id);
    }

    fn dispatch_all(&mut self, outbound: Vec<OutboundMessage>) -> Result<(), TcpServerError> {
        for outbound in outbound {
            match outbound.target {
                OutboundTarget::Session(session_id) => {
                    if let Some(connection_id) = self.session_connections.get(&session_id).copied()
                    {
                        self.send_to_connection(connection_id, outbound.message)?;
                    }
                }
                OutboundTarget::Broadcast => {
                    let connection_ids = self
                        .session_connections
                        .values()
                        .copied()
                        .collect::<Vec<_>>();
                    for connection_id in connection_ids {
                        self.send_to_connection(connection_id, outbound.message.clone())?;
                    }
                }
            }
        }
        Ok(())
    }

    fn send_to_connection(
        &mut self,
        connection_id: ConnectionId,
        message: ServerMessage,
    ) -> Result<(), TcpServerError> {
        let send_result = self
            .connections
            .get(&connection_id)
            .map(|connection| connection.outbound.try_send(message));
        if matches!(
            send_result,
            Some(Err(TrySendError::Full(_) | TrySendError::Disconnected(_)))
        ) {
            self.disconnect_connection(connection_id)?;
        }
        Ok(())
    }

    fn disconnect_connection(&mut self, connection_id: ConnectionId) -> Result<(), TcpServerError> {
        let Some(connection) = self.connections.remove(&connection_id) else {
            return Ok(());
        };
        let _ = connection.shutdown.shutdown(Shutdown::Both);
        if let Some(session_id) = connection.session_id {
            self.session_connections.remove(&session_id);
            let outbound = self.authoritative.disconnect_session(session_id)?;
            self.dispatch_all(outbound)?;
        }
        Ok(())
    }
}

fn reconnect_history_tail(
    history: &[CanonicalStreamRecord],
    start: InputStreamPosition,
    end: InputStreamPosition,
) -> Result<&[CanonicalStreamRecord], TcpServerError> {
    let start =
        usize::try_from(start.0).map_err(|_| TcpServerError::ReconnectHistoryUnavailable)?;
    let end = usize::try_from(end.0).map_err(|_| TcpServerError::ReconnectHistoryUnavailable)?;
    if start > end || end > history.len() {
        return Err(TcpServerError::ReconnectHistoryUnavailable);
    }
    Ok(&history[start..end])
}

fn spawn_reader(
    connection_id: ConnectionId,
    mut stream: TcpStream,
    inbound: mpsc::Sender<InboundEvent>,
) {
    thread::Builder::new()
        .name(format!("cf-net-read-{}", connection_id.0))
        .spawn(move || {
            loop {
                match read_frame::<_, ProtocolEnvelope<ClientMessage>>(&mut stream) {
                    Ok(frame) => {
                        if inbound
                            .send(InboundEvent::Frame(connection_id, frame))
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(FrameError::Io(error))
                        if matches!(
                            error.kind(),
                            io::ErrorKind::UnexpectedEof
                                | io::ErrorKind::ConnectionAborted
                                | io::ErrorKind::ConnectionReset
                                | io::ErrorKind::NotConnected
                                | io::ErrorKind::BrokenPipe
                        ) =>
                    {
                        let _ = inbound.send(InboundEvent::Disconnected(connection_id));
                        break;
                    }
                    Err(_) => {
                        let _ = inbound.send(InboundEvent::Malformed(connection_id));
                        break;
                    }
                }
            }
        })
        .expect("network reader thread creation must succeed");
}

fn spawn_writer(mut stream: TcpStream, outbound: Receiver<ServerMessage>) {
    thread::Builder::new()
        .name("cf-net-write".to_owned())
        .spawn(move || {
            while let Ok(message) = outbound.recv() {
                let envelope = ProtocolEnvelope::new(message);
                if write_frame(&mut stream, &envelope).is_err() {
                    break;
                }
            }
        })
        .expect("network writer thread creation must succeed");
}

#[cfg(test)]
mod tests {
    use super::*;
    use castle_fight_protocol::{
        CheckpointReport, ClientHello, CommandAcknowledgement, CommandRequest, ReconnectHello,
        SessionAssignment, WireAdmissionError, WireCanonicalStreamRecord, WireCommandExecution,
        WirePlayerCommand,
    };
    use castle_fight_sim::{
        CanonicalStreamRecord, CastleFightMatchConfig, ClientCommandSequence, MapVersion,
        MatchDriver, PlayerId, Simulation, create_castle_fight_match,
    };

    use crate::ServerMatchOptions;

    fn server() -> TcpAuthoritativeServer {
        server_with_options(ServerMatchOptions::default())
    }

    fn server_with_options(options: ServerMatchOptions) -> TcpAuthoritativeServer {
        let config =
            CastleFightMatchConfig::development_subset(MapVersion::CASTLE_FIGHT_9_27, "r1", 0x1234)
                .unwrap();
        let authoritative = AuthoritativeMatch::new(config, 1, options).unwrap();
        TcpAuthoritativeServer::bind("127.0.0.1:0", authoritative).unwrap()
    }

    fn connect(server: &TcpAuthoritativeServer) -> TcpStream {
        let stream = TcpStream::connect(server.local_addr().unwrap()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        stream
    }

    fn send_client(stream: &mut TcpStream, message: ClientMessage) {
        write_frame(stream, &ProtocolEnvelope::new(message)).unwrap();
    }

    fn receive_server(stream: &mut TcpStream) -> ServerMessage {
        let envelope: ProtocolEnvelope<ServerMessage> = read_frame(stream).unwrap();
        envelope.into_current().unwrap()
    }

    fn pump_until(server: &mut TcpAuthoritativeServer, predicate: impl Fn(&SelfView) -> bool) {
        for _ in 0..100 {
            server.poll_network().unwrap();
            let view = SelfView {
                authenticated: server.authenticated_connection_count(),
                started: server.is_started(),
                pending_commands: server.authoritative().driver().pending_commands().len(),
            };
            if predicate(&view) {
                return;
            }
            thread::sleep(Duration::from_millis(2));
        }
        panic!("TCP server did not reach expected state");
    }

    struct SelfView {
        authenticated: usize,
        started: bool,
        pending_commands: usize,
    }

    fn hello(server: &TcpAuthoritativeServer, nonce: u64) -> ClientMessage {
        ClientMessage::Hello {
            hello: ClientHello {
                compatibility: server.authoritative().compatibility().clone(),
                client_nonce: nonce,
            },
        }
    }

    fn pump_network(server: &mut TcpAuthoritativeServer) {
        for _ in 0..20 {
            server.poll_network().unwrap();
            thread::sleep(Duration::from_millis(1));
        }
    }

    struct ReplicaPeer {
        stream: TcpStream,
        simulation: Simulation,
        driver: MatchDriver,
        player: PlayerId,
    }

    impl ReplicaPeer {
        fn connect(
            server: &mut TcpAuthoritativeServer,
            config: CastleFightMatchConfig,
            nonce: u64,
            workers: usize,
            expected_connections: usize,
        ) -> Self {
            let mut stream = connect(server);
            server.poll_network().unwrap();
            send_client(&mut stream, hello(server, nonce));
            pump_until(server, |view| view.authenticated >= expected_connections);
            let assignment = match receive_server(&mut stream) {
                ServerMessage::HelloAccepted { assignment } => assignment,
                message => panic!("expected accepted replica handshake, got {message:?}"),
            };
            let game = create_castle_fight_match(config, workers).unwrap();
            let driver = MatchDriver::new(&game.simulation, game.content);
            Self {
                stream,
                simulation: game.simulation,
                driver,
                player: PlayerId(assignment.player_id),
            }
        }

        fn send_command(&mut self, sequence: u64, command: WirePlayerCommand) {
            send_client(
                &mut self.stream,
                ClientMessage::SubmitCommand {
                    request: CommandRequest {
                        client_sequence: sequence,
                        observed_completed_tick: self.simulation.tick().checked_sub(1),
                        command,
                    },
                },
            );
        }

        fn apply_finalized_tick(&mut self) -> CanonicalStreamRecord {
            let record = match receive_server(&mut self.stream) {
                ServerMessage::StreamRecord { record } => CanonicalStreamRecord::from(record),
                message => panic!("expected canonical stream record, got {message:?}"),
            };
            let result = self
                .driver
                .apply_stream_record(&mut self.simulation, record.clone())
                .unwrap()
                .expect("test server finalizes a simulation tick");
            let expected_executions = result
                .executions
                .iter()
                .copied()
                .map(WireCommandExecution::from)
                .collect::<Vec<_>>();
            match receive_server(&mut self.stream) {
                ServerMessage::TickExecutions { batch } => {
                    assert_eq!(batch.tick, result.finalized.tick);
                    assert_eq!(batch.executions, expected_executions);
                }
                message => panic!("expected deterministic execution batch, got {message:?}"),
            }
            match receive_server(&mut self.stream) {
                ServerMessage::Checkpoint { checkpoint } => {
                    assert_eq!(checkpoint.completed_tick, result.finalized.tick);
                    assert_eq!(checkpoint.checksum, self.simulation.checksum());
                    send_client(
                        &mut self.stream,
                        ClientMessage::CheckpointReport {
                            report: CheckpointReport {
                                completed_tick: checkpoint.completed_tick,
                                checksum: self.simulation.checksum(),
                            },
                        },
                    );
                }
                message => panic!("expected authoritative checkpoint, got {message:?}"),
            }
            record
        }

        fn apply_control(&mut self) -> CanonicalStreamRecord {
            let record = match receive_server(&mut self.stream) {
                ServerMessage::StreamRecord { record } => CanonicalStreamRecord::from(record),
                message => panic!("expected canonical control record, got {message:?}"),
            };
            assert!(matches!(record, CanonicalStreamRecord::Control(_)));
            let result = self
                .driver
                .apply_stream_record(&mut self.simulation, record.clone())
                .unwrap();
            assert!(result.is_none());
            record
        }
    }

    #[test]
    fn tcp_handshake_assigns_sessions_and_match_starts_only_when_roster_is_connected() {
        let mut server = server();
        let mut first = connect(&server);
        let mut second = connect(&server);
        server.poll_network().unwrap();

        send_client(&mut first, hello(&server, 1));
        pump_until(&mut server, |view| view.authenticated == 1);
        assert!(!server.is_started());
        assert!(matches!(
            receive_server(&mut first),
            ServerMessage::HelloAccepted {
                assignment: SessionAssignment { player_id: 0, .. }
            }
        ));

        send_client(&mut second, hello(&server, 2));
        pump_until(&mut server, |view| view.authenticated == 2 && view.started);
        assert!(matches!(
            receive_server(&mut second),
            ServerMessage::HelloAccepted {
                assignment: SessionAssignment { player_id: 6, .. }
            }
        ));
    }

    #[test]
    fn tcp_team_disconnect_timeout_uses_wall_clock_only_to_emit_terminal_control() {
        let disconnect_timeout = Duration::from_secs(10);
        let mut server = server_with_options(ServerMatchOptions {
            disconnect_timeout,
            ..ServerMatchOptions::default()
        });
        let mut first = connect(&server);
        let mut second = connect(&server);
        server.poll_network().unwrap();
        send_client(&mut first, hello(&server, 1));
        send_client(&mut second, hello(&server, 2));
        pump_until(&mut server, |view| view.started);
        let _ = receive_server(&mut first);
        let _ = receive_server(&mut second);

        second.shutdown(Shutdown::Both).unwrap();
        drop(second);
        pump_until(&mut server, |view| view.authenticated == 1);
        assert_eq!(
            server.authoritative().simulation().lifecycle(),
            MatchLifecycle::PausedForDisconnect {
                disconnected_teams_mask: 0b10,
            }
        );
        let disconnected_since = server.team_disconnect_since[1]
            .expect("team-wide disconnect must start an operational deadline");
        let history_len = server.authoritative().driver().history().len();

        server
            .update_disconnect_timeouts(
                disconnected_since + disconnect_timeout - Duration::from_millis(1),
            )
            .unwrap();
        assert_eq!(server.authoritative().driver().history().len(), history_len);
        assert!(matches!(
            server.authoritative().simulation().lifecycle(),
            MatchLifecycle::PausedForDisconnect { .. }
        ));

        server
            .update_disconnect_timeouts(disconnected_since + disconnect_timeout)
            .unwrap();
        assert_eq!(
            server.authoritative().driver().history().len(),
            history_len + 1
        );
        assert!(matches!(
            server.authoritative().driver().history().last(),
            Some(CanonicalStreamRecord::Control(control))
                if matches!(
                    control.event,
                    castle_fight_sim::MatchControlEvent::FinishMatch {
                        outcome: castle_fight_sim::MatchOutcome::Victory(castle_fight_sim::Team(0)),
                    }
                )
        ));
        assert!(matches!(
            server.authoritative().simulation().lifecycle(),
            MatchLifecycle::Finished {
                outcome: castle_fight_sim::MatchOutcome::Victory(castle_fight_sim::Team(0)),
                ..
            }
        ));
        assert_eq!(server.team_disconnect_since, [None; 2]);
    }

    #[test]
    fn tcp_duplicate_command_is_acknowledged_once_and_finalized_once() {
        let mut server = server();
        let mut first = connect(&server);
        let mut second = connect(&server);
        server.poll_network().unwrap();
        send_client(&mut first, hello(&server, 1));
        send_client(&mut second, hello(&server, 2));
        pump_until(&mut server, |view| view.started);
        let _ = receive_server(&mut first);
        let _ = receive_server(&mut second);

        let builder = server
            .authoritative()
            .simulation()
            .builder_for_player(PlayerId(0))
            .unwrap()
            .id;
        let request = ClientMessage::SubmitCommand {
            request: CommandRequest {
                client_sequence: 0,
                observed_completed_tick: None,
                command: WirePlayerCommand::StopBuilder { builder: builder.0 },
            },
        };
        send_client(&mut first, request.clone());
        pump_until(&mut server, |view| view.pending_commands == 1);
        assert!(matches!(
            receive_server(&mut first),
            ServerMessage::CommandAcknowledged {
                acknowledgement: CommandAcknowledgement::Scheduled {
                    duplicate: false,
                    ..
                }
            }
        ));

        send_client(&mut first, request);
        let mut duplicate_reader = first.try_clone().unwrap();
        let (ack_tx, ack_rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = ack_tx.send(receive_server(&mut duplicate_reader));
        });
        let duplicate_ack = (0..100).find_map(|_| {
            server.poll_network().unwrap();
            match ack_rx.try_recv() {
                Ok(message) => Some(message),
                Err(TryRecvError::Empty) => {
                    thread::sleep(Duration::from_millis(2));
                    None
                }
                Err(TryRecvError::Disconnected) => {
                    panic!("duplicate acknowledgement reader stopped")
                }
            }
        });
        assert!(matches!(
            duplicate_ack,
            Some(ServerMessage::CommandAcknowledged {
                acknowledgement: CommandAcknowledgement::Scheduled {
                    duplicate: true,
                    ..
                }
            })
        ));
        assert_eq!(server.authoritative().driver().pending_commands().len(), 1);

        server.finalize_next_tick().unwrap();
        let mut saw_tick = false;
        for _ in 0..2 {
            if let ServerMessage::StreamRecord {
                record: WireCanonicalStreamRecord::Tick { commands, .. },
            } = receive_server(&mut first)
            {
                assert_eq!(commands.len(), 1);
                saw_tick = true;
            }
        }
        assert!(saw_tick);
    }

    #[test]
    fn tcp_fault_injection_preserves_finalized_order_and_replica_checksums() {
        let config = CastleFightMatchConfig::development_subset(
            MapVersion::CASTLE_FIGHT_9_27,
            "r1",
            0x5eed_900d,
        )
        .unwrap();
        let authoritative = AuthoritativeMatch::new(
            config.clone(),
            1,
            ServerMatchOptions {
                checkpoint_interval_ticks: 1,
                ..ServerMatchOptions::default()
            },
        )
        .unwrap();
        let mut server = TcpAuthoritativeServer::bind("127.0.0.1:0", authoritative).unwrap();
        let mut first = ReplicaPeer::connect(&mut server, config.clone(), 11, 1, 1);
        let mut second = ReplicaPeer::connect(&mut server, config, 12, 2, 2);
        assert!(server.is_started());
        assert_eq!(first.player, PlayerId(0));
        assert_eq!(second.player, PlayerId(6));

        let first_builder = server
            .authoritative()
            .simulation()
            .builder_for_player(first.player)
            .unwrap()
            .id;
        let second_builder = server
            .authoritative()
            .simulation()
            .builder_for_player(second.player)
            .unwrap()
            .id;

        // Player 0 arrives during tick 0. Player 6 is deliberately delayed until tick 0 has
        // already been finalized, so it must not be retroactively inserted into that tick.
        first.send_command(
            0,
            WirePlayerCommand::StopBuilder {
                builder: first_builder.0,
            },
        );
        pump_until(&mut server, |view| view.pending_commands == 1);
        assert!(matches!(
            receive_server(&mut first.stream),
            ServerMessage::CommandAcknowledged {
                acknowledgement: CommandAcknowledgement::Scheduled {
                    client_sequence: 0,
                    tick: 0,
                    order: 0,
                    duplicate: false,
                }
            }
        ));

        server.finalize_next_tick().unwrap();
        let tick_zero_first = first.apply_finalized_tick();
        let tick_zero_second = second.apply_finalized_tick();
        assert_eq!(tick_zero_first, tick_zero_second);
        let CanonicalStreamRecord::Tick(tick_zero) = &tick_zero_first else {
            panic!("expected finalized tick 0")
        };
        assert_eq!(tick_zero.tick, 0);
        assert_eq!(tick_zero.commands.len(), 1);
        assert_eq!(tick_zero.commands[0].player, first.player);
        assert_eq!(
            first.simulation.checksum(),
            server.authoritative().simulation().checksum()
        );
        assert_eq!(
            second.simulation.checksum(),
            server.authoritative().simulation().checksum()
        );
        pump_network(&mut server);

        // Retry the already-finalized command. It must acknowledge the original schedule and must
        // not reopen tick 0 or enter the current pending set.
        first.send_command(
            0,
            WirePlayerCommand::StopBuilder {
                builder: first_builder.0,
            },
        );
        pump_network(&mut server);
        assert!(matches!(
            receive_server(&mut first.stream),
            ServerMessage::CommandAcknowledged {
                acknowledgement: CommandAcknowledgement::Scheduled {
                    client_sequence: 0,
                    tick: 0,
                    order: 0,
                    duplicate: true,
                }
            }
        ));
        assert!(
            server
                .authoritative()
                .driver()
                .pending_commands()
                .is_empty()
        );

        // The delayed player-6 command now arrives in the only open tick, tick 1.
        second.send_command(
            0,
            WirePlayerCommand::StopBuilder {
                builder: second_builder.0,
            },
        );
        pump_until(&mut server, |view| view.pending_commands == 1);
        assert!(matches!(
            receive_server(&mut second.stream),
            ServerMessage::CommandAcknowledged {
                acknowledgement: CommandAcknowledgement::Scheduled {
                    client_sequence: 0,
                    tick: 1,
                    order: 0,
                    duplicate: false,
                }
            }
        ));

        // Player 0 cannot use its next sequence to control player 6's builder. The rejected
        // request consumes the sequence but does not enter the canonical pending command set.
        first.send_command(
            1,
            WirePlayerCommand::StopBuilder {
                builder: second_builder.0,
            },
        );
        pump_network(&mut server);
        assert!(matches!(
            receive_server(&mut first.stream),
            ServerMessage::CommandAcknowledged {
                acknowledgement: CommandAcknowledgement::Rejected {
                    client_sequence: 1,
                    reason: WireAdmissionError::BuilderNotControllable { builder },
                    duplicate: false,
                }
            } if builder == second_builder.0
        ));
        assert_eq!(
            server
                .authoritative()
                .driver()
                .next_client_sequence(first.player),
            Some(ClientCommandSequence(2))
        );
        assert_eq!(server.authoritative().driver().pending_commands().len(), 1);

        server.finalize_next_tick().unwrap();
        let tick_one_first = first.apply_finalized_tick();
        let tick_one_second = second.apply_finalized_tick();
        assert_eq!(tick_one_first, tick_one_second);
        let CanonicalStreamRecord::Tick(tick_one) = &tick_one_first else {
            panic!("expected finalized tick 1")
        };
        assert_eq!(tick_one.tick, 1);
        assert_eq!(tick_one.commands.len(), 1);
        assert_eq!(tick_one.commands[0].player, second.player);
        assert_eq!(
            first.simulation.checksum(),
            server.authoritative().simulation().checksum()
        );
        assert_eq!(
            second.simulation.checksum(),
            server.authoritative().simulation().checksum()
        );
        pump_network(&mut server);

        // Dropping player 6 cannot mutate either finalized record. Because this is a 1v1, the
        // server first finalizes the currently open tick 2, then emits the canonical disconnect at
        // that completed boundary. The surviving peer applies both records and enters the same
        // team-disconnect pause without ever inferring an empty tick from transport silence.
        second.stream.shutdown(Shutdown::Both).unwrap();
        drop(second);
        pump_until(&mut server, |view| view.authenticated == 1);

        let tick_two_record = first.apply_finalized_tick();
        let CanonicalStreamRecord::Tick(tick_two) = &tick_two_record else {
            panic!("expected finalized tick 2")
        };
        assert_eq!(tick_two.tick, 2);
        assert!(tick_two.commands.is_empty());
        let disconnect_record = first.apply_control();
        assert!(matches!(
            disconnect_record,
            CanonicalStreamRecord::Control(control)
                if matches!(
                    control.event,
                    castle_fight_sim::MatchControlEvent::SetPlayerConnection {
                        player: PlayerId(6),
                        connection: castle_fight_sim::PlayerConnectionStatus::Disconnected,
                    }
                )
        ));
        assert_eq!(
            first.simulation.lifecycle(),
            MatchLifecycle::PausedForDisconnect {
                disconnected_teams_mask: 2,
            }
        );
        assert_eq!(
            first.simulation.checksum(),
            server.authoritative().simulation().checksum()
        );

        let mut hijack = connect(&server);
        server.poll_network().unwrap();
        send_client(&mut hijack, hello(&server, 99));
        pump_network(&mut server);
        assert!(matches!(
            receive_server(&mut hijack),
            ServerMessage::HelloRejected {
                reason: HandshakeRejectReason::MatchFull
            }
        ));

        let history = server.authoritative().driver().history();
        assert_eq!(history.first(), Some(&tick_zero_first));
        assert_eq!(history.get(1), Some(&tick_one_first));
        assert_eq!(history.get(2), Some(&tick_two_record));
        assert_eq!(history.get(3), Some(&disconnect_record));
    }

    #[test]
    fn tcp_reconnect_requires_bearer_token_and_binds_after_canonical_connect() {
        let mut server = server();
        let mut first = connect(&server);
        let mut second = connect(&server);
        server.poll_network().unwrap();
        send_client(&mut first, hello(&server, 1));
        send_client(&mut second, hello(&server, 2));
        pump_until(&mut server, |view| view.started);
        let first_assignment = match receive_server(&mut first) {
            ServerMessage::HelloAccepted { assignment } => assignment,
            message => panic!("expected first handshake, got {message:?}"),
        };
        let second_assignment = match receive_server(&mut second) {
            ServerMessage::HelloAccepted { assignment } => assignment,
            message => panic!("expected second handshake, got {message:?}"),
        };
        assert_ne!(
            first_assignment.reconnect_token, second_assignment.reconnect_token,
            "independent sessions must not share reconnect credentials"
        );

        second.shutdown(Shutdown::Both).unwrap();
        drop(second);
        pump_until(&mut server, |view| view.authenticated == 1);

        let mut disconnect_record = None;
        for _ in 0..3 {
            if let ServerMessage::StreamRecord { record } = receive_server(&mut first) {
                let record = CanonicalStreamRecord::from(record);
                if matches!(record, CanonicalStreamRecord::Control(_)) {
                    disconnect_record = Some(record);
                }
            }
        }
        assert!(matches!(
            disconnect_record,
            Some(CanonicalStreamRecord::Control(ref control))
                if matches!(
                    control.event,
                    castle_fight_sim::MatchControlEvent::SetPlayerConnection {
                        player: PlayerId(6),
                        connection: castle_fight_sim::PlayerConnectionStatus::Disconnected,
                    }
                )
        ));

        let reconnect = ReconnectHello {
            compatibility: server.authoritative().compatibility().clone(),
            session_id: second_assignment.session_id,
            reconnect_token: second_assignment.reconnect_token,
        };
        let mut invalid = reconnect.clone();
        invalid.reconnect_token.bytes[0] ^= 1;
        let mut hijack = connect(&server);
        server.poll_network().unwrap();
        send_client(&mut hijack, ClientMessage::Reconnect { reconnect: invalid });
        pump_network(&mut server);
        assert!(matches!(
            receive_server(&mut hijack),
            ServerMessage::HelloRejected {
                reason: HandshakeRejectReason::InvalidReconnect
            }
        ));
        assert_eq!(server.authenticated_connection_count(), 1);
        assert!(
            !server
                .authoritative()
                .session_is_connected(SessionId(second_assignment.session_id))
        );
        hijack.shutdown(Shutdown::Both).unwrap();
        drop(hijack);
        pump_network(&mut server);

        let mut replacement = connect(&server);
        server.poll_network().unwrap();
        send_client(
            &mut replacement,
            ClientMessage::Reconnect {
                reconnect: reconnect.clone(),
            },
        );
        pump_until(&mut server, |view| view.authenticated == 2);

        let reconnect_record = match receive_server(&mut first) {
            ServerMessage::StreamRecord { record } => CanonicalStreamRecord::from(record),
            message => panic!("expected canonical reconnect record, got {message:?}"),
        };
        assert!(matches!(
            reconnect_record,
            CanonicalStreamRecord::Control(ref control)
                if matches!(
                    control.event,
                    castle_fight_sim::MatchControlEvent::SetPlayerConnection {
                        player: PlayerId(6),
                        connection: castle_fight_sim::PlayerConnectionStatus::Connected,
                    }
                )
        ));
        assert_eq!(
            server.authoritative().driver().history().last(),
            Some(&reconnect_record),
            "the canonical reconnect must already be retained before the replacement joins live broadcasts"
        );

        let reassignment = match receive_server(&mut replacement) {
            ServerMessage::HelloAccepted { assignment } => assignment,
            message => {
                panic!("replacement must receive its handshake before live records: {message:?}")
            }
        };
        assert_eq!(reassignment.session_id, second_assignment.session_id);
        assert_eq!(
            reassignment.reconnect_token,
            second_assignment.reconnect_token
        );
        assert_eq!(reassignment.player_id, second_assignment.player_id);
        assert_eq!(
            reassignment.next_stream_position,
            server.authoritative().driver().next_stream_position().0
        );

        let begin = match receive_server(&mut replacement) {
            ServerMessage::SnapshotBegin { begin } => begin,
            message => panic!("expected reconnect snapshot header, got {message:?}"),
        };
        let reconnect_position = match &reconnect_record {
            CanonicalStreamRecord::Control(control) => control.stream_position,
            CanonicalStreamRecord::Tick(_) => unreachable!(),
        };
        assert_eq!(begin.snapshot_stream_position, reconnect_position.0);
        assert_eq!(begin.handoff_stream_position, reconnect_position.0 + 1);
        assert!(begin.snapshot_bytes as usize <= MAX_SNAPSHOT_BYTES);
        assert!(begin.chunk_count > 0);

        let mut snapshot_bytes = Vec::with_capacity(begin.snapshot_bytes as usize);
        for expected_index in 0..begin.chunk_count {
            let chunk = match receive_server(&mut replacement) {
                ServerMessage::SnapshotChunk { chunk } => chunk,
                message => panic!("expected reconnect snapshot chunk, got {message:?}"),
            };
            assert_eq!(chunk.transfer_id, begin.transfer_id);
            assert_eq!(chunk.chunk_index, expected_index);
            assert!(chunk.bytes.len() <= SNAPSHOT_CHUNK_BYTES);
            snapshot_bytes.extend_from_slice(&chunk.bytes);
        }
        assert_eq!(snapshot_bytes.len(), begin.snapshot_bytes as usize);
        let snapshot = castle_fight_sim::SimulationSnapshot::decode_wire(
            &snapshot_bytes,
            server.authoritative().driver().content(),
        )
        .unwrap();
        assert_eq!(snapshot.completed_tick(), begin.snapshot_completed_tick);
        assert_eq!(snapshot.checksum(), begin.snapshot_checksum);

        let replica_config =
            CastleFightMatchConfig::development_subset(MapVersion::CASTLE_FIGHT_9_27, "r1", 0x1234)
                .unwrap();
        let mut replica = create_castle_fight_match(replica_config, 2).unwrap();
        replica.simulation.restore_snapshot(&snapshot).unwrap();
        assert!(matches!(
            replica.simulation.lifecycle(),
            MatchLifecycle::PausedForDisconnect { .. }
        ));
        let mut replica_driver = MatchDriver::new_replica_from_snapshot(
            &replica.simulation,
            replica.content,
            InputStreamPosition(begin.snapshot_stream_position),
        );

        let handed_off_reconnect = match receive_server(&mut replacement) {
            ServerMessage::StreamRecord { record } => CanonicalStreamRecord::from(record),
            message => panic!("expected reconnect history record, got {message:?}"),
        };
        assert_eq!(handed_off_reconnect, reconnect_record);
        replica_driver
            .apply_stream_record(&mut replica.simulation, handed_off_reconnect)
            .unwrap();

        let complete = match receive_server(&mut replacement) {
            ServerMessage::CatchUpComplete { complete } => complete,
            message => panic!("expected reconnect completion marker, got {message:?}"),
        };
        assert_eq!(complete.transfer_id, begin.transfer_id);
        assert_eq!(
            complete.handoff_stream_position,
            begin.handoff_stream_position
        );
        assert_eq!(
            replica_driver.next_stream_position().0,
            complete.handoff_stream_position
        );
        assert_eq!(
            replica.simulation.tick().checked_sub(1),
            complete.completed_tick
        );
        assert_eq!(replica.simulation.checksum(), complete.checksum);
        assert_eq!(
            replica.simulation.checksum(),
            server.authoritative().simulation().checksum()
        );
        assert_eq!(replica.simulation.lifecycle(), MatchLifecycle::Running);
        assert_eq!(
            server.authoritative().simulation().lifecycle(),
            MatchLifecycle::Running
        );
    }
}
