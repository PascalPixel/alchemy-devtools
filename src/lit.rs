//! alchemy-lit: the colour correction chosen for showing Golden Sun's pictures
//! on a modern screen. It starts from the colours the game stores and changes
//! only two families of hue, by their stored OKLCH hue: yellows keep 75% of
//! their saturation and turn 28 degrees towards red (Vale's roofs read gold
//! rather than lemon), and greens keep 72% and turn 5 degrees towards yellow
//! (the grass loses its acid glare). Every other colour, and every lightness,
//! is the game's own: blues, purples (the Psynergy Stone), reds, greys, black
//! and white come out exactly as stored. Each band has smooth edges, so
//! neighbouring hues never jump. A colour a turn carries past what the screen
//! can show is clipped, as Pascal chose (26 October 2026).
//!
//! Pokefan531's GBA colour shader, which the docs site used before, mixes every
//! channel into every other; that one rule turns purples towards blue and greys
//! the stone. alchemy-lit decides by hue instead, which is why no single colour
//! matrix (and no SVG filter of that shape) reproduces it exactly.
//!
//! `alchemy-devtools lit IN.png OUT.png` recolours a picture, indexed (only its
//! palette changes) or true colour.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

/// A band of stored OKLCH hue: from, to, edge width (degrees, a Gaussian fall-off
/// outside), the share of saturation kept, and the degrees turned (positive
/// towards smaller hue: yellow towards red, green towards yellow).
struct Band {
    from: f64,
    to: f64,
    edge: f64,
    keep: f64,
    turn: f64,
}

const YELLOWS: Band = Band {
    from: 80.0,
    to: 115.0,
    edge: 12.0,
    keep: 0.75,
    turn: 28.0,
};
const GREENS: Band = Band {
    from: 122.0,
    to: 152.0,
    edge: 12.0,
    keep: 0.72,
    turn: 5.0,
};

/// How much of a band applies to a hue: 1 inside, falling off smoothly outside.
fn weight(band: &Band, hue: f64) -> f64 {
    let mut out_by = (band.from - hue).max(0.0) + (hue - band.to).max(0.0);
    out_by = out_by.min(360.0 - out_by);
    (-0.5 * (out_by / band.edge).powi(2)).exp()
}

fn to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn from_linear(c: f64) -> f64 {
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.max(0.0).powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB (0..1) to OKLab (Björn Ottosson's matrices).
fn to_oklab([r, g, b]: [f64; 3]) -> [f64; 3] {
    let (r, g, b) = (to_linear(r), to_linear(g), to_linear(b));
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    [
        0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    ]
}

/// OKLab to sRGB (0..1), unclipped.
fn from_oklab([l, a, b]: [f64; 3]) -> [f64; 3] {
    let l_ = (l + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m_ = (l - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s_ = (l - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);
    [
        from_linear(4.076_741_662_1 * l_ - 3.307_711_591_3 * m_ + 0.230_969_929_2 * s_),
        from_linear(-1.268_438_004_6 * l_ + 2.609_757_401_1 * m_ - 0.341_319_396_5 * s_),
        from_linear(-0.004_196_086_3 * l_ - 0.703_418_614_7 * m_ + 1.707_614_701_0 * s_),
    ]
}

/// One colour (sRGB, 0..1) through alchemy-lit.
pub fn lit(rgb: [f64; 3]) -> [f64; 3] {
    let [l, a, b] = to_oklab(rgb.map(|c| c.clamp(0.0, 1.0)));
    let hue = b.atan2(a).to_degrees().rem_euclid(360.0);
    let (wy, wg) = (weight(&YELLOWS, hue), weight(&GREENS, hue));
    let cut = ((1.0 - YELLOWS.keep) * wy).max((1.0 - GREENS.keep) * wg);
    let (a, b) = (a * (1.0 - cut), b * (1.0 - cut));
    let turn = -(YELLOWS.turn * wy + GREENS.turn * wg).to_radians();
    let (sin, cos) = turn.sin_cos();
    from_oklab([l, a * cos - b * sin, a * sin + b * cos]).map(|c| c.clamp(0.0, 1.0))
}

/// One 8-bit colour through alchemy-lit.
pub fn lit8(rgb: [u8; 3]) -> [u8; 3] {
    lit(rgb.map(|c| f64::from(c) / 255.0)).map(|c| (c * 255.0).round() as u8)
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let [input, output] = arguments else {
        return Err("usage: alchemy-devtools lit IN.png OUT.png\nRecolours a picture in alchemy-lit, the colour correction chosen for showing\nGolden Sun's pictures: an indexed picture keeps its pixels and gets a new\npalette, a true-colour one is recoloured pixel by pixel.".into());
    };
    recolour_png(Path::new(input), Path::new(output))
}

fn recolour_png(input: &Path, output: &Path) -> Result<(), String> {
    let error = |e: &dyn std::fmt::Display| format!("{}: {e}", input.display());
    let mut decoder = png::Decoder::new(File::open(input).map_err(|e| error(&e))?);
    decoder.set_transformations(png::Transformations::IDENTITY);
    let mut reader = decoder.read_info().map_err(|e| error(&e))?;
    let mut pixels = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut pixels).map_err(|e| error(&e))?;
    pixels.truncate(frame.buffer_size());
    let info = reader.info();
    if info.bit_depth != png::BitDepth::Eight && info.color_type != png::ColorType::Indexed {
        return Err(error(&"only 8-bit pictures are recoloured"));
    }
    let palette = info.palette.as_ref().map(|p| {
        p.chunks(3)
            .flat_map(|c| lit8([c[0], c[1], c[2]]))
            .collect::<Vec<u8>>()
    });
    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        _ => 0,
    };
    if channels > 0 {
        for px in pixels.chunks_mut(channels) {
            let [r, g, b] = lit8([px[0], px[1], px[2]]);
            px[0] = r;
            px[1] = g;
            px[2] = b;
        }
    } else if info.color_type != png::ColorType::Indexed {
        return Err(error(&"greyscale pictures have no hue to correct"));
    }
    let file = File::create(output).map_err(|e| format!("{}: {e}", output.display()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), info.width, info.height);
    encoder.set_color(info.color_type);
    encoder.set_depth(info.bit_depth);
    if let Some(palette) = palette {
        encoder.set_palette(palette);
    }
    if let Some(trns) = info.trns.as_ref() {
        encoder.set_trns(trns.to_vec());
    }
    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("{}: {e}", output.display()))?;
    writer
        .write_image_data(&pixels)
        .map_err(|e| format!("{}: {e}", output.display()))
}

#[cfg(test)]
mod tests {
    use super::lit8;

    #[test]
    fn colours_outside_the_bands_stay_the_games_own() {
        for c in [
            [0, 0, 0],
            [255, 255, 255],
            [128, 128, 128],
            [255, 0, 0],
            [0, 0, 255],
            [148, 57, 156],
            [32, 96, 200],
        ] {
            assert_eq!(lit8(c), c, "{c:?}");
        }
    }

    #[test]
    fn yellows_and_greens_match_the_chosen_correction() {
        // The values Pascal approved on 26 October 2026, from the prototype.
        for (stored, shown) in [
            ([255, 255, 0], [255, 231, 107]),
            ([248, 200, 40], [255, 187, 118]),
            ([214, 239, 107], [255, 220, 130]),
            ([88, 160, 56], [120, 152, 73]),
            ([123, 247, 49], [181, 233, 91]),
            ([148, 99, 57], [148, 98, 66]),
            ([0, 255, 0], [144, 241, 82]),
        ] {
            assert_eq!(lit8(stored), shown, "{stored:?}");
        }
    }
}
