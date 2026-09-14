use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

use bevy::{gltf::GltfAssetLabel, prelude::*, world_serialization::WorldInstanceReady};
use serde::Deserialize;

use crate::{
    terrain::client_asset_root,
    wc3_effects::{Wc3EmitterSource, Wc3ParticleEmitter, Wc3RibbonEmitter, Wc3RibbonSource},
};

const DOODAD_MANIFEST: &str = "wc3/doodads/manifest.json";
const DOODAD_ASSET_PREFIX: &str = "wc3/doodads";
const DOODAD_MANIFEST_SCHEMA_VERSION: u32 = 2;
const DOODAD_AMBIENT_ANIMATION_SPEED: f32 = 0.5;

pub struct DoodadPresentationPlugin;

impl Plugin for DoodadPresentationPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(DoodadSceneCatalog::load_default())
            .add_systems(Startup, spawn_doodads);
    }
}

#[derive(Component, Clone)]
struct DoodadAnimationToPlay {
    graph_handle: Handle<AnimationGraph>,
    index: AnimationNodeIndex,
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
    stand_animation_index: Option<usize>,
    particle_emitters: Vec<Wc3ParticleEmitter>,
    ribbon_emitters: Vec<Wc3RibbonEmitter>,
}

#[derive(Debug, Deserialize)]
struct DoodadManifest {
    schema_version: u32,
    objects: Vec<DoodadObjectManifest>,
    #[serde(default)]
    models: Vec<DoodadModelManifest>,
}

#[derive(Debug, Deserialize)]
struct DoodadModelManifest {
    gltf: String,
    #[serde(default)]
    animations: Vec<DoodadAnimationManifest>,
    #[serde(default)]
    particle_emitters: Vec<Wc3ParticleEmitter>,
    #[serde(default)]
    ribbon_emitters: Vec<Wc3RibbonEmitter>,
}

#[derive(Debug, Clone, Default)]
struct DoodadModelPresentation {
    stand_animation_index: Option<usize>,
    particle_emitters: Vec<Wc3ParticleEmitter>,
    ribbon_emitters: Vec<Wc3RibbonEmitter>,
}

#[derive(Debug, Deserialize)]
struct DoodadAnimationManifest {
    name: String,
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
        if manifest.schema_version != DOODAD_MANIFEST_SCHEMA_VERSION {
            return Err(format!(
                "unsupported doodad manifest schema {}; expected {} (re-run the WC3 doodad extractor)",
                manifest.schema_version, DOODAD_MANIFEST_SCHEMA_VERSION
            ));
        }

        let model_presentation: BTreeMap<_, _> = manifest
            .models
            .into_iter()
            .filter_map(|model| {
                let gltf = normalize_asset_path(&model.gltf).ok()?;
                let stand_animation_index = model
                    .animations
                    .iter()
                    .position(|animation| animation.name.trim().eq_ignore_ascii_case("stand"));
                Some((
                    gltf,
                    DoodadModelPresentation {
                        stand_animation_index,
                        particle_emitters: model
                            .particle_emitters
                            .into_iter()
                            .filter(|emitter| emitter.ambient_enabled)
                            .collect(),
                        ribbon_emitters: model.ribbon_emitters,
                    },
                ))
            })
            .collect();

