use std::collections::BTreeMap;

use bevy::{input::mouse::MouseWheel, prelude::*, time::Fixed, window::PrimaryWindow};
use castle_fight_sim::{
    BuildingFootprint, ProjectileView, ProjectileViewKind, SUBUNITS_PER_WORLD_UNIT, SimId,
    SimPoint, SimulationConfig, Team,
};

use crate::bridge::{
    BuildingSample, BuildingVisualKind, PresentationSamples, UnitSample, UnitVisualKind,
};

const UNIT_MELEE_HEIGHT: f32 = 10.0;
const UNIT_RANGED_HEIGHT: f32 = 8.0;
const BUILDING_HEIGHT: f32 = 18.0;
const STATIC_BLOCKER_HEIGHT: f32 = 5.0;
const PROJECTILE_HEIGHT: f32 = 6.0;
const BALLISTIC_ARC_HEIGHT: f32 = 34.0;
const DEATH_REMAINS_SECONDS: f32 = 0.7;
const UNIT_HEALTH_BAR_WIDTH: f32 = 20.0;
const HEALTH_BAR_DEPTH: f32 = 6.0;
const HEALTH_BAR_LAYERS: usize = 7;

#[derive(Resource, Debug, Clone)]
pub struct WorldMetrics {
    navigation_cell_size_subunits: i32,
    navigation_min: IVec2,
    navigation_max: IVec2,
    static_blockers: Vec<BuildingFootprint>,
}

impl WorldMetrics {
    #[must_use]
    pub fn from_simulation_config(config: &SimulationConfig) -> Self {
        Self {
            navigation_cell_size_subunits: config.navigation_cell_size,
            navigation_min: IVec2::new(config.navigation_min.x, config.navigation_min.y),
            navigation_max: IVec2::new(config.navigation_max.x, config.navigation_max.y),
            static_blockers: config.static_blockers.clone(),
        }
    }

    pub(crate) fn navigation_cell_world(&self) -> f32 {
        self.navigation_cell_size_subunits as f32 / SUBUNITS_PER_WORLD_UNIT as f32
    }

    fn world_min(&self) -> Vec2 {
        self.navigation_min.as_vec2() * self.navigation_cell_world()
    }

    fn world_max(&self) -> Vec2 {
        (self.navigation_max + IVec2::ONE).as_vec2() * self.navigation_cell_world()
    }

    fn world_size(&self) -> Vec2 {
        self.world_max() - self.world_min()
    }

    fn world_center(&self) -> Vec3 {
        let center = (self.world_min() + self.world_max()) * 0.5;
        Vec3::new(center.x, 0.0, center.y)
    }

    pub(crate) fn footprint_center_size(&self, footprint: BuildingFootprint) -> (Vec3, Vec2) {
        let cell = self.navigation_cell_world();
        let width = f32::from(footprint.width) * cell;
        let depth = f32::from(footprint.height) * cell;
        let min_x = footprint.min_x as f32 * cell;
        let min_z = footprint.min_y as f32 * cell;
        (
            Vec3::new(min_x + width * 0.5, 0.0, min_z + depth * 0.5),
            Vec2::new(width, depth),
        )
    }

    pub(crate) fn footprint_at_world(
        &self,
        world: Vec3,
        width: u16,
        height: u16,
    ) -> BuildingFootprint {
        let cell = self.navigation_cell_world();
        let cell_x = (world.x / cell).floor() as i32;
        let cell_y = (world.z / cell).floor() as i32;
        BuildingFootprint::new(
            cell_x - i32::from(width / 2),
            cell_y - i32::from(height / 2),
            width,
            height,
        )
    }
}

