//! Frame-rate independent cosmetic smoothing over the simulation's binary team visibility.
use crate::bridge::PresentationSamples;
use bevy::{
    core_pipeline::{
        FullscreenShader,
        prepass::{DepthPrepass, ViewPrepassTextures},
        schedule::{Core3d, Core3dSystems},
    },
    image::{ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        extract_component::{
            ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
            UniformComponentPlugin,
        },
        render_asset::RenderAssets,
        render_resource::{
            binding_types::{
                sampler, texture_2d, texture_depth_2d, texture_depth_2d_multisampled,
                uniform_buffer,
            },
            *,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        texture::GpuImage,
        view::{ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};
use castle_fight_sim::SUBUNITS_PER_WORLD_UNIT;

pub(crate) struct FogPlugin;

impl Plugin for FogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FogAnimation>().add_plugins((
            ExtractComponentPlugin::<FogUniform>::default(),
            UniformComponentPlugin::<FogUniform>::default(),
            ExtractComponentPlugin::<FogMask>::default(),
        ));
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .add_systems(RenderStartup, setup_pipeline)
                .add_systems(Core3d, composite_fog.in_set(Core3dSystems::PostProcess));
        }
    }
}

#[derive(Component, Clone, Copy, ExtractComponent, ShaderType)]
pub(crate) struct FogUniform {
    /// Origin x/z, inverse width/depth in world coordinates.
    bounds: Vec4,
}

#[derive(Component, Clone, ExtractComponent)]
struct FogMask(Handle<Image>);

#[derive(Resource, Default)]
pub(crate) struct FogAnimation {
    mask: Handle<Image>,
    layout: Option<(
        usize,
        usize,
        crate::bridge::ObserverVision,
        i32,
        castle_fight_sim::SimPoint,
    )>,
    revision: u64,
    tick: u64,
    target: Vec<Vec2>,
    horizontal: Vec<Vec2>,
    smoothed: Vec<Vec2>,
}

const FADE_SECONDS: f32 = 0.22;
const BLUR_WEIGHTS: [f32; 5] = [1.0 / 16.0, 4.0 / 16.0, 6.0 / 16.0, 4.0 / 16.0, 1.0 / 16.0];

