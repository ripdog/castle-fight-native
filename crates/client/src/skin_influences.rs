//! Profiling-only skinning variants for primitives with exactly zero trailing weights.
//!
//! Classification reads the extracted GPU-upload snapshot. Virtual layout attributes
//! encode its result without changing mesh bytes, palettes, bounds or skin identity.
//! Supported material callbacks keep the engine specialization and change only its
//! vertex shader and weight input width. The distinct input layouts also prevent
//! Bevy's second-level vertex-layout cache from aliasing different skin variants.

use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet},
    hash::Hash,
    sync::Arc,
    time::{Duration, Instant},
};

use bevy::{
    asset::{AssetId, UntypedAssetId, uuid_handle},
    material::{key::ErasedMaterialPipelineKey, specialize::UserSpecializeFn},
    mesh::{
        MeshVertexAttribute, MeshVertexBufferLayout, MeshVertexBufferLayoutRef,
        MeshVertexBufferLayouts, VertexAttributeValues,
    },
    pbr::{
        Material, MaterialPipeline, MaterialPipelineKey, PreparedMaterial, RenderMaterialInstances,
        RenderMeshInstances, SpecializedMaterialPipelineCache,
    },
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        camera::DirtySpecializations,
        erased_render_asset::ErasedRenderAssets,
        mesh::RenderMesh,
        render_asset::{AssetExtractionSystems, ExtractedAssets, RenderAssets},
        render_resource::{
            PipelineCache, RenderPipelineDescriptor, SpecializedMeshPipelineError, VertexFormat,
        },
    },
    shader::{ShaderDefVal, ShaderImport, Source},
};

use crate::{
    render_audit::RenderExperiment,
    wc3_effects::{Wc3AnimatedAlphaMaterial, Wc3TeamColorMaterial},
};

const SKIN_LIBRARY: Handle<Shader> = uuid_handle!("9874d320-83ae-4b46-986f-5a0100021001");
const SKIN_MESH: Handle<Shader> = uuid_handle!("9874d320-83ae-4b46-986f-5a0100021002");
const SKIN_PREPASS: Handle<Shader> = uuid_handle!("9874d320-83ae-4b46-986f-5a0100021003");
const SKIN_IMPORT: &str = "castle_fight::skin_influences";
const MARKERS: [MeshVertexAttribute; 3] = [
    MeshVertexAttribute::new("CF_Skin1", 0x9874_d320_83ae_0001, VertexFormat::Float32x4),
    MeshVertexAttribute::new("CF_Skin2", 0x9874_d320_83ae_0002, VertexFormat::Float32x4),
    MeshVertexAttribute::new("CF_Skin3", 0x9874_d320_83ae_0003, VertexFormat::Float32x4),
];

pub(crate) struct SkinInfluencePlugin;

impl Plugin for SkinInfluencePlugin {
    fn build(&self, app: &mut App) {
        if *app.world().resource::<RenderExperiment>() == RenderExperiment::SkinInfluences {
            app.init_resource::<SkinShaderState>()
                .add_systems(PostUpdate, prepare_skin_shaders);
        }
    }

    fn finish(&self, app: &mut App) {
        let enabled =
            *app.world().resource::<RenderExperiment>() == RenderExperiment::SkinInfluences;
        let Some(render) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render.init_resource::<SkinInfluenceStats>();
        if enabled {
            render
                .init_resource::<SkinShaderReady>()
                .init_resource::<SkinMeshMetadata>()
                .add_systems(
                    ExtractSchedule,
                    (extract_shader_ready, classify_extracted_meshes)
                        .chain()
                        .after(AssetExtractionSystems),
                )
                .add_systems(
                    Render,
                    (
                        apply_skin_layouts,
                        wrap_material_specialization,
                        invalidate_skin_variants,
                    )
                        .chain()
                        .after(RenderSystems::PrepareAssets)
                        .before(RenderSystems::Specialize),
                )
                .add_systems(
                    Render,
                    sample_skin_pipelines
                        .after(RenderSystems::Queue)
                        .before(RenderSystems::Render),
                );
        }
    }
}

