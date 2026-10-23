//! The world map's tilted view taken apart: the flat ground the hardware
//! samples, and the part of it each screen line reads.

use super::animation;
use super::field::{Field, Image};
use super::front::affine_plane;
use super::guide::Guide;
use super::guide_more::{canvas, mix, paste, screen};
use super::guide_take::text;
use super::mgba::{HEIGHT, WIDTH};

const KEY_DOWN: u32 = 0x80;
const KEY_L: u32 = 0x200;
const WHITE: u32 = 0xff_ff_ff;

/// The world map's scene and the entrance that leaves Vale.
const WORLD_MAP: (u16, u16) = (2, 1);
/// struct WorldMapFrameWork in HEAP_SLOT_WORLD_MAP_FRAME: its two pages of 160 lines'
/// struct WorldMapAffinePair (BG2's line, then BG3's, 16 bytes each).
const HEAP_SLOT_WORLD_MAP_FRAME: u32 = 7;
const FRAME_PAGES: u32 = 0xc80;
const PAIR: u32 = 32;
const PAGE: u32 = 160 * PAIR;
/// REG_BG2CNT and REG_BG3CNT.
const BG2CNT: u32 = 0x0400_000c;
const BG3CNT: u32 = 0x0400_000e;
/// The affine planes: 512 pixels square, wrapping (BGCNT_WRAP); a plane
/// pixel is two world map pixels, since a 32-pixel cell is drawn as 2 by 2
/// tiles (WorldMap_WriteLayerCellTile).
const PLANE: i32 = 512;
const WORLD_PER_PLANE: i32 = 2;
/// The screen lines picked out in colour, top to bottom, and their colours.
const PICKED: [usize; 5] = [0, 40, 80, 120, 159];
const PICKED_COLOURS: [u32; 5] = [0xff_40_40, 0xff_a0_20, 0xff_f0_40, 0x40_e0_ff, 0xe0_60_ff];

/// One screen line's BG2 affine values as the HBlank DMA feeds them
/// (struct WorldMapAffineLine): the matrix's first column in 8.8 fixed point and
/// the reference point in 20.8, both in plane pixels.
#[derive(Clone, Copy)]
struct Line {
    pa: i32,
    pc: i32,
    x: i32,
    y: i32,
}

impl Line {
    /// The plane pixel screen pixel `sx` of this line reads, unwrapped.
    fn at(&self, sx: i32) -> (i32, i32) {
        ((self.x + self.pa * sx) >> 8, (self.y + self.pc * sx) >> 8)
    }
}

/// The 160 lines of affine page `page`.
fn page(field: &Field, frame_work: u32, page: u32) -> Vec<Line> {
    (0..HEIGHT as u32)
        .map(|y| {
            let at = frame_work + FRAME_PAGES + page * PAGE + y * PAIR;
            Line { pa: i32::from(field.read16(at) as i16), pc: i32::from(field.read16(at + 4) as i16), x: field.read32(at + 8) as i32, y: field.read32(at + 12) as i32 }
        })
        .collect()
}

/// The ground as the hardware holds it: BG2 (mountains, woods and towns)
/// over BG3 (the ground), both drawn at priority 2, where BG2's colour index
/// isn't 0.
fn plane(field: &Field) -> Vec<u32> {
    let (ground, _) = affine_plane(field, field.read16(BG3CNT));
    let (over, index) = affine_plane(field, field.read16(BG2CNT));
    (0..(PLANE * PLANE) as usize).map(|i| if index[i] != 0 { over.pixels[i] } else { ground.pixels[i] }).collect()
}

fn texel(plane: &[u32], (u, v): (i32, i32)) -> u32 {
    plane[(v.rem_euclid(PLANE) * PLANE + u.rem_euclid(PLANE)) as usize]
}

/// The screen drawn from `plane` through `lines` alone, as the hardware
/// draws BG2 and BG3: what the page says the screen shows without its
/// sprites.
fn synthesize(plane: &[u32], lines: &[Line]) -> Vec<u32> {
    let mut out = Vec::with_capacity(WIDTH * HEIGHT);
    for line in lines {
        for sx in 0..WIDTH as i32 {
            out.push(texel(plane, line.at(sx)));
        }
    }
    out
}

