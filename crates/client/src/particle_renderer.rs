//! Dedicated renderer for transparent WC3 particle-emitter quads.
//!
//! Particles stay in a compact main-world resource instead of becoming Mesh3d
//! entities. By default the render world inserts one transparent phase item per
//! emitter (per texture slab and blend mode), positioned at the batch centroid so
//! it still interleaves with alpha-blended model geometry. Inside an item the
//! particles keep exact back-to-front order. Warcraft III likewise draws a
//! particle emitter as a unit instead of depth-sorting individual particles
//! against other emitters; only interpenetrating emitters can look different.
//! `--render-experiment particle-depth-sort` restores one item per particle.
//! After the phase is sorted, adjacent items that share a texture slab and blend
//! mode are collapsed into a single instanced draw without changing that
//! ordering. On hardware with non-uniform texture binding arrays, each slab holds
//! up to 64 particle textures, so texture changes within the run do not split
//! draws.

use bevy::platform::collections::HashMap;
use std::{mem::size_of, num::NonZeroU32, ops::Range};

use bevy::{
    asset::AssetId,
    camera::{
        primitives::{Aabb, Frustum, Sphere},
        visibility::NoFrustumCulling,
    },
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d, TransparentSortingInfo3d},
    core_pipeline::prepass::MotionVectorPrepass,
    ecs::{
        entity::{EntityGeneration, EntityIndex},
        query::ROQueryItem,
        system::{
            SystemParamItem,
            lifetimeless::{Read, SRes},
        },
    },
    mesh::VertexBufferLayout,
    pbr::{
        DrawMaterial, MeshLayouts, MeshPipeline, MeshPipelineViewLayoutKey,
        MeshPipelineViewLayouts, MeshViewBindGroup, RenderMeshInstances, SetMeshBindGroup,
        ViewKeyCache,
    },
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems,
        render_asset::RenderAssets,
        render_phase::{
            AddRenderCommand, DrawFunctionId, DrawFunctions, PhaseItem, PhaseItemExtraIndex,
            RenderCommand, RenderCommandResult, SetItemPipeline, SortedRenderPhase,
            TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            BlendComponent, BlendFactor, BlendOperation, BlendState, BufferUsages,
            ColorTargetState, ColorWrites, CompareFunction, DepthBiasState, DepthStencilState,
            FragmentState, MultisampleState, PipelineCache, PrimitiveState, PrimitiveTopology,
            RawBufferVec, RenderPipelineDescriptor, SamplerBindingType, SamplerId, ShaderStages,
            SpecializedRenderPipeline, SpecializedRenderPipelines, StencilFaceState, StencilState,
            TextureFormat, TextureSampleType, TextureViewId, VertexAttribute, VertexFormat,
            VertexState, VertexStepMode, WgpuFeatures,
            binding_types::{sampler, texture_2d, uniform_buffer},
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        texture::{FallbackImage, GpuImage},
        view::{ExtractedView, Msaa, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
    shader::ShaderDefVal,
};
use bytemuck::{Pod, Zeroable};

use crate::{
    perf_disable::PerfDisable,
    render_audit::RenderExperiment,
    wc3_effects::{
        Wc3AnimatedAlphaMaterial, Wc3BillboardParticleRenderData, Wc3BillboardParticles,
        Wc3ParticleBlendMode, Wc3RibbonTrail, Wc3TeamColorMaterial,
    },
};

const WC3_PARTICLE_SHADER_PATH: &str = "shaders/wc3_particles.wgsl";
const PARTICLE_TEXTURE_SLAB_CANDIDATES: [usize; 2] = [64, 16];

pub(crate) struct Wc3ParticleRenderPlugin;

impl Plugin for Wc3ParticleRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Wc3BillboardParticles>();
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<ExtractedWc3Particles>()
            .init_resource::<Wc3TransparentCoverage>()
            .init_resource::<Wc3ParticleGpuBuffer>()
            .init_resource::<Wc3ParticleTextureBindGroups>()
            .init_resource::<Wc3ParticleQueueStats>()
            .init_resource::<Wc3ParticleBatches>()
            .add_systems(
                RenderStartup,
                (|mut commands: Commands| commands.init_resource::<Wc3ParticlePipeline>())
                    .after(bevy::pbr::MeshPipelineSystems),
            )
            .init_resource::<SpecializedRenderPipelines<Wc3ParticlePipeline>>()
            .add_render_command::<Transparent3d, DrawWc3BillboardParticles>()
            .add_systems(
                ExtractSchedule,
                (extract_wc3_particles, extract_transparent_coverage),
            )
            .add_systems(Render, queue_wc3_particles.in_set(RenderSystems::Queue))
            .add_systems(
                Render,
                batch_and_upload_wc3_particles
                    .after(RenderSystems::PhaseSort)
                    .before(RenderSystems::Prepare),
            )
            .add_systems(
                Render,
                (
                    prepare_wc3_particle_view_bind_groups,
                    prepare_wc3_particle_texture_bind_groups,
                    prepare_wc3_particle_mesh_prefixes,
                )
                    .in_set(RenderSystems::PrepareBindGroups),
            );
    }
}

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct Wc3ParticleInstance {
    position_scale: [f32; 4],
    color: [f32; 4],
    uv_rect: [f32; 4],
    texture_slot: u32,
    _padding: [u32; 3],
}

#[derive(Clone, Copy)]
struct ExtractedWc3Particle {
    instance: Wc3ParticleInstance,
    source: Entity,
    emitter_index: u32,
    slab_entity: Entity,
    slab_index: u32,
    blend_mode: Wc3ParticleBlendMode,
}

#[derive(Resource)]
struct ExtractedWc3Particles {
    particles: Vec<ExtractedWc3Particle>,
    slab_entities: Vec<Entity>,
    slab_textures: Vec<Vec<Option<AssetId<Image>>>>,
    experiment: RenderExperiment,
    batching: ParticleBatching,
    /// Drop zero-alpha and off-frustum particles at queue time.
    cull: bool,
}

impl Default for ExtractedWc3Particles {
    fn default() -> Self {
        Self {
            particles: Vec::new(),
            slab_entities: Vec::new(),
            slab_textures: Vec::new(),
            experiment: RenderExperiment::default(),
            batching: ParticleBatching::Emitter,
            cull: true,
        }
    }
}

fn extract_wc3_particles(
    mut commands: Commands,
    particles: Extract<Res<Wc3BillboardParticles>>,
    experiment: Extract<Res<RenderExperiment>>,
    disable: Extract<Option<Res<PerfDisable>>>,
    pipeline: Res<Wc3ParticlePipeline>,
    mut extracted: ResMut<ExtractedWc3Particles>,
) {
    extracted.particles.clear();
    extracted.slab_textures.clear();
    extracted.experiment = **experiment;
    let disable = disable.as_deref().copied().unwrap_or_default();
    extracted.batching = if disable.emitter_batching {
        ParticleBatching::Individual
    } else {
        ParticleBatching::of(**experiment)
    };
    extracted.cull = !disable.particle_cull;

    let hide_particles = matches!(
        **experiment,
        RenderExperiment::HideParticles | RenderExperiment::HideTransparent
    );
    if hide_particles {
        return;
    }

    let slab_size = pipeline.texture_slab_size();
    let mut texture_slots = HashMap::<Option<AssetId<Image>>, (usize, u32)>::new();
    for Wc3BillboardParticleRenderData {
        source,
        emitter_index,
        texture,
        blend_mode,
        position,
        scale,
        color,
        uv_rect,
    } in particles.render_data()
    {
        let (slab_index, texture_slot) = if let Some(&slot) = texture_slots.get(&texture) {
            slot
        } else {
            let texture_index = texture_slots.len();
            let slab_index = texture_index / slab_size;
            let texture_slot = (texture_index % slab_size) as u32;
            if extracted.slab_textures.len() <= slab_index {
                extracted.slab_textures.push(Vec::with_capacity(slab_size));
            }
            extracted.slab_textures[slab_index].push(texture);
            texture_slots.insert(texture, (slab_index, texture_slot));
            (slab_index, texture_slot)
        };

        while extracted.slab_entities.len() <= slab_index {
            extracted.slab_entities.push(commands.spawn_empty().id());
        }
        let slab_entity = extracted.slab_entities[slab_index];
        extracted.particles.push(ExtractedWc3Particle {
            instance: Wc3ParticleInstance {
                position_scale: [position.x, position.y, position.z, scale],
                color,
                uv_rect,
                texture_slot,
                _padding: [0; 3],
            },
            source,
            emitter_index,
            slab_entity,
            slab_index: slab_index as u32,
            blend_mode,
        });
    }
}

