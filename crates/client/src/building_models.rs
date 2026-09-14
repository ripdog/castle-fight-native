use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

use bevy::{gltf::Gltf, prelude::*};
use serde::Deserialize;

use crate::{terrain::client_asset_root, wc3_effects::Wc3ParticleEmitter};

const BUILDING_MODEL_MANIFEST: &str = "wc3/buildings/manifest.json";
const BUILDING_MODEL_ASSET_PREFIX: &str = "wc3/buildings";
const BUILDING_MODEL_MANIFEST_SCHEMA_VERSION: u32 = 5;

#[derive(Resource, Default)]
pub struct BuildingModelSet {
    models: BTreeMap<u32, BuildingModelAsset>,
}

#[derive(Clone)]
pub struct BuildingModelAsset {
    pub scene: Handle<WorldAsset>,
    gltf: Handle<Gltf>,
    pub scale: f32,
    pub lifecycle_animations: BuildingLifecycleAnimationNames,
    pub emitters: Vec<Wc3ParticleEmitter>,
    animations_prepared: bool,
    animations: Option<BuildingAnimationSet>,
}

#[derive(Debug, Clone)]
pub struct BuildingAnimationClip {
    pub node: AnimationNodeIndex,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct BuildingAnimationSet {
    pub graph: Handle<AnimationGraph>,
    pub birth: Option<BuildingAnimationClip>,
    pub stand: Option<BuildingAnimationClip>,
    pub death: Option<BuildingAnimationClip>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct BuildingLifecycleAnimationNames {
    pub birth: Option<String>,
    pub stand: Option<String>,
    pub death: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BuildingAssetManifest {
    schema_version: u32,
    buildings: Vec<BuildingAssetManifestEntry>,
    models: Vec<BuildingModelManifestEntry>,
}

#[derive(Debug, Deserialize)]
struct BuildingAssetManifestEntry {
    rawcode: String,
    scale: f32,
    #[serde(default)]
    lifecycle_animations: BuildingLifecycleAnimationNames,
    #[serde(default)]
    fallback_to_base_art: bool,
    gltf: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BuildingModelManifestEntry {
    gltf: String,
    #[serde(default)]
    particle_emitters: Vec<Wc3ParticleEmitter>,
}

#[derive(Debug, Clone)]
struct ResolvedBuildingAsset {
    rawcode: u32,
    scale: f32,
    lifecycle_animations: BuildingLifecycleAnimationNames,
    emitters: Vec<Wc3ParticleEmitter>,
    asset_path: String,
}

impl BuildingModelSet {
    #[must_use]
    pub fn load_default(asset_server: &AssetServer) -> Self {
        let asset_root = client_asset_root();
        let manifest_path = asset_root.join(BUILDING_MODEL_MANIFEST);
        if !manifest_path.is_file() {
            return Self::default();
        }

        match load_manifest_entries(&manifest_path, BUILDING_MODEL_ASSET_PREFIX) {
            Ok(entries) => {
                let mut models = BTreeMap::new();
                for entry in entries {
                    if !asset_root.join(&entry.asset_path).is_file() {
                        eprintln!(
                            "warning: generated WC3 building model is missing: {}",
                            entry.asset_path
                        );
                        continue;
                    }
                    let scene = asset_server
                        .load(GltfAssetLabel::Scene(0).from_asset(entry.asset_path.clone()));
                    let gltf = asset_server.load(entry.asset_path.clone());
                    models.insert(
                        entry.rawcode,
                        BuildingModelAsset {
                            scene,
                            gltf,
                            scale: entry.scale,
                            lifecycle_animations: entry.lifecycle_animations,
                            emitters: entry.emitters,
                            animations_prepared: false,
                            animations: None,
                        },
                    );
                }
                if !models.is_empty() {
                    println!("Loaded {} generated WC3 building model(s)", models.len());
                }
                Self { models }
            }
            Err(error) => {
                eprintln!("warning: ignoring generated WC3 building models: {error}");
                Self::default()
            }
        }
    }

    pub fn prepare_animations(
        &mut self,
        gltfs: &Assets<Gltf>,
        animation_clips: &Assets<AnimationClip>,
        graphs: &mut Assets<AnimationGraph>,
    ) {
        for (rawcode, model) in self
            .models
            .iter_mut()
            .filter(|(_, model)| !model.animations_prepared)
        {
            let Some(gltf) = gltfs.get(&model.gltf) else {
                continue;
            };
            let birth = named_animation(gltf, model.lifecycle_animations.birth.as_deref());
            let stand = named_animation(gltf, model.lifecycle_animations.stand.as_deref());
            let death = named_animation(gltf, model.lifecycle_animations.death.as_deref());
            for (role, requested, resolved) in [
                (
                    "Birth",
                    model.lifecycle_animations.birth.as_deref(),
                    birth.as_ref(),
                ),
                (
                    "Stand",
                    model.lifecycle_animations.stand.as_deref(),
                    stand.as_ref(),
                ),
                (
                    "Death",
                    model.lifecycle_animations.death.as_deref(),
                    death.as_ref(),
                ),
            ] {
                if let Some(requested) = requested
                    && resolved.is_none()
                {
                    eprintln!(
                        "warning: WC3 building {} manifest selected {role} animation {requested:?}, but the generated glTF does not expose it",
                        String::from_utf8_lossy(&rawcode.to_be_bytes())
                    );
                }
            }
            let selected = [birth.as_ref(), stand.as_ref(), death.as_ref()];
            if selected
                .into_iter()
                .flatten()
                .any(|clip| animation_clips.get(clip).is_none())
            {
                continue;
            }

            let mut clips = Vec::new();
            let birth_slot = append_optional_clip(&mut clips, birth);
            let stand_slot = append_optional_clip(&mut clips, stand);
            let death_slot = append_optional_clip(&mut clips, death);
            if clips.is_empty() {
                model.animations_prepared = true;
                continue;
            }
            let (graph, nodes) = AnimationGraph::from_clips(clips);
            let graph = graphs.add(graph);
            model.animations = Some(BuildingAnimationSet {
                graph,
                birth: birth_slot.map(|slot| BuildingAnimationClip {
                    node: nodes[slot],
                    name: model
                        .lifecycle_animations
                        .birth
                        .clone()
                        .expect("birth slot requires a manifest sequence name"),
                }),
                stand: stand_slot.map(|slot| BuildingAnimationClip {
                    node: nodes[slot],
                    name: model
                        .lifecycle_animations
                        .stand
                        .clone()
                        .expect("stand slot requires a manifest sequence name"),
                }),
                death: death_slot.map(|slot| BuildingAnimationClip {
                    node: nodes[slot],
                    name: model
                        .lifecycle_animations
                        .death
                        .clone()
                        .expect("death slot requires a manifest sequence name"),
                }),
            });
            model.animations_prepared = true;
        }
    }

    #[must_use]
    pub fn get(&self, rawcode: u32) -> Option<&BuildingModelAsset> {
        self.models.get(&rawcode)
    }

    #[must_use]
    pub fn animations(&self, rawcode: u32) -> Option<&BuildingAnimationSet> {
        self.models.get(&rawcode)?.animations.as_ref()
    }
}

fn named_animation(gltf: &Gltf, name: Option<&str>) -> Option<Handle<AnimationClip>> {
    gltf.named_animations.get(name?).cloned()
}

fn append_optional_clip(
    clips: &mut Vec<Handle<AnimationClip>>,
    clip: Option<Handle<AnimationClip>>,
) -> Option<usize> {
    clip.map(|clip| {
        let slot = clips.len();
        clips.push(clip);
        slot
    })
}

fn load_manifest_entries(
    path: &Path,
    asset_prefix: &str,
) -> Result<Vec<ResolvedBuildingAsset>, String> {
    let json = fs::read_to_string(path)
        .map_err(|error| format!("failed reading {}: {error}", path.display()))?;
    resolve_manifest_entries(&json, asset_prefix)
}

fn resolve_manifest_entries(
    json: &str,
    asset_prefix: &str,
) -> Result<Vec<ResolvedBuildingAsset>, String> {
    let manifest: BuildingAssetManifest = serde_json::from_str(json)
        .map_err(|error| format!("invalid building asset manifest: {error}"))?;
    if manifest.schema_version != BUILDING_MODEL_MANIFEST_SCHEMA_VERSION {
        return Err(format!(
            "unsupported building asset manifest schema {}",
            manifest.schema_version
        ));
    }

    let mut model_emitters = BTreeMap::new();
    for model in manifest.models {
        let gltf = model.gltf.replace('\\', "/");
        validate_relative_asset_path(&gltf)?;
        if model_emitters
            .insert(gltf.clone(), model.particle_emitters)
            .is_some()
        {
            return Err(format!("duplicate building model manifest path {gltf}"));
        }
    }

    let mut resolved = BTreeMap::new();
    for entry in manifest.buildings {
        if entry.fallback_to_base_art {
            continue;
        }
        let Some(gltf) = entry.gltf else {
            continue;
        };
        let rawcode = parse_rawcode(&entry.rawcode)?;
        if !entry.scale.is_finite() || entry.scale <= 0.0 {
            return Err(format!(
                "building {} has invalid model scale {}",
                entry.rawcode, entry.scale
            ));
        }
        let gltf = gltf.replace('\\', "/");
        validate_relative_asset_path(&gltf)?;
        let emitters = model_emitters.get(&gltf).cloned().ok_or_else(|| {
            format!(
                "building {} references missing model manifest {gltf}",
                entry.rawcode
            )
        })?;
        let asset_path = format!("{}/{}", asset_prefix.trim_end_matches('/'), gltf);
        if resolved
            .insert(
                rawcode,
                ResolvedBuildingAsset {
                    rawcode,
                    scale: entry.scale,
                    lifecycle_animations: entry.lifecycle_animations,
                    emitters,
                    asset_path,
                },
            )
            .is_some()
        {
            return Err(format!("duplicate building rawcode {}", entry.rawcode));
        }
    }
    Ok(resolved.into_values().collect())
}

fn parse_rawcode(rawcode: &str) -> Result<u32, String> {
    let bytes: [u8; 4] = rawcode
        .as_bytes()
        .try_into()
        .map_err(|_| format!("building rawcode {rawcode:?} is not exactly four bytes"))?;
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
            "building model path {} is not a safe relative asset path",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_building_manifest_lifecycle_and_model_metadata() {
        let json = r#"{
            "schema_version": 5,
            "buildings": [
                {"rawcode": "h000", "scale": 0.5, "animation_properties": [], "lifecycle_animations": {"birth":"Birth","stand":"Stand","death":"Death"}, "fallback_to_base_art": false, "gltf": "models/humanbarracks.gltf"},
                {"rawcode": "h006", "scale": 0.8, "animation_properties": ["upgrade", "first"], "lifecycle_animations": {"birth":"Birth Upgrade First","stand":"Stand Upgrade First","death":"Death"}, "fallback_to_base_art": false, "gltf": "models/tower.gltf"},
                {"rawcode": "h07P", "scale": 0.8, "animation_properties": ["upgrade", "second"], "fallback_to_base_art": true, "gltf": "models/tower.gltf"},
                {"rawcode": "xxxx", "scale": 1.0, "animation_properties": [], "fallback_to_base_art": false, "gltf": null}
            ],
            "models": [
                {"gltf":"models/humanbarracks.gltf","particle_emitters":[]},
                {"gltf":"models/tower.gltf","particle_emitters":[]}
            ]
        }"#;
        let entries = resolve_manifest_entries(json, "wc3/buildings").expect("manifest resolves");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].rawcode, u32::from_be_bytes(*b"h000"));
        assert_eq!(entries[0].scale, 0.5);
        assert_eq!(
            entries[0].lifecycle_animations.birth.as_deref(),
            Some("Birth")
        );
        assert_eq!(
            entries[0].lifecycle_animations.stand.as_deref(),
            Some("Stand")
        );
        assert_eq!(
            entries[0].lifecycle_animations.death.as_deref(),
            Some("Death")
        );
        assert_eq!(
            entries[0].asset_path,
            "wc3/buildings/models/humanbarracks.gltf"
        );
        assert_eq!(entries[1].rawcode, u32::from_be_bytes(*b"h006"));
        assert_eq!(
            entries[1].lifecycle_animations.birth.as_deref(),
            Some("Birth Upgrade First")
        );
    }

    #[test]
    fn rejects_invalid_building_manifest_entries() {
        let stale_schema = r#"{
            "schema_version": 4,
            "buildings": [],
            "models": []
        }"#;
        assert!(
            resolve_manifest_entries(stale_schema, "wc3/buildings")
                .expect_err("stale schema must be rejected")
                .contains("unsupported building asset manifest schema")
        );

        let bad_scale = r#"{
            "schema_version": 5,
            "buildings": [{"rawcode": "h000", "scale": 0.0, "gltf": "models/foo.gltf"}],
            "models": [{"gltf":"models/foo.gltf"}]
        }"#;
        assert!(
            resolve_manifest_entries(bad_scale, "wc3/buildings")
                .expect_err("scale must be rejected")
                .contains("invalid model scale")
        );

        let bad_path = r#"{
            "schema_version": 5,
            "buildings": [{"rawcode": "h000", "scale": 1.0, "gltf": "../escape.gltf"}],
            "models": [{"gltf":"../escape.gltf"}]
        }"#;
        assert!(
            resolve_manifest_entries(bad_path, "wc3/buildings")
                .expect_err("path must be rejected")
                .contains("safe relative asset path")
        );
    }
}
