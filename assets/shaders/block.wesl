// The world's block faces: parallax occlusion mapping on the texture atlas.
//
// A face may span several blocks (the mesher merges neighbours with the same tile and light):
// the tile repeats across it, so the coordinates are wrapped into the tile here, and every
// texture is sampled with how fast the unwrapped coordinates change on screen. (Sampled with
// the wrapped ones, the jump where the tile repeats would pick the wrong detail level and draw
// a seam along every block's edge.)
//
// Each face's texture is moved along the view direction by its height map, marched in steps
// and refined between the last two, so mortar, cracks and the gaps between planks look sunken
// and hide behind the raised parts at a slant. The shifted coordinates stay inside the face's
// own tile in the atlas. Everything else (normal map, roughness, glow, lights, shadows, fog,
// reflections) is Bevy's standard material, sampled at the shifted point.
//
// Where the world has ground relief (docs/design/ground-relief.md), the tops of natural blocks
// dip a few centimetres: the dips come from the world's seed and the place alone (a copy of
// vox_core::relief, version 1), tilt the surface's normal by their slope and darken the
// hollows. They fade out where a wall or block is near (the corner shading says so) and in the
// distance, where they would only shimmer.

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing, calculate_tbn_mikktspace, apply_normal_mapping},
    pbr_types,
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, globals},
};
@if(!BINDLESS)
import bevy_pbr::render::pbr_bindings;

// x: depth as a fraction of a block face, y: steps, z: tile size in pixels
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> params: vec4<f32>;
// atlas width, height, cell size and border, in pixels
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> atlas: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var height_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var height_smp: sampler;
// ground relief: x the deepest dip in blocks (0: none in this world), y the block size (m)
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var<uniform> relief: vec4<f32>;
// ground relief: the seed, the world's origin on the map (x, z blocks, two's complement), the
// version
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var<uniform> relief_at: vec4<u32>;

// ---- ground relief: vox_core::relief, version 1. The hashes must stay bit for bit the same
// as there (u32 arithmetic wraps in both); the blend is smooth floating point, within about
// 1/255 of the integer one there.

fn mix32(h0: u32) -> u32 {
    var h = h0;
    h ^= h >> 16u;
    h *= 0x7feb352du;
    h ^= h >> 15u;
    h *= 0x846ca68bu;
    h ^= h >> 16u;
    return h;
}

fn relief_lattice(seed: u32, o: u32, x: i32, z: i32) -> f32 {
    return f32(mix32(seed ^ mix32((bitcast<u32>(x) * 0x9e3779b1u) ^ mix32(bitcast<u32>(z) ^ (o * 0x632be5abu)))) >> 16u);
}