#[derive(Resource)]
struct Wc3ParticleGpuBuffer {
    instances: RawBufferVec<Wc3ParticleInstance>,
    mesh_bindings: HashMap<(Entity, (Entity, MainEntity)), Wc3ParticleMeshBinding>,
}

impl Default for Wc3ParticleGpuBuffer {
    fn default() -> Self {
        let mut instances = RawBufferVec::new(BufferUsages::VERTEX);
        instances.set_label(Some("wc3 particle instances"));
        Self {
            instances,
            mesh_bindings: HashMap::new(),
        }
    }
}

#[derive(Resource, Default)]
struct Wc3ParticleTextureBindGroups(HashMap<Entity, CachedParticleTextureBindGroup>);

#[derive(Clone, Copy, PartialEq, Eq)]
struct ParticleTextureBinding {
    view: TextureViewId,
    sampler: SamplerId,
}

impl From<&GpuImage> for ParticleTextureBinding {
    fn from(image: &GpuImage) -> Self {
        Self {
            view: image.texture_view.id(),
            sampler: image.sampler.id(),
        }
    }
}

struct CachedParticleTextureBindGroup {
    resources: Vec<ParticleTextureBinding>,
    bound_count: usize,
    fallback: ParticleTextureBinding,
    bind_group: BindGroup,
}

fn particle_bindings_match(
    cached: &[ParticleTextureBinding],
    cached_bound_count: usize,
    resources: impl ExactSizeIterator<Item = ParticleTextureBinding>,
    bound_count: usize,
) -> bool {
    cached_bound_count == bound_count
        && cached.len() == resources.len()
        && cached.iter().copied().eq(resources)
}

/// Particle batches queued this frame (all views). A batch is one phase item
/// whose particles were sorted back-to-front among themselves at queue time.
#[derive(Resource, Default)]
pub(crate) struct Wc3ParticleBatches {
    /// Extracted-particle indices, grouped by batch and ordered back to front.
    order: Vec<u32>,
    /// Each batch's slice of `order`; the phase item's extra index selects one.
    ranges: Vec<Range<u32>>,
    /// Instance ranges of merged draws. A sorted phase advances over
    /// `batch_range.len()` *items* per draw, so with multi-particle items the
    /// instance range cannot live in `batch_range`; the draw item's extra index
    /// selects an entry here instead.
    runs: Vec<Range<u32>>,
}

impl Wc3ParticleBatches {
    /// Instances drawn by a merged batched particle item, if it is one.
    pub(crate) fn run_instances(&self, extra_index: &PhaseItemExtraIndex) -> Option<Range<u32>> {
        let PhaseItemExtraIndex::DynamicOffset(run) = extra_index else {
            return None;
        };
        self.runs.get(*run as usize).cloned()
    }
}

/// How queued particles are grouped into phase items.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ParticleBatching {
    /// Every particle is its own phase item, depth-sorted against all other
    /// transparent geometry (adjacent compatible items are merged afterwards).
    Individual,
    /// One phase item per (emitter, texture slab, blend mode), positioned at the
    /// batch centroid. Particles inside a batch keep exact back-to-front order.
    Emitter,
    /// As `Emitter`, but all emitters of one model instance share a batch.
    Source,
}

impl ParticleBatching {
    fn of(experiment: RenderExperiment) -> Self {
        match experiment {
            RenderExperiment::ParticleSourceBatching => Self::Source,
            // These experiments inspect or rewrite individual depth-sorted particle
            // items, so they keep the original per-particle path.
            RenderExperiment::ParticleDepthSort
            | RenderExperiment::ParticleOverlapAudit
            | RenderExperiment::ParticleOverlapBatching
            | RenderExperiment::ParticleSharedMesh
            | RenderExperiment::ParticleSharedMeshPartialBindings => Self::Individual,
            _ => Self::Emitter,
        }
    }
}

#[derive(Clone, Copy)]
struct ParticleBatchEntry {
    /// (source entity bits, emitter index, slab and blend mode)
    key: (u64, u32, u32),
    distance: f32,
    index: u32,
}

/// Orders entries by batch key, then back to front (ascending view distance,
/// matching the transparent phase), then by extraction index so the result is
/// independent of the unstable sort.
fn sort_batch_entries(entries: &mut [ParticleBatchEntry]) {
    entries.sort_unstable_by(|a, b| {
        a.key
            .cmp(&b.key)
            .then(a.distance.total_cmp(&b.distance))
            .then(a.index.cmp(&b.index))
    });
}

/// Runs of equal batch keys in entries ordered by [`sort_batch_entries`].
fn batch_ranges(entries: &[ParticleBatchEntry]) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut start = 0;
    std::iter::from_fn(move || {
        let first = entries.get(start)?;
        let len = entries[start..]
            .iter()
            .take_while(|entry| entry.key == first.key)
            .count();
        let range = start..start + len;
        start += len;
        Some(range)
    })
}

#[derive(Resource, Clone, Copy, Default)]
pub(crate) struct Wc3ParticleQueueStats {
    pub(crate) batches: usize,
    pub(crate) candidates: usize,
    pub(crate) queued: usize,
    pub(crate) zero_alpha: usize,
    pub(crate) outside: usize,
    pub(crate) populated_slots: usize,
    pub(crate) bound_slots: usize,
    pub(crate) texture_groups_created: usize,
    pub(crate) texture_groups_reused: usize,
    pub(crate) pending_texture_slots: usize,
    pub(crate) pending_texture_particles: usize,
    pub(crate) reorder_moves: usize,
    pub(crate) reorder_overlap_rejects: usize,
    pub(crate) reorder_barriers: usize,
    pub(crate) mesh_prefix_batches: usize,
    pub(crate) mesh_prefix_fallback_batches: usize,
}

#[derive(Clone, Copy)]
struct Wc3ParticleMeshBinding {
    entity: (Entity, MainEntity),
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum Wc3ParticleMeshLayout {
    Model,
    Lightmapped,
    Skinned,
    SkinnedMotion,
    Morphed,
    MorphedMotion,
    MorphedSkinned,
    MorphedSkinnedMotion,
}

impl Wc3ParticleMeshLayout {
    const ALL: [Self; 8] = [
        Self::Model,
        Self::Lightmapped,
        Self::Skinned,
        Self::SkinnedMotion,
        Self::Morphed,
        Self::MorphedMotion,
        Self::MorphedSkinned,
        Self::MorphedSkinnedMotion,
    ];

