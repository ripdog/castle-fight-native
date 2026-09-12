use std::collections::{HashMap, HashSet};

use bevy::{camera::ScalingMode, prelude::*, time::Fixed, window::PrimaryWindow};
use castle_fight_sim::{
    AttackDelivery, BuildingFootprint, BuildingSpawn, BuildingView, MovementProfile, NavCell,
    ProductionProfile, SUBUNITS_PER_WORLD_UNIT, SimId, SimPoint, Simulation, SimulationConfig,
    Team, UnitTemplate, UnitView,
};

const SIMULATION_HZ: f64 = 30.0;
const SIMULATION_HZ_I32: i32 = 30;
const MAP_WIDTH: f32 = 2_000.0;
const MAP_HEIGHT: f32 = 750.0;
const NAV_CELL_WORLD: i32 = 10;
const NAV_CELL_SUBUNITS: i32 = NAV_CELL_WORLD * SUBUNITS_PER_WORLD_UNIT;
const NAV_MAX_X: i32 = 199;
const NAV_MAX_Y: i32 = 74;
const PLAYER_BASE_MAX_X: i32 = 66;
const MIDDLE_MIN_X: i32 = 67;
const MIDDLE_MAX_X: i32 = 132;
const LANE_MIN_Y: i32 = 20;
const LANE_MAX_Y: i32 = 54;
const PRODUCTION_BUILDING_SIZE: u16 = 4;
const PRODUCTION_INTERVAL_TICKS: u16 = 300;
const ATTACK_COOLDOWN_TICKS: u16 = 30;
const ATTACK_TRACE_SECONDS: f32 = 0.18;
const PLAYER_CASTLE: BuildingFootprint = BuildingFootprint::new(30, 34, 7, 7);
const ENEMY_CASTLE: BuildingFootprint = BuildingFootprint::new(163, 34, 7, 7);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProductionKind {
    Melee,
    Ranged,
}

impl ProductionKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Melee => "melee",
            Self::Ranged => "ranged",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PendingPlacement {
    kind: ProductionKind,
    footprint: BuildingFootprint,
}

#[derive(Resource, Default)]
struct PendingPlacements(Vec<PendingPlacement>);

#[derive(Resource)]
struct UiStatus {
    text: String,
}

impl Default for UiStatus {
    fn default() -> Self {
        Self {
            text: "LMB: melee building • RMB: ranged building • placements mirror automatically"
                .into(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct AttackTrace {
    start: Vec2,
    end: Vec2,
    delivery: AttackDelivery,
    remaining: f32,
}

#[derive(Resource, Default)]
struct AttackVisuals(Vec<AttackTrace>);

#[derive(Resource)]
struct GameState {
    simulation: Simulation,
    presented_units: HashMap<SimId, Entity>,
    presented_buildings: HashMap<SimId, Entity>,
    player_castle: SimId,
    enemy_castle: SimId,
}

#[derive(Component)]
struct VerificationCamera;

#[derive(Component)]
struct PresentedUnit;

#[derive(Component)]
struct PresentedBuilding;

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.025, 0.03, 0.04)))
        .insert_resource(Time::<Fixed>::from_hz(SIMULATION_HZ))
        .init_resource::<PendingPlacements>()
        .init_resource::<AttackVisuals>()
        .init_resource::<UiStatus>()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Castle Fight Native — playable verification".into(),
                resolution: (1440, 720).into(),
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup)
        .add_systems(FixedUpdate, apply_placements_and_step)
        .add_systems(
            Update,
            (
                queue_build_input,
                sync_presentation,
                draw_cursor_preview,
                update_window_title,
            ),
        )
        .add_systems(Update, age_attack_traces.before(draw_attack_traces))
        .add_systems(Update, draw_attack_traces)
        .run();
}