// Value noise at p (blocks, in this world) on a lattice `cell` blocks apart, 0-65535. The
// world's origin is a whole number of swales (vox_core::relief::Relief::from_settings makes it
// so), so it moves the lattice exactly.
fn relief_noise(seed: u32, o: u32, p: vec2<f32>, cell: i32) -> f32 {
    let q = p / f32(cell);
    let i = floor(q);
    let f = q - i;
    let u = f * f * (3.0 - 2.0 * f);
    let x0 = i32(i.x) + bitcast<i32>(relief_at.y) / cell;
    let z0 = i32(i.y) + bitcast<i32>(relief_at.z) / cell;
    let a = relief_lattice(seed, o, x0, z0);
    let b = relief_lattice(seed, o, x0 + 1, z0);
    let c = relief_lattice(seed, o, x0, z0 + 1);
    let d = relief_lattice(seed, o, x0 + 1, z0 + 1);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// How deep the ground dips at p (blocks), 0 (flat) to 1 (the deepest): rounded bowls where the
// noise is above 0.45 of its range.
fn relief_depth(p: vec2<f32>) -> f32 {
    let seed = relief_at.x;
    let v = (relief_noise(seed, 0u, p, 4) * 3.0 + relief_noise(seed, 1u, p, 1)) / (4.0 * 65535.0);
    let t = clamp((v - 0.45) / 0.55, 0.0, 1.0);
    return t * t;
}

fn depth_at(uv: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>) -> f32 {
    return 1.0 - textureSampleGrad(height_tex, height_smp, uv, ddx, ddy).r;
}

fn parallax(in: VertexOutput, uv: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>) -> vec2<f32> {
    @if(VERTEX_TANGENTS) {
    let size = atlas.xy;
    // which tile this face shows, and its rectangle in the atlas
    let cell = floor(uv * size / atlas.z);
    let lo = (cell * atlas.z + atlas.w + 0.5) / size;
    let hi = (cell * atlas.z + atlas.w + params.z - 0.5) / size;
    let face = hi - lo;

    let n = normalize(in.world_normal);
    let t = normalize(in.world_tangent.xyz);
    let b = cross(n, t) * in.world_tangent.w;
    let v = normalize(view.world_position.xyz - in.world_position.xyz);
    let vt = vec3(dot(v, t), dot(v, b), dot(v, n));
    if vt.z <= 0.02 {
        return uv;
    }
    // how deep this tile goes (its bump; fine-grained tiles like ash barely move)
    let tile_depth = textureSampleGrad(height_tex, height_smp, uv, ddx, ddy).g;
    // how far the texture slides for the full depth (shallow angles are capped so the
    // texture doesn't smear across the face)
    let shift = -vt.xy / max(vt.z, 0.3) * params.x * tile_depth * face;
    // fewer steps looking straight on, more at a slant
    let steps = mix(params.y, params.y * 0.4, vt.z);
    let step = 1.0 / steps;
    var depth = 0.0;
    var at = uv;
    var d = depth_at(at, ddx, ddy);
    var prev_at = uv;
    var prev_gap = d;
    for (var i = 0; i < 32; i++) {
        if f32(i) >= steps || depth >= d {
            break;
        }
        prev_at = at;
        prev_gap = d - depth;
        depth += step;
        at = clamp(uv + shift * depth, lo, hi);
        d = depth_at(at, ddx, ddy);
    }
    // between the last step above the surface and the first below it
    let gap = d - depth;
    let w = clamp(gap / (gap - prev_gap + 1e-5), 0.0, 1.0);
    return clamp(mix(at, prev_at, w), lo, hi);
    } @else {
    return uv;
    }
}

fn hash3(p: vec3<f32>) -> f32 {
    let q = fract(p * vec3(0.1031, 0.1030, 0.0973));
    let r = q + dot(q, q.yxz + 33.33);
    return fract((r.x + r.y) * r.z);
}

fn noise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(mix(hash3(i), hash3(i + vec3(1.0, 0.0, 0.0)), u.x), mix(hash3(i + vec3(0.0, 1.0, 0.0)), hash3(i + vec3(1.0, 1.0, 0.0)), u.x), u.y);
    let b = mix(mix(hash3(i + vec3(0.0, 0.0, 1.0)), hash3(i + vec3(1.0, 0.0, 1.0)), u.x), mix(hash3(i + vec3(0.0, 1.0, 1.0)), hash3(i + vec3(1.0, 1.0, 1.0)), u.x), u.y);
    return mix(a, b, u.z);
}

