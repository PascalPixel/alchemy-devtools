//! Take-apart pictures for the window and shop guides: the window system's
//! work block byte by byte in both layouts, its decoded-text ring as a
//! message is decoded and typed, a window's sprites outlined as the
//! sprites they are, the area name's countdown, the menu slot shared by a
//! counter and the field menu, a sanctum's choice of members and the item
//! picker.

use super::field::{Image, Symbols};
use super::guide::Guide;
use super::guide_more::{canvas, mix, outline, paste, screen};
use super::guide_take::text;
use super::mgba::{HEIGHT, WIDTH};
use super::story::Trigger;

const KEY_A: u32 = 0x1;

/// Vale's part with the Sanctum, by day, and Garet's sister there.
const VALE: (u16, u16) = (4, 1);
const GARETS_SISTER: u16 = 18;
/// Talked to, her two pages typed and the window closed, as `talk` plays it.
const TALK: [(u32, u32); 7] = [(0, 20), (KEY_A, 3), (0, 150), (KEY_A, 3), (0, 120), (KEY_A, 3), (0, 40)];

/// The window system's work block (struct WindowWork, window.h), heap slot
/// HEAP_SLOT_WINDOW (gWindowWork), as tbs_edition.h places its fields.
#[derive(Clone, Copy)]
struct Layout {
    name: &'static str,
    window_count: u32,
    messages: u32,
    sprites: u32,
    text_tile_state: u32,
    text_buffer: u32,
    codes: u32,
    count: u32,
    argument_values: u32,
    portrait_pictures: u32,
    size: u32,
}

const INTERNATIONAL: Layout = Layout { name: "INTERNATIONAL", window_count: 8, messages: 0x620, sprites: 0x698, text_tile_state: 0xda0, text_buffer: 0xeb0, codes: 512, count: 0x12b2, argument_values: 0x12bc, portrait_pictures: 0x12ec, size: 0x12fc };
const JAPANESE: Layout = Layout { name: "JAPANESE", window_count: 12, messages: 0x6b0, sprites: 0x728, text_tile_state: 0xe30, text_buffer: 0xf40, codes: 256, count: 0x1142, argument_values: 0x114c, portrait_pictures: 0x117c, size: 0x118c };

/// struct Window, struct Message and struct WindowSprite (window.h,
/// render_input.h): sizes and the offsets read here.
const WINDOWS: u32 = 0x500;
const WINDOW_SIZE: u32 = 0x24;
const WINDOW_WIDTH: u32 = 0x08;
const WINDOW_FLAGS: u32 = 0x16;
const MESSAGE_TEXT_INDEX: u32 = 0x12;
const SPRITE_SIZE: u32 = 0x1c;
const SPRITE_KIND: u32 = 0x04;
const SPRITE_MOTION: u32 = 0x05;
const SPRITE_OAM: u32 = 0x14;
const SPRITE_COUNT: u32 = 64;
/// heap.h's HEAP_SLOT_WINDOW.
const HEAP_SLOT_WINDOW: u32 = 15;
/// Sprite attribute memory.
const OAM: u32 = 0x0700_0000;

const BACK: u32 = 0x20_22_28;
const TEXT: u32 = 0xe8_e8_e8;
const DIM: u32 = 0x80_84_8c;
const HEAD: u32 = 0x9c_c8_ff;
const MARK: u32 = 0xff_d8_40;

/// One frame of the talk: the screen and the work block's bytes.
struct TalkFrame {
    screen: Vec<u16>,
    block: Vec<u8>,
    /// The 128 OAM entries, four halfwords each.
    oam: Vec<u16>,
}

/// The block's fields as regions: name, first byte, end, colour.
fn regions(l: &Layout) -> Vec<(&'static str, u32, u32, u32)> {
    let after_sprites = l.sprites + SPRITE_SIZE * SPRITE_COUNT;
    let ring_end = l.text_buffer + 2 * l.codes;
    vec![
        ("TILEMAP, 32 X 20 TILES", 0, WINDOWS, 0x4a_78_c8),
        ("WINDOWS", WINDOWS, l.messages, 0xe0_a0_30),
        ("MESSAGES, 3", l.messages, l.sprites, 0xe0_50_50),
        ("SPRITES, 64", l.sprites, after_sprites, 0x50_b0_70),
        ("FREE SPRITES LIST", after_sprites, l.text_tile_state, 0xa0_e0_a0),
        ("TEXT TILE STATE", l.text_tile_state, l.text_tile_state + 0x100, 0x90_70_d0),
        ("CURSOR, MODE, STYLE", l.text_tile_state + 0x100, l.text_buffer, 0xc0_c0_c0),
        ("TEXT BUFFER, THE RING", l.text_buffer, ring_end, 0x40_c8_d8),
        ("COUNT, START, GLYPHS", ring_end, l.argument_values, 0xd8_d8_70),
        ("ARGUMENT VALUES, KINDS", l.argument_values, l.portrait_pictures, 0xe0_80_c0),
        ("PORTRAITS, VOICE, FLAGS", l.portrait_pictures, l.size, 0xb0_90_70),
    ]
}

/// The byte map of a block: 64 bytes a row, `cell` pixels a byte, each byte
/// in its field's colour, dark while it is 0.
fn byte_map(block: &[u8], l: &Layout, cell: usize) -> Image {
    const ROW: usize = 64;
    let rows = (l.size as usize).div_ceil(ROW);
    let mut image = canvas(ROW * cell, rows * cell, BACK);
    let fields = regions(l);
    for (i, &b) in block.iter().enumerate().take(l.size as usize) {
        let colour = fields.iter().find(|r| (r.1..r.2).contains(&(i as u32))).map_or(DIM, |r| r.3);
        let shade = if b == 0 { mix(BACK, colour, 0.28) } else { colour };
        let (x, y) = ((i % ROW) * cell, (i / ROW) * cell);
        for dy in 0..cell - 1 {
            for dx in 0..cell - 1 {
                image.pixels[(y + dy) * image.width + x + dx] = shade;
            }
        }
    }
    image
}

