use std::{
    collections::{HashMap, HashSet},
    env,
};

use bevy::{prelude::*, time::Fixed};
use castle_fight_sim::{
    BuildingFootprint, SUBUNITS_PER_WORLD_UNIT, SimId, Simulation, SimulationConfig, Team,
    populate_dense_cage_battle, populate_lane_battle,
};

const PIXELS_PER_WORLD_UNIT: f32 = 10.0;
const SIMULATION_HZ: f64 = 30.0;
const VIEWER_UNITS: usize = 1_000;

#[derive(Debug, Clone, Copy)]
enum ViewerScenario {
    Lane,
    Cage,
}

#[derive(Resource)]
struct SimState {
    simulation: Simulation,
    presented_units: HashMap<SimId, Entity>,
    presented_buildings: HashMap<SimId, Entity>,
}

#[derive(Component)]
struct PresentedUnit;

#[derive(Component)]
struct PresentedBuilding;

fn main() {
    let scenario = parse_scenario();
    App::new()
        .insert_resource(Time::<Fixed>::from_hz(SIMULATION_HZ))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: format!(
                    "Castle Fight Native — verification viewer ({})",
                    match scenario {
                        ViewerScenario::Lane => "lane",
                        ViewerScenario::Cage => "cage",
                    }
                ),
                resolution: (1280, 720).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(InitialScenario(scenario))
        .add_systems(Startup, setup)
        .add_systems(FixedUpdate, step_simulation)
        .add_systems(Update, (sync_units, sync_buildings, draw_targets))
        .run();
}

#[derive(Resource)]
struct InitialScenario(ViewerScenario);

fn setup(mut commands: Commands, scenario: Res<InitialScenario>) {
    let mut simulation = Simulation::new(SimulationConfig::default(), default_worker_count());
    match scenario.0 {
        ViewerScenario::Lane => populate_lane_battle(&mut simulation, VIEWER_UNITS),
        ViewerScenario::Cage => populate_dense_cage_battle(&mut simulation, VIEWER_UNITS),
    }

    commands.spawn((
        Camera2d,
        Transform::from_xyz(world_to_render(60 * SUBUNITS_PER_WORLD_UNIT), 0.0, 0.0),
    ));
    commands.insert_resource(SimState {
        simulation,
        presented_units: HashMap::new(),
        presented_buildings: HashMap::new(),
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

    state.presented_units.retain(|id, entity| {
        if live_ids.contains(id) {
            true
        } else {
            commands.entity(*entity).despawn();
            false
        }
    });

    for unit in units {
        let position = Vec3::new(
            world_to_render(unit.position.x),
            world_to_render(unit.position.y),
            1.0,
        );

        if let Some(&entity) = state.presented_units.get(&unit.id) {
            if let Ok(mut transform) = transforms.get_mut(entity) {
                transform.translation = position;
            }
            continue;
        }

        let entity = commands
            .spawn((
                Sprite::from_color(team_color(unit.team), Vec2::splat(5.0)),
                Transform::from_translation(position),
                PresentedUnit,
            ))
            .id();
        state.presented_units.insert(unit.id, entity);
    }
}

fn sync_buildings(
    mut commands: Commands,
    mut state: ResMut<SimState>,
    mut transforms: Query<&mut Transform, With<PresentedBuilding>>,
    mut sprites: Query<&mut Sprite, With<PresentedBuilding>>,
) {
    let buildings = state.simulation.buildings();
    let live_ids: HashSet<_> = buildings.iter().map(|building| building.id).collect();

    state.presented_buildings.retain(|id, entity| {
        if live_ids.contains(id) {
            true
        } else {
            commands.entity(*entity).despawn();
            false
        }
    });

    for building in buildings {
        let (position, size) = footprint_render_geometry(building.footprint);
        if let Some(&entity) = state.presented_buildings.get(&building.id) {
            if let Ok(mut transform) = transforms.get_mut(entity) {
                transform.translation = position;
            }
            if let Ok(mut sprite) = sprites.get_mut(entity) {
                sprite.custom_size = Some(size);
            }
            continue;
        }

        let entity = commands
            .spawn((
                Sprite::from_color(building_color(building.team), size),
                Transform::from_translation(position),
                PresentedBuilding,
            ))
            .id();
        state.presented_buildings.insert(building.id, entity);
    }
}

fn draw_targets(mut gizmos: Gizmos, state: Res<SimState>) {
    let units = state.simulation.units();
    let buildings = state.simulation.buildings();
    let mut positions = HashMap::with_capacity(units.len() + buildings.len());

    for unit in &units {
        positions.insert(
            unit.id,
            Vec2::new(
                world_to_render(unit.position.x),
                world_to_render(unit.position.y),
            ),
        );
    }
    for building in buildings {
        let (position, _) = footprint_render_geometry(building.footprint);
        positions.insert(building.id, position.truncate());
    }

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
            Color::srgba(1.0, 1.0, 1.0, 0.10),
        );
    }
}

fn footprint_render_geometry(footprint: BuildingFootprint) -> (Vec3, Vec2) {
    let width = f32::from(footprint.width) * PIXELS_PER_WORLD_UNIT;
    let height = f32::from(footprint.height) * PIXELS_PER_WORLD_UNIT;
    let center_x =
        (footprint.min_x as f32 + f32::from(footprint.width) / 2.0) * PIXELS_PER_WORLD_UNIT;
    let center_y =
        (footprint.min_y as f32 + f32::from(footprint.height) / 2.0) * PIXELS_PER_WORLD_UNIT;
    (Vec3::new(center_x, center_y, 0.0), Vec2::new(width, height))
}

fn team_color(team: Team) -> Color {
    match team.0 {
        0 => Color::srgb(0.20, 0.55, 1.0),
        1 => Color::srgb(1.0, 0.30, 0.25),
        _ => Color::WHITE,
    }
}

fn building_color(team: Team) -> Color {
    match team.0 {
        0 => Color::srgb(0.08, 0.25, 0.55),
        1 => Color::srgb(0.55, 0.10, 0.08),
        _ => Color::srgb(0.35, 0.35, 0.35),
    }
}

fn world_to_render(value: i32) -> f32 {
    value as f32 / SUBUNITS_PER_WORLD_UNIT as f32 * PIXELS_PER_WORLD_UNIT
}

fn default_worker_count() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(8)
}

fn parse_scenario() -> ViewerScenario {
    let mut args = env::args().skip(1);
    let Some(flag) = args.next() else {
        return ViewerScenario::Cage;
    };
    match (flag.as_str(), args.next().as_deref()) {
        ("--scenario", Some("lane")) => ViewerScenario::Lane,
        ("--scenario", Some("cage")) => ViewerScenario::Cage,
        ("-h" | "--help", _) => {
            println!("Usage: cargo run -p castle-fight-debug-viewer -- [--scenario lane|cage]");
            std::process::exit(0);
        }
        _ => panic!("expected --scenario lane|cage"),
    }
}
