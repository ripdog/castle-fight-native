@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<storage, read> rect_data: array<vec4<f32>>;

struct VertexInput {
    @location(0) position: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vertex(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let rect_index = u32(input.position.z + 0.5);
    let rect_count = u32(rect_data[0].x + 0.5);
    if rect_index >= rect_count {
        output.position = vec4<f32>(2.0, 2.0, 0.0, 1.0);
        output.color = vec4<f32>(0.0);
        return output;
    }

    let geometry = rect_data[1u + rect_index * 2u];
    let color = rect_data[2u + rect_index * 2u];
    let clip_xy = mix(geometry.xy, geometry.zw, input.position.xy);
    output.position = vec4<f32>(clip_xy, 0.0, 1.0);
    output.color = color;
    return output;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
