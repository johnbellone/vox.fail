// Water: the surface moves in overlapping waves (worked out per pixel in world space, so
// neighbouring blocks of water join up), it reflects more the flatter you look across it
// (Fresnel), and the sky, sun and anything lit show in it through Bevy's lighting with the
// world's environment map.
//
// A world can also give its sea a swell (`"water": {"swell": metres}`): long waves that really
// lift and drop the surface (the vertex shader moves the top of every pool), with pale crests
// where they peak. Heights are worked out from world x and z alone, so neighbouring blocks,
// chunks and the fragment shader all agree.

#import bevy_pbr::{
    mesh_functions,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    mesh_view_bindings::globals,
    view_transformations::position_world_to_clip,
}

// x: wave height, y: wave speed, z, w: the wind's direction (the main waves run with it)
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> waves: vec4<f32>;
// x: the swell's height (m, calm surface to the highest crest; 0: none), y: how pale the crests
// go, z: how opaque the water is looking straight down (0-1), w: the block size (m), to find
// the tops of pools
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> swell: vec4<f32>;

// The wind's direction, as a rotation taking the waves' own frame to the world's.
fn wind_turn() -> mat2x2<f32> {
    var wd = waves.zw;
    if dot(wd, wd) < 1e-4 {
        wd = vec2(0.8, 0.6);
    }
    wd = normalize(wd);
    return mat2x2<f32>(vec2(wd.x * 0.8 + wd.y * 0.6, -wd.x * 0.6 + wd.y * 0.8), vec2(wd.x * 0.6 - wd.y * 0.8, wd.x * 0.8 + wd.y * 0.6));
}

// The swell at p (metres): height (x, m above the calm surface) and slope (y, z).
fn swell_at(p: vec2<f32>, t: f32) -> vec3<f32> {
    if swell.x <= 0.0 {
        return vec3(0.0);
    }
    // direction, wavelength (m), share of the height: long and slow, all roughly downwind
    let w = array<vec4<f32>, 4>(
        vec4(0.8, 0.6, 31.0, 0.38),
        vec4(0.97, 0.24, 19.0, 0.28),
        vec4(0.45, 0.89, 12.5, 0.2),
        vec4(0.96, -0.28, 7.5, 0.14),
    );
    let rot = wind_turn();
    var out = vec3(0.0);
    for (var i = 0; i < 4; i++) {
        let d = normalize(rot * w[i].xy);
        let k = 6.2831853 / w[i].z;
        let c = sqrt(9.8 / k);
        let phase = k * dot(d, p) - k * c * t;
        // sharper crests than troughs: lift the sine towards its peak
        let s = sin(phase);
        let a = w[i].w * swell.x;
        let h = a * (s + 0.35 * (s * s - 0.5));
        out.x += h;
        let dh = a * k * cos(phase) * (1.0 + 0.7 * s);
        out.y += d.x * dh;
        out.z += d.y * dh;
    }
    return out;
}

// The top of a pool: the mesher puts it 0.12 of a block below the block's top.
fn at_surface(y: f32) -> bool {
    let s = max(swell.w, 1e-3);
    return abs(fract(y / s + 0.001) - 0.881) < 0.02;
}

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    var wp = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0));
    if swell.x > 0.0 && at_surface(wp.y) {
        wp.y += swell_at(wp.xz, globals.time).x;
    }
    out.world_position = wp;
    out.position = position_world_to_clip(wp.xyz);
#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(v.normal, v.instance_index);
#endif
#ifdef VERTEX_UVS_A
    out.uv = v.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = v.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, v.tangent, v.instance_index);
#endif
#ifdef VERTEX_COLORS
    out.color = v.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = v.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(v.instance_index, world_from_local[3]);
#endif
    return out;
}

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
    let rot = wind_turn();
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
    var crest = 0.0;
    if up {
        let sw = swell_at(in.world_position.xz, globals.time);
        let g = slope(in.world_position.xz, globals.time) + sw.yz;
        pbr_input.N = normalize(vec3(-g.x, 1.0, -g.y));
        // pale where the swell peaks and the water is steep: wind-torn crests
        if swell.x > 0.0 {
            let high = smoothstep(0.45, 0.95, sw.x / swell.x);
            let steep = smoothstep(0.08, 0.3, length(sw.yz));
            let streak = 0.5 + 0.5 * sin(dot(in.world_position.xz, vec2(3.1, 1.7)) + sin(in.world_position.z * 2.3 + globals.time));
            crest = clamp(high * (0.55 + 0.45 * steep) * (0.6 + 0.4 * streak), 0.0, 1.0) * swell.y;
        }
    }
    let ndv = max(dot(pbr_input.N, pbr_input.V), 0.0);
    // Schlick's Fresnel for water (reflectance 2% straight down, all of it at a graze)
    let fresnel = 0.02 + 0.98 * pow(1.0 - ndv, 5.0);
    var colour = pbr_input.material.base_color;
    colour.a = clamp(mix(swell.z, 1.0, fresnel), 0.0, 1.0);
    colour = mix(colour, vec4(0.62, 0.68, 0.7, 0.95), crest);
    pbr_input.material.base_color = colour;
    pbr_input.material.perceptual_roughness = mix(0.035, 0.6, crest);
    pbr_input.material.reflectance = vec3(0.5);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
