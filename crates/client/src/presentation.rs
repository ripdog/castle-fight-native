use std::collections::HashMap;

use bevy::{
    camera::primitives::{Frustum, Sphere},
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    input::mouse::MouseWheel,
    prelude::*,
    time::Fixed,
    window::PrimaryWindow,
};
use castle_fight_sim::{
    AbilityEffect, BuildingFootprint, MovementClass, ProjectileView, ProjectileViewKind,
    SUBUNITS_PER_WORLD_UNIT, SimId, SimPoint, SimulationConfig, Team,
};

use crate::{
    SimulationPlayback,
    bridge::{BuildingSample, BuildingVisualKind, PresentationSamples, UnitSample, UnitVisualKind},
    terrain::{TerrainSurface, TerrainTextureLayout, TerrainTextureSet},
};

const UNIT_MELEE_HEIGHT: f32 = 10.0;
const UNIT_RANGED_HEIGHT: f32 = 8.0;
const UNIT_BASE_COLLISION_RADIUS_WORLD: f32 = 4.0;
const AIR_UNIT_ALTITUDE: f32 = 64.0;
const AIR_HOVER_BOB_HEIGHT: f32 = 2.2;
const AIR_HOVER_PHASE_PER_TICK: f32 = 0.24;
const AIR_WING_FLAP_SPEED: f32 = 9.0;
const AIR_WING_FLAP_AMPLITUDE: f32 = 0.72;
const AIR_WING_BASE_ANGLE: f32 = 0.18;
const BUILDING_HEIGHT: f32 = 96.0;
const PROJECTILE_HEIGHT: f32 = 6.0;
const BALLISTIC_ARC_HEIGHT: f32 = 34.0;
const PROJECTILE_TRAIL_LENGTH: f32 = 14.0;
const PROJECTILE_IMPACT_SECONDS: f32 = 0.22;
const PROJECTILE_IMPACT_RADIUS: f32 = 8.0;
const ABILITY_AREA_EFFECT_SECONDS: f32 = 0.65;
const DEATH_REMAINS_SECONDS: f32 = 0.7;
const UNIT_HEALTH_BAR_WIDTH: f32 = 20.0;
const HEALTH_BAR_DEPTH: f32 = 6.0;
const CORPSE_SIZE: f32 = 7.5;
const CORPSE_THICKNESS: f32 = 0.8;
const SWORD_SWING_SECONDS: f32 = 0.24;
const GUN_RECOIL_SECONDS: f32 = 0.16;
const UNIT_WALK_BOB_HEIGHT: f32 = 0.55;
const UNIT_WALK_PHASE_PER_TICK: f32 = 0.58;
const UNIT_FACING_RESPONSE: f32 = 14.0;
const MISS_INDICATOR_SECONDS: f32 = 1.0;
const MISS_INDICATOR_RISE_PIXELS: f32 = 34.0;

#[derive(Resource, Debug, Clone)]
pub struct WorldMetrics {
    navigation_cell_size_subunits: i32,
    navigation_min: IVec2,
    navigation_max: IVec2,
}

