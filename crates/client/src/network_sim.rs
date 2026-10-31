//! Networked play: splits the processing of server events between the main thread and the
//! simulation thread without changing what is applied or in which order.
//!
//! Server events arrive as an ordered stream. `ingest_events` classifies each one:
//!
//! * Events that apply canonical records to the simulation or verify it against the server
//!   (finalized stream records, execution batches, checkpoints) become `SimItem`s.
//! * Events that touch neither the simulation nor the driver (acknowledgements, lobby
//!   rejections, chat, kicks, reconnect retries, protocol errors, lobby status with an unchanged
//!   roster) become `MainItem`s that
//!   are applied on the main thread, in stream order, when the results are applied.
//! * Everything that replaces or inspects simulation-wide state (disconnects, reconnects,
//!   snapshot transfer, roster changes, slot assignment) is a barrier. The items queued before it are first
//!   applied synchronously, then the barrier is handled with the simulation present.
//!
//! The queue left after ingest is executed by [`execute_items`], either on the simulation thread
//! (`sim_thread`) or inline, and yields [`NetOutput`]s in item order. `apply_outputs` performs the
//! main-thread follow-ups (presentation timeline, command feedback, performance counters,
//! checkpoint reports, status text) in that same order. Because both paths run the same
//! functions in the same sequence, the result is identical to applying the events one by one.

use castle_fight_protocol::{
    Checkpoint, CheckpointReport, ClientMessage, CommandAcknowledgement, LobbyRaceRejectReason,
    LobbySlotRejectReason, LobbyStartRejectReason, LobbyStatus, LobbyTeamSizeRejectReason,
    ProtocolErrorCode, ServerMessage, WireCommandExecution, WireExecutionBatch,
};
use castle_fight_sim::{
    CanonicalStreamRecord, DriverTickResult, MatchDriver, MatchLifecycle, PlayerId, Simulation,
    Team,
};

use crate::{
    AuthoritativeSimulation, AuthorityMode, SelectedMatch, append_snapshot_chunk,
    apply_network_lobby_roster, begin_snapshot_catch_up,
    bridge::{ObserverVision, PresentationSamples, PresentationSnapshot},
    chat::IncomingChat,
    finish_snapshot_catch_up,
    network::{NetworkClientError, NetworkEvent},
    performance_ui::PerformanceCounters,
    plan_network_lobby_roster,
};

/// One queued unit of network work, in server order.
pub(crate) enum NetItem {
    Sim(SimItem),
    Main(MainItem),
}

/// Work that needs the simulation and driver.
pub(crate) enum SimItem {
    Record(CanonicalStreamRecord),
    ExecutionBatch(WireExecutionBatch),
    Checkpoint(Checkpoint),
}

/// Work that only needs main-thread state.
pub(crate) enum MainItem {
    LobbyStatusUnchanged(LobbyStatus),
    LobbyMatchRulesRejected,
    LobbyRaceRejected(LobbyRaceRejectReason),
    LobbyStartRejected(LobbyStartRejectReason),
    LobbyTeamSizeRejected(LobbyTeamSizeRejectReason),
    LobbySlotRejected(LobbySlotRejectReason),
    Kicked,
    Chat(IncomingChat),
    Acknowledged(CommandAcknowledgement),
    ProtocolError(ProtocolErrorCode),
    ReconnectFailed(String),
}

/// How stream records are applied while the items run. Constant within one queue because every
/// transition of the snapshot transfer is a barrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CatchUpMode {
    snapshot_loaded: bool,
    handoff_stream_position: u64,
}

impl CatchUpMode {
    pub(crate) fn of(authoritative: &AuthoritativeSimulation) -> Option<Self> {
        authoritative.catch_up.as_ref().map(|catch_up| Self {
            snapshot_loaded: catch_up.snapshot_loaded,
            handoff_stream_position: catch_up.begin.handoff_stream_position,
        })
    }
}

/// Result of one checkpoint check, reported to the server on the main thread.
pub(crate) struct CheckpointOutcome {
    completed_tick: u64,
    local_checksum: u64,
    server_checksum: u64,
}

pub(crate) enum NetOutput {
    /// A record applied while replaying history after a snapshot (no presentation).
    CatchUpRecord(Option<Box<DriverTickResult>>),
    Tick {
        result: Box<DriverTickResult>,
        snapshot: Box<PresentationSnapshot>,
    },
    Boundary {
        snapshot: Box<PresentationSnapshot>,
    },
    Checkpoint(CheckpointOutcome),
    Main(MainItem),
}