fn setup(mut commands: Commands) {
    spawn_map_visuals(&mut commands);
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 1_020.0,
            },
            ..OrthographicProjection::default_2d()
        }),
        Transform::from_xyz(MAP_WIDTH / 2.0, MAP_HEIGHT / 2.0, 0.0),
        VerificationCamera,
    ));

    let mut simulation = Simulation::new(verification_config(), default_worker_count());
    let player_castle = simulation.spawn_building(BuildingSpawn {
        team: Team(0),
        footprint: PLAYER_CASTLE,
        health: 500,
        production: None,
    });
    let enemy_castle = simulation.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: ENEMY_CASTLE,
        health: 500,
        production: None,
    });

    commands.insert_resource(GameState {
        simulation,
        presented_units: HashMap::new(),
        presented_buildings: HashMap::new(),
        player_castle,
        enemy_castle,
    });
}

fn verification_config() -> SimulationConfig {
    SimulationConfig {
        spatial_cell_size: 40 * SUBUNITS_PER_WORLD_UNIT,
        navigation_cell_size: NAV_CELL_SUBUNITS,
        navigation_min: NavCell::new(0, 0),
        navigation_max: NavCell::new(NAV_MAX_X, NAV_MAX_Y),
        target_pursuit_extra_range: 30 * SUBUNITS_PER_WORLD_UNIT,
        unit_separation_distance: 8 * SUBUNITS_PER_WORLD_UNIT,
        max_separation_per_tick: SUBUNITS_PER_WORLD_UNIT,
        static_blockers: vec![
            BuildingFootprint::new(
                MIDDLE_MIN_X,
                0,
                (MIDDLE_MAX_X - MIDDLE_MIN_X + 1) as u16,
                LANE_MIN_Y as u16,
            ),
            BuildingFootprint::new(
                MIDDLE_MIN_X,
                LANE_MAX_Y + 1,
                (MIDDLE_MAX_X - MIDDLE_MIN_X + 1) as u16,
                (NAV_MAX_Y - LANE_MAX_Y) as u16,
            ),
        ],
        // These are approach cells immediately outside the opposing castle footprints.
        team_objective: [cell_center(162, 37), cell_center(37, 37)],
    }
}

fn spawn_map_visuals(commands: &mut Commands) {
    spawn_rect(
        commands,
        Vec2::new(MAP_WIDTH / 2.0, MAP_HEIGHT / 2.0),
        Vec2::new(MAP_WIDTH, MAP_HEIGHT),
        Color::srgb(0.10, 0.11, 0.12),
        -20.0,
    );

    spawn_rect(
        commands,
        Vec2::new(335.0, 375.0),
        Vec2::new(670.0, 750.0),
        Color::srgb(0.075, 0.11, 0.16),
        -19.0,
    );
    spawn_rect(
        commands,
        Vec2::new(1_665.0, 375.0),
        Vec2::new(670.0, 750.0),
        Color::srgb(0.16, 0.075, 0.075),
        -19.0,
    );
    spawn_rect(
        commands,
        Vec2::new(1_000.0, 375.0),
        Vec2::new(660.0, 350.0),
        Color::srgb(0.16, 0.16, 0.145),
        -18.0,
    );
    spawn_rect(
        commands,
        Vec2::new(1_000.0, 100.0),
        Vec2::new(660.0, 200.0),
        Color::srgb(0.035, 0.04, 0.045),
        -18.0,
    );
    spawn_rect(
        commands,
        Vec2::new(1_000.0, 650.0),
        Vec2::new(660.0, 200.0),
        Color::srgb(0.035, 0.04, 0.045),
        -18.0,
    );

    let border = Color::srgb(0.35, 0.37, 0.39);
    spawn_rect(
        commands,
        Vec2::new(1_000.0, 0.0),
        Vec2::new(2_004.0, 4.0),
        border,
        -10.0,
    );
    spawn_rect(
        commands,
        Vec2::new(1_000.0, 750.0),
        Vec2::new(2_004.0, 4.0),
        border,
        -10.0,
    );
    spawn_rect(
        commands,
        Vec2::new(0.0, 375.0),
        Vec2::new(4.0, 754.0),
        border,
        -10.0,
    );
    spawn_rect(
        commands,
        Vec2::new(2_000.0, 375.0),
        Vec2::new(4.0, 754.0),
        border,
        -10.0,
    );

    let division = Color::srgba(0.8, 0.82, 0.85, 0.25);
    spawn_rect(
        commands,
        Vec2::new(670.0, 375.0),
        Vec2::new(2.0, 750.0),
        division,
        -9.0,
    );
    spawn_rect(
        commands,
        Vec2::new(1_330.0, 375.0),
        Vec2::new(2.0, 750.0),
        division,
        -9.0,
    );
}

