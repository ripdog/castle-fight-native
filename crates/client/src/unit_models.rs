use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

use bevy::{gltf::Gltf, prelude::*};
use serde::Deserialize;

use crate::{
    terrain::client_asset_root,
    wc3_effects::{Wc3ParticleEmitter, Wc3RibbonEmitter},
};

const UNIT_MODEL_MANIFEST: &str = "wc3/units/manifest.json";
const UNIT_MODEL_ASSET_PREFIX: &str = "wc3/units";
const UNIT_MODEL_MANIFEST_SCHEMA_VERSION: u32 = 5;
#[derive(Resource, Default)]
pub struct UnitModelSet {
    models: BTreeMap<u32, UnitModelAsset>,
}

#[derive(Clone)]
pub struct UnitModelAsset {
    pub scene: Handle<WorldAsset>,
    gltf: Handle<Gltf>,
    pub scale: f32,
    pub overhead_height: Option<f32>,
    pub tint_rgb: Option<[u8; 3]>,
    pub attached_visuals: Vec<UnitAttachedVisual>,
    pub particle_emitters: Vec<Wc3ParticleEmitter>,
    pub ribbon_emitters: Vec<Wc3RibbonEmitter>,
    animations: Option<UnitAnimationSet>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UnitAttachedVisual {
    pub ability_rawcode: String,
    pub attachment_point: String,
}

#[derive(Debug, Clone, Copy)]
pub struct UnitAnimationClip {
    pub node: AnimationNodeIndex,
    pub duration_seconds: f32,
}

#[derive(Debug, Clone)]
pub struct UnitAnimationSequenceNames {
    pub stand: String,
    pub walk: Option<String>,
    pub attack: Option<String>,
    pub defend_stand: Option<String>,
    pub defend_walk: Option<String>,
    pub defend_attack: Option<String>,
    pub cast: Option<String>,
    pub death: Option<String>,
    pub decay_flesh: Option<String>,
    pub decay_bone: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UnitAnimationSet {
    pub graph: Handle<AnimationGraph>,
    pub sequences: UnitAnimationSequenceNames,
    pub stand: AnimationNodeIndex,
    pub walk: Option<AnimationNodeIndex>,
    pub attack: Option<AnimationNodeIndex>,
    pub defend_stand: Option<AnimationNodeIndex>,
    pub defend_walk: Option<AnimationNodeIndex>,
    pub defend_attack: Option<AnimationNodeIndex>,
    pub cast: Option<AnimationNodeIndex>,
    pub death: Option<UnitAnimationClip>,
    pub decay_flesh: Option<UnitAnimationClip>,
    pub decay_bone: Option<UnitAnimationClip>,
}

#[derive(Debug, Deserialize)]
struct UnitAssetManifest {
    schema_version: u32,
    units: Vec<UnitAssetManifestEntry>,
    models: Vec<UnitModelManifestEntry>,
}

#[derive(Debug, Deserialize)]
struct UnitAssetManifestEntry {
    rawcode: String,
    scale: f32,
    gltf: Option<String>,
    tint_rgb: Option<[u8; 3]>,
    #[serde(default)]
    attached_visuals: Vec<UnitAttachedVisual>,
}

#[derive(Debug, Deserialize)]
struct UnitModelManifestEntry {
    gltf: String,
    overhead_position: Option<[f32; 3]>,
    #[serde(default)]
    particle_emitters: Vec<Wc3ParticleEmitter>,
    #[serde(default)]
    ribbon_emitters: Vec<Wc3RibbonEmitter>,
}

#[derive(Debug, Clone)]
struct ResolvedUnitAsset {
    rawcode: u32,
    scale: f32,
    overhead_height: Option<f32>,
    asset_path: String,
    tint_rgb: Option<[u8; 3]>,
    attached_visuals: Vec<UnitAttachedVisual>,
    particle_emitters: Vec<Wc3ParticleEmitter>,
    ribbon_emitters: Vec<Wc3RibbonEmitter>,
}

impl UnitModelSet {
    #[must_use]
    pub fn load_selected(asset_server: &AssetServer, selected_rawcodes: &[u32]) -> Self {
        let asset_root = client_asset_root();
        let manifest_path = asset_root.join(UNIT_MODEL_MANIFEST);
        if !manifest_path.is_file() {
            return Self::default();
        }

        match load_manifest_entries(&manifest_path, UNIT_MODEL_ASSET_PREFIX) {
            Ok(entries) => {
                let mut models = BTreeMap::new();
                for entry in entries {
                    if !selected_rawcodes.contains(&entry.rawcode) {
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
                    let gltf = asset_server.load(entry.asset_path.clone());
                    models.insert(
                        entry.rawcode,
                        UnitModelAsset {
                            scene,
                            gltf,
                            scale: entry.scale,
                            overhead_height: entry.overhead_height,
                            tint_rgb: entry.tint_rgb,
                            attached_visuals: entry.attached_visuals,
                            particle_emitters: entry.particle_emitters,
                            ribbon_emitters: entry.ribbon_emitters,
                            animations: None,
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

    pub fn prepare_animations(
        &mut self,
        gltfs: &Assets<Gltf>,
        animation_clips: &Assets<AnimationClip>,
        graphs: &mut Assets<AnimationGraph>,
    ) {
        for model in self
            .models
            .values_mut()
            .filter(|model| model.animations.is_none())
        {
            let Some(gltf) = gltfs.get(&model.gltf) else {
                continue;
            };
            let Some(stand) = find_animation(gltf, AnimationRole::Stand) else {
                continue;
            };
            let walk = find_animation(gltf, AnimationRole::Walk);
            let attack = find_animation(gltf, AnimationRole::Attack);
            let defend_stand = find_animation(gltf, AnimationRole::DefendStand);
            let defend_walk = find_animation(gltf, AnimationRole::DefendWalk);
            let defend_attack = find_animation(gltf, AnimationRole::DefendAttack);
            let cast = find_animation(gltf, AnimationRole::Cast);
            let death = find_animation(gltf, AnimationRole::Death);
            let decay_flesh = find_animation(gltf, AnimationRole::DecayFlesh);
            let decay_bone = find_animation(gltf, AnimationRole::DecayBone);

            let selected = [
                Some(&stand),
                walk.as_ref(),
                attack.as_ref(),
                defend_stand.as_ref(),
                defend_walk.as_ref(),
                defend_attack.as_ref(),
                cast.as_ref(),
                death.as_ref(),
                decay_flesh.as_ref(),
                decay_bone.as_ref(),
            ];
            if selected
                .into_iter()
                .flatten()
                .any(|animation| animation_clips.get(&animation.clip).is_none())
            {
                continue;
            }

            let sequences = UnitAnimationSequenceNames {
                stand: stand.name.clone(),
                walk: walk.as_ref().map(|animation| animation.name.clone()),
                attack: attack.as_ref().map(|animation| animation.name.clone()),
                defend_stand: defend_stand
                    .as_ref()
                    .map(|animation| animation.name.clone()),
                defend_walk: defend_walk.as_ref().map(|animation| animation.name.clone()),
                defend_attack: defend_attack
                    .as_ref()
                    .map(|animation| animation.name.clone()),
                cast: cast.as_ref().map(|animation| animation.name.clone()),
                death: death.as_ref().map(|animation| animation.name.clone()),
                decay_flesh: decay_flesh.as_ref().map(|animation| animation.name.clone()),
                decay_bone: decay_bone.as_ref().map(|animation| animation.name.clone()),
            };
            let mut clips = vec![stand.clip];
            let walk_slot = append_optional_clip(&mut clips, walk.map(|animation| animation.clip));
            let attack_slot =
                append_optional_clip(&mut clips, attack.map(|animation| animation.clip));
            let defend_stand_slot =
                append_optional_clip(&mut clips, defend_stand.map(|animation| animation.clip));
            let defend_walk_slot =
                append_optional_clip(&mut clips, defend_walk.map(|animation| animation.clip));
            let defend_attack_slot =
                append_optional_clip(&mut clips, defend_attack.map(|animation| animation.clip));
            let cast_slot = append_optional_clip(&mut clips, cast.map(|animation| animation.clip));
            let death_slot =
                append_optional_clip(&mut clips, death.map(|animation| animation.clip));
            let decay_flesh_slot =
                append_optional_clip(&mut clips, decay_flesh.map(|animation| animation.clip));
            let decay_bone_slot =
                append_optional_clip(&mut clips, decay_bone.map(|animation| animation.clip));
            let durations: Vec<f32> = clips
                .iter()
                .map(|clip| {
                    animation_clips
                        .get(clip)
                        .expect("selected glTF animation clip must be loaded")
                        .duration()
                })
                .collect();
            let (graph, nodes) = AnimationGraph::from_clips(clips);
            let graph = graphs.add(graph);
            model.animations = Some(UnitAnimationSet {
                graph,
                sequences,
                stand: nodes[0],
                walk: walk_slot.map(|slot| nodes[slot]),
                attack: attack_slot.map(|slot| nodes[slot]),
                defend_stand: defend_stand_slot.map(|slot| nodes[slot]),
                defend_walk: defend_walk_slot.map(|slot| nodes[slot]),
                defend_attack: defend_attack_slot.map(|slot| nodes[slot]),
                cast: cast_slot.map(|slot| nodes[slot]),
                death: death_slot.map(|slot| UnitAnimationClip {
                    node: nodes[slot],
                    duration_seconds: durations[slot],
                }),
                decay_flesh: decay_flesh_slot.map(|slot| UnitAnimationClip {
                    node: nodes[slot],
                    duration_seconds: durations[slot],
                }),
                decay_bone: decay_bone_slot.map(|slot| UnitAnimationClip {
                    node: nodes[slot],
                    duration_seconds: durations[slot],
                }),
            });
        }
    }

    #[must_use]
    pub fn get(&self, rawcode: u32) -> Option<&UnitModelAsset> {
        self.models.get(&rawcode)
    }

    #[must_use]
    pub fn animations(&self, rawcode: u32) -> Option<&UnitAnimationSet> {
        self.models.get(&rawcode)?.animations.as_ref()
    }
}

#[derive(Debug, Clone)]
struct NamedUnitAnimation {
    name: String,
    clip: Handle<AnimationClip>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnimationRole {
    Stand,
    Walk,
    Attack,
    DefendStand,
    DefendWalk,
    DefendAttack,
    Cast,
    Death,
    DecayFlesh,
    DecayBone,
}

fn find_animation(gltf: &Gltf, role: AnimationRole) -> Option<NamedUnitAnimation> {
    gltf.named_animations
        .iter()
        .filter_map(|(name, clip)| animation_score(name, role).map(|score| (score, name, clip)))
        .min_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(right.1)))
        .map(|(_, name, clip)| NamedUnitAnimation {
            name: name.to_string(),
            clip: clip.clone(),
        })
}

fn animation_score(name: &str, role: AnimationRole) -> Option<u8> {
    let name = name.to_ascii_lowercase();
    match role {
        AnimationRole::Stand => match name.as_str() {
            "stand - 1" | "stand 1" => Some(0),
            "stand" => Some(1),
            _ if name.starts_with("stand")
                && !name.contains("defend")
                && !name.contains("victory") =>
            {
                Some(2)
            }
            _ => None,
        },
        AnimationRole::Walk => match name.as_str() {
            "walk" => Some(0),
            "walk - 1" | "walk 1" => Some(1),
            _ if name.starts_with("walk") && !name.contains("defend") => Some(2),
            _ => None,
        },
        AnimationRole::Attack => match name.as_str() {
            "attack - 1" | "attack 1" => Some(0),
            "attack" => Some(1),
            _ if name.starts_with("attack") && !name.contains("defend") => Some(2),
            _ => None,
        },
        AnimationRole::DefendStand => {
            (name.starts_with("stand") && name.contains("defend")).then_some(0)
        }
        AnimationRole::DefendWalk => {
            (name.starts_with("walk") && name.contains("defend")).then_some(0)
        }
        AnimationRole::DefendAttack => {
            (name.starts_with("attack") && name.contains("defend")).then_some(0)
        }
        AnimationRole::Cast => match name.as_str() {
            "spell" => Some(0),
            "spell - 1" | "spell 1" => Some(1),
            _ if name.starts_with("spell") => Some(2),
            _ => None,
        },
        AnimationRole::Death => match name.as_str() {
            "death" => Some(0),
            _ if name.starts_with("death") => Some(1),
            _ => None,
        },
        AnimationRole::DecayFlesh => match name.as_str() {
            "decay flesh" => Some(0),
            "decay" => Some(1),
            _ if name.starts_with("decay flesh") => Some(2),
            _ => None,
        },
        AnimationRole::DecayBone => match name.as_str() {
            "decay bone" => Some(0),
            _ if name.starts_with("decay bone") => Some(1),
            _ => None,
        },
    }
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
    if manifest.schema_version != UNIT_MODEL_MANIFEST_SCHEMA_VERSION {
        return Err(format!(
            "unsupported unit asset manifest schema {}",
            manifest.schema_version
        ));
    }

    let mut model_metadata = BTreeMap::new();
    for model in manifest.models {
        let gltf = model.gltf.replace('\\', "/");
        validate_relative_asset_path(&gltf)?;
        if model_metadata
            .insert(
                gltf.clone(),
                (
                    model.overhead_position,
                    model.particle_emitters,
                    model.ribbon_emitters,
                ),
            )
            .is_some()
        {
            return Err(format!("duplicate unit model manifest path {gltf}"));
        }
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
        let (overhead_position, particle_emitters, ribbon_emitters) =
            model_metadata.get(&gltf).cloned().ok_or_else(|| {
                format!(
                    "unit {} references missing model manifest {gltf}",
                    entry.rawcode
                )
            })?;
        let overhead_height = overhead_position
            .map(|position| position[1] * entry.scale)
            .filter(|height| height.is_finite() && *height > 0.0);
        let asset_path = format!("{}/{}", asset_prefix.trim_end_matches('/'), gltf);
        if resolved
            .insert(
                rawcode,
                ResolvedUnitAsset {
                    rawcode,
                    scale: entry.scale,
                    overhead_height,
                    asset_path,
                    tint_rgb: entry.tint_rgb,
                    attached_visuals: entry.attached_visuals,
                    particle_emitters,
                    ribbon_emitters,
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
    #[ignore = "requires a locally extracted SD assets/wc3 presentation pack"]
    fn extracted_pack_resolves_all_delivered_unit_bindings() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/wc3/units");
        let json = fs::read_to_string(root.join("manifest.json")).unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&json).unwrap();
        let entries = resolve_manifest_entries(&json, "wc3/units").unwrap();
        for source in manifest["units"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|source| source["gltf"].is_string())
        {
            let rawcode = parse_rawcode(source["rawcode"].as_str().unwrap()).unwrap();
            assert!(entries.iter().any(|entry| entry.rawcode == rawcode));
            assert!(root.join(source["gltf"].as_str().unwrap()).is_file());
        }
    }

    #[test]
    fn resolves_generated_unit_manifest_paths_and_scales() {
        let json = r#"{
            "schema_version": 5,
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
            ],
            "models": [
                {
                    "gltf": "models/units__human__footman__footman.gltf",
                    "overhead_position": [0.0, 120.0, 0.0]
                },
                {
                    "gltf": "models/units__human__gryphonrider__gryphonrider.gltf",
                    "overhead_position": [0.0, 150.0, 0.0]
                }
            ]
        }"#;

        let entries = resolve_manifest_entries(json, "wc3/units").expect("manifest resolves");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].rawcode, u32::from_be_bytes(*b"h016"));
        assert_eq!(entries[0].scale, 1.2);
        assert_eq!(entries[0].overhead_height, Some(180.0));
        assert_eq!(
            entries[0].asset_path,
            "wc3/units/models/units__human__gryphonrider__gryphonrider.gltf"
        );
        assert_eq!(entries[1].rawcode, u32::from_be_bytes(*b"hfoo"));
    }

    #[test]
    fn unit_manifest_preserves_model_local_particles_and_ribbons() {
        let json = r#"{
            "schema_version": 5,
            "units": [{
                "rawcode": "hfoo",
                "scale": 1.0,
                "gltf": "models/footman.gltf"
            }],
            "models": [{
                "gltf": "models/footman.gltf",
                "overhead_position": null,
                "particle_emitters": [{
                    "position": [1.0, 2.0, 3.0],
                    "filter_mode": 2,
                    "speed": 10.0,
                    "variation": 1.0,
                    "latitude": 0.5,
                    "gravity": -2.0,
                    "lifespan": 0.75,
                    "emission_rate": 12.0,
                    "rows": 2,
                    "columns": 4,
                    "segment_colors": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                    "segment_alpha": [255, 128, 0],
                    "segment_scaling": [8.0, 12.0, 4.0],
                    "texture": "textures/smoke.png",
                    "squirt": false,
                    "ambient_enabled": true,
                    "active_sequences": ["Stand"]
                }],
                "ribbon_emitters": [{
                    "position": [0.0, 1.0, 0.0],
                    "height_above": 4.0,
                    "height_below": 2.0,
                    "alpha": 0.5,
                    "color": [0.25, 0.5, 1.0],
                    "lifespan": 1.5,
                    "emission_rate": 20,
                    "rows": 1,
                    "columns": 1,
                    "filter_mode": "Additive",
                    "texture": "textures/ribbon.png",
                    "gravity": 0.0
                }]
            }]
        }"#;

        let entries = resolve_manifest_entries(json, "wc3/units").expect("manifest resolves");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].particle_emitters.len(), 1);
        assert_eq!(entries[0].particle_emitters[0].active_sequences, ["Stand"]);
        assert_eq!(entries[0].ribbon_emitters.len(), 1);
        assert_eq!(entries[0].ribbon_emitters[0].emission_rate, 20);
    }

