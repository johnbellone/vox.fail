# Real-voice test recordings

Unmodified recordings of real people, used by `cargo test` (src/tests_real.rs) to check the pitch
tracker, the vowel reading and the body-language rules against real speech.

| Files | Source | Licence |
|---|---|---|
| `{digit}_{speaker}_{n}.wav` (72 files, 6 speakers, 8 kHz) | Free Spoken Digit Dataset, github.com/Jakobovski/free-spoken-digit-dataset | CC BY-SA 4.0 |
| `LJ001-000*.wav` (4 files, 22.05 kHz) | LJ Speech Dataset (Keith Ito, 2017), via the Coqui TTS test set | Public domain |
| `laugh/*.wav` (16 clips, 44.1 kHz) | ESC-50 (Karol Piczak), class "laughing", only the clips whose originals are CC0 or CC BY; the original Freesound recordings and authors are listed below | CC0 / CC BY 3.0 |
| `sing/vocadito_1.wav`, `sing/vocadito_10.wav` | vocadito (Bittner et al., 2021), solo a cappella singing, via the mirdata and basic-pitch test resources | CC BY 4.0 |

Laughs (ESC-50 clip: Freesound original by author, licence):
1-33658 laughter.wav by sagetyrtle (CC0); 1-36164-A/B laughs2.wav by sagetyrtle (CC0);
1-72695 laugh5_woman.wav by sagetyrtle (CC0); 1-73123 0524laugh.wav by sagetyrtle (CC0);
3-118487 FemaleLaughing.wav by morningfrost (CC0); 3-119459 Male Laughter.wav by Eneasz (CC0);
4-181599 Laugh.wav by JulieBaka (CC0); 5-244526 laugh.mp3 by smartxxd (CC0);
5-263775-A/B Laughing Man by scottemoil (CC0); 2-60791, 2-60794, 2-60795 by J.Zazvurek (CC BY);
3-152912, 3-152997 by fmaudio (CC BY). All at freesound.org (sound IDs as in the file names).

The laugh and sing detectors (src/detect.rs) are pinned on these: at least 6 of the 16 laughs
caught (9 at the time of writing), both singers detected, and nothing set off by any speech
recording here or in demo/.

`praat_pitch.csv` holds Praat's pitch (parselmouth 0.4.7, `To Pitch (ac)`, 70-400 Hz) at the centre
of each of the app's 10 ms analysis frames: the reference the pitch test compares against.
