// Flames on crossed quads: noise rising through a flame-shaped mask, flickering, hottest
// (white-yellow) low in the middle, orange, then red at the ragged tips. Drawn added onto
// what's behind, bright enough for the bloom to make a glow round it.

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
    let uv = in.uv;
    var seed = 0.0;
    @if(VERTEX_COLORS)
    seed = in.color.r * 37.0;
    let t = globals.time + seed * 3.0;
    // noise streaming upwards, faster near the top
    let p = vec2(uv.x * 2.6 + seed, uv.y * 1.8 - t * 1.7);
    let n = fbm(p);
    let n2 = fbm(p * 2.3 + vec2(5.2, 1.3) - vec2(0.0, t * 0.9));
    // the flame leans and licks: its middle wanders with height
    // the wind's share along this quad leans the flame over, more towards its tip
    var along = 0.0;
    @if(VERTEX_TANGENTS)
    along = dot(params.yz, normalize(in.world_tangent.xz));
    let lean = clamp(along * 0.09, -0.7, 0.7) * pow(uv.y, 1.5);
    let x = (uv.x - 0.5) * 2.0 + (n - 0.5) * 0.9 * uv.y + sin(t * (2.3 + abs(along) * 0.4) + uv.y * 3.0) * 0.08 * uv.y - lean * 2.0;
    let width = mix(0.8, 0.04, pow(clamp(uv.y, 0.0, 1.0), 0.75));
    let body = 1.0 - smoothstep(width * 0.45, width, abs(x));
    let top = 1.0 - smoothstep(0.35 + 0.45 * n2, 0.98, uv.y);
    let root = smoothstep(0.0, 0.07, uv.y);
    let a = clamp(body * top * root * (0.55 + 0.9 * n), 0.0, 1.0);
    // heat: most in the low middle
    let heat = clamp(a * (1.25 - uv.y * 0.95), 0.0, 1.0);
    var c = mix(vec3(0.55, 0.04, 0.0), vec3(1.0, 0.38, 0.03), smoothstep(0.05, 0.45, heat));
    c = mix(c, vec3(1.0, 0.86, 0.5), smoothstep(0.5, 0.95, heat));
    // a slow overall flicker
    let flicker = 0.85 + 0.15 * noise(vec2(t * 5.0, seed));
    return vec4(c * a * params.x * flicker, a);
}
