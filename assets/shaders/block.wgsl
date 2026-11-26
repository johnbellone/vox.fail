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

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing, calculate_tbn_mikktspace, apply_normal_mapping},
    pbr_types,
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, globals},
}
#ifndef BINDLESS
#import bevy_pbr::pbr_bindings
#endif

// x: depth as a fraction of a block face, y: steps, z: tile size in pixels
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> params: vec4<f32>;
// atlas width, height, cell size and border, in pixels
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> atlas: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var height_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var height_smp: sampler;

fn depth_at(uv: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>) -> f32 {
    return 1.0 - textureSampleGrad(height_tex, height_smp, uv, ddx, ddy).r;
}

fn parallax(in: VertexOutput, uv: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>) -> vec2<f32> {
#ifdef VERTEX_TANGENTS
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
#else
    return uv;
#endif
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
#ifdef VERTEX_UVS_B
    // where in the tile: the place across the face (in blocks) wrapped, inside the tile's
    // rectangle; and how fast the unwrapped place changes, for every sample below (derivatives
    // first, while every pixel of the quad is still here: WebGPU insists)
    let ext = (params.z - 1.0) / atlas.xy;
    let ddx = dpdx(vertex_output.uv) * ext;
    let ddy = dpdy(vertex_output.uv) * ext;
    let wrapped = vertex_output.uv_b + fract(vertex_output.uv) * ext;
    in.uv = parallax(vertex_output, wrapped, ddx, ddy);
#endif
    var pbr_input = pbr_input_from_standard_material(in, is_front);
#ifdef VERTEX_UVS_B
#ifndef BINDLESS
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
#endif
#endif
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
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
