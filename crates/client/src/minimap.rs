//! The minimap in the bottom-left console slot.
//!
//! The terrain is shaded once into a small RGBA texture; units, buildings, the camera footprint
//! and attack pings are drawn on a copy of it a few times a second. Pressing or dragging inside it
//! moves the camera. Everything is derived from the presentation snapshot, so it shows exactly
//! what the viewer is allowed to see (fog included).
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    ui::RelativeCursorPosition,
    window::PrimaryWindow,
};
use castle_fight_sim::Team;

use crate::{
    SelectedMatch,
    alerts::AlertPing,
    bridge::PresentationSamples,
    debug_menu::DebugMenuState,
    presentation::{WorldMetrics, sim_point_to_world, viewport_ground_point},
    terrain::TerrainSurface,
};

const TEXTURE_WIDTH: u32 = 256;
const MIN_TEXTURE_HEIGHT: u32 = 48;
const REFRESH_SECONDS: f32 = 0.2;
const ALLY: [u8; 4] = [96, 170, 255, 255];
const ENEMY: [u8; 4] = [255, 86, 74, 255];
const BUILDER: [u8; 4] = [255, 232, 96, 255];
const VIEW_OUTLINE: [u8; 4] = [245, 245, 245, 255];
const PING: [u8; 4] = [255, 214, 64, 255];

pub(crate) struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MinimapState>()
            .init_resource::<MinimapCameraTarget>()
            .add_systems(
                Update,
                (setup_minimap, draw_minimap, steer_camera_from_minimap).chain(),
            );
    }
}

/// The console slot the minimap lives in (spawned by the inspection UI).
#[derive(Component)]
pub(crate) struct MinimapPanel;

#[derive(Component)]
struct MinimapSurface;

/// World-plane point (x, z) the camera should centre on, set while the minimap is pressed.
#[derive(Resource, Default)]
pub(crate) struct MinimapCameraTarget(pub(crate) Option<Vec2>);

#[derive(Resource, Default)]
struct MinimapState {
    texture: Option<Handle<Image>>,
    /// The shaded terrain every refresh starts from.
    base: Vec<u8>,
    size: UVec2,
    world_min: Vec2,
    world_size: Vec2,
    since_refresh: f32,
}

impl MinimapState {
    /// Texture pixel for a world-plane position (x, z).
    fn pixel(&self, x: f32, z: f32) -> IVec2 {
        let u = (x - self.world_min.x) / self.world_size.x;
        let v = (z - self.world_min.y) / self.world_size.y;
        IVec2::new(
            (u * self.size.x as f32).floor() as i32,
            (v * self.size.y as f32).floor() as i32,
        )
    }
}

