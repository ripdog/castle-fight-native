#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import bevy_render::view::View

struct FogUniform {
    bounds: vec4<f32>,
};
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var<uniform> fog: FogUniform;
@group(0) @binding(3) var mask: texture_2d<f32>;
#ifdef MULTISAMPLED
@group(0) @binding(4) var depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(4) var depth: texture_depth_2d;
#endif

@group(0) @binding(5) var<uniform> view: View;

@fragment
fn fragment(input: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(scene, linear_sampler, input.uv);
    let pixel = vec2<i32>(input.position.xy);
    let z = textureLoad(depth, pixel, 0);
    if z <= 0.0 { return color; }
    let clip = vec4<f32>(input.uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), z, 1.0);
    let homogeneous = view.world_from_clip * clip;
    let world = homogeneous.xyz / homogeneous.w;
    let uv = (world.xz - fog.bounds.xy) * fog.bounds.zw;
    let coverage = textureSample(mask, linear_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0))).rg;
    // Explored fog retains dim terrain/remembered silhouettes; unexplored shroud is black.
    let light = mix(0.0, 0.32, coverage.g) + coverage.r * 0.68;
    return vec4<f32>(color.rgb * clamp(light, 0.0, 1.0), color.a);
}
