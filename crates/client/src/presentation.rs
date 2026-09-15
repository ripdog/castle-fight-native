use std::{collections::HashMap, time::Duration};

use bevy::{
    asset::RenderAssetUsages,
    camera::{
        Exposure,
        primitives::{Frustum, Sphere},
        visibility::NoFrustumCulling,
    },
    ecs::system::SystemParam,
    gltf::Gltf,
    input::mouse::MouseWheel,
    light::AmbientLight,
    mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology},
    pbr::{Material, MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    reflect::TypePath,
    render::{
        render_resource::{
            AsBindGroup, CompareFunction, RenderPipelineDescriptor, SpecializedMeshPipelineError,
        },
        storage::ShaderBuffer,
        view::{ColorGrading, ColorGradingGlobal, ColorGradingSection},
    },
    shader::ShaderRef,
    time::Fixed,
    window::PrimaryWindow,
};
use castle_fight_sim::{
    AbilityCastTarget, AbilityEffect, BuildingFootprint, CASTLE_FIGHT_SIMULATION_HZ, CorpseView,
    MovementClass, ProjectileView, ProjectileViewKind, SUBUNITS_PER_WORLD_UNIT, SimId, SimPoint,
    SimulationConfig, Team,
};

use crate::{
    SelectedMatch, SimulationPlayback,
    bridge::{
        BuilderSample, BuildingSample, BuildingVisualKind, PresentationSamples, UnitSample,
        UnitVisualKind,
    },
    building_models::{BuildingAnimationClip, BuildingModelSet},
    terrain::{TerrainSurface, TerrainTextureLayout, TerrainTextureSet},
    unit_models::{UnitAnimationClip, UnitModelSet},
    wc3_effects::{
        Wc3AbilityVisualAnchor, Wc3EmitterSource, Wc3ParticleAssets, Wc3RibbonSource,
        Wc3StatusVisualKind, Wc3TeamTint, Wc3VisualAnimationGraphs, Wc3VisualModel, Wc3VisualSet,
        emit_wc3_particles, fix_wc3_scene_materials, setup_wc3_visual_animation_players,
        spawn_wc3_ribbon_trails, update_wc3_particles, update_wc3_ribbon_trails,
    },
};

const UNIT_MELEE_HEIGHT: f32 = 10.0;
const BUILDER_HEIGHT: f32 = 10.0;
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

// WC3's classic renderer is much flatter than Bevy's default Blender-calibrated exposure.
// Keep the existing daylight illuminance, but expose it as overcast daylight and restore enough
// ambient fill that faces turned away from the sun do not become nearly black.
const WC3_SCENE_DIRECTIONAL_ILLUMINANCE: f32 = 12_000.0;
const WC3_SCENE_EXPOSURE_EV100: f32 = Exposure::EV100_OVERCAST;
const WC3_SCENE_AMBIENT_BRIGHTNESS: f32 = 400.0;
const WC3_SCENE_POST_SATURATION: f32 = 1.12;
const WC3_SCENE_CONTRAST: f32 = 1.04;
const DEFAULT_BALLISTIC_ARC_HEIGHT: f32 = 34.0;
const PROJECTILE_TRAIL_LENGTH: f32 = 14.0;
const PROJECTILE_IMPACT_SECONDS: f32 = 0.22;
const PROJECTILE_IMPACT_RADIUS: f32 = 8.0;
const ABILITY_AREA_EFFECT_SECONDS: f32 = 0.65;
const ABILITY_MODEL_EFFECT_SECONDS: f32 = 0.9;
const LIGHTNING_EFFECT_SECONDS: f32 = 0.22;
const WC3_CHAIN_LIGHTNING_TEXTURE: &str =
    "wc3/effects/textures/replaceabletextures__weather__lightning.png";
const WC3_CHAIN_LIGHTNING_AVG_SEGMENT_LENGTH: f32 = 100.0;
const WC3_CHAIN_LIGHTNING_PRIMARY_WIDTH: f32 = 50.0;
const WC3_CHAIN_LIGHTNING_SECONDARY_WIDTH: f32 = 30.0;
const WC3_CHAIN_LIGHTNING_NOISE_SCALE: f32 = 0.05;
const DEATH_REMAINS_SECONDS: f32 = 0.7;
const FLESH_DECAY_TICKS: u64 = 2 * CASTLE_FIGHT_SIMULATION_HZ as u64;
const BONE_DECAY_TICKS: u64 = 25 * CASTLE_FIGHT_SIMULATION_HZ as u64;
const HEALTH_BAR_SHADER_PATH: &str = "shaders/health_bar_overlay.wgsl";
const HEALTH_BAR_BATCH_MAX_RECTS: usize = 8_192;
const HEALTH_BAR_BATCH_MIN_BUFFER_RECTS: usize = 64;
const HEALTH_BAR_HEIGHT_PIXELS: f32 = 9.0;
const PROGRESS_BAR_HEIGHT_PIXELS: f32 = 7.0;
const PROGRESS_BAR_GAP_PIXELS: f32 = 4.0;
const HEALTH_BAR_VERTICAL_GAP: f32 = 16.0;
const UNIT_HEALTH_BAR_MIN_WIDTH: f32 = 32.0;
const UNIT_HEALTH_BAR_COLLISION_SCALE: f32 = 4.0;
const UNIT_HEALTH_BAR_HEIGHT_SCALE: f32 = 0.60;
const BUILDING_HEALTH_BAR_FOOTPRINT_SCALE: f32 = 1.08;
const BUILDING_HEALTH_BAR_HEIGHT_SCALE: f32 = 0.60;
const CORPSE_SIZE: f32 = 7.5;
const CORPSE_THICKNESS: f32 = 0.8;
const SWORD_SWING_SECONDS: f32 = 0.24;
const GUN_RECOIL_SECONDS: f32 = 0.16;
const UNIT_WALK_BOB_HEIGHT: f32 = 0.55;
const UNIT_WALK_PHASE_PER_TICK: f32 = 0.58;
const UNIT_FACING_RESPONSE: f32 = 14.0;
const MISS_INDICATOR_SECONDS: f32 = 1.0;
const MISS_INDICATOR_RISE_PIXELS: f32 = 34.0;
const FPS_DISPLAY_SAMPLE_SECONDS: f32 = 0.5;
const WC3_MODEL_FACING_OFFSET: f32 = -std::f32::consts::FRAC_PI_2;
const WC3_PROJECTILE_FACING_OFFSET: f32 = -std::f32::consts::FRAC_PI_2;
const WC3_BUILDING_AMBIENT_ANIMATION_SPEED: f32 = 0.5;

#[derive(Resource, Debug, Clone)]
pub struct WorldMetrics {
    navigation_cell_size_subunits: i32,
    navigation_min: IVec2,
    navigation_max: IVec2,
    camera_focus_min_world: Vec2,
    camera_focus_max_world: Vec2,
}

impl WorldMetrics {
    #[must_use]
    pub fn from_simulation_config(config: &SimulationConfig) -> Self {
        let navigation_cell_world =
            config.navigation_cell_size as f32 / SUBUNITS_PER_WORLD_UNIT as f32;
        let navigation_min = IVec2::new(config.navigation_min.x, config.navigation_min.y);
        let navigation_max = IVec2::new(config.navigation_max.x, config.navigation_max.y);
        Self {
            navigation_cell_size_subunits: config.navigation_cell_size,
            navigation_min,
            navigation_max,
            camera_focus_min_world: navigation_min.as_vec2() * navigation_cell_world,
            camera_focus_max_world: (navigation_max + IVec2::ONE).as_vec2() * navigation_cell_world,
        }
    }

    #[must_use]
    pub(crate) fn with_camera_focus_bounds_world(
        mut self,
        min_x: f32,
        min_y: f32,
        max_x: f32,
        max_y: f32,
    ) -> Self {
        let min = Vec2::new(min_x, min_y);
        let max = Vec2::new(max_x, max_y);
        assert!(min.cmple(max).all(), "camera focus bounds must be ordered");
        assert!(
            min.cmpge(self.world_min()).all() && max.cmple(self.world_max()).all(),
            "camera focus bounds must stay inside navigation bounds"
        );
        self.camera_focus_min_world = min;
        self.camera_focus_max_world = max;
        self
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

    fn clamp_focus(&self, focus: Vec3) -> Vec3 {
        Vec3::new(
            focus
                .x
                .clamp(self.camera_focus_min_world.x, self.camera_focus_max_world.x),
            focus.y,
            focus
                .z
                .clamp(self.camera_focus_min_world.y, self.camera_focus_max_world.y),
        )
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
    lightning_material: Handle<StandardMaterial>,
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
            ProjectileViewKind::GuaranteedHit { .. } | ProjectileViewKind::Reflected { .. } => {
                self.guaranteed_projectile_mesh.clone()
            }
            ProjectileViewKind::Ballistic { .. } => self.ballistic_projectile_mesh.clone(),
            ProjectileViewKind::Bounce { .. } => self.bounce_projectile_mesh.clone(),
        }
    }