    fn descriptor(self, layouts: &MeshLayouts) -> &BindGroupLayoutDescriptor {
        match self {
            Self::Model => &layouts.model_only,
            Self::Lightmapped => &layouts.lightmapped,
            Self::Skinned => &layouts.skinned,
            Self::SkinnedMotion => &layouts.skinned_motion,
            Self::Morphed => &layouts.morphed,
            Self::MorphedMotion => &layouts.morphed_motion,
            Self::MorphedSkinned => &layouts.morphed_skinned,
            Self::MorphedSkinnedMotion => &layouts.morphed_skinned_motion,
        }
    }
}

fn uses_shared_mesh(experiment: RenderExperiment) -> bool {
    matches!(
        experiment,
        RenderExperiment::ParticleSharedMesh | RenderExperiment::ParticleSharedMeshPartialBindings
    )
}

#[derive(Component)]
struct Wc3ParticleViewBindGroup(BindGroup);

#[derive(Resource)]
struct Wc3ParticlePipeline {
    view_layout: BindGroupLayoutDescriptor,
    mesh_view_layouts: MeshPipelineViewLayouts,
    mesh_layouts: MeshLayouts,
    shared_mesh_supported: bool,
    single_texture_layout: BindGroupLayoutDescriptor,
    texture_array_layout: BindGroupLayoutDescriptor,
    texture_slab_size: usize,
    partial_binding_arrays: bool,
    shader: Handle<Shader>,
}

impl Wc3ParticlePipeline {
    fn bound_texture_count(&self, experiment: RenderExperiment, populated: usize) -> usize {
        if matches!(
            experiment,
            RenderExperiment::ParticlePartialBindings
                | RenderExperiment::ParticleSharedMeshPartialBindings
        ) && self.partial_binding_arrays
        {
            populated
        } else {
            self.texture_slab_size
        }
    }

    fn texture_slab_size(&self) -> usize {
        self.texture_slab_size
    }

    fn uses_binding_arrays(&self) -> bool {
        self.texture_slab_size > 1
    }

    fn texture_layout(&self) -> &BindGroupLayoutDescriptor {
        if self.uses_binding_arrays() {
            &self.texture_array_layout
        } else {
            &self.single_texture_layout
        }
    }
}

impl FromWorld for Wc3ParticlePipeline {
    fn from_world(world: &mut World) -> Self {
        let view_layout = BindGroupLayoutDescriptor::new(
            "wc3_particle_view_bind_group_layout",
            &BindGroupLayoutEntries::single(
                ShaderStages::VERTEX,
                uniform_buffer::<ViewUniform>(true),
            ),
        );
        let single_texture_layout = BindGroupLayoutDescriptor::new(
            "wc3_particle_texture_bind_group_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                ),
            ),
        );
        let render_device = world.resource::<RenderDevice>();
        let required_features = WgpuFeatures::TEXTURE_BINDING_ARRAY
            | WgpuFeatures::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING;
        let limits = render_device.limits();
        let partial_binding_arrays = render_device
            .features()
            .contains(WgpuFeatures::PARTIALLY_BOUND_BINDING_ARRAY);
        let max_array_size = limits
            .max_binding_array_elements_per_shader_stage
            .min(limits.max_binding_array_sampler_elements_per_shader_stage)
            as usize;
        let texture_slab_size = if render_device.features().contains(required_features) {
            PARTICLE_TEXTURE_SLAB_CANDIDATES
                .into_iter()
                .find(|&candidate| candidate <= max_array_size)
                .unwrap_or(1)
        } else {
            1
        };
        let array_count = NonZeroU32::new(texture_slab_size.max(2) as u32)
            .expect("particle texture slab size must be nonzero");
        let texture_array_layout = BindGroupLayoutDescriptor::new(
            "wc3_particle_texture_array_bind_group_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    texture_2d(TextureSampleType::Float { filterable: true }).count(array_count),
                    sampler(SamplerBindingType::Filtering).count(array_count),
                ),
            ),
        );
        let shader = world
            .resource::<AssetServer>()
            .load(WC3_PARTICLE_SHADER_PATH);
        let mesh_pipeline = world.resource::<MeshPipeline>();
        Self {
            view_layout,
            mesh_view_layouts: world.resource::<MeshPipelineViewLayouts>().clone(),
            mesh_layouts: mesh_pipeline.mesh_layouts.clone(),
            // Uniform-offset paths keep the existing shared-view renderer.
            shared_mesh_supported: !mesh_pipeline.skins_use_uniform_buffers
                && mesh_pipeline.per_object_buffer_batch_size.is_none()
                && limits.max_bind_groups >= 4,
            single_texture_layout,
            texture_array_layout,
            texture_slab_size,
            partial_binding_arrays,
            shader,
        }
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct Wc3ParticlePipelineKey {
    target_format: TextureFormat,
    sample_count: u32,
    texture_slab_size: u32,
    blend_mode: Wc3ParticleBlendMode,
    shared_view: Option<MeshPipelineViewLayoutKey>,
    shared_mesh: Option<Wc3ParticleMeshLayout>,
}

impl SpecializedRenderPipeline for Wc3ParticlePipeline {
    type Key = Wc3ParticlePipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        let mut shader_defs = Vec::with_capacity(2);
        if key.texture_slab_size > 1 {
            shader_defs.push(ShaderDefVal::Bool("PARTICLE_BINDING_ARRAY".into(), true));
            shader_defs.push(ShaderDefVal::UInt(
                "PARTICLE_TEXTURE_SLAB_SIZE".into(),
                key.texture_slab_size,
            ));
        }
        match key.blend_mode {
            Wc3ParticleBlendMode::Alpha => {}
            Wc3ParticleBlendMode::Add => {
                shader_defs.push(ShaderDefVal::Bool("PARTICLE_BLEND_ADD".into(), true));
            }
            Wc3ParticleBlendMode::Multiply => {
                shader_defs.push(ShaderDefVal::Bool("PARTICLE_BLEND_MULTIPLY".into(), true));
            }
        }
        let texture_layout = if key.texture_slab_size > 1 {
            self.texture_array_layout.clone()
        } else {
            self.single_texture_layout.clone()
        };
        let layout = if let Some(view_key) = key.shared_view {
            let view = self.mesh_view_layouts.get_view_layout(view_key);
            let mut layouts = vec![view.main_layout, view.binding_array_layout];
            if let Some(mesh) = key.shared_mesh {
                layouts.push(mesh.descriptor(&self.mesh_layouts).clone());
            }
            shader_defs.push(ShaderDefVal::UInt(
                "PARTICLE_TEXTURE_GROUP".into(),
                layouts.len() as u32,
            ));
            layouts.push(texture_layout);
            layouts
        } else {
            shader_defs.push(ShaderDefVal::UInt("PARTICLE_TEXTURE_GROUP".into(), 1));
            vec![self.view_layout.clone(), texture_layout]
        };
        let blend = match key.blend_mode {
            Wc3ParticleBlendMode::Alpha => BlendState::ALPHA_BLENDING,
            Wc3ParticleBlendMode::Add => BlendState::PREMULTIPLIED_ALPHA_BLENDING,
            Wc3ParticleBlendMode::Multiply => BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::Dst,
                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent::OVER,
            },
        };
        RenderPipelineDescriptor {
            label: Some("wc3 particle pipeline".into()),
            layout,
            vertex: VertexState {
                shader: self.shader.clone(),
                shader_defs: shader_defs.clone(),
                buffers: vec![VertexBufferLayout {
                    array_stride: size_of::<Wc3ParticleInstance>() as u64,
                    step_mode: VertexStepMode::Instance,
                    attributes: vec![
                        VertexAttribute {
                            format: VertexFormat::Float32x4,
                            offset: 0,
                            shader_location: 0,
                        },
                        VertexAttribute {
                            format: VertexFormat::Float32x4,
                            offset: 16,
                            shader_location: 1,
                        },
                        VertexAttribute {
                            format: VertexFormat::Float32x4,
                            offset: 32,
                            shader_location: 2,
                        },
                        VertexAttribute {
                            format: VertexFormat::Uint32,
                            offset: 48,
                            shader_location: 3,
                        },
                    ],
                }],
                ..default()
            },
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                shader_defs,
                targets: vec![Some(ColorTargetState {
                    format: key.target_format,
                    blend: Some(blend),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..default()
            },
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(CompareFunction::GreaterEqual),
                stencil: StencilState {
                    front: StencilFaceState::IGNORE,
                    back: StencilFaceState::IGNORE,
                    read_mask: 0,
                    write_mask: 0,
                },
                bias: DepthBiasState::default(),
            }),
            multisample: MultisampleState {
                count: key.sample_count,
                ..default()
            },
            ..default()
        }
    }
}