#[derive(Resource)]
struct PresentationAssets {
    melee_mesh: Handle<Mesh>,
    ranged_mesh: Handle<Mesh>,
    ballistic_unit_mesh: Handle<Mesh>,
    bounce_unit_mesh: Handle<Mesh>,
    building_mesh: Handle<Mesh>,
    projectile_mesh: Handle<Mesh>,
    unit_materials: [Handle<StandardMaterial>; 2],
    building_materials: [Handle<StandardMaterial>; 2],
    projectile_materials: [Handle<StandardMaterial>; 4],
    neutral_unit_material: Handle<StandardMaterial>,
    neutral_building_material: Handle<StandardMaterial>,
}

impl PresentationAssets {
    fn unit_mesh(&self, kind: UnitVisualKind) -> Handle<Mesh> {
        match kind {
            UnitVisualKind::Melee => self.melee_mesh.clone(),
            UnitVisualKind::Ranged => self.ranged_mesh.clone(),
            UnitVisualKind::Ballistic => self.ballistic_unit_mesh.clone(),
            UnitVisualKind::Bounce => self.bounce_unit_mesh.clone(),
        }
    }

    fn unit_material(&self, team: Team) -> Handle<StandardMaterial> {
        self.unit_materials
            .get(usize::from(team.0))
            .cloned()
            .unwrap_or_else(|| self.neutral_unit_material.clone())
    }

    fn building_material(&self, team: Team) -> Handle<StandardMaterial> {
        self.building_materials
            .get(usize::from(team.0))
            .cloned()
            .unwrap_or_else(|| self.neutral_building_material.clone())
    }

    fn projectile_material(&self, projectile: &ProjectileView) -> Handle<StandardMaterial> {
        let index = match projectile.kind {
            ProjectileViewKind::GuaranteedHit { .. } => 0,
            ProjectileViewKind::Ballistic { .. } => 1,
            ProjectileViewKind::Bounce {
                bounce_index: 0, ..
            } => 2,
            ProjectileViewKind::Bounce { .. } => 3,
        };
        self.projectile_materials[index].clone()
    }
}

#[derive(Debug, Clone, Copy)]
struct PresentedEntry {
    entity: Entity,
    max_health_seen: i32,
}

#[derive(Resource, Default)]
struct RenderMap {
    units: BTreeMap<SimId, PresentedEntry>,
    buildings: BTreeMap<SimId, PresentedEntry>,
    projectiles: BTreeMap<SimId, Entity>,
}

#[derive(Debug, Clone, Copy)]
struct DeathRemnant {
    position: Vec3,
    team: Team,
    building: bool,
    remaining: f32,
}

#[derive(Resource, Default)]
struct DeathRemnants(Vec<DeathRemnant>);

#[derive(Resource)]
struct DebugPresentation {
    overlays: bool,
    health_bars: bool,
}

impl Default for DebugPresentation {
    fn default() -> Self {
        Self {
            overlays: false,
            health_bars: true,
        }
    }
}

#[derive(Component)]
struct RtsCamera {
    focus: Vec3,
    distance: f32,
    yaw: f32,
    grab_anchor: Option<Vec3>,
}

pub struct CastlePresentationPlugin;

impl Plugin for CastlePresentationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RenderMap>()
            .init_resource::<DeathRemnants>()
            .init_resource::<DebugPresentation>()
            .add_systems(Startup, setup_scene)
            .add_systems(
                Update,
                (
                    toggle_debug_controls,
                    update_camera,
                    sync_render_entities,
                    interpolate_render_transforms,
                    age_death_remnants,
                    draw_presentation_gizmos,
                    update_window_title,
                )
                    .chain(),
            );
    }
}

