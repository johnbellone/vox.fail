#!/usr/bin/env python3
"""Check the app's voice analysis against Praat on real recordings.

    pip install praat-parselmouth
    cargo build --release
    python3 tools/eval_voice.py praat demo/*.wav               # pitch + F1/F2 vs Praat, frame by frame
    python3 tools/eval_voice.py vowels path/to/fsdd/recordings  # vowel accuracy on the Free Spoken Digit Dataset
    python3 tools/eval_voice.py rises your_recording.wav        # pitch rise at each phrase end

Praat's formant ceiling is 5000 Hz for male voices and 5500 Hz for female (pass --female).
"""
import csv, math, os, statistics as st, subprocess, sys, tempfile, collections

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, 'target', 'release', 'block_avatar' + ('.exe' if os.name == 'nt' else ''))
TMP = tempfile.mkdtemp()


def ours(wav, extra=(), cwd=TMP):
    out = os.path.join(TMP, 'o.csv')
    subprocess.run([BIN, '--analyze', os.path.abspath(wav), out, 'Visemes', *extra], capture_output=True, cwd=cwd)
    return list(csv.DictReader(open(out)))


def praat(paths, fmax):
    import parselmouth
    for wav in paths:
        snd = parselmouth.Sound(wav)
        p = snd.to_pitch_ac(time_step=0.01, pitch_floor=70, pitch_ceiling=400)
        fm = snd.to_formant_burg(time_step=0.01, max_number_of_formants=5, maximum_formant=fmax, window_length=0.025)
        vb = vo = both = gross = 0
        cents, e1, e2 = [], [], []
        for r in ours(wav):
            t = float(r['time_s']) - 0.015
            if t < 0.03:
                continue
            pp = p.get_value_at_time(t)
            pv = pp == pp and pp > 0
            ov = float(r['pitch_hz']) > 0 and float(r['clarity']) > 0.6
            vb += pv
            vo += ov
            if pv and ov:
                both += 1
                c = 1200 * math.log2(float(r['pitch_hz']) / pp)
                if abs(c) > 316:
                    gross += 1
                else:
                    cents.append(abs(c))
                f1, f2 = float(r['f1']), float(r['f2'])
                P1, P2 = fm.get_value_at_time(1, t), fm.get_value_at_time(2, t)
                if f1 > 0 and f2 > 0 and P1 == P1 and P2 == P2:
                    e1.append(abs(f1 - P1) / P1)
                    e2.append(abs(f2 - P2) / P2)
        print(f'{os.path.basename(wav)}: voicing agrees {both / max(vb, 1):.0%} (recall) {both / max(vo, 1):.0%} (precision), '
              f'pitch median error {st.median(cents) if cents else 0:.1f} cents, gross {gross / max(both, 1):.1%}, '
              f'F1 error {st.median(e1) if e1 else 0:.1%}, F2 error {st.median(e2) if e2 else 0:.1%} ({len(e1)} frames)')


WORDS = {'3': ('ee', 'bright', (0.3, 0.8)), '2': ('oo', 'round', (0.3, 0.8)), '5': ('ah', 'open', (0.15, 0.45)), '9': ('ah', 'open', (0.25, 0.5))}


def vowels(rec):
    speakers = sorted({f.split('_')[1] for f in os.listdir(rec) if f.endswith('.wav')})
    for extra, label in (((), 'formants'), (('--no-formants',), 'tone only')):
        hit = collections.Counter()
        tot = collections.Counter()
        for s in speakers:
            for d, (v, want, span) in WORDS.items():
                for i in range(25, 50, 2):
                    path = os.path.join(rec, f'{d}_{s}_{i}.wav')
                    if not os.path.exists(path):
                        continue
                    vf = [r for r in ours(path, extra) if float(r['clarity']) > 0.6 and float(r['pitch_hz']) > 0]
                    if len(vf) < 4:
                        continue
                    a = int(len(vf) * span[0])
                    b = max(a + 1, int(len(vf) * span[1]))
                    for r in vf[a:b]:
                        hit[v] += r['mouth'] == want
                        tot[v] += 1
        print(f'{label:9s}', '  '.join(f'{v} {hit[v] / tot[v]:.0%}' for v in sorted(tot)), f'| all {sum(hit.values()) / sum(tot.values()):.0%}')


def rises(paths):
    for wav in paths:
        pts = [(float(r['time_s']), 12 * math.log2(float(r['pitch_hz']) / 100)) for r in ours(wav) if float(r['pitch_hz']) > 0 and float(r['clarity']) > 0.6]
        out, phrase = [], pts[:1]
        for a, b in zip(pts, pts[1:]):
            if b[0] - a[0] >= 0.25:
                out.append((phrase, b[0] - a[0]))
                phrase = []
            phrase.append(b)
        out.append((phrase, float('inf')))
        res = []
        for ph, gap in out:
            if len(ph) < 2 or ph[-1][0] - ph[0][0] < 0.5:
                continue
            end = ph[-1][0]
            tail = [s for t, s in ph if t > end - 0.12]
            prior = [s for t, s in ph if end - 0.5 < t <= end - 0.2]
            if tail and prior:
                res.append(f'{end:.2f}s: {sum(tail) / len(tail) - sum(prior) / len(prior):+.1f} st, pause {gap:.2f}s')
        print(os.path.basename(wav), '; '.join(res))


if __name__ == '__main__':
    cmd, args = sys.argv[1], [a for a in sys.argv[2:] if not a.startswith('--')]
    if cmd == 'praat':
        praat(args, 5500 if '--female' in sys.argv else 5000)
    elif cmd == 'vowels':
        vowels(args[0])
    elif cmd == 'rises':
        rises(args)
