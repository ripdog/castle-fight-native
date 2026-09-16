use std::{
    fmt, io,
    net::{SocketAddr, TcpStream},
    sync::{Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use castle_fight_protocol::{
    ClientHello, ClientMessage, CompatibilityIdentity, FrameError, HandshakeRejectReason,
    ProtocolEnvelope, ReconnectHello, ReconnectToken, ServerMessage, SessionAssignment, read_frame,
    write_frame,
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const RECONNECT_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const OUTBOUND_QUEUE_CAPACITY: usize = 256;

#[derive(Debug)]
pub enum NetworkClientError {
    Io(io::Error),
    Frame(FrameError),
    ProtocolSchema { expected: u32, actual: u32 },
    HandshakeRejected(HandshakeRejectReason),
    UnexpectedHandshakeMessage(ServerMessage),
    InvalidReconnectAssignment,
    OutboundQueueFull,
    Disconnected,
}

impl fmt::Display for NetworkClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "network I/O error: {error}"),
            Self::Frame(error) => error.fmt(formatter),
            Self::ProtocolSchema { expected, actual } => write!(
                formatter,
                "server protocol schema mismatch: expected {expected}, got {actual}"
            ),
            Self::HandshakeRejected(reason) => {
                write!(formatter, "server rejected join: {reason:?}")
            }
            Self::UnexpectedHandshakeMessage(message) => {
                write!(
                    formatter,
                    "unexpected server handshake message: {message:?}"
                )
            }
            Self::InvalidReconnectAssignment => formatter
                .write_str("server reconnect assignment changed authenticated session identity"),
            Self::OutboundQueueFull => formatter.write_str("network command queue is full"),
            Self::Disconnected => formatter.write_str("network connection is closed"),
        }
    }
}

impl std::error::Error for NetworkClientError {}

impl From<io::Error> for NetworkClientError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<FrameError> for NetworkClientError {
    fn from(error: FrameError) -> Self {
        Self::Frame(error)
    }
}

#[derive(Debug)]
pub enum NetworkEvent {
    Message(ServerMessage),
    Disconnected(String),
    Reconnected(SessionAssignment),
    ReconnectFailed(String),
}

enum InternalNetworkEvent {
    Message {
        generation: u64,
        message: ServerMessage,
    },
    Disconnected {
        generation: u64,
        reason: String,
    },
    Reconnected {
        generation: u64,
        assignment: SessionAssignment,
        outbound: mpsc::SyncSender<ClientMessage>,
    },
    ReconnectFailed {
        generation: u64,
        reason: String,
    },
}

pub struct NetworkClient {
    address: SocketAddr,
    compatibility: CompatibilityIdentity,
    session_id: u64,
    reconnect_token: ReconnectToken,
    outbound: Option<mpsc::SyncSender<ClientMessage>>,
    events_tx: mpsc::Sender<InternalNetworkEvent>,
    events_rx: Mutex<mpsc::Receiver<InternalNetworkEvent>>,
    active_generation: u64,
    reconnecting: bool,
    next_reconnect_attempt: Instant,
}

impl NetworkClient {
    pub fn connect(
        address: SocketAddr,
        compatibility: CompatibilityIdentity,
        client_nonce: u64,
    ) -> Result<(Self, SessionAssignment), NetworkClientError> {
        let (stream, assignment) = connect_initial(address, compatibility.clone(), client_nonce)?;
        let (events_tx, events_rx) = mpsc::channel();
        let outbound = spawn_transport(stream, 0, events_tx.clone())?;
        Ok((
            Self {
                address,
                compatibility,
                session_id: assignment.session_id,
                reconnect_token: assignment.reconnect_token,
                outbound: Some(outbound),
                events_tx,
                events_rx: Mutex::new(events_rx),
                active_generation: 0,
                reconnecting: false,
                next_reconnect_attempt: Instant::now(),
            },
            assignment,
        ))
    }