fn setup_scene(
    mut commands: Commands,
    metrics: Res<WorldMetrics>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let melee_mesh = meshes.add(Cuboid::new(7.0, UNIT_MELEE_HEIGHT, 7.0));
    let ranged_mesh = meshes.add(Cuboid::new(6.0, UNIT_RANGED_HEIGHT, 6.0));
    let ballistic_unit_mesh = meshes.add(Cuboid::new(7.0, UNIT_RANGED_HEIGHT, 7.0));
    let bounce_unit_mesh = meshes.add(Cuboid::new(6.0, UNIT_RANGED_HEIGHT, 6.0));
    let building_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let projectile_mesh = meshes.add(Cuboid::new(3.0, 3.0, 3.0));

    let unit_materials = [
        materials.add(StandardMaterial {
            base_color: Color::srgb(0.18, 0.48, 0.95),
            perceptual_roughness: 0.72,
            ..default()
        }),
        materials.add(StandardMaterial {
            base_color: Color::srgb(0.90, 0.22, 0.18),
            perceptual_roughness: 0.72,
            ..default()
        }),
    ];
    let building_materials = [
        materials.add(StandardMaterial {
            base_color: Color::srgb(0.07, 0.20, 0.52),
            perceptual_roughness: 0.86,
            ..default()
        }),
        materials.add(StandardMaterial {
            base_color: Color::srgb(0.52, 0.08, 0.06),
            perceptual_roughness: 0.86,
            ..default()
        }),
    ];
    let projectile_materials = [
        materials.add(Color::srgb(0.55, 0.90, 1.0)),
        materials.add(Color::srgb(1.0, 0.62, 0.18)),
        materials.add(Color::srgb(0.65, 1.0, 0.35)),
        materials.add(Color::srgb(0.95, 1.0, 0.52)),
    ];
    let neutral_unit_material = materials.add(Color::srgb(0.72, 0.72, 0.74));
    let neutral_building_material = materials.add(Color::srgb(0.32, 0.32, 0.34));

    commands.insert_resource(PresentationAssets {
        melee_mesh,
        ranged_mesh,
        ballistic_unit_mesh,
        bounce_unit_mesh,
        building_mesh: building_mesh.clone(),
        projectile_mesh,
        unit_materials,
        building_materials,
        projectile_materials,
        neutral_unit_material,
        neutral_building_material,
    });

    let world_size = metrics.world_size();
    let world_center = metrics.world_center();
    let ground_mesh = meshes.add(Cuboid::new(world_size.x, 1.0, world_size.y));
    let ground_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.16, 0.18, 0.15),
        perceptual_roughness: 1.0,
        ..default()
    });
    commands.spawn((
        Mesh3d(ground_mesh),
        MeshMaterial3d(ground_material),
        Transform::from_xyz(world_center.x, -0.5, world_center.z),
    ));

    let blocker_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.085, 0.09, 0.095),
        perceptual_roughness: 0.94,
        ..default()
    });
    for blocker in &metrics.static_blockers {
        let (center, size) = metrics.footprint_center_size(*blocker);
        commands.spawn((
            Mesh3d(building_mesh.clone()),
            MeshMaterial3d(blocker_material.clone()),
            Transform {
                translation: Vec3::new(center.x, STATIC_BLOCKER_HEIGHT * 0.5, center.z),
                scale: Vec3::new(size.x, STATIC_BLOCKER_HEIGHT, size.y),
                ..default()
            },
        ));
    }

    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.85, -0.75, 0.0)),
    ));

    let distance = world_size.max_element() * 0.72;
    let rig = RtsCamera {
        focus: world_center,
        distance,
        yaw: 0.0,
        grab_anchor: None,
    };
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            far: 10_000.0,
            ..default()
        }),
        camera_transform(&rig),
        rig,
    ));
}

