//! The control page's `world:...` commands, and the state it shows.

use super::*;

pub(super) fn run_command(w: &mut WorldClient, cmd: &str) {
    let parts: Vec<&str> = cmd.split(':').collect();
    let num = |i: usize| parts.get(i).and_then(|v| v.trim().parse::<f32>().ok());
    let cell = |i: usize| -> Option<[i32; 3]> {
        Some([num(i)? .floor() as i32, num(i + 1)?.floor() as i32, num(i + 2)?.floor() as i32])
    };
    match parts.first().copied().unwrap_or("") {
        "place" => match (cell(1), parts.get(4).and_then(|n| w.block_id(n))) {
            (Some(c), Some(b)) => w.edit(c, b),
            _ => w.notice("world:place:x:y:z:block"),
        },
        "remove" => match cell(1) {
            Some(c) => w.edit(c, 0),
            None => w.notice("world:remove:x:y:z"),
        },
        "goto" => {
            if let (Some(x), Some(y), Some(z)) = (num(1), num(2), num(3)) {
                w.body.pos = [x, y, z];
                w.vy = 0.0;
            }
        }
        "look" => {
            if let (Some(y), Some(p)) = (num(1), num(2)) {
                w.cam_yaw = y.to_radians();
                w.cam_pitch = p.to_radians().clamp(-1.45, 1.3);
            }
        }
        "fly" => w.fly = parts.get(1).is_none_or(|v| *v != "off"),
        "portal" => match parts.get(1) {
            Some(n) => w.portal_go = Some(n.to_string()),
            None => w.notice("world:portal:<name>"),
        },
        "camera" => match parts.get(1).copied() {
            Some("diablo") => {
                w.cam_mode = CamMode::Diablo;
                w.forced_shot = None;
            }
            Some("behind") => {
                w.cam_mode = CamMode::Behind;
                w.forced_shot = None;
            }
            Some("first") => {
                w.cam_mode = CamMode::First;
                w.forced_shot = None;
            }
            Some("auto") => w.forced_shot = None,
            Some("fixed") => {
                let name = parts.get(2).copied().unwrap_or("");
                match w.shots.iter().position(|s| s.name == name) {
                    Some(i) => w.forced_shot = Some(i),
                    None => w.notice(format!("No camera called {name}")),
                }
            }
            _ => w.notice("world:camera:diablo|behind|auto|fixed:<name>"),
        },
        "zoom" => {
            if let Some(z) = num(1) {
                w.zoom = z.clamp(4.0, 45.0);
            }
        }
        // walk to a point (blocks), as a click on the ground does
        "moveto" => {
            if let (Some(x), Some(z)) = (num(1), num(2)) {
                w.goal = Some(Vec3::new(x, w.body.pos[1], z));
            }
        }
        "weather" => match parts.get(1).and_then(|n| Climate::preset(n, w.climate().ground)) {
            Some(c) => {
                w.set_climate(c);
                w.notice(format!("Weather: {}", parts[1]));
            }
            None => w.notice("world:weather:clear|haze|mist|fog|luminous|rain|storm|windy|snow"),
        },
        "wind" => {
            if let Some(speed) = num(1) {
                let mut c = w.climate();
                c.wind_speed = speed;
                if let Some(from) = num(2) {
                    c.wind_from = from;
                }
                w.set_climate(c);
            }
        }
        "humidity" => {
            if let Some(h) = num(1) {
                let c = Climate { humidity: h, ..w.climate() };
                w.set_climate(c);
            }
        }
        "temperature" => {
            if let Some(t) = num(1) {
                let c = Climate { temperature: t, ..w.climate() };
                w.set_climate(c);
            }
        }
        "time" | "clouds" => {
            let verb = parts[0].to_string();
            w.sky_cmds.push((verb, parts.get(1).copied().unwrap_or("").to_string()));
        }
        // walk:x:y:seconds — push the stick (x right, y forward) for a while
        "walk" => {
            if let (Some(x), Some(y), Some(t)) = (num(1), num(2), num(3)) {
                let until = w.now() + t.clamp(0.0, 30.0) as f64;
                w.auto = Some((Vec2::new(x, y), until));
            }
        }
        "jump" => w.auto_jump = true,
        "slot" => {
            if let Some(n) = num(1) {
                w.slot = (n as usize).saturating_sub(1).min(w.hotbar.len().saturating_sub(1));
            }
        }
        "click" => {
            if let Some(h) = w.target {
                if parts.get(1) == Some(&"left") {
                    w.edit(h.cell, 0);
                } else {
                    let c = [h.cell[0] + h.normal[0], h.cell[1] + h.normal[1], h.cell[2] + h.normal[2]];
                    let b = w.selected();
                    w.edit(c, b);
                }
            }
        }
        _ => w.notice(format!("Unknown world command: {cmd}")),
    }
}

/// For the control page's state.
pub fn json(w: &WorldClient) -> serde_json::Value {
    if !w.active() {
        return serde_json::Value::Null;
    }
    serde_json::json!({
        "url": w.url,
        "time": (w.hours * 100.0).round() / 100.0,
        "camera": w.cam_mode.name(),
        "shot": w.shot.map(|i| w.shots[i].name.clone()),
        "zoom": w.zoom,
        "floor": w.floor,
        "portals": w.portals.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
        "travelling": w.travelling(),
        "weather": w.air().map(|a| serde_json::json!({
            "temperature": a.temperature,
            "humidity": a.humidity,
            "dew_point": a.dew_point,
            "wind": a.wind,
            "cloud": a.cloud,
            "rain": a.rain,
            "visibility": a.visibility,
            "line": weather_line(&a),
        })),
        "climate": w.weather.as_ref().map(|x| serde_json::to_value(x.climate).unwrap_or_default()),
        "status": w.status,
        "world": w.world,
        "you": w.you,
        "ready": w.ready,
        "players": w.players(),
        "blocks": w.view.confirmed.count(),
        "pending": w.view.pending(),
        "seq": w.view.seq,
        "chunks": w.meshes.len(),
        "pos": w.body.pos,
        "grounded": w.grounded,
        "fly": w.fly,
        "target": w.target.map(|h| h.cell),
        "slot": w.slot + 1,
        "block": w.block_name(w.selected()),
        "edits": w.edits,
        "refused": w.refused,
        "notice": w.notice.as_ref().map(|n| n.0.clone()),
    })
}