    #[test]
    fn rejects_stale_unit_asset_manifest_schema() {
        let json = r#"{
            "schema_version": 3,
            "units": [],
            "models": []
        }"#;
        let error = resolve_manifest_entries(json, "wc3/units")
            .expect_err("stale generated packs must be regenerated");
        assert!(error.contains("unsupported unit asset manifest schema 3"));
    }

    #[test]
    fn rejects_unsafe_model_paths() {
        for path in ["../escape.gltf", "..\\\\escape.gltf"] {
            let json = format!(
                r#"{{
                    "schema_version": 5,
                    "units": [{{"rawcode": "hfoo", "scale": 1.0, "gltf": {path:?}}}],
                    "models": []
                }}"#
            );
            let error =
                resolve_manifest_entries(&json, "wc3/units").expect_err("path must be rejected");
            assert!(error.contains("safe relative asset path"));
        }
    }

    #[test]
    fn animation_roles_prefer_normal_locomotion_and_attack_clips() {
        assert_eq!(animation_score("Stand - 1", AnimationRole::Stand), Some(0));
        assert_eq!(animation_score("Stand Defend", AnimationRole::Stand), None);
        assert_eq!(animation_score("Walk", AnimationRole::Walk), Some(0));
        assert_eq!(animation_score("Walk - 1", AnimationRole::Walk), Some(1));
        assert_eq!(animation_score("Walk Defend", AnimationRole::Walk), None);
        assert_eq!(
            animation_score("Attack - 1", AnimationRole::Attack),
            Some(0)
        );
        assert_eq!(
            animation_score("Attack Defend", AnimationRole::Attack),
            None
        );
        assert_eq!(animation_score("Spell", AnimationRole::Cast), Some(0));
        assert_eq!(animation_score("Spell - 1", AnimationRole::Cast), Some(1));
        assert_eq!(animation_score("Death", AnimationRole::Death), Some(0));
        assert_eq!(
            animation_score("Decay Flesh", AnimationRole::DecayFlesh),
            Some(0)
        );
        assert_eq!(
            animation_score("Decay Bone", AnimationRole::DecayBone),
            Some(0)
        );
    }

    #[test]
    fn rejects_invalid_rawcodes_and_scales() {
        let bad_rawcode = r#"{
            "schema_version": 5,
            "units": [{"rawcode": "foo", "scale": 1.0, "gltf": "models/foo.gltf"}],
            "models": []
        }"#;
        assert!(
            resolve_manifest_entries(bad_rawcode, "wc3/units")
                .expect_err("rawcode must be rejected")
                .contains("exactly four bytes")
        );

        let bad_scale = r#"{
            "schema_version": 5,
            "units": [{"rawcode": "hfoo", "scale": 0.0, "gltf": "models/foo.gltf"}],
            "models": []
        }"#;
        assert!(
            resolve_manifest_entries(bad_scale, "wc3/units")
                .expect_err("scale must be rejected")
                .contains("invalid model scale")
        );
    }
}
