//! Live co-host: a second microphone drives a second character standing on your left, with the
//! same lip sync, nods and blinks as yours. For a friend in the room (a second mic) or on a call
//! (route Discord's output into a virtual audio cable and pick that as the co-host mic).
//!
//! Settings: `cohost_mic=` part of the input device's name (the app lists inputs with
//! `--list-mics`), `cohost_model=dog.glb`. Control page: Co-host on/off, next character.
//! For testing without a second person: `--cohost-wav file.wav` plays a recording into it.

use bevy::prelude::*;

use crate::lipsync::{LipMode, LipSync, Mouth, NoiseFloor, PickerSettings, level01};
use crate::puppet::Puppet;
use crate::voice::{Prosody, VoiceFrame};

pub const COHOST_X: f32 = -1.05;

#[derive(Resource)]
pub struct Cohost {
    pub on: bool,
    pub model: String,
    pub mic_name: Option<String>,
    pub wav: Option<String>,
    mic: Option<crate::Mic>,
    entity: Option<Entity>,
    spawned_model: String,
    picker: LipSync,
    prosody: Prosody,
    nf: Option<NoiseFloor>,
    nf_frames: usize,
    beat_t: f32,
    beat_amp: f32,
    speaking_since: Option<f32>,
    silent_since: f32,
    next_blink: f32,
    blink_until: f32,
    pub speaking: bool,
    pub status: String,
}

impl Cohost {
    pub fn new(on: bool, model: String, mic_name: Option<String>, wav: Option<String>) -> Self {
        Cohost {
            on,
            model,
            mic_name,
            wav,
            mic: None,
            entity: None,
            spawned_model: String::new(),
            picker: LipSync::new(PickerSettings::default(), LipMode::Visemes),
            prosody: Prosody::default(),
            nf: None,
            nf_frames: 0,
            beat_t: -10.0,
            beat_amp: 0.0,
            speaking_since: None,
            silent_since: 0.0,
            next_blink: 1.0,
            blink_until: 0.0,
            speaking: false,
            status: "Co-host: off".into(),
        }
    }

