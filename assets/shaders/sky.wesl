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

import bevy_pbr::render::{
    forward_io::VertexOutput,
    mesh_view_bindings::{view, globals},
};

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

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> sky: Sky;

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

// Stars in space: steady (no air to make them twinkle), each a soft point; the brightest
// (`spikes` > 0) with a cross of diffraction spikes. Colour by temperature: mostly white,
// some blue-white, some yellow and orange, a few red.
fn space_stars(dir: vec3<f32>, density: f32, scale: f32, gain: f32, spikes: f32) -> vec3<f32> {
    let p = dir * scale;
    let cell = floor(p);
    let h = hash3(cell);
    if h < 1.0 - density {
        return vec3(0.0);
    }
    let centre = cell + vec3(0.3) + 0.4 * vec3(hash3(cell + 7.1), hash3(cell + 3.3), hash3(cell + 9.9));
    // how far from the star across the sky (its direction, not the cell's corner, or stars
    // smear as the view turns)
    let o = (dir - normalize(centre)) * scale;
    // across the sky at this point (the star's own little plane)
    let t1 = normalize(cross(dir, vec3(0.0, 1.0, 0.0001)));
    let t2 = cross(dir, t1);
    let x = dot(o, t1);
    let y = dot(o, t2);
    let k = (h - (1.0 - density)) / density;
    let temp = hash3(cell + 1.7);
    var tint = vec3(1.0, 0.97, 0.94);
    if temp < 0.18 {
        tint = vec3(0.68, 0.78, 1.0);
    } else if temp < 0.32 {
        tint = vec3(1.0, 0.86, 0.62);
    } else if temp < 0.4 {
        tint = vec3(1.0, 0.62, 0.4);
    } else if temp < 0.43 {
        tint = vec3(1.0, 0.42, 0.3);
    }
    let bright = (0.35 + 2.6 * k * k) * gain;
    let d2 = x * x + y * y;
    // kept inside the star's own cell (anything wider is cut off at its edges)
    var v = exp(-d2 * 110.0) + 0.1 * exp(-d2 * 35.0);
    if spikes > 0.0 {
        v += spikes * k * (exp(-abs(x) * 45.0 - y * y * 30.0) + exp(-abs(y) * 45.0 - x * x * 30.0));
    }
    return tint * v * bright;
}

