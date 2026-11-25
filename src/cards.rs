//! Text and cards for shorts and comic strips: a title card (the character's portrait and what
//! happened), the caption burned into the top of every frame, an end card (your channel), and the
//! caption bar under a comic strip. Drawn with the app's pixel font, no ffmpeg filters needed.

use std::path::Path;

use ab_glyph::{Font, FontArc, PxScale, ScaleFont};

/// What a short is about and whose it is.
#[derive(Clone, Debug, Default)]
pub struct Brand {
    /// model path under assets/ (for the portrait)
    pub model: String,
    pub character: String,
    /// Twitch channel, if one is set
    pub channel: Option<String>,
}

pub struct Painter {
    font: FontArc,
}

/// An RGB image being drawn on.
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Canvas {
    pub fn new(w: usize, h: usize, rgb: [u8; 3]) -> Self {
        Canvas {
            w,
            h,
            px: rgb.iter().copied().cycle().take(w * h * 3).collect(),
        }
    }

    pub fn from_jpeg(jpg: &[u8]) -> Result<Self, String> {
        let mut d = jpeg_decoder::Decoder::new(std::io::Cursor::new(jpg));
        let px = d.decode().map_err(|e| e.to_string())?;
        let info = d.info().ok_or("no jpeg info")?;
        if info.pixel_format != jpeg_decoder::PixelFormat::RGB24 {
            return Err("not an RGB jpeg".into());
        }
        Ok(Canvas {
            w: info.width as usize,
            h: info.height as usize,
            px,
        })
    }

    pub fn jpeg(&self, quality: u8) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(90_000);
        jpeg_encoder::Encoder::new(&mut out, quality)
            .encode(
                &self.px,
                self.w as u16,
                self.h as u16,
                jpeg_encoder::ColorType::Rgb,
            )
            .map_err(|e| e.to_string())?;
        Ok(out)
    }

    fn blend(&mut self, x: i64, y: i64, c: [u8; 3], a: f32) {
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 || a <= 0.0 {
            return;
        }
        let i = (y as usize * self.w + x as usize) * 3;
        for k in 0..3 {
            let v = self.px[i + k] as f32 * (1.0 - a) + c[k] as f32 * a;
            self.px[i + k] = v.round().clamp(0.0, 255.0) as u8;
        }
    }

    pub fn rect(&mut self, x: i64, y: i64, w: i64, h: i64, c: [u8; 3], a: f32) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.blend(xx, yy, c, a);
            }
        }
    }

    /// Pastes an RGBA image (its alpha respected), scaled up by a whole number, nearest pixel.
    pub fn paste_rgba(&mut self, img: &[u8], iw: usize, ih: usize, x0: i64, y0: i64, scale: usize) {
        for y in 0..ih * scale {
            for x in 0..iw * scale {
                let s = ((y / scale) * iw + x / scale) * 4;
                let a = img[s + 3] as f32 / 255.0;
                self.blend(
                    x0 + x as i64,
                    y0 + y as i64,
                    [img[s], img[s + 1], img[s + 2]],
                    a,
                );
            }
        }
    }
}

impl Painter {
    pub fn load() -> Option<Self> {
        let b = std::fs::read(crate::asset_base().join("assets/fonts/PressStart2P-Regular.ttf"))
            .ok()?;
        Some(Painter {
            font: FontArc::try_from_vec(b).ok()?,
        })
    }

    pub fn text_width(&self, s: &str, size: f32) -> f32 {
        let f = self.font.as_scaled(PxScale::from(size));
        s.chars().map(|c| f.h_advance(f.glyph_id(c))).sum()
    }

