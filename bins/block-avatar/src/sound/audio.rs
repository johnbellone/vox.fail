//! The microphone: a dedicated thread owns the cpal stream and publishes the latest analysis
//! frame (also: a WAV file standing in for the mic, and --list-mics).

use crate::*;

// ------------------------------------------------------------------------------------------
// Microphone: a dedicated thread owns the cpal stream and publishes the latest analysis frame
// ------------------------------------------------------------------------------------------
#[derive(Default)]
pub(crate) struct MicShared {
    pub(crate) frame: Mutex<Frame>,
    pub(crate) frames: AtomicU32,
    pub(crate) running: AtomicBool,
    /// --wav playback waits for this, so a recording starts once the character is on screen
    pub(crate) go: AtomicBool,
    pub(crate) device: Mutex<String>,
    pub(crate) error: Mutex<Option<String>>,
    /// mono copy of the input for speech-to-text, when it's running
    pub(crate) tap: Mutex<Option<std::sync::mpsc::SyncSender<(u32, Vec<f32>)>>>,
    /// another mono copy, for sending your voice to a show (net.rs)
    pub(crate) net_tap: Mutex<Option<std::sync::mpsc::SyncSender<(u32, Vec<f32>)>>>,
    /// every frame since the app last looked (laugh / sing detection needs them all)
    pub(crate) hist: Mutex<Vec<Frame>>,
    /// set when this mic is replaced: the stream closes
    pub(crate) stop: AtomicBool,
}

impl MicShared {
    pub(crate) fn publish(&self, f: Frame) {
        *self.frame.lock().unwrap_or_else(|e| e.into_inner()) = f;
        self.frames.fetch_add(1, Ordering::Relaxed);
        let mut h = self.hist.lock().unwrap_or_else(|e| e.into_inner());
        if h.len() < 2000 {
            h.push(f);
        }
    }
    pub(crate) fn send_tap(&self, rate: u32, d: &[f32], ch: usize) {
        clips::program_mic(rate, d, ch);
        let mono = || -> Vec<f32> {
            d.chunks(ch.max(1))
                .map(|c| c.iter().sum::<f32>() / c.len() as f32)
                .collect()
        };
        if let Ok(g) = self.tap.try_lock() {
            if let Some(tx) = g.as_ref() {
                let _ = tx.try_send((rate, mono()));
            }
        }
        if let Ok(g) = self.net_tap.try_lock() {
            if let Some(tx) = g.as_ref() {
                let _ = tx.try_send((rate, mono()));
            }
        }
    }
}

#[derive(Resource, Clone)]
pub(crate) struct Mic(pub(crate) Arc<MicShared>);

