// A doorway to somewhere else: the echo of the place beyond (rendered by its own camera into
// a texture the size of the screen) shown where the doorway is, by screen position, so it
// looks like a hole into the other place. Far off it's faint, pale and wavering, like a
// memory; close up it's solid. Its edge shimmers and glows; the picture ripples a little near
// the edge.
//
// What it shows about where it leads (ADR 0004 decision 11) changes its look, in colour and in
// motion, so nobody needs to tell the colours apart: open and unchecked look as above; waking
// has slow rings rippling out from the middle, quickening; unlinked is a still cold grey with a
// violet edge and a fine static; restricted is amber, pulsing slowly, the picture faint behind.
// A change blends over half a second (the blend is worked out on the CPU).

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
// x: the state now, y: the state before, z: how far the change has blended (0-1), w: how long
// the state has been as it is, s. States: 0 unchecked, 1 open, 2 waking, 3 unlinked, 4 restricted
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var<uniform> look: vec4<f32>;

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

// One state's look, given the open look (`open`, with its alpha) and where this is: `uv`, `e`
// the distance to the edge (in heights), `t` the time, `since` how long the state has held.
fn state_look(code: f32, open: vec4<f32>, uv: vec2<f32>, aspect: f32, e: f32, t: f32, since: f32, presence: f32) -> vec4<f32> {
    let s = u32(round(code));
    let edge = 1.0 - smoothstep(0.0, 0.03, e);
    let inside = smoothstep(0.0, 0.05, e);
    // from the middle, even all round (in heights)
    let r = length(vec2((uv.x - 0.5) * aspect, uv.y - 0.5));
    let shown = 0.35 + 0.65 * presence;
    if s == 2u {
        // waking: rings ripple out from the middle, slow at first and quickening as it comes up
        let speed = 0.6 + min(since, 20.0) * 0.08;
        let rings = 0.5 + 0.5 * sin(r * 34.0 - t * speed * 6.2831);
        let soft = smoothstep(0.2, 0.9, rings);
        let c = mix(vec3(0.55, 0.68, 0.9), vec3(0.92, 0.96, 1.0), soft);
        let rim = vec3(0.85, 0.92, 1.0) * edge * (0.8 + 0.2 * soft);
        return vec4(c * inside * 0.75 + rim, clamp((0.35 + 0.35 * soft) * inside * shown + edge * presence, 0.0, 1.0));
    }
    if s == 3u {
        // unlinked: still and cold, a fine static where the picture was, a faint violet edge
        let grain = hash2(floor(uv * vec2(aspect, 1.0) * 140.0) + floor(t * 10.0));
        let c = vec3(0.42, 0.43, 0.46) + (grain - 0.5) * 0.08;
        let rim = vec3(0.6, 0.45, 0.85) * edge * 0.7;
        return vec4(c * inside + rim, clamp(0.6 * inside * shown + edge * 0.6 * presence, 0.0, 1.0));
    }
    if s == 4u {
        // restricted: amber, a slow pulse, the picture faint behind
        let pulse = 0.5 + 0.5 * sin(t * 1.6);
        let amber = vec3(1.0, 0.68, 0.22);
        let c = mix(open.rgb * 0.35, amber, 0.55 + 0.15 * pulse);
        let rim = amber * edge * (0.9 + 0.6 * pulse);
        return vec4(c * inside + rim, clamp((0.45 + 0.15 * pulse) * inside * shown + edge * presence, 0.0, 1.0));
    }
    // open, or not checked yet: today's look
    return open;
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

    let open = vec4(c * fade_in + rim_c, clamp(presence * (0.55 + 0.45 * solid) * fade_in + rim * presence, 0.0, 1.0));
    // what it shows about where it leads, blended from what it showed before
    let now_look = state_look(look.x, open, uv, aspect, e, t, look.w, presence);
    let was_look = state_look(look.y, open, uv, aspect, e, t, look.w + 0.5, presence);
    var out = mix(was_look, now_look, clamp(look.z, 0.0, 1.0));
    @if(TONEMAP_IN_SHADER)
    out = tone_mapping(out, view.color_grading);
    return out;
}