fn sync_render_entities(
    mut commands: Commands,
    samples: Res<PresentationSamples>,
    metrics: Res<WorldMetrics>,
    assets: Res<PresentationAssets>,
    mut render_map: ResMut<RenderMap>,
    mut remnants: ResMut<DeathRemnants>,
) {
    let stale_units: Vec<_> = render_map
        .units
        .keys()
        .copied()
        .filter(|id| !samples.current.units.contains_key(id))
        .collect();
    for id in stale_units {
        let entry = render_map
            .units
            .remove(&id)
            .expect("stale unit entry disappeared during presentation sync");
        commands.entity(entry.entity).despawn();
        if let Some(unit) = samples.previous.units.get(&id) {
            remnants.0.push(DeathRemnant {
                position: sim_point_to_world(unit.position),
                team: unit.team,
                building: false,
                remaining: DEATH_REMAINS_SECONDS,
            });
        }
    }

    let stale_buildings: Vec<_> = render_map
        .buildings
        .keys()
        .copied()
        .filter(|id| !samples.current.buildings.contains_key(id))
        .collect();
    for id in stale_buildings {
        let entry = render_map
            .buildings
            .remove(&id)
            .expect("stale building entry disappeared during presentation sync");
        commands.entity(entry.entity).despawn();
        if let Some(building) = samples.previous.buildings.get(&id) {
            let (center, _) = metrics.footprint_center_size(building.footprint);
            remnants.0.push(DeathRemnant {
                position: center,
                team: building.team,
                building: true,
                remaining: DEATH_REMAINS_SECONDS,
            });
        }
    }

    let stale_projectiles: Vec<_> = render_map
        .projectiles
        .keys()
        .copied()
        .filter(|id| !samples.current.projectiles.contains_key(id))
        .collect();
    for id in stale_projectiles {
        if let Some(entity) = render_map.projectiles.remove(&id) {
            commands.entity(entity).despawn();
        }
    }

    for unit in samples.current.units.values() {
        if let Some(entry) = render_map.units.get_mut(&unit.id) {
            entry.max_health_seen = entry.max_health_seen.max(unit.health);
            continue;
        }
        let position = sim_point_to_world(unit.position) + Vec3::Y * (unit_height(unit) * 0.5);
        let entity = commands
            .spawn((
                Mesh3d(assets.unit_mesh(unit.visual_kind)),
                MeshMaterial3d(assets.unit_material(unit.team)),
                Transform::from_translation(position),
            ))
            .id();
        render_map.units.insert(
            unit.id,
            PresentedEntry {
                entity,
                max_health_seen: unit.health.max(1),
            },
        );
    }

    for building in samples.current.buildings.values() {
        if let Some(entry) = render_map.buildings.get_mut(&building.id) {
            entry.max_health_seen = entry.max_health_seen.max(building.health);
            continue;
        }
        let (center, size) = metrics.footprint_center_size(building.footprint);
        let visual_height = building_height(building);
        let entity = commands
            .spawn((
                Mesh3d(assets.building_mesh.clone()),
                MeshMaterial3d(assets.building_material(building.team)),
                Transform {
                    translation: Vec3::new(center.x, visual_height * 0.5, center.z),
                    scale: Vec3::new(size.x * 0.92, visual_height, size.y * 0.92),
                    ..default()
                },
            ))
            .id();
        render_map.buildings.insert(
            building.id,
            PresentedEntry {
                entity,
                max_health_seen: building.health.max(1),
            },
        );
    }

    for projectile in samples.current.projectiles.values() {
        if render_map.projectiles.contains_key(&projectile.id) {
            continue;
        }
        let entity = commands
            .spawn((
                Mesh3d(assets.projectile_mesh.clone()),
                MeshMaterial3d(assets.projectile_material(projectile)),
                Transform::from_translation(sim_point_to_world(projectile.launch_position)),
            ))
            .id();
        render_map.projectiles.insert(projectile.id, entity);
    }
}

