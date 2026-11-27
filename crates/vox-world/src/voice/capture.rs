//! The microphone, as packets: opens an input device, brings its sound to 16 kHz mono, and
//! queues the packets the [`Packetizer`] cuts from it until the game drains them.
//!
//! The stream lives on its own thread ([`super::device`]). cpal streams are not `Send` on every
//! platform, and the game keeps a [`Capture`] as a resource, so the thread owns the stream and
//! the resource only holds shared state and a stop flag. Nothing opens at module load:
//! [`Capture::start`] is called only when voice is switched on, and returns at once.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use vox_voice::resample::Resampler;
use vox_voice::{Packet, Packetizer, RATE};

use super::device::{self, Slot, Status};

/// Most packets handed out per [`Capture::drain`] call (about 2 s of speech).
const DRAIN_MAX: usize = 100;
/// Packets kept while nobody drains; the oldest go first (about 10 s of speech).
const QUEUE_MAX: usize = 500;
/// Gain is clamped to this range; a louder mic can clip, a boost past 4x is noise.
const GAIN_MAX: f32 = 4.0;

/// State shared between the stream thread and the game.
struct Shared {
    packer: Mutex<Packetizer>,
    queue: Mutex<VecDeque<Packet>>,
    /// `f32` bits, so the game can change it while the stream runs
    gain: AtomicU32,
}

/// Your microphone as packets. Dropping it stops the stream within about 100 ms.
pub struct Capture {
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    slot: Slot,
}

/// The device's name. `description()` is the cpal 0.17 way; `name()` is deprecated.
fn dev_name(d: &cpal::Device) -> Option<String> {
    d.description().ok().map(|d| d.name().to_string())
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Names of the input devices this machine has. Empty if the audio host cannot list them.
pub fn input_devices() -> Vec<String> {
    let host = cpal::default_host();
    match host.input_devices() {
        Ok(devs) => devs.filter_map(|d| dev_name(&d)).collect(),
        Err(e) => {
            crate::note!("[voice] can't list input devices: {e}");
            Vec::new()
        }
    }
}

/// The device whose name contains `want` (case-insensitive), or the default when `want` is `None`.
/// A name that matches nothing is an error: a player who picked a mic should not get another.
fn find_device(want: Option<&str>) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    match want {
        None => host
            .default_input_device()
            .ok_or_else(|| "no microphone found".to_string()),
        Some(w) => {
            let w = w.to_lowercase();
            host.input_devices()
                .map_err(|e| format!("can't list microphones: {e}"))?
                .find(|d| dev_name(d).is_some_and(|n| n.to_lowercase().contains(&w)))
                .ok_or_else(|| format!("no microphone matches \"{w}\""))
        }
    }
}

/// Averages each frame of interleaved samples into one mono sample. `ch == 0` gives nothing.
fn downmix(interleaved: &[f32], ch: usize, out: &mut Vec<f32>) {
    out.clear();
    if ch == 0 {
        return;
    }
    out.extend(
        interleaved
            .chunks_exact(ch)
            .map(|fr| fr.iter().sum::<f32>() / ch as f32),
    );
}

/// Multiplies samples by `gain`, clamped to `0..=GAIN_MAX`. A non-finite gain counts as 1.
fn clamp_gain(gain: f32) -> f32 {
    if gain.is_finite() {
        gain.clamp(0.0, GAIN_MAX)
    } else {
        1.0
    }
}

/// Everything the audio callback needs; it is moved into the callback, so nothing locks for it.
struct Feed {
    ch: usize,
    rs: Resampler,
    mono: Vec<f32>,
    rate16: Vec<f32>,
    pk: Vec<Packet>,
}

impl Feed {
    /// One block of device audio: mono, gain, 16 kHz, packets, queue.
    fn push(&mut self, shared: &Shared, interleaved: &[f32]) {
        downmix(interleaved, self.ch, &mut self.mono);
        let gain = clamp_gain(f32::from_bits(shared.gain.load(Ordering::Relaxed)));
        if gain != 1.0 {
            self.mono.iter_mut().for_each(|v| *v *= gain);
        }
        self.rate16.clear();
        self.rs.push(&self.mono, &mut self.rate16);
        self.pk.clear();
        lock(&shared.packer).push(&self.rate16, &mut self.pk);
        if self.pk.is_empty() {
            return;
        }
        let mut q = lock(&shared.queue);
        q.extend(self.pk.drain(..));
        while q.len() > QUEUE_MAX {
            q.pop_front();
        }
    }
}

