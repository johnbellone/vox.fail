//! Every frame: lip sync from the microphone, the body following the voice, the HUD, and the
//! state the control page shows.

#[allow(unused_imports)]
use super::*;

// ------------------------------------------------------------------------------------------
// Per-frame behaviour
// ------------------------------------------------------------------------------------------
pub(super) fn update_lipsync(
    mic: Res<Mic>,
    mut lip: ResMut<Lip>,
    mut av: ResMut<Avatar>,
    time: Res<Time>,
    cam: Res<webcam::Webcam>,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let frames = mic.0.frames.load(Ordering::Relaxed);
    let fresh = frames != lip.last_frames;
    lip.last_frames = frames;
    let mut f = mic.frame();
    if av.muted || av.hush {
        f.db = -100.0;
    }
    if lip.calibrate_until > 0.0 {
        if fresh && !av.muted {
            lip.floor.push(f.db);
        }
        if now >= lip.calibrate_until {
            lip.calibrate_until = 0.0;
            if let Some(floor) = lip.floor.finish() {
                let gate = (floor + 6.0).clamp(-70.0, -20.0);
                lip.picker.settings.gate_db = gate;
                av.status = format!("Gate set to {gate:.0} dB (room {floor:.0} dB)");
            } else {
                av.status = "No mic signal yet, keeping the saved gate".into();
            }
        }
    }
    av.mouth = lip.picker.update(f, dt);
    av.level = level01(f, &lip.picker.settings);
    let gate = lip.picker.settings.gate_db;
    let voiced = f.db > gate;
    // speaking / silent tracking for the Idle <-> Talk body switch
    if av.mouth != Mouth::Rest {
        if av.speaking_since.is_none() {
            av.speaking_since = Some(now);
        }
        av.silent_since = now;
    } else if now - av.silent_since > 0.6 {
        av.speaking_since = None;
    }
    // beats: every syllable / drum onset while bopping
    if f.onsets != av.last_onsets {
        av.last_onsets = f.onsets;
        if av.bop && voiced && now - av.last_beat > 0.18 {
            av.last_beat = now;
            av.beat_side = !av.beat_side;
        }
    }
    // body language from pitch and stress
    let speech_db = lip.picker.speech_db;
    let v = voice::VoiceFrame {
        pitch_hz: f.pitch_hz,
        clarity: f.clarity,
        f1: f.f1,
        f2: f.f2,
    };
    let ev = lip.prosody.update(v, f.db, f.onsets, dt, gate, speech_db);
    av.brows_up = lip.prosody.brows_up;
    av.liveliness = lip.prosody.liveliness;
    if let Some(strength) = ev.beat {
        av.talk_beat_t = now;
        av.talk_beat_amp = strength;
        av.talk_beat_left = !av.talk_beat_left;
    }
    if ev.question {
        av.question_until = now + 1.3;
        av.tilt_side = if av.rand01() < 0.5 { -1.0 } else { 1.0 };
    }
    // guided vowel calibration
    if let Some(mut vc) = lip.vcal.take() {
        let el = now - vc.stage_start;
        av.status = format!(
            "Vowel calibration {}/3: hold a long {}  ({:.0})",
            vc.stage + 1,
            VOWEL_PROMPTS[vc.stage],
            (VOWEL_STAGE_S - el).max(0.0).ceil()
        );
        if el > 0.6 && fresh && !av.muted {
            vc.cal.push(v);
        }
        if el < VOWEL_STAGE_S {
            lip.vcal = Some(vc);
        } else if let Some(r) = vc.cal.finish() {
            vc.got.push(r);
            vc.stage += 1;
            vc.stage_start = now;
            if vc.stage < 3 {
                lip.vcal = Some(vc);
            } else {
                let refs = VowelRefs {
                    ah: vc.got[0],
                    ee: vc.got[1],
                    oo: vc.got[2],
                };
                // sanity: ah opens the jaw most, and ee is clearly more spread than oo
                if refs.ah.0 > refs.ee.0.max(refs.oo.0) * 1.2 && refs.ee.1 > refs.oo.1 * 1.15 {
                    lip.picker.refs = refs;
                    av.status = format!(
                        "Vowels set: ah {:.0}/{:.0}  ee {:.0}/{:.0}  oo {:.0}/{:.0} Hz",
                        refs.ah.0, refs.ah.1, refs.ee.0, refs.ee.1, refs.oo.0, refs.oo.1
                    );
                    save_settings(&av, &lip, &cam);
                } else {
                    av.status = "Those vowels came out too similar; keeping the old ones. Press P to try again, close to the mic".into();
                }
            }
        } else {
            av.status = format!(
                "Didn't hear a clear {}; press P to try again",
                VOWEL_PROMPTS[vc.stage]
            );
        }
    }
    if let Some((_, until)) = av.face_override {
        if now > until {
            av.face_override = None;
        }
    }
    if !av.reactions {
        return;
    }
    // shout: clearly louder than your own talking level for a moment -> rage face and a jolt
    if voiced && f.db > lip.picker.speech_db + 7.0 && f.db > gate + 14.0 {
        let since = *av.loud_since.get_or_insert(now);
        if now - since > 0.18 && av.face_override.is_none() {
            av.face_override = Some((RAGE, now + 1.4));
            if av.emote.is_none() {
                av.emote = Some((Emote::Jolt, now));
            }
            av.status = "Reaction: shout".into();
        }
    } else {
        av.loud_since = None;
    }
    // doze off after a long silence, wake up startled
    if !av.asleep && lip.picker.silent_for > av.sleep_after {
        av.asleep = true;
        av.status = "Reaction: dozing off".into();
    }
    if av.asleep && voiced {
        av.asleep = false;
        av.face_override = Some((SURPRISED, now + 0.9));
        if av.emote.is_none() {
            av.emote = Some((Emote::Hop, now));
        }
        av.status = "Reaction: woke up".into();
    }
    if voiced {
        av.next_fidget = av.next_fidget.max(now + 12.0);
    }
}

