use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

use bevy::{gltf::Gltf, prelude::*};
use serde::Deserialize;

use crate::terrain::client_asset_root;

const BUILDING_MODEL_MANIFEST: &str = "wc3/buildings/manifest.json";
const BUILDING_MODEL_ASSET_PREFIX: &str = "wc3/buildings";
const BUILDING_MODEL_MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Resource, Default)]
pub struct BuildingModelSet {
    models: BTreeMap<u32, BuildingModelAsset>,
}

#[derive(Clone)]
pub struct BuildingModelAsset {
    pub scene: Handle<WorldAsset>,
    gltf: Handle<Gltf>,
    pub scale: f32,
    animation: Option<BuildingAnimationSet>,
}

#[derive(Debug, Clone)]
pub struct BuildingAnimationSet {
    pub graph: Handle<AnimationGraph>,
    pub stand: AnimationNodeIndex,
}

#[derive(Debug, Deserialize)]
struct BuildingAssetManifest {
    schema_version: u32,
    buildings: Vec<BuildingAssetManifestEntry>,
}

#[derive(Debug, Deserialize)]
struct BuildingAssetManifestEntry {
    rawcode: String,
    scale: f32,
    gltf: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct ResolvedBuildingAsset {
    rawcode: u32,
    scale: f32,
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
                            animation: None,
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
        for model in self
            .models
            .values_mut()
            .filter(|model| model.animation.is_none())
        {
            let Some(gltf) = gltfs.get(&model.gltf) else {
                continue;
            };
            let Some(stand) = find_stand_animation(gltf) else {
                continue;
            };
            if animation_clips.get(&stand).is_none() {
                continue;
            }
            let (graph, nodes) = AnimationGraph::from_clips([stand]);
            model.animation = Some(BuildingAnimationSet {
                graph: graphs.add(graph),
                stand: nodes[0],
            });
        }
    }

    #[must_use]
    pub fn get(&self, rawcode: u32) -> Option<&BuildingModelAsset> {
        self.models.get(&rawcode)
    }

    #[must_use]
    pub fn animation(&self, rawcode: u32) -> Option<&BuildingAnimationSet> {
        self.models.get(&rawcode)?.animation.as_ref()
    }
}

fn find_stand_animation(gltf: &Gltf) -> Option<Handle<AnimationClip>> {
    gltf.named_animations
        .iter()
        .filter_map(|(name, clip)| stand_animation_score(name).map(|score| (score, name, clip)))
        .min_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(right.1)))
        .map(|(_, _, clip)| clip.clone())
}

fn stand_animation_score(name: &str) -> Option<u8> {
    let name = name.to_ascii_lowercase();
    match name.as_str() {
        "stand" => Some(0),
        "stand - 1" | "stand 1" => Some(1),
        _ if name.starts_with("stand") && !name.contains("work") && !name.contains("upgrade") => {
            Some(2)
        }
        _ => None,
    }
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

    let mut resolved = BTreeMap::new();
    for entry in manifest.buildings {
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
        let asset_path = format!("{}/{}", asset_prefix.trim_end_matches('/'), gltf);
        if resolved
            .insert(
                rawcode,
                ResolvedBuildingAsset {
                    rawcode,
                    scale: entry.scale,
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
    fn resolves_building_manifest_paths_and_scales() {
        let json = r#"{
            "schema_version": 1,
            "buildings": [
                {"rawcode": "h000", "scale": 4.0, "gltf": "models/blacksmith.gltf"},
                {"rawcode": "h006", "scale": 2.5, "gltf": "models/tower.gltf"},
                {"rawcode": "xxxx", "scale": 1.0, "gltf": null}
            ]
        }"#;
        let entries = resolve_manifest_entries(json, "wc3/buildings").expect("manifest resolves");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].rawcode, u32::from_be_bytes(*b"h000"));
        assert_eq!(entries[0].scale, 4.0);
        assert_eq!(
            entries[0].asset_path,
            "wc3/buildings/models/blacksmith.gltf"
        );
        assert_eq!(entries[1].rawcode, u32::from_be_bytes(*b"h006"));
    }

    #[test]
    fn building_stand_selector_uses_safe_unupgraded_idle_sequences() {
        assert_eq!(stand_animation_score("Stand"), Some(0));
        assert_eq!(stand_animation_score("Stand - 1"), Some(1));
        assert_eq!(stand_animation_score("Stand Ready Attack"), Some(2));
        assert_eq!(stand_animation_score("Stand Work"), None);
        assert_eq!(stand_animation_score("Stand Upgrade First"), None);
        assert_eq!(stand_animation_score("Birth"), None);
        assert_eq!(stand_animation_score("Death"), None);
    }

    #[test]
    fn rejects_invalid_building_manifest_entries() {
        let bad_scale = r#"{
            "schema_version": 1,
            "buildings": [{"rawcode": "h000", "scale": 0.0, "gltf": "models/foo.gltf"}]
        }"#;
        assert!(
            resolve_manifest_entries(bad_scale, "wc3/buildings")
                .expect_err("scale must be rejected")
                .contains("invalid model scale")
        );

        let bad_path = r#"{
            "schema_version": 1,
            "buildings": [{"rawcode": "h000", "scale": 1.0, "gltf": "../escape.gltf"}]
        }"#;
        assert!(
            resolve_manifest_entries(bad_path, "wc3/buildings")
                .expect_err("path must be rejected")
                .contains("safe relative asset path")
        );
    }
}