/// Outputs produced before the first failure, plus that failure.
pub(crate) struct NetRun {
    pub(crate) outputs: Vec<NetOutput>,
    pub(crate) error: Option<String>,
}

impl NetRun {
    /// Whether any of the queued items needs the simulation.
    pub(crate) fn needs_simulation(items: &[NetItem]) -> bool {
        items.iter().any(|item| matches!(item, NetItem::Sim(_)))
    }
}

/// Applies the queued items to the simulation and driver in order. Stops at the first failure,
/// exactly where sequential processing would have returned its error.
pub(crate) fn execute_items(
    simulation: &mut Simulation,
    driver: &mut MatchDriver,
    expected_execution_batch: &mut Option<(u64, Vec<WireCommandExecution>)>,
    catch_up: Option<CatchUpMode>,
    observer: Option<ObserverVision>,
    items: Vec<NetItem>,
) -> NetRun {
    let mut outputs = Vec::with_capacity(items.len());
    let mut run = |item: NetItem| -> Result<(), String> {
        match item {
            NetItem::Main(item) => outputs.push(NetOutput::Main(item)),
            NetItem::Sim(SimItem::Record(record)) => {
                let (output, late_error) = match catch_up {
                    Some(mode) => apply_catch_up_record(simulation, driver, mode, record)?,
                    None => (
                        apply_live_record(
                            simulation,
                            driver,
                            expected_execution_batch,
                            observer,
                            record,
                        )?,
                        None,
                    ),
                };
                outputs.push(output);
                if let Some(error) = late_error {
                    return Err(error);
                }
            }
            NetItem::Sim(SimItem::ExecutionBatch(batch)) => {
                let Some((expected_tick, expected_executions)) = expected_execution_batch.take()
                else {
                    return Err(format!(
                        "received execution batch for tick {} before its finalized input record",
                        batch.tick
                    ));
                };
                if expected_tick != batch.tick || expected_executions != batch.executions {
                    return Err(format!(
                        "server execution batch disagrees with deterministic local execution for tick {}",
                        batch.tick
                    ));
                }
            }
            NetItem::Sim(SimItem::Checkpoint(checkpoint)) => {
                let completed_tick = simulation.tick().checked_sub(1);
                if completed_tick != Some(checkpoint.completed_tick) {
                    return Err(format!(
                        "checkpoint for tick {} arrived while local completed tick is {completed_tick:?}",
                        checkpoint.completed_tick
                    ));
                }
                outputs.push(NetOutput::Checkpoint(CheckpointOutcome {
                    completed_tick: checkpoint.completed_tick,
                    local_checksum: simulation.checksum(),
                    server_checksum: checkpoint.checksum,
                }));
            }
        }
        Ok(())
    };
    let mut error = None;
    for item in items {
        if let Err(failure) = run(item) {
            error = Some(failure);
            break;
        }
    }
    NetRun { outputs, error }
}

fn apply_catch_up_record(
    simulation: &mut Simulation,
    driver: &mut MatchDriver,
    mode: CatchUpMode,
    record: CanonicalStreamRecord,
) -> Result<(NetOutput, Option<String>), String> {
    if !mode.snapshot_loaded {
        return Err("received reconnect history before snapshot completion".to_owned());
    }
    if driver.next_stream_position().0 >= mode.handoff_stream_position {
        return Err("received reconnect history beyond pinned handoff boundary".to_owned());
    }
    let applied = driver
        .apply_stream_record(simulation, record)
        .map_err(|error| format!("canonical catch-up stream error: {error:?}"))?;
    simulation.clear_presentation_events();
    // Sequential processing recorded the performance sample before this check, so the output
    // is kept and the failure is reported after it.
    let crossed = (driver.next_stream_position().0 > mode.handoff_stream_position)
        .then(|| "reconnect history crossed pinned handoff boundary".to_owned());
    Ok((NetOutput::CatchUpRecord(applied.map(Box::new)), crossed))
}

