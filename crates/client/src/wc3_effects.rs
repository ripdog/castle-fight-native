use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Component, Path},
};

use bevy::prelude::*;
use serde::Deserialize;

use crate::terrain::client_asset_root;

const EFFECT_MANIFEST: &str = "wc3/effects/manifest.json";
const EFFECT_ASSET_PREFIX: &str = "wc3/effects";
const MAX_PARTICLES_PER_EMITTER_PER_FRAME: u32 = 12;

#[derive(Resource, Default)]
pub struct Wc3VisualSet {
    projectile_by_rawcode: BTreeMap<u32, Wc3VisualModel>,
    ability_by_rawcode: BTreeMap<u32, Vec<Wc3AbilityVisual>>,
    chain_lightning_abilities: BTreeSet<u32>,
    stun: Option<Wc3VisualModel>,
}

#[derive(Clone)]
pub struct Wc3VisualModel {
    pub scene: Handle<WorldAsset>,
    pub emitters: Vec<Wc3ParticleEmitter>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wc3AbilityVisualAnchor {
    Source,
    Target,
}

#[derive(Clone)]
pub struct Wc3AbilityVisual {
    pub model: Wc3VisualModel,
    pub anchor: Wc3AbilityVisualAnchor,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Wc3ParticleEmitter {
    pub position: [f32; 3],
    pub speed: f32,
    pub variation: f32,
    pub latitude: f32,
    pub gravity: f32,
    pub lifespan: f32,
    pub emission_rate: f32,
    pub segment_colors: [[f32; 3]; 3],
    pub segment_alpha: [u8; 3],
    pub segment_scaling: [f32; 3],
    pub texture: Option<String>,
    pub squirt: bool,
}

#[derive(Debug, Deserialize)]
struct VisualManifest {
    schema_version: u32,
    assets: Vec<VisualBinding>,
    chain_lightning_abilities: Vec<String>,
    stun: Option<VisualBinding>,
    models: Vec<ModelManifest>,
}

#[derive(Debug, Clone, Deserialize)]
struct VisualBinding {
    owner_kind: String,
    owner_rawcode: String,
    role: String,
    gltf: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelManifest {
    gltf: String,
    #[serde(default)]
    particle_emitters: Vec<Wc3ParticleEmitter>,
}

#[derive(Component)]
pub struct Wc3EmitterSource {
    emitters: Vec<EmitterRuntime>,
}

#[derive(Clone)]
struct EmitterRuntime {
    spec: Wc3ParticleEmitter,
    accumulator: f32,
    burst_pending: bool,
    sequence: u32,
}

#[derive(Component)]
pub struct Wc3Particle {
    velocity: Vec3,
    gravity: f32,
    age: f32,
    lifespan: f32,
    scales: [f32; 3],
}

#[derive(Resource)]
pub struct Wc3ParticleAssets {
    quad: Handle<Mesh>,
    materials: HashMap<String, Handle<StandardMaterial>>,
}

impl Wc3VisualSet {
    #[must_use]
    pub fn load_default(asset_server: &AssetServer) -> Self {
        let asset_root = client_asset_root();
        let manifest_path = asset_root.join(EFFECT_MANIFEST);
        if !manifest_path.is_file() {
            return Self::default();
        }
        match load_manifest(&manifest_path, asset_server) {
            Ok(set) => {
                println!(
                    "Loaded {} WC3 projectile visual(s), {} ability visual(s), and {} chain-lightning id(s)",
                    set.projectile_by_rawcode.len(),
                    set.ability_by_rawcode.len(),
                    set.chain_lightning_abilities.len()
                );
                set
            }
            Err(error) => {
                eprintln!("warning: ignoring generated WC3 effect assets: {error}");
                Self::default()
            }
        }
    }

    #[must_use]
    pub fn projectile(&self, rawcode: u32) -> Option<&Wc3VisualModel> {
        self.projectile_by_rawcode.get(&rawcode)
    }

