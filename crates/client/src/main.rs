mod bridge;
mod build_ui;
mod demo;
mod inspection;
mod presentation;
mod terrain;

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
    time::Fixed,
    window::PresentMode,
};
use castle_fight_sim::Simulation;

use bridge::{PresentationSamples, PresentationSnapshot};
use build_ui::{BuildSelection, BuildUiPlugin, PendingBuildPlacements};
use demo::{create_demo_world, try_spawn_demo_building};
use inspection::InspectionPlugin;
use presentation::CastlePresentationPlugin;
use terrain::TerrainSurface;

const SIMULATION_HZ: f64 = 30.0;

#[derive(Resource)]
pub(crate) struct AuthoritativeSimulation {
    simulation: Simulation,
}

fn main() {
    let options = ClientOptions::parse();
    let demo = create_demo_world(default_worker_count(), options.stress_units);
    let initial_snapshot = PresentationSnapshot::capture(&demo.simulation);
    let present_mode = if options.stress_units.is_some() {
        PresentMode::AutoNoVsync
    } else {
        PresentMode::AutoVsync
    };

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.025, 0.03, 0.04)))
        .insert_resource(Time::<Fixed>::from_hz(SIMULATION_HZ))
        .insert_resource(AuthoritativeSimulation {
            simulation: demo.simulation,
        })
        .insert_resource(PresentationSamples::new(initial_snapshot))
        .insert_resource(demo.metrics)
        .insert_resource(TerrainSurface::new(demo.terrain))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Castle Fight Native 3D".into(),
                resolution: (1440, 900).into(),
                present_mode,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins((
            CastlePresentationPlugin::new(options.health_bars),
            BuildUiPlugin,
            InspectionPlugin,
        ))
        .add_systems(FixedUpdate, advance_authoritative_simulation);

    if options.perf_log {
        app.insert_resource(PerfTelemetry(Timer::from_seconds(
            1.0,
            TimerMode::Repeating,
        )))
        .add_systems(Update, print_perf_telemetry);
    }

    app.run();
}

#[derive(Resource)]
struct PerfTelemetry(Timer);

fn print_perf_telemetry(
    time: Res<Time>,
    diagnostics: Res<DiagnosticsStore>,
    mut telemetry: ResMut<PerfTelemetry>,
    presentation: Res<PresentationSamples>,
) {
    if !telemetry.0.tick(time.delta()).just_finished() {
        return;
    }
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|diagnostic| diagnostic.smoothed());
    let frame_ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|diagnostic| diagnostic.smoothed());
    println!(
        "client-perf fps={:.1} frame_ms={:.2} units={} buildings={} corpses={} projectiles={}",
        fps.unwrap_or_default(),
        frame_ms.unwrap_or_default(),
        presentation.current.units.len(),
        presentation.current.buildings.len(),
        presentation.current.corpses.len(),
        presentation.current.projectiles.len(),
    );
}

#[derive(Debug, Clone, Copy)]
struct ClientOptions {
    stress_units: Option<usize>,
    health_bars: bool,
    perf_log: bool,
}

impl ClientOptions {
    fn parse() -> Self {
        let mut options = Self {
            stress_units: None,
            health_bars: true,
            perf_log: false,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--stress-units" => {
                    let value = args
                        .next()
                        .expect("--stress-units requires a non-negative integer");
                    options.stress_units = Some(
                        value
                            .parse()
                            .expect("--stress-units requires a non-negative integer"),
                    );
                }
                "--no-health-bars" => options.health_bars = false,
                "--perf-log" => options.perf_log = true,
                "-h" | "--help" => {
                    println!(
                        "Usage: cargo run -p castle-fight-client -- [--stress-units N] [--no-health-bars] [--perf-log]"
                    );
                    std::process::exit(0);
                }
                unknown => panic!("unknown client option: {unknown}"),
            }
        }
        options
    }
}

fn advance_authoritative_simulation(
    mut authoritative: ResMut<AuthoritativeSimulation>,
    mut presentation: ResMut<PresentationSamples>,
    mut pending_builds: ResMut<PendingBuildPlacements>,
    mut build_selection: ResMut<BuildSelection>,
) {
    for request in pending_builds.0.drain(..) {
        match try_spawn_demo_building(
            &mut authoritative.simulation,
            request.team,
            request.footprint,
            request.kind,
        ) {
            Ok(_) => {
                let side = if request.team.0 == 0 { "Blue" } else { "Red" };
                build_selection.status = if let Some(gold_cost) = request.kind.gold_cost() {
                    format!(
                        "Placed {side} {} (map cost: {gold_cost} gold).",
                        request.kind.label()
                    )
                } else {
                    format!("Placed {side} {}.", request.kind.label())
                };
            }
            Err(error) => {
                build_selection.status = format!("Placement rejected by simulation: {error:?}.");
            }
        }
    }

    authoritative.simulation.step();
    presentation.publish(PresentationSnapshot::capture(&authoritative.simulation));
}

fn default_worker_count() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(8)
}