@fragment
fn fragment(vertex_output: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var in = vertex_output;
    // how much ground one pixel covers (taken here, while every pixel is still in step)
    let foot = max(length(dpdx(vertex_output.world_position.xyz)), length(dpdy(vertex_output.world_position.xyz)));
    // how fast the unwrapped place changes on screen (set below when the face has a tile)
    var ddx = vec2<f32>(0.0);
    var ddy = vec2<f32>(0.0);
    @if(VERTEX_UVS_B) {
    // where in the tile: the place across the face (in blocks) wrapped, inside the tile's
    // rectangle; and how fast the unwrapped place changes, for every sample below (derivatives
    // first, while every pixel of the quad is still here: WebGPU insists)
    let ext = (params.z - 1.0) / atlas.xy;
    ddx = dpdx(vertex_output.uv) * ext;
    ddy = dpdy(vertex_output.uv) * ext;
    let wrapped = vertex_output.uv_b + fract(vertex_output.uv) * ext;
    in.uv = parallax(vertex_output, wrapped, ddx, ddy);
    }
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    @if(VERTEX_UVS_B && !BINDLESS) {
    // sampled again with the smooth rates (the standard material used the wrapped ones)
    let m = pbr_bindings::material;
    let flags = pbr_input.material.flags;
    let uv = in.uv;
    if (flags & pbr_types::STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT) != 0u {
        pbr_input.material.base_color = m.base_color * in.color
            * textureSampleGrad(pbr_bindings::base_color_texture, pbr_bindings::base_color_sampler, uv, ddx, ddy);
    }
    if (flags & pbr_types::STANDARD_MATERIAL_FLAGS_EMISSIVE_TEXTURE_BIT) != 0u {
        let glow = textureSampleGrad(pbr_bindings::emissive_texture, pbr_bindings::emissive_sampler, uv, ddx, ddy);
        pbr_input.material.emissive = vec4(m.emissive.rgb * glow.rgb, m.emissive.a);
    }
    if (flags & pbr_types::STANDARD_MATERIAL_FLAGS_METALLIC_ROUGHNESS_TEXTURE_BIT) != 0u {
        let mr = textureSampleGrad(pbr_bindings::metallic_roughness_texture, pbr_bindings::metallic_roughness_sampler, uv, ddx, ddy);
        pbr_input.material.metallic = m.metallic * mr.b;
        pbr_input.material.perceptual_roughness = m.perceptual_roughness * mr.g;
    }
    let tbn = calculate_tbn_mikktspace(pbr_input.world_normal, in.world_tangent);
    let nt = textureSampleGrad(pbr_bindings::normal_map_texture, pbr_bindings::normal_map_sampler, uv, ddx, ddy).rgb;
    let double_sided = (flags & pbr_types::STANDARD_MATERIAL_FLAGS_DOUBLE_SIDED_BIT) != 0u;
    pbr_input.N = apply_normal_mapping(flags, tbn, double_sided, is_front, nt);
    }
    @if(VERTEX_UVS_B) {
    // ground relief, on the tops of blocks that have it (the tile data's blue)
    let dip = relief.x * textureSampleGrad(height_tex, height_smp, in.uv, ddx, ddy).b;
    if dip > 0.0 && vertex_output.world_normal.y > 0.5 {
        let size = max(relief.y, 1e-3);
        // gone where a pixel covers more than about a quarter of a block (it would shimmer),
        // and where the corner shading says a wall or block is close (ground meets it flat)
        let fade = 1.0 - smoothstep(0.08, 0.3, foot / size);
        let pin = smoothstep(0.82, 1.0, vertex_output.color.r);
        let k = dip * fade * pin;
        if k > 1e-4 {
            let p = in.world_position.xz / size;
            let e = 1.0 / 16.0;
            let d0 = relief_depth(p);
            let g = vec2(relief_depth(p + vec2(e, 0.0)) - d0, relief_depth(p + vec2(0.0, e)) - d0) / e;
            // the ground is k * depth below the top, so it leans away from where depth grows
            pbr_input.N = normalize(pbr_input.N + vec3(g.x, 0.0, g.y) * k);
            // and dust settles in the hollows: up to 18% darker at the deepest (k / relief.x is
            // this block's share of the world's deepest dip, faded and pinned)
            let dark = 0.18 * d0 * k / relief.x;
            pbr_input.material.base_color = vec4(pbr_input.material.base_color.rgb * (1.0 - dark), pbr_input.material.base_color.a);
        }
    }
    }
    pbr_input.material.base_color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);
    // molten glow (deep orange-red, unlike a lamp's warm white) breathes and flows slowly
    let e = pbr_input.material.emissive;
    if e.r > 0.01 && e.r > 3.5 * e.g {
        let q = in.world_position.xyz * 1.3 + vec3(0.0, -globals.time * 0.35, globals.time * 0.12);
        let k = 0.55 + 0.75 * noise3(q) + 0.15 * sin(globals.time * 1.7 + in.world_position.x);
        pbr_input.material.emissive = vec4(e.rgb * k, e.a);
    }
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