    #[must_use]
    pub fn ability(&self, rawcode: u32) -> &[Wc3AbilityVisual] {
        self.ability_by_rawcode
            .get(&rawcode)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    #[must_use]
    pub fn is_chain_lightning(&self, rawcode: u32) -> bool {
        self.chain_lightning_abilities.contains(&rawcode)
    }

    #[must_use]
    pub fn stun(&self) -> Option<&Wc3VisualModel> {
        self.stun.as_ref()
    }
}

impl Wc3EmitterSource {
    #[must_use]
    pub fn new(emitters: &[Wc3ParticleEmitter]) -> Self {
        Self {
            emitters: emitters
                .iter()
                .cloned()
                .map(|spec| EmitterRuntime {
                    burst_pending: spec.squirt,
                    spec,
                    accumulator: 0.0,
                    sequence: 0,
                })
                .collect(),
        }
    }
}

impl Wc3ParticleAssets {
    pub fn new(meshes: &mut Assets<Mesh>) -> Self {
        let quad = meshes.add(Rectangle::new(1.0, 1.0));
        Self {
            quad,
            materials: HashMap::new(),
        }
    }

    fn material(
        &mut self,
        emitter: &Wc3ParticleEmitter,
        asset_server: &AssetServer,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        let color = emitter.segment_colors[0];
        let alpha = f32::from(emitter.segment_alpha[0]) / 255.0;
        let texture_key = emitter.texture.as_deref().unwrap_or("<none>");
        let key = format!(
            "{texture_key}|{:.3}|{:.3}|{:.3}|{alpha:.3}",
            color[0], color[1], color[2]
        );
        if let Some(handle) = self.materials.get(&key) {
            return handle.clone();
        }
        let base_color = Color::srgba(color[0], color[1], color[2], alpha.max(0.05));
        let base_color_texture = emitter
            .texture
            .as_ref()
            .map(|texture| asset_server.load(format!("{EFFECT_ASSET_PREFIX}/{texture}")));
        let handle = materials.add(StandardMaterial {
            base_color,
            base_color_texture,
            emissive: LinearRgba::new(color[0], color[1], color[2], 1.0),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            double_sided: true,
            ..default()
        });
        self.materials.insert(key, handle.clone());
        handle
    }
}

pub fn emit_wc3_particles(
    mut commands: Commands,
    time: Res<Time>,
    asset_server: Res<AssetServer>,
    mut particle_assets: ResMut<Wc3ParticleAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sources: Query<(Entity, &GlobalTransform, &mut Wc3EmitterSource)>,
) {
    let dt = time.delta_secs().min(0.1);
    for (entity, transform, mut source) in &mut sources {
        let root = transform.compute_transform();
        for (emitter_index, emitter) in source.emitters.iter_mut().enumerate() {
            let mut count = if emitter.burst_pending {
                emitter.burst_pending = false;
                8
            } else {
                emitter.accumulator += emitter.spec.emission_rate.clamp(0.0, 240.0) * dt;
                let count = emitter.accumulator.floor() as u32;
                emitter.accumulator -= count as f32;
                count
            };
            count = count.min(MAX_PARTICLES_PER_EMITTER_PER_FRAME);
            if count == 0 || emitter.spec.lifespan <= 0.0 {
                continue;
            }
            let material = particle_assets.material(&emitter.spec, &asset_server, &mut materials);
            let origin = transform.transform_point(Vec3::from_array(emitter.spec.position));
            for _ in 0..count {
                let sequence = emitter.sequence;
                emitter.sequence = emitter.sequence.wrapping_add(1);
                let velocity = particle_velocity(
                    root.rotation,
                    entity,
                    emitter_index as u32,
                    sequence,
                    &emitter.spec,
                );
                commands.spawn((
                    Mesh3d(particle_assets.quad.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(origin)
                        .with_scale(Vec3::splat(emitter.spec.segment_scaling[0].max(0.01))),
                    Wc3Particle {
                        velocity,
                        gravity: emitter.spec.gravity,
                        age: 0.0,
                        lifespan: emitter.spec.lifespan.max(0.01),
                        scales: emitter.spec.segment_scaling,
                    },
                ));
            }
        }
    }
}

pub fn update_wc3_particles(
    mut commands: Commands,
    time: Res<Time>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut particles: Query<(Entity, &mut Transform, &mut Wc3Particle)>,
) {
    let dt = time.delta_secs().min(0.1);
    let camera_rotation = cameras.iter().next().map(GlobalTransform::rotation);
    for (entity, mut transform, mut particle) in &mut particles {
        particle.age += dt;
        if particle.age >= particle.lifespan {
            commands.entity(entity).despawn();
            continue;
        }
        particle.velocity.y -= particle.gravity * dt;
        transform.translation += particle.velocity * dt;
        if let Some(rotation) = camera_rotation {
            transform.rotation = rotation;
        }
        let t = particle.age / particle.lifespan;
        let scale = if t < 0.5 {
            particle.scales[0] + (particle.scales[1] - particle.scales[0]) * (t * 2.0)
        } else {
            particle.scales[1] + (particle.scales[2] - particle.scales[1]) * ((t - 0.5) * 2.0)
        };
        transform.scale = Vec3::splat(scale.max(0.01));
    }
}

fn particle_velocity(
    rotation: Quat,
    entity: Entity,
    emitter_index: u32,
    sequence: u32,
    emitter: &Wc3ParticleEmitter,
) -> Vec3 {
    let seed = (entity.to_bits() as u32).wrapping_mul(0x9e37_79b9)
        ^ emitter_index.wrapping_mul(0x85eb_ca6b)
        ^ sequence.wrapping_mul(0xc2b2_ae35);
    let a = hash_unit(seed);
    let b = hash_unit(seed ^ 0xa511_e9b3);
    let latitude = emitter
        .latitude
        .to_radians()
        .clamp(0.0, std::f32::consts::PI);
    let cone = latitude * a.sqrt();
    let azimuth = std::f32::consts::TAU * b;
    // Warcraft emitters use Z-up model space. The extractor converts model geometry with
    // (x, y, z) -> (x, z, -y), so apply the same basis change to emitted velocity vectors.
    // In particular, a zero-latitude emitter points along WC3 +Z, which is Bevy +Y.
    let wc3_direction = Vec3::new(
        cone.sin() * azimuth.cos(),
        cone.sin() * azimuth.sin(),
        cone.cos(),
    );
    let local_direction = wc3_direction_to_bevy(wc3_direction);
    let variation = 1.0 + (hash_unit(seed ^ 0x63d8_3595) * 2.0 - 1.0) * emitter.variation;
    rotation * local_direction * emitter.speed * variation.max(0.0)
}

fn wc3_direction_to_bevy(direction: Vec3) -> Vec3 {
    Vec3::new(direction.x, direction.z, -direction.y)
}

fn hash_unit(mut value: u32) -> f32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^= value >> 16;
    value as f32 / u32::MAX as f32
}

fn load_manifest(path: &Path, asset_server: &AssetServer) -> Result<Wc3VisualSet, String> {
    let json = fs::read_to_string(path)
        .map_err(|error| format!("failed reading {}: {error}", path.display()))?;
    let manifest: VisualManifest =
        serde_json::from_str(&json).map_err(|error| format!("invalid visual manifest: {error}"))?;
    if manifest.schema_version != 1 {
        return Err(format!(
            "unsupported visual asset manifest schema {}",
            manifest.schema_version
        ));
    }

    let model_by_gltf: BTreeMap<_, _> = manifest
        .models
        .into_iter()
        .map(|model| (model.gltf.clone(), model))
        .collect();
    let mut projectile_by_rawcode = BTreeMap::new();
    let mut ability_by_rawcode = BTreeMap::<u32, Vec<Wc3AbilityVisual>>::new();
    for binding in &manifest.assets {
        let Some(gltf) = &binding.gltf else {
            continue;
        };
        let Some(model) = model_by_gltf.get(gltf) else {
            continue;
        };
        let visual = resolve_visual_model(gltf, model, asset_server)?;
        match (binding.owner_kind.as_str(), binding.role.as_str()) {
            ("units", "attack1_projectile") => {
                projectile_by_rawcode.insert(parse_rawcode(&binding.owner_rawcode)?, visual);
            }
            ("abilities", role @ ("target" | "effect" | "special" | "caster")) => {
                ability_by_rawcode
                    .entry(parse_rawcode(&binding.owner_rawcode)?)
                    .or_default()
                    .push(Wc3AbilityVisual {
                        model: visual,
                        anchor: ability_visual_anchor(role),
                    });
            }
            // Missile art needs an authoritative travel interval/path. Do not pin a missile
            // model to either endpoint merely because the object data references one.
            ("abilities", "missile") => {}
            _ => {}
        }
    }

    let chain_lightning_abilities = manifest
        .chain_lightning_abilities
        .iter()
        .map(|rawcode| parse_rawcode(rawcode))
        .collect::<Result<_, _>>()?;
    let stun = manifest
        .stun
        .as_ref()
        .and_then(|binding| binding.gltf.as_ref())
        .and_then(|gltf| model_by_gltf.get(gltf).map(|model| (gltf, model)))
        .map(|(gltf, model)| resolve_visual_model(gltf, model, asset_server))
        .transpose()?;

    Ok(Wc3VisualSet {
        projectile_by_rawcode,
        ability_by_rawcode,
        chain_lightning_abilities,
        stun,
    })
}

fn resolve_visual_model(
    gltf: &str,
    model: &ModelManifest,
    asset_server: &AssetServer,
) -> Result<Wc3VisualModel, String> {
    let gltf = gltf.replace('\\', "/");
    validate_relative_asset_path(&gltf)?;
    let asset_path = format!("{EFFECT_ASSET_PREFIX}/{gltf}");
    Ok(Wc3VisualModel {
        scene: asset_server.load(GltfAssetLabel::Scene(0).from_asset(asset_path)),
        emitters: model.particle_emitters.clone(),
    })
}

fn ability_visual_anchor(role: &str) -> Wc3AbilityVisualAnchor {
    match role {
        "caster" => Wc3AbilityVisualAnchor::Source,
        "target" | "effect" | "special" => Wc3AbilityVisualAnchor::Target,
        _ => unreachable!("only supported stationary ability visual roles are classified"),
    }
}

fn parse_rawcode(rawcode: &str) -> Result<u32, String> {
    let bytes: [u8; 4] = rawcode
        .as_bytes()
        .try_into()
        .map_err(|_| format!("visual rawcode {rawcode:?} is not exactly four bytes"))?;
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
            "visual model path {} is not a safe relative asset path",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rawcodes_are_four_bytes() {
        assert_eq!(parse_rawcode("h016").unwrap(), u32::from_be_bytes(*b"h016"));
        assert!(parse_rawcode("bad").is_err());
    }

    #[test]
    fn effect_paths_must_stay_inside_asset_root() {
        assert!(validate_relative_asset_path("models/foo.gltf").is_ok());
        assert!(validate_relative_asset_path("../foo.gltf").is_err());
    }

    #[test]
    fn emitter_directions_use_the_same_wc3_to_bevy_basis_as_models() {
        assert_eq!(wc3_direction_to_bevy(Vec3::Z), Vec3::Y);
        assert_eq!(wc3_direction_to_bevy(Vec3::Y), Vec3::NEG_Z);
        assert_eq!(wc3_direction_to_bevy(Vec3::X), Vec3::X);
    }

    #[test]
    fn ability_art_roles_keep_wc3_attachment_side() {
        assert_eq!(
            ability_visual_anchor("caster"),
            Wc3AbilityVisualAnchor::Source
        );
        for role in ["target", "effect", "special"] {
            assert_eq!(ability_visual_anchor(role), Wc3AbilityVisualAnchor::Target);
        }
    }
}
