//! Doorways to other places: the geometry the client draws and walks through
//! (`vox-world::portals`) and the server checks, in one place so the two agree.
//!
//! A world's settings list them under `"portals"` (see `vox-world::portals` for the format).
//! Positions are in blocks. Going through from one side, a point by the doorway comes out at
//! the matching point by `arrive` on the other side, turned by the doorway's `yaw`.

use serde_json::Value;

use crate::in_blocks;

/// A doorway's width and height are each at least this and at most [`MAX_SIDE`], in blocks
/// (half a metre to 32 m).
pub const MIN_SIDE: f32 = in_blocks(0.5);
pub const MAX_SIDE: f32 = in_blocks(32.0);
/// The size a doorway that doesn't say gets: 2 m wide, 3 m high, in blocks.
pub const DEFAULT_SIZE: [f32; 2] = [in_blocks(2.0), in_blocks(3.0)];
/// Feet this far below the sill still count as in the opening, and the opening stops this far
/// below its top (the head has to fit): half a metre, in blocks.
pub const SILL_SLACK: f32 = in_blocks(0.5);

/// One side of a doorway, clamped to [`MIN_SIDE`]..=[`MAX_SIDE`].
pub fn clamp_side(c: f32) -> f32 {
    c.clamp(MIN_SIDE, MAX_SIDE)
}

/// One doorway.
#[derive(Clone, Debug, PartialEq)]
pub struct Door {
    pub name: String,
    /// the middle of the sill
    pub at: [f32; 3],
    /// walked through along x (else along z)
    pub along_x: bool,
    /// width and height
    pub size: [f32; 2],
    /// the level its echo shows
    pub level: String,
    /// the world it leads to; None: somewhere else in this one
    pub url: Option<String>,
    /// the matching point on the other side
    pub arrive: [f32; 3],
    /// which way walking through (north for z, west for x) faces there, radians
    pub yaw: f32,
}

/// The doorways in a world's settings (JSON). Ones missing `at`, `level` or `arrive` are left out.
pub fn parse(settings: &Value) -> Vec<Door> {
    let Some(list) = settings.get("portals").and_then(|v| v.as_array()) else { return Vec::new() };
    let v3 = |v: &Value, k: &str| -> Option<[f32; 3]> {
        let a = v.get(k)?.as_array()?;
        let p = [a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32];
        p.iter().all(|c| c.is_finite()).then_some(p)
    };
    list.iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let size = p.get("size").and_then(|s| s.as_array());
            let dim = |j: usize, d: f64| size.and_then(|s| s.get(j)).and_then(|v| v.as_f64()).unwrap_or(d) as f32;
            let yaw = p.get("yaw").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            Some(Door {
                name: p.get("name").and_then(|v| v.as_str()).map_or_else(|| format!("portal {}", i + 1), String::from),
                at: v3(p, "at")?,
                along_x: p.get("axis").and_then(|v| v.as_str()) == Some("x"),
                size: [clamp_side(dim(0, DEFAULT_SIZE[0] as f64)), clamp_side(dim(1, DEFAULT_SIZE[1] as f64))],
                level: p.get("level")?.as_str()?.to_string(),
                url: p.get("url").and_then(|v| v.as_str()).map(String::from),
                arrive: v3(p, "arrive")?,
                yaw: if yaw.is_finite() { yaw.to_radians() } else { 0.0 },
            })
        })
        .collect()
}

impl Door {
    /// The doorway of an entity [`crate::entity::Portal`]. The caller works out where it
    /// leads (`url`, None for somewhere in this world) and the arrival point there.
    /// Requires a validated Portal: `Component::validate` must have passed for it. That is
    /// where the size and yaw are checked; this only clamps the size as [`parse`] does.
    pub fn from_portal(p: &crate::entity::Portal, url: Option<String>, arrive: [f32; 3]) -> Door {
        use crate::entity::Axis;
        Door {
            name: p.name.clone(),
            at: p.at,
            along_x: p.axis == Axis::X,
            size: p.size.map(clamp_side),
            level: p.level.clone(),
            url,
            arrive,
            yaw: if p.yaw.is_finite() { p.yaw.to_radians() } else { 0.0 },
        }
    }

    /// The axis you walk through along: x or z.
    fn normal(&self) -> [f32; 3] {
        if self.along_x { [1.0, 0.0, 0.0] } else { [0.0, 0.0, 1.0] }
    }

