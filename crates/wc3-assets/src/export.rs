use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs::{self, File},
    io::{self, BufWriter},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Value, json};
use whiteout::{
    Bytes,
    casc::Storage as CascStorage,
    mdx::{
        InterpolationType, Layer, LayerFilterMode, LayerShaderType, LayerShadingFlag,
        LayerSlotType, Light, LightType, Model, Node, NodeFlag, Parser as MdxParser,
        ParticleEmitter2, SequenceFlag, TrackF32, TrackQuaternion, TrackU32, TrackVector3f,
    },
    mpq::Storage as MpqStorage,
    textures::{BlpParser, DdsParser, PixelFormat, PngParser, PngWriter, Texture, TgaParser},
};

use crate::catalog::{
    AttachedVisualSpec, BuildingAssetSpec, CATALOG_VERSION, DoodadAssetSpec, StatusVisualSpec,
    UiAssetCatalog, UnitAssetSpec, VisualAssetCatalog, VisualAssetSpec,
};

const GL_ARRAY_BUFFER: u32 = 34_962;
const GL_ELEMENT_ARRAY_BUFFER: u32 = 34_963;
const GL_FLOAT: u32 = 5_126;
const GL_UNSIGNED_SHORT: u32 = 5_123;
const NO_PARENT: u32 = u32::MAX;
const NO_GLOBAL_SEQUENCE: u32 = u32::MAX;
const ASSET_MANIFEST_SCHEMA_VERSION: u32 = 5;
const BUILDING_ASSET_MANIFEST_SCHEMA_VERSION: u32 = 5;
const DOODAD_MANIFEST_SCHEMA_VERSION: u32 = 2;
const UI_ASSET_MANIFEST_SCHEMA_VERSION: u32 = 2;
const MAX_MDX_INPUT_BYTES: usize = 64 * 1024 * 1024;
const WC3_PLAYER_COLOR_COUNT: u8 = 24;
const STOCK_BUILDING_ART_CATEGORIES: [&str; 7] = [
    "human", "orc", "undead", "nightelf", "naga", "other", "demon",
];

type TextureExport = (Vec<TextureManifest>, Vec<Option<usize>>);
type GltfBuildOutput = (Value, Vec<u8>, Vec<String>);
type MaterialBuildOutput = (Vec<Value>, Vec<String>, bool);

#[derive(Debug, Serialize)]
pub struct AssetManifest {
    pub schema_version: u32,
    pub castle_fight_catalog_version: &'static str,
    pub wc3_version: Option<String>,
    pub art_mode: &'static str,
    pub units: Vec<UnitManifest>,
    pub models: Vec<ModelManifest>,
    pub failures: Vec<FailureManifest>,
}

#[derive(Debug, Serialize)]
pub struct UnitManifest {
    pub rawcode: String,
    pub name: String,
    pub scale: f32,
    pub requested_model: Option<String>,
    pub source_model: String,
    pub fallback_to_base_art: bool,
    pub intentionally_hidden: bool,
    pub gltf: Option<String>,
    pub tint_rgb: Option<[u8; 3]>,
    pub attached_visuals: Vec<AttachedVisualSpec>,
}

#[derive(Debug, Serialize)]
pub struct BuildingAssetManifest {
    pub schema_version: u32,
    pub castle_fight_catalog_version: &'static str,
    pub wc3_version: Option<String>,
    pub art_mode: &'static str,
    pub buildings: Vec<BuildingManifest>,
    pub models: Vec<ModelManifest>,
    pub failures: Vec<BuildingFailureManifest>,
}

