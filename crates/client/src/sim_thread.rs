//! Runs offline simulation ticks on a dedicated worker thread, concurrently with the frame's
//! presentation work.
//!
//! The deterministic simulation is unchanged: the worker calls the same
//! `advance_authoritative_simulation_once` tick path as every other caller and produces the same
//! per-tick outputs in the same order. Only the *schedule* differs:
//!
//! 1. `FixedUpdate` does not step the simulation. It only counts the ticks the fixed clock says are
//!    due (`count_due_sim_ticks`), capped like the offline catch-up limit.
//! 2. `PostUpdate` starts with `launch_sim_ticks`, which moves the simulation and driver into a job
//!    for the worker thread. Every `Update` system has already run, so all commands submitted this
//!    frame are in the driver before the job starts. The job overlaps the rest of `PostUpdate`,
//!    `Last`, render extraction and the next frame's `First`/`PreUpdate`.
//! 3. `join_sim_ticks` runs at the end of `RunFixedMainLoop`, before any `Update` system. It
//!    blocks until the job is finished, returns the simulation and driver to the resource and
//!    applies the captured per-tick outputs (presentation samples, command feedback, performance
//!    counters) in tick order.
//!
//! While a job is in flight the resource's `simulation` and `driver` slots are empty and any access
//! panics, so a system that reads the simulation outside the window between `join_sim_ticks` and
//! `launch_sim_ticks` fails loudly instead of observing stale or missing state. Presentation shows
//! the ticks one frame after the fixed clock counted them; `PresentationSamples` records the fixed
//! overstep fraction from the launching frame so interpolation stays continuous.
//!
//! Networked play uses the same worker. `ingest_network_events` (end of `Update`'s successor,
//! `PostUpdate`, before the launch) drains the network client on the main thread and queues the
//! finalized stream records, execution batches and checkpoints it received, in server order; the
//! launch hands that queue to the worker, which applies it to the simulation (`network_sim`).
//! Events that replace or inspect the whole simulation (snapshot transfer, reconnects, roster
//! changes) are applied synchronously at ingest after the earlier queue has been applied. Join
//! applies the per-item outputs on the main thread in queue order: presentation timeline,
//! command feedback, checkpoint reports and status text.

use std::{
    ops::{Deref, DerefMut},
    sync::{
        Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use bevy::{
    ecs::schedule::SystemSet,
    prelude::*,
    time::{Fixed, Time},
};
use castle_fight_protocol::WireCommandExecution;
use castle_fight_sim::{CanonicalStreamError, DriverTickResult, MatchDriver, Simulation};

use crate::{
    AuthoritativeSimulation, SelectedMatch, SimulationPlayback,
    bridge::{ObserverVision, PresentationSamples, PresentationSnapshot},
    fixed_catch_up::OFFLINE_MAX_FIXED_STEPS_PER_FRAME,
    network_sim::{self, CatchUpMode, NetItem, NetRun},
    performance_ui::PerformanceCounters,
};

/// Ordering handle for systems that must run after the simulation has been joined back into the
/// resource and before any `Update` system.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SimJoin;

const SIM_THREAD_STACK_BYTES: usize = 32 * 1024 * 1024;

/// Owner of a value that can be moved to the simulation thread. Dereferencing while the value is
/// away panics.
pub(crate) struct Movable<T> {
    value: Option<T>,
    what: &'static str,
}

impl<T> Movable<T> {
    pub(crate) fn new(value: T, what: &'static str) -> Self {
        Self {
            value: Some(value),
            what,
        }
    }

    pub(crate) fn take(&mut self) -> Option<T> {
        self.value.take()
    }

    pub(crate) fn put_back(&mut self, value: T) {
        assert!(
            self.value.replace(value).is_none(),
            "{} returned while already present",
            self.what
        );
    }

    /// Replaces the value, whether or not it is currently present.
    pub(crate) fn set(&mut self, value: T) {
        self.value = Some(value);
    }
}

impl<T> Deref for Movable<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.value
            .as_ref()
            .unwrap_or_else(|| panic!("{} is in flight on the sim thread", self.what))
    }
}

impl<T> DerefMut for Movable<T> {
    fn deref_mut(&mut self) -> &mut T {
        let what = self.what;
        self.value
            .as_mut()
            .unwrap_or_else(|| panic!("{what} is in flight on the sim thread"))
    }
}