fn spawn_rect(commands: &mut Commands, center: Vec2, size: Vec2, color: Color, z: f32) {
    commands.spawn((
        Sprite::from_color(color, size),
        Transform::from_xyz(center.x, center.y, z),
    ));
}

fn queue_build_input(
    buttons: Res<ButtonInput<MouseButton>>,
    camera: Single<(&Camera, &GlobalTransform), With<VerificationCamera>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut pending: ResMut<PendingPlacements>,
    mut status: ResMut<UiStatus>,
) {
    let kind = if buttons.just_pressed(MouseButton::Left) {
        Some(ProductionKind::Melee)
    } else if buttons.just_pressed(MouseButton::Right) {
        Some(ProductionKind::Ranged)
    } else {
        None
    };
    let Some(kind) = kind else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let (camera, camera_transform) = *camera;
    let Ok(world) = camera.viewport_to_world_2d(camera_transform, cursor) else {
        return;
    };
    let Some(footprint) = player_footprint_at(world) else {
        status.text = "Placement rejected: click inside the blue player base".into();
        return;
    };

    pending.0.push(PendingPlacement { kind, footprint });
    status.text = format!("Queued mirrored {} production building", kind.label());
}

fn apply_placements_and_step(
    mut state: ResMut<GameState>,
    mut pending: ResMut<PendingPlacements>,
    mut status: ResMut<UiStatus>,
    mut attacks: ResMut<AttackVisuals>,
) {
    for placement in pending.0.drain(..) {
        let enemy_footprint = mirror_footprint(placement.footprint);
        if !state.simulation.can_place_building(placement.footprint)
            || !state.simulation.can_place_building(enemy_footprint)
        {
            status.text = "Placement rejected: footprint is occupied or blocked".into();
            continue;
        }

        state
            .simulation
            .try_spawn_building(production_building(
                Team(0),
                placement.footprint,
                placement.kind,
            ))
            .expect("prevalidated player placement unexpectedly failed");
        state
            .simulation
            .try_spawn_building(production_building(
                Team(1),
                enemy_footprint,
                placement.kind,
            ))
            .expect("mirrored enemy placement unexpectedly failed");
        status.text = format!(
            "Placed mirrored {} production buildings",
            placement.kind.label()
        );
    }

    state.simulation.step();
    attacks.0.extend(
        state
            .simulation
            .attacks_last_tick()
            .iter()
            .copied()
            .map(|event| AttackTrace {
                start: sim_point_to_world(event.source_position),
                end: sim_point_to_world(event.target_position),
                delivery: event.delivery,
                remaining: ATTACK_TRACE_SECONDS,
            }),
    );
}

fn production_building(
    team: Team,
    footprint: BuildingFootprint,
    kind: ProductionKind,
) -> BuildingSpawn {
    BuildingSpawn {
        team,
        footprint,
        health: 100,
        production: Some(ProductionProfile {
            initial_delay_ticks: PRODUCTION_INTERVAL_TICKS,
            interval_ticks: PRODUCTION_INTERVAL_TICKS,
            search_radius_cells: 12,
            unit: unit_template(kind),
        }),
    }
}