#[derive(Resource, Default)]
enum SkinShaderState {
    #[default]
    Loading,
    Ready,
    Unsupported,
}

#[derive(Resource, Default)]
struct SkinShaderReady(bool);

fn skin_library_source(source: &str) -> Option<String> {
    let header = "#define_import_path bevy_pbr::skinning";
    if source.matches(header).count() != 1 {
        return None;
    }
    let mut source = source.replace(header, &format!("#define_import_path {SKIN_IMPORT}"));
    for matrices in ["joint_matrices", "prev_joint_matrices"] {
        for uniform in [true, false] {
            let index = |slot: &str| {
                if uniform {
                    format!("{matrices}.data[indexes.{slot}]")
                } else {
                    format!("{matrices}[skin_index + indexes.{slot}]")
                }
            };
            let terms =
                ["x", "y", "z", "w"].map(|slot| format!("weights.{slot} * {}", index(slot)));
            let original = format!(
                "return {}\n        + {}\n        + {}\n        + {};",
                terms[0], terms[1], terms[2], terms[3]
            );
            if source.matches(&original).count() != 1 {
                return None;
            }
            let specialized = format!(
                "#ifdef CF_SKIN_1\n    return {};\n#else\n#ifdef CF_SKIN_2\n    return {} + {};\n#else\n#ifdef CF_SKIN_3\n    return {} + {} + {};\n#else\n    {original}\n#endif\n#endif\n#endif",
                terms[0], terms[0], terms[1], terms[0], terms[1], terms[2],
            );
            source = source.replace(&original, &specialized);
        }
    }
    Some(source)
}

fn skin_vertex_source(source: &str, prepass: bool) -> Option<String> {
    if source.matches("    skinning,\n").count() != 1
        || !source.contains("skinning::skin_model(")
        || !source.contains("skinning::skin_normals(")
        || (prepass && !source.contains("skinning::skin_prev_model("))
    {
        return None;
    }
    Some(format!(
        "#import {SKIN_IMPORT} as skin_influences\n{}",
        source
            .replace("    skinning,\n", "")
            .replace("skinning::", "skin_influences::")
    ))
}

fn prepare_skin_shaders(mut state: ResMut<SkinShaderState>, mut shaders: ResMut<Assets<Shader>>) {
    if !matches!(*state, SkinShaderState::Loading) {
        return;
    }
    let find = |predicate: &dyn Fn(&Shader) -> bool| {
        shaders
            .iter()
            .find(|(_, shader)| predicate(shader))
            .map(|(_, shader)| shader)
    };
    let Some(library) =
        find(&|shader| shader.import_path == ShaderImport::Custom("bevy_pbr::skinning".into()))
    else {
        return;
    };
    let Some(mesh) = find(&|shader| shader.path.ends_with("bevy_pbr/render/mesh.wgsl")) else {
        return;
    };
    let Some(prepass) = find(&|shader| shader.path.ends_with("bevy_pbr/prepass/prepass.wgsl"))
    else {
        return;
    };
    let sources = [library, mesh, prepass].map(|shader| match &shader.source {
        Source::Wgsl(source) => Some(source.as_ref()),
        _ => None,
    });
    let generated = sources[0]
        .and_then(skin_library_source)
        .zip(sources[1].and_then(|source| skin_vertex_source(source, false)))
        .zip(sources[2].and_then(|source| skin_vertex_source(source, true)));
    let Some(((library_source, mesh_source), prepass_source)) = generated else {
        warn!(
            "skin influences: unfamiliar engine shader source; retaining four-influence skinning"
        );
        *state = SkinShaderState::Unsupported;
        return;
    };
    let generated = [
        (SKIN_LIBRARY, library_source, library),
        (SKIN_MESH, mesh_source, mesh),
        (SKIN_PREPASS, prepass_source, prepass),
    ]
    .map(|(handle, source, original)| {
        let mut shader = Shader::from_wgsl(
            source,
            format!("generated/skin_influences/{}", original.path),
        );
        shader.shader_defs.clone_from(&original.shader_defs);
        shader
            .additional_imports
            .clone_from(&original.additional_imports);
        shader
            .file_dependencies
            .clone_from(&original.file_dependencies);
        shader.validate_shader = original.validate_shader.clone();
        (handle, shader)
    });
    for (handle, shader) in generated {
        shaders.insert(handle.id(), shader).unwrap();
    }
    *state = SkinShaderState::Ready;
}

