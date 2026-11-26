// The world's sky, drawn on a dome that follows the camera. Its styles (misc.w):
//
// - earth: blue overhead fading to the horizon's colour (warm at dusk), the ground's colour
//   below, the sun's disc and glow, clouds drifting high overhead (lit from the sun's side),
//   stars and the moon at night.
// - void: fog going on for ever in every direction, drifting in slow banks, with three
//   moons (each a lit sphere with craters, a terminator and a halo) veiled behind it.
// - orbit: 250 km up. Black space, stars and the Milky Way, a hard white sun; below, the
//   planet itself (a sphere, so its rim is 15.8° under eye level and curved), turning slowly
//   underneath: oceans with the sun's glint, land, clouds, city lights on the night side, the
//   atmosphere a blue haze towards the rim and a thin bright line above it.
//
// Colours come from `outdoors.rs`, which draws the same sky (without the small detail) into
// the environment map that lights the world.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::{view, globals},
}

struct Sky {
    sun: vec4<f32>,
    zenith: vec4<f32>,
    horizon: vec4<f32>,
    ground: vec4<f32>,
    sun_colour: vec4<f32>,
    // x: night, y: cloud cover, z: cloud brightness, w: style (0 earth, 1 fog (islands, sea),
    // 2 the luminous void, 3 orbit)
    misc: vec4<f32>,
    // direction + angular radius; colour + glow
    moon_a: vec4<f32>,
    moon_b: vec4<f32>,
    moon_c: vec4<f32>,
    moon_a_colour: vec4<f32>,
    moon_b_colour: vec4<f32>,
    moon_c_colour: vec4<f32>,
    // x, z: the wind the clouds drift on (m/s); w: how grey the clouds are (rain)
    wind: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> sky: Sky;

fn hash2(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2(123.34, 456.21));
    let r = q + dot(q, q + 45.32);
    return fract(r.x * r.y);
}