fn interpolate_render_transforms(
    fixed_time: Res<Time<Fixed>>,
    samples: Res<PresentationSamples>,
    metrics: Res<WorldMetrics>,
    render_map: Res<RenderMap>,
    mut transforms: Query<&mut Transform>,
) {
    let alpha = fixed_time.overstep_fraction();

    for (id, current) in &samples.current.units {
        let Some(entry) = render_map.units.get(id) else {
            continue;
        };
        let previous = samples.previous.units.get(id).unwrap_or(current);
        let position = sim_point_to_world_lerp(previous.position, current.position, alpha)
            + Vec3::Y * (unit_height(current) * 0.5);
        if let Ok(mut transform) = transforms.get_mut(entry.entity) {
            transform.translation = position;
        }
    }

    for (id, current) in &samples.current.buildings {
        let Some(entry) = render_map.buildings.get(id) else {
            continue;
        };
        let (center, _) = metrics.footprint_center_size(current.footprint);
        if let Ok(mut transform) = transforms.get_mut(entry.entity) {
            transform.translation = Vec3::new(center.x, building_height(current) * 0.5, center.z);
        }
    }

    let render_tick = samples.previous.tick as f32
        + (samples.current.tick.saturating_sub(samples.previous.tick) as f32) * alpha;
    for (id, projectile) in &samples.current.projectiles {
        let Some(&entity) = render_map.projectiles.get(id) else {
            continue;
        };
        let position = projectile_position(projectile, &samples, &metrics, alpha, render_tick);
        if let Ok(mut transform) = transforms.get_mut(entity) {
            transform.translation = position;
        }
    }
}

fn projectile_position(
    projectile: &ProjectileView,
    samples: &PresentationSamples,
    metrics: &WorldMetrics,
    alpha: f32,
    render_tick: f32,
) -> Vec3 {
    let start = sim_point_to_world(projectile.launch_position);
    let target = match projectile.kind {
        ProjectileViewKind::GuaranteedHit { target }
        | ProjectileViewKind::Bounce { target, .. } => {
            entity_render_position(target, samples, metrics, alpha).unwrap_or(start)
        }
        ProjectileViewKind::Ballistic { destination, .. } => sim_point_to_world(destination),
    };
    let travel_ticks = projectile
        .impact_tick
        .saturating_sub(projectile.launch_tick)
        .max(1) as f32;
    let progress = ((render_tick - projectile.launch_tick as f32) / travel_ticks).clamp(0.0, 1.0);
    let mut position = start.lerp(target, progress);
    position.y = PROJECTILE_HEIGHT;
    if matches!(projectile.kind, ProjectileViewKind::Ballistic { .. }) {
        position.y += BALLISTIC_ARC_HEIGHT * 4.0 * progress * (1.0 - progress);
    }
    position
}

fn entity_render_position(
    id: SimId,
    samples: &PresentationSamples,
    metrics: &WorldMetrics,
    alpha: f32,
) -> Option<Vec3> {
    if let Some(current) = samples.current.units.get(&id) {
        let previous = samples.previous.units.get(&id).unwrap_or(current);
        return Some(sim_point_to_world_lerp(
            previous.position,
            current.position,
            alpha,
        ));
    }
    samples.current.buildings.get(&id).map(|building| {
        let (center, _) = metrics.footprint_center_size(building.footprint);
        center
    })
}

fn age_death_remnants(time: Res<Time>, mut remnants: ResMut<DeathRemnants>) {
    let delta = time.delta_secs();
    for remnant in &mut remnants.0 {
        remnant.remaining -= delta;
    }
    remnants.0.retain(|remnant| remnant.remaining > 0.0);
}

