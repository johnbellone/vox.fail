//! Doorways to other places: the geometry the client draws and walks through
//! (`vox-world::portals`) and the server checks, in one place so the two agree.
//!
//! A world's settings list them under `"portals"` (see `vox-world::portals` for the format).
//! Positions are in blocks. Going through from one side, a point by the doorway comes out at
//! the matching point by `arrive` on the other side, turned by the doorway's `yaw`.

use serde_json::Value;

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
                size: [dim(0, 4.0).clamp(1.0, 64.0), dim(1, 6.0).clamp(1.0, 64.0)],
                level: p.get("level")?.as_str()?.to_string(),
                url: p.get("url").and_then(|v| v.as_str()).map(String::from),
                arrive: v3(p, "arrive")?,
                yaw: if yaw.is_finite() { yaw.to_radians() } else { 0.0 },
            })
        })
        .collect()
}

impl Door {
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
        dot(d, self.across()).abs() <= self.size[0] * 0.5 && d[1] >= -1.0 && d[1] <= self.size[1] - 1.0
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
}