type Wc3ParticleQueuePipelines<'w> = (
    Res<'w, PipelineCache>,
    Res<'w, Wc3ParticlePipeline>,
    ResMut<'w, SpecializedRenderPipelines<Wc3ParticlePipeline>>,
);

type Wc3ParticleQueueViews<'w, 's> = (
    Res<'w, ViewKeyCache>,
    Query<
        'w,
        's,
        (
            &'static ExtractedView,
            &'static Msaa,
            Option<&'static Frustum>,
        ),
    >,
);

fn queue_wc3_particles(
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    pipeline_state: Wc3ParticleQueuePipelines<'_>,
    textures: (
        Res<ExtractedWc3Particles>,
        Res<RenderAssets<GpuImage>>,
        Local<Vec<u64>>,
    ),
    mut batching: (ResMut<Wc3ParticleBatches>, Local<Vec<ParticleBatchEntry>>),
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    view_state: Wc3ParticleQueueViews,
    mut stats: ResMut<Wc3ParticleQueueStats>,
) {
    let (view_keys, views) = view_state;
    let (pipeline_cache, pipeline, mut pipelines) = pipeline_state;
    let (extracted, images, mut pending_slots) = textures;
    let (batches, entries) = &mut batching;
    batches.order.clear();
    batches.ranges.clear();
    batches.runs.clear();
    let batching_mode = extracted.batching;
    // GPU images have been prepared before Queue. Resolve readiness once per slot,
    // and reject only that slot's particles, never a slab containing ready effects.
    pending_slots.clear();
    pending_slots.extend(extracted.slab_textures.iter().map(|textures| {
        pending_particle_texture_slots(textures, |texture| images.get(texture).is_some())
    }));
    *stats = Wc3ParticleQueueStats {
        pending_texture_slots: pending_slots
            .iter()
            .map(|mask| mask.count_ones() as usize)
            .sum(),
        populated_slots: extracted.slab_textures.iter().map(Vec::len).sum(),
        bound_slots: extracted
            .slab_textures
            .iter()
            .map(|textures| pipeline.bound_texture_count(extracted.experiment, textures.len()))
            .sum(),
        ..default()
    };
    if extracted.particles.is_empty() {
        return;
    }
    let draw_function = draw_functions.read().id::<DrawWc3BillboardParticles>();

    for (view, msaa, frustum) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let shared_view = if extracted.experiment == RenderExperiment::ParticleSharedView
            || uses_shared_mesh(extracted.experiment)
        {
            let Some(view_key) = view_keys.get(&view.retained_view_entity) else {
                continue;
            };
            Some(MeshPipelineViewLayoutKey::from(*view_key))
        } else {
            None
        };
        let mut pipeline_id = |blend_mode| {
            pipelines.specialize(
                &pipeline_cache,
                &pipeline,
                Wc3ParticlePipelineKey {
                    target_format: view.target_format,
                    sample_count: msaa.samples(),
                    texture_slab_size: pipeline.texture_slab_size as u32,
                    blend_mode,
                    shared_view,
                    shared_mesh: None,
                },
            )
        };
        let alpha_pipeline = pipeline_id(Wc3ParticleBlendMode::Alpha);
        let add_pipeline = pipeline_id(Wc3ParticleBlendMode::Add);
        let multiply_pipeline = pipeline_id(Wc3ParticleBlendMode::Multiply);
        let pipeline_for = |blend_mode| match blend_mode {
            Wc3ParticleBlendMode::Alpha => alpha_pipeline,
            Wc3ParticleBlendMode::Add => add_pipeline,
            Wc3ParticleBlendMode::Multiply => multiply_pipeline,
        };
        let rangefinder = view.rangefinder3d();

        for (index, particle) in extracted.particles.iter().enumerate() {
            stats.candidates += 1;
            if pending_slots[particle.slab_index as usize] & (1 << particle.instance.texture_slot)
                != 0
            {
                stats.pending_texture_particles += 1;
                continue;
            }
            // Zero-alpha and off-frustum particles contribute no pixels; cull them always
            // (formerly the particle-cull render experiment).
            let [x, y, z, scale] = particle.instance.position_scale;
            // A camera-facing square fits inside this sphere at any camera angle.
            // All three transparent blend modes leave the destination unchanged
            // at exactly zero alpha. Keep their simulation/lifetime work intact.
            if extracted.cull && particle.instance.color[3] == 0.0 {
                stats.zero_alpha += 1;
                continue;
            }
            if extracted.cull
                && frustum.is_some_and(|frustum| {
                    !frustum.intersects_sphere(
                        &Sphere {
                            center: Vec3::new(x, y, z).into(),
                            radius: scale.abs() * std::f32::consts::FRAC_1_SQRT_2,
                        },
                        false,
                    )
                })
            {
                stats.outside += 1;
                continue;
            }
            stats.queued += 1;
            let particle_index = index as u32;
            if batching_mode != ParticleBatching::Individual {
                let emitter = match batching_mode {
                    ParticleBatching::Emitter => particle.emitter_index,
                    _ => 0,
                };
                entries.push(ParticleBatchEntry {
                    key: (
                        particle.source.to_bits(),
                        emitter,
                        (particle.slab_index << 2) | particle.blend_mode as u32,
                    ),
                    distance: rangefinder.distance(&Vec3::new(x, y, z)),
                    index: particle_index,
                });
                continue;
            }
            // Sorted phases require a unique (render entity, main entity) key.
            // The render entity identifies the texture slab; the synthetic main
            // entity only distinguishes particles within that transient slab item.
            let synthetic_main = transient_particle_main_entity(particle_index);
            phase.add_transient(Transparent3d {
                sorting_info: TransparentSortingInfo3d::Sorted {
                    mesh_center: Vec3::new(x, y, z),
                    depth_bias: 0.0,
                },
                entity: (particle.slab_entity, synthetic_main),
                pipeline: pipeline_for(particle.blend_mode),
                draw_function,
                distance: 0.0,
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::DynamicOffset(particle_index),
                indexed: false,
            });
        }

        // Batched modes: one phase item per group, ordered back to front inside.
        // Within a group the instance order is exact; only the group as a whole
        // is sorted against the other transparent items, at its centroid.
        sort_batch_entries(entries);
        for range in batch_ranges(entries) {
            let group = &entries[range];
            let mut sum = Vec3::ZERO;
            for entry in group {
                let [x, y, z, _] = extracted.particles[entry.index as usize]
                    .instance
                    .position_scale;
                sum += Vec3::new(x, y, z);
            }
            let first = &extracted.particles[group[0].index as usize];
            let batch_index = batches.ranges.len() as u32;
            let order_start = batches.order.len() as u32;
            batches.order.extend(group.iter().map(|entry| entry.index));
            batches
                .ranges
                .push(order_start..order_start + group.len() as u32);
            stats.batches += 1;
            phase.add_transient(Transparent3d {
                sorting_info: TransparentSortingInfo3d::Sorted {
                    mesh_center: sum / group.len() as f32,
                    depth_bias: 0.0,
                },
                entity: (
                    first.slab_entity,
                    transient_particle_main_entity(batch_index),
                ),
                pipeline: pipeline_for(first.blend_mode),
                draw_function,
                distance: 0.0,
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::DynamicOffset(batch_index),
                indexed: false,
            });
        }
        entries.clear();
    }
}

