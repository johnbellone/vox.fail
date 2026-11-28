// Fluid in a jar (crates/vox-world/src/fluid.rs): the jar's inside is a see-through layer, and
// everything of it above the surface is cut away, so the glass shows a waterline. The surface is
// a plane in world space through a point the app moves with the jar, tilted by the app's slosh
// spring, with small ripples on it: tip the head and the fluid stays level; nod and it rocks.
//
// Straight on, the fluid is nearly clear (the face behind it must read); at a slant it thickens
// to its colour, as a jar of liquid does at its edges. A bright band marks the surface where it
// meets the glass, and a few bubbles rise through it.

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
};

// the calm surface's centre (world, xyz), the ripples' height (w, m)
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> surface: vec4<f32>;
// the surface's slope (x: rise per metre of world x, y: per metre of world z), the jar's scale
// (z), the waves' clock (w, s)
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> slosh: vec4<f32>;

fn hash1(n: f32) -> f32 {
    return fract(sin(n * 127.1) * 43758.5453);
}

// The surface's height above the calm centre at d (world offset from it, m).
fn level(d: vec3<f32>, t: f32, k: f32) -> f32 {
    let x = d.x / k;
    let z = d.z / k;
    let ripple = 0.55 * sin(x * 38.0 + t * 3.1) + 0.3 * sin(z * 31.0 - t * 2.4 + 1.3) + 0.25 * sin((x + z) * 23.0 + t * 4.3 + 0.7);
    return slosh.x * d.x + slosh.y * d.z + surface.w * ripple;
}

// A bubble at q (m, along the glass and up from the calm surface), 0..1: its rim.
fn bubbles(q: vec2<f32>, t: f32) -> f32 {
    let cell = 0.045;
    let c = floor(q.x / cell);
    let h = hash1(c);
    if h < 0.55 {
        return 0.0;
    }
    let h2 = hash1(c + 17.3);
    let h3 = hash1(c + 41.9);
    // a rise of 0.45 m in 3 to 6 s, then round again
    let y = -0.45 + fract(t / (3.0 + 3.0 * h2) + h3) * 0.45;
    let x = (c + 0.25 + 0.5 * h3) * cell + 0.004 * sin(t * 5.0 + h * 20.0);
    let r = 0.0035 + 0.003 * h2;
    let dist = length(q - vec2(x, y));
    return smoothstep(r, r * 0.6, dist) * (0.35 + 0.65 * smoothstep(r * 0.4, r * 0.9, dist));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let k = max(slosh.z, 1e-3);
    let t = slosh.w;
    let d = in.world_position.xyz - surface.xyz;
    let below = level(d, t, k) - d.y;
    if below < 0.0 {
        discard;
    }
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let n = normalize(in.world_normal);
    let ndv = abs(dot(n, pbr_input.V));
    // thin straight on, thick at a slant
    var colour = pbr_input.material.base_color;
    var alpha = mix(0.22, 0.7, pow(1.0 - ndv, 1.6));
    // a little deeper colour lower down
    alpha += 0.06 * smoothstep(0.05 * k, 0.4 * k, below);
    // the meniscus: a bright line where the surface meets the glass
    let edge = 1.0 - smoothstep(0.002 * k, 0.008 * k, below);
    colour = vec4(mix(colour.rgb, vec3(0.92, 1.0, 1.0), 0.7 * edge), 1.0);
    alpha = max(alpha, 0.75 * edge);
    // bubbles, on the glass: along it is world x on faces looking along z, world z on the others
    let along = select(d.z, d.x, abs(n.z) > abs(n.x));
    let b = bubbles(vec2(along / k, d.y / k), t);
    colour = vec4(mix(colour.rgb, vec3(0.95, 1.0, 1.0), b), 1.0);
    alpha = max(alpha, 0.7 * b);
    colour.a = clamp(alpha, 0.0, 0.9);
    pbr_input.material.base_color = colour;
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