fn draw_presentation_gizmos(
    fixed_time: Res<Time<Fixed>>,
    samples: Res<PresentationSamples>,
    metrics: Res<WorldMetrics>,
    render_map: Res<RenderMap>,
    debug: Res<DebugPresentation>,
    remnants: Res<DeathRemnants>,
    mut gizmos: Gizmos,
) {
    let alpha = fixed_time.overstep_fraction();

    if debug.health_bars {
        for (id, unit) in &samples.current.units {
            let Some(entry) = render_map.units.get(id) else {
                continue;
            };
            let previous = samples.previous.units.get(id).unwrap_or(unit);
            let position = sim_point_to_world_lerp(previous.position, unit.position, alpha)
                + Vec3::Y * (unit_height(unit) + 3.0);
            draw_health_bar(
                &mut gizmos,
                position,
                UNIT_HEALTH_BAR_WIDTH,
                unit.health,
                entry.max_health_seen,
                unit.team,
            );
        }
        for (id, building) in &samples.current.buildings {
            let Some(entry) = render_map.buildings.get(id) else {
                continue;
            };
            let (center, size) = metrics.footprint_center_size(building.footprint);
            let position = center + Vec3::Y * (building_height(building) + 4.0);
            draw_health_bar(
                &mut gizmos,
                position,
                size.x.clamp(24.0, 72.0),
                building.health,
                entry.max_health_seen,
                building.team,
            );
        }
    }

    for remnant in &remnants.0 {
        let life = (remnant.remaining / DEATH_REMAINS_SECONDS).clamp(0.0, 1.0);
        let radius = if remnant.building { 13.0 } else { 6.0 } * (0.65 + 0.35 * life);
        gizmos.circle(
            Isometry3d::new(
                remnant.position + Vec3::Y * 0.35,
                Quat::from_rotation_arc(Vec3::Z, Vec3::Y),
            ),
            radius,
            team_color(remnant.team).with_alpha(life),
        );
    }

    if !debug.overlays {
        return;
    }

    for unit in samples.current.units.values() {
        let previous = samples.previous.units.get(&unit.id).unwrap_or(unit);
        let rendered = sim_point_to_world_lerp(previous.position, unit.position, alpha);
        let authoritative = sim_point_to_world(unit.position);
        gizmos.line(
            rendered + Vec3::Y * 0.2,
            authoritative + Vec3::Y * 0.2,
            Color::srgb(1.0, 0.9, 0.15),
        );
        gizmos.line(
            authoritative,
            authoritative + Vec3::Y * 4.0,
            team_color(unit.team).with_alpha(0.8),
        );
        if let Some(target) = unit.target
            && let Some(target_position) = entity_render_position(target, &samples, &metrics, alpha)
        {
            gizmos.line(
                rendered + Vec3::Y * 2.0,
                target_position + Vec3::Y * 2.0,
                Color::srgba(1.0, 1.0, 1.0, 0.18),
            );
        }
    }

    for building in samples.current.buildings.values() {
        draw_footprint_outline(
            &mut gizmos,
            &metrics,
            building.footprint,
            team_color(building.team),
        );
        if let Some(target) = building.target
            && let Some(target_position) = entity_render_position(target, &samples, &metrics, alpha)
        {
            let (center, _) = metrics.footprint_center_size(building.footprint);
            gizmos.line(
                center + Vec3::Y * 3.0,
                target_position + Vec3::Y * 2.0,
                Color::srgba(1.0, 1.0, 1.0, 0.18),
            );
        }
    }
}

fn draw_health_bar(
    gizmos: &mut Gizmos,
    center: Vec3,
    width: f32,
    health: i32,
    max_health: i32,
    team: Team,
) {
    let ratio = (health.max(0) as f32 / max_health.max(1) as f32).clamp(0.0, 1.0);
    let half = width * 0.5;
    let start_x = center.x - half;
    let fill_end_x = start_x + width * ratio;
    let background = Color::srgb(0.085, 0.085, 0.095);
    let fill = team_color(team);

    for layer in 0..HEALTH_BAR_LAYERS {
        let t = layer as f32 / (HEALTH_BAR_LAYERS - 1) as f32;
        let z = center.z + (t - 0.5) * HEALTH_BAR_DEPTH;
        gizmos.line(
            Vec3::new(start_x - 0.6, center.y, z),
            Vec3::new(center.x + half + 0.6, center.y, z),
            background,
        );
        if ratio > 0.0 {
            gizmos.line(
                Vec3::new(start_x, center.y + 0.12, z),
                Vec3::new(fill_end_x, center.y + 0.12, z),
                fill,
            );
        }
    }
}

