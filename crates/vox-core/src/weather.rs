//! Weather: a small atmosphere simulated over a world.
//!
//! The air over a world is a grid of cells (8 m across, 5 m tall, 32 × 10 × 32 by default)
//! holding temperature, water vapour, cloud and fog water, rain, and wind in three directions.
//! Each step:
//!
//! 1. **Forcing.** The wind is pulled towards the world's prevailing wind (slower near the
//!    ground: a log profile) with gusts from moving noise. The ground warms or cools the air
//!    just above it (the sun by day, losing heat at night, warmer and cooler patches), and wet
//!    ground breathes moisture into it. The whole air mass is nudged slowly towards the world's
//!    climate so it never wanders off.
//! 2. **Advection.** Everything is carried along by the wind (semi-Lagrangian, so any time step
//!    is stable); rain also falls through the air at its own speed.
//! 3. **Buoyancy.** Air warmer (or moister) than its layer rises; cloud and rain weigh it down.
//! 4. **Projection.** The wind is made divergence-free (air can't pile up), by solving for the
//!    pressure that pushes it apart: this is what turns rising warm air into a circulation.
//! 5. **Moisture.** Vapour past saturation (Tetens' formula) condenses into cloud or fog,
//!    warming the air by its latent heat; cloud in dry air evaporates and cools it. Cloud water
//!    turns into rain (Kessler's scheme: autoconversion and accretion); rain evaporates as it
//!    falls through dry air and what reaches the ground is the rain rate.
//!
//! The grid repeats across x and z, so the weather has no edge (like the world's fog), and it
//! is deterministic: the same climate, start and steps give the same weather. A world server
//! runs it and sends keyframes; players run it too between keyframes.

use serde::{Deserialize, Serialize};

const G: f32 = 9.81;
/// latent heat of vaporisation, J/kg
const LV: f32 = 2.5e6;
/// specific heat of air, J/(kg K)
const CP: f32 = 1004.0;
/// gas constant for water vapour, J/(kg K)
const RV: f32 = 461.5;
/// air density near the ground, kg/m³
const RHO: f32 = 1.2;
/// rain falls this fast, m/s
const RAIN_FALL: f32 = 5.0;
/// roughness length of the ground, m
const Z0: f32 = 0.1;
/// The grid is 50 m of air; a rain cloud is far deeper. What falls out of the grid's cloud is
/// scaled by this to stand for the cloud above it.
const CLOUD_ABOVE: f32 = 25.0;

/// A world's climate: the weather it tends towards.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Climate {
    /// air temperature near the ground, °C
    pub temperature: f32,
    /// relative humidity of the air mass, 0-1 (1 or more: fog and cloud will form)
    pub humidity: f32,
    /// prevailing wind at 10 m, m/s
    pub wind_speed: f32,
    /// where the wind comes from, degrees clockwise from north (-z): 90 = from the east
    pub wind_from: f32,
    /// how gusty, 0-1
    pub gusts: f32,
    /// how much the ground warms the air at the warmest time, °C above the air
    pub heating: f32,
    /// how wet the ground is (how much it breathes out), 0-1
    pub wetness: f32,
    /// sea-level pressure, hPa
    pub pressure: f32,
    /// where the ground is, m (the grid starts here)
    pub ground: f32,
}

impl Default for Climate {
    fn default() -> Self {
        Climate {
            temperature: 15.0,
            humidity: 0.6,
            wind_speed: 3.0,
            wind_from: 225.0,
            gusts: 0.4,
            heating: 2.0,
            wetness: 0.4,
            pressure: 1013.0,
            ground: -0.5,
        }
    }
}

impl Climate {
    /// Named weather: clear, haze, mist, fog, luminous (the void's bright standing fog),
    /// rain, storm, windy, snow.
    pub fn preset(name: &str, ground: f32) -> Option<Climate> {
        let base = Climate { ground, ..Default::default() };
        Some(match name {
            "clear" => Climate { humidity: 0.45, wind_speed: 2.5, gusts: 0.3, heating: 3.0, wetness: 0.2, ..base },
            "haze" => Climate { humidity: 0.78, wind_speed: 1.5, gusts: 0.2, heating: 2.5, ..base },
            "mist" => Climate { humidity: 0.97, wind_speed: 1.5, gusts: 0.25, heating: 0.5, wetness: 0.8, temperature: 11.0, ..base },
            "fog" => Climate { humidity: 1.04, wind_speed: 1.2, gusts: 0.2, heating: -0.5, wetness: 1.0, temperature: 9.0, ..base },
            "luminous" => {
                Climate { humidity: 1.03, wind_speed: 1.6, gusts: 0.5, heating: 0.4, wetness: 1.0, temperature: 12.0, ..base }
            }
            "rain" => Climate { humidity: 1.08, wind_speed: 5.0, gusts: 0.5, heating: 1.0, wetness: 1.0, temperature: 13.0, ..base },
            "storm" => Climate { humidity: 1.25, wind_speed: 13.0, gusts: 0.9, heating: 4.0, wetness: 1.0, temperature: 17.0, ..base },
            "windy" => Climate { humidity: 0.55, wind_speed: 11.0, gusts: 0.8, heating: 2.0, wetness: 0.3, ..base },
            "snow" => Climate { humidity: 1.06, wind_speed: 3.0, gusts: 0.4, heating: 0.0, wetness: 0.6, temperature: -3.0, ..base },
            // in orbit: no air to speak of (the simulation still runs, on next to nothing)
            "vacuum" => Climate { humidity: 0.02, wind_speed: 0.0, gusts: 0.0, heating: 0.0, wetness: 0.0, temperature: -40.0, ..base },
            _ => return None,
        })
    }

