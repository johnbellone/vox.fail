//! The settings file (plain key=value, easy to edit by hand), and a small hash for jitter.

#[allow(unused_imports)]
use super::*;

// ------------------------------------------------------------------------------------------
// Settings file (plain key=value so it's easy to edit by hand)
// ------------------------------------------------------------------------------------------
pub(super) fn load_settings() -> HashMap<String, String> {
    std::fs::read_to_string(settings_path())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            l.split_once('=')
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

/// Update some keys in the settings file, keeping the rest.
pub(super) fn save_keys(pairs: &[(&str, String)]) {
    let mut m: Vec<(String, String)> = std::fs::read_to_string(settings_path())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            l.split_once('=')
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        })
        .collect();
    // one line per key: a newline in a value would write extra keys into the file
    let one_line = |t: &str| t.chars().filter(|c| !c.is_control()).collect::<String>();
    for (k, v) in pairs {
        let v = one_line(v);
        match m.iter_mut().find(|e| e.0 == *k) {
            Some(e) => e.1 = v,
            None => m.push((k.to_string(), v)),
        }
    }
    let _ = std::fs::write(
        settings_path(),
        m.iter()
            .map(|(k, v)| format!("{k}={v}\n"))
            .collect::<String>(),
    );
}

pub(super) fn save_settings(av: &Avatar, lip: &Lip, cam: &webcam::Webcam) {
    let s = format!(
        "model={}\nexpression={}\nbackground={}\nframing={}\ngate_db={:.1}\nauto_talk={}\nhud={}\nlip_mode={}\nreactions={}\nbop={}\nsleep_after={:.0}\nwebcam={}\nwebcam_size={}x{}\nwebcam_fps={}\nbody_language={}\nformants={}\nvowels={}\n",
        av.models.get(av.model).map(|m| m.1.as_str()).unwrap_or(""),
        EXPRESSIONS[av.expression].name,
        av.background.name(),
        av.framing,
        lip.picker.settings.gate_db,
        av.auto_talk,
        av.hud,
        lip.picker.mode.name(),
        av.reactions,
        av.bop,
        av.sleep_after,
        cam.enabled,
        cam.width,
        cam.height,
        cam.fps,
        av.prosody_on,
        lip.picker.use_formants,
        lip.picker.refs.to_setting()
    );
    let pairs: Vec<(String, String)> = s
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let refs: Vec<(&str, String)> = pairs.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    save_keys(&refs);
}

/// Deterministic pseudo-random 0..1 from an index and a time, for scattering confetti.
pub(super) fn hash01(i: u32, t: f32) -> f32 {
    let mut x = i.wrapping_mul(2654435761) ^ t.to_bits().wrapping_mul(40503);
    x ^= x >> 15;
    x = x.wrapping_mul(2246822519);
    x ^= x >> 13;
    (x & 0xFFFF) as f32 / 65535.0
}
