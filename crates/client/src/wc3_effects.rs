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
const CONVERTED_MODEL_PACKS: [(&str, &str); 4] = [
    ("wc3/units/manifest.json", "wc3/units"),
    ("wc3/buildings/manifest.json", "wc3/buildings"),
    ("wc3/doodads/manifest.json", "wc3/doodads"),
    ("wc3/effects/manifest.json", "wc3/effects"),
];
const TEAM_COLOR_OVERLAY_DEPTH_BIAS_OFFSET: f32 = 2.0;
const TEAM_COLOR_UNDERLAY_DEPTH_BIAS_OFFSET: f32 = -1.0;
const MAX_PARTICLES_PER_EMITTER_PER_FRAME: u32 = 12;
const MAX_MODEL_PARTICLES_PER_EMITTER_PER_FRAME: u32 = 12;
const PARTICLE_MATERIAL_STEPS: u8 = 31;
const MAX_RIBBON_SAMPLES_PER_FRAME: u32 = 16;
const MAX_RIBBON_POINTS: usize = 512;
const GAMEPLAY_ANIMATION_POSE_INTERVAL: f32 = 1.0 / 30.0;
// Stock WC3 omni lights commonly use intensity 20. Map that to Bevy's 1,000,000-lumen
// default point light, while retaining WC3 attenuation end as the cutoff radius.
const WC3_MODEL_LIGHT_LUMENS_PER_INTENSITY: f32 = 50_000.0;

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
    stand_animation_name: Option<String>,
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
        Some(Wc3VisualAnimationSource {
            gltf: self.gltf.clone(),
            animation_name: self
                .stand_animation_name
                .as_ref()
                .or(self.animation_name.as_ref())?
                .clone(),
            looping: true,
        })
    }

    #[must_use]
    pub fn emitter_source(&self) -> Wc3EmitterSource {
        self.emitter_source_for(self.animation_name.as_deref())
    }

    #[must_use]
    pub fn looping_emitter_source(&self) -> Wc3EmitterSource {
        self.emitter_source_for(
            self.stand_animation_name
                .as_deref()
                .or(self.animation_name.as_deref()),
        )
    }

    fn emitter_source_for(&self, sequence: Option<&str>) -> Wc3EmitterSource {
        sequence.map_or_else(
            || Wc3EmitterSource::new(&self.emitters),
            |sequence| {
                Wc3EmitterSource::with_asset_prefix_for_sequence(
                    &self.emitters,
                    EFFECT_ASSET_PREFIX,
                    sequence,
                )
            },
        )
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

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Wc3ScalarInterpolation {
    DontInterp,
    Linear,
    Hermite,
    Bezier,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Wc3ScalarTrack {
    interpolation: Wc3ScalarInterpolation,
    global_sequence_id: Option<u32>,
    timestamps: Vec<u32>,
    values: Vec<f32>,
    #[serde(default)]
    in_tangents: Vec<f32>,
    #[serde(default)]
    out_tangents: Vec<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Wc3Vector3Track {
    interpolation: Wc3ScalarInterpolation,
    global_sequence_id: Option<u32>,
    timestamps: Vec<u32>,
    values: Vec<[f32; 3]>,
    #[serde(default)]
    in_tangents: Vec<[f32; 3]>,
    #[serde(default)]
    out_tangents: Vec<[f32; 3]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Wc3UnsignedTrack {
    #[serde(default = "default_dont_interp")]
    interpolation: Wc3ScalarInterpolation,
    global_sequence_id: Option<u32>,
    timestamps: Vec<u32>,
    values: Vec<u32>,
}

const fn default_dont_interp() -> Wc3ScalarInterpolation {
    Wc3ScalarInterpolation::DontInterp
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Wc3EmitterSequenceWindow {
    name: String,
    start_ms: u32,
    end_ms: u32,
    non_looping: bool,
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
    #[serde(default)]
    pub speed_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    pub variation_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    pub latitude_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    pub gravity_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    pub emission_rate_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    pub length_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    pub width_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    pub visibility_track: Option<Wc3ScalarTrack>,
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
    pub sequence_windows: Vec<Wc3EmitterSequenceWindow>,
    #[serde(default)]
    pub global_sequence_durations_ms: Vec<u32>,
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
    #[serde(default)]
    pub texture_slot: u32,
    pub filter_mode: String,
    pub texture: Option<String>,
    pub gravity: f32,
    #[serde(default)]
    pub height_above_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    pub height_below_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    pub alpha_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    pub color_track: Option<Wc3Vector3Track>,
    #[serde(default)]
    pub texture_slot_track: Option<Wc3UnsignedTrack>,
    #[serde(default)]
    pub visibility_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    pub sequence_windows: Vec<Wc3EmitterSequenceWindow>,
    #[serde(default)]
    pub global_sequence_durations_ms: Vec<u32>,
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

#[derive(Debug, Clone, Deserialize)]
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
    #[serde(default)]
    non_looping: bool,
}

#[derive(Debug, Deserialize)]
struct ConvertedModelPackManifest {
    #[serde(default)]
    models: Vec<ModelManifest>,
}

#[derive(Debug, Clone)]
struct RegisteredConvertedModel {
    asset_path: String,
    asset_prefix: &'static str,
    manifest: ModelManifest,
}

#[derive(Resource, Default)]
pub struct Wc3ConvertedModelRegistry {
    models: BTreeMap<String, RegisteredConvertedModel>,
}

#[derive(Component, Clone)]
pub struct Wc3VisualAnimationSource {
    gltf: Handle<Gltf>,
    animation_name: String,
    looping: bool,
}

#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct Wc3ModelSequenceSelection {
    name: String,
}

impl Wc3ModelSequenceSelection {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

#[derive(Component, Debug, Clone)]
pub(crate) struct Wc3ModelSequenceClock {
    sequence_name: String,
    sequence_elapsed_ms: f32,
    global_elapsed_ms: f32,
}

#[derive(Component, Debug, Clone)]
pub(crate) struct Wc3AnimatedMaterialAlpha {
    static_alpha: f32,
    track: Wc3ScalarTrack,
    sequence_windows: Vec<Wc3EmitterSequenceWindow>,
    global_sequence_durations_ms: Vec<u32>,
    fallback_elapsed_ms: f32,
    last_alpha_bits: u32,
}

#[derive(Component, Debug, Clone)]
pub(crate) struct Wc3AnimatedMaterialTexture {
    static_texture_id: u32,
    static_texture: Option<Handle<Image>>,
    textures: BTreeMap<u32, Handle<Image>>,
    track: Wc3UnsignedTrack,
    sequence_windows: Vec<Wc3EmitterSequenceWindow>,
    global_sequence_durations_ms: Vec<u32>,
    fallback_elapsed_ms: f32,
    last_texture_id: Option<u32>,
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
    sequence_clock: Option<Wc3EmitterSequenceClock>,
    global_elapsed_ms: f32,
}

#[derive(Debug, Clone)]
struct Wc3EmitterSequenceClock {
    start_ms: u32,
    end_ms: u32,
    non_looping: bool,
    elapsed_ms: f32,
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
    sequence_clock: Option<Wc3EmitterSequenceClock>,
    global_elapsed_ms: f32,
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

#[derive(Debug, Clone, Deserialize)]
struct Wc3LightSpec {
    light_type: String,
    attenuation_end: f32,
    color: [f32; 3],
    intensity: f32,
    #[serde(default)]
    attenuation_end_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    color_track: Option<Wc3Vector3Track>,
    #[serde(default)]
    intensity_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    visibility_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    sequence_windows: Vec<Wc3EmitterSequenceWindow>,
    #[serde(default)]
    global_sequence_durations_ms: Vec<u32>,
}

#[derive(Component, Debug, Clone)]
pub(crate) struct Wc3PointLightRuntime {
    static_color: [f32; 3],
    static_intensity: f32,
    static_range: f32,
    attenuation_end_track: Option<Wc3ScalarTrack>,
    color_track: Option<Wc3Vector3Track>,
    intensity_track: Option<Wc3ScalarTrack>,
    visibility_track: Option<Wc3ScalarTrack>,
    sequence_windows: Vec<Wc3EmitterSequenceWindow>,
    global_sequence_durations_ms: Vec<u32>,
    fallback_elapsed_ms: f32,
}

#[derive(Debug, Clone, Deserialize)]
struct Wc3ModelAttachmentSpec {
    gltf: String,
    #[serde(default)]
    visibility_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    sequence_windows: Vec<Wc3EmitterSequenceWindow>,
    #[serde(default)]
    global_sequence_durations_ms: Vec<u32>,
}

#[derive(Component, Debug, Clone)]
pub(crate) struct Wc3ModelAttachmentRuntime {
    child_model: RegisteredConvertedModel,
    visibility_track: Option<Wc3ScalarTrack>,
    sequence_windows: Vec<Wc3EmitterSequenceWindow>,
    global_sequence_durations_ms: Vec<u32>,
    fallback_elapsed_ms: f32,
    child: Option<Entity>,
}

#[derive(Debug, Clone, Deserialize)]
struct Wc3LegacyModelEmitterSpec {
    emission_rate: f32,
    gravity: f32,
    longitude: f32,
    latitude: f32,
    lifespan: f32,
    initial_velocity: f32,
    gltf: String,
    #[serde(default)]
    emission_rate_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    gravity_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    longitude_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    latitude_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    lifespan_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    speed_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    visibility_track: Option<Wc3ScalarTrack>,
    #[serde(default)]
    sequence_windows: Vec<Wc3EmitterSequenceWindow>,
    #[serde(default)]
    global_sequence_durations_ms: Vec<u32>,
}

#[derive(Component, Debug, Clone)]
pub(crate) struct Wc3LegacyModelEmitterRuntime {
    child_model: RegisteredConvertedModel,
    spec: Wc3LegacyModelEmitterSpec,
    fallback_elapsed_ms: f32,
    accumulator: f32,
    sequence: u32,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct Wc3LegacyModelParticle {
    velocity: Vec3,
    gravity: f32,
    age: f32,
    lifespan: f32,
}

#[derive(Debug, Deserialize)]
struct Wc3NodeExtras {
    #[serde(rename = "wc3ObjectId")]
    wc3_object_id: Option<u32>,
    #[serde(rename = "wc3Light", default)]
    wc3_light: Option<Wc3LightSpec>,
    #[serde(rename = "wc3Attachment", default)]
    wc3_attachment: Option<Wc3ModelAttachmentSpec>,
    #[serde(rename = "wc3ModelParticleEmitter", default)]
    wc3_model_particle_emitter: Option<Wc3LegacyModelEmitterSpec>,
}

fn inherited_world_asset_path(
    entity: Entity,
    parents: &Query<&ChildOf>,
    roots: &Query<&WorldAssetRoot>,
    asset_server: &AssetServer,
) -> Option<String> {
    let mut current = entity;
    for _ in 0..128 {
        if let Ok(root) = roots.get(current) {
            return asset_server
                .get_path(root.0.id())
                .map(|path| path.path().to_string_lossy().replace('\\', "/"));
        }
        current = parents.get(current).ok()?.parent();
    }
    None
}

fn child_model_asset_path(parent_asset_path: &str, child_gltf: &str) -> Option<String> {
    let child_gltf = child_gltf.replace('\\', "/");
    validate_relative_asset_path(&child_gltf).ok()?;
    let child_file = Path::new(&child_gltf).file_name()?;
    let parent_dir = Path::new(parent_asset_path).parent()?;
    Some(
        parent_dir
            .join(child_file)
            .to_string_lossy()
            .replace('\\', "/"),
    )
}

fn preferred_model_animation(model: &ModelManifest) -> Option<&ModelAnimationManifest> {
    model
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
}

fn registered_child_model_for_node(
    entity: Entity,
    child_gltf: &str,
    asset_server: &AssetServer,
    registry: &Wc3ConvertedModelRegistry,
    parents: &Query<&ChildOf>,
    roots: &Query<&WorldAssetRoot>,
) -> Option<RegisteredConvertedModel> {
    let parent_asset_path = inherited_world_asset_path(entity, parents, roots, asset_server)?;
    let child_asset_path = child_model_asset_path(&parent_asset_path, child_gltf)?;
    registry.get(&child_asset_path).cloned().or_else(|| {
        warn!("WC3 model references unregistered converted child model {child_asset_path}");
        None
    })
}

pub fn setup_wc3_model_composed_features(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    registry: Res<Wc3ConvertedModelRegistry>,
    nodes: Query<(Entity, &GltfExtras), Added<GltfExtras>>,
    parents: Query<&ChildOf>,
    roots: Query<&WorldAssetRoot>,
) {
    for (entity, raw_extras) in &nodes {
        let Ok(extras) = serde_json::from_str::<Wc3NodeExtras>(&raw_extras.value) else {
            continue;
        };
        if let Some(spec) = extras.wc3_attachment
            && let Some(child_model) = registered_child_model_for_node(
                entity,
                &spec.gltf,
                &asset_server,
                &registry,
                &parents,
                &roots,
            )
        {
            commands.entity(entity).insert(Wc3ModelAttachmentRuntime {
                child_model,
                visibility_track: spec.visibility_track,
                sequence_windows: spec.sequence_windows,
                global_sequence_durations_ms: spec.global_sequence_durations_ms,
                fallback_elapsed_ms: 0.0,
                child: None,
            });
        }
        if let Some(spec) = extras.wc3_model_particle_emitter
            && let Some(child_model) = registered_child_model_for_node(
                entity,
                &spec.gltf,
                &asset_server,
                &registry,
                &parents,
                &roots,
            )
        {
            commands
                .entity(entity)
                .insert(Wc3LegacyModelEmitterRuntime {
                    child_model,
                    spec,
                    fallback_elapsed_ms: 0.0,
                    accumulator: 0.0,
                    sequence: 0,
                });
        }
    }
}

fn sample_model_sequence_scalar(
    track: Option<&Wc3ScalarTrack>,
    default: f32,
    sequence_windows: &[Wc3EmitterSequenceWindow],
    global_sequence_durations_ms: &[u32],
    fallback_elapsed_ms: f32,
    clock: Option<&Wc3ModelSequenceClock>,
) -> f32 {
    let Some(track) = track else {
        return default;
    };
    if track.global_sequence_id.is_some() {
        let (sequence_time_ms, global_elapsed_ms) =
            clock.map_or((fallback_elapsed_ms, fallback_elapsed_ms), |clock| {
                (
                    material_sequence_time_ms(sequence_windows, clock),
                    clock.global_elapsed_ms,
                )
            });
        return sample_scalar_track(
            Some(track),
            default,
            sequence_time_ms,
            global_elapsed_ms,
            global_sequence_durations_ms,
        );
    }

    let Some(clock) = clock else {
        return default;
    };
    let Some(window) = sequence_windows
        .iter()
        .find(|window| window.name.eq_ignore_ascii_case(&clock.sequence_name))
    else {
        return default;
    };
    if !track
        .timestamps
        .iter()
        .any(|timestamp| *timestamp >= window.start_ms && *timestamp <= window.end_ms)
    {
        return default;
    }
    sample_scalar_track(
        Some(track),
        default,
        material_sequence_time_ms(sequence_windows, clock),
        clock.global_elapsed_ms,
        global_sequence_durations_ms,
    )
}

fn model_attachment_visibility(
    animation: &Wc3ModelAttachmentRuntime,
    clock: Option<&Wc3ModelSequenceClock>,
) -> f32 {
    sample_model_sequence_scalar(
        animation.visibility_track.as_ref(),
        1.0,
        &animation.sequence_windows,
        &animation.global_sequence_durations_ms,
        animation.fallback_elapsed_ms,
        clock,
    )
}

fn spawn_registered_converted_model(
    commands: &mut Commands,
    parent: Option<Entity>,
    model: &RegisteredConvertedModel,
    asset_server: &AssetServer,
    transform: Transform,
) -> Entity {
    let scene = asset_server.load(GltfAssetLabel::Scene(0).from_asset(model.asset_path.clone()));
    let gltf = asset_server.load(model.asset_path.clone());
    let mut child = commands.spawn((WorldAssetRoot(scene), transform, Visibility::default()));
    if let Some(animation) = preferred_model_animation(&model.manifest) {
        child.insert(Wc3VisualAnimationSource {
            gltf,
            animation_name: animation.name.clone(),
            looping: !animation.non_looping,
        });
        if !model.manifest.particle_emitters.is_empty() {
            child.insert(Wc3EmitterSource::with_asset_prefix_for_sequence(
                &model.manifest.particle_emitters,
                model.asset_prefix,
                &animation.name,
            ));
        }
        if !model.manifest.ribbon_emitters.is_empty() {
            child.insert(Wc3RibbonSource::with_asset_prefix_for_sequence(
                &model.manifest.ribbon_emitters,
                model.asset_prefix,
                &animation.name,
            ));
        }
    } else {
        if !model.manifest.particle_emitters.is_empty() {
            child.insert(Wc3EmitterSource::with_asset_prefix(
                &model.manifest.particle_emitters,
                model.asset_prefix,
            ));
        }
        if !model.manifest.ribbon_emitters.is_empty() {
            child.insert(Wc3RibbonSource::with_asset_prefix(
                &model.manifest.ribbon_emitters,
                model.asset_prefix,
            ));
        }
    }
    let child = child.id();
    if let Some(parent) = parent {
        commands.entity(parent).add_child(child);
    }
    child
}

pub fn update_wc3_model_attachments(
    mut commands: Commands,
    time: Res<Time>,
    asset_server: Res<AssetServer>,
    parents: Query<&ChildOf>,
    clocks: Query<&Wc3ModelSequenceClock>,
    mut attachments: Query<(Entity, &mut Wc3ModelAttachmentRuntime)>,
) {
    let dt_ms = time.delta_secs().max(0.0) * 1000.0;
    for (entity, mut attachment) in &mut attachments {
        attachment.fallback_elapsed_ms += dt_ms;
        let clock = inherited_wc3_model_sequence_clock(entity, &parents, &clocks);
        let visible = model_attachment_visibility(&attachment, clock.as_ref()) > 0.001;
        match (visible, attachment.child) {
            (true, None) => {
                attachment.child = Some(spawn_registered_converted_model(
                    &mut commands,
                    Some(entity),
                    &attachment.child_model,
                    &asset_server,
                    Transform::IDENTITY,
                ));
            }
            (false, Some(child)) => {
                commands.entity(child).try_despawn();
                attachment.child = None;
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Wc3LegacyModelEmitterSample {
    emission_rate: f32,
    gravity: f32,
    longitude: f32,
    latitude: f32,
    lifespan: f32,
    speed: f32,
    visibility: f32,
}

fn sample_legacy_model_emitter(
    emitter: &Wc3LegacyModelEmitterRuntime,
    clock: Option<&Wc3ModelSequenceClock>,
) -> Wc3LegacyModelEmitterSample {
    let sample = |track: Option<&Wc3ScalarTrack>, default: f32| {
        sample_model_sequence_scalar(
            track,
            default,
            &emitter.spec.sequence_windows,
            &emitter.spec.global_sequence_durations_ms,
            emitter.fallback_elapsed_ms,
            clock,
        )
    };
    Wc3LegacyModelEmitterSample {
        emission_rate: sample(
            emitter.spec.emission_rate_track.as_ref(),
            emitter.spec.emission_rate,
        ),
        gravity: sample(emitter.spec.gravity_track.as_ref(), emitter.spec.gravity),
        longitude: sample(
            emitter.spec.longitude_track.as_ref(),
            emitter.spec.longitude,
        ),
        latitude: sample(emitter.spec.latitude_track.as_ref(), emitter.spec.latitude),
        lifespan: sample(emitter.spec.lifespan_track.as_ref(), emitter.spec.lifespan),
        speed: sample(
            emitter.spec.speed_track.as_ref(),
            emitter.spec.initial_velocity,
        ),
        visibility: sample(emitter.spec.visibility_track.as_ref(), 1.0),
    }
}

fn legacy_model_particle_velocity(
    rotation: Quat,
    seed: u32,
    speed: f32,
    longitude_radians: f32,
    latitude_radians: f32,
) -> Vec3 {
    let cone = latitude_radians.abs().clamp(0.0, std::f32::consts::PI) * hash_unit(seed).sqrt();
    let longitude = longitude_radians.abs().clamp(0.0, std::f32::consts::PI);
    let azimuth = (hash_unit(seed ^ 0xa511_e9b3) * 2.0 - 1.0) * longitude;
    let wc3_direction = Vec3::new(
        cone.sin() * azimuth.cos(),
        cone.sin() * azimuth.sin(),
        cone.cos(),
    );
    rotation * wc3_direction_to_bevy(wc3_direction) * speed.max(0.0)
}

pub fn emit_wc3_model_particles(
    mut commands: Commands,
    time: Res<Time>,
    asset_server: Res<AssetServer>,
    parents: Query<&ChildOf>,
    clocks: Query<&Wc3ModelSequenceClock>,
    mut emitters: Query<(Entity, &GlobalTransform, &mut Wc3LegacyModelEmitterRuntime)>,
) {
    let dt = time.delta_secs().min(0.1);
    let dt_ms = dt * 1000.0;
    for (entity, transform, mut emitter) in &mut emitters {
        let clock = inherited_wc3_model_sequence_clock(entity, &parents, &clocks);
        let sample = sample_legacy_model_emitter(&emitter, clock.as_ref());
        emitter.fallback_elapsed_ms += dt_ms;
        if sample.visibility <= 0.001 {
            emitter.accumulator = 0.0;
            continue;
        }
        emitter.accumulator += sample.emission_rate.clamp(0.0, 240.0) * dt;
        let mut count = emitter.accumulator.floor() as u32;
        emitter.accumulator -= count as f32;
        count = count.min(MAX_MODEL_PARTICLES_PER_EMITTER_PER_FRAME);
        if count == 0 || sample.lifespan <= 0.0 {
            continue;
        }

        let (source_scale, source_rotation, source_translation) =
            transform.to_scale_rotation_translation();
        let uniform_scale = source_scale.abs().max_element().max(0.000_1);
        for _ in 0..count {
            let sequence = emitter.sequence;
            emitter.sequence = emitter.sequence.wrapping_add(1);
            let seed = particle_seed(entity, 0, sequence);
            let velocity = legacy_model_particle_velocity(
                source_rotation,
                seed,
                sample.speed,
                sample.longitude,
                sample.latitude,
            ) * uniform_scale;
            let child = spawn_registered_converted_model(
                &mut commands,
                None,
                &emitter.child_model,
                &asset_server,
                Transform::from_translation(source_translation)
                    .with_rotation(source_rotation)
                    .with_scale(Vec3::splat(uniform_scale)),
            );
            commands.entity(child).insert(Wc3LegacyModelParticle {
                velocity,
                gravity: sample.gravity * uniform_scale,
                age: 0.0,
                lifespan: sample.lifespan.max(0.01),
            });
        }
    }
}

pub fn update_wc3_model_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particles: Query<(Entity, &mut Transform, &mut Wc3LegacyModelParticle)>,
) {
    let dt = time.delta_secs().min(0.1);
    for (entity, mut transform, mut particle) in &mut particles {
        particle.age += dt;
        if particle.age >= particle.lifespan {
            commands.entity(entity).despawn();
            continue;
        }
        particle.velocity.y -= particle.gravity * dt;
        transform.translation += particle.velocity * dt;
    }
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

impl Wc3ConvertedModelRegistry {
    #[must_use]
    pub fn load_default() -> Self {
        let asset_root = client_asset_root();
        let mut models = BTreeMap::new();
        for (manifest_relative, asset_prefix) in CONVERTED_MODEL_PACKS {
            let manifest_path = asset_root.join(manifest_relative);
            if !manifest_path.is_file() {
                continue;
            }
            let Ok(json) = fs::read_to_string(&manifest_path) else {
                continue;
            };
            let Ok(manifest) = serde_json::from_str::<ConvertedModelPackManifest>(&json) else {
                continue;
            };
            for model in manifest.models {
                let gltf = model.gltf.replace('\\', "/");
                if validate_relative_asset_path(&gltf).is_err() {
                    continue;
                }
                let asset_path = format!("{asset_prefix}/{gltf}");
                models
                    .entry(asset_path.clone())
                    .or_insert(RegisteredConvertedModel {
                        asset_path,
                        asset_prefix,
                        manifest: model,
                    });
            }
        }
        Self { models }
    }

    fn get(&self, asset_path: &str) -> Option<&RegisteredConvertedModel> {
        self.models.get(asset_path)
    }
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
        Self::from_filtered(emitters.iter().cloned(), asset_prefix, None)
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
            Some(sequence),
        )
    }

    fn from_filtered(
        emitters: impl IntoIterator<Item = Wc3ParticleEmitter>,
        asset_prefix: &'static str,
        sequence: Option<&str>,
    ) -> Self {
        let specs = emitters.into_iter().collect::<Vec<_>>();
        let sequence_window = sequence
            .and_then(|sequence| find_emitter_sequence_window(&specs, sequence))
            .or_else(|| find_emitter_sequence_window(&specs, "Stand"))
            .or_else(|| find_emitter_sequence_window(&specs, "Birth"))
            .or_else(|| {
                specs
                    .iter()
                    .flat_map(|emitter| emitter.sequence_windows.iter())
                    .next()
                    .cloned()
            });
        let emitters = specs
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
            sequence_clock: sequence_window.map(|window| Wc3EmitterSequenceClock {
                start_ms: window.start_ms,
                end_ms: window.end_ms,
                non_looping: window.non_looping,
                elapsed_ms: 0.0,
            }),
            global_elapsed_ms: 0.0,
        }
    }
}

fn find_emitter_sequence_window(
    emitters: &[Wc3ParticleEmitter],
    sequence: &str,
) -> Option<Wc3EmitterSequenceWindow> {
    emitters
        .iter()
        .flat_map(|emitter| emitter.sequence_windows.iter())
        .find(|window| window.name.eq_ignore_ascii_case(sequence))
        .cloned()
}

impl Wc3EmitterSequenceClock {
    fn current_time_ms(&self) -> f32 {
        let duration = self.end_ms.saturating_sub(self.start_ms).max(1) as f32;
        let offset = if self.non_looping {
            self.elapsed_ms.min(duration)
        } else {
            self.elapsed_ms.rem_euclid(duration)
        };
        self.start_ms as f32 + offset
    }

    fn advance(&mut self, dt_seconds: f32) {
        self.elapsed_ms += dt_seconds.max(0.0) * 1000.0;
    }
}

impl Wc3RibbonSource {
    #[must_use]
    pub fn new(ribbons: &[Wc3RibbonEmitter]) -> Self {
        Self::with_asset_prefix(ribbons, EFFECT_ASSET_PREFIX)
    }

    #[must_use]
    pub fn with_asset_prefix(ribbons: &[Wc3RibbonEmitter], asset_prefix: &'static str) -> Self {
        Self::with_asset_prefix_and_sequence(ribbons, asset_prefix, None)
    }

    #[must_use]
    pub fn with_asset_prefix_for_sequence(
        ribbons: &[Wc3RibbonEmitter],
        asset_prefix: &'static str,
        sequence: &str,
    ) -> Self {
        Self::with_asset_prefix_and_sequence(ribbons, asset_prefix, Some(sequence))
    }

    fn with_asset_prefix_and_sequence(
        ribbons: &[Wc3RibbonEmitter],
        asset_prefix: &'static str,
        sequence: Option<&str>,
    ) -> Self {
        let sequence_window = sequence
            .and_then(|sequence| find_ribbon_sequence_window(ribbons, sequence))
            .or_else(|| find_ribbon_sequence_window(ribbons, "Stand"))
            .or_else(|| find_ribbon_sequence_window(ribbons, "Birth"))
            .or_else(|| {
                ribbons
                    .iter()
                    .flat_map(|ribbon| ribbon.sequence_windows.iter())
                    .next()
                    .cloned()
            });
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
            sequence_clock: sequence_window.map(|window| Wc3EmitterSequenceClock {
                start_ms: window.start_ms,
                end_ms: window.end_ms,
                non_looping: window.non_looping,
                elapsed_ms: 0.0,
            }),
            global_elapsed_ms: 0.0,
        }
    }
}

fn find_ribbon_sequence_window(
    ribbons: &[Wc3RibbonEmitter],
    sequence: &str,
) -> Option<Wc3EmitterSequenceWindow> {
    ribbons
        .iter()
        .flat_map(|ribbon| ribbon.sequence_windows.iter())
        .find(|window| window.name.eq_ignore_ascii_case(sequence))
        .cloned()
}

#[derive(Debug, Deserialize)]
struct Wc3MaterialExtras {
    #[serde(rename = "wc3FilterMode")]
    filter_mode: Option<String>,
    #[serde(rename = "wc3LayerAlpha", default = "default_material_alpha")]
    layer_alpha: f32,
    #[serde(rename = "wc3AlphaTrack", default)]
    alpha_track: Option<Wc3ScalarTrack>,
    #[serde(rename = "wc3TextureId", default)]
    texture_id: u32,
    #[serde(rename = "wc3TextureIdTrack", default)]
    texture_id_track: Option<Wc3UnsignedTrack>,
    #[serde(rename = "wc3TexturePaths", default)]
    texture_paths: Vec<Wc3MaterialTexturePath>,
    #[serde(rename = "wc3SequenceWindows", default)]
    sequence_windows: Vec<Wc3EmitterSequenceWindow>,
    #[serde(rename = "wc3GlobalSequenceDurationsMs", default)]
    global_sequence_durations_ms: Vec<u32>,
    #[serde(rename = "wc3PriorityPlane", default)]
    priority_plane: i32,
    #[serde(rename = "wc3TeamColorUnderlay", default)]
    team_color_underlay: bool,
    #[serde(rename = "wc3TeamGlowLayer", default)]
    team_glow_layer: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Wc3MaterialTexturePath {
    texture_id: u32,
    file_name: String,
}

const fn default_material_alpha() -> f32 {
    1.0
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
        let key = format!("ribbon|{asset_prefix}|{texture_key}|{}", ribbon.filter_mode);
        if let Some(handle) = self.materials.get(&key) {
            return handle.clone();
        }
        let base_color_texture = ribbon
            .texture
            .as_ref()
            .map(|texture| asset_server.load(format!("{asset_prefix}/{texture}")));
        let handle = materials.add(StandardMaterial {
            base_color: Color::WHITE,
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
) -> Option<(Entity, Wc3VisualAnimationSource)> {
    let mut current = entity;
    for _ in 0..128 {
        if let Ok(source) = roots.get(current) {
            return Some((current, source.clone()));
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
        let Some((model_root, source)) = wc3_visual_animation_source(entity, &parents, &roots)
        else {
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
        commands
            .entity(model_root)
            .insert(Wc3ModelSequenceSelection::new(source.animation_name));
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
            && extras.alpha_track.is_none()
            && extras.texture_id_track.is_none()
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

        let alpha_track = extras.alpha_track.clone();
        let texture_id_track = extras.texture_id_track.clone();
        if alpha_track.is_some() || texture_id_track.is_some() {
            let Some(source) = materials.get(&material_handle.0).cloned() else {
                continue 'mesh;
            };
            material_handle.0 = materials.add(source);
        }

        if let Some(track) = alpha_track {
            commands.entity(entity).insert(Wc3AnimatedMaterialAlpha {
                static_alpha: extras.layer_alpha.clamp(0.0, 1.0),
                track,
                sequence_windows: extras.sequence_windows.clone(),
                global_sequence_durations_ms: extras.global_sequence_durations_ms.clone(),
                fallback_elapsed_ms: 0.0,
                last_alpha_bits: u32::MAX,
            });
        }

        if let Some(track) = texture_id_track {
            let static_texture = materials
                .get(&material_handle.0)
                .and_then(|material| material.base_color_texture.clone());
            let textures = wc3_material_texture_handles(
                &asset_server,
                static_texture.as_ref(),
                &extras.texture_paths,
            );
            commands.entity(entity).insert(Wc3AnimatedMaterialTexture {
                static_texture_id: extras.texture_id,
                static_texture,
                textures,
                track,
                sequence_windows: extras.sequence_windows,
                global_sequence_durations_ms: extras.global_sequence_durations_ms,
                fallback_elapsed_ms: 0.0,
                last_texture_id: None,
            });
        }

        commands.entity(entity).insert(Wc3MaterialProcessed);
    }
}

fn wc3_material_texture_handles(
    asset_server: &AssetServer,
    source_texture: Option<&Handle<Image>>,
    texture_paths: &[Wc3MaterialTexturePath],
) -> BTreeMap<u32, Handle<Image>> {
    let Some(source_texture) = source_texture else {
        return BTreeMap::new();
    };
    let Some(source_path) = asset_server.get_path(source_texture.id()) else {
        return BTreeMap::new();
    };
    texture_paths
        .iter()
        .filter_map(|texture| {
            let path = source_path.resolve_embed_str(&texture.file_name).ok()?;
            let handle: Handle<Image> = asset_server.load(path);
            Some((texture.texture_id, handle))
        })
        .collect()
}

pub fn advance_wc3_model_sequence_clocks(
    mut commands: Commands,
    time: Res<Time>,
    mut roots: Query<(
        Entity,
        &Wc3ModelSequenceSelection,
        Option<&mut Wc3ModelSequenceClock>,
    )>,
) {
    let dt_ms = time.delta_secs().max(0.0) * 1000.0;
    for (entity, selection, clock) in &mut roots {
        if let Some(mut clock) = clock {
            advance_model_sequence_clock(&mut clock, selection, dt_ms);
        } else {
            commands.entity(entity).insert(Wc3ModelSequenceClock {
                sequence_name: selection.name.clone(),
                sequence_elapsed_ms: 0.0,
                global_elapsed_ms: dt_ms,
            });
        }
    }
}

fn advance_model_sequence_clock(
    clock: &mut Wc3ModelSequenceClock,
    selection: &Wc3ModelSequenceSelection,
    dt_ms: f32,
) {
    if clock.sequence_name != selection.name {
        clock.sequence_name.clone_from(&selection.name);
        clock.sequence_elapsed_ms = 0.0;
    } else {
        clock.sequence_elapsed_ms += dt_ms;
    }
    clock.global_elapsed_ms += dt_ms;
}

fn inherited_wc3_model_sequence_clock(
    entity: Entity,
    parents: &Query<&ChildOf>,
    clocks: &Query<&Wc3ModelSequenceClock>,
) -> Option<Wc3ModelSequenceClock> {
    let mut current = entity;
    for _ in 0..128 {
        if let Ok(clock) = clocks.get(current) {
            return Some(clock.clone());
        }
        let Ok(parent) = parents.get(current) else {
            return None;
        };
        current = parent.parent();
    }
    None
}

fn material_sequence_time_ms(
    sequence_windows: &[Wc3EmitterSequenceWindow],
    clock: &Wc3ModelSequenceClock,
) -> f32 {
    let Some(window) = sequence_windows
        .iter()
        .find(|window| window.name.eq_ignore_ascii_case(&clock.sequence_name))
    else {
        return clock.sequence_elapsed_ms;
    };
    let duration = window.end_ms.saturating_sub(window.start_ms).max(1) as f32;
    let offset = if window.non_looping {
        clock.sequence_elapsed_ms.min(duration)
    } else {
        clock.sequence_elapsed_ms.rem_euclid(duration)
    };
    window.start_ms as f32 + offset
}

fn wc3_model_light_color(color: [f32; 3]) -> Color {
    Color::srgb(color[0], color[1], color[2])
}

fn wc3_model_light_lumens(intensity: f32, visibility: f32) -> f32 {
    intensity.max(0.0) * visibility.max(0.0) * WC3_MODEL_LIGHT_LUMENS_PER_INTENSITY
}

fn wc3_model_light_range(attenuation_end: f32, source_scale: f32) -> f32 {
    (attenuation_end * source_scale.abs()).max(0.001)
}

pub fn setup_wc3_model_lights(
    mut commands: Commands,
    nodes: Query<(Entity, &GltfExtras), Added<GltfExtras>>,
) {
    for (entity, raw_extras) in &nodes {
        let Ok(extras) = serde_json::from_str::<Wc3NodeExtras>(&raw_extras.value) else {
            continue;
        };
        let Some(spec) = extras.wc3_light else {
            continue;
        };
        if !spec.light_type.eq_ignore_ascii_case("omni") {
            continue;
        }

        commands.entity(entity).insert(PointLight {
            color: wc3_model_light_color(spec.color),
            intensity: wc3_model_light_lumens(spec.intensity, 1.0),
            range: wc3_model_light_range(spec.attenuation_end, 1.0),
            shadow_maps_enabled: false,
            ..default()
        });

        commands.entity(entity).insert(Wc3PointLightRuntime {
            static_color: spec.color,
            static_intensity: spec.intensity,
            static_range: spec.attenuation_end,
            attenuation_end_track: spec.attenuation_end_track,
            color_track: spec.color_track,
            intensity_track: spec.intensity_track,
            visibility_track: spec.visibility_track,
            sequence_windows: spec.sequence_windows,
            global_sequence_durations_ms: spec.global_sequence_durations_ms,
            fallback_elapsed_ms: 0.0,
        });
    }
}

pub fn update_wc3_model_lights(
    time: Res<Time>,
    parents: Query<&ChildOf>,
    clocks: Query<&Wc3ModelSequenceClock>,
    mut lights: Query<(
        Entity,
        &GlobalTransform,
        &mut PointLight,
        &mut Wc3PointLightRuntime,
    )>,
) {
    let dt_ms = time.delta_secs().max(0.0) * 1000.0;
    for (entity, transform, mut light, mut animation) in &mut lights {
        animation.fallback_elapsed_ms += dt_ms;
        let inherited = inherited_wc3_model_sequence_clock(entity, &parents, &clocks);
        let (sequence_time_ms, global_elapsed_ms) = inherited.as_ref().map_or(
            (animation.fallback_elapsed_ms, animation.fallback_elapsed_ms),
            |clock| {
                (
                    material_sequence_time_ms(&animation.sequence_windows, clock),
                    clock.global_elapsed_ms,
                )
            },
        );
        let range = sample_scalar_track(
            animation.attenuation_end_track.as_ref(),
            animation.static_range,
            sequence_time_ms,
            global_elapsed_ms,
            &animation.global_sequence_durations_ms,
        );
        let color = sample_vector3_track(
            animation.color_track.as_ref(),
            animation.static_color,
            sequence_time_ms,
            global_elapsed_ms,
            &animation.global_sequence_durations_ms,
        );
        let intensity = sample_scalar_track(
            animation.intensity_track.as_ref(),
            animation.static_intensity,
            sequence_time_ms,
            global_elapsed_ms,
            &animation.global_sequence_durations_ms,
        );
        let visibility = sample_scalar_track(
            animation.visibility_track.as_ref(),
            1.0,
            sequence_time_ms,
            global_elapsed_ms,
            &animation.global_sequence_durations_ms,
        );

        let source_scale = transform
            .to_scale_rotation_translation()
            .0
            .abs()
            .max_element()
            .max(0.000_1);
        light.color = wc3_model_light_color(color);
        light.intensity = wc3_model_light_lumens(intensity, visibility);
        light.range = wc3_model_light_range(range, source_scale);
    }
}

pub fn update_wc3_material_alpha(
    time: Res<Time>,
    parents: Query<&ChildOf>,
    clocks: Query<&Wc3ModelSequenceClock>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut animated: Query<(
        Entity,
        &MeshMaterial3d<StandardMaterial>,
        &mut Wc3AnimatedMaterialAlpha,
    )>,
) {
    let dt_ms = time.delta_secs().max(0.0) * 1000.0;
    for (entity, material_handle, mut animation) in &mut animated {
        animation.fallback_elapsed_ms += dt_ms;
        let inherited = inherited_wc3_model_sequence_clock(entity, &parents, &clocks);
        let (sequence_time_ms, global_elapsed_ms) = inherited.as_ref().map_or(
            (animation.fallback_elapsed_ms, animation.fallback_elapsed_ms),
            |clock| {
                (
                    material_sequence_time_ms(&animation.sequence_windows, clock),
                    clock.global_elapsed_ms,
                )
            },
        );
        let alpha = sample_scalar_track(
            Some(&animation.track),
            animation.static_alpha,
            sequence_time_ms,
            global_elapsed_ms,
            &animation.global_sequence_durations_ms,
        )
        .clamp(0.0, 1.0);
        let alpha_bits = alpha.to_bits();
        if alpha_bits == animation.last_alpha_bits {
            continue;
        }
        let Some(mut material) = materials.get_mut(&material_handle.0) else {
            continue;
        };
        let base = material.base_color.to_linear();
        material.base_color = Color::linear_rgba(base.red, base.green, base.blue, alpha);
        animation.last_alpha_bits = alpha_bits;
    }
}

pub fn update_wc3_material_texture(
    time: Res<Time>,
    parents: Query<&ChildOf>,
    clocks: Query<&Wc3ModelSequenceClock>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut animated: Query<(
        Entity,
        &MeshMaterial3d<StandardMaterial>,
        &mut Wc3AnimatedMaterialTexture,
    )>,
) {
    let dt_ms = time.delta_secs().max(0.0) * 1000.0;
    for (entity, material_handle, mut animation) in &mut animated {
        animation.fallback_elapsed_ms += dt_ms;
        let inherited = inherited_wc3_model_sequence_clock(entity, &parents, &clocks);
        let (sequence_time_ms, global_elapsed_ms) = inherited.as_ref().map_or(
            (animation.fallback_elapsed_ms, animation.fallback_elapsed_ms),
            |clock| {
                (
                    material_sequence_time_ms(&animation.sequence_windows, clock),
                    clock.global_elapsed_ms,
                )
            },
        );
        let texture_id = sample_unsigned_track(
            Some(&animation.track),
            animation.static_texture_id,
            sequence_time_ms,
            global_elapsed_ms,
            &animation.global_sequence_durations_ms,
        );
        if animation.last_texture_id == Some(texture_id) {
            continue;
        }
        let selected = animation
            .textures
            .get(&texture_id)
            .cloned()
            .or_else(|| animation.static_texture.clone());
        let Some(mut material) = materials.get_mut(&material_handle.0) else {
            continue;
        };
        material.base_color_texture = selected;
        animation.last_texture_id = Some(texture_id);
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
        let sequence_time_ms = source_ribbons
            .sequence_clock
            .as_ref()
            .map(Wc3EmitterSequenceClock::current_time_ms)
            .unwrap_or(source_ribbons.global_elapsed_ms);
        for (ribbon_index, ribbon) in source_ribbons.ribbons.iter().enumerate() {
            let spec = &ribbon.spec;
            let sample =
                sample_ribbon_parameters(spec, sequence_time_ms, source_ribbons.global_elapsed_ms);
            if spec.emission_rate == 0
                || spec.lifespan <= 0.0
                || (spec.height_above <= 0.0 && spec.height_below <= 0.0)
            {
                continue;
            }
            let mesh = meshes.add(build_wc3_ribbon_mesh(spec, sample, &VecDeque::new(), 1.0));
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
    mut sources: Query<(Entity, &GlobalTransform, &mut Wc3RibbonSource)>,
    transforms: Query<&GlobalTransform>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut trails: Query<(Entity, &mut Wc3RibbonTrail)>,
) {
    let dt = time.delta_secs().min(0.1);
    let mut source_times = HashMap::new();
    for (entity, _, mut source) in &mut sources {
        let sequence_time_ms = source
            .sequence_clock
            .as_ref()
            .map(Wc3EmitterSequenceClock::current_time_ms)
            .unwrap_or(source.global_elapsed_ms);
        source_times.insert(entity, (sequence_time_ms, source.global_elapsed_ms));
        if let Some(clock) = source.sequence_clock.as_mut() {
            clock.advance(dt);
        }
        source.global_elapsed_ms += dt * 1000.0;
    }
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

        let source_transform = sources.get_mut(trail.source).ok();
        let source_missing = source_transform.is_none();
        let sample = source_times
            .get(&trail.source)
            .map(|(sequence_time_ms, global_elapsed_ms)| {
                sample_ribbon_parameters(&trail.spec, *sequence_time_ms, *global_elapsed_ms)
            })
            .unwrap_or_else(|| sample_ribbon_parameters(&trail.spec, 0.0, 0.0));
        if let Some((_, root_transform, source)) = source_transform
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
            if sample.visibility > 0.001 {
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
            } else {
                trail.emission_accumulator = 0.0;
            }
            trail.previous_origin = Some(origin);
            trail.previous_up = Some(up);
        }

        while trail.points.len() > MAX_RIBBON_POINTS {
            trail.points.pop_front();
        }
        if let Some(mut mesh) = meshes.get_mut(&trail.mesh) {
            *mesh = build_wc3_ribbon_mesh(&trail.spec, sample, &trail.points, trail.source_scale);
        }

        if source_missing && trail.points.is_empty() {
            meshes.remove(trail.mesh.id());
            commands.entity(entity).despawn();
        }
    }
}

fn build_wc3_ribbon_mesh(
    spec: &Wc3RibbonEmitter,
    sample: Wc3RibbonSample,
    points: &VecDeque<RibbonPoint>,
    source_scale: f32,
) -> Mesh {
    let mut positions = Vec::with_capacity(points.len() * 2);
    let mut normals = Vec::with_capacity(points.len() * 2);
    let mut uvs = Vec::with_capacity(points.len() * 2);
    let mut colors = Vec::with_capacity(points.len() * 2);
    let mut indices = Vec::with_capacity(points.len().saturating_sub(1) * 6);
    let last = points.len().saturating_sub(1).max(1) as f32;
    let columns = spec.columns.max(1);
    let rows = spec.rows.max(1);
    let frame_count = columns.saturating_mul(rows).max(1);
    let texture_slot = sample.texture_slot.min(frame_count - 1);
    let column = texture_slot % columns;
    let row = texture_slot / columns;
    let atlas_u = 1.0 / columns as f32;
    let atlas_v = 1.0 / rows as f32;
    let u0 = column as f32 * atlas_u;
    let u1 = (column + 1) as f32 * atlas_u;
    let v0 = row as f32 * atlas_v;
    let v1 = (row + 1) as f32 * atlas_v;

    for (index, point) in points.iter().enumerate() {
        let gravity_offset =
            Vec3::NEG_Y * (0.5 * spec.gravity * source_scale * point.age * point.age);
        let center = point.center + gravity_offset;
        let up = point.up.normalize_or(Vec3::Y);
        let top = center + up * sample.height_above.max(0.0) * source_scale;
        let bottom = center - up * sample.height_below.max(0.0) * source_scale;
        positions.push(top.to_array());
        positions.push(bottom.to_array());
        normals.push(Vec3::Z.to_array());
        normals.push(Vec3::Z.to_array());
        let v = v0 + index as f32 / last * (v1 - v0);
        uvs.push([u0, v]);
        uvs.push([u1, v]);
        let fade = (1.0 - point.age / spec.lifespan.max(0.01)).clamp(0.0, 1.0);
        let alpha = sample.alpha * fade;
        colors.push([sample.color[0], sample.color[1], sample.color[2], alpha]);
        colors.push([sample.color[0], sample.color[1], sample.color[2], alpha]);
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
        let sequence_time_ms = source
            .sequence_clock
            .as_ref()
            .map(Wc3EmitterSequenceClock::current_time_ms)
            .unwrap_or(source.global_elapsed_ms);
        let global_elapsed_ms = source.global_elapsed_ms;
        for (emitter_index, emitter) in source.emitters.iter_mut().enumerate() {
            let sample =
                sample_emitter_parameters(&emitter.spec, sequence_time_ms, global_elapsed_ms);
            if sample.visibility <= 0.001 {
                emitter.accumulator = 0.0;
                continue;
            }
            let mut count = if emitter.burst_pending {
                emitter.burst_pending = false;
                8
            } else {
                emitter.accumulator += sample.emission_rate.clamp(0.0, 240.0) * dt;
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
                let local_spawn = particle_spawn_offset(seed, sample.width, sample.length);
                let origin = base_origin + source_rotation * (local_spawn * source_scale);
                let velocity = particle_velocity(
                    source_rotation,
                    seed,
                    sample.speed,
                    sample.variation,
                    sample.latitude,
                ) * uniform_scale;
                commands.spawn((
                    Mesh3d(particle_mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(origin)
                        .with_scale(Vec3::splat(particle_scales[0].max(0.01))),
                    Wc3Particle {
                        velocity,
                        gravity: sample.gravity * uniform_scale,
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
        if let Some(clock) = source.sequence_clock.as_mut() {
            clock.advance(dt);
        }
        source.global_elapsed_ms += dt * 1000.0;
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

#[derive(Debug, Clone, Copy)]
struct Wc3EmitterSample {
    speed: f32,
    variation: f32,
    latitude: f32,
    gravity: f32,
    emission_rate: f32,
    length: f32,
    width: f32,
    visibility: f32,
}

fn sample_emitter_parameters(
    emitter: &Wc3ParticleEmitter,
    sequence_time_ms: f32,
    global_elapsed_ms: f32,
) -> Wc3EmitterSample {
    let value = |track: Option<&Wc3ScalarTrack>, default: f32| {
        sample_scalar_track(
            track,
            default,
            sequence_time_ms,
            global_elapsed_ms,
            &emitter.global_sequence_durations_ms,
        )
    };
    Wc3EmitterSample {
        speed: value(emitter.speed_track.as_ref(), emitter.speed),
        variation: value(emitter.variation_track.as_ref(), emitter.variation),
        latitude: value(emitter.latitude_track.as_ref(), emitter.latitude),
        gravity: value(emitter.gravity_track.as_ref(), emitter.gravity),
        emission_rate: value(emitter.emission_rate_track.as_ref(), emitter.emission_rate),
        length: value(emitter.length_track.as_ref(), emitter.length),
        width: value(emitter.width_track.as_ref(), emitter.width),
        visibility: value(emitter.visibility_track.as_ref(), 1.0),
    }
}

fn sample_scalar_track(
    track: Option<&Wc3ScalarTrack>,
    default: f32,
    sequence_time_ms: f32,
    global_elapsed_ms: f32,
    global_sequence_durations_ms: &[u32],
) -> f32 {
    let Some(track) = track else {
        return default;
    };
    if track.timestamps.is_empty() || track.values.is_empty() {
        return default;
    }
    let count = track.timestamps.len().min(track.values.len());
    if count == 0 {
        return default;
    }

    let time_ms = track
        .global_sequence_id
        .and_then(|id| global_sequence_durations_ms.get(id as usize).copied())
        .filter(|duration| *duration != 0)
        .map_or(sequence_time_ms, |duration| {
            global_elapsed_ms.rem_euclid(duration as f32)
        });

    if time_ms <= track.timestamps[0] as f32 {
        return track.values[0];
    }
    if time_ms >= track.timestamps[count - 1] as f32 {
        return track.values[count - 1];
    }

    let upper = track.timestamps[..count].partition_point(|timestamp| *timestamp as f32 <= time_ms);
    if upper == 0 {
        return track.values[0];
    }
    if upper >= count {
        return track.values[count - 1];
    }
    let lower = upper - 1;
    if track.interpolation == Wc3ScalarInterpolation::DontInterp {
        return track.values[lower];
    }

    let start_ms = track.timestamps[lower] as f32;
    let end_ms = track.timestamps[upper] as f32;
    let span = (end_ms - start_ms).max(f32::EPSILON);
    let t = ((time_ms - start_ms) / span).clamp(0.0, 1.0);
    let p0 = track.values[lower];
    let p1 = track.values[upper];
    match track.interpolation {
        Wc3ScalarInterpolation::DontInterp => p0,
        Wc3ScalarInterpolation::Linear => p0 + (p1 - p0) * t,
        Wc3ScalarInterpolation::Hermite => {
            let Some(out0) = track.out_tangents.get(lower).copied() else {
                return p0 + (p1 - p0) * t;
            };
            let Some(in1) = track.in_tangents.get(upper).copied() else {
                return p0 + (p1 - p0) * t;
            };
            let t2 = t * t;
            let t3 = t2 * t;
            (2.0 * t3 - 3.0 * t2 + 1.0) * p0
                + (t3 - 2.0 * t2 + t) * out0
                + (-2.0 * t3 + 3.0 * t2) * p1
                + (t3 - t2) * in1
        }
        Wc3ScalarInterpolation::Bezier => {
            let Some(out0) = track.out_tangents.get(lower).copied() else {
                return p0 + (p1 - p0) * t;
            };
            let Some(in1) = track.in_tangents.get(upper).copied() else {
                return p0 + (p1 - p0) * t;
            };
            let one_minus_t = 1.0 - t;
            one_minus_t.powi(3) * p0
                + 3.0 * one_minus_t.powi(2) * t * out0
                + 3.0 * one_minus_t * t * t * in1
                + t.powi(3) * p1
        }
    }
}

fn sample_vector3_track(
    track: Option<&Wc3Vector3Track>,
    default: [f32; 3],
    sequence_time_ms: f32,
    global_elapsed_ms: f32,
    global_sequence_durations_ms: &[u32],
) -> [f32; 3] {
    let Some(track) = track else {
        return default;
    };
    if track.timestamps.is_empty() || track.values.is_empty() {
        return default;
    }
    let count = track.timestamps.len().min(track.values.len());
    if count == 0 {
        return default;
    }
    let time_ms = track
        .global_sequence_id
        .and_then(|id| global_sequence_durations_ms.get(id as usize).copied())
        .filter(|duration| *duration != 0)
        .map_or(sequence_time_ms, |duration| {
            global_elapsed_ms.rem_euclid(duration as f32)
        });

    if time_ms <= track.timestamps[0] as f32 {
        return track.values[0];
    }
    if time_ms >= track.timestamps[count - 1] as f32 {
        return track.values[count - 1];
    }
    let upper = track.timestamps[..count].partition_point(|timestamp| *timestamp as f32 <= time_ms);
    if upper == 0 {
        return track.values[0];
    }
    if upper >= count {
        return track.values[count - 1];
    }
    let lower = upper - 1;
    if track.interpolation == Wc3ScalarInterpolation::DontInterp {
        return track.values[lower];
    }

    let start_ms = track.timestamps[lower] as f32;
    let end_ms = track.timestamps[upper] as f32;
    let span = (end_ms - start_ms).max(f32::EPSILON);
    let t = ((time_ms - start_ms) / span).clamp(0.0, 1.0);
    let p0 = track.values[lower];
    let p1 = track.values[upper];
    match track.interpolation {
        Wc3ScalarInterpolation::DontInterp => p0,
        Wc3ScalarInterpolation::Linear => {
            std::array::from_fn(|channel| p0[channel] + (p1[channel] - p0[channel]) * t)
        }
        Wc3ScalarInterpolation::Hermite => {
            let Some(out0) = track.out_tangents.get(lower).copied() else {
                return std::array::from_fn(|channel| {
                    p0[channel] + (p1[channel] - p0[channel]) * t
                });
            };
            let Some(in1) = track.in_tangents.get(upper).copied() else {
                return std::array::from_fn(|channel| {
                    p0[channel] + (p1[channel] - p0[channel]) * t
                });
            };
            let t2 = t * t;
            let t3 = t2 * t;
            std::array::from_fn(|channel| {
                (2.0 * t3 - 3.0 * t2 + 1.0) * p0[channel]
                    + (t3 - 2.0 * t2 + t) * out0[channel]
                    + (-2.0 * t3 + 3.0 * t2) * p1[channel]
                    + (t3 - t2) * in1[channel]
            })
        }
        Wc3ScalarInterpolation::Bezier => {
            let Some(out0) = track.out_tangents.get(lower).copied() else {
                return std::array::from_fn(|channel| {
                    p0[channel] + (p1[channel] - p0[channel]) * t
                });
            };
            let Some(in1) = track.in_tangents.get(upper).copied() else {
                return std::array::from_fn(|channel| {
                    p0[channel] + (p1[channel] - p0[channel]) * t
                });
            };
            let one_minus_t = 1.0 - t;
            std::array::from_fn(|channel| {
                one_minus_t.powi(3) * p0[channel]
                    + 3.0 * one_minus_t.powi(2) * t * out0[channel]
                    + 3.0 * one_minus_t * t * t * in1[channel]
                    + t.powi(3) * p1[channel]
            })
        }
    }
}

fn sample_unsigned_track(
    track: Option<&Wc3UnsignedTrack>,
    default: u32,
    sequence_time_ms: f32,
    global_elapsed_ms: f32,
    global_sequence_durations_ms: &[u32],
) -> u32 {
    let Some(track) = track else {
        return default;
    };
    if track.timestamps.is_empty() || track.values.is_empty() {
        return default;
    }
    let count = track.timestamps.len().min(track.values.len());
    if count == 0 {
        return default;
    }
    let time_ms = track
        .global_sequence_id
        .and_then(|id| global_sequence_durations_ms.get(id as usize).copied())
        .filter(|duration| *duration != 0)
        .map_or(sequence_time_ms, |duration| {
            global_elapsed_ms.rem_euclid(duration as f32)
        });
    if time_ms <= track.timestamps[0] as f32 {
        return track.values[0];
    }
    if time_ms >= track.timestamps[count - 1] as f32 {
        return track.values[count - 1];
    }
    let upper = track.timestamps[..count].partition_point(|timestamp| *timestamp as f32 <= time_ms);
    if upper == 0 {
        return track.values[0];
    }
    if upper >= count {
        return track.values[count - 1];
    }
    let lower = upper - 1;
    if track.interpolation == Wc3ScalarInterpolation::DontInterp {
        return track.values[lower];
    }
    let start_ms = track.timestamps[lower] as f32;
    let end_ms = track.timestamps[upper] as f32;
    let span = (end_ms - start_ms).max(f32::EPSILON);
    let t = ((time_ms - start_ms) / span).clamp(0.0, 1.0);
    let start = track.values[lower] as f32;
    let end = track.values[upper] as f32;
    (start + (end - start) * t).round().max(0.0) as u32
}

#[derive(Debug, Clone, Copy)]
struct Wc3RibbonSample {
    height_above: f32,
    height_below: f32,
    alpha: f32,
    color: [f32; 3],
    texture_slot: u32,
    visibility: f32,
}

fn sample_ribbon_parameters(
    ribbon: &Wc3RibbonEmitter,
    sequence_time_ms: f32,
    global_elapsed_ms: f32,
) -> Wc3RibbonSample {
    let durations = &ribbon.global_sequence_durations_ms;
    Wc3RibbonSample {
        height_above: sample_scalar_track(
            ribbon.height_above_track.as_ref(),
            ribbon.height_above,
            sequence_time_ms,
            global_elapsed_ms,
            durations,
        ),
        height_below: sample_scalar_track(
            ribbon.height_below_track.as_ref(),
            ribbon.height_below,
            sequence_time_ms,
            global_elapsed_ms,
            durations,
        ),
        alpha: sample_scalar_track(
            ribbon.alpha_track.as_ref(),
            ribbon.alpha,
            sequence_time_ms,
            global_elapsed_ms,
            durations,
        )
        .clamp(0.0, 1.0),
        color: sample_vector3_track(
            ribbon.color_track.as_ref(),
            ribbon.color,
            sequence_time_ms,
            global_elapsed_ms,
            durations,
        ),
        texture_slot: sample_unsigned_track(
            ribbon.texture_slot_track.as_ref(),
            ribbon.texture_slot,
            sequence_time_ms,
            global_elapsed_ms,
            durations,
        ),
        visibility: sample_scalar_track(
            ribbon.visibility_track.as_ref(),
            1.0,
            sequence_time_ms,
            global_elapsed_ms,
            durations,
        ),
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

fn particle_velocity(
    rotation: Quat,
    seed: u32,
    speed: f32,
    variation: f32,
    latitude_degrees: f32,
) -> Vec3 {
    let a = hash_unit(seed);
    let b = hash_unit(seed ^ 0xa511_e9b3);
    let latitude = latitude_degrees
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
    let speed_scale = 1.0 + (hash_unit(seed ^ 0x63d8_3595) * 2.0 - 1.0) * variation;
    rotation * local_direction * speed * speed_scale.max(0.0)
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
        stand_animation_name: model
            .animations
            .iter()
            .find(|animation| animation.name.eq_ignore_ascii_case("Stand"))
            .map(|animation| animation.name.clone()),
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
            speed_track: None,
            variation_track: None,
            latitude_track: None,
            gravity_track: None,
            emission_rate_track: None,
            length_track: None,
            width_track: None,
            visibility_track: None,
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
            sequence_windows: vec![Wc3EmitterSequenceWindow {
                name: "Stand".to_owned(),
                start_ms: 100,
                end_ms: 1100,
                non_looping: false,
            }],
            global_sequence_durations_ms: vec![500],
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
            texture_slot: 0,
            filter_mode: "Blend".to_owned(),
            texture: None,
            gravity: 0.0,
            height_above_track: None,
            height_below_track: None,
            alpha_track: None,
            color_track: None,
            texture_slot_track: None,
            visibility_track: None,
            sequence_windows: vec![Wc3EmitterSequenceWindow {
                name: "Stand".to_owned(),
                start_ms: 100,
                end_ms: 1100,
                non_looping: false,
            }],
            global_sequence_durations_ms: vec![500],
        }
    }

    #[test]
    fn visual_model_emitter_clock_matches_selected_animation_sequence() {
        let mut stand = test_particle_emitter(17);
        stand.active_sequences = vec!["Stand".to_owned()];
        stand.sequence_windows = vec![
            Wc3EmitterSequenceWindow {
                name: "Birth".to_owned(),
                start_ms: 0,
                end_ms: 500,
                non_looping: true,
            },
            Wc3EmitterSequenceWindow {
                name: "Stand".to_owned(),
                start_ms: 500,
                end_ms: 1500,
                non_looping: false,
            },
        ];
        let mut birth = stand.clone();
        birth.object_id = Some(18);
        birth.active_sequences = vec!["Birth".to_owned()];

        let model = Wc3VisualModel {
            scene: Handle::default(),
            gltf: Handle::default(),
            animation_name: Some("Birth".to_owned()),
            stand_animation_name: Some("Stand".to_owned()),
            emitters: vec![stand, birth],
            ribbons: Vec::new(),
        };

        let birth_source = model.emitter_source();
        assert_eq!(birth_source.emitters.len(), 1);
        assert_eq!(birth_source.emitters[0].spec.object_id, Some(18));
        let birth_clock = birth_source.sequence_clock.expect("birth sequence clock");
        assert_eq!(birth_clock.start_ms, 0);
        assert_eq!(birth_clock.end_ms, 500);
        assert!(birth_clock.non_looping);

        let stand_source = model.looping_emitter_source();
        assert_eq!(stand_source.emitters.len(), 1);
        assert_eq!(stand_source.emitters[0].spec.object_id, Some(17));
        let stand_clock = stand_source.sequence_clock.expect("stand sequence clock");
        assert_eq!(stand_clock.start_ms, 500);
        assert_eq!(stand_clock.end_ms, 1500);
        assert!(!stand_clock.non_looping);
        assert_eq!(
            model.looping_animation_source().unwrap().animation_name,
            "Stand"
        );
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
    fn legacy_model_particles_use_wc3_radian_cone_and_model_basis() {
        let straight = legacy_model_particle_velocity(Quat::IDENTITY, 17, 300.0, 0.0, 0.0);
        assert!((straight - Vec3::Y * 300.0).length() < 1.0e-4);

        let spread =
            legacy_model_particle_velocity(Quat::IDENTITY, 17, 300.0, std::f32::consts::PI, 0.8);
        assert!((spread.length() - 300.0).abs() < 1.0e-3);
        assert!(spread.y < 300.0);
    }

    #[test]
    fn wc3_model_light_mapping_preserves_scale_and_visibility() {
        assert_eq!(wc3_model_light_range(200.0, 0.5), 100.0);
        assert_eq!(wc3_model_light_range(200.0, 2.0), 400.0);
        assert_eq!(
            wc3_model_light_lumens(20.0, 1.0),
            PointLight::default().intensity
        );
        assert_eq!(wc3_model_light_lumens(20.0, 0.0), 0.0);
    }

    #[test]
    fn wc3_omni_light_extras_are_recognized() {
        let extras: Wc3NodeExtras = serde_json::from_str(
            r#"{
                "wc3ObjectId": 7,
                "wc3Light": {
                    "light_type": "omni",
                    "attenuation_end": 200.0,
                    "color": [1.0, 0.5, 0.25],
                    "intensity": 18.0,
                    "visibility_track": {
                        "interpolation": "dont_interp",
                        "global_sequence_id": null,
                        "timestamps": [0, 500],
                        "values": [1.0, 0.0]
                    }
                }
            }"#,
        )
        .expect("WC3 light node extras should deserialize");
        assert_eq!(extras.wc3_object_id, Some(7));
        let light = extras.wc3_light.expect("light extras");
        assert_eq!(light.light_type, "omni");
        assert_eq!(light.attenuation_end, 200.0);
        assert_eq!(light.intensity, 18.0);
        assert_eq!(
            light
                .visibility_track
                .as_ref()
                .expect("visibility track")
                .values,
            [1.0, 0.0]
        );
    }

    #[test]
    fn scalar_tracks_sample_sequence_and_global_sequence_time() {
        let linear = Wc3ScalarTrack {
            interpolation: Wc3ScalarInterpolation::Linear,
            global_sequence_id: None,
            timestamps: vec![100, 300],
            values: vec![10.0, 30.0],
            in_tangents: Vec::new(),
            out_tangents: Vec::new(),
        };
        assert_eq!(
            sample_scalar_track(Some(&linear), 0.0, 100.0, 0.0, &[]),
            10.0
        );
        assert_eq!(
            sample_scalar_track(Some(&linear), 0.0, 200.0, 0.0, &[]),
            20.0
        );
        assert_eq!(
            sample_scalar_track(Some(&linear), 0.0, 500.0, 0.0, &[]),
            30.0
        );

        let stepped = Wc3ScalarTrack {
            interpolation: Wc3ScalarInterpolation::DontInterp,
            global_sequence_id: None,
            timestamps: vec![100, 300],
            values: vec![4.0, 8.0],
            in_tangents: Vec::new(),
            out_tangents: Vec::new(),
        };
        assert_eq!(
            sample_scalar_track(Some(&stepped), 0.0, 299.0, 0.0, &[]),
            4.0
        );

        let global = Wc3ScalarTrack {
            interpolation: Wc3ScalarInterpolation::Linear,
            global_sequence_id: Some(0),
            timestamps: vec![0, 500],
            values: vec![0.0, 10.0],
            in_tangents: Vec::new(),
            out_tangents: Vec::new(),
        };
        assert_eq!(
            sample_scalar_track(Some(&global), 0.0, 9999.0, 750.0, &[500]),
            5.0
        );
    }

    #[test]
    fn ribbon_tracks_sample_sequence_and_global_sequence_time() {
        let mut ribbon = test_ribbon_emitter(23);
        ribbon.height_above_track = Some(Wc3ScalarTrack {
            interpolation: Wc3ScalarInterpolation::Linear,
            global_sequence_id: None,
            timestamps: vec![100, 1100],
            values: vec![1.0, 3.0],
            in_tangents: Vec::new(),
            out_tangents: Vec::new(),
        });
        ribbon.alpha_track = Some(Wc3ScalarTrack {
            interpolation: Wc3ScalarInterpolation::Linear,
            global_sequence_id: Some(0),
            timestamps: vec![0, 500],
            values: vec![1.0, 0.0],
            in_tangents: Vec::new(),
            out_tangents: Vec::new(),
        });
        ribbon.color_track = Some(Wc3Vector3Track {
            interpolation: Wc3ScalarInterpolation::Linear,
            global_sequence_id: None,
            timestamps: vec![100, 1100],
            values: vec![[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            in_tangents: Vec::new(),
            out_tangents: Vec::new(),
        });
        ribbon.texture_slot_track = Some(Wc3UnsignedTrack {
            interpolation: Wc3ScalarInterpolation::DontInterp,
            global_sequence_id: None,
            timestamps: vec![100, 600],
            values: vec![0, 3],
        });
        ribbon.visibility_track = Some(Wc3ScalarTrack {
            interpolation: Wc3ScalarInterpolation::DontInterp,
            global_sequence_id: None,
            timestamps: vec![100, 800],
            values: vec![1.0, 0.0],
            in_tangents: Vec::new(),
            out_tangents: Vec::new(),
        });

        let sample = sample_ribbon_parameters(&ribbon, 600.0, 250.0);
        assert_eq!(sample.height_above, 2.0);
        assert!((sample.alpha - 0.5).abs() < 1.0e-6);
        assert_eq!(sample.color, [0.5, 0.0, 0.5]);
        assert_eq!(sample.texture_slot, 3);
        assert_eq!(sample.visibility, 1.0);

        let hidden = sample_ribbon_parameters(&ribbon, 900.0, 250.0);
        assert_eq!(hidden.visibility, 0.0);
    }

    #[test]
    fn ribbon_sequence_clock_uses_selected_animation_window() {
        let mut ribbon = test_ribbon_emitter(23);
        ribbon.sequence_windows = vec![
            Wc3EmitterSequenceWindow {
                name: "Birth".to_owned(),
                start_ms: 0,
                end_ms: 400,
                non_looping: true,
            },
            Wc3EmitterSequenceWindow {
                name: "Stand".to_owned(),
                start_ms: 400,
                end_ms: 1400,
                non_looping: false,
            },
        ];
        let source =
            Wc3RibbonSource::with_asset_prefix_for_sequence(&[ribbon], "wc3/units", "Stand");
        let clock = source.sequence_clock.expect("Stand clock");
        assert_eq!(clock.start_ms, 400);
        assert_eq!(clock.end_ms, 1400);
        assert!(!clock.non_looping);
    }

    #[test]
    fn model_sequence_clock_preserves_global_time_across_sequence_changes() {
        let mut clock = Wc3ModelSequenceClock {
            sequence_name: "Stand".to_owned(),
            sequence_elapsed_ms: 250.0,
            global_elapsed_ms: 1000.0,
        };
        advance_model_sequence_clock(&mut clock, &Wc3ModelSequenceSelection::new("Stand"), 100.0);
        assert_eq!(clock.sequence_elapsed_ms, 350.0);
        assert_eq!(clock.global_elapsed_ms, 1100.0);

        advance_model_sequence_clock(&mut clock, &Wc3ModelSequenceSelection::new("Attack"), 50.0);
        assert_eq!(clock.sequence_name, "Attack");
        assert_eq!(clock.sequence_elapsed_ms, 0.0);
        assert_eq!(clock.global_elapsed_ms, 1150.0);
    }

    #[test]
    fn attachment_child_path_stays_inside_parent_model_pack() {
        assert_eq!(
            child_model_asset_path(
                "wc3/buildings/models/altar.gltf",
                "models/sharedmodels__nagabirth.gltf"
            )
            .as_deref(),
            Some("wc3/buildings/models/sharedmodels__nagabirth.gltf")
        );
        assert!(
            child_model_asset_path("wc3/buildings/models/altar.gltf", "../escape.gltf").is_none()
        );
    }

    #[test]
    fn attachment_visibility_does_not_leak_keys_from_other_sequences() {
        let animation = Wc3ModelAttachmentRuntime {
            child_model: RegisteredConvertedModel {
                asset_path: "wc3/buildings/models/sharedmodels__nagabirth.gltf".to_owned(),
                asset_prefix: "wc3/buildings",
                manifest: ModelManifest {
                    gltf: "models/sharedmodels__nagabirth.gltf".to_owned(),
                    animations: Vec::new(),
                    particle_emitters: Vec::new(),
                    ribbon_emitters: Vec::new(),
                },
            },
            visibility_track: Some(Wc3ScalarTrack {
                interpolation: Wc3ScalarInterpolation::DontInterp,
                global_sequence_id: None,
                timestamps: vec![61_667, 65_000],
                values: vec![0.0, 0.0],
                in_tangents: Vec::new(),
                out_tangents: Vec::new(),
            }),
            sequence_windows: vec![
                Wc3EmitterSequenceWindow {
                    name: "Birth".to_owned(),
                    start_ms: 0,
                    end_ms: 60_000,
                    non_looping: true,
                },
                Wc3EmitterSequenceWindow {
                    name: "Stand".to_owned(),
                    start_ms: 61_667,
                    end_ms: 64_333,
                    non_looping: false,
                },
            ],
            global_sequence_durations_ms: Vec::new(),
            fallback_elapsed_ms: 0.0,
            child: None,
        };
        let birth_clock = Wc3ModelSequenceClock {
            sequence_name: "Birth".to_owned(),
            sequence_elapsed_ms: 30_000.0,
            global_elapsed_ms: 30_000.0,
        };
        assert_eq!(
            model_attachment_visibility(&animation, Some(&birth_clock)),
            1.0
        );

        let stand_clock = Wc3ModelSequenceClock {
            sequence_name: "Stand".to_owned(),
            sequence_elapsed_ms: 0.0,
            global_elapsed_ms: 61_667.0,
        };
        assert_eq!(
            model_attachment_visibility(&animation, Some(&stand_clock)),
            0.0
        );
    }

    #[test]
    fn material_sequence_time_uses_selected_wc3_window() {
        let animation = Wc3AnimatedMaterialAlpha {
            static_alpha: 1.0,
            track: Wc3ScalarTrack {
                interpolation: Wc3ScalarInterpolation::Linear,
                global_sequence_id: None,
                timestamps: vec![500, 1500],
                values: vec![0.0, 1.0],
                in_tangents: Vec::new(),
                out_tangents: Vec::new(),
            },
            sequence_windows: vec![Wc3EmitterSequenceWindow {
                name: "Stand".to_owned(),
                start_ms: 500,
                end_ms: 1500,
                non_looping: false,
            }],
            global_sequence_durations_ms: Vec::new(),
            fallback_elapsed_ms: 0.0,
            last_alpha_bits: u32::MAX,
        };
        let clock = Wc3ModelSequenceClock {
            sequence_name: "Stand".to_owned(),
            sequence_elapsed_ms: 1250.0,
            global_elapsed_ms: 9000.0,
        };
        assert_eq!(
            material_sequence_time_ms(&animation.sequence_windows, &clock),
            750.0
        );
        assert_eq!(
            sample_scalar_track(
                Some(&animation.track),
                animation.static_alpha,
                material_sequence_time_ms(&animation.sequence_windows, &clock),
                clock.global_elapsed_ms,
                &animation.global_sequence_durations_ms,
            ),
            0.25
        );
    }

    #[test]
    fn emitter_sequence_clock_loops_or_clamps_from_authored_flags() {
        let mut looping = Wc3EmitterSequenceClock {
            start_ms: 1000,
            end_ms: 1400,
            non_looping: false,
            elapsed_ms: 0.0,
        };
        looping.advance(0.5);
        assert_eq!(looping.current_time_ms(), 1100.0);

        let mut one_shot = Wc3EmitterSequenceClock {
            start_ms: 2000,
            end_ms: 2300,
            non_looping: true,
            elapsed_ms: 0.0,
        };
        one_shot.advance(1.0);
        assert_eq!(one_shot.current_time_ms(), 2300.0);
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
            rows: 2,
            columns: 2,
            texture_slot: 3,
            filter_mode: "AddAlpha".to_owned(),
            texture: Some("textures/ribbon.png".to_owned()),
            gravity: 0.0,
            height_above_track: None,
            height_below_track: None,
            alpha_track: None,
            color_track: None,
            texture_slot_track: None,
            visibility_track: None,
            sequence_windows: Vec::new(),
            global_sequence_durations_ms: Vec::new(),
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
        let sample = sample_ribbon_parameters(&spec, 0.0, 0.0);
        let mesh = build_wc3_ribbon_mesh(&spec, sample, &points, 1.0);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .expect("ribbon positions")
            .as_float3()
            .expect("float positions");
        assert_eq!(positions.len(), 4);
        assert_eq!(positions[0], [0.0, 2.0, 0.0]);
        assert_eq!(positions[1], [0.0, -3.0, 0.0]);
        let bevy::mesh::VertexAttributeValues::Float32x2(uvs) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_0).expect("ribbon uvs")
        else {
            panic!("ribbon uvs must be float2");
        };
        assert_eq!(uvs, &[[0.5, 0.5], [1.0, 0.5], [0.5, 1.0], [1.0, 1.0]]);
        let bevy::mesh::VertexAttributeValues::Float32x4(colors) = mesh
            .attribute(Mesh::ATTRIBUTE_COLOR)
            .expect("ribbon colors")
        else {
            panic!("ribbon colors must be float4");
        };
        assert_eq!(colors[0], [0.4, 0.5, 0.6, 0.1]);
        assert_eq!(colors[2], [0.4, 0.5, 0.6, 0.4]);
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
