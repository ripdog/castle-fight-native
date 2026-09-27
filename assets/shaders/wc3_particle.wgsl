#import bevy_pbr::mesh_functions::{
    get_tag,
    get_world_from_local,
    mesh_position_local_to_clip,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<storage, read> particle_data: array<vec4<f32>>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var particle_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var particle_sampler: sampler;

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
    let color = particle_data[base];
    let uv_rect = particle_data[base + 1u];
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
    let texel = textureSample(particle_texture, particle_sampler, input.uv);
    let alpha = texel.a * input.color.a;
    // AlphaMode::Add uses Bevy's premultiplied-alpha blend state. Emit the
    // premultiplied additive contribution directly and zero source alpha.
    return vec4<f32>(texel.rgb * input.color.rgb * alpha, 0.0);
}
