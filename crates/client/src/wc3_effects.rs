use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    fs,
    path::{Component, Path},
};

use bevy::{
    asset::{AssetId, RenderAssetUsages},
    camera::visibility::NoFrustumCulling,
    gltf::GltfMaterialExtras,
    mesh::{Indices, PrimitiveTopology, skinning::SkinnedMesh},
    prelude::*,
    render::render_resource::TextureFormat,
};
use serde::Deserialize;

use crate::terrain::client_asset_root;

const EFFECT_MANIFEST: &str = "wc3/effects/manifest.json";
const EFFECT_ASSET_PREFIX: &str = "wc3/effects";
const TEAM_GLOW_RED_TEXTURE: &str = "textures/replaceabletextures__teamglow__teamglow00.png";
const TEAM_GLOW_BLUE_TEXTURE: &str = "textures/replaceabletextures__teamglow__teamglow01.png";
const TEAM_COLOR_OVERLAY_DEPTH_BIAS_OFFSET: f32 = 2.0;
const TEAM_COLOR_UNDERLAY_DEPTH_BIAS_OFFSET: f32 = -1.0;
const MAX_PARTICLES_PER_EMITTER_PER_FRAME: u32 = 12;
const MAX_RIBBON_SAMPLES_PER_FRAME: u32 = 16;
const MAX_RIBBON_POINTS: usize = 512;

#[derive(Resource, Default)]
pub struct Wc3VisualSet {
    projectile_by_rawcode: BTreeMap<u32, Wc3ProjectileVisual>,
    ability_by_rawcode: BTreeMap<u32, Vec<Wc3AbilityVisual>>,
    chain_lightning_abilities: BTreeSet<u32>,
    stun: Option<Wc3VisualModel>,
}

#[derive(Clone)]
pub struct Wc3VisualModel {
    pub scene: Handle<WorldAsset>,
    pub emitters: Vec<Wc3ParticleEmitter>,
    pub ribbons: Vec<Wc3RibbonEmitter>,
}