fn extract_shader_ready(state: Extract<Res<SkinShaderState>>, mut ready: ResMut<SkinShaderReady>) {
    ready.0 = matches!(**state, SkinShaderState::Ready);
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum InfluenceCount {
    One,
    Two,
    Three,
}

impl InfluenceCount {
    const fn index(self) -> usize {
        match self {
            Self::One => 0,
            Self::Two => 1,
            Self::Three => 2,
        }
    }
    const fn format(self) -> VertexFormat {
        match self {
            Self::One => VertexFormat::Float32,
            Self::Two => VertexFormat::Float32x2,
            Self::Three => VertexFormat::Float32x3,
        }
    }
}

fn classify_influences(mesh: &Mesh) -> Option<InfluenceCount> {
    // Unsupported attributes or missing CPU data retain the ordinary four-slot path.
    let weights = mesh.try_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT).ok()?;
    let VertexAttributeValues::Float32x4(weights) = weights else {
        return None;
    };
    let indices = mesh.try_attribute(Mesh::ATTRIBUTE_JOINT_INDEX).ok()?;
    if weights.is_empty() || indices.len() != weights.len() {
        return None;
    }
    let mut highest = 0;
    for weights in weights {
        for (slot, weight) in weights.iter().enumerate() {
            if !weight.is_finite() || *weight < 0.0 {
                return None;
            }
            if *weight != 0.0 {
                highest = highest.max(slot + 1);
            }
        }
    }
    match highest {
        1 => Some(InfluenceCount::One),
        2 => Some(InfluenceCount::Two),
        3 => Some(InfluenceCount::Three),
        _ => None,
    }
}

#[derive(Resource, Default)]
struct SkinMeshMetadata {
    pending: HashMap<AssetId<Mesh>, InfluenceCount>,
    active: HashMap<AssetId<Mesh>, (InfluenceCount, u32)>,
    layouts: HashMap<(MeshVertexBufferLayoutRef, InfluenceCount), MeshVertexBufferLayoutRef>,
    changed_meshes: HashSet<AssetId<Mesh>>,
    changed_materials: HashSet<UntypedAssetId>,
}

fn classify_extracted_meshes(
    extracted: Res<ExtractedAssets<RenderMesh>>,
    mut metadata: ResMut<SkinMeshMetadata>,
) {
    for id in &extracted.removed {
        metadata.pending.remove(id);
        metadata.active.remove(id);
    }
    for (id, mesh) in &extracted.extracted {
        // Bevy removes the old RenderMesh before preparing the extracted replacement,
        // even when the new upload is deferred. Classification belongs to this snapshot.
        metadata.pending.remove(id);
        metadata.active.remove(id);
        if let Some(count) = classify_influences(mesh) {
            metadata.pending.insert(*id, count);
        }
    }
}