/// Main-world id that distinguishes one transient particle phase item.
///
/// Bevy's sorted-phase batching looks every item's main entity up in
/// `RenderMeshInstances`. A synthetic id equal to a live mesh entity (as the
/// generation-0 ids of early-spawned doodads were) made Bevy rewrite the
/// particle item's batch range and extra index as if it were that mesh, so
/// whole emitters vanished or drew another emitter's instances for a frame.
/// The final generation of an index is unreachable in practice.
fn transient_particle_main_entity(index: u32) -> MainEntity {
    let index = EntityIndex::from_raw_u32(index).expect("particle item index below u32::MAX");
    MainEntity::from(Entity::from_index_and_generation(
        index,
        EntityGeneration::FIRST.after_versions(u32::MAX),
    ))
}

fn pending_particle_texture_slots(
    textures: &[Option<AssetId<Image>>],
    mut ready: impl FnMut(AssetId<Image>) -> bool,
) -> u64 {
    debug_assert!(textures.len() <= 64);
    textures
        .iter()
        .enumerate()
        .fold(0, |mask, (slot, texture)| {
            if texture.is_some_and(|texture| !ready(texture)) {
                mask | (1 << slot)
            } else {
                mask
            }
        })
}

#[derive(Resource, Default)]
struct Wc3TransparentCoverage(HashMap<MainEntity, [Vec3; 8]>);

type TransparentCoverageMeshes<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static Aabb, &'static GlobalTransform),
    (
        With<Mesh3d>,
        Without<NoFrustumCulling>,
        Without<Wc3RibbonTrail>,
        Without<bevy::mesh::morph::MeshMorphWeights>,
        Or<(
            With<MeshMaterial3d<StandardMaterial>>,
            With<MeshMaterial3d<Wc3AnimatedAlphaMaterial>>,
            With<MeshMaterial3d<Wc3TeamColorMaterial>>,
        )>,
    ),
>;

fn extract_transparent_coverage(
    meshes: Extract<TransparentCoverageMeshes>,
    experiment: Extract<Res<RenderExperiment>>,
    mut coverage: ResMut<Wc3TransparentCoverage>,
) {
    coverage.0.clear();
    if !matches!(
        **experiment,
        RenderExperiment::ParticleOverlapAudit | RenderExperiment::ParticleOverlapBatching
    ) {
        return;
    }
    for (entity, bounds, transform) in &meshes {
        let center: Vec3 = bounds.center.into();
        let half: Vec3 = bounds.half_extents.into();
        let corners = std::array::from_fn(|index| {
            let signs = Vec3::new(
                if index & 1 == 0 { -1.0 } else { 1.0 },
                if index & 2 == 0 { -1.0 } else { 1.0 },
                if index & 4 == 0 { -1.0 } else { 1.0 },
            );
            transform.transform_point(center + signs * half)
        });
        coverage.0.insert(MainEntity::from(entity), corners);
    }
}

#[derive(Clone, Copy)]
struct ScreenCoverage {
    min: Vec2,
    max: Vec2,
}

impl ScreenCoverage {
    fn from_points(
        points: impl IntoIterator<Item = Vec3>,
        clip_from_world: Mat4,
        guard: Vec2,
    ) -> Option<Self> {
        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        for point in points {
            let clip = clip_from_world * point.extend(1.0);
            // Near-plane/eye crossings and unknown bounds are ordering barriers.
            if !clip.is_finite() || clip.w <= 1e-5 {
                return None;
            }
            let position = clip.xy() / clip.w;
            min = min.min(position);
            max = max.max(position);
        }
        (min.is_finite() && max.is_finite()).then_some(Self {
            min: min - guard,
            max: max + guard,
        })
    }

    fn disjoint(self, other: Self) -> bool {
        self.max.x < other.min.x
            || other.max.x < self.min.x
            || self.max.y < other.min.y
            || other.max.y < self.min.y
    }

    fn union(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
}

#[derive(Clone, Copy)]
struct ParticleOrderItem {
    key: Option<(
        bevy::render::render_resource::CachedRenderPipelineId,
        Entity,
    )>,
    coverage: Option<ScreenCoverage>,
}

fn audit_or_reorder_particles(
    phase: &mut SortedRenderPhase<Transparent3d>,
    mut ordering: Vec<ParticleOrderItem>,
    apply: bool,
    stats: &mut Wc3ParticleQueueStats,
) {
    const LOOKAHEAD: usize = 32;
    let mut start = 0;
    while start < ordering.len() {
        let Some(key) = ordering[start].key else {
            start += 1;
            continue;
        };
        let mut end = start + 1;
        while end < ordering.len() && ordering[end].key == Some(key) {
            end += 1;
        }
        let limit = (end + LOOKAHEAD).min(ordering.len());
        let mut blocked: Option<ScreenCoverage> = None;
        let scan_start = end;
        for cursor in scan_start..limit {
            let Some(coverage) = ordering[cursor].coverage else {
                stats.reorder_barriers += 1;
                break;
            };
            if ordering[cursor].key == Some(key)
                && blocked.is_none_or(|blocked| blocked.disjoint(coverage))
            {
                if apply {
                    phase.items.move_index(cursor, end);
                }
                ordering[end..=cursor].rotate_right(1);
                end += 1;
                stats.reorder_moves += 1;
            } else {
                if ordering[cursor].key == Some(key) {
                    stats.reorder_overlap_rejects += 1;
                }
                blocked = Some(blocked.map_or(coverage, |blocked| blocked.union(coverage)));
            }
        }
        start = end;
    }
}

type Wc3ParticleBatchSources<'w, 's> = (
    Res<'w, ExtractedWc3Particles>,
    Res<'w, Wc3TransparentCoverage>,
    Query<'w, 's, &'static ExtractedView>,
);

fn batch_and_upload_wc3_particles(
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    sources: Wc3ParticleBatchSources,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut gpu: ResMut<Wc3ParticleGpuBuffer>,
    mut stats: ResMut<Wc3ParticleQueueStats>,
    mut batches: ResMut<Wc3ParticleBatches>,
    renderer: (Res<RenderDevice>, Res<RenderQueue>),
) {
    let (render_device, render_queue) = renderer;
    let draw_function = draw_functions.read().id::<DrawWc3BillboardParticles>();
    let (extracted, coverage, views) = sources;
    let batches = &mut *batches;
    let batched = extracted.batching != ParticleBatching::Individual;
    gpu.instances.clear();

    for view in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        if matches!(
            extracted.experiment,
            RenderExperiment::ParticleOverlapAudit | RenderExperiment::ParticleOverlapBatching
        ) {
            let model_draw = draw_functions.read().id::<DrawMaterial>();
            let world_from_view = view.world_from_view.to_matrix();
            let clip_from_world = view
                .clip_from_world
                .unwrap_or_else(|| view.clip_from_view * world_from_view.inverse());
            // Two pixels on each side conservatively contain MSAA coverage and
            // helper invocations near triangle edges.
            let guard = Vec2::new(
                4.0 / view.viewport.z.max(1) as f32,
                4.0 / view.viewport.w.max(1) as f32,
            );
            let right = world_from_view.x_axis.xyz();
            let up = world_from_view.y_axis.xyz();
            let ordering = phase
                .items
                .values()
                .map(|item| {
                    if item.draw_function == draw_function
                        && let PhaseItemExtraIndex::DynamicOffset(index) = item.extra_index
                        && let Some(particle) = extracted.particles.get(index as usize)
                    {
                        let [x, y, z, size] = particle.instance.position_scale;
                        let center = Vec3::new(x, y, z);
                        let half = size.abs() * 0.5;
                        let points = [
                            center - right * half - up * half,
                            center - right * half + up * half,
                            center + right * half - up * half,
                            center + right * half + up * half,
                        ];
                        return ParticleOrderItem {
                            key: Some((item.pipeline, item.entity.0)),
                            coverage: ScreenCoverage::from_points(points, clip_from_world, guard),
                        };
                    }
                    let bounds = if item.draw_function == model_draw && item.batch_range.len() <= 1
                    {
                        coverage.0.get(&item.entity.1).and_then(|points| {
                            ScreenCoverage::from_points(*points, clip_from_world, guard)
                        })
                    } else {
                        None
                    };
                    ParticleOrderItem {
                        key: None,
                        coverage: bounds,
                    }
                })
                .collect();
            audit_or_reorder_particles(
                phase,
                ordering,
                extracted.experiment == RenderExperiment::ParticleOverlapBatching,
                &mut stats,
            );
        }
        let instances = &mut gpu.instances;
        let instance_start = instances.len() as u32;
        plan_particle_draws(
            phase,
            draw_function,
            batched,
            &mut batches.runs,
            instance_start,
            |source_index| {
                if batched {
                    let range = batches.ranges.get(source_index as usize)?;
                    for &particle in &batches.order[range.start as usize..range.end as usize] {
                        instances.push(extracted.particles[particle as usize].instance);
                    }
                    Some(range.len() as u32)
                } else {
                    instances.push(extracted.particles.get(source_index as usize)?.instance);
                    Some(1)
                }
            },
        );
    }

