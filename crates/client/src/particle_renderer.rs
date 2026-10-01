//! Dedicated renderer for transparent WC3 particle-emitter quads.
//!
//! Particles stay in a compact main-world resource instead of becoming Mesh3d
//! entities. The render world still inserts one transparent phase item per
//! particle so sorting remains interleaved with alpha-blended model geometry.
//! After the phase is sorted, adjacent particles that share a texture slab are
//! collapsed into a single instanced draw without changing that ordering. On
//! hardware with non-uniform texture binding arrays, each slab holds up to 64
//! particle textures, so texture changes within the run do not split draws.

use std::{collections::HashMap, mem::size_of, num::NonZeroU32};

use bevy::{
    asset::AssetId,
    camera::{
        primitives::{Aabb, Frustum, Sphere},
        visibility::NoFrustumCulling,
    },
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d, TransparentSortingInfo3d},
    ecs::{
        query::ROQueryItem,
        system::{
            SystemParamItem,
            lifetimeless::{Read, SRes},
        },
    },
    mesh::VertexBufferLayout,
    pbr::{
        DrawMaterial, MeshPipelineViewLayoutKey, MeshPipelineViewLayouts, MeshViewBindGroup,
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
            .add_systems(
                RenderStartup,
                (|mut commands: Commands| commands.init_resource::<Wc3ParticlePipeline>())
                    .after(bevy::pbr::init_mesh_pipeline_view_layouts),
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
    slab_entity: Entity,
    blend_mode: Wc3ParticleBlendMode,
}

#[derive(Resource, Default)]
struct ExtractedWc3Particles {
    particles: Vec<ExtractedWc3Particle>,
    slab_entities: Vec<Entity>,
    slab_textures: Vec<Vec<Option<AssetId<Image>>>>,
    experiment: RenderExperiment,
}

fn extract_wc3_particles(
    mut commands: Commands,
    particles: Extract<Res<Wc3BillboardParticles>>,
    experiment: Extract<Res<RenderExperiment>>,
    pipeline: Res<Wc3ParticlePipeline>,
    mut extracted: ResMut<ExtractedWc3Particles>,
) {
    extracted.particles.clear();
    extracted.slab_textures.clear();
    extracted.experiment = **experiment;

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
            slab_entity,
            blend_mode,
        });
    }
}

#[derive(Resource)]
struct Wc3ParticleGpuBuffer {
    instances: RawBufferVec<Wc3ParticleInstance>,
}

