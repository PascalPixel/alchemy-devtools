//! What the battlefield shows behind the fighters: the battle's own
//! background, a grey gradient, or a picture of the game's backdrop size
//! (256 by 120) put in place of the battle's own, in its format. The game
//! then draws, scrolls and blends the picture itself.

use super::battle::{gradient, Background, Scene};
use std::collections::HashMap;
use std::path::Path;

/// A battle backdrop's size (games/tbs/src/graphics/battle_backgrounds).
pub const WIDTH: usize = 256;
pub const HEIGHT: usize = 120;
/// The colours of a backdrop's palette, BG palettes 6 to 13.
const COLOURS: usize = 128;
/// The first BG palette entry the picture's pixels name (palette 6).
const FIRST_ENTRY: u8 = 96;

/// The gradient's grey at the top and the bottom, in five-bit levels (206
/// and 74 of 255).
const TOP: u16 = 25;
const BOTTOM: u16 = 9;

/// `game`, `gradient`, or the path of a 256 by 120 PNG.
pub fn scene(value: &str) -> Result<Scene, String> {
    match value {
        "game" => Ok(Scene::Game),
        "gradient" => Ok(Scene::Lines(gradient(TOP, BOTTOM))),
        path => {
            let (w, h, pixels) = read_png(Path::new(path))?;
            if (w, h) != (WIDTH, HEIGHT) {
                return Err(format!("{path}: {w} by {h}; a battle backdrop is {WIDTH} by {HEIGHT}"));
            }
            Ok(Scene::Picture(background(&pixels)))
        }
    }
}

/// A picture (0xRRGGBB) as a battle background: its colours reduced to 128
/// BGR555 colours, and its pixels as BattleBackground_Load leaves them in
/// VRAM: 8 by 8 tiles in rows of 32, a byte a pixel naming a colour of BG
/// palettes 6 to 13 (a backdrop PNG's index plus 96, as the Suhalla Gate
/// backdrop decodes).
pub fn background(pixels: &[u32]) -> Background {
    let five: Vec<u16> = pixels.iter().map(|&c| bgr555(c)).collect();
    let mut counts: HashMap<u16, u64> = HashMap::new();
    for &c in &five {
        *counts.entry(c).or_default() += 1;
    }
    let mut histogram: Vec<(u16, u64)> = counts.into_iter().collect();
    histogram.sort_unstable();
    let mut palette: Vec<u16> =
        if histogram.len() <= COLOURS { histogram.iter().map(|&(c, _)| c).collect() } else { median_cut(&histogram, COLOURS) };
    let index: HashMap<u16, u8> = histogram.iter().map(|&(c, _)| (c, nearest(&palette, c))).collect();
    let mut vram = Vec::with_capacity(WIDTH * HEIGHT);
    for tile in 0..WIDTH * HEIGHT / 64 {
        let (tx, ty) = (tile % (WIDTH / 8), tile / (WIDTH / 8));
        for y in 0..8 {
            for x in 0..8 {
                vram.push(FIRST_ENTRY + index[&five[(ty * 8 + y) * WIDTH + tx * 8 + x]]);
            }
        }
    }
    palette.resize(COLOURS, 0);
    Background { vram, palette }
}

/// 0xRRGGBB to BGR555: the nearest eighth of each channel, as the build's
/// palettes take a channel that is a multiple of eight (ags-graphics).
fn bgr555(c: u32) -> u16 {
    let five = |shift: u32| ((((c >> shift) & 0xff) + 4) >> 3).min(31) as u16;
    five(16) | five(8) << 5 | five(0) << 10
}

fn channels(c: u16) -> [i32; 3] {
    [i32::from(c & 31), i32::from((c >> 5) & 31), i32::from((c >> 10) & 31)]
}

fn nearest(palette: &[u16], c: u16) -> u8 {
    let target = channels(c);
    let distance = |p: u16| channels(p).iter().zip(target).map(|(a, b)| (a - b) * (a - b)).sum::<i32>();
    palette.iter().enumerate().min_by_key(|(_, &p)| distance(p)).map_or(0, |(i, _)| i as u8)
}

/// At most `limit` colours for a histogram: the box with the most pixels is
/// split along its widest channel at its weighted median, each box standing
/// for its weighted mean.
fn median_cut(histogram: &[(u16, u64)], limit: usize) -> Vec<u16> {
    let weight = |b: &[(u16, u64)]| b.iter().map(|&(_, n)| n).sum::<u64>();
    let mut boxes: Vec<Vec<(u16, u64)>> = vec![histogram.to_vec()];
    while boxes.len() < limit {
        let Some((at, _)) =
            boxes.iter().enumerate().filter(|(_, b)| b.len() > 1).max_by_key(|(_, b)| weight(b))
        else {
            break;
        };
        let mut b = boxes.swap_remove(at);
        let spread = |k: usize| {
            let values = b.iter().map(|&(c, _)| channels(c)[k]);
            values.clone().max().unwrap_or(0) - values.min().unwrap_or(0)
        };
        let k = (0..3).max_by_key(|&k| spread(k)).unwrap_or(0);
        b.sort_by_key(|&(c, _)| channels(c)[k]);
        let total = weight(&b);
        let mut seen = 0;
        let mut split = b.len() - 1;
        for (i, &(_, n)) in b.iter().enumerate() {
            seen += n;
            if seen * 2 >= total {
                split = (i + 1).clamp(1, b.len() - 1);
                break;
            }
        }
        let rest = b.split_off(split);
        boxes.push(b);
        boxes.push(rest);
    }
    let mut palette: Vec<u16> = boxes
        .iter()
        .map(|b| {
            let total = weight(b).max(1);
            let mean = |k: usize| (b.iter().map(|&(c, n)| channels(c)[k] as u64 * n).sum::<u64>() + total / 2) / total;
            (mean(0) | mean(1) << 5 | mean(2) << 10) as u16
        })
        .collect();
    palette.sort_unstable();
    palette.dedup();
    palette
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_round_to_five_bits() {
        assert_eq!(bgr555(0xffffff), 0x7fff);
        assert_eq!(bgr555(0xff0000), 0x001f);
        assert_eq!(bgr555(0x000000), 0);
    }

    #[test]
    fn a_picture_names_palettes_6_to_13() {
        let mut pixels = vec![0x000000; WIDTH * HEIGHT];
        pixels[1] = 0xffffff;
        let b = background(&pixels);
        assert_eq!(b.palette.len(), COLOURS);
        assert_eq!((b.vram[0], b.vram[1]), (FIRST_ENTRY, FIRST_ENTRY + 1));
        // Pixel (0, 8) begins the second row of tiles.
        let mut pixels = vec![0x000000; WIDTH * HEIGHT];
        pixels[8 * WIDTH] = 0xffffff;
        let b = background(&pixels);
        assert_eq!(b.vram[32 * 64], FIRST_ENTRY + 1);
        assert_eq!((b.palette[0], b.palette[1]), (0, 0x7fff));
    }

    #[test]
    fn many_colours_reduce_to_the_palette() {
        let pixels: Vec<u32> = (0..WIDTH * HEIGHT).map(|i| (i as u32 % 256) * 0x010101 + (i as u32 / 256) * 0x10000).collect();
        let b = background(&pixels);
        assert!(b.vram.iter().all(|&v| (FIRST_ENTRY..FIRST_ENTRY + COLOURS as u8).contains(&v)));
    }
}
