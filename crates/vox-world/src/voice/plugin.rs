//! The voice plugin: the player's voice settings (saved as `voice.json`), the microphone and
//! the speakers while voice is on, and the link between them and the world.
//!
//! Voice is off unless the player turns it on, so nothing opens a device by default. Add
//! [`VoicePlugin`] after [`crate::input::InputPlugin`] (it reads the actions). On the browser
//! it is a stub: no devices, `available` stays false, and incoming voice is dropped.
//!
//! Sending: [`VoiceSettings::ptt`] on means the mic only sends while the push-to-talk action is
//! held; off means an open mic with the silence gate. Receiving: each packet from the server is
//! checked, its gains are worked out from where the speaker was last seen and where you stand
//! and face ([`super::mix`]), and it is handed to the speakers.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use vox_core::wire::{VOICE_NEARBY, VOICE_PARTY, VoicePacket};

use super::lipsync::MouthDrivers;
use super::mix;
use crate::client::WorldClient;
#[cfg(not(target_arch = "wasm32"))]
use crate::input::{Action, Actions};
#[cfg(not(target_arch = "wasm32"))]
use vox_voice::Packet;

/// How far a nearby voice carries, in blocks, when the world doesn't say: the server's default
/// `--voice-radius`. A world sends its own with its settings (`voice_radius`).
pub const NEARBY_RADIUS: f32 = 32.0;
/// The farthest radius a world may give (anything larger is taken as this).
pub const RADIUS_MAX: f32 = 512.0;

/// The nearby radius from a world's settings: a finite number above zero, at most
/// [`RADIUS_MAX`]; anything else (missing, zero, negative, not a number) means the default.
pub fn radius_from(v: Option<&serde_json::Value>) -> f32 {
    v.and_then(|v| v.as_f64())
        .map(|r| r as f32)
        .filter(|r| r.is_finite() && *r > 0.0)
        .map_or(NEARBY_RADIUS, |r| r.min(RADIUS_MAX))
}
/// After a failed device start, wait this long (seconds) before trying again.
#[cfg(not(target_arch = "wasm32"))]
const RETRY_EVERY: f64 = 5.0;
/// Speakers stay in [`VoiceState::speaking`] this long (seconds) after their last packet.
#[cfg(not(target_arch = "wasm32"))]
const KEEP_SPEAKING: f64 = 10.0;
/// Packets the server sent that the plugin has not handled yet. Only a stub or a missing plugin
/// lets this fill up; the oldest go first (about 4 s of speech).
pub(crate) const VOICE_QUEUE_MAX: usize = 200;

/// The player's voice settings. Everything but `muted` and `available` is saved.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceSettings {
    /// voice is on: the microphone and speakers are open. Off by default.
    pub enabled: bool,
    /// the microphone and speaker to use (a part of the device's name); `None` is the default
    pub device_in: Option<String>,
    pub device_out: Option<String>,
    /// mic gain, 0..=4
    pub in_gain: f32,
    /// master volume for the voices you hear, 0..=1
    pub out_gain: f32,
    /// push to talk (send only while the action is held); off is an open mic with the gate
    pub ptt: bool,
    /// [`VOICE_NEARBY`] or [`VOICE_PARTY`]
    pub channel: u8,
    /// players you don't hear (by server id); not saved, ids change with each visit
    #[serde(skip)]
    pub muted: HashSet<u32>,
    /// both devices are open right now (set by the plugin, never saved)
    #[serde(skip)]
    pub available: bool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings {
            enabled: false,
            device_in: None,
            device_out: None,
            in_gain: 1.0,
            out_gain: 1.0,
            ptt: true,
            channel: VOICE_NEARBY,
            muted: HashSet::new(),
            available: false,
        }
    }
}

