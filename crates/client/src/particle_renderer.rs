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
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d, TransparentSortingInfo3d},
    ecs::{
        query::ROQueryItem,
        system::{
            SystemParamItem,
            lifetimeless::{Read, SRes},
        },
    },
    mesh::VertexBufferLayout,
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        render_asset::RenderAssets,
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            BlendComponent, BlendFactor, BlendOperation, BlendState, BufferUsages,
            ColorTargetState, ColorWrites, CompareFunction, DepthBiasState, DepthStencilState,
            FragmentState, MultisampleState, PipelineCache, PrimitiveState, PrimitiveTopology,
            RawBufferVec, RenderPipelineDescriptor, SamplerBindingType, ShaderStages,
            SpecializedRenderPipeline, SpecializedRenderPipelines, StencilFaceState, StencilState,
            TextureFormat, TextureSampleType, VertexAttribute, VertexFormat, VertexState,
            VertexStepMode, WgpuFeatures,
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
    wc3_effects::{Wc3BillboardParticleRenderData, Wc3BillboardParticles, Wc3ParticleBlendMode},
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
            .init_resource::<Wc3ParticleGpuBuffer>()
            .init_resource::<Wc3ParticleTextureBindGroups>()
            .init_resource::<Wc3ParticlePipeline>()
            .init_resource::<SpecializedRenderPipelines<Wc3ParticlePipeline>>()
            .add_render_command::<Transparent3d, DrawWc3BillboardParticles>()
            .add_systems(ExtractSchedule, extract_wc3_particles)
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
struct Wc3ParticleTextureBindGroups(HashMap<Entity, BindGroup>);

#[derive(Component)]
struct Wc3ParticleViewBindGroup(BindGroup);

#[derive(Resource)]
struct Wc3ParticlePipeline {
    view_layout: BindGroupLayoutDescriptor,
    single_texture_layout: BindGroupLayoutDescriptor,
    texture_array_layout: BindGroupLayoutDescriptor,
    texture_slab_size: usize,
    shader: Handle<Shader>,
}

impl Wc3ParticlePipeline {
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
            single_texture_layout,
            texture_array_layout,
            texture_slab_size,
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
            layout: vec![self.view_layout.clone(), texture_layout],
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

fn queue_wc3_particles(
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<Wc3ParticlePipeline>,
    mut pipelines: ResMut<SpecializedRenderPipelines<Wc3ParticlePipeline>>,
    extracted: Res<ExtractedWc3Particles>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    views: Query<(&ExtractedView, &Msaa)>,
) {
    if extracted.particles.is_empty() {
        return;
    }
    let draw_function = draw_functions.read().id::<DrawWc3BillboardParticles>();

    for (view, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
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
                },
            )
        };
        let alpha_pipeline = pipeline_id(Wc3ParticleBlendMode::Alpha);
        let add_pipeline = pipeline_id(Wc3ParticleBlendMode::Add);
        let multiply_pipeline = pipeline_id(Wc3ParticleBlendMode::Multiply);

        for (index, particle) in extracted.particles.iter().enumerate() {
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

fn batch_and_upload_wc3_particles(
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    extracted: Res<ExtractedWc3Particles>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut gpu: ResMut<Wc3ParticleGpuBuffer>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
) {
    let draw_function = draw_functions.read().id::<DrawWc3BillboardParticles>();
    gpu.instances.clear();

    for phase in phases.values_mut() {
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
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<Wc3ParticlePipeline>,
    extracted: Res<ExtractedWc3Particles>,
    images: Res<RenderAssets<GpuImage>>,
    fallback_image: Res<FallbackImage>,
    mut bind_groups: ResMut<Wc3ParticleTextureBindGroups>,
) {
    bind_groups.0.clear();
    let layout = pipeline_cache.get_bind_group_layout(pipeline.texture_layout());
    for (slab_index, textures) in extracted.slab_textures.iter().enumerate() {
        let Some(&entity) = extracted.slab_entities.get(slab_index) else {
            continue;
        };
        let bind_group = if pipeline.uses_binding_arrays() {
            let fallback = &fallback_image.d2;
            let mut texture_views = vec![&*fallback.texture_view; pipeline.texture_slab_size];
            let mut samplers = vec![&*fallback.sampler; pipeline.texture_slab_size];
            let mut ready = true;
            for (slot, texture) in textures.iter().copied().enumerate() {
                let Some(texture) = texture else {
                    continue;
                };
                let Some(image) = images.get(texture) else {
                    ready = false;
                    break;
                };
                texture_views[slot] = &*image.texture_view;
                samplers[slot] = &*image.sampler;
            }
            if !ready {
                continue;
            }
            render_device.create_bind_group(
                "wc3 particle texture array bind group",
                &layout,
                &BindGroupEntries::sequential((&texture_views[..], &samplers[..])),
            )
        } else {
            let fallback = &fallback_image.d2;
            let image = match textures.first().copied().flatten() {
                Some(texture) => {
                    let Some(image) = images.get(texture) else {
                        continue;
                    };
                    image
                }
                None => fallback,
            };
            render_device.create_bind_group(
                "wc3 particle texture bind group",
                &layout,
                &BindGroupEntries::sequential((&image.texture_view, &image.sampler)),
            )
        };
        bind_groups.0.insert(entity, bind_group);
    }
}

struct DrawWc3BillboardParticleCommand;

impl<P: PhaseItem> RenderCommand<P> for DrawWc3BillboardParticleCommand {
    type Param = (
        SRes<Wc3ParticleGpuBuffer>,
        SRes<Wc3ParticleTextureBindGroups>,
    );
    type ViewQuery = (Read<ViewUniformOffset>, Read<Wc3ParticleViewBindGroup>);
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        (view_uniform, view_bind_group): ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (gpu, texture_bind_groups): SystemParamItem<'w, '_, Self::Param>,
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

        pass.set_bind_group(0, &view_bind_group.0, &[view_uniform.offset]);
        pass.set_bind_group(1, texture_bind_group, &[]);
        pass.set_vertex_buffer(0, instance_buffer.slice(..));
        pass.draw(0..6, item.batch_range().clone());
        RenderCommandResult::Success
    }
}

type DrawWc3BillboardParticles = (SetItemPipeline, DrawWc3BillboardParticleCommand);