    fn across(&self) -> [f32; 3] {
        if self.along_x { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] }
    }

    /// How far the world turns going through from `side` (+1: from the +x/+z side, walking
    /// towards -x/-z; -1 the other way), radians about y.
    pub fn turn(&self, side: f32) -> f32 {
        let base = if self.along_x { std::f32::consts::FRAC_PI_2 } else { 0.0 };
        self.yaw - base - if side < 0.0 { std::f32::consts::PI } else { 0.0 }
    }

    /// How this world meets the other side, going through from `side`: a point mapped by the
    /// frame is where [`Door::carry`] takes it, and a velocity or facing turns with it.
    pub fn frame(&self, side: f32) -> crate::travel::Frame {
        let turn = self.turn(side);
        let at = crate::travel::Frame { offset: [0.0; 3], turn }.point(self.at);
        crate::travel::Frame { offset: [self.arrive[0] - at[0], self.arrive[1] - at[1], self.arrive[2] - at[2]], turn }
    }

    /// A point here as the matching point on the other side, coming from `side`.
    pub fn carry(&self, p: [f32; 3], side: f32) -> [f32; 3] {
        let (s, c) = self.turn(side).sin_cos();
        let d = [p[0] - self.at[0], p[1] - self.at[1], p[2] - self.at[2]];
        // a turn about y, as glam's Quat::from_rotation_y applies it
        [self.arrive[0] + c * d[0] + s * d[2], self.arrive[1] + d[1], self.arrive[2] - s * d[0] + c * d[2]]
    }

    /// Is `p` (feet) in the doorway's opening, sideways and up?
    pub fn within(&self, p: [f32; 3]) -> bool {
        let d = sub(p, self.at);
        dot(d, self.across()).abs() <= self.size[0] * 0.5 && d[1] >= -SILL_SLACK && d[1] <= self.size[1] - SILL_SLACK
    }

    /// Which side of the doorway `p` is on: the signed distance from it.
    pub fn side_of(&self, p: [f32; 3]) -> f32 {
        dot(sub(p, self.at), self.normal())
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yard() -> Door {
        let s = serde_json::json!({ "portals": [
            {"name": "yard", "at": [0, 0, -39.5], "axis": "z", "size": [8, 16], "level": "yard", "url": ":7781", "arrive": [0, 0, 11.5], "yaw": 0},
            {"at": [1, 2], "level": "broken", "arrive": [0, 0, 0]},
        ]});
        let doors = parse(&s);
        assert_eq!(doors.len(), 1, "one without a whole position is left out");
        doors[0].clone()
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-4)
    }

    #[test]
    fn through_a_doorway() {
        let d = yard();
        assert_eq!(d.url.as_deref(), Some(":7781"));
        assert!(d.within([3.0, 0.0, -39.5]) && !d.within([5.0, 0.0, -39.5]) && !d.within([0.0, 16.0, -39.5]));
        assert!(d.side_of([0.0, 0.0, -38.0]) > 0.0 && d.side_of([0.0, 0.0, -41.0]) < 0.0);
        // walking north (from the +z side): straight through, a step to the left stays a step to the left
        assert!(close(d.carry([1.0, 0.0, -39.5], 1.0), [1.0, 0.0, 11.5]));
        // from the other side the world turns round
        assert!(close(d.carry([1.0, 0.0, -39.5], -1.0), [-1.0, 0.0, 11.5]));
        // a doorway along x turns a quarter
        let mut x = d.clone();
        x.along_x = true;
        assert!(close(x.carry([0.0, 0.0, -38.5], 1.0), [-1.0, 0.0, 11.5]));
    }

    #[test]
    fn from_portal_matches_parse() {
        // the yard's doorway, as data/levels/yard.json writes it
        let json = serde_json::json!({ "portals": [
            {"name": "void", "at": [0, 0, 11.5], "axis": "z", "size": [8, 16], "level": "void", "url": ":7780", "arrive": [0, 0, -39.5], "yaw": 0},
        ]});
        let parsed = parse(&json);
        let p = crate::entity::Portal {
            name: "void".into(),
            at: [0.0, 0.0, 11.5],
            axis: crate::entity::Axis::Z,
            size: [8.0, 16.0],
            yaw: 0.0,
            level: "void".into(),
            to: crate::entity::Dest::Url { url: ":7780".into(), arrive: [0.0, 0.0, -39.5] },
            key: None,
        };
        assert_eq!(Door::from_portal(&p, Some(":7780".into()), [0.0, 0.0, -39.5]), parsed[0]);
    }

    #[test]
    fn from_portal_matches_parse_when_turned_and_clamped() {
        // a doorway along x, turned a quarter, with a size that parse clamps to the largest width
        // and the smallest height
        let json = serde_json::json!({ "portals": [
            {"name": "far", "at": [5, 0, 2], "axis": "x", "size": [300, 0.5], "level": "far", "url": "/w/7781/ws", "arrive": [-3, 0, 2], "yaw": 90},
        ]});
        let parsed = parse(&json);
        assert_eq!(parsed[0].size, [MAX_SIDE, MIN_SIDE]);
        // the same values as a Portal, tidied the way the editor's input is
        let mut c = crate::entity::Component::Portal(crate::entity::Portal {
            name: "far".into(),
            at: [5.0, 0.0, 2.0],
            axis: crate::entity::Axis::X,
            size: [300.0, 0.5],
            yaw: 90.0,
            level: "far".into(),
            to: crate::entity::Dest::Url { url: "/w/7781/ws".into(), arrive: [-3.0, 0.0, 2.0] },
            key: None,
        });
        c.tidy();
        let crate::entity::Component::Portal(p) = c else { panic!("still a portal") };
        assert_eq!(Door::from_portal(&p, Some("/w/7781/ws".into()), [-3.0, 0.0, 2.0]), parsed[0]);
    }

    #[test]
    fn a_doorways_frame_takes_points_where_carry_does() {
        for (along_x, yaw) in [(false, 0.0), (true, 90.0), (false, 37.0), (true, -120.0)] {
            let d = Door {
                name: "d".into(),
                at: [10.0, 2.0, -4.0],
                along_x,
                size: DEFAULT_SIZE,
                level: "yard".into(),
                url: None,
                arrive: [-30.0, 7.0, 55.0],
                yaw: f32::to_radians(yaw),
            };
            for side in [1.0, -1.0] {
                let f = d.frame(side);
                for p in [[10.0, 2.0, -4.0], [11.5, 3.0, -3.0], [9.0, 2.5, -5.5]] {
                    let (a, b) = (f.point(p), d.carry(p, side));
                    assert!((0..3).all(|i| (a[i] - b[i]).abs() < 1e-3), "{along_x} {yaw} {side}: {a:?} vs {b:?}");
                }
            }
        }
    }
}