fn setup_minimap(
    mut commands: Commands,
    mut state: ResMut<MinimapState>,
    mut images: ResMut<Assets<Image>>,
    metrics: Res<WorldMetrics>,
    terrain: Res<TerrainSurface>,
    panel: Query<Entity, With<MinimapPanel>>,
) {
    if state.texture.is_some() {
        return;
    }
    let Ok(panel) = panel.single() else {
        return;
    };
    let world_min = metrics.world_min();
    let world_size = metrics.world_size();
    if world_size.x <= 0.0 || world_size.y <= 0.0 {
        return;
    }
    let height = ((TEXTURE_WIDTH as f32 * world_size.y / world_size.x).round() as u32)
        .max(MIN_TEXTURE_HEIGHT);
    let size = UVec2::new(TEXTURE_WIDTH, height);
    let base = shade_terrain(&terrain, world_min, world_size, size);
    let mut image = Image::new(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        base.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    let handle = images.add(image);
    commands.entity(panel).with_child((
        MinimapSurface,
        ImageNode::new(handle.clone()),
        Node {
            width: percent(100.0),
            aspect_ratio: Some(world_size.x / world_size.y),
            ..default()
        },
        Interaction::default(),
        RelativeCursorPosition::default(),
    ));
    *state = MinimapState {
        texture: Some(handle),
        base,
        size,
        world_min,
        world_size,
        since_refresh: REFRESH_SECONDS,
    };
}

/// Hill-shaded ground colour from the terrain heights.
fn shade_terrain(
    terrain: &TerrainSurface,
    world_min: Vec2,
    world_size: Vec2,
    size: UVec2,
) -> Vec<u8> {
    let sample = |u: f32, v: f32| {
        terrain.height_at_world(Vec2::new(
            world_min.x + u * world_size.x,
            world_min.y + v * world_size.y,
        ))
    };
    let (du, dv) = (1.0 / size.x as f32, 1.0 / size.y as f32);
    let heights = (0..size.y)
        .flat_map(|y| (0..size.x).map(move |x| (x, y)))
        .map(|(x, y)| sample((x as f32 + 0.5) * du, (y as f32 + 0.5) * dv))
        .collect::<Vec<_>>();
    let (low, high) = heights.iter().fold((f32::MAX, f32::MIN), |(low, high), h| {
        (low.min(*h), high.max(*h))
    });
    let range = (high - low).max(1.0);
    let at = |x: i32, y: i32| {
        let x = x.clamp(0, size.x as i32 - 1) as usize;
        let y = y.clamp(0, size.y as i32 - 1) as usize;
        heights[y * size.x as usize + x]
    };
    let mut pixels = Vec::with_capacity(heights.len() * 4);
    for y in 0..size.y as i32 {
        for x in 0..size.x as i32 {
            let height = (at(x, y) - low) / range;
            // Light from the upper left: slopes facing it are brighter.
            let slope = (at(x - 1, y - 1) - at(x + 1, y + 1)) / range;
            let shade = (1.0 + slope * 6.0).clamp(0.55, 1.45);
            let base = Vec3::new(0.19, 0.27, 0.16).lerp(Vec3::new(0.46, 0.50, 0.33), height);
            let color = (base * shade).clamp(Vec3::ZERO, Vec3::ONE);
            pixels.extend_from_slice(&[
                (color.x * 255.0) as u8,
                (color.y * 255.0) as u8,
                (color.z * 255.0) as u8,
                255,
            ]);
        }
    }
    pixels
}

fn put(pixels: &mut [u8], size: UVec2, at: IVec2, color: [u8; 4]) {
    if at.x < 0 || at.y < 0 || at.x >= size.x as i32 || at.y >= size.y as i32 {
        return;
    }
    let index = (at.y as usize * size.x as usize + at.x as usize) * 4;
    pixels[index..index + 4].copy_from_slice(&color);
}

fn fill(pixels: &mut [u8], size: UVec2, min: IVec2, max: IVec2, color: [u8; 4]) {
    for y in min.y..=max.y {
        for x in min.x..=max.x {
            put(pixels, size, IVec2::new(x, y), color);
        }
    }
}

/// Bresenham line, used for the camera footprint.
fn line(pixels: &mut [u8], size: UVec2, from: IVec2, to: IVec2, color: [u8; 4]) {
    let delta = (to - from).abs();
    let step = IVec2::new(
        if from.x < to.x { 1 } else { -1 },
        if from.y < to.y { 1 } else { -1 },
    );
    let mut error = delta.x - delta.y;
    let mut at = from;
    loop {
        put(pixels, size, at, color);
        if at == to {
            break;
        }
        let doubled = error * 2;
        if doubled > -delta.y {
            error -= delta.y;
            at.x += step.x;
        }
        if doubled < delta.x {
            error += delta.x;
            at.y += step.y;
        }
    }
}

/// Faded copy of a colour for buildings only remembered from earlier sight.
fn dimmed(color: [u8; 4]) -> [u8; 4] {
    [color[0] / 2, color[1] / 2, color[2] / 2, 255]
}

/// Everything the minimap reads from the rest of the client.
#[derive(bevy::ecs::system::SystemParam)]
struct MinimapInputs<'w> {
    samples: Res<'w, PresentationSamples>,
    selected: Res<'w, SelectedMatch>,
    debug_menu: Res<'w, DebugMenuState>,
    metrics: Res<'w, WorldMetrics>,
    terrain: Res<'w, TerrainSurface>,
    ping: Res<'w, AlertPing>,
}