    pub fn send(&self, message: ClientMessage) -> Result<(), NetworkClientError> {
        let Some(outbound) = &self.outbound else {
            return Err(NetworkClientError::Disconnected);
        };
        match outbound.try_send(message) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => Err(NetworkClientError::OutboundQueueFull),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(NetworkClientError::Disconnected),
        }
    }

    pub fn poll_reconnect(&mut self) {
        if self.outbound.is_some()
            || self.reconnecting
            || Instant::now() < self.next_reconnect_attempt
        {
            return;
        }
        let Some(generation) = self.active_generation.checked_add(1) else {
            return;
        };
        self.reconnecting = true;
        let address = self.address;
        let compatibility = self.compatibility.clone();
        let session_id = self.session_id;
        let reconnect_token = self.reconnect_token;
        let events = self.events_tx.clone();
        thread::Builder::new()
            .name("cf-client-net-reconnect".to_owned())
            .spawn(move || {
                match connect_reconnect(address, compatibility, session_id, reconnect_token)
                    .and_then(|(stream, assignment)| {
                        let outbound = prepare_transport(stream, generation, events.clone())?;
                        events
                            .send(InternalNetworkEvent::Reconnected {
                                generation,
                                assignment,
                                outbound: outbound.sender.clone(),
                            })
                            .map_err(|_| NetworkClientError::Disconnected)?;
                        outbound.spawn();
                        Ok(())
                    }) {
                    Ok(()) => {}
                    Err(error) => {
                        let _ = events.send(InternalNetworkEvent::ReconnectFailed {
                            generation,
                            reason: error.to_string(),
                        });
                    }
                }
            })
            .expect("network reconnect thread creation must succeed");
    }

    pub fn drain_events(&mut self) -> Vec<NetworkEvent> {
        let receiver = self
            .events_rx
            .lock()
            .expect("network receiver mutex poisoned");
        let mut internal = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            internal.push(event);
        }
        drop(receiver);

        let mut events = Vec::new();
        for event in internal {
            match event {
                InternalNetworkEvent::Message {
                    generation,
                    message,
                } if generation == self.active_generation => {
                    events.push(NetworkEvent::Message(message));
                }
                InternalNetworkEvent::Disconnected { generation, reason }
                    if generation == self.active_generation =>
                {
                    self.outbound = None;
                    self.reconnecting = false;
                    self.next_reconnect_attempt = Instant::now();
                    events.push(NetworkEvent::Disconnected(reason));
                }
                InternalNetworkEvent::Reconnected {
                    generation,
                    assignment,
                    outbound,
                } if generation > self.active_generation => {
                    self.active_generation = generation;
                    self.session_id = assignment.session_id;
                    self.reconnect_token = assignment.reconnect_token;
                    self.outbound = Some(outbound);
                    self.reconnecting = false;
                    events.push(NetworkEvent::Reconnected(assignment));
                }
                InternalNetworkEvent::ReconnectFailed { generation, reason }
                    if generation > self.active_generation =>
                {
                    self.reconnecting = false;
                    self.next_reconnect_attempt = Instant::now() + RECONNECT_RETRY_INTERVAL;
                    events.push(NetworkEvent::ReconnectFailed(reason));
                }
                InternalNetworkEvent::Message { .. }
                | InternalNetworkEvent::Disconnected { .. }
                | InternalNetworkEvent::Reconnected { .. }
                | InternalNetworkEvent::ReconnectFailed { .. } => {}
            }
        }
        events
    }

    #[cfg(test)]
    pub(crate) fn connected_test_fixture(compatibility: CompatibilityIdentity) -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        let (outbound, _outbound_rx) = mpsc::sync_channel(OUTBOUND_QUEUE_CAPACITY);
        Self {
            address: "127.0.0.1:9".parse().unwrap(),
            compatibility,
            session_id: 1,
            reconnect_token: ReconnectToken {
                bytes: [0; castle_fight_protocol::RECONNECT_TOKEN_BYTES],
            },
            outbound: Some(outbound),
            events_tx,
            events_rx: Mutex::new(events_rx),
            active_generation: 0,
            reconnecting: false,
            next_reconnect_attempt: Instant::now(),
        }
    }

    #[cfg(test)]
    pub(crate) fn inject_server_message_for_test(&self, message: ServerMessage) {
        self.events_tx
            .send(InternalNetworkEvent::Message {
                generation: self.active_generation,
                message,
            })
            .unwrap();
    }
}

fn connect_initial(
    address: SocketAddr,
    compatibility: CompatibilityIdentity,
    client_nonce: u64,
) -> Result<(TcpStream, SessionAssignment), NetworkClientError> {
    connect_with_message(
        address,
        ClientMessage::Hello {
            hello: ClientHello {
                compatibility,
                client_nonce,
            },
        },
    )
}

