//! The planes the hardware stacks to draw one moment, each written as its own
//! PNG at the screen's size with its empty pixels transparent, for an
//! illustration (an SVG in alchemy-docs) to place as crisp crops and label
//! with real text. No text is drawn into them.
//!
//! `planes_now` takes whatever moment a mode has reached. `layers-3d` and
//! every mode that pulls a moment apart through `explode_now` call it first,
//! into `planes/<name>/` beside their GIF (`planes/field`, `planes/battle`,
//! `planes/title-intro`); another mode can call it the same way. It writes
//! into one folder:
//!
//! - `frame.png`: the screen as the player sees it;
//! - `backdrop.png`: the colour drawn where no layer draws, filling the screen;
//! - one PNG a plane, named by its layer (`bg3.png`, `sprites.png`; sprites
//!   of several priorities split into `sprites-p1.png` and so on);
//! - `planes.tsv`: the planes back to front, with the register values that
//!   put them there, which the illustration's order is checked against, and
//!   notes where blending, windows or a DMA repeated each line change how a
//!   plane shows. A plane with nothing drawn is listed and not written.
//!
//! The order comes from the priorities, checked against the frame: where two
//! planes both draw in different colours the frame shows the nearer, which
//! settles a register that changes down the screen or a window.
//!
//! A plane is drawn without the alpha blending it may have on screen, since
//! blended alone it would take the colour it is captured over; planes.tsv
//! says which planes are blended and by how much, for the illustration to
//! draw them translucent.

use super::animation;
use super::field::Field;
use super::guide::Guide;
use super::mgba::{HEIGHT, WIDTH};
use std::collections::HashMap;
use std::path::Path;

/// DISPCNT and its layer bits: 8 to 11 turn BG0 to BG3 on, 12 the sprites.
const DISPCNT: u32 = 0x0400_0000;
const LAYER_BITS: u16 = 0x1f00;
/// The first background control register; BG1CNT to BG3CNT follow it.
const BG0CNT: u32 = 0x0400_0008;
/// The window controls: which layers show inside windows 0 and 1, and outside
/// them and inside the sprite window.
const WININ: u32 = 0x0400_0048;
const WINOUT: u32 = 0x0400_004a;
/// The blend control (bits 5..0 the first targets, 7..6 the mode, 13..8 the
/// second targets) and the alpha weights (bits 4..0 the first target's
/// sixteenths, 12..8 the second's).
const BLDCNT: u32 = 0x0400_0050;
const BLDALPHA: u32 = 0x0400_0052;
const BLEND_SECOND_TARGETS: u16 = 0x3f00;
/// DMA channel 0's destination and control; channels 1 to 3 follow 12 bytes
/// apart.
const DMA0DAD: u32 = 0x0400_00b4;
const DMA0CNT_H: u32 = 0x0400_00ba;
/// Sprite attribute memory: 128 entries of 8 bytes.
const OAM: u32 = 0x0700_0000;
/// Background palette colour 0: the backdrop.
const BACKDROP: u32 = 0x0500_0000;
/// The colour an empty pixel is captured over: full magenta, as guide.rs's
/// KEY.
const KEY: u16 = 0x7c1f;
/// The last line of vertical blank: the game's VBlank work is done and the
/// next picture not yet begun.
const LAST_BLANK_LINE: u16 = 227;

/// One plane, back to front.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Plane {
    /// A background, by number, at its BGxCNT priority.
    Background { bg: u32, priority: u16 },
    /// The sprites of one priority (OAM attribute 2 bits 11..10).
    Sprites { priority: u16 },
}

impl Plane {
    fn file(self, split: bool) -> String {
        match self {
            Plane::Background { bg, .. } => format!("bg{bg}.png"),
            Plane::Sprites { priority } if split => format!("sprites-p{priority}.png"),
            Plane::Sprites { .. } => "sprites.png".to_string(),
        }
    }

    fn layer(self) -> String {
        match self {
            Plane::Background { bg, .. } => format!("BG{bg}"),
            Plane::Sprites { .. } => "OBJ".to_string(),
        }
    }

    fn priority(self) -> u16 {
        match self {
            Plane::Background { priority, .. } | Plane::Sprites { priority } => priority,
        }
    }

    /// The plane's bit among BLDCNT's targets.
    fn target(self) -> u16 {
        match self {
            Plane::Background { bg, .. } => 1 << bg,
            Plane::Sprites { .. } => 1 << 4,
        }
    }
}