fn draw_minimap(
    time: Res<Time>,
    mut state: ResMut<MinimapState>,
    mut images: ResMut<Assets<Image>>,
    inputs: MinimapInputs,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
) {
    let MinimapInputs {
        samples,
        selected,
        debug_menu,
        metrics,
        terrain,
        ping,
    } = inputs;
    state.since_refresh += time.delta_secs();
    let Some(handle) = state.texture.clone() else {
        return;
    };
    if state.since_refresh < REFRESH_SECONDS {
        return;
    }
    state.since_refresh = 0.0;

    let size = state.size;
    let mut pixels = state.base.clone();
    let local_team = samples
        .current
        .players
        .get(&selected.local_player)
        .map(|player| player.team);
    // A viewer controlling both teams sees fixed team colours instead of ally/enemy.
    let team_color = |team: Team| {
        if debug_menu.controls_all_players() || local_team.is_none() {
            if team == Team(0) { ALLY } else { ENEMY }
        } else if Some(team) == local_team {
            ALLY
        } else {
            ENEMY
        }
    };

    for building in samples.current.buildings.values() {
        let (center, extent) = metrics.footprint_center_size(building.footprint);
        let min = state.pixel(center.x - extent.x * 0.5, center.z - extent.y * 0.5);
        let max = state.pixel(center.x + extent.x * 0.5, center.z + extent.y * 0.5);
        let color = team_color(building.team);
        fill(
            &mut pixels,
            size,
            min,
            max.max(min),
            if building.remembered {
                dimmed(color)
            } else {
                color
            },
        );
    }
    for unit in samples.current.units.values() {
        let world = sim_point_to_world(unit.position);
        let at = state.pixel(world.x, world.z);
        fill(
            &mut pixels,
            size,
            at,
            at + IVec2::ONE,
            team_color(unit.team),
        );
    }
    for builder in samples.current.builders.values() {
        let world = sim_point_to_world(builder.position);
        let at = state.pixel(world.x, world.z);
        let own = builder.owner == selected.local_player;
        fill(
            &mut pixels,
            size,
            at - IVec2::ONE,
            at + IVec2::ONE,
            if own {
                BUILDER
            } else {
                team_color(builder.team)
            },
        );
    }

    if let Some((center, age)) = ping.active() {
        // An expanding ring so an attack stands out from the units around it.
        let radius = 3 + (age * 6.0) as i32 % 9;
        let at = state.pixel(center.x, center.y);
        for step in 0..32 {
            let angle = step as f32 / 32.0 * std::f32::consts::TAU;
            let offset = IVec2::new(
                (angle.cos() * radius as f32).round() as i32,
                (angle.sin() * radius as f32).round() as i32,
            );
            put(&mut pixels, size, at + offset, PING);
        }
    }

    let (camera, camera_transform) = *camera;
    let (width, height) = (window.width(), window.height());
    let corners = [
        Vec2::new(0.0, 0.0),
        Vec2::new(width, 0.0),
        Vec2::new(width, height),
        Vec2::new(0.0, height),
    ]
    .map(|corner| viewport_ground_point(camera, camera_transform, corner, &terrain));
    if corners.iter().all(Option::is_some) {
        let points = corners.map(|corner| {
            let corner = corner.expect("checked above");
            state.pixel(corner.x, corner.z)
        });
        for index in 0..4 {
            line(
                &mut pixels,
                size,
                points[index],
                points[(index + 1) % 4],
                VIEW_OUTLINE,
            );
        }
    }

    if let Some(mut image) = images.get_mut(&handle) {
        image.data = Some(pixels);
    }
}

/// Pressing or dragging on the minimap recentres the camera there.
fn steer_camera_from_minimap(
    state: Res<MinimapState>,
    surface: Query<(&Interaction, &RelativeCursorPosition), With<MinimapSurface>>,
    mut target: ResMut<MinimapCameraTarget>,
) {
    let Ok((interaction, cursor)) = surface.single() else {
        return;
    };
    if *interaction != Interaction::Pressed {
        return;
    }
    if let Some(normalized) = cursor.normalized {
        let unit = (normalized + Vec2::splat(0.5)).clamp(Vec2::ZERO, Vec2::ONE);
        target.0 = Some(state.world_min + unit * state.world_size);
    }
}