impl<'a> Guide<'a> {
    /// Talking to Garet's sister in Vale, every frame with the window work
    /// block's bytes.
    fn talk_frames(&self, size: u32) -> Result<Vec<TalkFrame>, String> {
        let (mut entered, _) = self.stand_for(VALE, Trigger::Talk(GARETS_SISTER), GARETS_SISTER)?;
        let field = &mut entered.field;
        let slot = self.sym("gWindowWork")?;
        let mut frames = Vec::new();
        for &(keys, n) in &TALK {
            field.hold(keys);
            for _ in 0..n {
                field.frames(1)?;
                let work = field.read32(slot);
                let block = if work == 0 { vec![0; size as usize] } else { (0..size).map(|i| field.read8(work + i)).collect() };
                let oam = (0..512).map(|i| field.read16(OAM + 2 * i)).collect();
                frames.push(TalkFrame { screen: field.picture(), block, oam });
            }
        }
        Ok(frames)
    }

    /// The window work block of the international and Japanese editions side
    /// by side as Garet's sister talks in each, a byte a cell, as
    /// `work-block.gif`. Each block's size is read where the game allocates
    /// it (HeapSlot_Alloc for HEAP_SLOT_WINDOW) and must be
    /// tbs_edition.h's WINDOW_WORK_SIZE.
    pub fn window_work_block(&mut self) -> Result<(), String> {
        let root = crate::compile::root();
        let build = root.join("out/tbs-ja");
        let ja_map = std::fs::read(build.join("tbs-ja.map")).map_err(|e| format!("cannot read the Japanese map: {e} (make tbs-ja)"))?;
        let ja_map = crate::ld_map::Map::parse(&String::from_utf8_lossy(&ja_map));
        let ja_rom = std::fs::read(build.join("tbs-ja.gba")).map_err(|e| format!("cannot read the Japanese ROM: {e} (make tbs-ja)"))?;
        let ja_symbols = Symbols::find(&ja_map)?;
        let ja = Guide { rom: &ja_rom, symbols: &ja_symbols, flags: self.flags, out: self.out, map: &ja_map, written: Vec::new(), casts: Vec::new() };
        let runs = [(&ja, JAPANESE), (&*self, INTERNATIONAL)];
        let mut recorded = Vec::new();
        for (guide, layout) in runs {
            // The heap is a stack: the next block taken after the window work
            // starts where it ends.
            let size = {
                let (entered, _) = guide.stand_for(VALE, Trigger::Talk(GARETS_SISTER), GARETS_SISTER)?;
                let slots = guide.sym("gHeapSlots")?;
                let all: Vec<u32> = (0..64).map(|i| entered.field.read32(slots + 4 * i)).collect();
                let work = all[HEAP_SLOT_WINDOW as usize];
                let next = all.iter().copied().filter(|&a| a > work && a >> 24 == work >> 24).min().ok_or("no block after the window work")?;
                next - work
            };
            eprintln!("work-block: {}: the window work block is {size:#x} bytes to the next block", layout.name);
            if size != layout.size {
                return Err(format!("{}'s window work block is {size:#x} bytes, not {:#x}", layout.name, layout.size));
            }
            recorded.push(guide.talk_frames(size)?);
        }
        const CELL: usize = 4;
        const COLUMN: usize = 64 * CELL + 24;
        let legend_x = 2 * COLUMN + 16;
        let (w, h) = (legend_x + 300, 40 + HEIGHT + 30 + 76 * CELL + 16);
        let n = recorded.iter().map(Vec::len).max().unwrap_or(0);
        let mut out = Vec::new();
        for i in (0..n).step_by(2) {
            let mut image = canvas(w, h, BACK);
            for (k, (frames, (_, layout))) in recorded.iter().zip(runs.iter()).enumerate() {
                let f = &frames[i.min(frames.len() - 1)];
                let x = 12 + k * COLUMN;
                text(&mut image, x as i32 - 2, 6, &format!("{}: {} BYTES", layout.name, layout.size), HEAD, None, 2);
                text(&mut image, x as i32 - 2, 22, &format!("{} WINDOWS, A RING OF {} CODES", layout.window_count, layout.codes), DIM, None, 2);
                paste(&mut image, &screen(&f.screen), x + 8, 40, 1);
                paste(&mut image, &byte_map(&f.block, layout, CELL), x, 40 + HEIGHT + 30, 1);
                text(&mut image, x as i32 - 2, (40 + HEIGHT + 12) as i32, "64 BYTES A ROW", DIM, None, 2);
            }
            let mut y = 40;
            text(&mut image, legend_x as i32, 6, "STRUCT WINDOWWORK", HEAD, None, 2);
            text(&mut image, legend_x as i32, 22, "BRIGHT WHERE A BYTE IS NOT 0", DIM, None, 2);
            for (name, _, _, colour) in regions(&INTERNATIONAL) {
                for dy in 0..10 {
                    for dx in 0..10 {
                        image.pixels[(y + dy + 2) * w + legend_x + 4 + dx] = colour;
                    }
                }
                text(&mut image, legend_x as i32 + 18, y as i32, name, TEXT, None, 2);
                y += 20;
            }
            out.push(image.pixels);
        }
        let written = super::animation::write(&self.out.join("work-block.gif"), &out, w)?;
        self.written.push(("work-block.gif".to_string(), written.bytes, format!("the window work block in both layouts as a message is shown; {} frames", written.frames)));
        Ok(())
    }