enum Job {
    /// Offline: step the local driver `ticks` times.
    Local {
        simulation: Simulation,
        driver: MatchDriver,
        ticks: u32,
        observer: Option<ObserverVision>,
    },
    /// Networked: apply the queued server items in order.
    Network {
        simulation: Simulation,
        driver: MatchDriver,
        expected_execution_batch: Option<(u64, Vec<WireCommandExecution>)>,
        catch_up: Option<CatchUpMode>,
        observer: Option<ObserverVision>,
        items: Vec<NetItem>,
    },
}

/// Everything the main thread used to do right after one tick.
pub(crate) struct TickOutput {
    pub(crate) result: DriverTickResult,
    pub(crate) snapshot: PresentationSnapshot,
}

struct TickBatch {
    simulation: Simulation,
    driver: MatchDriver,
    outputs: Vec<TickOutput>,
    error: Option<CanonicalStreamError>,
}

struct NetBatch {
    simulation: Simulation,
    driver: MatchDriver,
    expected_execution_batch: Option<(u64, Vec<WireCommandExecution>)>,
    run: NetRun,
}

enum Batch {
    Local(TickBatch),
    Network(NetBatch),
}

/// Long-lived worker thread plus the bookkeeping of the job in flight.
#[derive(Resource)]
pub(crate) struct SimWorker {
    jobs: Option<Sender<Job>>,
    results: Mutex<Option<Receiver<Batch>>>,
    in_flight: bool,
    /// Fixed overstep fraction observed by the most recent launch.
    launched_overstep: Option<f32>,
}

impl Default for SimWorker {
    fn default() -> Self {
        Self {
            jobs: None,
            results: Mutex::new(None),
            in_flight: false,
            launched_overstep: None,
        }
    }
}

impl SimWorker {
    fn spawn(&mut self) {
        let (job_sender, job_receiver) = mpsc::channel::<Job>();
        let (result_sender, result_receiver) = mpsc::channel::<Batch>();
        thread::Builder::new()
            .name("sim-tick".to_owned())
            .stack_size(SIM_THREAD_STACK_BYTES)
            .spawn(move || {
                while let Ok(job) = job_receiver.recv() {
                    if result_sender.send(run_job(job)).is_err() {
                        return;
                    }
                }
            })
            .expect("failed to spawn the simulation thread");
        self.jobs = Some(job_sender);
        *self
            .results
            .get_mut()
            .expect("sim result channel lock poisoned") = Some(result_receiver);
    }

    fn submit(&mut self, job: Job) {
        if self.jobs.is_none() {
            self.spawn();
        }
        self.jobs
            .as_ref()
            .expect("sim thread job channel exists")
            .send(job)
            .expect("simulation thread terminated");
        self.in_flight = true;
    }

    fn wait(&mut self) -> Batch {
        let receiver = self
            .results
            .get_mut()
            .expect("sim result channel lock poisoned")
            .as_ref()
            .expect("sim thread was never started");
        let batch = receiver
            .recv()
            .expect("simulation thread terminated while a tick job was in flight");
        self.in_flight = false;
        batch
    }
}

fn run_job(job: Job) -> Batch {
    match job {
        Job::Local {
            simulation,
            driver,
            ticks,
            observer,
        } => Batch::Local(run_local_ticks(simulation, driver, ticks, observer)),
        Job::Network {
            mut simulation,
            mut driver,
            mut expected_execution_batch,
            catch_up,
            observer,
            items,
        } => {
            let run = network_sim::execute_items(
                &mut simulation,
                &mut driver,
                &mut expected_execution_batch,
                catch_up,
                observer,
                items,
            );
            Batch::Network(NetBatch {
                simulation,
                driver,
                expected_execution_batch,
                run,
            })
        }
    }
}

fn run_local_ticks(
    mut simulation: Simulation,
    mut driver: MatchDriver,
    ticks: u32,
    observer: Option<ObserverVision>,
) -> TickBatch {
    let mut outputs = Vec::with_capacity(ticks as usize);
    let mut error = None;
    for _ in 0..ticks {
        match driver.advance_local_tick(&mut simulation) {
            Ok(result) => {
                let mut snapshot = PresentationSnapshot::capture(&simulation);
                if let Some(observer) = observer {
                    snapshot.restrict_to_observer(observer);
                }
                outputs.push(TickOutput { result, snapshot });
            }
            // The match ended; later due ticks are no-ops exactly as they were on the main thread.
            Err(CanonicalStreamError::MatchNotRunning(_)) => break,
            Err(failure) => {
                error = Some(failure);
                break;
            }
        }
    }
    TickBatch {
        simulation,
        driver,
        outputs,
        error,
    }
}