    /// Within what the simulation handles (anything a player sends is put through this).
    pub fn sane(self) -> Climate {
        let f = |v: f32, lo: f32, hi: f32, d: f32| if v.is_finite() { v.clamp(lo, hi) } else { d };
        Climate {
            temperature: f(self.temperature, -40.0, 45.0, 15.0),
            humidity: f(self.humidity, 0.0, 1.3, 0.6),
            wind_speed: f(self.wind_speed, 0.0, 30.0, 3.0),
            wind_from: f(self.wind_from, -720.0, 720.0, 225.0).rem_euclid(360.0),
            gusts: f(self.gusts, 0.0, 1.0, 0.4),
            heating: f(self.heating, -5.0, 10.0, 2.0),
            wetness: f(self.wetness, 0.0, 1.0, 0.4),
            pressure: f(self.pressure, 900.0, 1080.0, 1013.0),
            ground: f(self.ground, -100.0, 100.0, 0.0),
        }
    }

    /// A world's climate from its settings' `"weather"`: a preset's name, or an object with any
    /// of the fields (over the defaults). `ground` is where the ground is, m.
    pub fn from_settings(v: Option<&serde_json::Value>, ground: f32) -> Climate {
        match v {
            Some(serde_json::Value::String(name)) => Climate::preset(name, ground).unwrap_or(Climate { ground, ..Default::default() }),
            Some(obj @ serde_json::Value::Object(_)) => {
                let mut base = serde_json::to_value(Climate { ground, ..Default::default() }).unwrap_or_default();
                if let (Some(b), Some(o)) = (base.as_object_mut(), obj.as_object()) {
                    for (k, v) in o {
                        b.insert(k.clone(), v.clone());
                    }
                }
                serde_json::from_value::<Climate>(base).map(Climate::sane).unwrap_or(Climate { ground, ..Default::default() })
            }
            _ => Climate { ground, ..Default::default() },
        }
    }

    /// The prevailing wind as a vector (x, z), m/s: where it blows to.
    pub fn wind(&self) -> [f32; 2] {
        let a = self.wind_from.to_radians();
        // from the north (0°) blows towards +z (south); from the east (90°) towards -x
        [-a.sin() * self.wind_speed, a.cos() * self.wind_speed]
    }
}

/// Saturation vapour pressure over water (Tetens), hPa, at °C.
pub fn saturation_pressure(t: f32) -> f32 {
    6.112 * (17.67 * t / (t + 243.5)).exp()
}

/// Saturation specific humidity, kg/kg, at °C and hPa.
pub fn saturation(t: f32, p: f32) -> f32 {
    let es = saturation_pressure(t).min(p * 0.5);
    0.622 * es / (p - 0.378 * es)
}

/// Dew point, °C, from temperature (°C) and relative humidity (0-1).
pub fn dew_point(t: f32, rh: f32) -> f32 {
    let g = (rh.max(1e-3)).ln() + 17.67 * t / (t + 243.5);
    243.5 * g / (17.67 - g)
}

/// How far you can see (m) through cloud water (g/m³) and humid haze: Kunkel's fog formula,
/// with a haze term that thickens as the air nears saturation.
pub fn visibility(lwc: f32, rh: f32) -> f32 {
    let fog = if lwc > 1e-4 { 27.0 / lwc.powf(0.88) } else { f32::INFINITY };
    let haze = 60_000.0 * (1.0 - rh.clamp(0.0, 0.995)).powf(0.6);
    (1.0 / (1.0 / fog + 1.0 / haze)).clamp(5.0, 60_000.0)
}

/// The air at one place.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Default)]
pub struct Air {
    /// °C
    pub temperature: f32,
    /// 0-1
    pub humidity: f32,
    /// °C
    pub dew_point: f32,
    /// m/s (x, y, z)
    pub wind: [f32; 3],
    /// cloud or fog water, g/m³
    pub cloud: f32,
    /// rain reaching the ground here, mm/h
    pub rain: f32,
    /// m
    pub visibility: f32,
    /// hPa
    pub pressure: f32,
}

/// The simulated air over a world.
#[derive(Clone, Debug)]
pub struct Weather {
    pub climate: Climate,
    pub nx: usize,
    pub ny: usize,
    pub nz: usize,
    /// cell size (x, y, z), m
    pub cell: [f32; 3],
    /// the grid's corner (x, z), m; y is `climate.ground`
    pub origin: [f32; 2],
    /// seconds simulated
    pub time: f64,
    /// how strongly the sun heats the ground now, 0-1 (set by the world's time of day)
    pub sun: f32,
    pub t: Vec<f32>,
    pub q: Vec<f32>,
    pub qc: Vec<f32>,
    pub qr: Vec<f32>,
    pub u: Vec<f32>,
    pub v: Vec<f32>,
    pub w: Vec<f32>,
    /// rain reaching the ground per column, mm/h
    pub rain: Vec<f32>,
    seed: u32,
}