fn connect_reconnect(
    address: SocketAddr,
    compatibility: CompatibilityIdentity,
    session_id: u64,
    reconnect_token: ReconnectToken,
) -> Result<(TcpStream, SessionAssignment), NetworkClientError> {
    let (stream, assignment) = connect_with_message(
        address,
        ClientMessage::Reconnect {
            reconnect: ReconnectHello {
                compatibility,
                session_id,
                reconnect_token,
            },
        },
    )?;
    if assignment.session_id != session_id || assignment.reconnect_token != reconnect_token {
        return Err(NetworkClientError::InvalidReconnectAssignment);
    }
    Ok((stream, assignment))
}

fn connect_with_message(
    address: SocketAddr,
    message: ClientMessage,
) -> Result<(TcpStream, SessionAssignment), NetworkClientError> {
    let mut stream = TcpStream::connect(address)?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    write_frame(&mut stream, &ProtocolEnvelope::new(message))?;
    let envelope: ProtocolEnvelope<ServerMessage> = read_frame(&mut stream)?;
    let message = envelope
        .into_current()
        .map_err(|error| NetworkClientError::ProtocolSchema {
            expected: error.expected,
            actual: error.actual,
        })?;
    let assignment = match message {
        ServerMessage::HelloAccepted { assignment } => assignment,
        ServerMessage::HelloRejected { reason } => {
            return Err(NetworkClientError::HandshakeRejected(reason));
        }
        unexpected => return Err(NetworkClientError::UnexpectedHandshakeMessage(unexpected)),
    };
    stream.set_read_timeout(None)?;
    Ok((stream, assignment))
}

struct PreparedTransport {
    sender: mpsc::SyncSender<ClientMessage>,
    reader: TcpStream,
    writer: TcpStream,
    outbound_rx: mpsc::Receiver<ClientMessage>,
    generation: u64,
    events: mpsc::Sender<InternalNetworkEvent>,
}

impl PreparedTransport {
    fn spawn(self) {
        spawn_reader(self.reader, self.generation, self.events.clone());
        spawn_writer(self.writer, self.outbound_rx, self.generation, self.events);
    }
}

fn prepare_transport(
    stream: TcpStream,
    generation: u64,
    events: mpsc::Sender<InternalNetworkEvent>,
) -> Result<PreparedTransport, NetworkClientError> {
    let reader = stream.try_clone()?;
    let writer = stream;
    let (sender, outbound_rx) = mpsc::sync_channel(OUTBOUND_QUEUE_CAPACITY);
    Ok(PreparedTransport {
        sender,
        reader,
        writer,
        outbound_rx,
        generation,
        events,
    })
}

fn spawn_transport(
    stream: TcpStream,
    generation: u64,
    events: mpsc::Sender<InternalNetworkEvent>,
) -> Result<mpsc::SyncSender<ClientMessage>, NetworkClientError> {
    let transport = prepare_transport(stream, generation, events)?;
    let sender = transport.sender.clone();
    transport.spawn();
    Ok(sender)
}