fn unit_template(kind: ProductionKind) -> UnitTemplate {
    let speed_per_tick = 40 * SUBUNITS_PER_WORLD_UNIT / SIMULATION_HZ_I32;
    match kind {
        ProductionKind::Melee => UnitTemplate {
            health: 10,
            attack: castle_fight_sim::AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 1,
                range: 14 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 80 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: ATTACK_COOLDOWN_TICKS,
            },
            movement: MovementProfile { speed_per_tick },
        },
        ProductionKind::Ranged => UnitTemplate {
            health: 10,
            attack: castle_fight_sim::AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit,
                damage: 1,
                range: 120 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 180 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: ATTACK_COOLDOWN_TICKS,
            },
            movement: MovementProfile { speed_per_tick },
        },
    }
}

fn sync_presentation(
    mut commands: Commands,
    mut state: ResMut<GameState>,
    mut unit_transforms: Query<&mut Transform, (With<PresentedUnit>, Without<PresentedBuilding>)>,
    mut building_transforms: Query<
        &mut Transform,
        (With<PresentedBuilding>, Without<PresentedUnit>),
    >,
) {
    let units = state.simulation.units();
    let live_units: HashSet<_> = units.iter().map(|unit| unit.id).collect();
    let stale_units: Vec<_> = state
        .presented_units
        .iter()
        .filter(|(id, _)| !live_units.contains(id))
        .map(|(id, entity)| (*id, *entity))
        .collect();
    for (id, entity) in stale_units {
        commands.entity(entity).despawn();
        state.presented_units.remove(&id);
    }

    for unit in units {
        let position = sim_point_to_world(unit.position).extend(5.0);
        if let Some(&entity) = state.presented_units.get(&unit.id) {
            if let Ok(mut transform) = unit_transforms.get_mut(entity) {
                transform.translation = position;
            }
            continue;
        }

        let entity = commands
            .spawn((
                Sprite::from_color(unit_color(&unit), unit_size(&unit)),
                Transform::from_translation(position),
                PresentedUnit,
            ))
            .id();
        state.presented_units.insert(unit.id, entity);
    }

    let buildings = state.simulation.buildings();
    let live_buildings: HashSet<_> = buildings.iter().map(|building| building.id).collect();
    let stale_buildings: Vec<_> = state
        .presented_buildings
        .iter()
        .filter(|(id, _)| !live_buildings.contains(id))
        .map(|(id, entity)| (*id, *entity))
        .collect();
    for (id, entity) in stale_buildings {
        commands.entity(entity).despawn();
        state.presented_buildings.remove(&id);
    }

    for building in buildings {
        let (center, size) = footprint_world_rect(building.footprint);
        let position = center.extend(3.0);
        if let Some(&entity) = state.presented_buildings.get(&building.id) {
            if let Ok(mut transform) = building_transforms.get_mut(entity) {
                transform.translation = position;
            }
            continue;
        }

        let entity = commands
            .spawn((
                Sprite::from_color(building_color(&building), size - Vec2::splat(2.0)),
                Transform::from_translation(position),
                PresentedBuilding,
            ))
            .id();
        state.presented_buildings.insert(building.id, entity);
    }
}

fn age_attack_traces(time: Res<Time>, mut attacks: ResMut<AttackVisuals>) {
    let elapsed = time.delta_secs();
    for trace in &mut attacks.0 {
        trace.remaining -= elapsed;
    }
    attacks.0.retain(|trace| trace.remaining > 0.0);
}

fn draw_attack_traces(mut gizmos: Gizmos, attacks: Res<AttackVisuals>) {
    for trace in &attacks.0 {
        let color = match trace.delivery {
            AttackDelivery::Melee => Color::srgba(1.0, 0.92, 0.62, 0.9),
            AttackDelivery::RangedGuaranteedHit => Color::srgba(0.72, 0.95, 1.0, 0.95),
        };
        gizmos.line_2d(trace.start, trace.end, color);
    }
}