impl VoiceSettings {
    /// Keeps values in range and device names printable, whatever the file says.
    fn sanitized(mut self) -> Self {
        let clean = |s: Option<String>| {
            s.map(|s| s.chars().filter(|c| !c.is_control()).collect::<String>())
                .filter(|s| !s.trim().is_empty())
        };
        self.device_in = clean(self.device_in);
        self.device_out = clean(self.device_out);
        self.in_gain = if self.in_gain.is_finite() { self.in_gain.clamp(0.0, 4.0) } else { 1.0 };
        self.out_gain = if self.out_gain.is_finite() { self.out_gain.clamp(0.0, 1.0) } else { 1.0 };
        if self.channel != VOICE_NEARBY && self.channel != VOICE_PARTY {
            self.channel = VOICE_NEARBY;
        }
        self
    }
}

/// The settings as JSON (what is saved).
pub fn to_json(s: &VoiceSettings) -> String {
    serde_json::to_string_pretty(s).unwrap_or_default()
}

/// Settings from JSON. Anything missing or unreadable keeps its default, so a broken file
/// leaves voice off.
pub fn from_json(text: &str) -> VoiceSettings {
    serde_json::from_str::<VoiceSettings>(text).unwrap_or_default().sanitized()
}

#[cfg(not(target_arch = "wasm32"))]
const FILE: &str = "voice.json";

/// The saved settings, or the defaults.
pub fn load() -> VoiceSettings {
    #[cfg(not(target_arch = "wasm32"))]
    {
        match std::fs::read_to_string(crate::files::user_dir().join(FILE)) {
            Ok(t) => from_json(&t),
            Err(_) => VoiceSettings::default(),
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        VoiceSettings::default()
    }
}

/// Keep the settings for next time. The browser build has no voice to save.
pub fn save(s: &VoiceSettings) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = crate::files::user_dir().join(FILE);
        if let Err(e) = std::fs::write(&path, to_json(s)) {
            crate::note!("[voice] can't save {}: {e}", path.display());
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = s;
    }
}

/// Who is speaking and what voice is doing, for the HUD and the menu.
#[derive(Resource, Default, Debug)]
pub struct VoiceState {
    /// player id -> when their last packet was heard (`WorldClient::now`, seconds)
    pub speaking: HashMap<u32, f64>,
    /// packets went out within the last [`TRANSMIT_HOLD`] seconds (they leave in bursts, so
    /// this is held, or the HUD marker would flicker)
    pub transmitting: bool,
    /// `WorldClient::now` when the last packet went out
    pub last_sent: f64,
    /// what happened last with the devices (an error, or "On")
    pub status: String,
}

/// How long the HUD keeps saying you are transmitting after the last packet, in seconds.
pub const TRANSMIT_HOLD: f64 = 0.3;

/// The channel after this one: nearby and party take turns. An unknown channel goes to nearby.
pub fn next_channel(channel: u8) -> u8 {
    if channel == VOICE_NEARBY { VOICE_PARTY } else { VOICE_NEARBY }
}

/// Whether a packet from `player` should be played: not muted, not your own, and a packet the
/// wire rules call valid.
pub fn keep_packet(muted: &HashSet<u32>, you: Option<u32>, player: u32, p: &VoicePacket) -> bool {
    !muted.contains(&player) && you != Some(player) && p.valid()
}

/// Left and right gains for a voice. `listener` is your position (blocks) and facing (yaw);
/// `speaker` is where they were last seen, if known; `radius` is how far nearby voice carries in
/// this world. Party voices play in both ears even when the position is unknown; a nearby voice
/// with no known position is not played (`None`).
pub fn gains_for(listener: ([f32; 3], f32), speaker: Option<[f32; 3]>, channel: u8, radius: f32) -> Option<(f32, f32)> {
    let at = match speaker {
        Some(at) => at,
        None if channel == VOICE_PARTY => listener.0,
        None => return None,
    };
    Some(mix::gains(listener.0, listener.1, at, channel, radius))
}

/// The wire packet as the codec's packet (the same fields; the data is copied).
#[cfg(not(target_arch = "wasm32"))]
fn packet_of(p: &VoicePacket) -> Packet {
    Packet {
        seq: p.seq,
        samples: p.samples,
        pred: p.pred,
        index: p.index,
        data: p.data.clone(),
    }
}