fn spawn_reader(
    mut stream: TcpStream,
    generation: u64,
    inbound: mpsc::Sender<InternalNetworkEvent>,
) {
    thread::Builder::new()
        .name("cf-client-net-read".to_owned())
        .spawn(move || {
            loop {
                match read_frame::<_, ProtocolEnvelope<ServerMessage>>(&mut stream) {
                    Ok(envelope) => match envelope.into_current() {
                        Ok(message) => {
                            if inbound
                                .send(InternalNetworkEvent::Message {
                                    generation,
                                    message,
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = inbound.send(InternalNetworkEvent::Disconnected {
                                generation,
                                reason: format!(
                                    "server protocol schema mismatch: expected {}, got {}",
                                    error.expected, error.actual
                                ),
                            });
                            break;
                        }
                    },
                    Err(error) => {
                        let _ = inbound.send(InternalNetworkEvent::Disconnected {
                            generation,
                            reason: error.to_string(),
                        });
                        break;
                    }
                }
            }
        })
        .expect("network reader thread creation must succeed");
}

fn spawn_writer(
    mut stream: TcpStream,
    outbound: mpsc::Receiver<ClientMessage>,
    generation: u64,
    inbound: mpsc::Sender<InternalNetworkEvent>,
) {
    thread::Builder::new()
        .name("cf-client-net-write".to_owned())
        .spawn(move || {
            while let Ok(message) = outbound.recv() {
                if let Err(error) = write_frame(&mut stream, &ProtocolEnvelope::new(message)) {
                    let _ = inbound.send(InternalNetworkEvent::Disconnected {
                        generation,
                        reason: error.to_string(),
                    });
                    break;
                }
            }
        })
        .expect("network writer thread creation must succeed");
}

#[cfg(test)]
mod tests {
    use super::*;
    use castle_fight_protocol::{RECONNECT_TOKEN_BYTES, SnapshotTransferBegin};
    use castle_fight_sim::MapVersion;
    use std::net::TcpListener;

    fn compatibility() -> CompatibilityIdentity {
        CompatibilityIdentity {
            snapshot_schema_version: 1,
            checksum_schema_version: 5,
            map_version: MapVersion::CASTLE_FIGHT_9_27,
            release_revision: "r1".to_owned(),
            content_schema_version: 1,
            content_gameplay_hash: 7,
            configuration_identity: 9,
        }
    }

    #[test]
    fn reconnect_reuses_credentials_and_precedes_snapshot_messages() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let token = ReconnectToken {
            bytes: [0x4a; RECONNECT_TOKEN_BYTES],
        };
        let initial_assignment = SessionAssignment {
            session_id: 17,
            reconnect_token: token,
            player_id: 6,
            team: 1,
            next_stream_position: 0,
            next_client_sequence: 0,
            completed_tick: None,
        };
        let reconnect_assignment = SessionAssignment {
            next_stream_position: 9,
            next_client_sequence: 4,
            completed_tick: Some(7),
            ..initial_assignment
        };
        let expected_compatibility = compatibility();
        let server = thread::spawn(move || {
            let (mut initial, _) = listener.accept().unwrap();
            let hello: ProtocolEnvelope<ClientMessage> = read_frame(&mut initial).unwrap();
            assert!(matches!(
                hello.into_current().unwrap(),
                ClientMessage::Hello { .. }
            ));
            write_frame(
                &mut initial,
                &ProtocolEnvelope::new(ServerMessage::HelloAccepted {
                    assignment: initial_assignment,
                }),
            )
            .unwrap();
            drop(initial);

            let (mut replacement, _) = listener.accept().unwrap();
            let reconnect: ProtocolEnvelope<ClientMessage> = read_frame(&mut replacement).unwrap();
            let ClientMessage::Reconnect { reconnect } = reconnect.into_current().unwrap() else {
                panic!("replacement connection must authenticate with reconnect credentials")
            };
            assert_eq!(reconnect.compatibility, expected_compatibility);
            assert_eq!(reconnect.session_id, initial_assignment.session_id);
            assert_eq!(reconnect.reconnect_token, token);
            write_frame(
                &mut replacement,
                &ProtocolEnvelope::new(ServerMessage::HelloAccepted {
                    assignment: reconnect_assignment,
                }),
            )
            .unwrap();
            write_frame(
                &mut replacement,
                &ProtocolEnvelope::new(ServerMessage::SnapshotBegin {
                    begin: SnapshotTransferBegin {
                        transfer_id: 3,
                        snapshot_stream_position: 8,
                        snapshot_completed_tick: Some(7),
                        snapshot_checksum: 11,
                        snapshot_bytes: 1,
                        chunk_count: 1,
                        handoff_stream_position: 9,
                    },
                }),
            )
            .unwrap();
        });

        let (mut client, assignment) =
            NetworkClient::connect(address, compatibility(), 123).unwrap();
        assert_eq!(assignment, initial_assignment);

        let mut saw_disconnect = false;
        for _ in 0..100 {
            if client
                .drain_events()
                .into_iter()
                .any(|event| matches!(event, NetworkEvent::Disconnected(_)))
            {
                saw_disconnect = true;
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        assert!(saw_disconnect);
        client.poll_reconnect();

        let mut received = Vec::new();
        for _ in 0..100 {
            received.extend(client.drain_events());
            if received.len() >= 2 {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        assert!(matches!(
            received.first(),
            Some(NetworkEvent::Reconnected(assignment)) if *assignment == reconnect_assignment
        ));
        assert!(matches!(
            received.get(1),
            Some(NetworkEvent::Message(ServerMessage::SnapshotBegin { .. }))
        ));
        server.join().unwrap();
    }
}