pub(crate) struct SimThreadPlugin;

impl Plugin for SimThreadPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SimWorker>()
            .add_systems(FixedUpdate, count_due_sim_ticks)
            .add_systems(
                PostUpdate,
                (ingest_network_events, launch_sim_ticks).chain(),
            )
            .add_systems(
                RunFixedMainLoop,
                join_sim_ticks
                    .in_set(SimJoin)
                    .in_set(RunFixedMainLoopSystems::AfterFixedMainLoop),
            );
    }
}

/// Reads this frame's server events on the main thread and queues the simulation work for the
/// next job. Runs in `PostUpdate` while the simulation is resident, before `launch_sim_ticks`.
/// Barrier events (snapshot transfer, reconnects, roster changes) are applied here, synchronously.
/// A queue that needs no simulation is applied immediately.
pub(crate) fn ingest_network_events(
    mut authoritative: ResMut<AuthoritativeSimulation>,
    mut presentation: ResMut<PresentationSamples>,
    mut selected_match: ResMut<SelectedMatch>,
    mut performance: Option<ResMut<PerformanceCounters>>,
) {
    if !authoritative.is_networked() {
        return;
    }
    let started = Instant::now();
    let revision = presentation.revision();
    let result = (|| {
        network_sim::ingest_events(
            &mut authoritative,
            presentation.bypass_change_detection(),
            &mut selected_match,
            performance.as_deref_mut(),
        )?;
        if NetRun::needs_simulation(&authoritative.network_queue) {
            return Ok(());
        }
        network_sim::run_queue_inline(
            &mut authoritative,
            presentation.bypass_change_detection(),
            performance.as_deref_mut(),
        )
    })();
    if presentation.revision() != revision {
        presentation.set_changed();
    }
    if let Some(performance) = performance.as_deref_mut() {
        performance.record_fixed_update_wall(started.elapsed());
    }
    if let Err(error) = result {
        panic!("network canonical stream invariant failed: {error}");
    }
}

/// Counts one due tick per fixed step. The fixed clock already runs at most
/// `OFFLINE_MAX_FIXED_STEPS_PER_FRAME` steps per frame; the cap here keeps a job bounded even
/// if that limit is bypassed.
pub(crate) fn count_due_sim_ticks(
    playback: Res<SimulationPlayback>,
    mut authoritative: ResMut<AuthoritativeSimulation>,
) {
    if authoritative.is_networked() || playback.paused {
        return;
    }
    authoritative.due_ticks = (authoritative.due_ticks + 1).min(OFFLINE_MAX_FIXED_STEPS_PER_FRAME);
}

/// Starts the due ticks on the simulation thread. Runs in `PostUpdate`, after every `Update`
/// system has finished with the simulation.
pub(crate) fn launch_sim_ticks(
    mut authoritative: ResMut<AuthoritativeSimulation>,
    presentation: Res<PresentationSamples>,
    fixed_time: Res<Time<Fixed>>,
    mut worker: ResMut<SimWorker>,
) {
    assert!(
        !worker.in_flight,
        "a simulation job was still in flight at launch"
    );
    worker.launched_overstep = Some(fixed_time.overstep_fraction());
    let ticks = std::mem::take(&mut authoritative.due_ticks);
    let networked = authoritative.is_networked();
    if networked {
        if authoritative.network_queue.is_empty() {
            return;
        }
    } else if ticks == 0 {
        return;
    }
    let simulation = authoritative
        .simulation
        .take()
        .expect("simulation is in flight on the sim thread");
    let driver = authoritative
        .driver
        .take()
        .expect("driver is in flight on the sim thread");
    let observer = presentation.observer();
    if networked {
        let authoritative = &mut *authoritative;
        worker.submit(Job::Network {
            simulation,
            driver,
            expected_execution_batch: authoritative.expected_execution_batch.take(),
            catch_up: CatchUpMode::of(authoritative),
            observer,
            items: std::mem::take(&mut authoritative.network_queue),
        });
    } else {
        worker.submit(Job::Local {
            simulation,
            driver,
            ticks,
            observer,
        });
    }
}