// A planet far off: a lit sphere (`n` its kind) with its own surface, phased by the sun, and
// for the ringed one its rings. Returns the colour and how much of what's behind it it covers.
// 0: a banded giant with a great storm, 1: a ringed giant, 2: a red desert world, 3: an ice world.
fn far_world(dir: vec3<f32>, at: vec3<f32>, r: f32, n: i32, sun: vec3<f32>) -> vec4<f32> {
    let c = normalize(at);
    let cosd = clamp(dot(dir, c), -1.0, 1.0);
    let ang = acos(cosd);
    // the ring plane, tilted, for the ringed giant
    let ring_n = normalize(vec3(0.25, 0.9, 0.35));
    var ring = vec4(0.0);
    if n == 1 && ang < r * 2.6 {
        let t = dot(c, ring_n) / dot(dir, ring_n);
        let rho = length(dir * t - c) / sin(r);
        if t > 0.0 && rho > 1.35 && rho < 2.35 {
            let bands = 0.55 + 0.45 * sin(rho * 38.0) * sin(rho * 11.0 + 1.0);
            let gap = smoothstep(0.02, 0.06, abs(rho - 1.92));
            // lit on the face the sun shines on, dimmer seen from the other side
            let lit = 0.25 + 0.75 * abs(dot(ring_n, sun));
            ring = vec4(vec3(0.85, 0.76, 0.58) * bands * lit * 0.9, 0.85 * gap * bands);
            // behind the planet: hidden where the planet's disc is
            if t > 1.0 && ang < r {
                ring.w = 0.0;
            }
        }
    }
    if ang >= r {
        // a faint glow round it, the light it sends
        return ring;
    }
    let side = normalize(cross(c, vec3(0.0, 1.0, 0.0001)));
    let up = cross(side, c);
    let off = dir - c * cosd;
    let p = vec2(dot(off, side), dot(off, up)) / sin(r);
    let z = sqrt(max(1.0 - dot(p, p), 0.0));
    let nrm = normalize(side * p.x + up * p.y - c * z);
    let lat = p.y;
    let lon = atan2(p.x, z);
    var col = vec3(0.5);
    if n == 0 {
        let wob = fbm(vec2(lon * 3.0, lat * 4.0) + globals.time * 0.002) * 0.25;
        let b = sin((lat + wob) * 22.0);
        col = mix(vec3(0.82, 0.72, 0.58), vec3(0.6, 0.42, 0.3), smoothstep(-0.2, 0.5, b));
        col = mix(col, vec3(0.92, 0.88, 0.8), smoothstep(0.7, 0.95, sin((lat + wob) * 9.0 + 1.0)) * 0.6);
        // the storm: an oval eye in the southern bands
        let storm = length((p - vec2(0.28, -0.32)) * vec2(1.0, 1.9));
        col = mix(col, vec3(0.78, 0.38, 0.26), smoothstep(0.17, 0.1, storm));
    } else if n == 1 {
        let b = sin(lat * 16.0 + fbm(vec2(lon * 2.0, lat * 6.0)) * 0.8);
        col = mix(vec3(0.9, 0.8, 0.6), vec3(0.78, 0.66, 0.46), smoothstep(-0.3, 0.6, b));
    } else if n == 2 {
        let land = fbm(p * 3.5 + 7.0);
        col = mix(vec3(0.72, 0.36, 0.2), vec3(0.42, 0.2, 0.13), smoothstep(0.45, 0.65, land));
        col = mix(col, vec3(0.95, 0.92, 0.9), smoothstep(0.82, 0.9, abs(lat)));
    } else {
        let swirl = fbm(p * 2.5 + 3.0);
        col = mix(vec3(0.5, 0.78, 0.86), vec3(0.35, 0.6, 0.78), smoothstep(0.4, 0.7, swirl));
        col = mix(col, vec3(0.95, 0.97, 1.0), smoothstep(0.62, 0.75, lat));
    }
    // lit by the sun, a soft terminator, darker towards the rim (and a thin bright atmosphere
    // on the worlds that have one)
    let light = smoothstep(-0.08, 0.3, dot(nrm, sun));
    let limb = 0.65 + 0.35 * z;
    var surface = col * limb * (0.012 + 1.15 * light);
    if n == 3 || n == 2 {
        surface += vec3(0.3, 0.5, 0.8) * pow(1.0 - z, 4.0) * light * 0.4;
    }
    let edge = smoothstep(r, r * 0.97, ang);
    // the rings in front of the planet cover it
    var out = vec4(surface, edge);
    if ring.w > 0.0 {
        out = vec4(mix(out.rgb, ring.rgb, ring.w), max(edge, ring.w));
    }
    return out;
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
            let graze = pow(1.0 - clamp(-dot(dir, n), 0.0, 1.0), 6.0);
            let dusk = smoothstep(0.25, 0.0, abs(ndl)) * smoothstep(-0.2, 0.05, ndl);
            col = mix(col, vec3(0.25, 0.45, 0.95) * (0.08 + 0.92 * day) + vec3(0.6, 0.25, 0.05) * dusk * 0.5, graze * 0.65);
            c = col;
        } else {
            c = sky.zenith.rgb;
            // the Milky Way: a band of faint light across the sky, brightest towards the
            // galaxy's heart, cut by dark lanes of dust, with a few glowing clouds of gas
            let galaxy = normalize(vec3(0.35, 0.62, -0.7));
            let gd = dot(dir, galaxy);
            let band = exp(-gd * gd * 26.0);
            let heart_dir = normalize(vec3(-0.85, 0.15, -0.5) - galaxy * dot(vec3(-0.85, 0.15, -0.5), galaxy));
            let heart = pow(max(dot(dir, heart_dir), 0.0), 5.0) * exp(-gd * gd * 60.0);
            // grainy, made of countless unresolved stars, and threaded with dust: thin dark
            // filaments (ridges of noise) rather than blobs
            let grain = fbm3(dir * 9.0) * (0.75 + 0.5 * noise3(dir * 160.0));
            let lanes = 1.0 - abs(fbm3(dir * 6.0 + 2.0) * 2.0 - 1.0);
            let dust = smoothstep(0.72, 0.95, lanes) * smoothstep(0.35, 0.6, fbm3(dir * 14.0 + 8.0)) * min(band + heart, 1.0);
            var glow = vec3(0.022, 0.022, 0.03) * band * (0.35 + grain) + vec3(0.06, 0.045, 0.032) * heart * (0.5 + grain);
            glow *= 1.0 - 0.75 * dust;
            let gas = smoothstep(0.6, 0.82, fbm3(dir * 3.2 + 11.0)) * (0.3 + band);
            glow += mix(vec3(0.05, 0.01, 0.032), vec3(0.008, 0.035, 0.045), fbm3(dir * 4.5 + 5.0)) * gas * (0.3 + fbm3(dir * 16.0));
            // the sun's glare drowns the faintest of it
            let glare_dim = 1.0 - 0.6 * day_here * pow(towards, 4.0);
            c += glow * glare_dim;
            // stars: many faint ones (more of them along the Milky Way), fewer bright, and a
            // handful brightest of all with spikes
            let more = 1.0 + 3.0 * band + 4.0 * heart;
            c += space_stars(dir, min(0.05 * more, 0.6), 900.0, 0.32, 0.0) * glare_dim;
            c += space_stars(dir, 0.02 * more, 380.0, 0.7, 0.0) * glare_dim;
            c += space_stars(dir, 0.02, 110.0, 1.4, 0.0);
            c += space_stars(dir, 0.012, 38.0, 2.2, 0.6);
            // the planets of this system, far off, lit by the sun
            var worlds = array<vec4<f32>, 4>(
                vec4(-0.55, 0.42, 0.72, 0.032),
                vec4(-0.45, 0.3, -0.84, 0.022),
                vec4(-0.95, 0.15, -0.1, 0.014),
                vec4(0.3, 0.8, -0.5, 0.017),
            );
            for (var i = 0; i < 4; i++) {
                let wd = far_world(dir, worlds[i].xyz, worlds[i].w, i, sun);
                c = mix(c, wd.rgb, wd.w);
            }
            // and the bright evening star, too small to show a disc
            let star_dir = normalize(vec3(0.82, 0.12, -0.56));
            c += vec3(1.0, 0.97, 0.9) * (exp(-acos(clamp(dot(dir, star_dir), -1.0, 1.0)) * 900.0) * 6.0);
            // the rim of the atmosphere: a thin bright line just above the planet's edge,
            // brightest on the sun's side
            let above = e - (-sqrt(1.0 - 1.0 / (h * h)));
            let flat_dir = normalize(vec3(dir.x, 0.0, dir.z) + vec3(1e-4, 0.0, 0.0));
            let flat_sun = normalize(vec3(sun.x, 0.0, sun.z) + vec3(1e-4, 0.0, 0.0));
            let sunside = 0.25 + 0.75 * smoothstep(-0.6, 0.8, dot(flat_dir, flat_sun));
            c += sky.horizon.rgb * (exp(-above / 0.006) * 1.6 + exp(-above / 0.03) * 0.3) * sunside;
            // the sun: a white disc darker towards its edge, a corona streaming out in rays,
            // and a little glare (no air to spread it)
            let sa = acos(clamp(dot(dir, sun), -1.0, 1.0));
            let sr = 0.014;
            let s_side = normalize(cross(sun, vec3(0.0, 1.0, 0.0001)));
            let s_up = cross(s_side, sun);
            let around = atan2(dot(dir, s_up), dot(dir, s_side));
            // the corona: streamers of different lengths all round, fading out over a few
            // of the sun's widths, and a faint wider glow
            let streak = fbm(vec2(around * 4.0, globals.time * 0.01)) * fbm(vec2(around * 11.0, 3.0 + globals.time * 0.007));
            let reach = sr * (0.6 + 3.5 * streak * streak);
            let out_by = max(sa - sr, 0.0);
            let corona = exp(-out_by / reach) * 1.2 + exp(-out_by / (sr * 0.35)) * 1.5 + exp(-sa / (sr * 5.0)) * 0.06;
            c += vec3(1.0, 0.93, 0.82) * day_here * corona + sky.sun_colour.rgb * day_here * pow(towards, 300.0) * 0.15;
            if sa < sr {
                let mu = sqrt(max(1.0 - (sa / sr) * (sa / sr), 0.0));
                c += vec3(1.0, 0.96, 0.88) * day_here * 70.0 * (0.45 + 0.55 * mu);
            }
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