fn apply_live_record(
    simulation: &mut Simulation,
    driver: &mut MatchDriver,
    expected_execution_batch: &mut Option<(u64, Vec<WireCommandExecution>)>,
    observer: Option<ObserverVision>,
    record: CanonicalStreamRecord,
) -> Result<NetOutput, String> {
    let applied = driver
        .apply_stream_record(simulation, record)
        .map_err(|error| format!("canonical stream error: {error:?}"))?;
    let mut snapshot = PresentationSnapshot::capture(simulation);
    if let Some(observer) = observer {
        snapshot.restrict_to_observer(observer);
    }
    Ok(match applied {
        Some(result) => {
            let expected = result
                .executions
                .iter()
                .copied()
                .map(WireCommandExecution::from)
                .collect();
            *expected_execution_batch = Some((result.finalized.tick, expected));
            NetOutput::Tick {
                result: Box::new(result),
                snapshot: Box::new(snapshot),
            }
        }
        None => NetOutput::Boundary {
            snapshot: Box::new(snapshot),
        },
    })
}

/// Applies the main-thread consequences of the outputs in order. Returns the first failure.
pub(crate) fn apply_outputs(
    authoritative: &mut AuthoritativeSimulation,
    presentation: &mut PresentationSamples,
    mut performance: Option<&mut PerformanceCounters>,
    outputs: Vec<NetOutput>,
) -> Result<(), String> {
    for output in outputs {
        match output {
            NetOutput::CatchUpRecord(applied) => {
                if let (Some(result), Some(performance)) = (applied, performance.as_deref_mut()) {
                    record_tick(performance, &result);
                }
            }
            NetOutput::Tick { result, snapshot } => {
                if let Some(performance) = performance.as_deref_mut() {
                    record_tick(performance, &result);
                }
                authoritative
                    .pending_feedback
                    .extend(result.executions.iter().copied());
                for execution in &result.executions {
                    authoritative.pending_build_commands.resolve(
                        execution.scheduled.player,
                        execution.scheduled.client_sequence.0,
                    );
                }
                presentation.enqueue_network_tick(*snapshot);
            }
            NetOutput::Boundary { snapshot } => presentation.enqueue_network_boundary(*snapshot),
            NetOutput::Checkpoint(outcome) => report_checkpoint(authoritative, outcome)?,
            NetOutput::Main(item) => apply_main_item(authoritative, item)?,
        }
    }
    Ok(())
}

fn record_tick(performance: &mut PerformanceCounters, result: &DriverTickResult) {
    performance.record_sim_tick(
        result.tick_result.completed_tick,
        result.tick_result.timings,
        result.tick_result.collision_fallback_searches,
        result.tick_result.collision_fallback_candidate_checks,
        result.tick_result.collision_fallback_max_ring,
    );
}

fn report_checkpoint(
    authoritative: &mut AuthoritativeSimulation,
    outcome: CheckpointOutcome,
) -> Result<(), String> {
    let report = ClientMessage::CheckpointReport {
        report: CheckpointReport {
            completed_tick: outcome.completed_tick,
            checksum: outcome.local_checksum,
        },
    };
    let send_result = match &authoritative.authority {
        AuthorityMode::Network { client, .. } => client.send(report),
        AuthorityMode::Local => unreachable!("network event in local mode"),
    };
    match send_result {
        Ok(()) => {}
        // A checkpoint can be queued ahead of the disconnect that closed the transport in the
        // same event batch. The disconnect event itself resets the session and reconnect
        // resynchronizes, so this is not an invariant failure.
        Err(NetworkClientError::Disconnected) => {
            authoritative.pending_status.push(format!(
                "Checkpoint for tick {} not reported: network connection is closed.",
                outcome.completed_tick
            ));
            return Ok(());
        }
        Err(error) => return Err(format!("could not report checkpoint: {error}")),
    }
    if outcome.local_checksum != outcome.server_checksum {
        if let AuthorityMode::Network { connected, .. } = &mut authoritative.authority {
            *connected = false;
        }
        authoritative.pending_status.push(format!(
            "Checksum mismatch at tick {}; requesting authoritative state replacement.",
            outcome.completed_tick
        ));
    }
    Ok(())
}