    gpu.instances.write_buffer(&render_device, &render_queue);
}

/// Merges adjacent particle items that share a texture slab and pipeline into
/// one draw and records each draw's instance range, starting at
/// `instance_start`. `push` appends a source item's instances and returns how
/// many it pushed, or `None` for an unknown source index. Returns the end of
/// the instances pushed.
///
/// This runs before Bevy batches the phase's meshes in Prepare. Transparent
/// mesh items are retained between frames, so their `batch_range` still holds
/// the previous frame's batch length here. Stepping over them by that length
/// skipped this frame's particle items, which then drew another run's
/// instances through their stale queue-time extra index, so whole emitters
/// vanished or drew twice. Non-particle items are stepped over one at a time.
fn plan_particle_draws(
    phase: &mut SortedRenderPhase<Transparent3d>,
    draw_function: DrawFunctionId,
    batched: bool,
    runs: &mut Vec<Range<u32>>,
    instance_start: u32,
    mut push: impl FnMut(u32) -> Option<u32>,
) -> u32 {
    let mut instance_end = instance_start;
    let mut item_index = 0usize;
    while item_index < phase.items.len() {
        let item = &phase.items[item_index];
        if item.draw_function != draw_function {
            item_index += 1;
            continue;
        }

        let texture_entity = item.entity.0;
        let run_pipeline = item.pipeline;
        let run_start = item_index;
        let gpu_start = instance_end;
        let mut run_end = run_start;

        while run_end < phase.items.len() {
            let candidate = &phase.items[run_end];
            if candidate.draw_function != draw_function
                || candidate.entity.0 != texture_entity
                || candidate.pipeline != run_pipeline
            {
                break;
            }
            let PhaseItemExtraIndex::DynamicOffset(source_index) = candidate.extra_index else {
                break;
            };
            let Some(pushed) = push(source_index) else {
                break;
            };
            instance_end += pushed;
            run_end += 1;
        }

        if batched {
            // The representative must still span the merged items so the
            // phase skips them; its instances are looked up by run.
            phase.items[run_start].batch_range = 0..(run_end - run_start) as u32;
            phase.items[run_start].extra_index =
                PhaseItemExtraIndex::DynamicOffset(runs.len() as u32);
            runs.push(gpu_start..instance_end);
        } else {
            phase.items[run_start].batch_range = gpu_start..instance_end;
            phase.items[run_start].extra_index = PhaseItemExtraIndex::None;
        }
        for skipped in run_start + 1..run_end {
            phase.items[skipped].batch_range = 0..0;
            phase.items[skipped].extra_index = PhaseItemExtraIndex::None;
        }
        item_index = run_end.max(run_start + 1);
    }
    instance_end
}

type Wc3ParticleMeshPrefixSources<'w, 's> = (
    Res<'w, ExtractedWc3Particles>,
    Query<'w, 's, (Entity, &'static ExtractedView, &'static Msaa)>,
    Res<'w, ViewKeyCache>,
    Res<'w, RenderMeshInstances>,
);

fn prepare_wc3_particle_mesh_prefixes(
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    sources: Wc3ParticleMeshPrefixSources,
    pipeline_state: Wc3ParticleQueuePipelines,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut gpu: ResMut<Wc3ParticleGpuBuffer>,
    mut stats: ResMut<Wc3ParticleQueueStats>,
) {
    gpu.mesh_bindings.clear();
    let (extracted, views, view_keys, mesh_instances) = sources;
    let (pipeline_cache, pipeline, mut pipelines) = pipeline_state;
    if !uses_shared_mesh(extracted.experiment) {
        return;
    }
    let functions = draw_functions.read();
    let particle_draw = functions.id::<DrawWc3BillboardParticles>();
    let model_draw = functions.id::<DrawMaterial>();
    for (view_entity, view, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let shared_view = pipeline
            .shared_mesh_supported
            .then(|| view_keys.get(&view.retained_view_entity).copied())
            .flatten()
            .map(MeshPipelineViewLayoutKey::from);
        let view_layout = shared_view.map(|key| pipeline.mesh_view_layouts.get_view_layout(key));
        let mut preceding_mesh = None;
        let mut index = 0;
        while index < phase.items.len() {
            let item = &phase.items[index];
            let count = item.batch_range.len();
            if count == 0 {
                index += 1;
                continue;
            }
            if item.draw_function != particle_draw {
                // This runs after Bevy batches meshes. Only the representative
                // that actually draws can establish the next binding prefix.
                preceding_mesh = view_layout.as_ref().and_then(|view_layout| {
                    if item.draw_function != model_draw
                        || mesh_instances.mesh_asset_id(item.entity.1).is_none()
                    {
                        return None;
                    }
                    pipeline_cache.get_render_pipeline(item.pipeline)?;
                    let descriptor = pipeline_cache.get_render_pipeline_descriptor(item.pipeline);
                    if descriptor.layout.first() != Some(&view_layout.main_layout)
                        || descriptor.layout.get(1) != Some(&view_layout.binding_array_layout)
                    {
                        return None;
                    }
                    let layout = descriptor.layout.get(2)?;
                    Wc3ParticleMeshLayout::ALL
                        .into_iter()
                        .find(|kind| kind.descriptor(&pipeline.mesh_layouts) == layout)
                        .map(|kind| {
                            (
                                kind,
                                Wc3ParticleMeshBinding {
                                    entity: item.entity,
                                },
                            )
                        })
                });
            } else {
                let compatible = preceding_mesh.and_then(|(layout, binding)| {
                    let shared_view = shared_view?;
                    pipeline_cache.get_render_pipeline(item.pipeline)?;
                    let descriptor = pipeline_cache.get_render_pipeline_descriptor(item.pipeline);
                    let blend_mode = descriptor
                        .fragment
                        .as_ref()?
                        .shader_defs
                        .iter()
                        .find_map(|def| match def {
                            ShaderDefVal::Bool(name, true) if name == "PARTICLE_BLEND_ADD" => {
                                Some(Wc3ParticleBlendMode::Add)
                            }
                            ShaderDefVal::Bool(name, true) if name == "PARTICLE_BLEND_MULTIPLY" => {
                                Some(Wc3ParticleBlendMode::Multiply)
                            }
                            _ => None,
                        })
                        .unwrap_or(Wc3ParticleBlendMode::Alpha);
                    let mesh_pipeline = pipelines.specialize(
                        &pipeline_cache,
                        &pipeline,
                        Wc3ParticlePipelineKey {
                            target_format: view.target_format,
                            sample_count: msaa.samples(),
                            texture_slab_size: pipeline.texture_slab_size as u32,
                            blend_mode,
                            shared_view: Some(shared_view),
                            shared_mesh: Some(layout),
                        },
                    );
                    // Keep the original draw while the variant compiles.
                    pipeline_cache.get_render_pipeline(mesh_pipeline)?;
                    Some((mesh_pipeline, binding))
                });
                if let Some((mesh_pipeline, binding)) = compatible {
                    gpu.mesh_bindings
                        .insert((view_entity, item.entity), binding);
                    phase.items[index].pipeline = mesh_pipeline;
                    stats.mesh_prefix_batches += 1;
                } else {
                    stats.mesh_prefix_fallback_batches += 1;
                }
            }
            index += count;
        }
    }
}

