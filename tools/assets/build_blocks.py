#!/usr/bin/env python3
"""Textures for the building blocks: realistic, seamlessly tiling 128 x 128 tiles, drawn from
noise (nothing copied from any game or photo).

Each tile is two files in assets/blocks/tiles/:
    <name>.png          colour (RGBA: alpha only matters for glass)
    <name>.height.png   bumps, white = high (the app turns it into a normal map)
and lamps also have <name>.glow.png (white = lit). The app builds its texture atlas from these at
start, so a tile can be repainted in any paint program; keep it tiling (wrap-around) and the
same size. data/blocks.json says which tiles each block uses on top, sides and bottom.

    python3 tools/assets/build_blocks.py                    # (re)make every tile
    python3 tools/assets/build_blocks.py --sheet out.png    # and a contact sheet to look at
"""
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "assets" / "blocks" / "tiles"
N = 128


# ---------------------------------------------------------------- noise (all of it wraps)


def rng(seed):
    return np.random.default_rng(seed)


def fbm(seed, beta=1.8, lo=1.0, stretch=(1.0, 1.0)):
    """Fractal noise by spectral synthesis: white noise shaped to 1/f^beta. Periodic by
    construction, so it tiles. `stretch` (sx, sy) > 1 makes features longer along that axis."""
    w = rng(seed).standard_normal((N, N))
    f = np.fft.fft2(w)
    fy = np.fft.fftfreq(N)[:, None] * N * stretch[1]
    fx = np.fft.fftfreq(N)[None, :] * N * stretch[0]
    r = np.sqrt(fx * fx + fy * fy)
    r[0, 0] = 1.0
    amp = 1.0 / np.maximum(r, lo) ** beta
    amp[0, 0] = 0.0
    out = np.real(np.fft.ifft2(f * amp))
    return norm(out)


def norm(a):
    a = a - a.min()
    m = a.max()
    return a / m if m > 0 else a


def voronoi(seed, count, jitter=1.0):
    """Wrapping Voronoi: (F1, F2, cell index) per pixel, distances in pixels."""
    r = rng(seed)
    pts = r.random((count, 2)) * N
    yy, xx = np.mgrid[0:N, 0:N].astype(float)
    f1 = np.full((N, N), 1e9)
    f2 = np.full((N, N), 1e9)
    cell = np.zeros((N, N), int)
    for i, (px, py) in enumerate(pts):
        dx = np.abs(xx - px)
        dy = np.abs(yy - py)
        dx = np.minimum(dx, N - dx)
        dy = np.minimum(dy, N - dy)
        d = np.sqrt(dx * dx + dy * dy)
        closer = d < f1
        f2 = np.where(closer, f1, np.minimum(f2, d))
        cell = np.where(closer, i, cell)
        f1 = np.where(closer, d, f1)
    return f1, f2, cell


def ramp(t, stops):
    """Colour ramp: stops = [(pos, (r, g, b) 0-255), ...]."""
    t = np.clip(t, 0, 1)
    out = np.zeros(t.shape + (3,))
    pos = [s[0] for s in stops]
    for c in range(3):
        out[..., c] = np.interp(t, pos, [s[1][c] for s in stops])
    return out / 255.0


def shade(rgb, k):
    return np.clip(rgb * k[..., None], 0, 1)


def blur(a, r=1):
    """Wrapping box blur."""
    out = a.copy()
    for _ in range(r):
        out = (out + np.roll(out, 1, 0) + np.roll(out, -1, 0) + np.roll(out, 1, 1) + np.roll(out, -1, 1)) / 5
    return out


def strokes(seed, count, length, width, angle_spread, base_angle=-np.pi / 2):
    """Short wrapping strokes (grass blades, pine needles): a coverage mask and a per-stroke
    random value."""
    r = rng(seed)
    yy, xx = np.mgrid[0:N, 0:N].astype(float)
    mask = np.zeros((N, N))
    val = np.zeros((N, N))
    for _ in range(count):
        x0, y0 = r.random() * N, r.random() * N
        a = base_angle + r.normal() * angle_spread
        ln = length * (0.6 + 0.8 * r.random())
        dx, dy = np.cos(a), np.sin(a)
        # distance from each pixel to the segment, with wrap
        px = (xx - x0 + N / 2) % N - N / 2
        py = (yy - y0 + N / 2) % N - N / 2
        t = np.clip(px * dx + py * dy, 0, ln)
        d = np.hypot(px - t * dx, py - t * dy)
        taper = width * (1 - t / ln * 0.8)
        hit = d < taper
        v = r.random()
        mask = np.where(hit, np.maximum(mask, 1 - d / np.maximum(taper, 1e-3) * 0.5), mask)
        val = np.where(hit, v, val)
    return mask, val


