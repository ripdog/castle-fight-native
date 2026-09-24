use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Value, json};
use whiteout::{
    Bytes,
    casc::Storage as CascStorage,
    mdx::{
        InterpolationType, Layer, LayerFilterMode, LayerShadingFlag, LayerSlotType, Model, Node,
        NodeFlag, Parser as MdxParser, ParticleEmitter2, SequenceFlag, TrackF32, TrackQuaternion,
        TrackU32, TrackVector3f,
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
const WHITEOUT_STABLE_MAX_MDX_VERSION: u32 = 1200;
const WC3_3_MDX_VERSION: u32 = 1800;
const MAX_MDX_INPUT_BYTES: usize = 64 * 1024 * 1024;
const WC3_PLAYER_COLOR_COUNT: u8 = 24;
const STOCK_BUILDING_ART_CATEGORIES: [&str; 7] = [
    "human", "orc", "undead", "nightelf", "naga", "other", "demon",
];

type TextureExport = (Vec<TextureManifest>, Vec<Option<usize>>);
type GltfBuildOutput = (Value, Vec<u8>, Vec<String>);
type WideSkinRewrite = (Option<Vec<u8>>, usize);

#[derive(Debug, Default)]
struct MdxCompatibilityPlan {
    patches: Vec<(u64, [u8; 4])>,
    warnings: Vec<String>,
}

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
    pub particle_emitters: Vec<ParticleEmitter2Manifest>,
    pub model_particle_emitters: Vec<ModelParticleEmitterManifest>,
    pub ribbon_emitters: Vec<RibbonEmitterManifest>,
    pub attachments: Vec<AttachmentManifest>,
    pub event_objects: Vec<EventObjectManifest>,
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
    pub segment_colors: [[f32; 3]; 3],
    pub segment_alpha: [u8; 3],
    pub segment_scaling: [f32; 3],
    pub texture: Option<String>,
    pub squirt: bool,
    pub replaceable_id: u32,
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
}

#[derive(Debug, Clone, Serialize)]
pub struct AttachmentManifest {
    pub object_id: u32,
    pub name: String,
    pub position: [f32; 3],
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility_track: Option<ScalarTrackManifest>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventObjectManifest {
    pub object_id: u32,
    pub name: String,
    pub position: [f32; 3],
    pub global_sequence_id: Option<u32>,
    pub event_track_times: Vec<u32>,
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
    pub filter_mode: String,
    pub texture: Option<String>,
    pub gravity: f32,
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
        self.map_storage
            .as_ref()
            .is_some_and(|storage| storage.file_exists(logical_path))
            || casc_asset_paths(logical_path)
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

        // The parsed Model owns its data. Do not keep a second native buffer containing the
        // source MDX alive while textures and glTF buffers are built.
        drop(model_bytes);

        let (texture_manifests, gltf_texture_indices) =
            self.export_model_textures(&model, replaceable_textures)?;
        let gltf_name = format!("models/{asset_name}.gltf");
        let bin_name = format!("models/{asset_name}.bin");
        let (gltf, bin, material_warnings) = build_gltf(
            &model,
            logical_path,
            &asset_name,
            &gltf_texture_indices,
            &texture_manifests,
            omit_team_glow_geosets,
        )?;
        warnings.extend(material_warnings);

        fs::write(self.output.join(&bin_name), &bin)?;
        let gltf_file = File::create(self.output.join(&gltf_name))?;
        serde_json::to_writer_pretty(BufWriter::new(gltf_file), &gltf)?;

        let animations = animation_manifests_from_gltf(&gltf)?;
        let features = model_feature_manifest(&model);
        let particle_emitters = particle_emitter_2_manifests(&model, &texture_manifests)?;
        let model_particle_emitters = model_particle_emitter_manifests(&model)?;
        let ribbon_emitters = ribbon_emitter_manifests(&model, &texture_manifests)?;
        let attachments = attachment_manifests(&model)?;
        let event_objects = event_object_manifests(&model);

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
            particle_emitters,
            model_particle_emitters,
            ribbon_emitters,
            attachments,
            event_objects,
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
        // whiteoutlib 0.1.7 only understands MDX through v1200. Warcraft III 3.0 ships v1800
        // models whose camera size word stores an 8-bit variant in the high byte; feeding those
        // bytes directly to 0.1.7 can make the parser walk tens of megabytes past EOF and allocate
        // gigabytes from garbage track counts. Preflight the binary and apply only compatibility
        // transformations whose semantics are known before handing it to native code.
        let extension = Path::new(logical_path)
            .extension()
            .and_then(|extension| extension.to_str())
            .filter(|extension| extension.eq_ignore_ascii_case("mdl"))
            .map_or("mdx", |_| "mdl");
        let (rewritten_model, narrowed_skin_streams) = if extension == "mdx" {
            rewrite_wide_skin_streams(model_bytes, logical_path)?
        } else {
            (None, 0)
        };
        let staged_bytes = rewritten_model.as_deref().unwrap_or(model_bytes);
        let mut compatibility = if extension == "mdx" {
            mdx_compatibility_plan(staged_bytes, logical_path)?
        } else {
            MdxCompatibilityPlan::default()
        };
        if narrowed_skin_streams != 0 {
            compatibility.warnings.push(format!(
                "narrowed {narrowed_skin_streams} v1400+ wide SKIN stream(s) to Whiteout 0.1.7's byte representation; all values fit in u8"
            ));
        }
        let staging_path = self.output.join("models").join(format!(
            ".{asset_name}.parse-{}.{}",
            std::process::id(),
            extension
        ));
        fs::write(&staging_path, staged_bytes)?;
        if !compatibility.patches.is_empty() {
            let mut staged = OpenOptions::new().write(true).open(&staging_path)?;
            for (offset, replacement) in &compatibility.patches {
                staged.seek(SeekFrom::Start(*offset))?;
                staged.write_all(replacement)?;
            }
            staged.flush()?;
        }

        let mut parser = MdxParser::new();
        let parsed = parser.parse_file(staging_path.to_string_lossy().as_ref());
        let mut warnings = parser.issues();
        warnings.extend(compatibility.warnings);
        fs::remove_file(&staging_path)?;
        let model = parsed
            .ok_or_else(|| io::Error::other(format!("failed to parse MDX {source_casc_path}")))?;
        Ok((model, warnings))
    }

    fn read_model(&mut self, logical_path: &str) -> Result<(String, Bytes), Box<dyn Error>> {
        if let Some(storage) = &self.map_storage
            && let Some(bytes) = storage.read_file(logical_path)
        {
            return Ok((format!("map:{logical_path}"), bytes));
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

    fn export_model_textures(
        &mut self,
        model: &Model,
        replaceable_textures: &BTreeMap<u32, String>,
    ) -> Result<TextureExport, Box<dyn Error>> {
        let mut manifests = Vec::with_capacity(model.textures_len());
        let mut gltf_indices = Vec::with_capacity(model.textures_len());
        let mut next_gltf_index = 0usize;

        if model
            .textures_iter()
            .any(|texture| texture.replaceable_id() == 2)
        {
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

        for texture in model.textures_iter() {
            let replaceable_id = texture.replaceable_id();
            let model_logical = normalize_texture_path(&texture.file_name());
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

fn rewrite_wide_skin_streams(
    bytes: &[u8],
    logical_path: &str,
) -> Result<WideSkinRewrite, Box<dyn Error>> {
    if bytes.len() < 12 || &bytes[..4] != b"MDLX" {
        return Ok((None, 0));
    }

    let mut version = None;
    let mut offset = 4usize;
    while offset + 8 <= bytes.len() {
        let size = read_u32_le(bytes, offset + 4)? as usize;
        let payload_start = offset + 8;
        let payload_end = payload_start.checked_add(size).ok_or_else(|| {
            io::Error::other(format!("MDX {logical_path} chunk size overflows usize"))
        })?;
        if payload_end > bytes.len() {
            return Err(io::Error::other(format!(
                "MDX {logical_path} top-level chunk overruns the file while checking SKIN data"
            ))
            .into());
        }
        if &bytes[offset..offset + 4] == b"VERS" && size >= 4 {
            version = Some(read_u32_le(bytes, payload_start)?);
        }
        offset = payload_end;
    }
    let Some(version) = version else {
        return Ok((None, 0));
    };
    if version < 1400 {
        return Ok((None, 0));
    }

    let mut output = Vec::with_capacity(bytes.len());
    output.extend_from_slice(b"MDLX");
    let mut rewritten_streams = 0usize;
    offset = 4;
    while offset < bytes.len() {
        let tag = &bytes[offset..offset + 4];
        let size = read_u32_le(bytes, offset + 4)? as usize;
        let payload_start = offset + 8;
        let payload_end = payload_start + size;
        if tag != b"GEOS" {
            output.extend_from_slice(&bytes[offset..payload_end]);
            offset = payload_end;
            continue;
        }

        let mut rewritten_geos = Vec::with_capacity(size);
        let mut geoset_offset = payload_start;
        while geoset_offset < payload_end {
            let inclusive_size = read_u32_le(bytes, geoset_offset)? as usize;
            if inclusive_size < 4 {
                return Err(io::Error::other(format!(
                    "MDX {logical_path} has invalid geoset size {inclusive_size} at byte {geoset_offset}"
                ))
                .into());
            }
            let geoset_end = geoset_offset.checked_add(inclusive_size).ok_or_else(|| {
                io::Error::other(format!("MDX {logical_path} geoset size overflows usize"))
            })?;
            if geoset_end > payload_end {
                return Err(io::Error::other(format!(
                    "MDX {logical_path} geoset at byte {geoset_offset} overruns GEOS chunk"
                ))
                .into());
            }
            let geoset = &bytes[geoset_offset..geoset_end];
            let mut candidates = Vec::new();
            for relative in 4..geoset.len().saturating_sub(7) {
                if &geoset[relative..relative + 4] != b"SKIN" {
                    continue;
                }
                let count = read_u32_le(geoset, relative + 4)? as usize;
                let Some(wide_bytes) = count.checked_mul(2) else {
                    continue;
                };
                let Some(data_end) = relative
                    .checked_add(8)
                    .and_then(|start| start.checked_add(wide_bytes))
                else {
                    continue;
                };
                if data_end <= geoset.len() {
                    candidates.push((relative, count, data_end));
                }
            }
            if candidates.len() > 1 {
                return Err(io::Error::other(format!(
                    "MDX {logical_path} geoset at byte {geoset_offset} has multiple plausible wide SKIN streams; refusing ambiguous rewrite"
                ))
                .into());
            }
            let Some((skin_offset, count, skin_end)) = candidates.first().copied() else {
                rewritten_geos.extend_from_slice(geoset);
                geoset_offset = geoset_end;
                continue;
            };

            let narrowed_size = inclusive_size.checked_sub(count).ok_or_else(|| {
                io::Error::other(format!("MDX {logical_path} wide SKIN size underflow"))
            })?;
            let narrowed_size_u32 = u32::try_from(narrowed_size)
                .map_err(|_| io::Error::other("rewritten MDX geoset exceeds u32 size"))?;
            rewritten_geos.extend_from_slice(&narrowed_size_u32.to_le_bytes());
            rewritten_geos.extend_from_slice(&geoset[4..skin_offset + 8]);
            for pair in geoset[skin_offset + 8..skin_end].as_chunks::<2>().0 {
                let value = u16::from_le_bytes(*pair);
                let narrowed = u8::try_from(value).map_err(|_| {
                    io::Error::other(format!(
                        "MDX {logical_path} v{version} SKIN value {value} exceeds Whiteout 0.1.7's u8 representation"
                    ))
                })?;
                rewritten_geos.push(narrowed);
            }
            rewritten_geos.extend_from_slice(&geoset[skin_end..]);
            rewritten_streams += 1;
            geoset_offset = geoset_end;
        }

        output.extend_from_slice(b"GEOS");
        let geos_size = u32::try_from(rewritten_geos.len())
            .map_err(|_| io::Error::other("rewritten GEOS chunk exceeds u32 size"))?;
        output.extend_from_slice(&geos_size.to_le_bytes());
        output.extend_from_slice(&rewritten_geos);
        offset = payload_end;
    }

    if rewritten_streams == 0 {
        Ok((None, 0))
    } else {
        Ok((Some(output), rewritten_streams))
    }
}

fn mdx_compatibility_plan(
    bytes: &[u8],
    logical_path: &str,
) -> Result<MdxCompatibilityPlan, Box<dyn Error>> {
    if bytes.len() > MAX_MDX_INPUT_BYTES {
        return Err(io::Error::other(format!(
            "MDX {logical_path} is {} bytes; refusing to parse inputs above {} bytes",
            bytes.len(),
            MAX_MDX_INPUT_BYTES
        ))
        .into());
    }
    if bytes.len() < 12 || &bytes[..4] != b"MDLX" {
        return Err(
            io::Error::other(format!("MDX {logical_path} is missing the MDLX header")).into(),
        );
    }

    #[derive(Clone, Copy)]
    struct Chunk<'a> {
        tag: &'a [u8],
        tag_offset: usize,
        payload_start: usize,
        payload_end: usize,
    }

    let mut chunks = Vec::new();
    let mut version = None;
    let mut offset = 4usize;
    while offset < bytes.len() {
        if bytes.len() - offset < 8 {
            return Err(io::Error::other(format!(
                "MDX {logical_path} has a truncated top-level chunk header at byte {offset}"
            ))
            .into());
        }
        let size = read_u32_le(bytes, offset + 4)? as usize;
        let payload_start = offset + 8;
        let payload_end = payload_start.checked_add(size).ok_or_else(|| {
            io::Error::other(format!("MDX {logical_path} chunk size overflows usize"))
        })?;
        if payload_end > bytes.len() {
            return Err(io::Error::other(format!(
                "MDX {logical_path} top-level chunk {:?} overruns the file: end {payload_end}, file {}",
                String::from_utf8_lossy(&bytes[offset..offset + 4]),
                bytes.len()
            ))
            .into());
        }
        let tag = &bytes[offset..offset + 4];
        if tag == b"VERS" {
            if size < 4 {
                return Err(io::Error::other(format!(
                    "MDX {logical_path} has a truncated VERS chunk"
                ))
                .into());
            }
            version = Some(read_u32_le(bytes, payload_start)?);
        }
        chunks.push(Chunk {
            tag,
            tag_offset: offset,
            payload_start,
            payload_end,
        });
        offset = payload_end;
    }

    let version =
        version.ok_or_else(|| io::Error::other(format!("MDX {logical_path} has no VERS chunk")))?;
    if version <= WHITEOUT_STABLE_MAX_MDX_VERSION {
        return Ok(MdxCompatibilityPlan::default());
    }
    if !matches!(version, 1300 | 1400 | 1600 | WC3_3_MDX_VERSION) {
        return Err(io::Error::other(format!(
            "MDX {logical_path} uses unsupported version {version}; stable Whiteout supports through v{WHITEOUT_STABLE_MAX_MDX_VERSION} and this extractor only has bounded compatibility shims through v{WC3_3_MDX_VERSION}"
        ))
        .into());
    }

    let mut plan = MdxCompatibilityPlan::default();
    for chunk in chunks {
        if chunk.tag == b"LITE" && version >= 1300 {
            // v1300+ lights gained fields that Whiteout 0.1.7 does not know about. Castle Fight
            // does not currently consume model-embedded lights, so turn this into an unknown
            // chunk and let Whiteout skip it by its trustworthy top-level byte size.
            plan.patches.push((chunk.tag_offset as u64, *b"XLIT"));
            plan.warnings.push(format!(
                "omitted v{version} embedded light chunk; Whiteout 0.1.7 only understands the v1200 light layout"
            ));
        }

        if chunk.tag == b"CAMS" && version >= WC3_3_MDX_VERSION {
            let mut camera_offset = chunk.payload_start;
            while camera_offset < chunk.payload_end {
                if chunk.payload_end - camera_offset < 4 {
                    return Err(io::Error::other(format!(
                        "MDX {logical_path} has a truncated v{version} camera entry at byte {camera_offset}"
                    ))
                    .into());
                }
                let size_and_variant = read_u32_le(bytes, camera_offset)?;
                let size = size_and_variant & 0x00ff_ffff;
                let variant = size_and_variant >> 24;
                if size < 120 {
                    return Err(io::Error::other(format!(
                        "MDX {logical_path} has invalid v{version} camera size {size} at byte {camera_offset}"
                    ))
                    .into());
                }
                if matches!(variant, 1 | 2) {
                    return Err(io::Error::other(format!(
                        "MDX {logical_path} uses unsupported v{version} camera variant {variant}; variants 1/2 contain extra fixed fields that Whiteout 0.1.7 cannot parse safely"
                    ))
                    .into());
                }
                if variant > 3 {
                    return Err(io::Error::other(format!(
                        "MDX {logical_path} has unknown v{version} camera variant {variant}"
                    ))
                    .into());
                }
                let camera_end = camera_offset.checked_add(size as usize).ok_or_else(|| {
                    io::Error::other(format!(
                        "MDX {logical_path} camera size overflows usize at byte {camera_offset}"
                    ))
                })?;
                if camera_end > chunk.payload_end {
                    return Err(io::Error::other(format!(
                        "MDX {logical_path} camera at byte {camera_offset} overruns CAMS chunk: end {camera_end}, chunk end {}",
                        chunk.payload_end
                    ))
                    .into());
                }
                if variant == 3 {
                    plan.patches
                        .push((camera_offset as u64, size.to_le_bytes()));
                }
                camera_offset = camera_end;
            }
            plan.warnings.push(format!(
                "normalized Warcraft III v{version} camera variant size words for bounded Whiteout 0.1.7 parsing"
            ));
        }
    }
    Ok(plan)
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, Box<dyn Error>> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| io::Error::other("u32 offset overflow"))?;
    let raw: [u8; 4] = bytes
        .get(offset..end)
        .ok_or_else(|| io::Error::other(format!("truncated u32 at byte {offset}")))?
        .try_into()
        .expect("slice length checked above");
    Ok(u32::from_le_bytes(raw))
}

fn texture_has_transparency(texture: &Texture) -> bool {
    match texture.format() {
        PixelFormat::R8
        | PixelFormat::R16
        | PixelFormat::R32F
        | PixelFormat::RG8
        | PixelFormat::RG16
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

fn model_feature_manifest(model: &Model) -> ModelFeatureManifest {
    let mut material_layer_count = 0;
    let mut multilayer_material_count = 0;
    let mut animated_material_alpha_layer_count = 0;
    let mut animated_material_texture_layer_count = 0;
    for material in model.materials_iter() {
        material_layer_count += material.layers_len();
        if material.layers_len() > 1 {
            multilayer_material_count += 1;
        }
        for layer in material.layers_iter() {
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

    ModelFeatureManifest {
        material_count: model.materials_len(),
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
        light_count: model.lights_len(),
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

fn particle_emitter_2_manifests(
    model: &Model,
    textures: &[TextureManifest],
) -> Result<Vec<ParticleEmitter2Manifest>, Box<dyn Error>> {
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
                segment_colors,
                segment_alpha: std::array::from_fn(|index| emitter.segment_alpha(index)),
                segment_scaling: std::array::from_fn(|index| emitter.segment_scaling(index)),
                texture: textures
                    .get(emitter.texture_id() as usize)
                    .and_then(|texture| texture.png.clone()),
                squirt: emitter.squirt() != 0,
                replaceable_id: emitter.replaceable_id(),
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
    let Some(samples) = sample_f32_track(model, track, start, end)? else {
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
                emission_rate_track: scalar_track_manifest(&emitter.emission_rate_tracks())?,
                gravity_track: scalar_track_manifest(&emitter.gravity_tracks())?,
                longitude_track: scalar_track_manifest(&emitter.longitude_tracks())?,
                latitude_track: scalar_track_manifest(&emitter.latitude_tracks())?,
                lifespan_track: scalar_track_manifest(&emitter.lifespan_tracks())?,
                speed_track: scalar_track_manifest(&emitter.speed_tracks())?,
                visibility_track: scalar_track_manifest(&emitter.visibility_tracks())?,
            })
        })
        .collect()
}

fn attachment_manifests(model: &Model) -> Result<Vec<AttachmentManifest>, Box<dyn Error>> {
    model
        .attachments_iter()
        .map(|attachment| {
            let node = attachment.node();
            Ok(AttachmentManifest {
                object_id: node.object_id(),
                name: node.name(),
                position: model_node_position(model, &node),
                path: attachment.path(),
                visibility_track: scalar_track_manifest(&attachment.visibility_tracks())?,
            })
        })
        .collect()
}

fn event_object_manifests(model: &Model) -> Vec<EventObjectManifest> {
    model
        .event_objects_iter()
        .map(|event| {
            let node = event.node();
            EventObjectManifest {
                object_id: node.object_id(),
                name: node.name(),
                position: model_node_position(model, &node),
                global_sequence_id: (event.global_sequence_id() != NO_GLOBAL_SEQUENCE)
                    .then_some(event.global_sequence_id()),
                event_track_times: event.event_track_times().to_vec(),
            }
        })
        .collect()
}

fn ribbon_emitter_manifests(
    model: &Model,
    texture_manifests: &[TextureManifest],
) -> Result<Vec<RibbonEmitterManifest>, Box<dyn Error>> {
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
                filter_mode,
                texture,
                gravity: emitter.gravity(),
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
    let selected = material.layers_iter().find(|layer| {
        texture_manifests
            .get(layer_diffuse_texture_id(layer) as usize)
            .and_then(|texture| texture.png.as_ref())
            .is_some()
    });
    let layer = selected.or_else(|| material.layers(0));
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
    for layer in material.layers_iter() {
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
    let selected = material.layers_iter().find(|layer| {
        texture_manifests
            .get(layer_diffuse_texture_id(layer) as usize)
            .and_then(|texture| texture.png.as_ref())
            .is_some()
    });
    let layer = selected.or_else(|| material.layers(0));
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
        build_materials(model, texture_indices, texture_manifests);
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

        let inherit_mask = NodeFlag::DONT_INHERIT_TRANSLATION
            | NodeFlag::DONT_INHERIT_ROTATION
            | NodeFlag::DONT_INHERIT_SCALING;
        if !(info.flags & inherit_mask).is_empty() {
            warnings.push(format!(
                "node {} ({}) uses WC3 non-inheritance flags {:?}; glTF hierarchy cannot represent those flags exactly",
                info.object_id,
                info.name,
                info.flags & inherit_mask
            ));
        }
    }

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
                "HD geoset skin has {} bytes for {vertex_count} vertices; expected {}",
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
    if let Some(samples) =
        sample_vec3_track(model, &translation_track, start, end, [0.0; 3], |value| {
            add3(rest_translation, wc3_vec3(value[0], value[1], value[2]))
        })?
    {
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
    if let Some(samples) = sample_quat_track(model, &rotation_track, start, end)? {
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
    binary: &mut BinaryBuilder,
    samplers: &mut Vec<Value>,
    channels: &mut Vec<Value>,
    baked_global_sequences: &mut bool,
) -> Result<(), Box<dyn Error>> {
    let track = animation.alpha_tracks();
    if track.global_sequence_id() != NO_GLOBAL_SEQUENCE && track.is_used() {
        *baked_global_sequences = true;
    }
    let duration = end.saturating_sub(start) as f32 / 1000.0;
    let samples = if let Some(samples) = sample_f32_track(model, &track, start, end)? {
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
    let times = local_times
        .iter()
        .map(|time| *time as f32 / 1000.0)
        .collect();
    Ok(Some(F32Samples { times, values }))
}

fn sample_vec3_track(
    model: &Model,
    track: &TrackVector3f,
    sequence_start: u32,
    sequence_end: u32,
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
    let times = local_times
        .iter()
        .map(|time| *time as f32 / 1000.0)
        .collect();
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
    let times = local_times
        .iter()
        .map(|time| *time as f32 / 1000.0)
        .collect();
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

#[derive(Debug)]
struct TrackWindow {
    indices: Vec<usize>,
    track_start: u32,
    track_end: u32,
    clip_duration: u32,
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
            self.sequence_start.saturating_add(local_ms)
        }
    }
}

fn track_window(
    timestamps: &[u32],
    global_sequence_id: u32,
    global_sequences: &[u32],
    sequence_start: u32,
    sequence_end: u32,
) -> Option<TrackWindow> {
    let clip_duration = sequence_end.checked_sub(sequence_start)?;
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
        for &index in &window.indices {
            times.insert(timestamps[index].saturating_sub(window.sequence_start));
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
) -> (Vec<Value>, Vec<String>, bool) {
    let mut result = Vec::with_capacity(model.materials_len());
    let mut warnings = Vec::new();
    let mut uses_unlit = false;

    for (material_index, material) in model.materials_iter().enumerate() {
        if material.layers_len() > 1 {
            warnings.push(format!(
                "material {material_index} has {} WC3 layers; glTF uses one representative layer",
                material.layers_len()
            ));
        }
        let selected = material.layers_iter().find(|layer| {
            texture_indices
                .get(layer_diffuse_texture_id(layer) as usize)
                .and_then(|index| *index)
                .is_some()
        });
        let fallback = material.layers(0);
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
            if let Some(Some(gltf_texture)) = texture_indices.get(texture_id) {
                pbr.as_object_mut()
                    .expect("pbr object")
                    .insert("baseColorTexture".into(), json!({ "index": gltf_texture }));
            } else if model
                .textures(texture_id)
                .is_some_and(|texture| texture.replaceable_id() != 0)
            {
                pbr.as_object_mut().expect("pbr object").insert(
                    "baseColorFactor".into(),
                    json!([0.85, 0.12, 0.12, layer.alpha()]),
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
            extras.insert("wc3LayerCount".into(), json!(material.layers_len()));
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

    (result, warnings, uses_unlit)
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

fn casc_asset_paths(logical_path: &str) -> [String; 3] {
    // Warcraft III 3.x keeps some presentation payloads referenced by base-namespace models in
    // layered CASC mods. Probe the base namespace first so classic assets retain their historical
    // source, then the DE/HD presentation layers used by current models and texture flipbooks.
    [
        format!("war3.w3mod:{logical_path}"),
        format!("war3.w3mod:_de.w3mod:{logical_path}"),
        format!("war3.w3mod:_hd.w3mod:{logical_path}"),
    ]
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

    fn mdx_with_chunks(version: u32, chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut bytes = b"MDLX".to_vec();
        bytes.extend_from_slice(b"VERS");
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&version.to_le_bytes());
        for (tag, payload) in chunks {
            bytes.extend_from_slice(*tag);
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(payload);
        }
        bytes
    }

    #[test]
    fn wc3_3_camera_variant_size_is_normalized_before_whiteout_parsing() {
        let mut camera = vec![0u8; 120];
        camera[..4].copy_from_slice(&(0x0300_0000u32 | 120).to_le_bytes());
        let bytes = mdx_with_chunks(1800, &[(b"CAMS", camera)]);

        let plan = mdx_compatibility_plan(&bytes, "camera.mdx").expect("compatibility plan");

        assert_eq!(plan.patches, vec![(24, 120u32.to_le_bytes())]);
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("camera variant size"))
        );
    }

    #[test]
    fn pre_wc3_3_mdx_needs_no_compatibility_patches() {
        let bytes = mdx_with_chunks(1200, &[]);
        let plan = mdx_compatibility_plan(&bytes, "classic.mdx").expect("compatibility plan");
        assert!(plan.patches.is_empty());
        assert!(plan.warnings.is_empty());
    }

    #[test]
    fn wc3_3_wide_skin_is_losslessly_narrowed_for_stable_whiteout() {
        let mut geoset = vec![0u8; 4];
        geoset.extend_from_slice(b"ABCD");
        geoset.extend_from_slice(b"SKIN");
        geoset.extend_from_slice(&4u32.to_le_bytes());
        for value in [1u16, 2, 232, 255] {
            geoset.extend_from_slice(&value.to_le_bytes());
        }
        geoset.extend_from_slice(b"TAIL");
        let geoset_size = geoset.len() as u32;
        geoset[..4].copy_from_slice(&geoset_size.to_le_bytes());
        let bytes = mdx_with_chunks(1800, &[(b"GEOS", geoset)]);

        let (rewritten, streams) =
            rewrite_wide_skin_streams(&bytes, "wide-skin.mdx").expect("rewrite");
        let rewritten = rewritten.expect("wide stream should be rewritten");

        assert_eq!(streams, 1);
        let skin = rewritten
            .windows(4)
            .position(|window| window == b"SKIN")
            .expect("SKIN tag");
        assert_eq!(read_u32_le(&rewritten, skin + 4).expect("count"), 4);
        assert_eq!(&rewritten[skin + 8..skin + 12], &[1, 2, 232, 255]);
        assert_eq!(read_u32_le(&rewritten, 20).expect("GEOS size"), 24);
        assert_eq!(read_u32_le(&rewritten, 24).expect("geoset size"), 24);
    }

    #[test]
    fn wc3_3_wide_skin_rejects_values_that_do_not_fit_stable_whiteout() {
        let mut geoset = vec![0u8; 4];
        geoset.extend_from_slice(b"SKIN");
        geoset.extend_from_slice(&1u32.to_le_bytes());
        geoset.extend_from_slice(&256u16.to_le_bytes());
        let geoset_size = geoset.len() as u32;
        geoset[..4].copy_from_slice(&geoset_size.to_le_bytes());
        let bytes = mdx_with_chunks(1800, &[(b"GEOS", geoset)]);

        let error = rewrite_wide_skin_streams(&bytes, "wide-skin.mdx")
            .expect_err("out-of-range skin value should fail")
            .to_string();
        assert!(error.contains("exceeds Whiteout 0.1.7's u8 representation"));
    }

    #[test]
    fn wc3_3_lights_are_skipped_instead_of_parsed_with_old_layout() {
        let bytes = mdx_with_chunks(1800, &[(b"LITE", vec![0u8; 16])]);
        let plan = mdx_compatibility_plan(&bytes, "light.mdx").expect("compatibility plan");
        assert_eq!(plan.patches, vec![(16, *b"XLIT")]);
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("light"))
        );
    }

    #[test]
    fn casc_asset_paths_probe_base_then_presentation_layers() {
        assert_eq!(
            casc_asset_paths(r"Textures\Water\Foam.dds"),
            [
                r"war3.w3mod:Textures\Water\Foam.dds".to_owned(),
                r"war3.w3mod:_de.w3mod:Textures\Water\Foam.dds".to_owned(),
                r"war3.w3mod:_hd.w3mod:Textures\Water\Foam.dds".to_owned(),
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
    fn model_feature_manifest_inventories_lossy_wc3_primitives() {
        let mut model = Model::new();
        model.set_global_sequences(&[1000]);

        model.resize_materials(1);
        {
            let mut material = model.materials_mut(0).expect("material");
            material.resize_layers(2);
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
        assert_eq!(features.non_inheritance_node_count, 1);
        assert_eq!(features.max_classic_skin_influences, 5);
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
    fn reforged_subtextures_resolve_diffuse_and_team_color_slots() {
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
        model.resize_ribbon_emitters(1);
        {
            let mut emitter = model.ribbon_emitters_mut(0).expect("ribbon emitter");
            emitter.set_material_id(0);
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
    fn material_priority_plane_is_preserved_for_native_depth_ordering() {
        let mut model = Model::new();
        model.resize_materials(1);
        {
            let mut material = model.materials_mut(0).expect("material");
            material.set_priority_plane(3);
            material.resize_layers(1);
            material.layers_mut(0).expect("layer").set_texture_id(0);
        }
        let (materials, _, _) = build_materials(&model, &[None], &[]);
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
    }

    #[test]
    fn model_dependencies_include_attachment_and_legacy_particle_children() {
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
                    emission_rate_track: None,
                    gravity_track: None,
                    longitude_track: None,
                    latitude_track: None,
                    lifespan_track: None,
                    speed_track: None,
                    visibility_track: None,
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
                    emission_rate_track: None,
                    gravity_track: None,
                    longitude_track: None,
                    latitude_track: None,
                    lifespan_track: None,
                    speed_track: None,
                    visibility_track: None,
                },
            ],
            ribbon_emitters: Vec::new(),
            attachments: vec![
                AttachmentManifest {
                    object_id: 3,
                    name: "birth".to_owned(),
                    position: [0.0; 3],
                    path: r"SharedModels\NEBirth.MDL".to_owned(),
                    visibility_track: None,
                },
                AttachmentManifest {
                    object_id: 4,
                    name: "duplicate".to_owned(),
                    position: [0.0; 3],
                    path: r"SharedModels\NEBirth.mdx".to_owned(),
                    visibility_track: None,
                },
            ],
            event_objects: Vec::new(),
            warnings: Vec::new(),
        };

        assert_eq!(
            model_dependency_paths(&model),
            [
                r"SharedModels\NEBirth.mdx".to_owned(),
                r"SharedModels\Smoke1_Green.mdx".to_owned(),
            ]
        );
    }

    #[test]
    fn attachment_manifest_preserves_child_path_pivot_and_visibility() {
        let mut model = Model::new();
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
        let visibility = attachment
            .visibility_track
            .as_ref()
            .expect("visibility track should be preserved");
        assert_eq!(visibility.timestamps, [0, 500]);
        assert_eq!(visibility.values, [0.0, 1.0]);
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

        let manifests = event_object_manifests(&model);
        assert_eq!(manifests.len(), 1);
        let event = &manifests[0];
        assert_eq!(event.object_id, 0);
        assert_eq!(event.name, "SNDxFootstep");
        assert_eq!(event.position, [4.0, 6.0, -5.0]);
        assert_eq!(event.global_sequence_id, Some(3));
        assert_eq!(event.event_track_times, [120, 480, 900]);
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
