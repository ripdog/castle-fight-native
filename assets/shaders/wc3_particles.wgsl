#import bevy_render::view::View

@group(0) @binding(0) var<uniform> view: View;
#ifdef PARTICLE_BINDING_ARRAY
@group(#{PARTICLE_TEXTURE_GROUP}) @binding(0) var particle_textures: binding_array<texture_2d<f32>, #{PARTICLE_TEXTURE_SLAB_SIZE}>;
@group(#{PARTICLE_TEXTURE_GROUP}) @binding(1) var particle_samplers: binding_array<sampler, #{PARTICLE_TEXTURE_SLAB_SIZE}>;
#else
@group(#{PARTICLE_TEXTURE_GROUP}) @binding(0) var particle_texture: texture_2d<f32>;
@group(#{PARTICLE_TEXTURE_GROUP}) @binding(1) var particle_sampler: sampler;
#endif

struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @location(0) position_scale: vec4<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv_rect: vec4<f32>,
    @location(3) texture_slot: u32,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) texture_slot: u32,
};

fn quad_corner(vertex_index: u32) -> vec2<f32> {
    switch vertex_index {
        case 0u: { return vec2(-0.5, -0.5); }
        case 1u: { return vec2( 0.5, -0.5); }
        case 2u: { return vec2( 0.5,  0.5); }
        case 3u: { return vec2(-0.5, -0.5); }
        case 4u: { return vec2( 0.5,  0.5); }
        default: { return vec2(-0.5,  0.5); }
    }
}

fn quad_uv(vertex_index: u32) -> vec2<f32> {
    switch vertex_index {
        case 0u: { return vec2(0.0, 1.0); }
        case 1u: { return vec2(1.0, 1.0); }
        case 2u: { return vec2(1.0, 0.0); }
        case 3u: { return vec2(0.0, 1.0); }
        case 4u: { return vec2(1.0, 0.0); }
        default: { return vec2(0.0, 0.0); }
    }
}

@vertex
fn vertex(input: VertexInput) -> VertexOutput {
    let corner = quad_corner(input.vertex_index) * input.position_scale.w;
    let right = view.world_from_view[0].xyz;
    let up = view.world_from_view[1].xyz;
    let world_position = input.position_scale.xyz + right * corner.x + up * corner.y;

    var output: VertexOutput;
    output.position = view.clip_from_world * vec4(world_position, 1.0);
    output.uv = mix(input.uv_rect.xy, input.uv_rect.zw, quad_uv(input.vertex_index));
    output.color = input.color;
    output.texture_slot = input.texture_slot;
    return output;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
#ifdef PARTICLE_BINDING_ARRAY
    let texel = textureSample(
        particle_textures[input.texture_slot],
        particle_samplers[input.texture_slot],
        input.uv,
    );
#else
    let texel = textureSample(particle_texture, particle_sampler, input.uv);
#endif
    let color = texel.rgb * input.color.rgb;
    let alpha = texel.a * input.color.a;
#ifdef PARTICLE_BLEND_ADD
    return vec4(color * alpha, 0.0);
#else ifdef PARTICLE_BLEND_MULTIPLY
    return vec4(color * alpha, alpha);
#else
    return vec4(color, alpha);
#endif
}