    /// Garet's sister's two pages in Vale beside the decoded-text ring, as
    /// `text-ring.gif`: each of the 512 codes of work->textBuffer a cell, a
    /// character as its letter and a control code by its name; the codes
    /// before the first message's next one lit, the message's start, its
    /// next code and where the next decoding goes marked.
    pub fn window_text_ring(&mut self) -> Result<(), String> {
        let l = INTERNATIONAL;
        let frames = self.talk_frames(l.size)?;
        const CELL_W: usize = 15;
        const CELL_H: usize = 18;
        const COLUMNS: usize = 32;
        let grid_x = 2 * WIDTH + 16;
        let (w, h) = (grid_x + COLUMNS * CELL_W + 12, 2 * HEIGHT + 110);
        let half = |b: &[u8], at: u32| u16::from_le_bytes([b[at as usize], b[at as usize + 1]]);
        let mut out = Vec::new();
        let mut last_state = None;
        for (i, f) in frames.iter().enumerate() {
            let b = &f.block;
            let count = half(b, l.count);
            let start = half(b, l.count + 2);
            let message_window = u32::from_le_bytes([b[l.messages as usize], b[l.messages as usize + 1], b[l.messages as usize + 2], b[l.messages as usize + 3]]);
            let next = (message_window != 0).then(|| half(b, l.messages + MESSAGE_TEXT_INDEX));
            let state = (count, start, next);
            if last_state != Some(state) {
                eprintln!("text-ring: frame {i}: count {count}, start {start}, message 0 {next:?}");
                last_state = Some(state);
            }
            if i % 2 == 1 {
                continue;
            }
            let mut image = canvas(w, h, BACK);
            paste(&mut image, &screen(&f.screen), 0, 0, 2);
            text(&mut image, grid_x as i32, 4, "WORK->ENTRIES: 512 CODES, 16 BITS EACH", HEAD, None, 2);
            let top = 24;
            for k in 0..l.codes {
                let code = half(b, l.text_buffer + 2 * k);
                let (cx, cy) = (grid_x + (k as usize % COLUMNS) * CELL_W, top + (k as usize / COLUMNS) * CELL_H);
                let typed = next.is_some_and(|n| k >= u32::from(start) && k < u32::from(n));
                let used = k < u32::from(count);
                let (fill, ink, label) = match code {
                    0 if used => (0x50_30_30, 0xff_a0_a0, "E".to_string()),
                    1 => (0x30_40_60, 0xa0_c0_ff, "P".to_string()),
                    2 => (0x50_40_20, 0xff_d0_80, "W".to_string()),
                    3 => (0x30_48_38, 0x90_e0_a0, "N".to_string()),
                    32 => (0x30_32_3a, TEXT, String::new()),
                    33..=126 => (0x30_32_3a, TEXT, (code as u8 as char).to_string()),
                    0 => (0x26_28_2e, DIM, String::new()),
                    _ => (0x30_32_3a, DIM, "?".to_string()),
                };
                let fill = if typed { mix(fill, 0xff_ff_ff, 0.35) } else { fill };
                for dy in 0..CELL_H - 2 {
                    for dx in 0..CELL_W - 2 {
                        image.pixels[(cy + dy) * w + cx + dx] = fill;
                    }
                }
                text(&mut image, cx as i32, cy as i32 + 1, &label, if typed { 0x10_10_10 } else { ink }, None, 2);
                let marks = [(Some(start), 0x60_e0_80), (next, 0xff_ff_ff), (Some(count), MARK)];
                for (m, colour) in marks {
                    if m == Some(k as u16) && (count > 0) {
                        outline(&mut image, cx as i32 - 1, cy as i32 - 1, CELL_W as i32, CELL_H as i32, colour);
                        outline(&mut image, cx as i32, cy as i32, CELL_W as i32 - 2, CELL_H as i32 - 2, colour);
                    }
                }
            }
            let ly = (top + 16 * CELL_H + 8) as i32;
            text(&mut image, grid_x as i32, ly, &format!("COUNT {count}: WHERE THE NEXT MESSAGE GOES"), MARK, None, 2);
            text(&mut image, grid_x as i32, ly + 16, &format!("LASTMESSAGESTART {start}: THE LAST ONE DECODED"), 0x60_e0_80, None, 2);
            let line = match next {
                Some(n) => format!("MESSAGE 0'S NEXT CODE {n}; TYPED ONES LIT"),
                None => "NO MESSAGE TYPING".to_string(),
            };
            text(&mut image, grid_x as i32, ly + 32, &line, TEXT, None, 2);
            text(&mut image, grid_x as i32, ly + 52, "N NEWLINE  P PAGE  W WAIT KEY  E END", DIM, None, 2);
            out.push(image.pixels);
        }
        let written = super::animation::write(&self.out.join("text-ring.gif"), &out, w)?;
        self.written.push(("text-ring.gif".to_string(), written.bytes, format!("a message decoded into the ring and typed out; {} frames", written.frames)));
        Ok(())
    }

