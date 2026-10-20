//! What the battlefield shows behind the fighters. The GBA's backdrop is
//! one colour a line, so the gradient is drawn by the GBA itself, one grey
//! per line. A picture needs a colour per pixel: the cast is played twice,
//! over black and over grey 16, which differ only where the backdrop shows,
//! by 16 times its weight in the GBA's blend (the second target's EVB/16, or
//! 1 - EVY/16 under a brightness fade). Each pixel is then the black run's
//! plus that weight of the picture's, the GBA's own arithmetic in eight bits.

use super::animation::rgb;
use super::battle::{gradient, grey, STRIP};
use super::mgba::{HEIGHT, WIDTH};
use std::path::Path;

/// The gradient's grey at the top and the bottom, in five-bit levels (206
/// and 74 of 255).
const TOP: u16 = 25;
const BOTTOM: u16 = 9;
/// The grey the second run of a picture is played over.
const GREY: u16 = 16;

pub enum Backdrop {
    Gradient,
    /// The battlefield's 240 by 136 pixels, 0xRRGGBB.
    Picture(Vec<u32>),
}

impl Backdrop {
    /// `gradient`, or a PNG scaled to cover the battlefield and centred.
    pub fn parse(value: &str) -> Result<Backdrop, String> {
        if value == "gradient" {
            return Ok(Backdrop::Gradient);
        }
        let (w, h, pixels) = read_png(Path::new(value))?;
        Ok(Backdrop::Picture(cover(w, h, &pixels, WIDTH, STRIP)))
    }

    /// The backdrop lines of each run.
    pub fn runs(&self) -> Vec<[u16; HEIGHT]> {
        match self {
            Backdrop::Gradient => vec![gradient(TOP, BOTTOM)],
            Backdrop::Picture(_) => [0, GREY]
                .iter()
                .map(|&level| {
                    let mut lines = [0; HEIGHT];
                    lines[..STRIP].fill(grey(level));
                    lines
                })
                .collect(),
        }
    }

    /// One frame from the runs' frames, as 0xRRGGBB.
    pub fn compose(&self, frames: &[&Vec<u16>]) -> Vec<u32> {
        match self {
            Backdrop::Gradient => frames[0].iter().map(|&c| rgb(c)).collect(),
            Backdrop::Picture(picture) => compose(frames[0], frames[1], picture),
        }
    }
}

fn five(c: u16, k: u32) -> i32 {
    i32::from((c >> (5 * k)) & 31)
}

/// The black run's pixels plus the picture by each pixel's backdrop weight;
/// the message strip keeps the black run's.
fn compose(black: &[u16], grey: &[u16], picture: &[u32]) -> Vec<u32> {
    black
        .iter()
        .zip(grey)
        .enumerate()
        .map(|(i, (&b, &g))| {
            let base = rgb(b);
            if i >= WIDTH * STRIP {
                return base;
            }
            let weight = (0..3).map(|k| five(g, k) - five(b, k)).max().unwrap_or(0).clamp(0, GREY as i32) as u32;
            let mut out = 0;
            for shift in [16, 8, 0] {
                let value = (base >> shift & 0xff) + ((picture[i] >> shift & 0xff) * weight + 8) / 16;
                out |= value.min(255) << shift;
            }
            out
        })
        .collect()
}

/// A PNG's width, height and pixels as 0xRRGGBB.
fn read_png(path: &Path) -> Result<(usize, usize, Vec<u32>), String> {
    let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let file = std::fs::File::open(path).map_err(|e| error(&e))?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| error(&e))?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).map_err(|e| error(&e))?;
    let samples = info.color_type.samples();
    let pixels = buffer[..info.buffer_size()]
        .chunks(samples)
        .map(|p| match samples {
            1 | 2 => u32::from(p[0]) * 0x010101,
            _ => u32::from(p[0]) << 16 | u32::from(p[1]) << 8 | u32::from(p[2]),
        })
        .collect();
    Ok((info.width as usize, info.height as usize, pixels))
}

/// A picture scaled to cover `width` by `height`, centred, sampled
/// bilinearly.
fn cover(w: usize, h: usize, pixels: &[u32], width: usize, height: usize) -> Vec<u32> {
    let scale = (width as f64 / w as f64).max(height as f64 / h as f64);
    let (ox, oy) = ((w as f64 - width as f64 / scale) / 2.0, (h as f64 - height as f64 / scale) / 2.0);
    let at = |x: usize, y: usize, shift: u32| f64::from(pixels[y.min(h - 1) * w + x.min(w - 1)] >> shift & 0xff);
    let mut out = Vec::with_capacity(width * height);
    for y in 0..height {
        let sy = ((y as f64 + 0.5) / scale + oy - 0.5).max(0.0);
        let (y0, fy) = (sy.floor() as usize, sy.fract());
        for x in 0..width {
            let sx = ((x as f64 + 0.5) / scale + ox - 0.5).max(0.0);
            let (x0, fx) = (sx.floor() as usize, sx.fract());
            let mut colour = 0;
            for shift in [16, 8, 0] {
                let top = at(x0, y0, shift) * (1.0 - fx) + at(x0 + 1, y0, shift) * fx;
                let bottom = at(x0, y0 + 1, shift) * (1.0 - fx) + at(x0 + 1, y0 + 1, shift) * fx;
                colour |= ((top * (1.0 - fy) + bottom * fy).round() as u32).min(255) << shift;
            }
            out.push(colour);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_covers_and_centres() {
        // Four columns of two rows, wider than a square: the middle two
        // columns cover it.
        let pixels = [0x000000, 0x404040, 0x808080, 0xc0c0c0, 0x000000, 0x404040, 0x808080, 0xc0c0c0];
        let out = cover(4, 2, &pixels, 2, 2);
        assert_eq!(out, vec![0x404040, 0x808080, 0x404040, 0x808080]);
    }

    #[test]
    fn the_picture_shows_by_the_backdrop_weight() {
        let picture = vec![0xff8000; WIDTH * STRIP];
        let mut black = vec![0u16; WIDTH * HEIGHT];
        let mut over = vec![grey(16); WIDTH * HEIGHT];
        // Pixel 1: a sprite, the same over both. Pixel 2: half blended.
        black[1] = 0x001f;
        over[1] = 0x001f;
        black[2] = grey(4);
        over[2] = grey(12);
        let out = compose(&black, &over, &picture);
        assert_eq!(out[0], 0xff8000);
        assert_eq!(out[1], 0xff0000);
        assert_eq!(out[2], 0xa16121);
        assert_eq!(out[WIDTH * STRIP], 0);
    }
}