fn hash(i: i32, j: i32, k: i32, seed: u32) -> f32 {
    let mut h = (i as u32).wrapping_mul(0x8da6_b343) ^ (j as u32).wrapping_mul(0xd816_3841) ^ (k as u32).wrapping_mul(0xcb1a_b31f) ^ seed;
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 65535.0
}

/// Smooth value noise, -1..1, periodic in x and z with `period` lattice cells.
fn noise(x: f32, y: f32, z: f32, period: i32, seed: u32) -> f32 {
    let (i, j, k) = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
    let s = |v: f32| {
        let t = v - v.floor();
        t * t * (3.0 - 2.0 * t)
    };
    let (a, b, c) = (s(x), s(y), s(z));
    let h = |di: i32, dj: i32, dk: i32| hash((i + di).rem_euclid(period), j + dj, (k + dk).rem_euclid(period), seed);
    let l = |p: f32, q: f32, t: f32| p + (q - p) * t;
    let x00 = l(h(0, 0, 0), h(1, 0, 0), a);
    let x10 = l(h(0, 1, 0), h(1, 1, 0), a);
    let x01 = l(h(0, 0, 1), h(1, 0, 1), a);
    let x11 = l(h(0, 1, 1), h(1, 1, 1), a);
    l(l(x00, x10, b), l(x01, x11, b), c) * 2.0 - 1.0
}

impl Weather {
    /// A world's weather: 32 × 10 × 32 cells of 8 × 5 × 8 m over x and z in
    /// [-128, 128) m, starting from the climate's own air.
    pub fn new(climate: Climate, seed: u32) -> Weather {
        Weather::with_grid(climate, seed, [32, 10, 32], [8.0, 5.0, 8.0], [-128.0, -128.0])
    }

    pub fn with_grid(climate: Climate, seed: u32, n: [usize; 3], cell: [f32; 3], origin: [f32; 2]) -> Weather {
        let len = n[0] * n[1] * n[2];
        let mut w = Weather {
            climate,
            nx: n[0],
            ny: n[1],
            nz: n[2],
            cell,
            origin,
            time: 0.0,
            sun: 0.5,
            t: vec![0.0; len],
            q: vec![0.0; len],
            qc: vec![0.0; len],
            qr: vec![0.0; len],
            u: vec![0.0; len],
            v: vec![0.0; len],
            w: vec![0.0; len],
            rain: vec![0.0; n[0] * n[2]],
            seed,
        };
        let wind = climate.wind();
        for k in 0..w.nz {
            for j in 0..w.ny {
                for i in 0..w.nx {
                    let c = w.idx(i, j, k);
                    let (t, q) = w.climate_air(j);
                    w.t[c] = t;
                    w.q[c] = q;
                    let f = w.profile(j);
                    w.u[c] = wind[0] * f;
                    w.v[c] = wind[1] * f;
                }
            }
        }
        w
    }

    pub fn idx(&self, i: usize, j: usize, k: usize) -> usize {
        (k * self.ny + j) * self.nx + i
    }

    /// Height of a layer's middle above the ground, m.
    fn height(&self, j: usize) -> f32 {
        (j as f32 + 0.5) * self.cell[1]
    }

    /// Pressure at a layer, hPa.
    fn pressure(&self, j: usize) -> f32 {
        self.climate.pressure * (-(self.climate.ground + self.height(j)) / 8400.0).exp()
    }

    /// The prevailing wind's share at a layer (a log profile, 1 at 10 m).
    fn profile(&self, j: usize) -> f32 {
        ((self.height(j) + Z0) / Z0).ln() / ((10.0 + Z0) / Z0).ln()
    }

    /// The climate's temperature and vapour at a layer.
    fn climate_air(&self, j: usize) -> (f32, f32) {
        let t = self.climate.temperature - 0.0065 * self.height(j);
        let q = self.climate.humidity * saturation(t, self.pressure(j));
        (t, q)
    }

    /// Trilinear sample of a field at a position in cells (periodic in x and z).
    fn sample_field(&self, f: &[f32], x: f32, y: f32, z: f32) -> f32 {
        let y = y.clamp(0.0, (self.ny - 1) as f32);
        let (i0, j0, k0) = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
        let (fx, fy, fz) = (x - x.floor(), y - y.floor(), z - z.floor());
        let (nx, ny, nz) = (self.nx as i32, self.ny as i32, self.nz as i32);
        let at = |i: i32, j: i32, k: i32| {
            f[self.idx(i.rem_euclid(nx) as usize, j.clamp(0, ny - 1) as usize, k.rem_euclid(nz) as usize)]
        };
        let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let c00 = l(at(i0, j0, k0), at(i0 + 1, j0, k0), fx);
        let c10 = l(at(i0, j0 + 1, k0), at(i0 + 1, j0 + 1, k0), fx);
        let c01 = l(at(i0, j0, k0 + 1), at(i0 + 1, j0, k0 + 1), fx);
        let c11 = l(at(i0, j0 + 1, k0 + 1), at(i0 + 1, j0 + 1, k0 + 1), fx);
        l(l(c00, c10, fy), l(c01, c11, fy), fz)
    }