#[derive(Clone)]
pub struct Wc3ProjectileVisual {
    pub model: Wc3VisualModel,
    pub missile_arc: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wc3AbilityVisualAnchor {
    Source,
    Target,
}

#[derive(Clone)]
pub struct Wc3AbilityVisual {
    pub model: Wc3VisualModel,
    pub anchor: Wc3AbilityVisualAnchor,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Wc3ParticleEmitter {
    pub position: [f32; 3],
    pub filter_mode: u32,
    pub speed: f32,
    pub variation: f32,
    pub latitude: f32,
    pub gravity: f32,
    pub lifespan: f32,
    pub emission_rate: f32,
    pub rows: u32,
    pub columns: u32,
    pub segment_colors: [[f32; 3]; 3],
    pub segment_alpha: [u8; 3],
    pub segment_scaling: [f32; 3],
    pub texture: Option<String>,
    pub squirt: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Wc3RibbonEmitter {
    pub position: [f32; 3],
    pub height_above: f32,
    pub height_below: f32,
    pub alpha: f32,
    pub color: [f32; 3],
    pub lifespan: f32,
    pub emission_rate: u32,
    pub rows: u32,
    pub columns: u32,
    pub filter_mode: String,
    pub texture: Option<String>,
    pub gravity: f32,
}

#[derive(Debug, Deserialize)]
struct VisualManifest {
    schema_version: u32,
    assets: Vec<VisualBinding>,
    chain_lightning_abilities: Vec<String>,
    stun: Option<VisualBinding>,
    models: Vec<ModelManifest>,
}

#[derive(Debug, Clone, Deserialize)]
struct VisualBinding {
    owner_kind: String,
    owner_rawcode: String,
    role: String,
    gltf: Option<String>,
    #[serde(default)]
    missile_arc: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct ModelManifest {
    gltf: String,
    #[serde(default)]
    particle_emitters: Vec<Wc3ParticleEmitter>,
    #[serde(default)]
    ribbon_emitters: Vec<Wc3RibbonEmitter>,
}

#[derive(Component)]
pub struct Wc3EmitterSource {
    emitters: Vec<EmitterRuntime>,
}

#[derive(Component)]
pub struct Wc3RibbonSource {
    ribbons: Vec<Wc3RibbonEmitter>,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct Wc3TeamTint {
    pub index: u8,
    pub color: Color,
    asset_prefix: &'static str,
}

impl Wc3TeamTint {
    #[must_use]
    pub const fn new(index: u8, color: Color, asset_prefix: &'static str) -> Self {
        Self {
            index,
            color,
            asset_prefix,
        }
    }
}

#[derive(Component)]
pub(crate) struct Wc3MaterialProcessed;

#[derive(Clone)]
struct EmitterRuntime {
    spec: Wc3ParticleEmitter,
    accumulator: f32,
    burst_pending: bool,
    sequence: u32,
}

#[derive(Component)]
pub struct Wc3Particle {
    velocity: Vec3,
    gravity: f32,
    age: f32,
    lifespan: f32,
    scales: [f32; 3],
}

#[derive(Debug, Clone, Copy)]
struct RibbonPoint {
    center: Vec3,
    up: Vec3,
    age: f32,
}

#[derive(Component)]
pub(crate) struct Wc3RibbonTrail {
    source: Entity,
    spec: Wc3RibbonEmitter,
    points: VecDeque<RibbonPoint>,
    emission_accumulator: f32,
    previous_origin: Option<Vec3>,
    previous_up: Option<Vec3>,
    mesh: Handle<Mesh>,
}

#[derive(Resource)]
pub struct Wc3ParticleAssets {
    particle_quads: HashMap<(u32, u32, u32), Handle<Mesh>>,
    materials: HashMap<String, Handle<StandardMaterial>>,
}

impl Wc3VisualSet {
    #[must_use]
    pub fn load_default(asset_server: &AssetServer) -> Self {
        let asset_root = client_asset_root();
        let manifest_path = asset_root.join(EFFECT_MANIFEST);
        if !manifest_path.is_file() {
            return Self::default();
        }
        match load_manifest(&manifest_path, asset_server) {
            Ok(set) => {
                println!(
                    "Loaded {} WC3 projectile visual(s), {} ability visual(s), and {} chain-lightning id(s)",
                    set.projectile_by_rawcode.len(),
                    set.ability_by_rawcode.len(),
                    set.chain_lightning_abilities.len()
                );
                set
            }
            Err(error) => {
                eprintln!("warning: ignoring generated WC3 effect assets: {error}");
                Self::default()
            }
        }
    }

    #[must_use]
    pub fn projectile(&self, rawcode: u32) -> Option<&Wc3ProjectileVisual> {
        self.projectile_by_rawcode.get(&rawcode)
    }

    #[must_use]
    pub fn ability(&self, rawcode: u32) -> &[Wc3AbilityVisual] {
        self.ability_by_rawcode
            .get(&rawcode)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    #[must_use]
    pub fn is_chain_lightning(&self, rawcode: u32) -> bool {
        self.chain_lightning_abilities.contains(&rawcode)
    }

    #[must_use]
    pub fn stun(&self) -> Option<&Wc3VisualModel> {
        self.stun.as_ref()
    }
}

impl Wc3EmitterSource {
    #[must_use]
    pub fn new(emitters: &[Wc3ParticleEmitter]) -> Self {
        Self {
            emitters: emitters
                .iter()
                .cloned()
                .map(|spec| EmitterRuntime {
                    burst_pending: spec.squirt,
                    spec,
                    accumulator: 0.0,
                    sequence: 0,
                })
                .collect(),
        }
    }
}

impl Wc3RibbonSource {
    #[must_use]
    pub fn new(ribbons: &[Wc3RibbonEmitter]) -> Self {
        Self {
            ribbons: ribbons.to_vec(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Wc3MaterialExtras {
    #[serde(rename = "wc3FilterMode")]
    filter_mode: Option<String>,
    #[serde(rename = "wc3PriorityPlane", default)]
    priority_plane: i32,
    #[serde(rename = "wc3TeamColorUnderlay", default)]
    team_color_underlay: bool,
    #[serde(rename = "wc3TeamGlowLayer", default)]
    team_glow_layer: bool,
}

impl Wc3ParticleAssets {
    pub fn new(meshes: &mut Assets<Mesh>) -> Self {
        let default_quad = meshes.add(build_particle_quad_mesh(1, 1, 0));
        let mut particle_quads = HashMap::new();
        particle_quads.insert((1, 1, 0), default_quad);
        Self {
            particle_quads,
            materials: HashMap::new(),
        }
    }

    fn particle_mesh(
        &mut self,
        rows: u32,
        columns: u32,
        frame: u32,
        meshes: &mut Assets<Mesh>,
    ) -> Handle<Mesh> {
        let rows = rows.max(1);
        let columns = columns.max(1);
        let frame_count = rows.saturating_mul(columns).max(1);
        let frame = frame.min(frame_count - 1);
        let key = (rows, columns, frame);
        if let Some(handle) = self.particle_quads.get(&key) {
            return handle.clone();
        }
        let handle = meshes.add(build_particle_quad_mesh(rows, columns, frame));
        self.particle_quads.insert(key, handle.clone());
        handle
    }

    fn material(
        &mut self,
        emitter: &Wc3ParticleEmitter,
        asset_server: &AssetServer,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        let color = emitter.segment_colors[0];
        let alpha = f32::from(emitter.segment_alpha[0]) / 255.0;
        let texture_key = emitter.texture.as_deref().unwrap_or("<none>");
        let key = format!(
            "{texture_key}|{}|{:.3}|{:.3}|{:.3}|{alpha:.3}",
            emitter.filter_mode, color[0], color[1], color[2]
        );
        if let Some(handle) = self.materials.get(&key) {
            return handle.clone();
        }
        let base_color = Color::srgba(color[0], color[1], color[2], alpha.max(0.05));
        let base_color_texture = emitter
            .texture
            .as_ref()
            .map(|texture| asset_server.load(format!("{EFFECT_ASSET_PREFIX}/{texture}")));
        let handle = materials.add(StandardMaterial {
            base_color,
            base_color_texture,
            emissive: LinearRgba::new(color[0], color[1], color[2], 1.0),
            alpha_mode: particle_alpha_mode(emitter.filter_mode),
            unlit: true,
            double_sided: true,
            ..default()
        });
        self.materials.insert(key, handle.clone());
        handle
    }

    fn ribbon_material(
        &mut self,
        ribbon: &Wc3RibbonEmitter,
        asset_server: &AssetServer,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        let texture_key = ribbon.texture.as_deref().unwrap_or("<none>");
        let key = format!(
            "ribbon|{texture_key}|{}|{:.3}|{:.3}|{:.3}|{:.3}",
            ribbon.filter_mode, ribbon.color[0], ribbon.color[1], ribbon.color[2], ribbon.alpha
        );
        if let Some(handle) = self.materials.get(&key) {
            return handle.clone();
        }
        let base_color_texture = ribbon
            .texture
            .as_ref()
            .map(|texture| asset_server.load(format!("{EFFECT_ASSET_PREFIX}/{texture}")));
        let handle = materials.add(StandardMaterial {
            base_color: Color::srgba(
                ribbon.color[0],
                ribbon.color[1],
                ribbon.color[2],
                ribbon.alpha.clamp(0.0, 1.0),
            ),
            base_color_texture,
            alpha_mode: wc3_material_alpha_mode(&ribbon.filter_mode, AlphaMode::Blend),
            unlit: true,
            double_sided: true,
            ..default()
        });
        self.materials.insert(key, handle.clone());
        handle
    }
}

fn particle_alpha_mode(filter_mode: u32) -> AlphaMode {
    match filter_mode {
        0 => AlphaMode::Blend,
        1 => AlphaMode::Add,
        2 | 3 => AlphaMode::Multiply,
        4 => AlphaMode::Mask(0.5),
        _ => AlphaMode::Blend,
    }
}

fn particle_atlas_uv_rect(rows: u32, columns: u32, frame: u32) -> [f32; 4] {
    let rows = rows.max(1);
    let columns = columns.max(1);
    let frame_count = rows.saturating_mul(columns).max(1);
    let frame = frame.min(frame_count - 1);
    let column = frame % columns;
    let row = frame / columns;
    let inv_columns = 1.0 / columns as f32;
    let inv_rows = 1.0 / rows as f32;
    [
        column as f32 * inv_columns,
        row as f32 * inv_rows,
        (column + 1) as f32 * inv_columns,
        (row + 1) as f32 * inv_rows,
    ]
}

fn build_particle_quad_mesh(rows: u32, columns: u32, frame: u32) -> Mesh {
    let [u0, v0, u1, v1] = particle_atlas_uv_rect(rows, columns, frame);

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-0.5, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.5, 0.5, 0.0],
            [-0.5, 0.5, 0.0],
        ],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4])
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
    )
    .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}

fn wc3_material_alpha_mode(filter_mode: &str, fallback: AlphaMode) -> AlphaMode {
    match filter_mode {
        "Transparent" => AlphaMode::Mask(0.5),
        "Blend" => AlphaMode::Blend,
        "Additive" | "AddAlpha" => AlphaMode::Add,
        "Modulate" | "Modulate2x" => AlphaMode::Multiply,
        "None" => fallback,
        _ => fallback,
    }
}

type TeamMaterialCache = HashMap<(AssetId<StandardMaterial>, u8), Handle<StandardMaterial>>;
type TeamImageCache = HashMap<(AssetId<Image>, u8), Handle<Image>>;

type Wc3MaterialWorld<'w, 's> = (
    Res<'w, AssetServer>,
    Query<'w, 's, &'static ChildOf>,
    Query<'w, 's, &'static Wc3TeamTint>,
);

type Wc3MaterialAssets<'w, 's> = (
    ResMut<'w, Assets<StandardMaterial>>,
    ResMut<'w, Assets<Image>>,
    Local<'s, TeamMaterialCache>,
    Local<'s, TeamMaterialCache>,
    Local<'s, TeamImageCache>,
);

type Wc3MaterialMeshQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Mesh3d,
        &'static mut MeshMaterial3d<StandardMaterial>,
        &'static GltfMaterialExtras,
        Option<&'static SkinnedMesh>,
    ),
    Without<Wc3MaterialProcessed>,
>;

pub fn fix_wc3_scene_materials(
    mut commands: Commands,
    world: Wc3MaterialWorld<'_, '_>,
    material_assets: Wc3MaterialAssets<'_, '_>,
    mut meshes: Wc3MaterialMeshQuery<'_, '_>,
) {
    let (asset_server, parents, team_roots) = world;
    let (mut materials, mut images, mut team_materials, mut team_glow_materials, mut team_images) =
        material_assets;
    'mesh: for (entity, mesh, mut material_handle, raw_extras, skin) in &mut meshes {
        let Ok(extras) = serde_json::from_str::<Wc3MaterialExtras>(&raw_extras.value) else {
            commands.entity(entity).insert(Wc3MaterialProcessed);
            continue;
        };
        if extras.filter_mode.is_none() && !extras.team_color_underlay && !extras.team_glow_layer {
            commands.entity(entity).insert(Wc3MaterialProcessed);
            continue;
        }

        let source_material_id = material_handle.0.id();
        let team = wc3_team_tint(entity, &parents, &team_roots);
        let building_team_color = team.is_some_and(|team| team.asset_prefix == "wc3/buildings");
        let overlay_depth_bias = if building_team_color {
            0.0
        } else {
            TEAM_COLOR_OVERLAY_DEPTH_BIAS_OFFSET
        };
        let material_template = {
            let Some(mut material) = materials.get_mut(&material_handle.0) else {
                continue;
            };
            if let Some(filter_mode) = extras.filter_mode.as_deref() {
                material.alpha_mode = wc3_material_alpha_mode(filter_mode, material.alpha_mode);
            }
            material.depth_bias = wc3_material_depth_bias(
                extras.priority_plane,
                extras.team_color_underlay,
                overlay_depth_bias,
            );
            material.clone()
        };

        if extras.team_glow_layer
            && let Some(team) = team
        {
            let key = (source_material_id, team.index);
            let team_glow_handle = if let Some(handle) = team_glow_materials.get(&key) {
                handle.clone()
            } else {
                let mut team_glow = material_template.clone();
                team_glow.base_color = Color::WHITE;
                team_glow.base_color_texture =
                    Some(asset_server.load(team_glow_texture_path(team)));
                team_glow.emissive = LinearRgba::WHITE;
                team_glow.alpha_mode = AlphaMode::Add;
                team_glow.unlit = true;
                let handle = materials.add(team_glow);
                team_glow_materials.insert(key, handle.clone());
                handle
            };
            material_handle.0 = team_glow_handle;
        }

        if extras.team_color_underlay
            && let Some(team) = team
        {
            let key = (source_material_id, team.index);
            if building_team_color {
                let flattened_handle = if let Some(handle) = team_materials.get(&key) {
                    handle.clone()
                } else {
                    let Some(source_texture) = material_template.base_color_texture.clone() else {
                        warn!("WC3 building team-color material has no textured overlay");
                        commands.entity(entity).insert(Wc3MaterialProcessed);
                        continue;
                    };
                    let image_key = (source_texture.id(), team.index);
                    let flattened_texture = if let Some(handle) = team_images.get(&image_key) {
                        handle.clone()
                    } else {
                        let Some(source_image) = images.get(&source_texture) else {
                            continue 'mesh;
                        };
                        let Some(flattened_image) =
                            flatten_team_color_image(source_image, team.color)
                        else {
                            warn!(
                                "WC3 building team-color texture uses unsupported image format {:?}",
                                source_image.texture_descriptor.format
                            );
                            commands.entity(entity).insert(Wc3MaterialProcessed);
                            continue;
                        };
                        let handle = images.add(flattened_image);
                        team_images.insert(image_key, handle.clone());
                        handle
                    };
                    let mut flattened_material = material_template.clone();
                    flattened_material.base_color = Color::WHITE;
                    flattened_material.base_color_texture = Some(flattened_texture);
                    flattened_material.alpha_mode = AlphaMode::Opaque;
                    flattened_material.depth_bias = extras.priority_plane as f32;
                    let handle = materials.add(flattened_material);
                    team_materials.insert(key, handle.clone());
                    handle
                };
                material_handle.0 = flattened_handle;
            } else {
                let underlay_handle = if let Some(handle) = team_materials.get(&key) {
                    handle.clone()
                } else {
                    let underlay = team_color_underlay_material(
                        material_template,
                        team.color,
                        TEAM_COLOR_UNDERLAY_DEPTH_BIAS_OFFSET,
                    );
                    let handle = materials.add(underlay);
                    team_materials.insert(key, handle.clone());
                    handle
                };
                let mut underlay_entity = commands.spawn((
                    Mesh3d(mesh.0.clone()),
                    MeshMaterial3d(underlay_handle),
                    Transform::IDENTITY,
                    Visibility::default(),
                    NoFrustumCulling,
                ));
                if let Some(skin) = skin {
                    underlay_entity.insert(skin.clone());
                }
                let underlay_entity = underlay_entity.id();
                commands.entity(entity).add_child(underlay_entity);
            }
        }

        commands.entity(entity).insert(Wc3MaterialProcessed);
    }
}

fn wc3_material_depth_bias(
    priority_plane: i32,
    team_color_underlay: bool,
    overlay_depth_bias: f32,
) -> f32 {
    priority_plane as f32
        + if team_color_underlay {
            overlay_depth_bias
        } else {
            0.0
        }
}

fn team_color_underlay_material(
    mut material: StandardMaterial,
    color: Color,
    underlay_depth_bias: f32,
) -> StandardMaterial {
    material.base_color_texture = None;
    material.base_color = color;
    material.emissive = LinearRgba::BLACK;
    material.alpha_mode = AlphaMode::Opaque;
    material.depth_bias += underlay_depth_bias;
    material
}

fn flatten_team_color_image(source: &Image, team_color: Color) -> Option<Image> {
    let format = source.texture_descriptor.format;
    let (red_index, green_index, blue_index) = match format {
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => (0, 1, 2),
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb => (2, 1, 0),
        _ => return None,
    };
    let is_srgb = matches!(
        format,
        TextureFormat::Rgba8UnormSrgb | TextureFormat::Bgra8UnormSrgb
    );
    let team = team_color.to_linear();
    let mut flattened = source.clone();
    let data = flattened.data.as_mut()?;
    for pixel in data.as_chunks_mut::<4>().0 {
        let alpha = f32::from(pixel[3]) / 255.0;
        let source_channel = |index: usize| {
            let encoded = f32::from(pixel[index]) / 255.0;
            if is_srgb {
                srgb_channel_to_linear(encoded)
            } else {
                encoded
            }
        };
        let red = source_channel(red_index) * alpha + team.red * (1.0 - alpha);
        let green = source_channel(green_index) * alpha + team.green * (1.0 - alpha);
        let blue = source_channel(blue_index) * alpha + team.blue * (1.0 - alpha);
        let encode = |linear: f32| {
            let value = if is_srgb {
                linear_channel_to_srgb(linear)
            } else {
                linear.clamp(0.0, 1.0)
            };
            (value * 255.0).round() as u8
        };
        pixel[red_index] = encode(red);
        pixel[green_index] = encode(green);
        pixel[blue_index] = encode(blue);
        pixel[3] = 255;
    }
    Some(flattened)
}

fn srgb_channel_to_linear(channel: f32) -> f32 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_channel_to_srgb(channel: f32) -> f32 {
    let channel = channel.clamp(0.0, 1.0);
    if channel <= 0.003_130_8 {
        channel * 12.92
    } else {
        1.055 * channel.powf(1.0 / 2.4) - 0.055
    }
}

fn team_glow_texture_path(team: Wc3TeamTint) -> String {
    let texture = if team.index == 0 {
        TEAM_GLOW_BLUE_TEXTURE
    } else {
        TEAM_GLOW_RED_TEXTURE
    };
    format!("{}/{texture}", team.asset_prefix.trim_end_matches('/'))
}

fn wc3_team_tint(
    entity: Entity,
    parents: &Query<&ChildOf>,
    team_roots: &Query<&Wc3TeamTint>,
) -> Option<Wc3TeamTint> {
    let mut current = entity;
    for _ in 0..128 {
        if let Ok(team) = team_roots.get(current) {
            return Some(*team);
        }
        let Ok(parent) = parents.get(current) else {
            return None;
        };
        current = parent.parent();
    }
    None
}

pub fn spawn_wc3_ribbon_trails(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut ribbon_assets: ResMut<Wc3ParticleAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sources: Query<(Entity, &Wc3RibbonSource), Added<Wc3RibbonSource>>,
) {
    for (source, source_ribbons) in &sources {
        for spec in &source_ribbons.ribbons {
            if spec.emission_rate == 0
                || spec.lifespan <= 0.0
                || (spec.height_above <= 0.0 && spec.height_below <= 0.0)
            {
                continue;
            }
            let mesh = meshes.add(build_wc3_ribbon_mesh(spec, &VecDeque::new()));
            let material = ribbon_assets.ribbon_material(spec, &asset_server, &mut materials);
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                Transform::IDENTITY,
                Visibility::default(),
                NoFrustumCulling,
                Wc3RibbonTrail {
                    source,
                    spec: spec.clone(),
                    points: VecDeque::new(),
                    emission_accumulator: 0.0,
                    previous_origin: None,
                    previous_up: None,
                    mesh,
                },
            ));
        }
    }
}

pub fn update_wc3_ribbon_trails(
    mut commands: Commands,
    time: Res<Time>,
    sources: Query<&GlobalTransform, With<Wc3RibbonSource>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut trails: Query<(Entity, &mut Wc3RibbonTrail)>,
) {
    let dt = time.delta_secs().min(0.1);
    for (entity, mut trail) in &mut trails {
        for point in &mut trail.points {
            point.age += dt;
        }
        while trail
            .points
            .front()
            .is_some_and(|point| point.age >= trail.spec.lifespan)
        {
            trail.points.pop_front();
        }

        let source_transform = sources.get(trail.source).ok();
        if let Some(transform) = source_transform {
            let origin = transform.transform_point(Vec3::from_array(trail.spec.position));
            let up = (transform.rotation() * Vec3::Y).normalize_or(Vec3::Y);
            if trail.previous_origin.is_none() {
                trail.points.push_back(RibbonPoint {
                    center: origin,
                    up,
                    age: 0.0,
                });
            }

            trail.emission_accumulator += trail.spec.emission_rate.min(240) as f32 * dt;
            let due = trail.emission_accumulator.floor() as u32;
            trail.emission_accumulator -= due as f32;
            let count = due.min(MAX_RIBBON_SAMPLES_PER_FRAME);
            if count > 0 {
                let previous_origin = trail.previous_origin.unwrap_or(origin);
                let previous_up = trail.previous_up.unwrap_or(up);
                for sample_index in 1..=count {
                    let t = sample_index as f32 / count as f32;
                    trail.points.push_back(RibbonPoint {
                        center: previous_origin.lerp(origin, t),
                        up: previous_up.lerp(up, t).normalize_or(up),
                        age: 0.0,
                    });
                }
            }
            trail.previous_origin = Some(origin);
            trail.previous_up = Some(up);
        }

        while trail.points.len() > MAX_RIBBON_POINTS {
            trail.points.pop_front();
        }
        if let Some(mut mesh) = meshes.get_mut(&trail.mesh) {
            *mesh = build_wc3_ribbon_mesh(&trail.spec, &trail.points);
        }

        if source_transform.is_none() && trail.points.is_empty() {
            meshes.remove(trail.mesh.id());
            commands.entity(entity).despawn();
        }
    }
}

fn build_wc3_ribbon_mesh(spec: &Wc3RibbonEmitter, points: &VecDeque<RibbonPoint>) -> Mesh {
    let mut positions = Vec::with_capacity(points.len() * 2);
    let mut normals = Vec::with_capacity(points.len() * 2);
    let mut uvs = Vec::with_capacity(points.len() * 2);
    let mut colors = Vec::with_capacity(points.len() * 2);
    let mut indices = Vec::with_capacity(points.len().saturating_sub(1) * 6);
    let last = points.len().saturating_sub(1).max(1) as f32;
    let atlas_u = 1.0 / spec.columns.max(1) as f32;
    let atlas_v = 1.0 / spec.rows.max(1) as f32;

    for (index, point) in points.iter().enumerate() {
        let gravity_offset = Vec3::NEG_Y * (0.5 * spec.gravity * point.age * point.age);
        let center = point.center + gravity_offset;
        let up = point.up.normalize_or(Vec3::Y);
        let top = center + up * spec.height_above.max(0.0);
        let bottom = center - up * spec.height_below.max(0.0);
        positions.push(top.to_array());
        positions.push(bottom.to_array());
        normals.push(Vec3::Z.to_array());
        normals.push(Vec3::Z.to_array());
        let v = index as f32 / last * atlas_v;
        uvs.push([0.0, v]);
        uvs.push([atlas_u, v]);
        let fade = (1.0 - point.age / spec.lifespan.max(0.01)).clamp(0.0, 1.0);
        colors.push([1.0, 1.0, 1.0, fade]);
        colors.push([1.0, 1.0, 1.0, fade]);
    }
    for segment in 0..points.len().saturating_sub(1) {
        let top = u32::try_from(segment * 2).expect("ribbon vertex count is bounded");
        let bottom = top + 1;
        let next_top = top + 2;
        let next_bottom = top + 3;
        indices.extend_from_slice(&[top, bottom, next_top, next_top, bottom, next_bottom]);
    }

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

pub fn emit_wc3_particles(
    mut commands: Commands,
    time: Res<Time>,
    asset_server: Res<AssetServer>,
    mut particle_assets: ResMut<Wc3ParticleAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut sources: Query<(Entity, &GlobalTransform, &mut Wc3EmitterSource)>,
) {
    let dt = time.delta_secs().min(0.1);
    for (entity, transform, mut source) in &mut sources {
        let root = transform.compute_transform();
        for (emitter_index, emitter) in source.emitters.iter_mut().enumerate() {
            let mut count = if emitter.burst_pending {
                emitter.burst_pending = false;
                8
            } else {
                emitter.accumulator += emitter.spec.emission_rate.clamp(0.0, 240.0) * dt;
                let count = emitter.accumulator.floor() as u32;
                emitter.accumulator -= count as f32;
                count
            };
            count = count.min(MAX_PARTICLES_PER_EMITTER_PER_FRAME);
            if count == 0 || emitter.spec.lifespan <= 0.0 {
                continue;
            }
            let material = particle_assets.material(&emitter.spec, &asset_server, &mut materials);
            let particle_mesh = particle_assets.particle_mesh(
                emitter.spec.rows,
                emitter.spec.columns,
                0,
                &mut meshes,
            );
            let origin = transform.transform_point(Vec3::from_array(emitter.spec.position));
            for _ in 0..count {
                let sequence = emitter.sequence;
                emitter.sequence = emitter.sequence.wrapping_add(1);
                let velocity = particle_velocity(
                    root.rotation,
                    entity,
                    emitter_index as u32,
                    sequence,
                    &emitter.spec,
                );
                commands.spawn((
                    Mesh3d(particle_mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(origin)
                        .with_scale(Vec3::splat(emitter.spec.segment_scaling[0].max(0.01))),
                    Wc3Particle {
                        velocity,
                        gravity: emitter.spec.gravity,
                        age: 0.0,
                        lifespan: emitter.spec.lifespan.max(0.01),
                        scales: emitter.spec.segment_scaling,
                    },
                ));
            }
        }
    }
}

pub fn update_wc3_particles(
    mut commands: Commands,
    time: Res<Time>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut particles: Query<(Entity, &mut Transform, &mut Wc3Particle)>,
) {
    let dt = time.delta_secs().min(0.1);
    let camera_rotation = cameras.iter().next().map(GlobalTransform::rotation);
    for (entity, mut transform, mut particle) in &mut particles {
        particle.age += dt;
        if particle.age >= particle.lifespan {
            commands.entity(entity).despawn();
            continue;
        }
        particle.velocity.y -= particle.gravity * dt;
        transform.translation += particle.velocity * dt;
        if let Some(rotation) = camera_rotation {
            transform.rotation = rotation;
        }
        let t = particle.age / particle.lifespan;
        let scale = if t < 0.5 {
            particle.scales[0] + (particle.scales[1] - particle.scales[0]) * (t * 2.0)
        } else {
            particle.scales[1] + (particle.scales[2] - particle.scales[1]) * ((t - 0.5) * 2.0)
        };
        transform.scale = Vec3::splat(scale.max(0.01));
    }
}

fn particle_velocity(
    rotation: Quat,
    entity: Entity,
    emitter_index: u32,
    sequence: u32,
    emitter: &Wc3ParticleEmitter,
) -> Vec3 {
    let seed = (entity.to_bits() as u32).wrapping_mul(0x9e37_79b9)
        ^ emitter_index.wrapping_mul(0x85eb_ca6b)
        ^ sequence.wrapping_mul(0xc2b2_ae35);
    let a = hash_unit(seed);
    let b = hash_unit(seed ^ 0xa511_e9b3);
    let latitude = emitter
        .latitude
        .to_radians()
        .clamp(0.0, std::f32::consts::PI);
    let cone = latitude * a.sqrt();
    let azimuth = std::f32::consts::TAU * b;
    // Warcraft emitters use Z-up model space. The extractor converts model geometry with
    // (x, y, z) -> (x, z, -y), so apply the same basis change to emitted velocity vectors.
    // In particular, a zero-latitude emitter points along WC3 +Z, which is Bevy +Y.
    let wc3_direction = Vec3::new(
        cone.sin() * azimuth.cos(),
        cone.sin() * azimuth.sin(),
        cone.cos(),
    );
    let local_direction = wc3_direction_to_bevy(wc3_direction);
    let variation = 1.0 + (hash_unit(seed ^ 0x63d8_3595) * 2.0 - 1.0) * emitter.variation;
    rotation * local_direction * emitter.speed * variation.max(0.0)
}

fn wc3_direction_to_bevy(direction: Vec3) -> Vec3 {
    Vec3::new(direction.x, direction.z, -direction.y)
}

fn hash_unit(mut value: u32) -> f32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^= value >> 16;
    value as f32 / u32::MAX as f32
}

fn load_manifest(path: &Path, asset_server: &AssetServer) -> Result<Wc3VisualSet, String> {
    let json = fs::read_to_string(path)
        .map_err(|error| format!("failed reading {}: {error}", path.display()))?;
    let manifest: VisualManifest =
        serde_json::from_str(&json).map_err(|error| format!("invalid visual manifest: {error}"))?;
    if manifest.schema_version != 3 {
        return Err(format!(
            "unsupported visual asset manifest schema {}",
            manifest.schema_version
        ));
    }

    let model_by_gltf: BTreeMap<_, _> = manifest
        .models
        .into_iter()
        .map(|model| (model.gltf.clone(), model))
        .collect();
    let mut projectile_by_rawcode = BTreeMap::new();
    let mut ability_by_rawcode = BTreeMap::<u32, Vec<Wc3AbilityVisual>>::new();
    for binding in &manifest.assets {
        let Some(gltf) = &binding.gltf else {
            continue;
        };
        let Some(model) = model_by_gltf.get(gltf) else {
            continue;
        };
        let visual = resolve_visual_model(gltf, model, asset_server)?;
        match (binding.owner_kind.as_str(), binding.role.as_str()) {
            ("units", "attack1_projectile") => {
                let missile_arc = binding.missile_arc.unwrap_or(0.0);
                if !missile_arc.is_finite() || missile_arc < 0.0 {
                    return Err(format!(
                        "projectile {} has invalid missile arc {missile_arc}",
                        binding.owner_rawcode
                    ));
                }
                projectile_by_rawcode.insert(
                    parse_rawcode(&binding.owner_rawcode)?,
                    Wc3ProjectileVisual {
                        model: visual,
                        missile_arc,
                    },
                );
            }
            ("abilities", role @ ("target" | "effect" | "special" | "caster")) => {
                ability_by_rawcode
                    .entry(parse_rawcode(&binding.owner_rawcode)?)
                    .or_default()
                    .push(Wc3AbilityVisual {
                        model: visual,
                        anchor: ability_visual_anchor(role),
                    });
            }
            // Missile art needs an authoritative travel interval/path. Do not pin a missile
            // model to either endpoint merely because the object data references one.
            ("abilities", "missile") => {}
            _ => {}
        }
    }

    let chain_lightning_abilities = manifest
        .chain_lightning_abilities
        .iter()
        .map(|rawcode| parse_rawcode(rawcode))
        .collect::<Result<_, _>>()?;
    let stun = manifest
        .stun
        .as_ref()
        .and_then(|binding| binding.gltf.as_ref())
        .and_then(|gltf| model_by_gltf.get(gltf).map(|model| (gltf, model)))
        .map(|(gltf, model)| resolve_visual_model(gltf, model, asset_server))
        .transpose()?;

    Ok(Wc3VisualSet {
        projectile_by_rawcode,
        ability_by_rawcode,
        chain_lightning_abilities,
        stun,
    })
}

fn resolve_visual_model(
    gltf: &str,
    model: &ModelManifest,
    asset_server: &AssetServer,
) -> Result<Wc3VisualModel, String> {
    let gltf = gltf.replace('\\', "/");
    validate_relative_asset_path(&gltf)?;
    let asset_path = format!("{EFFECT_ASSET_PREFIX}/{gltf}");
    Ok(Wc3VisualModel {
        scene: asset_server.load(GltfAssetLabel::Scene(0).from_asset(asset_path)),
        emitters: model.particle_emitters.clone(),
        ribbons: model.ribbon_emitters.clone(),
    })
}

fn ability_visual_anchor(role: &str) -> Wc3AbilityVisualAnchor {
    match role {
        "caster" => Wc3AbilityVisualAnchor::Source,
        "target" | "effect" | "special" => Wc3AbilityVisualAnchor::Target,
        _ => unreachable!("only supported stationary ability visual roles are classified"),
    }
}

fn parse_rawcode(rawcode: &str) -> Result<u32, String> {
    let bytes: [u8; 4] = rawcode
        .as_bytes()
        .try_into()
        .map_err(|_| format!("visual rawcode {rawcode:?} is not exactly four bytes"))?;
    Ok(u32::from_be_bytes(bytes))
}

fn validate_relative_asset_path(path: &str) -> Result<(), String> {
    let path = Path::new(path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "visual model path {} is not a safe relative asset path",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn building_team_color_flattens_overlay_and_underlay_into_one_opaque_texture() {
        let source = Image::new(
            bevy::render::render_resource::Extent3d {
                width: 2,
                height: 1,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            vec![0, 255, 0, 255, 12, 34, 56, 0],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        let flattened = flatten_team_color_image(&source, Color::srgb(1.0, 0.0, 0.0))
            .expect("rgba8 building texture must flatten");
        let data = flattened
            .data
            .expect("flattened image keeps CPU pixel data");
        assert_eq!(&data[0..4], &[0, 255, 0, 255]);
        assert_eq!(&data[4..8], &[255, 0, 0, 255]);
    }

    #[test]
    fn non_building_team_color_keeps_coplanar_underlay_depth_ordering() {
        assert_eq!(wc3_material_depth_bias(0, false, 2.0), 0.0);
        assert_eq!(wc3_material_depth_bias(0, true, 2.0), 2.0);
        let source = StandardMaterial {
            depth_bias: wc3_material_depth_bias(3, true, 2.0),
            ..default()
        };
        let material = team_color_underlay_material(source, Color::srgb(1.0, 0.0, 0.0), -1.0);
        assert_eq!(material.depth_bias, 4.0);
        assert!(material.base_color_texture.is_none());
        assert_eq!(material.alpha_mode, AlphaMode::Opaque);
    }

    #[test]
    fn team_glow_texture_uses_the_owning_model_pack() {
        let blue = Wc3TeamTint::new(0, Color::WHITE, "wc3/buildings");
        let red = Wc3TeamTint::new(1, Color::WHITE, "wc3/buildings");
        assert_eq!(
            team_glow_texture_path(blue),
            "wc3/buildings/textures/replaceabletextures__teamglow__teamglow01.png"
        );
        assert_eq!(
            team_glow_texture_path(red),
            "wc3/buildings/textures/replaceabletextures__teamglow__teamglow00.png"
        );
    }

    #[test]
    fn rawcodes_are_four_bytes() {
        assert_eq!(parse_rawcode("h016").unwrap(), u32::from_be_bytes(*b"h016"));
        assert!(parse_rawcode("bad").is_err());
    }

    #[test]
    fn effect_paths_must_stay_inside_asset_root() {
        assert!(validate_relative_asset_path("models/foo.gltf").is_ok());
        assert!(validate_relative_asset_path("../foo.gltf").is_err());
    }

    #[test]
    fn emitter_directions_use_the_same_wc3_to_bevy_basis_as_models() {
        assert_eq!(wc3_direction_to_bevy(Vec3::Z), Vec3::Y);
        assert_eq!(wc3_direction_to_bevy(Vec3::Y), Vec3::NEG_Z);
        assert_eq!(wc3_direction_to_bevy(Vec3::X), Vec3::X);
    }

    #[test]
    fn particle_atlas_uses_one_sprite_cell_instead_of_the_full_sheet() {
        assert_eq!(particle_atlas_uv_rect(8, 8, 0), [0.0, 0.0, 0.125, 0.125]);
        assert_eq!(particle_atlas_uv_rect(8, 8, 63), [0.875, 0.875, 1.0, 1.0]);
    }

    #[test]
    fn wc3_filter_modes_preserve_additive_and_transparent_rendering() {
        assert_eq!(particle_alpha_mode(0), AlphaMode::Blend);
        assert_eq!(particle_alpha_mode(1), AlphaMode::Add);
        assert_eq!(particle_alpha_mode(2), AlphaMode::Multiply);
        assert_eq!(particle_alpha_mode(3), AlphaMode::Multiply);
        assert_eq!(particle_alpha_mode(4), AlphaMode::Mask(0.5));
        assert_eq!(
            wc3_material_alpha_mode("Transparent", AlphaMode::Blend),
            AlphaMode::Mask(0.5)
        );
        assert_eq!(
            wc3_material_alpha_mode("Additive", AlphaMode::Blend),
            AlphaMode::Add
        );
        assert_eq!(
            wc3_material_alpha_mode("AddAlpha", AlphaMode::Blend),
            AlphaMode::Add
        );
    }

    #[test]
    fn ribbon_mesh_builds_a_fading_two_vertex_strip() {
        let spec = Wc3RibbonEmitter {
            position: [0.0; 3],
            height_above: 2.0,
            height_below: 3.0,
            alpha: 0.4,
            color: [0.4, 0.5, 0.6],
            lifespan: 1.0,
            emission_rate: 12,
            rows: 1,
            columns: 1,
            filter_mode: "AddAlpha".to_owned(),
            texture: Some("textures/ribbon.png".to_owned()),
            gravity: 0.0,
        };
        let points = VecDeque::from([
            RibbonPoint {
                center: Vec3::ZERO,
                up: Vec3::Y,
                age: 0.75,
            },
            RibbonPoint {
                center: Vec3::X * 10.0,
                up: Vec3::Y,
                age: 0.0,
            },
        ]);
        let mesh = build_wc3_ribbon_mesh(&spec, &points);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .expect("ribbon positions")
            .as_float3()
            .expect("float positions");
        assert_eq!(positions.len(), 4);
        assert_eq!(positions[0], [0.0, 2.0, 0.0]);
        assert_eq!(positions[1], [0.0, -3.0, 0.0]);
        assert!(mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_some());
    }

    #[test]
    fn ability_art_roles_keep_wc3_attachment_side() {
        assert_eq!(
            ability_visual_anchor("caster"),
            Wc3AbilityVisualAnchor::Source
        );
        for role in ["target", "effect", "special"] {
            assert_eq!(ability_visual_anchor(role), Wc3AbilityVisualAnchor::Target);
        }
    }
}