    /// Breaks text into lines that fit `max_w` pixels (at most `max_lines`, the last one cut short with "...").
    pub fn wrap(&self, s: &str, size: f32, max_w: f32, max_lines: usize) -> Vec<String> {
        let mut lines: Vec<String> = vec![];
        let mut cur = String::new();
        for word in s.split_whitespace() {
            let try_line = if cur.is_empty() {
                word.to_string()
            } else {
                format!("{cur} {word}")
            };
            if self.text_width(&try_line, size) <= max_w || cur.is_empty() {
                cur = try_line;
            } else {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
        if lines.len() > max_lines {
            lines.truncate(max_lines);
            let last = lines.last_mut().unwrap();
            while !last.is_empty() && self.text_width(&format!("{last}..."), size) > max_w {
                last.pop();
            }
            last.push_str("...");
        }
        // a single word longer than the line: cut it
        for l in &mut lines {
            while l.chars().count() > 1 && self.text_width(l, size) > max_w {
                l.pop();
            }
        }
        lines
    }

    /// Draws one line with its top-left at (x, y), with a 1-pixel-per-8 drop shadow.
    pub fn text(
        &self,
        c: &mut Canvas,
        s: &str,
        x: f32,
        y: f32,
        size: f32,
        rgb: [u8; 3],
        shadow: bool,
    ) {
        if shadow {
            let d = (size / 8.0).max(1.0).round();
            self.draw(c, s, x + d, y + d, size, [0, 0, 0]);
        }
        self.draw(c, s, x, y, size, rgb);
    }

    fn draw(&self, c: &mut Canvas, s: &str, x: f32, y: f32, size: f32, rgb: [u8; 3]) {
        let f = self.font.as_scaled(PxScale::from(size));
        let mut pen = x;
        let base = y + f.ascent();
        for ch in s.chars() {
            let id = f.glyph_id(ch);
            let g = id.with_scale_and_position(PxScale::from(size), ab_glyph::point(pen, base));
            pen += f.h_advance(id);
            if let Some(o) = self.font.outline_glyph(g) {
                let b = o.px_bounds();
                o.draw(|gx, gy, cov| {
                    c.blend(
                        b.min.x as i64 + gx as i64,
                        b.min.y as i64 + gy as i64,
                        rgb,
                        cov.min(1.0),
                    )
                });
            }
        }
    }

    /// Centred lines, `gap` pixels apart; returns the y below the last line.
    pub fn centred(
        &self,
        c: &mut Canvas,
        lines: &[String],
        y: f32,
        size: f32,
        gap: f32,
        rgb: [u8; 3],
    ) -> f32 {
        let mut y = y;
        for l in lines {
            let w = self.text_width(l, size);
            self.text(c, l, ((c.w as f32 - w) / 2.0).round(), y, size, rgb, true);
            y += size + gap;
        }
        y
    }
}

const GOLD: [u8; 3] = [242, 182, 50];
const INK: [u8; 3] = [238, 240, 246];
const NIGHT: [u8; 3] = [15, 17, 26];

/// The character's portrait (assets/portraits/<model>.png), as RGBA.
pub fn portrait(model: &str) -> Option<(Vec<u8>, usize, usize)> {
    let stem = Path::new(model).file_stem()?.to_string_lossy().into_owned();
    let f = std::fs::File::open(
        crate::asset_base()
            .join("assets/portraits")
            .join(format!("{stem}.png")),
    )
    .ok()?;
    let mut d = png::Decoder::new(std::io::BufReader::new(f));
    d.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut r = d.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buf.truncate(info.buffer_size());
    Some((buf, info.width as usize, info.height as usize))
}

/// First frame of a short: portrait, what happened, and who.
pub fn title_card(p: &Painter, w: usize, h: usize, label: &str, brand: &Brand) -> Canvas {
    let mut c = Canvas::new(w, h, NIGHT);
    // a soft spotlight behind the portrait
    let (cx, cy) = (w as f32 / 2.0, h as f32 * 0.36);
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() / (w as f32 * 0.55);
            c.blend(
                x as i64,
                y as i64,
                [52, 60, 96],
                (1.0 - d).clamp(0.0, 1.0) * 0.8,
            );
        }
    }
    if let Some((img, iw, ih)) = portrait(&brand.model) {
        let scale = (w * 6 / 10 / iw).max(1);
        c.paste_rgba(
            &img,
            iw,
            ih,
            ((w - iw * scale) / 2) as i64,
            (cy - (ih * scale) as f32 / 2.0) as i64,
            scale,
        );
    }
    let size = (w as f32 / 20.0).round();
    let lines = p.wrap(label, size, w as f32 * 0.86, 3);
    let y = p.centred(&mut c, &lines, h as f32 * 0.62, size, size * 0.7, GOLD);
    if !brand.character.is_empty() {
        p.centred(
            &mut c,
            &[brand.character.clone()],
            y + size,
            size * 0.6,
            0.0,
            INK,
        );
    }
    c
}

/// Last frame of a short: where to find you.
pub fn end_card(p: &Painter, w: usize, h: usize, brand: &Brand) -> Canvas {
    let mut c = Canvas::new(w, h, NIGHT);
    let size = (w as f32 / 22.0).round();
    if let Some((img, iw, ih)) = portrait(&brand.model) {
        let scale = (w * 45 / 100 / iw).max(1);
        c.paste_rgba(
            &img,
            iw,
            ih,
            ((w - iw * scale) / 2) as i64,
            (h as f32 * 0.5) as i64 - (ih * scale) as i64 - (size * 1.5) as i64,
            scale,
        );
    }
    let (top, bottom) = match &brand.channel {
        Some(ch) => ("Catch it live".to_string(), format!("twitch.tv/{ch}")),
        None => ("Thanks for watching".to_string(), String::new()),
    };
    let y = p.centred(&mut c, &[top], h as f32 * 0.56, size, 0.0, INK);
    if !bottom.is_empty() {
        // one line, smaller for a long channel name
        let fit = (size * w as f32 * 0.9 / p.text_width(&bottom, size).max(1.0))
            .floor()
            .min(size);
        p.centred(&mut c, &[bottom], y + size, fit, 0.0, GOLD);
    }
    c
}

