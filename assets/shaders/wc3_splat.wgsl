#import bevy_pbr::mesh_functions::{
    get_tag,
    get_world_from_local,
    mesh_position_local_to_clip,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<storage, read> splat_data: array<vec4<f32>>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var splat_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var splat_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> blend_mode: u32;

struct VertexInput {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vertex(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let slot = get_tag(input.instance_index);
    let base = slot * 2u;
    let color = splat_data[base];
    let uv_rect = splat_data[base + 1u];
    let world_from_local = get_world_from_local(input.instance_index);

    output.position = mesh_position_local_to_clip(
        world_from_local,
        vec4<f32>(input.position, 1.0),
    );
    output.uv = mix(uv_rect.xy, uv_rect.zw, input.uv);
    output.color = color;
    return output;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(splat_texture, splat_sampler, input.uv);
    let color = texel * input.color;
    if blend_mode == 1u {
        return vec4<f32>(color.rgb * color.a, 0.0);
    }
    return color;
}