pub(crate) fn update_fog(
    mut commands: Commands,
    time: Res<Time>,
    samples: Res<PresentationSamples>,
    mut animation: ResMut<FogAnimation>,
    mut images: ResMut<Assets<Image>>,
    cameras: Query<Entity, With<crate::presentation::RtsCamera>>,
) {
    let (Some(fog), Some(observer)) = (samples.current.fog.as_ref(), samples.current.observer)
    else {
        for entity in &cameras {
            commands.entity(entity).remove::<FogUniform>();
        }
        return;
    };
    let layout = (fog.width, fog.height, observer, fog.cell_size, fog.origin);
    let reset = animation.layout != Some(layout)
        || samples.current.tick < animation.tick
        || samples.revision() < animation.revision;
    if reset {
        animation.layout = Some(layout);
        let len = fog.width * fog.height;
        animation.target.resize(len, Vec2::ZERO);
        animation.horizontal.resize(len, Vec2::ZERO);
        animation.smoothed.resize(len, Vec2::ZERO);
        let mut image = Image::new_fill(
            Extent3d {
                width: fog.width as u32,
                height: fog.height as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[0, 0],
            TextureFormat::Rg8Unorm,
            default(),
        );
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
        if let Some(mut previous) = images.get_mut(&animation.mask) {
            *previous = image;
        } else {
            animation.mask = images.add(image);
        }
    }
    if reset || samples.revision() != animation.revision {
        let team = usize::from(observer.team.0);
        for (index, value) in animation.target.iter_mut().enumerate() {
            *value = Vec2::new(
                f32::from(fog.visible[team][index]),
                f32::from(fog.explored[team][index]),
            );
        }
        let FogAnimation {
            target, horizontal, ..
        } = &mut *animation;
        blur(target, horizontal, fog.width, fog.height, true);
        blur(horizontal, target, fog.width, fog.height, false);
        if reset {
            animation.smoothed = animation.target.clone();
        }
        animation.revision = samples.revision();
        animation.tick = samples.current.tick;
    }
    let weight = fade_weight(time.delta_secs());
    let FogAnimation {
        target, smoothed, ..
    } = &mut *animation;
    for (current, target) in smoothed.iter_mut().zip(target.iter()) {
        *current = current.lerp(*target, weight);
    }
    if let Some(mut image) = images.get_mut(&animation.mask)
        && let Some(data) = &mut image.data
    {
        for (pixel, value) in data
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(&animation.smoothed)
        {
            pixel[0] = (value.x.clamp(0.0, 1.0) * 255.0).round() as u8;
            pixel[1] = (value.y.max(value.x).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    let scale = SUBUNITS_PER_WORLD_UNIT as f32;
    let size = fog.cell_size as f32 / scale;
    for entity in &cameras {
        commands.entity(entity).insert((
            DepthPrepass,
            FogMask(animation.mask.clone()),
            FogUniform {
                bounds: Vec4::new(
                    fog.origin.x as f32 / scale,
                    fog.origin.y as f32 / scale,
                    1.0 / (size * fog.width as f32),
                    1.0 / (size * fog.height as f32),
                ),
            },
        ));
    }
}

fn fade_weight(seconds: f32) -> f32 {
    1.0 - (-seconds.max(0.0) / FADE_SECONDS).exp()
}

fn blur(input: &[Vec2], output: &mut [Vec2], width: usize, height: usize, horizontal: bool) {
    for y in 0..height {
        for x in 0..width {
            let mut sum = Vec2::ZERO;
            for (tap, weight) in BLUR_WEIGHTS.iter().enumerate() {
                let offset = tap as isize - 2;
                let sx = if horizontal {
                    (x as isize + offset).clamp(0, width as isize - 1) as usize
                } else {
                    x
                };
                let sy = if horizontal {
                    y
                } else {
                    (y as isize + offset).clamp(0, height as isize - 1) as usize
                };
                sum += input[sy * width + sx] * *weight;
            }
            output[y * width + x] = sum;
        }
    }
}

#[derive(Resource)]
struct FogPipeline {
    layout: BindGroupLayoutDescriptor,
    pipelines: [CachedRenderPipelineId; 2],
    sampler: Sampler,
}

fn setup_pipeline(
    mut commands: Commands,
    device: Res<RenderDevice>,
    assets: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
    cache: Res<PipelineCache>,
) {
    let mut layouts = Vec::new();
    let pipelines = [false, true].map(|multisampled| {
        let depth = if multisampled {
            texture_depth_2d_multisampled()
        } else {
            texture_depth_2d()
        };
        let layout = BindGroupLayoutDescriptor::new(
            "fog_composite",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    uniform_buffer::<FogUniform>(true),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    depth,
                    uniform_buffer::<ViewUniform>(true),
                ),
            ),
        );
        layouts.push(layout.clone());
        cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("fog_composite".into()),
            layout: vec![layout],
            vertex: fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: assets.load("shaders/fog_of_war.wgsl"),
                shader_defs: if multisampled {
                    vec!["MULTISAMPLED".into()]
                } else {
                    Vec::new()
                },
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::Rgba8UnormSrgb,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        })
    });
    // Multisampled depth needs its own layout descriptor as well as its own shader variant.
    commands.insert_resource(FogPipeline {
        layout: layouts.remove(0),
        pipelines,
        sampler: device.create_sampler(&SamplerDescriptor {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
    });
    commands.insert_resource(MultisampledFogLayout(layouts.remove(0)));
}

#[derive(Resource)]
struct MultisampledFogLayout(BindGroupLayoutDescriptor);

type FogView = (
    &'static ViewTarget,
    &'static DynamicUniformIndex<FogUniform>,
    &'static FogMask,
    &'static ViewPrepassTextures,
    &'static Msaa,
    &'static ViewUniformOffset,
);

fn composite_fog(
    view: ViewQuery<FogView>,
    pipelines: (Option<Res<FogPipeline>>, Option<Res<MultisampledFogLayout>>),
    cache: Res<PipelineCache>,
    bindings: (
        Res<ComponentUniforms<FogUniform>>,
        Res<ViewUniforms>,
        Res<RenderAssets<GpuImage>>,
    ),
    mut ctx: RenderContext,
) {
    let (pipeline, multisampled) = pipelines;
    let (uniforms, views, images) = bindings;
    let Some(pipeline) = pipeline else {
        return;
    };
    let (target, uniform_index, mask, prepass, msaa, view_offset) = view.into_inner();
    let index = usize::from(msaa.samples() > 1);
    let Some(render_pipeline) = cache.get_render_pipeline(pipeline.pipelines[index]) else {
        return;
    };
    let Some(uniforms) = uniforms.uniforms().binding() else {
        return;
    };
    let Some(view_binding) = views.uniforms.binding() else {
        return;
    };
    let Some(mask) = images.get(&mask.0) else {
        return;
    };
    let Some(depth) = prepass.depth_view() else {
        return;
    };
    let layout = if index == 0 {
        &pipeline.layout
    } else {
        let Some(multisampled) = multisampled.as_ref() else {
            return;
        };
        &multisampled.0
    };
    let post = target.post_process_write();
    let bindings = ctx.render_device().create_bind_group(
        "fog_composite",
        &cache.get_bind_group_layout(layout),
        &BindGroupEntries::sequential((
            post.source,
            &pipeline.sampler,
            uniforms,
            &mask.texture_view,
            depth,
            view_binding,
        )),
    );
    let mut pass = ctx
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("fog_composite"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: post.destination,
                depth_slice: None,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    pass.set_pipeline(render_pipeline);
    pass.set_bind_group(0, &bindings, &[uniform_index.index(), view_offset.offset]);
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fade_is_continuous_reversible_and_frame_rate_independent() {
        let a = 0.0_f32
            .lerp(1.0, fade_weight(0.1))
            .lerp(1.0, fade_weight(0.1));
        assert!((a - fade_weight(0.2)).abs() < 0.000001);
        let veiling = a.lerp(0.0, fade_weight(0.1));
        assert!(veiling > 0.0 && veiling < a);
        assert_eq!(fade_weight(0.0), 0.0);
    }
    #[test]
    fn edges_are_smooth_and_uniform_fields_do_not_darken_at_map_border() {
        let mut input = vec![Vec2::ONE; 49];
        let mut output = vec![Vec2::ZERO; 49];
        blur(&input, &mut output, 7, 7, true);
        assert!(output.iter().all(|v| *v == Vec2::ONE));
        input.fill(Vec2::ZERO);
        input[24] = Vec2::ONE;
        blur(&input, &mut output, 7, 7, true);
        assert!(output[24].x > output[23].x && output[23].x > output[22].x && output[22].x > 0.0);
    }
    #[test]
    #[ignore = "requires a Vulkan/GL rendering device; renders offscreen without a display"]
    fn gpu_composite_renders_shroud_fog_and_visible_terrain() {
        for msaa in [Msaa::Off, Msaa::Sample4] {
            render_gpu_fog(msaa);
        }
    }

    fn render_gpu_fog(msaa: Msaa) {
        use bevy::{
            camera::RenderTarget,
            render::{
                RenderPlugin,
                view::screenshot::{Screenshot, ScreenshotCaptured},
            },
            window::ExitCondition,
            winit::WinitPlugin,
        };
        #[derive(Resource, Default)]
        struct Capture(Option<Image>);
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: crate::terrain::client_asset_root()
                        .to_string_lossy()
                        .into_owned(),
                    ..default()
                })
                .set(RenderPlugin {
                    synchronous_pipeline_compilation: true,
                    ..default()
                })
                .disable::<WinitPlugin>(),
        )
        .add_plugins(FogPlugin)
        .init_resource::<Capture>();
        app.finish();
        app.cleanup();
        let mut mask = Image::new_fill(
            Extent3d {
                width: 4,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[0, 0],
            TextureFormat::Rg8Unorm,
            default(),
        );
        mask.data = Some(vec![0, 0, 0, 255, 255, 255, 255, 255]);
        let (target, mask) = {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            (
                images.add(Image::new_target_texture(
                    128,
                    128,
                    TextureFormat::Rgba8UnormSrgb,
                    None,
                )),
                images.add(mask),
            )
        };
        let projection = Projection::Orthographic(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::Fixed {
                width: 4.0,
                height: 4.0,
            },
            area: Rect::from_corners(Vec2::splat(-2.0), Vec2::splat(2.0)),
            ..OrthographicProjection::default_3d()
        });
        let transform = Transform::from_xyz(0.0, 5.0, 0.0).looking_at(Vec3::ZERO, Vec3::NEG_Z);
        app.world_mut().spawn((
            Camera3d::default(),
            msaa,
            DepthPrepass,
            projection.clone(),
            transform,
            RenderTarget::Image(target.clone().into()),
            FogMask(mask),
            FogUniform {
                bounds: Vec4::new(-2.0, -2.0, 0.25, 0.25),
            },
        ));
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Plane3d::default().mesh().size(8.0, 8.0));
        let material = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color: Color::WHITE,
                unlit: true,
                ..default()
            });
        app.world_mut()
            .spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::default()));
        for _ in 0..8 {
            app.update();
        }
        app.world_mut().spawn(Screenshot::image(target)).observe(
            |event: On<ScreenshotCaptured>, mut capture: ResMut<Capture>| {
                capture.0 = Some(event.image.clone());
            },
        );
        for _ in 0..100 {
            app.update();
            if app.world().resource::<Capture>().0.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let capture = app
            .world()
            .resource::<Capture>()
            .0
            .as_ref()
            .expect("GPU screenshot must complete");
        let pixel = |x: usize| capture.data.as_ref().unwrap()[(64 * 128 + x) * 4];
        assert!(
            pixel(12) < pixel(48) && pixel(48) < pixel(112),
            "shroud < explored fog < visible: {} {} {}",
            pixel(12),
            pixel(48),
            pixel(112)
        );
        capture
            .clone()
            .try_into_dynamic()
            .unwrap()
            .save(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(if msaa == Msaa::Off {
                    "../../target/fog-gpu-verification.png"
                } else {
                    "../../target/fog-gpu-msaa-verification.png"
                }),
            )
            .unwrap();
    }
}