pub(super) fn drive_body(
    mut av: ResMut<Avatar>,
    time: Res<Time>,
    clips: Res<Assets<AnimationClip>>,
    mut players: Players,
) {
    let now = time.elapsed_secs();
    let Some(pe) = av.player else { return };
    let speaking = av.speaking_since.map(|t| now - t > 0.35).unwrap_or(false);
    if av.gesture_active {
        // work out the end time once the clip is known
        if av.gesture_until == f32::MAX {
            let len = av
                .clips
                .get(&av.body)
                .and_then(|(_, h)| clips.get(h))
                .map(|c| c.duration())
                .unwrap_or(1.5);
            av.gesture_until = now + len.max(0.3);
        }
        if now < av.gesture_until && !(av.gesture_interruptible && speaking) {
            return;
        }
        av.gesture_active = false;
    }
    // idle fidget: a long idle now and then while you're quiet
    if av.reactions && !av.asleep && !speaking && now > av.next_fidget && av.base_loop.is_none() {
        let r = av.rand01();
        av.next_fidget = now + 20.0 + 25.0 * r;
        if av.clips.contains_key("Idle (long)") {
            play_gesture(&mut av, "Idle (long)", true, &mut players);
            return;
        }
    }
    let Ok((mut player, mut tr)) = players.get_mut(pe) else {
        return;
    };
    let base = av
        .base_loop
        .clone()
        .filter(|b| av.clips.contains_key(b))
        .unwrap_or_else(|| "Idle".into());
    let want =
        if av.auto_talk && speaking && av.base_loop.is_none() && av.clips.contains_key("Talk") {
            "Talk".to_string()
        } else {
            base
        };
    if av.body != want {
        if let Some((node, _)) = av.clips.get(&want).cloned() {
            tr.play(&mut player, node, Duration::from_millis(350))
                .repeat();
            av.body = want;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn update_hud(
    av: Res<Avatar>,
    lip: Res<Lip>,
    mic: Res<Mic>,
    cam: Res<webcam::Webcam>,
    cmds: Res<control::Commands>,
    (chat, caps, stages, guest): (
        Res<chat::Chat>,
        Res<bubbles::Captions>,
        Res<stage::Stages>,
        Res<guest::Guest>,
    ),
    wardrobe: Res<wardrobe::Wardrobe>,
    mut q: Query<(&mut Text, &mut Visibility), With<HudText>>,
) {
    let Ok((mut text, mut vis)) = q.single_mut() else {
        return;
    };
    *vis = if av.hud {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if !av.hud {
        return;
    }
    let f = mic.frame();
    let bars = ((f.db + 80.0) / 80.0 * 24.0).clamp(0.0, 24.0) as usize;
    let gate = ((lip.picker.settings.gate_db + 80.0) / 80.0 * 24.0).clamp(0.0, 24.0) as usize;
    let meter: String = (0..24)
        .map(|i| {
            if i == gate {
                '|'
            } else if i < bars {
                '#'
            } else {
                '.'
            }
        })
        .collect();
    let mic_line = if let Some(e) = mic.0.error.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        format!("Mic: {e}")
    } else if !mic.0.running.load(Ordering::Relaxed) {
        "Mic: starting...".into()
    } else {
        format!(
            "Mic: {}{}",
            mic.0.device.lock().unwrap_or_else(|e| e.into_inner()),
            if av.muted { "  [MUTED]" } else { "" }
        )
    };
    let onoff = |b: bool| if b { "on" } else { "off" };
    text.0 = format!(
        "{} | face {} | lip {} | mouth {}\nreactions {} | bop {} | body language {} | body {}{}\n{}\n{}\n{}\n{} | retro {}\n{}\n{}\n[{}] {:.0} dB  gate {:.0} dB\n{}\n{}\n\n1-0 faces | F1-F8 gestures | F9-F12 own clips | Tab character\nL lip style | R reactions | D bop | T body language | P vowel calibration\nQ test throw | S speech bubbles | Y retro sprite mode | A next question / end it | E clip that\nZ X C J K U I O nod shake tilt hop spin bow shiver lean\nB background | V framing | W webcam | G talk gestures | M mute | N calibrate | [ ] gate | H hide\nControl page (every clip): {}",
        av.models[av.model].0,
        EXPRESSIONS[av.expression].name,
        lip.picker.mode.name(),
        av.mouth.label(),
        onoff(av.reactions),
        onoff(av.bop),
        onoff(av.prosody_on),
        av.body,
        if av.asleep { " (asleep)" } else { "" },
        cam.status_line(),
        chat.status_line(),
        caps.status_line(),
        if stages.retro {
            format!("on ({} px)", stages.scale)
        } else {
            "off".into()
        },
        {
            let q = guest.queue.lock().unwrap_or_else(|e| e.into_inner());
            let approved = q
                .items
                .iter()
                .filter(|x| x.status == questions::Status::Approved)
                .count();
            format!(
                "Questions: {} | {} waiting, {} approved | ask page {}{}",
                if q.open { "open" } else { "closed" },
                q.waiting().len(),
                approved,
                guest.ask_url(),
                if guest.status.is_empty() {
                    String::new()
                } else {
                    format!(" | {}", guest.status)
                }
            )
        },
        wardrobe.summary(),
        mic_line,
        meter,
        f.db,
        lip.picker.settings.gate_db,
        voice_line(&f, &lip),
        av.status,
        cmds.1
    );
}

/// "Voice: 142 Hz (+2.1 st) | F1 650 F2 1180 = ah" for the HUD
pub(super) fn voice_line(f: &Frame, lip: &Lip) -> String {
    if f.pitch_hz <= 0.0 || f.clarity < 0.6 || f.db < lip.picker.settings.gate_db {
        return format!(
            "Voice: -   (vowels {})",
            if lip.picker.use_formants {
                "from formants"
            } else {
                "from tone only"
            }
        );
    }
    let mut s = format!(
        "Voice: {:.0} Hz ({:+.1} st)",
        f.pitch_hz, lip.prosody.rel_st
    );
    if f.f1 > 0.0 && f.f2 > 0.0 {
        let (v, d) = lip.picker.refs.nearest(f.f1, f.f2);
        let name = match v {
            voice::Vowel::Ah => "ah",
            voice::Vowel::Ee => "ee",
            voice::Vowel::Oo => "oo",
        };
        s += &format!(
            " | F1 {:.0} F2 {:.0} = {}{}",
            f.f1,
            f.f2,
            name,
            if d > 0.45 { "?" } else { "" }
        );
    }
    s
}

/// Snapshot for the control page's GET /state (clip list, current settings).
#[allow(clippy::too_many_arguments)]
pub(super) fn publish_state(
    av: Res<Avatar>,
    lip: Res<Lip>,
    cam: Res<webcam::Webcam>,
    (chat, caps, stages, guest): (
        Res<chat::Chat>,
        Res<bubbles::Captions>,
        Res<stage::Stages>,
        Res<guest::Guest>,
    ),
    (wardrobe, moments, polls, vfx, co, sh, cl, sfx, hk, bev, lit, crowd, parts, fcap): (
        Res<wardrobe::Wardrobe>,
        Res<events::Moments>,
        Res<poll::Polls>,
        Res<voicefx::VoiceFx>,
        Res<cohost::Cohost>,
        Res<show::Show>,
        Res<clips::Clips>,
        Res<fx::Fx>,
        Res<hotkeys::Hotkeys>,
        Res<bevel::Bevel>,
        Res<lighting::Studio>,
        Res<crowd::Crowd>,
        Res<particles::Particles>,
        Res<facecap::FaceCap>,
    ),
    state: Res<control::State>,
    (netr, worldr): (Res<net::Net>, Res<world::WorldClient>),
    mut last: Local<f32>,
    mut fps: Local<f32>,
    real: Res<Time<Real>>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs();
    let dt = real.delta_secs();
    if dt > 0.0 {
        *fps = if *fps == 0.0 {
            1.0 / dt
        } else {
            *fps * 0.95 + 0.05 / dt
        };
    }
    if now - *last < 0.25 {
        return;
    }
    *last = now;
    // pieces that come as JSON text
    let raw = |s: String| serde_json::from_str::<serde_json::Value>(&s).unwrap_or(serde_json::Value::Null);
    let ask_login = match &guest.ask {
        Some(a) if a.shared.cfg.lock().unwrap_or_else(|e| e.into_inner()).twitch_client_id.is_some() => "twitch",
        Some(a) if a.shared.cfg.lock().unwrap_or_else(|e| e.into_inner()).open_names => "names",
        Some(_) => "none",
        None => "off",
    };
    let on_stage = guest.stage.as_ref().map(|s| {
        serde_json::json!({"id": s.q.id, "user": s.name, "phase": match s.phase {
            guest::Phase::Loading | guest::Phase::Entering(_) => "arriving",
            guest::Phase::Speaking => "speaking",
            guest::Phase::After(_) => "waiting for your answer",
            guest::Phase::Leaving(..) => "leaving",
        }})
    });
    let json = serde_json::json!({
        "model": av.models[av.model].0,
        "face": av.expression,
        "lip": lip.picker.mode.name(),
        "lips": LIP_MODES.iter().map(|m| m.name()).collect::<Vec<_>>(),
        "reactions": av.reactions,
        "bop": av.bop,
        "webcam": cam.enabled,
        "webcam_status": cam.status_line(),
        "body_language": av.prosody_on,
        "formants": lip.picker.use_formants,
        "bubbles": caps.enabled,
        "retro": stages.retro,
        "retro_scale": stages.scale,
        "chat": chat.enabled,
        "twitch": chat.channel.as_deref().unwrap_or(""),
        "chat_status": chat.status_line(),
        "caption_source": caps.source,
        "ask_url": guest.ask_url(),
        "ask_login": ask_login,
        "ask_names": guest.ask.as_ref().is_some_and(|a| a.shared.cfg.lock().unwrap_or_else(|e| e.into_inner()).open_names),
        "guest": on_stage,
        "guest_status": guest.status,
        "wardrobe": raw(wardrobe.json()),
        "raid_crowd": moments.raid_crowd,
        "sub_party": moments.sub_party,
        "poll": raw(polls.json(now)),
        "voice": {"laugh": vfx.det.laugh_on, "sing": vfx.det.sing_on, "gags": vfx.gags_on, "gag_count": vfx.gags.len(), "last": vfx.last, "singing": av.singing},
        "cohost_on": co.on,
        "cohost_status": co.status,
        "net": {
            "role": match netr.role { Some(net::Role::Host) => "host", Some(net::Role::Viewer) => "viewer", Some(net::Role::Guest) => "guest", None => "" },
            "guest_on": netr.guest_on,
            "status": netr.status,
            "relay": netr.relay,
            "session": netr.session,
            "code": netr.code,
            "viewers": netr.viewers,
            "guest": netr.guest,
        },
        "world": world::json(&worldr),
        "show": {"set": sh.set, "sets": show::SETS, "director": sh.director, "press": sh.press},
        "shorts": {"on": cl.on, "auto": cl.auto, "comics": cl.comics, "cards": cl.cards, "status": *cl.status.lock().unwrap_or_else(|e| e.into_inner()), "saved": *cl.saved.lock().unwrap_or_else(|e| e.into_inner())},
        "models": av.models.iter().map(|m| m.0.clone()).collect::<Vec<_>>(),
        "mic_level": (av.level * 100.0).round() / 100.0,
        "problems": applog::problems(),
        "setup_done": av.setup_done,
        "sfx": sfx.json(),
        "hotkeys": hk.json(),
        "bevel": bev.json(),
        "light": lit.json(),
        "crowd": crowd.json(),
        "effects": parts.json(sh.live_set()),
        "facecap": fcap.json(now),
        "idle_life": av.idle_life,
        "presets": av.presets,
        // voice to lips: 10 ms analysis hop, 15 ms envelope attack, ~1.5 frames to the screen
        "lip_latency_ms": (10.0 + 15.0 + 1500.0 / fps.max(1.0)).round(),
        "fps": fps.round(),
        "rec": cl.on,
        "asleep": av.asleep,
        "body": av.body,
        "loop": av.base_loop.as_deref().unwrap_or("Idle"),
        "emotes": EMOTES.iter().map(|e| e.1).collect::<Vec<_>>(),
        "own": av.own_clips,
        "clips": av.clip_order,
    })
    .to_string();
    *state.0.lock().unwrap_or_else(|e| e.into_inner()) = json;
}
