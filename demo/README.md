# Demo recordings

Real, unmodified human speech for driving the avatar without a microphone (`--wav`) and for the
offline analysis (`--analyze`). Nothing synthetic, no added noise or effects; the files were only
decoded from Ogg to 16-bit WAV, which is the format the app reads.

| File | Reader | Length | From |
|---|---|---|---|
| `libri-female-198-209-0000.wav` | Heather Barnett, *Sense and Sensibility* ch. 18 | 14.8 s | LibriSpeech 198-209-0000 |
| `libri-male-5703-47212-0000.wav` | Garth Comira, *The Ashiel Mystery* ch. 16 | 13.9 s | LibriSpeech 5703-47212-0000 |
| `libri-male-3436-172162-0000.wav` | Anders Lankford, *The Age of Chivalry* ch. 9 | 16.7 s | LibriSpeech 3436-172162-0000 |

LibriSpeech ASR corpus, V. Panayotov, G. Chen, D. Povey and S. Khudanpur (2015), licensed CC BY 4.0
(openslr.org/12), built from LibriVox public-domain audiobook recordings. These copies come from
librosa's example data (github.com/librosa/data), checked against librosa's published SHA-256 hashes.
16 kHz mono.