fn apply_skin_layouts(
    ready: Res<SkinShaderReady>,
    mut metadata: ResMut<SkinMeshMetadata>,
    mut meshes: ResMut<RenderAssets<RenderMesh>>,
    mut layouts: ResMut<MeshVertexBufferLayouts>,
    mut stats: ResMut<SkinInfluenceStats>,
) {
    if !ready.0 {
        return;
    }
    let metadata = &mut *metadata;
    metadata.pending.retain(|id, count| {
        let Some(mesh) = meshes.get_mut(*id) else {
            return true;
        };
        let original = &mesh.layout;
        let Some(index) = original
            .0
            .attribute_ids()
            .iter()
            .position(|id| *id == Mesh::ATTRIBUTE_JOINT_WEIGHT.id)
        else {
            return false;
        };
        let weight = original.0.layout().attributes[index];
        if weight.format != VertexFormat::Float32x4 {
            return false;
        }
        let tagged = metadata
            .layouts
            .entry((original.clone(), *count))
            .or_insert_with(|| {
                let mut ids = original.0.attribute_ids().to_vec();
                let mut vertex_layout = original.0.layout().clone();
                // This metadata-only attribute aliases existing bytes. Engine get_layout
                // only requests real attributes, so it never adds a GPU input or upload.
                ids.push(MARKERS[count.index()].id);
                vertex_layout.attributes.push(weight);
                layouts.insert(MeshVertexBufferLayout::new(ids, vertex_layout))
            });
        mesh.layout = tagged.clone();
        metadata.changed_meshes.insert(*id);
        metadata.active.insert(*id, (*count, mesh.vertex_count));
        false
    });
    stats.meshes = [0; 3];
    stats.vertices = [0; 3];
    for (count, vertices) in metadata.active.values() {
        stats.meshes[count.index()] += 1;
        stats.vertices[count.index()] += u64::from(*vertices);
    }
    stats.pending_meshes = metadata.pending.len();
}

fn specialize_skin<M: Material>(
    pipeline: &dyn Any,
    descriptor: &mut RenderPipelineDescriptor,
    layout: &MeshVertexBufferLayoutRef,
    key: ErasedMaterialPipelineKey,
) -> Result<(), SpecializedMeshPipelineError>
where
    M::Data: Clone + Hash,
{
    // Same typed adapter as Bevy's user_specialize: preserve every original material decision.
    let pipeline = pipeline
        .downcast_ref::<MaterialPipeline>()
        .expect("PBR material pipeline");
    M::specialize(
        pipeline,
        descriptor,
        layout,
        MaterialPipelineKey {
            mesh_key: key.mesh_key.downcast(),
            bind_group_data: key.material_key.to_key(),
        },
    )?;
    let Some(index) = MARKERS.iter().position(|marker| layout.0.contains(*marker)) else {
        return Ok(());
    };
    let count = [
        InfluenceCount::One,
        InfluenceCount::Two,
        InfluenceCount::Three,
    ][index];
    let (shader, location) = if descriptor.vertex.shader == pipeline.mesh_pipeline.shader {
        (SKIN_MESH, 7)
    } else if descriptor
        .vertex
        .shader
        .path()
        .is_some_and(|path| path.path().ends_with("bevy_pbr/prepass/prepass.wgsl"))
    {
        (SKIN_PREPASS, 6)
    } else {
        return Ok(());
    }; // Custom vertex shaders keep all four weights.
    let Some(weight) = descriptor.vertex.buffers.first_mut().and_then(|buffer| {
        buffer.attributes.iter_mut().find(|attribute| {
            attribute.shader_location == location && attribute.format == VertexFormat::Float32x4
        })
    }) else {
        return Ok(());
    };
    weight.format = count.format();
    // Missing input components are driver-filled, but the specialized skin functions
    // never read them. Stride/offset remain those of the complete four-weight buffer.
    descriptor.vertex.shader = shader;
    descriptor
        .vertex
        .shader_defs
        .push(ShaderDefVal::Bool(format!("CF_SKIN_{}", index + 1), true));
    Ok(())
}