        let mut placements = Vec::new();
        for object in manifest.objects {
            for placement in object.placements {
                if !placement.visible {
                    continue;
                }
                let Some(gltf) = placement.gltf else {
                    continue;
                };
                let gltf = normalize_asset_path(&gltf)?;
                let model = model_presentation.get(&gltf).cloned().unwrap_or_default();
                placements.push(DoodadScenePlacement {
                    rawcode: object.rawcode.clone(),
                    name: object.name.clone(),
                    editor_id: placement.editor_id,
                    gltf: format!("{DOODAD_ASSET_PREFIX}/{gltf}"),
                    position: placement.position,
                    angle_degrees: placement.angle_degrees,
                    scale: placement.scale,
                    stand_animation_index: model.stand_animation_index,
                    particle_emitters: model.particle_emitters,
                    ribbon_emitters: model.ribbon_emitters,
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
    mut animation_graphs: ResMut<Assets<AnimationGraph>>,
    catalog: Res<DoodadSceneCatalog>,
) {
    if catalog.placements.is_empty() {
        return;
    }

    let mut animation_cache =
        BTreeMap::<String, (Handle<AnimationGraph>, AnimationNodeIndex)>::new();
    for placement in &catalog.placements {
        let scene = asset_server.load(GltfAssetLabel::Scene(0).from_asset(placement.gltf.clone()));
        let mut entity = commands.spawn((
            Name::new(format!(
                "WC3 doodad {} {} #{}",
                placement.rawcode, placement.name, placement.editor_id
            )),
            WorldAssetRoot(scene),
            doodad_transform(placement.position, placement.angle_degrees, placement.scale),
        ));

        if let Some(animation_index) = placement.stand_animation_index {
            let (graph_handle, index) = animation_cache
                .entry(placement.gltf.clone())
                .or_insert_with(|| {
                    let clip = asset_server.load(
                        GltfAssetLabel::Animation(animation_index)
                            .from_asset(placement.gltf.clone()),
                    );
                    let (graph, index) = AnimationGraph::from_clip(clip);
                    (animation_graphs.add(graph), index)
                })
                .clone();
            entity
                .insert(DoodadAnimationToPlay {
                    graph_handle,
                    index,
                })
                .observe(play_doodad_animation_when_ready);
        }
        if !placement.particle_emitters.is_empty() {
            entity.insert(Wc3EmitterSource::with_asset_prefix(
                &placement.particle_emitters,
                DOODAD_ASSET_PREFIX,
            ));
        }
        if !placement.ribbon_emitters.is_empty() {
            entity.insert(Wc3RibbonSource::with_asset_prefix(
                &placement.ribbon_emitters,
                DOODAD_ASSET_PREFIX,
            ));
        }
    }
    println!(
        "spawned {} Warcraft III doodad/destructable scene instances",
        catalog.placements.len()
    );
}

fn play_doodad_animation_when_ready(
    scene_ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    animations: Query<&DoodadAnimationToPlay>,
    mut players: Query<&mut AnimationPlayer>,
) {
    let Ok(animation) = animations.get(scene_ready.entity) else {
        return;
    };
    for child in children.iter_descendants(scene_ready.entity) {
        if let Ok(mut player) = players.get_mut(child) {
            player
                .play(animation.index)
                .repeat()
                .set_speed(DOODAD_AMBIENT_ANIMATION_SPEED);
            commands
                .entity(child)
                .insert(AnimationGraphHandle(animation.graph_handle.clone()));
        }
    }
}

fn normalize_asset_path(path: &str) -> Result<String, String> {
    let normalized = path.replace('\\', "/");
    let path = Path::new(&normalized);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "doodad model path {} is not a safe relative asset path",
            path.display()
        ));
    }
    Ok(normalized)
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
            "schema_version": 2,
            "objects": [{
                "rawcode": "TEST",
                "name": "Test",
                "placements": [
                    {"editor_id":2,"position":[0,0,0],"angle_degrees":0,"scale":[1,1,1],"visible":true,"gltf":"models/a.gltf"},
                    {"editor_id":1,"position":[0,0,0],"angle_degrees":0,"scale":[1,1,1],"visible":true,"gltf":null},
                    {"editor_id":3,"position":[0,0,0],"angle_degrees":0,"scale":[1,1,1],"visible":false,"gltf":"models/b.gltf"}
                ]
            }],
            "models": [{
                "gltf": "models/a.gltf",
                "animations": [{"name":"Birth"},{"name":"Stand"},{"name":"Death"}]
            }]
        }"#;
        let catalog = DoodadSceneCatalog::from_manifest_json(json).unwrap();
        assert_eq!(catalog.placements.len(), 1);
        assert_eq!(catalog.placements[0].editor_id, 2);
        assert_eq!(catalog.placements[0].gltf, "wc3/doodads/models/a.gltf");
        assert_eq!(catalog.placements[0].stand_animation_index, Some(1));
    }

    #[test]
    fn manifest_loader_does_not_autoplay_stand_variants() {
        let json = r#"{
            "schema_version": 2,
            "objects": [{
                "rawcode": "WALL",
                "name": "Wall",
                "placements": [
                    {"editor_id":1,"position":[0,0,0],"angle_degrees":0,"scale":[1,1,1],"visible":true,"gltf":"models/wall.gltf"}
                ]
            }],
            "models": [{
                "gltf": "models/wall.gltf",
                "animations": [{"name":"Stand Hit"},{"name":"Death"}]
            }]
        }"#;
        let catalog = DoodadSceneCatalog::from_manifest_json(json).unwrap();
        assert_eq!(catalog.placements.len(), 1);
        assert_eq!(catalog.placements[0].stand_animation_index, None);
    }

    #[test]
    fn manifest_loader_preserves_wc3_particle_emitters() {
        let json = r#"{
            "schema_version": 2,
            "objects": [{
                "rawcode": "FIRE",
                "name": "Fire",
                "placements": [
                    {"editor_id":1,"position":[0,0,0],"angle_degrees":0,"scale":[1,1,1],"visible":true,"gltf":"models/fire.gltf"}
                ]
            }],
            "models": [{
                "gltf": "models/fire.gltf",
                "animations": [{"name":"Stand"}],
                "particle_emitters": [{
                    "position":[1,2,3],
                    "filter_mode":1,
                    "speed":10,
                    "variation":0,
                    "latitude":5,
                    "gravity":0,
                    "lifespan":1,
                    "emission_rate":20,
                    "rows":8,
                    "columns":8,
                    "segment_colors":[[1,0.5,0],[1,0,0],[0,0,0]],
                    "segment_alpha":[255,128,0],
                    "segment_scaling":[2,1,0.5],
                    "texture":"textures/flame.png",
                    "squirt":false,
                    "ambient_enabled":true
                },{
                    "position":[0,0,0],
                    "filter_mode":0,
                    "speed":0,
                    "variation":0,
                    "latitude":0,
                    "gravity":0,
                    "lifespan":1,
                    "emission_rate":8,
                    "rows":1,
                    "columns":1,
                    "segment_colors":[[1,1,1],[1,1,1],[1,1,1]],
                    "segment_alpha":[255,255,0],
                    "segment_scaling":[1,1,1],
                    "texture":"textures/death-only.png",
                    "squirt":false,
                    "ambient_enabled":false
                }]
            }]
        }"#;
        let catalog = DoodadSceneCatalog::from_manifest_json(json).unwrap();
        assert_eq!(catalog.placements.len(), 1);
        let placement = &catalog.placements[0];
        assert_eq!(placement.stand_animation_index, Some(0));
        assert_eq!(placement.particle_emitters.len(), 1);
        assert_eq!(placement.particle_emitters[0].rows, 8);
        assert_eq!(placement.particle_emitters[0].columns, 8);
        assert_eq!(
            placement.particle_emitters[0].texture.as_deref(),
            Some("textures/flame.png")
        );
    }

    #[test]
    fn manifest_loader_rejects_stale_doodad_schema() {
        let json = r#"{
            "schema_version": 1,
            "objects": [],
            "models": []
        }"#;
        let error = DoodadSceneCatalog::from_manifest_json(json)
            .expect_err("stale doodad pack must require regeneration");
        assert!(error.contains("re-run the WC3 doodad extractor"));
    }

    #[test]
    fn manifest_loader_rejects_unsafe_model_paths() {
        let json = r#"{
            "schema_version": 2,
            "objects": [{
                "rawcode": "TEST",
                "name": "Test",
                "placements": [
                    {"editor_id":1,"position":[0,0,0],"angle_degrees":0,"scale":[1,1,1],"visible":true,"gltf":"../escape.gltf"}
                ]
            }]
        }"#;
        assert!(
            DoodadSceneCatalog::from_manifest_json(json)
                .expect_err("unsafe path must be rejected")
                .contains("safe relative asset path")
        );
    }
}
