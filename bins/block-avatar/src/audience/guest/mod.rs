//! The guest on stage: an approved viewer question, asked by the viewer's chosen character next
//! to yours, in the viewer's own recorded voice.
//!
//! When a question airs, the character pops in beside you, the camera widens to a two-shot, the
//! recording plays (through the default sound output, or `guest_output=` in the settings, e.g. a
//! virtual cable for OBS) and drives the guest's mouth, brows and head with the same lip-sync and
//! body-language analysis as yours. Your character turns to listen and, unless you turn it off
//! (`guest_mutes_host=false`), ignores your mic while the guest talks so speaker spill doesn't
//! move your mouth. After the question the guest waits with a quizzical tilt until you dismiss it
//! (A key, or Done on the control page). Cut stops everything at once.

mod playback;
mod voice;
mod update;
#[allow(unused_imports)]
pub use playback::*;
#[allow(unused_imports)]
pub use voice::*;
#[allow(unused_imports)]
pub use update::*;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

use bevy::{
    gltf::Gltf,
    prelude::*,
    world_serialization::{WorldAssetRoot, WorldInstanceReady},
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::lipsync::{Analyzer, Frame, LipMode, LipSync, Mouth, PickerSettings, level01};
use crate::questions::{Question, Queue, Status, parse_wav_mono16};
use crate::voice::{Prosody, VoiceFrame};

/// Where the guest stands, beside the host.
pub const GUEST_X: f32 = 1.05;
const GUEST_YAW: f32 = -0.38;
const ENTER_S: f32 = 0.45;
const LEAVE_S: f32 = 0.3;
/// gap between landing and talking
const LEAD_S: f32 = 0.25;

#[derive(Component)]
pub struct GuestRoot;
#[derive(Component)]
pub struct GuestPart(&'static str);
#[derive(Component)]
pub struct GuestHead;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Loading,
    /// popping in since t
    Entering(f32),
    Speaking,
    /// question finished at t; waiting for the answer
    After(f32),
    /// popping out since t, then the question is marked with this status
    Leaving(f32, Status),
}

pub struct OnStage {
    pub q: Question,
    /// name as shown on stream (hidden if it matches the blocked words)
    pub name: String,
    gltf: Handle<Gltf>,
    graph: Option<Handle<AnimationGraph>>,
    idle: Option<AnimationNodeIndex>,
    talk: Option<AnimationNodeIndex>,
    pub root: Option<Entity>,
    player: Option<Entity>,
    pub phase: Phase,
    voice: Voice,
    audio: Option<Playback>,
    picker: LipSync,
    prosody: Prosody,
    pub mouth: Mouth,
    pub level: f32,
    brows_up: bool,
    beat_t: f32,
    beat_amp: f32,
    speaking_since: Option<f32>,
    silent_since: f32,
    body_talk: bool,
    next_blink: f32,
    blink_until: f32,
    /// 0..1 of the words revealed in the bubble
    pub reveal: f32,
}

#[derive(Resource)]
pub struct Guest {
    pub queue: Arc<Mutex<Queue>>,
    pub ask: Option<crate::ask::AskServer>,
    pub ask_error: Option<String>,
    pub mute_host: bool,
    pub output: Option<String>,
    pub stage: Option<OnStage>,
    /// a question waiting for the stage to clear
    pending: Option<Question>,
    pub status: String,
    pub lip_mode: LipMode,
}

impl Guest {
    pub fn new(
        queue: Arc<Mutex<Queue>>,
        ask: Result<crate::ask::AskServer, String>,
        mute_host: bool,
        output: Option<String>,
    ) -> Self {
        let (ask, ask_error) = match ask {
            Ok(a) => (Some(a), None),
            Err(e) => (None, Some(e)),
        };
        Guest {
            queue,
            ask,
            ask_error,
            mute_host,
            output,
            stage: None,
            pending: None,
            status: String::new(),
            lip_mode: LipMode::Visemes,
        }
    }

    pub fn ask_url(&self) -> String {
        match (&self.ask, &self.ask_error) {
            (Some(a), _) => a.url.clone(),
            (None, Some(e)) => format!("off ({e})"),
            _ => "off".into(),
        }
    }

    /// Is the guest's recording playing right now?
    pub fn talking(&self) -> bool {
        self.stage
            .as_ref()
            .is_some_and(|s| s.phase == Phase::Speaking)
    }

    /// The bubble text: name on the first line, then the question, revealed as it's spoken.
    pub fn bubble_text(&self) -> Option<String> {
        let s = self.stage.as_ref()?;
        if matches!(
            s.phase,
            Phase::Loading | Phase::Entering(_) | Phase::Leaving(..)
        ) {
            return None;
        }
        let text = s.q.text.trim();
        if text.is_empty() {
            return Some(format!("{} asks:", s.name));
        }
        let all: Vec<char> = text.chars().collect();
        let n = ((all.len() as f32) * s.reveal).ceil() as usize;
        let shown: String = all[..n.min(all.len())].iter().collect();
        Some(format!("{}:\n{}", s.name, shown))
    }

    /// Control page / hotkey commands starting with "ask:". Returns a status line.
    pub fn command(&mut self, m: &str, now: f32) -> Option<String> {
        let rest = m.strip_prefix("ask:")?;
        let (verb, arg) = rest.split_once(':').unwrap_or((rest, ""));
        let id = arg.split(':').next().and_then(|x| x.parse::<u32>().ok());
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        let msg = match verb {
            "open" | "close" | "toggle" => {
                q.open = match verb {
                    "open" => true,
                    "close" => false,
                    _ => !q.open,
                };
                format!("Questions {}", if q.open { "open" } else { "closed" })
            }
            "names" => {
                let Some(a) = &self.ask else {
                    return Some("The ask page isn't running".into());
                };
                let mut c = a.shared.cfg.lock().unwrap_or_else(|e| e.into_inner());
                c.open_names = !c.open_names;
                format!(
                    "Typed names (no Twitch login) {}",
                    if c.open_names { "allowed" } else { "off" }
                )
            }
            "approve" => match id.map(|i| q.approve(i)) {
                Some(Ok(())) => format!("Question {} approved", id.unwrap_or(0)),
                Some(Err(e)) => e,
                None => "approve: which question?".into(),
            },
            "skip" => {
                let Some(i) = id else {
                    return Some("skip: which question?".into());
                };
                if self.stage.as_ref().is_some_and(|s| s.q.id == i) {
                    drop(q);
                    self.leave(now, Status::Skipped, true);
                    return Some(format!("Question {i} cut"));
                }
                q.finish(i, Status::Skipped);
                format!("Question {i} skipped")
            }
            "ban" => {
                let Some(i) = id else {
                    return Some("ban: which question?".into());
                };
                let acct = q.ban_question(i);
                drop(q);
                if self.stage.as_ref().is_some_and(|s| s.q.id == i) {
                    self.leave(now, Status::Skipped, true);
                }
                return Some(match acct {
                    Some(a) => format!("Banned {a}"),
                    None => format!("No question {i}"),
                });
            }
            "unban" => {
                let acct = crate::control::decode(arg);
                if q.unban(&acct) {
                    format!("Unbanned {acct}")
                } else {
                    format!("{acct} wasn't banned")
                }
            }
            "text" => {
                let Some(i) = id else {
                    return Some("text: which question?".into());
                };
                let text = arg.split_once(':').map(|x| x.1).unwrap_or("");
                q.set_text(i, &crate::control::decode(text));
                let t = q.get(i).map(|x| x.text.clone()).unwrap_or_default();
                if let Some(s) = self.stage.as_mut().filter(|s| s.q.id == i) {
                    s.q.text = t;
                }
                format!("Bubble text for question {i} saved")
            }
            "air" | "next" => {
                let Some(i) = (if verb == "next" {
                    q.next_approved()
                } else {
                    id
                }) else {
                    return Some("Nothing approved to air".into());
                };
                match q.take_for_air(i) {
                    Ok(x) => {
                        drop(q);
                        if self.stage.is_some() {
                            self.leave(now, Status::Done, false);
                        }
                        if let Some(old) = self.pending.replace(x) {
                            // a question that was waiting for the stage goes back in line
                            let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
                            q.finish_back(old.id);
                        }
                        return Some(format!("Question {i} going on air"));
                    }
                    Err(e) => e,
                }
            }
            "done" | "dismiss" => {
                drop(q);
                return Some(if self.leave(now, Status::Done, false) {
                    "Thanks for the question!".into()
                } else {
                    "Nobody on stage".into()
                });
            }
            "cut" => {
                drop(q);
                if let Some(p) = self.pending.take() {
                    self.queue.lock().unwrap_or_else(|e| e.into_inner()).finish(p.id, Status::Skipped);
                }
                return Some(if self.leave(now, Status::Skipped, true) {
                    "Cut".into()
                } else {
                    "Nobody on stage".into()
                });
            }
            "key" => {
                // the A key: air the next one, or end the current one (cut while it's still talking)
                drop(q);
                return match self.stage.as_ref().map(|s| s.phase) {
                    None => self.command("ask:next", now),
                    Some(Phase::Speaking) | Some(Phase::Loading) | Some(Phase::Entering(_)) => {
                        self.command("ask:cut", now)
                    }
                    Some(_) => self.command("ask:done", now),
                };
            }
            "clear" => {
                q.clear_finished();
                "Finished questions cleared".into()
            }
            "words" => {
                q.reload_words();
                format!("Blocked words reloaded ({})", q.blocked_words.len())
            }
            _ => return None,
        };
        Some(msg)
    }

    /// Starts the guest's exit. `now_cut` stops the audio at once.
    fn leave(&mut self, now: f32, status: Status, now_cut: bool) -> bool {
        let Some(s) = self.stage.as_mut() else {
            return false;
        };
        if now_cut || s.phase == Phase::Loading {
            if let Some(a) = s.audio.take() {
                a.stop();
            }
        }
        if !matches!(s.phase, Phase::Leaving(..)) {
            s.phase = Phase::Leaving(now, status);
        }
        true
    }
}

impl Queue {
    /// A question taken for air that never made it on stage goes back to approved.
    pub fn finish_back(&mut self, id: u32) {
        if let Some(q) = self
            .items
            .iter_mut()
            .find(|q| q.id == id && q.status == Status::OnAir)
        {
            q.status = Status::Approved;
        }
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_of_a_real_question_tracks_the_speech() {
        let path = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .join("demo/questions/clb-b0352.wav");
        let (rate, s) = parse_wav_mono16(&std::fs::read(path).unwrap()).unwrap();
        let s = crate::questions::level_voice(&s).unwrap();
        let v = Voice::analyse(rate, s);
        assert!((v.hop_s - 0.01).abs() < 1e-4);
        assert!((v.secs() - 2.27).abs() < 0.02);
        // the words come out over the clip, not all at once
        assert!(v.spoken_at(0.05) < 0.1, "{}", v.spoken_at(0.05));
        assert!(
            (0.25..0.75).contains(&v.spoken_at(v.secs() / 2.0)),
            "{}",
            v.spoken_at(v.secs() / 2.0)
        );
        assert_eq!(v.spoken_at(v.secs()), 1.0);
        // and the mouth moves while it plays: several shape changes, not stuck open or shut
        let mut picker = LipSync::new(
            PickerSettings {
                gate_db: v.gate_db,
                ..Default::default()
            },
            LipMode::Visemes,
        );
        let mut shapes = std::collections::HashSet::new();
        let mut changes = 0;
        let mut last = Mouth::Rest;
        let mut t = 0.0;
        while t < v.secs() {
            let m = picker.update(v.frame_at(t).unwrap(), 1.0 / 60.0);
            if m != last {
                changes += 1;
                last = m;
            }
            shapes.insert(m.slot());
            t += 1.0 / 60.0;
        }
        assert!(
            changes >= 8 && shapes.len() >= 3,
            "changes {changes}, shapes {shapes:?}"
        );
    }

    #[test]
    fn playback_clock_runs_without_a_sound_card() {
        // in CI there's no output device: the clock must still advance at real speed
        let p = Playback::start(
            Arc::new(vec![0.0; 16000]),
            16000,
            Some("no-such-device-xyz".into()),
        );
        std::thread::sleep(Duration::from_millis(300));
        let t = p.played_s(16000);
        assert!(t > 0.15 && t <= 1.0, "{t}");
        p.stop();
    }
}
