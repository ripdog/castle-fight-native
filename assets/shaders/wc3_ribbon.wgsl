// Vertex stage for WC3 ribbon trails. A trail's points live in a shared storage buffer, so its
// mesh is a static strip whose UVs encode (point index, side); the StandardMaterial fragment
// stage shades the result. Slot positions are already in world space, so the entity transform
// (which only places the trail for transparent sorting) is not applied.
//
// Slot layout, in vec4s from the mesh tag:
//   0: point count, left u, right u, first v
//   1: tint rgb, last v
//   2 + 2p: top position of point p, point alpha
//   3 + 2p: bottom position of point p
#import bevy_pbr::{
    forward_io::{Vertex, VertexOutput},
    mesh_functions,
    view_transformations::position_world_to_clip,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<storage, read> ribbon_data: array<vec4<f32>>;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    let base = mesh_functions::get_tag(vertex.instance_index);
    let header = ribbon_data[base];
    let tint = ribbon_data[base + 1u];
    let count = u32(header.x);
    // Strip vertices past the trail's end collapse onto its last point, leaving zero-area
    // triangles.
    let point = min(u32(vertex.uv.x), max(count, 1u) - 1u);
    let bottom = u32(vertex.uv.y);
    let top_sample = ribbon_data[base + 2u + point * 2u];
    let world_position = select(top_sample.xyz, ribbon_data[base + 3u + point * 2u].xyz, bottom == 1u);
    let last = max(f32(count) - 1.0, 1.0);

    var out: VertexOutput;
    out.world_position = vec4<f32>(world_position, 1.0);
    out.position = position_world_to_clip(world_position);
    out.world_normal = vec3<f32>(0.0, 0.0, 1.0);
#ifdef VERTEX_UVS_A
    out.uv = vec2<f32>(select(header.y, header.z, bottom == 1u), header.w + f32(point) / last * (tint.w - header.w));
#endif
#ifdef VERTEX_COLORS
    out.color = vec4<f32>(tint.rgb, top_sample.w);
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex.instance_index,
        mesh_functions::get_world_from_local(vertex.instance_index)[3],
    );
#endif
    return out;
}