/// Adds voice: settings, state, and the systems that run them. Needs [`crate::input::InputPlugin`].
pub struct VoicePlugin;

impl Plugin for VoicePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(load()).init_resource::<VoiceState>().init_resource::<MouthDrivers>();
        #[cfg(not(target_arch = "wasm32"))]
        app.init_resource::<native::Devices>()
            .add_systems(
                Update,
                (native::devices, native::voice, native::save_settings)
                    .chain()
                    .after(crate::client::receive),
            )
            .add_systems(Update, crate::client::local_mouth.after(native::voice));
        #[cfg(target_arch = "wasm32")]
        app.add_systems(Update, stub::drop_inbox.after(crate::client::receive));
    }
}

#[cfg(target_arch = "wasm32")]
mod stub {
    use super::*;

    /// The browser has no voice: forget what the server sends so the queue stays empty.
    pub fn drop_inbox(mut w: ResMut<WorldClient>) {
        w.voice_in.clear();
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;

    use crate::voice::capture::Capture;
    use crate::voice::device::Status;
    use crate::voice::playback::Playback;

    /// The open devices, and when the next start may be tried.
    #[derive(Resource, Default)]
    pub struct Devices {
        capture: Option<Capture>,
        playback: Option<Playback>,
        retry_at: f64,
    }

    /// Opens the devices while voice is on, and closes them when it is off. Opening never waits:
    /// the speakers start first, and the microphone only once they are open (so the mic's
    /// indicator never lights for nothing); each is polled every frame until it is open or has
    /// failed. A failure closes both and tries again after [`RETRY_EVERY`].
    pub fn devices(
        mut d: ResMut<Devices>,
        w: Res<WorldClient>,
        mut settings: ResMut<VoiceSettings>,
        mut state: ResMut<VoiceState>,
    ) {
        if !settings.enabled {
            d.capture = None;
            d.playback = None;
            d.retry_at = 0.0;
            state.transmitting = false;
            if settings.available {
                settings.available = false;
            }
            return;
        }
        let now = w.now();
        let fail = |d: &mut Devices, state: &mut VoiceState, why: String| {
            crate::note!("[voice] {why}");
            state.status = why;
            d.capture = None;
            d.playback = None;
            d.retry_at = now + RETRY_EVERY;
        };
        let ok = match (&d.playback, &d.capture) {
            (None, _) => {
                if now >= d.retry_at {
                    match Playback::start(settings.device_out.as_deref()) {
                        Ok(p) => {
                            d.playback = Some(p);
                            state.status = "Starting".into();
                        }
                        Err(e) => fail(&mut d, &mut state, e),
                    }
                }
                false
            }
            (Some(p), None) => {
                match p.status() {
                    Status::Starting => {}
                    Status::Failed(e) => fail(&mut d, &mut state, e),
                    Status::Ready(_) => match Capture::start(settings.device_in.as_deref()) {
                        Ok(c) => d.capture = Some(c),
                        Err(e) => fail(&mut d, &mut state, e),
                    },
                }
                false
            }
            (Some(_), Some(c)) => match c.status() {
                Status::Starting => false,
                Status::Failed(e) => {
                    fail(&mut d, &mut state, e);
                    false
                }
                Status::Ready(name) => {
                    if state.status != "On" {
                        crate::note!("[voice] microphone: {name}");
                        state.status = "On".into();
                    }
                    true
                }
            },
        };
        if settings.available != ok {
            settings.available = ok;
        }
    }

    /// Changes the channel on the key, sends the mic's packets, and plays the voices that came
    /// in.
    pub fn voice(
        mut w: ResMut<WorldClient>,
        actions: Res<Actions>,
        mut settings: ResMut<VoiceSettings>,
        mut state: ResMut<VoiceState>,
        mut mouths: ResMut<MouthDrivers>,
        d: Res<Devices>,
    ) {
        // taken first, so packets are dropped (not kept) while voice is off
        let inbox = std::mem::take(&mut w.voice_in);
        let now = w.now();
        // mouths follow every speaker's voice, muted or not; your own packets are not heard
        // back, so they are skipped here (your mouth is fed from the mic below)
        for (player, packet) in &inbox {
            if w.you == Some(*player) || !packet.valid() {
                continue;
            }
            if let Some(s) = packet_of(packet).decode() {
                mouths.feed_remote(*player, &s, now);
            }
        }
        mouths.prune(now);
        if !settings.enabled || !settings.available {
            state.transmitting = false;
            return;
        }
        if actions.just_pressed(Action::VoiceChannel) {
            settings.channel = next_channel(settings.channel);
            let name = if settings.channel == VOICE_PARTY { "party" } else { "nearby" };
            w.notice(format!("Voice: {name}"));
        }
        if let Some(c) = &d.capture {
            c.set_gain(settings.in_gain);
            // with push to talk off the mic is open: the packetizer's gate decides
            let ptt = settings.ptt.then(|| actions.pressed(Action::PushToTalk));
            let packets = c.drain(ptt);
            // your mouth follows the packets you would send (under push to talk, only while held)
            for p in &packets {
                if let Some(s) = p.decode() {
                    mouths.feed_local(&s, now);
                }
            }
            let ready = w.voice_ready();
            if ready && !packets.is_empty() {
                state.last_sent = now;
            }
            state.transmitting = ready && now - state.last_sent < TRANSMIT_HOLD;
            if ready {
                for p in packets {
                    w.send_voice(VoicePacket {
                        channel: settings.channel,
                        seq: p.seq,
                        samples: p.samples,
                        pred: p.pred,
                        index: p.index,
                        data: p.data,
                    });
                }
            }
        }
        if let Some(p) = &d.playback {
            p.set_volume(settings.out_gain);
            // a muted player stops at once, not after what was already queued for them
            p.forget(&settings.muted);
            let listener = w.listener();
            for (player, packet) in inbox {
                if !keep_packet(&settings.muted, w.you, player, &packet) {
                    continue;
                }
                let Some(g) = gains_for(listener, w.place_of(player), packet.channel, w.voice_radius) else {
                    continue;
                };
                p.hear(player, &packet_of(&packet), g);
                state.speaking.insert(player, now);
            }
        }
        state.speaking.retain(|_, t| now - *t < KEEP_SPEAKING);
    }

    /// Saves the settings when they change (the first look only records them).
    pub fn save_settings(settings: Res<VoiceSettings>, mut last: Local<Option<String>>) {
        let text = to_json(&settings);
        let prev = last.replace(text.clone());
        if prev.is_some() && prev.as_deref() != Some(text.as_str()) {
            save(&settings);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(channel: u8) -> VoicePacket {
        VoicePacket {
            channel,
            seq: 1,
            samples: vox_core::wire::VOICE_SAMPLES,
            pred: 0,
            index: 0,
            data: vec![0; vox_core::wire::VOICE_BYTES],
        }
    }

    #[test]
    fn voice_settings_round_trip_and_skip_muted_and_available() {
        let mut s = VoiceSettings {
            enabled: true,
            device_in: Some("USB Mic".into()),
            device_out: None,
            in_gain: 2.5,
            out_gain: 0.5,
            ptt: false,
            channel: VOICE_PARTY,
            ..Default::default()
        };
        s.muted.insert(7);
        s.available = true;
        let back = from_json(&to_json(&s));
        assert!(back.enabled);
        assert_eq!(back.device_in.as_deref(), Some("USB Mic"));
        assert_eq!(back.device_out, None);
        assert_eq!((back.in_gain, back.out_gain), (2.5, 0.5));
        assert!(!back.ptt);
        assert_eq!(back.channel, VOICE_PARTY);
        assert!(back.muted.is_empty(), "muted is not saved");
        assert!(!back.available, "available is not saved");
    }

    #[test]
    fn voice_defaults_are_off_and_push_to_talk() {
        let s = VoiceSettings::default();
        assert!(!s.enabled);
        assert!(s.ptt);
        assert_eq!(s.channel, VOICE_NEARBY);
        assert_eq!(from_json(&to_json(&s)), s);
        assert_eq!(from_json("not json"), s);
    }

    #[test]
    fn voice_bad_values_in_the_file_are_fixed() {
        let s = from_json(r#"{"enabled":true,"in_gain":99,"out_gain":-1,"channel":5,"device_in":"\u0007  "}"#);
        assert!(s.enabled);
        assert_eq!(s.in_gain, 4.0);
        assert_eq!(s.out_gain, 0.0);
        assert_eq!(s.channel, VOICE_NEARBY);
        assert_eq!(s.device_in, None);
    }

    #[test]
    fn voice_channel_toggles_between_nearby_and_party() {
        assert_eq!(next_channel(VOICE_NEARBY), VOICE_PARTY);
        assert_eq!(next_channel(VOICE_PARTY), VOICE_NEARBY);
        assert_eq!(next_channel(VOICE_PARTY), VOICE_NEARBY);
    }

    #[test]
    fn voice_inbox_skips_muted_own_and_invalid() {
        let mut muted = HashSet::new();
        muted.insert(3);
        let good = packet(VOICE_NEARBY);
        assert!(keep_packet(&muted, Some(1), 2, &good));
        assert!(!keep_packet(&muted, Some(1), 3, &good), "muted");
        assert!(!keep_packet(&muted, Some(1), 1, &good), "own voice");
        let mut bad = packet(VOICE_NEARBY);
        bad.data.pop();
        assert!(!keep_packet(&muted, Some(1), 2, &bad), "wrong size");
        assert!(!keep_packet(&muted, Some(1), 2, &packet(9)), "unknown channel");
    }

    #[test]
    fn voice_gains_use_the_last_known_place() {
        let me = ([0.0, 0.0, 0.0], 0.0);
        // a party voice with no known place still plays in both ears
        assert_eq!(gains_for(me, None, VOICE_PARTY, NEARBY_RADIUS), Some((1.0, 1.0)));
        // a nearby voice with no known place is not played
        assert_eq!(gains_for(me, None, VOICE_NEARBY, NEARBY_RADIUS), None);
        // a nearby voice far away is silent
        assert_eq!(gains_for(me, Some([500.0, 0.0, 0.0]), VOICE_NEARBY, NEARBY_RADIUS), Some((0.0, 0.0)));
        // a voice dead ahead (yaw 0 faces -Z) within two blocks: full volume, equal power per ear
        let (l, r) = gains_for(me, Some([0.0, 0.0, -1.0]), VOICE_NEARBY, NEARBY_RADIUS).unwrap();
        let half = std::f32::consts::FRAC_1_SQRT_2;
        assert!((l - half).abs() < 1e-5 && (r - half).abs() < 1e-5, "{l} {r}");
        // the world's radius is the one used: 40 blocks away is silent at 32, heard at 64
        assert_eq!(gains_for(me, Some([40.0, 0.0, 0.0]), VOICE_NEARBY, 32.0), Some((0.0, 0.0)));
        let (l, r) = gains_for(me, Some([40.0, 0.0, 0.0]), VOICE_NEARBY, 64.0).unwrap();
        assert!(l + r > 0.1, "{l} {r}");
    }

    #[test]
    fn voice_radius_from_the_worlds_settings() {
        use serde_json::json;
        assert_eq!(radius_from(None), NEARBY_RADIUS);
        assert_eq!(radius_from(Some(&json!(48))), 48.0);
        assert_eq!(radius_from(Some(&json!(12.5))), 12.5);
        // nonsense is the default, too much is capped
        for bad in [json!(0), json!(-3), json!("far"), json!(null)] {
            assert_eq!(radius_from(Some(&bad)), NEARBY_RADIUS, "{bad}");
        }
        assert_eq!(radius_from(Some(&json!(1e9))), RADIUS_MAX);
    }
}