/// The planes back to front, as the hardware stacks them: a lower priority is
/// nearer, sprites are nearer than a background of the same priority, and of
/// two backgrounds of one priority the lower-numbered is nearer. Unlike
/// guide.rs's `stacking`, which gives all sprites the priority most of them
/// use, each priority the sprites use is its own plane.
pub(super) fn plane_order(backgrounds: &[(u32, u16)], sprite_priorities: &[u16]) -> Vec<Plane> {
    let mut planes: Vec<(u16, u8, u32, Plane)> = backgrounds.iter().map(|&(bg, priority)| (priority, 1, bg, Plane::Background { bg, priority })).collect();
    let mut sprites = sprite_priorities.to_vec();
    sprites.sort_unstable();
    sprites.dedup();
    planes.extend(sprites.into_iter().map(|priority| (priority, 0, 0, Plane::Sprites { priority })));
    planes.sort_by_key(|&(priority, kind, n, _)| (priority, kind, n));
    planes.into_iter().rev().map(|(.., plane)| plane).collect()
}

/// A drawn OAM entry's priority and whether it is semi-transparent, or None
/// where it draws nothing: hidden (attribute 0 bits 9..8 = 2) or a sprite
/// window mask (bits 11..10 = 2).
pub(super) fn oam_entry(attr0: u16, attr2: u16) -> Option<(u16, bool)> {
    if attr0 & 0x300 == 0x200 || attr0 & 0xc00 == 0x800 {
        return None;
    }
    Some((attr2 >> 10 & 3, attr0 & 0xc00 == 0x400))
}

/// How a plane blends on screen, from BLDCNT and BLDALPHA: the first target's
/// weight in sixteenths where it is alpha blended (mode 1, or a
/// semi-transparent sprite, which blends whatever the mode), else None.
pub(super) fn alpha_of(blend: u16, alpha: u16, target: u16, semi_transparent: bool) -> Option<u16> {
    let alpha_mode = blend >> 6 & 3 == 1 && blend & target != 0;
    (alpha_mode || semi_transparent).then_some((alpha & 0x1f).min(16))
}

/// Writes a sheet (None where empty) as an indexed PNG with transparency, or
/// RGBA where it has more than 255 colours.
pub(super) fn write_sheet(path: &Path, width: usize, height: usize, sheet: &[Option<u32>]) -> Result<u64, String> {
    let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| error(&e))?;
    }
    let file = std::fs::File::create(path).map_err(|e| error(&e))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width as u32, height as u32);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Best);
    let mut colours: Vec<u32> = sheet.iter().flatten().copied().collect();
    colours.sort_unstable();
    colours.dedup();
    let empty = sheet.iter().any(Option::is_none);
    let data: Vec<u8> = if colours.len() + usize::from(empty) <= 256 {
        // Index 0 is the empty pixel where there is one.
        let first = u8::from(empty);
        let mut palette: Vec<u8> = if empty { vec![0, 0, 0] } else { Vec::new() };
        palette.extend(colours.iter().flat_map(|&c| [(c >> 16) as u8, (c >> 8) as u8, c as u8]));
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_palette(palette);
        if empty {
            encoder.set_trns(vec![0u8]);
        }
        let index: HashMap<u32, u8> = colours.iter().enumerate().map(|(i, &c)| (c, i as u8 + first)).collect();
        sheet.iter().map(|c| c.map_or(0, |c| index[&c])).collect()
    } else {
        encoder.set_color(png::ColorType::Rgba);
        sheet.iter().flat_map(|c| c.map_or([0; 4], |c| [(c >> 16) as u8, (c >> 8) as u8, c as u8, 255])).collect()
    };
    let mut writer = encoder.write_header().map_err(|e| error(&e))?;
    writer.write_image_data(&data).map_err(|e| error(&e))?;
    writer.finish().map_err(|e| error(&e))?;
    Ok(std::fs::metadata(path).map_err(|e| error(&e))?.len())
}

/// What is captured for one picture: the layer bits to show, the sprite
/// priority to keep (None keeps every sprite), and whether to leave the
/// screen as it is.
#[derive(Clone, Copy)]
struct Capture {
    layers: u16,
    priority: Option<u16>,
    as_shown: bool,
}

