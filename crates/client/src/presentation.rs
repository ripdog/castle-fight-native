use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};

use bevy::{
    asset::{AssetEvent, AssetId, RenderAssetUsages},
    audio::SpatialListener,
    camera::{
        Exposure,
        primitives::{Frustum, Sphere},
        visibility::NoFrustumCulling,
    },
    ecs::{entity_disabling::Disabled, system::SystemParam},
    gltf::{Gltf, GltfMaterialExtras},
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
    world_serialization::{WorldAsset, WorldInstance, WorldInstanceSpawner},
};
use castle_fight_sim::{
    AbilityCastTarget, AbilityEffect, AttackDelivery, BuildingFootprint,
    CASTLE_FIGHT_SIMULATION_HZ, CorpseView, MovementClass, PassiveUnitEffect, PlayerId,
    ProjectileView, ProjectileViewKind, SUBUNITS_PER_WORLD_UNIT, SimId, SimPoint, SimulationConfig,
    Team,
};

use crate::{
    SelectedMatch, SimulationPlayback,
    bridge::{
        BuilderSample, BuildingSample, BuildingVisualKind, PresentationSamples, UnitSample,
        UnitVisualKind,
    },
    building_models::{BuildingAnimationClip, BuildingModelSet},
    control_modifier_pressed,
    particle_renderer::Wc3ParticleRenderPlugin,
    performance_ui::{
        begin_presentation_profile, finish_animation_profile, finish_camera_profile,
        finish_effects_profile, finish_entity_sync_profile, finish_model_prep_profile,
        finish_presentation_profile, finish_scene_setup_profile, finish_transform_profile,
    },
    render_audit::RenderExperiment,
    terrain::{TerrainSurface, TerrainTextureLayout, TerrainTextureSet},
    unit_models::{UnitAnimationClip, UnitAnimationSet, UnitModelAsset, UnitModelSet},
    wc3_effects::{
        Wc3AbilityVisualAnchor, Wc3AnimatedAlphaMaterial, Wc3AttachToNode, Wc3AttachmentOwner,
        Wc3ComposedMetadataCache, Wc3ConvertedModelRegistry, Wc3EffectReusePending,
        Wc3EffectWarmup, Wc3EmitterSource, Wc3MaterialProcessed, Wc3ModelSequenceSelection,
        Wc3ParticleAssets, Wc3RibbonSource, Wc3SplatMaterial, Wc3StatusVisualKind,
        Wc3TeamColorMaterial, Wc3TeamTint, Wc3VertexTint, Wc3VisualAnimationGraphs,
        Wc3VisualAnimationSource, Wc3VisualModel, Wc3VisualSet, advance_wc3_model_sequence_clocks,
        apply_wc3_non_inheritance, emit_wc3_model_particles, emit_wc3_particles,
        emit_wc3_sound_events, emit_wc3_spawn_events, emit_wc3_splat_events,
        fix_wc3_scene_materials, flush_wc3_particle_buffers, index_wc3_model_attachments,
        mark_wc3_effect_warmup_hierarchy, reset_reused_wc3_effect_instances,
        resolve_wc3_emitter_nodes, resolve_wc3_visual_attachments,
        setup_wc3_model_composed_features, setup_wc3_model_lights,
        setup_wc3_visual_animation_players, spawn_wc3_ribbon_trails, update_wc3_material_alpha,
        update_wc3_material_texture, update_wc3_model_attachments, update_wc3_model_lights,
        update_wc3_model_particles, update_wc3_particles, update_wc3_ribbon_trails,
        update_wc3_spawned_event_models, update_wc3_spawned_splats,
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
const WC3_PLAYER_COLOR_COUNT: usize = 12;

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
const MANA_BAR_HEIGHT_PIXELS: f32 = 6.0;
const MANA_BAR_GAP_PIXELS: f32 = 3.0;
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
const UNIT_FACING_SNAP_RADIANS: f32 = 0.002;
const MISS_INDICATOR_SECONDS: f32 = 1.0;
const MISS_INDICATOR_RISE_PIXELS: f32 = 34.0;
const FPS_DISPLAY_SAMPLE_SECONDS: f32 = 0.5;
pub(crate) const WC3_MODEL_FACING_OFFSET: f32 = -std::f32::consts::FRAC_PI_2;
const WC3_PROJECTILE_FACING_OFFSET: f32 = -std::f32::consts::FRAC_PI_2;
pub(crate) const WC3_BUILDING_AMBIENT_ANIMATION_SPEED: f32 = 0.5;
const CAMERA_EDGE_SCROLL_MARGIN: f32 = 8.0;
const CAMERA_PAN_SPEED_WORLD_PER_SECOND: f32 = 6_000.0;
const CAMERA_DEFAULT_DISTANCE_WORLD: f32 = 4_500.0;
const CAMERA_MIN_DISTANCE_FACTOR: f32 = 0.20;
const CAMERA_MAX_DISTANCE_WORLD: f32 = 8_000.0;
const SIDE_TERRAIN_MASK_COLOR: Color = Color::srgb(0.006, 0.009, 0.006);
const MAP_GRID_HEIGHT_OFFSET: f32 = 0.35;
const MAP_GRID_MAJOR_INTERVAL: i32 = 4;
const MAP_GRID_MAJOR_OFFSET_WORLD: f32 = 2.0;
const MAP_GRID_LINE_COLOR: Color = Color::srgba(0.62, 0.70, 0.64, 0.34);
const MAP_GRID_MAJOR_COLOR: Color = Color::srgba(0.70, 0.78, 0.68, 0.52);

#[derive(Resource, Debug, Clone)]
pub struct WorldMetrics {
    navigation_cell_size_subunits: i32,
    navigation_min: IVec2,
    navigation_max: IVec2,
    build_regions: Vec<BuildingFootprint>,
    team_build_regions: [Vec<BuildingFootprint>; 2],
    build_static_blockers: Vec<BuildingFootprint>,
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
        let navigation_footprint = || {
            BuildingFootprint::new(
                config.navigation_min.x,
                config.navigation_min.y,
                u16::try_from(config.navigation_max.x - config.navigation_min.x + 1)
                    .expect("navigation width must fit a building footprint"),
                u16::try_from(config.navigation_max.y - config.navigation_min.y + 1)
                    .expect("navigation height must fit a building footprint"),
            )
        };
        let team_build_regions = if config.team_build_regions.iter().any(Vec::is_empty) {
            let region = navigation_footprint();
            [vec![region], vec![region]]
        } else {
            config.team_build_regions.clone()
        };
        let build_regions =
            team_build_regions
                .iter()
                .flatten()
                .copied()
                .fold(Vec::new(), |mut regions, region| {
                    if !regions.contains(&region) {
                        regions.push(region);
                    }
                    regions
                });
        Self {
            navigation_cell_size_subunits: config.navigation_cell_size,
            navigation_min,
            navigation_max,
            build_regions,
            team_build_regions,
            build_static_blockers: config
                .static_blockers
                .iter()
                .chain(&config.build_static_blockers)
                .copied()
                .collect(),
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

    fn build_regions(&self) -> impl Iterator<Item = BuildingFootprint> + '_ {
        self.build_regions.iter().copied()
    }

    fn team_build_regions(&self, team: Team) -> impl Iterator<Item = BuildingFootprint> + '_ {
        self.team_build_regions
            .get(usize::from(team.0))
            .into_iter()
            .flatten()
            .copied()
    }

    fn build_region_world_bounds(&self, region: BuildingFootprint) -> (Vec2, Vec2) {
        let cell = self.navigation_cell_world();
        (
            Vec2::new(region.min_x as f32 * cell, region.min_y as f32 * cell),
            Vec2::new(
                (region.max_x() + 1) as f32 * cell,
                (region.max_y() + 1) as f32 * cell,
            ),
        )
    }

    fn buildable_world_bounds(&self) -> (Vec2, Vec2) {
        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        for region in self.build_regions() {
            let (region_min, region_max) = self.build_region_world_bounds(region);
            min = min.min(region_min);
            max = max.max(region_max);
        }
        debug_assert!(min.is_finite() && max.is_finite());
        (min, max)
    }

    fn buildable_world_x_bounds(&self) -> (f32, f32) {
        let (min, max) = self.buildable_world_bounds();
        (min.x, max.x)
    }

    fn placement_grid_anchor_cells(
        &self,
        region: BuildingFootprint,
        grid_size: u16,
    ) -> Option<IVec2> {
        if grid_size == 0 || region.width < grid_size || region.height < grid_size {
            return None;
        }
        let (build_min, build_max) = self.buildable_world_bounds();
        let build_center_x = (build_min.x + build_max.x) * 0.5;
        let (region_min, region_max) = self.build_region_world_bounds(region);
        let anchored_left = (region_min.x + region_max.x) * 0.5 <= build_center_x;
        let spacing = self.navigation_cell_world() * f32::from(grid_size);
        let grid_origin = (build_min + build_max) * 0.5 + Vec2::splat(spacing * 0.5);
        let (_, last_z) =
            map_grid_axis_line_bounds(region_min.y, region_max.y, spacing, grid_origin.y)?;
        let top_boundary_cell = ((grid_origin.y + last_z as f32 * spacing)
            / self.navigation_cell_world())
        .round() as i32;
        Some(IVec2::new(
            if anchored_left {
                region.min_x
            } else {
                region.max_x() + 1 - i32::from(grid_size)
            },
            top_boundary_cell - i32::from(grid_size),
        ))
    }

    fn map_grid_cell_unblocked(&self, world: Vec2) -> bool {
        let cell = IVec2::new(
            (world.x / self.navigation_cell_world()).floor() as i32,
            (world.y / self.navigation_cell_world()).floor() as i32,
        );
        !self.build_static_blockers.iter().any(|blocker| {
            cell.x >= blocker.min_x
                && cell.x <= blocker.max_x()
                && cell.y >= blocker.min_y
                && cell.y <= blocker.max_y()
        })
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

    /// Snaps a square building footprint to the independently anchored grid for its side.
    ///
    /// The left grid starts at the top-left build-region corner. The right grid starts one
    /// building width inside the top-right corner, so its footprint origins extend leftward.
    pub(crate) fn snapped_footprint_at_world(
        &self,
        world: Vec3,
        team: Team,
        size: u16,
        grid_size: u16,
    ) -> BuildingFootprint {
        let unsnapped = self.footprint_at_world(world, size, size);
        let Some(region) = self.team_build_regions(team).min_by_key(|region| {
            let center_x = i64::from(region.min_x) * 2 + i64::from(region.width);
            let center_y = i64::from(region.min_y) * 2 + i64::from(region.height);
            let world_cell_x = (world.x / self.navigation_cell_world()).floor() as i64;
            let world_cell_y = (world.z / self.navigation_cell_world()).floor() as i64;
            (world_cell_x * 2 - center_x).pow(2) + (world_cell_y * 2 - center_y).pow(2)
        }) else {
            return unsnapped;
        };
        let Some(anchor) = self.placement_grid_anchor_cells(region, grid_size) else {
            return unsnapped;
        };
        let step = i32::from(grid_size);
        BuildingFootprint::new(
            snap_cell_to_lattice(unsnapped.min_x, anchor.x, step),
            snap_cell_to_lattice(unsnapped.min_y, anchor.y, step),
            size,
            size,
        )
    }
}

fn snap_cell_to_lattice(cell: i32, anchor: i32, step: i32) -> i32 {
    let delta = cell - anchor;
    anchor + (delta + step.div_euclid(2)).div_euclid(step) * step
}

/// Presentation elevation for a flat building model covering `footprint`.
///
/// WC3 building models are rigid even when the terrain under their pathing footprint spans a
/// ramp. Sampling only the footprint centre can therefore bury the low geosets under a higher
/// edge of the terrain. Use the highest presentation-height sample on the navigation-cell grid
/// covered by the footprint so both placement ghosts and completed buildings remain entirely
/// above the rendered ground.
pub(crate) fn building_terrain_height(
    metrics: &WorldMetrics,
    terrain: &TerrainSurface,
    footprint: BuildingFootprint,
) -> f32 {
    let cell = metrics.navigation_cell_world();
    let max_x = footprint.max_x().saturating_add(1);
    let max_y = footprint.max_y().saturating_add(1);
    let mut maximum = f32::NEG_INFINITY;
    for cell_y in footprint.min_y..=max_y {
        for cell_x in footprint.min_x..=max_x {
            maximum = maximum.max(
                terrain.height_at_world(Vec2::new(cell_x as f32 * cell, cell_y as f32 * cell)),
            );
        }
    }
    maximum
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
    unit_materials: [Handle<StandardMaterial>; WC3_PLAYER_COLOR_COUNT],
    building_materials: [Handle<StandardMaterial>; WC3_PLAYER_COLOR_COUNT],
    building_accent_materials: [Handle<StandardMaterial>; WC3_PLAYER_COLOR_COUNT],
    unit_accent_materials: [Handle<StandardMaterial>; WC3_PLAYER_COLOR_COUNT],
    corpse_materials: [Handle<StandardMaterial>; WC3_PLAYER_COLOR_COUNT],
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

    fn unit_material(&self, owner: PlayerId) -> Handle<StandardMaterial> {
        self.unit_materials
            .get(usize::from(owner.0))
            .cloned()
            .unwrap_or_else(|| self.neutral_unit_material.clone())
    }

    fn unit_accent_material(&self, owner: PlayerId) -> Handle<StandardMaterial> {
        self.unit_accent_materials
            .get(usize::from(owner.0))
            .cloned()
            .unwrap_or_else(|| self.neutral_unit_accent_material.clone())
    }

    fn building_material(&self, owner: Option<PlayerId>) -> Handle<StandardMaterial> {
        owner
            .and_then(|owner| self.building_materials.get(usize::from(owner.0)))
            .cloned()
            .unwrap_or_else(|| self.neutral_building_material.clone())
    }

    fn building_accent_material(&self, owner: Option<PlayerId>) -> Handle<StandardMaterial> {
        owner
            .and_then(|owner| self.building_accent_materials.get(usize::from(owner.0)))
            .cloned()
            .unwrap_or_else(|| self.neutral_building_accent_material.clone())
    }

    fn corpse_material(&self, owner: PlayerId) -> Handle<StandardMaterial> {
        self.corpse_materials
            .get(usize::from(owner.0))
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
    constructing: bool,
}

#[derive(Debug, Clone, Copy)]
struct PresentedProjectile {
    entity: Entity,
    last_position: Vec3,
    missile_arc: Option<f32>,
    pooled_visual: Option<(Entity, Wc3EffectPoolKey)>,
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
    status_effects: HashMap<StatusEffectKey, PresentedStatusEffect>,
}

#[derive(Debug, Clone, Copy)]
struct DeathRemnant {
    position: Vec3,
    owner: Option<PlayerId>,
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
    pooled_lightning: bool,
    pooled_scene: Option<Wc3EffectPoolKey>,
}

#[derive(Debug, Clone)]
struct PooledLightningEffect {
    entity: Entity,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

#[derive(Resource, Default)]
struct TimedWc3Effects(Vec<TimedWc3Effect>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Wc3EffectPlayback {
    OneShot,
    Looping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Wc3EffectPoolKey {
    scene: AssetId<WorldAsset>,
    playback: Wc3EffectPlayback,
}

impl Wc3EffectPlayback {
    fn key(self, model: &Wc3VisualModel) -> Wc3EffectPoolKey {
        Wc3EffectPoolKey {
            scene: model.pool_key(),
            playback: self,
        }
    }

    fn emitter_source(self, model: &Wc3VisualModel) -> Wc3EmitterSource {
        match self {
            Self::OneShot => model.emitter_source(),
            Self::Looping => model.looping_emitter_source(),
        }
    }

    fn animation_source(self, model: &Wc3VisualModel) -> Option<Wc3VisualAnimationSource> {
        match self {
            Self::OneShot => model.animation_source(),
            Self::Looping => model.looping_animation_source(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PresentedStatusEffect {
    entity: Entity,
    pooled_scene: Option<Wc3EffectPoolKey>,
}

#[derive(Resource, Default)]
struct TimedWc3EffectPool {
    lightning: Vec<PooledLightningEffect>,
    scenes: HashMap<Wc3EffectPoolKey, Vec<Entity>>,
}

struct Wc3EffectPrewarmRequest {
    model: Wc3VisualModel,
    playback: Wc3EffectPlayback,
    remaining: usize,
    capacity: usize,
    activation_capacity: usize,
    desired_capacity: usize,
}

#[derive(Resource, Default)]
pub(crate) struct Wc3EffectPrewarmPlan {
    populations: BTreeMap<u32, usize>,
    requests: Vec<Wc3EffectPrewarmRequest>,
    prepared_total: usize,
}

impl Wc3EffectPrewarmPlan {
    pub(crate) fn format(&self) -> String {
        format!(
            "  effect reserves: {} templates, {} planned roots, {} still awaiting creation, {} prepared in total\n",
            self.requests.len(),
            self.requests
                .iter()
                .map(|request| request.capacity)
                .sum::<usize>(),
            self.requests
                .iter()
                .map(|request| request.remaining)
                .sum::<usize>(),
            self.prepared_total
        )
    }
}

#[derive(Component)]
struct PrewarmingWc3Effect {
    key: Wc3EffectPoolKey,
    needs_animation: bool,
}

// Renderer resource budgets, independent of Castle Fight content/tuning.
const EFFECT_PREWARM_ROOTS_PER_FRAME: usize = 8;
const EFFECT_PREWARM_ROOTS_PER_TEMPLATE: usize = 128;
const EFFECT_PREWARM_TOTAL_ROOTS: usize = 1024;

#[derive(Resource)]
pub(crate) struct DebugPresentation {
    pub(crate) overlays: bool,
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

#[derive(Resource, Default)]
pub(crate) struct CameraFocusRequest(pub(crate) Option<SimId>);

#[derive(Resource)]
pub(crate) struct BenchmarkCamera {
    pub(crate) lock_input: bool,
}

#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct ProfileVisualStress {
    pub(crate) rawcode: u32,
    pub(crate) count: usize,
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
    rawcode: u32,
    presentation_root: Entity,
    model_root: Entity,
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
    critical: bool,
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
    material: Handle<HealthBarMaterial>,
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

type HealthBarCameraData = (&'static Camera, &'static Transform, &'static Frustum);
type HealthBarCameraFilter = (With<Camera3d>, Without<HealthBarBatchEntity>);

#[derive(SystemParam)]
struct HealthBarRenderParams<'w, 's> {
    debug: Res<'w, DebugPresentation>,
    camera: Single<'w, 's, HealthBarCameraData, HealthBarCameraFilter>,
    batch: ResMut<'w, HealthBarBatch>,
    health_bar_materials: ResMut<'w, Assets<HealthBarMaterial>>,
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

#[derive(Default, Reflect, GizmoConfigGroup)]
struct MapGridGizmos;

#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MapGridState {
    pub(crate) enabled: bool,
}

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BuildingGridSnapState {
    pub(crate) enabled: bool,
}

impl Default for BuildingGridSnapState {
    fn default() -> Self {
        Self { enabled: true }
    }
}

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
            .add_plugins(MaterialPlugin::<Wc3AnimatedAlphaMaterial>::default())
            .add_plugins(Wc3ParticleRenderPlugin)
            .add_plugins(MaterialPlugin::<Wc3SplatMaterial>::default())
            .add_plugins(MaterialPlugin::<Wc3TeamColorMaterial>::default())
            .init_resource::<RenderMap>()
            .init_resource::<UnitModelSet>()
            .init_resource::<BuildingModelSet>()
            .init_resource::<Wc3VisualSet>()
            .init_resource::<Wc3ConvertedModelRegistry>()
            .init_resource::<Wc3ComposedMetadataCache>()
            .init_resource::<Wc3VisualAnimationGraphs>()
            .init_resource::<FpsDisplay>()
            .init_resource::<MapGridState>()
            .init_resource::<BuildingGridSnapState>()
            .init_resource::<CameraFocusRequest>()
            .init_resource::<DeathRemnants>()
            .init_resource::<ProjectileImpacts>()
            .init_resource::<AbilityAreaImpacts>()
            .init_resource::<TimedWc3Effects>()
            .init_resource::<TimedWc3EffectPool>()
            .init_resource::<Wc3EffectPrewarmPlan>()
            .init_gizmo_group::<ProjectileEffectGizmos>()
            .init_gizmo_group::<MapGridGizmos>()
            .add_observer(index_wc3_model_attachments)
            .add_observer(mark_wc3_effect_warmup_hierarchy)
            .insert_resource(DebugPresentation {
                health_bars: self.health_bars,
                ..default()
            })
            .add_systems(Startup, setup_scene)
            .add_systems(
                Update,
                (
                    toggle_debug_controls,
                    begin_presentation_profile,
                    update_camera,
                    finish_camera_profile,
                    prepare_unit_model_animations,
                    prepare_building_model_animations,
                    (invalidate_wc3_effect_pool_assets, prewarm_timed_wc3_effects).chain(),
                    finish_model_prep_profile,
                    sync_render_entities,
                    finish_entity_sync_profile,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    (reset_reused_wc3_effect_instances, fix_wc3_scene_materials).chain(),
                    (setup_wc3_model_lights, setup_wc3_model_composed_features),
                    resolve_wc3_visual_attachments,
                    resolve_wc3_emitter_nodes,
                    (
                        setup_wc3_visual_animation_players,
                        retain_prewarmed_wc3_effects,
                    )
                        .chain(),
                    setup_imported_unit_animation_players,
                    setup_imported_building_animation_players,
                    finish_scene_setup_profile,
                    update_imported_building_animations,
                    trigger_attack_animations,
                    update_imported_unit_animations,
                    advance_wc3_model_sequence_clocks,
                    (
                        emit_wc3_spawn_events,
                        emit_wc3_sound_events,
                        emit_wc3_splat_events,
                        update_wc3_model_attachments,
                    ),
                    update_wc3_material_alpha,
                    update_wc3_material_texture,
                    update_wc3_model_lights,
                    spawn_miss_indicators,
                    finish_animation_profile,
                    interpolate_render_transforms,
                    finish_transform_profile,
                )
                    .chain()
                    .after(finish_entity_sync_profile),
            )
            .add_systems(
                PostUpdate,
                apply_wc3_non_inheritance.after(bevy::transform::TransformSystems::Propagate),
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
                    (
                        update_wc3_model_particles,
                        update_wc3_spawned_event_models,
                        update_wc3_spawned_splats,
                    ),
                    update_wc3_particles,
                    emit_wc3_model_particles,
                    (emit_wc3_particles, flush_wc3_particle_buffers).chain(),
                    finish_effects_profile,
                    draw_projectile_effects,
                    update_health_bar_batch,
                    draw_map_grid,
                    draw_presentation_gizmos,
                    sample_display_fps,
                    finish_presentation_profile,
                )
                    .chain()
                    .after(finish_transform_profile),
            );
    }
}

fn initial_camera_focus(
    local_player: PlayerId,
    samples: &PresentationSamples,
    metrics: &WorldMetrics,
    terrain: &TerrainSurface,
    map_center: bool,
) -> Vec3 {
    if map_center {
        let mut center = metrics.clamp_focus(metrics.world_center());
        center.y = terrain.height_at_world(center.xz());
        return center;
    }
    let focus = samples
        .current
        .builders
        .values()
        .find(|builder| builder.owner == local_player)
        .map_or_else(
            || {
                let mut center = metrics.world_center();
                center.y = terrain.height_at_world(center.xz());
                center
            },
            |builder| sim_point_to_terrain_world(builder.position, terrain),
        );
    metrics.clamp_focus(focus)
}

#[derive(SystemParam)]
struct SceneWorldResources<'w> {
    metrics: Res<'w, WorldMetrics>,
    terrain: Res<'w, TerrainSurface>,
    terrain_texture_layout: Res<'w, TerrainTextureLayout>,
    terrain_textures: Res<'w, TerrainTextureSet>,
    benchmark_camera: Option<Res<'w, BenchmarkCamera>>,
    visual_stress: Option<Res<'w, ProfileVisualStress>>,
}

fn setup_scene(
    mut commands: Commands,
    model_sets: (
        ResMut<UnitModelSet>,
        ResMut<BuildingModelSet>,
        ResMut<Wc3VisualSet>,
        ResMut<Wc3ConvertedModelRegistry>,
    ),
    selected_match: Res<SelectedMatch>,
    samples: Res<PresentationSamples>,
    world: SceneWorldResources<'_>,
    assets: SceneAssetResources<'_>,
    mut gizmo_configs: ResMut<GizmoConfigStore>,
) {
    let (mut unit_models, mut building_models, mut wc3_visuals, mut converted_models) = model_sets;
    let SceneWorldResources {
        metrics,
        terrain,
        terrain_texture_layout,
        terrain_textures,
        benchmark_camera,
        visual_stress,
    } = world;
    let SceneAssetResources {
        asset_server,
        mut meshes,
        mut materials,
        mut health_bar_materials,
        mut shader_buffers,
    } = assets;
    let mut selected_unit_models = selected_match
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
    if let Some(stress) = visual_stress.as_ref()
        && !selected_unit_models.contains(&stress.rawcode)
    {
        selected_unit_models.push(stress.rawcode);
    }
    *unit_models = UnitModelSet::load_selected(&asset_server, &selected_unit_models);
    if let Some(stress) = visual_stress.as_ref() {
        spawn_profile_visual_stress(
            &mut commands,
            stress.rawcode,
            stress.count,
            &unit_models,
            &terrain,
        );
    }
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
    *converted_models = Wc3ConvertedModelRegistry::load_default();

    let health_bar_buffer_data = vec![[0.0; 4]; 1 + HEALTH_BAR_BATCH_MIN_BUFFER_RECTS * 2];
    let health_bar_buffer = shader_buffers.add(ShaderBuffer::from(health_bar_buffer_data.clone()));
    let health_bar_material = health_bar_materials.add(HealthBarMaterial {
        rect_data: health_bar_buffer.clone(),
    });
    let health_bar_mesh = meshes.add(health_bar_batch_mesh());
    commands.spawn((
        Mesh3d(health_bar_mesh),
        MeshMaterial3d(health_bar_material.clone()),
        Transform::default(),
        Visibility::Hidden,
        NoFrustumCulling,
        HealthBarBatchEntity,
    ));
    commands.insert_resource(HealthBarBatch {
        buffer: health_bar_buffer,
        material: health_bar_material,
        rects: Vec::with_capacity(256),
        gpu_data: health_bar_buffer_data,
        gpu_capacity_rects: HEALTH_BAR_BATCH_MIN_BUFFER_RECTS,
        last_rect_count: 0,
    });

    let (projectile_effect_config, _) = gizmo_configs.config_mut::<ProjectileEffectGizmos>();
    projectile_effect_config.line.width = 3.0;
    let (map_grid_config, _) = gizmo_configs.config_mut::<MapGridGizmos>();
    map_grid_config.line.width = 1.0;
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

    let unit_materials = std::array::from_fn(|index| {
        let player = PlayerId(u8::try_from(index).expect("WC3 player material index fits u8"));
        materials.add(StandardMaterial {
            base_color: player_color(player),
            perceptual_roughness: 0.72,
            ..default()
        })
    });
    let building_materials = std::array::from_fn(|index| {
        let player = PlayerId(u8::try_from(index).expect("WC3 player material index fits u8"));
        materials.add(StandardMaterial {
            base_color: player_color(player),
            perceptual_roughness: 0.86,
            ..default()
        })
    });
    let building_accent_materials = std::array::from_fn(|index| {
        let player = PlayerId(u8::try_from(index).expect("WC3 player material index fits u8"));
        let color = player_color(player);
        materials.add(StandardMaterial {
            base_color: color,
            emissive: color.to_linear() * 0.16,
            perceptual_roughness: 0.52,
            ..default()
        })
    });
    let unit_accent_materials = std::array::from_fn(|index| {
        let player = PlayerId(u8::try_from(index).expect("WC3 player material index fits u8"));
        let color = player_color(player);
        materials.add(StandardMaterial {
            base_color: color,
            emissive: color.to_linear() * 0.20,
            metallic: 0.15,
            perceptual_roughness: 0.35,
            ..default()
        })
    });
    let corpse_materials = std::array::from_fn(|index| {
        let player = PlayerId(u8::try_from(index).expect("WC3 player material index fits u8"));
        materials.add(StandardMaterial {
            base_color: player_color(player).with_alpha(0.42),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        })
    });
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

    let initial_camera_focus = initial_camera_focus(
        selected_match.local_player,
        &samples,
        &metrics,
        &terrain,
        benchmark_camera.is_some(),
    );
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

    // The retained Warcraft terrain extends well beyond the authored playable rectangle on the
    // west/east sides. WC3 presents those side bands as almost-black dead space. Keep the actual
    // terrain geometry underneath for camera/background continuity, but depth-occlude its diffuse
    // layers with a conforming unlit mask derived from the authoritative build-region bounds.
    let (buildable_min_x, buildable_max_x) = metrics.buildable_world_x_bounds();
    commands.spawn((
        Mesh3d(meshes.add(terrain.side_mask_mesh(buildable_min_x, buildable_max_x))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: SIDE_TERRAIN_MASK_COLOR,
            unlit: true,
            ..default()
        })),
        Transform::IDENTITY,
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: WC3_SCENE_DIRECTIONAL_ILLUMINANCE,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.85, -0.75, 0.0)),
    ));

    let distance = CAMERA_DEFAULT_DISTANCE_WORLD;
    let rig = RtsCamera {
        focus: initial_camera_focus,
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
        SpatialListener::default(),
        camera_transform(&rig),
        rig,
    ));
}

fn spawn_profile_visual_stress(
    commands: &mut Commands,
    rawcode: u32,
    count: usize,
    unit_models: &UnitModelSet,
    terrain: &TerrainSurface,
) {
    const COLUMNS: usize = 40;
    const SPACING_WORLD: f32 = 96.0;

    let model = unit_models.get(rawcode).unwrap_or_else(|| {
        panic!("profiling visual rawcode {rawcode:#010x} is not in the generated unit manifest")
    });
    let rows = count.div_ceil(COLUMNS).max(1);
    for index in 0..count {
        let column = index % COLUMNS;
        let row = index / COLUMNS;
        let x = (column as f32 - (COLUMNS.saturating_sub(1)) as f32 * 0.5) * SPACING_WORLD;
        let z = (row as f32 - (rows.saturating_sub(1)) as f32 * 0.5) * SPACING_WORLD;
        let y = terrain.height_at_world(Vec2::new(x, z));
        let entity = commands
            .spawn((
                WorldAssetRoot(model.scene.clone()),
                Transform {
                    translation: Vec3::new(x, y, z),
                    rotation: Quat::from_rotation_y(WC3_MODEL_FACING_OFFSET),
                    scale: Vec3::splat(model.scale),
                },
            ))
            .id();
        if let Some(tint) = model.tint_rgb {
            commands.entity(entity).insert(Wc3VertexTint(tint));
        }
    }
    println!("spawned {count} profiling WC3 visual scene(s) for rawcode {rawcode:#010x}");
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
    owner: PlayerId,
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
                    MeshMaterial3d(assets.unit_accent_material(owner)),
                    Transform::from_xyz(0.0, 3.0, 4.0),
                ));
            }
        });
        if visual_kind.is_caster() {
            let caster_material = assets.unit_accent_material(owner);
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

    let material = assets.unit_accent_material(unit.owner);
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
        let Some((model_root, root)) = imported_model_root(entity, &parents, &roots) else {
            continue;
        };
        let Some(animations) = unit_models.animations(root.rawcode) else {
            continue;
        };

        let mut transitions = AnimationTransitions::new();
        transitions
            .play(&mut player, animations.stand, Duration::ZERO)
            .repeat();
        set_unit_emitter_sequence(
            &mut commands,
            &unit_models,
            model_root,
            root.rawcode,
            Some(&animations.sequences.stand),
        );
        commands.entity(entity).insert((
            AnimationGraphHandle(animations.graph.clone()),
            transitions,
            ImportedUnitAnimationController {
                sim_id: root.sim_id,
                rawcode: root.rawcode,
                presentation_root: root.presentation_root,
                model_root,
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
        commands
            .entity(model_root)
            .remove::<Wc3EmitterSource>()
            .remove::<Wc3RibbonSource>()
            .remove::<Wc3ModelSequenceSelection>();
        return;
    };
    commands.entity(model_root).insert((
        Wc3EmitterSource::with_asset_prefix_for_sequence(
            &model.emitters,
            "wc3/buildings",
            sequence,
        ),
        Wc3ModelSequenceSelection::new(sequence),
    ));
    if !model.ribbons.is_empty() {
        commands
            .entity(model_root)
            .insert(Wc3RibbonSource::with_asset_prefix_for_sequence(
                &model.ribbons,
                "wc3/buildings",
                sequence,
            ));
    }
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

fn building_presentation_needs_rebuild(
    imported_rawcode: Option<u32>,
    was_constructing: bool,
    desired_rawcode: Option<u32>,
    constructing: bool,
) -> bool {
    imported_rawcode != desired_rawcode
        || (imported_rawcode.is_some() && was_constructing && !constructing)
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
) -> Option<(Entity, ImportedUnitModelRoot)> {
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

fn unit_animation_sequence_name(
    animations: &UnitAnimationSet,
    state: ImportedUnitAnimationState,
    defend_active: bool,
) -> Option<&str> {
    let names = &animations.sequences;
    match state {
        ImportedUnitAnimationState::Stand if defend_active => {
            names.defend_stand.as_deref().or(Some(names.stand.as_str()))
        }
        ImportedUnitAnimationState::Walk if defend_active => names
            .defend_walk
            .as_deref()
            .or(names.defend_stand.as_deref())
            .or(names.walk.as_deref())
            .or(Some(names.stand.as_str())),
        ImportedUnitAnimationState::Attack if defend_active => {
            names.defend_attack.as_deref().or(names.attack.as_deref())
        }
        ImportedUnitAnimationState::Stand => Some(names.stand.as_str()),
        ImportedUnitAnimationState::Walk => names.walk.as_deref().or(Some(names.stand.as_str())),
        ImportedUnitAnimationState::Attack => names.attack.as_deref(),
        ImportedUnitAnimationState::Cast => names.cast.as_deref(),
        ImportedUnitAnimationState::Death => names.death.as_deref(),
        ImportedUnitAnimationState::DecayFlesh => names
            .decay_flesh
            .as_deref()
            .or(names.decay_bone.as_deref())
            .or(names.death.as_deref()),
        ImportedUnitAnimationState::DecayBone => names
            .decay_bone
            .as_deref()
            .or(names.decay_flesh.as_deref())
            .or(names.death.as_deref()),
    }
}

fn set_unit_emitter_sequence(
    commands: &mut Commands,
    unit_models: &UnitModelSet,
    model_root: Entity,
    rawcode: u32,
    sequence: Option<&str>,
) {
    let Some(model) = unit_models.get(rawcode) else {
        return;
    };
    let Some(sequence) = sequence else {
        commands
            .entity(model_root)
            .remove::<Wc3EmitterSource>()
            .remove::<Wc3RibbonSource>()
            .remove::<Wc3ModelSequenceSelection>();
        return;
    };
    commands.entity(model_root).insert((
        Wc3EmitterSource::with_asset_prefix_for_sequence(
            &model.particle_emitters,
            "wc3/units",
            sequence,
        ),
        Wc3ModelSequenceSelection::new(sequence),
    ));
    if !model.ribbon_emitters.is_empty() {
        commands
            .entity(model_root)
            .insert(Wc3RibbonSource::with_asset_prefix_for_sequence(
                &model.ribbon_emitters,
                "wc3/units",
                sequence,
            ));
    }
}

fn sync_unit_emitter_sequence(
    commands: &mut Commands,
    unit_models: &UnitModelSet,
    controller: &ImportedUnitAnimationController,
) {
    let sequence = unit_models
        .animations(controller.rawcode)
        .and_then(|animations| {
            unit_animation_sequence_name(animations, controller.state, controller.defend_active)
        });
    set_unit_emitter_sequence(
        commands,
        unit_models,
        controller.model_root,
        controller.rawcode,
        sequence,
    );
}

fn sync_unit_emitter_if_animation_changed(
    commands: &mut Commands,
    unit_models: &UnitModelSet,
    controller: &ImportedUnitAnimationController,
    previous_state: ImportedUnitAnimationState,
    previous_defend_active: bool,
) {
    if controller.state != previous_state || controller.defend_active != previous_defend_active {
        sync_unit_emitter_sequence(commands, unit_models, controller);
    }
}

fn update_imported_unit_animations(
    mut commands: Commands,
    unit_models: Res<UnitModelSet>,
    samples: Res<PresentationSamples>,
    dying_roots: Query<(), With<ImportedDeathRemnant>>,
    mut players: Query<(
        &mut AnimationPlayer,
        &mut AnimationTransitions,
        &mut ImportedUnitAnimationController,
    )>,
) {
    for (mut player, mut transitions, mut controller) in &mut players {
        let previous_state = controller.state;
        let previous_defend_active = controller.defend_active;
        if let Some(current) = samples.current.builders.get(&controller.sim_id) {
            update_live_imported_builder_animation(
                &samples,
                current,
                &mut player,
                &mut transitions,
                &mut controller,
            );
            sync_unit_emitter_if_animation_changed(
                &mut commands,
                &unit_models,
                &controller,
                previous_state,
                previous_defend_active,
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
            sync_unit_emitter_if_animation_changed(
                &mut commands,
                &unit_models,
                &controller,
                previous_state,
                previous_defend_active,
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
            sync_unit_emitter_if_animation_changed(
                &mut commands,
                &unit_models,
                &controller,
                previous_state,
                previous_defend_active,
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
            sync_unit_emitter_if_animation_changed(
                &mut commands,
                &unit_models,
                &controller,
                previous_state,
                previous_defend_active,
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
    seek_seconds: f32,
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
    animation.set_seek_time(playback.seek_seconds).pause();
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
        seek_seconds: corpse_animation_seek_seconds(phase, clip.duration_seconds),
    })
}

fn corpse_animation_seek_seconds(phase: CorpseAnimationPhase, clip_duration_seconds: f32) -> f32 {
    if phase.state == ImportedUnitAnimationState::Death {
        return (phase.elapsed_ticks as f32 / CASTLE_FIGHT_SIMULATION_HZ as f32)
            .min(clip_duration_seconds);
    }
    let normalized = phase.elapsed_ticks as f32 / phase.duration_ticks.max(1) as f32;
    clip_duration_seconds * normalized.clamp(0.0, 1.0)
}

fn corpse_animation_phase(current_tick: u64, corpse: &CorpseView) -> Option<CorpseAnimationPhase> {
    let death_ticks = corpse
        .decay_start_tick
        .saturating_sub(corpse.created_tick)
        .max(1);
    let age = current_tick.saturating_sub(corpse.created_tick);

    if current_tick < corpse.decay_start_tick {
        return Some(CorpseAnimationPhase {
            state: ImportedUnitAnimationState::Death,
            elapsed_ticks: age,
            duration_ticks: death_ticks,
        });
    }

    let flesh_age = current_tick.saturating_sub(corpse.decay_start_tick);
    if flesh_age < FLESH_DECAY_TICKS {
        return Some(CorpseAnimationPhase {
            state: ImportedUnitAnimationState::DecayFlesh,
            elapsed_ticks: flesh_age,
            duration_ticks: FLESH_DECAY_TICKS,
        });
    }

    Some(CorpseAnimationPhase {
        state: ImportedUnitAnimationState::DecayBone,
        elapsed_ticks: flesh_age.saturating_sub(FLESH_DECAY_TICKS),
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
        .filter(|attack| attack.missed || attack.critical)
    {
        let critical = attack.critical;
        commands.spawn((
            Text::new(if critical { "CRIT!" } else { "MISS" }),
            TextFont::from_font_size(24.0),
            TextColor(if critical {
                Color::srgb(1.0, 0.30, 0.10)
            } else {
                Color::srgb(1.0, 0.88, 0.20)
            }),
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
                critical,
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
        color.0 = if indicator.critical {
            Color::srgba(1.0, 0.30, 0.10, life.min(0.95))
        } else {
            Color::srgba(1.0, 0.88, 0.20, life.min(0.95))
        };
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
    let body_material = assets.building_material(building.owner);
    let accent_material = assets.building_accent_material(building.owner);
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
    Option<Res<'w, RenderExperiment>>,
);

type SyncRenderEffects<'w> = (
    ResMut<'w, DeathRemnants>,
    ResMut<'w, ProjectileImpacts>,
    ResMut<'w, AbilityAreaImpacts>,
    ResMut<'w, TimedWc3Effects>,
    ResMut<'w, TimedWc3EffectPool>,
    ResMut<'w, Assets<Mesh>>,
    ResMut<'w, Assets<StandardMaterial>>,
);

fn spawn_persistent_unit_attachments(
    commands: &mut Commands,
    model_root: Entity,
    model: &UnitModelAsset,
    visuals: &Wc3VisualSet,
) {
    for attachment in &model.attached_visuals {
        let Ok(rawcode) = <[u8; 4]>::try_from(attachment.ability_rawcode.as_bytes()) else {
            continue;
        };
        for visual in visuals.ability(u32::from_be_bytes(rawcode)) {
            if visual.anchor != Wc3AbilityVisualAnchor::Target {
                continue;
            }
            let effect = commands
                .spawn((
                    WorldAssetRoot(visual.model.scene.clone()),
                    Transform::IDENTITY,
                    visual.model.looping_emitter_source(),
                    Wc3RibbonSource::new(&visual.model.ribbons),
                    Wc3VertexTint([255; 3]),
                    Wc3AttachToNode {
                        owner_root: model_root,
                        attachment_point: attachment.attachment_point.clone(),
                    },
                ))
                .id();
            if let Some(animation) = visual.model.looping_animation_source() {
                commands.entity(effect).insert(animation);
            }
            commands.entity(model_root).add_child(effect);
        }
    }
}

fn invalidate_wc3_effect_pool_assets(
    mut commands: Commands,
    mut events: MessageReader<AssetEvent<WorldAsset>>,
    mut pool: ResMut<TimedWc3EffectPool>,
    mut plan: ResMut<Wc3EffectPrewarmPlan>,
    pending: Query<(Entity, &PrewarmingWc3Effect)>,
) {
    for event in events.read() {
        let (AssetEvent::Modified { id } | AssetEvent::Removed { id }) = event else {
            continue;
        };
        pool.scenes.retain(|key, entities| {
            if key.scene != *id {
                return true;
            }
            for &entity in entities.iter() {
                commands.entity(entity).try_despawn();
            }
            false
        });
        for (entity, warmup) in &pending {
            if warmup.key.scene == *id {
                commands.entity(entity).try_despawn();
            }
        }
        for request in &mut plan.requests {
            if request.model.pool_key() == *id {
                request.remaining = request.capacity;
            }
        }
    }
}

fn prewarm_timed_wc3_effects(
    mut commands: Commands,
    selected: Res<SelectedMatch>,
    samples: Res<PresentationSamples>,
    visuals: Res<Wc3VisualSet>,
    asset_server: Res<AssetServer>,
    experiment: Res<RenderExperiment>,
    mut plan: ResMut<Wc3EffectPrewarmPlan>,
) {
    if matches!(
        *experiment,
        RenderExperiment::LegacyEffectPooling | RenderExperiment::ColdEffectPools
    ) {
        return;
    }
    if samples.is_changed() {
        let mut populations = BTreeMap::<u32, usize>::new();
        for unit in samples.current.units.values() {
            if let Some(content) = unit.content {
                *populations.entry(content.rawcode).or_default() += 1;
            }
        }
        let mut changed = false;
        for (rawcode, population) in populations {
            let previous = plan.populations.entry(rawcode).or_default();
            if population > *previous {
                *previous = population;
                changed = true;
            }
        }
        if changed {
            let mut capacities = HashMap::<Wc3EffectPoolKey, (Wc3VisualModel, usize, usize)>::new();
            for definition in selected.content.unit_definitions() {
                let Some(&population) = plan.populations.get(&definition.rawcode) else {
                    continue;
                };
                let mut reserve = |model: &Wc3VisualModel,
                                   playback: Wc3EffectPlayback,
                                   lifetime: f32,
                                   interval_ticks: u16| {
                    let interval =
                        f32::from(interval_ticks.max(1)) / CASTLE_FIGHT_SIMULATION_HZ as f32;
                    let concurrent = (lifetime / interval).ceil().max(1.0) as usize;
                    let entry = capacities
                        .entry(playback.key(model))
                        .or_insert_with(|| (model.clone(), 0, 0));
                    entry.1 = (entry.1 + population.saturating_mul(concurrent))
                        .min(EFFECT_PREWARM_ROOTS_PER_TEMPLATE);
                    entry.2 = (entry.2 + population).min(EFFECT_PREWARM_ROOTS_PER_TEMPLATE);
                };
                if let Some(visual) = visuals.projectile(definition.rawcode) {
                    let lifetime = match definition.attack.delivery {
                        AttackDelivery::Melee => 0.0,
                        AttackDelivery::RangedInstant => visual
                            .model
                            .effect_lifetime_seconds(ABILITY_MODEL_EFFECT_SECONDS),
                        AttackDelivery::RangedGuaranteedHit { speed_per_tick }
                        | AttackDelivery::RangedBallistic { speed_per_tick, .. } => {
                            definition.attack.range.max(0) as f32
                                / speed_per_tick.max(1) as f32
                                / CASTLE_FIGHT_SIMULATION_HZ as f32
                        }
                        AttackDelivery::Bounce {
                            speed_per_tick,
                            bounce_range,
                            max_bounces,
                            ..
                        } => {
                            (definition.attack.range.max(0) as f32
                                + bounce_range.max(0) as f32 * f32::from(max_bounces))
                                / speed_per_tick.max(1) as f32
                                / CASTLE_FIGHT_SIMULATION_HZ as f32
                        }
                    };
                    reserve(
                        &visual.model,
                        Wc3EffectPlayback::OneShot,
                        lifetime,
                        definition.attack.cooldown_ticks,
                    );
                }
                let automatic = definition.automatic_abilities().map(|ability| {
                    let status = match ability.effect {
                        AbilityEffect::ModifyMovementSpeedPercent {
                            modifier,
                            duration_ticks,
                            ..
                        }
                        | AbilityEffect::HolyAid {
                            modifier,
                            duration_ticks,
                            ..
                        }
                        | AbilityEffect::Prayer {
                            modifier,
                            duration_ticks,
                            ..
                        }
                        | AbilityEffect::HolyFervour {
                            modifier,
                            duration_ticks,
                            ..
                        } => Some((modifier.0, duration_ticks)),
                        AbilityEffect::FrostArmor {
                            modifier,
                            armor_duration_ticks,
                            ..
                        } => Some((modifier.0, armor_duration_ticks)),
                        AbilityEffect::FaerieFire {
                            modifier,
                            duration_ticks,
                            ..
                        } => Some((modifier.0, duration_ticks)),
                        AbilityEffect::Damage { .. }
                        | AbilityEffect::Stun { .. }
                        | AbilityEffect::AreaDamage { .. }
                        | AbilityEffect::Purification { .. }
                        | AbilityEffect::ArtilleryBombardment { .. } => None,
                    };
                    (ability.id.0, ability.cooldown_ticks, status)
                });
                let passive = definition
                    .passive_effects
                    .iter()
                    .filter_map(|effect| match effect {
                        PassiveUnitEffect::Defend(profile) => {
                            Some((profile.ability.0, u16::MAX, None))
                        }
                        PassiveUnitEffect::TriggeredSpellProc(profile) => {
                            Some((profile.ability.0, definition.attack.cooldown_ticks, None))
                        }
                        _ => None,
                    });
                for (ability, interval, status) in automatic.chain(passive) {
                    for visual in visuals.ability_for_source(ability, Some(definition.rawcode)) {
                        reserve(
                            &visual.model,
                            Wc3EffectPlayback::OneShot,
                            visual
                                .model
                                .effect_lifetime_seconds(ABILITY_MODEL_EFFECT_SECONDS),
                            interval,
                        );
                    }
                    // Persistent status visuals use Stand/looping playback, so they
                    // must never acquire a timed Birth instance of the same scene.
                    let (status_id, duration) = status.unwrap_or((ability, 0));
                    for visual in visuals.status(status_id) {
                        reserve(
                            &visual.model,
                            Wc3EffectPlayback::Looping,
                            f32::from(duration) / CASTLE_FIGHT_SIMULATION_HZ as f32,
                            interval,
                        );
                    }
                }
            }
            // Stable asset-path ordering keeps startup budgets reproducible; this has
            // no connection to authoritative simulation ordering.
            let mut requests: Vec<_> = capacities
                .into_iter()
                .map(|(key, (model, desired_capacity, activation_capacity))| {
                    Wc3EffectPrewarmRequest {
                        model,
                        playback: key.playback,
                        remaining: 0,
                        capacity: 0,
                        activation_capacity,
                        desired_capacity,
                    }
                })
                .collect();
            requests.sort_by_key(|request| {
                (
                    asset_server
                        .get_path(request.model.scene.id())
                        .map(|path| path.to_string()),
                    matches!(request.playback, Wc3EffectPlayback::Looping),
                )
            });
            let allocated: usize = plan.requests.iter().map(|request| request.capacity).sum();
            let mut available = EFFECT_PREWARM_TOTAL_ROOTS.saturating_sub(allocated);
            for request in &mut requests {
                if let Some(previous) = plan.requests.iter().find(|previous| {
                    previous.playback.key(&previous.model) == request.playback.key(&request.model)
                }) {
                    request.capacity = previous.capacity;
                    request.remaining = previous.remaining;
                }
            }
            // Share the finite budget across activation waves before allocating
            // extra concurrent occupancy. Asset-path priority alone can otherwise
            // reserve long-lived buffs while leaving an entire missile wave cold.
            for activation_wave in [true, false] {
                loop {
                    let mut allocated = false;
                    for request in &mut requests {
                        if available == 0 {
                            break;
                        }
                        let target = if activation_wave {
                            request.activation_capacity
                        } else {
                            request.desired_capacity
                        };
                        if request.capacity < target {
                            request.capacity += 1;
                            request.remaining += 1;
                            available -= 1;
                            allocated = true;
                        }
                    }
                    if !allocated || available == 0 {
                        break;
                    }
                }
            }
            plan.requests = requests;
        }
    }
    let mut budget = EFFECT_PREWARM_ROOTS_PER_FRAME;
    for request in &mut plan.requests {
        if budget == 0 {
            break;
        }
        if request.remaining == 0 || !request.model.assets_ready(&asset_server) {
            continue;
        }
        let count = request.remaining.min(budget);
        for _ in 0..count {
            let mut entity = commands.spawn((
                WorldAssetRoot(request.model.scene.clone()),
                Transform::IDENTITY,
                Visibility::Hidden,
                Wc3EffectWarmup,
                request.playback.emitter_source(&request.model),
                PrewarmingWc3Effect {
                    key: request.playback.key(&request.model),
                    needs_animation: request.playback.animation_source(&request.model).is_some(),
                },
            ));
            if let Some(animation) = request.playback.animation_source(&request.model) {
                entity.insert(animation);
            }
        }
        request.remaining -= count;
        budget -= count;
    }
}

type PendingWc3EffectMaterials<'w, 's> = Query<
    'w,
    's,
    (),
    (
        With<Mesh3d>,
        With<MeshMaterial3d<StandardMaterial>>,
        With<GltfMaterialExtras>,
        Without<Wc3MaterialProcessed>,
    ),
>;

fn retain_prewarmed_wc3_effects(
    mut commands: Commands,
    spawner: Res<WorldInstanceSpawner>,
    roots: Query<(
        Entity,
        &WorldInstance,
        &PrewarmingWc3Effect,
        &Wc3EmitterSource,
        Has<Wc3ModelSequenceSelection>,
    )>,
    pending_materials: PendingWc3EffectMaterials<'_, '_>,
    mut pool: ResMut<TimedWc3EffectPool>,
    mut plan: ResMut<Wc3EffectPrewarmPlan>,
) {
    // This runs after all WC3 node/material/animation setup and its deferred
    // commands. A root request alone is insufficient to enter the reserve.
    for (entity, instance, warmup, emitters, animation_ready) in &roots {
        if !spawner.instance_is_ready(**instance)
            || (warmup.needs_animation && !animation_ready)
            || !emitters.node_bindings_ready()
            || spawner
                .iter_instance_entities(**instance)
                .any(|entity| pending_materials.contains(entity))
        {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PrewarmingWc3Effect>()
            .insert_recursive::<Children>(Disabled);
        pool.scenes.entry(warmup.key).or_default().push(entity);
        plan.prepared_total += 1;
    }
}

fn spawn_or_reuse_timed_wc3_visual(
    commands: &mut Commands,
    pool: &mut TimedWc3EffectPool,
    model: &Wc3VisualModel,
    transform: Transform,
    pooling_enabled: bool,
) -> (Entity, Option<Wc3EffectPoolKey>) {
    spawn_or_reuse_wc3_visual(
        commands,
        pool,
        model,
        transform,
        pooling_enabled,
        Wc3EffectPlayback::OneShot,
    )
}

fn spawn_or_reuse_wc3_visual(
    commands: &mut Commands,
    pool: &mut TimedWc3EffectPool,
    model: &Wc3VisualModel,
    transform: Transform,
    pooling_enabled: bool,
    playback: Wc3EffectPlayback,
) -> (Entity, Option<Wc3EffectPoolKey>) {
    let pool_key = playback.key(model);
    if pooling_enabled && let Some(entity) = pool.scenes.get_mut(&pool_key).and_then(Vec::pop) {
        commands
            .entity(entity)
            .remove_recursive::<Children, Disabled>()
            .remove_recursive::<Children, Wc3EffectWarmup>()
            .insert((transform, Visibility::Inherited, Wc3EffectReusePending));
        // Scene-local emitter bindings survive reuse; the reset system rewinds
        // counters/clocks without reparsing the hierarchy. Ribbon scenes are
        // prepared for one use and create their trails only when activated.
        if !model.ribbons.is_empty() {
            commands
                .entity(entity)
                .insert(Wc3RibbonSource::new(&model.ribbons));
        }
        return (entity, model.poolable_instance().then_some(pool_key));
    }

    let entity = commands
        .spawn((
            WorldAssetRoot(model.scene.clone()),
            transform,
            playback.emitter_source(model),
            Wc3RibbonSource::new(&model.ribbons),
        ))
        .id();
    if let Some(animation) = playback.animation_source(model) {
        commands.entity(entity).insert(animation);
    }
    let pooled_scene = (pooling_enabled && model.poolable_instance()).then_some(pool_key);
    (entity, pooled_scene)
}

fn sync_render_entities(
    mut commands: Commands,
    samples: Res<PresentationSamples>,
    world: SyncRenderWorld<'_>,
    mut render_map: ResMut<RenderMap>,
    effects: SyncRenderEffects<'_>,
    imported_roots: Query<(Entity, &ImportedUnitModelRoot)>,
    world_instances: Query<(), With<WorldInstance>>,
) {
    let (metrics, terrain, assets, unit_models, building_models, wc3_visuals, experiment) = world;
    let (
        mut remnants,
        mut projectile_impacts,
        mut ability_impacts,
        mut timed_effects,
        mut effect_pool,
        mut meshes,
        mut materials,
    ) = effects;
    let legacy_effect_pooling = experiment
        .as_ref()
        .is_some_and(|experiment| **experiment == RenderExperiment::LegacyEffectPooling);
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
                owner: Some(unit.owner),
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
            center.y = building_terrain_height(&metrics, &terrain, building.footprint);
            remnants.0.push(DeathRemnant {
                position: center,
                owner: building.owner,
                building: true,
                remaining: DEATH_REMAINS_SECONDS,
            });
        }
    }

    // Upgrades deliberately retain the authoritative building SimId and footprint. If the
    // content rawcode changes in place, replace only the presentation root so the target model can
    // play its Birth sequence; cancellation performs the inverse swap back to the precursor's
    // Stand model without fabricating a death/remnant. Recreate imported models once construction
    // completes as well. Construction drives Birth by seeking a paused animation; Bevy transitions
    // deliberately do not fade a paused outgoing clip, so Birth can otherwise remain active beside
    // Stand. WC3 Birth clips can also animate transform channels that Stand never touches, leaving
    // the last construction pose latched even if Birth is stopped.
    let building_presentations_to_rebuild: Vec<_> = render_map
        .buildings
        .iter()
        .filter_map(|(id, entry)| {
            let building = samples.current.buildings.get(id)?;
            let desired_rawcode = building.content.and_then(|content| {
                building_models
                    .get(content.rawcode)
                    .map(|_| content.rawcode)
            });
            building_presentation_needs_rebuild(
                entry.imported_rawcode,
                entry.constructing,
                desired_rawcode,
                building.construction_complete_tick.is_some(),
            )
            .then_some(*id)
        })
        .collect();
    for id in building_presentations_to_rebuild {
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
            if let Some((model_root, key)) = projectile_entry.pooled_visual
                && world_instances.contains(model_root)
            {
                commands
                    .entity(model_root)
                    .remove::<ChildOf>()
                    .insert_recursive::<Children>(Disabled);
                effect_pool.scenes.entry(key).or_default().push(model_root);
            }
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

    for attack in &samples.current.attacks {
        if attack.missed || attack.delivery != AttackDelivery::RangedInstant {
            continue;
        }
        let rawcode = samples
            .current
            .units
            .get(&attack.source)
            .or_else(|| samples.previous.units.get(&attack.source))
            .and_then(|unit| unit.content)
            .map(|content| content.rawcode);
        let Some(visual) = rawcode.and_then(|rawcode| wc3_visuals.projectile(rawcode)) else {
            continue;
        };
        let position = sim_point_to_terrain_world(attack.target_position, &terrain) + Vec3::Y * 8.0;
        let (entity, pooled_scene) = spawn_or_reuse_timed_wc3_visual(
            &mut commands,
            &mut effect_pool,
            &visual.model,
            Transform::from_translation(position),
            !legacy_effect_pooling,
        );
        let lifetime = visual
            .model
            .effect_lifetime_seconds(ABILITY_MODEL_EFFECT_SECONDS);
        timed_effects.0.push(TimedWc3Effect {
            entity,
            remaining: lifetime,
            lifetime,
            mesh: None,
            fade_material: None,
            pooled_lightning: false,
            pooled_scene,
        });
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
            let lightning_mesh = build_wc3_chain_lightning_mesh(start, end, seed, width);
            let (entity, mesh, lightning_material, pooled_lightning) = if !legacy_effect_pooling
                && !effect_pool.lightning.is_empty()
            {
                let effect = effect_pool
                    .lightning
                    .pop()
                    .expect("non-empty lightning pool must yield an effect");
                *meshes
                    .get_mut(&effect.mesh)
                    .expect("pooled WC3 Chain Lightning mesh must remain allocated") =
                    lightning_mesh;
                materials
                    .get_mut(&effect.material)
                    .expect("pooled WC3 Chain Lightning material must remain allocated")
                    .base_color = Color::WHITE;
                commands
                    .entity(effect.entity)
                    .insert((Transform::IDENTITY, Visibility::Inherited));
                (effect.entity, effect.mesh, effect.material, true)
            } else {
                let mesh = meshes.add(lightning_mesh);
                let lightning_material = materials.get(&assets.lightning_material).cloned().expect(
                    "WC3 Chain Lightning material must exist while presentation is running",
                );
                let lightning_material = materials.add(lightning_material);
                let entity = commands
                    .spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(lightning_material.clone()),
                        Transform::IDENTITY,
                    ))
                    .id();
                (entity, mesh, lightning_material, !legacy_effect_pooling)
            };
            timed_effects.0.push(TimedWc3Effect {
                entity,
                remaining: LIGHTNING_EFFECT_SECONDS,
                lifetime: LIGHTNING_EFFECT_SECONDS,
                mesh: Some(mesh),
                fade_material: Some(lightning_material),
                pooled_lightning,
                pooled_scene: None,
            });
        }
    }

    for revival in &samples.current.shrine_revivals {
        let Some(model) = wc3_visuals.system_model(revival.model_path) else {
            continue;
        };
        let (entity, pooled_scene) = spawn_or_reuse_timed_wc3_visual(
            &mut commands,
            &mut effect_pool,
            model,
            Transform::from_translation(sim_point_to_terrain_world(revival.position, &terrain)),
            !legacy_effect_pooling,
        );
        let lifetime = model.effect_lifetime_seconds(ABILITY_MODEL_EFFECT_SECONDS);
        timed_effects.0.push(TimedWc3Effect {
            entity,
            remaining: lifetime,
            lifetime,
            mesh: None,
            fade_material: None,
            pooled_lightning: false,
            pooled_scene,
        });
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
                AbilityCastTarget::AllFriendlyUnits => source_position,
                AbilityCastTarget::Point(position) => {
                    Some(sim_point_to_terrain_world(position, &terrain))
                }
            });

        let source_rawcode = samples
            .current
            .units
            .get(&cast.source)
            .or_else(|| samples.previous.units.get(&cast.source))
            .and_then(|unit| unit.content.map(|content| content.rawcode))
            .or_else(|| {
                samples
                    .current
                    .buildings
                    .get(&cast.source)
                    .or_else(|| samples.previous.buildings.get(&cast.source))
                    .and_then(|building| building.content.map(|content| content.rawcode))
            });
        for visual in wc3_visuals.ability_for_source(cast.ability.0, source_rawcode) {
            let position = match visual.anchor {
                Wc3AbilityVisualAnchor::Source => source_position,
                Wc3AbilityVisualAnchor::Target => target_position,
            };
            let Some(position) = position else {
                continue;
            };
            let (entity, pooled_scene) = spawn_or_reuse_timed_wc3_visual(
                &mut commands,
                &mut effect_pool,
                &visual.model,
                Transform::from_translation(position),
                !legacy_effect_pooling,
            );
            let lifetime = visual
                .model
                .effect_lifetime_seconds(ABILITY_MODEL_EFFECT_SECONDS);
            timed_effects.0.push(TimedWc3Effect {
                entity,
                remaining: lifetime,
                lifetime,
                mesh: None,
                fade_material: None,
                pooled_lightning: false,
                pooled_scene,
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
            let owner_model_root = imported_roots
                .iter()
                .find_map(|(entity, root)| (root.sim_id == unit.id).then_some(entity));
            let (entity, pooled_scene) = spawn_or_reuse_timed_wc3_visual(
                &mut commands,
                &mut effect_pool,
                &visual.model,
                if owner_model_root.is_some() {
                    Transform::IDENTITY
                } else {
                    Transform::from_translation(position)
                },
                !legacy_effect_pooling,
            );
            commands.entity(entity).insert(Wc3VertexTint([255; 3]));
            if let Some(root) = owner_model_root {
                commands.entity(root).add_child(entity);
                commands.entity(entity).insert(Wc3AttachToNode {
                    owner_root: root,
                    attachment_point: "hand left".to_owned(),
                });
            }
            let lifetime = visual
                .model
                .effect_lifetime_seconds(ABILITY_MODEL_EFFECT_SECONDS);
            timed_effects.0.push(TimedWc3Effect {
                entity,
                remaining: lifetime,
                lifetime,
                mesh: None,
                fade_material: None,
                pooled_lightning: false,
                pooled_scene,
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
            let model_root = commands
                .spawn((
                    WorldAssetRoot(model.scene.clone()),
                    ImportedUnitModelRoot {
                        sim_id: builder.id,
                        rawcode: builder.appearance.rawcode,
                        presentation_root: entity,
                    },
                    Wc3AttachmentOwner,
                    Wc3TeamTint::new(builder.owner.0, player_color(builder.owner), "wc3/units"),
                    Transform {
                        translation: Vec3::NEG_Y * BUILDER_HEIGHT * 0.5,
                        rotation: Quat::from_rotation_y(WC3_MODEL_FACING_OFFSET),
                        scale: Vec3::splat(model.scale),
                    },
                ))
                .id();
            commands.entity(entity).add_child(model_root);
            if let Some(tint) = model.tint_rgb {
                commands.entity(model_root).insert(Wc3VertexTint(tint));
            }
            if !model.ribbon_emitters.is_empty() {
                commands
                    .entity(model_root)
                    .insert(Wc3RibbonSource::with_asset_prefix(
                        &model.ribbon_emitters,
                        "wc3/units",
                    ));
            }
            (entity, Some(builder.appearance.rawcode))
        } else {
            let entity = commands
                .spawn((
                    Mesh3d(assets.melee_mesh.clone()),
                    MeshMaterial3d(assets.unit_material(builder.owner)),
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
                constructing: false,
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
            let model_root = commands
                .spawn((
                    WorldAssetRoot(model.scene.clone()),
                    ImportedUnitModelRoot {
                        sim_id: unit.id,
                        rawcode,
                        presentation_root: entity,
                    },
                    Wc3AttachmentOwner,
                    Wc3TeamTint::new(unit.owner.0, player_color(unit.owner), "wc3/units"),
                    Transform {
                        translation: Vec3::NEG_Y * unit_height(unit) * 0.5,
                        rotation: Quat::from_rotation_y(WC3_MODEL_FACING_OFFSET),
                        scale: Vec3::splat(model.scale),
                    },
                ))
                .id();
            commands.entity(entity).add_child(model_root);
            if let Some(tint) = model.tint_rgb {
                commands.entity(model_root).insert(Wc3VertexTint(tint));
            }
            if !model.ribbon_emitters.is_empty() {
                commands
                    .entity(model_root)
                    .insert(Wc3RibbonSource::with_asset_prefix(
                        &model.ribbon_emitters,
                        "wc3/units",
                    ));
            }
            spawn_persistent_unit_attachments(&mut commands, model_root, model, &wc3_visuals);
            (entity, None, Some(rawcode))
        } else {
            let entity = commands
                .spawn((
                    Mesh3d(assets.unit_mesh(unit.visual_kind)),
                    MeshMaterial3d(assets.unit_material(unit.owner)),
                    Transform {
                        translation: position,
                        scale: Vec3::splat(unit_render_scale(unit)),
                        ..default()
                    },
                ))
                .id();
            let weapon =
                spawn_unit_weapon(&mut commands, &assets, entity, unit.owner, unit.visual_kind);
            spawn_air_wings(&mut commands, &assets, entity, unit);
            (entity, Some(weapon), None)
        };
        render_map.units.insert(
            unit.id,
            PresentedEntry {
                entity,
                weapon,
                imported_rawcode,
                constructing: false,
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
        if let Some(effect) = render_map.status_effects.remove(&key) {
            if let Some(pool_key) = effect.pooled_scene
                && world_instances.contains(effect.entity)
            {
                commands
                    .entity(effect.entity)
                    .insert_recursive::<Children>(Disabled);
                effect_pool
                    .scenes
                    .entry(pool_key)
                    .or_default()
                    .push(effect.entity);
            } else {
                commands.entity(effect.entity).despawn();
            }
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
                (&wc3_visuals, &mut effect_pool, !legacy_effect_pooling),
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
                (&wc3_visuals, &mut effect_pool, !legacy_effect_pooling),
                unit,
                modifier.id.0,
                Wc3StatusVisualKind::Armor,
                &terrain,
            );
        }
        let attack_speed_count = usize::from(unit.status.attack_speed_modifier_count);
        for modifier in unit.status.attack_speed_modifiers[..attack_speed_count]
            .iter()
            .filter(|modifier| modifier.expires_tick > samples.current.tick)
        {
            spawn_unit_status_visuals(
                &mut commands,
                &mut render_map,
                (&wc3_visuals, &mut effect_pool, !legacy_effect_pooling),
                unit,
                modifier.id.0,
                Wc3StatusVisualKind::AttackSpeed,
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
            center.y = building_terrain_height(&metrics, &terrain, building.footprint);
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
        center.y = building_terrain_height(&metrics, &terrain, building.footprint);
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
                    Wc3AttachmentOwner,
                    Wc3TeamTint::new(
                        building.owner.map_or(24, |owner| owner.0),
                        owner_color(building.owner),
                        "wc3/buildings",
                    ),
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
            if !model.ribbons.is_empty() {
                commands
                    .entity(model_root)
                    .insert(Wc3RibbonSource::with_asset_prefix(
                        &model.ribbons,
                        "wc3/buildings",
                    ));
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
                constructing: building.construction_complete_tick.is_some(),
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
            let model_root = commands
                .spawn((
                    WorldAssetRoot(model.scene.clone()),
                    ImportedUnitModelRoot {
                        sim_id: corpse.source_unit,
                        rawcode,
                        presentation_root: entity,
                    },
                    Wc3AttachmentOwner,
                    Wc3TeamTint::new(
                        corpse.source_owner.0,
                        player_color(corpse.source_owner),
                        "wc3/units",
                    ),
                    Transform {
                        rotation: Quat::from_rotation_y(WC3_MODEL_FACING_OFFSET),
                        scale: Vec3::splat(model.scale),
                        ..default()
                    },
                ))
                .id();
            if !model.ribbon_emitters.is_empty() {
                commands
                    .entity(model_root)
                    .insert(Wc3RibbonSource::with_asset_prefix(
                        &model.ribbon_emitters,
                        "wc3/units",
                    ));
            }
            commands.entity(entity).add_child(model_root);
            entity
        } else {
            let position = corpse_render_position(corpse.position, &terrain);
            commands
                .spawn((
                    Mesh3d(assets.corpse_mesh.clone()),
                    MeshMaterial3d(assets.corpse_material(corpse.source_owner)),
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
        let mut pooled_visual = None;
        let entity = if let Some(visual) = imported_projectile {
            let model = &visual.model;
            let entity = commands
                .spawn((Transform::from_translation(position), Visibility::default()))
                .id();
            let (model_root, pooled_scene) = spawn_or_reuse_timed_wc3_visual(
                &mut commands,
                &mut effect_pool,
                model,
                Transform::from_rotation(Quat::from_rotation_y(WC3_PROJECTILE_FACING_OFFSET)),
                !legacy_effect_pooling,
            );
            pooled_visual = pooled_scene.map(|key| (model_root, key));
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
                pooled_visual,
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
            if transform.translation != position {
                transform.translation = position;
            }
            if previous.position != current.position {
                let delta =
                    sim_point_to_world(current.position) - sim_point_to_world(previous.position);
                if delta.length_squared() > f32::EPSILON {
                    let desired = Quat::from_rotation_y(delta.x.atan2(delta.z));
                    update_facing_rotation(&mut transform, desired, facing_blend);
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
        let bob = if entry.imported_rawcode.is_some() {
            0.0
        } else {
            unit_motion_bob(current.id, current.movement_class, render_tick, moving)
        };
        let position = ground_position + Vec3::Y * (unit_height(current) * 0.5 + bob);
        let desired_rotation =
            unit_facing_rotation(current, previous, &samples, &metrics, &terrain, alpha);
        if let Ok(mut transform) = transforms.get_mut(entry.entity) {
            if transform.translation != position {
                transform.translation = position;
            }
            if let Some(desired_rotation) = desired_rotation {
                update_facing_rotation(&mut transform, desired_rotation, facing_blend);
            }
        }
        if let Some(stun_entity) = render_map.stun_effects.get(id)
            && let Ok(mut transform) = transforms.get_mut(*stun_entity)
        {
            transform.translation =
                ground_position + Vec3::Y * unit_stun_height(current, &render_map, &unit_models);
        }
    }

    for (key, effect) in &render_map.status_effects {
        let Some(current) = samples.current.units.get(&key.target) else {
            continue;
        };
        let previous = samples.previous.units.get(&key.target).unwrap_or(current);
        let moving = previous.position != current.position;
        let bob = render_map.units.get(&key.target).map_or(0.0, |entry| {
            if entry.imported_rawcode.is_some() {
                0.0
            } else {
                unit_motion_bob(current.id, current.movement_class, render_tick, moving)
            }
        });
        let position = unit_ground_position_lerp(
            previous.position,
            current.position,
            current.movement_class,
            alpha,
            &terrain,
        ) + Vec3::Y * bob;
        if let Ok(mut transform) = transforms.get_mut(effect.entity)
            && transform.translation != position
        {
            transform.translation = position;
        }
    }

    for (id, current) in &samples.current.buildings {
        let Some(entry) = render_map.buildings.get(id) else {
            continue;
        };
        let previous = samples.previous.buildings.get(id).unwrap_or(current);
        let has_stun_effect = render_map.stun_effects.contains_key(id);
        if !has_stun_effect
            && previous.footprint == current.footprint
            && previous.visual_kind == current.visual_kind
        {
            continue;
        }
        let (mut center, _) = metrics.footprint_center_size(current.footprint);
        center.y = building_terrain_height(&metrics, &terrain, current.footprint);
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

fn update_facing_rotation(transform: &mut Transform, desired: Quat, blend: f32) {
    let angle = transform.rotation.angle_between(desired);
    if angle <= UNIT_FACING_SNAP_RADIANS {
        if transform.rotation != desired {
            transform.rotation = desired;
        }
        return;
    }
    transform.rotation = transform.rotation.slerp(desired, blend);
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
        Wc3StatusVisualKind::AttackSpeed => {
            let count = usize::from(unit.status.attack_speed_modifier_count);
            unit.status.attack_speed_modifiers[..count]
                .iter()
                .any(|modifier| modifier.id.0 == ability_rawcode && modifier.expires_tick > tick)
        }
    }
}

fn spawn_unit_status_visuals(
    commands: &mut Commands,
    render_map: &mut RenderMap,
    visuals: (&Wc3VisualSet, &mut TimedWc3EffectPool, bool),
    unit: &UnitSample,
    ability_rawcode: u32,
    kind: Wc3StatusVisualKind,
    terrain: &TerrainSurface,
) {
    let (wc3_visuals, pool, pooling_enabled) = visuals;
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
        let (entity, pooled_scene) = spawn_or_reuse_wc3_visual(
            commands,
            pool,
            &visual.model,
            Transform::from_translation(position),
            pooling_enabled,
            Wc3EffectPlayback::Looping,
        );
        render_map.status_effects.insert(
            key,
            PresentedStatusEffect {
                entity,
                pooled_scene,
            },
        );
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
            model.emitter_source(),
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
        center.y = building_terrain_height(metrics, terrain, building.footprint);
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
    mut effect_pool: ResMut<TimedWc3EffectPool>,
    world_instances: Query<(), With<WorldInstance>>,
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
            if let Some(pool_key) = effect.pooled_scene.take() {
                if world_instances.get(effect.entity).is_ok() {
                    commands
                        .entity(effect.entity)
                        .remove::<ChildOf>()
                        .remove::<Wc3AttachToNode>()
                        .insert_recursive::<Children>(Disabled);
                    effect_pool
                        .scenes
                        .entry(pool_key)
                        .or_default()
                        .push(effect.entity);
                } else {
                    commands.entity(effect.entity).despawn();
                }
            } else if effect.pooled_lightning {
                let mesh = effect
                    .mesh
                    .take()
                    .expect("pooled lightning effect must retain its mesh");
                let material = effect
                    .fade_material
                    .take()
                    .expect("pooled lightning effect must retain its material");
                commands.entity(effect.entity).insert(Visibility::Hidden);
                effect_pool.lightning.push(PooledLightningEffect {
                    entity: effect.entity,
                    mesh,
                    material,
                });
            } else {
                commands.entity(effect.entity).despawn();
                if let Some(mesh) = effect.mesh.take() {
                    meshes.remove(mesh.id());
                }
                if let Some(material) = effect.fade_material.take() {
                    materials.remove(material.id());
                }
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
        mut health_bar_materials,
        mut shader_buffers,
        mut batch_entity,
    } = params;
    let (camera, camera_transform, frustum) = *camera;
    // Camera movement is applied earlier in Update, while Bevy propagates GlobalTransform in
    // PostUpdate. This camera is an unparented root, so derive its current global transform
    // directly from Transform to keep screen-space bars on the same frame as camera motion.
    let camera_global = GlobalTransform::from(*camera_transform);
    let (batch_transform, batch_visibility) = &mut *batch_entity;

    batch.rects.clear();

    if debug.health_bars {
        let Some(viewport) = camera.logical_viewport_rect() else {
            clear_health_bar_buffer_if_needed(
                &mut batch,
                &mut health_bar_materials,
                &mut shader_buffers,
            );
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
                health_bar_screen_layout(camera, &camera_global, anchor, world_width, viewport)
            else {
                continue;
            };
            push_split_bar(
                &mut batch.rects,
                screen,
                HEALTH_BAR_HEIGHT_PIXELS,
                health_ratio(unit.health, unit.health_max),
                player_color(unit.owner),
                Color::srgb(0.085, 0.085, 0.095),
                viewport,
            );
            if let Some((mana, maximum)) = unit.mana_current.zip(unit.mana_maximum) {
                push_mana_bar(&mut batch.rects, screen, mana, maximum, viewport);
            }
        }

        for (id, building) in &samples.current.buildings {
            let Some(entry) = render_map.buildings.get(id) else {
                continue;
            };
            let (mut center, size) = metrics.footprint_center_size(building.footprint);
            center.y = building_terrain_height(&metrics, &terrain, building.footprint);
            let overhead_height = building_bar_overhead_height(building, entry, &building_models);
            let world_width = building_health_bar_width(size, overhead_height);
            let anchor = center + Vec3::Y * (overhead_height + HEALTH_BAR_VERTICAL_GAP);
            if !health_bar_world_visible(frustum, anchor, world_width) {
                continue;
            }
            let Some(screen) =
                health_bar_screen_layout(camera, &camera_global, anchor, world_width, viewport)
            else {
                continue;
            };
            push_split_bar(
                &mut batch.rects,
                screen,
                HEALTH_BAR_HEIGHT_PIXELS,
                health_ratio(building.health, building.health_max),
                owner_color(building.owner),
                Color::srgb(0.085, 0.085, 0.095),
                viewport,
            );
            if building.production_queue.is_none()
                && let Some((mana, maximum)) = building.mana_current.zip(building.mana_maximum)
            {
                push_mana_bar(&mut batch.rects, screen, mana, maximum, viewport);
            }
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
        batch_transform.translation = camera_transform.translation;
    }
    upload_health_bar_rects(&mut batch, &mut health_bar_materials, &mut shader_buffers);
}

fn clear_health_bar_buffer_if_needed(
    batch: &mut HealthBarBatch,
    health_bar_materials: &mut Assets<HealthBarMaterial>,
    shader_buffers: &mut Assets<ShaderBuffer>,
) {
    if batch.last_rect_count == 0 {
        return;
    }
    batch.rects.clear();
    upload_health_bar_rects(batch, health_bar_materials, shader_buffers);
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

fn push_mana_bar(
    rects: &mut Vec<HealthBarRect>,
    health_layout: HealthBarScreenLayout,
    mana: i32,
    maximum: i32,
    viewport: Rect,
) {
    let mana_layout = HealthBarScreenLayout {
        top: health_layout.top + HEALTH_BAR_HEIGHT_PIXELS + MANA_BAR_GAP_PIXELS,
        ..health_layout
    };
    push_split_bar(
        rects,
        mana_layout,
        MANA_BAR_HEIGHT_PIXELS,
        health_ratio(mana, maximum),
        Color::srgb(0.12, 0.34, 0.88),
        Color::srgb(0.06, 0.07, 0.10),
        viewport,
    );
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

fn upload_health_bar_rects(
    batch: &mut HealthBarBatch,
    health_bar_materials: &mut Assets<HealthBarMaterial>,
    shader_buffers: &mut Assets<ShaderBuffer>,
) {
    let rect_count = batch.rects.len().min(HEALTH_BAR_BATCH_MAX_RECTS);
    let required_capacity = rect_count
        .max(HEALTH_BAR_BATCH_MIN_BUFFER_RECTS)
        .next_power_of_two()
        .min(HEALTH_BAR_BATCH_MAX_RECTS);
    let grew = required_capacity > batch.gpu_capacity_rects;
    if grew {
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

    if grew {
        // A ShaderBuffer whose byte length changes is recreated on the GPU. The material bind
        // group does not observe that recreation through an unchanged Handle, so explicitly
        // replace the asset and retarget the material whenever capacity grows.
        let new_buffer = shader_buffers.add(ShaderBuffer::from(batch.gpu_data.clone()));
        batch.buffer = new_buffer.clone();
        let mut material = health_bar_materials
            .get_mut(&batch.material)
            .expect("health bar material must outlive its batch");
        material.rect_data = new_buffer;
    } else if let Some(mut buffer) = shader_buffers.get_mut(&batch.buffer) {
        buffer.set_data(batch.gpu_data.clone());
    }
    batch.last_rect_count = rect_count;
}

fn map_grid_base_cells(selected_match: &SelectedMatch) -> Option<u16> {
    selected_match
        .content
        .production_building_definitions()
        .map(|definition| definition.footprint_size_cells)
        .min()
}

fn draw_map_grid(
    state: Res<MapGridState>,
    selected_match: Res<SelectedMatch>,
    metrics: Res<WorldMetrics>,
    terrain: Res<TerrainSurface>,
    mut gizmos: Gizmos<MapGridGizmos>,
) {
    if !state.enabled {
        return;
    }
    let Some(base_cells) = map_grid_base_cells(&selected_match) else {
        return;
    };
    if base_cells == 0 {
        return;
    }

    let spacing = metrics.navigation_cell_world() * f32::from(base_cells);
    let (build_min, build_max) = metrics.buildable_world_bounds();
    let layout = MapGridLayoutContext {
        base_cells,
        spacing,
        origin: (build_min + build_max) * 0.5 + Vec2::splat(spacing * 0.5),
        build_center_x: (build_min.x + build_max.x) * 0.5,
    };
    for region in metrics.build_regions() {
        draw_map_grid_region(&mut gizmos, &terrain, &metrics, region, layout);
    }
}

#[derive(Debug, Clone, Copy)]
struct MapGridLayoutContext {
    base_cells: u16,
    spacing: f32,
    origin: Vec2,
    build_center_x: f32,
}

fn map_grid_axis_line_bounds(
    region_min: f32,
    region_max: f32,
    spacing: f32,
    origin: f32,
) -> Option<(i32, i32)> {
    let first = ((region_min - origin) / spacing).ceil() as i32;
    let last = ((region_max - origin) / spacing).floor() as i32;
    (first <= last).then_some((first, last))
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct MapGridHorizontalLayout {
    anchor: f32,
    opposite: f32,
    line_count: u32,
    anchored_left: bool,
}

fn map_grid_horizontal_layout(
    region: BuildingFootprint,
    region_min_x: f32,
    region_max_x: f32,
    base_cells: u16,
    spacing: f32,
    build_center_x: f32,
) -> Option<MapGridHorizontalLayout> {
    if base_cells == 0 || region.width < base_cells {
        return None;
    }
    let anchored_left = (region_min_x + region_max_x) * 0.5 <= build_center_x;
    let full_columns = u32::from((region.width - base_cells) / base_cells) + 1;
    let line_count = full_columns + u32::from(!anchored_left);
    let anchor = if anchored_left {
        region_min_x
    } else {
        region_max_x
    };
    let direction = if anchored_left { 1.0 } else { -1.0 };
    Some(MapGridHorizontalLayout {
        anchor,
        opposite: anchor + direction * (line_count - 1) as f32 * spacing,
        line_count,
        anchored_left,
    })
}

fn draw_map_grid_region(
    gizmos: &mut Gizmos<MapGridGizmos>,
    terrain: &TerrainSurface,
    metrics: &WorldMetrics,
    region: BuildingFootprint,
    layout: MapGridLayoutContext,
) {
    if region.width < layout.base_cells || region.height < layout.base_cells {
        return;
    }
    let (region_min, region_max) = metrics.build_region_world_bounds(region);
    let Some((first_z, last_z)) =
        map_grid_axis_line_bounds(region_min.y, region_max.y, layout.spacing, layout.origin.y)
    else {
        return;
    };
    let Some(horizontal) = map_grid_horizontal_layout(
        region,
        region_min.x,
        region_max.x,
        layout.base_cells,
        layout.spacing,
        layout.build_center_x,
    ) else {
        return;
    };
    let anchor_z = layout.origin.y + last_z as f32 * layout.spacing;
    let opposite_z = layout.origin.y + first_z as f32 * layout.spacing;
    let min = Vec2::new(horizontal.anchor.min(horizontal.opposite), opposite_z);
    let max = Vec2::new(horizontal.anchor.max(horizontal.opposite), anchor_z);
    let clip = MapGridClip {
        min,
        max,
        segment_length: layout.spacing,
        metrics,
    };

    for index in 0..horizontal.line_count {
        let direction = if horizontal.anchored_left { 1.0 } else { -1.0 };
        let x = horizontal.anchor + direction * index as f32 * layout.spacing;
        if index.is_multiple_of(MAP_GRID_MAJOR_INTERVAL as u32) {
            draw_map_grid_x_line_clipped(
                gizmos,
                terrain,
                clip,
                x - MAP_GRID_MAJOR_OFFSET_WORLD,
                MAP_GRID_MAJOR_COLOR,
            );
            draw_map_grid_x_line_clipped(
                gizmos,
                terrain,
                clip,
                x + MAP_GRID_MAJOR_OFFSET_WORLD,
                MAP_GRID_MAJOR_COLOR,
            );
        } else {
            draw_map_grid_x_line_clipped(gizmos, terrain, clip, x, MAP_GRID_LINE_COLOR);
        }
    }

    for index in first_z..=last_z {
        let z = layout.origin.y + index as f32 * layout.spacing;
        if index.rem_euclid(MAP_GRID_MAJOR_INTERVAL) == 0 {
            draw_map_grid_z_line_clipped(
                gizmos,
                terrain,
                clip,
                z - MAP_GRID_MAJOR_OFFSET_WORLD,
                MAP_GRID_MAJOR_COLOR,
            );
            draw_map_grid_z_line_clipped(
                gizmos,
                terrain,
                clip,
                z + MAP_GRID_MAJOR_OFFSET_WORLD,
                MAP_GRID_MAJOR_COLOR,
            );
        } else {
            draw_map_grid_z_line_clipped(gizmos, terrain, clip, z, MAP_GRID_LINE_COLOR);
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct MapGridClip<'a> {
    min: Vec2,
    max: Vec2,
    segment_length: f32,
    metrics: &'a WorldMetrics,
}

fn draw_map_grid_x_line_clipped(
    gizmos: &mut Gizmos<MapGridGizmos>,
    terrain: &TerrainSurface,
    clip: MapGridClip<'_>,
    x: f32,
    color: Color,
) {
    if x < clip.min.x || x > clip.max.x {
        return;
    }
    let segment_count = ((clip.max.y - clip.min.y) / clip.segment_length).ceil() as u32;
    for segment in 0..segment_count {
        let y0 = clip.min.y + segment as f32 * clip.segment_length;
        let y1 = (y0 + clip.segment_length).min(clip.max.y);
        if clip
            .metrics
            .map_grid_cell_unblocked(Vec2::new(x, (y0 + y1) * 0.5))
        {
            draw_map_grid_x_line(
                gizmos,
                terrain,
                Vec2::new(clip.min.x, y0),
                Vec2::new(clip.max.x, y1),
                x,
                clip.segment_length,
                color,
            );
        }
    }
}

fn draw_map_grid_x_line(
    gizmos: &mut Gizmos<MapGridGizmos>,
    terrain: &TerrainSurface,
    min: Vec2,
    max: Vec2,
    x: f32,
    segment_length: f32,
    color: Color,
) {
    if x < min.x || x > max.x {
        return;
    }
    let segment_count = ((max.y - min.y) / segment_length).ceil() as u32;
    for segment in 0..segment_count {
        let z0 = min.y + segment as f32 * segment_length;
        let z1 = (z0 + segment_length).min(max.y);
        let p0 = Vec3::new(
            x,
            terrain.height_at_world(Vec2::new(x, z0)) + MAP_GRID_HEIGHT_OFFSET,
            z0,
        );
        let p1 = Vec3::new(
            x,
            terrain.height_at_world(Vec2::new(x, z1)) + MAP_GRID_HEIGHT_OFFSET,
            z1,
        );
        gizmos.line(p0, p1, color);
    }
}

fn draw_map_grid_z_line_clipped(
    gizmos: &mut Gizmos<MapGridGizmos>,
    terrain: &TerrainSurface,
    clip: MapGridClip<'_>,
    z: f32,
    color: Color,
) {
    if z < clip.min.y || z > clip.max.y {
        return;
    }
    let segment_count = ((clip.max.x - clip.min.x) / clip.segment_length).ceil() as u32;
    for segment in 0..segment_count {
        let x0 = clip.min.x + segment as f32 * clip.segment_length;
        let x1 = (x0 + clip.segment_length).min(clip.max.x);
        if clip
            .metrics
            .map_grid_cell_unblocked(Vec2::new((x0 + x1) * 0.5, z))
        {
            draw_map_grid_z_line(
                gizmos,
                terrain,
                Vec2::new(x0, clip.min.y),
                Vec2::new(x1, clip.max.y),
                z,
                clip.segment_length,
                color,
            );
        }
    }
}

fn draw_map_grid_z_line(
    gizmos: &mut Gizmos<MapGridGizmos>,
    terrain: &TerrainSurface,
    min: Vec2,
    max: Vec2,
    z: f32,
    segment_length: f32,
    color: Color,
) {
    if z < min.y || z > max.y {
        return;
    }
    let segment_count = ((max.x - min.x) / segment_length).ceil() as u32;
    for segment in 0..segment_count {
        let x0 = min.x + segment as f32 * segment_length;
        let x1 = (x0 + segment_length).min(max.x);
        let p0 = Vec3::new(
            x0,
            terrain.height_at_world(Vec2::new(x0, z)) + MAP_GRID_HEIGHT_OFFSET,
            z,
        );
        let p1 = Vec3::new(
            x1,
            terrain.height_at_world(Vec2::new(x1, z)) + MAP_GRID_HEIGHT_OFFSET,
            z,
        );
        gizmos.line(p0, p1, color);
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
            owner_color(remnant.owner).with_alpha(life),
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
            player_color(unit.owner).with_alpha(0.8),
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
            owner_color(building.owner),
        );
        if let Some(target) = building.target
            && let Some(target_position) =
                entity_render_position(target, &samples, &metrics, &terrain, alpha)
        {
            let (mut center, _) = metrics.footprint_center_size(building.footprint);
            center.y = building_terrain_height(&metrics, &terrain, building.footprint);
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
    if building.production_queue == Some(0) {
        return None;
    }
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

fn toggle_debug_controls(
    keys: Res<ButtonInput<KeyCode>>,
    action_panel: Option<Res<crate::build_ui::ActionPanelState>>,
    mut debug: ResMut<DebugPresentation>,
) {
    if keys.just_pressed(KeyCode::F1) {
        debug.overlays = !debug.overlays;
    }
    let build_menu_open = action_panel
        .as_ref()
        .is_some_and(|panel| panel.mode == crate::build_ui::ActionPanelMode::BuildMenu);
    if !build_menu_open && keys.just_pressed(KeyCode::KeyH) {
        debug.health_bars = !debug.health_bars;
    }
}

#[derive(SystemParam)]
struct CameraControlResources<'w> {
    benchmark: Option<Res<'w, BenchmarkCamera>>,
    time: Res<'w, Time>,
    fixed_time: Res<'w, Time<Fixed>>,
    playback: Res<'w, SimulationPlayback>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse_buttons: Res<'w, ButtonInput<MouseButton>>,
    metrics: Res<'w, WorldMetrics>,
    terrain: Res<'w, TerrainSurface>,
    hotkey_capture: Option<Res<'w, crate::build_ui::ActionPanelHotkeyCapture>>,
    samples: Res<'w, PresentationSamples>,
    camera_focus: ResMut<'w, CameraFocusRequest>,
    portrait_hold: Res<'w, crate::inspection::PortraitCameraHold>,
}

fn update_camera(
    mut mouse_wheel: MessageReader<MouseWheel>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut camera: Single<(&Camera, &mut RtsCamera, &mut Transform), With<Camera3d>>,
    mut resources: CameraControlResources<'_>,
) {
    if resources
        .benchmark
        .as_ref()
        .is_some_and(|camera| camera.lock_input)
    {
        mouse_wheel.clear();
        return;
    }
    let (camera_component, rig, transform) = &mut *camera;

    if let Some(target) = resources.camera_focus.0.take() {
        if let Some(builder) = resources.samples.current.builders.get(&target) {
            rig.focus = sim_point_to_terrain_world(builder.position, &resources.terrain);
            rig.grab_anchor = None;
        } else if let Some(building) = resources.samples.current.buildings.get(&target) {
            let (mut center, _) = resources.metrics.footprint_center_size(building.footprint);
            center.y = resources.terrain.height_at_world(center.xz());
            rig.focus = center;
            rig.grab_anchor = None;
        } else if let Some(unit) = resources.samples.current.units.get(&target) {
            rig.focus = sim_point_to_terrain_world(unit.position, &resources.terrain);
            rig.grab_anchor = None;
        }
    }

    let tracking = resources.portrait_hold.0.and_then(|target| {
        tracked_camera_focus(
            target,
            &resources.samples,
            &resources.metrics,
            &resources.terrain,
            resources
                .playback
                .interpolation_alpha(&resources.fixed_time),
        )
    });
    if let Some(focus) = tracking {
        rig.focus = focus;
        rig.grab_anchor = None;
    }

    if tracking.is_none()
        && resources.mouse_buttons.just_pressed(MouseButton::Middle)
        && let Some(cursor) = window.cursor_position()
    {
        let camera_global = GlobalTransform::from(**transform);
        rig.grab_anchor =
            viewport_ground_point(camera_component, &camera_global, cursor, &resources.terrain);
    }

    let dt = resources.time.delta_secs();
    let forward = Vec3::new(-rig.yaw.sin(), 0.0, -rig.yaw.cos());
    let right = Vec3::new(rig.yaw.cos(), 0.0, -rig.yaw.sin());
    let camera_key_pressed = |key| {
        resources.keys.pressed(key)
            && !control_modifier_pressed(&resources.keys)
            && !resources
                .hotkey_capture
                .as_ref()
                .is_some_and(|capture| capture.captures(key))
    };
    let mut movement = Vec3::ZERO;
    if camera_key_pressed(KeyCode::KeyW) || resources.keys.pressed(KeyCode::ArrowUp) {
        movement += forward;
    }
    if camera_key_pressed(KeyCode::KeyS) || resources.keys.pressed(KeyCode::ArrowDown) {
        movement -= forward;
    }
    if camera_key_pressed(KeyCode::KeyD) || resources.keys.pressed(KeyCode::ArrowRight) {
        movement += right;
    }
    if camera_key_pressed(KeyCode::KeyA) || resources.keys.pressed(KeyCode::ArrowLeft) {
        movement -= right;
    }
    if window.focused
        && !resources.mouse_buttons.pressed(MouseButton::Middle)
        && let Some(cursor) = window.cursor_position()
    {
        let edge = camera_edge_scroll_axes(
            cursor,
            Vec2::new(window.width(), window.height()),
            CAMERA_EDGE_SCROLL_MARGIN,
        );
        movement += right * edge.x + forward * edge.y;
    }
    if tracking.is_none() {
        pan_camera_focus(rig, movement, dt);
    }
    if camera_key_pressed(KeyCode::KeyQ) {
        rig.yaw += 0.9 * dt;
    }
    if camera_key_pressed(KeyCode::KeyE) {
        rig.yaw -= 0.9 * dt;
    }
    let scroll: f32 = mouse_wheel.read().map(|event| event.y).sum();
    if scroll != 0.0 {
        rig.distance *= (1.0 - scroll * 0.10).clamp(0.55, 1.45);
    }
    let world_size = resources.metrics.world_size();
    rig.distance = rig.distance.clamp(
        world_size.min_element() * CAMERA_MIN_DISTANCE_FACTOR,
        CAMERA_MAX_DISTANCE_WORLD,
    );
    if tracking.is_none() && resources.keys.just_pressed(KeyCode::Home) {
        rig.focus = resources.metrics.world_center();
        rig.focus.y = resources.terrain.height_at_world(rig.focus.xz());
        rig.distance = CAMERA_DEFAULT_DISTANCE_WORLD;
        rig.yaw = 0.0;
    }

    if tracking.is_none()
        && resources.mouse_buttons.pressed(MouseButton::Middle)
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

fn tracked_camera_focus(
    target: SimId,
    samples: &PresentationSamples,
    metrics: &WorldMetrics,
    terrain: &TerrainSurface,
    alpha: f32,
) -> Option<Vec3> {
    if let Some(builder) = samples.current.builders.get(&target) {
        let previous = samples.previous.builders.get(&target).unwrap_or(builder);
        return Some(sim_point_to_terrain_world_lerp(
            previous.position,
            builder.position,
            alpha,
            terrain,
        ));
    }
    if let Some(unit) = samples.current.units.get(&target) {
        let previous = samples.previous.units.get(&target).unwrap_or(unit);
        return Some(sim_point_to_terrain_world_lerp(
            previous.position,
            unit.position,
            alpha,
            terrain,
        ));
    }
    samples.current.buildings.get(&target).map(|building| {
        let (mut center, _) = metrics.footprint_center_size(building.footprint);
        center.y = terrain.height_at_world(center.xz());
        center
    })
}

fn pan_camera_focus(rig: &mut RtsCamera, movement: Vec3, dt: f32) {
    if movement != Vec3::ZERO {
        rig.focus += movement.normalize() * CAMERA_PAN_SPEED_WORLD_PER_SECOND * dt;
    }
}

fn camera_edge_scroll_axes(cursor: Vec2, window_size: Vec2, margin: f32) -> Vec2 {
    if margin <= 0.0 || window_size.x <= 0.0 || window_size.y <= 0.0 {
        return Vec2::ZERO;
    }
    let mut axes = Vec2::ZERO;
    if cursor.x <= margin {
        axes.x -= 1.0;
    }
    if cursor.x >= window_size.x - margin {
        axes.x += 1.0;
    }
    if cursor.y <= margin {
        axes.y += 1.0;
    }
    if cursor.y >= window_size.y - margin {
        axes.y -= 1.0;
    }
    axes
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

pub(crate) fn player_color(player: PlayerId) -> Color {
    match player.0 {
        0 => Color::srgb(1.000, 0.012, 0.012),  // red
        1 => Color::srgb(0.000, 0.259, 1.000),  // blue
        2 => Color::srgb(0.110, 0.902, 0.725),  // teal
        3 => Color::srgb(0.329, 0.000, 0.506),  // purple
        4 => Color::srgb(1.000, 0.988, 0.004),  // yellow
        5 => Color::srgb(0.996, 0.541, 0.055),  // orange
        6 => Color::srgb(0.125, 0.753, 0.000),  // green
        7 => Color::srgb(0.898, 0.357, 0.690),  // pink
        8 => Color::srgb(0.584, 0.588, 0.592),  // gray
        9 => Color::srgb(0.494, 0.749, 0.945),  // light blue
        10 => Color::srgb(0.063, 0.384, 0.275), // dark green
        11 => Color::srgb(0.306, 0.165, 0.016), // brown
        12 => Color::srgb(0.608, 0.000, 0.000), // maroon
        13 => Color::srgb(0.000, 0.000, 0.765), // navy
        14 => Color::srgb(0.000, 0.918, 1.000), // turquoise
        15 => Color::srgb(0.745, 0.000, 0.996), // violet
        16 => Color::srgb(0.922, 0.804, 0.529), // wheat
        17 => Color::srgb(0.973, 0.643, 0.545), // peach
        18 => Color::srgb(0.749, 1.000, 0.502), // mint
        19 => Color::srgb(0.863, 0.725, 0.922), // lavender
        20 => Color::srgb(0.157, 0.157, 0.157), // coal
        21 => Color::srgb(0.922, 0.941, 1.000), // snow
        22 => Color::srgb(0.000, 0.471, 0.118), // emerald
        23 => Color::srgb(0.643, 0.435, 0.200), // peanut
        _ => Color::srgb(0.78, 0.78, 0.80),
    }
}

fn owner_color(owner: Option<PlayerId>) -> Color {
    owner.map_or(Color::srgb(0.78, 0.78, 0.80), player_color)
}

#[cfg(test)]
mod tests {
    use castle_fight_sim::{CorpseDefinitionId, NavCell, Team, TerrainElevationMap};

    use super::*;
    use crate::bridge::PresentationSnapshot;

    fn original_terrain() -> TerrainSurface {
        TerrainSurface::new(
            TerrainElevationMap::from_wc3_terrain_json(include_str!(
                "../../../docs/original_map/extracted/terrain.json"
            ))
            .unwrap(),
        )
    }

    #[test]
    fn map_grid_uses_selected_release_production_building_footprint_scale() {
        let demo = crate::demo::create_demo_world(1, None);
        let selected_match = SelectedMatch {
            content: demo.content,
            direct_buildings: demo.direct_buildings,
            local_player: PlayerId(0),
        };
        assert_eq!(map_grid_base_cells(&selected_match), Some(4));
        assert_eq!(demo.metrics.navigation_cell_world() * 4.0, 128.0);
    }

    #[test]
    fn map_boundary_and_grid_geometry_come_from_authoritative_build_regions() {
        let demo = crate::demo::create_demo_world(1, None);
        let regions = demo.metrics.build_regions().collect::<Vec<_>>();
        assert_eq!(regions.len(), 2);

        let (build_min, build_max) = demo.metrics.buildable_world_bounds();
        assert_eq!(build_min, Vec2::new(-6_176.0, -2_048.0));
        assert_eq!(build_max, Vec2::new(6_176.0, 2_048.0));
        assert_eq!(demo.metrics.buildable_world_x_bounds(), (-6_176.0, 6_176.0));

        let spacing = demo.metrics.navigation_cell_world() * 4.0;
        assert!(
            !demo
                .metrics
                .map_grid_cell_unblocked(Vec2::new(4_000.0, 0.0))
        );
        assert!(
            !demo
                .metrics
                .map_grid_cell_unblocked(Vec2::new(5_760.0, 320.0))
        );
        assert!(
            demo.metrics
                .map_grid_cell_unblocked(Vec2::new(5_760.0, 0.0))
        );
        assert!(
            !demo
                .metrics
                .map_grid_cell_unblocked(Vec2::new(5_440.0, 0.0))
        );
        assert!(
            !demo
                .metrics
                .map_grid_cell_unblocked(Vec2::new(6_048.0, 0.0))
        );

        // Grid lines bound build squares. The left keeps its authored phase, while the right
        // remains flush with its outside edge and gains exactly one square toward the centre.
        assert_eq!(regions[0], BuildingFootprint::new(-193, -64, 133, 128));
        assert_eq!(regions[1], BuildingFootprint::new(60, -64, 133, 128));
        let grid_origin = (build_min + build_max) * 0.5 + Vec2::splat(spacing * 0.5);
        let build_center_x = (build_min.x + build_max.x) * 0.5;
        let (left_min, left_max) = demo.metrics.build_region_world_bounds(regions[0]);
        let (right_min, right_max) = demo.metrics.build_region_world_bounds(regions[1]);
        let left_layout = map_grid_horizontal_layout(
            regions[0],
            left_min.x,
            left_max.x,
            4,
            spacing,
            build_center_x,
        )
        .unwrap();
        let right_layout = map_grid_horizontal_layout(
            regions[1],
            right_min.x,
            right_max.x,
            4,
            spacing,
            build_center_x,
        )
        .unwrap();
        assert_eq!(
            (left_layout.anchor, left_layout.opposite),
            (-6_176.0, -2_080.0)
        );
        assert_eq!(
            (right_layout.opposite, right_layout.anchor),
            (1_952.0, 6_176.0)
        );
        let vertical =
            map_grid_axis_line_bounds(left_min.y, left_max.y, spacing, grid_origin.y).unwrap();
        assert_eq!(
            (
                grid_origin.y + vertical.0 as f32 * spacing,
                grid_origin.y + vertical.1 as f32 * spacing,
            ),
            (-1_984.0, 1_984.0)
        );
        assert_eq!(
            demo.metrics.placement_grid_anchor_cells(regions[0], 4),
            Some(IVec2::new(-193, 58))
        );
        assert_eq!(
            demo.metrics.placement_grid_anchor_cells(regions[1], 4),
            Some(IVec2::new(189, 58))
        );
        let left_top = demo.metrics.snapped_footprint_at_world(
            Vec3::new(-6_111.0, 0.0, 1_921.0),
            Team(0),
            4,
            4,
        );
        let right_top = demo.metrics.snapped_footprint_at_world(
            Vec3::new(6_113.0, 0.0, 1_921.0),
            Team(1),
            4,
            4,
        );
        let right_inner = demo.metrics.snapped_footprint_at_world(
            Vec3::new(2_017.0, 0.0, -1_950.0),
            Team(1),
            4,
            4,
        );
        assert_eq!(left_top, BuildingFootprint::new(-193, 58, 4, 4));
        assert_eq!(right_top, BuildingFootprint::new(189, 58, 4, 4));
        assert_eq!(right_inner, BuildingFootprint::new(61, -62, 4, 4));
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
            source_owner: PlayerId(0),
            source_team: Team(0),
            definition: CorpseDefinitionId(u32::from_be_bytes(*b"n015")),
            created_tick: 100,
            decay_start_tick: 190,
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
    fn death_animation_advances_at_native_rate_then_holds_final_pose() {
        let early = CorpseAnimationPhase {
            state: ImportedUnitAnimationState::Death,
            elapsed_ticks: 15,
            duration_ticks: 153,
        };
        assert!((corpse_animation_seek_seconds(early, 1.5) - 0.5).abs() < 1.0e-6);

        let held = CorpseAnimationPhase {
            state: ImportedUnitAnimationState::Death,
            elapsed_ticks: 120,
            duration_ticks: 153,
        };
        assert!((corpse_animation_seek_seconds(held, 1.5) - 1.5).abs() < 1.0e-6);

        let decay = CorpseAnimationPhase {
            state: ImportedUnitAnimationState::DecayFlesh,
            elapsed_ticks: FLESH_DECAY_TICKS / 2,
            duration_ticks: FLESH_DECAY_TICKS,
        };
        assert!((corpse_animation_seek_seconds(decay, 2.0) - 1.0).abs() < 1.0e-6);
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
    fn camera_edge_scroll_axes_follow_screen_edges_and_corners() {
        let size = Vec2::new(1_440.0, 900.0);
        assert_eq!(
            camera_edge_scroll_axes(Vec2::new(4.0, 450.0), size, CAMERA_EDGE_SCROLL_MARGIN),
            Vec2::new(-1.0, 0.0)
        );
        assert_eq!(
            camera_edge_scroll_axes(Vec2::new(1_436.0, 450.0), size, CAMERA_EDGE_SCROLL_MARGIN),
            Vec2::new(1.0, 0.0)
        );
        assert_eq!(
            camera_edge_scroll_axes(Vec2::new(720.0, 4.0), size, CAMERA_EDGE_SCROLL_MARGIN),
            Vec2::new(0.0, 1.0)
        );
        assert_eq!(
            camera_edge_scroll_axes(Vec2::new(4.0, 896.0), size, CAMERA_EDGE_SCROLL_MARGIN),
            Vec2::new(-1.0, -1.0)
        );
        assert_eq!(
            camera_edge_scroll_axes(Vec2::new(720.0, 450.0), size, CAMERA_EDGE_SCROLL_MARGIN),
            Vec2::ZERO
        );
    }

    #[test]
    fn camera_pan_speed_is_independent_of_zoom_distance() {
        let mut near = RtsCamera {
            focus: Vec3::ZERO,
            distance: 1_433.6,
            yaw: 0.0,
            grab_anchor: None,
        };
        let mut far = RtsCamera {
            focus: Vec3::ZERO,
            distance: 8_000.0,
            yaw: 0.0,
            grab_anchor: None,
        };
        pan_camera_focus(&mut near, Vec3::X, 0.5);
        pan_camera_focus(&mut far, Vec3::X, 0.5);
        assert_eq!(near.focus, far.focus);
        assert_eq!(near.focus, Vec3::new(3_000.0, 0.0, 0.0));
    }

    #[test]
    fn initial_camera_focus_uses_local_players_builder() {
        let demo = crate::demo::create_demo_world(1, None);
        let samples = PresentationSamples::new(PresentationSnapshot::capture(&demo.simulation));
        let terrain = TerrainSurface::new(demo.terrain);

        for player in [PlayerId(0), PlayerId(6)] {
            let builder = samples
                .current
                .builders
                .values()
                .find(|builder| builder.owner == player)
                .expect("development player has a builder");
            assert_eq!(
                initial_camera_focus(player, &samples, &demo.metrics, &terrain, false),
                sim_point_to_terrain_world(builder.position, &terrain)
            );
            assert_eq!(
                tracked_camera_focus(builder.id, &samples, &demo.metrics, &terrain, 1.0),
                Some(sim_point_to_terrain_world(builder.position, &terrain))
            );
        }
        assert_eq!(
            tracked_camera_focus(SimId(u64::MAX), &samples, &demo.metrics, &terrain, 1.0),
            None
        );
    }

    #[test]
    fn benchmark_camera_focus_uses_terrain_height_at_map_center() {
        let demo = crate::demo::create_demo_world(1, Some(500));
        let samples = PresentationSamples::new(PresentationSnapshot::capture(&demo.simulation));
        let terrain = TerrainSurface::new(demo.terrain);
        let center = demo.metrics.world_center();
        let expected = Vec3::new(center.x, terrain.height_at_world(center.xz()), center.z);
        for player in [PlayerId(0), PlayerId(6)] {
            assert_eq!(
                initial_camera_focus(player, &samples, &demo.metrics, &terrain, true),
                expected
            );
            assert_ne!(
                initial_camera_focus(player, &samples, &demo.metrics, &terrain, false),
                expected
            );
        }
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
            owner: Some(PlayerId(0)),
            team: Team(0),
            footprint: BuildingFootprint::new(0, 0, 4, 4),
            health: 1_000,
            health_max: 1_000,
            construction_started_tick: None,
            construction_complete_tick: None,
            attack: None,
            damage_type: None,
            armor: castle_fight_sim::ArmorProfile::UNARMORED,
            target: None,
            next_spawn_tick: Some(40),
            production_queue: Some(2),
            production_interval_ticks: Some(20),
            cooldown_remaining: None,
            mana_current: None,
            mana_maximum: None,
            ability_ready_tick: None,
            ability_autocast_enabled: None,
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
    fn imported_building_model_rebuilds_once_when_construction_finishes() {
        let rawcode = u32::from_be_bytes(*b"h02I");
        assert!(!building_presentation_needs_rebuild(
            Some(rawcode),
            true,
            Some(rawcode),
            true,
        ));
        assert!(building_presentation_needs_rebuild(
            Some(rawcode),
            true,
            Some(rawcode),
            false,
        ));
        assert!(!building_presentation_needs_rebuild(
            Some(rawcode),
            false,
            Some(rawcode),
            false,
        ));
        assert!(!building_presentation_needs_rebuild(
            None, true, None, false,
        ));
    }

    #[test]
    fn construction_progress_uses_the_same_overhead_progress_bar() {
        let building = BuildingSample {
            id: SimId(1),
            content: None,
            owner: Some(PlayerId(0)),
            team: Team(0),
            footprint: BuildingFootprint::new(0, 0, 4, 4),
            health: 1_000,
            health_max: 1_000,
            construction_started_tick: Some(20),
            construction_complete_tick: Some(80),
            attack: None,
            damage_type: None,
            armor: castle_fight_sim::ArmorProfile::UNARMORED,
            target: None,
            next_spawn_tick: None,
            production_queue: None,
            production_interval_ticks: None,
            cooldown_remaining: None,
            mana_current: None,
            mana_maximum: None,
            ability_ready_tick: None,
            ability_autocast_enabled: None,
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
    fn health_bar_buffer_growth_rebinds_the_material() {
        let mut shader_buffers = Assets::<ShaderBuffer>::default();
        let mut health_bar_materials = Assets::<HealthBarMaterial>::default();
        let gpu_data = vec![[0.0; 4]; 1 + HEALTH_BAR_BATCH_MIN_BUFFER_RECTS * 2];
        let old_buffer = shader_buffers.add(ShaderBuffer::from(gpu_data.clone()));
        let material = health_bar_materials.add(HealthBarMaterial {
            rect_data: old_buffer.clone(),
        });
        let rect = HealthBarRect {
            min: Vec2::ZERO,
            max: Vec2::ONE,
            color: [1.0; 4],
        };
        let mut batch = HealthBarBatch {
            buffer: old_buffer.clone(),
            material: material.clone(),
            rects: vec![rect; HEALTH_BAR_BATCH_MIN_BUFFER_RECTS + 1],
            gpu_data,
            gpu_capacity_rects: HEALTH_BAR_BATCH_MIN_BUFFER_RECTS,
            last_rect_count: 0,
        };

        upload_health_bar_rects(&mut batch, &mut health_bar_materials, &mut shader_buffers);

        assert_eq!(batch.gpu_capacity_rects, 128);
        assert_ne!(batch.buffer.id(), old_buffer.id());
        assert_eq!(
            health_bar_materials
                .get(&material)
                .expect("test health bar material should exist")
                .rect_data
                .id(),
            batch.buffer.id()
        );
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

    #[test]
    fn building_elevation_uses_highest_terrain_under_footprint() {
        let terrain = original_terrain();
        let config = SimulationConfig {
            navigation_cell_size: 32 * SUBUNITS_PER_WORLD_UNIT,
            ..SimulationConfig::default()
        };
        let metrics = WorldMetrics::from_simulation_config(&config);
        // This 4x4 site crosses the authored ramp north-east of the left castle. Its centre is
        // substantially lower than one edge, which used to bury low building geosets.
        let footprint = BuildingFootprint::new(-168, 9, 4, 4);
        let (center, _) = metrics.footprint_center_size(footprint);
        let center_height = terrain.height_at_world(center.xz());
        let support_height = building_terrain_height(&metrics, &terrain, footprint);

        assert!(support_height > center_height + 30.0);
        assert!(support_height >= 574.0);
    }
}
