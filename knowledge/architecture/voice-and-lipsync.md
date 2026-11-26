---
type: Architecture
title: Voice Analysis and Lip Sync
description: "How the microphone signal becomes mouth shapes, vowels, pitch-based body language and laugh/sing events, with measured accuracy."
tags: [architecture, audio, dsp, lipsync, voice]
generated: { by: agent/mcp, at: "2026-11-26T15:57:58Z" }
status: stable
code_refs: [crates/avatar-voice/src/lipsync.rs, crates/avatar-voice/src/voice.rs, crates/avatar-voice/src/detect.rs, bins/block-avatar/src/sound/audio.rs, crates/avatar-voice/src/tests_real.rs]
---

# Voice Analysis and Lip Sync

Pure Rust with no Bevy dependency in `lipsync.rs`, `voice.rs` and `detect.rs`, so they run offline (`--analyze in.wav out.csv [mode]`) and in unit tests. Runs on the audio thread in well under 1 ms per 10 ms of audio.

## Per 10 ms window (`lipsync.rs`)
Level (RMS envelope, 15 ms attack, 120 ms release, dBFS), share of energy below 500 Hz, above 900 Hz and above 2.5 kHz, zero-crossing rate, and syllable starts. Level above a self-calibrating noise gate decides closed / half / wide; "wide" adapts to the speaker. Low-heavy sound reads as round ("oo"), bright as spread ("ee"), bright plus many zero crossings as hiss, a quick dip on a low hum as pressed lips. Shapes hold 50-90 ms so the block mouth does not flicker.

Five styles: **Visemes** (5 shapes, default), **Detailed** (7), **Flap**, **Volume**, **Chatter**. All ignore the spoken language.

## Pitch, formants, prosody (`voice.rs`)
- Signal low-passed and decimated to 11-16 kHz.
- **Pitch**: YIN on a 40 ms window, 70-400 Hz, with a clarity score that doubles as the voicing decision.
- **Formants**: Burg LPC (order = kHz + 2) on 25 ms, roots of the predictor as Praat does. F1 ~ jaw opening, F2 ~ lip spread, so voiced frames are placed in the speaker's own vowel space. Calibration (key `P`: hold ah, ee, oo ~2 s each) stores vowels in settings.
- **Prosody** against a running average: a louder or higher syllable start is a beat (nod, alternating hand beat); a phrase ending 4+ semitones up followed by a 0.6 s pause tilts the head; pitch held 3.5+ semitones up raises brows.

## Laugh and sing (`detect.rs`)
Laugh = four quick, evenly spaced, breathy bursts in 1.6 s (3-8 per second). Sing = pitch held on one note for 0.25 s or more. Both work without speech-to-text.

## Measured accuracy (tools/eval_voice.py against Praat; real unmodified recordings)
| Check | Result |
| --- | --- |
| Voicing decision vs Praat, 3 LibriSpeech readers | same on ~93% of frames; median pitch error 5-8 cents |
| F1 / F2 vs Praat at 16, 44.1, 48 kHz | median error 5-8% / 3-5% |
| Vowel decision at vowel centre, 6 speakers, 312 words | 89% (91% after calibration) |
| Mouth shown across vowels incl. transitions, uncalibrated | 62% with formants, 24% tone-only |
| False question-tilts on 11 read-speech recordings | 0 |
| Laughs caught on real recordings | about 9 of 16, none fired on 11 speech clips or singing |

Caveats: the real-voice sets are English and mostly male; higher voices were checked through one reader, so calibrate. Tilt hit rate on real questions is untested.

## Latency
Mouth lags the mic by about 50 ms at 60 fps (mic buffer plus one frame); the control page shows the figure so an OBS Sync Offset can match it.

## Tests that pin this
`tests_real.rs` and the `lipsync`, `voice`, `detect` unit tests fail if real-voice accuracy drops. Fixtures in `testdata/real/`. See `architecture/testing-and-ci`.

# Related Concepts
- [Audio and Speech Technology](../technologies/audio-and-speech.md): audio libraries and Whisper