pub(crate) fn draw_footprint_outline(
    gizmos: &mut Gizmos,
    metrics: &WorldMetrics,
    footprint: BuildingFootprint,
    color: Color,
) {
    let (center, size) = metrics.footprint_center_size(footprint);
    let half = size * 0.5;
    let y = 0.15;
    let min = Vec3::new(center.x - half.x, y, center.z - half.y);
    let max = Vec3::new(center.x + half.x, y, center.z + half.y);
    gizmos.line(
        Vec3::new(min.x, y, min.z),
        Vec3::new(max.x, y, min.z),
        color,
    );
    gizmos.line(
        Vec3::new(max.x, y, min.z),
        Vec3::new(max.x, y, max.z),
        color,
    );
    gizmos.line(
        Vec3::new(max.x, y, max.z),
        Vec3::new(min.x, y, max.z),
        color,
    );
    gizmos.line(
        Vec3::new(min.x, y, max.z),
        Vec3::new(min.x, y, min.z),
        color,
    );
}

fn toggle_debug_controls(keys: Res<ButtonInput<KeyCode>>, mut debug: ResMut<DebugPresentation>) {
    if keys.just_pressed(KeyCode::F1) {
        debug.overlays = !debug.overlays;
    }
    if keys.just_pressed(KeyCode::KeyH) {
        debug.health_bars = !debug.health_bars;
    }
}

fn update_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut mouse_wheel: MessageReader<MouseWheel>,
    window: Single<&Window, With<PrimaryWindow>>,
    metrics: Res<WorldMetrics>,
    mut camera: Single<(&Camera, &mut RtsCamera, &mut Transform), With<Camera3d>>,
) {
    let (camera_component, rig, transform) = &mut *camera;

    if mouse_buttons.just_pressed(MouseButton::Middle)
        && let Some(cursor) = window.cursor_position()
    {
        let camera_global = GlobalTransform::from(**transform);
        rig.grab_anchor = viewport_ground_point(camera_component, &camera_global, cursor);
    }

    let dt = time.delta_secs();
    let forward = Vec3::new(-rig.yaw.sin(), 0.0, -rig.yaw.cos());
    let right = Vec3::new(rig.yaw.cos(), 0.0, -rig.yaw.sin());
    let mut movement = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        movement += forward;
    }
    if keys.pressed(KeyCode::KeyS) {
        movement -= forward;
    }
    if keys.pressed(KeyCode::KeyD) {
        movement += right;
    }
    if keys.pressed(KeyCode::KeyA) {
        movement -= right;
    }
    if movement != Vec3::ZERO {
        let pan_speed = rig.distance * 0.65;
        rig.focus += movement.normalize() * pan_speed * dt;
    }
    if keys.pressed(KeyCode::KeyQ) {
        rig.yaw += 0.9 * dt;
    }
    if keys.pressed(KeyCode::KeyE) {
        rig.yaw -= 0.9 * dt;
    }
    let scroll: f32 = mouse_wheel.read().map(|event| event.y).sum();
    if scroll != 0.0 {
        rig.distance *= (1.0 - scroll * 0.10).clamp(0.55, 1.45);
    }
    let world_size = metrics.world_size();
    rig.distance = rig.distance.clamp(
        world_size.min_element() * 0.28,
        world_size.max_element() * 1.7,
    );
    if keys.just_pressed(KeyCode::Home) {
        rig.focus = metrics.world_center();
        rig.distance = world_size.max_element() * 0.72;
        rig.yaw = 0.0;
    }

    if mouse_buttons.pressed(MouseButton::Middle)
        && let Some(anchor) = rig.grab_anchor
        && let Some(cursor) = window.cursor_position()
    {
        let proposed = camera_transform(rig);
        let proposed_global = GlobalTransform::from(proposed);
        if let Some(cursor_world) =
            viewport_ground_point(camera_component, &proposed_global, cursor)
        {
            let correction = anchor - cursor_world;
            rig.focus += Vec3::new(correction.x, 0.0, correction.z);
        }
    }

    if mouse_buttons.just_released(MouseButton::Middle) {
        rig.grab_anchor = None;
    }

    **transform = camera_transform(rig);
}