/// One moment: the screen, the plane, the page the DMA fed it and the party.
struct Moment {
    picture: Image,
    plane: Vec<u32>,
    lines: Vec<Line>,
    party: (i32, i32),
    zoomed: bool,
}

impl Guide<'_> {
    /// The world map laid flat beside the tilted screen, as
    /// `world-map-footprint.gif`: the party walks south from Vale, then L
    /// zooms the view out (FieldEffect_RunVisibilityTransition) and back. The
    /// flat part is BG2 over BG3 as they sit in video memory, the 2 by 2
    /// squares unpacked around the party, at their own size (a pixel is two
    /// of the world's). On it, every plane pixel some screen pixel reads
    /// is lit and the rest dimmed, so the trapezoid the 160 lines sample
    /// shows, and five lines are picked out in colour on both sides. Each
    /// line's values are read from the affine page the HBlank DMA feeds
    /// (WorldMap_ApplyAffineFrame): of the two pages, the one whose
    /// lines draw the screen's ground pixel for pixel. The pitch never
    /// changes on the world map (0xe000, set by
    /// WorldMap_InitializePerspective), and every line meets the ground, so
    /// there is no horizon on screen.
    pub fn world_map_footprint(&mut self) -> Result<(), String> {
        let slots = self.sym("gHeapSlots")?;
        let mut field = self.enter(WORLD_MAP)?;
        let mut moments = Vec::new();
        let steps: [(u32, u32); 4] = [(KEY_DOWN, 90), (0, 20), (KEY_L, 70), (0, 40)];
        for (keys, n) in steps {
            field.hold(keys);
            for _ in 0..n {
                field.frames(1)?;
                moments.push(Self::moment(&field, slots, keys == KEY_L)?);
            }
        }
        field.hold(0);
        // The crop of the plane: every line's reach over the whole walk, in
        // unwrapped plane pixels, and a margin.
        let (mut u0, mut v0, mut u1, mut v1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for m in &moments {
            for line in &m.lines {
                for sx in [0, WIDTH as i32 - 1] {
                    let (u, v) = line.at(sx);
                    (u0, v0, u1, v1) = (u0.min(u), v0.min(v), u1.max(u), v1.max(v));
                }
            }
        }
        let margin = 24;
        let (u0, v0) = (u0 - margin, v0 - margin);
        let (cw, ch) = ((u1 - u0 + 1 + margin) as usize, (v1 - v0 + 1 + margin) as usize);
        // The planes hold only the 2 by 2 squares around the party
        // (WorldMap_UpdateCurrentTileBlock: from 256 world pixels left of it and
        // 320 above, in 512-pixel squares); past them the wrap shows other
        // squares, so the crop must stay inside them throughout.
        for m in &moments {
            let left = ((m.party.0 - 256) >> 9) * 512 / WORLD_PER_PLANE;
            let top = ((m.party.1 - 320) >> 9) * 512 / WORLD_PER_PLANE;
            if u0 < left || v0 < top || u0 + cw as i32 > left + PLANE || v0 + ch as i32 > top + PLANE {
                return Err("the crop leaves the squares unpacked around the party".to_string());
            }
        }
        let scale = 2usize;
        let (sw, sh) = (2 * WIDTH, 2 * HEIGHT);
        let gap = 8;
        let label = 20;
        let width = sw.max(cw * scale) + 2 * gap;
        let height = gap + label + sh + gap + label + ch * scale + gap;
        let mut frames = Vec::new();
        for m in &moments {
            let mut out = canvas(width, height, 0x20_20_24);
            // The screen, its picked lines drawn across it.
            let mut shown = Image { width: WIDTH, height: HEIGHT, pixels: m.picture.pixels.clone() };
            for (k, &y) in PICKED.iter().enumerate() {
                for x in 0..WIDTH {
                    shown.pixels[y * WIDTH + x] = PICKED_COLOURS[k];
                }
            }
            let sx0 = (width - sw) / 2;
            text(&mut out, sx0 as i32, gap as i32, if m.zoomed { "THE SCREEN WITH L HELD" } else { "THE SCREEN" }, WHITE, None, 2);
            paste(&mut out, &shown, sx0, gap + label, 2);
            // The plane, dimmed but where a screen pixel reads it.
            let mut flat = canvas(cw, ch, 0);
            let mut read = vec![false; cw * ch];
            for line in &m.lines {
                for sx in 0..WIDTH as i32 {
                    let (u, v) = line.at(sx);
                    let (cx, cy) = (u - u0, v - v0);
                    if cx >= 0 && cy >= 0 && (cx as usize) < cw && (cy as usize) < ch {
                        read[cy as usize * cw + cx as usize] = true;
                    }
                }
            }
            for cy in 0..ch {
                for cx in 0..cw {
                    let c = texel(&m.plane, (u0 + cx as i32, v0 + cy as i32));
                    flat.pixels[cy * cw + cx] = if read[cy * cw + cx] { c } else { mix(c, 0, 0.6) };
                }
            }
            for (k, &y) in PICKED.iter().enumerate() {
                for sx in 0..WIDTH as i32 {
                    let (u, v) = m.lines[y].at(sx);
                    let (cx, cy) = (u - u0, v - v0);
                    if cx >= 0 && cy >= 0 && (cx as usize) < cw && (cy as usize) < ch {
                        flat.pixels[cy as usize * cw + cx as usize] = PICKED_COLOURS[k];
                    }
                }
            }
            // The party's place on the ground, a white cross.
            let (px, py) = (m.party.0 / WORLD_PER_PLANE - u0, m.party.1 / WORLD_PER_PLANE - v0);
            for d in -3..=3 {
                for (x, y) in [(px + d, py), (px, py + d)] {
                    if x >= 0 && y >= 0 && (x as usize) < cw && (y as usize) < ch {
                        flat.pixels[y as usize * cw + x as usize] = WHITE;
                    }
                }
            }
            let fy = gap + label + sh + gap;
            let fx = (width - cw * scale) / 2;
            text(&mut out, fx as i32, fy as i32, "THE GROUND IN VIDEO MEMORY", WHITE, None, 2);
            paste(&mut out, &flat, fx, fy + label, scale);
            frames.push(out.pixels);
        }
        let written = animation::write(&self.out.join("world-map-footprint.gif"), &frames, width)?;
        self.written.push((
            "world-map-footprint.gif".to_string(),
            written.bytes,
            format!("the world map's tilted screen above the flat ground its lines read, walking and zoomed out; {} frames", written.frames),
        ));
        Ok(())
    }

    /// The world map now: the screen, the planes, the page whose lines draw
    /// the screen's ground, and the party.
    fn moment(field: &Field, slots: u32, zoomed: bool) -> Result<Moment, String> {
        let frame_work = field.read32(slots + 4 * HEAP_SLOT_WORLD_MAP_FRAME);
        let picture = screen(&field.picture());
        let plane = plane(field);
        // The page the DMA fed this picture: the one that draws it. The
        // sprites cover some pixels, so the better match wins, and
        // it must match nearly all.
        let mut best = (0usize, Vec::new());
        for p in 0..2 {
            let lines = page(field, frame_work, p);
            let drawn = synthesize(&plane, &lines);
            let same = drawn.iter().zip(&picture.pixels).filter(|(a, b)| a == b).count();
            if same > best.0 {
                best = (same, lines);
            }
        }
        if best.0 * 10 < WIDTH * HEIGHT * 9 {
            return Err(format!("neither affine page draws the screen: at best {} of {} pixels", best.0, WIDTH * HEIGHT));
        }
        let leader = field.leader().ok_or("no leader")?;
        Ok(Moment { picture, plane, lines: best.1, party: (leader.x, leader.z), zoomed })
    }
}