# ---------------------------------------------------------------- the tiles
# each returns (rgb 0..1, height 0..1[, alpha][, glow])


def dirt(seed=1):
    base = fbm(seed, 1.7)
    fine = fbm(seed + 1, 0.9)
    f1, f2, cell = voronoi(seed + 2, 60)
    pebble = np.clip(1 - f1 / 4.0, 0, 1) * (rng(seed + 3).random(60)[cell] > 0.6)
    t = base * 0.7 + fine * 0.3
    rgb = ramp(t, [(0, (62, 42, 28)), (0.5, (104, 74, 48)), (1, (140, 104, 70))])
    rgb = rgb * (1 - pebble[..., None] * 0.15) + pebble[..., None] * np.array([150, 132, 110]) / 255 * 0.15 * 2
    specks = rng(seed + 4).random((N, N)) > 0.985
    rgb[specks] *= 0.55
    h = norm(base * 0.6 + fine * 0.3 + pebble * 0.6)
    return rgb, h


def grass_top(seed=11):
    under = fbm(seed, 1.5)
    rgb = ramp(under, [(0, (40, 72, 26)), (1, (78, 120, 44))])
    h = under * 0.3
    for layer, (count, ln, w) in enumerate([(260, 14, 1.6), (220, 11, 1.3), (180, 8, 1.1)]):
        m, v = strokes(seed + 10 + layer, count, ln, w, np.pi, base_angle=0)
        col = ramp(v, [(0, (52, 96, 30)), (0.5, (86, 140, 46)), (1, (128, 168, 64))])
        rgb = np.where((m > 0)[..., None], col * (0.75 + 0.25 * m[..., None]), rgb)
        h = np.maximum(h, m * (0.6 + 0.15 * layer))
    return rgb, norm(blur(h, 1))


def grass_side(seed=21):
    rgb, h = dirt(seed)
    g, gh = grass_top(seed + 1)
    yy = np.mgrid[0:N, 0:N][0].astype(float)
    # grass overhang: a jagged edge 16-30 px down from the top
    edge = 18 + fbm(seed + 2, 1.2)[0] * 14 + (rng(seed + 3).random(N) > 0.8) * rng(seed + 4).random(N) * 10
    cover = yy < edge[None, :]
    rgb = np.where(cover[..., None], g, rgb)
    lip = np.clip(1 - np.abs(yy - edge[None, :]) / 2.5, 0, 1)
    rgb = rgb * (1 - 0.35 * (lip * (yy > edge[None, :]))[..., None])
    h = np.where(cover, 0.6 + gh * 0.4, h * 0.6)
    return rgb, h


def stone(seed=31):
    big = fbm(seed, 2.2)
    mid = fbm(seed + 1, 1.4)
    fine = fbm(seed + 2, 0.6)
    t = big * 0.45 + mid * 0.35 + fine * 0.2
    rgb = ramp(t, [(0, (86, 86, 88)), (0.45, (120, 119, 118)), (0.75, (146, 144, 140)), (1, (168, 166, 160))])
    # mineral flecks: dark and light grains
    g = rng(seed + 4).random((N, N))
    rgb[g > 0.985] *= 1.3
    rgb[g < 0.02] *= 0.6
    # a few weathered pits
    f1, _, _ = voronoi(seed + 5, 18)
    pit = np.clip(1 - f1 / 2.2, 0, 1) * (rng(seed + 6).random((N, N)) > 0.2)
    rgb = shade(rgb, 1 - pit * 0.3)
    h = norm(big * 0.4 + mid * 0.35 + fine * 0.25 - pit * 0.5)
    return np.clip(rgb, 0, 1), h