fn prepare_wc3_particle_view_bind_groups(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    view_uniforms: Res<ViewUniforms>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<Wc3ParticlePipeline>,
    views: Query<Entity, With<ViewUniformOffset>>,
) {
    let Some(binding) = view_uniforms.uniforms.binding() else {
        return;
    };
    let layout = pipeline_cache.get_bind_group_layout(&pipeline.view_layout);
    for entity in &views {
        let bind_group = render_device.create_bind_group(
            "wc3 particle view bind group",
            &layout,
            &BindGroupEntries::single(binding.clone()),
        );
        commands
            .entity(entity)
            .insert(Wc3ParticleViewBindGroup(bind_group));
    }
}

fn prepare_wc3_particle_texture_bind_groups(
    renderer: (
        Res<RenderDevice>,
        Res<PipelineCache>,
        Res<Wc3ParticlePipeline>,
    ),
    extracted: Res<ExtractedWc3Particles>,
    images: Res<RenderAssets<GpuImage>>,
    fallback_image: Res<FallbackImage>,
    mut bind_groups: ResMut<Wc3ParticleTextureBindGroups>,
    mut stats: ResMut<Wc3ParticleQueueStats>,
) {
    let (render_device, pipeline_cache, pipeline) = renderer;
    // Keep only active slabs. GPU resource identity, rather than just Image IDs,
    // invalidates a cached group when an image reloads or its sampler changes.
    bind_groups.0.retain(|entity, _| {
        extracted.slab_entities[..extracted.slab_textures.len()].contains(entity)
    });
    let layout = pipeline_cache.get_bind_group_layout(pipeline.texture_layout());
    for (slab_index, textures) in extracted.slab_textures.iter().enumerate() {
        let entity = extracted.slab_entities[slab_index];
        let fallback = &fallback_image.d2;
        let resolved = textures
            .iter()
            // Pending slots still need valid bindings, even though Queue omits
            // their particles. The ready slots keep drawing throughout a reload.
            .map(|texture| {
                texture
                    .and_then(|texture| images.get(texture))
                    .unwrap_or(fallback)
            })
            .collect::<Vec<_>>();
        let bound_count = pipeline.bound_texture_count(extracted.experiment, textures.len());
        if extracted.experiment != RenderExperiment::ParticleUncachedBindings
            && bind_groups.0.get(&entity).is_some_and(|cached| {
                cached.fallback == ParticleTextureBinding::from(fallback)
                    && particle_bindings_match(
                        &cached.resources,
                        cached.bound_count,
                        resolved
                            .iter()
                            .map(|image| ParticleTextureBinding::from(*image)),
                        bound_count,
                    )
            })
        {
            stats.texture_groups_reused += 1;
            continue;
        }
        stats.texture_groups_created += 1;
        let bind_group = if pipeline.uses_binding_arrays() {
            // All instance slots address this populated prefix. Unsupported devices
            // retain full arrays; an empty slab is never emitted by extraction.
            let mut texture_views = vec![&*fallback.texture_view; bound_count];
            let mut samplers = vec![&*fallback.sampler; bound_count];
            for (slot, image) in resolved.iter().enumerate() {
                texture_views[slot] = &*image.texture_view;
                samplers[slot] = &*image.sampler;
            }
            render_device.create_bind_group(
                "wc3 particle texture array bind group",
                &layout,
                &BindGroupEntries::sequential((&texture_views[..], &samplers[..])),
            )
        } else {
            let image = resolved[0];
            render_device.create_bind_group(
                "wc3 particle texture bind group",
                &layout,
                &BindGroupEntries::sequential((&image.texture_view, &image.sampler)),
            )
        };
        bind_groups.0.insert(
            entity,
            CachedParticleTextureBindGroup {
                resources: resolved
                    .into_iter()
                    .map(ParticleTextureBinding::from)
                    .collect(),
                bound_count,
                fallback: ParticleTextureBinding::from(fallback),
                bind_group,
            },
        );
    }
}

struct DrawWc3BillboardParticleCommand;

impl<P: PhaseItem> RenderCommand<P> for DrawWc3BillboardParticleCommand {
    type Param = (
        SRes<Wc3ParticleGpuBuffer>,
        SRes<Wc3ParticleTextureBindGroups>,
        SRes<ExtractedWc3Particles>,
        SRes<Wc3ParticleBatches>,
        <SetMeshBindGroup<2> as RenderCommand<Transparent3d>>::Param,
    );
    type ViewQuery = (
        Entity,
        Read<ViewUniformOffset>,
        Read<Wc3ParticleViewBindGroup>,
        Option<Read<MeshViewBindGroup>>,
        Has<MotionVectorPrepass>,
    );
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        (view_entity, view_uniform, view_bind_group, mesh_view, motion_vectors): ROQueryItem<
            'w,
            '_,
            Self::ViewQuery,
        >,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (gpu, texture_bind_groups, extracted, batches, mesh_params): SystemParamItem<
            'w,
            '_,
            Self::Param,
        >,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let gpu = gpu.into_inner();
        let texture_bind_groups = texture_bind_groups.into_inner();
        let Some(instance_buffer) = gpu.instances.buffer() else {
            return RenderCommandResult::Skip;
        };
        let Some(texture_bind_group) = texture_bind_groups.0.get(&item.entity()) else {
            return RenderCommandResult::Skip;
        };

        let experiment = extracted.into_inner().experiment;
        if experiment == RenderExperiment::ParticleSharedView || uses_shared_mesh(experiment) {
            let Some(mesh_view) = mesh_view else {
                return RenderCommandResult::Skip;
            };
            pass.set_bind_group(0, &mesh_view.main, &mesh_view.main_offsets);
            pass.set_bind_group(1, &mesh_view.binding_array, &[]);
            if let Some(binding) = gpu
                .mesh_bindings
                .get(&(view_entity, (item.entity(), item.main_entity())))
            {
                // Storage-buffer variants need no dynamic mesh/skin offsets.
                // Resolve the preceding mesh's current bind group through the
                // engine; never cache GPU buffers or rewrite skin indices.
                let mesh_item = Transparent3d {
                    sorting_info: TransparentSortingInfo3d::Sorted {
                        mesh_center: Vec3::ZERO,
                        depth_bias: 0.0,
                    },
                    distance: 0.0,
                    pipeline: bevy::render::render_resource::CachedRenderPipelineId::INVALID,
                    entity: binding.entity,
                    draw_function: item.draw_function(),
                    batch_range: 0..1,
                    extra_index: PhaseItemExtraIndex::None,
                    indexed: false,
                };
                let result = <SetMeshBindGroup<2> as RenderCommand<Transparent3d>>::render(
                    &mesh_item,
                    motion_vectors,
                    Some(()),
                    mesh_params,
                    pass,
                );
                if !matches!(result, RenderCommandResult::Success) {
                    return result;
                }
                pass.set_bind_group(3, &texture_bind_group.bind_group, &[]);
            } else {
                pass.set_bind_group(2, &texture_bind_group.bind_group, &[]);
            }
        } else {
            pass.set_bind_group(0, &view_bind_group.0, &[view_uniform.offset]);
            pass.set_bind_group(1, &texture_bind_group.bind_group, &[]);
        }
        pass.set_vertex_buffer(0, instance_buffer.slice(..));
        let instances = batches
            .run_instances(&item.extra_index())
            .unwrap_or_else(|| item.batch_range().clone());
        pass.draw(0..6, instances);
        RenderCommandResult::Success
    }
}