/// Returns the simulation to the resource and applies the outputs of the finished job.
pub(crate) fn join_sim_ticks(
    mut authoritative: ResMut<AuthoritativeSimulation>,
    mut presentation: ResMut<PresentationSamples>,
    mut worker: ResMut<SimWorker>,
    mut performance: Option<ResMut<PerformanceCounters>>,
) {
    let joined_at = Instant::now();
    let networked = authoritative.is_networked();
    if worker.in_flight {
        match worker.wait() {
            Batch::Local(batch) => join_local_batch(
                &mut authoritative,
                &mut presentation,
                performance.as_deref_mut(),
                joined_at,
                batch,
            ),
            Batch::Network(batch) => {
                let revision = presentation.revision();
                let result = join_network_batch(
                    &mut authoritative,
                    presentation.bypass_change_detection(),
                    performance.as_deref_mut(),
                    batch,
                );
                if presentation.revision() != revision {
                    presentation.set_changed();
                }
                if let Some(performance) = performance.as_deref_mut() {
                    performance.record_fixed_update_wall(joined_at.elapsed());
                }
                if let Err(error) = result {
                    panic!("network canonical stream invariant failed: {error}");
                }
            }
        }
    }
    // The live fixed clock drives offline interpolation; the network timeline overrides it.
    presentation
        .bypass_change_detection()
        .set_render_overstep(if networked {
            None
        } else {
            worker.launched_overstep
        });
}

fn join_local_batch(
    authoritative: &mut AuthoritativeSimulation,
    presentation: &mut PresentationSamples,
    mut performance: Option<&mut PerformanceCounters>,
    joined_at: Instant,
    batch: TickBatch,
) {
    let TickBatch {
        simulation,
        driver,
        outputs,
        error,
    } = batch;
    authoritative.simulation.put_back(simulation);
    authoritative.driver.put_back(driver);
    let ticks = outputs.len();
    for TickOutput { result, snapshot } in outputs {
        for execution in &result.executions {
            authoritative.pending_build_commands.resolve(
                execution.scheduled.player,
                execution.scheduled.client_sequence.0,
            );
        }
        authoritative
            .pending_feedback
            .extend(result.executions.iter().copied());
        // Snapshots were already restricted to the observer on the worker.
        presentation.publish(snapshot);
        if let Some(performance) = performance.as_deref_mut() {
            performance.record_sim_tick(
                result.tick_result.completed_tick,
                result.tick_result.timings,
                result.tick_result.collision_fallback_searches,
                result.tick_result.collision_fallback_candidate_checks,
                result.tick_result.collision_fallback_max_ring,
            );
        }
    }
    if let Some(error) = error {
        panic!("local canonical stream invariant failed: {error:?}");
    }
    if let Some(performance) = performance {
        let wall = joined_at.elapsed();
        for run in 0..ticks.max(1) {
            performance.record_fixed_update_wall(if run == 0 { wall } else { Duration::ZERO });
        }
    }
}

fn join_network_batch(
    authoritative: &mut AuthoritativeSimulation,
    presentation: &mut PresentationSamples,
    performance: Option<&mut PerformanceCounters>,
    batch: NetBatch,
) -> Result<(), String> {
    let NetBatch {
        simulation,
        driver,
        expected_execution_batch,
        run: NetRun { outputs, error },
    } = batch;
    authoritative.simulation.put_back(simulation);
    authoritative.driver.put_back(driver);
    authoritative.expected_execution_batch = expected_execution_batch;
    network_sim::finish_run(authoritative, presentation, performance, outputs, error)
}

#[cfg(test)]
mod tests {
    use castle_fight_sim::PlayerId;

    use super::*;
    use crate::{advance_authoritative_simulation_once, demo::create_demo_world};

    fn threaded_app(authoritative: AuthoritativeSimulation, samples: PresentationSamples) -> App {
        let mut app = App::new();
        app.insert_resource(SimulationPlayback::default())
            .insert_resource(authoritative)
            .insert_resource(samples)
            .init_resource::<SimWorker>()
            .insert_resource(Time::<Fixed>::from_hz(30.0))
            .add_systems(
                Update,
                (count_due_sim_ticks, launch_sim_ticks, join_sim_ticks).chain(),
            );
        app
    }