    /// cohost (toggle), cohost:next (character), cohost:mic:<name>
    pub fn command(&mut self, arg: &str, models: &[(String, String)]) -> Option<String> {
        let (verb, rest) = arg.split_once(':').unwrap_or((arg, ""));
        Some(match verb {
            "" | "toggle" => {
                self.on = !self.on;
                format!("Co-host {}", if self.on { "on" } else { "off" })
            }
            "next" => {
                let i = models
                    .iter()
                    .position(|m| m.1 == self.model)
                    .map(|i| (i + 1) % models.len())
                    .unwrap_or(0);
                self.model = models[i].1.clone();
                format!("Co-host plays {}", models[i].0)
            }
            "mic" => {
                self.mic_name = Some(rest.to_string()).filter(|s| !s.is_empty());
                self.mic = None;
                format!("Co-host mic: {}", rest)
            }
            _ => return None,
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut c: ResMut<Cohost>,
    mut av: ResMut<crate::Avatar>,
    lip: Res<crate::Lip>,
    time: Res<Time>,
    assets: Res<AssetServer>,
    mut puppets: Query<&mut Puppet>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    // off, or the character changed: clear the stage
    if (!c.on || c.spawned_model != c.model) && c.entity.is_some() {
        if let Some(e) = c.entity.take() {
            commands.entity(e).try_despawn();
        }
    }
    if !c.on {
        c.mic = None;
        av.cohost_on = false;
        av.look_left_to = 0.0;
        c.speaking = false;
        c.status = "Co-host: off".into();
        return;
    }
    if c.mic.is_none() {
        c.mic = match (&c.wav, &c.mic_name) {
            (Some(w), _) => {
                let m = crate::start_wav(w.clone());
                m.0.go.store(true, std::sync::atomic::Ordering::Relaxed);
                Some(m)
            }
            (None, Some(n)) => Some(crate::start_input(Some(n.clone()), false)),
            (None, None) => {
                c.status =
                    "Co-host: set cohost_mic= in avatar_settings.txt (or the control page)".into();
                av.cohost_on = false;
                return;
            }
        };
        c.nf = Some(NoiseFloor::new());
        c.nf_frames = 0;
        c.picker = LipSync::new(PickerSettings::default(), lip.picker.mode);
        c.picker.use_formants = lip.picker.use_formants;
    }
    if c.entity.is_none() {
        let mut p = Puppet::new(&c.model, &assets);
        p.face = ["eyes_open", "brows_normal", "mouth_flat"];
        c.entity = Some(
            commands
                .spawn((
                    p,
                    Transform::from_xyz(COHOST_X, 0.0, 0.0)
                        .with_rotation(Quat::from_rotation_y(0.38)),
                ))
                .id(),
        );
        c.spawned_model = c.model.clone();
    }
    av.cohost_on = true;
    let Some(mic) = c.mic.clone() else { return };
    if let Some(e) = mic.0.error.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        c.status = format!("Co-host mic: {e}");
    } else {
        c.status = format!(
            "Co-host: {} on {}",
            c.model.rsplit('/').next().unwrap_or(""),
            mic.0.device.lock().unwrap_or_else(|e| e.into_inner())
        );
    }
    // calibrate the gate on the first 1.5 s, like your own mic
    let frames = mic.drain();
    let c = &mut *c;
    if let Some(nf) = c.nf.as_mut() {
        frames.iter().for_each(|f| nf.push(f.db));
        c.nf_frames += frames.len();
        if c.nf_frames >= 150 {
            let floor = nf.finish().unwrap_or(-60.0);
            c.picker.settings.gate_db = (floor + 6.0).clamp(-70.0, -25.0);
            c.nf = None;
        }
    }
    let f = mic.frame();
    let mouth = c.picker.update(f, dt);
    let level = level01(f, &c.picker.settings);
    let (gate, speech) = (c.picker.settings.gate_db, c.picker.speech_db);
    let ev = c.prosody.update(
        VoiceFrame {
            pitch_hz: f.pitch_hz,
            clarity: f.clarity,
            f1: f.f1,
            f2: f.f2,
        },
        f.db,
        f.onsets,
        dt,
        gate,
        speech,
    );
    if let Some(k) = ev.beat {
        c.beat_t = now;
        c.beat_amp = k;
    }
    if mouth != Mouth::Rest {
        c.speaking_since.get_or_insert(now);
        c.silent_since = now;
    } else if now - c.silent_since > 0.6 {
        c.speaking_since = None;
    }
    c.speaking = c.speaking_since.is_some();
    if mouth == Mouth::Rest && now >= c.next_blink {
        c.blink_until = now + 0.12;
        c.next_blink = now + 2.0 + crate::hash01(7, now) * 3.5;
    }
    let brows_up = c.prosody.brows_up;
    let (beat_t, beat_amp, talking, blink) = (
        c.beat_t,
        c.beat_amp,
        c.speaking_since.is_some_and(|t| now - t > 0.25),
        now < c.blink_until,
    );
    if let Some(mut p) = c.entity.and_then(|e| puppets.get_mut(e).ok()) {
        let ex = &crate::EXPRESSIONS[0];
        p.face = [
            if blink { "eyes_shut" } else { ex.eyes },
            if brows_up { "brows_raised" } else { ex.brows },
            ex.mouths[mouth.slot()],
        ];
        let mut x = level * 0.10
            + if mouth != Mouth::Rest {
                (now * 7.0).sin() * 0.025
            } else {
                0.0
            };
        let b = (now - beat_t) / 0.3;
        if b < 1.0 {
            x += 0.09 * (std::f32::consts::PI * b).sin() * beat_amp;
        }
        // looks at you while you talk; you glance over while they do
        let host_talking = av.mouth != Mouth::Rest;
        av.look_left_to = if talking && !host_talking { 1.0 } else { 0.0 };
        p.head = Vec3::new(
            x,
            if host_talking { 0.25 } else { 0.0 },
            (now * 1.1).sin() * 0.02,
        );
        p.want = if talking { "Talk" } else { "Idle" }.into();
    }
}