fn apply_main_item(
    authoritative: &mut AuthoritativeSimulation,
    item: MainItem,
) -> Result<(), String> {
    match item {
        MainItem::LobbyStatusUnchanged(status) => {
            authoritative.network_lobby_error = None;
            authoritative.network_lobby_status = Some(status);
        }
        MainItem::LobbyMatchRulesRejected => {
            authoritative.network_lobby_error =
                Some("Only the host may change match settings before start.".to_owned());
        }
        MainItem::LobbyRaceRejected(reason) => {
            authoritative.network_lobby_error = Some(match reason {
                LobbyRaceRejectReason::AlreadyStarted => {
                    "Race selection is closed after the match starts.".to_owned()
                }
                LobbyRaceRejectReason::RaceUnavailable => {
                    "That race is unavailable in this release.".to_owned()
                }
            });
        }
        MainItem::LobbyStartRejected(reason) => {
            authoritative.network_lobby_error = Some(match reason {
                LobbyStartRejectReason::NotHost => "Only the host can start the match.".to_owned(),
                LobbyStartRejectReason::WaitingForPlayers => {
                    "Waiting for all configured player slots to connect.".to_owned()
                }
                LobbyStartRejectReason::AlreadyStarted => {
                    "The match has already started.".to_owned()
                }
            });
        }
        MainItem::LobbyTeamSizeRejected(reason) => {
            authoritative.network_lobby_error = Some(
                match reason {
                    LobbyTeamSizeRejectReason::NotHost => "Only the host can change the team size.",
                    LobbyTeamSizeRejectReason::AlreadyStarted => "The match has already started.",
                    LobbyTeamSizeRejectReason::InvalidSize => "That team size is not supported.",
                    LobbyTeamSizeRejectReason::TooManyPlayers => {
                        "More players are connected than the smaller match has slots."
                    }
                }
                .to_owned(),
            );
        }
        MainItem::LobbySlotRejected(reason) => {
            authoritative.network_lobby_error = Some(
                match reason {
                    LobbySlotRejectReason::AlreadyStarted => "The match has already started.",
                    LobbySlotRejectReason::SlotUnavailable => {
                        "That slot is taken or no longer exists."
                    }
                }
                .to_owned(),
            );
        }
        MainItem::Kicked => authoritative.kicked = true,
        MainItem::Chat(chat) => authoritative.pending_chat.push(chat),
        MainItem::ReconnectFailed(reason) => {
            // Retried every second, so it updates the connection banner rather than adding a new
            // message each time.
            authoritative.connection_issue = Some(format!("Reconnect attempt failed: {reason}"));
        }
        MainItem::Acknowledged(acknowledgement) => {
            let status = match acknowledgement {
                CommandAcknowledgement::Scheduled {
                    client_sequence,
                    tick,
                    duplicate,
                    ..
                } => {
                    if duplicate {
                        format!("Command #{client_sequence} already scheduled for tick {tick}.")
                    } else {
                        format!("Command #{client_sequence} accepted for tick {tick}.")
                    }
                }
                CommandAcknowledgement::Rejected {
                    client_sequence,
                    reason,
                    duplicate,
                } => {
                    if let AuthorityMode::Network {
                        assigned_player, ..
                    } = authoritative.authority
                    {
                        authoritative
                            .pending_build_commands
                            .resolve(assigned_player, client_sequence);
                    }
                    let duplicate = if duplicate { " duplicate" } else { "" };
                    format!("Command #{client_sequence}{duplicate} rejected by server: {reason:?}.")
                }
            };
            authoritative.pending_status.push(status);
        }
        MainItem::ProtocolError(code) => {
            authoritative
                .pending_status
                .push(format!("Server protocol error: {code:?}."));
            if matches!(
                code,
                ProtocolErrorCode::MalformedMessage | ProtocolErrorCode::Unauthorized
            ) {
                return Err(format!("fatal server protocol error: {code:?}"));
            }
        }
    }
    Ok(())
}

/// Executes the queue now, on the calling thread, against the resident simulation.
pub(crate) fn run_queue_inline(
    authoritative: &mut AuthoritativeSimulation,
    presentation: &mut PresentationSamples,
    performance: Option<&mut PerformanceCounters>,
) -> Result<(), String> {
    let items = std::mem::take(&mut authoritative.network_queue);
    if items.is_empty() {
        return Ok(());
    }
    let catch_up = CatchUpMode::of(authoritative);
    let observer = presentation.observer();
    let AuthoritativeSimulation {
        simulation,
        driver,
        expected_execution_batch,
        ..
    } = authoritative;
    let NetRun { outputs, error } = execute_items(
        simulation,
        driver,
        expected_execution_batch,
        catch_up,
        observer,
        items,
    );
    finish_run(authoritative, presentation, performance, outputs, error)
}

/// Applies the outputs and then the failure of the run, in the order sequential processing would
/// have met them.
pub(crate) fn finish_run(
    authoritative: &mut AuthoritativeSimulation,
    presentation: &mut PresentationSamples,
    performance: Option<&mut PerformanceCounters>,
    outputs: Vec<NetOutput>,
    error: Option<String>,
) -> Result<(), String> {
    apply_outputs(authoritative, presentation, performance, outputs)?;
    error.map_or(Ok(()), Err)
}