    /// Garet's sister's first page typed out in Vale with each window sprite
    /// outlined where its OAM attributes put it, as `render-outputs.gif`:
    /// the sprites of each open window's list, by kind, and the free list's
    /// count.
    pub fn window_render_outputs(&mut self) -> Result<(), String> {
        let l = INTERNATIONAL;
        let frames = self.talk_frames(l.size)?;
        let word = |b: &[u8], at: u32| u32::from_le_bytes([b[at as usize], b[at as usize + 1], b[at as usize + 2], b[at as usize + 3]]);
        let half = |b: &[u8], at: u32| u16::from_le_bytes([b[at as usize], b[at as usize + 1]]);
        let slot = self.sym("gWindowWork")?;
        // The block's address, the same all through the talk.
        let (entered, _) = self.stand_for(VALE, Trigger::Talk(GARETS_SISTER), GARETS_SISTER)?;
        let work = entered.field.read32(slot);
        drop(entered);
        const PANEL: usize = 400;
        let (w, h) = (2 * WIDTH + PANEL, 2 * HEIGHT + 48);
        let mut out = Vec::new();
        let mut shown = 0;
        let mut unmatched = 0;
        for (i, f) in frames.iter().enumerate() {
            let b = &f.block;
            // Each open window's sprites, walked from its list's head.
            let mut lists: Vec<(u32, Vec<u32>)> = Vec::new();
            for s in 0..l.window_count {
                let base = WINDOWS + WINDOW_SIZE * s;
                if half(b, base + WINDOW_FLAGS) == 0 {
                    continue;
                }
                let mut items = Vec::new();
                let mut p = word(b, base);
                while p != 0 && items.len() < 64 {
                    let index = (p.wrapping_sub(work + l.sprites)) / SPRITE_SIZE;
                    if index >= SPRITE_COUNT {
                        break;
                    }
                    items.push(index);
                    p = word(b, l.sprites + SPRITE_SIZE * index);
                }
                lists.push((s, items));
            }
            let total: usize = lists.iter().map(|(_, v)| v.len()).sum();
            // Each output's attributes are what the game copies into OAM: the
            // same three halfwords must be in this frame's OAM.
            for &o in lists.iter().flat_map(|(_, v)| v) {
                let at = l.sprites + SPRITE_SIZE * o + SPRITE_OAM;
                let attrs = [half(b, at), half(b, at + 2), half(b, at + 4)];
                // OAM is written in the next VBlank, so a change shows a frame
                // later.
                let in_oam = |oam: &[u16]| (0..128).any(|e| oam[4 * e..4 * e + 3] == attrs);
                if !in_oam(&f.oam) && !frames.get(i + 1).is_some_and(|n| in_oam(&n.oam)) {
                    unmatched += 1;
                }
            }
            if total != shown {
                eprintln!("render-outputs: frame {i}: {:?}", lists.iter().map(|(s, v)| (s, v.iter().map(|&o| b[(l.sprites + SPRITE_SIZE * o + SPRITE_KIND) as usize]).collect::<Vec<_>>())).collect::<Vec<_>>());
                shown = total;
            }
            if !(20..180).contains(&i) || i % 2 == 1 {
                continue;
            }
            let mut image = canvas(w, h, BACK);
            paste(&mut image, &screen(&f.screen), 0, 0, 2);
            let x0 = (2 * WIDTH + 12) as i32;
            text(&mut image, x0, 4, "EACH OPEN WINDOW'S OUTPUT LIST", HEAD, None, 2);
            let mut y = 22;
            for (s, items) in &lists {
                let width = half(b, WINDOWS + WINDOW_SIZE * s + WINDOW_WIDTH);
                let role = if items.iter().any(|&o| b[(l.sprites + SPRITE_SIZE * o + SPRITE_KIND) as usize] == 2) { "PORTRAIT" } else { "MESSAGE" };
                text(&mut image, x0, y, &format!("WINDOW SLOT {s}: {role}, {width} TILES WIDE"), TEXT, None, 2);
                y += 18;
                for &o in items {
                    let at = l.sprites + SPRITE_SIZE * o;
                    let kind = b[(at + SPRITE_KIND) as usize];
                    let motion = b[(at + SPRITE_MOTION) as usize];
                    let attr0 = half(b, at + SPRITE_OAM);
                    let attr1 = half(b, at + SPRITE_OAM + 2);
                    let (sw, sh) = sprite_size(attr0 >> 14, attr1 >> 14);
                    let (sx, sy) = (i32::from(attr1 & 0x1ff), i32::from(attr0 & 0xff));
                    let colour = KIND_COLOURS[usize::from(kind).min(2)];
                    outline(&mut image, 2 * sx, 2 * sy, 2 * sw, 2 * sh, colour);
                    outline(&mut image, 2 * sx + 1, 2 * sy + 1, 2 * sw - 2, 2 * sh - 2, colour);
                    let what = match (kind, motion) {
                        (0, 2) => "PAGE ARROW",
                        (0, _) => "TWO LETTERS",
                        (2, _) => "PORTRAIT",
                        _ => "SPRITE",
                    };
                    text(&mut image, x0 + 8, y, &format!("{what}, {sw}X{sh} PX AT {sx},{sy}"), colour, None, 2);
                    y += 16;
                }
                y += 6;
            }
            let (mut lx, ly) = (8i32, (2 * HEIGHT + 14) as i32);
            for (k, name) in ["GLYPH OUTPUT: TWO LETTERS OR THE ARROW", "SPRITE OUTPUT", "PORTRAIT OUTPUT"].iter().enumerate() {
                if k == 1 {
                    // No sprite output in this message.
                    continue;
                }
                for dy in 0..10 {
                    for dx in 0..10 {
                        image.pixels[(ly as usize + dy + 2) * w + lx as usize + dx] = KIND_COLOURS[k];
                    }
                }
                lx += 14 + text(&mut image, lx + 14, ly, name, TEXT, None, 2) + 24;
            }
            text(&mut image, 8, ly + 18, "EACH OUTLINE: WHERE THE OUTPUT'S OWN OAM ATTRIBUTES PUT IT", DIM, None, 2);
            out.push(image.pixels);
        }
        eprintln!("render-outputs: {unmatched} output-frames whose attributes are not in OAM");
        let written = super::animation::write(&self.out.join("render-outputs.gif"), &out, w)?;
        self.written.push(("render-outputs.gif".to_string(), written.bytes, format!("a message's window sprites outlined; {} frames", written.frames)));
        Ok(())
    }
}

/// enum WindowSpriteKind's colours: glyph, sprite, portrait.
const KIND_COLOURS: [u32; 3] = [0xff_e8_40, 0x40_e0_ff, 0xff_60_a0];

/// A sprite's width and height in pixels from its OAM shape and size.
fn sprite_size(shape: u16, size: u16) -> (i32, i32) {
    const SIZES: [[(i32, i32); 4]; 3] = [[(8, 8), (16, 16), (32, 32), (64, 64)], [(16, 8), (32, 8), (32, 16), (64, 32)], [(8, 16), (8, 32), (16, 32), (32, 64)]];
    SIZES.get(usize::from(shape)).map_or((8, 8), |row| row[usize::from(size & 3)])
}

/// The Vale Sanctum from Vale (SceneId_Sanctum, entrance 1) and its Great
/// Healer (ACTOR_HEALER), who opens the services when talked to from the
/// front.
const VALE_SANCTUM: (u16, u16) = (9, 1);
const SANCTUM_HEALER: u16 = 8;
const FACING_NORTH: u32 = 0xc000;
/// struct Unit's fields read and set here (battle_unit.h).
const UNIT_HP_GAUGE: u32 = 0x14;
const UNIT_MAX_HP: u32 = 0x34;
const UNIT_HP: u32 = 0x38;
const UNIT_INVENTORY: u32 = 0xd8;
const UNIT_RESTRAINT: u32 = 0x130;
const UNIT_POISON: u32 = 0x131;
const UNIT_HAUNT: u32 = 0x140;
const INVENTORY_SLOTS: u32 = 15;
/// ShopWork.partyMemberIds and its count (shop.h).
const SHOP_MEMBER_IDS: u32 = 0x36e;
const SHOP_MEMBER_COUNT: u32 = 0x3a7;
const SHOP_ACTION: u32 = 0x3aa;
const KEY_B: u32 = 0x2;
const KEY_RIGHT: u32 = 0x10;
const KEY_LEFT: u32 = 0x20;
const KEY_DOWN: u32 = 0x80;