    /// Where a world position (m) is in cell units (cell centres at whole numbers).
    fn to_cells(&self, p: [f32; 3]) -> [f32; 3] {
        [
            (p[0] - self.origin[0]) / self.cell[0] - 0.5,
            (p[1] - self.climate.ground) / self.cell[1] - 0.5,
            (p[2] - self.origin[1]) / self.cell[2] - 0.5,
        ]
    }

    /// The air at a world position (m).
    pub fn air(&self, p: [f32; 3]) -> Air {
        let [x, y, z] = self.to_cells(p);
        let t = self.sample_field(&self.t, x, y, z);
        let q = self.sample_field(&self.q, x, y, z);
        let qc = self.sample_field(&self.qc, x, y, z).max(0.0);
        let j = y.clamp(0.0, (self.ny - 1) as f32) as usize;
        let pr = self.pressure(j);
        let rh = (q / saturation(t, pr)).clamp(0.0, 1.0);
        let lwc = qc * RHO * 1000.0;
        let col = {
            let (i, k) = (x.round().rem_euclid(self.nx as f32) as usize, z.round().rem_euclid(self.nz as f32) as usize);
            self.rain[k * self.nx + i]
        };
        Air {
            temperature: t,
            humidity: rh,
            dew_point: dew_point(t, rh),
            wind: [self.sample_field(&self.u, x, y, z), self.sample_field(&self.w, x, y, z), self.sample_field(&self.v, x, y, z)],
            cloud: lwc,
            rain: col,
            visibility: visibility(lwc, rh),
            pressure: pr,
        }
    }

    /// Cloud and fog water at a world position, g/m³ (for drawing fog).
    pub fn cloud_at(&self, p: [f32; 3]) -> f32 {
        let [x, y, z] = self.to_cells(p);
        if y < -0.5 || y > self.ny as f32 - 0.5 {
            return 0.0;
        }
        self.sample_field(&self.qc, x, y, z).max(0.0) * RHO * 1000.0
    }

    /// Relative humidity at a world position, 0-1.
    pub fn humidity_at(&self, p: [f32; 3]) -> f32 {
        let [x, y, z] = self.to_cells(p);
        let t = self.sample_field(&self.t, x, y, z);
        let q = self.sample_field(&self.q, x, y, z);
        let j = y.clamp(0.0, (self.ny - 1) as f32) as usize;
        (q / saturation(t, self.pressure(j))).clamp(0.0, 1.0)
    }

    /// Advance by `dt` seconds (any size; long steps are cut into pieces of at most 2 s).
    pub fn step(&mut self, dt: f32) {
        let n = (dt / 2.0).ceil().max(1.0) as usize;
        for _ in 0..n {
            self.substep(dt / n as f32);
        }
    }

    fn substep(&mut self, dt: f32) {
        self.force(dt);
        self.advect(dt);
        self.buoyancy(dt);
        self.project(24);
        self.moisture(dt);
        self.time += dt as f64;
    }

    fn force(&mut self, dt: f32) {
        let c = self.climate;
        let wind = c.wind();
        let t = self.time as f32;
        let gust_scale = 48.0; // m across a gust
        let px = (self.nx as f32 * self.cell[0] / gust_scale).round().max(1.0) as i32;
        let pz = (self.nz as f32 * self.cell[2] / gust_scale).round().max(1.0) as i32;
        let relax_wind = (dt / 40.0).min(1.0);
        let relax_climate = (dt / 1800.0).min(1.0);
        // moist air keeps coming in when the climate is wetter than saturation (rain, storms)
        let relax_inflow = (dt / if c.humidity > 1.0 { 400.0 } else { 1800.0 }).min(1.0);
        let relax_ground = (dt / 240.0).min(1.0);
        let relax_moist = (dt / 600.0).min(1.0);
        for k in 0..self.nz {
            for j in 0..self.ny {
                for i in 0..self.nx {
                    let ci = self.idx(i, j, k);
                    // gusts: noise carried along with the wind
                    let gx = i as f32 / self.nx as f32 * px as f32 - wind[0] * t / gust_scale;
                    let gz = k as f32 / self.nz as f32 * pz as f32 - wind[1] * t / gust_scale;
                    let gy = j as f32 * 0.3 + t / 25.0;
                    let g1 = noise(gx, gy, gz, px.max(pz), self.seed);
                    let g2 = noise(gx + 7.3, gy + 3.1, gz - 2.2, px.max(pz), self.seed ^ 0x9e37);
                    let gust = c.gusts * c.wind_speed.max(0.8) * 0.6;
                    let f = self.profile(j);
                    let tu = wind[0] * f + g1 * gust;
                    let tv = wind[1] * f + g2 * gust;
                    self.u[ci] += (tu - self.u[ci]) * relax_wind;
                    self.v[ci] += (tv - self.v[ci]) * relax_wind;
                    // the air mass drifts back towards the climate
                    let (ct, cq) = self.climate_air(j);
                    self.t[ci] += (ct - self.t[ci]) * relax_climate;
                    self.q[ci] += (cq - self.q[ci]) * relax_inflow;
                    if j == 0 {
                        // the ground: warmer and cooler patches that stay put
                        let patch = noise(i as f32 * 0.25, 0.5, k as f32 * 0.25, (self.nx / 4).max(1) as i32, self.seed ^ 0x51ed);
                        let ground_t = c.temperature + c.heating * (0.3 + 0.7 * self.sun) + patch * (0.6 + 0.4 * c.heating.abs());
                        self.t[ci] += (ground_t - self.t[ci]) * relax_ground;
                        let qs = saturation(ground_t, self.pressure(0));
                        let target = c.wetness * qs;
                        if target > self.q[ci] {
                            self.q[ci] += (target - self.q[ci]) * relax_moist;
                        }
                        // friction
                        self.u[ci] *= 1.0 - (dt / 120.0).min(0.5);
                        self.v[ci] *= 1.0 - (dt / 120.0).min(0.5);
                    }
                }
            }
        }
    }

