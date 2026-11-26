//! A guest's voice as the face reads it: the recording analysed ahead of time.

#[allow(unused_imports)]
use super::*;

// ------------------------------------------------------------------------------------------
pub struct Playback {
    pos: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    pub device: Arc<Mutex<String>>,
}

impl Playback {
    /// Plays mono samples on the default output (or the first device whose name contains
    /// `want`). With no usable output it still runs the clock, so the guest still talks.
    pub fn start(samples: Arc<Vec<f32>>, rate: u32, want: Option<String>) -> Playback {
        Self::start_with(samples, rate, want, true)
    }
    /// The same, without a line in the log (sound effects).
    pub fn quiet(samples: Arc<Vec<f32>>, rate: u32) -> Playback {
        Self::start_with(samples, rate, None, false)
    }
    pub(super) fn start_with(samples: Arc<Vec<f32>>, rate: u32, want: Option<String>, log: bool) -> Playback {
        let pos = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let device = Arc::new(Mutex::new(String::from("starting")));
        let (p, st, dev) = (pos.clone(), stop.clone(), device.clone());
        std::thread::Builder::new()
            .name("guest-audio".into())
            .spawn(move || {
                let epoch = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0);
                if log {
                    crate::note!("[guest] audio started at {epoch:.3}");
                }
                // what has actually played goes into the program mix for clips, so a cut stays cut
                let mut sent = 0usize;
                let mut feed = |upto: usize| {
                    let upto = upto.min(samples.len());
                    if upto > sent {
                        crate::clips::program_play(rate, &samples[sent..upto]);
                        sent = upto;
                    }
                };
                match open_output(samples.clone(), rate, want, p.clone(), st.clone()) {
                    Ok((stream, name)) => {
                        *dev.lock().unwrap_or_else(|e| e.into_inner()) = name;
                        while !st.load(Ordering::Relaxed)
                            && (p.load(Ordering::Relaxed) as usize) < samples.len()
                        {
                            std::thread::sleep(Duration::from_millis(10));
                            feed(p.load(Ordering::Relaxed) as usize);
                        }
                        // let the device play out its buffer
                        std::thread::sleep(Duration::from_millis(150));
                        drop(stream);
                    }
                    Err(e) => {
                        *dev.lock().unwrap_or_else(|e| e.into_inner()) = format!("no sound output ({e}); timing only");
                        let t0 = std::time::Instant::now();
                        while !st.load(Ordering::Relaxed) {
                            let n = (t0.elapsed().as_secs_f64() * rate as f64) as u64;
                            p.store(n.min(samples.len() as u64), Ordering::Relaxed);
                            feed(n as usize);
                            if n as usize >= samples.len() {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(5));
                        }
                    }
                }
            })
            .expect("spawn guest audio thread");
        Playback { pos, stop, device }
    }
    /// Seconds of the recording played so far.
    pub fn played_s(&self, rate: u32) -> f32 {
        self.pos.load(Ordering::Relaxed) as f32 / rate as f32
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
    pub fn finished(&self, rate: u32) -> bool {
        self.stop.load(Ordering::Relaxed)
            || self.played_s(rate) > 3600.0
            || Arc::strong_count(&self.pos) == 1
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.stop();
    }
}

