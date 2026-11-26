//! Your voice does more:
//! - **Laugh** (detect.rs): your character cracks up for a couple of seconds.
//! - **Sing** (detect.rs): a microphone appears in its hand, music notes float up, and it sways
//!   until you stop.
//! - **Catchphrase gags**: phrases you choose, heard by the speech-to-text (Whisper, a caption
//!   file, or the Say box), fire control-page commands: confetti, an emote, a clip, a face, a hat,
//!   or a sound file of yours. Written in `gags.txt` next to avatar_settings.txt:
//!
//!       let's go = throw:confetti; emote:hop
//!       big if true = emote:spin; face:4
//!       no comment = play:No comment
//!       mic drop = sound:boom.wav; emote:bow
//!
//!   Sounds are your own .wav files in `sounds/`. Each gag waits 8 s before it can fire again.

use bevy::{gltf::GltfAssetLabel, prelude::*, world_serialization::WorldAssetRoot};

use crate::detect::{Detector, Event};

pub struct Gag {
    pub words: Vec<String>,
    pub phrase: String,
    pub actions: Vec<String>,
}

fn norm_words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .replace(['\u{2019}', '\''], "")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect()
}

pub fn parse_gags(text: &str) -> Vec<Gag> {
    text.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (p, a) = l.split_once('=')?;
            let words = norm_words(p);
            let actions: Vec<String> = a
                .split(';')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect();
            (!words.is_empty() && !actions.is_empty()).then(|| Gag {
                words,
                phrase: p.trim().to_string(),
                actions,
            })
        })
        .collect()
}