def cobblestone(seed=41):
    f1, f2, cell = voronoi(seed, 11)
    edge = f2 - f1
    stone_mask = np.clip((edge - 2.0) / 3.0, 0, 1)
    dome = np.clip(1 - (f1 / (f1 + edge / 2 + 1e-6)) ** 2, 0, 1)
    tone = rng(seed + 1).random(11)[cell]
    grain = fbm(seed + 2, 1.1)
    rgb = ramp(tone * 0.6 + grain * 0.4, [(0, (88, 86, 84)), (0.5, (122, 118, 112)), (0.8, (140, 132, 120)), (1, (160, 156, 150))])
    rgb = shade(rgb, 0.75 + 0.3 * dome)
    mortar = ramp(fbm(seed + 3, 1.3), [(0, (56, 52, 48)), (1, (84, 80, 74))])
    rgb = rgb * stone_mask[..., None] + mortar * (1 - stone_mask[..., None])
    h = norm(stone_mask * (0.5 + 0.5 * dome) + grain * 0.1)
    return rgb, h


def bricks(seed=51):
    rows, per = 4, 2
    yy, xx = np.mgrid[0:N, 0:N].astype(float)
    bh = N / rows
    bw = N / per
    row = (yy // bh).astype(int)
    off = (row % 2) * bw / 2
    xb = (xx + off) % N
    col = (xb // bw).astype(int)
    lx = xb - col * bw
    ly = yy - row * bh
    m = 3.0  # mortar half-width
    rough = fbm(seed, 1.2) * 2.0
    d = np.minimum(np.minimum(lx, bw - lx), np.minimum(ly, bh - ly)) - rough
    brick = np.clip((d - m) / 1.5, 0, 1)
    idx = row * per + col
    tone = rng(seed + 1).random(rows * per)[idx]
    grain = fbm(seed + 2, 1.0)
    rgb = ramp(tone * 0.5 + grain * 0.5, [(0, (120, 50, 38)), (0.5, (150, 66, 46)), (1, (176, 92, 64))])
    soot = fbm(seed + 3, 2.0)
    rgb = shade(rgb, 0.85 + 0.25 * soot)
    mortar = ramp(fbm(seed + 4, 1.0), [(0, (170, 164, 152)), (1, (204, 198, 186))])
    rgb = rgb * brick[..., None] + mortar * (1 - brick[..., None])
    bevel = np.clip((d - m) / 6.0, 0, 1)
    h = norm(brick * (0.6 + 0.4 * bevel) + grain * 0.12)
    return rgb, h


def planks(seed=61):
    rows = 4
    yy, xx = np.mgrid[0:N, 0:N].astype(float)
    ph = N / rows
    row = (yy // ph).astype(int)
    ly = yy - row * ph
    grain = np.zeros((N, N))
    for r in range(rows):
        g = fbm(seed + r, 1.6, stretch=(0.08, 1.0))
        rings = np.sin((yy * 0.9 + g * 26 + r * 7) * 0.9)
        part = 0.6 * g + 0.4 * (rings * 0.5 + 0.5)
        grain = np.where(row == r, part, grain)
    tone = rng(seed + 10).random(rows)[row]
    rgb = ramp(grain * 0.7 + tone * 0.3, [(0, (118, 78, 44)), (0.5, (156, 108, 64)), (1, (190, 140, 88))])
    gap = np.clip(1 - np.minimum(ly, ph - ly) / 1.5, 0, 1)
    # each plank ends somewhere along the tile: a butt joint
    ends = (rng(seed + 11).random(rows) * N)[row]
    joint = np.clip(1 - np.abs(((xx - ends + N / 2) % N) - N / 2) / 1.2, 0, 1)
    nails = np.zeros((N, N))
    for r in range(rows):
        for dx in (-5, 5):
            cx = (rng(seed + 11).random(rows)[r] * N + dx) % N
            for cy in (ph * r + ph * 0.3, ph * r + ph * 0.7):
                d = np.hypot(((xx - cx + N / 2) % N) - N / 2, yy - cy)
                nails = np.maximum(nails, np.clip(1 - d / 1.6, 0, 1))
    k = 1 - 0.6 * np.maximum(gap, joint) - 0.45 * nails
    rgb = shade(rgb, k)
    h = norm(grain * 0.35 + (1 - np.maximum(gap, joint)) * 0.65 - nails * 0.2)
    return rgb, h


def bark(seed=71):
    g = fbm(seed, 1.7, stretch=(1.0, 0.12))
    ridges = np.abs(np.sin(g * 9.0))
    fine = fbm(seed + 1, 0.9)
    t = ridges * 0.7 + fine * 0.3
    rgb = ramp(t, [(0, (44, 30, 20)), (0.5, (82, 60, 40)), (1, (120, 94, 66))])
    lichen = (fbm(seed + 2, 2.0) > 0.78) * fbm(seed + 3, 0.8)
    rgb = rgb * (1 - lichen[..., None] * 0.5) + lichen[..., None] * np.array([110, 128, 84]) / 255 * 0.5
    return rgb, norm(ridges * 0.8 + fine * 0.2)


def log_top(seed=81):
    yy, xx = np.mgrid[0:N, 0:N].astype(float)
    cx = cy = N / 2 - 0.5
    r = np.hypot(xx - cx, yy - cy)
    warp = fbm(seed, 1.8) * 6
    rings = np.sin((r + warp) * 0.75) * 0.5 + 0.5
    rgb = ramp(rings * 0.6 + fbm(seed + 1, 1.0) * 0.4, [(0, (150, 108, 64)), (0.5, (184, 140, 90)), (1, (212, 172, 118))])
    ang = np.arctan2(yy - cy, xx - cx)
    crack = np.clip(1 - np.abs(np.sin(ang * 1.5 + 0.7)) / 0.03, 0, 1) * (r < 40) * (r > 6)
    rgb = shade(rgb, 1 - crack * 0.5)
    b_rgb, b_h = bark(seed + 2)
    edge = r > N / 2 - 9
    rgb = np.where(edge[..., None], b_rgb, rgb)
    h = np.where(edge, 0.5 + b_h * 0.5, 0.45 + rings * 0.1 - crack * 0.3)
    return rgb, norm(h)


def sand(seed=91):
    fine = fbm(seed, 0.6)
    yy, xx = np.mgrid[0:N, 0:N].astype(float)
    ripple = np.sin((yy + fbm(seed + 1, 2.2) * 30) * 2 * np.pi / (N / 6)) * 0.5 + 0.5
    t = fine * 0.6 + ripple * 0.4
    rgb = ramp(t, [(0, (196, 172, 122)), (1, (226, 206, 158))])
    grains = rng(seed + 2).random((N, N))
    rgb[grains > 0.97] *= 0.8
    rgb[grains < 0.02] *= 1.08
    return np.clip(rgb, 0, 1), norm(ripple * 0.7 + fine * 0.3)


def gravel(seed=101):
    f1, f2, cell = voronoi(seed, 90)
    edge = f2 - f1
    stones = np.clip((edge - 0.8) / 1.5, 0, 1)
    dome = np.clip(1 - f1 / 6.0, 0, 1)
    tone = rng(seed + 1).random(90)[cell]
    hue = rng(seed + 2).random(90)[cell]
    grey = ramp(tone, [(0, (96, 94, 92)), (1, (170, 166, 160))])
    brown = ramp(tone, [(0, (104, 88, 70)), (1, (168, 148, 120))])
    rgb = np.where((hue > 0.65)[..., None], brown, grey)
    rgb = shade(rgb, 0.7 + 0.35 * dome) * stones[..., None] + np.array([44, 40, 36]) / 255 * (1 - stones[..., None])
    return rgb, norm(stones * (0.4 + 0.6 * dome))


def snow(seed=111):
    soft = fbm(seed, 2.0)
    fine = fbm(seed + 1, 0.8)
    t = soft * 0.7 + fine * 0.3
    rgb = ramp(t, [(0, (212, 222, 236)), (1, (246, 250, 255))])
    sparkle = rng(seed + 2).random((N, N)) > 0.996
    rgb[sparkle] = 1.0
    return rgb, norm(soft * 0.8 + fine * 0.2)


def snow_side(seed=121):
    rgb, h = dirt(seed)
    s, sh = snow(seed + 1)
    yy = np.mgrid[0:N, 0:N][0].astype(float)
    edge = 22 + fbm(seed + 2, 1.4)[0] * 16
    drip = (rng(seed + 3).random(N) > 0.85) * rng(seed + 4).random(N) * 14
    edge = edge + blur(drip[None, :].repeat(3, 0), 1)[1]
    cover = yy < edge[None, :]
    rgb = np.where(cover[..., None], s, rgb)
    h = np.where(cover, 0.65 + sh * 0.35, h * 0.6)
    return rgb, h


def leaves(seed=131):
    rgb = ramp(fbm(seed, 1.4), [(0, (18, 40, 16)), (1, (34, 64, 24))])
    h = np.zeros((N, N))
    r = rng(seed + 1)
    yy, xx = np.mgrid[0:N, 0:N].astype(float)
    for _ in range(170):
        cx, cy = r.random() * N, r.random() * N
        a = r.random() * np.pi
        ln, wd = 6 + r.random() * 5, 2.5 + r.random() * 1.5
        px = ((xx - cx + N / 2) % N) - N / 2
        py = ((yy - cy + N / 2) % N) - N / 2
        u = px * np.cos(a) + py * np.sin(a)
        v = -px * np.sin(a) + py * np.cos(a)
        d = (u / ln) ** 2 + (v / wd) ** 2
        hit = d < 1
        tone = r.random()
        col = ramp(np.full((N, N), tone), [(0, (46, 90, 30)), (0.6, (70, 124, 40)), (1, (110, 150, 52))])
        lit = 0.75 + 0.35 * (1 - d) - 0.15 * (u / ln)
        rgb = np.where(hit[..., None], col * lit[..., None], rgb)
        h = np.where(hit, np.maximum(h, 0.4 + 0.6 * (1 - d)), h)
    return np.clip(rgb, 0, 1), norm(blur(h, 1))


def glass(seed=141):
    yy, xx = np.mgrid[0:N, 0:N].astype(float)
    frame = np.minimum(np.minimum(xx, N - 1 - xx), np.minimum(yy, N - 1 - yy)) < 5
    rgb = np.ones((N, N, 3)) * np.array([214, 232, 240]) / 255
    rgb[frame] = np.array([200, 204, 208]) / 255
    streak = np.clip(1 - np.abs((xx + yy) - N * 0.7) / 6, 0, 1) + 0.6 * np.clip(1 - np.abs((xx + yy) - N * 0.95) / 3, 0, 1)
    rgb = np.clip(rgb + streak[..., None] * 0.2, 0, 1)
    alpha = np.where(frame, 1.0, 0.22 + 0.25 * np.clip(streak, 0, 1))
    h = np.where(frame, 1.0, 0.5)
    return rgb, h, alpha


def concrete(seed=151):
    base = fbm(seed, 2.0)
    fine = fbm(seed + 1, 0.7)
    rgb = ramp(base * 0.5 + fine * 0.5, [(0, (136, 134, 130)), (1, (176, 174, 168))])
    stain = np.clip(fbm(seed + 2, 2.4) - 0.6, 0, 1) * 2
    rgb = shade(rgb, 1 - stain * 0.18)
    f1, _, _ = voronoi(seed + 3, 40)
    pores = (f1 < 1.1) * (rng(seed + 4).random((N, N)) > 0.3)
    rgb = shade(rgb, 1 - pores * 0.35)
    return rgb, norm(base * 0.4 + fine * 0.4 - pores * 0.4)


def metal(seed=161):
    brushed = fbm(seed, 1.2, stretch=(0.06, 1.0))
    yy, xx = np.mgrid[0:N, 0:N].astype(float)
    rgb = ramp(brushed, [(0, (128, 132, 138)), (1, (178, 182, 188))])
    seam = np.minimum(np.minimum(xx, N - 1 - xx), np.minimum(yy, N - 1 - yy)) < 2
    rgb[seam] *= 0.6
    h = brushed * 0.15 + 0.5
    h = np.where(seam, 0.2, h)
    for cx in (10, N - 11):
        for cy in (10, N - 11):
            d = np.hypot(xx - cx, yy - cy)
            rivet = np.clip(1 - d / 4.5, 0, 1)
            rgb = rgb * (1 - rivet[..., None] * 0.1) + rivet[..., None] * 0.1 * 0.9
            h = np.maximum(h, 0.5 + rivet * 0.5)
    scratches = (fbm(seed + 1, 0.9, stretch=(0.03, 1.0)) > 0.93)
    rgb[scratches] = np.clip(rgb[scratches] * 1.2, 0, 1)
    return rgb, norm(h)


def tile(seed=171):
    yy, xx = np.mgrid[0:N, 0:N].astype(float)
    per = 2
    s = N / per
    lx, ly = xx % s, yy % s
    d = np.minimum(np.minimum(lx, s - lx), np.minimum(ly, s - ly))
    face = np.clip((d - 2.0) / 1.0, 0, 1)
    idx = (yy // s * per + xx // s).astype(int)
    tone = rng(seed).random(per * per)[idx]
    glaze = fbm(seed + 1, 2.4)
    rgb = ramp(tone * 0.4 + glaze * 0.6, [(0, (222, 216, 202)), (1, (244, 240, 230))])
    grout = np.array([128, 124, 116]) / 255
    rgb = rgb * face[..., None] + grout * (1 - face[..., None])
    bevel = np.clip((d - 2.0) / 5.0, 0, 1)
    return rgb, norm(face * (0.6 + 0.4 * bevel) + glaze * 0.03)


def lamp(seed=181):
    yy, xx = np.mgrid[0:N, 0:N].astype(float)
    bars = (np.minimum(np.minimum(xx, N - 1 - xx), np.minimum(yy, N - 1 - yy)) < 9) | (np.abs(xx - N / 2) < 4) | (np.abs(yy - N / 2) < 4)
    iron = ramp(fbm(seed, 1.0), [(0, (34, 32, 30)), (1, (66, 62, 58))])
    r = np.hypot(((xx % (N / 2)) - N / 4), ((yy % (N / 2)) - N / 4))
    warm = ramp(1 - r / (N / 3), [(0, (240, 170, 80)), (1, (255, 236, 180))])
    rgb = np.where(bars[..., None], iron, warm)
    glow = np.where(bars, 0.0, np.clip(1.1 - r / (N / 2.2), 0.4, 1.0))
    h = np.where(bars, 0.9, 0.3)
    return rgb, h, None, glow


def water(seed=191):
    # still water: deep teal with soft ripples; the world's water shader moves the surface
    ripple = fbm(seed, 1.6, stretch=(1.0, 0.55))
    fine = fbm(seed + 1, 0.8)
    rgb = ramp(ripple * 0.7 + fine * 0.3, [(0, (22, 70, 98)), (0.6, (34, 104, 132)), (1, (70, 150, 170))])
    h = norm(ripple * 0.8 + fine * 0.2) * 0.3 + 0.35
    alpha = np.full((N, N), 0.72)
    return rgb, h, alpha


def obsidian(seed=201):
    # volcanic glass: near-black with violet depths, glassy conchoidal facets and bright edges
    f1, f2, cell = voronoi(seed, 14)
    edge = f2 - f1
    facet = np.clip(edge / 9.0, 0, 1)
    tone = rng(seed + 1).random(14)[cell]
    swirl = fbm(seed + 2, 1.6)
    rgb = ramp(swirl * 0.6 + tone * 0.4, [(0, (8, 6, 14)), (0.6, (24, 16, 40)), (1, (52, 34, 78))])
    ridge = np.clip(1 - edge / 1.6, 0, 1)
    rgb = rgb + ridge[..., None] * np.array([70, 56, 110]) / 255 * 0.6
    glint = (rng(seed + 3).random((N, N)) > 0.996)
    rgb[glint] = np.array([0.8, 0.75, 0.95])
    h = norm(facet * 0.7 + swirl * 0.3)
    return np.clip(rgb, 0, 1), h


def basalt_side(seed=211):
    # columnar basalt seen from the side: tall grey-black columns, cracked across
    yy, xx = np.mgrid[0:N, 0:N].astype(float)
    cols = 4
    w = N / cols
    lx = xx % w
    d = np.minimum(lx, w - lx)
    face = np.clip((d - 1.5) / 3.0, 0, 1)
    idx = (xx // w).astype(int)
    tone = rng(seed).random(cols)[idx]
    grain = fbm(seed + 1, 1.0, stretch=(1.0, 0.25))
    rgb = ramp(tone * 0.4 + grain * 0.6, [(0, (34, 34, 38)), (0.6, (58, 58, 62)), (1, (82, 80, 82))])
    crack = (fbm(seed + 2, 1.4, stretch=(0.2, 1.0)) > 0.86)
    rgb = shade(rgb, 0.55 + 0.45 * face)
    rgb[crack] *= 0.55
    h = norm(face * (0.7 + 0.3 * grain) - crack * 0.4)
    return np.clip(rgb, 0, 1), h


def basalt_top(seed=221):
    # the column ends: hexagonal-ish cells with dark joints
    f1, f2, cell = voronoi(seed, 9)
    edge = f2 - f1
    face = np.clip((edge - 1.5) / 3.0, 0, 1)
    tone = rng(seed + 1).random(9)[cell]
    grain = fbm(seed + 2, 1.2)
    rgb = ramp(tone * 0.5 + grain * 0.5, [(0, (40, 40, 44)), (1, (84, 82, 84))])
    rgb = shade(rgb, 0.5 + 0.5 * face)
    h = norm(face * (0.8 + 0.2 * grain))
    return rgb, h


def magma(seed=231):
    # cooling crust over molten rock: black-red plates with glowing cracks between them
    f1, f2, cell = voronoi(seed, 16)
    edge = f2 - f1
    crack = np.clip(1 - edge / 4.0, 0, 1)
    heat = fbm(seed + 1, 1.5)
    crust = ramp(fbm(seed + 2, 1.1), [(0, (28, 14, 10)), (1, (70, 34, 22))])
    hot = ramp(np.clip(crack * 0.8 + heat * 0.4, 0, 1), [(0, (150, 30, 0)), (0.5, (240, 100, 10)), (1, (255, 210, 90))])
    k = np.clip(crack * 1.3 + (heat > 0.75) * 0.5, 0, 1)
    rgb = crust * (1 - k[..., None]) + hot * k[..., None]
    glow = np.clip(k * 1.1, 0, 1)
    h = norm((1 - crack) * 0.8 + fbm(seed + 3, 1.0) * 0.2)
    return np.clip(rgb, 0, 1), h, None, glow


def ash(seed=241):
    # grey-black ash with cinders
    base = fbm(seed, 1.6)
    fine = fbm(seed + 1, 0.6)
    rgb = ramp(base * 0.6 + fine * 0.4, [(0, (38, 36, 36)), (0.6, (66, 62, 60)), (1, (96, 92, 88))])
    g = rng(seed + 2).random((N, N))
    rgb[g > 0.992] = np.array([0.55, 0.2, 0.08])
    rgb[g < 0.01] *= 0.5
    return rgb, norm(base * 0.5 + fine * 0.5)


def crystal(seed=251):
    # violet crystal: sharp facets, light inside
    f1, f2, cell = voronoi(seed, 8)
    edge = f2 - f1
    tone = rng(seed + 1).random(8)[cell]
    yy, xx = np.mgrid[0:N, 0:N].astype(float)
    streak = 0.5 + 0.5 * np.sin((xx * 0.8 + yy * 0.3) / 9.0 + tone * 6)
    rgb = ramp(tone * 0.5 + streak * 0.5, [(0, (70, 30, 140)), (0.6, (150, 80, 230)), (1, (225, 190, 255))])
    ridge = np.clip(1 - edge / 2.0, 0, 1)
    rgb = rgb * (1 - 0.4 * ridge[..., None]) + ridge[..., None] * 0.35
    glow = np.clip(0.35 + 0.65 * streak * (1 - ridge), 0, 1)
    h = norm(np.clip(edge / 10.0, 0, 1))
    return np.clip(rgb, 0, 1), h, None, glow


def fire(seed=261):
    # only for the block bar and the atlas: the flames themselves are drawn by a shader
    yy, xx = np.mgrid[0:N, 0:N].astype(float)
    t = 1 - yy / N
    n = fbm(seed, 1.3, stretch=(1.0, 0.4))
    shape = np.clip(1 - np.abs(xx - N / 2) / (N * (0.45 - 0.35 * (1 - t))) + (n - 0.5), 0, 1)
    rgb = ramp(np.clip(shape * t * 1.4, 0, 1), [(0, (120, 20, 0)), (0.5, (250, 120, 10)), (1, (255, 240, 160))])
    return rgb, shape * 0.5, None, shape


def pearl(seed=271):
    # pale polished stone: white with the faintest warm and cool veils, smooth
    veil = fbm(seed, 2.2, stretch=(1.0, 0.35))
    fine = fbm(seed + 1, 0.7)
    rgb = ramp(veil * 0.7 + fine * 0.3, [(0, (222, 220, 226)), (0.5, (236, 234, 236)), (1, (246, 243, 238))])
    vein = np.clip(1 - np.abs(fbm(seed + 2, 1.4) - 0.5) * 30, 0, 1) * 0.12
    rgb = rgb * (1 - vein[..., None]) + np.array([0.82, 0.84, 0.9]) * vein[..., None]
    return np.clip(rgb, 0, 1), norm(veil * 0.3 + fine * 0.7) * 0.25 + 0.6


def lumen(seed=281):
    # soft white light: a pale block glowing from inside, brightest in the middle
    yy, xx = np.mgrid[0:N, 0:N].astype(float)
    r = np.hypot(xx - N / 2, yy - N / 2) / (N / 2)
    swirl = fbm(seed, 1.6)
    rgb = ramp(np.clip(1 - r * 0.5 + swirl * 0.2, 0, 1), [(0, (220, 225, 235)), (1, (255, 252, 244))])
    glow = np.clip(0.55 + 0.45 * (1 - r) + 0.1 * swirl, 0, 1)
    return rgb, norm(swirl) * 0.2 + 0.7, None, glow


TILES = {
    "dirt": dirt,
    "grass_top": grass_top,
    "grass_side": grass_side,
    "stone": stone,
    "cobblestone": cobblestone,
    "bricks": bricks,
    "planks": planks,
    "bark": bark,
    "log_top": log_top,
    "sand": sand,
    "gravel": gravel,
    "snow": snow,
    "snow_side": snow_side,
    "leaves": leaves,
    "glass": glass,
    "concrete": concrete,
    "metal": metal,
    "tile": tile,
    "lamp": lamp,
    "water": water,
    "obsidian": obsidian,
    "basalt_side": basalt_side,
    "basalt_top": basalt_top,
    "magma": magma,
    "ash": ash,
    "crystal": crystal,
    "fire": fire,
    "pearl": pearl,
    "lumen": lumen,
}


def save(name, out):
    rgb, h = out[0], out[1]
    alpha = out[2] if len(out) > 2 and out[2] is not None else np.ones((N, N))
    rgba = np.concatenate([rgb, alpha[..., None]], axis=2)
    Image.fromarray((np.clip(rgba, 0, 1) * 255 + 0.5).astype(np.uint8), "RGBA").save(OUT / f"{name}.png")
    Image.fromarray((np.clip(h, 0, 1) * 255 + 0.5).astype(np.uint8), "L").save(OUT / f"{name}.height.png")
    if len(out) > 3 and out[3] is not None:
        Image.fromarray((np.clip(out[3], 0, 1) * 255 + 0.5).astype(np.uint8), "L").save(OUT / f"{name}.glow.png")


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for name, fn in TILES.items():
        save(name, fn())
        print("tile", name)
    if "--sheet" in sys.argv:
        path = sys.argv[sys.argv.index("--sheet") + 1]
        names = list(TILES)
        cols = 5
        rows = (len(names) + cols - 1) // cols
        S = N * 2
        sheet = Image.new("RGB", (cols * (S * 2 + 12), rows * (S + 22)), "white")
        from PIL import ImageDraw

        dr = ImageDraw.Draw(sheet)
        for i, n in enumerate(names):
            x, y = (i % cols) * (S * 2 + 12), (i // cols) * (S + 22)
            c = Image.open(OUT / f"{n}.png").convert("RGB")
            # 2 x 2 repeat shows the tiling
            rep = Image.new("RGB", (N * 2, N * 2))
            for a in (0, N):
                for b in (0, N):
                    rep.paste(c, (a, b))
            sheet.paste(rep, (x, y + 18))
            sheet.paste(Image.open(OUT / f"{n}.height.png").convert("RGB").resize((S, S)), (x + S + 4, y + 18))
            dr.text((x, y + 2), n, fill="black")
        sheet.save(path)
        print("sheet", path)


if __name__ == "__main__":
    main()