    #[test]
    fn threaded_ticks_match_synchronous_ticks_and_publish_in_order() {
        let sync_demo = create_demo_world(1, Some(0));
        let threaded_demo = create_demo_world(1, Some(0));
        let builder = sync_demo
            .simulation
            .builder_for_player(PlayerId(0))
            .expect("western builder")
            .id;
        let destination = castle_fight_sim::SimPoint::new(
            sync_demo.simulation.builder(builder).unwrap().position.x
                + 20 * castle_fight_sim::SUBUNITS_PER_WORLD_UNIT,
            sync_demo.simulation.builder(builder).unwrap().position.y,
        );
        let command = castle_fight_sim::PlayerCommand::MoveBuilder {
            builder,
            destination,
        };

        let mut synchronous = AuthoritativeSimulation::new(sync_demo.simulation, sync_demo.content);
        let mut sync_samples =
            PresentationSamples::new(PresentationSnapshot::capture(&synchronous.simulation));
        synchronous.submit_local_command(PlayerId(0), command);

        let samples =
            PresentationSamples::new(PresentationSnapshot::capture(&threaded_demo.simulation));
        let mut authoritative =
            AuthoritativeSimulation::new(threaded_demo.simulation, threaded_demo.content);
        authoritative.submit_local_command(PlayerId(0), command);
        let mut app = threaded_app(authoritative, samples);

        for _ in 0..8 {
            advance_authoritative_simulation_once(&mut synchronous, &mut sync_samples)
                .expect("synchronous tick");
            app.update();
            let authoritative = app.world().resource::<AuthoritativeSimulation>();
            let samples = app.world().resource::<PresentationSamples>();
            assert_eq!(
                authoritative.simulation.checksum(),
                synchronous.simulation.checksum()
            );
            assert_eq!(samples.current.tick, sync_samples.current.tick);
            assert_eq!(samples.previous.tick, sync_samples.previous.tick);
        }
        assert_eq!(
            app.world()
                .resource::<AuthoritativeSimulation>()
                .pending_feedback
                .len(),
            synchronous.pending_feedback.len()
        );
    }

    /// Server messages for a short match: a command, ticks with execution batches, an
    /// acknowledgement, a lobby rejection, chat, a boundary control, one matching and one
    /// diverging checkpoint.
    fn scripted_server_messages() -> Vec<castle_fight_protocol::ServerMessage> {
        use castle_fight_protocol::{Checkpoint, ServerMessage, WireExecutionBatch};
        use castle_fight_sim::CanonicalStreamRecord;

        let mut source = create_demo_world(1, None);
        let builder = source.simulation.builder_for_player(PlayerId(0)).unwrap();
        let mut driver = MatchDriver::new(&source.simulation, source.content);
        driver.submit_local_command(
            &source.simulation,
            PlayerId(0),
            castle_fight_sim::PlayerCommand::MoveBuilder {
                builder: builder.id,
                destination: castle_fight_sim::SimPoint::new(
                    builder.position.x + 20 * castle_fight_sim::SUBUNITS_PER_WORLD_UNIT,
                    builder.position.y,
                ),
            },
        );
        let mut messages = Vec::new();
        for step in 0..6 {
            let result = driver.advance_local_tick(&mut source.simulation).unwrap();
            messages.push(ServerMessage::StreamRecord {
                record: (&CanonicalStreamRecord::Tick(result.finalized.clone())).into(),
            });
            messages.push(ServerMessage::TickExecutions {
                batch: WireExecutionBatch {
                    tick: result.finalized.tick,
                    executions: result.executions.iter().copied().map(Into::into).collect(),
                },
            });
            if step == 1 {
                messages.push(ServerMessage::CommandAcknowledged {
                    acknowledgement: castle_fight_protocol::CommandAcknowledgement::Scheduled {
                        client_sequence: 0,
                        tick: 1,
                        duplicate: false,
                        order: 0,
                    },
                });
            }
            if step == 0 {
                messages.push(ServerMessage::LobbySlotRejected {
                    reason: castle_fight_protocol::LobbySlotRejectReason::AlreadyStarted,
                });
            }
            if step == 4 {
                messages.push(ServerMessage::Chat {
                    player_id: 6,
                    text: "gg".to_owned(),
                    team_only: false,
                });
            }
            if step == 3 {
                let control = driver
                    .emit_local_control(
                        &mut source.simulation,
                        castle_fight_sim::MatchControlEvent::Debug(
                            castle_fight_sim::debug::DebugCommand::GrantResources,
                        ),
                    )
                    .unwrap();
                messages.push(ServerMessage::StreamRecord {
                    record: (&CanonicalStreamRecord::Control(control)).into(),
                });
            }
            if step == 2 || step == 5 {
                let checksum = source.simulation.checksum();
                messages.push(ServerMessage::Checkpoint {
                    checkpoint: Checkpoint {
                        completed_tick: source.simulation.tick() - 1,
                        // The last checkpoint diverges to exercise the mismatch path.
                        checksum: if step == 5 { checksum ^ 1 } else { checksum },
                    },
                });
            }
        }
        messages
    }