    fn projectile_material(&self, projectile: &ProjectileView) -> Handle<StandardMaterial> {
        let index = match projectile.kind {
            ProjectileViewKind::GuaranteedHit { .. } | ProjectileViewKind::Reflected { .. } => 0,
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
    imported_rawcode: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
struct PresentedProjectile {
    entity: Entity,
    last_position: Vec3,
    missile_arc: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct StatusEffectKey {
    target: SimId,
    ability_rawcode: u32,
    kind: Wc3StatusVisualKind,
    slot: u16,
}

#[derive(Resource, Default)]
struct RenderMap {
    units: HashMap<SimId, PresentedEntry>,
    builders: HashMap<SimId, PresentedEntry>,
    buildings: HashMap<SimId, PresentedEntry>,
    corpses: HashMap<SimId, Entity>,
    projectiles: HashMap<SimId, PresentedProjectile>,
    stun_effects: HashMap<SimId, Entity>,
    status_effects: HashMap<StatusEffectKey, Entity>,
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

#[derive(Debug, Clone)]
struct TimedWc3Effect {
    entity: Entity,
    remaining: f32,
    lifetime: f32,
    mesh: Option<Handle<Mesh>>,
    fade_material: Option<Handle<StandardMaterial>>,
}

#[derive(Resource, Default)]
struct TimedWc3Effects(Vec<TimedWc3Effect>);

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

#[derive(Component, Debug, Clone, Copy)]
struct ImportedUnitModelRoot {
    sim_id: SimId,
    rawcode: u32,
    presentation_root: Entity,
}

#[derive(Component, Debug, Clone, Copy)]
struct ImportedBuildingModelRoot {
    sim_id: SimId,
    rawcode: u32,
    presentation_root: Entity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportedBuildingAnimationState {
    Birth,
    Stand,
    Death,
}

#[derive(Component, Debug, Clone)]
struct ImportedBuildingAnimationController {
    sim_id: SimId,
    rawcode: u32,
    presentation_root: Entity,
    model_root: Entity,
    birth: Option<BuildingAnimationClip>,
    stand: Option<BuildingAnimationClip>,
    death: Option<BuildingAnimationClip>,
    state: ImportedBuildingAnimationState,
}

#[derive(Component, Debug, Clone, Copy)]
struct ImportedDeathRemnant;

#[derive(Component, Debug, Clone, Copy)]
struct ImportedBuildingDeathRemnant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportedUnitAnimationState {
    Stand,
    Walk,
    Attack,
    Cast,
    Death,
    DecayFlesh,
    DecayBone,
}

#[derive(Component, Debug, Clone, Copy)]
struct ImportedUnitAnimationController {
    sim_id: SimId,
    presentation_root: Entity,
    stand: AnimationNodeIndex,
    walk: Option<AnimationNodeIndex>,
    attack: Option<AnimationNodeIndex>,
    defend_stand: Option<AnimationNodeIndex>,
    defend_walk: Option<AnimationNodeIndex>,
    defend_attack: Option<AnimationNodeIndex>,
    cast: Option<AnimationNodeIndex>,
    death: Option<UnitAnimationClip>,
    decay_flesh: Option<UnitAnimationClip>,
    decay_bone: Option<UnitAnimationClip>,
    state: ImportedUnitAnimationState,
    last_attack_snapshot_tick: Option<u64>,
    last_cast_snapshot_tick: Option<u64>,
    defend_active: bool,
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

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
struct HealthBarMaterial {
    #[storage(0, read_only)]
    rect_data: Handle<ShaderBuffer>,
}

impl Material for HealthBarMaterial {
    fn vertex_shader() -> ShaderRef {
        HEALTH_BAR_SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        HEALTH_BAR_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout
            .0
            .get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.primitive.cull_mode = None;
        if let Some(depth_stencil) = descriptor.depth_stencil.as_mut() {
            depth_stencil.depth_compare = Some(CompareFunction::Always);
            depth_stencil.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

#[derive(Component)]
struct HealthBarBatchEntity;

#[derive(Debug, Clone, Copy)]
struct HealthBarRect {
    min: Vec2,
    max: Vec2,
    color: [f32; 4],
}

// World-attached bars are deliberately batched into one static clip-space mesh and one small
// storage buffer. Per-entity Bevy UI nodes made camera motion main-thread-bound even when bars
// were offscreen, while mutating a Mesh asset every frame would force Bevy to re-prepare it.
#[derive(Resource)]
struct HealthBarBatch {
    buffer: Handle<ShaderBuffer>,
    rects: Vec<HealthBarRect>,
    gpu_data: Vec<[f32; 4]>,
    gpu_capacity_rects: usize,
    last_rect_count: usize,
}

#[derive(SystemParam)]
struct SceneAssetResources<'w> {
    asset_server: Res<'w, AssetServer>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    health_bar_materials: ResMut<'w, Assets<HealthBarMaterial>>,
    shader_buffers: ResMut<'w, Assets<ShaderBuffer>>,
}

#[derive(SystemParam)]
struct HealthBarRenderParams<'w, 's> {
    debug: Res<'w, DebugPresentation>,
    camera: Single<
        'w,
        's,
        (&'static Camera, &'static GlobalTransform, &'static Frustum),
        With<Camera3d>,
    >,
    batch: ResMut<'w, HealthBarBatch>,
    shader_buffers: ResMut<'w, Assets<ShaderBuffer>>,
    batch_entity: Single<
        'w,
        's,
        (&'static mut Transform, &'static mut Visibility),
        With<HealthBarBatchEntity>,
    >,
}

#[derive(Default, Reflect, GizmoConfigGroup)]
struct ProjectileEffectGizmos;

#[derive(Resource, Debug, Default)]
pub(crate) struct FpsDisplay {
    elapsed_seconds: f32,
    frames: u32,
    fps: Option<f32>,
}

impl FpsDisplay {
    fn record_frame(&mut self, delta_seconds: f32) {
        self.elapsed_seconds += delta_seconds.max(0.0);
        self.frames = self.frames.saturating_add(1);
        if self.elapsed_seconds >= FPS_DISPLAY_SAMPLE_SECONDS {
            self.fps = Some(self.frames as f32 / self.elapsed_seconds);
            self.elapsed_seconds = 0.0;
            self.frames = 0;
        }
    }

    #[must_use]
    pub(crate) const fn fps(&self) -> Option<f32> {
        self.fps
    }
}

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
        app.add_plugins(MaterialPlugin::<HealthBarMaterial>::default())
            .init_resource::<RenderMap>()
            .init_resource::<UnitModelSet>()
            .init_resource::<BuildingModelSet>()
            .init_resource::<Wc3VisualSet>()
            .init_resource::<Wc3VisualAnimationGraphs>()
            .init_resource::<FpsDisplay>()
            .init_resource::<DeathRemnants>()
            .init_resource::<ProjectileImpacts>()
            .init_resource::<AbilityAreaImpacts>()
            .init_resource::<TimedWc3Effects>()
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
                    prepare_unit_model_animations,
                    prepare_building_model_animations,
                    sync_render_entities,
                    fix_wc3_scene_materials,
                    setup_wc3_visual_animation_players,
                    setup_imported_unit_animation_players,
                    setup_imported_building_animation_players,
                    update_imported_building_animations,
                    trigger_attack_animations,
                    update_imported_unit_animations,
                    spawn_miss_indicators,
                    interpolate_render_transforms,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    update_miss_indicators,
                    animate_unit_weapons,
                    animate_air_wings,
                    age_death_remnants,
                    age_projectile_impacts,
                    age_ability_area_impacts,
                    age_timed_wc3_effects,
                    spawn_wc3_ribbon_trails,
                    update_wc3_ribbon_trails,
                    update_wc3_particles,
                    emit_wc3_particles,
                    draw_projectile_effects,
                    update_health_bar_batch,
                    draw_presentation_gizmos,
                    sample_display_fps,
                )
                    .chain()
                    .after(interpolate_render_transforms),
            );
    }
}

fn setup_scene(
    mut commands: Commands,
    model_sets: (
        ResMut<UnitModelSet>,
        ResMut<BuildingModelSet>,
        ResMut<Wc3VisualSet>,
    ),
    selected_match: Res<SelectedMatch>,
    world: (
        Res<WorldMetrics>,
        Res<TerrainSurface>,
        Res<TerrainTextureLayout>,
        Res<TerrainTextureSet>,
    ),
    assets: SceneAssetResources<'_>,
    mut gizmo_configs: ResMut<GizmoConfigStore>,
) {
    let (mut unit_models, mut building_models, mut wc3_visuals) = model_sets;
    let (metrics, terrain, terrain_texture_layout, terrain_textures) = world;
    let SceneAssetResources {
        asset_server,
        mut meshes,
        mut materials,
        mut health_bar_materials,
        mut shader_buffers,
    } = assets;
    let selected_unit_models = selected_match
        .content
        .unit_definitions()
        .map(|definition| definition.rawcode)
        .chain(
            selected_match
                .content
                .builder_definitions()
                .map(|definition| definition.rawcode),
        )
        .collect::<Vec<_>>();
    *unit_models = UnitModelSet::load_selected(&asset_server, &selected_unit_models);
    let selected_building_models = selected_match
        .content
        .production_building_definitions()
        .map(|definition| definition.rawcode)
        .chain(
            selected_match
                .content
                .tower_definitions()
                .map(|definition| definition.rawcode),
        )
        .chain(std::iter::once(u32::from_be_bytes(*b"hcas")))
        .collect::<Vec<_>>();
    *building_models = BuildingModelSet::load_selected(&asset_server, &selected_building_models);
    *wc3_visuals = Wc3VisualSet::load_default(&asset_server);

    let health_bar_buffer_data = vec![[0.0; 4]; 1 + HEALTH_BAR_BATCH_MIN_BUFFER_RECTS * 2];
    let health_bar_buffer = shader_buffers.add(ShaderBuffer::from(health_bar_buffer_data.clone()));
    let health_bar_mesh = meshes.add(health_bar_batch_mesh());
    commands.spawn((
        Mesh3d(health_bar_mesh),
        MeshMaterial3d(health_bar_materials.add(HealthBarMaterial {
            rect_data: health_bar_buffer.clone(),
        })),
        Transform::default(),
        Visibility::Hidden,
        NoFrustumCulling,
        HealthBarBatchEntity,
    ));
    commands.insert_resource(HealthBarBatch {
        buffer: health_bar_buffer,
        rects: Vec::with_capacity(256),
        gpu_data: health_bar_buffer_data,
        gpu_capacity_rects: HEALTH_BAR_BATCH_MIN_BUFFER_RECTS,
        last_rect_count: 0,
    });

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
    commands.insert_resource(Wc3ParticleAssets::new(&mut meshes));

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
    let lightning_material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(asset_server.load(WC3_CHAIN_LIGHTNING_TEXTURE)),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        double_sided: true,
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
        lightning_material,
    });

    let world_size = metrics.world_size();
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
            illuminance: WC3_SCENE_DIRECTIONAL_ILLUMINANCE,
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
        Exposure {
            ev100: WC3_SCENE_EXPOSURE_EV100,
        },
        ColorGrading::with_identical_sections(
            ColorGradingGlobal {
                post_saturation: WC3_SCENE_POST_SATURATION,
                ..default()
            },
            ColorGradingSection {
                contrast: WC3_SCENE_CONTRAST,
                ..default()
            },
        ),
        AmbientLight {
            color: Color::WHITE,
            brightness: WC3_SCENE_AMBIENT_BRIGHTNESS,
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            far: 10_000.0,
            ..default()
        }),
        camera_transform(&rig),
        rig,
    ));
}

fn prepare_unit_model_animations(
    mut unit_models: ResMut<UnitModelSet>,
    gltfs: Res<Assets<Gltf>>,
    animation_clips: Res<Assets<AnimationClip>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    unit_models.prepare_animations(&gltfs, &animation_clips, &mut graphs);
}

fn prepare_building_model_animations(
    mut building_models: ResMut<BuildingModelSet>,
    gltfs: Res<Assets<Gltf>>,
    animation_clips: Res<Assets<AnimationClip>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    building_models.prepare_animations(&gltfs, &animation_clips, &mut graphs);
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

fn setup_imported_unit_animation_players(
    mut commands: Commands,
    unit_models: Res<UnitModelSet>,
    parents: Query<&ChildOf>,
    roots: Query<&ImportedUnitModelRoot>,
    mut players: Query<(Entity, &mut AnimationPlayer), Without<ImportedUnitAnimationController>>,
) {
    for (entity, mut player) in &mut players {
        let Some(root) = imported_model_root(entity, &parents, &roots) else {
            continue;
        };
        let Some(animations) = unit_models.animations(root.rawcode) else {
            continue;
        };

        let mut transitions = AnimationTransitions::new();
        transitions
            .play(&mut player, animations.stand, Duration::ZERO)
            .repeat();
        commands.entity(entity).insert((
            AnimationGraphHandle(animations.graph.clone()),
            transitions,
            ImportedUnitAnimationController {
                sim_id: root.sim_id,
                presentation_root: root.presentation_root,
                stand: animations.stand,
                walk: animations.walk,
                attack: animations.attack,
                defend_stand: animations.defend_stand,
                defend_walk: animations.defend_walk,
                defend_attack: animations.defend_attack,
                cast: animations.cast,
                death: animations.death,
                decay_flesh: animations.decay_flesh,
                decay_bone: animations.decay_bone,
                state: ImportedUnitAnimationState::Stand,
                last_attack_snapshot_tick: None,
                last_cast_snapshot_tick: None,
                defend_active: false,
            },
        ));
    }
}

fn setup_imported_building_animation_players(
    mut commands: Commands,
    building_models: Res<BuildingModelSet>,
    samples: Res<PresentationSamples>,
    parents: Query<&ChildOf>,
    roots: Query<&ImportedBuildingModelRoot>,
    dying_roots: Query<(), With<ImportedBuildingDeathRemnant>>,
    mut players: Query<
        (Entity, &mut AnimationPlayer),
        Without<ImportedBuildingAnimationController>,
    >,
) {
    for (entity, mut player) in &mut players {
        let Some((model_root, root)) = imported_building_model_root(entity, &parents, &roots)
        else {
            continue;
        };
        let Some(animations) = building_models.animations(root.rawcode) else {
            continue;
        };

        let dying = dying_roots.get(root.presentation_root).is_ok();
        let building = samples.current.buildings.get(&root.sim_id);
        let constructing =
            building.is_some_and(|building| building.construction_complete_tick.is_some());
        let (state, initial) = if dying {
            (
                ImportedBuildingAnimationState::Death,
                animations.death.clone(),
            )
        } else if constructing && let Some(birth) = animations.birth.clone() {
            (ImportedBuildingAnimationState::Birth, Some(birth))
        } else {
            (
                ImportedBuildingAnimationState::Stand,
                animations.stand.clone(),
            )
        };
        let Some(initial) = initial else {
            if dying {
                commands.entity(root.presentation_root).despawn();
            }
            continue;
        };

        let mut transitions = AnimationTransitions::new();
        let active = transitions.play(&mut player, initial.node, Duration::ZERO);
        if state == ImportedBuildingAnimationState::Stand {
            active
                .repeat()
                .set_speed(WC3_BUILDING_AMBIENT_ANIMATION_SPEED);
        } else if state == ImportedBuildingAnimationState::Birth
            && let Some(building) = building
            && let Some(phase) = building_construction_phase(samples.current.tick, building)
        {
            active
                .set_seek_time(initial.duration_seconds * phase)
                .pause();
        }
        set_building_emitter_sequence(
            &mut commands,
            &building_models,
            model_root,
            root.rawcode,
            Some(&initial.name),
        );
        commands.entity(entity).insert((
            AnimationGraphHandle(animations.graph.clone()),
            transitions,
            ImportedBuildingAnimationController {
                sim_id: root.sim_id,
                rawcode: root.rawcode,
                presentation_root: root.presentation_root,
                model_root,
                birth: animations.birth.clone(),
                stand: animations.stand.clone(),
                death: animations.death.clone(),
                state,
            },
        ));
    }
}

fn imported_building_model_root(
    entity: Entity,
    parents: &Query<&ChildOf>,
    roots: &Query<&ImportedBuildingModelRoot>,
) -> Option<(Entity, ImportedBuildingModelRoot)> {
    let mut current = entity;
    for _ in 0..128 {
        if let Ok(root) = roots.get(current) {
            return Some((current, *root));
        }
        let Ok(parent) = parents.get(current) else {
            return None;
        };
        current = parent.parent();
    }
    None
}

fn set_building_emitter_sequence(
    commands: &mut Commands,
    building_models: &BuildingModelSet,
    model_root: Entity,
    rawcode: u32,
    sequence: Option<&str>,
) {
    let Some(model) = building_models.get(rawcode) else {
        return;
    };
    let Some(sequence) = sequence else {
        commands.entity(model_root).remove::<Wc3EmitterSource>();
        return;
    };
    commands
        .entity(model_root)
        .insert(Wc3EmitterSource::with_asset_prefix_for_sequence(
            &model.emitters,
            "wc3/buildings",
            sequence,
        ));
}

fn update_imported_building_animations(
    mut commands: Commands,
    building_models: Res<BuildingModelSet>,
    samples: Res<PresentationSamples>,
    dying_roots: Query<(), With<ImportedBuildingDeathRemnant>>,
    mut players: Query<(
        &mut AnimationPlayer,
        &mut AnimationTransitions,
        &mut ImportedBuildingAnimationController,
    )>,
) {
    for (mut player, mut transitions, mut controller) in &mut players {
        if dying_roots.get(controller.presentation_root).is_ok() {
            if controller.state != ImportedBuildingAnimationState::Death {
                let Some(death) = controller.death.clone() else {
                    commands.entity(controller.presentation_root).despawn();
                    continue;
                };
                transitions.play(&mut player, death.node, Duration::from_millis(50));
                set_building_emitter_sequence(
                    &mut commands,
                    &building_models,
                    controller.model_root,
                    controller.rawcode,
                    Some(&death.name),
                );
                controller.state = ImportedBuildingAnimationState::Death;
                continue;
            }
            let finished = controller
                .death
                .as_ref()
                .and_then(|death| player.animation(death.node))
                .is_some_and(|animation| animation.is_finished());
            if finished {
                commands.entity(controller.presentation_root).despawn();
            }
            continue;
        }

        let construction = samples
            .current
            .buildings
            .get(&controller.sim_id)
            .and_then(|building| {
                building_construction_phase(samples.current.tick, building)
                    .map(|phase| (building, phase))
            });
        if let Some((_, phase)) = construction {
            let Some(birth) = controller.birth.clone() else {
                continue;
            };
            if controller.state != ImportedBuildingAnimationState::Birth {
                transitions.play(&mut player, birth.node, Duration::ZERO);
                set_building_emitter_sequence(
                    &mut commands,
                    &building_models,
                    controller.model_root,
                    controller.rawcode,
                    Some(&birth.name),
                );
                controller.state = ImportedBuildingAnimationState::Birth;
            }
            if let Some(animation) = player.animation_mut(birth.node) {
                animation
                    .set_seek_time(birth.duration_seconds * phase)
                    .pause();
            }
            continue;
        }

        if controller.state != ImportedBuildingAnimationState::Birth {
            continue;
        }
        if let Some(stand) = controller.stand.clone() {
            transitions
                .play(&mut player, stand.node, Duration::from_millis(50))
                .repeat()
                .set_speed(WC3_BUILDING_AMBIENT_ANIMATION_SPEED);
            set_building_emitter_sequence(
                &mut commands,
                &building_models,
                controller.model_root,
                controller.rawcode,
                Some(&stand.name),
            );
            controller.state = ImportedBuildingAnimationState::Stand;
        } else {
            set_building_emitter_sequence(
                &mut commands,
                &building_models,
                controller.model_root,
                controller.rawcode,
                None,
            );
            controller.state = ImportedBuildingAnimationState::Stand;
        }
    }
}

fn building_construction_phase(current_tick: u64, building: &BuildingSample) -> Option<f32> {
    let started_tick = building.construction_started_tick?;
    let complete_tick = building.construction_complete_tick?;
    let duration_ticks = complete_tick.saturating_sub(started_tick).max(1);
    let elapsed_ticks = current_tick
        .saturating_sub(started_tick)
        .min(duration_ticks);
    Some(elapsed_ticks as f32 / duration_ticks as f32)
}

fn imported_model_root(
    entity: Entity,
    parents: &Query<&ChildOf>,
    roots: &Query<&ImportedUnitModelRoot>,
) -> Option<ImportedUnitModelRoot> {
    let mut current = entity;
    for _ in 0..128 {
        if let Ok(root) = roots.get(current) {
            return Some(*root);
        }
        let Ok(parent) = parents.get(current) else {
            return None;
        };
        current = parent.parent();
    }
    None
}

fn update_imported_unit_animations(
    mut commands: Commands,
    samples: Res<PresentationSamples>,
    dying_roots: Query<(), With<ImportedDeathRemnant>>,
    mut players: Query<(
        &mut AnimationPlayer,
        &mut AnimationTransitions,
        &mut ImportedUnitAnimationController,
    )>,
) {
    for (mut player, mut transitions, mut controller) in &mut players {
        if let Some(current) = samples.current.builders.get(&controller.sim_id) {
            update_live_imported_builder_animation(
                &samples,
                current,
                &mut player,
                &mut transitions,
                &mut controller,
            );
            continue;
        }
        if let Some(current) = samples.current.units.get(&controller.sim_id) {
            update_live_imported_unit_animation(
                &samples,
                current,
                &mut player,
                &mut transitions,
                &mut controller,
            );
            continue;
        }

        if let Some(corpse) = samples
            .current
            .corpses
            .values()
            .find(|corpse| corpse.source_unit == controller.sim_id)
        {
            update_imported_corpse_animation(
                samples.current.tick,
                corpse,
                &mut player,
                &mut transitions,
                &mut controller,
            );
            continue;
        }

        if dying_roots.get(controller.presentation_root).is_ok() {
            update_imported_death_remnant(
                &mut commands,
                &mut player,
                &mut transitions,
                &mut controller,
            );
        }
    }
}

fn update_live_imported_builder_animation(
    samples: &PresentationSamples,
    current: &BuilderSample,
    player: &mut AnimationPlayer,
    transitions: &mut AnimationTransitions,
    controller: &mut ImportedUnitAnimationController,
) {
    let previous = samples
        .previous
        .builders
        .get(&controller.sim_id)
        .unwrap_or(current);
    let continuous_motion = previous.destination.is_some()
        || previous.follow_target.is_some()
        || previous.repair_target.is_some()
        || previous.build_footprint.is_some()
        || current.destination.is_some()
        || current.follow_target.is_some()
        || current.repair_target.is_some()
        || current.build_footprint.is_some();
    let desired = if continuous_motion
        && previous.position != current.position
        && controller.walk.is_some()
    {
        ImportedUnitAnimationState::Walk
    } else {
        ImportedUnitAnimationState::Stand
    };
    if controller.state == desired {
        return;
    }
    let animation = match desired {
        ImportedUnitAnimationState::Stand => controller.stand,
        ImportedUnitAnimationState::Walk => controller.walk.unwrap_or(controller.stand),
        _ => unreachable!("builder animation only uses stand/walk locomotion"),
    };
    transitions
        .play(player, animation, Duration::from_millis(100))
        .repeat();
    controller.state = desired;
}

fn update_live_imported_unit_animation(
    samples: &PresentationSamples,
    current: &UnitSample,
    player: &mut AnimationPlayer,
    transitions: &mut AnimationTransitions,
    controller: &mut ImportedUnitAnimationController,
) {
    let cast_this_snapshot = controller.last_cast_snapshot_tick != Some(samples.current.tick)
        && samples
            .current
            .ability_casts
            .iter()
            .any(|cast| cast.source == controller.sim_id);
    if cast_this_snapshot {
        controller.last_cast_snapshot_tick = Some(samples.current.tick);
        if let Some(cast) = controller.cast {
            transitions.play(player, cast, Duration::from_millis(50));
            controller.state = ImportedUnitAnimationState::Cast;
            return;
        }
    }

    let defend_active = current.active_defend_ability.is_some();
    let attack_this_snapshot = controller.last_attack_snapshot_tick != Some(samples.current.tick)
        && samples
            .current
            .attacks
            .iter()
            .any(|attack| attack.source == controller.sim_id);
    if attack_this_snapshot {
        controller.last_attack_snapshot_tick = Some(samples.current.tick);
        let attack = if defend_active {
            controller.defend_attack.or(controller.attack)
        } else {
            controller.attack
        };
        if let Some(attack) = attack {
            transitions.play(player, attack, Duration::from_millis(50));
            controller.state = ImportedUnitAnimationState::Attack;
            controller.defend_active = defend_active;
            return;
        }
    }

    let one_shot_still_playing = match controller.state {
        ImportedUnitAnimationState::Attack if controller.defend_active => {
            controller.defend_attack.or(controller.attack)
        }
        ImportedUnitAnimationState::Attack => controller.attack,
        ImportedUnitAnimationState::Cast => controller.cast,
        _ => None,
    }
    .and_then(|animation| player.animation(animation))
    .is_some_and(|animation| !animation.is_finished());
    if one_shot_still_playing {
        return;
    }

    let previous = samples
        .previous
        .units
        .get(&controller.sim_id)
        .unwrap_or(current);
    let desired = if previous.position != current.position && controller.walk.is_some() {
        ImportedUnitAnimationState::Walk
    } else {
        ImportedUnitAnimationState::Stand
    };
    if controller.state == desired && controller.defend_active == defend_active {
        return;
    }
    let animation = match desired {
        ImportedUnitAnimationState::Stand if defend_active => {
            controller.defend_stand.unwrap_or(controller.stand)
        }
        ImportedUnitAnimationState::Walk if defend_active => controller
            .defend_walk
            .or(controller.defend_stand)
            .unwrap_or_else(|| controller.walk.unwrap_or(controller.stand)),
        ImportedUnitAnimationState::Stand => controller.stand,
        ImportedUnitAnimationState::Walk => controller.walk.unwrap_or(controller.stand),
        ImportedUnitAnimationState::Attack
        | ImportedUnitAnimationState::Cast
        | ImportedUnitAnimationState::Death
        | ImportedUnitAnimationState::DecayFlesh
        | ImportedUnitAnimationState::DecayBone => {
            unreachable!("one-shot/death animation is handled before locomotion")
        }
    };
    transitions
        .play(player, animation, Duration::from_millis(100))
        .repeat();
    controller.state = desired;
    controller.defend_active = defend_active;
}

fn update_imported_death_remnant(
    commands: &mut Commands,
    player: &mut AnimationPlayer,
    transitions: &mut AnimationTransitions,
    controller: &mut ImportedUnitAnimationController,
) {
    let Some(death) = controller.death else {
        commands.entity(controller.presentation_root).despawn();
        return;
    };
    if controller.state != ImportedUnitAnimationState::Death {
        transitions.play(player, death.node, Duration::from_millis(50));
        controller.state = ImportedUnitAnimationState::Death;
        return;
    }
    if player
        .animation(death.node)
        .is_some_and(|animation| animation.is_finished())
    {
        commands.entity(controller.presentation_root).despawn();
    }
}

#[derive(Debug, Clone, Copy)]
struct CorpseAnimationPlayback {
    state: ImportedUnitAnimationState,
    clip: UnitAnimationClip,
    elapsed_ticks: u64,
    duration_ticks: u64,
}

fn update_imported_corpse_animation(
    current_tick: u64,
    corpse: &CorpseView,
    player: &mut AnimationPlayer,
    transitions: &mut AnimationTransitions,
    controller: &mut ImportedUnitAnimationController,
) {
    let Some(playback) = corpse_animation_playback(current_tick, corpse, controller) else {
        return;
    };
    if controller.state != playback.state {
        transitions.play(player, playback.clip.node, Duration::ZERO);
        controller.state = playback.state;
    }
    let Some(animation) = player.animation_mut(playback.clip.node) else {
        return;
    };
    let phase = playback.elapsed_ticks as f32 / playback.duration_ticks.max(1) as f32;
    animation
        .set_seek_time(playback.clip.duration_seconds * phase.clamp(0.0, 1.0))
        .pause();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CorpseAnimationPhase {
    state: ImportedUnitAnimationState,
    elapsed_ticks: u64,
    duration_ticks: u64,
}

fn corpse_animation_playback(
    current_tick: u64,
    corpse: &CorpseView,
    controller: &ImportedUnitAnimationController,
) -> Option<CorpseAnimationPlayback> {
    let phase = corpse_animation_phase(current_tick, corpse)?;
    let clip = match phase.state {
        ImportedUnitAnimationState::Death => controller.death,
        ImportedUnitAnimationState::DecayFlesh => controller
            .decay_flesh
            .or(controller.decay_bone)
            .or(controller.death),
        ImportedUnitAnimationState::DecayBone => controller
            .decay_bone
            .or(controller.decay_flesh)
            .or(controller.death),
        _ => None,
    }?;
    Some(CorpseAnimationPlayback {
        state: phase.state,
        clip,
        elapsed_ticks: phase.elapsed_ticks,
        duration_ticks: phase.duration_ticks,
    })
}

fn corpse_animation_phase(current_tick: u64, corpse: &CorpseView) -> Option<CorpseAnimationPhase> {
    let total_ticks = corpse
        .expires_tick
        .map(|expires| expires.saturating_sub(corpse.created_tick))?;
    let death_ticks = total_ticks
        .saturating_sub(FLESH_DECAY_TICKS + BONE_DECAY_TICKS)
        .max(1);
    let age = current_tick.saturating_sub(corpse.created_tick);

    if age < death_ticks {
        return Some(CorpseAnimationPhase {
            state: ImportedUnitAnimationState::Death,
            elapsed_ticks: age,
            duration_ticks: death_ticks,
        });
    }

    let flesh_age = age.saturating_sub(death_ticks);
    if flesh_age < FLESH_DECAY_TICKS {
        return Some(CorpseAnimationPhase {
            state: ImportedUnitAnimationState::DecayFlesh,
            elapsed_ticks: flesh_age,
            duration_ticks: FLESH_DECAY_TICKS,
        });
    }

    Some(CorpseAnimationPhase {
        state: ImportedUnitAnimationState::DecayBone,
        elapsed_ticks: age.saturating_sub(death_ticks + FLESH_DECAY_TICKS),
        duration_ticks: BONE_DECAY_TICKS,
    })
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

type SyncRenderWorld<'w> = (
    Res<'w, WorldMetrics>,
    Res<'w, TerrainSurface>,
    Res<'w, PresentationAssets>,
    Res<'w, UnitModelSet>,
    Res<'w, BuildingModelSet>,
    Res<'w, Wc3VisualSet>,
);

type SyncRenderEffects<'w> = (
    ResMut<'w, DeathRemnants>,
    ResMut<'w, ProjectileImpacts>,
    ResMut<'w, AbilityAreaImpacts>,
    ResMut<'w, TimedWc3Effects>,
    ResMut<'w, Assets<Mesh>>,
    ResMut<'w, Assets<StandardMaterial>>,
);

fn sync_render_entities(
    mut commands: Commands,
    samples: Res<PresentationSamples>,
    world: SyncRenderWorld<'_>,
    mut render_map: ResMut<RenderMap>,
    effects: SyncRenderEffects<'_>,
) {
    let (metrics, terrain, assets, unit_models, building_models, wc3_visuals) = world;
    let (
        mut remnants,
        mut projectile_impacts,
        mut ability_impacts,
        mut timed_effects,
        mut meshes,
        mut materials,
    ) = effects;
    if !samples.is_changed() {
        return;
    }

    let stale_builders: Vec<_> = render_map
        .builders
        .keys()
        .copied()
        .filter(|id| !samples.current.builders.contains_key(id))
        .collect();
    for id in stale_builders {
        if let Some(entry) = render_map.builders.remove(&id) {
            commands.entity(entry.entity).despawn();
        }
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
        let authoritative_corpse = samples
            .current
            .corpses
            .values()
            .find(|corpse| corpse.source_unit == id)
            .copied();
        if let Some(corpse) = authoritative_corpse
            && entry.imported_rawcode.is_some()
        {
            render_map.corpses.insert(corpse.id, entry.entity);
            continue;
        }
        if let Some(rawcode) = entry.imported_rawcode
            && unit_models
                .animations(rawcode)
                .is_some_and(|animations| animations.death.is_some())
        {
            commands.entity(entry.entity).insert(ImportedDeathRemnant);
            continue;
        }

        commands.entity(entry.entity).despawn();
        if authoritative_corpse.is_none()
            && let Some(unit) = samples.previous.units.get(&id)
        {
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
        if samples
            .previous
            .buildings
            .get(&id)
            .is_some_and(|building| building.construction_complete_tick.is_some())
        {
            // Cancelled construction disappears instead of playing a completed building's death
            // sequence. This also keeps an unfinished shell from leaving a fake corpse/remnant.
            commands.entity(entry.entity).despawn();
            continue;
        }
        if let Some(rawcode) = entry.imported_rawcode
            && building_models
                .get(rawcode)
                .is_some_and(|model| model.lifecycle_animations.death.is_some())
        {
            commands
                .entity(entry.entity)
                .insert(ImportedBuildingDeathRemnant);
            continue;
        }

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

    // Upgrades deliberately retain the authoritative building SimId and footprint. If the
    // content rawcode changes in place, replace only the presentation root so the target model can
    // play its Birth sequence; cancellation performs the inverse swap back to the precursor's
    // Stand model without fabricating a death/remnant.
    let changed_building_models: Vec<_> = render_map
        .buildings
        .iter()
        .filter_map(|(id, entry)| {
            let building = samples.current.buildings.get(id)?;
            let desired_rawcode = building.content.and_then(|content| {
                building_models
                    .get(content.rawcode)
                    .map(|_| content.rawcode)
            });
            (entry.imported_rawcode != desired_rawcode).then_some(*id)
        })
        .collect();
    for id in changed_building_models {
        if let Some(entry) = render_map.buildings.remove(&id) {
            commands.entity(entry.entity).despawn();
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

    for chain in &samples.current.chain_lightnings {
        if !wc3_visuals.is_chain_lightning(chain.ability.0) {
            continue;
        }
        for (segment_index, points) in chain.points().windows(2).enumerate() {
            let start = sim_point_to_terrain_world(points[0], &terrain) + Vec3::Y * 8.0;
            let end = sim_point_to_terrain_world(points[1], &terrain) + Vec3::Y * 8.0;
            let width = wc3_chain_lightning_width(chain.bounce_index);
            let seed = chain.ability.0
                ^ chain.source.0 as u32
                ^ samples.current.tick as u32
                ^ lightning_segment_seed(segment_index as u32, 0x9e37_79b9);
            let mesh = meshes.add(build_wc3_chain_lightning_mesh(start, end, seed, width));
            let lightning_material = materials
                .get(&assets.lightning_material)
                .cloned()
                .expect("WC3 Chain Lightning material must exist while presentation is running");
            let lightning_material = materials.add(lightning_material);
            let entity = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(lightning_material.clone()),
                    Transform::IDENTITY,
                ))
                .id();
            timed_effects.0.push(TimedWc3Effect {
                entity,
                remaining: LIGHTNING_EFFECT_SECONDS,
                lifetime: LIGHTNING_EFFECT_SECONDS,
                mesh: Some(mesh),
                fade_material: Some(lightning_material),
            });
        }
    }

    for cast in &samples.current.ability_casts {
        let source_position =
            entity_render_position(cast.source, &samples, &metrics, &terrain, 1.0);
        let target_position = cast
            .target_position
            .map(|position| sim_point_to_terrain_world(position, &terrain))
            .or_else(|| match cast.target {
                AbilityCastTarget::Unit(target) => {
                    entity_render_position(target, &samples, &metrics, &terrain, 1.0)
                }
                AbilityCastTarget::AllEnemyUnits => None,
            });

        for visual in wc3_visuals.ability(cast.ability.0) {
            let position = match visual.anchor {
                Wc3AbilityVisualAnchor::Source => source_position,
                Wc3AbilityVisualAnchor::Target => target_position,
            };
            let Some(position) = position else {
                continue;
            };
            let entity = commands
                .spawn((
                    WorldAssetRoot(visual.model.scene.clone()),
                    Transform::from_translation(position),
                    Wc3EmitterSource::new(&visual.model.emitters),
                    Wc3RibbonSource::new(&visual.model.ribbons),
                ))
                .id();
            if let Some(animation) = visual.model.animation_source() {
                commands.entity(entity).insert(animation);
            }
            timed_effects.0.push(TimedWc3Effect {
                entity,
                remaining: ABILITY_MODEL_EFFECT_SECONDS,
                lifetime: ABILITY_MODEL_EFFECT_SECONDS,
                mesh: None,
                fade_material: None,
            });
        }

        if let AbilityEffect::AreaDamage { radius, .. } = cast.effect
            && let Some(target_position) = cast.target_position
        {
            ability_impacts.0.push(AbilityAreaImpact {
                position: sim_point_to_terrain_world(target_position, &terrain) + Vec3::Y * 2.0,
                radius: radius as f32 / SUBUNITS_PER_WORLD_UNIT as f32,
                remaining: ABILITY_AREA_EFFECT_SECONDS,
            });
        }
    }

    // Defend is auto-maintained by Castle Fight rather than cast through the normal automatic
    // spell system. Detect the authoritative inactive->active transition so the stock WC3
    // DefendCaster particle/model still plays exactly when the shield state comes online.
    for unit in samples.current.units.values() {
        let Some(ability) = unit.active_defend_ability else {
            continue;
        };
        if samples
            .previous
            .units
            .get(&unit.id)
            .is_some_and(|previous| previous.active_defend_ability == Some(ability))
        {
            continue;
        }
        let Some(position) = entity_render_position(unit.id, &samples, &metrics, &terrain, 1.0)
        else {
            continue;
        };
        for visual in wc3_visuals.ability(ability.0) {
            let entity = commands
                .spawn((
                    WorldAssetRoot(visual.model.scene.clone()),
                    Transform::from_translation(position),
                    Wc3EmitterSource::new(&visual.model.emitters),
                    Wc3RibbonSource::new(&visual.model.ribbons),
                ))
                .id();
            if let Some(animation) = visual.model.animation_source() {
                commands.entity(entity).insert(animation);
            }
            timed_effects.0.push(TimedWc3Effect {
                entity,
                remaining: ABILITY_MODEL_EFFECT_SECONDS,
                lifetime: ABILITY_MODEL_EFFECT_SECONDS,
                mesh: None,
                fade_material: None,
            });
        }
    }

    for builder in samples.current.builders.values() {
        if render_map.builders.contains_key(&builder.id) {
            continue;
        }
        let position = sim_point_to_terrain_world(builder.position, &terrain)
            + Vec3::Y * (BUILDER_HEIGHT * 0.5);
        let imported_model = unit_models.get(builder.appearance.rawcode);
        let (entity, imported_rawcode) = if let Some(model) = imported_model {
            let entity = commands
                .spawn((Transform::from_translation(position), Visibility::default()))
                .id();
            commands.entity(entity).with_child((
                WorldAssetRoot(model.scene.clone()),
                ImportedUnitModelRoot {
                    sim_id: builder.id,
                    rawcode: builder.appearance.rawcode,
                    presentation_root: entity,
                },
                Wc3TeamTint::new(builder.team.0, team_color(builder.team), "wc3/units"),
                Transform {
                    translation: Vec3::NEG_Y * BUILDER_HEIGHT * 0.5,
                    rotation: Quat::from_rotation_y(WC3_MODEL_FACING_OFFSET),
                    scale: Vec3::splat(model.scale),
                },
            ));
            (entity, Some(builder.appearance.rawcode))
        } else {
            let entity = commands
                .spawn((
                    Mesh3d(assets.melee_mesh.clone()),
                    MeshMaterial3d(assets.unit_material(builder.team)),
                    Transform {
                        translation: position,
                        scale: Vec3::splat(0.9),
                        ..default()
                    },
                ))
                .id();
            (entity, None)
        };
        render_map.builders.insert(
            builder.id,
            PresentedEntry {
                entity,
                weapon: None,
                imported_rawcode,
            },
        );
    }

    for unit in samples.current.units.values() {
        if render_map.units.contains_key(&unit.id) {
            continue;
        }
        let position = unit_ground_position(unit.position, unit.movement_class, &terrain)
            + Vec3::Y * (unit_height(unit) * 0.5);
        let imported_model = unit.content.and_then(|content| {
            unit_models
                .get(content.rawcode)
                .map(|model| (content.rawcode, model))
        });
        let (entity, weapon, imported_rawcode) = if let Some((rawcode, model)) = imported_model {
            let entity = commands
                .spawn((Transform::from_translation(position), Visibility::default()))
                .id();
            commands.entity(entity).with_child((
                WorldAssetRoot(model.scene.clone()),
                ImportedUnitModelRoot {
                    sim_id: unit.id,
                    rawcode,
                    presentation_root: entity,
                },
                Wc3TeamTint::new(unit.team.0, team_color(unit.team), "wc3/units"),
                Transform {
                    translation: Vec3::NEG_Y * unit_height(unit) * 0.5,
                    rotation: Quat::from_rotation_y(WC3_MODEL_FACING_OFFSET),
                    scale: Vec3::splat(model.scale),
                },
            ));
            (entity, None, Some(rawcode))
        } else {
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
            let weapon =
                spawn_unit_weapon(&mut commands, &assets, entity, unit.team, unit.visual_kind);
            spawn_air_wings(&mut commands, &assets, entity, unit);
            (entity, Some(weapon), None)
        };
        render_map.units.insert(
            unit.id,
            PresentedEntry {
                entity,
                weapon,
                imported_rawcode,
            },
        );
    }

    let stale_status_effects: Vec<_> = render_map
        .status_effects
        .keys()
        .copied()
        .filter(|key| {
            !samples.current.units.get(&key.target).is_some_and(|unit| {
                unit_status_visual_is_active(
                    unit,
                    samples.current.tick,
                    key.ability_rawcode,
                    key.kind,
                )
            })
        })
        .collect();
    for key in stale_status_effects {
        if let Some(entity) = render_map.status_effects.remove(&key) {
            commands.entity(entity).despawn();
        }
    }
    for unit in samples.current.units.values() {
        let movement_count = usize::from(unit.status.movement_modifier_count);
        for modifier in unit.status.movement_modifiers[..movement_count]
            .iter()
            .filter(|modifier| modifier.expires_tick > samples.current.tick)
        {
            spawn_unit_status_visuals(
                &mut commands,
                &mut render_map,
                &wc3_visuals,
                unit,
                modifier.id.0,
                Wc3StatusVisualKind::Movement,
                &terrain,
            );
        }
        let armor_count = usize::from(unit.status.armor_modifier_count);
        for modifier in unit.status.armor_modifiers[..armor_count]
            .iter()
            .filter(|modifier| modifier.expires_tick > samples.current.tick)
        {
            spawn_unit_status_visuals(
                &mut commands,
                &mut render_map,
                &wc3_visuals,
                unit,
                modifier.id.0,
                Wc3StatusVisualKind::Armor,
                &terrain,
            );
        }
    }

    let stale_stun_effects: Vec<_> = render_map
        .stun_effects
        .keys()
        .copied()
        .filter(|id| !entity_is_stunned(*id, &samples.current, samples.current.tick))
        .collect();
    for id in stale_stun_effects {
        if let Some(entity) = render_map.stun_effects.remove(&id) {
            commands.entity(entity).despawn();
        }
    }
    if let Some(stun_model) = wc3_visuals.stun() {
        for unit in samples
            .current
            .units
            .values()
            .filter(|unit| unit.stunned_until_tick > samples.current.tick)
        {
            if render_map.stun_effects.contains_key(&unit.id) {
                continue;
            }
            let position = unit_ground_position(unit.position, unit.movement_class, &terrain)
                + Vec3::Y * unit_stun_height(unit, &render_map, &unit_models);
            spawn_stun_effect(
                &mut commands,
                &mut render_map,
                unit.id,
                position,
                stun_model,
            );
        }
        for building in samples.current.buildings.values().filter(|building| {
            building
                .stunned_until_tick
                .is_some_and(|until| until > samples.current.tick)
        }) {
            if render_map.stun_effects.contains_key(&building.id) {
                continue;
            }
            let (mut center, _) = metrics.footprint_center_size(building.footprint);
            center.y = terrain.height_at_world(center.xz());
            let position = center + Vec3::Y * building_height(building) * 1.08;
            spawn_stun_effect(
                &mut commands,
                &mut render_map,
                building.id,
                position,
                stun_model,
            );
        }
    }

    for building in samples.current.buildings.values() {
        if render_map.buildings.contains_key(&building.id) {
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
        let imported_model = building.content.and_then(|content| {
            building_models
                .get(content.rawcode)
                .map(|model| (content.rawcode, model))
        });
        let imported_rawcode = if let Some((rawcode, model)) = imported_model {
            let constructing = building.construction_complete_tick.is_some();
            let lifecycle_sequence = if constructing && model.lifecycle_animations.birth.is_some() {
                model.lifecycle_animations.birth.as_deref()
            } else {
                model.lifecycle_animations.stand.as_deref()
            };
            let model_root = commands
                .spawn((
                    WorldAssetRoot(model.scene.clone()),
                    ImportedBuildingModelRoot {
                        sim_id: building.id,
                        rawcode,
                        presentation_root: entity,
                    },
                    Wc3TeamTint::new(building.team.0, team_color(building.team), "wc3/buildings"),
                    Transform {
                        translation: Vec3::NEG_Y * visual_height * 0.5,
                        rotation: Quat::from_rotation_y(WC3_MODEL_FACING_OFFSET),
                        scale: Vec3::splat(model.scale),
                    },
                ))
                .id();
            if let Some(sequence) = lifecycle_sequence {
                commands.entity(model_root).insert(
                    Wc3EmitterSource::with_asset_prefix_for_sequence(
                        &model.emitters,
                        "wc3/buildings",
                        sequence,
                    ),
                );
            }
            commands.entity(entity).add_child(model_root);
            Some(rawcode)
        } else {
            spawn_building_visual(
                &mut commands,
                &assets,
                entity,
                building,
                size,
                visual_height,
            );
            None
        };
        render_map.buildings.insert(
            building.id,
            PresentedEntry {
                entity,
                weapon: None,
                imported_rawcode,
            },
        );
    }

    for corpse in samples.current.corpses.values() {
        if render_map.corpses.contains_key(&corpse.id) {
            continue;
        }
        let rawcode = corpse.definition.0;
        let entity = if let Some(model) = unit_models.get(rawcode) {
            let position = sim_point_to_terrain_world(corpse.position, &terrain);
            let entity = commands
                .spawn((Transform::from_translation(position), Visibility::default()))
                .id();
            commands.entity(entity).with_child((
                WorldAssetRoot(model.scene.clone()),
                ImportedUnitModelRoot {
                    sim_id: corpse.source_unit,
                    rawcode,
                    presentation_root: entity,
                },
                Wc3TeamTint::new(
                    corpse.source_team.0,
                    team_color(corpse.source_team),
                    "wc3/units",
                ),
                Transform {
                    rotation: Quat::from_rotation_y(WC3_MODEL_FACING_OFFSET),
                    scale: Vec3::splat(model.scale),
                    ..default()
                },
            ));
            entity
        } else {
            let position = corpse_render_position(corpse.position, &terrain);
            commands
                .spawn((
                    Mesh3d(assets.corpse_mesh.clone()),
                    MeshMaterial3d(assets.corpse_material(corpse.source_team)),
                    Transform::from_translation(position),
                ))
                .id()
        };
        render_map.corpses.insert(corpse.id, entity);
    }

    for projectile in samples.current.projectiles.values() {
        if render_map.projectiles.contains_key(&projectile.id) {
            continue;
        }
        let position = sim_point_to_terrain_world(projectile.launch_position, &terrain)
            + Vec3::Y * PROJECTILE_HEIGHT;
        let imported_projectile = projectile_source_rawcode(projectile, &samples)
            .and_then(|rawcode| wc3_visuals.projectile(rawcode));
        let missile_arc = imported_projectile.map(|visual| visual.missile_arc);
        let entity = if let Some(visual) = imported_projectile {
            let model = &visual.model;
            let entity = commands
                .spawn((Transform::from_translation(position), Visibility::default()))
                .id();
            let model_root = commands
                .spawn((
                    WorldAssetRoot(model.scene.clone()),
                    Transform::from_rotation(Quat::from_rotation_y(WC3_PROJECTILE_FACING_OFFSET)),
                    Wc3EmitterSource::new(&model.emitters),
                    Wc3RibbonSource::new(&model.ribbons),
                ))
                .id();
            if let Some(animation) = model.animation_source() {
                commands.entity(model_root).insert(animation);
            }
            commands.entity(entity).add_child(model_root);
            entity
        } else {
            commands
                .spawn((
                    Mesh3d(assets.projectile_mesh(projectile)),
                    MeshMaterial3d(assets.projectile_material(projectile)),
                    Transform::from_translation(position),
                ))
                .id()
        };
        render_map.projectiles.insert(
            projectile.id,
            PresentedProjectile {
                entity,
                last_position: position,
                missile_arc,
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
        Res<UnitModelSet>,
    ),
    mut render_map: ResMut<RenderMap>,
    mut transforms: Query<&mut Transform>,
) {
    let (time, fixed_time, playback) = clocks;
    let (samples, metrics, terrain, unit_models) = world;
    let alpha = playback.interpolation_alpha(&fixed_time);
    let render_tick = samples.previous.tick as f32
        + (samples.current.tick.saturating_sub(samples.previous.tick) as f32) * alpha;
    let facing_blend = 1.0 - (-UNIT_FACING_RESPONSE * time.delta_secs()).exp();

    for (id, current) in &samples.current.builders {
        let Some(entry) = render_map.builders.get(id) else {
            continue;
        };
        let previous = samples.previous.builders.get(id).unwrap_or(current);
        let continuous_motion = previous.destination.is_some()
            || previous.follow_target.is_some()
            || previous.repair_target.is_some()
            || previous.build_footprint.is_some()
            || current.destination.is_some()
            || current.follow_target.is_some()
            || current.repair_target.is_some()
            || current.build_footprint.is_some();
        let ground_position = if continuous_motion {
            sim_point_to_terrain_world_lerp(previous.position, current.position, alpha, &terrain)
        } else {
            sim_point_to_terrain_world(current.position, &terrain)
        };
        let position = ground_position + Vec3::Y * (BUILDER_HEIGHT * 0.5);
        if let Ok(mut transform) = transforms.get_mut(entry.entity) {
            transform.translation = position;
            if previous.position != current.position {
                let delta =
                    sim_point_to_world(current.position) - sim_point_to_world(previous.position);
                if delta.length_squared() > f32::EPSILON {
                    let desired = Quat::from_rotation_y(delta.x.atan2(delta.z));
                    transform.rotation = transform.rotation.slerp(desired, facing_blend);
                }
            }
        }
    }

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
        if let Some(stun_entity) = render_map.stun_effects.get(id)
            && let Ok(mut transform) = transforms.get_mut(*stun_entity)
        {
            transform.translation =
                ground_position + Vec3::Y * unit_stun_height(current, &render_map, &unit_models);
        }
    }

    for (key, entity) in &render_map.status_effects {
        let Some(current) = samples.current.units.get(&key.target) else {
            continue;
        };
        let previous = samples.previous.units.get(&key.target).unwrap_or(current);
        let moving = previous.position != current.position;
        let bob = unit_motion_bob(current.id, current.movement_class, render_tick, moving);
        let position = unit_ground_position_lerp(
            previous.position,
            current.position,
            current.movement_class,
            alpha,
            &terrain,
        ) + Vec3::Y * bob;
        if let Ok(mut transform) = transforms.get_mut(*entity)
            && transform.translation != position
        {
            transform.translation = position;
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
        if let Some(stun_entity) = render_map.stun_effects.get(id)
            && let Ok(mut transform) = transforms.get_mut(*stun_entity)
        {
            transform.translation = center + Vec3::Y * building_height(current) * 1.08;
        }
    }

    for (id, projectile) in &samples.current.projectiles {
        let Some(projectile_entry) = render_map.projectiles.get_mut(id) else {
            continue;
        };
        let (position, rotation) = projectile_pose(
            projectile,
            projectile_entry.missile_arc,
            &samples,
            &metrics,
            &terrain,
            alpha,
            render_tick,
        );
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
    missile_arc: Option<f32>,
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
    let position =
        projectile_position_at_progress(projectile, start, target, progress, missile_arc);
    let tangent_progress = if progress < 0.98 {
        (progress + 0.02).min(1.0)
    } else {
        (progress - 0.02).max(0.0)
    };
    let tangent_position =
        projectile_position_at_progress(projectile, start, target, tangent_progress, missile_arc);
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
        | ProjectileViewKind::Reflected { target, .. }
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
    missile_arc: Option<f32>,
) -> Vec3 {
    let mut position = start.lerp(target, progress);
    position.y += PROJECTILE_HEIGHT;
    let arc_height = missile_arc
        .map(|arc| wc3_missile_arc_height(start, target, arc))
        .or_else(|| {
            matches!(projectile.kind, ProjectileViewKind::Ballistic { .. })
                .then_some(DEFAULT_BALLISTIC_ARC_HEIGHT)
        });
    if let Some(arc_height) = arc_height {
        position.y += arc_height * 4.0 * progress * (1.0 - progress);
    }
    position
}

fn wc3_missile_arc_height(start: Vec3, target: Vec3, missile_arc: f32) -> f32 {
    let horizontal_distance = (target - start).xz().length();
    let launch_angle = missile_arc.clamp(0.0, 0.99) * std::f32::consts::FRAC_PI_2;
    launch_angle.tan() * horizontal_distance * 0.25
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

fn unit_status_visual_is_active(
    unit: &UnitSample,
    tick: u64,
    ability_rawcode: u32,
    kind: Wc3StatusVisualKind,
) -> bool {
    match kind {
        Wc3StatusVisualKind::Movement => {
            let count = usize::from(unit.status.movement_modifier_count);
            unit.status.movement_modifiers[..count]
                .iter()
                .any(|modifier| modifier.id.0 == ability_rawcode && modifier.expires_tick > tick)
        }
        Wc3StatusVisualKind::Armor => {
            let count = usize::from(unit.status.armor_modifier_count);
            unit.status.armor_modifiers[..count]
                .iter()
                .any(|modifier| modifier.id.0 == ability_rawcode && modifier.expires_tick > tick)
        }
    }
}

fn spawn_unit_status_visuals(
    commands: &mut Commands,
    render_map: &mut RenderMap,
    wc3_visuals: &Wc3VisualSet,
    unit: &UnitSample,
    ability_rawcode: u32,
    kind: Wc3StatusVisualKind,
    terrain: &TerrainSurface,
) {
    let position = unit_ground_position(unit.position, unit.movement_class, terrain);
    for (slot, visual) in wc3_visuals.status(ability_rawcode).iter().enumerate() {
        if visual.kind != kind {
            continue;
        }
        let key = StatusEffectKey {
            target: unit.id,
            ability_rawcode,
            kind,
            slot: u16::try_from(slot).expect("WC3 status visual slot exceeds u16"),
        };
        if render_map.status_effects.contains_key(&key) {
            continue;
        }
        let entity = commands
            .spawn((
                WorldAssetRoot(visual.model.scene.clone()),
                Transform::from_translation(position),
                Wc3EmitterSource::new(&visual.model.emitters),
                Wc3RibbonSource::new(&visual.model.ribbons),
            ))
            .id();
        if let Some(animation) = visual.model.looping_animation_source() {
            commands.entity(entity).insert(animation);
        }
        render_map.status_effects.insert(key, entity);
    }
}

fn entity_is_stunned(id: SimId, snapshot: &crate::bridge::PresentationSnapshot, tick: u64) -> bool {
    snapshot
        .units
        .get(&id)
        .is_some_and(|unit| unit.stunned_until_tick > tick)
        || snapshot.buildings.get(&id).is_some_and(|building| {
            building
                .stunned_until_tick
                .is_some_and(|until| until > tick)
        })
}

fn spawn_stun_effect(
    commands: &mut Commands,
    render_map: &mut RenderMap,
    id: SimId,
    position: Vec3,
    model: &Wc3VisualModel,
) {
    let entity = commands
        .spawn((
            WorldAssetRoot(model.scene.clone()),
            Transform::from_translation(position),
            Wc3EmitterSource::new(&model.emitters),
            Wc3RibbonSource::new(&model.ribbons),
        ))
        .id();
    if let Some(animation) = model.animation_source() {
        commands.entity(entity).insert(animation);
    }
    render_map.stun_effects.insert(id, entity);
}

fn projectile_source_rawcode(
    projectile: &ProjectileView,
    samples: &PresentationSamples,
) -> Option<u32> {
    samples
        .current
        .units
        .get(&projectile.source)
        .and_then(|unit| unit.content)
        .or_else(|| {
            samples
                .previous
                .units
                .get(&projectile.source)
                .and_then(|unit| unit.content)
        })
        .or_else(|| {
            samples
                .current
                .buildings
                .get(&projectile.source)
                .and_then(|building| building.content)
        })
        .or_else(|| {
            samples
                .previous
                .buildings
                .get(&projectile.source)
                .and_then(|building| building.content)
        })
        .map(|content| content.rawcode)
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

fn age_timed_wc3_effects(
    mut commands: Commands,
    time: Res<Time>,
    mut effects: ResMut<TimedWc3Effects>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let delta = time.delta_secs();
    for effect in &mut effects.0 {
        effect.remaining -= delta;
        if let Some(material_handle) = effect.fade_material.as_ref()
            && let Some(mut material) = materials.get_mut(material_handle)
        {
            material.base_color = material.base_color.with_alpha(wc3_lightning_fade_strength(
                effect.remaining,
                effect.lifetime,
            ));
        }
        if effect.remaining <= 0.0 {
            commands.entity(effect.entity).despawn();
            if let Some(mesh) = effect.mesh.take() {
                meshes.remove(mesh.id());
            }
            if let Some(material) = effect.fade_material.take() {
                materials.remove(material.id());
            }
        }
    }
    effects.0.retain(|effect| effect.remaining > 0.0);
}

fn wc3_lightning_fade_strength(remaining: f32, lifetime: f32) -> f32 {
    if lifetime <= f32::EPSILON {
        return 0.0;
    }
    // Keep the initial strike crisp, then smoothly decay the additive contribution instead of
    // popping the bolt off at the end of its short WC3 lifetime.
    let normalized = (remaining / lifetime).clamp(0.0, 1.0);
    let fade = (normalized / 0.75).clamp(0.0, 1.0);
    fade * fade * (3.0 - 2.0 * fade)
}

fn wc3_chain_lightning_width(bounce_index: u8) -> f32 {
    if bounce_index == 0 {
        WC3_CHAIN_LIGHTNING_PRIMARY_WIDTH
    } else {
        WC3_CHAIN_LIGHTNING_SECONDARY_WIDTH
    }
}

fn build_wc3_chain_lightning_mesh(start: Vec3, end: Vec3, seed: u32, width: f32) -> Mesh {
    let delta = end - start;
    let length = delta.length().max(1.0);
    let segment_count = (length / WC3_CHAIN_LIGHTNING_AVG_SEGMENT_LENGTH)
        .ceil()
        .clamp(1.0, 64.0) as usize;
    let direction = delta.normalize_or(Vec3::X);
    let lateral = Vec3::Y.cross(direction).normalize_or(Vec3::X);
    let noise_amplitude = (length * WC3_CHAIN_LIGHTNING_NOISE_SCALE)
        .min(WC3_CHAIN_LIGHTNING_AVG_SEGMENT_LENGTH * 0.75);

    let mut centers = Vec::with_capacity(segment_count + 1);
    for point_index in 0..=segment_count {
        let t = point_index as f32 / segment_count as f32;
        let mut point = start.lerp(end, t);
        if point_index != 0 && point_index != segment_count {
            let envelope = (std::f32::consts::PI * t).sin();
            let side_noise =
                lightning_hash(seed ^ lightning_segment_seed(point_index as u32, 0x9e37_79b9))
                    * 2.0
                    - 1.0;
            let height_noise =
                lightning_hash(seed ^ lightning_segment_seed(point_index as u32, 0x85eb_ca6b))
                    * 2.0
                    - 1.0;
            point += lateral * side_noise * noise_amplitude * envelope;
            point += Vec3::Y * height_noise * noise_amplitude * 0.5 * envelope;
        }
        centers.push(point);
    }

    let mut positions = Vec::with_capacity(segment_count * 4);
    let mut normals = Vec::with_capacity(segment_count * 4);
    let mut uvs = Vec::with_capacity(segment_count * 4);
    let mut indices = Vec::with_capacity(segment_count * 6);
    let half_width = width * 0.5;

    for segment in 0..segment_count {
        let from = centers[segment];
        let to = centers[segment + 1];
        let segment_direction = (to - from).normalize_or(direction);
        let segment_lateral = Vec3::Y.cross(segment_direction).normalize_or(lateral) * half_width;
        let base = u32::try_from(positions.len()).expect("lightning mesh vertex count fits u32");
        positions.extend_from_slice(&[
            (from - segment_lateral).to_array(),
            (from + segment_lateral).to_array(),
            (to - segment_lateral).to_array(),
            (to + segment_lateral).to_array(),
        ]);
        normals.extend_from_slice(&[[0.0, 1.0, 0.0]; 4]);
        // AvgSegLen in LightningData controls the texture segment length, so each generated
        // segment receives one complete copy of the stock 256x64 lightning texture.
        uvs.extend_from_slice(&[[0.0, 1.0], [0.0, 0.0], [1.0, 1.0], [1.0, 0.0]]);
        indices.extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
    }

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

const fn lightning_segment_seed(segment: u32, salt: u32) -> u32 {
    segment.wrapping_mul(salt)
}

fn lightning_hash(mut value: u32) -> f32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^= value >> 16;
    value as f32 / u32::MAX as f32
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

fn update_health_bar_batch(
    clocks: (Res<Time<Fixed>>, Res<SimulationPlayback>),
    samples: Res<PresentationSamples>,
    world: (Res<WorldMetrics>, Res<TerrainSurface>),
    models: (Res<UnitModelSet>, Res<BuildingModelSet>),
    render_map: Res<RenderMap>,
    params: HealthBarRenderParams<'_, '_>,
) {
    let (fixed_time, playback) = clocks;
    let (metrics, terrain) = world;
    let (unit_models, building_models) = models;
    let HealthBarRenderParams {
        debug,
        camera,
        mut batch,
        mut shader_buffers,
        mut batch_entity,
    } = params;
    let (camera, camera_transform, frustum) = *camera;
    let (batch_transform, batch_visibility) = &mut *batch_entity;

    batch.rects.clear();

    if debug.health_bars {
        let Some(viewport) = camera.logical_viewport_rect() else {
            clear_health_bar_buffer_if_needed(&mut batch, &mut shader_buffers);
            **batch_visibility = Visibility::Hidden;
            return;
        };
        let alpha = playback.interpolation_alpha(&fixed_time);
        let rendered_tick = interpolated_sim_tick(&samples, alpha);

        for (id, unit) in &samples.current.units {
            let Some(entry) = render_map.units.get(id) else {
                continue;
            };
            let previous = samples.previous.units.get(id).unwrap_or(unit);
            let ground_position = unit_ground_position_lerp(
                previous.position,
                unit.position,
                unit.movement_class,
                alpha,
                &terrain,
            );
            let overhead_height = unit_bar_overhead_height(unit, entry, &unit_models);
            let world_width = unit_health_bar_width(unit, entry, &unit_models);
            let anchor = ground_position + Vec3::Y * (overhead_height + HEALTH_BAR_VERTICAL_GAP);
            if !health_bar_world_visible(frustum, anchor, world_width) {
                continue;
            }
            let Some(screen) =
                health_bar_screen_layout(camera, camera_transform, anchor, world_width, viewport)
            else {
                continue;
            };
            push_split_bar(
                &mut batch.rects,
                screen,
                HEALTH_BAR_HEIGHT_PIXELS,
                health_ratio(unit.health, unit.health_max),
                team_color(unit.team),
                Color::srgb(0.085, 0.085, 0.095),
                viewport,
            );
        }

        for (id, building) in &samples.current.buildings {
            let Some(entry) = render_map.buildings.get(id) else {
                continue;
            };
            let (mut center, size) = metrics.footprint_center_size(building.footprint);
            center.y = terrain.height_at_world(center.xz());
            let overhead_height = building_bar_overhead_height(building, entry, &building_models);
            let world_width = building_health_bar_width(size, overhead_height);
            let anchor = center + Vec3::Y * (overhead_height + HEALTH_BAR_VERTICAL_GAP);
            if !health_bar_world_visible(frustum, anchor, world_width) {
                continue;
            }
            let Some(screen) =
                health_bar_screen_layout(camera, camera_transform, anchor, world_width, viewport)
            else {
                continue;
            };
            push_split_bar(
                &mut batch.rects,
                screen,
                HEALTH_BAR_HEIGHT_PIXELS,
                health_ratio(building.health, building.health_max),
                team_color(building.team),
                Color::srgb(0.085, 0.085, 0.095),
                viewport,
            );
            if let Some(progress) = building_progress(building, rendered_tick) {
                let progress_screen = HealthBarScreenLayout {
                    top: screen.top + HEALTH_BAR_HEIGHT_PIXELS + PROGRESS_BAR_GAP_PIXELS,
                    ..screen
                };
                push_split_bar(
                    &mut batch.rects,
                    progress_screen,
                    PROGRESS_BAR_HEIGHT_PIXELS,
                    progress,
                    Color::srgb(0.72, 0.72, 0.74),
                    Color::srgb(0.16, 0.16, 0.17),
                    viewport,
                );
            }
        }
    }

    if batch.rects.is_empty() && batch.last_rect_count == 0 {
        **batch_visibility = Visibility::Hidden;
        return;
    }
    if batch.rects.is_empty() {
        **batch_visibility = Visibility::Hidden;
    } else {
        **batch_visibility = Visibility::Visible;
        // Transparent meshes are sorted back-to-front. The shader ignores this transform, but
        // keeping a non-empty batch at the camera sorts it after ordinary transparent geometry.
        batch_transform.translation = camera_transform.translation();
    }
    upload_health_bar_rects(&mut batch, &mut shader_buffers);
}

fn clear_health_bar_buffer_if_needed(
    batch: &mut HealthBarBatch,
    shader_buffers: &mut Assets<ShaderBuffer>,
) {
    if batch.last_rect_count == 0 {
        return;
    }
    batch.rects.clear();
    upload_health_bar_rects(batch, shader_buffers);
}

fn health_bar_world_visible(frustum: &Frustum, center: Vec3, width: f32) -> bool {
    frustum.intersects_sphere(
        &Sphere {
            center: center.into(),
            radius: width * 0.6,
        },
        false,
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct HealthBarScreenLayout {
    left: f32,
    top: f32,
    width: f32,
}

fn health_bar_screen_layout(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    world_anchor: Vec3,
    world_width: f32,
    viewport: Rect,
) -> Option<HealthBarScreenLayout> {
    let camera_right = camera_transform.rotation() * Vec3::X;
    let half_width = world_width * 0.5;
    let left = camera
        .world_to_viewport(camera_transform, world_anchor - camera_right * half_width)
        .ok()?;
    let right = camera
        .world_to_viewport(camera_transform, world_anchor + camera_right * half_width)
        .ok()?;
    let center = (left + right) * 0.5;
    let width = (right.x - left.x).abs();
    if !width.is_finite() || width <= 0.0 {
        return None;
    }
    let layout = HealthBarScreenLayout {
        left: center.x - width * 0.5,
        top: center.y - HEALTH_BAR_HEIGHT_PIXELS * 0.5,
        width,
    };
    let max_bottom = layout.top
        + HEALTH_BAR_HEIGHT_PIXELS
        + PROGRESS_BAR_GAP_PIXELS
        + PROGRESS_BAR_HEIGHT_PIXELS;
    if layout.left + layout.width < viewport.min.x
        || layout.left > viewport.max.x
        || max_bottom < viewport.min.y
        || layout.top > viewport.max.y
    {
        return None;
    }
    Some(layout)
}

fn health_ratio(health: i32, max_health: i32) -> f32 {
    (health.max(0) as f32 / max_health.max(1) as f32).clamp(0.0, 1.0)
}

fn push_split_bar(
    rects: &mut Vec<HealthBarRect>,
    layout: HealthBarScreenLayout,
    height: f32,
    ratio: f32,
    foreground: Color,
    background: Color,
    viewport: Rect,
) {
    let ratio = ratio.clamp(0.0, 1.0);
    let fill_width = layout.width * ratio;
    if fill_width > 0.0 {
        push_screen_rect(
            rects,
            layout.left,
            layout.top,
            fill_width,
            height,
            foreground,
            viewport,
        );
    }
    let empty_width = layout.width - fill_width;
    if empty_width > 0.0 {
        push_screen_rect(
            rects,
            layout.left + fill_width,
            layout.top,
            empty_width,
            height,
            background,
            viewport,
        );
    }
}

fn push_screen_rect(
    rects: &mut Vec<HealthBarRect>,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    color: Color,
    viewport: Rect,
) {
    let right = (left + width).min(viewport.max.x);
    let bottom = (top + height).min(viewport.max.y);
    let left = left.max(viewport.min.x);
    let top = top.max(viewport.min.y);
    if left >= right || top >= bottom {
        return;
    }

    let viewport_size = viewport.size();
    let x0 = ((left - viewport.min.x) / viewport_size.x) * 2.0 - 1.0;
    let x1 = ((right - viewport.min.x) / viewport_size.x) * 2.0 - 1.0;
    let y0 = 1.0 - ((bottom - viewport.min.y) / viewport_size.y) * 2.0;
    let y1 = 1.0 - ((top - viewport.min.y) / viewport_size.y) * 2.0;
    let linear: LinearRgba = color.into();
    rects.push(HealthBarRect {
        min: Vec2::new(x0, y0),
        max: Vec2::new(x1, y1),
        color: [linear.red, linear.green, linear.blue, linear.alpha],
    });
}

fn health_bar_batch_mesh() -> Mesh {
    let mut positions = Vec::with_capacity(HEALTH_BAR_BATCH_MAX_RECTS * 6);
    for rect_index in 0..HEALTH_BAR_BATCH_MAX_RECTS {
        let index = rect_index as f32;
        positions.extend_from_slice(&[
            [0.0, 0.0, index],
            [1.0, 0.0, index],
            [1.0, 1.0, index],
            [0.0, 0.0, index],
            [1.0, 1.0, index],
            [0.0, 1.0, index],
        ]);
    }

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
}

fn upload_health_bar_rects(batch: &mut HealthBarBatch, shader_buffers: &mut Assets<ShaderBuffer>) {
    let rect_count = batch.rects.len().min(HEALTH_BAR_BATCH_MAX_RECTS);
    let required_capacity = rect_count
        .max(HEALTH_BAR_BATCH_MIN_BUFFER_RECTS)
        .next_power_of_two()
        .min(HEALTH_BAR_BATCH_MAX_RECTS);
    if required_capacity > batch.gpu_capacity_rects {
        batch.gpu_capacity_rects = required_capacity;
        batch
            .gpu_data
            .resize(1 + batch.gpu_capacity_rects * 2, [0.0; 4]);
    }

    batch.gpu_data[0] = [rect_count as f32, 0.0, 0.0, 0.0];
    for (index, rect) in batch.rects.iter().take(rect_count).enumerate() {
        batch.gpu_data[1 + index * 2] = [rect.min.x, rect.min.y, rect.max.x, rect.max.y];
        batch.gpu_data[2 + index * 2] = rect.color;
    }

    if let Some(mut buffer) = shader_buffers.get_mut(&batch.buffer) {
        buffer.set_data(batch.gpu_data.clone());
    }
    batch.last_rect_count = rect_count;
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

fn interpolated_sim_tick(samples: &PresentationSamples, alpha: f32) -> f64 {
    let elapsed_ticks = samples.current.tick.saturating_sub(samples.previous.tick);
    samples.previous.tick as f64 + elapsed_ticks as f64 * f64::from(alpha.clamp(0.0, 1.0))
}

fn construction_progress(building: &BuildingSample, rendered_tick: f64) -> Option<f32> {
    let started_tick = building.construction_started_tick?;
    let complete_tick = building.construction_complete_tick?;
    let duration_ticks = complete_tick.saturating_sub(started_tick).max(1) as f64;
    let elapsed_ticks = (rendered_tick - started_tick as f64).clamp(0.0, duration_ticks);
    Some((elapsed_ticks / duration_ticks) as f32)
}

fn production_progress(building: &BuildingSample, rendered_tick: f64) -> Option<f32> {
    let next_spawn_tick = building.next_spawn_tick?;
    let interval_ticks = building.production_interval_ticks?;
    if interval_ticks == 0 {
        return None;
    }
    let interval = f64::from(interval_ticks);
    let remaining = (next_spawn_tick as f64 - rendered_tick).clamp(0.0, interval);
    Some((1.0 - remaining / interval) as f32)
}

fn building_progress(building: &BuildingSample, rendered_tick: f64) -> Option<f32> {
    construction_progress(building, rendered_tick)
        .or_else(|| production_progress(building, rendered_tick))
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

#[derive(SystemParam)]
struct CameraControlResources<'w> {
    time: Res<'w, Time>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse_buttons: Res<'w, ButtonInput<MouseButton>>,
    metrics: Res<'w, WorldMetrics>,
    terrain: Res<'w, TerrainSurface>,
    inspection: Option<Res<'w, crate::inspection::InspectionSelection>>,
    samples: Res<'w, PresentationSamples>,
}

fn update_camera(
    mut mouse_wheel: MessageReader<MouseWheel>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut camera: Single<(&Camera, &mut RtsCamera, &mut Transform), With<Camera3d>>,
    resources: CameraControlResources<'_>,
) {
    let builder_selected = resources
        .inspection
        .as_ref()
        .and_then(|inspection| inspection.selected)
        .is_some_and(|id| resources.samples.current.builders.contains_key(&id));
    let (camera_component, rig, transform) = &mut *camera;

    if resources.mouse_buttons.just_pressed(MouseButton::Middle)
        && let Some(cursor) = window.cursor_position()
    {
        let camera_global = GlobalTransform::from(**transform);
        rig.grab_anchor =
            viewport_ground_point(camera_component, &camera_global, cursor, &resources.terrain);
    }

    let dt = resources.time.delta_secs();
    let forward = Vec3::new(-rig.yaw.sin(), 0.0, -rig.yaw.cos());
    let right = Vec3::new(rig.yaw.cos(), 0.0, -rig.yaw.sin());
    let mut movement = Vec3::ZERO;
    if resources.keys.pressed(KeyCode::KeyW) || resources.keys.pressed(KeyCode::ArrowUp) {
        movement += forward;
    }
    if resources.keys.pressed(KeyCode::KeyS) || resources.keys.pressed(KeyCode::ArrowDown) {
        movement -= forward;
    }
    if (resources.keys.pressed(KeyCode::KeyD) && !builder_selected)
        || resources.keys.pressed(KeyCode::ArrowRight)
    {
        movement += right;
    }
    if resources.keys.pressed(KeyCode::KeyA) || resources.keys.pressed(KeyCode::ArrowLeft) {
        movement -= right;
    }
    if movement != Vec3::ZERO {
        let pan_speed = rig.distance * 0.65;
        rig.focus += movement.normalize() * pan_speed * dt;
    }
    if resources.keys.pressed(KeyCode::KeyQ) {
        rig.yaw += 0.9 * dt;
    }
    if resources.keys.pressed(KeyCode::KeyE) {
        rig.yaw -= 0.9 * dt;
    }
    let scroll: f32 = mouse_wheel.read().map(|event| event.y).sum();
    if scroll != 0.0 {
        rig.distance *= (1.0 - scroll * 0.10).clamp(0.55, 1.45);
    }
    let world_size = resources.metrics.world_size();
    rig.distance = rig.distance.clamp(
        world_size.min_element() * 0.28,
        world_size.max_element() * 1.7,
    );
    if resources.keys.just_pressed(KeyCode::Home) {
        rig.focus = resources.metrics.world_center();
        rig.focus.y = resources.terrain.height_at_world(rig.focus.xz());
        rig.distance = world_size.max_element() * 0.72;
        rig.yaw = 0.0;
    }

    if resources.mouse_buttons.pressed(MouseButton::Middle)
        && let Some(anchor) = rig.grab_anchor
        && let Some(cursor) = window.cursor_position()
    {
        let proposed = camera_transform(rig);
        let proposed_global = GlobalTransform::from(proposed);
        if let Some(cursor_world) = viewport_ground_point(
            camera_component,
            &proposed_global,
            cursor,
            &resources.terrain,
        ) {
            let correction = anchor - cursor_world;
            rig.focus += Vec3::new(correction.x, 0.0, correction.z);
        }
    }

    if resources.mouse_buttons.just_released(MouseButton::Middle) {
        rig.grab_anchor = None;
    }

    // Keep focus inside the authored camera rectangle even when simulation/navigation extends
    // farther into the playable map (for example the rear build cells behind each castle).
    rig.focus = resources.metrics.clamp_focus(rig.focus);
    rig.focus.y = resources.terrain.height_at_world(rig.focus.xz());

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

fn sample_display_fps(time: Res<Time>, mut display: ResMut<FpsDisplay>) {
    display.record_frame(time.delta_secs());
}

pub(crate) fn sim_point_to_world(point: SimPoint) -> Vec3 {
    Vec3::new(
        point.x as f32 / SUBUNITS_PER_WORLD_UNIT as f32,
        0.0,
        point.y as f32 / SUBUNITS_PER_WORLD_UNIT as f32,
    )
}

pub(crate) fn world_to_sim_point(point: Vec3) -> SimPoint {
    let scale = SUBUNITS_PER_WORLD_UNIT as f32;
    SimPoint::new(
        (point.x * scale).round() as i32,
        (point.z * scale).round() as i32,
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

fn imported_unit_overhead_height(entry: &PresentedEntry, models: &UnitModelSet) -> Option<f32> {
    entry
        .imported_rawcode
        .and_then(|rawcode| models.get(rawcode))
        .and_then(|model| model.overhead_height)
}

fn unit_bar_overhead_height(
    unit: &UnitSample,
    entry: &PresentedEntry,
    models: &UnitModelSet,
) -> f32 {
    imported_unit_overhead_height(entry, models).unwrap_or_else(|| unit_height(unit))
}

fn unit_health_bar_width(unit: &UnitSample, entry: &PresentedEntry, models: &UnitModelSet) -> f32 {
    let collision_radius_world = unit.collision_radius as f32 / SUBUNITS_PER_WORLD_UNIT as f32;
    let collision_width = collision_radius_world * UNIT_HEALTH_BAR_COLLISION_SCALE;
    let model_width = imported_unit_overhead_height(entry, models)
        .map_or(0.0, |height| height * UNIT_HEALTH_BAR_HEIGHT_SCALE);
    collision_width
        .max(model_width)
        .max(UNIT_HEALTH_BAR_MIN_WIDTH)
}

fn unit_stun_height(unit: &UnitSample, render_map: &RenderMap, models: &UnitModelSet) -> f32 {
    render_map
        .units
        .get(&unit.id)
        .and_then(|entry| imported_unit_overhead_height(entry, models))
        .map_or_else(|| unit_height(unit) * 1.15, |height| height * 1.05)
}

fn imported_building_overhead_height(
    entry: &PresentedEntry,
    models: &BuildingModelSet,
) -> Option<f32> {
    entry
        .imported_rawcode
        .and_then(|rawcode| models.get(rawcode))
        .and_then(|model| model.overhead_height)
}

fn building_bar_overhead_height(
    building: &BuildingSample,
    entry: &PresentedEntry,
    models: &BuildingModelSet,
) -> f32 {
    imported_building_overhead_height(entry, models).unwrap_or_else(|| building_height(building))
}

fn building_health_bar_width(size: Vec2, overhead_height: f32) -> f32 {
    let footprint_width = size.x.max(size.y) * BUILDING_HEALTH_BAR_FOOTPRINT_SCALE;
    let model_width = overhead_height * BUILDING_HEALTH_BAR_HEIGHT_SCALE;
    footprint_width.max(model_width)
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
        ProjectileViewKind::Reflected { .. } => Color::srgb(0.92, 0.92, 1.0),
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
    use castle_fight_sim::{CorpseDefinitionId, NavCell, TerrainElevationMap};

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
    fn wc3_lighting_reduces_overexposed_sun_without_crushing_ambient_fill() {
        let default_exposure = Exposure::default().exposure();
        let calibrated_exposure = Exposure {
            ev100: WC3_SCENE_EXPOSURE_EV100,
        }
        .exposure();
        let default_ambient = AmbientLight::default().brightness;

        let old_shadow_fill = default_ambient * default_exposure;
        let new_shadow_fill = WC3_SCENE_AMBIENT_BRIGHTNESS * calibrated_exposure;
        assert!((new_shadow_fill / old_shadow_fill - 1.0).abs() < 0.05);

        let old_sunlit = (WC3_SCENE_DIRECTIONAL_ILLUMINANCE + default_ambient) * default_exposure;
        let new_sunlit = (WC3_SCENE_DIRECTIONAL_ILLUMINANCE + WC3_SCENE_AMBIENT_BRIGHTNESS)
            * calibrated_exposure;
        assert!(new_sunlit < old_sunlit * 0.25);
    }

    #[test]
    fn wc3_chain_lightning_uses_primary_width_only_for_first_jump() {
        assert_eq!(wc3_chain_lightning_width(0), 50.0);
        assert_eq!(wc3_chain_lightning_width(1), 30.0);
        assert_eq!(wc3_chain_lightning_width(7), 30.0);
    }

    #[test]
    fn wc3_chain_lightning_fades_smoothly_after_the_initial_flash() {
        assert_eq!(
            wc3_lightning_fade_strength(LIGHTNING_EFFECT_SECONDS, LIGHTNING_EFFECT_SECONDS),
            1.0
        );
        assert_eq!(
            wc3_lightning_fade_strength(LIGHTNING_EFFECT_SECONDS * 0.75, LIGHTNING_EFFECT_SECONDS),
            1.0
        );
        let halfway =
            wc3_lightning_fade_strength(LIGHTNING_EFFECT_SECONDS * 0.5, LIGHTNING_EFFECT_SECONDS);
        assert!(halfway > 0.0 && halfway < 1.0);
        assert_eq!(
            wc3_lightning_fade_strength(0.0, LIGHTNING_EFFECT_SECONDS),
            0.0
        );
    }

    #[test]
    fn wc3_chain_lightning_mesh_uses_stock_segment_length_and_width() {
        let mesh = build_wc3_chain_lightning_mesh(
            Vec3::ZERO,
            Vec3::new(250.0, 0.0, 0.0),
            0x1234_5678,
            WC3_CHAIN_LIGHTNING_PRIMARY_WIDTH,
        );
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();

        // ceil(250 / 100) = three stock-length textured segments, four vertices each.
        assert_eq!(positions.len(), 12);
        let first_left = Vec3::from_array(positions[0]);
        let first_right = Vec3::from_array(positions[1]);
        assert!((first_left.distance(first_right) - 50.0).abs() < 1.0e-4);
    }

    #[test]
    fn lightning_segment_seed_wraps_without_debug_overflow() {
        assert_eq!(
            lightning_segment_seed(2, 0x9e37_79b9),
            2u32.wrapping_mul(0x9e37_79b9)
        );
        assert_eq!(
            lightning_segment_seed(u32::MAX, 0x85eb_ca6b),
            u32::MAX.wrapping_mul(0x85eb_ca6b)
        );
    }

    #[test]
    fn fps_display_updates_only_after_each_sample_window() {
        let mut display = FpsDisplay::default();

        for _ in 0..4 {
            display.record_frame(0.1);
            assert_eq!(display.fps, None);
        }
        display.record_frame(0.1);
        assert!((display.fps.unwrap() - 10.0).abs() < 1.0e-5);

        display.record_frame(0.25);
        assert!((display.fps.unwrap() - 10.0).abs() < 1.0e-5);
        display.record_frame(0.25);
        assert!((display.fps.unwrap() - 4.0).abs() < 1.0e-5);
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
    fn authoritative_corpse_timing_maps_to_death_flesh_and_bone_phases() {
        let corpse = CorpseView {
            id: SimId(20),
            position: SimPoint::new(0, 0),
            source_unit: SimId(7),
            source_team: Team(0),
            definition: CorpseDefinitionId(u32::from_be_bytes(*b"n015")),
            created_tick: 100,
            expires_tick: Some(1_000),
        };

        assert_eq!(
            corpse_animation_phase(100, &corpse),
            Some(CorpseAnimationPhase {
                state: ImportedUnitAnimationState::Death,
                elapsed_ticks: 0,
                duration_ticks: 90,
            })
        );
        assert_eq!(
            corpse_animation_phase(190, &corpse),
            Some(CorpseAnimationPhase {
                state: ImportedUnitAnimationState::DecayFlesh,
                elapsed_ticks: 0,
                duration_ticks: 60,
            })
        );
        assert_eq!(
            corpse_animation_phase(250, &corpse),
            Some(CorpseAnimationPhase {
                state: ImportedUnitAnimationState::DecayBone,
                elapsed_ticks: 0,
                duration_ticks: 750,
            })
        );
        assert_eq!(
            corpse_animation_phase(999, &corpse),
            Some(CorpseAnimationPhase {
                state: ImportedUnitAnimationState::DecayBone,
                elapsed_ticks: 749,
                duration_ticks: 750,
            })
        );
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
        let position = projectile_position_at_progress(&projectile, start, target, 0.40, None);
        let tangent = projectile_position_at_progress(&projectile, start, target, 0.42, None);
        let rotation = projectile_rotation(position, tangent, true);
        assert!((rotation * Vec3::Z - Vec3::X).length() < 1e-5);
    }

    #[test]
    fn imported_wc3_projectile_models_map_positive_x_onto_client_forward() {
        let correction = Quat::from_rotation_y(WC3_PROJECTILE_FACING_OFFSET);
        assert!((correction * Vec3::X - Vec3::Z).length() < 1e-5);
    }

    #[test]
    fn imported_wc3_buildings_use_the_same_clockwise_facing_correction() {
        let correction = Quat::from_rotation_y(WC3_MODEL_FACING_OFFSET);
        assert!((correction * Vec3::X - Vec3::Z).length() < 1e-5);
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
        let rising = projectile_position_at_progress(&projectile, start, target, 0.25, None);
        let rising_next = projectile_position_at_progress(&projectile, start, target, 0.27, None);
        let falling = projectile_position_at_progress(&projectile, start, target, 0.75, None);
        let falling_next = projectile_position_at_progress(&projectile, start, target, 0.77, None);
        assert!(rising_next.y > rising.y);
        assert!(falling_next.y < falling.y);
        assert!((projectile_rotation(rising, rising_next, true) * Vec3::Z).y > 0.0);
        assert!((projectile_rotation(falling, falling_next, true) * Vec3::Z).y < 0.0);
    }

    #[test]
    fn extracted_catapult_arc_is_much_higher_than_placeholder_arc() {
        let start = Vec3::ZERO;
        let target = Vec3::new(1_000.0, 0.0, 0.0);
        let extracted = wc3_missile_arc_height(start, target, 0.4);
        assert!(extracted > DEFAULT_BALLISTIC_ARC_HEIGHT * 5.0);
        assert!(extracted < 250.0);
    }

    #[test]
    fn camera_focus_bounds_can_be_inset_from_navigation_bounds() {
        let config = SimulationConfig {
            navigation_cell_size: 32 * SUBUNITS_PER_WORLD_UNIT,
            navigation_min: NavCell::new(-200, -112),
            navigation_max: NavCell::new(199, 111),
            ..SimulationConfig::default()
        };
        let metrics = WorldMetrics::from_simulation_config(&config)
            .with_camera_focus_bounds_world(-5_888.0, -3_328.0, 5_888.0, 3_328.0);

        assert_eq!(
            metrics.clamp_focus(Vec3::new(-6_144.0, 50.0, 2_048.0)),
            Vec3::new(-5_888.0, 50.0, 2_048.0)
        );
        assert_eq!(
            metrics.clamp_focus(Vec3::new(6_144.0, 50.0, -3_500.0)),
            Vec3::new(5_888.0, 50.0, -3_328.0)
        );
    }

    #[test]
    fn production_progress_fills_toward_the_next_spawn_tick() {
        let mut building = BuildingSample {
            id: SimId(1),
            content: None,
            team: Team(0),
            footprint: BuildingFootprint::new(0, 0, 4, 4),
            health: 1_000,
            health_max: 1_000,
            construction_started_tick: None,
            construction_complete_tick: None,
            damage_type: None,
            armor: castle_fight_sim::ArmorProfile::UNARMORED,
            target: None,
            next_spawn_tick: Some(40),
            production_interval_ticks: Some(20),
            cooldown_remaining: None,
            mana_current: None,
            mana_maximum: None,
            ability_ready_tick: None,
            stunned_until_tick: None,
            visual_kind: BuildingVisualKind::Production,
        };

        assert_eq!(production_progress(&building, 20.0), Some(0.0));
        assert_eq!(production_progress(&building, 30.0), Some(0.5));
        assert_eq!(production_progress(&building, 40.0), Some(1.0));
        assert_eq!(production_progress(&building, 45.0), Some(1.0));

        building.production_interval_ticks = None;
        assert_eq!(production_progress(&building, 30.0), None);
    }

    #[test]
    fn construction_progress_uses_the_same_overhead_progress_bar() {
        let building = BuildingSample {
            id: SimId(1),
            content: None,
            team: Team(0),
            footprint: BuildingFootprint::new(0, 0, 4, 4),
            health: 1_000,
            health_max: 1_000,
            construction_started_tick: Some(20),
            construction_complete_tick: Some(80),
            damage_type: None,
            armor: castle_fight_sim::ArmorProfile::UNARMORED,
            target: None,
            next_spawn_tick: None,
            production_interval_ticks: None,
            cooldown_remaining: None,
            mana_current: None,
            mana_maximum: None,
            ability_ready_tick: None,
            stunned_until_tick: None,
            visual_kind: BuildingVisualKind::Production,
        };

        assert_eq!(building_progress(&building, 20.0), Some(0.0));
        assert_eq!(building_progress(&building, 50.0), Some(0.5));
        assert_eq!(building_progress(&building, 80.0), Some(1.0));
    }

    #[test]
    fn batched_bar_fill_and_background_do_not_overlap() {
        let viewport = Rect::new(0.0, 0.0, 1_000.0, 800.0);
        let layout = HealthBarScreenLayout {
            left: 100.0,
            top: 50.0,
            width: 200.0,
        };
        let mut rects = Vec::new();
        push_split_bar(
            &mut rects,
            layout,
            HEALTH_BAR_HEIGHT_PIXELS,
            0.25,
            Color::WHITE,
            Color::BLACK,
            viewport,
        );

        assert_eq!(rects.len(), 2);
        assert!((rects[0].max.x - rects[1].min.x).abs() < 1.0e-6);
        assert_eq!(rects[0].min.y, rects[1].min.y);
        assert_eq!(rects[0].max.y, rects[1].max.y);
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