    fn advect(&mut self, dt: f32) {
        let (nx, ny, nz) = (self.nx, self.ny, self.nz);
        let mut out: [Vec<f32>; 7] = std::array::from_fn(|_| vec![0.0; nx * ny * nz]);
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    let c = self.idx(i, j, k);
                    let bx = i as f32 - self.u[c] * dt / self.cell[0];
                    let by = j as f32 - self.w[c] * dt / self.cell[1];
                    let bz = k as f32 - self.v[c] * dt / self.cell[2];
                    out[0][c] = self.sample_field(&self.u, bx, by, bz);
                    out[1][c] = self.sample_field(&self.v, bx, by, bz);
                    out[2][c] = self.sample_field(&self.w, bx, by, bz);
                    out[3][c] = self.sample_field(&self.t, bx, by, bz);
                    out[4][c] = self.sample_field(&self.q, bx, by, bz);
                    out[5][c] = self.sample_field(&self.qc, bx, by, bz);
                    // rain also falls
                    let ry = j as f32 - (self.w[c] - RAIN_FALL) * dt / self.cell[1];
                    out[6][c] = if ry > (ny - 1) as f32 { 0.0 } else { self.sample_field(&self.qr, bx, ry, bz) };
                }
            }
        }
        let [u, v, w, t, q, qc, qr] = out;
        self.u = u;
        self.v = v;
        self.w = w;
        self.t = t;
        self.q = q;
        self.qc = qc;
        self.qr = qr;
        // rain in the bottom layer falls out onto the ground: its rate is the flux there
        // (kg/m² a second is mm a second)
        for k in 0..nz {
            for i in 0..nx {
                let c = self.idx(i, 0, k);
                self.rain[k * nx + i] = self.qr[c] * RAIN_FALL * RHO * 3600.0 * CLOUD_ABOVE;
                let frac = (RAIN_FALL * dt / self.cell[1]).min(1.0);
                self.qr[c] *= 1.0 - frac;
            }
        }
    }

    fn buoyancy(&mut self, dt: f32) {
        for j in 0..self.ny {
            let (mut tm, mut qm, mut n) = (0.0, 0.0, 0.0);
            for k in 0..self.nz {
                for i in 0..self.nx {
                    let c = self.idx(i, j, k);
                    tm += self.t[c];
                    qm += self.q[c];
                    n += 1.0;
                }
            }
            let (tm, qm) = (tm / n, qm / n);
            for k in 0..self.nz {
                for i in 0..self.nx {
                    let c = self.idx(i, j, k);
                    let b = G * ((self.t[c] - tm) / (tm + 273.15) + 0.61 * (self.q[c] - qm) - self.qc[c] - self.qr[c]);
                    self.w[c] += b * dt;
                    // nothing comes up through the ground or goes out of the top
                    if j == 0 && self.w[c] < 0.0 {
                        self.w[c] = 0.0;
                    }
                    if j == self.ny - 1 && self.w[c] > 0.0 {
                        self.w[c] = 0.0;
                    }
                }
            }
        }
    }

    /// Make the wind divergence-free: solve ∇²p = ∇·u by Jacobi iteration, then subtract ∇p.
    fn project(&mut self, iterations: usize) {
        let (nx, ny, nz) = (self.nx as i32, self.ny as i32, self.nz as i32);
        let [dx, dy, dz] = self.cell;
        let at = |i: i32, j: i32, k: i32| -> Option<usize> {
            if j < 0 || j >= ny {
                None
            } else {
                Some(((k.rem_euclid(nz) * ny + j) * nx + i.rem_euclid(nx)) as usize)
            }
        };
        let len = self.u.len();
        let mut div = vec![0.0f32; len];
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    let c = at(i, j, k).unwrap();
                    let uxp = self.u[at(i + 1, j, k).unwrap()];
                    let uxm = self.u[at(i - 1, j, k).unwrap()];
                    let vzp = self.v[at(i, j, k + 1).unwrap()];
                    let vzm = self.v[at(i, j, k - 1).unwrap()];
                    let wyp = at(i, j + 1, k).map_or(0.0, |n| self.w[n]);
                    let wym = at(i, j - 1, k).map_or(0.0, |n| self.w[n]);
                    div[c] = (uxp - uxm) / (2.0 * dx) + (vzp - vzm) / (2.0 * dz) + (wyp - wym) / (2.0 * dy);
                }
            }
        }
        let mut p = vec![0.0f32; len];
        let mut next = vec![0.0f32; len];
        let (ax, ay, az) = (1.0 / (dx * dx), 1.0 / (dy * dy), 1.0 / (dz * dz));
        for _ in 0..iterations {
            for k in 0..nz {
                for j in 0..ny {
                    for i in 0..nx {
                        let c = at(i, j, k).unwrap();
                        // the ground and the top are walls: the pressure there mirrors the cell's
                        let py = |n: Option<usize>| n.map_or(p[c], |n| p[n]);
                        let sum = ax * (p[at(i + 1, j, k).unwrap()] + p[at(i - 1, j, k).unwrap()])
                            + az * (p[at(i, j, k + 1).unwrap()] + p[at(i, j, k - 1).unwrap()])
                            + ay * (py(at(i, j + 1, k)) + py(at(i, j - 1, k)));
                        next[c] = (sum - div[c]) / (2.0 * ax + 2.0 * ay + 2.0 * az);
                    }
                }
            }
            std::mem::swap(&mut p, &mut next);
        }
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    let c = at(i, j, k).unwrap();
                    let py = |n: Option<usize>| n.map_or(p[c], |n| p[n]);
                    self.u[c] -= (p[at(i + 1, j, k).unwrap()] - p[at(i - 1, j, k).unwrap()]) / (2.0 * dx);
                    self.v[c] -= (p[at(i, j, k + 1).unwrap()] - p[at(i, j, k - 1).unwrap()]) / (2.0 * dz);
                    self.w[c] -= (py(at(i, j + 1, k)) - py(at(i, j - 1, k))) / (2.0 * dy);
                }
            }
        }
    }

    fn moisture(&mut self, dt: f32) {
        for k in 0..self.nz {
            for j in 0..self.ny {
                let p = self.pressure(j);
                for i in 0..self.nx {
                    let c = self.idx(i, j, k);
                    let t = self.t[c];
                    let tk = t + 273.15;
                    let qs = saturation(t, p);
                    let factor = 1.0 + LV * LV * qs / (CP * RV * tk * tk);
                    if self.q[c] > qs {
                        // condense into cloud or fog, warming the air
                        let d = (self.q[c] - qs) / factor;
                        self.q[c] -= d;
                        self.qc[c] += d;
                        self.t[c] += LV / CP * d;
                    } else if self.qc[c] > 0.0 {
                        // cloud in dry air evaporates, cooling it
                        let d = ((qs - self.q[c]) / factor).min(self.qc[c]);
                        self.q[c] += d;
                        self.qc[c] -= d;
                        self.t[c] -= LV / CP * d;
                    }
                    // cloud into rain (Kessler's scheme: autoconversion and accretion). The air
                    // here is 50 m deep where a real rain cloud is kilometres, so its rate is
                    // scaled up twentyfold to rain like one
                    let qc = self.qc[c];
                    let auto = if qc > 2e-4 { 2e-2 * (qc - 2e-4) } else { 0.0 };
                    let accrete = 2.2 * qc * self.qr[c].max(0.0).powf(0.875);
                    let to_rain = ((auto + accrete) * dt).min(qc);
                    self.qc[c] -= to_rain;
                    self.qr[c] += to_rain;
                    // rain evaporates falling through dry air
                    let rh = self.q[c] / qs;
                    if rh < 1.0 && self.qr[c] > 0.0 {
                        let d = (3e-4 * (1.0 - rh) * self.qr[c].sqrt() * dt).min(self.qr[c]).min((qs - self.q[c]).max(0.0));
                        self.qr[c] -= d;
                        self.q[c] += d;
                        self.t[c] -= LV / CP * d;
                    }
                    self.qc[c] = self.qc[c].max(0.0);
                    self.qr[c] = self.qr[c].max(0.0);
                    self.q[c] = self.q[c].max(0.0);
                }
            }
        }
    }

    /// Averages over the whole grid: temperature (°C), relative humidity, cloud water (g/m³),
    /// wind speed (m/s), rain (mm/h).
    pub fn summary(&self) -> [f32; 5] {
        let n = self.t.len() as f32;
        let mut s = [0.0f32; 5];
        for c in 0..self.t.len() {
            let j = (c / self.nx) % self.ny;
            s[0] += self.t[c];
            s[1] += (self.q[c] / saturation(self.t[c], self.pressure(j))).min(1.0);
            s[2] += self.qc[c] * RHO * 1000.0;
            s[3] += (self.u[c] * self.u[c] + self.v[c] * self.v[c] + self.w[c] * self.w[c]).sqrt();
        }
        s[4] = self.rain.iter().sum::<f32>() / self.rain.len() as f32;
        [s[0] / n, s[1] / n, s[2] / n, s[3] / n, s[4]]
    }

    /// The whole state, quantised to bytes, for sending: a server sends these as keyframes.
    pub fn keyframe(&self) -> Keyframe {
        let q8 = |v: f32, lo: f32, hi: f32| (((v - lo) / (hi - lo)).clamp(0.0, 1.0) * 65535.0).round() as u16;
        let mut fields = Vec::with_capacity(self.t.len() * 7);
        for c in 0..self.t.len() {
            fields.push(q8(self.t[c], -60.0, 60.0));
            fields.push(q8(self.q[c].sqrt(), 0.0, 0.25));
            fields.push(q8(self.qc[c].sqrt(), 0.0, 0.12));
            fields.push(q8(self.qr[c].sqrt(), 0.0, 0.12));
            fields.push(q8(self.u[c], -60.0, 60.0));
            fields.push(q8(self.v[c], -60.0, 60.0));
            fields.push(q8(self.w[c], -60.0, 60.0));
        }
        Keyframe {
            climate: self.climate,
            time: self.time,
            sun: self.sun,
            size: [self.nx as u16, self.ny as u16, self.nz as u16],
            cell: self.cell,
            origin: self.origin,
            seed: self.seed,
            fields,
            rain: self.rain.iter().map(|r| q8(r.sqrt(), 0.0, 20.0)).collect(),
        }
    }

    /// Weather from a keyframe.
    pub fn from_keyframe(k: &Keyframe) -> Result<Weather, String> {
        let n = [k.size[0] as usize, k.size[1] as usize, k.size[2] as usize];
        let bad = || "a weather keyframe of the wrong size".to_string();
        let len = n[0].checked_mul(n[1]).and_then(|v| v.checked_mul(n[2])).ok_or_else(bad)?;
        let rain = n[0].checked_mul(n[2]).ok_or_else(bad)?;
        if len == 0 || len > 1 << 20 || k.fields.len() != len * 7 || k.rain.len() != rain {
            return Err(bad());
        }
        let mut w = Weather::with_grid(k.climate.sane(), k.seed, n, k.cell, k.origin);
        let d = |b: u16, lo: f32, hi: f32| lo + (b as f32 / 65535.0) * (hi - lo);
        for c in 0..len {
            let f = &k.fields[c * 7..c * 7 + 7];
            w.t[c] = d(f[0], -60.0, 60.0);
            w.q[c] = d(f[1], 0.0, 0.25).powi(2);
            w.qc[c] = d(f[2], 0.0, 0.12).powi(2);
            w.qr[c] = d(f[3], 0.0, 0.12).powi(2);
            w.u[c] = d(f[4], -60.0, 60.0);
            w.v[c] = d(f[5], -60.0, 60.0);
            w.w[c] = d(f[6], -60.0, 60.0);
        }
        w.rain = k.rain.iter().map(|r| d(*r, 0.0, 20.0).powi(2)).collect();
        w.time = k.time;
        w.sun = k.sun;
        Ok(w)
    }
}