/// The caption strip across the top of each frame of a short.
pub fn caption(p: &Painter, c: &mut Canvas, label: &str) {
    let size = (c.w as f32 / 27.0).round();
    let lines = p.wrap(label, size, c.w as f32 * 0.9, 2);
    let pad = size * 0.8;
    let bar_h =
        pad * 2.0 + lines.len() as f32 * size + (lines.len().saturating_sub(1)) as f32 * size * 0.6;
    let y0 = (c.h as f32 * 0.07).round();
    c.rect(
        0,
        y0 as i64,
        c.w as i64,
        bar_h.round() as i64,
        [0, 0, 0],
        0.62,
    );
    p.centred(c, &lines, y0 + pad, size, size * 0.6, GOLD);
}

/// The bar under a comic strip: what happened on the left, the channel on the right.
pub fn comic_bar(
    p: &Painter,
    c: &mut Canvas,
    y: usize,
    h: usize,
    margin: usize,
    label: &str,
    brand: &Brand,
) {
    c.rect(0, y as i64, c.w as i64, h as i64, [20, 20, 24], 1.0);
    let size = (h as f32 * 0.32).round();
    let right = brand
        .channel
        .as_ref()
        .map(|ch| format!("twitch.tv/{ch}"))
        .unwrap_or_default();
    let rw = if right.is_empty() {
        0.0
    } else {
        p.text_width(&right, size * 0.75)
    };
    let lines = p.wrap(label, size, c.w as f32 - margin as f32 * 3.0 - rw, 1);
    let ty = y as f32 + (h as f32 - size) / 2.0;
    if let Some(l) = lines.first() {
        p.text(c, l, margin as f32, ty, size, GOLD, false);
    }
    if !right.is_empty() {
        p.text(
            c,
            &right,
            c.w as f32 - margin as f32 - rw,
            y as f32 + (h as f32 - size * 0.75) / 2.0,
            size * 0.75,
            INK,
            false,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_fits_and_cards_draw() {
        let p = Painter::load().expect("pixel font");
        let lines = p.wrap(
            "TestCheerer cheered 600 bits and the whole chat went wild about it",
            20.0,
            486.0,
            2,
        );
        assert_eq!(lines.len(), 2);
        assert!(lines[1].ends_with("..."));
        for l in &lines {
            assert!(p.text_width(l, 20.0) <= 486.0, "{l}");
        }
        assert_eq!(p.wrap("short", 20.0, 486.0, 2), vec!["short"]);
        let brand = Brand {
            model: "models/dog.glb".into(),
            character: "Dog".into(),
            channel: Some("somechannel".into()),
        };
        let t = title_card(&p, 540, 960, "Raid from SomeStreamer (60)", &brand);
        // the portrait and the gold text are there
        let gold =
            t.px.chunks(3)
                .filter(|c| c[0] > 200 && c[1] > 150 && c[1] < 200 && c[2] < 80)
                .count();
        assert!(gold > 500, "gold text {gold}");
        let dog =
            t.px.chunks(3)
                .filter(|c| c[0] > 180 && c[1] > 120 && c[1] < 170 && c[2] < 100 && c[2] > 40)
                .count();
        assert!(dog > 2000, "portrait {dog}");
        let e = end_card(
            &p,
            540,
            960,
            &Brand {
                channel: Some("a_rather_long_channel_name".into()),
                ..brand.clone()
            },
        );
        assert!(e.jpeg(85).unwrap().len() > 1000);
        // the whole channel name fits: no gold pixels in the outer 4% on either side
        let edge = (0..960).any(|y| {
            [
                0usize, 1, 2, 3, 4, 5, 6, 7, 8, 9, 530, 531, 532, 533, 534, 535, 536, 537, 538, 539,
            ]
            .iter()
            .any(|x| {
                let i = (y * 540 + x) * 3;
                e.px[i] > 200 && e.px[i + 1] > 150 && e.px[i + 2] < 80
            })
        });
        assert!(!edge, "channel name runs off the card");
        let mut f = Canvas::new(540, 960, [0, 200, 0]);
        caption(&p, &mut f, "Splat!");
        let top = &f.px[(80 * 540) * 3..(81 * 540) * 3];
        assert!(
            top.chunks(3).all(|c| c[1] < 120),
            "the bar darkens the frame"
        );
        assert!(f.px[(500 * 540) * 3 + 1] == 200, "the rest is untouched");
    }
}
