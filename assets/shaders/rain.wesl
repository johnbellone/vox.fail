// Rain (or snow) round the camera, drawn on three see-through cylinders at different
// distances so it has depth: streaks falling at their own speed, slanting with the wind,
// thicker the harder it rains; or flakes drifting and swaying. Each column of drops falls at
// its own speed, and the rain comes in sheets: heavier and lighter curtains drifting past.

import bevy_pbr::render::{
    forward_io::VertexOutput,
    mesh_view_bindings::globals,
};

// x: how hard (0-1), y: snow (0 or 1), z, w: the wind across x and z (m/s)
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: vec4<f32>;

fn hash1(x: f32) -> f32 {
    return fract(sin(x * 127.1) * 43758.5453);
}

// Smooth noise along one line, 0-1.
fn noise1(x: f32) -> f32 {
    let i = floor(x);
    let f = fract(x);
    return mix(hash1(i), hash1(i + 1.0), f * f * (3.0 - 2.0 * f));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // uv: x = metres round the cylinder, y = metres up it; colour red = which cylinder (0-1)
    let layer = in.color.r;
    let t = globals.time;
    let n = normalize(in.world_normal);
    let side = normalize(cross(vec3(0.0, 1.0, 0.0), n));
    let across = dot(params.zw, side.xz);
    let snow = params.y > 0.5;
    let fall = select(9.0, 1.1, snow);
    // the wind pushes the drops sideways as they fall
    let x = in.uv.x + in.uv.y * across / fall;
    // columns of drops, more of them the harder it rains
    let per_m = mix(4.0, 18.0, params.x) * (1.0 + layer);
    let col = floor(x * per_m);
    let fx = fract(x * per_m) - 0.5;
    let seed = hash1(col + layer * 91.0);
    // each column its own speed (bigger drops fall faster; snow drifts at different rates)
    let y = in.uv.y + t * fall * mix(0.8, 1.2, hash1(col * 1.7 + layer * 13.0 + 5.0));
    if seed > 0.35 + 0.6 * params.x {
        return vec4(0.0);
    }
    var a = 0.0;
    if snow {
        let cell = 0.6;
        let cy = (y + seed * 37.0) / cell;
        let fy = fract(cy) - 0.5;
        let sway = sin(t * mix(0.9, 1.8, hash1(seed * 7.0 + floor(cy))) + seed * 20.0 + floor(cy)) * 0.18;
        let d = length(vec2((fx + sway) * 1.0, fy * per_m * cell));
        a = smoothstep(0.32, 0.12, d) * 0.85;
    } else {
        let len = 0.55;
        let gap = 2.2 + seed * 3.0;
        let fy = fract((y + seed * 50.0) / gap);
        let streak = smoothstep(0.0, 0.06, fy) * (1.0 - smoothstep(len / gap * 0.6, len / gap, fy));
        a = streak * smoothstep(0.42, 0.05, abs(fx)) * 0.35;
    }
    // fade the top and bottom of the cylinder, and the nearest layer a little
    let edge = smoothstep(0.0, 3.0, in.uv.y) * (1.0 - smoothstep(13.0, 16.0, in.uv.y));
    a *= edge * mix(0.7, 1.0, layer) * clamp(params.x * 2.0, 0.25, 1.0);
    // sheets: curtains a few metres wide, heavier and lighter, carried along by the wind
    let sheet = noise1(in.uv.x * 0.18 - t * (0.25 + 0.05 * abs(across)) + layer * 17.0);
    a *= mix(0.45, 1.3, sheet);
    let c = select(vec3(0.75, 0.8, 0.88), vec3(1.0), snow);
    return vec4(c * a, a);
}