    struct NetworkedRun {
        authoritative: AuthoritativeSimulation,
        reports: Vec<(u64, u64)>,
        displayed_ticks: Vec<(u64, u64)>,
    }

    fn networked_fixture() -> (
        AuthoritativeSimulation,
        PresentationSamples,
        crate::SelectedMatch,
        std::sync::mpsc::Receiver<castle_fight_protocol::ClientMessage>,
    ) {
        let demo = create_demo_world(4, None);
        let (client, outbound) =
            crate::network::NetworkClient::connected_test_fixture_with_outbound(
                crate::compatibility_identity_for_demo(&demo),
            );
        let samples = PresentationSamples::new(PresentationSnapshot::capture(&demo.simulation));
        let selected = crate::SelectedMatch {
            content: demo.content,
            direct_buildings: demo.direct_buildings,
            local_player: PlayerId(0),
        };
        let authoritative = AuthoritativeSimulation::new_networked(
            demo.simulation,
            demo.content,
            demo.match_config,
            client,
            PlayerId(0),
            0,
        );
        (authoritative, samples, selected, outbound)
    }

    fn inject(
        authoritative: &AuthoritativeSimulation,
        messages: &[castle_fight_protocol::ServerMessage],
    ) {
        let crate::AuthorityMode::Network { client, .. } = &authoritative.authority else {
            unreachable!()
        };
        for message in messages {
            client.inject_server_message_for_test(message.clone());
        }
    }

    fn finish_run(
        authoritative: AuthoritativeSimulation,
        mut samples: PresentationSamples,
        outbound: &std::sync::mpsc::Receiver<castle_fight_protocol::ClientMessage>,
    ) -> NetworkedRun {
        let reports = outbound
            .try_iter()
            .map(|message| match message {
                castle_fight_protocol::ClientMessage::CheckpointReport { report } => {
                    (report.completed_tick, report.checksum)
                }
                other => panic!("unexpected client message {other:?}"),
            })
            .collect();
        let mut displayed_ticks = Vec::new();
        for _ in 0..40 {
            samples.advance_network_timeline(1.0 / 30.0, 1.0, false, true);
            displayed_ticks.push((samples.previous.tick, samples.current.tick));
        }
        NetworkedRun {
            authoritative,
            reports,
            displayed_ticks,
        }
    }

    fn run_networked_synchronously(
        messages: &[castle_fight_protocol::ServerMessage],
    ) -> NetworkedRun {
        let (mut authoritative, mut samples, mut selected, outbound) = networked_fixture();
        inject(&authoritative, messages);
        crate::process_network_events(&mut authoritative, &mut samples, &mut selected, None)
            .expect("synchronous network events");
        finish_run(authoritative, samples, &outbound)
    }

    /// Feeds the messages to the threaded systems in `frames` roughly equal slices, one frame each.
    fn run_networked_threaded(
        messages: &[castle_fight_protocol::ServerMessage],
        frames: usize,
    ) -> NetworkedRun {
        let (authoritative, samples, selected, outbound) = networked_fixture();
        let mut app = App::new();
        app.insert_resource(SimulationPlayback::default())
            .insert_resource(authoritative)
            .insert_resource(samples)
            .insert_resource(selected)
            .init_resource::<SimWorker>()
            .insert_resource(Time::<Fixed>::from_hz(30.0))
            .add_systems(
                Update,
                (ingest_network_events, launch_sim_ticks, join_sim_ticks).chain(),
            );
        for slice in messages.chunks(messages.len().div_ceil(frames)) {
            inject(app.world().resource::<AuthoritativeSimulation>(), slice);
            app.update();
        }
        // One more frame joins nothing and must leave the simulation resident.
        app.update();
        let authoritative = app
            .world_mut()
            .remove_resource::<AuthoritativeSimulation>()
            .unwrap();
        let samples = app
            .world_mut()
            .remove_resource::<PresentationSamples>()
            .unwrap();
        finish_run(authoritative, samples, &outbound)
    }