/// Drains the network client and classifies every event. Queued items stay in
/// `authoritative.network_queue` for the caller to execute.
pub(crate) fn ingest_events(
    authoritative: &mut AuthoritativeSimulation,
    presentation: &mut PresentationSamples,
    selected_match: &mut SelectedMatch,
    mut performance: Option<&mut PerformanceCounters>,
) -> Result<(), String> {
    let events = match &mut authoritative.authority {
        AuthorityMode::Local => return Ok(()),
        AuthorityMode::Network { client, .. } => {
            // A finished match is over for good: the server stops and drops its clients, and there
            // is nothing to rejoin.
            if !match_finished(&authoritative.simulation) {
                client.poll_reconnect();
            }
            client.drain_events()
        }
    };
    presentation.enable_network_timeline();
    for event in events {
        match classify(authoritative, selected_match, event) {
            Classified::Queue(item) => authoritative.network_queue.push(item),
            Classified::Barrier(event) => {
                run_queue_inline(authoritative, presentation, performance.as_deref_mut())?;
                handle_barrier(authoritative, presentation, selected_match, event)?;
            }
        }
    }
    Ok(())
}

enum Classified {
    Queue(NetItem),
    Barrier(NetworkEvent),
}

fn classify(
    authoritative: &AuthoritativeSimulation,
    selected_match: &SelectedMatch,
    event: NetworkEvent,
) -> Classified {
    let message = match event {
        NetworkEvent::Message(message) => message,
        NetworkEvent::ReconnectFailed(reason) => {
            return Classified::Queue(NetItem::Main(MainItem::ReconnectFailed(reason)));
        }
        event @ (NetworkEvent::Disconnected(_) | NetworkEvent::Reconnected(_)) => {
            return Classified::Barrier(event);
        }
    };
    let item = match message {
        ServerMessage::LobbyStatus { status } => {
            let unchanged = matches!(
                plan_network_lobby_roster(
                    &authoritative.authority,
                    selected_match.content,
                    &status
                ),
                Ok(None)
            );
            if !unchanged {
                return Classified::Barrier(NetworkEvent::Message(ServerMessage::LobbyStatus {
                    status,
                }));
            }
            NetItem::Main(MainItem::LobbyStatusUnchanged(status))
        }
        ServerMessage::LobbyMatchRulesRejected => NetItem::Main(MainItem::LobbyMatchRulesRejected),
        ServerMessage::LobbyRaceRejected { reason } => {
            NetItem::Main(MainItem::LobbyRaceRejected(reason))
        }
        ServerMessage::LobbyStartRejected { reason } => {
            NetItem::Main(MainItem::LobbyStartRejected(reason))
        }
        ServerMessage::LobbyTeamSizeRejected { reason } => {
            NetItem::Main(MainItem::LobbyTeamSizeRejected(reason))
        }
        ServerMessage::LobbySlotRejected { reason } => {
            NetItem::Main(MainItem::LobbySlotRejected(reason))
        }
        ServerMessage::Kicked => NetItem::Main(MainItem::Kicked),
        ServerMessage::Chat {
            player_id,
            text,
            team_only,
        } => NetItem::Main(MainItem::Chat(IncomingChat {
            player_id,
            text,
            team_only,
        })),
        ServerMessage::CommandAcknowledged { acknowledgement } => {
            NetItem::Main(MainItem::Acknowledged(acknowledgement))
        }
        ServerMessage::ProtocolError { code } => NetItem::Main(MainItem::ProtocolError(code)),
        ServerMessage::StreamRecord { record } => {
            NetItem::Sim(SimItem::Record(CanonicalStreamRecord::from(record)))
        }
        ServerMessage::TickExecutions { batch } => NetItem::Sim(SimItem::ExecutionBatch(batch)),
        ServerMessage::Checkpoint { checkpoint } => NetItem::Sim(SimItem::Checkpoint(checkpoint)),
        // Changes the observer that live records restrict their snapshots to.
        message @ (ServerMessage::SlotAssigned { .. }
        | ServerMessage::SnapshotBegin { .. }
        | ServerMessage::SnapshotChunk { .. }
        | ServerMessage::CatchUpComplete { .. }
        | ServerMessage::HelloAccepted { .. }
        | ServerMessage::HelloRejected { .. }) => {
            return Classified::Barrier(NetworkEvent::Message(message));
        }
    };
    Classified::Queue(item)
}