fn draw_cursor_preview(
    camera: Single<(&Camera, &GlobalTransform), With<VerificationCamera>>,
    window: Single<&Window, With<PrimaryWindow>>,
    state: Res<GameState>,
    mut gizmos: Gizmos,
) {
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let (camera, camera_transform) = *camera;
    let Ok(world) = camera.viewport_to_world_2d(camera_transform, cursor) else {
        return;
    };
    let Some(player) = player_footprint_at(world) else {
        return;
    };
    let enemy = mirror_footprint(player);
    let valid =
        state.simulation.can_place_building(player) && state.simulation.can_place_building(enemy);
    let color = if valid {
        Color::srgba(0.55, 1.0, 0.65, 0.9)
    } else {
        Color::srgba(1.0, 0.35, 0.35, 0.9)
    };
    draw_footprint_outline(&mut gizmos, player, color);
    draw_footprint_outline(&mut gizmos, enemy, color.with_alpha(0.45));
}

fn draw_footprint_outline(gizmos: &mut Gizmos, footprint: BuildingFootprint, color: Color) {
    let (center, size) = footprint_world_rect(footprint);
    let half = size / 2.0;
    let min = center - half;
    let max = center + half;
    gizmos.line_2d(Vec2::new(min.x, min.y), Vec2::new(max.x, min.y), color);
    gizmos.line_2d(Vec2::new(max.x, min.y), Vec2::new(max.x, max.y), color);
    gizmos.line_2d(Vec2::new(max.x, max.y), Vec2::new(min.x, max.y), color);
    gizmos.line_2d(Vec2::new(min.x, max.y), Vec2::new(min.x, min.y), color);
}

fn update_window_title(
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    state: Res<GameState>,
    status: Res<UiStatus>,
) {
    let seconds = state.simulation.tick() as f64 / SIMULATION_HZ;
    let player_castle_hp = state
        .simulation
        .building(state.player_castle)
        .map_or(0, |castle| castle.health);
    let enemy_castle_hp = state
        .simulation
        .building(state.enemy_castle)
        .map_or(0, |castle| castle.health);
    window.title = format!(
        "Castle Fight verification | LMB melee • RMB ranged | t={seconds:.1}s • units={} • buildings={} • castles {player_castle_hp}/{enemy_castle_hp} | {}",
        state.simulation.unit_count(),
        state.simulation.building_count(),
        status.text,
    );
}

fn player_footprint_at(world: Vec2) -> Option<BuildingFootprint> {
    if !(0.0..MAP_WIDTH).contains(&world.x) || !(0.0..MAP_HEIGHT).contains(&world.y) {
        return None;
    }
    let cell_x = (world.x / NAV_CELL_WORLD as f32).floor() as i32;
    let cell_y = (world.y / NAV_CELL_WORLD as f32).floor() as i32;
    let footprint = BuildingFootprint::new(
        cell_x - i32::from(PRODUCTION_BUILDING_SIZE / 2),
        cell_y - i32::from(PRODUCTION_BUILDING_SIZE / 2),
        PRODUCTION_BUILDING_SIZE,
        PRODUCTION_BUILDING_SIZE,
    );
    (footprint.min_x >= 0
        && footprint.max_x() <= PLAYER_BASE_MAX_X
        && footprint.min_y >= 0
        && footprint.max_y() <= NAV_MAX_Y)
        .then_some(footprint)
}

fn mirror_footprint(footprint: BuildingFootprint) -> BuildingFootprint {
    BuildingFootprint::new(
        NAV_MAX_X - footprint.max_x(),
        footprint.min_y,
        footprint.width,
        footprint.height,
    )
}

fn cell_center(x: i32, y: i32) -> SimPoint {
    SimPoint::new(
        x * NAV_CELL_SUBUNITS + NAV_CELL_SUBUNITS / 2,
        y * NAV_CELL_SUBUNITS + NAV_CELL_SUBUNITS / 2,
    )
}

fn sim_point_to_world(point: SimPoint) -> Vec2 {
    Vec2::new(
        point.x as f32 / SUBUNITS_PER_WORLD_UNIT as f32,
        point.y as f32 / SUBUNITS_PER_WORLD_UNIT as f32,
    )
}