impl Guide<'_> {
    /// The moment `field` is at, written as its planes into `folder` (see
    /// the module comment). Every picture is taken two frames on from the
    /// same saved moment: one frame run, then at the end of its vertical
    /// blank, after the game's VBlank work has copied OAM, the registers are
    /// set for the one plane (DISPCNT to its layer, BLDCNT without second
    /// targets so nothing alpha blends, and the OAM entries of other
    /// priorities hidden) and the next frame drawn.
    pub(super) fn planes_now(&mut self, field: &mut Field, folder: &str, what: &str) -> Result<(), String> {
        let saved = field.save();
        let shown = field.read16(DISPCNT);
        let backdrop = animation::rgb(field.read16(BACKDROP));
        let (blend, alpha) = (field.read16(BLDCNT), field.read16(BLDALPHA));
        let (windows_in, windows_out) = (field.read16(WININ), field.read16(WINOUT));
        // DMA channels set to start at each horizontal blank (DMAxCNT_H bits
        // 13..12 = 2) and enabled (bit 15).
        let hblank: Vec<(u32, u32)> = (0..4u32)
            .filter(|&n| field.read16(DMA0CNT_H + 12 * n) & 0xb000 == 0xa000)
            .map(|n| (n, field.read32(DMA0DAD + 12 * n)))
            .collect();
        // Mode 0 has four text backgrounds, mode 1 BG0 to BG2, mode 2 only the
        // affine BG2 and BG3.
        let modes = match shown & 7 {
            0 => 0..4u32,
            1 => 0..3,
            2 => 2..4,
            _ => 2..3,
        };
        let backgrounds: Vec<(u32, u16)> = modes.filter(|bg| shown & (0x100 << bg) != 0).map(|bg| (bg, field.read16(BG0CNT + 2 * bg) & 3)).collect();
        let entries: Vec<(u16, bool)> =
            if shown & 0x1000 != 0 { (0..128u32).filter_map(|i| oam_entry(field.read16(OAM + 8 * i), field.read16(OAM + 8 * i + 4))).collect() } else { Vec::new() };
        let sprite_priorities: Vec<u16> = entries.iter().map(|e| e.0).collect();
        let by_registers = plane_order(&backgrounds, &sprite_priorities);
        let split = by_registers.iter().filter(|p| matches!(p, Plane::Sprites { .. })).count() > 1;

        let capture = |field: &mut Field, c: Capture| -> Result<Vec<Option<u32>>, String> {
            field.load(&saved);
            field.frames(1)?;
            field.until_line(LAST_BLANK_LINE)?;
            if !c.as_shown {
                let layers = (shown & !LAYER_BITS) | c.layers;
                field.io_write16(DISPCNT, layers);
                field.io_write16(BLDCNT, field.read16(BLDCNT) & !BLEND_SECOND_TARGETS);
                if let Some(keep) = c.priority {
                    for i in 0..128u32 {
                        let attr0 = field.read16(OAM + 8 * i);
                        if oam_entry(attr0, field.read16(OAM + 8 * i + 4)).is_some_and(|(p, _)| p != keep) {
                            field.write16(OAM + 8 * i, (attr0 & !0x300) | 0x200);
                        }
                    }
                }
                field.set_backdrop(KEY);
            }
            field.frames(1)?;
            if !c.as_shown && field.read16(DISPCNT) & LAYER_BITS != c.layers {
                return Err(format!("the game rewrote DISPCNT while {:#x} was captured", c.layers));
            }
            let picture = field.picture();
            field.clear_backdrop();
            Ok(picture.iter().map(|&p| (c.as_shown || p != KEY).then(|| animation::rgb(p))).collect())
        };
        let mut sheets: Vec<Sheet> = Vec::new();
        for &plane in &by_registers {
            let (c, semi) = match plane {
                Plane::Background { bg, .. } => (Capture { layers: 0x100 << bg, priority: None, as_shown: false }, false),
                Plane::Sprites { priority } => (Capture { layers: 0x1000, priority: Some(priority), as_shown: false }, entries.iter().any(|&(p, s)| p == priority && s)),
            };
            let pixels = capture(field, c)?;
            sheets.push(Sheet { plane, pixels, semi, blended: alpha_of(blend, alpha, plane.target(), semi) });
        }
        let frame = capture(field, Capture { layers: shown & LAYER_BITS, priority: None, as_shown: true })?;
        field.load(&saved);
        // The registers can change down the screen (a DMA each line) and the
        // windows choose layers by place, so the order is checked against the
        // frame itself: where two unblended planes both draw, in different
        // colours, the frame shows the nearer one.
        let order = order_by_frame(&sheets, &frame);

        let mut rows = vec!["# back to front\tfile\tlayer\tpriority\tdrawn pixels\talpha (sixteenths)\twhat".to_string()];
        let mut written = Vec::new();
        let mut write = |file: &str, sheet: &[Option<u32>], layer: &str, priority: String, blended: Option<u16>, note: &str| -> Result<(), String> {
            let drawn = sheet.iter().filter(|c| c.is_some()).count();
            let blended = blended.map_or("-".to_string(), |a| a.to_string());
            if drawn == 0 {
                rows.push(format!("-\t-\t{layer}\t{priority}\t0\t{blended}\tnothing drawn, not written"));
                return Ok(());
            }
            let bytes = write_sheet(&self.out.join(folder).join(file), WIDTH, HEIGHT, sheet)?;
            let place = if layer == "screen" { "-".to_string() } else { (written.len() + 1).to_string() };
            rows.push(format!("{place}\t{file}\t{layer}\t{priority}\t{drawn}\t{blended}\t{note}"));
            written.push((format!("{folder}/{file}"), bytes, format!("{what}: {note}")));
            Ok(())
        };
        write("backdrop.png", &vec![Some(backdrop); WIDTH * HEIGHT], "backdrop", "-".to_string(), None, "the backdrop colour")?;
        for &i in &order {
            let s = &sheets[i];
            let note = match s.plane {
                Plane::Background { bg, .. } => format!("BG{bg} alone"),
                Plane::Sprites { .. } if s.semi => "the sprites, some semi-transparent".to_string(),
                Plane::Sprites { .. } => "the sprites".to_string(),
            };
            write(&s.plane.file(split), &s.pixels, &s.plane.layer(), s.plane.priority().to_string(), s.blended, &note)?;
        }
        write("frame.png", &frame, "screen", "-".to_string(), None, "the screen as the player sees it")?;
        rows.push(format!("# DISPCNT {shown:#06x}, BLDCNT {blend:#06x}, BLDALPHA {alpha:#06x}, WININ {windows_in:#06x}, WINOUT {windows_out:#06x}"));
        if shown & 0xe000 != 0 {
            rows.push("# windows are on: WININ and WINOUT choose which layers show where".to_string());
        }
        for (channel, destination) in &hblank {
            rows.push(format!("# DMA{channel} repeats each line into {destination:#010x}, so a register it writes changes down the screen"));
        }
        let moved: Vec<String> = order.iter().enumerate().filter(|&(at, &i)| at != i).map(|(_, &i)| sheets[i].plane.layer()).collect();
        if !moved.is_empty() {
            rows.push(format!("# the order follows the frame rather than the priorities read in vertical blank for {}", moved.join(", ")));
        }
        let tsv = self.out.join(folder).join("planes.tsv");
        std::fs::write(&tsv, rows.join("\n") + "\n").map_err(|e| format!("{}: {e}", tsv.display()))?;
        self.written.extend(written);
        let names: Vec<String> = order.iter().map(|&i| format!("{} (priority {})", sheets[i].plane.layer(), sheets[i].plane.priority())).collect();
        self.written.push((format!("{folder}/planes.tsv"), 0, format!("{what}, back to front: backdrop, {}", names.join(", "))));
        Ok(())
    }
}

