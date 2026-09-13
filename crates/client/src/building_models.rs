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
const BUILDING_MODEL_MANIFEST_SCHEMA_VERSION: u32 = 3;

#[derive(Resource, Default)]
pub struct BuildingModelSet {
    models: BTreeMap<u32, BuildingModelAsset>,
}

#[derive(Clone)]
pub struct BuildingModelAsset {
    pub scene: Handle<WorldAsset>,
    gltf: Handle<Gltf>,
    pub scale: f32,
    animation_properties: Vec<String>,
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
    #[serde(default)]
    animation_properties: Vec<String>,
    #[serde(default)]
    fallback_to_base_art: bool,
    gltf: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct ResolvedBuildingAsset {
    rawcode: u32,
    scale: f32,
    animation_properties: Vec<String>,
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
                            animation_properties: entry.animation_properties,
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
            let Some(stand) = find_stand_animation(gltf, &model.animation_properties) else {
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

fn find_stand_animation(
    gltf: &Gltf,
    animation_properties: &[String],
) -> Option<Handle<AnimationClip>> {
    gltf.named_animations
        .iter()
        .filter_map(|(name, clip)| {
            stand_animation_score(name, animation_properties).map(|score| (score, name, clip))
        })
        .min_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(right.1)))
        .map(|(_, _, clip)| clip.clone())
}

fn stand_animation_score(name: &str, animation_properties: &[String]) -> Option<u8> {
    let name = name.to_ascii_lowercase();
    let words: Vec<_> = name
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    if words.first().copied() != Some("stand") || words.contains(&"work") {
        return None;
    }

    let required: Vec<_> = animation_properties
        .iter()
        .map(|property| property.to_ascii_lowercase())
        .collect();
    if !required
        .iter()
        .all(|property| words.contains(&property.as_str()))
    {
        return None;
    }
    if required.is_empty() && words.contains(&"upgrade") {
        return None;
    }

    let canonical = if required.is_empty() {
        "stand".to_owned()
    } else {
        format!("stand {}", required.join(" "))
    };
    if name == canonical {
        Some(0)
    } else if required.is_empty() && matches!(name.as_str(), "stand - 1" | "stand 1") {
        Some(1)
    } else {
        Some(2)
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
        let asset_path = format!("{}/{}", asset_prefix.trim_end_matches('/'), gltf);
        if resolved
            .insert(
                rawcode,
                ResolvedBuildingAsset {
                    rawcode,
                    scale: entry.scale,
                    animation_properties: entry.animation_properties,
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
    fn resolves_building_manifest_paths_scales_and_animation_properties() {
        let json = r#"{
            "schema_version": 3,
            "buildings": [
                {"rawcode": "h000", "scale": 0.5, "animation_properties": [], "fallback_to_base_art": false, "gltf": "models/humanbarracks.gltf"},
                {"rawcode": "h006", "scale": 0.8, "animation_properties": ["upgrade", "first"], "fallback_to_base_art": false, "gltf": "models/tower.gltf"},
                {"rawcode": "h07P", "scale": 0.8, "animation_properties": ["upgrade", "second"], "fallback_to_base_art": true, "gltf": "models/tower.gltf"},
                {"rawcode": "xxxx", "scale": 1.0, "animation_properties": [], "fallback_to_base_art": false, "gltf": null}
            ]
        }"#;
        let entries = resolve_manifest_entries(json, "wc3/buildings").expect("manifest resolves");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].rawcode, u32::from_be_bytes(*b"h000"));
        assert_eq!(entries[0].scale, 0.5);
        assert!(entries[0].animation_properties.is_empty());
        assert_eq!(
            entries[0].asset_path,
            "wc3/buildings/models/humanbarracks.gltf"
        );
        assert_eq!(entries[1].rawcode, u32::from_be_bytes(*b"h006"));
        assert_eq!(
            entries[1].animation_properties,
            vec!["upgrade".to_owned(), "first".to_owned()]
        );
    }

    #[test]
    fn building_stand_selector_honors_required_animation_properties() {
        let none: &[String] = &[];
        let guard_tower = ["upgrade".to_owned(), "first".to_owned()];
        let castle = ["upgrade".to_owned(), "second".to_owned()];

        assert_eq!(stand_animation_score("Stand", none), Some(0));
        assert_eq!(stand_animation_score("Stand - 1", none), Some(1));
        assert_eq!(stand_animation_score("Stand Ready Attack", none), Some(2));
        assert_eq!(stand_animation_score("Stand Work", none), None);
        assert_eq!(stand_animation_score("Stand Upgrade First", none), None);
        assert_eq!(
            stand_animation_score("Stand Upgrade First Ready Attack", &guard_tower),
            Some(2)
        );
        assert_eq!(
            stand_animation_score("Stand Upgrade Second", &castle),
            Some(0)
        );
        assert_eq!(
            stand_animation_score("Stand Upgrade Second", &guard_tower),
            None
        );
        assert_eq!(
            stand_animation_score("Stand Work Upgrade First", &guard_tower),
            None
        );
        assert_eq!(stand_animation_score("Birth", none), None);
        assert_eq!(stand_animation_score("Death", none), None);
    }

    #[test]
    fn rejects_invalid_building_manifest_entries() {
        let stale_schema = r#"{
            "schema_version": 2,
            "buildings": []
        }"#;
        assert!(
            resolve_manifest_entries(stale_schema, "wc3/buildings")
                .expect_err("stale schema must be rejected")
                .contains("unsupported building asset manifest schema")
        );

        let bad_scale = r#"{
            "schema_version": 3,
            "buildings": [{"rawcode": "h000", "scale": 0.0, "gltf": "models/foo.gltf"}]
        }"#;
        assert!(
            resolve_manifest_entries(bad_scale, "wc3/buildings")
                .expect_err("scale must be rejected")
                .contains("invalid model scale")
        );

        let bad_path = r#"{
            "schema_version": 3,
            "buildings": [{"rawcode": "h000", "scale": 1.0, "gltf": "../escape.gltf"}]
        }"#;
        assert!(
            resolve_manifest_entries(bad_path, "wc3/buildings")
                .expect_err("path must be rejected")
                .contains("safe relative asset path")
        );
    }
}