impl Default for Wc3ParticleGpuBuffer {
    fn default() -> Self {
        let mut instances = RawBufferVec::new(BufferUsages::VERTEX);
        instances.set_label(Some("wc3 particle instances"));
        Self { instances }
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

#[derive(Resource, Clone, Copy, Default)]
pub(crate) struct Wc3ParticleQueueStats {
    pub(crate) candidates: usize,
    pub(crate) queued: usize,
    pub(crate) zero_alpha: usize,
    pub(crate) outside: usize,
    pub(crate) populated_slots: usize,
    pub(crate) bound_slots: usize,
    pub(crate) texture_groups_created: usize,
    pub(crate) texture_groups_reused: usize,
    pub(crate) reorder_moves: usize,
    pub(crate) reorder_overlap_rejects: usize,
    pub(crate) reorder_barriers: usize,
}

#[derive(Component)]
struct Wc3ParticleViewBindGroup(BindGroup);

#[derive(Resource)]
struct Wc3ParticlePipeline {
    view_layout: BindGroupLayoutDescriptor,
    mesh_view_layouts: MeshPipelineViewLayouts,
    single_texture_layout: BindGroupLayoutDescriptor,
    texture_array_layout: BindGroupLayoutDescriptor,
    texture_slab_size: usize,
    partial_binding_arrays: bool,
    shader: Handle<Shader>,
}

impl Wc3ParticlePipeline {
    fn bound_texture_count(&self, experiment: RenderExperiment, populated: usize) -> usize {
        if experiment == RenderExperiment::ParticlePartialBindings && self.partial_binding_arrays {
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
        Self {
            view_layout,
            mesh_view_layouts: world.resource::<MeshPipelineViewLayouts>().clone(),
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
            shader_defs.push(ShaderDefVal::UInt("PARTICLE_TEXTURE_GROUP".into(), 2));
            vec![view.main_layout, view.binding_array_layout, texture_layout]
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

fn queue_wc3_particles(
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    pipeline_state: Wc3ParticleQueuePipelines<'_>,
    extracted: Res<ExtractedWc3Particles>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    view_keys: Res<ViewKeyCache>,
    views: Query<(&ExtractedView, &Msaa, Option<&Frustum>)>,
    mut stats: ResMut<Wc3ParticleQueueStats>,
) {
    let (pipeline_cache, pipeline, mut pipelines) = pipeline_state;
    *stats = Wc3ParticleQueueStats {
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
        let shared_view = if extracted.experiment == RenderExperiment::ParticleSharedView {
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
                },
            )
        };
        let alpha_pipeline = pipeline_id(Wc3ParticleBlendMode::Alpha);
        let add_pipeline = pipeline_id(Wc3ParticleBlendMode::Add);
        let multiply_pipeline = pipeline_id(Wc3ParticleBlendMode::Multiply);

        for (index, particle) in extracted.particles.iter().enumerate() {
            stats.candidates += 1;
            if extracted.experiment == RenderExperiment::ParticleCull {
                let [x, y, z, scale] = particle.instance.position_scale;
                // A camera-facing square fits inside this sphere at any camera angle.
                // All three transparent blend modes leave the destination unchanged
                // at exactly zero alpha. Keep their simulation/lifetime work intact.
                if particle.instance.color[3] == 0.0 {
                    stats.zero_alpha += 1;
                    continue;
                }
                if frustum.is_some_and(|frustum| {
                    !frustum.intersects_sphere(
                        &Sphere {
                            center: Vec3::new(x, y, z).into(),
                            radius: scale.abs() * std::f32::consts::FRAC_1_SQRT_2,
                        },
                        false,
                    )
                }) {
                    stats.outside += 1;
                    continue;
                }
            }
            stats.queued += 1;
            let particle_index = index as u32;
            // Sorted phases require a unique (render entity, main entity) key.
            // The render entity identifies the texture slab; the synthetic main
            // entity only distinguishes particles within that transient slab item.
            let synthetic_main = Entity::from_bits(u64::from(particle_index) + 1);
            phase.add_transient(Transparent3d {
                sorting_info: TransparentSortingInfo3d::Sorted {
                    mesh_center: Vec3::new(
                        particle.instance.position_scale[0],
                        particle.instance.position_scale[1],
                        particle.instance.position_scale[2],
                    ),
                    depth_bias: 0.0,
                },
                entity: (particle.slab_entity, MainEntity::from(synthetic_main)),
                pipeline: match particle.blend_mode {
                    Wc3ParticleBlendMode::Alpha => alpha_pipeline,
                    Wc3ParticleBlendMode::Add => add_pipeline,
                    Wc3ParticleBlendMode::Multiply => multiply_pipeline,
                },
                draw_function,
                distance: 0.0,
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::DynamicOffset(particle_index),
                indexed: false,
            });
        }
    }
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

fn batch_and_upload_wc3_particles(
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    sources: (
        Res<ExtractedWc3Particles>,
        Res<Wc3TransparentCoverage>,
        Query<&ExtractedView>,
    ),
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut gpu: ResMut<Wc3ParticleGpuBuffer>,
    mut stats: ResMut<Wc3ParticleQueueStats>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
) {
    let draw_function = draw_functions.read().id::<DrawWc3BillboardParticles>();
    let (extracted, coverage, views) = sources;
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
        let mut item_index = 0usize;
        while item_index < phase.items.len() {
            let item = &phase.items[item_index];
            let item_span = item.batch_range.len().max(1);
            if item.draw_function != draw_function {
                item_index += item_span;
                continue;
            }

            let texture_entity = item.entity.0;
            let pipeline = item.pipeline;
            let run_start = item_index;
            let gpu_start = gpu.instances.len() as u32;
            let mut run_end = run_start;

            while run_end < phase.items.len() {
                let candidate = &phase.items[run_end];
                if candidate.draw_function != draw_function
                    || candidate.entity.0 != texture_entity
                    || candidate.pipeline != pipeline
                {
                    break;
                }
                let PhaseItemExtraIndex::DynamicOffset(source_index) = candidate.extra_index else {
                    break;
                };
                let Some(source) = extracted.particles.get(source_index as usize) else {
                    break;
                };
                gpu.instances.push(source.instance);
                run_end += 1;
            }

            let gpu_end = gpu.instances.len() as u32;
            phase.items[run_start].batch_range = gpu_start..gpu_end;
            phase.items[run_start].extra_index = PhaseItemExtraIndex::None;
            for skipped in run_start + 1..run_end {
                phase.items[skipped].batch_range = 0..0;
                phase.items[skipped].extra_index = PhaseItemExtraIndex::None;
            }
            item_index = run_end.max(run_start + 1);
        }
    }

    gpu.instances.write_buffer(&render_device, &render_queue);
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
            .map(|texture| match texture {
                Some(texture) => images.get(*texture),
                None => Some(fallback),
            })
            .collect::<Option<Vec<_>>>();
        let Some(resolved) = resolved else {
            // Never keep drawing a stale texture while its replacement is pending.
            bind_groups.0.remove(&entity);
            continue;
        };
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
    );
    type ViewQuery = (
        Read<ViewUniformOffset>,
        Read<Wc3ParticleViewBindGroup>,
        Option<Read<MeshViewBindGroup>>,
    );
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        (view_uniform, view_bind_group, mesh_view): ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (gpu, texture_bind_groups, extracted): SystemParamItem<'w, '_, Self::Param>,
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

        if extracted.into_inner().experiment == RenderExperiment::ParticleSharedView {
            let Some(mesh_view) = mesh_view else {
                return RenderCommandResult::Skip;
            };
            pass.set_bind_group(0, &mesh_view.main, &mesh_view.main_offsets);
            pass.set_bind_group(1, &mesh_view.binding_array, &[]);
            pass.set_bind_group(2, &texture_bind_group.bind_group, &[]);
        } else {
            pass.set_bind_group(0, &view_bind_group.0, &[view_uniform.offset]);
            pass.set_bind_group(1, &texture_bind_group.bind_group, &[]);
        }
        pass.set_vertex_buffer(0, instance_buffer.slice(..));
        pass.draw(0..6, item.batch_range().clone());
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
