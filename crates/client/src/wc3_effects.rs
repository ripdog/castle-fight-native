use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    fs,
    path::{Component, Path},
    sync::Arc,
    time::Duration,
};

use bevy::{
    asset::{AssetId, RenderAssetUsages},
    camera::visibility::DynamicSkinnedMeshBounds,
    ecs::system::SystemParam,
    gltf::{Gltf, GltfExtras, GltfMaterialExtras},
    mesh::{Indices, PrimitiveTopology, skinning::SkinnedMesh},
    prelude::*,
    render::render_resource::TextureFormat,
};
use serde::Deserialize;

use crate::terrain::client_asset_root;

const EFFECT_MANIFEST: &str = "wc3/effects/manifest.json";
const EFFECT_ASSET_PREFIX: &str = "wc3/effects";
const TEAM_COLOR_OVERLAY_DEPTH_BIAS_OFFSET: f32 = 2.0;
const TEAM_COLOR_UNDERLAY_DEPTH_BIAS_OFFSET: f32 = -1.0;
const MAX_PARTICLES_PER_EMITTER_PER_FRAME: u32 = 12;
const PARTICLE_MATERIAL_STEPS: u8 = 31;
const MAX_RIBBON_SAMPLES_PER_FRAME: u32 = 16;
const MAX_RIBBON_POINTS: usize = 512;
const GAMEPLAY_ANIMATION_POSE_INTERVAL: f32 = 1.0 / 30.0;

#[derive(Default)]
pub(crate) struct GameplayAnimationPoseClock {
    accumulated_seconds: f32,
    initialized: bool,
}

#[derive(Resource, Default)]
pub struct Wc3VisualSet {
    projectile_by_rawcode: BTreeMap<u32, Wc3ProjectileVisual>,
    ability_by_rawcode: BTreeMap<u32, Vec<Wc3AbilityVisual>>,
    status_by_rawcode: BTreeMap<u32, Vec<Wc3StatusVisual>>,
    chain_lightning_abilities: BTreeSet<u32>,
    stun: Option<Wc3VisualModel>,
}

#[derive(Clone)]
pub struct Wc3VisualModel {
    pub scene: Handle<WorldAsset>,
    gltf: Handle<Gltf>,
    animation_name: Option<String>,
    pub emitters: Vec<Wc3ParticleEmitter>,
    pub ribbons: Vec<Wc3RibbonEmitter>,
}

impl Wc3VisualModel {
    #[must_use]
    pub fn animation_source(&self) -> Option<Wc3VisualAnimationSource> {
        self.animation_source_with_looping(false)
    }

    #[must_use]
    pub fn looping_animation_source(&self) -> Option<Wc3VisualAnimationSource> {
        self.animation_source_with_looping(true)
    }