    #[test]
    fn threaded_network_ticks_match_synchronous_application() {
        let messages = scripted_server_messages();
        let synchronous = run_networked_synchronously(&messages);
        assert_eq!(synchronous.reports.len(), 2);
        assert_eq!(synchronous.authoritative.pending_chat.len(), 1);
        assert!(
            synchronous
                .authoritative
                .pending_status
                .iter()
                .any(|status| status.starts_with("Checksum mismatch"))
        );
        for frames in [1, 2, 5, messages.len()] {
            let threaded = run_networked_threaded(&messages, frames);
            assert_eq!(
                threaded.authoritative.simulation.checksum(),
                synchronous.authoritative.simulation.checksum(),
                "{frames} frames"
            );
            assert_eq!(
                threaded.authoritative.simulation.tick(),
                synchronous.authoritative.simulation.tick()
            );
            assert_eq!(threaded.reports, synchronous.reports);
            assert_eq!(threaded.displayed_ticks, synchronous.displayed_ticks);
            assert_eq!(
                threaded.authoritative.pending_feedback.len(),
                synchronous.authoritative.pending_feedback.len()
            );
            assert_eq!(
                threaded.authoritative.pending_status,
                synchronous.authoritative.pending_status
            );
            assert_eq!(
                threaded.authoritative.network_lobby_error,
                synchronous.authoritative.network_lobby_error
            );
            let chat = |run: &NetworkedRun| {
                run.authoritative
                    .pending_chat
                    .iter()
                    .map(|chat| chat.text.clone())
                    .collect::<Vec<_>>()
            };
            assert_eq!(chat(&threaded), chat(&synchronous));
            assert_eq!(
                threaded.authoritative.expected_execution_batch,
                synchronous.authoritative.expected_execution_batch
            );
            assert_eq!(
                threaded.authoritative.driver.next_stream_position(),
                synchronous.authoritative.driver.next_stream_position()
            );
            assert!(threaded.authoritative.network_queue.is_empty());
        }
    }