/// Handles an event that must observe or replace the whole simulation, with every earlier item
/// already applied.
fn handle_barrier(
    authoritative: &mut AuthoritativeSimulation,
    presentation: &mut PresentationSamples,
    selected_match: &mut SelectedMatch,
    event: NetworkEvent,
) -> Result<(), String> {
    match event {
        NetworkEvent::Disconnected(reason) => {
            authoritative.pending_build_commands.clear();
            presentation.freeze_network_timeline();
            authoritative.catch_up = None;
            authoritative.expected_execution_batch = None;
            if let AuthorityMode::Network {
                connected,
                pending_handoff_position,
                ..
            } = &mut authoritative.authority
            {
                *connected = false;
                *pending_handoff_position = None;
            }
            if !match_finished(&authoritative.simulation) {
                authoritative
                    .pending_status
                    .push(format!("Disconnected from server: {reason}"));
                authoritative.connection_issue = Some(reason);
            }
        }
        NetworkEvent::Reconnected(assignment) => {
            authoritative.connection_issue = None;
            presentation.freeze_network_timeline();
            let (assigned_player, expected_team) = match &authoritative.authority {
                AuthorityMode::Network {
                    assigned_player, ..
                } => {
                    let expected_team = authoritative
                        .simulation
                        .player(*assigned_player)
                        .ok_or_else(|| "assigned network player disappeared locally".to_owned())?
                        .team
                        .0;
                    (*assigned_player, expected_team)
                }
                AuthorityMode::Local => unreachable!("network event in local mode"),
            };
            if assignment.player_id != assigned_player.0 || assignment.team != expected_team {
                return Err("server reconnect assignment changed player identity".to_owned());
            }
            authoritative.catch_up = None;
            authoritative.expected_execution_batch = None;
            authoritative.pending_feedback.clear();
            if let AuthorityMode::Network {
                next_sequence,
                connected,
                pending_handoff_position,
                ..
            } = &mut authoritative.authority
            {
                *next_sequence = assignment.next_client_sequence;
                *connected = false;
                *pending_handoff_position = Some(assignment.next_stream_position);
            }
            authoritative
                .pending_status
                .push("Reconnected; synchronizing authoritative state.".to_owned());
        }
        NetworkEvent::ReconnectFailed(_) => unreachable!("reconnect failures are never barriers"),
        NetworkEvent::Message(message) => match message {
            ServerMessage::LobbyStatus { status } => {
                apply_network_lobby_roster(authoritative, presentation, selected_match, &status)?;
                authoritative.network_lobby_error = None;
                authoritative.network_lobby_status = Some(status);
            }
            ServerMessage::SlotAssigned { player_id, team } => {
                if let AuthorityMode::Network {
                    assigned_player, ..
                } = &mut authoritative.authority
                {
                    *assigned_player = PlayerId(player_id);
                }
                selected_match.local_player = PlayerId(player_id);
                presentation.set_observer_team(Some(Team(team)));
            }
            ServerMessage::SnapshotBegin { begin } => {
                begin_snapshot_catch_up(authoritative, begin)?
            }
            ServerMessage::SnapshotChunk { chunk } => append_snapshot_chunk(authoritative, chunk)?,
            ServerMessage::CatchUpComplete { complete } => {
                finish_snapshot_catch_up(authoritative, presentation, complete)?;
            }
            ServerMessage::HelloAccepted { .. } | ServerMessage::HelloRejected { .. } => {
                return Err("received handshake response after session startup".to_owned());
            }
            ServerMessage::LobbyMatchRulesRejected
            | ServerMessage::LobbyRaceRejected { .. }
            | ServerMessage::LobbyStartRejected { .. }
            | ServerMessage::LobbyTeamSizeRejected { .. }
            | ServerMessage::LobbySlotRejected { .. }
            | ServerMessage::Kicked
            | ServerMessage::Chat { .. }
            | ServerMessage::CommandAcknowledged { .. }
            | ServerMessage::StreamRecord { .. }
            | ServerMessage::TickExecutions { .. }
            | ServerMessage::Checkpoint { .. }
            | ServerMessage::ProtocolError { .. } => {
                unreachable!("queued messages are never barriers")
            }
        },
    }
    Ok(())
}

fn match_finished(simulation: &Simulation) -> bool {
    matches!(simulation.lifecycle(), MatchLifecycle::Finished { .. })
}
