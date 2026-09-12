use std::collections::{HashMap, HashSet};

use bevy::{prelude::*, time::Fixed};
use castle_fight_sim::{
    SUBUNITS_PER_WORLD_UNIT, SimId, Simulation, SimulationConfig, Team, populate_lane_battle,
};

const PIXELS_PER_WORLD_UNIT: f32 = 10.0;
const SIMULATION_HZ: f64 = 30.0;
const VIEWER_UNITS: usize = 1_000;

#[derive(Resource)]
struct SimState {
    simulation: Simulation,
    presented: HashMap<SimId, Entity>,
}

#[derive(Component)]
struct PresentedUnit;

fn main() {
    App::new()
        .insert_resource(Time::<Fixed>::from_hz(SIMULATION_HZ))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Castle Fight Native — verification viewer".into(),
                resolution: (1280, 720).into(),
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup)
        .add_systems(FixedUpdate, step_simulation)
        .add_systems(Update, (sync_units, draw_targets))
        .run();
}

fn setup(mut commands: Commands) {
    let mut simulation = Simulation::new(SimulationConfig::default(), default_worker_count());
    populate_lane_battle(&mut simulation, VIEWER_UNITS);

    commands.spawn((
        Camera2d,
        Transform::from_xyz(world_to_render_x(60 * SUBUNITS_PER_WORLD_UNIT), 0.0, 0.0),
    ));
    commands.insert_resource(SimState {
        simulation,
        presented: HashMap::new(),
    });
}

fn step_simulation(mut state: ResMut<SimState>) {
    state.simulation.step();
}

fn sync_units(
    mut commands: Commands,
    mut state: ResMut<SimState>,
    mut transforms: Query<&mut Transform, With<PresentedUnit>>,
) {
    let units = state.simulation.units();
    let live_ids: HashSet<_> = units.iter().map(|unit| unit.id).collect();

    state.presented.retain(|id, entity| {
        if live_ids.contains(id) {
            true
        } else {
            commands.entity(*entity).despawn();
            false
        }
    });

    for unit in units {
        let position = Vec3::new(
            world_to_render_x(unit.position.x),
            world_to_render_x(unit.position.y),
            0.0,
        );

        if let Some(&entity) = state.presented.get(&unit.id) {
            if let Ok(mut transform) = transforms.get_mut(entity) {
                transform.translation = position;
            }
            continue;
        }

        let color = team_color(unit.team);
        let entity = commands
            .spawn((
                Sprite::from_color(color, Vec2::splat(5.0)),
                Transform::from_translation(position),
                PresentedUnit,
            ))
            .id();
        state.presented.insert(unit.id, entity);
    }
}

fn draw_targets(mut gizmos: Gizmos, state: Res<SimState>) {
    let units = state.simulation.units();
    let positions: HashMap<_, _> = units
        .iter()
        .map(|unit| {
            (
                unit.id,
                Vec2::new(
                    world_to_render_x(unit.position.x),
                    world_to_render_x(unit.position.y),
                ),
            )
        })
        .collect();

    for unit in units {
        let Some(target) = unit.target else {
            continue;
        };
        let Some(&target_position) = positions.get(&target) else {
            continue;
        };
        let Some(&source_position) = positions.get(&unit.id) else {
            continue;
        };

        gizmos.line_2d(
            source_position,
            target_position,
            Color::srgba(1.0, 1.0, 1.0, 0.12),
        );
    }
}

fn team_color(team: Team) -> Color {
    match team.0 {
        0 => Color::srgb(0.20, 0.55, 1.0),
        1 => Color::srgb(1.0, 0.30, 0.25),
        _ => Color::WHITE,
    }
}

fn world_to_render_x(value: i32) -> f32 {
    value as f32 / SUBUNITS_PER_WORLD_UNIT as f32 * PIXELS_PER_WORLD_UNIT
}

fn default_worker_count() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(8)
}