    #[test]
    #[should_panic(
        expected = "network canonical stream invariant failed: received execution batch for tick 7 before its finalized input record"
    )]
    fn threaded_network_invariant_failure_surfaces_at_join() {
        let (authoritative, samples, selected, _outbound) = networked_fixture();
        let mut app = App::new();
        app.insert_resource(SimulationPlayback::default())
            .insert_resource(authoritative)
            .insert_resource(samples)
            .insert_resource(selected)
            .init_resource::<SimWorker>()
            .insert_resource(Time::<Fixed>::from_hz(30.0))
            .add_systems(
                Update,
                (ingest_network_events, launch_sim_ticks, join_sim_ticks).chain(),
            );
        inject(
            app.world().resource::<AuthoritativeSimulation>(),
            &[castle_fight_protocol::ServerMessage::TickExecutions {
                batch: castle_fight_protocol::WireExecutionBatch {
                    tick: 7,
                    executions: Vec::new(),
                },
            }],
        );
        app.update();
    }

    /// Real-time loopback check against an in-process server: one client applies the stream on the
    /// sim thread, the other synchronously, and both must stay in lockstep with the server.
    /// Run with `--ignored`; it depends on wall-clock pacing.
    #[test]
    #[ignore = "real-time loopback smoke test"]
    fn threaded_client_tracks_a_real_server_in_lockstep_with_a_synchronous_client() {
        use castle_fight_server::{
            AuthoritativeMatch, ServerMatchOptions, tcp::TcpAuthoritativeServer,
        };
        use castle_fight_sim::{
            CastleFightBuilderRace, CastleFightMatchConfig, CastleFightParticipantConfig,
            MapVersion, Team,
        };

        let participants = [(0u8, Team(0)), (6u8, Team(1))]
            .into_iter()
            .map(|(slot, team)| CastleFightParticipantConfig {
                id: PlayerId(slot),
                team,
                builder_race: CastleFightBuilderRace::Human,
            })
            .collect();
        let config = CastleFightMatchConfig::development_subset_with_participants(
            MapVersion::CASTLE_FIGHT_9_27,
            "r1",
            0x5eed,
            participants,
        )
        .unwrap();
        let server = TcpAuthoritativeServer::bind_lobby(
            "127.0.0.1:0",
            AuthoritativeMatch::new(config.clone(), 1, ServerMatchOptions::default()).unwrap(),
        )
        .unwrap();
        let address = server.local_addr().unwrap();
        std::thread::spawn(move || {
            let _ = server.run_until_match_end();
        });

        let connect = |nonce: u64| {
            let demo =
                crate::demo::create_demo_world_for_match_config(1, None, config.clone()).unwrap();
            let (client, assignment) = crate::network::NetworkClient::connect(
                address,
                crate::compatibility_identity_for_demo(&demo),
                nonce,
                "Tester",
            )
            .unwrap();
            let samples = PresentationSamples::new(PresentationSnapshot::capture(&demo.simulation));
            let selected = crate::SelectedMatch {
                content: demo.content,
                direct_buildings: demo.direct_buildings,
                local_player: PlayerId(assignment.player_id),
            };
            let authoritative = AuthoritativeSimulation::new_networked(
                demo.simulation,
                demo.content,
                demo.match_config,
                client,
                PlayerId(assignment.player_id),
                assignment.next_client_sequence,
            );
            (authoritative, samples, selected)
        };
        let (mut host, host_samples, host_selected) = connect(1);
        let (mut peer, mut peer_samples, mut peer_selected) = connect(2);
        host.request_network_start().expect("host starts the match");

        let mut app = App::new();
        app.insert_resource(SimulationPlayback::default())
            .insert_resource(host)
            .insert_resource(host_samples)
            .insert_resource(host_selected)
            .init_resource::<SimWorker>()
            .insert_resource(Time::<Fixed>::from_hz(30.0))
            .add_systems(
                Update,
                (ingest_network_events, launch_sim_ticks, join_sim_ticks).chain(),
            );

        let mut commanded = false;
        for _ in 0..150 {
            std::thread::sleep(Duration::from_millis(33));
            crate::process_network_events(&mut peer, &mut peer_samples, &mut peer_selected, None)
                .expect("synchronous client");
            app.update();
            let mut host = app.world_mut().resource_mut::<AuthoritativeSimulation>();
            if !commanded && host.simulation.tick() > 5 {
                commanded = true;
                let player = PlayerId(0);
                let builder = host.simulation.builder_for_player(player).unwrap();
                let (id, position) = (builder.id, builder.position);
                host.submit_local_command(
                    player,
                    castle_fight_sim::PlayerCommand::MoveBuilder {
                        builder: id,
                        destination: castle_fight_sim::SimPoint::new(
                            position.x + 20 * castle_fight_sim::SUBUNITS_PER_WORLD_UNIT,
                            position.y,
                        ),
                    },
                );
            }
        }
        // Let both consumers drain whatever is still in flight, then compare at the same tick.
        for _ in 0..200 {
            crate::process_network_events(&mut peer, &mut peer_samples, &mut peer_selected, None)
                .expect("synchronous client");
            app.update();
            let host = app.world().resource::<AuthoritativeSimulation>();
            if host.simulation.tick() == peer.simulation.tick() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let host = app.world().resource::<AuthoritativeSimulation>();
        assert!(host.simulation.tick() > 60, "match did not advance");
        assert_eq!(host.simulation.tick(), peer.simulation.tick());
        assert_eq!(host.simulation.checksum(), peer.simulation.checksum());
        for status in host.pending_status.iter().chain(&peer.pending_status) {
            assert!(
                !status.contains("mismatch") && !status.contains("Disconnected"),
                "{status}"
            );
        }
    }

    #[test]
    #[should_panic(expected = "simulation is in flight on the sim thread")]
    fn simulation_access_while_in_flight_fails_loudly() {
        let demo = create_demo_world(1, Some(0));
        let mut authoritative = AuthoritativeSimulation::new(demo.simulation, demo.content);
        drop(authoritative.simulation.take());
        let _ = authoritative.simulation.tick();
    }
}