/// One captured plane.
struct Sheet {
    plane: Plane,
    pixels: Vec<Option<u32>>,
    semi: bool,
    blended: Option<u16>,
}

/// The fewest pixels two planes must overlap on, in different colours, for
/// the frame to settle which is nearer.
const EVIDENCE: usize = 16;

/// The sheets' order back to front (indexes into `sheets`, which are in the
/// registers' order): adjacent sheets swap wherever the frame shows the one
/// behind on most of the pixels both draw in different colours, until none
/// does. Blended sheets show in neither colour and keep their place.
fn order_by_frame(sheets: &[Sheet], frame: &[Option<u32>]) -> Vec<usize> {
    let nearer = |back: usize, front: usize| -> Option<bool> {
        if sheets[back].blended.is_some() || sheets[front].blended.is_some() {
            return None;
        }
        let (mut shows_front, mut shows_back) = (0, 0);
        for ((b, f), shown) in sheets[back].pixels.iter().zip(&sheets[front].pixels).zip(frame) {
            if let (Some(b), Some(f), Some(shown)) = (b, f, shown) {
                if b != f {
                    if shown == f {
                        shows_front += 1;
                    } else if shown == b {
                        shows_back += 1;
                    }
                }
            }
        }
        (shows_front + shows_back >= EVIDENCE).then_some(shows_front >= shows_back)
    };
    let mut order: Vec<usize> = (0..sheets.len()).collect();
    for _ in 0..sheets.len() {
        let mut swapped = false;
        for at in 0..order.len().saturating_sub(1) {
            if nearer(order[at], order[at + 1]) == Some(false) {
                order.swap(at, at + 1);
                swapped = true;
            }
        }
        if !swapped {
            break;
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planes_stack_as_the_hardware_draws_them() {
        // Most layouts: BG3, BG2 and BG1 at 3, 2 and 1, sprites at 2.
        let order = plane_order(&[(1, 1), (2, 2), (3, 3)], &[2, 2, 2]);
        assert_eq!(order, vec![
            Plane::Background { bg: 3, priority: 3 },
            Plane::Background { bg: 2, priority: 2 },
            Plane::Sprites { priority: 2 },
            Plane::Background { bg: 1, priority: 1 },
        ]);
        // Two backgrounds at one priority: the lower number is nearer; a
        // second sprite priority is a plane of its own.
        let order = plane_order(&[(1, 1), (2, 1), (3, 3)], &[0, 2]);
        assert_eq!(order, vec![
            Plane::Background { bg: 3, priority: 3 },
            Plane::Sprites { priority: 2 },
            Plane::Background { bg: 2, priority: 1 },
            Plane::Background { bg: 1, priority: 1 },
            Plane::Sprites { priority: 0 },
        ]);
    }

    #[test]
    fn oam_entries_and_blending() {
        // Hidden, and a sprite window mask: nothing drawn.
        assert_eq!(oam_entry(0x0200, 0), None);
        assert_eq!(oam_entry(0x0800, 0), None);
        // Priority 3; semi-transparent at priority 1.
        assert_eq!(oam_entry(0x8000, 3 << 10), Some((3, false)));
        assert_eq!(oam_entry(0x0400, 1 << 10), Some((1, true)));
        // The title intro's BLDCNT: sprites the first target of alpha mode.
        assert_eq!(alpha_of(0x3f50, 0x0a06, 1 << 4, false), Some(6));
        assert_eq!(alpha_of(0x3f50, 0x0a06, 1 << 2, false), None);
        // Brightening (mode 2) is drawn into the plane, not blended.
        assert_eq!(alpha_of(0x3f9e, 0x0a06, 1 << 2, false), None);
        assert_eq!(alpha_of(0x0000, 0x0a06, 1 << 4, true), Some(6));
    }

    #[test]
    fn the_frame_settles_which_plane_is_nearer() {
        // The registers put the floor (1) in front of the units (0); the
        // frame shows the units where both draw.
        let n = 20;
        let units = Sheet { plane: Plane::Sprites { priority: 2 }, pixels: vec![Some(1); n], semi: false, blended: None };
        let floor = Sheet { plane: Plane::Background { bg: 2, priority: 1 }, pixels: vec![Some(2); n], semi: false, blended: None };
        assert_eq!(order_by_frame(&[units, floor], &vec![Some(1); n]), vec![1, 0]);
        // Too few pixels to tell: the registers' order stays.
        let units = Sheet { plane: Plane::Sprites { priority: 2 }, pixels: vec![Some(1); 4], semi: false, blended: None };
        let floor = Sheet { plane: Plane::Background { bg: 2, priority: 1 }, pixels: vec![Some(2); 4], semi: false, blended: None };
        assert_eq!(order_by_frame(&[units, floor], &vec![Some(1); 4]), vec![0, 1]);
    }

    #[test]
    fn sheets_keep_their_empty_pixels_transparent() {
        let dir = std::env::temp_dir().join(format!("alchemy-planes-{}", std::process::id()));
        let path = dir.join("sheet.png");
        let sheet = [None, Some(0xff0000), Some(0x00ff00), None];
        write_sheet(&path, 2, 2, &sheet).unwrap();
        let decoder = png::Decoder::new(std::fs::File::open(&path).unwrap());
        let mut reader = decoder.read_info().unwrap();
        let info = reader.info();
        assert_eq!(info.color_type, png::ColorType::Indexed);
        assert_eq!(info.trns.as_deref(), Some(&[0u8][..]));
        let mut data = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut data).unwrap();
        // One byte a pixel; empty pixels take index 0.
        assert_eq!(data[0], 0);
        assert_ne!(data[1], 0);
        assert_eq!(data[3], 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