impl<'a> Guide<'a> {
    /// The Vale Sanctum's Revive with the four in the party: Garet and Mia
    /// down, Ivan poisoned, Isaac well, as `sanctum.gif`. The screen beside
    /// each member of ShopWork.partyMemberIds with the unit fields
    /// Sanctum_NeedsService reads and its answer for the service in
    /// shop->action, and the calls the game makes to it. The hand
    /// starts on Garet, the first who needs the service; A on Ivan only
    /// buzzes; Garet is revived; the choice is offered again with Mia the
    /// only one left, and B leaves.
    pub fn sanctum_members(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.stand_where(VALE_SANCTUM, Trigger::Talk(SANCTUM_HEALER), SANCTUM_HEALER, |s| s.facing == FACING_NORTH)?;
        let field = &mut entered.field;
        field.set_party(&[0, 1, 2, 3]);
        field.set_coins(2000);
        let mut units = Vec::new();
        for (owner, down, poison) in [(0u8, false, false), (1, true, false), (2, false, true), (3, true, false)] {
            let unit = field.unit(owner)?;
            if down {
                field.write16(unit + UNIT_HP, 0);
                field.write16(unit + UNIT_HP_GAUGE, 0);
            }
            if poison {
                field.write8(unit + UNIT_POISON, 1);
            }
            units.push((owner, unit));
        }
        let menu = self.sym("gMenuWork")?;
        let can_serve = self.sym("Sanctum_NeedsService")?;
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(u32, u32, u32)>::new()));
        let log = calls.clone();
        field.probe_pcs = vec![can_serve];
        field.probe = Some(Box::new(move |gba, pc, frame| {
            if pc == can_serve {
                log.borrow_mut().push((frame, gba.register(0), gba.register(1)));
            }
        }));
        // Talk; the welcome; Revive; its line; "Who shall I revive?"; then
        // the hand right to Ivan and A, on to Mia, back to Garet and A; the
        // donation, Yes, the healing, and the choice again, left with B.
        let mut path = vec![(0, 20), (KEY_A, 3), (0, 100)];
        for _ in 0..2 {
            path.extend([(KEY_A, 3), (0, 60)]);
        }
        path.extend([(0, 60), (KEY_RIGHT, 3), (0, 45), (KEY_A, 3), (0, 45), (KEY_RIGHT, 3), (0, 45), (KEY_LEFT, 3), (0, 30), (KEY_LEFT, 3), (0, 45), (KEY_A, 3), (0, 60)]);
        for _ in 0..3 {
            path.extend([(KEY_A, 3), (0, 60)]);
        }
        path.extend([(0, 200), (KEY_A, 3), (0, 90), (KEY_B, 3), (0, 90)]);
        const PANEL: usize = 400;
        let (w, h) = (2 * WIDTH + PANEL, 2 * HEIGHT);
        let mut out = Vec::new();
        let mut n = 0usize;
        let mut seen_calls = 0;
        for &(keys, k) in &path {
            field.hold(keys);
            for _ in 0..k {
                field.frames(1)?;
                n += 1;
                let shown = calls.borrow().len();
                if shown != seen_calls {
                    let c = calls.borrow();
                    eprintln!("sanctum: frame {n}: Sanctum_NeedsService calls {:?}", c[seen_calls..].iter().map(|&(_, u, s)| (u, s)).collect::<Vec<_>>());
                    seen_calls = shown;
                }
                if !n.is_multiple_of(3) {
                    continue;
                }
                let work = field.read32(menu);
                let mut image = canvas(w, h, BACK);
                paste(&mut image, &screen(&field.picture()), 0, 0, 2);
                let x0 = (2 * WIDTH + 12) as i32;
                if work == 0 {
                    text(&mut image, x0, 6, "NO SHOP WORK BLOCK", DIM, None, 2);
                    out.push(image.pixels);
                    continue;
                }
                let action = field.read8(work + SHOP_ACTION) as i8;
                let service = ["SANCTUM_SERVICE_REVIVE", "SANCTUM_SERVICE_CURE_POISON", "SANCTUM_SERVICE_REPEL_EVIL", "SANCTUM_SERVICE_REMOVE_CURSE"].get(action as usize).copied().unwrap_or("NO SERVICE");
                let need = ["NEEDED BY A MEMBER WHO IS DOWN", "NEEDED BY A POISONED MEMBER", "NEEDED BY A HAUNTED MEMBER", "NEEDED BY A CURSED MEMBER"].get(action as usize).copied().unwrap_or("");
                text(&mut image, x0, 6, "THE SERVICE CHOSEN:", HEAD, None, 2);
                text(&mut image, x0, 22, service, TEXT, None, 2);
                text(&mut image, x0, 38, need, DIM, None, 2);
                text(&mut image, x0, 62, "EACH MEMBER: SANCTUM_NEEDSSERVICE", HEAD, None, 2);
                let count = field.read8(work + SHOP_MEMBER_COUNT);
                let mut y = 82;
                for m in 0..u32::from(count) {
                    let id = field.read16(work + SHOP_MEMBER_IDS + 2 * m) as u8;
                    let Some(&(_, unit)) = units.iter().find(|(o, _)| *o == id) else { continue };
                    let hp = field.read16(unit + UNIT_HP) as i16;
                    let poison = field.read8(unit + UNIT_POISON);
                    let haunt = field.read8(unit + UNIT_HAUNT);
                    let restraint = field.read8(unit + UNIT_RESTRAINT);
                    // Sanctum_NeedsService's test, on the fields it reads.
                    let yes = match action {
                        0 => hp <= 0,
                        1 => poison != 0,
                        2 => haunt != 0,
                        3 => restraint != 0,
                        _ => false,
                    };
                    let colour = if yes { MARK } else { DIM };
                    let mut state = vec![if hp <= 0 { "DOWN".to_string() } else { format!("HP {hp} OF {}", field.read16(unit + UNIT_MAX_HP)) }];
                    if poison != 0 {
                        state.push("POISONED".to_string());
                    }
                    if haunt != 0 {
                        state.push("HAUNTED".to_string());
                    }
                    if restraint != 0 {
                        state.push("CURSED".to_string());
                    }
                    text(&mut image, x0, y, &format!("{}: {}", super::plan::character_name(id).to_uppercase(), if yes { "CAN BE CHOSEN" } else { "NOT NEEDED" }), colour, None, 2);
                    text(&mut image, x0 + 16, y + 16, &state.join(", "), TEXT, None, 2);
                    y += 40;
                }
                out.push(image.pixels);
            }
        }
        let written = super::animation::write(&self.out.join("sanctum.gif"), &out, w)?;
        self.written.push(("sanctum.gif".to_string(), written.bytes, format!("the Vale Sanctum's Revive offered to the members who need it; {} frames", written.frames)));
        Ok(())
    }
}

/// Items to fill the empty bag slots with, one of each, and the item given.
const FILLERS: [&str; 26] = [
    "Herb", "Vial", "Potion", "Psy Crystal", "Antidote", "Elixir", "Mist Potion", "Power Bread", "Cookie", "Apple", "Hard Nut", "Mint", "Lucky Pepper", "Smoke Bomb", "Sleep Bomb", "Bramble Seed", "Crystal Powder", "Oil Drop", "Weasel's Claw", "Bone", "Corn", "Sacred Feather", "Lucky Medal", "Mythril Bag", "Small Jewel", "Dragon Skin",
];
const GIVEN: &str = "Nut";

