//! Frames to a looping GIF: every second frame of the GBA's 60 for 30 a
//! second, at the GBA's own size, over one palette for the file. A cast
//! shows a few hundred to a few thousand of the GBA's colours; up to 255 of
//! them are kept exactly, and past that a median cut picks the palette.
//! After the first frame each frame holds only the rectangle that changed,
//! unchanged pixels in it transparent, and a frame the same as the last
//! lengthens it.

use std::collections::HashMap;
use std::path::Path;

/// Display times are whole hundredths of a second: frame i starts at
/// i * 100 / 30, so delays run 3, 4, 3.
const FPS: usize = 30;
/// GBA frames for each frame shown.
const STRIDE: usize = 2;

pub struct Written {
    /// Frames at 30 a second.
    pub frames: usize,
    /// Frames the file stores, after identical ones are joined.
    pub stored: usize,
    pub colours: usize,
    pub bytes: u64,
}

/// A BGR555 colour as 0xRRGGBB, each channel widened as libmgba widens it.
pub fn rgb(c: u16) -> u32 {
    let wide = |v: u16| u32::from((v & 31) << 3 | (v & 31) >> 2);
    wide(c) << 16 | wide(c >> 5) << 8 | wide(c >> 10)
}

fn channels(c: u32) -> [i32; 3] {
    [(c >> 16) as i32 & 0xff, (c >> 8) as i32 & 0xff, c as i32 & 0xff]
}

/// Writes `frames` (0xRRGGBB, `width` pixels a row) to `path`.
pub fn write(path: &Path, frames: &[Vec<u32>], width: usize) -> Result<Written, String> {
    encode(path, &frames.iter().step_by(STRIDE).collect::<Vec<_>>(), width)
}

/// Writes `frames` (BGR555, as the GBA draws them) to `path`, widening only
/// the frames shown.
pub fn write_bgr555(path: &Path, frames: &[&[u16]], width: usize) -> Result<Written, String> {
    let shown: Vec<Vec<u32>> = frames.iter().step_by(STRIDE).map(|f| f.iter().map(|&c| rgb(c)).collect()).collect();
    encode(path, &shown.iter().collect::<Vec<_>>(), width)
}

/// Writes the frames shown, at 30 a second.
fn encode(path: &Path, shown: &[&Vec<u32>], width: usize) -> Result<Written, String> {
    let first = shown.first().ok_or("no frames")?;
    let height = first.len() / width;

    let mut counts: HashMap<u32, u64> = HashMap::new();
    for frame in shown {
        for &c in frame.iter() {
            *counts.entry(c).or_default() += 1;
        }
    }
    let mut histogram: Vec<(u32, u64)> = counts.into_iter().collect();
    histogram.sort_unstable();
    let colours = histogram.len();
    let palette = if colours <= 255 { histogram.iter().map(|&(c, _)| c).collect() } else { median_cut(&histogram, 255) };
    let index: HashMap<u32, u8> = histogram.iter().map(|&(c, _)| (c, nearest(&palette, c))).collect();
    let transparent = palette.len() as u8;
    let mut table: Vec<u8> = palette.iter().flat_map(|&c| [(c >> 16) as u8, (c >> 8) as u8, c as u8]).collect();
    table.extend([0, 0, 0]);

    let file = std::fs::File::create(path).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    let error = |e: gif::EncodingError| format!("{}: {e}", path.display());
    let mut encoder =
        gif::Encoder::new(std::io::BufWriter::new(file), width as u16, height as u16, &table).map_err(error)?;
    encoder.set_repeat(gif::Repeat::Infinite).map_err(error)?;

    let start = |i: usize| (i * 100 + FPS / 2) / FPS;
    let indexed = |frame: &Vec<u32>| -> Vec<u8> { frame.iter().map(|c| index[c]).collect() };
    let mut canvas = indexed(first);
    let mut pending = Some((0, gif_frame(&canvas, None, (0, 0, width, height), width, None)));
    let mut stored = 0;
    for (i, frame) in shown.iter().enumerate().skip(1) {
        let next = indexed(frame);
        let Some(rect) = changed(&canvas, &next, width) else {
            continue;
        };
        if let Some((at, mut f)) = pending.take() {
            f.delay = (start(i) - start(at)) as u16;
            encoder.write_frame(&f).map_err(error)?;
            stored += 1;
        }
        pending = Some((i, gif_frame(&next, Some(&canvas), rect, width, Some(transparent))));
        canvas = next;
    }
    if let Some((at, mut f)) = pending.take() {
        f.delay = (start(shown.len()) - start(at)) as u16;
        encoder.write_frame(&f).map_err(error)?;
        stored += 1;
    }
    encoder.into_inner().map_err(error)?;
    let bytes = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    Ok(Written { frames: shown.len(), stored, colours, bytes })
}

