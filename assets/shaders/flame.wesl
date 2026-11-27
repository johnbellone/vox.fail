// Flames on crossed quads: noise rising through a flame-shaped mask, flickering, hottest
// (white-yellow) low in the middle, orange, then red at the ragged tips. Drawn added onto
// what's behind, bright enough for the bloom to make a glow round it.
//
// No two fires burn alike: each one's seed (the vertex colour's red, from its block) sets how
// fast it rises, how tall and wide it stands and how fast it flickers, and slow noise makes it
// surge and settle by itself. The two crossed planes of one fire show different flames.

import bevy_pbr::render::{
    forward_io::VertexOutput,
    mesh_view_bindings::globals,
};

// x: brightness, y, z: the wind across x and z (m/s): the flames lean with it
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: vec4<f32>;

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

// A number 0-1 from a fire's seed, a different one for each k.
fn trait_of(seed: f32, k: f32) -> f32 {
    return fract(sin(seed * 12.9898 + k * 78.233) * 43758.5453);
}

fn fbm(p0: vec2<f32>) -> f32 {
    var p = p0;
    var s = 0.0;
    var a = 0.5;
    for (var i = 0; i < 4; i++) {
        s += a * noise(p);
        p = p * 2.1 + vec2(3.1, 1.7);
        a *= 0.5;
    }
    return s;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var r = 0.0;
    @if(VERTEX_COLORS)
    r = in.color.r;
    let seed = r * 37.0;
    // this fire's own character
    let rise = mix(1.3, 2.2, trait_of(r, 1.0));
    // (tall times surge stays under 1: the flame never reaches the top of its quad and is cut off)
    let tall = mix(0.8, 0.95, trait_of(r, 2.0));
    let wide = mix(0.8, 1.25, trait_of(r, 3.0));
    let flick = mix(3.2, 7.5, trait_of(r, 4.0));
    let sway = mix(1.6, 3.4, trait_of(r, 5.0));
    let t = globals.time + seed * 3.0;
    // gusts: a slow wander (a few seconds) that lifts the fire and brightens it, then lets it
    // sink back
    let gust = noise(vec2(t * mix(0.25, 0.6, trait_of(r, 6.0)), seed * 1.7));
    let surge = mix(0.92, 1.05, gust);
    // the fire's height: a taller fire reaches higher up the quad
    let uv = vec2(in.uv.x, in.uv.y / (tall * surge));
    // which of the two crossed planes this is: each shows its own noise
    var plane = 0.0;
    @if(VERTEX_TANGENTS)
    plane = step(0.0, in.world_tangent.x * in.world_tangent.z) * 13.7;
    // noise streaming upwards, faster near the top
    let p = vec2(uv.x * 2.6 + seed + plane, uv.y * 1.8 - t * rise);
    let n = fbm(p);
    let n2 = fbm(p * 2.3 + vec2(5.2, 1.3) - vec2(0.0, t * rise * 0.53));
    // the flame leans and licks: its middle wanders with height
    // the wind's share along this quad leans the flame over, more towards its tip
    var along = 0.0;
    @if(VERTEX_TANGENTS)
    along = dot(params.yz, normalize(in.world_tangent.xz));
    let lean = clamp(along * 0.09, -0.7, 0.7) * pow(clamp(uv.y, 0.0, 1.0), 1.5);
    let x = (uv.x - 0.5) * 2.0 / wide + (n - 0.5) * 0.9 * uv.y + sin(t * (sway + abs(along) * 0.4) + uv.y * 3.0 + plane) * 0.08 * uv.y - lean * 2.0;
    let width = mix(0.8, 0.04, pow(clamp(uv.y, 0.0, 1.0), 0.75));
    let body = 1.0 - smoothstep(width * 0.45, width, abs(x));
    let top = 1.0 - smoothstep(0.35 + 0.45 * n2, 0.98, uv.y);
    let root = smoothstep(0.0, 0.07, uv.y);
    let a = clamp(body * top * root * (0.55 + 0.9 * n), 0.0, 1.0);
    // heat: most in the low middle
    let heat = clamp(a * (1.25 - uv.y * 0.95), 0.0, 1.0);
    var c = mix(vec3(0.55, 0.04, 0.0), vec3(1.0, 0.38, 0.03), smoothstep(0.05, 0.45, heat));
    c = mix(c, vec3(1.0, 0.86, 0.5), smoothstep(0.5, 0.95, heat));
    // its own flicker, and brighter while a gust lifts it
    let flicker = (0.85 + 0.15 * noise(vec2(t * flick, seed))) * mix(0.9, 1.1, gust);
    return vec4(c * a * params.x * flicker, a);
}