impl<'a> Guide<'a> {
    /// The item picker: Isaac and Garet in Vale with all 30 bag slots full,
    /// given a Nut by PartyInventory_GiveItem, the scene command that gives
    /// an item, called from the field as a scene would, as
    /// `item-picker.gif`. The screen beside both bags read from their
    /// units each frame; the picker opens, the hand moves to Garet and back,
    /// an item of Isaac's is chosen and dropped, and the Nut takes its slot.
    pub fn item_picker(&mut self) -> Result<(), String> {
        let names = super::guide_apart::item_names()?;
        let id_of = |n: &str| names.iter().position(|x| x == n).map(|i| i as u16);
        let pick = self.sym("ItemPicker_Run")?;
        let give = self.sym("PartyInventory_GiveItem")?;
        let mut entered = self.enter_with(VALE, &[])?;
        let field = &mut entered.field;
        field.set_party(&[0, 1]);
        let mut fillers = FILLERS.iter().filter_map(|n| id_of(n));
        let mut units = Vec::new();
        for owner in [0u8, 1] {
            let unit = field.unit(owner)?;
            for s in 0..INVENTORY_SLOTS {
                if field.read16(unit + UNIT_INVENTORY + 2 * s) == 0 {
                    let item = fillers.next().ok_or("too few fillers")?;
                    field.write16(unit + UNIT_INVENTORY + 2 * s, item);
                }
            }
            units.push((owner, unit));
        }
        let given = id_of(GIVEN).ok_or("no Nut")?;
        field.start_call(give, [u32::from(given), 0]);
        let opened = std::rc::Rc::new(std::cell::Cell::new(None));
        let seen = opened.clone();
        field.probe_pcs = vec![pick];
        field.probe = Some(Box::new(move |_gba, pc, frame| {
            if pc == pick && seen.get().is_none() {
                seen.set(Some(frame));
            }
        }));
        let item_name = |entry: u16| -> String {
            if entry == 0 {
                return "-".to_string();
            }
            let name = names.get(usize::from(entry & 0x1ff)).cloned().unwrap_or_default().to_uppercase();
            let count = (entry >> 11) + 1;
            let equipped = if entry & 0x200 != 0 { ", EQUIPPED" } else { "" };
            if count > 1 { format!("{count} {name}{equipped}") } else { format!("{name}{equipped}") }
        };
        let path = PICKER_PATH;
        const PANEL: usize = 500;
        let (w, h) = (2 * WIDTH + PANEL, 2 * HEIGHT + 48);
        let mut out = Vec::new();
        let mut n = 0;
        let mut before: Option<Vec<Vec<u16>>> = None;
        for &(keys, k) in path {
            field.hold(keys);
            for _ in 0..k {
                field.frames(1)?;
                n += 1;
                let bags: Vec<Vec<u16>> = units.iter().map(|&(_, u)| (0..INVENTORY_SLOTS).map(|s| field.read16(u + UNIT_INVENTORY + 2 * s)).collect()).collect();
                if before.is_none() {
                    before = Some(bags.clone());
                }
                if n % 2 != 0 {
                    continue;
                }
                let mut image = canvas(w, h, BACK);
                paste(&mut image, &screen(&field.picture()), 0, 0, 2);
                let x0 = (2 * WIDTH + 10) as i32;
                let state = match opened.get() {
                    None => format!("PARTYINVENTORY_GIVEITEM({})", item_name(given)),
                    Some(_) => "ITEMPICKER_RUN HAS RUN".to_string(),
                };
                text(&mut image, 8, (2 * HEIGHT + 8) as i32, &state, HEAD, None, 2);
                text(&mut image, 8, (2 * HEIGHT + 26) as i32, "YELLOW: A SLOT WHOSE ITEM HAS CHANGED", MARK, None, 2);
                for (k, (owner, _)) in units.iter().enumerate() {
                    let (cx, y0) = (x0 + k as i32 * 244, 4);
                    text(&mut image, cx, y0, &format!("{}'S BAG", super::plan::character_name(*owner).to_uppercase()), TEXT, None, 2);
                    for (s, &entry) in bags[k].iter().enumerate() {
                        let changed = before.as_ref().is_some_and(|b| b[k][s] != entry);
                        let colour = if changed { MARK } else { DIM };
                        text(&mut image, cx, y0 + 20 + s as i32 * 18, &item_name(entry), colour, None, 2);
                    }
                }
                out.push(image.pixels);
            }
        }
        eprintln!("item-picker: ItemPicker_Run at frame {:?}", opened.get());
        let written = super::animation::write(&self.out.join("item-picker.gif"), &out, w)?;
        self.written.push(("item-picker.gif".to_string(), written.bytes, format!("a Nut given with every bag full; {} frames", written.frames)));
        Ok(())
    }
}

/// The keys from the arrival: through the gift's lines to the picker, the
/// hand to Garet and back, Isaac's bag, an item, the drop and the lines.
const PICKER_PATH: &[(u32, u32)] = &[
    (0, 60), (KEY_A, 3), (0, 60), (KEY_A, 3), (0, 60), (KEY_A, 3), (0, 60), (KEY_A, 3), (0, 60),
    (KEY_RIGHT, 3), (0, 50), (KEY_LEFT, 3), (0, 50), (KEY_A, 3), (0, 50), (KEY_DOWN, 3), (0, 40), (KEY_A, 3), (0, 120),
];

/// Bilibin's item shop (SceneId_CottageRoomsC, entrance
/// COTTAGE_ROOMS_C_ITEM_SHOP) and its keeper.
const BILIBIN_ITEM_SHOP: (u16, u16) = (31, 5);
const BILIBIN_ITEM_SHOPKEEPER: u16 = 18;
/// sizeof(struct ShopWork), the menu work's size.
const MENU_WORK_SIZE: u32 = 0xa70;

