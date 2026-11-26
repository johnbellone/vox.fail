//! WAV files: one reader for everything (sound effects, test recordings, viewers' uploaded
//! questions), bounds-checked so a bad or truncated file is an error, never a panic.

use std::path::Path;

/// A decoded WAV: sample rate, channels, and the samples interleaved as -1.0..1.0.
pub struct Wav {
    pub rate: u32,
    pub channels: usize,
    pub format: u16,
    pub bits: u16,
    pub samples: Vec<f32>,
}

/// Read 16-bit PCM or 32-bit float WAV bytes.
pub fn parse(b: &[u8]) -> Result<Wav, String> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let u16_at = |i: usize| -> Option<u16> { Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?])) };
    let u32_at = |i: usize| -> Option<u32> { Some(u32::from_le_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?])) };
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    let mut i = 12usize;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32_at(i + 4).ok_or("truncated chunk")? as usize;
        let start = i + 8;
        let end = start.checked_add(len).ok_or("bad chunk size")?.min(b.len());
        if id == b"fmt " {
            if end - start < 16 {
                return Err("short format chunk".into());
            }
            fmt = Some((u16_at(start).unwrap_or(0), u16_at(start + 2).unwrap_or(0), u32_at(start + 4).unwrap_or(0), u16_at(start + 14).unwrap_or(0)));
        } else if id == b"data" {
            let (format, ch, rate, bits) = fmt.ok_or("data before format")?;
            let body = &b[start..end];
            let samples: Vec<f32> = match (format, bits) {
                (1, 16) => body.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect(),
                (3, 32) => body.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
                _ => return Err(format!("unsupported WAV format {format}/{bits}-bit")),
            };
            return Ok(Wav { rate, channels: (ch as usize).max(1), format, bits, samples });
        }
        i = start.checked_add(len + (len & 1)).ok_or("bad chunk size")?;
    }
    Err("no data chunk".into())
}

/// (rate, channels, interleaved samples) from WAV bytes.
pub fn parse_wav(b: &[u8]) -> Result<(u32, usize, Vec<f32>), String> {
    parse(b).map(|w| (w.rate, w.channels, w.samples))
}

/// (rate, channels, interleaved samples) from a WAV file.
pub fn read_wav(path: &str) -> Result<(u32, usize, Vec<f32>), String> {
    let b = std::fs::read(path).map_err(|e| e.to_string())?;
    parse_wav(&b)
}

/// Any 16-bit or float WAV, mixed down to mono.
pub fn decode_wav_mono(b: &[u8]) -> Result<(u32, Vec<f32>), String> {
    let w = parse(b)?;
    Ok((w.rate, w.samples.chunks(w.channels).map(|c| c.iter().sum::<f32>() / c.len() as f32).collect()))
}

/// An uploaded recording: 16-bit PCM, mono, 8 to 48 kHz, and nothing else.
pub fn parse_wav_mono16(b: &[u8]) -> Result<(u32, Vec<f32>), String> {
    let w = parse(b).map_err(|e| if e.starts_with("unsupported WAV format") { "expected 16-bit PCM".into() } else { e })?;
    if w.format != 1 || w.bits != 16 {
        return Err("expected 16-bit PCM".into());
    }
    if w.channels != 1 {
        return Err("expected mono".into());
    }
    if !(8000..=48000).contains(&w.rate) {
        return Err(format!("unsupported sample rate {}", w.rate));
    }
    Ok((w.rate, w.samples))
}

pub fn write_wav16(path: &Path, rate: u32, s: &[f32]) -> std::io::Result<()> {
    std::fs::write(path, wav16_bytes(rate, s))
}

/// Mono 16-bit PCM WAV bytes.
pub fn wav16_bytes(rate: u32, s: &[f32]) -> Vec<u8> {
    let data_len = (s.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + s.len() * 2);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for x in s {
        b.extend_from_slice(&((x.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_mixes_down() {
        let b = wav16_bytes(16000, &[0.25, -0.5, 0.0]);
        let (rate, s) = parse_wav_mono16(&b).unwrap();
        assert_eq!(rate, 16000);
        assert_eq!(s.len(), 3);
        assert!((s[1] + 0.5).abs() < 1e-3);
        assert_eq!(decode_wav_mono(&b).unwrap().1.len(), 3);
    }

    #[test]
    fn bad_and_truncated_files_are_errors_not_panics() {
        let good = wav16_bytes(16000, &[0.1; 64]);
        for n in 0..good.len() {
            let _ = parse(&good[..n]);
        }
        assert!(parse(b"").is_err());
        let mut huge = good.clone();
        huge[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse(&huge).is_err());
    }
}