fn footprint_world_rect(footprint: BuildingFootprint) -> (Vec2, Vec2) {
    let width = f32::from(footprint.width) * NAV_CELL_WORLD as f32;
    let height = f32::from(footprint.height) * NAV_CELL_WORLD as f32;
    let min_x = footprint.min_x as f32 * NAV_CELL_WORLD as f32;
    let min_y = footprint.min_y as f32 * NAV_CELL_WORLD as f32;
    (
        Vec2::new(min_x + width / 2.0, min_y + height / 2.0),
        Vec2::new(width, height),
    )
}

fn unit_color(unit: &UnitView) -> Color {
    match (unit.team.0, unit.attack_delivery) {
        (0, AttackDelivery::Melee) => Color::srgb(0.22, 0.55, 1.0),
        (0, AttackDelivery::RangedGuaranteedHit) => Color::srgb(0.42, 0.88, 1.0),
        (1, AttackDelivery::Melee) => Color::srgb(1.0, 0.30, 0.26),
        (1, AttackDelivery::RangedGuaranteedHit) => Color::srgb(1.0, 0.60, 0.32),
        _ => Color::WHITE,
    }
}

fn unit_size(unit: &UnitView) -> Vec2 {
    match unit.attack_delivery {
        AttackDelivery::Melee => Vec2::splat(9.0),
        AttackDelivery::RangedGuaranteedHit => Vec2::splat(7.0),
    }
}

fn building_color(building: &BuildingView) -> Color {
    match (
        building.team.0,
        building
            .production
            .map(|profile| profile.unit.attack.delivery),
    ) {
        (0, None) => Color::srgb(0.45, 0.70, 1.0),
        (1, None) => Color::srgb(1.0, 0.48, 0.42),
        (0, Some(AttackDelivery::Melee)) => Color::srgb(0.12, 0.36, 0.72),
        (0, Some(AttackDelivery::RangedGuaranteedHit)) => Color::srgb(0.20, 0.62, 0.78),
        (1, Some(AttackDelivery::Melee)) => Color::srgb(0.72, 0.18, 0.16),
        (1, Some(AttackDelivery::RangedGuaranteedHit)) => Color::srgb(0.82, 0.40, 0.16),
        _ => Color::srgb(0.6, 0.6, 0.6),
    }
}

fn default_worker_count() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_map_matches_requested_dimensions() {
        assert_eq!(NAV_MAX_X + 1, 200);
        assert_eq!(NAV_MAX_Y + 1, 75);
        assert_eq!((NAV_MAX_X + 1) * NAV_CELL_WORLD, 2_000);
        assert_eq!((NAV_MAX_Y + 1) * NAV_CELL_WORLD, 750);
        assert_eq!((LANE_MAX_Y - LANE_MIN_Y + 1) * NAV_CELL_WORLD, 350);
        assert_eq!(verification_config().static_blockers.len(), 2);
    }

    #[test]
    fn mirrored_placement_preserves_size_and_y() {
        let player = BuildingFootprint::new(12, 18, 4, 4);
        let enemy = mirror_footprint(player);
        assert_eq!(enemy, BuildingFootprint::new(184, 18, 4, 4));
        assert_eq!(mirror_footprint(enemy), player);
    }

    #[test]
    fn verification_units_are_ten_hp_and_one_dps() {
        for kind in [ProductionKind::Melee, ProductionKind::Ranged] {
            let unit = unit_template(kind);
            assert_eq!(unit.health, 10);
            assert_eq!(unit.attack.damage, 1);
            assert_eq!(unit.attack.cooldown_ticks, 30);
        }
    }

    #[test]
    fn verification_buildings_produce_every_ten_seconds() {
        let building = production_building(
            Team(0),
            BuildingFootprint::new(10, 10, 4, 4),
            ProductionKind::Melee,
        );
        let production = building
            .production
            .expect("production building missing profile");
        assert_eq!(production.initial_delay_ticks, 300);
        assert_eq!(production.interval_ticks, 300);
    }
}
