use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs, io,
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Value, json};
use whiteout::{
    casc::Storage,
    mdx::{
        InterpolationType, LayerFilterMode, LayerShadingFlag, MDLXFormat, Model, Node, NodeFlag,
        Parser as MdxParser, SequenceFlag, TrackQuaternion, TrackVector3f,
    },
    textures::{BlpParser, DdsParser, PixelFormat, PngParser, PngWriter, Texture, TgaParser},
};

use crate::catalog::{CATALOG_VERSION, DoodadAssetSpec, UnitAssetSpec};

const GL_ARRAY_BUFFER: u32 = 34_962;
const GL_ELEMENT_ARRAY_BUFFER: u32 = 34_963;
const GL_FLOAT: u32 = 5_126;
const GL_UNSIGNED_SHORT: u32 = 5_123;
const NO_PARENT: u32 = u32::MAX;
const NO_GLOBAL_SEQUENCE: u32 = u32::MAX;

type TextureExport = (Vec<TextureManifest>, Vec<Option<usize>>);
type GltfBuildOutput = (Value, Vec<u8>, Vec<String>);

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
    pub gltf: Option<String>,
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
    pub animations: Vec<AnimationManifest>,
    pub textures: Vec<TextureManifest>,
    pub warnings: Vec<String>,
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
    storage: Storage,
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
    scale: f32,
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
        output: &Path,
        keep_source: bool,
    ) -> Result<Self, Box<dyn Error>> {
        let storage = Storage::open(&wc3_install.to_string_lossy(), None).ok_or_else(|| {
            io::Error::other(format!(
                "failed to open Warcraft III CASC storage at {}",
                wc3_install.display()
            ))
        })?;
        let wc3_version = read_wc3_version(wc3_install);
        let unit_skin_bytes = storage
            .read_file(r"war3.w3mod:units\unitskin.txt")
            .ok_or_else(|| io::Error::other("failed to read war3.w3mod:units\\unitskin.txt"))?;
        let unit_skin = parse_unit_skin(&String::from_utf8_lossy(&unit_skin_bytes));
        let doodad_skin_bytes = storage
            .read_file(r"war3.w3mod:doodads\doodadskins.txt")
            .ok_or_else(|| {
                io::Error::other("failed to read war3.w3mod:doodads\\doodadskins.txt")
            })?;
        let doodad_skin = parse_doodad_skin(&String::from_utf8_lossy(&doodad_skin_bytes));
        let destructable_skin_bytes = storage
            .read_file(r"war3.w3mod:units\destructableskin.txt")
            .ok_or_else(|| {
                io::Error::other("failed to read war3.w3mod:units\\destructableskin.txt")
            })?;
        let destructable_skin =
            parse_doodad_skin(&String::from_utf8_lossy(&destructable_skin_bytes));

        fs::create_dir_all(output.join("models"))?;
        fs::create_dir_all(output.join("textures"))?;
        if keep_source {
            fs::create_dir_all(output.join("source/models"))?;
            fs::create_dir_all(output.join("source/textures"))?;
        }
        Ok(Self {
            storage,
            output: output.to_path_buf(),
            keep_source,
            wc3_version,
            unit_skin,
            doodad_skin,
            destructable_skin,
            texture_cache: BTreeMap::new(),
        })
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
            grouped
                .entry(unit.source_model.to_ascii_lowercase())
                .or_default()
                .push(unit);
        }

        let mut models = Vec::new();
        let mut failures = Vec::new();
        let mut model_outputs = BTreeMap::<String, String>::new();
        for (key, group) in &grouped {
            let source = group[0].source_model.clone();
            match self.export_model(&source) {
                Ok(model) => {
                    model_outputs.insert(key.clone(), model.gltf.clone());
                    models.push(model);
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
                requested_model: unit.requested_model.clone(),
                gltf: model_outputs
                    .get(&unit.source_model.to_ascii_lowercase())
                    .cloned(),
                source_model: unit.source_model.clone(),
                fallback_to_base_art: unit.fallback_to_base_art,
            })
            .collect();

        Ok(AssetManifest {
            schema_version: 1,
            castle_fight_catalog_version: CATALOG_VERSION,
            wc3_version: self.wc3_version.clone(),
            art_mode: "sd",
            units,
            models,
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
            match self.export_model_with_replacements(source_model, &resolved.replaceable_textures)
            {
                Ok(model) => {
                    model_outputs.insert(key, model.gltf.clone());
                    models.push(model);
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
            schema_version: 1,
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
        let scale = unit
            .scale
            .or_else(|| profile.and_then(|profile| profile.model_scale_sd))
            .or_else(|| profile.and_then(|profile| profile.model_scale))
            .unwrap_or(1.0);
        Ok(ResolvedUnit {
            rawcode: unit.rawcode.clone(),
            name: unit.name.clone(),
            requested_model,
            source_model,
            fallback_to_base_art,
            scale,
        })
    }

    fn model_exists(&self, logical_path: &str) -> bool {
        self.storage
            .file_exists(&format!("war3.w3mod:{logical_path}"))
    }

    fn export_model(&mut self, logical_path: &str) -> Result<ModelManifest, Box<dyn Error>> {
        self.export_model_with_replacements(logical_path, &BTreeMap::new())
    }

    fn export_model_with_replacements(
        &mut self,
        logical_path: &str,
        replaceable_textures: &BTreeMap<u32, String>,
    ) -> Result<ModelManifest, Box<dyn Error>> {
        let (source_casc_path, model_bytes) = self.read_model(logical_path)?;
        let mut parser = MdxParser::new();
        let model = parser
            .parse(&model_bytes, MDLXFormat::MDX)
            .ok_or_else(|| io::Error::other(format!("failed to parse MDX {source_casc_path}")))?;
        let mut warnings = parser.issues();

        let asset_name = doodad_asset_name(logical_path, replaceable_textures);
        if self.keep_source {
            fs::write(
                self.output
                    .join("source/models")
                    .join(format!("{asset_name}.mdx")),
                &model_bytes,
            )?;
        }

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
        )?;
        warnings.extend(material_warnings);

        fs::write(self.output.join(&bin_name), &bin)?;
        fs::write(
            self.output.join(&gltf_name),
            serde_json::to_vec_pretty(&gltf)?,
        )?;

        let animations = animation_manifests_from_gltf(&gltf)?;

        Ok(ModelManifest {
            source_model: logical_path.to_owned(),
            source_casc_path,
            gltf: gltf_name,
            bin: bin_name,
            geosets: model.geosets_len(),
            bones: model.bones_len(),
            animations,
            textures: texture_manifests,
            warnings,
        })
    }

    fn read_model(&self, logical_path: &str) -> Result<(String, Vec<u8>), Box<dyn Error>> {
        let casc_path = format!("war3.w3mod:{logical_path}");
        let bytes = self.storage.read_file(&casc_path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("WC3 model not found in CASC: {casc_path}"),
            )
        })?;
        Ok((casc_path, bytes.to_vec()))
    }

    fn export_model_textures(
        &mut self,
        model: &Model,
        replaceable_textures: &BTreeMap<u32, String>,
    ) -> Result<TextureExport, Box<dyn Error>> {
        let mut manifests = Vec::with_capacity(model.textures_len());
        let mut gltf_indices = Vec::with_capacity(model.textures_len());
        let mut next_gltf_index = 0usize;

        for texture in model.textures_iter() {
            let replaceable_id = texture.replaceable_id();
            let model_logical = normalize_texture_path(&texture.file_name());
            let logical = if replaceable_id == 0 {
                (!model_logical.is_empty()).then_some(model_logical.clone())
            } else {
                replaceable_textures.get(&replaceable_id).cloned()
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

    fn export_texture(&self, logical_path: &str) -> Result<TextureManifest, Box<dyn Error>> {
        let (source_casc_path, source_bytes, source_ext) = self.read_texture(logical_path)?;
        let png_name = format!("textures/{}.png", flat_asset_name(logical_path));
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
        let has_transparency = texture_has_transparency(&decoded);
        let png_bytes = if source_ext == "png" {
            source_bytes.to_vec()
        } else {
            texture_to_png(&decoded)?
        };
        fs::write(self.output.join(&png_name), png_bytes)?;

        if self.keep_source {
            let source_name = format!("{}.{}", flat_asset_name(logical_path), source_ext);
            fs::write(
                self.output.join("source/textures").join(source_name),
                &source_bytes,
            )?;
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
        &self,
        logical_path: &str,
    ) -> Result<(String, Vec<u8>, String), Box<dyn Error>> {
        let path = Path::new(logical_path);
        let requested_ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let stem = path.with_extension("").to_string_lossy().replace('/', "\\");

        let mut extensions = Vec::new();
        if !requested_ext.is_empty() {
            extensions.push(requested_ext);
        }
        for fallback in ["dds", "blp", "tga", "png"] {
            if !extensions.iter().any(|ext| ext == fallback) {
                extensions.push(fallback.to_owned());
            }
        }

        for ext in extensions {
            let candidate = format!("{stem}.{ext}");
            let casc_path = format!("war3.w3mod:{candidate}");
            if let Some(bytes) = self.storage.read_file(&casc_path) {
                return Ok((casc_path, bytes.to_vec(), ext));
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("WC3 texture not found in CASC for logical path {logical_path}"),
        )
        .into())
    }
}

fn texture_has_transparency(texture: &Texture) -> bool {
    let Some(rgba) = texture.copy_as_format(PixelFormat::RGBA8, None) else {
        return false;
    };
    rgba.mip_data(0, 0)
        .as_ref()
        .as_chunks::<4>()
        .0
        .iter()
        .any(|pixel| pixel[3] < u8::MAX)
}

fn texture_to_png(texture: &Texture) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut writer = PngWriter::new();
    let bytes = writer.write(texture);
    if bytes.is_empty() {
        return Err(io::Error::other(format!(
            "PNG encoder produced no data: {:?}",
            writer.issues()
        ))
        .into());
    }
    Ok(bytes.to_vec())
}

fn build_gltf(
    model: &Model,
    logical_path: &str,
    asset_name: &str,
    texture_indices: &[Option<usize>],
    texture_manifests: &[TextureManifest],
) -> Result<GltfBuildOutput, Box<dyn Error>> {
    let mut binary = BinaryBuilder::default();
    let mut warnings = Vec::new();
    let skeleton = build_skeleton(model, &mut binary, &mut warnings)?;

    let mut primitives = Vec::new();
    for (geoset_index, geoset) in model.geosets_iter().enumerate() {
        if geoset.vertex_positions().is_empty() || geoset.faces().is_empty() {
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
        if let Some(skin) = build_geoset_skin(&geoset, &skeleton)? {
            let joints_accessor = binary.push_vec4_u16(&skin.joints_0, Some(GL_ARRAY_BUFFER));
            let weights_accessor = binary.push_vec4_f32(&skin.weights_0, Some(GL_ARRAY_BUFFER));
            attributes.insert("JOINTS_0".into(), json!(joints_accessor));
            attributes.insert("WEIGHTS_0".into(), json!(weights_accessor));
            if let (Some(joints), Some(weights)) = (skin.joints_1.as_ref(), skin.weights_1.as_ref())
            {
                let joints_accessor = binary.push_vec4_u16(joints, Some(GL_ARRAY_BUFFER));
                let weights_accessor = binary.push_vec4_f32(weights, Some(GL_ARRAY_BUFFER));
                attributes.insert("JOINTS_1".into(), json!(joints_accessor));
                attributes.insert("WEIGHTS_1".into(), json!(weights_accessor));
            }
        }

        primitives.push(json!({
            "attributes": attributes,
            "indices": index_accessor,
            "material": geoset.material_id(),
            "mode": 4,
            "extras": {
                "wc3Geoset": geoset_index,
                "wc3Lod": geoset.lod(),
            }
        }));
    }

    let animations = build_animations(model, &skeleton, &mut binary, &mut warnings)?;
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

    let mut mesh_node = json!({ "name": asset_name, "mesh": 0 });
    if skeleton.skin.is_some() {
        mesh_node
            .as_object_mut()
            .expect("mesh node object")
            .insert("skin".into(), json!(0));
    }
    let mut nodes = vec![mesh_node];
    nodes.extend(skeleton.nodes.iter().cloned());
    let mut scene_nodes = vec![0usize];
    scene_nodes.extend(skeleton.scene_roots.iter().copied());

    let mut root = json!({
        "asset": {
            "version": "2.0",
            "generator": "Castle Fight Native wc3 asset extractor"
        },
        "scene": 0,
        "scenes": [{ "nodes": scene_nodes }],
        "nodes": nodes,
        "meshes": [{ "name": asset_name, "primitives": primitives }],
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
    if model.bones_len() == 0 {
        return Ok(SkeletonBuild::default());
    }

    let mut infos = BTreeMap::<u32, SkeletonNodeInfo>::new();
    for bone in model.bones_iter() {
        let node = bone.node();
        insert_skeleton_node(model, &node, &mut infos)?;
    }
    for helper in model.helpers_iter() {
        let node = helper.node();
        insert_skeleton_node(model, &node, &mut infos)?;
    }
    for emitter in model.sound_emitters_iter() {
        let node = emitter.node();
        insert_skeleton_node(model, &node, &mut infos)?;
    }
    for attachment in model.attachments_iter() {
        let node = attachment.node();
        insert_skeleton_node(model, &node, &mut infos)?;
    }
    for light in model.lights_iter() {
        let node = light.node();
        insert_skeleton_node(model, &node, &mut infos)?;
    }
    for emitter in model.particle_emitters_iter() {
        let node = emitter.node();
        insert_skeleton_node(model, &node, &mut infos)?;
    }
    for emitter in model.particle_emitters_2_iter() {
        let node = emitter.node();
        insert_skeleton_node(model, &node, &mut infos)?;
    }
    for emitter in model.ribbon_emitters_iter() {
        let node = emitter.node();
        insert_skeleton_node(model, &node, &mut infos)?;
    }
    for event in model.event_objects_iter() {
        let node = event.node();
        insert_skeleton_node(model, &node, &mut infos)?;
    }
    for shape in model.collision_shapes_iter() {
        let node = shape.node();
        insert_skeleton_node(model, &node, &mut infos)?;
    }
    for emitter in model.corn_emitters_iter() {
        let node = emitter.node();
        insert_skeleton_node(model, &node, &mut infos)?;
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
    let inverse_bind_accessor = binary.push_mat4_f32(&inverse_bind);
    result.skin = Some(json!({
        "name": "wc3_skin",
        "inverseBindMatrices": inverse_bind_accessor,
        "joints": result.joint_nodes,
    }));
    Ok(result)
}

fn insert_skeleton_node(
    model: &Model,
    node: &Node,
    infos: &mut BTreeMap<u32, SkeletonNodeInfo>,
) -> Result<(), Box<dyn Error>> {
    let object_id = node.object_id();
    let pivot = model
        .pivot_points()
        .get(object_id as usize)
        .ok_or_else(|| {
            io::Error::other(format!(
                "node {} ({}) has no matching pivot point",
                object_id,
                node.name()
            ))
        })?;
    let info = SkeletonNodeInfo {
        object_id,
        parent_id: node.parent_id(),
        name: node.name(),
        pivot: wc3_vec3(pivot.x, pivot.y, pivot.z),
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
    joints_1: Option<Vec<[u16; 4]>>,
    weights_1: Option<Vec<[f32; 4]>>,
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
            joints_1: None,
            weights_1: None,
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

    let uses_second_set = geoset.matrix_groups().iter().any(|&count| count > 4);
    let mut joints_0 = Vec::with_capacity(vertex_count);
    let mut weights_0 = Vec::with_capacity(vertex_count);
    let mut joints_1 = uses_second_set.then(|| Vec::with_capacity(vertex_count));
    let mut weights_1 = uses_second_set.then(|| Vec::with_capacity(vertex_count));
    for &group in geoset.vertex_groups() {
        let group = group as usize;
        let count = *geoset.matrix_groups().get(group).ok_or_else(|| {
            io::Error::other(format!("vertex references missing matrix group {group}"))
        })? as usize;
        if count == 0 || count > 8 {
            return Err(io::Error::other(format!(
                "classic matrix group {group} has {count} influences; exporter supports 1..=8 via JOINTS_0/1"
            ))
            .into());
        }
        let first = group_offsets[group];
        let matrix_ids = &geoset.matrix_indices()[first..first + count];
        let mut vertex_joints_0 = [0u16; 4];
        let mut vertex_weights_0 = [0.0f32; 4];
        let mut vertex_joints_1 = [0u16; 4];
        let mut vertex_weights_1 = [0.0f32; 4];
        let weight = 1.0 / count as f32;
        for (slot, &object_id) in matrix_ids.iter().enumerate() {
            let joint = skeleton.joint_by_object.get(&object_id).ok_or_else(|| {
                io::Error::other(format!(
                    "classic geoset matrix group references non-bone object id {object_id}"
                ))
            })?;
            if slot < 4 {
                vertex_joints_0[slot] = *joint;
                vertex_weights_0[slot] = weight;
            } else {
                vertex_joints_1[slot - 4] = *joint;
                vertex_weights_1[slot - 4] = weight;
            }
        }
        joints_0.push(vertex_joints_0);
        weights_0.push(vertex_weights_0);
        if let Some(joints) = joints_1.as_mut() {
            joints.push(vertex_joints_1);
        }
        if let Some(weights) = weights_1.as_mut() {
            weights.push(vertex_weights_1);
        }
    }
    Ok(Some(GeosetSkin {
        joints_0,
        weights_0,
        joints_1,
        weights_1,
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

fn build_animations(
    model: &Model,
    skeleton: &SkeletonBuild,
    binary: &mut BinaryBuilder,
    warnings: &mut Vec<String>,
) -> Result<Vec<Value>, Box<dyn Error>> {
    if skeleton.joint_nodes.is_empty() {
        return Ok(Vec::new());
    }
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
            "global-sequence bone/helper transforms are baked into every glTF clip starting at global time zero; Warcraft keeps that clock running across sequence changes"
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
                .get(layer.texture_id() as usize)
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

        if let Some(layer) = layer {
            let texture_id = layer.texture_id() as usize;
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

fn normalize_texture_path(path: &str) -> String {
    path.trim()
        .replace('/', "\\")
        .trim_start_matches('\\')
        .to_owned()
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
    fn classic_skin_preserves_more_than_four_equal_influences() {
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
        assert_eq!(skin.joints_1.as_ref().unwrap()[0], [4, 5, 6, 0]);
        let total: f32 = skin.weights_0[0]
            .iter()
            .chain(skin.weights_1.as_ref().unwrap()[0].iter())
            .sum();
        assert!((total - 1.0).abs() < 1.0e-6);
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