impl Mic {
    pub(crate) fn frame(&self) -> Frame {
        *self.0.frame.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// All frames since the last call.
    pub(crate) fn drain(&self) -> Vec<Frame> {
        std::mem::take(&mut *self.0.hist.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

#[allow(deprecated)]
pub(crate) fn device_name(d: &cpal::Device) -> String {
    d.name().unwrap_or_else(|_| "unknown device".into())
}

/// Feed a WAV file into the analyzer in real time, looping. Handy for tuning without talking,
/// and for recording demos.
pub(crate) fn start_wav(path: String) -> Mic {
    let shared = Arc::new(MicShared::default());
    shared.frame.lock().unwrap_or_else(|e| e.into_inner()).db = -100.0;
    let s = shared.clone();
    std::thread::spawn(move || {
        let (rate, ch, samples) = match lipsync::read_wav(&path) {
            Ok(w) => w,
            Err(e) => {
                *s.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(format!("{path}: {e}"));
                return;
            }
        };
        *s.device.lock().unwrap_or_else(|e| e.into_inner()) = format!("file {path}");
        s.running.store(true, Ordering::Relaxed);
        let mut an = Analyzer::new(rate);
        let block = (rate as usize / 100) * ch;
        while !s.go.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        crate::note!("[wav] playback started at {epoch:.3}");
        let start = std::time::Instant::now();
        let mut pos = 0usize;
        let mut fed = 0usize;
        loop {
            let end = (pos + block).min(samples.len());
            s.send_tap(rate, &samples[pos..end], ch);
            an.process(&samples[pos..end], ch, |f| s.publish(f));
            fed += end - pos;
            pos = if end >= samples.len() { 0 } else { end };
            let due = Duration::from_secs_f64(fed as f64 / ch as f64 / rate as f64);
            if let Some(wait) = due.checked_sub(start.elapsed()) {
                std::thread::sleep(wait);
            }
        }
    });
    Mic(shared)
}

pub(crate) fn start_mic(want: Option<String>) -> Mic {
    start_input(want, true)
}

/// `fallback`: use the default input if no device matches `want` (not for a second mic).
pub(crate) fn start_input(want: Option<String>, fallback: bool) -> Mic {
    let shared = Arc::new(MicShared::default());
    shared.frame.lock().unwrap_or_else(|e| e.into_inner()).db = -100.0;
    let s = shared.clone();
    std::thread::Builder::new()
        .name("mic".into())
        .spawn(move || {
            let fail = |msg: String| {
                crate::problem!("[mic] {msg}");
                *s.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(msg);
            };
            let host = cpal::default_host();
            let device = match &want {
                Some(w) => host
                    .input_devices()
                    .ok()
                    .and_then(|mut it| {
                        it.find(|d| device_name(d).to_lowercase().contains(&w.to_lowercase()))
                    })
                    .or_else(|| {
                        if fallback {
                            host.default_input_device()
                        } else {
                            None
                        }
                    }),
                None => host.default_input_device(),
            };
            let Some(device) = device else {
                return fail("no input device found".into());
            };
            let config = match device.default_input_config() {
                Ok(c) => c,
                Err(e) => return fail(format!("can't read input config: {e}")),
            };
            let (rate, ch) = (config.sample_rate(), config.channels() as usize);
            *s.device.lock().unwrap_or_else(|e| e.into_inner()) =
                format!("{} ({} Hz, {} ch)", device_name(&device), rate, ch);
            crate::note!("[mic] using {}", s.device.lock().unwrap_or_else(|e| e.into_inner()));
            let mut analyzer = Analyzer::new(rate);
            let out = s.clone();
            let tap = s.clone();
            let publish = move |f: Frame| out.publish(f);
            let err_s = s.clone();
            let on_err = move |e: cpal::StreamError| {
                *err_s.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(format!("stream error: {e}"));
            };
            let stream_cfg: cpal::StreamConfig = config.config();
            let stream = match config.sample_format() {
                cpal::SampleFormat::F32 => device.build_input_stream(
                    &stream_cfg,
                    move |d: &[f32], _: &_| {
                        tap.send_tap(rate, d, ch);
                        analyzer.process(d, ch, &publish)
                    },
                    on_err,
                    None,
                ),
                cpal::SampleFormat::I16 => {
                    let mut buf = Vec::new();
                    device.build_input_stream(
                        &stream_cfg,
                        move |d: &[i16], _: &_| {
                            buf.clear();
                            buf.extend(d.iter().map(|v| *v as f32 / 32768.0));
                            tap.send_tap(rate, &buf, ch);
                            analyzer.process(&buf, ch, &publish)
                        },
                        on_err,
                        None,
                    )
                }
                cpal::SampleFormat::I32 => {
                    let mut buf = Vec::new();
                    device.build_input_stream(
                        &stream_cfg,
                        move |d: &[i32], _: &_| {
                            buf.clear();
                            buf.extend(d.iter().map(|v| *v as f32 / 2147483648.0));
                            tap.send_tap(rate, &buf, ch);
                            analyzer.process(&buf, ch, &publish)
                        },
                        on_err,
                        None,
                    )
                }
                cpal::SampleFormat::U16 => {
                    let mut buf = Vec::new();
                    device.build_input_stream(
                        &stream_cfg,
                        move |d: &[u16], _: &_| {
                            buf.clear();
                            buf.extend(d.iter().map(|v| (*v as f32 - 32768.0) / 32768.0));
                            tap.send_tap(rate, &buf, ch);
                            analyzer.process(&buf, ch, &publish)
                        },
                        on_err,
                        None,
                    )
                }
                other => return fail(format!("unsupported sample format {other:?}")),
            };
            let stream = match stream {
                Ok(st) => st,
                Err(e) => return fail(format!("can't open the microphone: {e}")),
            };
            if let Err(e) = stream.play() {
                return fail(format!("can't start the microphone: {e}"));
            }
            s.running.store(true, Ordering::Relaxed);
            // The stream lives as long as this thread: until this mic is replaced.
            while !s.stop.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        })
        .expect("spawn mic thread");
    Mic(shared)
}

pub(crate) fn list_mics() {
    let host = cpal::default_host();
    let default = host.default_input_device().map(|d| device_name(&d));
    match host.input_devices() {
        Ok(devs) => {
            for d in devs {
                let n = device_name(&d);
                println!(
                    "{} {n}",
                    if Some(&n) == default.as_ref() {
                        "*"
                    } else {
                        " "
                    }
                );
            }
        }
        Err(e) => println!("can't list input devices: {e}"),
    }
}