#[allow(deprecated)]
pub(super) fn open_output(
    samples: Arc<Vec<f32>>,
    rate: u32,
    want: Option<String>,
    pos: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
) -> Result<(cpal::Stream, String), String> {
    let host = cpal::default_host();
    let device = match &want {
        Some(w) => host
            .output_devices()
            .ok()
            .and_then(|mut it| {
                it.find(|d| {
                    d.name()
                        .map(|n| n.to_lowercase().contains(&w.to_lowercase()))
                        .unwrap_or(false)
                })
            })
            .or_else(|| host.default_output_device()),
        None => host.default_output_device(),
    }
    .ok_or("no output device")?;
    let name = device.name().unwrap_or_else(|_| "output".into());
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let (dev_rate, ch) = (config.sample_rate() as f64, config.channels() as usize);
    let step = rate as f64 / dev_rate;
    // fractional read position in the recording, advanced per output frame
    let mut at = 0.0f64;
    let mut next = move || -> f32 {
        if stop.load(Ordering::Relaxed) {
            return 0.0;
        }
        let i = at as usize;
        if i + 1 >= samples.len() {
            pos.store(samples.len() as u64, Ordering::Relaxed);
            return 0.0;
        }
        let f = (at - i as f64) as f32;
        let v = samples[i] + (samples[i + 1] - samples[i]) * f;
        at += step;
        pos.store(at as u64, Ordering::Relaxed);
        v
    };
    let err = |e: cpal::StreamError| crate::problem!("[guest] output error: {e}");
    let cfg: cpal::StreamConfig = config.config();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &cfg,
            move |d: &mut [f32], _: &_| {
                for fr in d.chunks_mut(ch) {
                    let v = next();
                    fr.iter_mut().for_each(|x| *x = v);
                }
            },
            err,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &cfg,
            move |d: &mut [i16], _: &_| {
                for fr in d.chunks_mut(ch) {
                    let v = (next() * 32767.0) as i16;
                    fr.iter_mut().for_each(|x| *x = v);
                }
            },
            err,
            None,
        ),
        cpal::SampleFormat::I32 => device.build_output_stream(
            &cfg,
            move |d: &mut [i32], _: &_| {
                for fr in d.chunks_mut(ch) {
                    let v = (next() as f64 * 2147483647.0) as i32;
                    fr.iter_mut().for_each(|x| *x = v);
                }
            },
            err,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_output_stream(
            &cfg,
            move |d: &mut [u16], _: &_| {
                for fr in d.chunks_mut(ch) {
                    let v = (next() * 32767.0 + 32768.0) as u16;
                    fr.iter_mut().for_each(|x| *x = v);
                }
            },
            err,
            None,
        ),
        other => return Err(format!("unsupported output format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, name))
}

// ------------------------------------------------------------------------------------------
// The recording, analysed up front so the face can follow the playback clock exactly
// ------------------------------------------------------------------------------------------
pub struct Voice {
    pub rate: u32,
    pub samples: Arc<Vec<f32>>,
    pub frames: Vec<Frame>,
    pub hop_s: f32,
    /// share of the speech heard by each frame (0..1), for revealing the words as they're said
    pub spoken: Vec<f32>,
    pub gate_db: f32,
}

impl Voice {
    pub fn analyse(rate: u32, samples: Vec<f32>) -> Voice {
        let mut an = Analyzer::new(rate);
        let mut frames = vec![];
        an.process(&samples, 1, |f| frames.push(f));
        let hop = (rate as f32 / 100.0).round().max(1.0);
        // gate from the recording itself: a little above its quietest stretch
        let mut dbs: Vec<f32> = frames.iter().map(|f| f.db).collect();
        dbs.sort_by(|a, b| a.total_cmp(b));
        let floor = dbs.get(dbs.len() / 10).copied().unwrap_or(-70.0);
        let loud = dbs.get(dbs.len() * 9 / 10).copied().unwrap_or(-20.0);
        let gate_db = (floor + 6.0).max(loud - 30.0).clamp(-70.0, -25.0);
        let voiced: Vec<bool> = frames.iter().map(|f| f.db > gate_db).collect();
        let total = voiced.iter().filter(|v| **v).count().max(1) as f32;
        let mut acc = 0.0;
        let spoken = voiced
            .iter()
            .map(|v| {
                if *v {
                    acc += 1.0;
                }
                acc / total
            })
            .collect();
        Voice {
            rate,
            samples: Arc::new(samples),
            frames,
            hop_s: hop / rate as f32,
            spoken,
            gate_db,
        }
    }
    pub fn secs(&self) -> f32 {
        self.samples.len() as f32 / self.rate as f32
    }
    pub fn frame_at(&self, t: f32) -> Option<Frame> {
        let i = (t / self.hop_s) as usize;
        self.frames
            .get(i.min(self.frames.len().saturating_sub(1)))
            .copied()
    }
    pub fn spoken_at(&self, t: f32) -> f32 {
        if t >= self.secs() {
            return 1.0;
        }
        let i = (t / self.hop_s) as usize;
        self.spoken.get(i).copied().unwrap_or(1.0)
    }
}