/// A weather state for sending.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Keyframe {
    pub climate: Climate,
    pub time: f64,
    pub sun: f32,
    pub size: [u16; 3],
    pub cell: [f32; 3],
    pub origin: [f32; 2],
    pub seed: u32,
    pub fields: Vec<u16>,
    pub rain: Vec<u16>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small(c: Climate) -> Weather {
        Weather::with_grid(c, 7, [16, 8, 16], [8.0, 5.0, 8.0], [-64.0, -64.0])
    }

    #[test]
    fn saturation_and_dew_point() {
        // about 23.4 hPa at 20 °C, 6.1 at 0 °C
        assert!((saturation_pressure(20.0) - 23.4).abs() < 0.2);
        assert!((saturation_pressure(0.0) - 6.11).abs() < 0.01);
        assert!((saturation(20.0, 1013.0) * 1000.0 - 14.4).abs() < 0.3, "{}", saturation(20.0, 1013.0));
        // saturated air: the dew point is the temperature; drier, lower
        assert!((dew_point(15.0, 1.0) - 15.0).abs() < 0.01);
        assert!((dew_point(20.0, 0.5) - 9.3).abs() < 0.2, "{}", dew_point(20.0, 0.5));
    }

    #[test]
    fn visibility_in_fog_and_haze() {
        // thick fog (0.3 g/m³): under 100 m; light fog (0.05): a few hundred; clear dry air: far
        assert!(visibility(0.3, 1.0) < 100.0);
        let light = visibility(0.05, 1.0);
        assert!(light > 200.0 && light < 600.0, "{light}");
        assert!(visibility(0.0, 0.4) > 20_000.0);
        assert!(visibility(0.0, 0.95) < visibility(0.0, 0.5));
    }

    #[test]
    fn wind_directions() {
        let from_north = Climate { wind_from: 0.0, wind_speed: 5.0, ..Default::default() }.wind();
        assert!(from_north[0].abs() < 1e-4 && (from_north[1] - 5.0).abs() < 1e-4, "blows south (+z)");
        let from_east = Climate { wind_from: 90.0, wind_speed: 5.0, ..Default::default() }.wind();
        assert!((from_east[0] + 5.0).abs() < 1e-4, "blows west (-x)");
    }

    #[test]
    fn dry_air_stays_clear_and_wind_follows_the_climate() {
        let mut w = small(Climate::preset("clear", 0.0).unwrap());
        w.step(120.0);
        let s = w.summary();
        assert!(s[2] < 1e-3, "no cloud in dry air: {s:?}");
        assert!(s[4] < 1e-3);
        let a = w.air([0.0, 10.0, 0.0]);
        let want = Climate::preset("clear", 0.0).unwrap().wind();
        let along = (a.wind[0] * want[0] + a.wind[2] * want[1]) / (want[0].hypot(want[1]));
        assert!(along > 0.4 * want[0].hypot(want[1]), "{a:?}");
        // slower near the ground
        let low = w.air([0.0, 1.0, 0.0]);
        assert!(low.wind[0].hypot(low.wind[2]) < a.wind[0].hypot(a.wind[2]) + 0.5);
    }

    #[test]
    fn saturated_air_makes_fog_and_releases_heat() {
        let c = Climate::preset("fog", 0.0).unwrap();
        let mut w = small(c);
        let before = w.summary();
        w.step(30.0);
        let s = w.summary();
        assert!(s[2] > 0.02, "fog forms: {s:?}");
        assert!(s[0] > before[0], "condensing warms the air");
        let a = w.air([0.0, 2.0, 0.0]);
        assert!(a.humidity > 0.99 && a.visibility < 2000.0, "{a:?}");
        assert!((a.dew_point - a.temperature).abs() < 0.5);
    }

    #[test]
    fn heavy_cloud_rains() {
        let mut w = small(Climate::preset("storm", 0.0).unwrap());
        w.step(600.0);
        let s = w.summary();
        assert!(s[4] > 1.0, "rain reaches the ground: {s:?}");
        // the void's luminous fog stands still: no rain
        let mut l = small(Climate::preset("luminous", 0.0).unwrap());
        l.step(600.0);
        assert!(l.summary()[2] > 0.05 && l.summary()[4] < 0.1, "{:?}", l.summary());
        assert!(s.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn the_wind_is_divergence_free_after_projection() {
        let mut w = small(Climate::preset("windy", 0.0).unwrap());
        // a hot spot to stir things
        let c = w.idx(8, 0, 8);
        w.t[c] += 10.0;
        w.step(20.0);
        let mut worst = 0.0f32;
        let total = {
            let (nx, ny, nz) = (w.nx as i32, w.ny as i32, w.nz as i32);
            let at = |i: i32, j: i32, k: i32| ((k.rem_euclid(nz) * ny + j) * nx + i.rem_euclid(nx)) as usize;
            let mut sum = 0.0;
            for k in 0..nz {
                for j in 1..ny - 1 {
                    for i in 0..nx {
                        let d = (w.u[at(i + 1, j, k)] - w.u[at(i - 1, j, k)]) / 16.0
                            + (w.v[at(i, j, k + 1)] - w.v[at(i, j, k - 1)]) / 16.0
                            + (w.w[at(i, j + 1, k)] - w.w[at(i, j - 1, k)]) / 10.0;
                        worst = worst.max(d.abs());
                        sum += d.abs();
                    }
                }
            }
            sum
        };
        assert!(worst < 0.05, "{worst} {total}");
    }

    #[test]
    fn the_same_start_gives_the_same_weather() {
        let mut a = small(Climate::preset("mist", 0.0).unwrap());
        let mut b = small(Climate::preset("mist", 0.0).unwrap());
        a.step(40.0);
        b.step(40.0);
        assert_eq!(a.t, b.t);
        assert_eq!(a.qc, b.qc);
    }

    #[test]
    fn keyframes_round_trip() {
        let mut w = small(Climate::preset("rain", 0.0).unwrap());
        w.step(60.0);
        let k = w.keyframe();
        let back: Keyframe = crate::wire::decode(&crate::wire::encode(&k)).unwrap();
        let w2 = Weather::from_keyframe(&back).unwrap();
        for (a, b) in w.t.iter().zip(&w2.t) {
            assert!((a - b).abs() < 0.01);
        }
        for (a, b) in w.qc.iter().zip(&w2.qc) {
            assert!((a - b).abs() < 1e-5, "{a} {b}");
        }
        let p = [3.0, 4.0, -5.0];
        assert!((w.air(p).temperature - w2.air(p).temperature).abs() < 0.02);
        // a bad keyframe is refused
        let mut bad = k.clone();
        bad.fields.pop();
        assert!(Weather::from_keyframe(&bad).is_err());
        // a full-size keyframe is a reasonable message
        let full = Weather::new(Climate::default(), 1).keyframe();
        assert!(crate::wire::encode(&full).len() < 240_000);
    }

    #[test]
    fn climate_from_settings_and_sanity() {
        let c = Climate::from_settings(Some(&serde_json::json!("fog")), 2.0);
        assert_eq!(c, Climate::preset("fog", 2.0).unwrap());
        let c = Climate::from_settings(Some(&serde_json::json!({"wind_speed": 8.0, "humidity": 5.0})), 0.0);
        assert_eq!(c.wind_speed, 8.0);
        assert_eq!(c.humidity, 1.3, "clamped");
        assert_eq!(Climate::from_settings(None, 1.0).ground, 1.0);
        let wild = Climate { temperature: f32::NAN, wind_from: -90.0, ..Default::default() }.sane();
        assert_eq!(wild.temperature, 15.0);
        assert_eq!(wild.wind_from, 270.0);
    }

    #[test]
    fn presets() {
        for n in ["clear", "haze", "mist", "fog", "luminous", "rain", "storm", "windy", "snow", "vacuum"] {
            assert!(Climate::preset(n, 0.0).is_some(), "{n}");
        }
        assert!(Climate::preset("hail", 0.0).is_none());
    }
}
