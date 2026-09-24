use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

use bevy::{gltf::Gltf, prelude::*};
use serde::Deserialize;

use crate::terrain::client_asset_root;

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
pub struct UnitAnimationSet {
    pub graph: Handle<AnimationGraph>,
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
}

#[derive(Debug, Clone, PartialEq)]
struct ResolvedUnitAsset {
    rawcode: u32,
    scale: f32,
    overhead_height: Option<f32>,
    asset_path: String,
    tint_rgb: Option<[u8; 3]>,
    attached_visuals: Vec<UnitAttachedVisual>,
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
                .any(|clip| animation_clips.get(clip).is_none())
            {
                continue;
            }

            let mut clips = vec![stand];
            let walk_slot = append_optional_clip(&mut clips, walk);
            let attack_slot = append_optional_clip(&mut clips, attack);
            let defend_stand_slot = append_optional_clip(&mut clips, defend_stand);
            let defend_walk_slot = append_optional_clip(&mut clips, defend_walk);
            let defend_attack_slot = append_optional_clip(&mut clips, defend_attack);
            let cast_slot = append_optional_clip(&mut clips, cast);
            let death_slot = append_optional_clip(&mut clips, death);
            let decay_flesh_slot = append_optional_clip(&mut clips, decay_flesh);
            let decay_bone_slot = append_optional_clip(&mut clips, decay_bone);
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

fn find_animation(gltf: &Gltf, role: AnimationRole) -> Option<Handle<AnimationClip>> {
    gltf.named_animations
        .iter()
        .filter_map(|(name, clip)| animation_score(name, role).map(|score| (score, name, clip)))
        .min_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(right.1)))
        .map(|(_, _, clip)| clip.clone())
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

    let overhead_by_gltf: BTreeMap<_, _> = manifest
        .models
        .into_iter()
        .map(|model| (model.gltf.replace('\\', "/"), model.overhead_position))
        .collect();

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
        let overhead_height = overhead_by_gltf
            .get(&gltf)
            .copied()
            .flatten()
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
