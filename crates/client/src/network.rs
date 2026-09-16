use std::{
    fmt, io,
    net::{SocketAddr, TcpStream},
    sync::{Mutex, mpsc},
    thread,
    time::Duration,
};

use castle_fight_protocol::{
    ClientHello, ClientMessage, CompatibilityIdentity, FrameError, HandshakeRejectReason,
    ProtocolEnvelope, ServerMessage, SessionAssignment, read_frame, write_frame,
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const OUTBOUND_QUEUE_CAPACITY: usize = 256;

#[derive(Debug)]
pub enum NetworkClientError {
    Io(io::Error),
    Frame(FrameError),
    ProtocolSchema { expected: u32, actual: u32 },
    HandshakeRejected(HandshakeRejectReason),
    UnexpectedHandshakeMessage(ServerMessage),
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
}

pub struct NetworkClient {
    outbound: mpsc::SyncSender<ClientMessage>,
    inbound: Mutex<mpsc::Receiver<NetworkEvent>>,
}

impl NetworkClient {
    pub fn connect(
        address: SocketAddr,
        compatibility: CompatibilityIdentity,
        client_nonce: u64,
    ) -> Result<(Self, SessionAssignment), NetworkClientError> {
        let mut stream = TcpStream::connect(address)?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
        write_frame(
            &mut stream,
            &ProtocolEnvelope::new(ClientMessage::Hello {
                hello: ClientHello {
                    compatibility,
                    client_nonce,
                },
            }),
        )?;
        let envelope: ProtocolEnvelope<ServerMessage> = read_frame(&mut stream)?;
        let message =
            envelope
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

        let reader = stream.try_clone()?;
        let writer = stream;
        let (outbound, outbound_rx) = mpsc::sync_channel(OUTBOUND_QUEUE_CAPACITY);
        let (inbound_tx, inbound_rx) = mpsc::channel();
        spawn_reader(reader, inbound_tx.clone());
        spawn_writer(writer, outbound_rx, inbound_tx);
        Ok((
            Self {
                outbound,
                inbound: Mutex::new(inbound_rx),
            },
            assignment,
        ))
    }

    pub fn send(&self, message: ClientMessage) -> Result<(), NetworkClientError> {
        match self.outbound.try_send(message) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => Err(NetworkClientError::OutboundQueueFull),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(NetworkClientError::Disconnected),
        }
    }

    pub fn drain_events(&self) -> Vec<NetworkEvent> {
        let receiver = self
            .inbound
            .lock()
            .expect("network receiver mutex poisoned");
        let mut events = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            events.push(event);
        }
        events
    }
}

fn spawn_reader(mut stream: TcpStream, inbound: mpsc::Sender<NetworkEvent>) {
    thread::Builder::new()
        .name("cf-client-net-read".to_owned())
        .spawn(move || {
            loop {
                match read_frame::<_, ProtocolEnvelope<ServerMessage>>(&mut stream) {
                    Ok(envelope) => match envelope.into_current() {
                        Ok(message) => {
                            if inbound.send(NetworkEvent::Message(message)).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = inbound.send(NetworkEvent::Disconnected(format!(
                                "server protocol schema mismatch: expected {}, got {}",
                                error.expected, error.actual
                            )));
                            break;
                        }
                    },
                    Err(error) => {
                        let _ = inbound.send(NetworkEvent::Disconnected(error.to_string()));
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
    inbound: mpsc::Sender<NetworkEvent>,
) {
    thread::Builder::new()
        .name("cf-client-net-write".to_owned())
        .spawn(move || {
            while let Ok(message) = outbound.recv() {
                if let Err(error) = write_frame(&mut stream, &ProtocolEnvelope::new(message)) {
                    let _ = inbound.send(NetworkEvent::Disconnected(error.to_string()));
                    break;
                }
            }
        })
        .expect("network writer thread creation must succeed");
}
