use std::{
    collections::BTreeMap,
    fmt, io,
    net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    thread,
    time::{Duration, Instant},
};

use castle_fight_protocol::{
    ClientMessage, FrameError, HandshakeRejectReason, ProtocolEnvelope, ProtocolErrorCode,
    ProtocolSchemaError, ServerMessage, read_frame, write_frame,
};
use castle_fight_sim::{CASTLE_FIGHT_SIMULATION_HZ, MatchLifecycle};

use crate::{
    AuthoritativeMatch, HandshakeResult, OutboundMessage, OutboundTarget, ServerMatchError,
    SessionId,
};

const OUTBOUND_QUEUE_CAPACITY: usize = 256;
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(1);

#[derive(Debug)]
pub enum TcpServerError {
    Io(io::Error),
    Match(ServerMatchError),
    ConnectionIdExhausted,
}

impl fmt::Display for TcpServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "TCP server I/O error: {error}"),
            Self::Match(error) => error.fmt(formatter),
            Self::ConnectionIdExhausted => formatter.write_str("TCP connection ID space exhausted"),
        }
    }
}

impl std::error::Error for TcpServerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Match(error) => Some(error),
            Self::ConnectionIdExhausted => None,
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
        self.accept_pending_connections()?;
        loop {
            match self.inbound_rx.try_recv() {
                Ok(event) => self.process_inbound(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        if !self.started && self.authoritative.all_players_connected() {
            self.started = true;
        }
        Ok(())
    }

    pub fn finalize_next_tick(&mut self) -> Result<(), TcpServerError> {
        let outbound = self.authoritative.finalize_next_tick()?;
        self.dispatch_all(outbound);
        Ok(())
    }

    pub fn run_until_match_end(mut self) -> Result<(), TcpServerError> {
        let tick_hz = f64::from(CASTLE_FIGHT_SIMULATION_HZ);
        let tick_period = Duration::from_secs_f64(1.0 / tick_hz);
        let mut next_tick_deadline = Instant::now() + tick_period;

        loop {
            self.poll_network()?;
            if self.authoritative.simulation().lifecycle() != MatchLifecycle::Running {
                return Ok(());
            }
            if !self.started {
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

    fn process_inbound(&mut self, event: InboundEvent) {
        match event {
            InboundEvent::Frame(connection_id, envelope) => {
                self.process_frame(connection_id, envelope);
            }
            InboundEvent::Malformed(connection_id) | InboundEvent::Disconnected(connection_id) => {
                self.disconnect_connection(connection_id);
            }
        }
    }

    fn process_frame(
        &mut self,
        connection_id: ConnectionId,
        envelope: ProtocolEnvelope<ClientMessage>,
    ) {
        let Some(session_id) = self
            .connections
            .get(&connection_id)
            .and_then(|connection| connection.session_id)
        else {
            self.process_unbound_frame(connection_id, envelope);
            return;
        };

        let message = match envelope.into_current() {
            Ok(message) => message,
            Err(_) => {
                self.send_to_connection(
                    connection_id,
                    ServerMessage::ProtocolError {
                        code: ProtocolErrorCode::MalformedMessage,
                    },
                );
                return;
            }
        };
        let outbound = self
            .authoritative
            .handle_client_message(session_id, message);
        self.dispatch_all(outbound);
    }

    fn process_unbound_frame(
        &mut self,
        connection_id: ConnectionId,
        envelope: ProtocolEnvelope<ClientMessage>,
    ) {
        let message = match envelope.into_current() {
            Ok(message) => message,
            Err(ProtocolSchemaError { expected, actual }) => {
                self.send_to_connection(
                    connection_id,
                    ServerMessage::HelloRejected {
                        reason: HandshakeRejectReason::ProtocolSchema { expected, actual },
                    },
                );
                return;
            }
        };
        let ClientMessage::Hello { hello } = message else {
            self.send_to_connection(
                connection_id,
                ServerMessage::ProtocolError {
                    code: ProtocolErrorCode::ExpectedHello,
                },
            );
            return;
        };

        let result = self.authoritative.accept_hello(hello);
        if let HandshakeResult::Accepted { session_id, .. } = &result {
            if let Some(connection) = self.connections.get_mut(&connection_id) {
                connection.session_id = Some(*session_id);
            }
            self.session_connections.insert(*session_id, connection_id);
        }
        self.send_to_connection(connection_id, result.message());
    }

    fn dispatch_all(&mut self, outbound: Vec<OutboundMessage>) {
        for outbound in outbound {
            match outbound.target {
                OutboundTarget::Session(session_id) => {
                    if let Some(connection_id) = self.session_connections.get(&session_id).copied()
                    {
                        self.send_to_connection(connection_id, outbound.message);
                    }
                }
                OutboundTarget::Broadcast => {
                    let connection_ids = self
                        .session_connections
                        .values()
                        .copied()
                        .collect::<Vec<_>>();
                    for connection_id in connection_ids {
                        self.send_to_connection(connection_id, outbound.message.clone());
                    }
                }
            }
        }
    }

    fn send_to_connection(&mut self, connection_id: ConnectionId, message: ServerMessage) {
        let send_result = self
            .connections
            .get(&connection_id)
            .map(|connection| connection.outbound.try_send(message));
        if matches!(
            send_result,
            Some(Err(TrySendError::Full(_) | TrySendError::Disconnected(_)))
        ) {
            self.disconnect_connection(connection_id);
        }
    }

    fn disconnect_connection(&mut self, connection_id: ConnectionId) {
        let Some(connection) = self.connections.remove(&connection_id) else {
            return;
        };
        let _ = connection.shutdown.shutdown(Shutdown::Both);
        if let Some(session_id) = connection.session_id {
            self.session_connections.remove(&session_id);
            self.authoritative.disconnect_session(session_id);
        }
    }
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
        ClientHello, CommandAcknowledgement, CommandRequest, SessionAssignment,
        WireCanonicalStreamRecord, WirePlayerCommand,
    };
    use castle_fight_sim::{CastleFightMatchConfig, MapVersion, PlayerId};

    use crate::ServerMatchOptions;

    fn server() -> TcpAuthoritativeServer {
        let config =
            CastleFightMatchConfig::development_subset(MapVersion::CASTLE_FIGHT_9_27, "r1", 0x1234)
                .unwrap();
        let authoritative =
            AuthoritativeMatch::new(config, 1, ServerMatchOptions::default()).unwrap();
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
}