pub(crate) fn viewport_ground_point(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    cursor: Vec2,
) -> Option<Vec3> {
    let ray = camera.viewport_to_world(camera_transform, cursor).ok()?;
    let distance = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))?;
    Some(ray.origin + ray.direction.normalize() * distance)
}

fn camera_transform(rig: &RtsCamera) -> Transform {
    let horizontal = rig.distance * 0.68;
    let offset = Vec3::new(
        rig.yaw.sin() * horizontal,
        rig.distance * 0.72,
        rig.yaw.cos() * horizontal,
    );
    Transform::from_translation(rig.focus + offset).looking_at(rig.focus, Vec3::Y)
}

fn update_window_title(
    samples: Res<PresentationSamples>,
    debug: Res<DebugPresentation>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    window.title = format!(
        "Castle Fight Native 3D | tick {} | units {} | buildings {} | projectiles {} | F1 debug {} | H health {} | WASD pan • MMB grab • Q/E rotate • wheel zoom • Home reset",
        samples.current.tick,
        samples.current.units.len(),
        samples.current.buildings.len(),
        samples.current.projectiles.len(),
        if debug.overlays { "on" } else { "off" },
        if debug.health_bars { "on" } else { "off" },
    );
}

fn sim_point_to_world(point: SimPoint) -> Vec3 {
    Vec3::new(
        point.x as f32 / SUBUNITS_PER_WORLD_UNIT as f32,
        0.0,
        point.y as f32 / SUBUNITS_PER_WORLD_UNIT as f32,
    )
}

fn sim_point_to_world_lerp(previous: SimPoint, current: SimPoint, alpha: f32) -> Vec3 {
    sim_point_to_world(previous).lerp(sim_point_to_world(current), alpha)
}

fn unit_height(unit: &UnitSample) -> f32 {
    match unit.visual_kind {
        UnitVisualKind::Melee => UNIT_MELEE_HEIGHT,
        UnitVisualKind::Ranged | UnitVisualKind::Ballistic | UnitVisualKind::Bounce => {
            UNIT_RANGED_HEIGHT
        }
    }
}

fn building_height(building: &BuildingSample) -> f32 {
    match building.visual_kind {
        BuildingVisualKind::Structure => BUILDING_HEIGHT * 1.25,
        BuildingVisualKind::Production => BUILDING_HEIGHT,
        BuildingVisualKind::Attack => BUILDING_HEIGHT * 1.15,
        BuildingVisualKind::Spellcaster => BUILDING_HEIGHT * 1.10,
    }
}

fn team_color(team: Team) -> Color {
    match team.0 {
        0 => Color::srgb(0.20, 0.58, 1.0),
        1 => Color::srgb(1.0, 0.28, 0.22),
        _ => Color::srgb(0.78, 0.78, 0.80),
    }
}

#[cfg(test)]
mod tests {
    use castle_fight_sim::NavCell;

    use super::*;

    #[test]
    fn sim_subunits_map_one_to_one_to_render_world_units() {
        let point = SimPoint::new(7 * SUBUNITS_PER_WORLD_UNIT, -3 * SUBUNITS_PER_WORLD_UNIT);
        assert_eq!(sim_point_to_world(point), Vec3::new(7.0, 0.0, -3.0));
    }

    #[test]
    fn building_footprints_use_authoritative_navigation_cell_scale() {
        let config = SimulationConfig {
            navigation_cell_size: 10 * SUBUNITS_PER_WORLD_UNIT,
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(199, 74),
            ..SimulationConfig::default()
        };
        let metrics = WorldMetrics::from_simulation_config(&config);
        let (center, size) = metrics.footprint_center_size(BuildingFootprint::new(30, 34, 7, 7));

        assert_eq!(size, Vec2::new(70.0, 70.0));
        assert_eq!(center, Vec3::new(335.0, 0.0, 375.0));
    }
}
