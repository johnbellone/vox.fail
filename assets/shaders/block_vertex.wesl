// The world's block faces, packed: 20 bytes a vertex instead of 64. The position comes as
// usual; the corner of the face's tile in the atlas as two 16-bit fractions; and one number
// holds which of the six faces this is (bits 0-2, in the mesher's FACES order), the corner's
// shading (bits 8-15) and where the vertex is across the face, in blocks (bits 16-21 along u,
// 22-27 along v: a merged face spans several). The normal and tangent are the face's, rebuilt
// here; the shading becomes the vertex colour; `uv` carries the place across the face and
// `uv_b` the tile, for block.wesl to repeat the tile across the face.

import bevy_pbr::render::{
    mesh_functions,
    forward_io::VertexOutput,
    view_transformations::position_world_to_clip,
};

struct BlockVertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) tile: vec2<f32>,
    @location(2) face: u32,
};

// +X, -X, +Z, -Z, +Y, -Y: the outward normal
fn face_normal(f: u32) -> vec3<f32> {
    switch f {
        case 0u: { return vec3(1.0, 0.0, 0.0); }
        case 1u: { return vec3(-1.0, 0.0, 0.0); }
        case 2u: { return vec3(0.0, 0.0, 1.0); }
        case 3u: { return vec3(0.0, 0.0, -1.0); }
        case 4u: { return vec3(0.0, 1.0, 0.0); }
        default: { return vec3(0.0, -1.0, 0.0); }
    }
}

// the direction of +u on the texture, and the handedness (w): as the mesher sets them
fn face_tangent(f: u32) -> vec4<f32> {
    switch f {
        case 0u: { return vec4(0.0, 0.0, -1.0, -1.0); }
        case 1u: { return vec4(0.0, 0.0, 1.0, -1.0); }
        case 2u: { return vec4(1.0, 0.0, 0.0, -1.0); }
        case 3u: { return vec4(-1.0, 0.0, 0.0, -1.0); }
        case 4u: { return vec4(1.0, 0.0, 0.0, -1.0); }
        default: { return vec4(1.0, 0.0, 0.0, -1.0); }
    }
}

@vertex
fn vertex(v: BlockVertex) -> VertexOutput {
    var out: VertexOutput;
    let f = v.face & 7u;
    let shade = f32((v.face >> 8u) & 255u) / 255.0;
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(face_normal(f), v.instance_index);
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, face_tangent(f), v.instance_index);
    out.uv = vec2<f32>(f32((v.face >> 16u) & 63u), f32((v.face >> 22u) & 63u));
    out.uv_b = v.tile;
    out.color = vec4<f32>(shade, shade, shade, 1.0);
    @if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = v.instance_index;
    @if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(v.instance_index, world_from_local[3]);
    return out;
}
