mod bridge;
mod build_ui;
mod demo;
mod presentation;

use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    prelude::*,
    time::Fixed,
};
use castle_fight_sim::Simulation;

use bridge::{PresentationSamples, PresentationSnapshot};
use build_ui::{BuildSelection, BuildUiPlugin, PendingBuildPlacements};
use demo::{create_demo_world, production_structure};
use presentation::CastlePresentationPlugin;

const SIMULATION_HZ: f64 = 30.0;

#[derive(Resource)]
pub(crate) struct AuthoritativeSimulation {
    simulation: Simulation,
}

fn main() {
    let options = ClientOptions::parse();
    let demo = create_demo_world(default_worker_count(), options.stress_units);
    let initial_snapshot = PresentationSnapshot::capture(&demo.simulation);

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.025, 0.03, 0.04)))
        .insert_resource(Time::<Fixed>::from_hz(SIMULATION_HZ))
        .insert_resource(AuthoritativeSimulation {
            simulation: demo.simulation,
        })
        .insert_resource(PresentationSamples::new(initial_snapshot))
        .insert_resource(demo.metrics)
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Castle Fight Native 3D".into(),
                resolution: (1440, 900).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins((
            CastlePresentationPlugin::new(options.health_bars),
            BuildUiPlugin,
        ))
        .add_systems(FixedUpdate, advance_authoritative_simulation);

    if options.perf_log {
        app.add_plugins(LogDiagnosticsPlugin::default());
    }

    app.run();
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
        match authoritative
            .simulation
            .try_spawn_building(production_structure(
                request.team,
                request.footprint,
                request.kind,
            )) {
            Ok(_) => {
                build_selection.status = format!(
                    "Placed {} {}.",
                    if request.team.0 == 0 { "Blue" } else { "Red" },
                    request.kind.label()
                );
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