fn hash3(p: vec3<f32>) -> f32 {
    let q = fract(p * vec3(0.1031, 0.1030, 0.0973));
    let r = q + dot(q, q.yxz + 33.33);
    return fract((r.x + r.y) * r.z);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2(1.0, 0.0));
    let c = hash2(i + vec2(0.0, 1.0));
    let d = hash2(i + vec2(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn noise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(mix(hash3(i), hash3(i + vec3(1.0, 0.0, 0.0)), u.x), mix(hash3(i + vec3(0.0, 1.0, 0.0)), hash3(i + vec3(1.0, 1.0, 0.0)), u.x), u.y);
    let b = mix(mix(hash3(i + vec3(0.0, 0.0, 1.0)), hash3(i + vec3(1.0, 0.0, 1.0)), u.x), mix(hash3(i + vec3(0.0, 1.0, 1.0)), hash3(i + vec3(1.0, 1.0, 1.0)), u.x), u.y);
    return mix(a, b, u.z);
}

fn fbm(p0: vec2<f32>) -> f32 {
    var p = p0;
    var s = 0.0;
    var a = 0.5;
    for (var i = 0; i < 5; i++) {
        s += a * noise(p);
        p = p * 2.03 + vec2(1.7, 9.2);
        a *= 0.5;
    }
    return s;
}

fn fbm3(p0: vec3<f32>) -> f32 {
    var p = p0;
    var s = 0.0;
    var a = 0.5;
    for (var i = 0; i < 5; i++) {
        s += a * noise3(p);
        p = p * 2.02 + vec3(1.7, 9.2, 4.1);
        a *= 0.5;
    }
    return s;
}

// Stars: one candidate per cell of a grid on the sky, drawn as a soft round point.
fn stars(dir: vec3<f32>, density: f32, scale: f32) -> vec3<f32> {
    let p = dir * scale;
    let cell = floor(p);
    let h = hash3(cell);
    if h < 1.0 - density {
        return vec3(0.0);
    }
    let centre = cell + vec3(hash3(cell + 7.1), hash3(cell + 3.3), hash3(cell + 9.9));
    let d = length(p - centre);
    let k = (h - (1.0 - density)) / density;
    let tw = 0.7 + 0.3 * sin(globals.time * (1.5 + k * 7.0) + h * 50.0);
    // white, a few blue, a few orange
    let tint = mix(vec3(1.0, 0.75, 0.55), vec3(0.7, 0.8, 1.0), hash3(cell + 1.7));
    return tint * smoothstep(0.32, 0.0, d) * (0.6 + 3.0 * k * k) * tw;
}

// A moon: a lit sphere with craters, a soft halo. Returns the colour to add and how much of
// what's behind it the disc covers.
fn moon(dir: vec3<f32>, m: vec4<f32>, col: vec4<f32>, seed: f32) -> vec4<f32> {
    if m.w <= 0.0 {
        return vec4(0.0);
    }
    let md = normalize(m.xyz);
    let r = m.w;
    let cosd = clamp(dot(dir, md), -1.0, 1.0);
    let ang = acos(cosd);
    var add = col.rgb * col.w * (0.05 * exp(-ang / (r * 2.2)) + 0.012 * exp(-ang / (r * 9.0)));
    if ang >= r * 1.02 {
        return vec4(add, 0.0);
    }
    // where on the disc, -1..1
    let side = normalize(cross(md, vec3(0.0, 1.0, 0.0)));
    let up = cross(side, md);
    let off = dir - md * cosd;
    let p = vec2(dot(off, side), dot(off, up)) / sin(r);
    let z = sqrt(max(1.0 - dot(p, p), 0.0));
    let n = vec3(p, z);
    // lit from one side: a gibbous moon with a soft terminator
    let light = normalize(vec3(-0.55, 0.35, 0.75));
    let lit = smoothstep(-0.05, 0.25, dot(n, light));
    // maria and craters
    let q = n * 2.6 + vec3(seed);
    let maria = smoothstep(0.45, 0.7, fbm3(q));
    var crater = 0.0;
    for (var i = 0; i < 3; i++) {
        let f = fract(n.xy * (3.0 + f32(i) * 2.7) + seed * 0.37 + f32(i) * 0.21) - 0.5;
        crater += smoothstep(0.22, 0.12, length(f)) * smoothstep(0.05, 0.14, length(f)) * 0.25;
    }
    let albedo = (1.0 - 0.35 * maria) * (1.0 - crater * 0.6) * (0.85 + 0.15 * fbm3(q * 4.0));
    let limb = 0.75 + 0.25 * z;
    let surface = col.rgb * albedo * limb * (0.03 + 1.5 * lit);
    let edge = smoothstep(r * 1.02, r * 0.98, ang);
    return vec4(add * (1.0 - edge) + surface * edge, edge);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dir = normalize(in.world_position.xyz - view.world_position.xyz);
    let sun = normalize(sky.sun.xyz);
    let night = sky.misc.x;
    let void_sky = sky.misc.w > 0.5 && sky.misc.w < 1.5;
    let luminous = sky.misc.w > 1.5 && sky.misc.w < 2.5;
    let orbit = sky.misc.w > 2.5;
    var planet = false;
    let e = dir.y;

    var c: vec3<f32>;
    if e >= 0.0 {
        c = mix(sky.horizon.rgb, sky.zenith.rgb, pow(e, 0.45));
    } else {
        c = mix(sky.horizon.rgb, sky.ground.rgb, clamp(-e * 5.0, 0.0, 1.0));
    }

    if orbit {
        // the planet (radius 1) seen from 250 km up (6371 km radius): where does the look hit it?
        let h = 1.0 + 250.0 / 6371.0;
        let b = h * e;
        let disc = b * b - (h * h - 1.0);
        let towards = max(dot(dir, sun), 0.0);
        let day_here = 1.0 - night;
        if disc >= 0.0 && b < 0.0 {
            planet = true;
            let t = -b - sqrt(disc);
            let n = normalize(vec3(0.0, h, 0.0) + dir * t);
            // the ground slides by beneath (once round in 90 minutes, sped up tenfold so it's
            // seen to move); land and cloud are read from the turned position
            let a = globals.time * 0.0116;
            let q = vec3(n.x, n.y * cos(a) - n.z * sin(a), n.y * sin(a) + n.z * cos(a));
            let land = smoothstep(0.53, 0.57, fbm3(q * 3.1 + vec3(2.0, 5.0, 1.0)));
            let ocean = vec3(0.012, 0.04, 0.11);
            let ground = mix(vec3(0.11, 0.095, 0.06), vec3(0.05, 0.085, 0.035), fbm3(q * 11.0));
            let swirl = fbm3(q * 4.0 + 3.0);
            let cloud = smoothstep(0.48, 0.72, fbm3(q * vec3(6.0, 10.0, 6.0) + swirl * 1.8));
            let ndl = dot(n, sun);
            let day = smoothstep(-0.12, 0.22, ndl);
            var col = mix(mix(ocean, ground, land), vec3(0.92, 0.93, 0.95), cloud * 0.92) * (0.02 + 1.4 * day * max(ndl, 0.15));
            // the sun's glint off open water
            let glint = pow(max(dot(reflect(dir, n), sun), 0.0), 300.0) * (1.0 - land) * (1.0 - cloud) * day;
            col += sky.sun_colour.rgb * glint * 4.0;
            // cities on the night side, in clusters on land
            let cell = floor(q * 260.0);
            let city = land * (1.0 - cloud) * (1.0 - day) * step(0.975, hash3(cell)) * smoothstep(0.45, 0.7, fbm3(q * 20.0));
            col += vec3(1.0, 0.62, 0.28) * city * 0.35;
            // looking through more air towards the rim: blue haze, and the sunset's orange
            // along the line between day and night
            let graze = pow(1.0 - clamp(-dot(dir, n), 0.0, 1.0), 3.0);
            let dusk = smoothstep(0.25, 0.0, abs(ndl)) * smoothstep(-0.2, 0.05, ndl);
            col = mix(col, vec3(0.25, 0.45, 0.95) * (0.08 + 0.92 * day) + vec3(0.6, 0.25, 0.05) * dusk * 0.5, graze * 0.75);
            c = col;
        } else {
            c = sky.zenith.rgb;
            // stars, dense and steady (no air to make them twinkle), and the Milky Way's band
            c += stars(dir, 0.025, 420.0) * 0.7 + stars(dir, 0.006, 140.0) * 1.2;
            let galaxy = normalize(vec3(0.35, 0.62, -0.7));
            let band = exp(-abs(dot(dir, galaxy)) * 7.0);
            c += vec3(0.05, 0.045, 0.06) * band * (0.3 + fbm3(dir * 7.0)) * (0.4 + 0.6 * night);
            // the rim of the atmosphere: a thin bright line just above the planet's edge,
            // brightest on the sun's side
            let above = e - (-sqrt(1.0 - 1.0 / (h * h)));
            let flat_dir = normalize(vec3(dir.x, 0.0, dir.z) + vec3(1e-4, 0.0, 0.0));
            let flat_sun = normalize(vec3(sun.x, 0.0, sun.z) + vec3(1e-4, 0.0, 0.0));
            let sunside = 0.25 + 0.75 * smoothstep(-0.6, 0.8, dot(flat_dir, flat_sun));
            c += sky.horizon.rgb * (exp(-above / 0.006) * 1.6 + exp(-above / 0.03) * 0.3) * sunside;
            // the sun: a hard white disc and a little glare (no air to spread it)
            c += sky.sun_colour.rgb * day_here * (pow(towards, 2000.0) * 3.0 + pow(towards, 80.0) * 0.05);
            let sun_disc = smoothstep(0.99990, 0.99995, towards);
            c += sky.sun_colour.rgb * sun_disc * 60.0 * day_here;
        }
    } else if luminous {
        // the luminous void: white light everywhere, warmest and brightest at eye level, a
        // little cooler overhead, pale below; soft slow veils of brighter and dimmer mist
        if e >= 0.0 {
            c = mix(sky.horizon.rgb, sky.zenith.rgb, smoothstep(0.0, 1.0, e));
        } else {
            c = mix(sky.horizon.rgb, sky.ground.rgb, smoothstep(0.0, 0.5, -e));
        }
        let drift = vec3(globals.time * 0.006, 0.0, globals.time * 0.004);
        let veil = fbm3(vec3(dir.x * 1.6, dir.y * 6.0, dir.z * 1.6) + drift);
        let glow = exp(-abs(e) * 5.0);
        c *= 0.93 + 0.1 * veil;
        c += sky.horizon.rgb * 0.12 * glow * (0.5 + veil);
    } else if void_sky {
        // fog without end: darker overhead, brightest in a band at eye level where the
        // moonlight lies in it, a sea of fog below
        if e >= 0.0 {
            c = mix(sky.horizon.rgb, sky.zenith.rgb, smoothstep(0.0, 0.9, e));
        } else {
            c = mix(sky.horizon.rgb, sky.ground.rgb, smoothstep(0.0, 0.6, -e));
        }
        // strata: noise squashed flat so the fog lies in layers, warped by a second noise so the
        // layers curl, both drifting slowly
        let drift = vec3(globals.time * 0.009, 0.0, globals.time * 0.006);
        let warp = fbm3(dir * 3.0 + drift * 0.7);
        let strata = fbm3(vec3(dir.x * 2.2, dir.y * 11.0, dir.z * 2.2) + vec3(warp * 1.6) + drift);
        let wisp = fbm3(dir * 7.0 - drift * 1.6 + 11.0);
        c *= 0.72 + 0.5 * strata * (0.6 + 0.8 * wisp);
        // where the moonlight lies thickest
        c += sky.horizon.rgb * 0.35 * exp(-abs(e) * 7.0) * (0.6 + 0.8 * strata);
    } else {
        // the sun's glow and disc (the disc about half a degree across)
        let towards = max(dot(dir, sun), 0.0);
        c += sky.sun_colour.rgb * (1.0 - night) * (pow(towards, 10.0) * 0.35 + pow(towards, 300.0) * 2.0);
        let disc = smoothstep(0.99990, 0.99996, towards) * step(-0.02, sun.y);
        c += sky.sun_colour.rgb * disc * 40.0;
        // stars, only above the horizon
        if night > 0.01 && e > 0.0 {
            c += stars(dir, 0.01, 300.0) * night * smoothstep(0.0, 0.2, e);
        }
        // clouds on a plane far overhead, drifting with the wind
        let cover = sky.misc.y;
        if e > 0.0 && cover > 0.0 {
            // drifting on the wind (a cloud deck far up moves slowly across the sky)
            let uv = dir.xz / (e + 0.12) * 0.9 + (vec2(0.004, 0.0015) + sky.wind.xz * 0.0016) * globals.time;
            let n = fbm(uv);
            let density = smoothstep(1.0 - cover * 0.85 - 0.2, 1.0 - cover * 0.85 + 0.25, n) * smoothstep(0.0, 0.18, e);
            let lit = 0.7 + 0.5 * pow(towards, 3.0);
            let base = mix(sky.horizon.rgb, vec3(1.0), 0.75) * sky.misc.z * (1.0 - 0.55 * sky.wind.w);
            let cloud = mix(base * lit, sky.sun_colour.rgb * lit * 1.3, 0.3 * (1.0 - night)) * (1.0 - 0.3 * smoothstep(0.55, 0.95, n));
            c = mix(c, cloud, density * 0.92);
        }
    }

    // the moons, in front of everything else in the sky
    for (var i = 0; i < 3; i++) {
        var m = sky.moon_a;
        var col = sky.moon_a_colour;
        if i == 1 {
            m = sky.moon_b;
            col = sky.moon_b_colour;
        } else if i == 2 {
            m = sky.moon_c;
            col = sky.moon_c_colour;
        }
        // the planet hides what's behind it
        if planet {
            break;
        }
        let mo = moon(dir, m, col, f32(i) * 3.7 + 1.3);
        // in the void the fog veils them: faint discs in a glow
        let veil = select(select(1.0, 0.12, void_sky), 0.18, luminous);
        c = mix(c, c * (1.0 - mo.a) + mo.rgb, veil);
        // a corona round each moon where the fog scatters its light
        if void_sky && m.w > 0.0 {
            let ang = acos(clamp(dot(dir, normalize(m.xyz)), -1.0, 1.0));
            c += col.rgb * col.w * (0.05 * exp(-ang / (m.w * 1.6)) + 0.02 * exp(-ang / (m.w * 6.0)));
        }
    }
    return vec4(c, 1.0);
}