fn count(hay: &[String], needle: &[String]) -> usize {
    if needle.is_empty() || hay.len() < needle.len() {
        return 0;
    }
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

const DEFAULT_GAGS: &str = "# Catchphrase gags: when the speech-to-text hears a phrase, these happen.\n# phrase = action; action   (actions are control-page commands)\n#   throw:confetti  throw:tomato  emote:hop  emote:spin  play:Victory  face:1  wear:crown\n#   sound:yourfile.wav  (from the sounds folder)\nlet's go = throw:confetti; emote:hop\nbig if true = emote:spin\n";

#[derive(Resource)]
pub struct VoiceFx {
    pub det: Detector,
    pub gags: Vec<Gag>,
    pub gags_on: bool,
    /// per gag: how many times it's been heard in the current caption line, and when it last fired
    fired: Vec<(usize, f32)>,
    last_text: String,
    pub last: String,
    notes: Option<Entity>,
    sounds: Vec<crate::guest::Playback>,
}

impl VoiceFx {
    pub fn new(laugh: bool, sing: bool, gags_on: bool) -> Self {
        let path = crate::applog::user_dir().join("gags.txt");
        if !path.exists() {
            let _ = std::fs::write(&path, DEFAULT_GAGS);
        }
        let gags = parse_gags(&std::fs::read_to_string(&path).unwrap_or_default());
        let fired = vec![(0, -100.0); gags.len()];
        let mut det = Detector::default();
        det.laugh_on = laugh;
        det.sing_on = sing;
        VoiceFx {
            det,
            gags,
            gags_on,
            fired,
            last_text: String::new(),
            last: String::new(),
            notes: None,
            sounds: vec![],
        }
    }

    pub fn reload(&mut self) -> usize {
        self.gags = parse_gags(
            &std::fs::read_to_string(crate::applog::user_dir().join("gags.txt"))
                .unwrap_or_default(),
        );
        self.fired = vec![(0, -100.0); self.gags.len()];
        self.gags.len()
    }

    /// voice:laugh, voice:sing, voice:gags (toggles), voice:reload
    pub fn command(&mut self, arg: &str) -> Option<String> {
        Some(match arg {
            "laugh" => {
                self.det.laugh_on = !self.det.laugh_on;
                format!(
                    "Laugh detection {}",
                    if self.det.laugh_on { "on" } else { "off" }
                )
            }
            "sing" => {
                self.det.sing_on = !self.det.sing_on;
                format!(
                    "Sing detection {}",
                    if self.det.sing_on { "on" } else { "off" }
                )
            }
            "gags" => {
                self.gags_on = !self.gags_on;
                format!(
                    "Catchphrase gags {}",
                    if self.gags_on { "on" } else { "off" }
                )
            }
            "reload" => format!("{} gags loaded from gags.txt", self.reload()),
            _ => return None,
        })
    }

    /// New caption text in: the actions of every gag newly heard (respecting each gag's cooldown).
    pub fn hear(&mut self, text: &str, now: f32) -> Vec<String> {
        let continues = !self.last_text.is_empty()
            && text.starts_with(&self.last_text[..self.last_text.len().min(text.len())]);
        if !continues {
            for f in &mut self.fired {
                f.0 = 0;
            }
        }
        self.last_text = text.to_string();
        let words = norm_words(text);
        let mut out = vec![];
        for (i, g) in self.gags.iter().enumerate() {
            let n = count(&words, &g.words);
            if n > self.fired[i].0 {
                self.fired[i].0 = n;
                if now - self.fired[i].1 >= 8.0 {
                    self.fired[i].1 = now;
                    self.last = format!("Gag: \"{}\"", g.phrase);
                    out.extend(g.actions.iter().cloned());
                }
            }
        }
        out
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut fx: ResMut<VoiceFx>,
    mut av: ResMut<crate::Avatar>,
    mic: Res<crate::Mic>,
    lip: Res<crate::Lip>,
    mut caps: ResMut<crate::bubbles::Captions>,
    mut inject: ResMut<crate::Inject>,
    mut wardrobe: ResMut<crate::wardrobe::Wardrobe>,
    heads: Query<(Entity, &crate::Joint)>,
    assets: Res<AssetServer>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    fx.det.gate = lip.picker.settings.gate_db;
    let muted = av.muted || av.hush;
    for f in mic.drain() {
        if muted {
            continue;
        }
        let ev = fx.det.push(f);
        if let Some(e) = ev {
            let epoch = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            crate::note!("[voice] {e:?} at {epoch:.3}");
        }
        match ev {
            Some(Event::Laugh) => {
                av.laugh_until = now + 2.2;
                fx.last = "You laughed".into();
            }
            Some(Event::SingStart) => {
                av.singing = true;
                wardrobe.override_hand = crate::wardrobe::find("mic", None);
                fx.last = "You're singing".into();
            }
            Some(Event::SingStop) => {
                av.singing = false;
                wardrobe.override_hand = None;
            }
            None => {}
        }
    }
    if (muted || !fx.det.sing_on) && av.singing {
        av.singing = false;
        fx.det.singing = false;
        wardrobe.override_hand = None;
    }
    // music notes floating off the head while singing
    let head = heads
        .iter()
        .find(|(_, j)| **j == crate::Joint::Head)
        .map(|(e, _)| e);
    match (av.singing, fx.notes, head) {
        (true, None, Some(h)) => {
            fx.notes =
                Some(
                    commands
                        .spawn((
                            WorldAssetRoot(assets.load(
                                GltfAssetLabel::Scene(0).from_asset("props/lead/fx_notes.glb"),
                            )),
                            Transform::from_xyz(0.35, 0.55, 0.0).with_scale(Vec3::splat(1.2)),
                            ChildOf(h),
                        ))
                        .id(),
                );
        }
        (false, Some(e), _) => {
            commands.entity(e).try_despawn();
            fx.notes = None;
        }
        _ => {}
    }
    // gags
    let heard = std::mem::take(&mut caps.heard);
    if fx.gags_on {
        for t in heard {
            for a in fx.hear(&t, now) {
                if let Some(file) = a.strip_prefix("sound:") {
                    let path = crate::applog::user_dir().join("sounds").join(file.trim());
                    match std::fs::read(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|b| crate::lipsync::decode_wav_mono(&b))
                    {
                        Ok((rate, s)) => {
                            fx.sounds.retain(|p| !p.finished(rate));
                            fx.sounds.push(crate::guest::Playback::start(
                                std::sync::Arc::new(s),
                                rate,
                                None,
                            ));
                        }
                        Err(e) => av.status = format!("Gag sound {}: {e}", path.display()),
                    }
                } else {
                    inject.0.push(a);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gags_fire_once_per_saying() {
        let mut fx = VoiceFx {
            gags: parse_gags(DEFAULT_GAGS),
            fired: vec![(0, -100.0); 2],
            ..VoiceFx::new(true, true, true)
        };
        assert_eq!(fx.gags.len(), 2);
        assert_eq!(fx.gags[0].words, vec!["lets", "go"]);
        // a growing caption line: fires when the phrase appears, not again as the line grows
        assert!(fx.hear("okay", 0.0).is_empty());
        assert_eq!(fx.hear("okay let's", 0.5), Vec::<String>::new());
        assert_eq!(
            fx.hear("okay let's go", 1.0),
            vec!["throw:confetti", "emote:hop"]
        );
        assert!(fx.hear("okay let's go everybody", 1.5).is_empty());
        // said again in the same line, but inside the cooldown
        assert!(
            fx.hear("okay let's go everybody, let's go!", 3.0)
                .is_empty()
        );
        // a new line after the cooldown
        assert_eq!(
            fx.hear("Let's GO!", 10.0),
            vec!["throw:confetti", "emote:hop"]
        );
        assert!(
            fx.hear("big if it's true", 11.0).is_empty(),
            "words must be in a row"
        );
        assert_eq!(
            fx.hear("that's big, if true", 12.0),
            vec!["emote:spin"],
            "punctuation doesn't matter"
        );
        assert!(parse_gags("# c\n = x\nhello =\nhi there = a; ;b").len() == 1);
    }
}