type DrawWc3BillboardParticles = (SetItemPipeline, DrawWc3BillboardParticleCommand);

pub(crate) fn particle_draw_function_id(
    draw_functions: &DrawFunctions<Transparent3d>,
) -> DrawFunctionId {
    draw_functions.read().id::<DrawWc3BillboardParticles>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        source: u64,
        emitter: u32,
        slab_blend: u32,
        distance: f32,
        index: u32,
    ) -> ParticleBatchEntry {
        ParticleBatchEntry {
            key: (source, emitter, slab_blend),
            distance,
            index,
        }
    }

    #[test]
    fn particle_runs_ignore_stale_batch_ranges_of_retained_mesh_items() {
        let particle_draw = DrawFunctionId(0);
        let mesh_draw = DrawFunctionId(1);
        let slab = Entity::from_raw_u32(1).unwrap();
        let item = |draw_function, entity, main, extra_index, batch_range| Transparent3d {
            sorting_info: TransparentSortingInfo3d::Sorted {
                mesh_center: Vec3::ZERO,
                depth_bias: 0.0,
            },
            entity: (entity, transient_particle_main_entity(main)),
            pipeline: bevy::render::render_resource::CachedRenderPipelineId::INVALID,
            draw_function,
            distance: 0.0,
            batch_range,
            extra_index,
            indexed: false,
        };
        let mut phase = SortedRenderPhase::<Transparent3d>::default();
        // A retained mesh item still carrying last frame's four-item batch range.
        phase.add_retained(item(
            mesh_draw,
            Entity::from_raw_u32(2).unwrap(),
            100,
            PhaseItemExtraIndex::None,
            7..11,
        ));
        for source in 0..3 {
            phase.add_transient(item(
                particle_draw,
                slab,
                source,
                PhaseItemExtraIndex::DynamicOffset(source),
                0..1,
            ));
        }

        let mut runs = Vec::new();
        let mut pushed = Vec::new();
        let end = plan_particle_draws(&mut phase, particle_draw, true, &mut runs, 0, |source| {
            pushed.push(source);
            Some(2)
        });

        assert_eq!(pushed, [0, 1, 2]);
        assert_eq!(end, 6);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0], 0..6);
        assert_eq!(phase.items[1].batch_range, 0..3);
        assert!(matches!(
            phase.items[1].extra_index,
            PhaseItemExtraIndex::DynamicOffset(0)
        ));
    }

    #[test]
    fn batches_split_by_emitter_slab_and_blend_and_order_back_to_front() {
        let mut entries = vec![
            entry(7, 0, 0, -1.0, 0),
            entry(7, 1, 0, -5.0, 1),
            entry(7, 0, 0, -9.0, 2),
            // Same emitter, different texture slab or blend mode: separate draws.
            entry(7, 0, 1 << 2, -3.0, 3),
            entry(7, 0, 1, -4.0, 4),
            entry(3, 0, 0, -2.0, 5),
            entry(7, 0, 0, -9.0, 6),
        ];
        sort_batch_entries(&mut entries);
        let batches: Vec<Vec<u32>> = batch_ranges(&entries)
            .map(|range| entries[range].iter().map(|entry| entry.index).collect())
            .collect();
        // Ascending distance is back to front; equal distances fall back to index.
        assert_eq!(
            batches,
            vec![vec![5], vec![2, 6, 0], vec![4], vec![3], vec![1]]
        );
    }

    #[test]
    fn batch_ranges_cover_every_entry_exactly_once() {
        assert_eq!(batch_ranges(&[]).count(), 0);
        let mut entries = (0..10)
            .map(|index| entry(u64::from(index % 3), 0, 0, -(index as f32), index))
            .collect::<Vec<_>>();
        sort_batch_entries(&mut entries);
        let ranges = batch_ranges(&entries).collect::<Vec<_>>();
        assert_eq!(ranges.first().map(|range| range.start), Some(0));
        assert_eq!(ranges.last().map(|range| range.end), Some(entries.len()));
        assert!(ranges.windows(2).all(|pair| pair[0].end == pair[1].start));
        assert_eq!(ranges.len(), 3);
    }

    #[test]
    fn default_path_batches_by_emitter_and_depth_sort_experiments_keep_per_particle_items() {
        assert!(ParticleBatching::of(RenderExperiment::Baseline) == ParticleBatching::Emitter);
        assert!(
            ParticleBatching::of(RenderExperiment::ParticleSourceBatching)
                == ParticleBatching::Source
        );
        for experiment in [
            RenderExperiment::ParticleDepthSort,
            RenderExperiment::ParticleOverlapBatching,
            RenderExperiment::ParticleSharedMesh,
        ] {
            assert!(ParticleBatching::of(experiment) == ParticleBatching::Individual);
        }
    }

    #[test]
    fn pending_texture_slots_leave_ready_and_untextured_particles_drawable() {
        let ready = Handle::<Image>::default().id();
        let pending = AssetId::<Image>::Uuid {
            uuid: bevy::asset::uuid::Uuid::from_u128(1),
        };
        let textures = [Some(ready), Some(pending), None, Some(ready)];
        assert_eq!(
            pending_particle_texture_slots(&textures, |id| id == ready),
            0b0010
        );
        assert_eq!(pending_particle_texture_slots(&textures, |_| true), 0);
        assert_eq!(pending_particle_texture_slots(&textures, |_| false), 0b1011);
        // The highest slot must work on the 64-texture path, and the fallback
        // single-texture path must make the same per-slot readiness decision.
        let mut full_slab = [None; 64];
        full_slab[63] = Some(pending);
        assert_eq!(
            pending_particle_texture_slots(&full_slab, |_| false),
            1 << 63
        );
        assert_eq!(
            pending_particle_texture_slots(&[Some(pending)], |_| false),
            1
        );
    }

    #[test]
    fn particle_binding_cache_invalidates_replaced_resources_and_reordered_slots() {
        let first = ParticleTextureBinding {
            view: TextureViewId::new(),
            sampler: SamplerId::new(),
        };
        let second = ParticleTextureBinding {
            view: TextureViewId::new(),
            sampler: SamplerId::new(),
        };
        let resources = [first, second];
        assert!(particle_bindings_match(
            &resources,
            64,
            resources.into_iter(),
            64
        ));
        assert!(!particle_bindings_match(
            &resources,
            64,
            [second, first].into_iter(),
            64
        ));
        assert!(!particle_bindings_match(
            &resources,
            64,
            [first].into_iter(),
            64
        ));
        assert!(!particle_bindings_match(
            &resources,
            64,
            resources.into_iter(),
            2
        ));
        let replaced_view = ParticleTextureBinding {
            view: TextureViewId::new(),
            ..first
        };
        let replaced_sampler = ParticleTextureBinding {
            sampler: SamplerId::new(),
            ..first
        };
        assert!(!particle_bindings_match(
            &resources,
            64,
            [replaced_view, second].into_iter(),
            64
        ));
        assert!(!particle_bindings_match(
            &resources,
            64,
            [replaced_sampler, second].into_iter(),
            64
        ));
        assert!(particle_bindings_match(&[first], 1, [first].into_iter(), 1));
    }
}