/// Opens the microphone on the calling thread (the device thread).
fn open(shared: Arc<Shared>, want: Option<String>) -> Result<(cpal::Stream, String), String> {
        let device = find_device(want.as_deref())?;
        let name = dev_name(&device).unwrap_or_else(|| "microphone".into());
        let config = device
            .default_input_config()
            .map_err(|e| format!("can't read the microphone settings: {e}"))?;
        let ch = config.channels() as usize;
        if ch == 0 {
            return Err("the microphone reports no channels".into());
        }
        let rate = config.sample_rate();
        let feed = Feed {
            ch,
            rs: Resampler::new(rate, RATE),
            mono: Vec::new(),
            rate16: Vec::new(),
            pk: Vec::new(),
        };
        let cfg: cpal::StreamConfig = config.config();
        let err = |e: cpal::StreamError| crate::note!("[voice] microphone error: {e}");
        let open_err = |e: cpal::BuildStreamError| {
            format!(
                "can't open the microphone: {e}. Check that no other app is using it and that \
                 this app may use the microphone."
            )
        };
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                let (s, mut feed) = (shared.clone(), feed);
                device.build_input_stream(
                    &cfg,
                    move |d: &[f32], _: &_| feed.push(&s, d),
                    err,
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let (s, mut feed) = (shared.clone(), feed);
                let mut buf = Vec::new();
                device.build_input_stream(
                    &cfg,
                    move |d: &[i16], _: &_| {
                        buf.clear();
                        buf.extend(d.iter().map(|v| *v as f32 / 32768.0));
                        feed.push(&s, &buf)
                    },
                    err,
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                let (s, mut feed) = (shared.clone(), feed);
                let mut buf = Vec::new();
                device.build_input_stream(
                    &cfg,
                    move |d: &[u16], _: &_| {
                        buf.clear();
                        buf.extend(d.iter().map(|v| (*v as f32 - 32768.0) / 32768.0));
                        feed.push(&s, &buf)
                    },
                    err,
                    None,
                )
            }
            other => return Err(format!("unsupported microphone format {other:?}")),
        }
        .map_err(open_err)?;
        stream
            .play()
            .map_err(|e| format!("can't start the microphone: {e}"))?;
        Ok((stream, name))
}

impl Capture {
    /// Starts opening the microphone named `device` (a part of its name), or the default one.
    /// Returns at once: [`Self::status`] says when it is open, or why it couldn't be.
    pub fn start(device: Option<&str>) -> Result<Capture, String> {
        let shared = Arc::new(Shared {
            packer: Mutex::new(Packetizer::new()),
            queue: Mutex::new(VecDeque::new()),
            gain: AtomicU32::new(1.0f32.to_bits()),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let (s, want) = (shared.clone(), device.map(str::to_string));
        let slot = device::spawn("voice-in", stop.clone(), move || open(s, want))?;
        Ok(Capture { shared, stop, slot })
    }

    /// Opening, open (with the device's name), or failed.
    pub fn status(&self) -> Status {
        self.slot.status()
    }

    /// Sets push to talk (see [`Packetizer::set_ptt`]), then returns up to 100 packets made since
    /// the last call, oldest first.
    pub fn drain(&self, ptt: Option<bool>) -> Vec<Packet> {
        lock(&self.shared.packer).set_ptt(ptt);
        let mut q = lock(&self.shared.queue);
        let n = q.len().min(DRAIN_MAX);
        q.drain(..n).collect()
    }

    /// Scales the mic before it is packed. 1.0 is unchanged; the range is `0..=4`.
    pub fn set_gain(&self, gain: f32) {
        self.shared
            .gain
            .store(clamp_gain(gain).to_bits(), Ordering::Relaxed);
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_each_frame() {
        let mut out = Vec::new();
        downmix(&[0.2, 0.4, -1.0, 1.0], 2, &mut out);
        assert_eq!(out.len(), 2);
        assert!((out[0] - 0.3).abs() < 1e-6);
        assert!(out[1].abs() < 1e-6);
    }

    #[test]
    fn downmix_mono_is_a_copy_and_zero_channels_is_empty() {
        let mut out = Vec::new();
        downmix(&[0.5, -0.5], 1, &mut out);
        assert_eq!(out, vec![0.5, -0.5]);
        downmix(&[0.5, -0.5], 0, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn downmix_drops_a_partial_frame() {
        let mut out = Vec::new();
        downmix(&[1.0, 1.0, 1.0], 2, &mut out);
        assert_eq!(out, vec![1.0]);
    }

    #[test]
    fn gain_is_clamped_and_non_finite_means_unity() {
        assert_eq!(clamp_gain(1.0), 1.0);
        assert_eq!(clamp_gain(-2.0), 0.0);
        assert_eq!(clamp_gain(10.0), GAIN_MAX);
        assert_eq!(clamp_gain(f32::NAN), 1.0);
        assert_eq!(clamp_gain(f32::INFINITY), 1.0);
    }

    #[test]
    fn gain_round_trips_through_the_atomic() {
        let shared = Shared {
            packer: Mutex::new(Packetizer::new()),
            queue: Mutex::new(VecDeque::new()),
            gain: AtomicU32::new(1.0f32.to_bits()),
        };
        shared
            .gain
            .store(clamp_gain(2.5).to_bits(), Ordering::Relaxed);
        assert_eq!(f32::from_bits(shared.gain.load(Ordering::Relaxed)), 2.5);
    }
}