    fn animation_source_with_looping(&self, looping: bool) -> Option<Wc3VisualAnimationSource> {
        Some(Wc3VisualAnimationSource {
            gltf: self.gltf.clone(),
            animation_name: self.animation_name.clone()?,
            looping,
        })
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Wc3StatusVisualKind {
    Movement,
    Armor,
}

#[derive(Clone)]
pub struct Wc3StatusVisual {
    pub model: Wc3VisualModel,
    pub kind: Wc3StatusVisualKind,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Wc3ParticleEmitter {
    #[serde(default)]
    pub object_id: Option<u32>,
    pub position: [f32; 3],
    pub filter_mode: u32,
    pub speed: f32,
    pub variation: f32,
    pub latitude: f32,
    pub gravity: f32,
    pub lifespan: f32,
    pub emission_rate: f32,
    #[serde(default)]
    pub length: f32,
    #[serde(default)]
    pub width: f32,
    pub rows: u32,
    pub columns: u32,
    #[serde(default)]
    pub head_or_tail: u32,
    #[serde(default = "default_particle_middle_time")]
    pub time: f32,
    #[serde(default)]
    pub head_interval: [u32; 3],
    #[serde(default)]
    pub head_decay_interval: [u32; 3],
    #[serde(default)]
    pub tail_interval: [u32; 3],
    #[serde(default)]
    pub tail_decay_interval: [u32; 3],
    pub segment_colors: [[f32; 3]; 3],
    pub segment_alpha: [u8; 3],
    pub segment_scaling: [f32; 3],
    pub texture: Option<String>,
    pub squirt: bool,
    #[serde(default)]
    pub ambient_enabled: bool,
    #[serde(default)]
    pub active_sequences: Vec<String>,
}

const fn default_particle_middle_time() -> f32 {
    0.5
}

#[derive(Debug, Clone, Deserialize)]
pub struct Wc3RibbonEmitter {
    #[serde(default)]
    pub object_id: Option<u32>,
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
    status_visuals: Vec<StatusVisualBinding>,
    chain_lightning_abilities: Vec<String>,
    stun: Option<VisualBinding>,
    models: Vec<ModelManifest>,
}

#[derive(Debug, Clone, Deserialize)]
struct StatusVisualBinding {
    ability_rawcode: String,
    status_kind: String,
    gltf: Option<String>,
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
    animations: Vec<ModelAnimationManifest>,
    #[serde(default)]
    particle_emitters: Vec<Wc3ParticleEmitter>,
    #[serde(default)]
    ribbon_emitters: Vec<Wc3RibbonEmitter>,
}

#[derive(Debug, Clone, Deserialize)]
struct ModelAnimationManifest {
    name: String,
}

#[derive(Component, Clone)]
pub struct Wc3VisualAnimationSource {
    gltf: Handle<Gltf>,
    animation_name: String,
    looping: bool,
}

#[derive(Component)]
pub struct Wc3VisualAnimationController;

#[derive(Resource, Default)]
pub struct Wc3VisualAnimationGraphs {
    entries: HashMap<(AssetId<Gltf>, String), (Handle<AnimationGraph>, AnimationNodeIndex)>,
}

#[derive(Component)]
pub struct Wc3EmitterSource {
    emitters: Vec<EmitterRuntime>,
    node_binding_complete: bool,
}

#[derive(Clone)]
struct RibbonRuntime {
    spec: Wc3RibbonEmitter,
    source_node: Option<Entity>,
}

#[derive(Component)]
pub struct Wc3RibbonSource {
    ribbons: Vec<RibbonRuntime>,
    asset_prefix: &'static str,
    node_binding_complete: bool,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct Wc3TeamTint {
    pub index: u8,
    pub color: Color,
    asset_prefix: &'static str,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct Wc3VertexTint(pub [u8; 3]);

#[derive(Component)]
pub struct Wc3AttachToNode {
    pub owner_root: Entity,
    pub attachment_point: String,
}

pub fn resolve_wc3_visual_attachments(
    mut commands: Commands,
    pending: Query<(Entity, &Wc3AttachToNode)>,
    names: Query<(Entity, &Name)>,
    parents: Query<&ChildOf>,
) {
    for (effect, binding) in &pending {
        let requested = normalize_attachment_name(&binding.attachment_point);
        let target = names
            .iter()
            .filter_map(|(entity, name)| {
                let normalized = normalize_attachment_name(name.as_str());
                if !normalized.starts_with(&requested) {
                    return None;
                }
                let priority = if normalized.strip_prefix(&requested) == Some("ref") {
                    0
                } else if normalized == requested {
                    1
                } else {
                    2
                };
                let mut current = entity;
                for _ in 0..128 {
                    if current == effect {
                        return None;
                    }
                    if current == binding.owner_root {
                        return Some((priority, entity));
                    }
                    current = parents.get(current).ok()?.parent();
                }
                None
            })
            .min_by_key(|(priority, _)| *priority)
            .map(|(_, entity)| entity);
        if let Some(node) = target {
            commands.entity(node).add_child(effect);
            commands.entity(effect).remove::<Wc3AttachToNode>();
        }
    }
}

#[derive(Debug, Deserialize)]
struct Wc3NodeExtras {
    #[serde(rename = "wc3ObjectId")]
    wc3_object_id: Option<u32>,
}

fn belongs_to_model_root(entity: Entity, root: Entity, parents: &Query<&ChildOf>) -> bool {
    let mut current = entity;
    for _ in 0..128 {
        if current == root {
            return true;
        }
        let Ok(parent) = parents.get(current) else {
            return false;
        };
        current = parent.parent();
    }
    false
}

fn resolve_wc3_object_nodes(
    root: Entity,
    requested: &BTreeSet<u32>,
    extras: &Query<(Entity, &GltfExtras)>,
    parents: &Query<&ChildOf>,
) -> BTreeMap<u32, Entity> {
    let mut resolved = BTreeMap::new();
    for (entity, raw_extras) in extras.iter() {
        let Ok(node_extras) = serde_json::from_str::<Wc3NodeExtras>(&raw_extras.value) else {
            continue;
        };
        let Some(object_id) = node_extras.wc3_object_id else {
            continue;
        };
        if requested.contains(&object_id) && belongs_to_model_root(entity, root, parents) {
            resolved.entry(object_id).or_insert(entity);
        }
    }
    resolved
}

pub fn resolve_wc3_emitter_nodes(
    mut emitter_sources: Query<(Entity, &mut Wc3EmitterSource)>,
    mut ribbon_sources: Query<(Entity, &mut Wc3RibbonSource)>,
    extras: Query<(Entity, &GltfExtras)>,
    parents: Query<&ChildOf>,
) {
    for (root, mut source) in &mut emitter_sources {
        if source.node_binding_complete {
            continue;
        }
        let requested = source
            .emitters
            .iter()
            .filter_map(|emitter| emitter.spec.object_id)
            .collect::<BTreeSet<_>>();
        if requested.is_empty() {
            source.node_binding_complete = true;
            continue;
        }

        let resolved = resolve_wc3_object_nodes(root, &requested, &extras, &parents);
        for emitter in &mut source.emitters {
            if emitter.source_node.is_none()
                && let Some(object_id) = emitter.spec.object_id
                && let Some(entity) = resolved.get(&object_id)
            {
                emitter.source_node = Some(*entity);
            }
        }
        source.node_binding_complete = source
            .emitters
            .iter()
            .all(|emitter| emitter.spec.object_id.is_none() || emitter.source_node.is_some());
    }

    for (root, mut source) in &mut ribbon_sources {
        if source.node_binding_complete {
            continue;
        }
        let requested = source
            .ribbons
            .iter()
            .filter_map(|ribbon| ribbon.spec.object_id)
            .collect::<BTreeSet<_>>();
        if requested.is_empty() {
            source.node_binding_complete = true;
            continue;
        }

        let resolved = resolve_wc3_object_nodes(root, &requested, &extras, &parents);
        for ribbon in &mut source.ribbons {
            if ribbon.source_node.is_none()
                && let Some(object_id) = ribbon.spec.object_id
                && let Some(entity) = resolved.get(&object_id)
            {
                ribbon.source_node = Some(*entity);
            }
        }
        source.node_binding_complete = source
            .ribbons
            .iter()
            .all(|ribbon| ribbon.spec.object_id.is_none() || ribbon.source_node.is_some());
    }
}

fn normalize_attachment_name(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
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
    visual: Arc<Wc3ParticleVisualSpec>,
    source_node: Option<Entity>,
    accumulator: f32,
    burst_pending: bool,
    sequence: u32,
}

#[derive(Debug, Clone, Copy)]
struct Wc3ParticleAtlasAnimation {
    rows: u32,
    columns: u32,
    middle_time: f32,
    life_interval: [u32; 3],
    decay_interval: [u32; 3],
}

#[derive(Debug)]
struct Wc3ParticleVisualSpec {
    filter_mode: u32,
    middle_time: f32,
    segment_colors: [[f32; 3]; 3],
    segment_alpha: [u8; 3],
    texture: Option<String>,
    asset_prefix: &'static str,
}

#[derive(Component)]
pub struct Wc3Particle {
    velocity: Vec3,
    gravity: f32,
    age: f32,
    lifespan: f32,
    middle_time: f32,
    scales: [f32; 3],
    atlas: Wc3ParticleAtlasAnimation,
    atlas_frame: u32,
    visual: Arc<Wc3ParticleVisualSpec>,
    material_step: u8,
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
    ribbon_index: usize,
    spec: Wc3RibbonEmitter,
    source_scale: f32,
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
                    "Loaded {} WC3 projectile visual(s), {} ability visual(s), {} status visual id(s), and {} chain-lightning id(s)",
                    set.projectile_by_rawcode.len(),
                    set.ability_by_rawcode.len(),
                    set.status_by_rawcode.len(),
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
    pub fn status(&self, rawcode: u32) -> &[Wc3StatusVisual] {
        self.status_by_rawcode
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
        Self::with_asset_prefix(emitters, EFFECT_ASSET_PREFIX)
    }

    #[must_use]
    pub fn with_asset_prefix(emitters: &[Wc3ParticleEmitter], asset_prefix: &'static str) -> Self {
        Self::from_filtered(emitters.iter().cloned(), asset_prefix)
    }

    #[must_use]
    pub fn with_asset_prefix_for_sequence(
        emitters: &[Wc3ParticleEmitter],
        asset_prefix: &'static str,
        sequence: &str,
    ) -> Self {
        Self::from_filtered(
            emitters
                .iter()
                .filter(|emitter| {
                    emitter
                        .active_sequences
                        .iter()
                        .any(|active| active.eq_ignore_ascii_case(sequence))
                })
                .cloned(),
            asset_prefix,
        )
    }

    fn from_filtered(
        emitters: impl IntoIterator<Item = Wc3ParticleEmitter>,
        asset_prefix: &'static str,
    ) -> Self {
        let emitters = emitters
            .into_iter()
            .map(|spec| {
                let visual = Arc::new(Wc3ParticleVisualSpec {
                    filter_mode: spec.filter_mode,
                    middle_time: spec.time,
                    segment_colors: spec.segment_colors,
                    segment_alpha: spec.segment_alpha,
                    texture: spec.texture.clone(),
                    asset_prefix,
                });
                EmitterRuntime {
                    burst_pending: spec.squirt,
                    spec,
                    visual,
                    source_node: None,
                    accumulator: 0.0,
                    sequence: 0,
                }
            })
            .collect::<Vec<_>>();
        let node_binding_complete = emitters
            .iter()
            .all(|emitter| emitter.spec.object_id.is_none());
        Self {
            emitters,
            node_binding_complete,
        }
    }
}

impl Wc3RibbonSource {
    #[must_use]
    pub fn new(ribbons: &[Wc3RibbonEmitter]) -> Self {
        Self::with_asset_prefix(ribbons, EFFECT_ASSET_PREFIX)
    }

    #[must_use]
    pub fn with_asset_prefix(ribbons: &[Wc3RibbonEmitter], asset_prefix: &'static str) -> Self {
        let ribbons = ribbons
            .iter()
            .cloned()
            .map(|spec| RibbonRuntime {
                spec,
                source_node: None,
            })
            .collect::<Vec<_>>();
        let node_binding_complete = ribbons.iter().all(|ribbon| ribbon.spec.object_id.is_none());
        Self {
            ribbons,
            asset_prefix,
            node_binding_complete,
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
        visual: &Wc3ParticleVisualSpec,
        material_step: u8,
        asset_server: &AssetServer,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        let t = f32::from(material_step) / f32::from(PARTICLE_MATERIAL_STEPS);
        let (color, alpha) = particle_lifecycle_color_alpha(visual, t);
        let texture_key = visual.texture.as_deref().unwrap_or("<none>");
        let key = format!(
            "{}|{texture_key}|{}|{:.3}|{:.3}|{:.3}|{alpha:.3}",
            visual.asset_prefix, visual.filter_mode, color[0], color[1], color[2]
        );
        if let Some(handle) = self.materials.get(&key) {
            return handle.clone();
        }
        let base_color = Color::srgba(color[0], color[1], color[2], alpha);
        let base_color_texture = visual
            .texture
            .as_ref()
            .map(|texture| asset_server.load(format!("{}/{texture}", visual.asset_prefix)));
        let handle = materials.add(StandardMaterial {
            base_color,
            base_color_texture,
            emissive: LinearRgba::new(color[0], color[1], color[2], 1.0),
            alpha_mode: particle_alpha_mode(visual.filter_mode),
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
        asset_prefix: &str,
        asset_server: &AssetServer,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        let texture_key = ribbon.texture.as_deref().unwrap_or("<none>");
        let key = format!(
            "ribbon|{asset_prefix}|{texture_key}|{}|{:.3}|{:.3}|{:.3}|{:.3}",
            ribbon.filter_mode, ribbon.color[0], ribbon.color[1], ribbon.color[2], ribbon.alpha
        );
        if let Some(handle) = self.materials.get(&key) {
            return handle.clone();
        }
        let base_color_texture = ribbon
            .texture
            .as_ref()
            .map(|texture| asset_server.load(format!("{asset_prefix}/{texture}")));
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

fn wc3_visual_animation_source(
    entity: Entity,
    parents: &Query<&ChildOf>,
    roots: &Query<&Wc3VisualAnimationSource>,
) -> Option<Wc3VisualAnimationSource> {
    let mut current = entity;
    for _ in 0..128 {
        if let Ok(source) = roots.get(current) {
            return Some(source.clone());
        }
        let Ok(parent) = parents.get(current) else {
            return None;
        };
        current = parent.parent();
    }
    None
}

type Wc3AnimationAssets<'w> = (
    Res<'w, Assets<Gltf>>,
    Res<'w, Assets<AnimationClip>>,
    ResMut<'w, Assets<AnimationGraph>>,
);

pub fn throttle_gameplay_animation_poses(
    time: Res<Time>,
    mut players: Query<
        &mut AnimationPlayer,
        (
            With<AnimationTransitions>,
            Without<Wc3VisualAnimationController>,
        ),
    >,
    mut clock: Local<GameplayAnimationPoseClock>,
) {
    if !clock.initialized {
        clock.initialized = true;
        return;
    }

    clock.accumulated_seconds += time.delta_secs();
    if clock.accumulated_seconds >= GAMEPLAY_ANIMATION_POSE_INTERVAL {
        clock.accumulated_seconds %= GAMEPLAY_ANIMATION_POSE_INTERVAL;
        return;
    }

    for mut player in &mut players {
        if player.all_paused() {
            continue;
        }
        for (_, animation) in player.playing_animations_mut() {
            animation.set_weight(0.0);
        }
    }
}

pub fn skip_unchanged_paused_animation_poses(
    mut players: Query<(Entity, &mut AnimationPlayer), With<AnimationTransitions>>,
    mut last_poses: Local<HashMap<Entity, Vec<(usize, u32)>>>,
) {
    for (entity, mut player) in &mut players {
        if !player.all_paused() {
            last_poses.remove(&entity);
            continue;
        }

        let mut pose = player
            .playing_animations()
            .map(|(node, animation)| (node.index(), animation.seek_time().to_bits()))
            .collect::<Vec<_>>();
        if pose.is_empty() {
            last_poses.remove(&entity);
            continue;
        }
        pose.sort_unstable();
        if last_poses
            .get(&entity)
            .is_some_and(|previous| *previous == pose)
        {
            for (_, animation) in player.playing_animations_mut() {
                animation.set_weight(0.0);
            }
        } else {
            last_poses.insert(entity, pose);
        }
    }
}

pub fn setup_wc3_visual_animation_players(
    mut commands: Commands,
    animation_assets: Wc3AnimationAssets<'_>,
    mut cache: ResMut<Wc3VisualAnimationGraphs>,
    parents: Query<&ChildOf>,
    roots: Query<&Wc3VisualAnimationSource>,
    mut players: Query<(Entity, &mut AnimationPlayer), Without<Wc3VisualAnimationController>>,
) {
    let (gltfs, clips, mut graphs) = animation_assets;
    for (entity, mut player) in &mut players {
        let Some(source) = wc3_visual_animation_source(entity, &parents, &roots) else {
            continue;
        };
        let Some(gltf) = gltfs.get(&source.gltf) else {
            continue;
        };
        let Some((_, clip)) = gltf
            .named_animations
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&source.animation_name))
        else {
            continue;
        };
        if clips.get(clip).is_none() {
            continue;
        }
        let key = (source.gltf.id(), source.animation_name.clone());
        let (graph, node) = cache.entries.entry(key).or_insert_with(|| {
            let (graph, nodes) = AnimationGraph::from_clips(vec![clip.clone()]);
            (graphs.add(graph), nodes[0])
        });
        let mut transitions = AnimationTransitions::new();
        let active = transitions.play(&mut player, *node, Duration::ZERO);
        if source.looping {
            active.repeat();
        }
        commands.entity(entity).insert((
            AnimationGraphHandle(graph.clone()),
            transitions,
            Wc3VisualAnimationController,
        ));
    }
}

type TeamMaterialCache = HashMap<(AssetId<StandardMaterial>, u8), Handle<StandardMaterial>>;
type TeamImageCache = HashMap<(AssetId<Image>, u8), Handle<Image>>;

type Wc3MaterialWorld<'w, 's> = (
    Res<'w, AssetServer>,
    Query<'w, 's, &'static ChildOf>,
    Query<'w, 's, &'static Wc3TeamTint>,
    Query<'w, 's, &'static Wc3VertexTint>,
);

type Wc3MaterialAssets<'w, 's> = (
    ResMut<'w, Assets<StandardMaterial>>,
    ResMut<'w, Assets<Image>>,
    Local<'s, TeamMaterialCache>,
    Local<'s, TeamMaterialCache>,
    Local<'s, TeamImageCache>,
    Local<'s, HashMap<(AssetId<StandardMaterial>, [u8; 3]), Handle<StandardMaterial>>>,
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
    let (asset_server, parents, team_roots, tint_roots) = world;
    let (
        mut materials,
        mut images,
        mut team_materials,
        mut team_glow_materials,
        mut team_images,
        mut tinted_materials,
    ) = material_assets;
    'mesh: for (entity, mesh, mut material_handle, raw_extras, skin) in &mut meshes {
        let Ok(extras) = serde_json::from_str::<Wc3MaterialExtras>(&raw_extras.value) else {
            commands.entity(entity).insert(Wc3MaterialProcessed);
            continue;
        };
        let tint = wc3_vertex_tint(entity, &parents, &tint_roots);
        if extras.filter_mode.is_none()
            && !extras.team_color_underlay
            && !extras.team_glow_layer
            && tint.is_none()
        {
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
                ));
                if let Some(skin) = skin {
                    underlay_entity.insert((skin.clone(), DynamicSkinnedMeshBounds));
                }
                let underlay_entity = underlay_entity.id();
                commands.entity(entity).add_child(underlay_entity);
            }
        }

        if let Some(tint) = tint {
            let key = (material_handle.0.id(), tint);
            let tinted = if let Some(handle) = tinted_materials.get(&key) {
                handle.clone()
            } else {
                let Some(source) = materials.get(&material_handle.0) else {
                    continue 'mesh;
                };
                let mut tinted = source.clone();
                let base = tinted.base_color.to_linear();
                let rgb = Color::srgb_u8(tint[0], tint[1], tint[2]).to_linear();
                tinted.base_color = Color::linear_rgba(
                    base.red * rgb.red,
                    base.green * rgb.green,
                    base.blue * rgb.blue,
                    base.alpha,
                );
                let handle = materials.add(tinted);
                tinted_materials.insert(key, handle.clone());
                handle
            };
            material_handle.0 = tinted;
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
    format!(
        "{}/textures/replaceabletextures__teamglow__teamglow{:02}.png",
        team.asset_prefix.trim_end_matches('/'),
        team.index,
    )
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

fn wc3_vertex_tint(
    entity: Entity,
    parents: &Query<&ChildOf>,
    tint_roots: &Query<&Wc3VertexTint>,
) -> Option<[u8; 3]> {
    let mut current = entity;
    for _ in 0..128 {
        if let Ok(tint) = tint_roots.get(current) {
            return (tint.0 != [255; 3]).then_some(tint.0);
        }
        current = parents.get(current).ok()?.parent();
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
        for (ribbon_index, ribbon) in source_ribbons.ribbons.iter().enumerate() {
            let spec = &ribbon.spec;
            if spec.emission_rate == 0
                || spec.lifespan <= 0.0
                || (spec.height_above <= 0.0 && spec.height_below <= 0.0)
            {
                continue;
            }
            let mesh = meshes.add(build_wc3_ribbon_mesh(spec, &VecDeque::new(), 1.0));
            let material = ribbon_assets.ribbon_material(
                spec,
                source_ribbons.asset_prefix,
                &asset_server,
                &mut materials,
            );
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                Transform::IDENTITY,
                Visibility::default(),
                Wc3RibbonTrail {
                    source,
                    ribbon_index,
                    spec: spec.clone(),
                    source_scale: 1.0,
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
    sources: Query<(&GlobalTransform, &Wc3RibbonSource)>,
    transforms: Query<&GlobalTransform>,
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
        if let Some((root_transform, source)) = source_transform
            && let Some(ribbon) = source.ribbons.get(trail.ribbon_index)
        {
            let bound_transform = ribbon
                .source_node
                .and_then(|node| transforms.get(node).ok());
            let transform = bound_transform.unwrap_or(root_transform);
            let (scale, rotation, translation) = transform.to_scale_rotation_translation();
            trail.source_scale = scale.abs().max_element().max(0.000_1);
            let origin = if bound_transform.is_some() {
                translation
            } else {
                root_transform.transform_point(Vec3::from_array(trail.spec.position))
            };
            let up = (rotation * Vec3::Y).normalize_or(Vec3::Y);
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
            *mesh = build_wc3_ribbon_mesh(&trail.spec, &trail.points, trail.source_scale);
        }

        if source_transform.is_none() && trail.points.is_empty() {
            meshes.remove(trail.mesh.id());
            commands.entity(entity).despawn();
        }
    }
}

fn build_wc3_ribbon_mesh(
    spec: &Wc3RibbonEmitter,
    points: &VecDeque<RibbonPoint>,
    source_scale: f32,
) -> Mesh {
    let mut positions = Vec::with_capacity(points.len() * 2);
    let mut normals = Vec::with_capacity(points.len() * 2);
    let mut uvs = Vec::with_capacity(points.len() * 2);
    let mut colors = Vec::with_capacity(points.len() * 2);
    let mut indices = Vec::with_capacity(points.len().saturating_sub(1) * 6);
    let last = points.len().saturating_sub(1).max(1) as f32;
    let atlas_u = 1.0 / spec.columns.max(1) as f32;
    let atlas_v = 1.0 / spec.rows.max(1) as f32;

    for (index, point) in points.iter().enumerate() {
        let gravity_offset =
            Vec3::NEG_Y * (0.5 * spec.gravity * source_scale * point.age * point.age);
        let center = point.center + gravity_offset;
        let up = point.up.normalize_or(Vec3::Y);
        let top = center + up * spec.height_above.max(0.0) * source_scale;
        let bottom = center - up * spec.height_below.max(0.0) * source_scale;
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

#[derive(SystemParam)]
pub(crate) struct Wc3ParticleRenderAssets<'w> {
    asset_server: Res<'w, AssetServer>,
    particle_assets: ResMut<'w, Wc3ParticleAssets>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    meshes: ResMut<'w, Assets<Mesh>>,
}

pub fn emit_wc3_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut assets: Wc3ParticleRenderAssets,
    mut sources: Query<(Entity, &GlobalTransform, &mut Wc3EmitterSource)>,
    transforms: Query<&GlobalTransform>,
) {
    let dt = time.delta_secs().min(0.1);
    for (entity, transform, mut source) in &mut sources {
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
            let material_step = particle_material_step(0.0);
            let material = assets.particle_assets.material(
                &emitter.visual,
                material_step,
                &assets.asset_server,
                &mut assets.materials,
            );
            let atlas = Wc3ParticleAtlasAnimation {
                rows: emitter.spec.rows,
                columns: emitter.spec.columns,
                middle_time: emitter.spec.time,
                life_interval: if emitter.spec.head_or_tail == 1 {
                    emitter.spec.tail_interval
                } else {
                    emitter.spec.head_interval
                },
                decay_interval: if emitter.spec.head_or_tail == 1 {
                    emitter.spec.tail_decay_interval
                } else {
                    emitter.spec.head_decay_interval
                },
            };
            let initial_frame = particle_atlas_frame(atlas, 0.0);
            let particle_mesh = assets.particle_assets.particle_mesh(
                emitter.spec.rows,
                emitter.spec.columns,
                initial_frame,
                &mut assets.meshes,
            );
            let bound_transform = emitter
                .source_node
                .and_then(|node| transforms.get(node).ok());
            let source_transform = bound_transform.unwrap_or(transform);
            let (source_scale, source_rotation, source_translation) =
                source_transform.to_scale_rotation_translation();
            let uniform_scale = source_scale.abs().max_element().max(0.000_1);
            let base_origin = if bound_transform.is_some() {
                source_translation
            } else {
                transform.transform_point(Vec3::from_array(emitter.spec.position))
            };
            let particle_scales = emitter
                .spec
                .segment_scaling
                .map(|scale| scale * uniform_scale);
            for _ in 0..count {
                let sequence = emitter.sequence;
                emitter.sequence = emitter.sequence.wrapping_add(1);
                let seed = particle_seed(entity, emitter_index as u32, sequence);
                let local_spawn =
                    particle_spawn_offset(seed, emitter.spec.width, emitter.spec.length);
                let origin = base_origin + source_rotation * (local_spawn * source_scale);
                let velocity =
                    particle_velocity(source_rotation, seed, &emitter.spec) * uniform_scale;
                commands.spawn((
                    Mesh3d(particle_mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(origin)
                        .with_scale(Vec3::splat(particle_scales[0].max(0.01))),
                    Wc3Particle {
                        velocity,
                        gravity: emitter.spec.gravity * uniform_scale,
                        age: 0.0,
                        lifespan: emitter.spec.lifespan.max(0.01),
                        middle_time: emitter.spec.time,
                        scales: particle_scales,
                        atlas,
                        atlas_frame: initial_frame,
                        visual: emitter.visual.clone(),
                        material_step,
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
    mut assets: Wc3ParticleRenderAssets,
    mut particles: Query<(
        Entity,
        &mut Transform,
        &mut Mesh3d,
        &mut MeshMaterial3d<StandardMaterial>,
        &mut Wc3Particle,
    )>,
) {
    let dt = time.delta_secs().min(0.1);
    let camera_rotation = cameras.iter().next().map(GlobalTransform::rotation);
    for (entity, mut transform, mut mesh, mut material, mut particle) in &mut particles {
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
        let t = (particle.age / particle.lifespan).clamp(0.0, 1.0);
        let scale = three_stage_lerp(particle.scales, t, particle.middle_time);
        transform.scale = Vec3::splat(scale.max(0.01));

        let atlas_frame = particle_atlas_frame(particle.atlas, t);
        if atlas_frame != particle.atlas_frame {
            mesh.0 = assets.particle_assets.particle_mesh(
                particle.atlas.rows,
                particle.atlas.columns,
                atlas_frame,
                &mut assets.meshes,
            );
            particle.atlas_frame = atlas_frame;
        }

        let material_step = particle_material_step(t);
        if material_step != particle.material_step {
            material.0 = assets.particle_assets.material(
                &particle.visual,
                material_step,
                &assets.asset_server,
                &mut assets.materials,
            );
            particle.material_step = material_step;
        }
    }
}

fn particle_seed(entity: Entity, emitter_index: u32, sequence: u32) -> u32 {
    (entity.to_bits() as u32).wrapping_mul(0x9e37_79b9)
        ^ emitter_index.wrapping_mul(0x85eb_ca6b)
        ^ sequence.wrapping_mul(0xc2b2_ae35)
}

fn particle_spawn_offset(seed: u32, width: f32, length: f32) -> Vec3 {
    let x = (hash_unit(seed ^ 0x2c92_7a2d) - 0.5) * width.max(0.0);
    let y = (hash_unit(seed ^ 0x1656_67b1) - 0.5) * length.max(0.0);
    wc3_direction_to_bevy(Vec3::new(x, y, 0.0))
}

fn particle_velocity(rotation: Quat, seed: u32, emitter: &Wc3ParticleEmitter) -> Vec3 {
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

fn particle_material_step(t: f32) -> u8 {
    (t.clamp(0.0, 1.0) * f32::from(PARTICLE_MATERIAL_STEPS)).round() as u8
}

fn particle_lifecycle_color_alpha(visual: &Wc3ParticleVisualSpec, t: f32) -> ([f32; 3], f32) {
    let color = std::array::from_fn(|channel| {
        three_stage_lerp(
            [
                visual.segment_colors[0][channel],
                visual.segment_colors[1][channel],
                visual.segment_colors[2][channel],
            ],
            t,
            visual.middle_time,
        )
    });
    let alpha = three_stage_lerp(
        visual.segment_alpha.map(|alpha| f32::from(alpha) / 255.0),
        t,
        visual.middle_time,
    )
    .clamp(0.0, 1.0);
    (color, alpha)
}

fn three_stage_lerp(values: [f32; 3], t: f32, middle_time: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let middle = middle_time.clamp(0.0, 1.0);
    if middle <= f32::EPSILON {
        return values[1] + (values[2] - values[1]) * t;
    }
    if middle >= 1.0 - f32::EPSILON {
        return values[0] + (values[1] - values[0]) * t;
    }
    if t <= middle {
        values[0] + (values[1] - values[0]) * (t / middle)
    } else {
        values[1] + (values[2] - values[1]) * ((t - middle) / (1.0 - middle))
    }
}

fn particle_atlas_frame(atlas: Wc3ParticleAtlasAnimation, t: f32) -> u32 {
    let middle = atlas.middle_time.clamp(0.0, 1.0);
    let (interval, phase) = if middle <= f32::EPSILON {
        (atlas.decay_interval, t.clamp(0.0, 1.0))
    } else if middle >= 1.0 - f32::EPSILON || t <= middle {
        (
            atlas.life_interval,
            (t / middle.max(f32::EPSILON)).clamp(0.0, 1.0),
        )
    } else {
        (
            atlas.decay_interval,
            ((t - middle) / (1.0 - middle)).clamp(0.0, 1.0),
        )
    };
    let frame_count = atlas
        .rows
        .max(1)
        .saturating_mul(atlas.columns.max(1))
        .max(1);
    atlas_interval_frame(interval, phase).min(frame_count - 1)
}

fn atlas_interval_frame(interval: [u32; 3], phase: f32) -> u32 {
    let start = interval[0];
    let end = interval[1];
    if start == end {
        return start;
    }
    let frame_span = start.abs_diff(end).saturating_add(1);
    let repeats = interval[2].max(1);
    let total_steps = frame_span.saturating_mul(repeats).max(1);
    let phase = phase.clamp(0.0, 1.0 - f32::EPSILON);
    let step = ((phase * total_steps as f32).floor() as u32) % frame_span;
    if end >= start {
        start.saturating_add(step)
    } else {
        start.saturating_sub(step)
    }
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
    if manifest.schema_version != 4 {
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

    let mut status_by_rawcode = BTreeMap::<u32, Vec<Wc3StatusVisual>>::new();
    for binding in &manifest.status_visuals {
        let Some(gltf) = &binding.gltf else {
            continue;
        };
        let Some(model) = model_by_gltf.get(gltf) else {
            continue;
        };
        status_by_rawcode
            .entry(parse_rawcode(&binding.ability_rawcode)?)
            .or_default()
            .push(Wc3StatusVisual {
                model: resolve_status_visual_model(gltf, model, asset_server)?,
                kind: parse_status_visual_kind(&binding.status_kind)?,
            });
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
        status_by_rawcode,
        chain_lightning_abilities,
        stun,
    })
}

fn parse_status_visual_kind(value: &str) -> Result<Wc3StatusVisualKind, String> {
    match value {
        "movement" => Ok(Wc3StatusVisualKind::Movement),
        "armor" => Ok(Wc3StatusVisualKind::Armor),
        other => Err(format!("unsupported WC3 status visual kind {other:?}")),
    }
}

fn resolve_visual_model(
    gltf: &str,
    model: &ModelManifest,
    asset_server: &AssetServer,
) -> Result<Wc3VisualModel, String> {
    let gltf = gltf.replace('\\', "/");
    validate_relative_asset_path(&gltf)?;
    let asset_path = format!("{EFFECT_ASSET_PREFIX}/{gltf}");
    let animation_name = model
        .animations
        .iter()
        .find(|animation| animation.name.eq_ignore_ascii_case("Birth"))
        .or_else(|| {
            model
                .animations
                .iter()
                .find(|animation| animation.name.eq_ignore_ascii_case("Stand"))
        })
        .or_else(|| {
            model
                .animations
                .iter()
                .find(|animation| !animation.name.eq_ignore_ascii_case("Nothing"))
        })
        .map(|animation| animation.name.clone());
    Ok(Wc3VisualModel {
        scene: asset_server.load(GltfAssetLabel::Scene(0).from_asset(asset_path.clone())),
        gltf: asset_server.load(asset_path),
        animation_name,
        emitters: model.particle_emitters.clone(),
        ribbons: model.ribbon_emitters.clone(),
    })
}

fn resolve_status_visual_model(
    gltf: &str,
    model: &ModelManifest,
    asset_server: &AssetServer,
) -> Result<Wc3VisualModel, String> {
    let mut visual = resolve_visual_model(gltf, model, asset_server)?;
    visual.animation_name = model
        .animations
        .iter()
        .find(|animation| animation.name.eq_ignore_ascii_case("Stand"))
        .or_else(|| {
            model
                .animations
                .iter()
                .find(|animation| animation.name.eq_ignore_ascii_case("Birth"))
        })
        .or_else(|| {
            model
                .animations
                .iter()
                .find(|animation| !animation.name.eq_ignore_ascii_case("Nothing"))
        })
        .map(|animation| animation.name.clone());
    Ok(visual)
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

    fn test_particle_emitter(object_id: u32) -> Wc3ParticleEmitter {
        Wc3ParticleEmitter {
            object_id: Some(object_id),
            position: [0.0; 3],
            filter_mode: 0,
            speed: 0.0,
            variation: 0.0,
            latitude: 0.0,
            gravity: 0.0,
            lifespan: 1.0,
            emission_rate: 1.0,
            length: 0.0,
            width: 0.0,
            rows: 1,
            columns: 1,
            head_or_tail: 0,
            time: 0.5,
            head_interval: [0, 0, 1],
            head_decay_interval: [0, 0, 1],
            tail_interval: [0, 0, 1],
            tail_decay_interval: [0, 0, 1],
            segment_colors: [[1.0; 3]; 3],
            segment_alpha: [255; 3],
            segment_scaling: [1.0; 3],
            texture: None,
            squirt: false,
            ambient_enabled: true,
            active_sequences: vec!["Stand".to_owned()],
        }
    }

    fn test_ribbon_emitter(object_id: u32) -> Wc3RibbonEmitter {
        Wc3RibbonEmitter {
            object_id: Some(object_id),
            position: [0.0; 3],
            height_above: 1.0,
            height_below: 1.0,
            alpha: 1.0,
            color: [1.0; 3],
            lifespan: 1.0,
            emission_rate: 1,
            rows: 1,
            columns: 1,
            filter_mode: "Blend".to_owned(),
            texture: None,
            gravity: 0.0,
        }
    }

    #[test]
    fn model_local_vfx_object_ids_bind_within_their_own_scene_root() {
        let mut app = App::new();
        app.add_systems(Update, resolve_wc3_emitter_nodes);

        let root_a = app
            .world_mut()
            .spawn((
                Wc3EmitterSource::with_asset_prefix(&[test_particle_emitter(17)], "wc3/units"),
                Wc3RibbonSource::with_asset_prefix(&[test_ribbon_emitter(23)], "wc3/units"),
            ))
            .id();
        let root_b = app
            .world_mut()
            .spawn((
                Wc3EmitterSource::with_asset_prefix(&[test_particle_emitter(17)], "wc3/units"),
                Wc3RibbonSource::with_asset_prefix(&[test_ribbon_emitter(23)], "wc3/units"),
            ))
            .id();

        let emitter_a = app
            .world_mut()
            .spawn(GltfExtras {
                value: r#"{"wc3ObjectId":17}"#.to_owned(),
            })
            .id();
        let ribbon_a = app
            .world_mut()
            .spawn(GltfExtras {
                value: r#"{"wc3ObjectId":23}"#.to_owned(),
            })
            .id();
        let emitter_b = app
            .world_mut()
            .spawn(GltfExtras {
                value: r#"{"wc3ObjectId":17}"#.to_owned(),
            })
            .id();
        let ribbon_b = app
            .world_mut()
            .spawn(GltfExtras {
                value: r#"{"wc3ObjectId":23}"#.to_owned(),
            })
            .id();
        app.world_mut()
            .entity_mut(root_a)
            .add_children(&[emitter_a, ribbon_a]);
        app.world_mut()
            .entity_mut(root_b)
            .add_children(&[emitter_b, ribbon_b]);

        app.update();

        let source_a = app
            .world()
            .get::<Wc3EmitterSource>(root_a)
            .expect("root A emitter source");
        let source_b = app
            .world()
            .get::<Wc3EmitterSource>(root_b)
            .expect("root B emitter source");
        assert_eq!(source_a.emitters[0].source_node, Some(emitter_a));
        assert_eq!(source_b.emitters[0].source_node, Some(emitter_b));

        let ribbons_a = app
            .world()
            .get::<Wc3RibbonSource>(root_a)
            .expect("root A ribbon source");
        let ribbons_b = app
            .world()
            .get::<Wc3RibbonSource>(root_b)
            .expect("root B ribbon source");
        assert_eq!(ribbons_a.ribbons[0].source_node, Some(ribbon_a));
        assert_eq!(ribbons_b.ribbons[0].source_node, Some(ribbon_b));
    }

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
    fn team_glow_texture_uses_owner_slot_and_model_pack() {
        let red = Wc3TeamTint::new(0, Color::WHITE, "wc3/buildings");
        let teal = Wc3TeamTint::new(2, Color::WHITE, "wc3/buildings");
        let green = Wc3TeamTint::new(6, Color::WHITE, "wc3/units");
        assert_eq!(
            team_glow_texture_path(red),
            "wc3/buildings/textures/replaceabletextures__teamglow__teamglow00.png"
        );
        assert_eq!(
            team_glow_texture_path(teal),
            "wc3/buildings/textures/replaceabletextures__teamglow__teamglow02.png"
        );
        assert_eq!(
            team_glow_texture_path(green),
            "wc3/units/textures/replaceabletextures__teamglow__teamglow06.png"
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
    fn particle_spawn_rectangle_uses_wc3_xy_plane_in_client_basis() {
        let offset = particle_spawn_offset(0x1234_5678, 20.0, 10.0);
        assert!(offset.x.abs() <= 10.0);
        assert_eq!(offset.y, 0.0);
        assert!(offset.z.abs() <= 5.0);
    }

    #[test]
    fn particle_three_stage_lifecycle_uses_authored_middle_time() {
        let values = [0.0, 10.0, 20.0];
        assert_eq!(three_stage_lerp(values, 0.0, 0.25), 0.0);
        assert_eq!(three_stage_lerp(values, 0.25, 0.25), 10.0);
        assert!((three_stage_lerp(values, 0.625, 0.25) - 15.0).abs() < 1.0e-6);
        assert_eq!(three_stage_lerp(values, 1.0, 0.25), 20.0);
    }

    #[test]
    fn particle_color_and_alpha_follow_the_same_authored_middle_time() {
        let visual = Wc3ParticleVisualSpec {
            filter_mode: 0,
            middle_time: 0.25,
            segment_colors: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            segment_alpha: [255, 128, 0],
            texture: None,
            asset_prefix: "wc3/units",
        };
        let (middle_color, middle_alpha) = particle_lifecycle_color_alpha(&visual, 0.25);
        assert_eq!(middle_color, [0.0, 1.0, 0.0]);
        assert!((middle_alpha - 128.0 / 255.0).abs() < 1.0e-6);

        let (end_color, end_alpha) = particle_lifecycle_color_alpha(&visual, 1.0);
        assert_eq!(end_color, [0.0, 0.0, 1.0]);
        assert_eq!(end_alpha, 0.0);
        assert_eq!(particle_material_step(0.0), 0);
        assert_eq!(particle_material_step(1.0), PARTICLE_MATERIAL_STEPS);
    }

    #[test]
    fn particle_atlas_uses_authored_life_and_decay_intervals() {
        let atlas = Wc3ParticleAtlasAnimation {
            rows: 2,
            columns: 4,
            middle_time: 0.5,
            life_interval: [0, 3, 1],
            decay_interval: [4, 7, 1],
        };
        assert_eq!(particle_atlas_frame(atlas, 0.0), 0);
        assert_eq!(particle_atlas_frame(atlas, 0.49), 3);
        assert_eq!(particle_atlas_frame(atlas, 0.5), 3);
        assert_eq!(particle_atlas_frame(atlas, 0.51), 4);
        assert_eq!(particle_atlas_frame(atlas, 1.0), 7);

        assert_eq!(atlas_interval_frame([0, 1, 2], 0.0), 0);
        assert_eq!(atlas_interval_frame([0, 1, 2], 0.25), 1);
        assert_eq!(atlas_interval_frame([0, 1, 2], 0.5), 0);
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
            object_id: None,
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
        let mesh = build_wc3_ribbon_mesh(&spec, &points, 1.0);
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
    fn status_visual_kinds_are_manifest_driven() {
        assert_eq!(
            parse_status_visual_kind("movement").unwrap(),
            Wc3StatusVisualKind::Movement
        );
        assert_eq!(
            parse_status_visual_kind("armor").unwrap(),
            Wc3StatusVisualKind::Armor
        );
        assert!(parse_status_visual_kind("unknown").is_err());
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