/// The rectangle (x, y, width, height) where two frames differ.
fn changed(old: &[u8], new: &[u8], width: usize) -> Option<(usize, usize, usize, usize)> {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for (i, _) in old.iter().zip(new).enumerate().filter(|(_, (a, b))| a != b) {
        let (x, y) = (i % width, i / width);
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    (x0 != usize::MAX).then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

/// The pixels of `rect`, those the same as `under` transparent.
fn gif_frame(
    frame: &[u8],
    under: Option<&[u8]>,
    (x, y, w, h): (usize, usize, usize, usize),
    width: usize,
    transparent: Option<u8>,
) -> gif::Frame<'static> {
    let mut buffer = Vec::with_capacity(w * h);
    for row in y..y + h {
        for i in row * width + x..row * width + x + w {
            buffer.push(match (under, transparent) {
                (Some(under), Some(t)) if under[i] == frame[i] => t,
                _ => frame[i],
            });
        }
    }
    gif::Frame {
        left: x as u16,
        top: y as u16,
        width: w as u16,
        height: h as u16,
        dispose: gif::DisposalMethod::Keep,
        transparent,
        buffer: buffer.into(),
        ..gif::Frame::default()
    }
}

fn nearest(palette: &[u32], c: u32) -> u8 {
    let target = channels(c);
    let distance = |p: u32| channels(p).iter().zip(target).map(|(a, b)| (a - b) * (a - b)).sum::<i32>();
    palette.iter().enumerate().min_by_key(|(_, &p)| distance(p)).map_or(0, |(i, _)| i as u8)
}

/// At most `limit` colours for a histogram: the box with the most pixels is
/// split along its widest channel at its weighted median until there are
/// `limit` boxes, each standing for its weighted mean.
fn median_cut(histogram: &[(u32, u64)], limit: usize) -> Vec<u32> {
    let weight = |b: &[(u32, u64)]| b.iter().map(|&(_, n)| n).sum::<u64>();
    let mut boxes: Vec<Vec<(u32, u64)>> = vec![histogram.to_vec()];
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
    let mut palette: Vec<u32> = boxes
        .iter()
        .map(|b| {
            let total = weight(b).max(1);
            let mean = |k: usize| (b.iter().map(|&(c, n)| channels(c)[k] as u64 * n).sum::<u64>() + total / 2) / total;
            (mean(0) << 16 | mean(1) << 8 | mean(2)) as u32
        })
        .collect();
    palette.sort_unstable();
    palette.dedup();
    palette
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_widen_as_libmgba_widens_them() {
        assert_eq!(rgb(0x7fff), 0xffffff);
        assert_eq!(rgb(25 | 25 << 5 | 25 << 10), 0xcecece);
        assert_eq!(rgb(31), 0xff0000);
    }

    #[test]
    fn the_changed_rectangle_bounds_every_difference() {
        let old = [0u8; 12];
        let mut new = old;
        new[5] = 1;
        new[10] = 1;
        assert_eq!(changed(&old, &new, 4), Some((1, 1, 2, 2)));
        assert_eq!(changed(&old, &old, 4), None);
    }

    #[test]
    fn median_cut_keeps_far_colours_apart() {
        let histogram = [(0u32, 100), (0x000008, 1), (0x00ff00, 5), (0xff0000, 5)];
        let palette = median_cut(&histogram, 3);
        assert_eq!(palette.len(), 3);
        assert!(palette.contains(&0) && palette.contains(&0x00ff00));
    }
}