/// struct ShopWork's named fields (shop.h).
const SHOP_FIELDS: [(&str, u32, u32, u32); 7] = [
    ("MONEYWINDOW, ITEMWINDOW", 0x00c, 0x024, 0xe0_a0_30),
    ("PARTY ICONS, X, Y, SCALE", 0x114, 0x174, 0x50_b0_70),
    ("STOCKITEMIDS", 0x26c, 0x36e, 0x40_c8_d8),
    ("PARTYMEMBERIDS", 0x36e, 0x380, 0xe0_80_c0),
    ("CURSOR, ITS ICONS", 0x380, 0x3a6, 0xd8_d8_70),
    ("COUNTS, MODE, ACTION", 0x3a6, 0x3b0, 0xe0_50_50),
    ("EFFECTS, 24 SLOTS", 0x3b0, 0xa70, 0x90_70_d0),
];
/// struct ItemMenuWork's named fields (item_menu.h), the Item
/// screen's reading of the same block.
const ITEM_SCREEN_FIELDS: [(&str, u32, u32, u32); 6] = [
    ("WINDOWS AND PANE ICONS", 0x010, 0x048, 0xe0_a0_30),
    ("ENTRY AND CATEGORY ICONS", 0x048, 0x0e8, 0x4a_78_c8),
    ("OWNER FIGURES, X, Y, SCALE", 0x114, 0x174, 0x50_b0_70),
    ("SELECTION, CURSOR", 0x174, 0x180, 0xd8_d8_70),
    ("ITEMS LISTED, OWNERIDS", 0x1c8, 0x218, 0x40_c8_d8),
    ("COUNTS, MODES, RESOURCES", 0x218, 0x396, 0xe0_50_50),
];

/// A row of `menu-slot.png`: its title, the block.s address, the screen, the
/// bytes and the fields they are read as.
type MenuSlotRow<'b> = (&'b str, u32, &'b [u16], &'b [u8], &'b [(&'b str, u32, u32, u32)]);

/// The menu work's bytes, 64 a row, each in its field's colour (grey where
/// no field is named), dark while 0.
fn menu_map(block: &[u8], fields: &[(&str, u32, u32, u32)], cell: usize) -> Image {
    const ROW: usize = 64;
    let mut image = canvas(ROW * cell, block.len().div_ceil(ROW) * cell, BACK);
    for (i, &b) in block.iter().enumerate() {
        let colour = fields.iter().find(|f| (f.1..f.2).contains(&(i as u32))).map_or(0x90_94_9c, |f| f.3);
        let shade = if b == 0 { mix(BACK, colour, 0.25) } else { colour };
        let (x, y) = ((i % ROW) * cell, (i / ROW) * cell);
        for dy in 0..cell - 1 {
            for dx in 0..cell - 1 {
                image.pixels[(y + dy) * image.width + x + dx] = shade;
            }
        }
    }
    image
}

impl<'a> Guide<'a> {
    /// The menu heap slot (gMenuWork, HEAP_SLOT_MENU) in Bilibin's item
    /// shop: at the counter's Buy list, read as struct ShopWork, and,
    /// in a run of its own, with the field menu's Item screen open there,
    /// read as struct ItemMenuWork, as `menu-slot.png`. Each byte of
    /// the 0xa70 bytes is a cell; the block's address is read from the
    /// slot each time.
    pub fn shop_menu_slot(&mut self) -> Result<(), String> {
        let slot = self.sym("gMenuWork")?;
        // The counter: talk, then Buy.
        let (mut entered, _) = self.stand_where(BILIBIN_ITEM_SHOP, Trigger::Talk(BILIBIN_ITEM_SHOPKEEPER), BILIBIN_ITEM_SHOPKEEPER, |s| s.facing == FACING_NORTH)?;
        entered.field.set_coins(500);
        let shop_frames = Guide::press_path(&mut entered.field, &[(0, 20), (KEY_A, 3), (0, 100), (KEY_A, 3), (0, 60)])?;
        let read = |field: &super::field::Field| -> (u32, Vec<u8>) {
            let work = field.read32(slot);
            (work, (0..MENU_WORK_SIZE).map(|i| if work == 0 { 0 } else { field.read8(work + i) }).collect())
        };
        let (shop_at, shop_block) = read(&entered.field);
        let stock = (0..24).map(|i| entered.field.read16(shop_at + 0x26c + 2 * i)).take_while(|&id| id != 0).count();
        eprintln!("menu-slot: the shop's work at {shop_at:#x}, {stock} items in stockItemIds");
        // The Item screen: the field menu, along to its third icon, A.
        let mut entered = self.enter_with(BILIBIN_ITEM_SHOP, &[])?;
        let menu_frames = Guide::press_path(&mut entered.field, &[(0, 20), (KEY_A, 4), (0, 50), (KEY_RIGHT, 4), (0, 30), (KEY_RIGHT, 4), (0, 30), (KEY_A, 4), (0, 90)])?;
        let (menu_at, menu_block) = read(&entered.field);
        eprintln!("menu-slot: the Item screen's work at {menu_at:#x}");
        if shop_at == 0 || menu_at == 0 {
            return Err("the menu heap slot was empty".to_string());
        }
        const CELL: usize = 5;
        let map_w = 64 * CELL;
        if shop_at != menu_at {
            return Err(format!("the counter's block at {shop_at:#x} and the Item screen's at {menu_at:#x} differ"));
        }
        const ROW_H: usize = 24 + 2 * HEIGHT + 12 + 4 * 20 + 24;
        let (w, h) = (8 + 2 * WIDTH + 16 + map_w + 8, 30 + 2 * ROW_H);
        let mut image = canvas(w, h, BACK);
        text(&mut image, 8, 6, "GMENUWORK HOLDS THE SAME BLOCK BOTH TIMES", HEAD, None, 2);
        let rows: [MenuSlotRow; 2] = [
            ("AT THE COUNTER, READ AS STRUCT SHOPWORK", shop_at, shop_frames.last().ok_or("no frames")?, &shop_block, &SHOP_FIELDS),
            ("THE ITEM SCREEN, READ AS STRUCT ITEMMENUWORK", menu_at, menu_frames.last().ok_or("no frames")?, &menu_block, &ITEM_SCREEN_FIELDS),
        ];
        for (r, (title, _, picture, block, fields)) in rows.iter().enumerate() {
            let y0 = 30 + r * ROW_H;
            text(&mut image, 8, y0 as i32, title, TEXT, None, 2);
            paste(&mut image, &screen(picture), 8, y0 + 20, 2);
            let mx = 8 + 2 * WIDTH + 16;
            paste(&mut image, &menu_map(block, fields, CELL), mx, y0 + 20, 1);
            text(&mut image, mx as i32, (y0 + 20 + 42 * CELL + 6) as i32, "2,672 BYTES, ONE A CELL,", DIM, None, 2);
            text(&mut image, mx as i32, (y0 + 20 + 42 * CELL + 22) as i32, "64 A ROW; BRIGHT IF NOT 0", DIM, None, 2);
            let top = (y0 + 20 + 2 * HEIGHT + 10) as i32;
            let mut entries: Vec<(String, u32)> = fields.iter().map(|(name, from, to, colour)| (format!("{name}: BYTES {from} TO {}", to - 1), *colour)).collect();
            entries.push(("NOT NAMED IN THE STRUCT".to_string(), 0x90_94_9c));
            for (k, (label, colour)) in entries.iter().enumerate() {
                let (lx, ly) = (8 + (k % 2) as i32 * 410, top + (k / 2) as i32 * 20);
                for dy in 0..10 {
                    for dx in 0..10 {
                        image.pixels[(ly as usize + dy + 2) * w + lx as usize + dx] = *colour;
                    }
                }
                text(&mut image, lx + 14, ly, label, TEXT, None, 2);
            }
        }
        self.guide_png("menu-slot.png", &image, "the menu heap slot as a counter and as the Item screen use it")
    }
}

