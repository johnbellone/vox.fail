// The world's block faces, packed: 20 bytes a vertex instead of 64. The position comes as
// usual; the atlas coordinates as two 16-bit fractions; and one number holds which of the six
// faces this is (bits 0-2, in the mesher's FACES order) and the corner's shading (bits 8-15).
// The normal and tangent are the face's, rebuilt here, and the shading becomes the vertex
// colour the standard material multiplies in, as before.

#import bevy_pbr::{
    mesh_functions,
    forward_io::VertexOutput,
    view_transformations::position_world_to_clip,
}

struct BlockVertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
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
    out.uv = v.uv;
    out.color = vec4<f32>(shade, shade, shade, 1.0);
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = v.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(v.instance_index, world_from_local[3]);
#endif
    return out;
}