#[derive(Debug, Serialize)]
pub struct BuildingManifest {
    pub rawcode: String,
    pub name: String,
    pub scale: f32,
    pub animation_properties: Vec<String>,
    pub lifecycle_animations: BuildingLifecycleAnimationManifest,
    pub requested_model: Option<String>,
    pub source_model: String,
    pub fallback_to_base_art: bool,
    pub gltf: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct BuildingLifecycleAnimationManifest {
    pub birth: Option<String>,
    pub stand: Option<String>,
    pub death: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BuildingFailureManifest {
    pub source_model: String,
    pub buildings: Vec<String>,
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct DoodadAssetManifest {
    pub schema_version: u32,
    pub castle_fight_catalog_version: &'static str,
    pub wc3_version: Option<String>,
    pub art_mode: &'static str,
    pub objects: Vec<DoodadObjectManifest>,
    pub models: Vec<ModelManifest>,
    pub failures: Vec<DoodadFailureManifest>,
}

#[derive(Debug, Serialize)]
pub struct DoodadObjectManifest {
    pub rawcode: String,
    pub object_kind: String,
    pub name: String,
    pub placements: Vec<DoodadPlacementManifest>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoodadPlacementManifest {
    pub editor_id: u32,
    pub position: [f32; 3],
    pub angle_degrees: f32,
    pub scale: [f32; 3],
    pub visible: bool,
    pub solid: bool,
    pub fixed_z: bool,
    pub variation: u32,
    pub requested_model: Option<String>,
    pub source_model: Option<String>,
    pub fallback_to_base_art: bool,
    pub gltf: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DoodadFailureManifest {
    pub rawcode: String,
    pub variation: u32,
    pub source_model: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelManifest {
    pub source_model: String,
    pub source_casc_path: String,
    pub gltf: String,
    pub bin: String,
    pub geosets: usize,
    pub bones: usize,
    pub features: ModelFeatureManifest,
    pub overhead_position: Option<[f32; 3]>,
    pub animations: Vec<AnimationManifest>,
    pub textures: Vec<TextureManifest>,
    pub materials: Vec<MaterialManifest>,
    pub geoset_animations: Vec<GeosetAnimationManifest>,
    pub particle_emitters: Vec<ParticleEmitter2Manifest>,
    pub model_particle_emitters: Vec<ModelParticleEmitterManifest>,
    pub ribbon_emitters: Vec<RibbonEmitterManifest>,
    pub attachments: Vec<AttachmentManifest>,
    pub event_objects: Vec<EventObjectManifest>,
    pub lights: Vec<LightManifest>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct ModelFeatureManifest {
    pub material_count: usize,
    pub material_layer_count: usize,
    pub multilayer_material_count: usize,
    pub animated_material_alpha_layer_count: usize,
    pub animated_material_texture_layer_count: usize,
    pub animated_geoset_alpha_count: usize,
    pub global_sequence_count: usize,
    pub attachment_count: usize,
    pub attachment_models: Vec<String>,
    pub particle_emitter_count: usize,
    pub particle_emitter_animated_track_count: usize,
    pub particle_emitter_2_count: usize,
    pub particle_emitter_2_animated_track_count: usize,
    pub ribbon_emitter_count: usize,
    pub ribbon_emitter_animated_track_count: usize,
    pub corn_emitter_count: usize,
    pub corn_emitter_animated_track_count: usize,
    pub event_object_count: usize,
    pub light_count: usize,
    pub omni_light_count: usize,
    pub non_omni_light_count: usize,
    pub non_inheritance_node_count: usize,
    pub max_classic_skin_influences: u32,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScalarTrackInterpolationManifest {
    DontInterp,
    Linear,
    Hermite,
    Bezier,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ScalarTrackManifest {
    pub interpolation: ScalarTrackInterpolationManifest,
    pub global_sequence_id: Option<u32>,
    pub timestamps: Vec<u32>,
    pub values: Vec<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub in_tangents: Vec<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub out_tangents: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Vector3TrackManifest {
    pub interpolation: ScalarTrackInterpolationManifest,
    pub global_sequence_id: Option<u32>,
    pub timestamps: Vec<u32>,
    pub values: Vec<[f32; 3]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub in_tangents: Vec<[f32; 3]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub out_tangents: Vec<[f32; 3]>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UnsignedTrackManifest {
    pub interpolation: ScalarTrackInterpolationManifest,
    pub global_sequence_id: Option<u32>,
    pub timestamps: Vec<u32>,
    pub values: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub in_tangents: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub out_tangents: Vec<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeosetAnimationManifest {
    pub geoset_id: u32,
    pub flags: String,
    pub alpha: f32,
    pub color: [f32; 3],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_track: Option<Vector3TrackManifest>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaterialManifest {
    pub material_id: usize,
    pub priority_plane: i32,
    pub layers: Vec<MaterialLayerManifest>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaterialLayerManifest {
    pub layer_index: usize,
    pub shader: String,
    pub filter_mode: String,
    pub texture_id: u32,
    pub alpha: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture_id_track: Option<UnsignedTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_track: Option<ScalarTrackManifest>,
    pub sub_textures: Vec<MaterialSubTextureManifest>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaterialSubTextureManifest {
    pub slot: String,
    pub texture_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture_id_track: Option<UnsignedTrackManifest>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ParticleEmitterSequenceManifest {
    pub name: String,
    pub start_ms: u32,
    pub end_ms: u32,
    pub non_looping: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParticleEmitter2Manifest {
    pub object_id: u32,
    pub name: String,
    pub position: [f32; 3],
    pub speed: f32,
    pub variation: f32,
    pub latitude: f32,
    pub gravity: f32,
    pub lifespan: f32,
    pub emission_rate: f32,
    pub length: f32,
    pub width: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variation_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gravity_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emission_rate_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility_track: Option<ScalarTrackManifest>,
    pub filter_mode: u32,
    pub rows: u32,
    pub columns: u32,
    pub head_or_tail: u32,
    pub tail_length: f32,
    pub time: f32,
    pub head_interval: [u32; 3],
    pub head_decay_interval: [u32; 3],
    pub tail_interval: [u32; 3],
    pub tail_decay_interval: [u32; 3],
    pub priority_plane: i32,
    pub segment_colors: [[f32; 3]; 3],
    pub segment_alpha: [u8; 3],
    pub segment_scaling: [f32; 3],
    pub texture: Option<String>,
    pub squirt: bool,
    pub replaceable_id: u32,
    pub sequence_windows: Vec<ParticleEmitterSequenceManifest>,
    pub global_sequence_durations_ms: Vec<u32>,
    /// Whether this emitter is active during the model's ambient Stand sequence. Doodads use this
    /// to avoid replaying death/decay-only debris emitters while still rendering fires/torches.
    pub ambient_enabled: bool,
    /// Exact exported animation sequence names during which this emitter can become visible and
    /// emit. Building presentation uses this to switch birth/stand/death particle sets alongside
    /// the selected lifecycle clip without hardcoding model-specific emitter indices.
    pub active_sequences: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelParticleEmitterManifest {
    pub object_id: u32,
    pub name: String,
    pub position: [f32; 3],
    pub emission_rate: f32,
    pub gravity: f32,
    pub longitude: f32,
    pub latitude: f32,
    pub lifespan: f32,
    pub initial_velocity: f32,
    pub spawn_model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gltf: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emission_rate_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gravity_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifespan_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility_track: Option<ScalarTrackManifest>,
    pub sequence_windows: Vec<ParticleEmitterSequenceManifest>,
    pub global_sequence_durations_ms: Vec<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttachmentManifest {
    pub object_id: u32,
    pub name: String,
    pub position: [f32; 3],
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gltf: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility_track: Option<ScalarTrackManifest>,
    pub sequence_windows: Vec<ParticleEmitterSequenceManifest>,
    pub global_sequence_durations_ms: Vec<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventObjectKindManifest {
    Sound,
    Splat,
    Footprint,
    Spawn,
    UberSplat,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventSoundManifest {
    pub sound_name: String,
    pub source_files: Vec<String>,
    pub files: Vec<String>,
    pub silent: bool,
    pub volume: f32,
    pub volume_variance: f32,
    pub pitch: f32,
    pub pitch_variance: f32,
    pub maximum_concurrent_instances: i32,
    pub priority: i32,
    pub channel: i32,
    pub flags: String,
    pub min_distance: f32,
    pub max_distance: f32,
    pub distance_cutoff: f32,
    pub eax_flags: String,
    pub rolloff_points: String,
}

#[derive(Debug, Clone)]
struct AnimationSoundSpec {
    sound_name: String,
    source_files: Vec<String>,
    silent: bool,
    volume: f32,
    volume_variance: f32,
    pitch: f32,
    pitch_variance: f32,
    maximum_concurrent_instances: i32,
    priority: i32,
    channel: i32,
    flags: String,
    min_distance: f32,
    max_distance: f32,
    distance_cutoff: f32,
    eax_flags: String,
    rolloff_points: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventSplatManifest {
    pub texture: String,
    pub source_texture: String,
    pub rows: u32,
    pub columns: u32,
    pub blend_mode: u32,
    pub scale: f32,
    pub lifespan: f32,
    pub decay: f32,
    pub birth_time: f32,
    pub pause_time: f32,
    pub uv_lifespan: [u32; 2],
    pub lifespan_repeat: u32,
    pub uv_decay: [u32; 2],
    pub decay_repeat: u32,
    pub colors: [[u8; 4]; 3],
}

#[derive(Debug, Clone)]
struct SplatServiceSpec {
    source_texture: String,
    rows: u32,
    columns: u32,
    blend_mode: u32,
    scale: f32,
    lifespan: f32,
    decay: f32,
    birth_time: f32,
    pause_time: f32,
    uv_lifespan: [u32; 2],
    lifespan_repeat: u32,
    uv_decay: [u32; 2],
    decay_repeat: u32,
    colors: [[u8; 4]; 3],
}

#[derive(Debug, Clone, Serialize)]
pub struct EventObjectManifest {
    pub object_id: u32,
    pub name: String,
    pub position: [f32; 3],
    pub kind: EventObjectKindManifest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_code: Option<String>,
    pub lookup_resolved: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spawn_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gltf: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sound: Option<EventSoundManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub splat: Option<EventSplatManifest>,
    pub global_sequence_id: Option<u32>,
    pub event_track_times: Vec<u32>,
    pub sequence_windows: Vec<ParticleEmitterSequenceManifest>,
    pub global_sequence_durations_ms: Vec<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LightManifest {
    pub object_id: u32,
    pub name: String,
    pub position: [f32; 3],
    pub light_type: String,
    pub attenuation_start: f32,
    pub attenuation_end: f32,
    pub color: [f32; 3],
    pub intensity: f32,
    pub ambient_color: [f32; 3],
    pub ambient_intensity: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attenuation_start_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attenuation_end_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_track: Option<Vector3TrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intensity_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ambient_color_track: Option<Vector3TrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ambient_intensity_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility_track: Option<ScalarTrackManifest>,
    pub sequence_windows: Vec<ParticleEmitterSequenceManifest>,
    pub global_sequence_durations_ms: Vec<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RibbonEmitterManifest {
    pub object_id: u32,
    pub name: String,
    pub position: [f32; 3],
    pub height_above: f32,
    pub height_below: f32,
    pub alpha: f32,
    pub color: [f32; 3],
    pub lifespan: f32,
    pub emission_rate: u32,
    pub rows: u32,
    pub columns: u32,
    pub material_id: u32,
    pub texture_slot: u32,
    pub filter_mode: String,
    pub texture: Option<String>,
    pub gravity: f32,
    pub sequence_windows: Vec<ParticleEmitterSequenceManifest>,
    pub global_sequence_durations_ms: Vec<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height_above_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height_below_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_track: Option<ScalarTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_track: Option<Vector3TrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture_slot_track: Option<UnsignedTrackManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility_track: Option<ScalarTrackManifest>,
}

#[derive(Debug, Serialize)]
pub struct VisualAssetManifest {
    pub schema_version: u32,
    pub castle_fight_catalog_version: &'static str,
    pub wc3_version: Option<String>,
    pub art_mode: &'static str,
    pub assets: Vec<VisualBindingManifest>,
    pub status_visuals: Vec<StatusVisualBindingManifest>,
    pub chain_lightning_abilities: Vec<String>,
    pub stun: Option<VisualBindingManifest>,
    pub models: Vec<ModelManifest>,
    pub failures: Vec<VisualFailureManifest>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusVisualBindingManifest {
    pub ability_rawcode: String,
    pub status_kind: String,
    pub source_model: String,
    pub gltf: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VisualBindingManifest {
    pub owner_kind: String,
    pub owner_rawcode: String,
    pub role: String,
    pub source_model: String,
    pub gltf: Option<String>,
    pub missile_arc: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VisualFailureManifest {
    pub source_model: String,
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct UiAssetManifest {
    pub schema_version: u32,
    pub castle_fight_catalog_version: &'static str,
    pub wc3_version: Option<String>,
    pub art_mode: &'static str,
    pub assets: Vec<UiBindingManifest>,
    pub textures: Vec<TextureManifest>,
    pub failures: Vec<UiFailureManifest>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UiBindingManifest {
    pub owner_kind: String,
    pub owner_rawcode: String,
    pub role: String,
    pub source_texture: String,
    pub png: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UiFailureManifest {
    pub source_texture: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnimationManifest {
    pub name: String,
    pub start_ms: u32,
    pub end_ms: u32,
    pub move_speed: f32,
    pub non_looping: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TextureManifest {
    pub source_texture: String,
    pub source_casc_path: Option<String>,
    pub png: Option<String>,
    pub replaceable_id: u32,
    pub has_transparency: bool,
}

#[derive(Debug, Serialize)]
pub struct FailureManifest {
    pub source_model: String,
    pub units: Vec<String>,
    pub error: String,
}

pub struct Exporter {
    storage: CascStorage,
    map_storage: Option<MpqStorage>,
    output: PathBuf,
    keep_source: bool,
    wc3_version: Option<String>,
    unit_skin: UnitSkinCatalog,
    doodad_skin: DoodadSkinCatalog,
    destructable_skin: DoodadSkinCatalog,
    spawn_event_models: BTreeMap<String, String>,
    animation_sounds: BTreeMap<String, AnimationSoundSpec>,
    splat_services: BTreeMap<String, SplatServiceSpec>,
    uber_splat_services: BTreeMap<String, SplatServiceSpec>,
    sound_cache: BTreeMap<String, Option<String>>,
    sound_file_index: Option<BTreeMap<String, Vec<String>>>,
    texture_cache: BTreeMap<String, TextureManifest>,
}

#[derive(Debug)]
struct ResolvedUnit {
    rawcode: String,
    name: String,
    requested_model: Option<String>,
    source_model: String,
    fallback_to_base_art: bool,
    intentionally_hidden: bool,
    scale: f32,
    tint_rgb: Option<[u8; 3]>,
    attached_visuals: Vec<AttachedVisualSpec>,
}

#[derive(Debug)]
struct ResolvedBuilding {
    rawcode: String,
    name: String,
    requested_model: Option<String>,
    source_model: String,
    fallback_to_base_art: bool,
    scale: f32,
    animation_properties: Vec<String>,
}

#[derive(Debug, Clone)]
struct ResolvedDoodadVariant {
    requested_model: Option<String>,
    source_model: Option<String>,
    fallback_to_base_art: bool,
    replaceable_textures: BTreeMap<u32, String>,
}

#[derive(Debug, Default)]
struct UnitSkinProfile {
    file: Option<String>,
    file_sd: Option<String>,
    model_scale: Option<f32>,
    model_scale_sd: Option<f32>,
}

type UnitSkinCatalog = BTreeMap<String, UnitSkinProfile>;

#[derive(Debug, Default)]
struct DoodadSkinProfile {
    file: Option<String>,
    file_sd: Option<String>,
    num_variations: Option<u32>,
    replaceable_texture_id: Option<u32>,
    replaceable_texture: Option<String>,
}

type DoodadSkinCatalog = BTreeMap<String, DoodadSkinProfile>;

impl Exporter {
    pub fn open(
        wc3_install: &Path,
        map_archive: Option<&Path>,
        output: &Path,
        keep_source: bool,
    ) -> Result<Self, Box<dyn Error>> {
        let mut storage =
            CascStorage::open(&wc3_install.to_string_lossy(), None).ok_or_else(|| {
                io::Error::other(format!(
                    "failed to open Warcraft III CASC storage at {}",
                    wc3_install.display()
                ))
            })?;
        let map_storage = map_archive
            .map(|path| {
                MpqStorage::open(&path.to_string_lossy(), None).ok_or_else(|| {
                    io::Error::other(format!(
                        "failed to open Warcraft III map archive at {}",
                        path.display()
                    ))
                })
            })
            .transpose()?;
        let wc3_version = read_wc3_version(wc3_install);
        let unit_skin = {
            let bytes = storage
                .read_file(r"war3.w3mod:units\unitskin.txt")
                .ok_or_else(|| io::Error::other("failed to read war3.w3mod:units\\unitskin.txt"))?;
            let parsed = parse_unit_skin(&String::from_utf8_lossy(&bytes));
            drop(bytes);
            storage.flush_cache();
            parsed
        };
        let doodad_skin = {
            let bytes = storage
                .read_file(r"war3.w3mod:doodads\doodadskins.txt")
                .ok_or_else(|| {
                    io::Error::other("failed to read war3.w3mod:doodads\\doodadskins.txt")
                })?;
            let parsed = parse_doodad_skin(&String::from_utf8_lossy(&bytes));
            drop(bytes);
            storage.flush_cache();
            parsed
        };
        let destructable_skin = {
            let bytes = storage
                .read_file(r"war3.w3mod:units\destructableskin.txt")
                .ok_or_else(|| {
                    io::Error::other("failed to read war3.w3mod:units\\destructableskin.txt")
                })?;
            let parsed = parse_doodad_skin(&String::from_utf8_lossy(&bytes));
            drop(bytes);
            storage.flush_cache();
            parsed
        };
        let spawn_event_models = read_spawn_event_catalog(&mut storage, map_storage.as_ref())?;
        let animation_sounds = read_animation_sound_catalog(&mut storage, map_storage.as_ref())?;
        let splat_services = read_splat_service_catalog(&mut storage, map_storage.as_ref(), false)?;
        let uber_splat_services =
            read_splat_service_catalog(&mut storage, map_storage.as_ref(), true)?;
        // Whiteout's CASC reader keeps decoded data containers in an internal cache.
        // Asset extraction is a streaming workload and does not benefit enough from retaining
        // those potentially-large containers to justify letting the cache grow across models.

        fs::create_dir_all(output.join("models"))?;
        fs::create_dir_all(output.join("textures"))?;
        if keep_source {
            fs::create_dir_all(output.join("source/models"))?;
            fs::create_dir_all(output.join("source/textures"))?;
        }
        Ok(Self {
            storage,
            map_storage,
            output: output.to_path_buf(),
            keep_source,
            wc3_version,
            unit_skin,
            doodad_skin,
            destructable_skin,
            spawn_event_models,
            animation_sounds,
            splat_services,
            uber_splat_services,
            sound_cache: BTreeMap::new(),
            sound_file_index: None,
            texture_cache: BTreeMap::new(),
        })
    }

    pub fn switch_output(&mut self, output: &Path) -> Result<(), Box<dyn Error>> {
        fs::create_dir_all(output.join("models"))?;
        fs::create_dir_all(output.join("textures"))?;
        if self.keep_source {
            fs::create_dir_all(output.join("source/models"))?;
            fs::create_dir_all(output.join("source/textures"))?;
        }
        self.output = output.to_path_buf();
        self.sound_cache.clear();
        self.texture_cache.clear();
        Ok(())
    }

    pub fn export_units(
        &mut self,
        units: &[UnitAssetSpec],
    ) -> Result<AssetManifest, Box<dyn Error>> {
        let resolved: Vec<_> = units
            .iter()
            .map(|unit| self.resolve_unit(unit))
            .collect::<Result<_, _>>()?;
        let mut grouped = BTreeMap::<String, Vec<&ResolvedUnit>>::new();
        for unit in &resolved {
            if unit.intentionally_hidden {
                continue;
            }
            grouped
                .entry(unit.source_model.to_ascii_lowercase())
                .or_default()
                .push(unit);
        }

        let mut models = Vec::new();
        let mut emitted_models = BTreeSet::new();
        let mut failures = Vec::new();
        let mut model_outputs = BTreeMap::<String, String>::new();
        for (key, group) in &grouped {
            let source = group[0].source_model.clone();
            match self.export_model_closure(&source, false) {
                Ok(model_tree) => {
                    let root_gltf = model_tree
                        .first()
                        .expect("model closure always contains its root")
                        .gltf
                        .clone();
                    model_outputs.insert(key.clone(), root_gltf);
                    for model in model_tree {
                        if emitted_models.insert(model.gltf.to_ascii_lowercase()) {
                            models.push(model);
                        }
                    }
                }
                Err(error) => failures.push(FailureManifest {
                    source_model: source,
                    units: group.iter().map(|unit| unit.rawcode.clone()).collect(),
                    error: error.to_string(),
                }),
            }
        }

        let units = resolved
            .iter()
            .map(|unit| UnitManifest {
                rawcode: unit.rawcode.clone(),
                name: unit.name.clone(),
                scale: unit.scale,
                tint_rgb: unit.tint_rgb,
                attached_visuals: unit.attached_visuals.clone(),
                requested_model: unit.requested_model.clone(),
                gltf: model_outputs
                    .get(&unit.source_model.to_ascii_lowercase())
                    .cloned(),
                source_model: unit.source_model.clone(),
                fallback_to_base_art: unit.fallback_to_base_art,
                intentionally_hidden: unit.intentionally_hidden,
            })
            .collect();

        Ok(AssetManifest {
            schema_version: ASSET_MANIFEST_SCHEMA_VERSION,
            castle_fight_catalog_version: CATALOG_VERSION,
            wc3_version: self.wc3_version.clone(),
            art_mode: "sd",
            units,
            models,
            failures,
        })
    }

    pub fn export_buildings(
        &mut self,
        buildings: &[BuildingAssetSpec],
    ) -> Result<BuildingAssetManifest, Box<dyn Error>> {
        let resolved: Vec<_> = buildings
            .iter()
            .map(|building| self.resolve_building(building))
            .collect::<Result<_, _>>()?;
        let mut grouped = BTreeMap::<String, Vec<&ResolvedBuilding>>::new();
        for building in &resolved {
            grouped
                .entry(building.source_model.to_ascii_lowercase())
                .or_default()
                .push(building);
        }

        let mut models = Vec::new();
        let mut emitted_models = BTreeSet::new();
        let mut failures = Vec::new();
        let mut model_outputs = BTreeMap::<String, (String, Vec<AnimationManifest>)>::new();
        for (key, group) in &grouped {
            let source = group[0].source_model.clone();
            match self.export_model_closure(&source, true) {
                Ok(model_tree) => {
                    let root = model_tree
                        .first()
                        .expect("model closure always contains its root");
                    model_outputs.insert(key.clone(), (root.gltf.clone(), root.animations.clone()));
                    for model in model_tree {
                        if emitted_models.insert(model.gltf.to_ascii_lowercase()) {
                            models.push(model);
                        }
                    }
                }
                Err(error) => failures.push(BuildingFailureManifest {
                    source_model: source,
                    buildings: group
                        .iter()
                        .map(|building| building.rawcode.clone())
                        .collect(),
                    error: error.to_string(),
                }),
            }
        }

        let buildings = resolved
            .iter()
            .map(|building| {
                let model_output = model_outputs.get(&building.source_model.to_ascii_lowercase());
                let lifecycle_animations = model_output.map_or_else(
                    BuildingLifecycleAnimationManifest::default,
                    |(_, animations)| {
                        select_building_lifecycle_animations(
                            animations,
                            &building.animation_properties,
                        )
                    },
                );
                BuildingManifest {
                    rawcode: building.rawcode.clone(),
                    name: building.name.clone(),
                    scale: building.scale,
                    animation_properties: building.animation_properties.clone(),
                    lifecycle_animations,
                    requested_model: building.requested_model.clone(),
                    gltf: model_output.map(|(gltf, _)| gltf.clone()),
                    source_model: building.source_model.clone(),
                    fallback_to_base_art: building.fallback_to_base_art,
                }
            })
            .collect();

        Ok(BuildingAssetManifest {
            schema_version: BUILDING_ASSET_MANIFEST_SCHEMA_VERSION,
            castle_fight_catalog_version: CATALOG_VERSION,
            wc3_version: self.wc3_version.clone(),
            art_mode: "sd",
            buildings,
            models,
            failures,
        })
    }

    pub fn export_visuals(
        &mut self,
        catalog: &VisualAssetCatalog,
    ) -> Result<VisualAssetManifest, Box<dyn Error>> {
        let normalized_assets: Vec<_> = catalog
            .assets
            .iter()
            .map(|asset| VisualAssetSpec {
                owner_kind: asset.owner_kind.clone(),
                owner_rawcode: asset.owner_rawcode.clone(),
                role: asset.role.clone(),
                model_path: normalize_model_path(&asset.model_path),
                missile_arc: asset.missile_arc,
            })
            .collect();
        let normalized_status_visuals: Vec<_> = catalog
            .status_visuals
            .iter()
            .map(|visual| StatusVisualSpec {
                ability_rawcode: visual.ability_rawcode.clone(),
                status_kind: visual.status_kind.clone(),
                model_path: normalize_model_path(&visual.model_path),
            })
            .collect();
        let stun_source = catalog.stun_model_path.as_deref().map(normalize_model_path);

        let mut sources = BTreeSet::new();
        for asset in &normalized_assets {
            sources.insert(asset.model_path.clone());
        }
        for visual in &normalized_status_visuals {
            sources.insert(visual.model_path.clone());
        }
        if let Some(source) = &stun_source {
            sources.insert(source.clone());
        }

        let mut models = Vec::new();
        let mut emitted_models = BTreeSet::new();
        let mut failures = Vec::new();
        let mut model_outputs = BTreeMap::<String, String>::new();
        for source in sources {
            if !self.model_exists(&source) {
                failures.push(VisualFailureManifest {
                    source_model: source,
                    error: "model is not present in the map archive or Warcraft III CASC install"
                        .to_owned(),
                });
                continue;
            }
            match self.export_model_closure(&source, false) {
                Ok(model_tree) => {
                    let root_gltf = model_tree
                        .first()
                        .expect("model closure always contains its root")
                        .gltf
                        .clone();
                    model_outputs.insert(source.to_ascii_lowercase(), root_gltf);
                    for model in model_tree {
                        if emitted_models.insert(model.gltf.to_ascii_lowercase()) {
                            models.push(model);
                        }
                    }
                }
                Err(error) => failures.push(VisualFailureManifest {
                    source_model: source,
                    error: error.to_string(),
                }),
            }
        }

        let assets = normalized_assets
            .into_iter()
            .map(|asset| VisualBindingManifest {
                owner_kind: asset.owner_kind,
                owner_rawcode: asset.owner_rawcode,
                role: asset.role,
                gltf: model_outputs
                    .get(&asset.model_path.to_ascii_lowercase())
                    .cloned(),
                source_model: asset.model_path,
                missile_arc: asset.missile_arc,
            })
            .collect();
        let status_visuals = normalized_status_visuals
            .into_iter()
            .map(|visual| StatusVisualBindingManifest {
                ability_rawcode: visual.ability_rawcode,
                status_kind: visual.status_kind,
                gltf: model_outputs
                    .get(&visual.model_path.to_ascii_lowercase())
                    .cloned(),
                source_model: visual.model_path,
            })
            .collect();
        let stun = stun_source.map(|source_model| VisualBindingManifest {
            owner_kind: "status".to_owned(),
            owner_rawcode: "stun".to_owned(),
            role: "target".to_owned(),
            gltf: model_outputs
                .get(&source_model.to_ascii_lowercase())
                .cloned(),
            source_model,
            missile_arc: None,
        });

        Ok(VisualAssetManifest {
            schema_version: 4,
            castle_fight_catalog_version: CATALOG_VERSION,
            wc3_version: self.wc3_version.clone(),
            art_mode: "sd",
            assets,
            status_visuals,
            chain_lightning_abilities: catalog.chain_lightning_abilities.clone(),
            stun,
            models,
            failures,
        })
    }

    pub fn export_ui(
        &mut self,
        catalog: &UiAssetCatalog,
    ) -> Result<UiAssetManifest, Box<dyn Error>> {
        let normalized_assets: Vec<_> = catalog
            .assets
            .iter()
            .map(|asset| {
                (
                    asset.owner_kind.clone(),
                    asset.owner_rawcode.clone(),
                    asset.role.clone(),
                    normalize_texture_path(&asset.texture_path),
                )
            })
            .collect();

        let mut sources = BTreeMap::<String, String>::new();
        for (_, _, _, source) in &normalized_assets {
            sources
                .entry(source.to_ascii_lowercase())
                .or_insert_with(|| source.clone());
        }

        let mut textures = Vec::new();
        let mut failures = Vec::new();
        let mut outputs = BTreeMap::<String, TextureManifest>::new();
        for (key, source) in sources {
            match self.export_texture(&source) {
                Ok(texture) => {
                    outputs.insert(key, texture.clone());
                    textures.push(texture);
                }
                Err(error) => failures.push(UiFailureManifest {
                    source_texture: source,
                    error: error.to_string(),
                }),
            }
        }

        let assets = normalized_assets
            .into_iter()
            .map(
                |(owner_kind, owner_rawcode, role, source_texture)| UiBindingManifest {
                    owner_kind,
                    owner_rawcode,
                    role,
                    png: outputs
                        .get(&source_texture.to_ascii_lowercase())
                        .and_then(|texture| texture.png.clone()),
                    source_texture,
                },
            )
            .collect();

        Ok(UiAssetManifest {
            schema_version: UI_ASSET_MANIFEST_SCHEMA_VERSION,
            castle_fight_catalog_version: CATALOG_VERSION,
            wc3_version: self.wc3_version.clone(),
            art_mode: "sd",
            assets,
            textures,
            failures,
        })
    }

    pub fn export_doodads(
        &mut self,
        doodads: &[DoodadAssetSpec],
    ) -> Result<DoodadAssetManifest, Box<dyn Error>> {
        let mut variants = BTreeMap::<(String, u32), ResolvedDoodadVariant>::new();
        let mut failures = Vec::new();
        for doodad in doodads {
            let variations: BTreeSet<_> = doodad
                .placements
                .iter()
                .filter(|placement| placement.visible)
                .map(|placement| placement.variation)
                .collect();
            for variation in variations {
                match self.resolve_doodad_variant(doodad, variation) {
                    Ok(resolved) => {
                        variants.insert((doodad.rawcode.clone(), variation), resolved);
                    }
                    Err(error) => failures.push(DoodadFailureManifest {
                        rawcode: doodad.rawcode.clone(),
                        variation,
                        source_model: doodad
                            .model_path
                            .clone()
                            .unwrap_or_else(|| format!("base:{}", doodad.base_rawcode)),
                        error: error.to_string(),
                    }),
                }
            }
        }

        let mut models = Vec::new();
        let mut emitted_models = BTreeSet::new();
        let mut model_outputs = BTreeMap::<String, String>::new();
        let mut model_errors = BTreeMap::<String, String>::new();
        for ((rawcode, variation), resolved) in &variants {
            let Some(source_model) = resolved.source_model.as_deref() else {
                continue;
            };
            let key = doodad_model_key(source_model, &resolved.replaceable_textures);
            if model_outputs.contains_key(&key) || model_errors.contains_key(&key) {
                continue;
            }
            match self.export_model_closure_with_replacements(
                source_model,
                &resolved.replaceable_textures,
                false,
            ) {
                Ok(model_tree) => {
                    let root_gltf = model_tree
                        .first()
                        .expect("model closure always contains its root")
                        .gltf
                        .clone();
                    model_outputs.insert(key, root_gltf);
                    for model in model_tree {
                        if emitted_models.insert(model.gltf.to_ascii_lowercase()) {
                            models.push(model);
                        }
                    }
                }
                Err(error) => {
                    let error = error.to_string();
                    model_errors.insert(key, error.clone());
                    failures.push(DoodadFailureManifest {
                        rawcode: rawcode.clone(),
                        variation: *variation,
                        source_model: source_model.to_owned(),
                        error,
                    });
                }
            }
        }

        let objects = doodads
            .iter()
            .map(|doodad| {
                let placements = doodad
                    .placements
                    .iter()
                    .map(|placement| {
                        let resolved = variants.get(&(doodad.rawcode.clone(), placement.variation));
                        let gltf = resolved
                            .and_then(|resolved| {
                                resolved
                                    .source_model
                                    .as_deref()
                                    .map(|source| (resolved, source))
                            })
                            .and_then(|(resolved, source)| {
                                model_outputs
                                    .get(&doodad_model_key(source, &resolved.replaceable_textures))
                                    .cloned()
                            });
                        DoodadPlacementManifest {
                            editor_id: placement.editor_id,
                            position: placement.position,
                            angle_degrees: placement.angle_degrees,
                            scale: placement.scale,
                            visible: placement.visible,
                            solid: placement.solid,
                            fixed_z: placement.fixed_z,
                            variation: placement.variation,
                            requested_model: resolved
                                .and_then(|resolved| resolved.requested_model.clone()),
                            source_model: resolved
                                .and_then(|resolved| resolved.source_model.clone()),
                            fallback_to_base_art: resolved
                                .is_some_and(|resolved| resolved.fallback_to_base_art),
                            gltf,
                        }
                    })
                    .collect();
                DoodadObjectManifest {
                    rawcode: doodad.rawcode.clone(),
                    object_kind: doodad.object_kind.clone(),
                    name: doodad.name.clone(),
                    placements,
                }
            })
            .collect();

        Ok(DoodadAssetManifest {
            schema_version: DOODAD_MANIFEST_SCHEMA_VERSION,
            castle_fight_catalog_version: CATALOG_VERSION,
            wc3_version: self.wc3_version.clone(),
            art_mode: "sd",
            objects,
            models,
            failures,
        })
    }

    fn resolve_doodad_variant(
        &self,
        doodad: &DoodadAssetSpec,
        variation: u32,
    ) -> Result<ResolvedDoodadVariant, Box<dyn Error>> {
        let profiles = if doodad.object_kind == "destructable" {
            &self.destructable_skin
        } else {
            &self.doodad_skin
        };
        let profile = profiles.get(&doodad.base_rawcode.to_ascii_lowercase());
        let profile_model =
            profile.and_then(|profile| profile.file_sd.as_deref().or(profile.file.as_deref()));
        let num_variations = doodad
            .num_variations
            .or_else(|| profile.and_then(|profile| profile.num_variations))
            .unwrap_or(1)
            .max(1);

        if doodad.model_path.is_none()
            && profile_model.is_some_and(|path| path.to_ascii_lowercase().contains("losblocker"))
        {
            return Ok(ResolvedDoodadVariant {
                requested_model: None,
                source_model: None,
                fallback_to_base_art: false,
                replaceable_textures: BTreeMap::new(),
            });
        }

        let requested_model = doodad
            .model_path
            .as_deref()
            .and_then(|path| self.find_doodad_model_variant(path, variation, num_variations));
        let requested_logical = doodad
            .model_path
            .as_deref()
            .map(|path| preferred_doodad_model_path(path, variation, num_variations));
        let base_model = profile_model
            .and_then(|path| self.find_doodad_model_variant(path, variation, num_variations));
        let (source_model, fallback_to_base_art) = if let Some(requested) = requested_model {
            (requested, false)
        } else if let Some(base) = base_model {
            (base, doodad.model_path.is_some())
        } else {
            let requested = requested_logical.as_deref().unwrap_or("<none>");
            let base = profile_model.unwrap_or("<none>");
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "{} {} ({}) variation {} has no install-resident model: requested {}, base {} -> {}",
                    doodad.object_kind,
                    doodad.rawcode,
                    doodad.name,
                    variation,
                    requested,
                    doodad.base_rawcode,
                    base
                ),
            )
            .into());
        };

        let mut replaceable_textures = BTreeMap::new();
        if let Some(profile) = profile
            && let (Some(id), Some(texture)) = (
                profile.replaceable_texture_id,
                profile.replaceable_texture.as_deref(),
            )
            && id != 0
        {
            replaceable_textures.insert(id, texture.to_owned());
        }

        Ok(ResolvedDoodadVariant {
            requested_model: requested_logical,
            source_model: Some(source_model),
            fallback_to_base_art,
            replaceable_textures,
        })
    }

    fn find_doodad_model_variant(
        &self,
        model_path: &str,
        variation: u32,
        num_variations: u32,
    ) -> Option<String> {
        doodad_model_candidates(model_path, variation, num_variations)
            .into_iter()
            .find(|path| self.model_exists(path))
    }

    fn resolve_unit(&self, unit: &UnitAssetSpec) -> Result<ResolvedUnit, Box<dyn Error>> {
        let profile = self.unit_skin.get(&unit.base_rawcode.to_ascii_lowercase());
        let requested_model = unit.model_path.as_deref().map(normalize_model_path);
        let scale = unit
            .scale
            .or_else(|| profile.and_then(|profile| profile.model_scale_sd))
            .or_else(|| profile.and_then(|profile| profile.model_scale))
            .unwrap_or(1.0);
        if requested_model
            .as_deref()
            .is_some_and(is_intentionally_hidden_model_path)
        {
            return Ok(ResolvedUnit {
                rawcode: unit.rawcode.clone(),
                name: unit.name.clone(),
                source_model: requested_model
                    .clone()
                    .expect("intentional hidden model path was checked above"),
                requested_model,
                fallback_to_base_art: false,
                intentionally_hidden: true,
                scale,
                tint_rgb: unit.tint_rgb,
                attached_visuals: unit.attached_visuals.clone(),
            });
        }
        let base_model = profile
            .and_then(|profile| profile.file_sd.as_deref().or(profile.file.as_deref()))
            .map(normalize_model_path);
        let requested_available = requested_model
            .as_deref()
            .is_some_and(|path| self.model_exists(path));
        let base_available = base_model
            .as_deref()
            .is_some_and(|path| self.model_exists(path));
        let (source_model, fallback_to_base_art) = if requested_available {
            (requested_model.clone().expect("checked above"), false)
        } else if base_available {
            (
                base_model.clone().expect("checked above"),
                requested_model.is_some(),
            )
        } else {
            let requested = requested_model.as_deref().unwrap_or("<none>");
            let base = base_model.as_deref().unwrap_or("<none>");
            return Err(io::Error::other(format!(
                "unit {} ({}) has no install-resident model: requested {}, base profile {} -> {}",
                unit.rawcode, unit.name, requested, unit.base_rawcode, base
            ))
            .into());
        };
        Ok(ResolvedUnit {
            rawcode: unit.rawcode.clone(),
            name: unit.name.clone(),
            requested_model,
            source_model,
            fallback_to_base_art,
            intentionally_hidden: false,
            scale,
            tint_rgb: unit.tint_rgb,
            attached_visuals: unit.attached_visuals.clone(),
        })
    }

    fn resolve_building(
        &self,
        building: &BuildingAssetSpec,
    ) -> Result<ResolvedBuilding, Box<dyn Error>> {
        let profile = self
            .unit_skin
            .get(&building.base_rawcode.to_ascii_lowercase());
        let requested_model = building.model_path.as_deref().map(normalize_model_path);
        let base_model = profile
            .and_then(|profile| profile.file_sd.as_deref().or(profile.file.as_deref()))
            .map(normalize_model_path);
        let requested_resolved = requested_model
            .as_deref()
            .and_then(|path| self.resolve_building_model_path(path));
        let base_resolved = base_model
            .as_deref()
            .and_then(|path| self.resolve_building_model_path(path));
        let (source_model, fallback_to_base_art) = if let Some(resolved) = requested_resolved {
            (resolved, false)
        } else if let Some(resolved) = base_resolved {
            (resolved, requested_model.is_some())
        } else {
            let requested = requested_model.as_deref().unwrap_or("<none>");
            let base = base_model.as_deref().unwrap_or("<none>");
            return Err(io::Error::other(format!(
                "building {} ({}) has no install-resident model: requested {}, base profile {} -> {}",
                building.rawcode, building.name, requested, building.base_rawcode, base
            ))
            .into());
        };
        let scale = building
            .scale
            .or_else(|| profile.and_then(|profile| profile.model_scale_sd))
            .or_else(|| profile.and_then(|profile| profile.model_scale))
            .unwrap_or(1.0);
        Ok(ResolvedBuilding {
            rawcode: building.rawcode.clone(),
            name: building.name.clone(),
            requested_model,
            source_model,
            fallback_to_base_art,
            scale,
            animation_properties: building.animation_properties.clone(),
        })
    }

    fn resolve_building_model_path(&self, logical_path: &str) -> Option<String> {
        if self.model_exists(logical_path) {
            return Some(logical_path.to_owned());
        }

        let mut relocated = stock_building_model_relocation_candidates(logical_path)
            .into_iter()
            .filter(|candidate| self.model_exists(candidate));
        let resolved = relocated.next()?;
        if relocated.next().is_some() {
            return None;
        }
        Some(resolved)
    }

    fn model_exists(&self, logical_path: &str) -> bool {
        self.map_storage.as_ref().is_some_and(|storage| {
            map_model_candidates(logical_path)
                .iter()
                .any(|candidate| storage.file_exists(candidate))
        }) || casc_asset_paths(logical_path)
            .iter()
            .any(|path| self.storage.file_exists(path))
    }

    fn export_model_closure(
        &mut self,
        logical_path: &str,
        omit_team_glow_geosets: bool,
    ) -> Result<Vec<ModelManifest>, Box<dyn Error>> {
        self.export_model_closure_with_replacements(
            logical_path,
            &BTreeMap::new(),
            omit_team_glow_geosets,
        )
    }

    fn export_model_closure_with_replacements(
        &mut self,
        logical_path: &str,
        replaceable_textures: &BTreeMap<u32, String>,
        omit_team_glow_geosets: bool,
    ) -> Result<Vec<ModelManifest>, Box<dyn Error>> {
        let mut seen = BTreeSet::new();
        let mut models = Vec::new();
        self.export_model_closure_inner(
            logical_path,
            replaceable_textures,
            omit_team_glow_geosets,
            &mut seen,
            &mut models,
        )?;
        Ok(models)
    }

    fn export_model_closure_inner(
        &mut self,
        logical_path: &str,
        replaceable_textures: &BTreeMap<u32, String>,
        omit_team_glow_geosets: bool,
        seen: &mut BTreeSet<String>,
        models: &mut Vec<ModelManifest>,
    ) -> Result<(), Box<dyn Error>> {
        let normalized = normalize_model_path(logical_path);
        let mut key = doodad_model_key(&normalized, replaceable_textures);
        if omit_team_glow_geosets {
            key.push_str("__omit_engine_planes");
        }
        if !seen.insert(key) {
            return Ok(());
        }

        let model = self.export_model_with_replacements(
            &normalized,
            replaceable_textures,
            omit_team_glow_geosets,
        )?;
        let dependencies = model_dependency_paths(&model);
        models.push(model);

        let no_replacements = BTreeMap::new();
        for dependency in dependencies {
            if !self.model_exists(&dependency) {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "model dependency {dependency} referenced by {normalized} is not present in the map archive or Warcraft III CASC install"
                    ),
                )
                .into());
            }
            self.export_model_closure_inner(&dependency, &no_replacements, false, seen, models)?;
        }
        Ok(())
    }

    fn export_model_with_replacements(
        &mut self,
        logical_path: &str,
        replaceable_textures: &BTreeMap<u32, String>,
        omit_team_glow_geosets: bool,
    ) -> Result<ModelManifest, Box<dyn Error>> {
        let asset_name = doodad_asset_name(logical_path, replaceable_textures);
        let (source_casc_path, model_bytes) = self.read_model(logical_path)?;
        if self.keep_source {
            fs::write(
                self.output
                    .join("source/models")
                    .join(format!("{asset_name}.mdx")),
                model_bytes.as_ref(),
            )?;
        }
        let (model, mut warnings) = self.parse_model_from_staged_file(
            logical_path,
            &asset_name,
            &source_casc_path,
            model_bytes.as_ref(),
        )?;
        validate_classic_renderable_materials(&model, logical_path)?;

        // The parsed Model owns its data. Do not keep a second native buffer containing the
        // source MDX alive while textures and glTF buffers are built.
        drop(model_bytes);

        let (texture_manifests, gltf_texture_indices) =
            self.export_model_textures(&model, replaceable_textures)?;
        let gltf_name = format!("models/{asset_name}.gltf");
        let bin_name = format!("models/{asset_name}.bin");
        let (mut gltf, bin, material_warnings) = build_gltf(
            &model,
            logical_path,
            &asset_name,
            &gltf_texture_indices,
            &texture_manifests,
            omit_team_glow_geosets,
        )?;
        warnings.extend(material_warnings);
        if model.geoset_animations_len() != 0 {
            warnings.push(
                "whiteoutlib 0.2.1 Rust binding exposes GeosetAnimation::flags() as SequenceFlag; raw geoset-animation flags are intentionally omitted from this manifest until upstream fixes the binding"
                    .to_owned(),
            );
        }

        fs::write(self.output.join(&bin_name), &bin)?;

        let animations = animation_manifests_from_gltf(&gltf)?;
        let features = model_feature_manifest(&model);
        let materials = material_manifests(&model)?;
        let geoset_animations = geoset_animation_manifests(&model)?;
        let particle_emitters = particle_emitter_2_manifests(&model, &texture_manifests)?;
        let model_particle_emitters = model_particle_emitter_manifests(&model)?;
        let ribbon_emitters = ribbon_emitter_manifests(&model, &texture_manifests)?;
        let attachments = attachment_manifests(&model)?;
        let mut event_objects = event_object_manifests(
            &model,
            &self.spawn_event_models,
            &self.animation_sounds,
            &self.splat_services,
            &self.uber_splat_services,
            |path| self.model_exists(path),
        );
        self.export_event_sound_assets(&mut event_objects)?;
        self.export_event_splat_assets(&mut event_objects);
        attach_event_object_extras(&mut gltf, &event_objects)?;
        let lights = light_manifests(&model)?;

        let gltf_file = File::create(self.output.join(&gltf_name))?;
        serde_json::to_writer_pretty(BufWriter::new(gltf_file), &gltf)?;

        Ok(ModelManifest {
            source_model: logical_path.to_owned(),
            source_casc_path,
            gltf: gltf_name,
            bin: bin_name,
            geosets: model.geosets_len(),
            bones: model.bones_len(),
            features,
            overhead_position: model_overhead_position(&model),
            animations,
            textures: texture_manifests,
            materials,
            geoset_animations,
            particle_emitters,
            model_particle_emitters,
            ribbon_emitters,
            attachments,
            event_objects,
            lights,
            warnings,
        })
    }

    fn parse_model_from_staged_file(
        &self,
        logical_path: &str,
        asset_name: &str,
        source_casc_path: &str,
        model_bytes: &[u8],
    ) -> Result<(Model, Vec<String>), Box<dyn Error>> {
        if model_bytes.len() > MAX_MDX_INPUT_BYTES {
            return Err(io::Error::other(format!(
                "MDX {logical_path} is {} bytes; refusing to parse inputs above {} bytes",
                model_bytes.len(),
                MAX_MDX_INPUT_BYTES
            ))
            .into());
        }

        let extension = Path::new(logical_path)
            .extension()
            .and_then(|extension| extension.to_str())
            .filter(|extension| extension.eq_ignore_ascii_case("mdl"))
            .map_or("mdx", |_| "mdl");
        let staging_path = self.output.join("models").join(format!(
            ".{asset_name}.parse-{}.{}",
            std::process::id(),
            extension
        ));
        fs::write(&staging_path, model_bytes)?;

        let mut parser = MdxParser::new();
        let parsed = parser.parse_file(staging_path.to_string_lossy().as_ref());
        let warnings = parser.issues();
        fs::remove_file(&staging_path)?;
        let model = parsed
            .ok_or_else(|| io::Error::other(format!("failed to parse MDX {source_casc_path}")))?;
        Ok((model, warnings))
    }

    fn read_model(&mut self, logical_path: &str) -> Result<(String, Bytes), Box<dyn Error>> {
        if let Some(storage) = &self.map_storage {
            for candidate in map_model_candidates(logical_path) {
                if let Some(bytes) = storage.read_file(&candidate) {
                    return Ok((format!("map:{candidate}"), bytes));
                }
            }
        }
        for casc_path in casc_asset_paths(logical_path) {
            if let Some(bytes) = self.storage.read_file(&casc_path) {
                self.storage.flush_cache();
                return Ok((casc_path, bytes));
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("WC3 model not found in map archive or CASC for logical path {logical_path}"),
        )
        .into())
    }

    fn export_event_sound_assets(
        &mut self,
        event_objects: &mut [EventObjectManifest],
    ) -> Result<(), Box<dyn Error>> {
        for event in event_objects {
            let Some(sound) = event.sound.as_mut() else {
                continue;
            };
            if sound.silent {
                sound.files.clear();
                event.lookup_resolved = true;
                continue;
            }
            let mut files = Vec::new();
            for source in &sound.source_files {
                if let Some(output) = self.export_sound_file(source)? {
                    files.push(output);
                }
            }
            files.sort();
            files.dedup();
            sound.files = files;
            event.lookup_resolved = !sound.files.is_empty();
        }
        Ok(())
    }

    fn export_event_splat_assets(&mut self, events: &mut [EventObjectManifest]) {
        for event in events {
            let Some(splat) = event.splat.as_mut() else {
                continue;
            };
            let key = splat.source_texture.to_ascii_lowercase();
            let exported = if let Some(cached) = self.texture_cache.get(&key) {
                Some(cached.clone())
            } else {
                match self.export_texture(&splat.source_texture) {
                    Ok(texture) => {
                        self.texture_cache.insert(key, texture.clone());
                        Some(texture)
                    }
                    Err(_) => None,
                }
            };
            if let Some(png) = exported.and_then(|texture| texture.png) {
                splat.texture = png;
                event.lookup_resolved = true;
            } else {
                event.lookup_resolved = false;
            }
        }
    }

    fn export_sound_file(&mut self, logical_path: &str) -> Result<Option<String>, Box<dyn Error>> {
        let normalized = logical_path
            .trim()
            .replace('/', "\\")
            .trim_start_matches('\\')
            .to_owned();
        if normalized.is_empty() || normalized == "_" {
            return Ok(None);
        }
        let cache_key = normalized.to_ascii_lowercase();
        if let Some(cached) = self.sound_cache.get(&cache_key) {
            return Ok(cached.clone());
        }

        let mut resolved_source = normalized.clone();
        let bytes = if let Some(storage) = &self.map_storage {
            storage.read_file(&normalized)
        } else {
            None
        };
        let bytes = if let Some(bytes) = bytes {
            bytes
        } else {
            let mut resolved = None;
            for casc_path in casc_asset_paths(&normalized) {
                for candidate in [casc_path.clone(), casc_path.to_ascii_lowercase()] {
                    if let Some(bytes) = self.storage.read_file(&candidate) {
                        resolved_source = candidate;
                        resolved = Some(bytes);
                        break;
                    }
                }
                if resolved.is_some() {
                    break;
                }
            }
            if resolved.is_none()
                && let Some(candidate) = self.resolve_legacy_sound_path(&normalized)
                && let Some(bytes) = self.storage.read_file(&candidate)
            {
                resolved_source = candidate;
                resolved = Some(bytes);
            }
            let Some(bytes) = resolved else {
                self.sound_cache.insert(cache_key, None);
                return Ok(None);
            };
            bytes
        };

        let Some(extension) = Path::new(&resolved_source)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
        else {
            self.sound_cache.insert(cache_key, None);
            return Ok(None);
        };
        let output = format!("audio/{}.{}", flat_asset_name(&normalized), extension);
        let output_path = self.output.join(&output);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&output_path, bytes.as_ref())?;
        self.storage.flush_cache();
        self.sound_cache.insert(cache_key, Some(output.clone()));
        Ok(Some(output))
    }

    fn resolve_legacy_sound_path(&mut self, logical_path: &str) -> Option<String> {
        if self.sound_file_index.is_none() {
            self.sound_file_index = Some(build_sound_file_index(self.storage.list_files()));
        }
        select_legacy_sound_path(logical_path, self.sound_file_index.as_ref()?)
    }

    fn export_model_textures(
        &mut self,
        model: &Model,
        replaceable_textures: &BTreeMap<u32, String>,
    ) -> Result<TextureExport, Box<dyn Error>> {
        let relevant_texture_ids = classic_relevant_texture_ids(model);
        let mut manifests = Vec::with_capacity(model.textures_len());
        let mut gltf_indices = Vec::with_capacity(model.textures_len());
        let mut next_gltf_index = 0usize;

        if relevant_texture_ids.iter().any(|&texture_id| {
            model
                .textures(texture_id)
                .is_some_and(|texture| texture.replaceable_id() == 2)
        }) {
            // Replaceable ID 2 is player team glow. Export the complete modern WC3 player-colour
            // set once so runtime presentation can select the texture from the authoritative
            // PlayerId/slot instead of baking a red/blue placeholder into each converted model.
            for player_index in 0..WC3_PLAYER_COLOR_COUNT {
                let logical = team_glow_texture_logical(player_index);
                let key = logical.to_ascii_lowercase();
                if !self.texture_cache.contains_key(&key) {
                    let exported = self.export_texture(&logical)?;
                    self.texture_cache.insert(key, exported);
                }
            }
        }

        for (texture_id, texture) in model.textures_iter().enumerate() {
            let replaceable_id = texture.replaceable_id();
            let model_logical = normalize_texture_path(&texture.file_name());
            if !relevant_texture_ids.contains(&texture_id) {
                manifests.push(TextureManifest {
                    source_texture: model_logical,
                    source_casc_path: None,
                    png: None,
                    replaceable_id,
                    has_transparency: false,
                });
                gltf_indices.push(None);
                continue;
            }
            let logical = if replaceable_id == 0 {
                (!model_logical.is_empty()).then_some(model_logical.clone())
            } else {
                replaceable_textures
                    .get(&replaceable_id)
                    .cloned()
                    .or_else(|| (replaceable_id == 2).then(|| team_glow_texture_logical(0)))
            };
            let Some(logical) = logical else {
                manifests.push(TextureManifest {
                    source_texture: model_logical,
                    source_casc_path: None,
                    png: None,
                    replaceable_id,
                    has_transparency: false,
                });
                gltf_indices.push(None);
                continue;
            };

            let key = logical.to_ascii_lowercase();
            let mut manifest = if let Some(cached) = self.texture_cache.get(&key) {
                cached.clone()
            } else {
                let exported = self.export_texture(&logical)?;
                self.texture_cache.insert(key, exported.clone());
                exported
            };
            manifest.replaceable_id = replaceable_id;
            manifests.push(manifest);
            gltf_indices.push(Some(next_gltf_index));
            next_gltf_index += 1;
        }
        Ok((manifests, gltf_indices))
    }

    fn export_texture(&mut self, logical_path: &str) -> Result<TextureManifest, Box<dyn Error>> {
        let (source_casc_path, source_bytes, source_ext) = self.read_texture(logical_path)?;
        let png_name = format!("textures/{}.png", flat_asset_name(logical_path));
        if self.keep_source {
            let source_name = format!("{}.{}", flat_asset_name(logical_path), source_ext);
            fs::write(
                self.output.join("source/textures").join(source_name),
                source_bytes.as_ref(),
            )?;
        }
        if source_ext == "png" {
            fs::write(self.output.join(&png_name), source_bytes.as_ref())?;
        }

        let decoded = match source_ext.as_str() {
            "blp" => {
                let mut parser = BlpParser::new();
                parser.parse(&source_bytes).ok_or_else(|| {
                    io::Error::other(format!(
                        "failed to decode BLP {source_casc_path}: {:?}",
                        parser.issues()
                    ))
                })?
            }
            "dds" => {
                let mut parser = DdsParser::new();
                parser.parse(&source_bytes).ok_or_else(|| {
                    io::Error::other(format!(
                        "failed to decode DDS {source_casc_path}: {:?}",
                        parser.issues()
                    ))
                })?
            }
            "tga" => {
                let mut parser = TgaParser::new();
                parser.parse(&source_bytes).ok_or_else(|| {
                    io::Error::other(format!(
                        "failed to decode TGA {source_casc_path}: {:?}",
                        parser.issues()
                    ))
                })?
            }
            "png" => {
                let mut parser = PngParser::new();
                parser.parse(&source_bytes).ok_or_else(|| {
                    io::Error::other(format!(
                        "failed to decode PNG {source_casc_path}: {:?}",
                        parser.issues()
                    ))
                })?
            }
            other => {
                return Err(io::Error::other(format!(
                    "unsupported WC3 texture format .{other}: {source_casc_path}"
                ))
                .into());
            }
        };
        // Parsing owns the decoded image, so the compressed/source payload can be released before
        // alpha inspection and PNG encoding allocate any additional buffers.
        drop(source_bytes);
        let has_transparency = texture_has_transparency(&decoded);
        if source_ext != "png" {
            let png_bytes = texture_to_png(&decoded)?;
            fs::write(self.output.join(&png_name), png_bytes.as_ref())?;
        }

        Ok(TextureManifest {
            source_texture: logical_path.to_owned(),
            source_casc_path: Some(source_casc_path),
            png: Some(png_name),
            replaceable_id: 0,
            has_transparency,
        })
    }

    fn read_texture(
        &mut self,
        logical_path: &str,
    ) -> Result<(String, Bytes, String), Box<dyn Error>> {
        let path = Path::new(logical_path);
        let requested_ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let stem = path.with_extension("").to_string_lossy().replace('/', "\\");
        let stems = legacy_texture_stems(&stem);

        let mut extensions = Vec::new();
        if !requested_ext.is_empty() {
            extensions.push(requested_ext);
        }
        for fallback in ["dds", "blp", "tga", "png"] {
            if !extensions.iter().any(|ext| ext == fallback) {
                extensions.push(fallback.to_owned());
            }
        }

        for stem in stems {
            for ext in &extensions {
                let candidate = format!("{stem}.{ext}");
                if let Some(storage) = &self.map_storage
                    && let Some(bytes) = storage.read_file(&candidate)
                {
                    return Ok((format!("map:{candidate}"), bytes, ext.clone()));
                }
                for casc_path in casc_asset_paths(&candidate) {
                    if let Some(bytes) = self.storage.read_file(&casc_path) {
                        self.storage.flush_cache();
                        return Ok((casc_path, bytes, ext.clone()));
                    }
                }
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("WC3 texture not found in map archive or CASC for logical path {logical_path}"),
        )
        .into())
    }
}

fn texture_has_transparency(texture: &Texture) -> bool {
    match texture.format() {
        PixelFormat::R8
        | PixelFormat::R16
        | PixelFormat::R16F
        | PixelFormat::R32F
        | PixelFormat::RG8
        | PixelFormat::RG16
        | PixelFormat::RG16F
        | PixelFormat::RG32F
        | PixelFormat::BC4
        | PixelFormat::BC5
        | PixelFormat::BC6H => false,
        PixelFormat::RGBA8 => texture
            .mip_data(0, 0)
            .as_ref()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] < u8::MAX),
        PixelFormat::RGBA16 => texture
            .mip_data(0, 0)
            .as_ref()
            .as_chunks::<8>()
            .0
            .iter()
            .any(|pixel| u16::from_le_bytes([pixel[6], pixel[7]]) < u16::MAX),
        PixelFormat::RGBA32F => texture
            .mip_data(0, 0)
            .as_ref()
            .as_chunks::<16>()
            .0
            .iter()
            .any(|pixel| f32::from_le_bytes([pixel[12], pixel[13], pixel[14], pixel[15]]) < 1.0),
        PixelFormat::RGBA16F => texture
            .copy_as_format(PixelFormat::RGBA32F, None)
            .is_some_and(|rgba| texture_has_transparency(&rgba)),
        // Block-compressed formats with alpha need decoding to inspect actual coverage. Keep this
        // fallback narrow so the common decoded RGBA8 path never allocates a second full image.
        PixelFormat::BC1 | PixelFormat::BC2 | PixelFormat::BC3 | PixelFormat::BC7 => texture
            .copy_as_format(PixelFormat::RGBA8, None)
            .is_some_and(|rgba| {
                rgba.mip_data(0, 0)
                    .as_ref()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] < u8::MAX)
            }),
    }
}

fn texture_to_png(texture: &Texture) -> Result<Bytes, Box<dyn Error>> {
    let mut writer = PngWriter::new();
    let bytes = writer.write(texture);
    if bytes.is_empty() {
        return Err(io::Error::other(format!(
            "PNG encoder produced no data: {:?}",
            writer.issues()
        ))
        .into());
    }
    Ok(bytes)
}

fn is_classic_material_layer(layer: &Layer) -> bool {
    layer.shader() == LayerShaderType::SD
}

fn classic_relevant_texture_ids(model: &Model) -> BTreeSet<usize> {
    let mut texture_ids = BTreeSet::new();
    for material in model.materials_iter() {
        for layer in material
            .layers_iter()
            .filter(|layer| is_classic_material_layer(layer))
        {
            texture_ids.insert(layer_diffuse_texture_id(&layer) as usize);
            if let Some(diffuse) = layer
                .sub_textures_iter()
                .find(|sub_texture| sub_texture.slot() == LayerSlotType::DiffuseMap)
            {
                texture_ids.extend(diffuse.tracks().keys().iter().map(|&id| id as usize));
            } else {
                texture_ids.extend(
                    layer
                        .texture_id_tracks()
                        .keys()
                        .iter()
                        .map(|&id| id as usize),
                );
            }
        }
    }
    texture_ids.extend(
        model
            .particle_emitters_2_iter()
            .map(|emitter| emitter.texture_id() as usize),
    );
    texture_ids
}

fn validate_classic_renderable_materials(
    model: &Model,
    logical_path: &str,
) -> Result<(), Box<dyn Error>> {
    let mut material_uses = BTreeMap::<usize, Vec<String>>::new();
    for (geoset_index, geoset) in model.geosets_iter().enumerate() {
        if geoset.vertex_positions().is_empty() || geoset.faces().is_empty() {
            continue;
        }
        material_uses
            .entry(geoset.material_id() as usize)
            .or_default()
            .push(format!("geoset {geoset_index}"));
    }
    for (ribbon_index, ribbon) in model.ribbon_emitters_iter().enumerate() {
        material_uses
            .entry(ribbon.material_id() as usize)
            .or_default()
            .push(format!("ribbon emitter {ribbon_index}"));
    }

    for (material_id, uses) in material_uses {
        let material = model.materials(material_id).ok_or_else(|| {
            io::Error::other(format!(
                "Classic/SD model {logical_path} references missing material {material_id} from {}",
                uses.join(", ")
            ))
        })?;
        if !material
            .layers_iter()
            .any(|layer| is_classic_material_layer(&layer))
        {
            return Err(io::Error::other(format!(
                "Classic/SD model {logical_path} has no SD layer in material {material_id} used by {}; HD/Reforged/Definitive material rendering is out of scope",
                uses.join(", ")
            ))
            .into());
        }
    }
    Ok(())
}

fn model_feature_manifest(model: &Model) -> ModelFeatureManifest {
    let mut material_count = 0;
    let mut material_layer_count = 0;
    let mut multilayer_material_count = 0;
    let mut animated_material_alpha_layer_count = 0;
    let mut animated_material_texture_layer_count = 0;
    for material in model.materials_iter() {
        let classic_layers = material
            .layers_iter()
            .filter(|layer| is_classic_material_layer(layer))
            .collect::<Vec<_>>();
        if classic_layers.is_empty() {
            continue;
        }
        material_count += 1;
        material_layer_count += classic_layers.len();
        if classic_layers.len() > 1 {
            multilayer_material_count += 1;
        }
        for layer in classic_layers {
            if layer.alpha_tracks().is_used() {
                animated_material_alpha_layer_count += 1;
            }
            if layer.texture_id_tracks().is_used()
                || layer
                    .sub_textures_iter()
                    .any(|sub_texture| sub_texture.tracks().is_used())
            {
                animated_material_texture_layer_count += 1;
            }
        }
    }

    let animated_geoset_alpha_count = model
        .geoset_animations_iter()
        .filter(|animation| animation.alpha_tracks().is_used())
        .count();

    let mut attachment_models = model
        .attachments_iter()
        .map(|attachment| attachment.path())
        .filter(|path| !path.trim().is_empty())
        .collect::<Vec<_>>();
    attachment_models.sort_by_key(|path| path.to_ascii_lowercase());
    attachment_models.dedup_by(|left, right| left.eq_ignore_ascii_case(right));

    let particle_emitter_animated_track_count = model
        .particle_emitters_iter()
        .map(|emitter| {
            [
                emitter.emission_rate_tracks().is_used(),
                emitter.gravity_tracks().is_used(),
                emitter.longitude_tracks().is_used(),
                emitter.latitude_tracks().is_used(),
                emitter.lifespan_tracks().is_used(),
                emitter.speed_tracks().is_used(),
                emitter.visibility_tracks().is_used(),
            ]
            .into_iter()
            .filter(|used| *used)
            .count()
        })
        .sum();

    let particle_emitter_2_animated_track_count = model
        .particle_emitters_2_iter()
        .map(|emitter| {
            [
                emitter.speed_tracks().is_used(),
                emitter.variation_tracks().is_used(),
                emitter.latitude_tracks().is_used(),
                emitter.gravity_tracks().is_used(),
                emitter.emission_rate_tracks().is_used(),
                emitter.length_tracks().is_used(),
                emitter.width_tracks().is_used(),
                emitter.visibility_tracks().is_used(),
            ]
            .into_iter()
            .filter(|used| *used)
            .count()
        })
        .sum();

    let ribbon_emitter_animated_track_count = model
        .ribbon_emitters_iter()
        .map(|emitter| {
            [
                emitter.height_above_tracks().is_used(),
                emitter.height_below_tracks().is_used(),
                emitter.alpha_tracks().is_used(),
                emitter.color_tracks().is_used(),
                emitter.texture_slot_tracks().is_used(),
                emitter.visibility_tracks().is_used(),
            ]
            .into_iter()
            .filter(|used| *used)
            .count()
        })
        .sum();

    let corn_emitter_animated_track_count = model
        .corn_emitters_iter()
        .map(|emitter| {
            [
                emitter.life_span_tracks().is_used(),
                emitter.emission_rate_tracks().is_used(),
                emitter.speed_tracks().is_used(),
                emitter.color_tracks().is_used(),
                emitter.alpha_tracks().is_used(),
                emitter.visibility_tracks().is_used(),
            ]
            .into_iter()
            .filter(|used| *used)
            .count()
        })
        .sum();

    let non_inheritance_node_count = model
        .bones_iter()
        .map(|bone| node_uses_non_inheritance(&bone.node()))
        .chain(
            model
                .helpers_iter()
                .map(|helper| node_uses_non_inheritance(&helper.node())),
        )
        .chain(
            model
                .sound_emitters_iter()
                .map(|emitter| node_uses_non_inheritance(&emitter.node())),
        )
        .chain(
            model
                .attachments_iter()
                .map(|attachment| node_uses_non_inheritance(&attachment.node())),
        )
        .chain(
            model
                .lights_iter()
                .map(|light| node_uses_non_inheritance(&light.node())),
        )
        .chain(
            model
                .particle_emitters_iter()
                .map(|emitter| node_uses_non_inheritance(&emitter.node())),
        )
        .chain(
            model
                .particle_emitters_2_iter()
                .map(|emitter| node_uses_non_inheritance(&emitter.node())),
        )
        .chain(
            model
                .ribbon_emitters_iter()
                .map(|emitter| node_uses_non_inheritance(&emitter.node())),
        )
        .chain(
            model
                .event_objects_iter()
                .map(|event| node_uses_non_inheritance(&event.node())),
        )
        .chain(
            model
                .collision_shapes_iter()
                .map(|shape| node_uses_non_inheritance(&shape.node())),
        )
        .chain(
            model
                .corn_emitters_iter()
                .map(|emitter| node_uses_non_inheritance(&emitter.node())),
        )
        .filter(|uses| *uses)
        .count();

    let max_classic_skin_influences = model
        .geosets_iter()
        .map(|geoset| geoset.matrix_groups().iter().copied().max().unwrap_or(0))
        .max()
        .unwrap_or(0);

    let omni_light_count = model
        .lights_iter()
        .filter(|light| light.type_() == LightType::Omni)
        .count();
    let light_count = model.lights_len();

    ModelFeatureManifest {
        material_count,
        material_layer_count,
        multilayer_material_count,
        animated_material_alpha_layer_count,
        animated_material_texture_layer_count,
        animated_geoset_alpha_count,
        global_sequence_count: model.global_sequences().len(),
        attachment_count: model.attachments_len(),
        attachment_models,
        particle_emitter_count: model.particle_emitters_len(),
        particle_emitter_animated_track_count,
        particle_emitter_2_count: model.particle_emitters_2_len(),
        particle_emitter_2_animated_track_count,
        ribbon_emitter_count: model.ribbon_emitters_len(),
        ribbon_emitter_animated_track_count,
        corn_emitter_count: model.corn_emitters_len(),
        corn_emitter_animated_track_count,
        event_object_count: model.event_objects_len(),
        light_count,
        omni_light_count,
        non_omni_light_count: light_count.saturating_sub(omni_light_count),
        non_inheritance_node_count,
        max_classic_skin_influences,
    }
}

fn node_uses_non_inheritance(node: &Node) -> bool {
    let inherit_mask = NodeFlag::DONT_INHERIT_TRANSLATION
        | NodeFlag::DONT_INHERIT_ROTATION
        | NodeFlag::DONT_INHERIT_SCALING;
    !(node.flags() & inherit_mask).is_empty()
}

fn model_dependency_paths(model: &ModelManifest) -> Vec<String> {
    let mut dependencies = BTreeSet::new();
    for attachment in &model.attachments {
        let path = attachment.path.trim();
        if path.is_empty() {
            continue;
        }
        let normalized = normalize_model_path(path);
        if !is_intentionally_hidden_model_path(&normalized) {
            dependencies.insert(normalized);
        }
    }
    for emitter in &model.model_particle_emitters {
        let path = emitter.spawn_model.trim();
        if path.is_empty() {
            continue;
        }
        let normalized = normalize_model_path(path);
        if !is_intentionally_hidden_model_path(&normalized) {
            dependencies.insert(normalized);
        }
    }
    for event in &model.event_objects {
        if event.gltf.is_none() {
            continue;
        }
        let Some(path) = event.spawn_model.as_deref() else {
            continue;
        };
        let normalized = normalize_model_path(path);
        if !is_intentionally_hidden_model_path(&normalized) {
            dependencies.insert(normalized);
        }
    }
    dependencies.into_iter().collect()
}

fn scalar_track_manifest(track: &TrackF32) -> Result<Option<ScalarTrackManifest>, Box<dyn Error>> {
    if !track.is_used() {
        return Ok(None);
    }
    validate_f32_track(track)?;
    let interpolation = track_interpolation_manifest(track.interpolation_type());
    let smooth = matches!(
        interpolation,
        ScalarTrackInterpolationManifest::Hermite | ScalarTrackInterpolationManifest::Bezier
    );
    let mut values = Vec::with_capacity(track.key_count());
    let mut in_tangents = if smooth {
        Vec::with_capacity(track.key_count())
    } else {
        Vec::new()
    };
    let mut out_tangents = if smooth {
        Vec::with_capacity(track.key_count())
    } else {
        Vec::new()
    };
    for key in 0..track.key_count() {
        values.push(f32_key(track, key, 0));
        if smooth {
            in_tangents.push(f32_key(track, key, 1));
            out_tangents.push(f32_key(track, key, 2));
        }
    }
    Ok(Some(ScalarTrackManifest {
        interpolation,
        global_sequence_id: (track.global_sequence_id() != NO_GLOBAL_SEQUENCE)
            .then_some(track.global_sequence_id()),
        timestamps: track.timestamps().to_vec(),
        values,
        in_tangents,
        out_tangents,
    }))
}

fn vector3_track_manifest(
    track: &TrackVector3f,
) -> Result<Option<Vector3TrackManifest>, Box<dyn Error>> {
    if !track.is_used() {
        return Ok(None);
    }
    validate_vec3_track(track)?;
    let interpolation = track_interpolation_manifest(track.interpolation_type());
    let smooth = matches!(
        interpolation,
        ScalarTrackInterpolationManifest::Hermite | ScalarTrackInterpolationManifest::Bezier
    );
    let mut values = Vec::with_capacity(track.key_count());
    let mut in_tangents = if smooth {
        Vec::with_capacity(track.key_count())
    } else {
        Vec::new()
    };
    let mut out_tangents = if smooth {
        Vec::with_capacity(track.key_count())
    } else {
        Vec::new()
    };
    for key in 0..track.key_count() {
        values.push(vec3_key(track, key, 0));
        if smooth {
            in_tangents.push(vec3_key(track, key, 1));
            out_tangents.push(vec3_key(track, key, 2));
        }
    }
    Ok(Some(Vector3TrackManifest {
        interpolation,
        global_sequence_id: (track.global_sequence_id() != NO_GLOBAL_SEQUENCE)
            .then_some(track.global_sequence_id()),
        timestamps: track.timestamps().to_vec(),
        values,
        in_tangents,
        out_tangents,
    }))
}

fn unsigned_track_manifest(
    track: &TrackU32,
) -> Result<Option<UnsignedTrackManifest>, Box<dyn Error>> {
    if !track.is_used() {
        return Ok(None);
    }
    validate_u32_track(track)?;
    let interpolation = track_interpolation_manifest(track.interpolation_type());
    let smooth = matches!(
        interpolation,
        ScalarTrackInterpolationManifest::Hermite | ScalarTrackInterpolationManifest::Bezier
    );
    let mut values = Vec::with_capacity(track.key_count());
    let mut in_tangents = if smooth {
        Vec::with_capacity(track.key_count())
    } else {
        Vec::new()
    };
    let mut out_tangents = if smooth {
        Vec::with_capacity(track.key_count())
    } else {
        Vec::new()
    };
    for key in 0..track.key_count() {
        values.push(u32_key(track, key, 0));
        if smooth {
            in_tangents.push(u32_key(track, key, 1));
            out_tangents.push(u32_key(track, key, 2));
        }
    }
    Ok(Some(UnsignedTrackManifest {
        interpolation,
        global_sequence_id: (track.global_sequence_id() != NO_GLOBAL_SEQUENCE)
            .then_some(track.global_sequence_id()),
        timestamps: track.timestamps().to_vec(),
        values,
        in_tangents,
        out_tangents,
    }))
}

fn track_interpolation_manifest(
    interpolation: InterpolationType,
) -> ScalarTrackInterpolationManifest {
    match interpolation {
        InterpolationType::None => ScalarTrackInterpolationManifest::DontInterp,
        InterpolationType::Linear => ScalarTrackInterpolationManifest::Linear,
        InterpolationType::Hermite => ScalarTrackInterpolationManifest::Hermite,
        InterpolationType::Bezier => ScalarTrackInterpolationManifest::Bezier,
    }
}

fn geoset_animation_manifests(
    model: &Model,
) -> Result<Vec<GeosetAnimationManifest>, Box<dyn Error>> {
    model
        .geoset_animations_iter()
        .map(|animation| {
            let color = animation.color();
            Ok(GeosetAnimationManifest {
                geoset_id: animation.geoset_id(),
                flags: "unavailable:whiteoutlib-0.2.1-binding".to_owned(),
                alpha: animation.alpha(),
                color: [color.x, color.y, color.z],
                alpha_track: scalar_track_manifest(&animation.alpha_tracks())?,
                color_track: vector3_track_manifest(&animation.color_tracks())?,
            })
        })
        .collect()
}

fn material_manifests(model: &Model) -> Result<Vec<MaterialManifest>, Box<dyn Error>> {
    model
        .materials_iter()
        .enumerate()
        .map(|(material_id, material)| {
            let layers = material
                .layers_iter()
                .enumerate()
                .filter(|(_, layer)| is_classic_material_layer(layer))
                .map(|(layer_index, layer)| {
                    let sub_textures = layer
                        .sub_textures_iter()
                        .map(|sub_texture| {
                            Ok(MaterialSubTextureManifest {
                                slot: format!("{:?}", sub_texture.slot()),
                                texture_id: sub_texture.texture_id(),
                                texture_id_track: unsigned_track_manifest(&sub_texture.tracks())?,
                            })
                        })
                        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
                    Ok(MaterialLayerManifest {
                        layer_index,
                        shader: format!("{:?}", layer.shader()),
                        filter_mode: format!("{:?}", layer.filter_mode()),
                        texture_id: layer.texture_id(),
                        alpha: layer.alpha(),
                        texture_id_track: unsigned_track_manifest(&layer.texture_id_tracks())?,
                        alpha_track: scalar_track_manifest(&layer.alpha_tracks())?,
                        sub_textures,
                    })
                })
                .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
            Ok(MaterialManifest {
                material_id,
                priority_plane: material.priority_plane(),
                layers,
            })
        })
        .collect()
}

fn particle_emitter_2_manifests(
    model: &Model,
    textures: &[TextureManifest],
) -> Result<Vec<ParticleEmitter2Manifest>, Box<dyn Error>> {
    let sequence_windows = model
        .sequences_iter()
        .map(|sequence| ParticleEmitterSequenceManifest {
            name: sequence.name(),
            start_ms: sequence.interval_start(),
            end_ms: sequence.interval_end(),
            non_looping: sequence.flags() == SequenceFlag::NonLooping,
        })
        .collect::<Vec<_>>();
    let global_sequence_durations_ms = model.global_sequences().to_vec();

    model
        .particle_emitters_2_iter()
        .map(|emitter| {
            let node = emitter.node();
            let segment_colors = std::array::from_fn(|index| {
                let color = emitter.segment_color(index);
                [color.x, color.y, color.z]
            });
            let active_sequences = particle_emitter_active_sequences(model, &emitter)?;
            let ambient_enabled = active_sequences
                .iter()
                .any(|sequence| sequence.eq_ignore_ascii_case("stand"))
                || (!model
                    .sequences_iter()
                    .any(|sequence| sequence.name().trim().eq_ignore_ascii_case("stand"))
                    && emitter.emission_rate() > 0.0
                    && !emitter.emission_rate_tracks().is_used()
                    && !emitter.visibility_tracks().is_used());
            Ok(ParticleEmitter2Manifest {
                object_id: node.object_id(),
                name: node.name(),
                position: model_node_position(model, &node),
                speed: emitter.speed(),
                variation: emitter.variation(),
                latitude: emitter.latitude(),
                gravity: emitter.gravity(),
                lifespan: emitter.lifespan(),
                emission_rate: emitter.emission_rate(),
                length: emitter.length(),
                width: emitter.width(),
                speed_track: scalar_track_manifest(&emitter.speed_tracks())?,
                variation_track: scalar_track_manifest(&emitter.variation_tracks())?,
                latitude_track: scalar_track_manifest(&emitter.latitude_tracks())?,
                gravity_track: scalar_track_manifest(&emitter.gravity_tracks())?,
                emission_rate_track: scalar_track_manifest(&emitter.emission_rate_tracks())?,
                length_track: scalar_track_manifest(&emitter.length_tracks())?,
                width_track: scalar_track_manifest(&emitter.width_tracks())?,
                visibility_track: scalar_track_manifest(&emitter.visibility_tracks())?,
                filter_mode: emitter.filter_mode(),
                rows: emitter.rows(),
                columns: emitter.columns(),
                head_or_tail: emitter.head_or_tail(),
                tail_length: emitter.tail_length(),
                time: emitter.time(),
                head_interval: std::array::from_fn(|index| emitter.head_interval(index)),
                head_decay_interval: std::array::from_fn(|index| {
                    emitter.head_decay_interval(index)
                }),
                tail_interval: std::array::from_fn(|index| emitter.tail_interval(index)),
                tail_decay_interval: std::array::from_fn(|index| {
                    emitter.tail_decay_interval(index)
                }),
                priority_plane: emitter.priority_plane(),
                segment_colors,
                segment_alpha: std::array::from_fn(|index| emitter.segment_alpha(index)),
                segment_scaling: std::array::from_fn(|index| emitter.segment_scaling(index)),
                texture: textures
                    .get(emitter.texture_id() as usize)
                    .and_then(|texture| texture.png.clone()),
                squirt: emitter.squirt() != 0,
                replaceable_id: emitter.replaceable_id(),
                sequence_windows: sequence_windows.clone(),
                global_sequence_durations_ms: global_sequence_durations_ms.clone(),
                ambient_enabled,
                active_sequences,
            })
        })
        .collect()
}

fn particle_emitter_active_sequences(
    model: &Model,
    emitter: &ParticleEmitter2,
) -> Result<Vec<String>, Box<dyn Error>> {
    let mut active = Vec::new();
    for sequence in model.sequences_iter() {
        let start = sequence.interval_start();
        let end = sequence.interval_end();
        if end <= start {
            continue;
        }
        let visibility = max_f32_track_value(model, &emitter.visibility_tracks(), start, end, 1.0)?;
        let emission_rate = max_f32_track_value(
            model,
            &emitter.emission_rate_tracks(),
            start,
            end,
            emitter.emission_rate(),
        )?;
        if visibility > 0.001 && emission_rate > 0.0 {
            active.push(sequence.name());
        }
    }
    Ok(active)
}

fn max_f32_track_value(
    model: &Model,
    track: &TrackF32,
    start: u32,
    end: u32,
    default: f32,
) -> Result<f32, Box<dyn Error>> {
    let Some(samples) = sample_f32_track(model, track, start, end, end - start)? else {
        return Ok(default);
    };
    let mut values = samples.values.into_iter().filter(|value| value.is_finite());
    let Some(first) = values.next() else {
        return Ok(default);
    };
    Ok(values.fold(first, f32::max))
}

fn model_particle_emitter_manifests(
    model: &Model,
) -> Result<Vec<ModelParticleEmitterManifest>, Box<dyn Error>> {
    model
        .particle_emitters_iter()
        .map(|emitter| {
            let node = emitter.node();
            Ok(ModelParticleEmitterManifest {
                object_id: node.object_id(),
                name: node.name(),
                position: model_node_position(model, &node),
                emission_rate: emitter.emission_rate(),
                gravity: emitter.gravity(),
                longitude: emitter.longitude(),
                latitude: emitter.latitude(),
                lifespan: emitter.lifespan(),
                initial_velocity: emitter.initial_velocity(),
                spawn_model: emitter.spawn_model_file_name(),
                gltf: dependency_child_gltf(&emitter.spawn_model_file_name()),
                emission_rate_track: scalar_track_manifest(&emitter.emission_rate_tracks())?,
                gravity_track: scalar_track_manifest(&emitter.gravity_tracks())?,
                longitude_track: scalar_track_manifest(&emitter.longitude_tracks())?,
                latitude_track: scalar_track_manifest(&emitter.latitude_tracks())?,
                lifespan_track: scalar_track_manifest(&emitter.lifespan_tracks())?,
                speed_track: scalar_track_manifest(&emitter.speed_tracks())?,
                visibility_track: scalar_track_manifest(&emitter.visibility_tracks())?,
                sequence_windows: model_sequence_windows(model),
                global_sequence_durations_ms: model.global_sequences().to_vec(),
            })
        })
        .collect()
}

fn attachment_manifests(model: &Model) -> Result<Vec<AttachmentManifest>, Box<dyn Error>> {
    let sequence_windows = model_sequence_windows(model);
    model
        .attachments_iter()
        .map(|attachment| {
            let node = attachment.node();
            let path = attachment.path();
            let gltf = dependency_child_gltf(&path);
            Ok(AttachmentManifest {
                object_id: node.object_id(),
                name: node.name(),
                position: model_node_position(model, &node),
                path,
                gltf,
                visibility_track: scalar_track_manifest(&attachment.visibility_tracks())?,
                sequence_windows: sequence_windows.clone(),
                global_sequence_durations_ms: model.global_sequences().to_vec(),
            })
        })
        .collect()
}

fn dependency_child_gltf(path: &str) -> Option<String> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let normalized = normalize_model_path(path);
    if is_intentionally_hidden_model_path(&normalized) {
        return None;
    }
    Some(format!(
        "models/{}.gltf",
        doodad_asset_name(&normalized, &BTreeMap::new())
    ))
}

fn model_sequence_windows(model: &Model) -> Vec<ParticleEmitterSequenceManifest> {
    model
        .sequences_iter()
        .map(|sequence| ParticleEmitterSequenceManifest {
            name: sequence.name(),
            start_ms: sequence.interval_start(),
            end_ms: sequence.interval_end(),
            non_looping: sequence.flags() == SequenceFlag::NonLooping,
        })
        .collect()
}

fn classify_event_object_name(name: &str) -> (EventObjectKindManifest, Option<String>) {
    let bytes = name.as_bytes();
    let kind = match bytes.get(..3).map(|prefix| prefix.to_ascii_uppercase()) {
        Some(prefix) if prefix == b"SND" => EventObjectKindManifest::Sound,
        Some(prefix) if prefix == b"SPL" => EventObjectKindManifest::Splat,
        Some(prefix) if prefix == b"FPT" => EventObjectKindManifest::Footprint,
        Some(prefix) if prefix == b"SPN" => EventObjectKindManifest::Spawn,
        Some(prefix) if prefix == b"UBR" => EventObjectKindManifest::UberSplat,
        _ => EventObjectKindManifest::Unknown,
    };
    let event_code = (kind != EventObjectKindManifest::Unknown && bytes.len() >= 8)
        .then(|| String::from_utf8_lossy(&bytes[4..8]).to_ascii_uppercase());
    (kind, event_code)
}

fn event_object_manifests(
    model: &Model,
    spawn_event_models: &BTreeMap<String, String>,
    animation_sounds: &BTreeMap<String, AnimationSoundSpec>,
    splat_services: &BTreeMap<String, SplatServiceSpec>,
    uber_splat_services: &BTreeMap<String, SplatServiceSpec>,
    model_exists: impl Fn(&str) -> bool,
) -> Vec<EventObjectManifest> {
    let sequence_windows = model_sequence_windows(model);
    model
        .event_objects_iter()
        .map(|event| {
            let node = event.node();
            let name = node.name();
            let (kind, event_code) = classify_event_object_name(&name);
            let spawn_model = (kind == EventObjectKindManifest::Spawn)
                .then(|| {
                    event_code
                        .as_ref()
                        .and_then(|code| spawn_event_models.get(code))
                })
                .flatten()
                .cloned();
            let spawn_asset_exists = spawn_model.as_deref().is_some_and(&model_exists);
            let gltf = spawn_model
                .as_deref()
                .filter(|_| spawn_asset_exists)
                .and_then(dependency_child_gltf);
            let sound = (kind == EventObjectKindManifest::Sound)
                .then(|| {
                    event_code
                        .as_ref()
                        .and_then(|code| animation_sounds.get(code))
                })
                .flatten()
                .map(|sound| EventSoundManifest {
                    sound_name: sound.sound_name.clone(),
                    source_files: sound.source_files.clone(),
                    files: Vec::new(),
                    silent: sound.silent,
                    volume: sound.volume,
                    volume_variance: sound.volume_variance,
                    pitch: sound.pitch,
                    pitch_variance: sound.pitch_variance,
                    maximum_concurrent_instances: sound.maximum_concurrent_instances,
                    priority: sound.priority,
                    channel: sound.channel,
                    flags: sound.flags.clone(),
                    min_distance: sound.min_distance,
                    max_distance: sound.max_distance,
                    distance_cutoff: sound.distance_cutoff,
                    eax_flags: sound.eax_flags.clone(),
                    rolloff_points: sound.rolloff_points.clone(),
                });
            let service = match kind {
                EventObjectKindManifest::Splat | EventObjectKindManifest::Footprint => event_code
                    .as_ref()
                    .and_then(|code| splat_services.get(code)),
                EventObjectKindManifest::UberSplat => event_code
                    .as_ref()
                    .and_then(|code| uber_splat_services.get(code)),
                _ => None,
            };
            let splat = service.map(|service| EventSplatManifest {
                texture: String::new(),
                source_texture: service.source_texture.clone(),
                rows: service.rows,
                columns: service.columns,
                blend_mode: service.blend_mode,
                scale: service.scale,
                lifespan: service.lifespan,
                decay: service.decay,
                birth_time: service.birth_time,
                pause_time: service.pause_time,
                uv_lifespan: service.uv_lifespan,
                lifespan_repeat: service.lifespan_repeat,
                uv_decay: service.uv_decay,
                decay_repeat: service.decay_repeat,
                colors: service.colors,
            });
            let lookup_resolved = match kind {
                EventObjectKindManifest::Spawn => spawn_asset_exists,
                EventObjectKindManifest::Sound => sound.is_some(),
                EventObjectKindManifest::Unknown => false,
                _ => false,
            };
            EventObjectManifest {
                object_id: node.object_id(),
                name,
                position: model_node_position(model, &node),
                kind,
                event_code,
                lookup_resolved,
                spawn_model,
                gltf,
                sound,
                splat,
                global_sequence_id: (event.global_sequence_id() != NO_GLOBAL_SEQUENCE)
                    .then_some(event.global_sequence_id()),
                event_track_times: event.event_track_times().to_vec(),
                sequence_windows: sequence_windows.clone(),
                global_sequence_durations_ms: model.global_sequences().to_vec(),
            }
        })
        .collect()
}

fn attach_event_object_extras(
    gltf: &mut Value,
    event_objects: &[EventObjectManifest],
) -> Result<(), Box<dyn Error>> {
    let by_object_id = event_objects
        .iter()
        .map(|event| (event.object_id, event))
        .collect::<BTreeMap<_, _>>();
    let Some(nodes) = gltf.get_mut("nodes").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for node in nodes {
        let Some(object_id) = node
            .get("extras")
            .and_then(|extras| extras.get("wc3ObjectId"))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
        else {
            continue;
        };
        let Some(event) = by_object_id.get(&object_id) else {
            continue;
        };
        node["extras"]
            .as_object_mut()
            .expect("WC3 skeleton nodes always carry object extras")
            .insert("wc3EventObject".into(), serde_json::to_value(event)?);
    }
    Ok(())
}

fn light_manifest(model: &Model, light: &Light) -> Result<LightManifest, Box<dyn Error>> {
    let node = light.node();
    let color = light.color();
    let ambient_color = light.ambient_color();
    let light_type = match light.type_() {
        LightType::Omni => "omni",
        LightType::Directional => "directional",
        LightType::Ambient => "ambient",
    }
    .to_owned();
    let sequence_windows = model
        .sequences_iter()
        .map(|sequence| ParticleEmitterSequenceManifest {
            name: sequence.name(),
            start_ms: sequence.interval_start(),
            end_ms: sequence.interval_end(),
            non_looping: sequence.flags() == SequenceFlag::NonLooping,
        })
        .collect();

    Ok(LightManifest {
        object_id: node.object_id(),
        name: node.name(),
        position: model_node_position(model, &node),
        light_type,
        attenuation_start: light.attenuation_start(),
        attenuation_end: light.attenuation_end(),
        color: [color.x, color.y, color.z],
        intensity: light.intensity(),
        ambient_color: [ambient_color.x, ambient_color.y, ambient_color.z],
        ambient_intensity: light.ambient_intensity(),
        attenuation_start_track: scalar_track_manifest(&light.attenuation_start_tracks())?,
        attenuation_end_track: scalar_track_manifest(&light.attenuation_end_tracks())?,
        color_track: vector3_track_manifest(&light.color_tracks())?,
        intensity_track: scalar_track_manifest(&light.intensity_tracks())?,
        ambient_color_track: vector3_track_manifest(&light.ambient_color_tracks())?,
        ambient_intensity_track: scalar_track_manifest(&light.ambient_intensity_tracks())?,
        visibility_track: scalar_track_manifest(&light.visibility_tracks())?,
        sequence_windows,
        global_sequence_durations_ms: model.global_sequences().to_vec(),
    })
}

fn light_manifests(model: &Model) -> Result<Vec<LightManifest>, Box<dyn Error>> {
    model
        .lights_iter()
        .map(|light| light_manifest(model, &light))
        .collect()
}

fn ribbon_emitter_manifests(
    model: &Model,
    texture_manifests: &[TextureManifest],
) -> Result<Vec<RibbonEmitterManifest>, Box<dyn Error>> {
    let sequence_windows = model
        .sequences_iter()
        .map(|sequence| ParticleEmitterSequenceManifest {
            name: sequence.name(),
            start_ms: sequence.interval_start(),
            end_ms: sequence.interval_end(),
            non_looping: sequence.flags() == SequenceFlag::NonLooping,
        })
        .collect::<Vec<_>>();
    let global_sequence_durations_ms = model.global_sequences().to_vec();

    model
        .ribbon_emitters_iter()
        .map(|emitter| {
            let node = emitter.node();
            let color = emitter.color();
            let (filter_mode, texture) =
                ribbon_material_properties(model, texture_manifests, emitter.material_id());
            Ok(RibbonEmitterManifest {
                object_id: node.object_id(),
                name: node.name(),
                position: model_node_position(model, &node),
                height_above: emitter.height_above(),
                height_below: emitter.height_below(),
                alpha: emitter.alpha(),
                color: [color.x, color.y, color.z],
                lifespan: emitter.lifespan(),
                emission_rate: emitter.emission_rate(),
                rows: emitter.rows(),
                columns: emitter.columns(),
                material_id: emitter.material_id(),
                texture_slot: emitter.texture_slot(),
                filter_mode,
                texture,
                gravity: emitter.gravity(),
                sequence_windows: sequence_windows.clone(),
                global_sequence_durations_ms: global_sequence_durations_ms.clone(),
                height_above_track: scalar_track_manifest(&emitter.height_above_tracks())?,
                height_below_track: scalar_track_manifest(&emitter.height_below_tracks())?,
                alpha_track: scalar_track_manifest(&emitter.alpha_tracks())?,
                color_track: vector3_track_manifest(&emitter.color_tracks())?,
                texture_slot_track: unsigned_track_manifest(&emitter.texture_slot_tracks())?,
                visibility_track: scalar_track_manifest(&emitter.visibility_tracks())?,
            })
        })
        .collect()
}

fn ribbon_material_properties(
    model: &Model,
    texture_manifests: &[TextureManifest],
    material_id: u32,
) -> (String, Option<String>) {
    let Some(material) = model.materials(material_id as usize) else {
        return ("Blend".to_owned(), None);
    };
    let selected = material
        .layers_iter()
        .filter(|layer| is_classic_material_layer(layer))
        .find(|layer| {
            texture_manifests
                .get(layer_diffuse_texture_id(layer) as usize)
                .and_then(|texture| texture.png.as_ref())
                .is_some()
        });
    let layer = selected.or_else(|| {
        material
            .layers_iter()
            .find(|layer| is_classic_material_layer(layer))
    });
    let Some(layer) = layer else {
        return ("Blend".to_owned(), None);
    };
    let texture = texture_manifests
        .get(layer_diffuse_texture_id(&layer) as usize)
        .and_then(|texture| texture.png.clone());
    (format!("{:?}", layer.filter_mode()), texture)
}

fn is_building_background_texture(texture: &TextureManifest) -> bool {
    let normalized = texture
        .source_texture
        .replace('\\', "/")
        .to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "textures/background.blp" | "textures/background.dds" | "textures/background.tga"
    )
}

fn is_building_engine_plane_texture(texture: &TextureManifest) -> bool {
    texture.replaceable_id == 2
        || texture
            .source_texture
            .replace('\\', "/")
            .to_ascii_lowercase()
            .contains("replaceabletextures/teamglow/")
        || is_building_background_texture(texture)
}

fn material_is_building_engine_plane(
    model: &Model,
    material_id: usize,
    texture_manifests: &[TextureManifest],
) -> bool {
    let Some(material) = model.materials(material_id) else {
        return false;
    };
    let mut saw_layer = false;
    for layer in material
        .layers_iter()
        .filter(|layer| is_classic_material_layer(layer))
    {
        saw_layer = true;
        let Some(texture) = texture_manifests.get(layer_diffuse_texture_id(&layer) as usize) else {
            return false;
        };
        if !is_building_engine_plane_texture(texture) {
            return false;
        }
    }
    saw_layer
}

fn material_selected_diffuse_is_background(
    model: &Model,
    material_id: usize,
    texture_manifests: &[TextureManifest],
) -> bool {
    let Some(material) = model.materials(material_id) else {
        return false;
    };
    let selected = material
        .layers_iter()
        .filter(|layer| is_classic_material_layer(layer))
        .find(|layer| {
            texture_manifests
                .get(layer_diffuse_texture_id(layer) as usize)
                .and_then(|texture| texture.png.as_ref())
                .is_some()
        });
    let layer = selected.or_else(|| {
        material
            .layers_iter()
            .find(|layer| is_classic_material_layer(layer))
    });
    let Some(layer) = layer else {
        return false;
    };
    texture_manifests
        .get(layer_diffuse_texture_id(&layer) as usize)
        .is_some_and(is_building_background_texture)
}

fn model_node_position(model: &Model, node: &Node) -> [f32; 3] {
    model
        .pivot_points()
        .get(node.object_id() as usize)
        .map(|pivot| wc3_vec3(pivot.x, pivot.y, pivot.z))
        .unwrap_or([0.0; 3])
}

fn build_gltf(
    model: &Model,
    logical_path: &str,
    asset_name: &str,
    texture_indices: &[Option<usize>],
    texture_manifests: &[TextureManifest],
    omit_team_glow_geosets: bool,
) -> Result<GltfBuildOutput, Box<dyn Error>> {
    let mut binary = BinaryBuilder::default();
    let mut warnings = Vec::new();
    let skeleton = build_skeleton(model, &mut binary, &mut warnings)?;

    let mut meshes = Vec::new();
    let mut geoset_nodes = Vec::new();
    let mut geoset_node_by_index = BTreeMap::new();
    for (geoset_index, geoset) in model.geosets_iter().enumerate() {
        if geoset.vertex_positions().is_empty() || geoset.faces().is_empty() {
            continue;
        }
        let material_id = geoset.material_id() as usize;
        let is_background_quad = geoset.vertex_positions().len() == 4
            && geoset.faces().len() == 6
            && material_selected_diffuse_is_background(model, material_id, texture_manifests);
        if omit_team_glow_geosets
            && (material_is_building_engine_plane(model, material_id, texture_manifests)
                || is_background_quad)
        {
            warnings.push(format!(
                "omitting building engine-plane geoset {geoset_index}; Warcraft renders team glow/background planes with engine-specific billboard/decal semantics"
            ));
            continue;
        }

        let positions: Vec<[f32; 3]> = geoset
            .vertex_positions()
            .iter()
            .map(|p| wc3_vec3(p.x, p.y, p.z))
            .collect();
        let position_accessor = binary.push_vec3_f32(&positions, Some(GL_ARRAY_BUFFER), true);

        let normals = geoset.vertex_normals();
        let normal_accessor = (normals.len() == positions.len()).then(|| {
            let converted: Vec<[f32; 3]> =
                normals.iter().map(|n| wc3_vec3(n.x, n.y, n.z)).collect();
            binary.push_vec3_f32(&converted, Some(GL_ARRAY_BUFFER), false)
        });

        let texcoord_accessor = if geoset.texture_coordinate_sets_len() > 0 {
            let uvs = geoset.texture_coordinate_sets(0);
            if uvs.len() == positions.len() {
                let converted: Vec<[f32; 2]> = uvs.iter().map(|uv| [uv.x, uv.y]).collect();
                Some(binary.push_vec2_f32(&converted, Some(GL_ARRAY_BUFFER)))
            } else {
                None
            }
        } else {
            None
        };
        let index_accessor = binary.push_u16(geoset.faces(), Some(GL_ELEMENT_ARRAY_BUFFER));

        let mut attributes = serde_json::Map::new();
        attributes.insert("POSITION".into(), json!(position_accessor));
        if let Some(accessor) = normal_accessor {
            attributes.insert("NORMAL".into(), json!(accessor));
        }
        if let Some(accessor) = texcoord_accessor {
            attributes.insert("TEXCOORD_0".into(), json!(accessor));
        }
        if geoset.matrix_groups().iter().any(|&count| count > 4) {
            let max_influences = geoset.matrix_groups().iter().copied().max().unwrap_or(0);
            warnings.push(format!(
                "geoset {geoset_index} uses up to {max_influences} classic skin influences per vertex; truncating to four and renormalizing for Bevy glTF compatibility"
            ));
        }
        if let Some(skin) = build_geoset_skin(&geoset, &skeleton)? {
            let joints_accessor = binary.push_vec4_u16(&skin.joints_0, Some(GL_ARRAY_BUFFER));
            let weights_accessor = binary.push_vec4_f32(&skin.weights_0, Some(GL_ARRAY_BUFFER));
            attributes.insert("JOINTS_0".into(), json!(joints_accessor));
            attributes.insert("WEIGHTS_0".into(), json!(weights_accessor));
        }

        let primitive = json!({
            "attributes": attributes,
            "indices": index_accessor,
            "material": geoset.material_id(),
            "mode": 4,
            "extras": {
                "wc3Geoset": geoset_index,
                "wc3Lod": geoset.lod(),
            }
        });
        let mesh_index = meshes.len();
        meshes.push(json!({
            "name": format!("{asset_name}_geoset_{geoset_index}"),
            "primitives": [primitive],
        }));

        let node_index = 1 + skeleton.nodes.len() + geoset_nodes.len();
        let mut geoset_node = json!({
            "name": format!("{asset_name}_geoset_{geoset_index}"),
            "mesh": mesh_index,
            "scale": visibility_scale(default_geoset_alpha(model, geoset_index)?),
            "extras": {
                "wc3Geoset": geoset_index,
                "wc3Lod": geoset.lod(),
            }
        });
        if skeleton.skin.is_some() {
            geoset_node
                .as_object_mut()
                .expect("geoset node object")
                .insert("skin".into(), json!(0));
        }
        geoset_node_by_index.insert(geoset_index, node_index);
        geoset_nodes.push(geoset_node);
    }

    let animations = build_animations(
        model,
        &skeleton,
        &geoset_node_by_index,
        &mut binary,
        &mut warnings,
    )?;
    let (materials, material_warnings, uses_unlit) =
        build_materials(model, texture_indices, texture_manifests)?;
    warnings.extend(material_warnings);
    let images: Vec<Value> = texture_manifests
        .iter()
        .filter_map(|texture| texture.png.as_ref())
        .map(|png| {
            let file_name = Path::new(png)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(png);
            json!({ "uri": format!("../textures/{file_name}") })
        })
        .collect();
    let textures: Vec<Value> = (0..images.len())
        .map(|source| json!({ "sampler": 0, "source": source }))
        .collect();

    let mut model_children = skeleton.scene_roots.clone();
    model_children.extend(geoset_node_by_index.values().copied());
    let model_root = json!({
        "name": asset_name,
        "children": model_children,
    });
    let mut nodes = vec![model_root];
    nodes.extend(skeleton.nodes.iter().cloned());
    nodes.extend(geoset_nodes);
    let scene_nodes = vec![0usize];

    let mut root = json!({
        "asset": {
            "version": "2.0",
            "generator": "Castle Fight Native wc3 asset extractor"
        },
        "scene": 0,
        "scenes": [{ "nodes": scene_nodes }],
        "nodes": nodes,
        "meshes": meshes,
        "buffers": [{
            "uri": format!("{asset_name}.bin"),
            "byteLength": binary.bytes.len()
        }],
        "bufferViews": binary.views,
        "accessors": binary.accessors,
        "samplers": [{
            "magFilter": 9729,
            "minFilter": 9987,
            "wrapS": 10497,
            "wrapT": 10497
        }],
        "images": images,
        "textures": textures,
        "materials": materials,
        "animations": animations,
        "extras": {
            "wc3SourceModel": logical_path,
            "wc3Bones": model.bones_len(),
            "wc3AnimationSequences": model.sequences_len(),
            "wc3SmoothInterpolation": "source keys plus interval midpoints, linearized for glTF",
        }
    });
    if let Some(skin) = skeleton.skin {
        root.as_object_mut()
            .expect("gltf root object")
            .insert("skins".into(), json!([skin]));
    }
    if uses_unlit {
        root.as_object_mut()
            .expect("gltf root object")
            .insert("extensionsUsed".into(), json!(["KHR_materials_unlit"]));
    }

    Ok((root, binary.bytes, warnings))
}

#[derive(Default)]
struct SkeletonBuild {
    nodes: Vec<Value>,
    scene_roots: Vec<usize>,
    skin: Option<Value>,
    node_by_object: BTreeMap<u32, usize>,
    joint_by_object: BTreeMap<u32, u16>,
    rest_translation_by_object: BTreeMap<u32, [f32; 3]>,
    joint_nodes: Vec<usize>,
}

#[derive(Debug, Clone)]
struct SkeletonNodeInfo {
    object_id: u32,
    parent_id: u32,
    name: String,
    pivot: [f32; 3],
    flags: NodeFlag,
}

fn build_skeleton(
    model: &Model,
    binary: &mut BinaryBuilder,
    warnings: &mut Vec<String>,
) -> Result<SkeletonBuild, Box<dyn Error>> {
    let mut infos = BTreeMap::<u32, SkeletonNodeInfo>::new();
    for bone in model.bones_iter() {
        let node = bone.node();
        insert_skeleton_node(model, &node, &mut infos, warnings)?;
    }
    for helper in model.helpers_iter() {
        let node = helper.node();
        insert_skeleton_node(model, &node, &mut infos, warnings)?;
    }
    for emitter in model.sound_emitters_iter() {
        let node = emitter.node();
        insert_skeleton_node(model, &node, &mut infos, warnings)?;
    }
    for attachment in model.attachments_iter() {
        let node = attachment.node();
        insert_skeleton_node(model, &node, &mut infos, warnings)?;
    }
    for light in model.lights_iter() {
        let node = light.node();
        insert_skeleton_node(model, &node, &mut infos, warnings)?;
    }
    for emitter in model.particle_emitters_iter() {
        let node = emitter.node();
        insert_skeleton_node(model, &node, &mut infos, warnings)?;
    }
    for emitter in model.particle_emitters_2_iter() {
        let node = emitter.node();
        insert_skeleton_node(model, &node, &mut infos, warnings)?;
    }
    for emitter in model.ribbon_emitters_iter() {
        let node = emitter.node();
        insert_skeleton_node(model, &node, &mut infos, warnings)?;
    }
    for event in model.event_objects_iter() {
        let node = event.node();
        insert_skeleton_node(model, &node, &mut infos, warnings)?;
    }
    for shape in model.collision_shapes_iter() {
        let node = shape.node();
        insert_skeleton_node(model, &node, &mut infos, warnings)?;
    }
    for emitter in model.corn_emitters_iter() {
        let node = emitter.node();
        insert_skeleton_node(model, &node, &mut infos, warnings)?;
    }

    if infos.is_empty() {
        return Ok(SkeletonBuild::default());
    }

    let mut result = SkeletonBuild::default();
    for (ordinal, object_id) in infos.keys().copied().enumerate() {
        result.node_by_object.insert(object_id, ordinal + 1);
    }

    let mut children = BTreeMap::<usize, Vec<usize>>::new();
    for info in infos.values() {
        let node_index = result.node_by_object[&info.object_id];
        let parent_pivot = infos
            .get(&info.parent_id)
            .map(|parent| parent.pivot)
            .unwrap_or([0.0; 3]);
        let rest_translation = sub3(info.pivot, parent_pivot);
        result
            .rest_translation_by_object
            .insert(info.object_id, rest_translation);

        if info.parent_id != NO_PARENT {
            if let Some(parent_index) = result.node_by_object.get(&info.parent_id) {
                children.entry(*parent_index).or_default().push(node_index);
            } else {
                warnings.push(format!(
                    "node {} ({}) references parent object {} outside the exported bone/helper hierarchy; treating it as a root",
                    info.object_id, info.name, info.parent_id
                ));
                result.scene_roots.push(node_index);
            }
        } else {
            result.scene_roots.push(node_index);
        }
    }

    let light_extras = model
        .lights_iter()
        .map(|light| {
            let object_id = light.node().object_id();
            Ok((object_id, light_manifest(model, &light)?))
        })
        .collect::<Result<BTreeMap<_, _>, Box<dyn Error>>>()?;
    let attachment_extras = attachment_manifests(model)?
        .into_iter()
        .map(|attachment| (attachment.object_id, attachment))
        .collect::<BTreeMap<_, _>>();
    let model_particle_extras = model_particle_emitter_manifests(model)?
        .into_iter()
        .map(|emitter| (emitter.object_id, emitter))
        .collect::<BTreeMap<_, _>>();

    for info in infos.values() {
        let node_index = result.node_by_object[&info.object_id];
        let rest = result.rest_translation_by_object[&info.object_id];
        let mut value = json!({
            "name": info.name,
            "translation": rest,
            "extras": {
                "wc3ObjectId": info.object_id,
                "wc3NodeFlags": info.flags.0,
            }
        });
        if let Some(light) = light_extras.get(&info.object_id) {
            value["extras"]
                .as_object_mut()
                .expect("node extras object")
                .insert("wc3Light".into(), serde_json::to_value(light)?);
        }
        if let Some(attachment) = attachment_extras
            .get(&info.object_id)
            .filter(|attachment| attachment.gltf.is_some())
        {
            value["extras"]
                .as_object_mut()
                .expect("node extras object")
                .insert("wc3Attachment".into(), serde_json::to_value(attachment)?);
        }
        if let Some(emitter) = model_particle_extras
            .get(&info.object_id)
            .filter(|emitter| emitter.gltf.is_some())
        {
            value["extras"]
                .as_object_mut()
                .expect("node extras object")
                .insert(
                    "wc3ModelParticleEmitter".into(),
                    serde_json::to_value(emitter)?,
                );
        }
        if let Some(node_children) = children.remove(&node_index) {
            value
                .as_object_mut()
                .expect("skeleton node object")
                .insert("children".into(), json!(node_children));
        }
        result.nodes.push(value);
    }

    let mut inverse_bind = Vec::with_capacity(model.bones_len());
    for (joint_index, bone) in model.bones_iter().enumerate() {
        let node = bone.node();
        let object_id = node.object_id();
        let gltf_node = *result.node_by_object.get(&object_id).ok_or_else(|| {
            io::Error::other(format!(
                "bone object {object_id} missing from skeleton node map"
            ))
        })?;
        let joint_slot = u16::try_from(joint_index)
            .map_err(|_| io::Error::other("glTF exporter supports at most 65535 joints"))?;
        result.joint_by_object.insert(object_id, joint_slot);
        result.joint_nodes.push(gltf_node);
        let pivot = infos.get(&object_id).expect("bone descriptor exists").pivot;
        inverse_bind.push(inverse_translation_matrix(pivot));
    }
    if !inverse_bind.is_empty() {
        let inverse_bind_accessor = binary.push_mat4_f32(&inverse_bind);
        result.skin = Some(json!({
            "name": "wc3_skin",
            "inverseBindMatrices": inverse_bind_accessor,
            "joints": result.joint_nodes,
        }));
    }
    Ok(result)
}

fn insert_skeleton_node(
    model: &Model,
    node: &Node,
    infos: &mut BTreeMap<u32, SkeletonNodeInfo>,
    warnings: &mut Vec<String>,
) -> Result<(), Box<dyn Error>> {
    let object_id = node.object_id();
    let pivot = if let Some(pivot) = model.pivot_points().get(object_id as usize) {
        wc3_vec3(pivot.x, pivot.y, pivot.z)
    } else {
        warnings.push(format!(
            "node {} ({}) has no matching pivot point; using origin",
            object_id,
            node.name()
        ));
        [0.0; 3]
    };
    let info = SkeletonNodeInfo {
        object_id,
        parent_id: node.parent_id(),
        name: node.name(),
        pivot,
        flags: node.flags(),
    };
    if infos.insert(object_id, info).is_some() {
        return Err(io::Error::other(format!(
            "duplicate MDX node object id {object_id} in bone/helper hierarchy"
        ))
        .into());
    }
    Ok(())
}

struct GeosetSkin {
    joints_0: Vec<[u16; 4]>,
    weights_0: Vec<[f32; 4]>,
}

fn build_geoset_skin(
    geoset: &whiteout::mdx::Geoset,
    skeleton: &SkeletonBuild,
) -> Result<Option<GeosetSkin>, Box<dyn Error>> {
    if skeleton.joint_nodes.is_empty() {
        return Ok(None);
    }
    let vertex_count = geoset.vertex_positions().len();
    let skin = geoset.skin_data();
    if !skin.is_empty() {
        if skin.len() != vertex_count * 8 {
            return Err(io::Error::other(format!(
                "HD geoset skin has {} values for {vertex_count} vertices; expected {}",
                skin.len(),
                vertex_count * 8
            ))
            .into());
        }
        let mut joints = Vec::with_capacity(vertex_count);
        let mut weights = Vec::with_capacity(vertex_count);
        for vertex in skin.as_chunks::<8>().0 {
            let mut vertex_joints = [0u16; 4];
            let mut vertex_weights = [0.0f32; 4];
            for slot in 0..4 {
                let weight = vertex[slot + 4] as f32 / 255.0;
                let joint = vertex[slot] as usize;
                if weight > 0.0 && joint >= skeleton.joint_nodes.len() {
                    return Err(io::Error::other(format!(
                        "HD geoset references joint index {joint}, but model has {} joints",
                        skeleton.joint_nodes.len()
                    ))
                    .into());
                }
                vertex_joints[slot] = joint as u16;
                vertex_weights[slot] = weight;
            }
            joints.push(vertex_joints);
            weights.push(vertex_weights);
        }
        return Ok(Some(GeosetSkin {
            joints_0: joints,
            weights_0: weights,
        }));
    }

    if geoset.matrix_groups().is_empty() || geoset.vertex_groups().is_empty() {
        return Ok(None);
    }
    if geoset.vertex_groups().len() != vertex_count {
        return Err(io::Error::other(format!(
            "classic geoset has {} vertex groups for {vertex_count} vertices",
            geoset.vertex_groups().len()
        ))
        .into());
    }

    let mut group_offsets = Vec::with_capacity(geoset.matrix_groups().len());
    let mut offset = 0usize;
    for &count in geoset.matrix_groups() {
        group_offsets.push(offset);
        offset = offset
            .checked_add(count as usize)
            .ok_or_else(|| io::Error::other("matrix group offset overflow"))?;
    }
    if offset > geoset.matrix_indices().len() {
        return Err(io::Error::other(format!(
            "classic geoset matrix groups require {offset} indices but only {} are present",
            geoset.matrix_indices().len()
        ))
        .into());
    }

    let mut joints_0 = Vec::with_capacity(vertex_count);
    let mut weights_0 = Vec::with_capacity(vertex_count);
    for &group in geoset.vertex_groups() {
        let group = group as usize;
        let count = *geoset.matrix_groups().get(group).ok_or_else(|| {
            io::Error::other(format!("vertex references missing matrix group {group}"))
        })? as usize;
        if count == 0 {
            return Err(io::Error::other(format!(
                "classic matrix group {group} has no bone influences"
            ))
            .into());
        }
        let first = group_offsets[group];
        let matrix_ids = &geoset.matrix_indices()[first..first + count];
        let mut vertex_joints_0 = [0u16; 4];
        let mut vertex_weights_0 = [0.0f32; 4];
        let exported_count = count.min(4);
        let weight = 1.0 / exported_count as f32;
        for (slot, &object_id) in matrix_ids.iter().take(exported_count).enumerate() {
            let joint = skeleton.joint_by_object.get(&object_id).ok_or_else(|| {
                io::Error::other(format!(
                    "classic geoset matrix group references non-bone object id {object_id}"
                ))
            })?;
            vertex_joints_0[slot] = *joint;
            vertex_weights_0[slot] = weight;
        }
        joints_0.push(vertex_joints_0);
        weights_0.push(vertex_weights_0);
    }
    Ok(Some(GeosetSkin {
        joints_0,
        weights_0,
    }))
}

#[derive(Debug)]
struct Vec3Samples {
    times: Vec<f32>,
    values: Vec<[f32; 3]>,
    interpolation: &'static str,
}

#[derive(Debug)]
struct QuatSamples {
    times: Vec<f32>,
    values: Vec<[f32; 4]>,
    interpolation: &'static str,
}

#[derive(Debug, Clone, Copy)]
enum BuildingLifecycleAnimationRole {
    Birth,
    Stand,
    Death,
}

fn select_building_lifecycle_animations(
    animations: &[AnimationManifest],
    animation_properties: &[String],
) -> BuildingLifecycleAnimationManifest {
    BuildingLifecycleAnimationManifest {
        birth: select_building_lifecycle_animation(
            animations,
            animation_properties,
            BuildingLifecycleAnimationRole::Birth,
        ),
        stand: select_building_lifecycle_animation(
            animations,
            animation_properties,
            BuildingLifecycleAnimationRole::Stand,
        ),
        death: select_building_lifecycle_animation(
            animations,
            animation_properties,
            BuildingLifecycleAnimationRole::Death,
        ),
    }
}

fn select_building_lifecycle_animation(
    animations: &[AnimationManifest],
    animation_properties: &[String],
    role: BuildingLifecycleAnimationRole,
) -> Option<String> {
    let required = animation_properties
        .iter()
        .flat_map(|property| animation_words(property))
        .collect::<BTreeSet<_>>();

    animations
        .iter()
        .filter_map(|animation| {
            let words = animation_words(&animation.name);
            if !building_animation_matches_role(&words, role) {
                return None;
            }
            let words_set = words.iter().map(String::as_str).collect::<BTreeSet<_>>();
            let missing_required = required
                .iter()
                .filter(|property| !words_set.contains(property.as_str()))
                .count();
            let extra_words = words
                .iter()
                .filter(|word| {
                    !required.contains(*word)
                        && word.as_str() != building_animation_role_word(role)
                        && !word.chars().all(|character| character.is_ascii_digit())
                })
                .count();
            let looping_penalty = usize::from(
                !animation.non_looping && !matches!(role, BuildingLifecycleAnimationRole::Stand),
            );
            Some((
                missing_required,
                looping_penalty,
                extra_words,
                animation.name.to_ascii_lowercase(),
                animation.name.clone(),
            ))
        })
        .min()
        .map(|(_, _, _, _, name)| name)
}

fn animation_words(value: &str) -> Vec<String> {
    value
        .to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

fn building_animation_role_word(role: BuildingLifecycleAnimationRole) -> &'static str {
    match role {
        BuildingLifecycleAnimationRole::Birth => "birth",
        BuildingLifecycleAnimationRole::Stand => "stand",
        BuildingLifecycleAnimationRole::Death => "death",
    }
}

fn building_animation_matches_role(words: &[String], role: BuildingLifecycleAnimationRole) -> bool {
    let contains = |word: &str| words.iter().any(|candidate| candidate == word);
    match role {
        BuildingLifecycleAnimationRole::Birth => {
            contains("birth") && !contains("death") && !contains("decay") && !contains("portrait")
        }
        BuildingLifecycleAnimationRole::Stand => {
            words.first().is_some_and(|word| word == "stand")
                && !contains("work")
                && !contains("birth")
                && !contains("death")
                && !contains("decay")
                && !contains("portrait")
        }
        BuildingLifecycleAnimationRole::Death => {
            contains("death") && !contains("decay") && !contains("portrait")
        }
    }
}

fn animation_manifests_from_gltf(gltf: &Value) -> Result<Vec<AnimationManifest>, Box<dyn Error>> {
    let Some(animations) = gltf.get("animations").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    animations
        .iter()
        .map(|animation| {
            let name = animation
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| io::Error::other("emitted glTF animation has no name"))?;
            let extras = animation
                .get("extras")
                .and_then(Value::as_object)
                .ok_or_else(|| io::Error::other("emitted glTF animation has no WC3 extras"))?;
            let start_ms = extras
                .get("wc3StartMs")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| io::Error::other("emitted glTF animation has invalid wc3StartMs"))?;
            let end_ms = extras
                .get("wc3EndMs")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| io::Error::other("emitted glTF animation has invalid wc3EndMs"))?;
            let move_speed = extras
                .get("wc3MoveSpeed")
                .and_then(Value::as_f64)
                .ok_or_else(|| {
                    io::Error::other("emitted glTF animation has invalid wc3MoveSpeed")
                })? as f32;
            let non_looping = extras
                .get("wc3NonLooping")
                .and_then(Value::as_bool)
                .ok_or_else(|| {
                    io::Error::other("emitted glTF animation has invalid wc3NonLooping")
                })?;
            Ok(AnimationManifest {
                name: name.to_owned(),
                start_ms,
                end_ms,
                move_speed,
                non_looping,
            })
        })
        .collect()
}

#[derive(Debug)]
struct F32Samples {
    times: Vec<f32>,
    values: Vec<f32>,
}

fn build_animations(
    model: &Model,
    skeleton: &SkeletonBuild,
    geoset_node_by_index: &BTreeMap<usize, usize>,
    binary: &mut BinaryBuilder,
    warnings: &mut Vec<String>,
) -> Result<Vec<Value>, Box<dyn Error>> {
    let mut animations = Vec::new();
    let mut baked_global_sequences = false;

    for sequence in model.sequences_iter() {
        let start = sequence.interval_start();
        let end = sequence.interval_end();
        if end <= start {
            continue;
        }
        // WC3 global sequences keep their own clock while short Stand clips loop.
        // Keep sampling until the global motion completes before repeating the glTF clip.
        let output_duration = if sequence.flags() == SequenceFlag::NonLooping {
            end - start
        } else {
            model
                .global_sequences()
                .iter()
                .copied()
                .max()
                .unwrap_or(0)
                .max(end - start)
        };
        let mut samplers = Vec::new();
        let mut channels = Vec::new();

        for bone in model.bones_iter() {
            let node = bone.node();
            append_node_animation(
                model,
                &node,
                skeleton,
                start,
                end,
                output_duration,
                binary,
                &mut samplers,
                &mut channels,
                &mut baked_global_sequences,
            )?;
        }
        for helper in model.helpers_iter() {
            let node = helper.node();
            append_node_animation(
                model,
                &node,
                skeleton,
                start,
                end,
                output_duration,
                binary,
                &mut samplers,
                &mut channels,
                &mut baked_global_sequences,
            )?;
        }
        for emitter in model.sound_emitters_iter() {
            let node = emitter.node();
            append_node_animation(
                model,
                &node,
                skeleton,
                start,
                end,
                output_duration,
                binary,
                &mut samplers,
                &mut channels,
                &mut baked_global_sequences,
            )?;
        }
        for attachment in model.attachments_iter() {
            let node = attachment.node();
            append_node_animation(
                model,
                &node,
                skeleton,
                start,
                end,
                output_duration,
                binary,
                &mut samplers,
                &mut channels,
                &mut baked_global_sequences,
            )?;
        }
        for light in model.lights_iter() {
            let node = light.node();
            append_node_animation(
                model,
                &node,
                skeleton,
                start,
                end,
                output_duration,
                binary,
                &mut samplers,
                &mut channels,
                &mut baked_global_sequences,
            )?;
        }
        for emitter in model.particle_emitters_iter() {
            let node = emitter.node();
            append_node_animation(
                model,
                &node,
                skeleton,
                start,
                end,
                output_duration,
                binary,
                &mut samplers,
                &mut channels,
                &mut baked_global_sequences,
            )?;
        }
        for emitter in model.particle_emitters_2_iter() {
            let node = emitter.node();
            append_node_animation(
                model,
                &node,
                skeleton,
                start,
                end,
                output_duration,
                binary,
                &mut samplers,
                &mut channels,
                &mut baked_global_sequences,
            )?;
        }
        for emitter in model.ribbon_emitters_iter() {
            let node = emitter.node();
            append_node_animation(
                model,
                &node,
                skeleton,
                start,
                end,
                output_duration,
                binary,
                &mut samplers,
                &mut channels,
                &mut baked_global_sequences,
            )?;
        }
        for event in model.event_objects_iter() {
            let node = event.node();
            append_node_animation(
                model,
                &node,
                skeleton,
                start,
                end,
                output_duration,
                binary,
                &mut samplers,
                &mut channels,
                &mut baked_global_sequences,
            )?;
        }
        for shape in model.collision_shapes_iter() {
            let node = shape.node();
            append_node_animation(
                model,
                &node,
                skeleton,
                start,
                end,
                output_duration,
                binary,
                &mut samplers,
                &mut channels,
                &mut baked_global_sequences,
            )?;
        }
        for emitter in model.corn_emitters_iter() {
            let node = emitter.node();
            append_node_animation(
                model,
                &node,
                skeleton,
                start,
                end,
                output_duration,
                binary,
                &mut samplers,
                &mut channels,
                &mut baked_global_sequences,
            )?;
        }
        for geoset_animation in model.geoset_animations_iter() {
            let geoset_index = geoset_animation.geoset_id() as usize;
            let Some(&gltf_node) = geoset_node_by_index.get(&geoset_index) else {
                continue;
            };
            append_geoset_visibility_animation(
                model,
                &geoset_animation,
                gltf_node,
                start,
                end,
                output_duration,
                binary,
                &mut samplers,
                &mut channels,
                &mut baked_global_sequences,
            )?;
        }

        if !channels.is_empty() {
            animations.push(json!({
                "name": sequence.name(),
                "samplers": samplers,
                "channels": channels,
                "extras": {
                    "wc3StartMs": start,
                    "wc3EndMs": end,
                    "wc3MoveSpeed": sequence.move_speed(),
                    "wc3NonLooping": sequence.flags() == SequenceFlag::NonLooping,
                }
            }));
        }
    }

    if baked_global_sequences {
        warnings.push(
            "global-sequence transforms are baked into every glTF clip starting at global time zero; Warcraft keeps that clock running across sequence changes"
                .to_owned(),
        );
    }
    Ok(animations)
}

#[allow(clippy::too_many_arguments)]
fn append_node_animation(
    model: &Model,
    node: &Node,
    skeleton: &SkeletonBuild,
    start: u32,
    end: u32,
    output_duration: u32,
    binary: &mut BinaryBuilder,
    samplers: &mut Vec<Value>,
    channels: &mut Vec<Value>,
    baked_global_sequences: &mut bool,
) -> Result<(), Box<dyn Error>> {
    let object_id = node.object_id();
    let Some(&gltf_node) = skeleton.node_by_object.get(&object_id) else {
        return Ok(());
    };
    let rest_translation = skeleton
        .rest_translation_by_object
        .get(&object_id)
        .copied()
        .unwrap_or([0.0; 3]);

    let translation_track = node.translation_tracks();
    if translation_track.global_sequence_id() != NO_GLOBAL_SEQUENCE && translation_track.is_used() {
        *baked_global_sequences = true;
    }
    if let Some(samples) = sample_vec3_track(
        model,
        &translation_track,
        start,
        end,
        output_duration,
        [0.0; 3],
        |value| add3(rest_translation, wc3_vec3(value[0], value[1], value[2])),
    )? {
        push_vec3_animation_channel(
            binary,
            samplers,
            channels,
            gltf_node,
            "translation",
            samples,
        );
    }

    let rotation_track = node.rotation_tracks();
    if rotation_track.global_sequence_id() != NO_GLOBAL_SEQUENCE && rotation_track.is_used() {
        *baked_global_sequences = true;
    }
    if let Some(samples) = sample_quat_track(model, &rotation_track, start, end, output_duration)? {
        push_quat_animation_channel(binary, samplers, channels, gltf_node, samples);
    }

    let scaling_track = node.scaling_tracks();
    if scaling_track.global_sequence_id() != NO_GLOBAL_SEQUENCE && scaling_track.is_used() {
        *baked_global_sequences = true;
    }
    if let Some(samples) = sample_vec3_track(
        model,
        &scaling_track,
        start,
        end,
        output_duration,
        [1.0, 1.0, 1.0],
        |value| [value[0], value[2], value[1]],
    )? {
        push_vec3_animation_channel(binary, samplers, channels, gltf_node, "scale", samples);
    }
    Ok(())
}

fn default_geoset_alpha(model: &Model, geoset_index: usize) -> Result<f32, Box<dyn Error>> {
    let Some(animation) = model
        .geoset_animations_iter()
        .find(|animation| animation.geoset_id() as usize == geoset_index)
    else {
        return Ok(1.0);
    };
    let base_alpha = animation.alpha();
    let track = animation.alpha_tracks();
    if !track.is_used() || track.key_count() == 0 {
        return Ok(base_alpha);
    }
    validate_f32_track(&track)?;
    let Some(sequence) = model.sequences_iter().next() else {
        return Ok(base_alpha);
    };
    let start = sequence.interval_start();
    let Some(key_index) = track
        .timestamps()
        .iter()
        .position(|timestamp| *timestamp == start)
    else {
        return Ok(base_alpha);
    };
    Ok(f32_key(&track, key_index, 0))
}

fn visibility_scale(alpha: f32) -> [f32; 3] {
    let visible = if alpha > 0.001 { 1.0 } else { 0.0 };
    [visible; 3]
}

#[allow(clippy::too_many_arguments)]
fn append_geoset_visibility_animation(
    model: &Model,
    animation: &whiteout::mdx::GeosetAnimation,
    gltf_node: usize,
    start: u32,
    end: u32,
    output_duration: u32,
    binary: &mut BinaryBuilder,
    samplers: &mut Vec<Value>,
    channels: &mut Vec<Value>,
    baked_global_sequences: &mut bool,
) -> Result<(), Box<dyn Error>> {
    let track = animation.alpha_tracks();
    if track.global_sequence_id() != NO_GLOBAL_SEQUENCE && track.is_used() {
        *baked_global_sequences = true;
    }
    let duration = output_duration as f32 / 1000.0;
    let samples =
        if let Some(samples) = sample_f32_track(model, &track, start, end, output_duration)? {
            Vec3Samples {
                times: samples.times,
                values: samples.values.into_iter().map(visibility_scale).collect(),
                // glTF cannot animate primitive/material visibility. Use a binary node-scale
                // approximation rather than shrinking the mesh through partial alpha values.
                interpolation: "STEP",
            }
        } else {
            let scale = visibility_scale(animation.alpha());
            Vec3Samples {
                times: vec![0.0, duration],
                values: vec![scale, scale],
                interpolation: "STEP",
            }
        };
    push_vec3_animation_channel(binary, samplers, channels, gltf_node, "scale", samples);
    Ok(())
}

fn sample_f32_track(
    model: &Model,
    track: &TrackF32,
    sequence_start: u32,
    sequence_end: u32,
    output_duration: u32,
) -> Result<Option<F32Samples>, Box<dyn Error>> {
    if !track.is_used() || track.key_count() == 0 {
        return Ok(None);
    }
    validate_f32_track(track)?;
    let window = track_window(
        track.timestamps(),
        track.global_sequence_id(),
        model.global_sequences(),
        sequence_start,
        sequence_end,
        output_duration,
    );
    let Some(window) = window else {
        return Ok(None);
    };
    if window.indices.is_empty() {
        return Ok(None);
    }

    let local_times = sample_times(track.interpolation_type(), track.timestamps(), &window);
    let mut values = Vec::with_capacity(local_times.len());
    for &local_ms in &local_times {
        let frame = window.frame_for_local(local_ms);
        values.push(evaluate_f32(
            track,
            &window.indices,
            window.track_start,
            window.track_end,
            frame,
        ));
    }
    let mut times: Vec<f32> = local_times
        .iter()
        .map(|time| *time as f32 / 1000.0)
        .collect();
    close_extended_loop(
        &mut times,
        &mut values,
        sequence_end - sequence_start,
        output_duration,
    );
    Ok(Some(F32Samples { times, values }))
}

fn sample_vec3_track(
    model: &Model,
    track: &TrackVector3f,
    sequence_start: u32,
    sequence_end: u32,
    output_duration: u32,
    default: [f32; 3],
    convert: impl Fn([f32; 3]) -> [f32; 3],
) -> Result<Option<Vec3Samples>, Box<dyn Error>> {
    if !track.is_used() || track.key_count() == 0 {
        return Ok(None);
    }
    validate_vec3_track(track)?;
    let window = track_window(
        track.timestamps(),
        track.global_sequence_id(),
        model.global_sequences(),
        sequence_start,
        sequence_end,
        output_duration,
    );
    let Some(window) = window else {
        return Ok(None);
    };
    if window.indices.is_empty() {
        return Ok(None);
    }

    let local_times = sample_times(track.interpolation_type(), track.timestamps(), &window);
    let mut values = Vec::with_capacity(local_times.len());
    for &local_ms in &local_times {
        let frame = window.frame_for_local(local_ms);
        let value = evaluate_vec3(
            track,
            &window.indices,
            window.track_start,
            window.track_end,
            frame,
            default,
        );
        values.push(convert(value));
    }
    let mut times: Vec<f32> = local_times
        .iter()
        .map(|time| *time as f32 / 1000.0)
        .collect();
    close_extended_loop(
        &mut times,
        &mut values,
        sequence_end - sequence_start,
        output_duration,
    );
    Ok(Some(Vec3Samples {
        times,
        values,
        interpolation: if track.interpolation_type() == InterpolationType::None {
            "STEP"
        } else {
            "LINEAR"
        },
    }))
}

fn sample_quat_track(
    model: &Model,
    track: &TrackQuaternion,
    sequence_start: u32,
    sequence_end: u32,
    output_duration: u32,
) -> Result<Option<QuatSamples>, Box<dyn Error>> {
    if !track.is_used() || track.key_count() == 0 {
        return Ok(None);
    }
    validate_quat_track(track)?;
    let window = track_window(
        track.timestamps(),
        track.global_sequence_id(),
        model.global_sequences(),
        sequence_start,
        sequence_end,
        output_duration,
    );
    let Some(window) = window else {
        return Ok(None);
    };
    if window.indices.is_empty() {
        return Ok(None);
    }

    let local_times = sample_times(track.interpolation_type(), track.timestamps(), &window);
    let mut values = Vec::with_capacity(local_times.len());
    for &local_ms in &local_times {
        let frame = window.frame_for_local(local_ms);
        let value = evaluate_quat(
            track,
            &window.indices,
            window.track_start,
            window.track_end,
            frame,
        );
        values.push(wc3_quat(value));
    }
    let mut times: Vec<f32> = local_times
        .iter()
        .map(|time| *time as f32 / 1000.0)
        .collect();
    close_extended_loop(
        &mut times,
        &mut values,
        sequence_end - sequence_start,
        output_duration,
    );
    Ok(Some(QuatSamples {
        times,
        values,
        interpolation: if track.interpolation_type() == InterpolationType::None {
            "STEP"
        } else {
            "LINEAR"
        },
    }))
}

fn close_extended_loop<T: Clone>(
    times: &mut Vec<f32>,
    values: &mut Vec<T>,
    sequence_duration: u32,
    output_duration: u32,
) {
    if output_duration > sequence_duration
        && let Some(first) = values.first().cloned()
    {
        // Distinct WC3 global periods rarely share a short common multiple. A short
        // return to the initial pose avoids a visible reset at the glTF loop seam.
        times.push((output_duration + 500) as f32 / 1000.0);
        values.push(first);
    }
}

#[derive(Debug)]
struct TrackWindow {
    indices: Vec<usize>,
    track_start: u32,
    track_end: u32,
    clip_duration: u32,
    sequence_duration: u32,
    global_duration: Option<u32>,
    sequence_start: u32,
}

impl TrackWindow {
    fn frame_for_local(&self, local_ms: u32) -> u32 {
        if let Some(duration) = self.global_duration {
            if duration == 0 {
                0
            } else {
                local_ms % duration
            }
        } else {
            let local = if local_ms == self.sequence_duration {
                local_ms
            } else {
                local_ms % self.sequence_duration.max(1)
            };
            self.sequence_start.saturating_add(local)
        }
    }
}

fn track_window(
    timestamps: &[u32],
    global_sequence_id: u32,
    global_sequences: &[u32],
    sequence_start: u32,
    sequence_end: u32,
    output_duration: u32,
) -> Option<TrackWindow> {
    let sequence_duration = sequence_end.checked_sub(sequence_start)?;
    let clip_duration = output_duration.max(sequence_duration);
    if global_sequence_id != NO_GLOBAL_SEQUENCE {
        let duration = *global_sequences.get(global_sequence_id as usize)?;
        if duration == 0 {
            return None;
        }
        let mut indices: Vec<_> = timestamps
            .iter()
            .enumerate()
            .filter_map(|(index, &frame)| (frame <= duration).then_some(index))
            .collect();
        if indices.is_empty() && timestamps.first().is_some_and(|frame| *frame > duration) {
            indices.push(0);
        }
        Some(TrackWindow {
            indices,
            track_start: 0,
            track_end: duration,
            clip_duration,
            sequence_duration,
            global_duration: Some(duration),
            sequence_start,
        })
    } else {
        let indices = timestamps
            .iter()
            .enumerate()
            .filter_map(|(index, &frame)| {
                (frame >= sequence_start && frame <= sequence_end).then_some(index)
            })
            .collect::<Vec<_>>();
        if indices.is_empty() {
            return None;
        }
        Some(TrackWindow {
            indices,
            track_start: sequence_start,
            track_end: sequence_end,
            clip_duration,
            sequence_duration,
            global_duration: None,
            sequence_start,
        })
    }
}

fn sample_times(
    interpolation: InterpolationType,
    timestamps: &[u32],
    window: &TrackWindow,
) -> Vec<u32> {
    if window.indices.len() == 1 {
        return vec![0, window.clip_duration];
    }
    let mut times = std::collections::BTreeSet::new();
    times.insert(0);
    times.insert(window.clip_duration);

    if let Some(global_duration) = window.global_duration {
        let mut cycle = 0u32;
        while cycle <= window.clip_duration {
            for &index in &window.indices {
                let frame = timestamps[index].min(global_duration);
                let local = cycle.saturating_add(frame);
                if local <= window.clip_duration {
                    times.insert(local);
                }
            }
            match cycle.checked_add(global_duration) {
                Some(next) if next > cycle => cycle = next,
                _ => break,
            }
        }
    } else {
        let mut cycle = 0u32;
        while cycle <= window.clip_duration {
            for &index in &window.indices {
                let local =
                    cycle.saturating_add(timestamps[index].saturating_sub(window.sequence_start));
                if local <= window.clip_duration {
                    times.insert(local);
                }
            }
            match cycle.checked_add(window.sequence_duration) {
                Some(next) if next > cycle => cycle = next,
                _ => break,
            }
        }
    }

    if matches!(
        interpolation,
        InterpolationType::Hermite | InterpolationType::Bezier
    ) {
        let base_times: Vec<_> = times.iter().copied().collect();
        for pair in base_times.windows(2) {
            let midpoint = pair[0] + (pair[1] - pair[0]) / 2;
            if midpoint != pair[0] && midpoint != pair[1] {
                times.insert(midpoint);
            }
        }
    }
    times.into_iter().collect()
}

fn evaluate_f32(track: &TrackF32, indices: &[usize], start: u32, end: u32, frame: u32) -> f32 {
    if indices.len() == 1 {
        return f32_key(track, indices[0], 0);
    }
    let (a, b, t) = interpolation_pair(track.timestamps(), indices, start, end, frame);
    let va = f32_key(track, a, 0);
    let vb = f32_key(track, b, 0);
    match track.interpolation_type() {
        InterpolationType::None => va,
        InterpolationType::Linear => va + (vb - va) * t,
        InterpolationType::Hermite => {
            let out_tangent = f32_key(track, a, 2);
            let in_tangent = f32_key(track, b, 1);
            hermite(va, out_tangent, in_tangent, vb, t)
        }
        InterpolationType::Bezier => {
            let out_tangent = f32_key(track, a, 2);
            let in_tangent = f32_key(track, b, 1);
            bezier(va, out_tangent, in_tangent, vb, t)
        }
    }
}

fn evaluate_vec3(
    track: &TrackVector3f,
    indices: &[usize],
    start: u32,
    end: u32,
    frame: u32,
    default: [f32; 3],
) -> [f32; 3] {
    if indices.is_empty() {
        return default;
    }
    if indices.len() == 1 {
        return vec3_key(track, indices[0], 0);
    }
    let (a, b, t) = interpolation_pair(track.timestamps(), indices, start, end, frame);
    let va = vec3_key(track, a, 0);
    let vb = vec3_key(track, b, 0);
    match track.interpolation_type() {
        InterpolationType::None => va,
        InterpolationType::Linear => lerp3(va, vb, t),
        InterpolationType::Hermite => {
            let out_tangent = vec3_key(track, a, 2);
            let in_tangent = vec3_key(track, b, 1);
            hermite3(va, out_tangent, in_tangent, vb, t)
        }
        InterpolationType::Bezier => {
            let out_tangent = vec3_key(track, a, 2);
            let in_tangent = vec3_key(track, b, 1);
            bezier3(va, out_tangent, in_tangent, vb, t)
        }
    }
}

fn evaluate_quat(
    track: &TrackQuaternion,
    indices: &[usize],
    start: u32,
    end: u32,
    frame: u32,
) -> [f32; 4] {
    if indices.len() == 1 {
        return normalize_quat(quat_key(track, indices[0], 0));
    }
    let (a, b, t) = interpolation_pair(track.timestamps(), indices, start, end, frame);
    let qa = normalize_quat(quat_key(track, a, 0));
    let qb = normalize_quat(quat_key(track, b, 0));
    match track.interpolation_type() {
        InterpolationType::None => qa,
        InterpolationType::Linear => slerp_quat(qa, qb, t),
        InterpolationType::Hermite | InterpolationType::Bezier => {
            let out_tangent = normalize_quat(quat_key(track, a, 2));
            let in_tangent = normalize_quat(quat_key(track, b, 1));
            sqlerp_quat(qa, out_tangent, in_tangent, qb, t)
        }
    }
}

fn interpolation_pair(
    timestamps: &[u32],
    indices: &[usize],
    sequence_start: u32,
    sequence_end: u32,
    frame: u32,
) -> (usize, usize, f32) {
    let first = indices[0];
    let last = *indices.last().expect("indices nonempty");
    let (start_index, end_index) = if frame < timestamps[first] || frame >= timestamps[last] {
        (last, first)
    } else {
        let mut pair = (last, first);
        for window in indices.windows(2) {
            if timestamps[window[1]] > frame {
                pair = (window[0], window[1]);
                break;
            }
        }
        pair
    };

    let mut start_frame = timestamps[start_index] as i64;
    let end_frame = timestamps[end_index] as i64;
    let mut between = end_frame - start_frame;
    if between < 0 {
        between += (sequence_end - sequence_start) as i64;
        if (frame as i64) < start_frame {
            start_frame = end_frame;
        }
    }
    let t = if between == 0 {
        0.0
    } else {
        (frame as i64 - start_frame) as f32 / between as f32
    };
    (start_index, end_index, t)
}

fn f32_key(track: &TrackF32, key: usize, component: usize) -> f32 {
    let stride = if matches!(
        track.interpolation_type(),
        InterpolationType::Hermite | InterpolationType::Bezier
    ) {
        3
    } else {
        1
    };
    track.keys()[key * stride + component.min(stride - 1)]
}

fn u32_key(track: &TrackU32, key: usize, component: usize) -> u32 {
    let stride = if matches!(
        track.interpolation_type(),
        InterpolationType::Hermite | InterpolationType::Bezier
    ) {
        3
    } else {
        1
    };
    track.keys()[key * stride + component.min(stride - 1)]
}

fn vec3_key(track: &TrackVector3f, key: usize, component: usize) -> [f32; 3] {
    let stride = if matches!(
        track.interpolation_type(),
        InterpolationType::Hermite | InterpolationType::Bezier
    ) {
        3
    } else {
        1
    };
    let value = track.keys()[key * stride + component.min(stride - 1)];
    [value.x, value.y, value.z]
}

fn quat_key(track: &TrackQuaternion, key: usize, component: usize) -> [f32; 4] {
    let stride = if matches!(
        track.interpolation_type(),
        InterpolationType::Hermite | InterpolationType::Bezier
    ) {
        3
    } else {
        1
    };
    let value = track.keys()[key * stride + component.min(stride - 1)];
    [value.x, value.y, value.z, value.w]
}

fn validate_f32_track(track: &TrackF32) -> Result<(), Box<dyn Error>> {
    let stride = if matches!(
        track.interpolation_type(),
        InterpolationType::Hermite | InterpolationType::Bezier
    ) {
        3
    } else {
        1
    };
    validate_track_layout(
        track.key_count(),
        track.timestamps().len(),
        track.keys().len(),
        stride,
    )
}

fn validate_u32_track(track: &TrackU32) -> Result<(), Box<dyn Error>> {
    let stride = if matches!(
        track.interpolation_type(),
        InterpolationType::Hermite | InterpolationType::Bezier
    ) {
        3
    } else {
        1
    };
    validate_track_layout(
        track.key_count(),
        track.timestamps().len(),
        track.keys().len(),
        stride,
    )
}

fn validate_vec3_track(track: &TrackVector3f) -> Result<(), Box<dyn Error>> {
    let stride = if matches!(
        track.interpolation_type(),
        InterpolationType::Hermite | InterpolationType::Bezier
    ) {
        3
    } else {
        1
    };
    validate_track_layout(
        track.key_count(),
        track.timestamps().len(),
        track.keys().len(),
        stride,
    )
}

fn validate_quat_track(track: &TrackQuaternion) -> Result<(), Box<dyn Error>> {
    let stride = if matches!(
        track.interpolation_type(),
        InterpolationType::Hermite | InterpolationType::Bezier
    ) {
        3
    } else {
        1
    };
    validate_track_layout(
        track.key_count(),
        track.timestamps().len(),
        track.keys().len(),
        stride,
    )
}

fn validate_track_layout(
    key_count: usize,
    timestamps: usize,
    values: usize,
    stride: usize,
) -> Result<(), Box<dyn Error>> {
    if timestamps != key_count || values != key_count * stride {
        return Err(io::Error::other(format!(
            "malformed MDX animation track: keyCount={key_count}, timestamps={timestamps}, values={values}, stride={stride}"
        ))
        .into());
    }
    Ok(())
}

fn push_vec3_animation_channel(
    binary: &mut BinaryBuilder,
    samplers: &mut Vec<Value>,
    channels: &mut Vec<Value>,
    node: usize,
    path: &str,
    samples: Vec3Samples,
) {
    let input = binary.push_scalar_f32(&samples.times, true);
    let output = binary.push_vec3_f32(&samples.values, None, false);
    let sampler = samplers.len();
    samplers.push(json!({
        "input": input,
        "output": output,
        "interpolation": samples.interpolation,
    }));
    channels.push(json!({
        "sampler": sampler,
        "target": { "node": node, "path": path }
    }));
}

fn push_quat_animation_channel(
    binary: &mut BinaryBuilder,
    samplers: &mut Vec<Value>,
    channels: &mut Vec<Value>,
    node: usize,
    samples: QuatSamples,
) {
    let input = binary.push_scalar_f32(&samples.times, true);
    let output = binary.push_vec4_f32(&samples.values, None);
    let sampler = samplers.len();
    samplers.push(json!({
        "input": input,
        "output": output,
        "interpolation": samples.interpolation,
    }));
    channels.push(json!({
        "sampler": sampler,
        "target": { "node": node, "path": "rotation" }
    }));
}

fn wc3_vec3(x: f32, y: f32, z: f32) -> [f32; 3] {
    [x, z, -y]
}

fn model_overhead_position(model: &Model) -> Option<[f32; 3]> {
    fn score(name: &str) -> Option<u8> {
        let name = name.to_ascii_lowercase();
        if name.contains("overhead") {
            Some(0)
        } else if name.contains("head") {
            Some(1)
        } else {
            None
        }
    }

    let attachments = model.attachments_iter().filter_map(|attachment| {
        let node = attachment.node();
        score(&node.name()).map(|score| (score, model_node_position(model, &node)))
    });
    let helpers = model.helpers_iter().filter_map(|helper| {
        let node = helper.node();
        score(&node.name()).map(|score| (score, model_node_position(model, &node)))
    });
    attachments
        .chain(helpers)
        .min_by_key(|(score, _)| *score)
        .map(|(_, position)| position)
}

fn wc3_quat(value: [f32; 4]) -> [f32; 4] {
    normalize_quat([value[0], value[2], -value[1], value[3]])
}

fn inverse_translation_matrix(pivot: [f32; 3]) -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -pivot[0], -pivot[1],
        -pivot[2], 1.0,
    ]
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn hermite(a: f32, b: f32, c: f32, d: f32, t: f32) -> f32 {
    let t2 = t * t;
    let f1 = t2 * (2.0 * t - 3.0) + 1.0;
    let f2 = t2 * (t - 2.0) + t;
    let f3 = t2 * (t - 1.0);
    let f4 = t2 * (3.0 - 2.0 * t);
    a * f1 + b * f2 + c * f3 + d * f4
}

fn bezier(a: f32, b: f32, c: f32, d: f32, t: f32) -> f32 {
    let inv = 1.0 - t;
    let f1 = inv * inv * inv;
    let f2 = 3.0 * t * inv * inv;
    let f3 = 3.0 * t * t * inv;
    let f4 = t * t * t;
    a * f1 + b * f2 + c * f3 + d * f4
}

fn hermite3(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3], t: f32) -> [f32; 3] {
    let t2 = t * t;
    let f1 = t2 * (2.0 * t - 3.0) + 1.0;
    let f2 = t2 * (t - 2.0) + t;
    let f3 = t2 * (t - 1.0);
    let f4 = t2 * (3.0 - 2.0 * t);
    [
        a[0] * f1 + b[0] * f2 + c[0] * f3 + d[0] * f4,
        a[1] * f1 + b[1] * f2 + c[1] * f3 + d[1] * f4,
        a[2] * f1 + b[2] * f2 + c[2] * f3 + d[2] * f4,
    ]
}

fn bezier3(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3], t: f32) -> [f32; 3] {
    let inv = 1.0 - t;
    let f1 = inv * inv * inv;
    let f2 = 3.0 * t * inv * inv;
    let f3 = 3.0 * t * t * inv;
    let f4 = t * t * t;
    [
        a[0] * f1 + b[0] * f2 + c[0] * f3 + d[0] * f4,
        a[1] * f1 + b[1] * f2 + c[1] * f3 + d[1] * f4,
        a[2] * f1 + b[2] * f2 + c[2] * f3 + d[2] * f4,
    ]
}

fn normalize_quat(mut q: [f32; 4]) -> [f32; 4] {
    let length = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if length <= f32::EPSILON {
        return [0.0, 0.0, 0.0, 1.0];
    }
    for component in &mut q {
        *component /= length;
    }
    q
}

fn slerp_quat(a: [f32; 4], mut b: [f32; 4], t: f32) -> [f32; 4] {
    let a = normalize_quat(a);
    b = normalize_quat(b);
    let mut dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    if dot < 0.0 {
        dot = -dot;
        for component in &mut b {
            *component = -*component;
        }
    }
    if dot > 0.9995 {
        return normalize_quat([
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
            a[3] + (b[3] - a[3]) * t,
        ]);
    }
    let theta_0 = dot.clamp(-1.0, 1.0).acos();
    let sin_theta_0 = theta_0.sin();
    if sin_theta_0.abs() <= f32::EPSILON {
        return a;
    }
    let theta = theta_0 * t;
    let s0 = (theta_0 - theta).sin() / sin_theta_0;
    let s1 = theta.sin() / sin_theta_0;
    normalize_quat([
        a[0] * s0 + b[0] * s1,
        a[1] * s0 + b[1] * s1,
        a[2] * s0 + b[2] * s1,
        a[3] * s0 + b[3] * s1,
    ])
}

fn sqlerp_quat(a: [f32; 4], b: [f32; 4], c: [f32; 4], d: [f32; 4], t: f32) -> [f32; 4] {
    let first = slerp_quat(a, d, t);
    let second = slerp_quat(b, c, t);
    slerp_quat(first, second, 2.0 * t * (1.0 - t))
}

fn layer_diffuse_texture_id(layer: &Layer) -> u32 {
    layer
        .sub_textures_iter()
        .find(|sub_texture| sub_texture.slot() == LayerSlotType::DiffuseMap)
        .map_or_else(
            || layer.texture_id(),
            |sub_texture| sub_texture.texture_id(),
        )
}

fn layer_diffuse_texture_id_track(
    layer: &Layer,
) -> Result<Option<UnsignedTrackManifest>, Box<dyn Error>> {
    if let Some(sub_texture) = layer
        .sub_textures_iter()
        .find(|sub_texture| sub_texture.slot() == LayerSlotType::DiffuseMap)
    {
        unsigned_track_manifest(&sub_texture.tracks())
    } else {
        unsigned_track_manifest(&layer.texture_id_tracks())
    }
}

fn texture_file_name(texture: &TextureManifest) -> Option<String> {
    let png = texture.png.as_deref()?;
    Path::new(png)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
}

fn layer_diffuse_uses_replaceable(model: &Model, layer: &Layer, replaceable_id: u32) -> bool {
    model
        .textures(layer_diffuse_texture_id(layer) as usize)
        .is_some_and(|texture| texture.replaceable_id() == replaceable_id)
}

fn layer_static_uses_replaceable(model: &Model, layer: &Layer, replaceable_id: u32) -> bool {
    let texture_matches = |texture_id: u32| {
        model
            .textures(texture_id as usize)
            .is_some_and(|texture| texture.replaceable_id() == replaceable_id)
    };
    texture_matches(layer.texture_id())
        || layer
            .sub_textures_iter()
            .any(|sub_texture| texture_matches(sub_texture.texture_id()))
}

#[cfg(test)]
fn layer_uses_replaceable(model: &Model, layer: &Layer, replaceable_id: u32) -> bool {
    let texture_matches = |texture_id: u32| {
        model
            .textures(texture_id as usize)
            .is_some_and(|texture| texture.replaceable_id() == replaceable_id)
    };
    if layer_static_uses_replaceable(model, layer, replaceable_id)
        || layer
            .texture_id_tracks()
            .keys()
            .iter()
            .copied()
            .any(texture_matches)
    {
        return true;
    }
    layer.sub_textures_iter().any(|sub_texture| {
        sub_texture
            .tracks()
            .keys()
            .iter()
            .copied()
            .any(texture_matches)
    })
}

fn build_materials(
    model: &Model,
    texture_indices: &[Option<usize>],
    texture_manifests: &[TextureManifest],
) -> Result<MaterialBuildOutput, Box<dyn Error>> {
    let mut result = Vec::with_capacity(model.materials_len());
    let mut warnings = Vec::new();
    let mut uses_unlit = false;

    for (material_index, material) in model.materials_iter().enumerate() {
        let classic_layer_count = material
            .layers_iter()
            .filter(|layer| is_classic_material_layer(layer))
            .count();
        if classic_layer_count > 1 {
            warnings.push(format!(
                "material {material_index} has {classic_layer_count} Classic/SD WC3 layers; glTF uses one representative layer"
            ));
        }
        let selected = material
            .layers_iter()
            .filter(|layer| is_classic_material_layer(layer))
            .find(|layer| {
                texture_indices
                    .get(layer_diffuse_texture_id(layer) as usize)
                    .and_then(|index| *index)
                    .is_some()
            });
        let fallback = material
            .layers_iter()
            .find(|layer| is_classic_material_layer(layer));
        let layer = selected.or(fallback);

        let mut pbr = json!({
            "metallicFactor": 0.0,
            "roughnessFactor": 1.0
        });
        let mut alpha_mode = "OPAQUE";
        let mut double_sided = false;
        let mut extensions = serde_json::Map::new();
        let mut extras = serde_json::Map::new();
        extras.insert("wc3PriorityPlane".into(), json!(material.priority_plane()));

        let has_team_color_underlay = material
            .layers_iter()
            .filter(|layer| is_classic_material_layer(layer))
            .any(|layer| layer_static_uses_replaceable(model, &layer, 1));
        let has_team_glow_layer = layer
            .as_ref()
            .is_some_and(|layer| layer_diffuse_uses_replaceable(model, layer, 2));
        if has_team_color_underlay {
            extras.insert("wc3TeamColorUnderlay".into(), json!(true));
        }
        if has_team_glow_layer {
            extras.insert("wc3TeamGlowLayer".into(), json!(true));
        }

        if let Some(layer) = layer {
            let texture_id = layer_diffuse_texture_id(&layer) as usize;
            let layer_alpha = layer.alpha().clamp(0.0, 1.0);
            if let Some(Some(gltf_texture)) = texture_indices.get(texture_id) {
                let pbr = pbr.as_object_mut().expect("pbr object");
                pbr.insert("baseColorTexture".into(), json!({ "index": gltf_texture }));
                pbr.insert(
                    "baseColorFactor".into(),
                    json!([1.0, 1.0, 1.0, layer_alpha]),
                );
            } else if model
                .textures(texture_id)
                .is_some_and(|texture| texture.replaceable_id() != 0)
            {
                pbr.as_object_mut().expect("pbr object").insert(
                    "baseColorFactor".into(),
                    json!([0.85, 0.12, 0.12, layer_alpha]),
                );
            } else {
                pbr.as_object_mut().expect("pbr object").insert(
                    "baseColorFactor".into(),
                    json!([1.0, 1.0, 1.0, layer_alpha]),
                );
            }

            let alpha_track = scalar_track_manifest(&layer.alpha_tracks())?;
            let texture_id_track = layer_diffuse_texture_id_track(&layer)?;
            extras.insert("wc3LayerAlpha".into(), json!(layer_alpha));
            if let Some(alpha_track) = alpha_track {
                extras.insert("wc3AlphaTrack".into(), json!(alpha_track));
            }
            if let Some(texture_id_track) = texture_id_track {
                let mut texture_ids = texture_id_track.values.clone();
                texture_ids.push(texture_id as u32);
                texture_ids.sort_unstable();
                texture_ids.dedup();
                let texture_paths = texture_ids
                    .into_iter()
                    .filter_map(|texture_id| {
                        let texture = texture_manifests.get(texture_id as usize)?;
                        let file_name = texture_file_name(texture)?;
                        Some(json!({
                            "textureId": texture_id,
                            "fileName": file_name,
                        }))
                    })
                    .collect::<Vec<_>>();
                extras.insert("wc3TextureId".into(), json!(texture_id));
                extras.insert("wc3TextureIdTrack".into(), json!(texture_id_track));
                extras.insert("wc3TexturePaths".into(), json!(texture_paths));
            }
            if extras.contains_key("wc3AlphaTrack") || extras.contains_key("wc3TextureIdTrack") {
                let sequence_windows = model
                    .sequences_iter()
                    .map(|sequence| ParticleEmitterSequenceManifest {
                        name: sequence.name(),
                        start_ms: sequence.interval_start(),
                        end_ms: sequence.interval_end(),
                        non_looping: sequence.flags() == SequenceFlag::NonLooping,
                    })
                    .collect::<Vec<_>>();
                extras.insert("wc3SequenceWindows".into(), json!(sequence_windows));
                extras.insert(
                    "wc3GlobalSequenceDurationsMs".into(),
                    json!(model.global_sequences()),
                );
            }

            let texture_has_transparency = texture_manifests
                .get(texture_id)
                .is_some_and(|texture| texture.has_transparency);
            alpha_mode =
                gltf_alpha_mode(layer.filter_mode(), layer.alpha(), texture_has_transparency);
            double_sided = layer.shading_flags().contains(LayerShadingFlag::TWO_SIDED);
            let unlit = layer.shading_flags().contains(LayerShadingFlag::UNSHADED)
                || layer.shading_flags().contains(LayerShadingFlag::UNLIT);
            if unlit {
                extensions.insert("KHR_materials_unlit".into(), json!({}));
                uses_unlit = true;
            }
            extras.insert(
                "wc3FilterMode".into(),
                json!(format!("{:?}", layer.filter_mode())),
            );
            extras.insert("wc3LayerCount".into(), json!(classic_layer_count));
        }

        let mut value = json!({
            "name": format!("wc3_material_{material_index}"),
            "pbrMetallicRoughness": pbr,
            "alphaMode": alpha_mode,
            "doubleSided": double_sided,
            "extras": extras,
        });
        if alpha_mode == "MASK" {
            value
                .as_object_mut()
                .expect("material object")
                .insert("alphaCutoff".into(), json!(0.5));
        }
        if !extensions.is_empty() {
            value
                .as_object_mut()
                .expect("material object")
                .insert("extensions".into(), Value::Object(extensions));
        }
        result.push(value);
    }

    Ok((result, warnings, uses_unlit))
}

fn gltf_alpha_mode(
    filter_mode: LayerFilterMode,
    layer_alpha: f32,
    texture_has_transparency: bool,
) -> &'static str {
    match filter_mode {
        LayerFilterMode::None if layer_alpha < 0.999 => "BLEND",
        LayerFilterMode::None if texture_has_transparency => "MASK",
        LayerFilterMode::None => "OPAQUE",
        LayerFilterMode::Transparent => "MASK",
        _ => "BLEND",
    }
}

#[derive(Default)]
struct BinaryBuilder {
    bytes: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl BinaryBuilder {
    fn push_vec3_f32(&mut self, values: &[[f32; 3]], target: Option<u32>, bounds: bool) -> usize {
        self.align4();
        let offset = self.bytes.len();
        for value in values {
            for component in value {
                self.bytes.extend_from_slice(&component.to_le_bytes());
            }
        }
        let view = self.push_view(offset, self.bytes.len() - offset, target);
        let mut accessor = json!({
            "bufferView": view,
            "componentType": GL_FLOAT,
            "count": values.len(),
            "type": "VEC3"
        });
        if bounds && !values.is_empty() {
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for value in values {
                for axis in 0..3 {
                    min[axis] = min[axis].min(value[axis]);
                    max[axis] = max[axis].max(value[axis]);
                }
            }
            let object = accessor.as_object_mut().expect("accessor object");
            object.insert("min".into(), json!(min));
            object.insert("max".into(), json!(max));
        }
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    fn push_vec2_f32(&mut self, values: &[[f32; 2]], target: Option<u32>) -> usize {
        self.align4();
        let offset = self.bytes.len();
        for value in values {
            for component in value {
                self.bytes.extend_from_slice(&component.to_le_bytes());
            }
        }
        let view = self.push_view(offset, self.bytes.len() - offset, target);
        self.accessors.push(json!({
            "bufferView": view,
            "componentType": GL_FLOAT,
            "count": values.len(),
            "type": "VEC2"
        }));
        self.accessors.len() - 1
    }

    fn push_vec4_f32(&mut self, values: &[[f32; 4]], target: Option<u32>) -> usize {
        self.align4();
        let offset = self.bytes.len();
        for value in values {
            for component in value {
                self.bytes.extend_from_slice(&component.to_le_bytes());
            }
        }
        let view = self.push_view(offset, self.bytes.len() - offset, target);
        self.accessors.push(json!({
            "bufferView": view,
            "componentType": GL_FLOAT,
            "count": values.len(),
            "type": "VEC4"
        }));
        self.accessors.len() - 1
    }

    fn push_vec4_u16(&mut self, values: &[[u16; 4]], target: Option<u32>) -> usize {
        self.align4();
        let offset = self.bytes.len();
        for value in values {
            for component in value {
                self.bytes.extend_from_slice(&component.to_le_bytes());
            }
        }
        let view = self.push_view(offset, self.bytes.len() - offset, target);
        self.accessors.push(json!({
            "bufferView": view,
            "componentType": GL_UNSIGNED_SHORT,
            "count": values.len(),
            "type": "VEC4"
        }));
        self.accessors.len() - 1
    }

    fn push_u16(&mut self, values: &[u16], target: Option<u32>) -> usize {
        self.align4();
        let offset = self.bytes.len();
        for value in values {
            self.bytes.extend_from_slice(&value.to_le_bytes());
        }
        let view = self.push_view(offset, self.bytes.len() - offset, target);
        self.accessors.push(json!({
            "bufferView": view,
            "componentType": GL_UNSIGNED_SHORT,
            "count": values.len(),
            "type": "SCALAR"
        }));
        self.accessors.len() - 1
    }

    fn push_scalar_f32(&mut self, values: &[f32], bounds: bool) -> usize {
        self.align4();
        let offset = self.bytes.len();
        for value in values {
            self.bytes.extend_from_slice(&value.to_le_bytes());
        }
        let view = self.push_view(offset, self.bytes.len() - offset, None);
        let mut accessor = json!({
            "bufferView": view,
            "componentType": GL_FLOAT,
            "count": values.len(),
            "type": "SCALAR"
        });
        if bounds && !values.is_empty() {
            let min = values.iter().copied().fold(f32::INFINITY, f32::min);
            let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let object = accessor.as_object_mut().expect("accessor object");
            object.insert("min".into(), json!([min]));
            object.insert("max".into(), json!([max]));
        }
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    fn push_mat4_f32(&mut self, values: &[[f32; 16]]) -> usize {
        self.align4();
        let offset = self.bytes.len();
        for value in values {
            for component in value {
                self.bytes.extend_from_slice(&component.to_le_bytes());
            }
        }
        let view = self.push_view(offset, self.bytes.len() - offset, None);
        self.accessors.push(json!({
            "bufferView": view,
            "componentType": GL_FLOAT,
            "count": values.len(),
            "type": "MAT4"
        }));
        self.accessors.len() - 1
    }

    fn push_view(&mut self, offset: usize, length: usize, target: Option<u32>) -> usize {
        let mut view = json!({
            "buffer": 0,
            "byteOffset": offset,
            "byteLength": length,
        });
        if let Some(target) = target {
            view.as_object_mut()
                .expect("buffer view object")
                .insert("target".into(), json!(target));
        }
        self.views.push(view);
        self.views.len() - 1
    }

    fn align4(&mut self) {
        while !self.bytes.len().is_multiple_of(4) {
            self.bytes.push(0);
        }
    }
}

fn read_wc3_version(install: &Path) -> Option<String> {
    let text = fs::read_to_string(install.join(".build.info")).ok()?;
    let mut lines = text.lines();
    let headers: Vec<_> = lines.next()?.split('|').collect();
    let active_col = headers
        .iter()
        .position(|header| *header == "Active!DEC:1")?;
    let version_col = headers
        .iter()
        .position(|header| *header == "Version!STRING:0")?;
    let product_col = headers
        .iter()
        .position(|header| *header == "Product!STRING:0")?;
    for line in lines {
        let values: Vec<_> = line.split('|').collect();
        if values.get(active_col) == Some(&"1") && values.get(product_col) == Some(&"w3") {
            let version = values.get(version_col)?.trim();
            if !version.is_empty() {
                return Some(version.to_owned());
            }
        }
    }
    None
}

fn read_game_data_table(
    storage: &mut CascStorage,
    map_storage: Option<&MpqStorage>,
    logical_path: &str,
) -> Result<String, Box<dyn Error>> {
    if let Some(map_storage) = map_storage
        && let Some(bytes) = map_storage.read_file(logical_path)
    {
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    for casc_path in casc_asset_paths(logical_path) {
        if let Some(bytes) = storage.read_file(&casc_path) {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            drop(bytes);
            storage.flush_cache();
            return Ok(text);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("WC3 game-data table not found: {logical_path}"),
    )
    .into())
}

fn parse_sylk_cells(text: &str) -> BTreeMap<usize, BTreeMap<usize, String>> {
    let mut rows = BTreeMap::<usize, BTreeMap<usize, String>>::new();
    let mut current_y = None;
    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r');
        if !line.starts_with("C;") {
            continue;
        }
        let mut x = None;
        let mut y = None;
        let mut value = None;
        for field in line.split(';').skip(1) {
            if let Some(raw) = field.strip_prefix('X') {
                x = raw.parse::<usize>().ok();
            } else if let Some(raw) = field.strip_prefix('Y') {
                y = raw.parse::<usize>().ok();
            } else if let Some(raw) = field.strip_prefix('K') {
                let raw = raw.trim();
                value = Some(
                    raw.strip_prefix('"')
                        .and_then(|raw| raw.strip_suffix('"'))
                        .unwrap_or(raw)
                        .replace("\"\"", "\""),
                );
            }
        }
        if y.is_some() {
            current_y = y;
        }
        let Some(x) = x else {
            continue;
        };
        let Some(y) = current_y else {
            continue;
        };
        let Some(value) = value else {
            continue;
        };
        rows.entry(y).or_default().insert(x, value);
    }
    rows
}

fn sylk_header_column(
    rows: &BTreeMap<usize, BTreeMap<usize, String>>,
    header: &str,
) -> Option<usize> {
    rows.get(&1)?
        .iter()
        .find_map(|(column, value)| value.eq_ignore_ascii_case(header).then_some(*column))
}

fn parse_animation_sound_catalog(
    text: &str,
) -> Result<BTreeMap<String, AnimationSoundSpec>, Box<dyn Error>> {
    let rows = parse_sylk_cells(text);
    let event_code_col = sylk_header_column(&rows, "AnimationEventCode")
        .ok_or_else(|| io::Error::other("AnimSounds.slk is missing AnimationEventCode"))?;
    let sound_name_col = sylk_header_column(&rows, "SoundName")
        .ok_or_else(|| io::Error::other("AnimSounds.slk is missing SoundName"))?;
    let file_names_col = sylk_header_column(&rows, "FileNames")
        .ok_or_else(|| io::Error::other("AnimSounds.slk is missing FileNames"))?;
    let volume_col = sylk_header_column(&rows, "Volume");
    let volume_variance_col = sylk_header_column(&rows, "VolumeVariance");
    let pitch_col = sylk_header_column(&rows, "Pitch");
    let pitch_variance_col = sylk_header_column(&rows, "PitchVariance");
    let maximum_concurrent_instances_col = sylk_header_column(&rows, "MaximumConcurrentInstances");
    let priority_col = sylk_header_column(&rows, "Priority");
    let channel_col = sylk_header_column(&rows, "Channel");
    let flags_col = sylk_header_column(&rows, "Flags");
    let min_distance_col = sylk_header_column(&rows, "MinDistance");
    let max_distance_col = sylk_header_column(&rows, "MaxDistance");
    let distance_cutoff_col = sylk_header_column(&rows, "DistanceCutoff");
    let eax_flags_col = sylk_header_column(&rows, "EAXFlags");
    let rolloff_points_col = sylk_header_column(&rows, "RolloffPoints");

    let scalar = |row: &BTreeMap<usize, String>, column: Option<usize>, default: f32| {
        column
            .and_then(|column| row.get(&column))
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(default)
    };
    let integer = |row: &BTreeMap<usize, String>, column: Option<usize>, default: i32| {
        column
            .and_then(|column| row.get(&column))
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(default)
    };

    let mut result = BTreeMap::new();
    for (row_id, row) in rows {
        if row_id == 1 {
            continue;
        }
        let Some(event_code) = row.get(&event_code_col).map(|value| value.trim()) else {
            continue;
        };
        if event_code.len() != 4 {
            continue;
        }
        let file_names = row.get(&file_names_col).map(String::as_str).unwrap_or("");
        let silent = file_names.trim() == "_";
        let source_files = file_names
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "_")
            .map(|value| value.replace('/', "\\"))
            .collect::<Vec<_>>();
        if source_files.is_empty() && !silent {
            continue;
        }
        result.insert(
            event_code.to_ascii_uppercase(),
            AnimationSoundSpec {
                sound_name: row
                    .get(&sound_name_col)
                    .cloned()
                    .unwrap_or_else(|| event_code.to_owned()),
                source_files,
                silent,
                volume: scalar(&row, volume_col, 127.0),
                volume_variance: scalar(&row, volume_variance_col, 0.0),
                pitch: scalar(&row, pitch_col, 1.0),
                pitch_variance: scalar(&row, pitch_variance_col, 0.0),
                maximum_concurrent_instances: integer(&row, maximum_concurrent_instances_col, -1),
                priority: integer(&row, priority_col, 0),
                channel: integer(&row, channel_col, 0),
                flags: flags_col
                    .and_then(|column| row.get(&column))
                    .cloned()
                    .unwrap_or_default(),
                min_distance: scalar(&row, min_distance_col, 0.0),
                max_distance: scalar(&row, max_distance_col, 0.0),
                distance_cutoff: scalar(&row, distance_cutoff_col, 0.0),
                eax_flags: eax_flags_col
                    .and_then(|column| row.get(&column))
                    .cloned()
                    .unwrap_or_default(),
                rolloff_points: rolloff_points_col
                    .and_then(|column| row.get(&column))
                    .cloned()
                    .unwrap_or_default(),
            },
        );
    }
    Ok(result)
}

fn read_animation_sound_catalog(
    storage: &mut CascStorage,
    map_storage: Option<&MpqStorage>,
) -> Result<BTreeMap<String, AnimationSoundSpec>, Box<dyn Error>> {
    let text = read_game_data_table(storage, map_storage, r"ui\soundinfo\animsounds.slk")?;
    parse_animation_sound_catalog(&text)
}

fn read_spawn_event_catalog(
    storage: &mut CascStorage,
    map_storage: Option<&MpqStorage>,
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let text = read_game_data_table(storage, map_storage, r"splats\spawndata.slk")?;
    let rows = parse_sylk_cells(&text);
    let mut result = BTreeMap::new();
    for (row_id, row) in rows {
        if row_id == 1 {
            continue;
        }
        let Some(code) = row.get(&1).map(|value| value.trim()) else {
            continue;
        };
        let Some(model) = row.get(&2).map(|value| value.trim()) else {
            continue;
        };
        if code.is_empty() || model.is_empty() || model.eq_ignore_ascii_case("INIT") {
            continue;
        }
        result.insert(code.to_ascii_uppercase(), normalize_model_path(model));
    }
    Ok(result)
}

fn read_splat_service_catalog(
    storage: &mut CascStorage,
    map_storage: Option<&MpqStorage>,
    uber: bool,
) -> Result<BTreeMap<String, SplatServiceSpec>, Box<dyn Error>> {
    let path = if uber {
        r"splats\ubersplatdata.slk"
    } else {
        r"splats\splatdata.slk"
    };
    let text = read_game_data_table(storage, map_storage, path)?;
    parse_splat_service_catalog(&text, uber)
}

fn parse_splat_service_catalog(
    text: &str,
    uber: bool,
) -> Result<BTreeMap<String, SplatServiceSpec>, Box<dyn Error>> {
    let rows = parse_sylk_cells(text);
    let column = |name: &str| {
        sylk_header_column(&rows, name)
            .ok_or_else(|| io::Error::other(format!("splat service is missing {name}")))
    };
    let name = column("Name")?;
    let dir = column("Dir")?;
    let file = column("file")?;
    let blend = column("BlendMode")?;
    let scale = column("Scale")?;
    let get = |row: &BTreeMap<usize, String>, key: &str, default: f32| -> f32 {
        sylk_header_column(&rows, key)
            .and_then(|col| row.get(&col))
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let color = |row: &BTreeMap<usize, String>, prefix: &str| -> [u8; 4] {
        let channel =
            |suffix: &str| get(row, &format!("{prefix}{suffix}"), 255.0).clamp(0.0, 255.0) as u8;
        [channel("R"), channel("G"), channel("B"), channel("A")]
    };
    let mut result = BTreeMap::new();
    for (row_id, row) in &rows {
        if *row_id == 1 {
            continue;
        }
        let Some(code) = row.get(&name).map(String::as_str) else {
            continue;
        };
        if code.len() != 4 || code.eq_ignore_ascii_case("INIT") {
            continue;
        }
        let (Some(directory), Some(filename)) = (row.get(&dir), row.get(&file)) else {
            continue;
        };
        if directory.is_empty() || filename.is_empty() {
            continue;
        }
        let source_texture = format!(r"{}\{}", directory.trim_end_matches(['\\', '/']), filename);
        let number = |col: usize, default: f32| {
            row.get(&col)
                .and_then(|v| v.parse().ok())
                .unwrap_or(default)
        };
        result.insert(
            code.to_ascii_uppercase(),
            SplatServiceSpec {
                source_texture,
                rows: if uber {
                    1
                } else {
                    get(row, "Rows", 1.0).max(1.0) as u32
                },
                columns: if uber {
                    1
                } else {
                    get(row, "Columns", 1.0).max(1.0) as u32
                },
                blend_mode: number(blend, 0.0).max(0.0) as u32,
                scale: number(scale, 1.0),
                lifespan: get(row, "Lifespan", 0.0),
                decay: get(row, "Decay", 0.0),
                birth_time: get(row, "BirthTime", 0.0),
                pause_time: get(row, "PauseTime", 0.0),
                uv_lifespan: [
                    get(row, "UVLifespanStart", 0.0) as u32,
                    get(row, "UVLifespanEnd", 0.0) as u32,
                ],
                lifespan_repeat: get(row, "LifespanRepeat", 1.0).max(1.0) as u32,
                uv_decay: [
                    get(row, "UVDecayStart", 0.0) as u32,
                    get(row, "UVDecayEnd", 0.0) as u32,
                ],
                decay_repeat: get(row, "DecayRepeat", 1.0).max(1.0) as u32,
                colors: [color(row, "Start"), color(row, "Middle"), color(row, "End")],
            },
        );
    }
    Ok(result)
}

fn parse_doodad_skin(text: &str) -> DoodadSkinCatalog {
    let mut result = DoodadSkinCatalog::new();
    let mut current: Option<String> = None;
    for raw_line in text.lines() {
        let line = raw_line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with("//") || line.starts_with(';') {
            continue;
        }
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            let key = section.trim().to_ascii_lowercase();
            result.entry(key.clone()).or_default();
            current = Some(key);
            continue;
        }
        let Some(current) = current.as_ref() else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let profile = result
            .get_mut(current)
            .expect("current section was inserted");
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "file:sd" => profile.file_sd = nonempty(value),
            "file" => profile.file = nonempty(value),
            "numvar" => profile.num_variations = value.parse().ok(),
            "texid" => profile.replaceable_texture_id = value.parse().ok(),
            "texfile:sd" | "texfile" => profile.replaceable_texture = nonempty(value),
            _ => {}
        }
    }
    result
}

fn parse_unit_skin(text: &str) -> UnitSkinCatalog {
    let mut result = UnitSkinCatalog::new();
    let mut current: Option<String> = None;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with(';') {
            continue;
        }
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            let key = section.trim().to_ascii_lowercase();
            result.entry(key.clone()).or_default();
            current = Some(key);
            continue;
        }
        let Some(current) = current.as_ref() else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let profile = result
            .get_mut(current)
            .expect("current section was inserted");
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "file:sd" => profile.file_sd = nonempty(value),
            "file" => profile.file = nonempty(value),
            "modelscale:sd" => profile.model_scale_sd = value.parse().ok(),
            "modelscale" => profile.model_scale = value.parse().ok(),
            _ => {}
        }
    }
    result
}

fn nonempty(value: &str) -> Option<String> {
    (!value.is_empty() && value != "-").then(|| value.to_owned())
}

fn doodad_model_key(logical_path: &str, replacements: &BTreeMap<u32, String>) -> String {
    let mut key = logical_path.to_ascii_lowercase();
    for (id, texture) in replacements {
        key.push('|');
        key.push_str(&id.to_string());
        key.push('=');
        key.push_str(&texture.to_ascii_lowercase());
    }
    key
}

fn doodad_asset_name(logical_path: &str, replacements: &BTreeMap<u32, String>) -> String {
    let mut name = flat_asset_name(logical_path);
    for (id, texture) in replacements {
        name.push_str("__r");
        name.push_str(&id.to_string());
        name.push('_');
        name.push_str(&flat_asset_name(texture));
    }
    name
}

fn preferred_doodad_model_path(path: &str, variation: u32, num_variations: u32) -> String {
    doodad_model_candidates(path, variation, num_variations)
        .into_iter()
        .next()
        .expect("doodad model candidate list is never empty")
}

fn doodad_model_candidates(path: &str, variation: u32, num_variations: u32) -> Vec<String> {
    let exact = normalize_model_path(path);
    let stem = exact
        .strip_suffix(".mdx")
        .expect("normalized model path always has mdx suffix");
    let varied = format!("{stem}{variation}.mdx");
    if num_variations > 1 && varied != exact {
        vec![varied, exact]
    } else if varied != exact {
        vec![exact, varied]
    } else {
        vec![exact]
    }
}

fn is_intentionally_hidden_model_path(path: &str) -> bool {
    path.rsplit('\\').next().is_some_and(|name| {
        name.eq_ignore_ascii_case("no_model.mdx") || name.eq_ignore_ascii_case("none.mdx")
    })
}

fn map_model_candidates(logical_path: &str) -> Vec<String> {
    let normalized = logical_path.trim().replace('/', "\\");
    let lower = normalized.to_ascii_lowercase();
    let mut candidates = vec![normalized.clone()];
    let alternate_extension = if lower.ends_with(".mdx") {
        Some(format!("{}.mdl", &normalized[..normalized.len() - 4]))
    } else if lower.ends_with(".mdl") {
        Some(format!("{}.mdx", &normalized[..normalized.len() - 4]))
    } else {
        None
    };
    if let Some(alternate) = &alternate_extension {
        candidates.push(alternate.clone());
    }
    if !normalized.contains('\\') {
        candidates.push(format!(r"war3mapImported\{normalized}"));
        if let Some(alternate) = alternate_extension {
            candidates.push(format!(r"war3mapImported\{alternate}"));
        }
    }
    candidates
}

pub fn normalize_model_path(path: &str) -> String {
    let mut path = path.trim().replace('/', "\\");
    while path.starts_with('\\') {
        path.remove(0);
    }
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".mdl") || lower.ends_with(".mdx") {
        path.truncate(path.len() - 4);
    }
    path.push_str(".mdx");
    path
}

fn stock_building_model_relocation_candidates(logical_path: &str) -> Vec<String> {
    let normalized = normalize_model_path(logical_path);
    let mut parts = normalized.split('\\');
    let Some(root) = parts.next() else {
        return Vec::new();
    };
    let Some(category) = parts.next() else {
        return Vec::new();
    };
    if !root.eq_ignore_ascii_case("buildings") {
        return Vec::new();
    }
    let tail = parts.collect::<Vec<_>>().join("\\");
    if tail.is_empty() {
        return Vec::new();
    }

    STOCK_BUILDING_ART_CATEGORIES
        .iter()
        .copied()
        .filter(|candidate| !candidate.eq_ignore_ascii_case(category))
        .map(|candidate| format!("buildings\\{candidate}\\{tail}"))
        .collect()
}

fn normalize_texture_path(path: &str) -> String {
    path.trim()
        .replace('/', "\\")
        .trim_start_matches('\\')
        .to_owned()
}

fn casc_asset_paths(logical_path: &str) -> [String; 1] {
    // Castle Fight Native currently targets Classic/SD Warcraft III presentation only. Modern
    // installs also contain DE/HD presentation mods, but those are a separate rendering target
    // and must never be used as transparent fallbacks for missing Classic assets.
    [format!("war3.w3mod:{logical_path}")]
}

fn is_classic_sound_casc_asset_path(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    let Some(logical_path) = path.strip_prefix("war3.w3mod:") else {
        return false;
    };
    !logical_path.contains(".w3mod:") || logical_path.starts_with("_locales\\")
}

fn sound_file_name(path: &str) -> &str {
    path.rsplit(['\\', '/', ':']).next().unwrap_or(path)
}

fn sound_stem(path: &str) -> Option<String> {
    Path::new(sound_file_name(path))
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|stem| !stem.is_empty())
}

fn sound_logical_stem(path: &str) -> Option<String> {
    let logical = path.rsplit(':').next()?.trim().replace('/', "\\");
    let extension = Path::new(sound_file_name(&logical)).extension()?.to_str()?;
    let without_extension = logical.strip_suffix(&format!(".{extension}"))?;
    (!without_extension.is_empty()).then(|| without_extension.to_ascii_lowercase())
}

fn sound_path_index_key(path: &str) -> Option<String> {
    sound_logical_stem(path).map(|stem| format!("path:{stem}"))
}

fn sound_name_index_key(path: &str) -> Option<String> {
    sound_stem(path).map(|stem| format!("name:{stem}"))
}

fn is_audio_file(path: &str) -> bool {
    Path::new(sound_file_name(path))
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "flac" | "ogg" | "wav" | "mp3"
            )
        })
}

fn build_sound_file_index(paths: Vec<String>) -> BTreeMap<String, Vec<String>> {
    let mut index = BTreeMap::<String, Vec<String>>::new();
    for path in paths {
        if !is_classic_sound_casc_asset_path(&path) || !is_audio_file(&path) {
            continue;
        }
        if let Some(key) = sound_path_index_key(&path) {
            index.entry(key).or_default().push(path.clone());
        }
        if let Some(key) = sound_name_index_key(&path) {
            index.entry(key).or_default().push(path);
        }
    }
    for candidates in index.values_mut() {
        candidates.sort_by_key(|path| path.to_ascii_lowercase());
        candidates.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    }
    index
}

fn select_sound_candidate(candidates: &[String]) -> Option<String> {
    let [selected] = candidates else {
        return None;
    };
    Some(selected.clone())
}

fn select_legacy_sound_path(
    logical_path: &str,
    index: &BTreeMap<String, Vec<String>>,
) -> Option<String> {
    let normalized = logical_path.trim().replace('/', "\\");
    if normalized.is_empty() || normalized == "_" {
        return None;
    }

    if normalized.contains('\\') {
        let key = sound_path_index_key(&normalized)?;
        return index
            .get(&key)
            .and_then(|candidates| select_sound_candidate(candidates));
    }
    if Path::new(&normalized).extension().is_some() {
        return None;
    }

    let stem = normalized.to_ascii_lowercase();
    let key = format!("name:{stem}");
    if let Some(candidates) = index.get(&key)
        && let Some(candidate) = select_sound_candidate(candidates)
    {
        return Some(candidate);
    }
    let stripped = stem.strip_suffix('1')?;
    let candidates = index.get(&format!("name:{stripped}"))?;
    select_sound_candidate(candidates)
}

fn team_glow_texture_logical(player_index: u8) -> String {
    format!(r"ReplaceableTextures\TeamGlow\TeamGlow{player_index:02}.blp")
}

fn legacy_texture_stems(stem: &str) -> Vec<String> {
    let mut stems = vec![stem.to_owned()];
    if stem.eq_ignore_ascii_case(r"Textures\Clouds8x8") {
        stems.push(r"ReplaceableTextures\Weather\Clouds8x8".to_owned());
    }
    stems
}

fn flat_asset_name(path: &str) -> String {
    let without_extension = Path::new(path)
        .with_extension("")
        .to_string_lossy()
        .to_ascii_lowercase();
    let mut result = String::with_capacity(without_extension.len());
    let mut was_separator = false;
    for ch in without_extension.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            result.push(ch);
            was_separator = false;
        } else if !was_separator {
            result.push_str("__");
            was_separator = true;
        }
    }
    result.trim_matches('_').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn casc_asset_paths_are_classic_only() {
        assert_eq!(
            casc_asset_paths(r"Textures\Water\Foam.dds"),
            [r"war3.w3mod:Textures\Water\Foam.dds".to_owned()]
        );
    }

    #[test]
    fn legacy_sound_lookup_ignores_modern_presentation_namespaces() {
        let index = build_sound_file_index(vec![
            r"war3.w3mod:_hd.w3mod:units\nightelf\archer\archerdeath1_hd.flac".to_owned(),
            r"war3.w3mod:_de.w3mod:units\nightelf\archer\archerdeath1.flac".to_owned(),
            r"war3.w3mod:_teen.w3mod:units\nightelf\archer\archerdeath1.flac".to_owned(),
        ]);
        assert!(select_legacy_sound_path("ArcherDeath1", &index).is_none());
    }

    #[test]
    fn legacy_sound_lookup_uses_classic_namespace_and_trailing_one_fallback() {
        let index = build_sound_file_index(vec![
            r"war3.w3mod:_hd.w3mod:units\nightelf\archer\archerdeath1_hd.flac".to_owned(),
            r"war3.w3mod:_de.w3mod:units\nightelf\archer\archerdeath1.flac".to_owned(),
            r"war3.w3mod:units\nightelf\archer\archerdeath1.ogg".to_owned(),
            r"war3.w3mod:units\human\footman\footmandeath.ogg".to_owned(),
            r"war3.w3mod:_locales\enus.w3mod:units\human\gyrocopter\gyrocopterdeath1.ogg"
                .to_owned(),
        ]);
        assert_eq!(
            select_legacy_sound_path("ArcherDeath1", &index).as_deref(),
            Some(r"war3.w3mod:units\nightelf\archer\archerdeath1.ogg")
        );
        assert_eq!(
            select_legacy_sound_path("FootmanDeath1", &index).as_deref(),
            Some(r"war3.w3mod:units\human\footman\footmandeath.ogg")
        );
        assert_eq!(
            select_legacy_sound_path(r"Units\NightElf\Archer\ArcherDeath1.flac", &index).as_deref(),
            Some(r"war3.w3mod:units\nightelf\archer\archerdeath1.ogg")
        );
        assert_eq!(
            select_legacy_sound_path(r"Units\Human\Gyrocopter\GyrocopterDeath1.flac", &index)
                .as_deref(),
            Some(r"war3.w3mod:_locales\enus.w3mod:units\human\gyrocopter\gyrocopterdeath1.ogg")
        );
    }

    #[test]
    fn legacy_sound_lookup_rejects_ambiguous_same_namespace_matches() {
        let index = build_sound_file_index(vec![
            r"war3.w3mod:units\human\foo\death.ogg".to_owned(),
            r"war3.w3mod:units\orc\bar\death.ogg".to_owned(),
        ]);
        assert!(select_legacy_sound_path("Death1", &index).is_none());
    }

    #[test]
    fn bare_map_models_probe_import_directory_and_alternate_extension() {
        assert_eq!(
            map_model_candidates("Progressbar.mdx"),
            [
                "Progressbar.mdx".to_owned(),
                "Progressbar.mdl".to_owned(),
                r"war3mapImported\Progressbar.mdx".to_owned(),
                r"war3mapImported\Progressbar.mdl".to_owned(),
            ]
        );
        assert_eq!(
            map_model_candidates(r"war3mapImported\Model.mdx"),
            [
                r"war3mapImported\Model.mdx".to_owned(),
                r"war3mapImported\Model.mdl".to_owned(),
            ]
        );
    }

    #[test]
    fn normalizes_warcraft_model_paths() {
        assert_eq!(
            normalize_model_path(r"units\nightelf\Shandris\Shandris.mdl"),
            r"units\nightelf\Shandris\Shandris.mdx"
        );
        assert_eq!(
            normalize_model_path("units/human/Footman/Footman"),
            r"units\human\Footman\Footman.mdx"
        );
    }

    #[test]
    fn explicit_no_model_paths_are_intentionally_hidden() {
        assert!(is_intentionally_hidden_model_path("no_model.mdx"));
        assert!(is_intentionally_hidden_model_path(
            r"war3mapImported\NO_MODEL.MDX"
        ));
        assert!(is_intentionally_hidden_model_path("none.mdx"));
        assert!(!is_intentionally_hidden_model_path(
            r"units\human\Footman\Footman.mdx"
        ));
    }

    #[test]
    fn stock_building_relocation_candidates_preserve_model_tail() {
        let candidates = stock_building_model_relocation_candidates(
            r"buildings\other\TombofRelics\TombofRelics.mdl",
        );
        assert!(candidates.contains(&r"buildings\undead\TombofRelics\TombofRelics.mdx".to_owned()));
        assert!(!candidates.contains(&r"buildings\other\TombofRelics\TombofRelics.mdx".to_owned()));
        assert!(
            stock_building_model_relocation_candidates(r"units\human\Footman\Footman.mdl")
                .is_empty()
        );
    }

    #[test]
    fn legacy_cloud_texture_uses_stock_weather_alias() {
        assert_eq!(
            legacy_texture_stems(r"Textures\Clouds8x8"),
            [
                r"Textures\Clouds8x8".to_owned(),
                r"ReplaceableTextures\Weather\Clouds8x8".to_owned(),
            ]
        );
        assert_eq!(
            legacy_texture_stems(r"Textures\Other"),
            [r"Textures\Other".to_owned()]
        );
    }

    #[test]
    fn team_glow_paths_follow_wc3_player_slot_indices() {
        assert_eq!(
            team_glow_texture_logical(0),
            r"ReplaceableTextures\TeamGlow\TeamGlow00.blp"
        );
        assert_eq!(
            team_glow_texture_logical(6),
            r"ReplaceableTextures\TeamGlow\TeamGlow06.blp"
        );
        assert_eq!(
            team_glow_texture_logical(23),
            r"ReplaceableTextures\TeamGlow\TeamGlow23.blp"
        );
    }

    #[test]
    fn asset_names_include_the_logical_path() {
        assert_eq!(
            flat_asset_name(r"units\human\Footman\Footman.mdx"),
            "units__human__footman__footman"
        );
    }

    #[test]
    fn doodad_variations_append_the_editor_variation_index() {
        assert_eq!(
            doodad_model_candidates(r"Doodads\Ashenvale\Rocks\AshenRock\AshenRock", 7, 10)[0],
            r"Doodads\Ashenvale\Rocks\AshenRock\AshenRock7.mdx"
        );
        assert_eq!(
            doodad_model_candidates(r"Doodads\Ruins\Terrain\RuinsWall90\RuinsWall900.mdl", 0, 1,)
                [0],
            r"Doodads\Ruins\Terrain\RuinsWall90\RuinsWall900.mdx"
        );
    }

    #[test]
    fn skeleton_nodes_without_pivots_fall_back_to_origin() {
        let model = Model::new();
        let mut node = Node::new();
        node.set_name("PortraitBackground");
        node.set_object_id(0);
        let mut infos = BTreeMap::new();
        let mut warnings = Vec::new();

        insert_skeleton_node(&model, &node, &mut infos, &mut warnings).expect("insert node");

        assert_eq!(infos.get(&0).expect("node info").pivot, [0.0; 3]);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("no matching pivot point"));
    }

    #[test]
    fn pe2_manifest_preserves_lifecycle_and_uv_animation_fields() {
        let mut model = Model::new();
        model.resize_particle_emitters_2(1);
        {
            let mut emitter = model
                .particle_emitters_2_mut(0)
                .expect("particle emitter 2");
            emitter.set_length(20.0);
            emitter.set_width(10.0);
            emitter.set_head_or_tail(2);
            emitter.set_tail_length(48.0);
            emitter.set_time(0.35);
            for (index, value) in [0, 3, 2].into_iter().enumerate() {
                emitter.set_head_interval(index, value);
            }
            for (index, value) in [4, 7, 1].into_iter().enumerate() {
                emitter.set_head_decay_interval(index, value);
            }
            for (index, value) in [8, 11, 3].into_iter().enumerate() {
                emitter.set_tail_interval(index, value);
            }
            for (index, value) in [12, 15, 1].into_iter().enumerate() {
                emitter.set_tail_decay_interval(index, value);
            }
            emitter.set_priority_plane(4);
            emitter.set_replaceable_id(2);
        }

        let manifests =
            particle_emitter_2_manifests(&model, &[]).expect("PE2 manifest should export");
        assert_eq!(manifests.len(), 1);
        let emitter = &manifests[0];
        assert_eq!(emitter.length, 20.0);
        assert_eq!(emitter.width, 10.0);
        assert_eq!(emitter.head_or_tail, 2);
        assert_eq!(emitter.tail_length, 48.0);
        assert_eq!(emitter.time, 0.35);
        assert_eq!(emitter.head_interval, [0, 3, 2]);
        assert_eq!(emitter.head_decay_interval, [4, 7, 1]);
        assert_eq!(emitter.tail_interval, [8, 11, 3]);
        assert_eq!(emitter.tail_decay_interval, [12, 15, 1]);
        assert_eq!(emitter.priority_plane, 4);
        assert_eq!(emitter.replaceable_id, 2);
    }

    #[test]
    fn model_feature_manifest_inventories_lossy_wc3_primitives() {
        let mut model = Model::new();
        model.set_global_sequences(&[1000]);

        model.resize_materials(1);
        {
            let mut material = model.materials_mut(0).expect("material");
            material.resize_layers(3);
            material
                .layers_mut(0)
                .expect("alpha layer")
                .alpha_tracks_mut()
                .set_is_used(true);
            material
                .layers_mut(1)
                .expect("texture layer")
                .texture_id_tracks_mut()
                .set_is_used(true);
            {
                let mut hd = material.layers_mut(2).expect("out-of-scope HD layer");
                hd.set_shader(LayerShaderType::HD);
                hd.alpha_tracks_mut().set_is_used(true);
                hd.texture_id_tracks_mut().set_is_used(true);
            }
        }

        model.resize_geoset_animations(1);
        model
            .geoset_animations_mut(0)
            .expect("geoset animation")
            .alpha_tracks_mut()
            .set_is_used(true);

        model.resize_geosets(1);
        model
            .geosets_mut(0)
            .expect("geoset")
            .set_matrix_groups(&[5]);

        model.resize_attachments(1);
        {
            let mut attachment = model.attachments_mut(0).expect("attachment");
            attachment.set_path(r"SharedModels\Child.mdl");
            attachment
                .node_mut()
                .set_flags(NodeFlag::DONT_INHERIT_SCALING);
        }

        model.resize_particle_emitters_2(1);
        model
            .particle_emitters_2_mut(0)
            .expect("particle emitter 2")
            .emission_rate_tracks_mut()
            .set_is_used(true);
        model.resize_particle_emitters(1);
        model.resize_ribbon_emitters(1);
        model.resize_corn_emitters(1);
        model.resize_event_objects(1);
        model.resize_lights(1);

        let features = model_feature_manifest(&model);
        assert_eq!(features.material_count, 1);
        assert_eq!(features.material_layer_count, 2);
        assert_eq!(features.multilayer_material_count, 1);
        assert_eq!(features.animated_material_alpha_layer_count, 1);
        assert_eq!(features.animated_material_texture_layer_count, 1);
        assert_eq!(features.animated_geoset_alpha_count, 1);
        assert_eq!(features.global_sequence_count, 1);
        assert_eq!(features.attachment_count, 1);
        assert_eq!(
            features.attachment_models,
            [r"SharedModels\Child.mdl".to_owned()]
        );
        assert_eq!(features.particle_emitter_count, 1);
        assert_eq!(features.particle_emitter_2_count, 1);
        assert_eq!(features.particle_emitter_2_animated_track_count, 1);
        assert_eq!(features.ribbon_emitter_count, 1);
        assert_eq!(features.corn_emitter_count, 1);
        assert_eq!(features.event_object_count, 1);
        assert_eq!(features.light_count, 1);
        assert_eq!(features.omni_light_count, 1);
        assert_eq!(features.non_omni_light_count, 0);
        assert_eq!(features.non_inheritance_node_count, 1);
        assert_eq!(features.max_classic_skin_influences, 5);
    }

    #[test]
    fn global_rotation_track_spans_its_cycle_beyond_short_stand_sequence() {
        let window = track_window(&[0, 500, 1000], 0, &[1000], 2000, 2333, 1000)
            .expect("global track has a valid sequence window");
        assert_eq!(window.clip_duration, 1000);
        assert_eq!(window.frame_for_local(500), 500);
        assert_eq!(window.frame_for_local(1000), 0);
        assert_eq!(
            sample_times(InterpolationType::Linear, &[0, 500, 1000], &window),
            [0, 500, 1000]
        );

        let local = track_window(
            &[2000, 2166, 2333],
            NO_GLOBAL_SEQUENCE,
            &[],
            2000,
            2333,
            1000,
        )
        .expect("local track has a valid sequence window");
        assert_eq!(local.frame_for_local(500), 2167);
        assert!(
            sample_times(InterpolationType::Linear, &[2000, 2166, 2333], &local).contains(&666)
        );
    }

    #[test]
    fn light_manifest_preserves_omni_parameters_and_tracks() {
        let mut model = Model::new();
        model.resize_sequences(1);
        {
            let mut sequence = model.sequences_mut(0).expect("sequence");
            sequence.set_name("Stand");
            sequence.set_interval_start(100);
            sequence.set_interval_end(1100);
            sequence.set_flags(SequenceFlag::None);
        }
        model.set_global_sequences(&[750]);
        model.resize_lights(1);
        {
            let mut light = model.lights_mut(0).expect("light");
            light.set_type_(LightType::Omni);
            light.set_attenuation_start(40.0);
            light.set_attenuation_end(200.0);
            light.set_color(whiteout::math::Vector3f {
                x: 1.0,
                y: 0.5,
                z: 0.25,
            });
            light.set_intensity(18.0);
            light.set_ambient_color(whiteout::math::Vector3f {
                x: 0.1,
                y: 0.2,
                z: 0.3,
            });
            light.set_ambient_intensity(0.4);
            {
                let mut intensity = light.intensity_tracks_mut();
                intensity.set_is_used(true);
                intensity.set_interpolation_type(InterpolationType::Linear);
                intensity.set_global_sequence_id(NO_GLOBAL_SEQUENCE);
                intensity.set_key_count(2);
                intensity.set_timestamps(&[100, 1100]);
                intensity.set_keys(&[2.0, 18.0]);
            }
            {
                let mut visibility = light.visibility_tracks_mut();
                visibility.set_is_used(true);
                visibility.set_interpolation_type(InterpolationType::None);
                visibility.set_global_sequence_id(0);
                visibility.set_key_count(2);
                visibility.set_timestamps(&[0, 500]);
                visibility.set_keys(&[1.0, 0.0]);
            }
            let mut node = light.node_mut();
            node.set_object_id(0);
            node.set_name("Omni01");
        }
        model.set_pivot_points(&[whiteout::math::Vector3f {
            x: 10.0,
            y: 20.0,
            z: 30.0,
        }]);

        let lights = light_manifests(&model).expect("lights should serialize");
        assert_eq!(lights.len(), 1);
        let light = &lights[0];
        assert_eq!(light.object_id, 0);
        assert_eq!(light.name, "Omni01");
        assert_eq!(light.position, [10.0, 30.0, -20.0]);
        assert_eq!(light.light_type, "omni");
        assert_eq!(light.attenuation_start, 40.0);
        assert_eq!(light.attenuation_end, 200.0);
        assert_eq!(light.color, [1.0, 0.5, 0.25]);
        assert_eq!(light.intensity, 18.0);
        assert_eq!(light.ambient_color, [0.1, 0.2, 0.3]);
        assert_eq!(light.ambient_intensity, 0.4);
        assert_eq!(
            light
                .intensity_track
                .as_ref()
                .expect("intensity track")
                .values,
            [2.0, 18.0]
        );
        let visibility = light.visibility_track.as_ref().expect("visibility track");
        assert_eq!(visibility.global_sequence_id, Some(0));
        assert_eq!(visibility.values, [1.0, 0.0]);
        assert_eq!(light.sequence_windows[0].name, "Stand");
        assert_eq!(light.global_sequence_durations_ms, [750]);
    }

    #[test]
    fn overhead_attachment_uses_wc3_model_pivot() {
        let mut model = Model::new();
        model.resize_attachments(1);
        {
            let mut attachment = model.attachments_mut(0).expect("attachment");
            let mut node = attachment.node_mut();
            node.set_name("Overhead Ref");
            node.set_object_id(0);
        }
        model.set_pivot_points(&[whiteout::math::Vector3f {
            x: 3.0,
            y: 4.0,
            z: 120.0,
        }]);
        assert_eq!(model_overhead_position(&model), Some([3.0, 120.0, -4.0]));
    }

    #[test]
    fn classic_texture_inventory_ignores_hd_layers_but_keeps_particle_textures() {
        let mut model = Model::new();
        model.resize_textures(3);
        model.resize_materials(1);
        {
            let mut material = model.materials_mut(0).expect("material");
            material.resize_layers(2);
            material.layers_mut(0).expect("SD layer").set_texture_id(0);
            {
                let mut hd = material.layers_mut(1).expect("HD layer");
                hd.set_shader(LayerShaderType::HD);
                hd.set_texture_id(1);
            }
        }
        model.resize_particle_emitters_2(1);
        model
            .particle_emitters_2_mut(0)
            .expect("particle emitter")
            .set_texture_id(2);

        assert_eq!(classic_relevant_texture_ids(&model), BTreeSet::from([0, 2]));
    }

    #[test]
    fn classic_material_validation_rejects_hd_only_visible_materials() {
        let mut model = Model::new();
        model.resize_materials(1);
        {
            let mut material = model.materials_mut(0).expect("material");
            material.resize_layers(1);
            material
                .layers_mut(0)
                .expect("HD layer")
                .set_shader(LayerShaderType::HD);
        }
        model.resize_geosets(1);
        {
            let mut geoset = model.geosets_mut(0).expect("geoset");
            geoset.set_vertex_positions(&[
                whiteout::math::Vector3f::default(),
                whiteout::math::Vector3f::default(),
                whiteout::math::Vector3f::default(),
            ]);
            geoset.set_faces(&[0, 1, 2]);
            geoset.set_material_id(0);
        }

        let error = validate_classic_renderable_materials(&model, "HDOnly.mdx")
            .expect_err("HD-only geoset material must not enter the Classic pipeline")
            .to_string();
        assert!(error.contains("no SD layer"));
        assert!(error.contains("HD/Reforged/Definitive"));

        model
            .materials_mut(0)
            .expect("material")
            .layers_mut(0)
            .expect("layer")
            .set_shader(LayerShaderType::SD);
        validate_classic_renderable_materials(&model, "Classic.mdx")
            .expect("SD geoset material should be accepted");
    }

    #[test]
    fn layered_subtextures_resolve_diffuse_and_team_color_slots() {
        let mut model = Model::new();
        model.resize_textures(3);
        model
            .textures_mut(1)
            .expect("team color texture")
            .set_replaceable_id(1);
        model
            .textures_mut(2)
            .expect("team glow texture")
            .set_replaceable_id(2);

        let mut layer = Layer::new();
        layer.resize_sub_textures(5);
        {
            let mut diffuse = layer.sub_textures_mut(0).expect("diffuse slot");
            diffuse.set_slot(LayerSlotType::DiffuseMap);
            diffuse.set_texture_id(0);
        }
        {
            let mut team_color = layer.sub_textures_mut(4).expect("team color slot");
            team_color.set_slot(LayerSlotType::TeamColor);
            team_color.set_texture_id(1);
        }

        assert_eq!(layer_diffuse_texture_id(&layer), 0);
        assert!(!layer_diffuse_uses_replaceable(&model, &layer, 1));
        assert!(layer_static_uses_replaceable(&model, &layer, 1));
        assert!(layer_uses_replaceable(&model, &layer, 1));
        assert!(!layer_static_uses_replaceable(&model, &layer, 2));
        assert!(!layer_uses_replaceable(&model, &layer, 2));
    }

    #[test]
    fn animated_team_glow_texture_does_not_replace_the_static_diffuse_texture() {
        let mut model = Model::new();
        model.resize_textures(2);
        model
            .textures_mut(1)
            .expect("team glow texture")
            .set_replaceable_id(2);

        let mut layer = Layer::new();
        layer.set_texture_id(0);
        {
            let mut track = layer.texture_id_tracks_mut();
            track.set_is_used(true);
            track.set_timestamps(&[0]);
            track.set_keys(&[1]);
        }

        assert!(!layer_diffuse_uses_replaceable(&model, &layer, 2));
        assert!(!layer_static_uses_replaceable(&model, &layer, 2));
        assert!(layer_uses_replaceable(&model, &layer, 2));
    }

    #[test]
    fn building_engine_plane_detection_catches_team_glow_and_background_textures() {
        let mut model = Model::new();
        model.resize_materials(3);
        {
            let mut material = model.materials_mut(0).expect("replaceable glow material");
            material.resize_layers(1);
            material
                .layers_mut(0)
                .expect("replaceable glow layer")
                .set_texture_id(0);
        }
        {
            let mut material = model.materials_mut(1).expect("fixed glow material");
            material.resize_layers(1);
            material
                .layers_mut(0)
                .expect("fixed glow layer")
                .set_texture_id(1);
        }
        {
            let mut material = model.materials_mut(2).expect("background material");
            material.resize_layers(1);
            material
                .layers_mut(0)
                .expect("background layer")
                .set_texture_id(2);
        }
        let textures = vec![
            TextureManifest {
                source_texture: String::new(),
                source_casc_path: None,
                png: Some("textures/teamglow01.png".to_owned()),
                replaceable_id: 2,
                has_transparency: false,
            },
            TextureManifest {
                source_texture: r"ReplaceableTextures\TeamGlow\TeamGlow08.blp".to_owned(),
                source_casc_path: None,
                png: Some("textures/teamglow08.png".to_owned()),
                replaceable_id: 0,
                has_transparency: false,
            },
            TextureManifest {
                source_texture: r"Textures\Background.blp".to_owned(),
                source_casc_path: None,
                png: Some("textures/background.png".to_owned()),
                replaceable_id: 0,
                has_transparency: false,
            },
        ];

        assert!(material_is_building_engine_plane(&model, 0, &textures));
        assert!(material_is_building_engine_plane(&model, 1, &textures));
        assert!(material_is_building_engine_plane(&model, 2, &textures));
    }

    #[test]
    fn selected_background_layer_marks_multilayer_quad_for_omission() {
        let mut model = Model::new();
        model.resize_materials(1);
        {
            let mut material = model.materials_mut(0).expect("background material");
            material.resize_layers(2);
            material
                .layers_mut(0)
                .expect("background layer")
                .set_texture_id(0);
            material
                .layers_mut(1)
                .expect("ordinary layer")
                .set_texture_id(1);
        }
        let textures = vec![
            TextureManifest {
                source_texture: r"Textures\Background.blp".to_owned(),
                source_casc_path: None,
                png: Some("textures/background.png".to_owned()),
                replaceable_id: 0,
                has_transparency: false,
            },
            TextureManifest {
                source_texture: r"Textures\NorthrendNatural03.blp".to_owned(),
                source_casc_path: None,
                png: Some("textures/northrendnatural03.png".to_owned()),
                replaceable_id: 0,
                has_transparency: false,
            },
        ];

        assert!(!material_is_building_engine_plane(&model, 0, &textures));
        assert!(material_selected_diffuse_is_background(
            &model, 0, &textures
        ));
    }

    #[test]
    fn ribbon_manifest_preserves_scalar_color_and_texture_tracks() {
        let mut model = Model::new();
        model.resize_sequences(1);
        {
            let mut sequence = model.sequences_mut(0).expect("sequence");
            sequence.set_name("Stand");
            sequence.set_interval_start(100);
            sequence.set_interval_end(1100);
            sequence.set_flags(SequenceFlag::None);
        }
        model.set_global_sequences(&[750]);
        model.resize_ribbon_emitters(1);
        {
            let mut emitter = model.ribbon_emitters_mut(0).expect("ribbon emitter");
            emitter.set_material_id(0);
            emitter.set_texture_slot(3);
            {
                let mut alpha = emitter.alpha_tracks_mut();
                alpha.set_is_used(true);
                alpha.set_interpolation_type(InterpolationType::Linear);
                alpha.set_global_sequence_id(NO_GLOBAL_SEQUENCE);
                alpha.set_key_count(2);
                alpha.set_timestamps(&[0, 1000]);
                alpha.set_keys(&[1.0, 0.0]);
            }
            {
                let mut color = emitter.color_tracks_mut();
                color.set_is_used(true);
                color.set_interpolation_type(InterpolationType::Linear);
                color.set_global_sequence_id(NO_GLOBAL_SEQUENCE);
                color.set_key_count(2);
                color.set_timestamps(&[0, 1000]);
                color.set_keys(&[
                    whiteout::math::Vector3f {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    whiteout::math::Vector3f {
                        x: 0.0,
                        y: 0.0,
                        z: 1.0,
                    },
                ]);
            }
            {
                let mut texture = emitter.texture_slot_tracks_mut();
                texture.set_is_used(true);
                texture.set_interpolation_type(InterpolationType::None);
                texture.set_global_sequence_id(NO_GLOBAL_SEQUENCE);
                texture.set_key_count(2);
                texture.set_timestamps(&[0, 500]);
                texture.set_keys(&[0, 1]);
            }
            let mut node = emitter.node_mut();
            node.set_object_id(0);
            node.set_name("Ribbon");
        }
        model.set_pivot_points(&[whiteout::math::Vector3f::default()]);

        let ribbons = ribbon_emitter_manifests(&model, &[]).expect("ribbons should serialize");
        assert_eq!(ribbons.len(), 1);
        let ribbon = &ribbons[0];
        assert_eq!(ribbon.texture_slot, 3);
        assert_eq!(ribbon.sequence_windows.len(), 1);
        assert_eq!(ribbon.sequence_windows[0].name, "Stand");
        assert_eq!(ribbon.sequence_windows[0].start_ms, 100);
        assert_eq!(ribbon.sequence_windows[0].end_ms, 1100);
        assert!(!ribbon.sequence_windows[0].non_looping);
        assert_eq!(ribbon.global_sequence_durations_ms, [750]);
        assert_eq!(
            ribbon.alpha_track.as_ref().expect("alpha").values,
            [1.0, 0.0]
        );
        assert_eq!(
            ribbon.color_track.as_ref().expect("color").values,
            [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]
        );
        assert_eq!(
            ribbon
                .texture_slot_track
                .as_ref()
                .expect("texture slot")
                .values,
            [0, 1]
        );
    }

    #[test]
    fn ribbon_material_properties_resolve_texture_and_filter_mode() {
        let mut model = Model::new();
        model.resize_textures(1);
        model.resize_materials(1);
        {
            let mut material = model.materials_mut(0).expect("material");
            material.resize_layers(1);
            let mut layer = material.layers_mut(0).expect("layer");
            layer.set_texture_id(0);
            layer.set_filter_mode(LayerFilterMode::AddAlpha);
        }
        let textures = vec![TextureManifest {
            source_texture: r"Textures\Ribbon.blp".to_owned(),
            source_casc_path: None,
            png: Some("textures/ribbon.png".to_owned()),
            replaceable_id: 0,
            has_transparency: true,
        }];

        assert_eq!(
            ribbon_material_properties(&model, &textures, 0),
            (
                "AddAlpha".to_owned(),
                Some("textures/ribbon.png".to_owned())
            )
        );
    }

    #[test]
    fn material_manifest_preserves_alpha_and_texture_selection_tracks() {
        let mut model = Model::new();
        model.resize_materials(1);
        {
            let mut material = model.materials_mut(0).expect("material");
            material.set_priority_plane(2);
            material.resize_layers(1);
            let mut layer = material.layers_mut(0).expect("layer");
            layer.set_filter_mode(LayerFilterMode::AddAlpha);
            layer.set_texture_id(3);
            layer.set_alpha(0.75);
            {
                let mut alpha = layer.alpha_tracks_mut();
                alpha.set_is_used(true);
                alpha.set_interpolation_type(InterpolationType::Linear);
                alpha.set_global_sequence_id(NO_GLOBAL_SEQUENCE);
                alpha.set_key_count(2);
                alpha.set_timestamps(&[0, 500]);
                alpha.set_keys(&[0.0, 1.0]);
            }
            {
                let mut texture = layer.texture_id_tracks_mut();
                texture.set_is_used(true);
                texture.set_interpolation_type(InterpolationType::None);
                texture.set_global_sequence_id(NO_GLOBAL_SEQUENCE);
                texture.set_key_count(2);
                texture.set_timestamps(&[0, 500]);
                texture.set_keys(&[3, 4]);
            }
            layer.resize_sub_textures(1);
            {
                let mut sub = layer.sub_textures_mut(0).expect("sub texture");
                sub.set_slot(LayerSlotType::TeamColor);
                sub.set_texture_id(7);
                let mut track = sub.tracks_mut();
                track.set_is_used(true);
                track.set_interpolation_type(InterpolationType::None);
                track.set_global_sequence_id(NO_GLOBAL_SEQUENCE);
                track.set_key_count(2);
                track.set_timestamps(&[100, 600]);
                track.set_keys(&[7, 8]);
            }
        }

        let materials = material_manifests(&model).expect("materials should serialize");
        assert_eq!(materials.len(), 1);
        let material = &materials[0];
        assert_eq!(material.material_id, 0);
        assert_eq!(material.priority_plane, 2);
        let layer = &material.layers[0];
        assert_eq!(layer.filter_mode, "AddAlpha");
        assert_eq!(layer.texture_id, 3);
        assert_eq!(layer.alpha, 0.75);
        assert_eq!(
            layer.alpha_track.as_ref().expect("alpha track").values,
            [0.0, 1.0]
        );
        assert_eq!(
            layer
                .texture_id_track
                .as_ref()
                .expect("texture track")
                .values,
            [3, 4]
        );
        assert_eq!(layer.sub_textures[0].slot, "TeamColor");
        assert_eq!(
            layer.sub_textures[0]
                .texture_id_track
                .as_ref()
                .expect("sub texture track")
                .values,
            [7, 8]
        );
    }

    #[test]
    fn gltf_material_preserves_static_and_animated_wc3_alpha() {
        let mut model = Model::new();
        model.resize_sequences(1);
        {
            let mut sequence = model.sequences_mut(0).expect("sequence");
            sequence.set_name("Stand");
            sequence.set_interval_start(100);
            sequence.set_interval_end(1100);
            sequence.set_flags(SequenceFlag::None);
        }
        model.set_global_sequences(&[750]);
        model.resize_materials(1);
        {
            let mut material = model.materials_mut(0).expect("material");
            material.resize_layers(1);
            let mut layer = material.layers_mut(0).expect("layer");
            layer.set_filter_mode(LayerFilterMode::Blend);
            layer.set_texture_id(0);
            layer.set_alpha(0.35);
            let mut alpha = layer.alpha_tracks_mut();
            alpha.set_is_used(true);
            alpha.set_interpolation_type(InterpolationType::Linear);
            alpha.set_global_sequence_id(NO_GLOBAL_SEQUENCE);
            alpha.set_key_count(2);
            alpha.set_timestamps(&[100, 1100]);
            alpha.set_keys(&[0.25, 0.75]);
        }

        let (materials, _, _) =
            build_materials(&model, &[Some(0)], &[]).expect("materials should build");
        let material = &materials[0];
        let base_color = material["pbrMetallicRoughness"]["baseColorFactor"]
            .as_array()
            .expect("baseColorFactor");
        assert_eq!(&base_color[..3], &[json!(1.0), json!(1.0), json!(1.0)]);
        assert!(
            (base_color[3].as_f64().expect("alpha") - 0.35).abs() < 1.0e-6,
            "static WC3 alpha must survive glTF conversion"
        );
        assert!(
            (material["extras"]["wc3LayerAlpha"]
                .as_f64()
                .expect("wc3LayerAlpha")
                - 0.35)
                .abs()
                < 1.0e-6
        );
        assert_eq!(
            material["extras"]["wc3AlphaTrack"]["values"],
            json!([0.25, 0.75])
        );
        assert_eq!(
            material["extras"]["wc3SequenceWindows"][0]["name"],
            json!("Stand")
        );
        assert_eq!(
            material["extras"]["wc3GlobalSequenceDurationsMs"],
            json!([750])
        );
    }

    #[test]
    fn gltf_material_preserves_animated_diffuse_texture_selection() {
        let mut model = Model::new();
        model.resize_sequences(1);
        {
            let mut sequence = model.sequences_mut(0).expect("sequence");
            sequence.set_name("Stand");
            sequence.set_interval_start(100);
            sequence.set_interval_end(500);
            sequence.set_flags(SequenceFlag::None);
        }
        model.set_global_sequences(&[400]);
        model.resize_materials(1);
        {
            let mut material = model.materials_mut(0).expect("material");
            material.resize_layers(1);
            let mut layer = material.layers_mut(0).expect("layer");
            layer.set_filter_mode(LayerFilterMode::Blend);
            layer.set_texture_id(0);
            layer.resize_sub_textures(1);
            let mut diffuse = layer.sub_textures_mut(0).expect("diffuse");
            diffuse.set_slot(LayerSlotType::DiffuseMap);
            diffuse.set_texture_id(0);
            let mut texture = diffuse.tracks_mut();
            texture.set_is_used(true);
            texture.set_interpolation_type(InterpolationType::None);
            texture.set_global_sequence_id(0);
            texture.set_key_count(2);
            texture.set_timestamps(&[0, 200]);
            texture.set_keys(&[0, 1]);
        }
        let texture_manifests = vec![
            TextureManifest {
                source_texture: "Textures\\Frame0.blp".to_owned(),
                source_casc_path: None,
                png: Some("textures/frame0.png".to_owned()),
                replaceable_id: 0,
                has_transparency: true,
            },
            TextureManifest {
                source_texture: "Textures\\Frame1.blp".to_owned(),
                source_casc_path: None,
                png: Some("textures/frame1.png".to_owned()),
                replaceable_id: 0,
                has_transparency: true,
            },
        ];

        let (materials, _, _) = build_materials(&model, &[Some(0), Some(1)], &texture_manifests)
            .expect("materials should build");
        let extras = &materials[0]["extras"];
        assert_eq!(extras["wc3TextureId"], json!(0));
        assert_eq!(extras["wc3TextureIdTrack"]["values"], json!([0, 1]));
        assert_eq!(
            extras["wc3TexturePaths"],
            json!([
                {"textureId": 0, "fileName": "frame0.png"},
                {"textureId": 1, "fileName": "frame1.png"},
            ])
        );
        assert_eq!(extras["wc3SequenceWindows"][0]["name"], json!("Stand"));
        assert_eq!(extras["wc3GlobalSequenceDurationsMs"], json!([400]));
    }

    #[test]
    fn material_priority_plane_is_preserved_for_native_depth_ordering() {
        let mut model = Model::new();
        model.resize_materials(1);
        {
            let mut material = model.materials_mut(0).expect("material");
            material.set_priority_plane(3);
            material.resize_layers(1);
            material.layers_mut(0).expect("layer").set_texture_id(0);
        }
        let (materials, _, _) =
            build_materials(&model, &[None], &[]).expect("materials should build");
        assert_eq!(materials[0]["extras"]["wc3PriorityPlane"], json!(3));
    }

    #[test]
    fn opaque_wc3_layers_use_texture_alpha_as_a_cutout_mask() {
        assert_eq!(gltf_alpha_mode(LayerFilterMode::None, 1.0, false), "OPAQUE");
        assert_eq!(gltf_alpha_mode(LayerFilterMode::None, 1.0, true), "MASK");
        assert_eq!(gltf_alpha_mode(LayerFilterMode::None, 0.5, true), "BLEND");
        assert_eq!(
            gltf_alpha_mode(LayerFilterMode::Transparent, 1.0, false),
            "MASK"
        );
    }

    #[test]
    fn detects_transparent_pixels_in_decoded_textures() {
        let mut opaque = Texture::create_2d(PixelFormat::RGBA8, 1, 1, 1).unwrap();
        opaque.set_data(&[1, 2, 3, 255]);
        assert!(!texture_has_transparency(&opaque));

        let mut transparent = Texture::create_2d(PixelFormat::RGBA8, 1, 1, 1).unwrap();
        transparent.set_data(&[1, 2, 3, 0]);
        assert!(texture_has_transparency(&transparent));
    }

    #[test]
    fn doodad_skin_parser_recovers_model_variations_and_replaceable_texture() {
        let profiles = parse_doodad_skin(
            "\u{feff}[ATtr]\nnumVar=5\nfile=Doodads\\Terrain\\AshenTree\\AshenTree\ntexID=32\ntexFile=ReplaceableTextures\\AshenvaleTree\\AshenTree\n",
        );
        let profile = &profiles["attr"];
        assert_eq!(profile.num_variations, Some(5));
        assert_eq!(
            profile.file.as_deref(),
            Some(r"Doodads\Terrain\AshenTree\AshenTree")
        );
        assert_eq!(profile.replaceable_texture_id, Some(32));
        assert_eq!(
            profile.replaceable_texture.as_deref(),
            Some(r"ReplaceableTextures\AshenvaleTree\AshenTree")
        );
    }

    #[test]
    fn animation_manifest_only_reports_emitted_gltf_clips() {
        let gltf = json!({
            "animations": [{
                "name": "Death",
                "extras": {
                    "wc3StartMs": 1000,
                    "wc3EndMs": 2500,
                    "wc3MoveSpeed": 0.0,
                    "wc3NonLooping": true
                }
            }]
        });
        let animations = animation_manifests_from_gltf(&gltf).unwrap();
        assert_eq!(animations.len(), 1);
        assert_eq!(animations[0].name, "Death");
        assert_eq!(animations[0].start_ms, 1000);
        assert_eq!(animations[0].end_ms, 2500);
        assert!(animations[0].non_looping);
    }

    #[test]
    fn building_lifecycle_selector_honors_required_animation_properties() {
        let animation = |name: &str, non_looping: bool| AnimationManifest {
            name: name.to_owned(),
            start_ms: 0,
            end_ms: 1000,
            move_speed: 0.0,
            non_looping,
        };
        let animations = vec![
            animation("Birth", true),
            animation("Stand", false),
            animation("Stand Work", false),
            animation("Birth Upgrade First", true),
            animation("Stand Upgrade First", false),
            animation("Birth Upgrade Second", true),
            animation("Stand Upgrade Second", false),
            animation("Death", true),
        ];

        assert_eq!(
            select_building_lifecycle_animations(
                &animations,
                &["upgrade".to_owned(), "second".to_owned()],
            ),
            BuildingLifecycleAnimationManifest {
                birth: Some("Birth Upgrade Second".to_owned()),
                stand: Some("Stand Upgrade Second".to_owned()),
                death: Some("Death".to_owned()),
            }
        );
    }

    #[test]
    fn building_lifecycle_selector_supports_combined_wc3_sequence_tags() {
        let animation = |name: &str, non_looping: bool| AnimationManifest {
            name: name.to_owned(),
            start_ms: 0,
            end_ms: 1000,
            move_speed: 0.0,
            non_looping,
        };
        let animations = vec![
            animation("Birth Alternate", true),
            animation("stand birth alternate work upgrade first second", true),
            animation("Stand Alternate Upgrade First Second", false),
            animation("Death Alternate", true),
            animation("Death", true),
        ];
        let properties = [
            "Stand".to_owned(),
            "Alternate".to_owned(),
            "Upgrade".to_owned(),
            "First".to_owned(),
        ];

        assert_eq!(
            select_building_lifecycle_animations(&animations, &properties),
            BuildingLifecycleAnimationManifest {
                birth: Some("stand birth alternate work upgrade first second".to_owned()),
                stand: Some("Stand Alternate Upgrade First Second".to_owned()),
                death: Some("Death Alternate".to_owned()),
            }
        );
    }

    #[test]
    fn building_lifecycle_selector_prefers_non_looping_birth_and_death() {
        let animations = vec![
            AnimationManifest {
                name: "Birth".to_owned(),
                start_ms: 0,
                end_ms: 1000,
                move_speed: 0.0,
                non_looping: false,
            },
            AnimationManifest {
                name: "Birth Alternate".to_owned(),
                start_ms: 0,
                end_ms: 1000,
                move_speed: 0.0,
                non_looping: true,
            },
            AnimationManifest {
                name: "Death".to_owned(),
                start_ms: 0,
                end_ms: 1000,
                move_speed: 0.0,
                non_looping: true,
            },
        ];
        let selected = select_building_lifecycle_animations(&animations, &[]);
        assert_eq!(selected.birth.as_deref(), Some("Birth Alternate"));
        assert_eq!(selected.death.as_deref(), Some("Death"));
    }

    #[test]
    fn wide_skin_data_preserves_joint_indices_above_255() {
        let mut geoset = whiteout::mdx::Geoset::new();
        geoset.set_vertex_positions(&[whiteout::math::Vector3f::default()]);
        geoset.set_skin_data(&[300, 0, 0, 0, 255, 0, 0, 0]);

        let skeleton = SkeletonBuild {
            joint_nodes: vec![1; 301],
            ..Default::default()
        };

        let skin = build_geoset_skin(&geoset, &skeleton)
            .unwrap()
            .expect("wide skin data should preserve 16-bit joint indices");
        assert_eq!(skin.joints_0[0], [300, 0, 0, 0]);
        assert_eq!(skin.weights_0[0], [1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn classic_skin_truncates_to_four_equal_influences_for_bevy() {
        let mut geoset = whiteout::mdx::Geoset::new();
        geoset.set_vertex_positions(&[whiteout::math::Vector3f::default()]);
        geoset.set_vertex_groups(&[0]);
        geoset.set_matrix_groups(&[7]);
        geoset.set_matrix_indices(&[10, 11, 12, 13, 14, 15, 16]);

        let mut skeleton = SkeletonBuild {
            joint_nodes: vec![1; 7],
            ..Default::default()
        };
        for (joint, object_id) in (10u32..=16).enumerate() {
            skeleton.joint_by_object.insert(object_id, joint as u16);
        }

        let skin = build_geoset_skin(&geoset, &skeleton)
            .unwrap()
            .expect("classic geoset should be skinned");
        assert_eq!(skin.joints_0[0], [0, 1, 2, 3]);
        assert_eq!(skin.weights_0[0], [0.25; 4]);
        let total: f32 = skin.weights_0[0].iter().sum();
        assert!((total - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn geoset_animation_manifest_preserves_exact_alpha_and_color_tracks() {
        let mut model = Model::new();
        model.resize_geoset_animations(1);
        {
            let mut animation = model.geoset_animations_mut(0).expect("geoset animation");
            animation.set_geoset_id(3);
            animation.set_alpha(0.6);
            animation.set_color(whiteout::math::Vector3f {
                x: 0.2,
                y: 0.4,
                z: 0.8,
            });
            {
                let mut alpha = animation.alpha_tracks_mut();
                alpha.set_is_used(true);
                alpha.set_interpolation_type(InterpolationType::Linear);
                alpha.set_global_sequence_id(NO_GLOBAL_SEQUENCE);
                alpha.set_key_count(3);
                alpha.set_timestamps(&[0, 250, 500]);
                alpha.set_keys(&[0.0, 0.5, 1.0]);
            }
            {
                let mut color = animation.color_tracks_mut();
                color.set_is_used(true);
                color.set_interpolation_type(InterpolationType::Linear);
                color.set_global_sequence_id(NO_GLOBAL_SEQUENCE);
                color.set_key_count(2);
                color.set_timestamps(&[0, 500]);
                color.set_keys(&[
                    whiteout::math::Vector3f {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    whiteout::math::Vector3f {
                        x: 0.0,
                        y: 1.0,
                        z: 0.0,
                    },
                ]);
            }
        }

        let manifests =
            geoset_animation_manifests(&model).expect("geoset animation should serialize");
        assert_eq!(manifests.len(), 1);
        let animation = &manifests[0];
        assert_eq!(animation.geoset_id, 3);
        assert_eq!(animation.alpha, 0.6);
        assert_eq!(animation.color, [0.2, 0.4, 0.8]);
        assert_eq!(
            animation.alpha_track.as_ref().expect("alpha").values,
            [0.0, 0.5, 1.0]
        );
        assert_eq!(
            animation.color_track.as_ref().expect("color").values,
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        );
    }

    #[test]
    fn geoset_alpha_maps_to_binary_node_visibility() {
        assert_eq!(visibility_scale(0.0), [0.0; 3]);
        assert_eq!(visibility_scale(0.001), [0.0; 3]);
        assert_eq!(visibility_scale(0.01), [1.0; 3]);
        assert_eq!(visibility_scale(1.0), [1.0; 3]);
    }

    #[test]
    fn step_scalar_track_keeps_hidden_geoset_hidden_until_key_change() {
        let mut track = TrackF32::new();
        track.set_is_used(true);
        track.set_interpolation_type(InterpolationType::None);
        track.set_global_sequence_id(NO_GLOBAL_SEQUENCE);
        track.set_key_count(2);
        track.set_timestamps(&[100, 200]);
        track.set_keys(&[0.0, 1.0]);
        validate_f32_track(&track).expect("track layout must be valid");
        assert_eq!(evaluate_f32(&track, &[0, 1], 100, 200, 150), 0.0);
        assert_eq!(evaluate_f32(&track, &[0, 1], 100, 200, 200), 1.0);
    }

    #[test]
    fn legacy_particle_manifest_preserves_animated_scalar_tracks() {
        let mut model = Model::new();
        model.resize_sequences(1);
        {
            let mut sequence = model.sequences_mut(0).expect("sequence");
            sequence.set_name("Death");
            sequence.set_interval_start(100);
            sequence.set_interval_end(500);
            sequence.set_flags(SequenceFlag::NonLooping);
        }
        model.set_global_sequences(&[750]);
        model.resize_particle_emitters(1);
        {
            let mut emitter = model.particle_emitters_mut(0).expect("particle emitter");
            emitter.set_emission_rate(7.0);
            emitter.set_spawn_model_file_name(r"SharedModels\Smoke1_Green.MDL");
            {
                let mut track = emitter.emission_rate_tracks_mut();
                track.set_is_used(true);
                track.set_interpolation_type(InterpolationType::Linear);
                track.set_global_sequence_id(NO_GLOBAL_SEQUENCE);
                track.set_key_count(2);
                track.set_timestamps(&[100, 400]);
                track.set_keys(&[2.0, 9.0]);
            }
            let mut node = emitter.node_mut();
            node.set_object_id(0);
            node.set_name("Legacy Particle");
        }
        model.set_pivot_points(&[whiteout::math::Vector3f::default()]);

        let emitters =
            model_particle_emitter_manifests(&model).expect("legacy emitters should serialize");
        assert_eq!(emitters.len(), 1);
        let emitter = &emitters[0];
        assert_eq!(emitter.emission_rate, 7.0);
        assert_eq!(emitter.spawn_model, r"SharedModels\Smoke1_Green.MDL");
        assert_eq!(
            emitter.gltf.as_deref(),
            Some("models/sharedmodels__smoke1_green.gltf")
        );
        assert_eq!(emitter.sequence_windows.len(), 1);
        assert_eq!(emitter.sequence_windows[0].name, "Death");
        assert!(emitter.sequence_windows[0].non_looping);
        assert_eq!(emitter.global_sequence_durations_ms, [750]);
        let track = emitter
            .emission_rate_track
            .as_ref()
            .expect("animated emission rate should be preserved");
        assert_eq!(
            track.interpolation,
            ScalarTrackInterpolationManifest::Linear
        );
        assert_eq!(track.timestamps, [100, 400]);
        assert_eq!(track.values, [2.0, 9.0]);

        let mut binary = BinaryBuilder::default();
        let skeleton =
            build_skeleton(&model, &mut binary, &mut Vec::new()).expect("skeleton should build");
        assert_eq!(
            skeleton.nodes[0]["extras"]["wc3ModelParticleEmitter"]["gltf"],
            json!("models/sharedmodels__smoke1_green.gltf")
        );
    }

    #[test]
    fn model_dependencies_include_attachment_particle_and_event_children() {
        let model = ModelManifest {
            source_model: "root.mdx".to_owned(),
            source_casc_path: "root.mdx".to_owned(),
            gltf: "models/root.gltf".to_owned(),
            bin: "models/root.bin".to_owned(),
            geosets: 0,
            bones: 0,
            features: ModelFeatureManifest::default(),
            overhead_position: None,
            animations: Vec::new(),
            textures: Vec::new(),
            materials: Vec::new(),
            geoset_animations: Vec::new(),
            particle_emitters: Vec::new(),
            model_particle_emitters: vec![
                ModelParticleEmitterManifest {
                    object_id: 1,
                    name: "spawn".to_owned(),
                    position: [0.0; 3],
                    emission_rate: 1.0,
                    gravity: 0.0,
                    longitude: 0.0,
                    latitude: 0.0,
                    lifespan: 1.0,
                    initial_velocity: 0.0,
                    spawn_model: r"SharedModels\Smoke1_Green.MDL".to_owned(),
                    gltf: Some("models/sharedmodels__smoke1_green.gltf".to_owned()),
                    emission_rate_track: None,
                    gravity_track: None,
                    longitude_track: None,
                    latitude_track: None,
                    lifespan_track: None,
                    speed_track: None,
                    visibility_track: None,
                    sequence_windows: Vec::new(),
                    global_sequence_durations_ms: Vec::new(),
                },
                ModelParticleEmitterManifest {
                    object_id: 2,
                    name: "hidden".to_owned(),
                    position: [0.0; 3],
                    emission_rate: 1.0,
                    gravity: 0.0,
                    longitude: 0.0,
                    latitude: 0.0,
                    lifespan: 1.0,
                    initial_velocity: 0.0,
                    spawn_model: "none.mdl".to_owned(),
                    gltf: None,
                    emission_rate_track: None,
                    gravity_track: None,
                    longitude_track: None,
                    latitude_track: None,
                    lifespan_track: None,
                    speed_track: None,
                    visibility_track: None,
                    sequence_windows: Vec::new(),
                    global_sequence_durations_ms: Vec::new(),
                },
            ],
            ribbon_emitters: Vec::new(),
            attachments: vec![
                AttachmentManifest {
                    object_id: 3,
                    name: "birth".to_owned(),
                    position: [0.0; 3],
                    path: r"SharedModels\NEBirth.MDL".to_owned(),
                    gltf: Some("models/sharedmodels__nebirth.gltf".to_owned()),
                    visibility_track: None,
                    sequence_windows: Vec::new(),
                    global_sequence_durations_ms: Vec::new(),
                },
                AttachmentManifest {
                    object_id: 4,
                    name: "duplicate".to_owned(),
                    position: [0.0; 3],
                    path: r"SharedModels\NEBirth.mdx".to_owned(),
                    gltf: Some("models/sharedmodels__nebirth.gltf".to_owned()),
                    visibility_track: None,
                    sequence_windows: Vec::new(),
                    global_sequence_durations_ms: Vec::new(),
                },
            ],
            event_objects: vec![
                EventObjectManifest {
                    object_id: 5,
                    name: "SPNxUDIS".to_owned(),
                    position: [0.0; 3],
                    kind: EventObjectKindManifest::Spawn,
                    event_code: Some("UDIS".to_owned()),
                    lookup_resolved: true,
                    spawn_model: Some(r"Objects\Spawnmodels\Undead\UndeadDissipate.mdx".to_owned()),
                    gltf: Some(
                        "models/objects__spawnmodels__undead__undeaddissipate.gltf".to_owned(),
                    ),
                    sound: None,
                    splat: None,
                    global_sequence_id: None,
                    event_track_times: vec![100],
                    sequence_windows: Vec::new(),
                    global_sequence_durations_ms: Vec::new(),
                },
                EventObjectManifest {
                    object_id: 6,
                    name: "SPNxMISS".to_owned(),
                    position: [0.0; 3],
                    kind: EventObjectKindManifest::Spawn,
                    event_code: Some("MISS".to_owned()),
                    lookup_resolved: false,
                    spawn_model: Some(r"Objects\Missing.mdx".to_owned()),
                    gltf: None,
                    sound: None,
                    splat: None,
                    global_sequence_id: None,
                    event_track_times: vec![100],
                    sequence_windows: Vec::new(),
                    global_sequence_durations_ms: Vec::new(),
                },
            ],
            lights: Vec::new(),
            warnings: Vec::new(),
        };

        assert_eq!(
            model_dependency_paths(&model),
            [
                r"Objects\Spawnmodels\Undead\UndeadDissipate.mdx".to_owned(),
                r"SharedModels\NEBirth.mdx".to_owned(),
                r"SharedModels\Smoke1_Green.mdx".to_owned(),
            ]
        );
    }

    #[test]
    fn attachment_manifest_preserves_child_path_pivot_and_visibility() {
        let mut model = Model::new();
        model.resize_sequences(1);
        {
            let mut sequence = model.sequences_mut(0).expect("sequence");
            sequence.set_name("Birth");
            sequence.set_interval_start(0);
            sequence.set_interval_end(600);
            sequence.set_flags(SequenceFlag::NonLooping);
        }
        model.set_global_sequences(&[750]);
        model.resize_attachments(1);
        {
            let mut attachment = model.attachments_mut(0).expect("attachment");
            attachment.set_path(r"SharedModels\NEBirth.MDL");
            {
                let mut visibility = attachment.visibility_tracks_mut();
                visibility.set_is_used(true);
                visibility.set_interpolation_type(InterpolationType::None);
                visibility.set_global_sequence_id(NO_GLOBAL_SEQUENCE);
                visibility.set_key_count(2);
                visibility.set_timestamps(&[0, 500]);
                visibility.set_keys(&[0.0, 1.0]);
            }
            let mut node = attachment.node_mut();
            node.set_object_id(0);
            node.set_name("Birth Attachment");
        }
        model.set_pivot_points(&[whiteout::math::Vector3f {
            x: 10.0,
            y: 20.0,
            z: 30.0,
        }]);

        let attachments = attachment_manifests(&model).expect("attachments should serialize");
        assert_eq!(attachments.len(), 1);
        let attachment = &attachments[0];
        assert_eq!(attachment.object_id, 0);
        assert_eq!(attachment.name, "Birth Attachment");
        assert_eq!(attachment.position, [10.0, 30.0, -20.0]);
        assert_eq!(attachment.path, r"SharedModels\NEBirth.MDL");
        assert_eq!(
            attachment.gltf.as_deref(),
            Some("models/sharedmodels__nebirth.gltf")
        );
        assert_eq!(attachment.sequence_windows.len(), 1);
        assert_eq!(attachment.sequence_windows[0].name, "Birth");
        assert!(attachment.sequence_windows[0].non_looping);
        assert_eq!(attachment.global_sequence_durations_ms, [750]);
        let visibility = attachment
            .visibility_track
            .as_ref()
            .expect("visibility track should be preserved");
        assert_eq!(visibility.timestamps, [0, 500]);
        assert_eq!(visibility.values, [0.0, 1.0]);

        let mut binary = BinaryBuilder::default();
        let skeleton =
            build_skeleton(&model, &mut binary, &mut Vec::new()).expect("skeleton should build");
        assert_eq!(
            skeleton.nodes[0]["extras"]["wc3Attachment"]["gltf"],
            json!("models/sharedmodels__nebirth.gltf")
        );
        assert_eq!(
            skeleton.nodes[0]["extras"]["wc3Attachment"]["sequence_windows"][0]["name"],
            json!("Birth")
        );
    }

    #[test]
    fn event_object_manifest_preserves_timeline_and_global_sequence() {
        let mut model = Model::new();
        model.resize_event_objects(1);
        {
            let mut event = model.event_objects_mut(0).expect("event object");
            event.set_global_sequence_id(3);
            event.set_event_track_times(&[120, 480, 900]);
            let mut node = event.node_mut();
            node.set_object_id(0);
            node.set_name("SNDxFootstep");
        }
        model.set_pivot_points(&[whiteout::math::Vector3f {
            x: 4.0,
            y: 5.0,
            z: 6.0,
        }]);

        let manifests = event_object_manifests(
            &model,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            |_| false,
        );
        assert_eq!(manifests.len(), 1);
        let event = &manifests[0];
        assert_eq!(event.object_id, 0);
        assert_eq!(event.name, "SNDxFootstep");
        assert_eq!(event.position, [4.0, 6.0, -5.0]);
        assert_eq!(event.kind, EventObjectKindManifest::Sound);
        assert_eq!(event.event_code.as_deref(), Some("FOOT"));
        assert!(!event.lookup_resolved);
        assert_eq!(event.global_sequence_id, Some(3));
        assert_eq!(event.event_track_times, [120, 480, 900]);
    }

    #[test]
    fn sound_event_manifest_resolves_animation_sound_metadata() {
        let mut model = Model::new();
        model.resize_event_objects(1);
        {
            let mut event = model.event_objects_mut(0).expect("event object");
            event.set_event_track_times(&[250]);
            let mut node = event.node_mut();
            node.set_object_id(0);
            node.set_name("SNDxFDFR");
        }
        model.set_pivot_points(&[whiteout::math::Vector3f::default()]);

        let sounds = BTreeMap::from([(
            "FDFR".to_owned(),
            AnimationSoundSpec {
                sound_name: "DeepFootstep2".to_owned(),
                source_files: vec![
                    r"Sound\Units\Footsteps\Step1.flac".to_owned(),
                    r"Sound\Units\Footsteps\Step2.flac".to_owned(),
                ],
                silent: false,
                volume: 40.0,
                volume_variance: 0.0,
                pitch: 1.0,
                pitch_variance: 0.1,
                maximum_concurrent_instances: -1,
                priority: 3,
                channel: 11,
                flags: "WANT3D,RANDOMPITCH".to_owned(),
                min_distance: 300.0,
                max_distance: 3_500.0,
                distance_cutoff: 3_000.0,
                eax_flags: "SpellsEAX".to_owned(),
                rolloff_points: "_".to_owned(),
            },
        )]);
        let manifests = event_object_manifests(
            &model,
            &BTreeMap::new(),
            &sounds,
            &BTreeMap::new(),
            &BTreeMap::new(),
            |_| false,
        );
        let event = &manifests[0];
        assert!(event.lookup_resolved);
        let sound = event.sound.as_ref().expect("sound metadata");
        assert_eq!(sound.sound_name, "DeepFootstep2");
        assert_eq!(sound.source_files.len(), 2);
        assert!(!sound.silent);
        assert_eq!(sound.volume, 40.0);
        assert_eq!(sound.pitch_variance, 0.1);
        assert_eq!(sound.flags, "WANT3D,RANDOMPITCH");
        assert_eq!(sound.min_distance, 300.0);
        assert_eq!(sound.distance_cutoff, 3_000.0);
    }

    #[test]
    fn spawn_event_manifest_resolves_child_model_and_gltf() {
        let mut model = Model::new();
        model.resize_sequences(1);
        {
            let mut sequence = model.sequences_mut(0).expect("sequence");
            sequence.set_name("Death");
            sequence.set_interval_start(1_000);
            sequence.set_interval_end(2_000);
            sequence.set_flags(SequenceFlag::NonLooping);
        }
        model.resize_event_objects(1);
        {
            let mut event = model.event_objects_mut(0).expect("event object");
            event.set_event_track_times(&[1_500]);
            let mut node = event.node_mut();
            node.set_object_id(0);
            node.set_name("SPNxUDIS");
        }
        model.set_pivot_points(&[whiteout::math::Vector3f::default()]);

        let spawn_catalog = BTreeMap::from([(
            "UDIS".to_owned(),
            r"Objects\Spawnmodels\Undead\UndeadDissipate\UndeadDissipate.mdx".to_owned(),
        )]);
        let manifests = event_object_manifests(
            &model,
            &spawn_catalog,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            |_| true,
        );
        let event = &manifests[0];
        assert_eq!(event.kind, EventObjectKindManifest::Spawn);
        assert_eq!(event.event_code.as_deref(), Some("UDIS"));
        assert!(event.lookup_resolved);
        assert_eq!(
            event.spawn_model.as_deref(),
            Some(r"Objects\Spawnmodels\Undead\UndeadDissipate\UndeadDissipate.mdx")
        );
        assert_eq!(
            event.gltf.as_deref(),
            Some("models/objects__spawnmodels__undead__undeaddissipate__undeaddissipate.gltf")
        );
        assert_eq!(event.sequence_windows.len(), 1);
        assert_eq!(event.sequence_windows[0].name, "Death");

        let unresolved = event_object_manifests(
            &model,
            &spawn_catalog,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            |_| false,
        );
        let unresolved = &unresolved[0];
        assert!(!unresolved.lookup_resolved);
        assert!(unresolved.spawn_model.is_some());
        assert!(unresolved.gltf.is_none());
    }

    #[test]
    #[ignore = "diagnostic helper for a local Warcraft install"]
    fn dump_wc3_animation_sound_tables() {
        let install = std::env::var("WC3_INSTALL").expect("WC3_INSTALL");
        let storage = CascStorage::open(&install, None).expect("open Warcraft III CASC storage");
        let text = storage
            .read_file(r"war3.w3mod:ui\soundinfo\animsounds.slk")
            .expect("read AnimSounds");
        let rows = parse_sylk_cells(&String::from_utf8_lossy(&text));
        for (row, cells) in rows.iter().take(8) {
            eprintln!("{row}: {cells:?}");
        }
        for (row, cells) in &rows {
            if cells.values().any(|value| {
                value.eq_ignore_ascii_case("DHLS")
                    || value.eq_ignore_ascii_case("DOLS")
                    || value.eq_ignore_ascii_case("FDFR")
            }) {
                eprintln!("MATCH {row}: {cells:?}");
            }
        }
    }

    #[test]
    fn animation_sound_catalog_parses_variants_and_authored_parameters() {
        let text = concat!(
            "ID;PWXL;N;E\n",
            "C;X1;Y1;K\"SoundName\"\n",
            "C;X2;K\"AnimationEventCode\"\n",
            "C;X3;K\"FileNames\"\n",
            "C;X4;K\"Volume\"\n",
            "C;X5;K\"PitchVariance\"\n",
            "C;X6;K\"Priority\"\n",
            "C;X7;K\"Channel\"\n",
            "C;X8;K\"Flags\"\n",
            "C;X9;K\"MinDistance\"\n",
            "C;X10;K\"MaxDistance\"\n",
            "C;X11;K\"DistanceCutoff\"\n",
            "C;X12;K\"EAXFlags\"\n",
            "C;X13;K\"RolloffPoints\"\n",
            "C;X1;Y2;K\"DeepFootstep2\"\n",
            "C;X2;KFDFR\n",
            "C;X3;K\"Sound/Units/Footsteps/Step1.flac, Sound/Units/Footsteps/Step2.flac\"\n",
            "C;X4;K40\n",
            "C;X5;K0.1\n",
            "C;X6;K3\n",
            "C;X7;K11\n",
            "C;X8;K\"WANT3D,RANDOMPITCH\"\n",
            "C;X9;K300\n",
            "C;X10;K3500\n",
            "C;X11;K3000\n",
            "C;X12;K\"SpellsEAX\"\n",
            "C;X13;K_\n",
            "C;X1;Y3;K\"SentinelMissileHit\"\n",
            "C;X2;KMSEH\n",
            "C;X3;K_\n",
        );
        let catalog = parse_animation_sound_catalog(text).expect("sound catalog");
        let sound = &catalog["FDFR"];
        assert_eq!(sound.sound_name, "DeepFootstep2");
        assert_eq!(
            sound.source_files,
            [
                r"Sound\Units\Footsteps\Step1.flac",
                r"Sound\Units\Footsteps\Step2.flac"
            ]
        );
        assert_eq!(sound.volume, 40.0);
        assert_eq!(sound.pitch_variance, 0.1);
        assert_eq!(sound.priority, 3);
        assert_eq!(sound.channel, 11);
        assert_eq!(sound.flags, "WANT3D,RANDOMPITCH");
        assert_eq!(sound.min_distance, 300.0);
        assert_eq!(sound.max_distance, 3_500.0);
        assert_eq!(sound.distance_cutoff, 3_000.0);
        assert_eq!(sound.eax_flags, "SpellsEAX");
        assert_eq!(sound.rolloff_points, "_");
        assert!(!sound.silent);

        let silent = &catalog["MSEH"];
        assert!(silent.silent);
        assert!(silent.source_files.is_empty());
    }

    #[test]
    fn splat_catalog_preserves_classic_atlas_and_lifecycle() {
        let text = concat!(
            "ID;PWXL;N;E\n",
            "C;X1;Y1;K\"Name\"\nC;X2;K\"Dir\"\nC;X3;K\"file\"\n",
            "C;X4;K\"Rows\"\nC;X5;K\"Columns\"\nC;X6;K\"BlendMode\"\n",
            "C;X7;K\"Scale\"\nC;X8;K\"Lifespan\"\nC;X9;K\"Decay\"\n",
            "C;X10;K\"UVLifespanStart\"\nC;X11;K\"UVLifespanEnd\"\n",
            "C;X12;K\"StartR\"\nC;X13;K\"StartA\"\n",
            "C;X1;Y2;K\"DBL0\"\nC;X2;K\"ReplaceableTextures\\Splats\"\n",
            "C;X3;K\"Splat01Mature\"\nC;X4;K16\nC;X5;K16\nC;X6;K1\n",
            "C;X7;K50\nC;X8;K2\nC;X9;K120\nC;X10;K0\nC;X11;K15\n",
            "C;X12;K60\nC;X13;K200\n"
        );
        let catalog = parse_splat_service_catalog(text, false).unwrap();
        let splat = &catalog["DBL0"];
        assert_eq!(
            splat.source_texture,
            r"ReplaceableTextures\Splats\Splat01Mature"
        );
        assert_eq!((splat.rows, splat.columns), (16, 16));
        assert_eq!(splat.uv_lifespan, [0, 15]);
        assert_eq!(splat.colors[0], [60, 255, 255, 200]);
    }

    #[test]
    fn uber_splat_catalog_preserves_birth_pause_and_decay() {
        let text = concat!(
            "ID;PWXL;N;E\n",
            "C;X1;Y1;K\"Name\"\nC;X2;K\"Dir\"\nC;X3;K\"file\"\n",
            "C;X4;K\"BlendMode\"\nC;X5;K\"Scale\"\n",
            "C;X6;K\"BirthTime\"\nC;X7;K\"PauseTime\"\nC;X8;K\"Decay\"\n",
            "C;X9;K\"StartA\"\nC;X10;K\"MiddleA\"\nC;X11;K\"EndA\"\n",
            "C;X1;Y2;K\"LSDS\"\nC;X2;K\"ReplaceableTextures\\Splats\"\n",
            "C;X3;K\"DirtUberSplat\"\nC;X4;K0\nC;X5;K110\n",
            "C;X6;K1\nC;X7;K5\nC;X8;K2\n",
            "C;X9;K0\nC;X10;K255\nC;X11;K0\n",
        );
        let catalog = parse_splat_service_catalog(text, true).unwrap();
        let splat = &catalog["LSDS"];
        assert_eq!((splat.rows, splat.columns), (1, 1));
        assert_eq!(
            (splat.birth_time, splat.pause_time, splat.decay),
            (1.0, 5.0, 2.0)
        );
        assert_eq!(
            [splat.colors[0][3], splat.colors[1][3], splat.colors[2][3]],
            [0, 255, 0]
        );
    }

    #[test]
    fn sylk_parser_preserves_sparse_row_coordinates() {
        let table = parse_sylk_cells(
            "ID;PWXL;N;E\nC;X1;Y1;K\"Name\"\nC;X2;K\"Model\"\nC;X1;Y2;K\"UDIS\"\nC;X2;K\"Objects\\Spawn.mdl\"\n",
        );
        assert_eq!(table[&1][&1], "Name");
        assert_eq!(table[&1][&2], "Model");
        assert_eq!(table[&2][&1], "UDIS");
        assert_eq!(table[&2][&2], r"Objects\Spawn.mdl");
    }

    #[test]
    fn scalar_track_manifest_preserves_smooth_keys_tangents_and_global_sequence() {
        let mut track = TrackF32::new();
        track.set_is_used(true);
        track.set_interpolation_type(InterpolationType::Hermite);
        track.set_global_sequence_id(7);
        track.set_key_count(2);
        track.set_timestamps(&[100, 250]);
        track.set_keys(&[1.0, 0.5, 1.5, 2.0, 1.7, 2.3]);

        let manifest = scalar_track_manifest(&track)
            .expect("track should serialize")
            .expect("used track should be retained");
        assert_eq!(
            manifest.interpolation,
            ScalarTrackInterpolationManifest::Hermite
        );
        assert_eq!(manifest.global_sequence_id, Some(7));
        assert_eq!(manifest.timestamps, [100, 250]);
        assert_eq!(manifest.values, [1.0, 2.0]);
        assert_eq!(manifest.in_tangents, [0.5, 1.7]);
        assert_eq!(manifest.out_tangents, [1.5, 2.3]);
    }

    #[test]
    fn scalar_track_manifest_omits_unused_tracks() {
        let track = TrackF32::new();
        assert!(
            scalar_track_manifest(&track)
                .expect("unused track should be valid")
                .is_none()
        );
    }

    #[test]
    fn smooth_vector_interpolation_keeps_endpoints() {
        let a = [1.0, 2.0, 3.0];
        let b = [4.0, 5.0, 6.0];
        let c = [7.0, 8.0, 9.0];
        let d = [10.0, 11.0, 12.0];
        assert_eq!(hermite3(a, b, c, d, 0.0), a);
        assert_eq!(hermite3(a, b, c, d, 1.0), d);
        assert_eq!(bezier3(a, b, c, d, 0.0), a);
        assert_eq!(bezier3(a, b, c, d, 1.0), d);
    }

    #[test]
    fn quaternion_slerp_remains_normalized() {
        let a = [0.0, 0.0, 0.0, 1.0];
        let b = normalize_quat([0.2, 0.3, 0.4, 0.8]);
        let q = slerp_quat(a, b, 0.5);
        let length = (q.iter().map(|value| value * value).sum::<f32>()).sqrt();
        assert!((length - 1.0).abs() < 1.0e-6);
    }
}
