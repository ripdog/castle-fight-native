use std::fs;

use bevy::{gltf::GltfAssetLabel, prelude::*};
use serde::Deserialize;

use crate::terrain::client_asset_root;

const DOODAD_MANIFEST: &str = "wc3/doodads/manifest.json";
const DOODAD_ASSET_PREFIX: &str = "wc3/doodads";

pub struct DoodadPresentationPlugin;

impl Plugin for DoodadPresentationPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(DoodadSceneCatalog::load_default())
            .add_systems(Startup, spawn_doodads);
    }
}

#[derive(Resource, Debug, Clone, Default)]
struct DoodadSceneCatalog {
    placements: Vec<DoodadScenePlacement>,
}

#[derive(Debug, Clone)]
struct DoodadScenePlacement {
    rawcode: String,
    name: String,
    editor_id: u32,
    gltf: String,
    position: [f32; 3],
    angle_degrees: f32,
    scale: [f32; 3],
}

#[derive(Debug, Deserialize)]
struct DoodadManifest {
    schema_version: u32,
    objects: Vec<DoodadObjectManifest>,
}

#[derive(Debug, Deserialize)]
struct DoodadObjectManifest {
    rawcode: String,
    name: String,
    placements: Vec<DoodadPlacementManifest>,
}

#[derive(Debug, Deserialize)]
struct DoodadPlacementManifest {
    editor_id: u32,
    position: [f32; 3],
    angle_degrees: f32,
    scale: [f32; 3],
    visible: bool,
    gltf: Option<String>,
}

impl DoodadSceneCatalog {
    fn load_default() -> Self {
        let path = client_asset_root().join(DOODAD_MANIFEST);
        if !path.is_file() {
            return Self::default();
        }
        match fs::read_to_string(&path)
            .map_err(|error| format!("failed reading {}: {error}", path.display()))
            .and_then(|json| Self::from_manifest_json(&json))
        {
            Ok(catalog) => catalog,
            Err(error) => {
                eprintln!("warning: ignoring generated WC3 doodad assets: {error}");
                Self::default()
            }
        }
    }

    fn from_manifest_json(json: &str) -> Result<Self, String> {
        let manifest: DoodadManifest = serde_json::from_str(json)
            .map_err(|error| format!("invalid doodad manifest: {error}"))?;
        if manifest.schema_version != 1 {
            return Err(format!(
                "unsupported doodad manifest schema {}",
                manifest.schema_version
            ));
        }

        let mut placements = Vec::new();
        for object in manifest.objects {
            for placement in object.placements {
                if !placement.visible {
                    continue;
                }
                let Some(gltf) = placement.gltf else {
                    continue;
                };
                placements.push(DoodadScenePlacement {
                    rawcode: object.rawcode.clone(),
                    name: object.name.clone(),
                    editor_id: placement.editor_id,
                    gltf: format!("{DOODAD_ASSET_PREFIX}/{}", gltf.replace('\\', "/")),
                    position: placement.position,
                    angle_degrees: placement.angle_degrees,
                    scale: placement.scale,
                });
            }
        }
        placements.sort_by_key(|placement| placement.editor_id);
        Ok(Self { placements })
    }
}

fn spawn_doodads(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    catalog: Res<DoodadSceneCatalog>,
) {
    if catalog.placements.is_empty() {
        return;
    }

    for placement in &catalog.placements {
        let scene = asset_server.load(GltfAssetLabel::Scene(0).from_asset(placement.gltf.clone()));
        commands.spawn((
            Name::new(format!(
                "WC3 doodad {} {} #{}",
                placement.rawcode, placement.name, placement.editor_id
            )),
            WorldAssetRoot(scene),
            doodad_transform(placement.position, placement.angle_degrees, placement.scale),
        ));
    }
    println!(
        "spawned {} Warcraft III doodad/destructable scene instances",
        catalog.placements.len()
    );
}

fn doodad_transform(position: [f32; 3], angle_degrees: f32, scale: [f32; 3]) -> Transform {
    Transform {
        // The client terrain uses WC3 X/Y as Bevy X/Z, while MDX geometry was
        // converted from WC3 Z-up to glTF Y-up by the shared asset exporter.
        translation: Vec3::new(position[0], position[2], position[1]),
        rotation: Quat::from_rotation_y(angle_degrees.to_radians()),
        scale: Vec3::new(scale[0], scale[2], scale[1]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doodad_transform_maps_wc3_world_axes_to_client_world() {
        let transform = doodad_transform([12.0, -34.0, 56.0], 90.0, [1.0, 2.0, 3.0]);
        assert_eq!(transform.translation, Vec3::new(12.0, 56.0, -34.0));
        assert_eq!(transform.scale, Vec3::new(1.0, 3.0, 2.0));
        let facing = transform.rotation * Vec3::X;
        assert!((facing.x).abs() < 0.0001);
        assert!((facing.z + 1.0).abs() < 0.0001);
    }

    #[test]
    fn manifest_loader_omits_invisible_or_unresolved_placements() {
        let json = r#"{
            "schema_version": 1,
            "objects": [{
                "rawcode": "TEST",
                "name": "Test",
                "placements": [
                    {"editor_id":2,"position":[0,0,0],"angle_degrees":0,"scale":[1,1,1],"visible":true,"gltf":"models/a.gltf"},
                    {"editor_id":1,"position":[0,0,0],"angle_degrees":0,"scale":[1,1,1],"visible":true,"gltf":null},
                    {"editor_id":3,"position":[0,0,0],"angle_degrees":0,"scale":[1,1,1],"visible":false,"gltf":"models/b.gltf"}
                ]
            }]
        }"#;
        let catalog = DoodadSceneCatalog::from_manifest_json(json).unwrap();
        assert_eq!(catalog.placements.len(), 1);
        assert_eq!(catalog.placements[0].editor_id, 2);
        assert_eq!(catalog.placements[0].gltf, "wc3/doodads/models/a.gltf");
    }
}
