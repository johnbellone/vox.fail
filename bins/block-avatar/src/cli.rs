//! The command line: arguments, and analysing a recording to a lip-sync file without the window.

#[allow(unused_imports)]
use super::*;

// ------------------------------------------------------------------------------------------
// main
// ------------------------------------------------------------------------------------------
pub(super) struct Args {
    pub(super) model: Option<String>,
    pub(super) mic: Option<String>,
    pub(super) wav: Option<String>,
    pub(super) transparent: bool,
    pub(super) port: u16,
    pub(super) webcam: bool,
    pub(super) webcam_size: Option<(u32, u32)>,
    pub(super) webcam_fps: Option<u32>,
    pub(super) webcam_dump: Option<String>,
    pub(super) ask_port: Option<u16>,
    pub(super) cohost_wav: Option<String>,
    pub(super) no_browser: bool,
    /// --snapshot <png> [--snapshot-set <set>] [--snapshot-fx <burst>]: one picture, then quit
    /// (visual tests)
    pub(super) snapshot: Option<(String, Option<String>, Option<String>)>,
    /// --watch http://relay:7779/<session> [--code ABC123]: watch someone's show
    pub(super) watch: Option<(String, String)>,
    /// --join http://relay:7779/<session> --code ABC123 [--name Sam]: be a show's guest
    pub(super) join: Option<(String, String, String)>,
    /// --world ws://host:7780 [--name Sam]: walk and build in a Vox world
    pub(super) world: Option<String>,
    pub(super) name: String,
}

pub(super) fn parse_args() -> Option<Args> {
    let a: Vec<String> = std::env::args().collect();
    let get = |flag: &str| {
        a.iter()
            .position(|x| x == flag)
            .and_then(|i| a.get(i + 1))
            .cloned()
    };
    if a.iter().any(|x| x == "--list-mics") {
        list_mics();
        return None;
    }
    if let Some(i) = a.iter().position(|x| x == "--analyze") {
        let (inp, out) = (
            a.get(i + 1).cloned().unwrap_or_default(),
            a.get(i + 2).cloned().unwrap_or("lipsync.csv".into()),
        );
        let mode = a
            .get(i + 3)
            .and_then(|m| LipMode::parse(m))
            .unwrap_or(LipMode::Visemes);
        let formants = !a.iter().any(|x| x == "--no-formants");
        if let Err(e) = analyze_file(&inp, &out, mode, formants) {
            eprintln!("analyze failed: {e}");
            std::process::exit(1);
        }
        return None;
    }
    Some(Args {
        model: get("--model"),
        mic: get("--mic"),
        wav: get("--wav"),
        transparent: a.iter().any(|x| x == "--transparent"),
        port: get("--port").and_then(|p| p.parse().ok()).unwrap_or(7777),
        webcam: a.iter().any(|x| x == "--webcam"),
        webcam_size: get("--webcam-size").and_then(|s| parse_size(&s)),
        webcam_fps: get("--webcam-fps").and_then(|f| f.parse().ok()),
        ask_port: get("--ask-port").and_then(|p| p.parse().ok()),
        cohost_wav: get("--cohost-wav"),
        no_browser: a.iter().any(|x| x == "--no-browser")
            || std::env::var_os("BLOCK_AVATAR_NO_BROWSER").is_some(),
        webcam_dump: get("--webcam-dump"),
        snapshot: get("--snapshot").map(|p| (p, get("--snapshot-set"), get("--snapshot-fx"))),
        watch: get("--watch").map(|u| (u, get("--code").unwrap_or_default())),
        join: get("--join").map(|u| (u, get("--code").unwrap_or_default(), get("--name").unwrap_or_default())),
        world: get("--world"),
        name: get("--name")
            .or_else(|| std::env::var("USER").ok())
            .or_else(|| std::env::var("USERNAME").ok())
            .unwrap_or_else(|| "Player".into()),
    })
}

pub(super) fn parse_size(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

/// Offline mode: run the same analyzer and picker over a WAV and write a 60 fps viseme timeline.
pub(super) fn analyze_file(inp: &str, out: &str, mode: LipMode, formants: bool) -> Result<(), String> {
    let (rate, ch, samples) = lipsync::read_wav(inp)?;
    let mut an = Analyzer::new(rate);
    let mut frames = Vec::new();
    an.process(&samples, ch, |f| frames.push(f));
    // calibrate the gate from the quietest stretch, as the live app does at startup
    let mut nf = NoiseFloor::new();
    for f in &frames {
        nf.push(f.db);
    }
    let floor = nf.finish().unwrap_or(-60.0);
    let mut settings = PickerSettings::default();
    settings.gate_db = (floor + 6.0).clamp(-70.0, -20.0);
    let mut picker = LipSync::new(settings, mode);
    picker.use_formants = formants;
    // use your calibrated vowels if the app has saved some
    if let Some(r) = load_settings()
        .get("vowels")
        .and_then(|v| VowelRefs::from_setting(v))
    {
        picker.refs = r;
        crate::note!("using calibrated vowels from {}", settings_path().display());
    }
    let mut csv = String::from(
        "time_s,db,bright,low,high,zcr,pitch_hz,clarity,f1,f2,mouth,level,pitch_st,event\n",
    );
    let mut prosody = Prosody::default();
    let total = frames.len() as f32 * 0.01;
    let mut t = 0.0f32;
    while t < total {
        let f = frames[((t / 0.01) as usize).min(frames.len() - 1)];
        let v = picker.update(f, 1.0 / 60.0);
        let vf = voice::VoiceFrame {
            pitch_hz: f.pitch_hz,
            clarity: f.clarity,
            f1: f.f1,
            f2: f.f2,
        };
        let ev = prosody.update(
            vf,
            f.db,
            f.onsets,
            1.0 / 60.0,
            picker.settings.gate_db,
            picker.speech_db,
        );
        let event = match (ev.question, ev.beat) {
            (true, _) => "question".to_string(),
            (_, Some(s)) => format!("beat {s:.2}"),
            _ => String::new(),
        };
        csv += &format!(
            "{t:.4},{:.1},{:.3},{:.3},{:.3},{:.3},{:.1},{:.2},{:.0},{:.0},{},{:.3},{:.1},{}\n",
            f.db,
            f.bright,
            f.low,
            f.high,
            f.zcr,
            f.pitch_hz,
            f.clarity,
            f.f1,
            f.f2,
            v.label(),
            level01(f, &picker.settings),
            prosody.rel_st,
            event
        );
        t += 1.0 / 60.0;
    }
    std::fs::write(out, csv).map_err(|e| e.to_string())?;
    println!(
        "{} mode, gate {:.1} dB (floor {:.1}); wrote {out}",
        mode.name(),
        settings.gate_db,
        floor
    );
    Ok(())
}
