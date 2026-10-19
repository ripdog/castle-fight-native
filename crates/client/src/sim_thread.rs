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
//! Networked play keeps stepping synchronously in `FixedUpdate` (`advance_network_simulation`).

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
use castle_fight_sim::{CanonicalStreamError, DriverTickResult, MatchDriver, Simulation};

use crate::{
    AuthoritativeSimulation, SelectedMatch, SimulationPlayback,
    bridge::{ObserverVision, PresentationSamples, PresentationSnapshot},
    fixed_catch_up::OFFLINE_MAX_FIXED_STEPS_PER_FRAME,
    performance_ui::PerformanceCounters,
    process_network_events,
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

struct TickJob {
    simulation: Simulation,
    driver: MatchDriver,
    ticks: u32,
    observer: Option<ObserverVision>,
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

/// Long-lived worker thread plus the bookkeeping of the job in flight.
#[derive(Resource)]
pub(crate) struct SimWorker {
    jobs: Option<Sender<TickJob>>,
    results: Mutex<Option<Receiver<TickBatch>>>,
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
        let (job_sender, job_receiver) = mpsc::channel::<TickJob>();
        let (result_sender, result_receiver) = mpsc::channel::<TickBatch>();
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

    fn submit(&mut self, job: TickJob) {
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

    fn wait(&mut self) -> TickBatch {
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

fn run_job(job: TickJob) -> TickBatch {
    let TickJob {
        mut simulation,
        mut driver,
        ticks,
        observer,
    } = job;
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
            .add_systems(
                FixedUpdate,
                (advance_network_simulation, count_due_sim_ticks),
            )
            .add_systems(PostUpdate, launch_sim_ticks)
            .add_systems(
                RunFixedMainLoop,
                join_sim_ticks
                    .in_set(SimJoin)
                    .in_set(RunFixedMainLoopSystems::AfterFixedMainLoop),
            );
    }
}

/// Networked play applies server records synchronously, as before.
pub(crate) fn advance_network_simulation(
    mut authoritative: ResMut<AuthoritativeSimulation>,
    mut presentation: ResMut<PresentationSamples>,
    mut selected_match: ResMut<SelectedMatch>,
    mut performance: Option<ResMut<PerformanceCounters>>,
) {
    if !authoritative.is_networked() {
        return;
    }
    let fixed_started = Instant::now();
    let revision = presentation.revision();
    let result = process_network_events(
        &mut authoritative,
        presentation.bypass_change_detection(),
        &mut selected_match,
        performance.as_deref_mut(),
    );
    if presentation.revision() != revision {
        presentation.set_changed();
    }
    if let Some(performance) = performance.as_deref_mut() {
        performance.record_fixed_update_wall(fixed_started.elapsed());
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
    if ticks == 0 || authoritative.is_networked() {
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
    worker.submit(TickJob {
        simulation,
        driver,
        ticks,
        observer: presentation.observer(),
    });
}

/// Returns the simulation to the resource and applies the outputs of the finished ticks.
pub(crate) fn join_sim_ticks(
    mut authoritative: ResMut<AuthoritativeSimulation>,
    mut presentation: ResMut<PresentationSamples>,
    mut worker: ResMut<SimWorker>,
    mut performance: Option<ResMut<PerformanceCounters>>,
) {
    if authoritative.is_networked() {
        // The live fixed clock drives interpolation; the network timeline overrides it anyway.
        presentation
            .bypass_change_detection()
            .set_render_overstep(None);
        return;
    }
    let joined_at = Instant::now();
    if worker.in_flight {
        let TickBatch {
            simulation,
            driver,
            outputs,
            error,
        } = worker.wait();
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
        if let Some(performance) = performance.as_deref_mut() {
            let wall = joined_at.elapsed();
            for run in 0..ticks.max(1) {
                performance.record_fixed_update_wall(if run == 0 { wall } else { Duration::ZERO });
            }
        }
    }
    presentation
        .bypass_change_detection()
        .set_render_overstep(worker.launched_overstep);
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

    #[test]
    #[should_panic(expected = "simulation is in flight on the sim thread")]
    fn simulation_access_while_in_flight_fails_loudly() {
        let demo = create_demo_world(1, Some(0));
        let mut authoritative = AuthoritativeSimulation::new(demo.simulation, demo.content);
        drop(authoritative.simulation.take());
        let _ = authoritative.simulation.tick();
    }
}
