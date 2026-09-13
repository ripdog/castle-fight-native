use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

use bevy::prelude::*;
use serde::Deserialize;

use crate::terrain::client_asset_root;

const UNIT_MODEL_MANIFEST: &str = "wc3/units/manifest.json";
const UNIT_MODEL_ASSET_PREFIX: &str = "wc3/units";
const INITIAL_WC3_MODEL_RAWCODES: [u32; 2] =
    [u32::from_be_bytes(*b"hfoo"), u32::from_be_bytes(*b"n015")];

#[derive(Resource, Default)]
pub struct UnitModelSet {
    models: BTreeMap<u32, UnitModelAsset>,
}

#[derive(Clone)]
pub struct UnitModelAsset {
    pub scene: Handle<WorldAsset>,
    pub scale: f32,
}

#[derive(Debug, Deserialize)]
struct UnitAssetManifest {
    schema_version: u32,
    units: Vec<UnitAssetManifestEntry>,
}

#[derive(Debug, Deserialize)]
struct UnitAssetManifestEntry {
    rawcode: String,
    scale: f32,
    gltf: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct ResolvedUnitAsset {
    rawcode: u32,
    scale: f32,
    asset_path: String,
}

impl UnitModelSet {
    #[must_use]
    pub fn load_default(asset_server: &AssetServer) -> Self {
        let asset_root = client_asset_root();
        let manifest_path = asset_root.join(UNIT_MODEL_MANIFEST);
        if !manifest_path.is_file() {
            return Self::default();
        }

        match load_manifest_entries(&manifest_path, UNIT_MODEL_ASSET_PREFIX) {
            Ok(entries) => {
                let mut models = BTreeMap::new();
                for entry in entries {
                    if !INITIAL_WC3_MODEL_RAWCODES.contains(&entry.rawcode) {
                        continue;
                    }
                    if !asset_root.join(&entry.asset_path).is_file() {
                        eprintln!(
                            "warning: generated WC3 unit model is missing: {}",
                            entry.asset_path
                        );
                        continue;
                    }
                    let scene = asset_server
                        .load(GltfAssetLabel::Scene(0).from_asset(entry.asset_path.clone()));
                    models.insert(
                        entry.rawcode,
                        UnitModelAsset {
                            scene,
                            scale: entry.scale,
                        },
                    );
                }
                if !models.is_empty() {
                    println!("Loaded {} generated WC3 unit model(s)", models.len());
                }
                Self { models }
            }
            Err(error) => {
                eprintln!("warning: ignoring generated WC3 unit models: {error}");
                Self::default()
            }
        }
    }

    #[must_use]
    pub fn get(&self, rawcode: u32) -> Option<&UnitModelAsset> {
        self.models.get(&rawcode)
    }
}

fn load_manifest_entries(
    path: &Path,
    asset_prefix: &str,
) -> Result<Vec<ResolvedUnitAsset>, String> {
    let json = fs::read_to_string(path)
        .map_err(|error| format!("failed reading {}: {error}", path.display()))?;
    resolve_manifest_entries(&json, asset_prefix)
}

fn resolve_manifest_entries(
    json: &str,
    asset_prefix: &str,
) -> Result<Vec<ResolvedUnitAsset>, String> {
    let manifest: UnitAssetManifest = serde_json::from_str(json)
        .map_err(|error| format!("invalid unit asset manifest: {error}"))?;
    if manifest.schema_version != 1 {
        return Err(format!(
            "unsupported unit asset manifest schema {}",
            manifest.schema_version
        ));
    }

    let mut resolved = BTreeMap::new();
    for entry in manifest.units {
        let Some(gltf) = entry.gltf else {
            continue;
        };
        let rawcode = parse_rawcode(&entry.rawcode)?;
        if !entry.scale.is_finite() || entry.scale <= 0.0 {
            return Err(format!(
                "unit {} has invalid model scale {}",
                entry.rawcode, entry.scale
            ));
        }
        let gltf = gltf.replace('\\', "/");
        validate_relative_asset_path(&gltf)?;
        let asset_path = format!("{}/{}", asset_prefix.trim_end_matches('/'), gltf);
        if resolved
            .insert(
                rawcode,
                ResolvedUnitAsset {
                    rawcode,
                    scale: entry.scale,
                    asset_path,
                },
            )
            .is_some()
        {
            return Err(format!("duplicate unit rawcode {}", entry.rawcode));
        }
    }
    Ok(resolved.into_values().collect())
}

fn parse_rawcode(rawcode: &str) -> Result<u32, String> {
    let bytes = rawcode.as_bytes();
    let bytes: [u8; 4] = bytes
        .try_into()
        .map_err(|_| format!("unit rawcode {rawcode:?} is not exactly four bytes"))?;
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
            "unit model path {} is not a safe relative asset path",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_generated_unit_manifest_paths_and_scales() {
        let json = r#"{
            "schema_version": 1,
            "units": [
                {
                    "rawcode": "hfoo",
                    "scale": 1.0,
                    "gltf": "models/units__human__footman__footman.gltf"
                },
                {
                    "rawcode": "h016",
                    "scale": 1.2,
                    "gltf": "models/units__human__gryphonrider__gryphonrider.gltf"
                },
                {
                    "rawcode": "x000",
                    "scale": 1.0,
                    "gltf": null
                }
            ]
        }"#;

        let entries = resolve_manifest_entries(json, "wc3/units").expect("manifest resolves");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].rawcode, u32::from_be_bytes(*b"h016"));
        assert_eq!(entries[0].scale, 1.2);
        assert_eq!(
            entries[0].asset_path,
            "wc3/units/models/units__human__gryphonrider__gryphonrider.gltf"
        );
        assert_eq!(entries[1].rawcode, u32::from_be_bytes(*b"hfoo"));
    }

    #[test]
    fn rejects_unsafe_model_paths() {
        for path in ["../escape.gltf", "..\\\\escape.gltf"] {
            let json = format!(
                r#"{{
                    "schema_version": 1,
                    "units": [{{"rawcode": "hfoo", "scale": 1.0, "gltf": {path:?}}}]
                }}"#
            );
            let error =
                resolve_manifest_entries(&json, "wc3/units").expect_err("path must be rejected");
            assert!(error.contains("safe relative asset path"));
        }
    }

    #[test]
    fn rejects_invalid_rawcodes_and_scales() {
        let bad_rawcode = r#"{
            "schema_version": 1,
            "units": [{"rawcode": "foo", "scale": 1.0, "gltf": "models/foo.gltf"}]
        }"#;
        assert!(
            resolve_manifest_entries(bad_rawcode, "wc3/units")
                .expect_err("rawcode must be rejected")
                .contains("exactly four bytes")
        );

        let bad_scale = r#"{
            "schema_version": 1,
            "units": [{"rawcode": "hfoo", "scale": 0.0, "gltf": "models/foo.gltf"}]
        }"#;
        assert!(
            resolve_manifest_entries(bad_scale, "wc3/units")
                .expect_err("scale must be rejected")
                .contains("invalid model scale")
        );
    }
}
