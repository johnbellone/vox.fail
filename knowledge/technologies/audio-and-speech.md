---
type: Technology
title: Audio and Speech Technology
description: "Microphone capture, playback, hand-written voice DSP, network voice coding and optional Whisper speech-to-text."
tags: [technology, audio, whisper, cpal, dsp]
generated: { by: agent/mcp, at: "2026-11-26T15:54:16Z" }
status: stable
---

# Audio and Speech

- **Capture**: `cpal` 0.17 on a dedicated thread publishes the latest analysis frame (`audio.rs`); a WAV file can stand in for the mic (`--wav`); `--list-mics`, `--mic <name part>`. Known issue: on Linux ALSA, device names can be `null` or `default`, and `cpal::Device::name` is deprecated.
- **Playback**: Bevy audio (rodio). Every sound opens its own output stream and thread (capped at 8 for effects) - a single mixer stream is a known improvement (risk 8).
- **DSP (own code, no crate)**: RMS envelope, band shares, zero crossing, YIN pitch, Burg LPC formants, prosody, laugh and sing detectors. See `architecture/voice-and-lipsync`.
- **Network voice**: IMA ADPCM, 16 kHz mono, 20 ms packets (~8 KB/s); Opus considered later (needs a C library and CMake on Windows).
- **Speech-to-text**: optional `whisper-rs` 0.16 (whisper.cpp) via `--features whisper`; model `ggml-base.en.bin` (142 MB) recommended, tiny and small also work; looked for in the user folder's `whisper/`, `assets/models/whisper/` or `whisper_model=`. Alternatives: a caption file (e.g. OBS LocalVocal) or typed `say:`. **Installed Windows builds have no Whisper** (risk 6).
- **Test audio**: only real human recordings, never synthetic (`convention/hard-constraints`).

See `architecture/show-networking` for the voice transport.
