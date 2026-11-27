// The void's endless floor: a pale, polished surface like still water over white stone, that
// goes on for ever. Faint seams every few metres (fading with distance so they never shimmer),
// slow broad undulations in its sheen, and Bevy's lighting on top: the bright fog and sky
// reflect in it, things standing on it cast soft shadows and show as blurred reflections of the
// environment.

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, globals},
};

// x: seam spacing (m), y: seam strength, z: ripple strength, w: unused
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> floor_params: vec4<f32>;

fn hash2(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2(127.1, 311.7));
    let r = q + dot(q, q + 34.53);
    return fract(r.x * r.y);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash2(i), hash2(i + vec2(1.0, 0.0)), u.x), mix(hash2(i + vec2(0.0, 1.0)), hash2(i + vec2(1.0, 1.0)), u.x), u.y);
}

@fragment
fn fragment(vertex_output: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var in = vertex_output;
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let p = in.world_position.xz;
    let t = globals.time;
    let dist = length(in.world_position.xyz - view.world_position.xyz);

    // seams: thin lines every `spacing` metres, anti-aliased by their screen width, fading out
    // with distance
    let spacing = floor_params.x;
    let g = abs(fract(p / spacing - 0.5) - 0.5) * spacing;
    let w = fwidth(p) * 1.2 + vec2(0.004);
    let line = 1.0 - min(smoothstep(vec2(0.0), w, g).x, smoothstep(vec2(0.0), w, g).y);
    let seams = line * floor_params.y * (1.0 - smoothstep(10.0, 70.0, dist));

    // slow, broad undulations in the surface (a still film of water over stone)
    let k = 0.35;
    let e = 0.05;
    let hx = noise((p + vec2(e, 0.0)) * k + vec2(t * 0.05, t * 0.03)) - noise((p - vec2(e, 0.0)) * k + vec2(t * 0.05, t * 0.03));
    let hz = noise((p + vec2(0.0, e)) * k + vec2(t * 0.05, t * 0.03)) - noise((p - vec2(0.0, e)) * k + vec2(t * 0.05, t * 0.03));
    let ripple = floor_params.z * (1.0 - smoothstep(20.0, 120.0, dist));
    pbr_input.N = normalize(vec3(-hx / (2.0 * e) * ripple, 1.0, -hz / (2.0 * e) * ripple));

    // a faint pearly variation in the stone itself
    let tint = 0.96 + 0.04 * noise(p * 0.07);
    var base = pbr_input.material.base_color;
    base = vec4(base.rgb * tint * (1.0 - seams), base.a);
    pbr_input.material.base_color = base;
    pbr_input.material.perceptual_roughness = mix(0.07, 0.2, seams);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