/// Bilibin from the world map's road (SceneId_Bilibin, entrance 1): a new
/// place group, so FLAG_SHOW_AREA_NAME is set on arrival.
const BILIBIN: (u16, u16) = (30, 1);
/// struct TimedNoticeWork (window/shortcuts.c): the notice's window and
/// countdown in the event work.
const NOTICE_WINDOW: u32 = 0x230;
const NOTICE_COUNTDOWN: u32 = 0x234;
/// struct Window's place and size, in tiles.
const WINDOW_HEIGHT: u32 = 0x0a;
const WINDOW_X: u32 = 0x0c;
const WINDOW_Y: u32 = 0x0e;
/// AREA_NAME_WINDOW_FRAMES.
const NOTICE_FRAMES: u32 = 90;

impl<'a> Guide<'a> {
    /// Arriving in Bilibin: from the frame AreaNameWindow_Show runs, the
    /// screen with the notice's window outlined where its Window puts it,
    /// beside its countdown read from the event work each frame, until a
    /// second after the window has closed, as `area-name.gif`.
    pub fn window_area_name(&mut self) -> Result<(), String> {
        let create = self.sym("AreaNameWindow_Show|MapNamePopup_Show")?;
        let event_work = self.sym("gFieldWork")?;
        let mut field = self.arrive(BILIBIN, &[], &["FLAG_VALE_STORM", "FLAG_CHILDHOOD"])?;
        field.show_notice = true;
        let created = std::rc::Rc::new(std::cell::Cell::new(None));
        let seen = created.clone();
        field.probe_pcs = vec![create];
        field.probe = Some(Box::new(move |gba, pc, frame| {
            if pc == create && seen.get().is_none() {
                eprintln!("area-name: AreaNameWindow_Show at frame {frame}, gFieldWork {:#x}", gba.read32(event_work));
                seen.set(Some(frame));
            }
        }));
        const PANEL: usize = 300;
        let (w, h) = (2 * WIDTH + PANEL, 2 * HEIGHT);
        let mut out = Vec::new();
        let mut before: Vec<Vec<u32>> = Vec::new();
        let mut closed_for = 0;
        let mut last_count = None;
        for i in 0..1200 {
            field.frames(1)?;
            let work = field.read32(event_work);
            let window = if work == 0 { 0 } else { field.read32(work + NOTICE_WINDOW) };
            let count = if work == 0 { 0 } else { u32::from(field.read16(work + NOTICE_COUNTDOWN)) };
            let open = window != 0 && field.read16(window + WINDOW_FLAGS) != 0;
            let mut image = canvas(w, h, BACK);
            paste(&mut image, &screen(&field.picture()), 0, 0, 2);
            let x0 = (2 * WIDTH + 12) as i32;
            text(&mut image, x0, 6, "BILIBIN, ON ARRIVAL", HEAD, None, 2);
            if created.get().is_none() {
                text(&mut image, x0, 30, "NO NOTICE YET", DIM, None, 2);
                before.push(image.pixels);
                continue;
            }
            if last_count != Some(count) && (count % 10 == 0 || count == NOTICE_FRAMES) {
                eprintln!("area-name: frame {i}: countdown {count}, window {window:#x} open {open}");
            }
            last_count = Some(count);
            if open {
                let (wx, wy) = (i32::from(field.read16(window + WINDOW_X)), i32::from(field.read16(window + WINDOW_Y)));
                let (ww, wh) = (i32::from(field.read16(window + WINDOW_WIDTH)), i32::from(field.read16(window + WINDOW_HEIGHT)));
                outline(&mut image, 16 * wx, 16 * wy, 16 * ww, 16 * wh, MARK);
                outline(&mut image, 16 * wx + 1, 16 * wy + 1, 16 * ww - 2, 16 * wh - 2, MARK);
                text(&mut image, x0, 30, &format!("WINDOW: {ww} X {wh} TILES AT {wx},{wy}"), MARK, None, 2);
            } else {
                text(&mut image, x0, 30, "WINDOW CLOSED", DIM, None, 2);
            }
            text(&mut image, x0, 56, "THE EVENT WORK'S COUNTDOWN, FROM 90", TEXT, None, 2);
            text(&mut image, x0, 74, &format!("{count} FRAMES LEFT"), TEXT, None, 3);
            let bar = (count.min(NOTICE_FRAMES) as usize * (PANEL - 30)) / NOTICE_FRAMES as usize;
            for dy in 0..12 {
                for dx in 0..PANEL - 30 {
                    image.pixels[(104 + dy) * w + x0 as usize + dx] = if dx < bar { MARK } else { 0x40_42_48 };
                }
            }
            text(&mut image, x0, 130, "AREANAMEWINDOW_UPDATE TAKES ONE", DIM, None, 2);
            text(&mut image, x0, 146, "A FRAME AND CLOSES IT AT 0", DIM, None, 2);
            if out.is_empty() {
                // Half a second of the arrival before the notice.
                let keep = before.len().saturating_sub(30);
                out.extend(before.drain(keep..));
            }
            out.push(image.pixels);
            if !open {
                closed_for += 1;
                if closed_for > 40 {
                    break;
                }
            }
        }
        if created.get().is_none() {
            return Err("AreaNameWindow_Show never ran".to_string());
        }
        let written = super::animation::write(&self.out.join("area-name.gif"), &out, w)?;
        self.written.push(("area-name.gif".to_string(), written.bytes, format!("Bilibin's area name and its countdown; {} frames", written.frames)));
        Ok(())
    }
}
