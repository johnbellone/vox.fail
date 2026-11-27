// A doorway to somewhere else: the echo of the place beyond (rendered by its own camera into
// a texture the size of the screen) shown where the doorway is, by screen position, so it
// looks like a hole into the other place. Far off it's faint, pale and wavering, like a
// memory; close up it's solid. Its edge shimmers and glows; the picture ripples a little near
// the edge.

import bevy_pbr::render::{
    forward_io::VertexOutput,
    mesh_view_bindings::{view, globals},
};
@if(TONEMAP_IN_SHADER)
import bevy_core_pipeline::tonemapping::tone_mapping;

// x: how present (0-1), y: the echo is ready (0/1), z: width / height, w: a seed
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var echo_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var echo_smp: sampler;

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
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let presence = params.x;
    let ready = params.y;
    let aspect = params.z;
    let t = globals.time + params.w;
    let uv = in.uv;
    // distance to the edge, in units of the doorway's height (so it's even all round)
    let ex = min(uv.x, 1.0 - uv.x) * aspect;
    let ey = min(uv.y, 1.0 - uv.y);
    let e = min(ex, ey);
    // a ragged edge that breathes: the echo reaches further in as you come closer
    let ragged = noise(vec2(uv.x * aspect * 9.0 + t * 0.4, uv.y * 9.0 - t * 0.3)) * 0.05;
    let fade_in = smoothstep(0.0, 0.07 + (1.0 - presence) * 0.15, e - ragged * (1.0 - presence * 0.6));

    // the echo, by where this is on screen; rippling near the edge
    let screen = (in.position.xy - view.viewport.xy) / view.viewport.zw;
    let edge_k = 1.0 - smoothstep(0.0, 0.2, e);
    let wob = vec2(sin(uv.y * 31.0 + t * 2.1), cos(uv.x * 27.0 * aspect - t * 1.7)) * (0.002 + 0.006 * edge_k) * (1.4 - presence);
    var c = textureSample(echo_tex, echo_smp, clamp(screen + wob, vec2(0.0), vec2(1.0))).rgb * ready;
    // not loaded yet: a soft pale glow
    let glow = vec3(0.9, 0.92, 0.98) * 0.6;
    c = mix(glow, c, ready);

    // ghostly far off: washed out and pale; full colour close up
    let solid = smoothstep(0.45, 1.0, presence);
    let grey = dot(c, vec3(0.2126, 0.7152, 0.0722));
    let pale = mix(vec3(grey), c, 0.3) * 0.85 + vec3(0.06, 0.065, 0.075);
    c = mix(pale, c, solid);
    // faint drifting bands, like heat haze, while it's still an echo
    let bands = noise(vec2(uv.x * aspect * 3.0, uv.y * 14.0 - t * 0.6));
    c *= 1.0 + (bands - 0.5) * 0.25 * (1.0 - solid);

    // the rim: thin, bright, pulsing slowly
    let rim = (1.0 - smoothstep(0.0, 0.025, e)) * (0.7 + 0.3 * sin(t * 1.3 + (uv.x + uv.y) * 9.0));
    let rim_c = vec3(1.0, 0.96, 0.88) * rim * (0.6 + 1.6 * presence);

    var out = vec4(c * fade_in + rim_c, clamp(presence * (0.55 + 0.45 * solid) * fade_in + rim * presence, 0.0, 1.0));
    @if(TONEMAP_IN_SHADER)
    out = tone_mapping(out, view.color_grading);
    return out;
}