impl WorldMetrics {
    #[must_use]
    pub fn from_simulation_config(config: &SimulationConfig) -> Self {
        Self {
            navigation_cell_size_subunits: config.navigation_cell_size,
            navigation_min: IVec2::new(config.navigation_min.x, config.navigation_min.y),
            navigation_max: IVec2::new(config.navigation_max.x, config.navigation_max.y),
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
    sword_blade_mesh: Handle<Mesh>,
    sword_guard_mesh: Handle<Mesh>,
    gun_body_mesh: Handle<Mesh>,
    gun_barrel_mesh: Handle<Mesh>,
    gun_grip_mesh: Handle<Mesh>,
    mortar_base_mesh: Handle<Mesh>,
    mortar_barrel_mesh: Handle<Mesh>,
    bounce_staff_mesh: Handle<Mesh>,
    bounce_orb_mesh: Handle<Mesh>,
    caster_crystal_mesh: Handle<Mesh>,
    caster_ring_mesh: Handle<Mesh>,
    air_wing_mesh: Handle<Mesh>,
    building_mesh: Handle<Mesh>,
    guaranteed_projectile_mesh: Handle<Mesh>,
    ballistic_projectile_mesh: Handle<Mesh>,
    bounce_projectile_mesh: Handle<Mesh>,
    corpse_mesh: Handle<Mesh>,
    unit_materials: [Handle<StandardMaterial>; 2],
    building_materials: [Handle<StandardMaterial>; 2],
    building_accent_materials: [Handle<StandardMaterial>; 2],
    unit_accent_materials: [Handle<StandardMaterial>; 2],
    corpse_materials: [Handle<StandardMaterial>; 2],
    projectile_materials: [Handle<StandardMaterial>; 4],
    weapon_material: Handle<StandardMaterial>,
    neutral_unit_material: Handle<StandardMaterial>,
    neutral_building_material: Handle<StandardMaterial>,
    neutral_building_accent_material: Handle<StandardMaterial>,
    neutral_unit_accent_material: Handle<StandardMaterial>,
    neutral_corpse_material: Handle<StandardMaterial>,
}

impl PresentationAssets {
    fn unit_mesh(&self, kind: UnitVisualKind) -> Handle<Mesh> {
        match kind.weapon_kind() {
            UnitVisualKind::Melee => self.melee_mesh.clone(),
            UnitVisualKind::Ranged => self.ranged_mesh.clone(),
            UnitVisualKind::Ballistic => self.ballistic_unit_mesh.clone(),
            UnitVisualKind::Bounce => self.bounce_unit_mesh.clone(),
            _ => unreachable!("caster kind must map to a base delivery kind"),
        }
    }

    fn unit_material(&self, team: Team) -> Handle<StandardMaterial> {
        self.unit_materials
            .get(usize::from(team.0))
            .cloned()
            .unwrap_or_else(|| self.neutral_unit_material.clone())
    }

    fn unit_accent_material(&self, team: Team) -> Handle<StandardMaterial> {
        self.unit_accent_materials
            .get(usize::from(team.0))
            .cloned()
            .unwrap_or_else(|| self.neutral_unit_accent_material.clone())
    }

    fn building_material(&self, team: Team) -> Handle<StandardMaterial> {
        self.building_materials
            .get(usize::from(team.0))
            .cloned()
            .unwrap_or_else(|| self.neutral_building_material.clone())
    }

    fn building_accent_material(&self, team: Team) -> Handle<StandardMaterial> {
        self.building_accent_materials
            .get(usize::from(team.0))
            .cloned()
            .unwrap_or_else(|| self.neutral_building_accent_material.clone())
    }

    fn corpse_material(&self, team: Team) -> Handle<StandardMaterial> {
        self.corpse_materials
            .get(usize::from(team.0))
            .cloned()
            .unwrap_or_else(|| self.neutral_corpse_material.clone())
    }

    fn projectile_mesh(&self, projectile: &ProjectileView) -> Handle<Mesh> {
        match projectile.kind {
            ProjectileViewKind::GuaranteedHit { .. } => self.guaranteed_projectile_mesh.clone(),
            ProjectileViewKind::Ballistic { .. } => self.ballistic_projectile_mesh.clone(),
            ProjectileViewKind::Bounce { .. } => self.bounce_projectile_mesh.clone(),
        }
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
    weapon: Option<Entity>,
    max_health_seen: i32,
}

#[derive(Debug, Clone, Copy)]
struct PresentedProjectile {
    entity: Entity,
    last_position: Vec3,
}

#[derive(Resource, Default)]
struct RenderMap {
    units: HashMap<SimId, PresentedEntry>,
    buildings: HashMap<SimId, PresentedEntry>,
    corpses: HashMap<SimId, Entity>,
    projectiles: HashMap<SimId, PresentedProjectile>,
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

#[derive(Debug, Clone, Copy)]
struct ProjectileImpact {
    position: Vec3,
    kind: ProjectileViewKind,
    remaining: f32,
}

#[derive(Resource, Default)]
struct ProjectileImpacts(Vec<ProjectileImpact>);

#[derive(Debug, Clone, Copy)]
struct AbilityAreaImpact {
    position: Vec3,
    radius: f32,
    remaining: f32,
}

#[derive(Resource, Default)]
struct AbilityAreaImpacts(Vec<AbilityAreaImpact>);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WeaponKind {
    Sword,
    Gun,
    Mortar,
    BounceStaff,
}

#[derive(Component)]
struct WeaponPresentation {
    kind: WeaponKind,
    elapsed: Option<f32>,
}

#[derive(Component)]
struct AirWingPresentation {
    side: f32,
    phase: f32,
}

#[derive(Component)]
struct MissIndicator {
    target: SimId,
    fallback_position: SimPoint,
    remaining: f32,
}

#[derive(Default, Reflect, GizmoConfigGroup)]
struct HealthBarGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct ProjectileEffectGizmos;

pub struct CastlePresentationPlugin {
    health_bars: bool,
}

impl CastlePresentationPlugin {
    #[must_use]
    pub const fn new(health_bars: bool) -> Self {
        Self { health_bars }
    }
}

impl Plugin for CastlePresentationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RenderMap>()
            .init_resource::<DeathRemnants>()
            .init_resource::<ProjectileImpacts>()
            .init_resource::<AbilityAreaImpacts>()
            .init_gizmo_group::<HealthBarGizmos>()
            .init_gizmo_group::<ProjectileEffectGizmos>()
            .insert_resource(DebugPresentation {
                health_bars: self.health_bars,
                ..default()
            })
            .add_systems(Startup, setup_scene)
            .add_systems(
                Update,
                (
                    toggle_debug_controls,
                    update_camera,
                    sync_render_entities,
                    trigger_attack_animations,
                    spawn_miss_indicators,
                    interpolate_render_transforms,
                    update_miss_indicators,
                    animate_unit_weapons,
                    animate_air_wings,
                    age_death_remnants,
                    age_projectile_impacts,
                    age_ability_area_impacts,
                    draw_projectile_effects,
                    draw_health_bars,
                    draw_presentation_gizmos,
                    update_window_title,
                )
                    .chain(),
            );
    }
}

fn setup_scene(
    mut commands: Commands,
    world: (
        Res<WorldMetrics>,
        Res<TerrainSurface>,
        Res<TerrainTextureLayout>,
        Res<TerrainTextureSet>,
    ),
    assets: (
        Res<AssetServer>,
        ResMut<Assets<Mesh>>,
        ResMut<Assets<StandardMaterial>>,
    ),
    mut gizmo_configs: ResMut<GizmoConfigStore>,
) {
    let (metrics, terrain, terrain_texture_layout, terrain_textures) = world;
    let (asset_server, mut meshes, mut materials) = assets;
    let (health_bar_config, _) = gizmo_configs.config_mut::<HealthBarGizmos>();
    health_bar_config.line.width = 6.0;
    health_bar_config.line.perspective = false;
    let (projectile_effect_config, _) = gizmo_configs.config_mut::<ProjectileEffectGizmos>();
    projectile_effect_config.line.width = 3.0;
    let melee_mesh = meshes.add(Cuboid::new(7.0, UNIT_MELEE_HEIGHT, 7.0));
    let ranged_mesh = meshes.add(Cuboid::new(6.0, UNIT_RANGED_HEIGHT, 6.0));
    let ballistic_unit_mesh = meshes.add(Cuboid::new(7.0, UNIT_RANGED_HEIGHT, 7.0));
    let bounce_unit_mesh = meshes.add(Cuboid::new(6.0, UNIT_RANGED_HEIGHT, 6.0));
    let sword_blade_mesh = meshes.add(Cuboid::new(0.9, 6.0, 0.65));
    let sword_guard_mesh = meshes.add(Cuboid::new(2.8, 0.45, 0.8));
    let gun_body_mesh = meshes.add(Cuboid::new(1.6, 1.5, 5.0));
    let gun_barrel_mesh = meshes.add(Cuboid::new(0.8, 0.8, 4.0));
    let gun_grip_mesh = meshes.add(Cuboid::new(1.1, 2.2, 1.1));
    let mortar_base_mesh = meshes.add(Cuboid::new(3.8, 1.0, 3.8));
    let mortar_barrel_mesh = meshes.add(Cuboid::new(1.8, 1.8, 6.5));
    let bounce_staff_mesh = meshes.add(Cuboid::new(0.65, 0.65, 5.8));
    let bounce_orb_mesh = meshes.add(Cuboid::new(2.8, 2.8, 2.8));
    let caster_crystal_mesh = meshes.add(Cuboid::new(2.2, 5.0, 2.2));
    let caster_ring_mesh = meshes.add(Cuboid::new(4.6, 0.45, 4.6));
    let air_wing_mesh = meshes.add(Cuboid::new(6.5, 0.45, 4.4));
    let building_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let guaranteed_projectile_mesh = meshes.add(Cuboid::new(1.3, 1.3, 5.5));
    let ballistic_projectile_mesh = meshes.add(Cuboid::new(3.8, 3.8, 3.8));
    let bounce_projectile_mesh = meshes.add(Cuboid::new(2.1, 2.1, 4.8));
    let corpse_mesh = meshes.add(Cuboid::new(CORPSE_SIZE, CORPSE_THICKNESS, CORPSE_SIZE));

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
    let building_accent_materials = [
        materials.add(StandardMaterial {
            base_color: Color::srgb(0.20, 0.62, 1.0),
            emissive: LinearRgba::new(0.04, 0.13, 0.30, 1.0),
            perceptual_roughness: 0.52,
            ..default()
        }),
        materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.30, 0.20),
            emissive: LinearRgba::new(0.30, 0.05, 0.03, 1.0),
            perceptual_roughness: 0.52,
            ..default()
        }),
    ];
    let unit_accent_materials = [
        materials.add(StandardMaterial {
            base_color: Color::srgb(0.38, 0.78, 1.0),
            emissive: LinearRgba::new(0.05, 0.20, 0.42, 1.0),
            metallic: 0.15,
            perceptual_roughness: 0.35,
            ..default()
        }),
        materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.42, 0.28),
            emissive: LinearRgba::new(0.42, 0.07, 0.04, 1.0),
            metallic: 0.15,
            perceptual_roughness: 0.35,
            ..default()
        }),
    ];
    let corpse_materials = [
        materials.add(StandardMaterial {
            base_color: Color::srgba(0.12, 0.32, 0.62, 0.42),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
        materials.add(StandardMaterial {
            base_color: Color::srgba(0.58, 0.14, 0.11, 0.42),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
    ];
    let projectile_materials = [
        materials.add(Color::srgb(0.55, 0.90, 1.0)),
        materials.add(Color::srgb(1.0, 0.62, 0.18)),
        materials.add(Color::srgb(0.65, 1.0, 0.35)),
        materials.add(Color::srgb(0.95, 1.0, 0.52)),
    ];
    let weapon_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.72, 0.76, 0.80),
        metallic: 0.65,
        perceptual_roughness: 0.34,
        ..default()
    });
    let neutral_unit_material = materials.add(Color::srgb(0.72, 0.72, 0.74));
    let neutral_building_material = materials.add(Color::srgb(0.32, 0.32, 0.34));
    let neutral_building_accent_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.70, 0.70, 0.74),
        emissive: LinearRgba::new(0.08, 0.08, 0.09, 1.0),
        ..default()
    });
    let neutral_unit_accent_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.76, 0.76, 0.82),
        emissive: LinearRgba::new(0.10, 0.10, 0.12, 1.0),
        ..default()
    });
    let neutral_corpse_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.34, 0.34, 0.36, 0.40),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });

    commands.insert_resource(PresentationAssets {
        melee_mesh,
        ranged_mesh,
        ballistic_unit_mesh,
        bounce_unit_mesh,
        sword_blade_mesh,
        sword_guard_mesh,
        gun_body_mesh,
        gun_barrel_mesh,
        gun_grip_mesh,
        mortar_base_mesh,
        mortar_barrel_mesh,
        bounce_staff_mesh,
        bounce_orb_mesh,
        caster_crystal_mesh,
        caster_ring_mesh,
        air_wing_mesh,
        building_mesh: building_mesh.clone(),
        guaranteed_projectile_mesh,
        ballistic_projectile_mesh,
        bounce_projectile_mesh,
        corpse_mesh,
        unit_materials,
        building_materials,
        building_accent_materials,
        unit_accent_materials,
        corpse_materials,
        projectile_materials,
        weapon_material,
        neutral_unit_material,
        neutral_building_material,
        neutral_building_accent_material,
        neutral_unit_accent_material,
        neutral_corpse_material,
    });

    let world_size = terrain.world_size();
    let mut world_center = metrics.world_center();
    world_center.y = terrain.height_at_world(world_center.xz());
    let ground_mesh = meshes.add(terrain.mesh());
    let ground_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.16, 0.20, 0.13),
        perceptual_roughness: 0.95,
        ..default()
    });
    commands.spawn((
        Mesh3d(ground_mesh),
        MeshMaterial3d(ground_material),
        Transform::IDENTITY,
    ));

    if terrain_textures.is_available() {
        match terrain.textured_meshes(&terrain_texture_layout, &terrain_textures) {
            Ok(texture_meshes) => {
                for texture_mesh in texture_meshes {
                    let atlas = terrain_textures
                        .atlas(texture_mesh.palette_index)
                        .expect("validated terrain texture mesh references a known atlas");
                    let material = materials.add(StandardMaterial {
                        base_color_texture: Some(asset_server.load(atlas.asset_path().to_owned())),
                        alpha_mode: AlphaMode::Blend,
                        // The Warcraft ground atlases already contain their intended diffuse
                        // shading. Re-lighting the steep transition geometry made ramps turn
                        // nearly black when their normals faced away from our single sun light.
                        unlit: true,
                        // WC3 composes terrain layers in palette order. All palette meshes share
                        // one AABB center, so this bias gives Bevy a camera-independent transparent
                        // sort order and also prevents coplanar depth fighting.
                        depth_bias: texture_mesh.palette_index as f32 + 1.0,
                        perceptual_roughness: 0.95,
                        ..default()
                    });
                    commands.spawn((
                        Mesh3d(meshes.add(texture_mesh.mesh)),
                        MeshMaterial3d(material),
                        Transform::IDENTITY,
                    ));
                }
            }
            Err(error) => {
                eprintln!("warning: failed to build WC3 terrain texture meshes: {error}");
            }
        }
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

fn spawn_unit_weapon(
    commands: &mut Commands,
    assets: &PresentationAssets,
    unit_entity: Entity,
    team: Team,
    visual_kind: UnitVisualKind,
) -> Entity {
    let kind = match visual_kind.weapon_kind() {
        UnitVisualKind::Melee => WeaponKind::Sword,
        UnitVisualKind::Ranged => WeaponKind::Gun,
        UnitVisualKind::Ballistic => WeaponKind::Mortar,
        UnitVisualKind::Bounce => WeaponKind::BounceStaff,
        _ => unreachable!("caster kind must map to a base delivery kind"),
    };
    let mut weapon_entity = None;
    commands.entity(unit_entity).with_children(|unit| {
        let mut weapon = unit.spawn((
            weapon_transform(kind, None),
            Visibility::default(),
            WeaponPresentation {
                kind,
                elapsed: None,
            },
        ));
        weapon_entity = Some(weapon.id());
        weapon.with_children(|weapon| match kind {
            WeaponKind::Sword => {
                weapon.spawn((
                    Mesh3d(assets.sword_blade_mesh.clone()),
                    MeshMaterial3d(assets.weapon_material.clone()),
                    Transform::from_xyz(0.0, 3.1, 0.0),
                ));
                weapon.spawn((
                    Mesh3d(assets.sword_guard_mesh.clone()),
                    MeshMaterial3d(assets.weapon_material.clone()),
                    Transform::from_xyz(0.0, 0.15, 0.0),
                ));
            }
            WeaponKind::Gun => {
                weapon.spawn((
                    Mesh3d(assets.gun_body_mesh.clone()),
                    MeshMaterial3d(assets.weapon_material.clone()),
                    Transform::from_xyz(0.0, 0.0, 1.8),
                ));
                weapon.spawn((
                    Mesh3d(assets.gun_barrel_mesh.clone()),
                    MeshMaterial3d(assets.weapon_material.clone()),
                    Transform::from_xyz(0.0, 0.0, 5.5),
                ));
                weapon.spawn((
                    Mesh3d(assets.gun_grip_mesh.clone()),
                    MeshMaterial3d(assets.weapon_material.clone()),
                    Transform::from_xyz(0.0, -1.25, 1.0),
                ));
            }
            WeaponKind::Mortar => {
                weapon.spawn((
                    Mesh3d(assets.mortar_base_mesh.clone()),
                    MeshMaterial3d(assets.weapon_material.clone()),
                    Transform::from_xyz(0.0, -1.2, 0.0),
                ));
                weapon.spawn((
                    Mesh3d(assets.mortar_barrel_mesh.clone()),
                    MeshMaterial3d(assets.weapon_material.clone()),
                    Transform::from_xyz(0.0, 0.8, 2.8),
                ));
            }
            WeaponKind::BounceStaff => {
                weapon.spawn((
                    Mesh3d(assets.bounce_staff_mesh.clone()),
                    MeshMaterial3d(assets.weapon_material.clone()),
                    Transform::from_xyz(0.0, 0.0, 1.8),
                ));
                weapon.spawn((
                    Mesh3d(assets.bounce_orb_mesh.clone()),
                    MeshMaterial3d(assets.unit_accent_material(team)),
                    Transform::from_xyz(0.0, 3.0, 4.0),
                ));
            }
        });
        if visual_kind.is_caster() {
            let caster_material = assets.unit_accent_material(team);
            unit.spawn((
                Mesh3d(assets.caster_ring_mesh.clone()),
                MeshMaterial3d(caster_material.clone()),
                Transform::from_xyz(0.0, 5.2, 0.0),
            ));
            unit.spawn((
                Mesh3d(assets.caster_crystal_mesh.clone()),
                MeshMaterial3d(caster_material),
                Transform {
                    translation: Vec3::new(0.0, 8.4, 0.0),
                    rotation: Quat::from_euler(EulerRot::XYZ, 0.3, 0.4, 0.2),
                    scale: Vec3::splat(0.8),
                },
            ));
        }
    });
    weapon_entity.expect("unit weapon entity was not spawned")
}

fn spawn_air_wings(
    commands: &mut Commands,
    assets: &PresentationAssets,
    unit_entity: Entity,
    unit: &UnitSample,
) {
    if unit.movement_class != MovementClass::Air {
        return;
    }

    let material = assets.unit_accent_material(unit.team);
    let phase = unit.id.0 as f32 * 0.61;
    commands.entity(unit_entity).with_children(|unit_root| {
        for side in [-1.0_f32, 1.0] {
            unit_root
                .spawn((
                    Transform::from_xyz(side * 3.0, 0.5, -0.4),
                    Visibility::default(),
                    AirWingPresentation { side, phase },
                ))
                .with_child((
                    Mesh3d(assets.air_wing_mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(side * 3.1, 0.0, 0.0),
                ));
        }
    });
}

fn trigger_attack_animations(
    samples: Res<PresentationSamples>,
    render_map: Res<RenderMap>,
    mut weapons: Query<&mut WeaponPresentation>,
) {
    if !samples.is_changed() {
        return;
    }
    for attack in &samples.current.attacks {
        let Some(weapon_entity) = render_map
            .units
            .get(&attack.source)
            .and_then(|entry| entry.weapon)
        else {
            continue;
        };
        if let Ok(mut weapon) = weapons.get_mut(weapon_entity) {
            weapon.elapsed = Some(0.0);
        }
    }
}

fn spawn_miss_indicators(mut commands: Commands, samples: Res<PresentationSamples>) {
    if !samples.is_changed() {
        return;
    }

    for attack in samples
        .current
        .attacks
        .iter()
        .filter(|attack| attack.missed)
    {
        commands.spawn((
            Text::new("MISS"),
            TextFont::from_font_size(24.0),
            TextColor(Color::srgb(1.0, 0.88, 0.20)),
            Node {
                position_type: PositionType::Absolute,
                left: px(-10_000.0),
                top: px(-10_000.0),
                ..default()
            },
            ZIndex(100),
            MissIndicator {
                target: attack.target,
                fallback_position: attack.target_position,
                remaining: MISS_INDICATOR_SECONDS,
            },
        ));
    }
}

fn update_miss_indicators(
    mut commands: Commands,
    state: (Res<Time>, Res<PresentationSamples>, Res<TerrainSurface>),
    render_map: Res<RenderMap>,
    transforms: Query<&Transform>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut indicators: Query<(Entity, &mut MissIndicator, &mut Node, &mut TextColor)>,
) {
    let (time, samples, terrain) = state;
    let (camera, camera_transform) = *camera;
    let delta = time.delta_secs();

    for (entity, mut indicator, mut node, mut color) in &mut indicators {
        indicator.remaining -= delta;
        if indicator.remaining <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        let fallback =
            terrain.clamp_world_position(sim_point_to_world(indicator.fallback_position));
        let mut world = render_map
            .units
            .get(&indicator.target)
            .and_then(|entry| transforms.get(entry.entity).ok())
            .map_or(fallback + Vec3::Y * 12.0, |transform| {
                let extra_height = samples
                    .current
                    .units
                    .get(&indicator.target)
                    .map_or(10.0, |unit| unit_height(unit) * 0.65 + 5.0);
                transform.translation + Vec3::Y * extra_height
            });
        let life = (indicator.remaining / MISS_INDICATOR_SECONDS).clamp(0.0, 1.0);
        world.y += (1.0 - life) * 5.0;

        let Ok(viewport) = camera.world_to_viewport(camera_transform, world) else {
            node.left = px(-10_000.0);
            node.top = px(-10_000.0);
            continue;
        };
        node.left = px(viewport.x - 28.0);
        node.top = px(viewport.y - 16.0 - (1.0 - life) * MISS_INDICATOR_RISE_PIXELS);
        color.0 = Color::srgba(1.0, 0.88, 0.20, life.min(0.95));
    }
}

fn animate_unit_weapons(
    time: Res<Time>,
    mut weapons: Query<(&mut WeaponPresentation, &mut Transform)>,
) {
    for (mut weapon, mut transform) in &mut weapons {
        let Some(mut elapsed) = weapon.elapsed else {
            continue;
        };
        elapsed += time.delta_secs();
        let duration = match weapon.kind {
            WeaponKind::Sword => SWORD_SWING_SECONDS,
            WeaponKind::Gun => GUN_RECOIL_SECONDS,
            WeaponKind::Mortar => 0.30,
            WeaponKind::BounceStaff => 0.26,
        };
        let progress = (elapsed / duration).clamp(0.0, 1.0);
        *transform = weapon_transform(weapon.kind, Some(progress));
        if progress >= 1.0 {
            weapon.elapsed = None;
            *transform = weapon_transform(weapon.kind, None);
        } else {
            weapon.elapsed = Some(elapsed);
        }
    }
}

fn animate_air_wings(time: Res<Time>, mut wings: Query<(&AirWingPresentation, &mut Transform)>) {
    let elapsed = time.elapsed_secs();
    for (wing, mut transform) in &mut wings {
        transform.rotation = air_wing_rotation(wing.side, wing.phase, elapsed);
    }
}

fn air_wing_rotation(side: f32, phase: f32, elapsed: f32) -> Quat {
    let flap = (elapsed * AIR_WING_FLAP_SPEED + phase).sin() * AIR_WING_FLAP_AMPLITUDE;
    Quat::from_rotation_z(side * (AIR_WING_BASE_ANGLE + flap))
}

fn weapon_transform(kind: WeaponKind, progress: Option<f32>) -> Transform {
    let envelope = progress.map_or(0.0, |progress| (std::f32::consts::PI * progress).sin());
    match kind {
        WeaponKind::Sword => Transform {
            translation: Vec3::new(4.2, -1.7, 0.0),
            rotation: Quat::from_rotation_z(-0.35 - 1.75 * envelope),
            ..default()
        },
        WeaponKind::Gun => Transform::from_translation(Vec3::new(3.8, 0.0, -1.7 * envelope)),
        WeaponKind::Mortar => Transform {
            translation: Vec3::new(0.0, 0.2, 0.4),
            rotation: Quat::from_rotation_x(0.55 + 0.30 * envelope),
            ..default()
        },
        WeaponKind::BounceStaff => Transform {
            translation: Vec3::new(3.0, 0.0, 0.0),
            rotation: Quat::from_rotation_z(-0.65 + 0.55 * envelope),
            ..default()
        },
    }
}

fn spawn_building_visual(
    commands: &mut Commands,
    assets: &PresentationAssets,
    root: Entity,
    building: &BuildingSample,
    size: Vec2,
    height: f32,
) {
    let body_material = assets.building_material(building.team);
    let accent_material = assets.building_accent_material(building.team);
    let weapon_material = assets.weapon_material.clone();
    commands
        .entity(root)
        .with_children(|parent| match building.visual_kind {
            BuildingVisualKind::Structure => {
                parent.spawn((
                    Mesh3d(assets.building_mesh.clone()),
                    MeshMaterial3d(body_material.clone()),
                    Transform {
                        translation: Vec3::new(0.0, -height * 0.10, 0.0),
                        scale: Vec3::new(size.x * 0.78, height * 0.78, size.y * 0.78),
                        ..default()
                    },
                ));
                for x in [-0.34, 0.34] {
                    for z in [-0.34, 0.34] {
                        parent.spawn((
                            Mesh3d(assets.building_mesh.clone()),
                            MeshMaterial3d(accent_material.clone()),
                            Transform {
                                translation: Vec3::new(size.x * x, height * 0.16, size.y * z),
                                scale: Vec3::new(size.x * 0.18, height * 0.60, size.y * 0.18),
                                ..default()
                            },
                        ));
                    }
                }
            }
            BuildingVisualKind::Production
            | BuildingVisualKind::ProductionAttack
            | BuildingVisualKind::ProductionSpellcaster
            | BuildingVisualKind::ProductionAttackSpellcaster => {
                parent.spawn((
                    Mesh3d(assets.building_mesh.clone()),
                    MeshMaterial3d(body_material.clone()),
                    Transform {
                        translation: Vec3::new(0.0, -height * 0.16, 0.0),
                        scale: Vec3::new(size.x * 0.86, height * 0.66, size.y * 0.82),
                        ..default()
                    },
                ));
                parent.spawn((
                    Mesh3d(assets.building_mesh.clone()),
                    MeshMaterial3d(accent_material.clone()),
                    Transform {
                        translation: Vec3::new(0.0, height * 0.20, 0.0),
                        scale: Vec3::new(size.x * 0.92, height * 0.10, size.y * 0.88),
                        ..default()
                    },
                ));
                for x in [-0.24, 0.24] {
                    parent.spawn((
                        Mesh3d(assets.building_mesh.clone()),
                        MeshMaterial3d(body_material.clone()),
                        Transform {
                            translation: Vec3::new(size.x * x, height * 0.30, -size.y * 0.18),
                            scale: Vec3::new(size.x * 0.14, height * 0.54, size.y * 0.14),
                            ..default()
                        },
                    ));
                }
                if matches!(
                    building.visual_kind,
                    BuildingVisualKind::ProductionAttack
                        | BuildingVisualKind::ProductionAttackSpellcaster
                ) {
                    spawn_building_attack_details(
                        parent,
                        assets,
                        &size,
                        height,
                        &weapon_material,
                        &accent_material,
                    );
                }
                if matches!(
                    building.visual_kind,
                    BuildingVisualKind::ProductionSpellcaster
                        | BuildingVisualKind::ProductionAttackSpellcaster
                ) {
                    spawn_building_spell_details(parent, assets, &size, height, &accent_material);
                }
            }
            BuildingVisualKind::Attack => {
                spawn_building_combat_body(parent, assets, &body_material, &size, height);
                spawn_building_attack_details(
                    parent,
                    assets,
                    &size,
                    height,
                    &weapon_material,
                    &accent_material,
                );
            }
            BuildingVisualKind::Spellcaster => {
                spawn_building_combat_body(parent, assets, &body_material, &size, height);
                spawn_building_spell_details(parent, assets, &size, height, &accent_material);
            }
            BuildingVisualKind::AttackSpellcaster => {
                spawn_building_combat_body(parent, assets, &body_material, &size, height);
                spawn_building_attack_details(
                    parent,
                    assets,
                    &size,
                    height,
                    &weapon_material,
                    &accent_material,
                );
                spawn_building_spell_details(parent, assets, &size, height, &accent_material);
            }
        });
}

fn spawn_building_combat_body(
    parent: &mut ChildSpawnerCommands,
    assets: &PresentationAssets,
    body_material: &Handle<StandardMaterial>,
    size: &Vec2,
    height: f32,
) {
    parent.spawn((
        Mesh3d(assets.building_mesh.clone()),
        MeshMaterial3d(body_material.clone()),
        Transform {
            translation: Vec3::new(0.0, -height * 0.16, 0.0),
            scale: Vec3::new(size.x * 0.72, height * 0.68, size.y * 0.72),
            ..default()
        },
    ));
}

fn spawn_building_attack_details(
    parent: &mut ChildSpawnerCommands,
    assets: &PresentationAssets,
    size: &Vec2,
    height: f32,
    weapon_material: &Handle<StandardMaterial>,
    accent_material: &Handle<StandardMaterial>,
) {
    parent.spawn((
        Mesh3d(assets.building_mesh.clone()),
        MeshMaterial3d(accent_material.clone()),
        Transform {
            translation: Vec3::new(0.0, height * 0.28, 0.0),
            scale: Vec3::new(size.x * 0.44, height * 0.26, size.y * 0.44),
            ..default()
        },
    ));
    parent.spawn((
        Mesh3d(assets.building_mesh.clone()),
        MeshMaterial3d(weapon_material.clone()),
        Transform {
            translation: Vec3::new(0.0, height * 0.29, size.y * 0.38),
            scale: Vec3::new(size.x * 0.11, height * 0.10, size.y * 0.60),
            ..default()
        },
    ));
}

fn spawn_building_spell_details(
    parent: &mut ChildSpawnerCommands,
    assets: &PresentationAssets,
    size: &Vec2,
    height: f32,
    accent_material: &Handle<StandardMaterial>,
) {
    parent.spawn((
        Mesh3d(assets.building_mesh.clone()),
        MeshMaterial3d(accent_material.clone()),
        Transform {
            translation: Vec3::new(0.0, height * 0.18, 0.0),
            scale: Vec3::new(size.x * 0.24, height * 0.52, size.y * 0.24),
            ..default()
        },
    ));
    parent.spawn((
        Mesh3d(assets.caster_crystal_mesh.clone()),
        MeshMaterial3d(accent_material.clone()),
        Transform {
            translation: Vec3::new(0.0, height * 0.49, 0.0),
            rotation: Quat::from_euler(EulerRot::XYZ, 0.45, 0.65, 0.35),
            scale: Vec3::splat(0.8),
        },
    ));
}

fn sync_render_entities(
    mut commands: Commands,
    samples: Res<PresentationSamples>,
    world: (
        Res<WorldMetrics>,
        Res<TerrainSurface>,
        Res<PresentationAssets>,
    ),
    mut render_map: ResMut<RenderMap>,
    mut remnants: ResMut<DeathRemnants>,
    mut projectile_impacts: ResMut<ProjectileImpacts>,
    mut ability_impacts: ResMut<AbilityAreaImpacts>,
) {
    let (metrics, terrain, assets) = world;
    if !samples.is_changed() {
        return;
    }

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
        let became_authoritative_corpse = samples
            .current
            .corpses
            .values()
            .any(|corpse| corpse.source_unit == id);
        if !became_authoritative_corpse && let Some(unit) = samples.previous.units.get(&id) {
            remnants.0.push(DeathRemnant {
                position: sim_point_to_terrain_world(unit.position, &terrain),
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
            let (mut center, _) = metrics.footprint_center_size(building.footprint);
            center.y = terrain.height_at_world(center.xz());
            remnants.0.push(DeathRemnant {
                position: center,
                team: building.team,
                building: true,
                remaining: DEATH_REMAINS_SECONDS,
            });
        }
    }

    let stale_corpses: Vec<_> = render_map
        .corpses
        .keys()
        .copied()
        .filter(|id| !samples.current.corpses.contains_key(id))
        .collect();
    for id in stale_corpses {
        if let Some(entity) = render_map.corpses.remove(&id) {
            commands.entity(entity).despawn();
        }
    }

    let stale_projectiles: Vec<_> = render_map
        .projectiles
        .keys()
        .copied()
        .filter(|id| !samples.current.projectiles.contains_key(id))
        .collect();
    for id in stale_projectiles {
        if let Some(projectile_entry) = render_map.projectiles.remove(&id) {
            commands.entity(projectile_entry.entity).despawn();
            if let Some(projectile) = samples.previous.projectiles.get(&id) {
                projectile_impacts.0.push(ProjectileImpact {
                    position: projectile_entry.last_position,
                    kind: projectile.kind,
                    remaining: PROJECTILE_IMPACT_SECONDS,
                });
            }
        }
    }

    for cast in &samples.current.ability_casts {
        let AbilityEffect::AreaDamage { radius, .. } = cast.effect else {
            continue;
        };
        let Some(target_position) = cast.target_position else {
            continue;
        };
        ability_impacts.0.push(AbilityAreaImpact {
            position: sim_point_to_terrain_world(target_position, &terrain) + Vec3::Y * 2.0,
            radius: radius as f32 / SUBUNITS_PER_WORLD_UNIT as f32,
            remaining: ABILITY_AREA_EFFECT_SECONDS,
        });
    }

    for unit in samples.current.units.values() {
        if let Some(entry) = render_map.units.get_mut(&unit.id) {
            entry.max_health_seen = entry.max_health_seen.max(unit.health);
            continue;
        }
        let position = unit_ground_position(unit.position, unit.movement_class, &terrain)
            + Vec3::Y * (unit_height(unit) * 0.5);
        let entity = commands
            .spawn((
                Mesh3d(assets.unit_mesh(unit.visual_kind)),
                MeshMaterial3d(assets.unit_material(unit.team)),
                Transform {
                    translation: position,
                    scale: Vec3::splat(unit_render_scale(unit)),
                    ..default()
                },
            ))
            .id();
        let weapon = spawn_unit_weapon(&mut commands, &assets, entity, unit.team, unit.visual_kind);
        spawn_air_wings(&mut commands, &assets, entity, unit);
        render_map.units.insert(
            unit.id,
            PresentedEntry {
                entity,
                weapon: Some(weapon),
                max_health_seen: unit.health.max(1),
            },
        );
    }

    for building in samples.current.buildings.values() {
        if let Some(entry) = render_map.buildings.get_mut(&building.id) {
            entry.max_health_seen = entry.max_health_seen.max(building.health);
            continue;
        }
        let (mut center, size) = metrics.footprint_center_size(building.footprint);
        center.y = terrain.height_at_world(center.xz());
        let visual_height = building_height(building);
        let entity = commands
            .spawn((
                Transform::from_xyz(center.x, center.y + visual_height * 0.5, center.z),
                Visibility::default(),
            ))
            .id();
        spawn_building_visual(
            &mut commands,
            &assets,
            entity,
            building,
            size,
            visual_height,
        );
        render_map.buildings.insert(
            building.id,
            PresentedEntry {
                entity,
                weapon: None,
                max_health_seen: building.health.max(1),
            },
        );
    }

    for corpse in samples.current.corpses.values() {
        if render_map.corpses.contains_key(&corpse.id) {
            continue;
        }
        let position = corpse_render_position(corpse.position, &terrain);
        let entity = commands
            .spawn((
                Mesh3d(assets.corpse_mesh.clone()),
                MeshMaterial3d(assets.corpse_material(corpse.source_team)),
                Transform::from_translation(position),
            ))
            .id();
        render_map.corpses.insert(corpse.id, entity);
    }

    for projectile in samples.current.projectiles.values() {
        if render_map.projectiles.contains_key(&projectile.id) {
            continue;
        }
        let position = sim_point_to_terrain_world(projectile.launch_position, &terrain)
            + Vec3::Y * PROJECTILE_HEIGHT;
        let entity = commands
            .spawn((
                Mesh3d(assets.projectile_mesh(projectile)),
                MeshMaterial3d(assets.projectile_material(projectile)),
                Transform::from_translation(position),
            ))
            .id();
        render_map.projectiles.insert(
            projectile.id,
            PresentedProjectile {
                entity,
                last_position: position,
            },
        );
    }
}

fn interpolate_render_transforms(
    clocks: (Res<Time>, Res<Time<Fixed>>, Res<SimulationPlayback>),
    world: (
        Res<PresentationSamples>,
        Res<WorldMetrics>,
        Res<TerrainSurface>,
    ),
    mut render_map: ResMut<RenderMap>,
    mut transforms: Query<&mut Transform>,
) {
    let (time, fixed_time, playback) = clocks;
    let (samples, metrics, terrain) = world;
    let alpha = playback.interpolation_alpha(&fixed_time);
    let render_tick = samples.previous.tick as f32
        + (samples.current.tick.saturating_sub(samples.previous.tick) as f32) * alpha;
    let facing_blend = 1.0 - (-UNIT_FACING_RESPONSE * time.delta_secs()).exp();

    for (id, current) in &samples.current.units {
        let Some(entry) = render_map.units.get(id) else {
            continue;
        };
        let previous = samples.previous.units.get(id).unwrap_or(current);
        let ground_position = unit_ground_position_lerp(
            previous.position,
            current.position,
            current.movement_class,
            alpha,
            &terrain,
        );
        let moving = previous.position != current.position;
        let bob = unit_motion_bob(current.id, current.movement_class, render_tick, moving);
        let position = ground_position + Vec3::Y * (unit_height(current) * 0.5 + bob);
        let desired_rotation =
            unit_facing_rotation(current, previous, &samples, &metrics, &terrain, alpha);
        if let Ok(mut transform) = transforms.get_mut(entry.entity) {
            if transform.translation != position {
                transform.translation = position;
            }
            if let Some(desired_rotation) = desired_rotation {
                transform.rotation = transform.rotation.slerp(desired_rotation, facing_blend);
            }
        }
    }

    for (id, current) in &samples.current.buildings {
        let Some(entry) = render_map.buildings.get(id) else {
            continue;
        };
        let (mut center, _) = metrics.footprint_center_size(current.footprint);
        center.y = terrain.height_at_world(center.xz());
        let position = Vec3::new(
            center.x,
            center.y + building_height(current) * 0.5,
            center.z,
        );
        if let Ok(mut transform) = transforms.get_mut(entry.entity)
            && transform.translation != position
        {
            transform.translation = position;
        }
    }

    for (id, projectile) in &samples.current.projectiles {
        let Some(projectile_entry) = render_map.projectiles.get_mut(id) else {
            continue;
        };
        let (position, rotation) =
            projectile_pose(projectile, &samples, &metrics, &terrain, alpha, render_tick);
        projectile_entry.last_position = position;
        if let Ok(mut transform) = transforms.get_mut(projectile_entry.entity) {
            if transform.translation != position {
                transform.translation = position;
            }
            transform.rotation = rotation;
        }
    }
}

fn projectile_pose(
    projectile: &ProjectileView,
    samples: &PresentationSamples,
    metrics: &WorldMetrics,
    terrain: &TerrainSurface,
    alpha: f32,
    render_tick: f32,
) -> (Vec3, Quat) {
    let start = sim_point_to_terrain_world(projectile.launch_position, terrain);
    let target = projectile_target(projectile, samples, metrics, terrain, alpha, start);
    let travel_ticks = projectile
        .impact_tick
        .saturating_sub(projectile.launch_tick)
        .max(1) as f32;
    let progress = ((render_tick - projectile.launch_tick as f32) / travel_ticks).clamp(0.0, 1.0);
    let position = projectile_position_at_progress(projectile, start, target, progress);
    let tangent_progress = if progress < 0.98 {
        (progress + 0.02).min(1.0)
    } else {
        (progress - 0.02).max(0.0)
    };
    let tangent_position =
        projectile_position_at_progress(projectile, start, target, tangent_progress);
    let rotation = projectile_rotation(position, tangent_position, progress < 0.98);
    (position, rotation)
}

fn projectile_rotation(position: Vec3, tangent_position: Vec3, samples_forward: bool) -> Quat {
    let direction = if samples_forward {
        tangent_position - position
    } else {
        position - tangent_position
    };
    if direction.length_squared() > 0.0001 {
        Quat::from_rotation_arc(Vec3::Z, direction.normalize())
    } else {
        Quat::IDENTITY
    }
}

fn projectile_target(
    projectile: &ProjectileView,
    samples: &PresentationSamples,
    metrics: &WorldMetrics,
    terrain: &TerrainSurface,
    alpha: f32,
    fallback: Vec3,
) -> Vec3 {
    match projectile.kind {
        ProjectileViewKind::GuaranteedHit { target }
        | ProjectileViewKind::Bounce { target, .. } => {
            entity_render_position(target, samples, metrics, terrain, alpha).unwrap_or(fallback)
        }
        ProjectileViewKind::Ballistic { destination, .. } => {
            sim_point_to_terrain_world(destination, terrain)
        }
    }
}

fn projectile_position_at_progress(
    projectile: &ProjectileView,
    start: Vec3,
    target: Vec3,
    progress: f32,
) -> Vec3 {
    let mut position = start.lerp(target, progress);
    position.y += PROJECTILE_HEIGHT;
    if matches!(projectile.kind, ProjectileViewKind::Ballistic { .. }) {
        position.y += BALLISTIC_ARC_HEIGHT * 4.0 * progress * (1.0 - progress);
    }
    position
}

fn unit_facing_rotation(
    current: &UnitSample,
    previous: &UnitSample,
    samples: &PresentationSamples,
    metrics: &WorldMetrics,
    terrain: &TerrainSurface,
    alpha: f32,
) -> Option<Quat> {
    let current_position = unit_ground_position_lerp(
        previous.position,
        current.position,
        current.movement_class,
        alpha,
        terrain,
    );
    let direction = current
        .target
        .and_then(|target| entity_render_position(target, samples, metrics, terrain, alpha))
        .map(|target| target - current_position)
        .filter(|direction| direction.xz().length_squared() > 0.001)
        .or_else(|| {
            let movement = sim_point_to_terrain_world(current.position, terrain)
                - sim_point_to_terrain_world(previous.position, terrain);
            (movement.xz().length_squared() > 0.001).then_some(movement)
        })?;
    Some(facing_rotation(direction))
}

fn facing_rotation(direction: Vec3) -> Quat {
    Quat::from_rotation_y(direction.x.atan2(direction.z))
}

fn unit_motion_bob(
    id: SimId,
    movement_class: MovementClass,
    render_tick: f32,
    moving: bool,
) -> f32 {
    match movement_class {
        MovementClass::Ground => walk_bob(id, render_tick, moving),
        MovementClass::Air => {
            let phase = render_tick * AIR_HOVER_PHASE_PER_TICK + id.0 as f32 * 0.47;
            phase.sin() * AIR_HOVER_BOB_HEIGHT
        }
    }
}

fn walk_bob(id: SimId, render_tick: f32, moving: bool) -> f32 {
    if !moving {
        return 0.0;
    }
    let phase = render_tick * UNIT_WALK_PHASE_PER_TICK + id.0 as f32 * 0.71;
    phase.sin().abs() * UNIT_WALK_BOB_HEIGHT
}

fn entity_render_position(
    id: SimId,
    samples: &PresentationSamples,
    metrics: &WorldMetrics,
    terrain: &TerrainSurface,
    alpha: f32,
) -> Option<Vec3> {
    if let Some(current) = samples.current.units.get(&id) {
        let previous = samples.previous.units.get(&id).unwrap_or(current);
        return Some(unit_ground_position_lerp(
            previous.position,
            current.position,
            current.movement_class,
            alpha,
            terrain,
        ));
    }
    samples.current.buildings.get(&id).map(|building| {
        let (mut center, _) = metrics.footprint_center_size(building.footprint);
        center.y = terrain.height_at_world(center.xz());
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

fn age_projectile_impacts(time: Res<Time>, mut impacts: ResMut<ProjectileImpacts>) {
    let delta = time.delta_secs();
    for impact in &mut impacts.0 {
        impact.remaining -= delta;
    }
    impacts.0.retain(|impact| impact.remaining > 0.0);
}

fn age_ability_area_impacts(time: Res<Time>, mut impacts: ResMut<AbilityAreaImpacts>) {
    let delta = time.delta_secs();
    for impact in &mut impacts.0 {
        impact.remaining -= delta;
    }
    impacts.0.retain(|impact| impact.remaining > 0.0);
}

fn draw_projectile_effects(
    samples: Res<PresentationSamples>,
    render_map: Res<RenderMap>,
    transforms: Query<&Transform>,
    impacts: Res<ProjectileImpacts>,
    ability_impacts: Res<AbilityAreaImpacts>,
    mut gizmos: Gizmos<ProjectileEffectGizmos>,
) {
    for (id, projectile) in &samples.current.projectiles {
        let Some(entry) = render_map.projectiles.get(id) else {
            continue;
        };
        let Ok(transform) = transforms.get(entry.entity) else {
            continue;
        };
        let forward = transform.rotation * Vec3::Z;
        let color = projectile_effect_color(projectile.kind);
        gizmos.line(
            transform.translation,
            transform.translation - forward * PROJECTILE_TRAIL_LENGTH,
            color.with_alpha(0.58),
        );
    }

    for impact in &impacts.0 {
        let life = (impact.remaining / PROJECTILE_IMPACT_SECONDS).clamp(0.0, 1.0);
        let radius = PROJECTILE_IMPACT_RADIUS * (1.0 + (1.0 - life) * 0.75);
        gizmos.circle(
            Isometry3d::new(impact.position, Quat::from_rotation_arc(Vec3::Z, Vec3::Y)),
            radius,
            projectile_effect_color(impact.kind).with_alpha(life * 0.85),
        );
    }

    for impact in &ability_impacts.0 {
        let life = (impact.remaining / ABILITY_AREA_EFFECT_SECONDS).clamp(0.0, 1.0);
        let radius = impact.radius * (0.35 + (1.0 - life) * 0.65);
        let color = Color::srgb(0.78, 0.42, 1.0).with_alpha(life * 0.92);
        let orientation = Quat::from_rotation_arc(Vec3::Z, Vec3::Y);
        gizmos.circle(Isometry3d::new(impact.position, orientation), radius, color);
        gizmos.circle(
            Isometry3d::new(impact.position + Vec3::Y * 5.0, orientation),
            radius * 0.72,
            color.with_alpha(life * 0.55),
        );
    }
}

fn draw_health_bars(
    clocks: (Res<Time<Fixed>>, Res<SimulationPlayback>),
    samples: Res<PresentationSamples>,
    world: (Res<WorldMetrics>, Res<TerrainSurface>),
    render_map: Res<RenderMap>,
    debug: Res<DebugPresentation>,
    camera_frustum: Single<&Frustum, With<Camera3d>>,
    mut health_gizmos: Gizmos<HealthBarGizmos>,
) {
    let (fixed_time, playback) = clocks;
    let (metrics, terrain) = world;
    if !debug.health_bars {
        return;
    }

    let alpha = playback.interpolation_alpha(&fixed_time);
    for (id, unit) in &samples.current.units {
        let Some(entry) = render_map.units.get(id) else {
            continue;
        };
        let previous = samples.previous.units.get(id).unwrap_or(unit);
        let position = unit_ground_position_lerp(
            previous.position,
            unit.position,
            unit.movement_class,
            alpha,
            &terrain,
        ) + Vec3::Y * (unit_height(unit) + 3.0);
        if !health_bar_visible(&camera_frustum, position, UNIT_HEALTH_BAR_WIDTH) {
            continue;
        }
        draw_health_bar(
            &mut health_gizmos,
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
        let (mut center, size) = metrics.footprint_center_size(building.footprint);
        center.y = terrain.height_at_world(center.xz());
        let position = center + Vec3::Y * (building_height(building) + 4.0);
        let width = size.x.clamp(24.0, 72.0);
        if !health_bar_visible(&camera_frustum, position, width) {
            continue;
        }
        draw_health_bar(
            &mut health_gizmos,
            position,
            width,
            building.health,
            entry.max_health_seen,
            building.team,
        );
    }
}

fn draw_presentation_gizmos(
    clocks: (Res<Time<Fixed>>, Res<SimulationPlayback>),
    samples: Res<PresentationSamples>,
    metrics: Res<WorldMetrics>,
    terrain: Res<TerrainSurface>,
    debug: Res<DebugPresentation>,
    remnants: Res<DeathRemnants>,
    mut gizmos: Gizmos,
) {
    let (fixed_time, playback) = clocks;
    let alpha = playback.interpolation_alpha(&fixed_time);

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
        let rendered = unit_ground_position_lerp(
            previous.position,
            unit.position,
            unit.movement_class,
            alpha,
            &terrain,
        );
        let authoritative = unit_ground_position(unit.position, unit.movement_class, &terrain);
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
            && let Some(target_position) =
                entity_render_position(target, &samples, &metrics, &terrain, alpha)
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
            &terrain,
            building.footprint,
            team_color(building.team),
        );
        if let Some(target) = building.target
            && let Some(target_position) =
                entity_render_position(target, &samples, &metrics, &terrain, alpha)
        {
            let (mut center, _) = metrics.footprint_center_size(building.footprint);
            center.y = terrain.height_at_world(center.xz());
            gizmos.line(
                center + Vec3::Y * 3.0,
                target_position + Vec3::Y * 2.0,
                Color::srgba(1.0, 1.0, 1.0, 0.18),
            );
        }
    }
}

fn health_bar_visible(frustum: &Frustum, center: Vec3, width: f32) -> bool {
    frustum.intersects_sphere(
        &Sphere {
            center: center.into(),
            radius: width * 0.6,
        },
        false,
    )
}

fn draw_health_bar(
    gizmos: &mut Gizmos<HealthBarGizmos>,
    center: Vec3,
    width: f32,
    health: i32,
    max_health: i32,
    team: Team,
) {
    let ratio = (health.max(0) as f32 / max_health.max(1) as f32).clamp(0.0, 1.0);
    let half = width * 0.5;
    let start = center - Vec3::X * half;
    let end = center + Vec3::X * half;
    let fill_end = start + Vec3::X * width * ratio;
    let background = Color::srgb(0.085, 0.085, 0.095);

    gizmos.line(start, end, background);
    if ratio > 0.0 {
        let foreground_offset = Vec3::new(0.0, 0.04, HEALTH_BAR_DEPTH * 0.02);
        gizmos.line(
            start + foreground_offset,
            fill_end + foreground_offset,
            team_color(team),
        );
    }
}

pub(crate) fn draw_footprint_outline(
    gizmos: &mut Gizmos,
    metrics: &WorldMetrics,
    terrain: &TerrainSurface,
    footprint: BuildingFootprint,
    color: Color,
) {
    let (center, size) = metrics.footprint_center_size(footprint);
    let half = size * 0.5;
    let corner = |x: f32, z: f32| Vec3::new(x, terrain.height_at_world(Vec2::new(x, z)) + 0.15, z);
    let min_min = corner(center.x - half.x, center.z - half.y);
    let max_min = corner(center.x + half.x, center.z - half.y);
    let max_max = corner(center.x + half.x, center.z + half.y);
    let min_max = corner(center.x - half.x, center.z + half.y);
    gizmos.line(min_min, max_min, color);
    gizmos.line(max_min, max_max, color);
    gizmos.line(max_max, min_max, color);
    gizmos.line(min_max, min_min, color);
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
    world: (Res<WorldMetrics>, Res<TerrainSurface>),
    mut camera: Single<(&Camera, &mut RtsCamera, &mut Transform), With<Camera3d>>,
) {
    let (metrics, terrain) = world;
    let (camera_component, rig, transform) = &mut *camera;

    if mouse_buttons.just_pressed(MouseButton::Middle)
        && let Some(cursor) = window.cursor_position()
    {
        let camera_global = GlobalTransform::from(**transform);
        rig.grab_anchor = viewport_ground_point(camera_component, &camera_global, cursor, &terrain);
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
        rig.focus.y = terrain.height_at_world(rig.focus.xz());
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
            viewport_ground_point(camera_component, &proposed_global, cursor, &terrain)
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
    terrain: &TerrainSurface,
) -> Option<Vec3> {
    let ray = camera.viewport_to_world(camera_transform, cursor).ok()?;
    let direction = ray.direction.normalize();
    if direction.y.abs() < 1.0e-5 {
        return None;
    }
    let mut distance = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))?;
    let mut point = ray.origin + direction * distance;
    for _ in 0..8 {
        let height = terrain.height_at_world(point.xz());
        distance += (height - point.y) / direction.y;
        point = ray.origin + direction * distance;
    }
    if !terrain.contains_world(point.xz()) {
        return None;
    }
    point.y = terrain.height_at_world(point.xz());
    Some(point)
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

fn corpse_render_position(position: SimPoint, terrain: &TerrainSurface) -> Vec3 {
    sim_point_to_terrain_world(position, terrain) + Vec3::Y * (-CORPSE_THICKNESS * 0.5 + 0.08)
}

fn update_window_title(
    samples: Res<PresentationSamples>,
    playback: Res<SimulationPlayback>,
    debug: Res<DebugPresentation>,
    diagnostics: Res<DiagnosticsStore>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|diagnostic| diagnostic.smoothed())
        .map_or_else(|| "--".to_owned(), |fps| format!("{fps:.0}"));
    window.title = format!(
        "Castle Fight Native 3D | {} | {fps} FPS | tick {} | units {} | buildings {} | corpses {} | projectiles {} | Space/P pause | F1 debug {} | H health {} | WASD pan • MMB grab • Q/E rotate • wheel zoom • Home reset",
        if playback.paused { "PAUSED" } else { "RUNNING" },
        samples.current.tick,
        samples.current.units.len(),
        samples.current.buildings.len(),
        samples.current.corpses.len(),
        samples.current.projectiles.len(),
        if debug.overlays { "on" } else { "off" },
        if debug.health_bars { "on" } else { "off" },
    );
}

pub(crate) fn sim_point_to_world(point: SimPoint) -> Vec3 {
    Vec3::new(
        point.x as f32 / SUBUNITS_PER_WORLD_UNIT as f32,
        0.0,
        point.y as f32 / SUBUNITS_PER_WORLD_UNIT as f32,
    )
}

pub(crate) fn sim_point_to_world_lerp(previous: SimPoint, current: SimPoint, alpha: f32) -> Vec3 {
    sim_point_to_world(previous).lerp(sim_point_to_world(current), alpha)
}

pub(crate) fn sim_point_to_terrain_world(point: SimPoint, terrain: &TerrainSurface) -> Vec3 {
    terrain.clamp_world_position(sim_point_to_world(point))
}

pub(crate) fn sim_point_to_terrain_world_lerp(
    previous: SimPoint,
    current: SimPoint,
    alpha: f32,
    terrain: &TerrainSurface,
) -> Vec3 {
    terrain.clamp_world_position(sim_point_to_world_lerp(previous, current, alpha))
}

fn unit_ground_position(
    point: SimPoint,
    movement_class: MovementClass,
    terrain: &TerrainSurface,
) -> Vec3 {
    sim_point_to_terrain_world(point, terrain) + Vec3::Y * unit_visual_altitude(movement_class)
}

fn unit_ground_position_lerp(
    previous: SimPoint,
    current: SimPoint,
    movement_class: MovementClass,
    alpha: f32,
    terrain: &TerrainSurface,
) -> Vec3 {
    sim_point_to_terrain_world_lerp(previous, current, alpha, terrain)
        + Vec3::Y * unit_visual_altitude(movement_class)
}

pub(crate) const fn unit_visual_altitude(movement_class: MovementClass) -> f32 {
    match movement_class {
        MovementClass::Ground => 0.0,
        MovementClass::Air => AIR_UNIT_ALTITUDE,
    }
}

pub(crate) fn unit_visual_center_lerp(
    previous: SimPoint,
    current: SimPoint,
    unit: &UnitSample,
    alpha: f32,
    terrain: &TerrainSurface,
) -> Vec3 {
    unit_ground_position_lerp(previous, current, unit.movement_class, alpha, terrain)
        + Vec3::Y * (unit_height(unit) * 0.5)
}

fn unit_render_scale(unit: &UnitSample) -> f32 {
    let collision_world = unit.collision_radius as f32 / SUBUNITS_PER_WORLD_UNIT as f32;
    (collision_world / UNIT_BASE_COLLISION_RADIUS_WORLD).max(1.0)
}

pub(crate) fn unit_height(unit: &UnitSample) -> f32 {
    let base = match unit.visual_kind.weapon_kind() {
        UnitVisualKind::Melee => UNIT_MELEE_HEIGHT,
        UnitVisualKind::Ranged | UnitVisualKind::Ballistic | UnitVisualKind::Bounce => {
            UNIT_RANGED_HEIGHT
        }
        _ => unreachable!("caster kind must map to a base delivery kind"),
    };
    base * unit_render_scale(unit)
}

fn building_height(building: &BuildingSample) -> f32 {
    match building.visual_kind {
        BuildingVisualKind::Structure => BUILDING_HEIGHT * 1.25,
        BuildingVisualKind::Production | BuildingVisualKind::ProductionSpellcaster => {
            BUILDING_HEIGHT
        }
        BuildingVisualKind::Attack
        | BuildingVisualKind::Spellcaster
        | BuildingVisualKind::ProductionAttack
        | BuildingVisualKind::AttackSpellcaster => BUILDING_HEIGHT * 1.15,
        BuildingVisualKind::ProductionAttackSpellcaster => BUILDING_HEIGHT * 1.20,
    }
}

fn projectile_effect_color(kind: ProjectileViewKind) -> Color {
    match kind {
        ProjectileViewKind::GuaranteedHit { .. } => Color::srgb(0.48, 0.90, 1.0),
        ProjectileViewKind::Ballistic { .. } => Color::srgb(1.0, 0.56, 0.12),
        ProjectileViewKind::Bounce { bounce_index, .. } if bounce_index % 2 == 0 => {
            Color::srgb(0.58, 1.0, 0.26)
        }
        ProjectileViewKind::Bounce { .. } => Color::srgb(0.92, 1.0, 0.34),
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
    use castle_fight_sim::{NavCell, TerrainElevationMap};

    use super::*;

    fn original_terrain() -> TerrainSurface {
        TerrainSurface::new(
            TerrainElevationMap::from_wc3_terrain_json(include_str!(
                "../../../docs/original_map/extracted/terrain.json"
            ))
            .unwrap(),
        )
    }

    #[test]
    fn sim_subunits_map_one_to_one_to_render_world_units() {
        let point = SimPoint::new(7 * SUBUNITS_PER_WORLD_UNIT, -3 * SUBUNITS_PER_WORLD_UNIT);
        assert_eq!(sim_point_to_world(point), Vec3::new(7.0, 0.0, -3.0));
    }

    #[test]
    fn weapon_attack_poses_move_from_rest_and_return() {
        let sword_rest = weapon_transform(WeaponKind::Sword, None);
        let sword_mid = weapon_transform(WeaponKind::Sword, Some(0.5));
        let sword_end = weapon_transform(WeaponKind::Sword, Some(1.0));
        assert_ne!(sword_mid.rotation, sword_rest.rotation);
        assert!(sword_end.rotation.dot(sword_rest.rotation).abs() > 0.999_999);

        let gun_rest = weapon_transform(WeaponKind::Gun, None);
        let gun_mid = weapon_transform(WeaponKind::Gun, Some(0.5));
        let gun_end = weapon_transform(WeaponKind::Gun, Some(1.0));
        assert!(gun_mid.translation.z < gun_rest.translation.z);
        assert!(gun_end.translation.distance(gun_rest.translation) < 1.0e-5);
    }

    #[test]
    fn corpse_marker_is_mostly_sunk_into_ground() {
        let terrain = original_terrain();
        let ground = terrain.height_at_world(Vec2::ZERO);
        let position = corpse_render_position(SimPoint::new(0, 0), &terrain);
        let top = position.y + CORPSE_THICKNESS * 0.5;
        assert!(position.y < ground);
        assert!(top > ground);
        assert!(top < ground + CORPSE_THICKNESS * 0.25);
    }

    #[test]
    fn rendered_units_are_clamped_to_original_terrain() {
        let terrain = original_terrain();
        let lane = sim_point_to_terrain_world(SimPoint::new(0, 0), &terrain);
        let outside = sim_point_to_terrain_world(
            SimPoint::new(
                100_000 * SUBUNITS_PER_WORLD_UNIT,
                100_000 * SUBUNITS_PER_WORLD_UNIT,
            ),
            &terrain,
        );

        assert_eq!(lane.y, terrain.height_at_world(Vec2::ZERO));
        assert_eq!(outside.xz(), terrain.world_max());
        assert_eq!(outside.y, terrain.height_at_world(terrain.world_max()));
    }

    #[test]
    fn unit_facing_uses_local_positive_z_as_forward() {
        let right = facing_rotation(Vec3::X);
        let forward = right * Vec3::Z;
        assert!((forward - Vec3::X).length() < 1e-5);

        let backward = facing_rotation(-Vec3::Z) * Vec3::Z;
        assert!((backward + Vec3::Z).length() < 1e-5);
    }

    #[test]
    fn walk_bob_only_moves_visually_moving_units() {
        assert_eq!(walk_bob(SimId(7), 42.5, false), 0.0);
        let bob = walk_bob(SimId(7), 42.5, true);
        assert!((0.0..=UNIT_WALK_BOB_HEIGHT).contains(&bob));
    }

    #[test]
    fn air_units_hover_even_when_stationary() {
        let first = unit_motion_bob(SimId(7), MovementClass::Air, 42.5, false);
        let later = unit_motion_bob(SimId(7), MovementClass::Air, 44.5, false);
        assert!(first.abs() <= AIR_HOVER_BOB_HEIGHT);
        assert!(later.abs() <= AIR_HOVER_BOB_HEIGHT);
        assert_ne!(first, later);
        assert!(
            unit_visual_altitude(MovementClass::Air) > unit_visual_altitude(MovementClass::Ground)
        );
    }

    #[test]
    fn air_wings_flap_and_mirror_each_other() {
        let left_start = air_wing_rotation(-1.0, 0.25, 0.0);
        let left_later = air_wing_rotation(-1.0, 0.25, 0.1);
        let right_start = air_wing_rotation(1.0, 0.25, 0.0);

        assert!(left_start.dot(left_later).abs() < 0.999_999);
        let left_tip = left_start * Vec3::X;
        let right_tip = right_start * Vec3::X;
        assert!((left_tip.y + right_tip.y).abs() < 1.0e-5);
    }

    #[test]
    fn guaranteed_projectile_points_along_travel_direction() {
        let projectile = ProjectileView {
            id: SimId(1),
            source: SimId(2),
            launch_position: SimPoint::new(0, 0),
            launch_tick: 0,
            impact_tick: 10,
            kind: ProjectileViewKind::GuaranteedHit { target: SimId(3) },
        };
        let start = Vec3::ZERO;
        let target = Vec3::new(100.0, 0.0, 0.0);
        let position = projectile_position_at_progress(&projectile, start, target, 0.40);
        let tangent = projectile_position_at_progress(&projectile, start, target, 0.42);
        let rotation = projectile_rotation(position, tangent, true);
        assert!((rotation * Vec3::Z - Vec3::X).length() < 1e-5);
    }

    #[test]
    fn ballistic_projectile_follows_and_pitches_with_arc() {
        let projectile = ProjectileView {
            id: SimId(1),
            source: SimId(2),
            launch_position: SimPoint::new(0, 0),
            launch_tick: 0,
            impact_tick: 10,
            kind: ProjectileViewKind::Ballistic {
                destination: SimPoint::new(100 * SUBUNITS_PER_WORLD_UNIT, 0),
                impact_radius: 0,
            },
        };
        let start = Vec3::ZERO;
        let target = Vec3::new(100.0, 0.0, 0.0);
        let rising = projectile_position_at_progress(&projectile, start, target, 0.25);
        let rising_next = projectile_position_at_progress(&projectile, start, target, 0.27);
        let falling = projectile_position_at_progress(&projectile, start, target, 0.75);
        let falling_next = projectile_position_at_progress(&projectile, start, target, 0.77);
        assert!(rising_next.y > rising.y);
        assert!(falling_next.y < falling.y);
        assert!((projectile_rotation(rising, rising_next, true) * Vec3::Z).y > 0.0);
        assert!((projectile_rotation(falling, falling_next, true) * Vec3::Z).y < 0.0);
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
