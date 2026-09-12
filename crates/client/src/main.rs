mod bridge;
mod demo;
mod presentation;

use bevy::{prelude::*, time::Fixed};
use castle_fight_sim::Simulation;

use bridge::{PresentationSamples, PresentationSnapshot};
use demo::create_demo_world;
use presentation::CastlePresentationPlugin;

const SIMULATION_HZ: f64 = 30.0;

#[derive(Resource)]
struct AuthoritativeSimulation {
    simulation: Simulation,
}

fn main() {
    let demo = create_demo_world(default_worker_count());
    let initial_snapshot = PresentationSnapshot::capture(&demo.simulation);

    App::new()
        .insert_resource(ClearColor(Color::srgb(0.025, 0.03, 0.04)))
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
        .add_plugins(CastlePresentationPlugin)
        .add_systems(FixedUpdate, advance_authoritative_simulation)
        .run();
}

fn advance_authoritative_simulation(
    mut authoritative: ResMut<AuthoritativeSimulation>,
    mut presentation: ResMut<PresentationSamples>,
) {
    authoritative.simulation.step();
    presentation.publish(PresentationSnapshot::capture(&authoritative.simulation));
}

fn default_worker_count() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(8)
}