fn wrap_material_specialization(
    ready: Res<SkinShaderReady>,
    mut materials: ResMut<ErasedRenderAssets<PreparedMaterial>>,
    mut metadata: ResMut<SkinMeshMetadata>,
    mut stats: ResMut<SkinInfluenceStats>,
) {
    if !ready.0 {
        return;
    }
    stats.materials = [0; 3];
    stats.shared_material_fallbacks = 0;
    for (id, material) in materials.iter_mut() {
        let (index, callback): (usize, UserSpecializeFn) =
            if id.type_id() == TypeId::of::<StandardMaterial>() {
                (0, specialize_skin::<StandardMaterial>)
            } else if id.type_id() == TypeId::of::<Wc3AnimatedAlphaMaterial>() {
                (1, specialize_skin::<Wc3AnimatedAlphaMaterial>)
            } else if id.type_id() == TypeId::of::<Wc3TeamColorMaterial>() {
                (2, specialize_skin::<Wc3TeamColorMaterial>)
            } else {
                continue;
            };
        if material
            .properties
            .user_specialize
            .is_some_and(|current| std::ptr::fn_addr_eq(current, callback))
        {
            stats.materials[index] += 1;
        } else if let Some(properties) = Arc::get_mut(&mut material.properties) {
            // New PreparedMaterial properties are unique before specialization. Never
            // mutate a shared/cached property set or replace any asset/binding identity.
            properties.user_specialize = Some(callback);
            metadata.changed_materials.insert(id);
            stats.materials[index] += 1;
        } else {
            stats.shared_material_fallbacks += 1;
        }
    }
}

fn invalidate_skin_variants(
    mut metadata: ResMut<SkinMeshMetadata>,
    materials: Res<RenderMaterialInstances>,
    meshes: Res<RenderMeshInstances>,
    mut dirty: ResMut<DirtySpecializations>,
) {
    if metadata.changed_meshes.is_empty() && metadata.changed_materials.is_empty() {
        return;
    }
    for (entity, material) in &materials.instances {
        if metadata.changed_materials.contains(&material.asset_id)
            || meshes
                .mesh_asset_id(*entity)
                .is_some_and(|id| metadata.changed_meshes.contains(&id))
        {
            dirty.changed_renderables.insert(*entity);
        }
    }
    metadata.changed_meshes.clear();
    metadata.changed_materials.clear();
}

fn sample_skin_pipelines(
    cache: Res<SpecializedMaterialPipelineCache>,
    pipelines: Res<PipelineCache>,
    mut stats: ResMut<SkinInfluenceStats>,
    mut last_sample: Local<Option<Instant>>,
) {
    let now = Instant::now();
    if last_sample.is_some_and(|last| now.duration_since(last) < Duration::from_secs(1)) {
        return;
    }
    *last_sample = Some(now);
    stats.cached_instances = [0; 3];
    stats.compiled_pipelines = [0; 3];
    let mut counted = HashSet::new();
    for view in cache.values() {
        for id in view.values() {
            // Queue may have just allocated this ID in new_pipelines. Descriptor
            // lookup indexes the processed cache directly and panics for such IDs.
            // The safe readiness lookup also excludes compiling/failed pipelines.
            if pipelines.get_render_pipeline(*id).is_none() {
                continue;
            }
            let descriptor = pipelines.get_render_pipeline_descriptor(*id);
            if descriptor.vertex.shader != SKIN_MESH {
                continue;
            }
            let Some(index) = descriptor
                .vertex
                .shader_defs
                .iter()
                .find_map(|def| match def {
                    ShaderDefVal::Bool(name, true) => match name.as_str() {
                        "CF_SKIN_1" => Some(0),
                        "CF_SKIN_2" => Some(1),
                        "CF_SKIN_3" => Some(2),
                        _ => None,
                    },
                    _ => None,
                })
            else {
                continue;
            };
            stats.cached_instances[index] += 1;
            if counted.insert(*id) {
                stats.compiled_pipelines[index] += 1;
            }
        }
    }
}

#[derive(Resource, Default, Clone, Copy)]
pub(crate) struct SkinInfluenceStats {
    pub(crate) meshes: [usize; 3],
    pub(crate) vertices: [u64; 3],
    pub(crate) pending_meshes: usize,
    pub(crate) materials: [usize; 3],
    pub(crate) shared_material_fallbacks: usize,
    pub(crate) cached_instances: [usize; 3],
    pub(crate) compiled_pipelines: [usize; 3],
}
