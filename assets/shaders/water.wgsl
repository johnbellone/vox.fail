// Water: the surface moves in overlapping waves (worked out per pixel in world space, so
// neighbouring blocks of water join up), it reflects more the flatter you look across it
// (Fresnel), and the sky, sun and anything lit show in it through Bevy's lighting with the
// world's environment map.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::globals,
}

// x: wave height, y: wave speed, z, w: the wind's direction (the main waves run with it)
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> waves: vec4<f32>;

// The slope (d height / dx, d height / dz) of a sum of travelling waves at p (metres).
fn slope(p: vec2<f32>, t: f32) -> vec2<f32> {
    var g = vec2(0.0);
    // direction, wavelength (m), height (m), speed (m/s)
    let w = array<vec4<f32>, 5>(
        vec4(0.8, 0.6, 2.3, 0.022),
        vec4(-0.5, 0.86, 1.4, 0.014),
        vec4(0.2, -0.98, 0.9, 0.009),
        vec4(-0.94, -0.34, 0.55, 0.005),
        vec4(0.66, 0.75, 0.31, 0.003),
    );
    // turn the whole set so the first (longest) waves run with the wind
    var wd = waves.zw;
    if dot(wd, wd) < 1e-4 {
        wd = vec2(0.8, 0.6);
    }
    wd = normalize(wd);
    let rot = mat2x2<f32>(vec2(wd.x * 0.8 + wd.y * 0.6, -wd.x * 0.6 + wd.y * 0.8), vec2(wd.x * 0.6 - wd.y * 0.8, wd.x * 0.8 + wd.y * 0.6));
    for (var i = 0; i < 5; i++) {
        let d = normalize(rot * w[i].xy);
        let k = 6.2831853 / w[i].z;
        let c = sqrt(9.8 / k) * waves.y;
        let phase = k * dot(d, p) - k * c * t;
        g += d * (w[i].w * waves.x * k * cos(phase));
    }
    return g;
}

@fragment
fn fragment(vertex_output: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var in = vertex_output;
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let up = in.world_normal.y > 0.5;
    if up {
        let g = slope(in.world_position.xz, globals.time);
        pbr_input.N = normalize(vec3(-g.x, 1.0, -g.y));
    }
    let ndv = max(dot(pbr_input.N, pbr_input.V), 0.0);
    // Schlick's Fresnel for water (reflectance 2% straight down, all of it at a graze)
    let fresnel = 0.02 + 0.98 * pow(1.0 - ndv, 5.0);
    var colour = pbr_input.material.base_color;
    colour.a = clamp(mix(0.55, 1.0, fresnel), 0.0, 1.0);
    pbr_input.material.base_color = colour;
    pbr_input.material.perceptual_roughness = 0.035;
    pbr_input.material.reflectance = vec3(0.5);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
