//! Take-apart pictures for the folder guides and the place overlays: one
//! mechanism each, drawn beside the screen it makes from the game's own
//! memory (a split scanline, a palette fading, the cells an event rewrites).

use super::animation;
use super::field::{Field, Image};
use super::guide::Guide;
use super::guide_more::{canvas, outline, paste, screen};
use super::mgba::{HEIGHT, WIDTH};

const KEY_RIGHT: u32 = 0x10;
const KEY_LEFT: u32 = 0x20;

/// DISPCNT and its layer bits: 8 to 11 turn BG0 to BG3 on, 12 the sprites.
const DISPCNT: u32 = 0x0400_0000;
const LAYER_BITS: u16 = 0x1f00;
/// What a layer shown alone is drawn over where it is empty.
const EMPTY: u16 = 0x0c63;

/// The colours the marks are drawn in.
const RED: u32 = 0xff_30_30;
const WHITE: u32 = 0xff_ff_ff;

impl<'a> Guide<'a> {
    /// The address of `name` in the edition's linker map.
    pub(super) fn sym(&self, name: &str) -> Result<u32, String> {
        self.map.symbol(name).map(|(_, a)| a & !1).ok_or_else(|| format!("the map has no {name}"))
    }

    /// The address of a scene overlay's `name`, a static the main map leaves
    /// out, from the overlay ELF that also defines `anchor`.
    pub(super) fn overlay_sym(&self, anchor: &str, name: &str) -> Result<u32, String> {
        let dir = crate::compile::root().join("out/tbs-en/overlays");
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let file = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !file.ends_with(".elf") || file.ends_with(".symbols.elf") {
                continue;
            }
            let Ok(data) = std::fs::read(&path) else { continue };
            let Ok(elf) = crate::elf::Elf::parse(&data) else { continue };
            if !elf.symbols.iter().any(|s| s.name == anchor) {
                continue;
            }
            return elf.symbols.iter().find(|s| s.name == name).map(|s| s.value & !1).ok_or_else(|| format!("{file} has no {name}"));
        }
        Err(format!("no overlay defines {anchor}"))
    }
}

/// The moment `field` is at with only the layers in `bits` (DISPCNT bits 8
/// to 12) shown, over EMPTY; the field is put back as it was.
pub(super) fn alone(field: &mut Field, bits: u16) -> Result<Image, String> {
    let saved = field.save();
    let shown = field.read16(DISPCNT);
    field.io_write16(DISPCNT, (shown & !LAYER_BITS) | bits);
    field.set_backdrop(EMPTY);
    field.frames(1)?;
    let image = screen(&field.picture());
    field.clear_backdrop();
    field.resume(&saved);
    Ok(image)
}

/// A 3 by 5 font: each glyph's rows, three bits to a row, top first.
fn glyph(c: char) -> [u8; 5] {
    match c.to_ascii_uppercase() {
        '0' => [7, 5, 5, 5, 7],
        '1' => [2, 6, 2, 2, 7],
        '2' => [7, 1, 7, 4, 7],
        '3' => [7, 1, 3, 1, 7],
        '4' => [5, 5, 7, 1, 1],
        '5' => [7, 4, 7, 1, 7],
        '6' => [7, 4, 7, 5, 7],
        '7' => [7, 1, 1, 1, 1],
        '8' => [7, 5, 7, 5, 7],
        '9' => [7, 5, 7, 1, 7],
        'A' => [2, 5, 7, 5, 5],
        'B' => [6, 5, 6, 5, 6],
        'C' => [3, 4, 4, 4, 3],
        'D' => [6, 5, 5, 5, 6],
        'E' => [7, 4, 6, 4, 7],
        'F' => [7, 4, 6, 4, 4],
        'G' => [3, 4, 5, 5, 3],
        'H' => [5, 5, 7, 5, 5],
        'I' => [7, 2, 2, 2, 7],
        'J' => [1, 1, 1, 5, 2],
        'K' => [5, 5, 6, 5, 5],
        'L' => [4, 4, 4, 4, 7],
        'M' => [5, 7, 7, 5, 5],
        'N' => [6, 5, 5, 5, 5],
        'O' => [2, 5, 5, 5, 2],
        'P' => [6, 5, 6, 4, 4],
        'Q' => [2, 5, 5, 6, 3],
        'R' => [6, 5, 6, 5, 5],
        'S' => [3, 4, 2, 1, 6],
        'T' => [7, 2, 2, 2, 2],
        'U' => [5, 5, 5, 5, 7],
        'V' => [5, 5, 5, 5, 2],
        'W' => [5, 5, 7, 7, 5],
        'X' => [5, 5, 2, 5, 5],
        'Y' => [5, 5, 2, 2, 2],
        'Z' => [7, 1, 2, 4, 7],
        '-' => [0, 0, 7, 0, 0],
        '_' => [0, 0, 0, 0, 7],
        '+' => [0, 2, 7, 2, 0],
        '/' => [1, 1, 2, 4, 4],
        ':' => [0, 2, 0, 2, 0],
        '.' => [0, 0, 0, 0, 2],
        '=' => [0, 7, 0, 7, 0],
        '[' => [3, 2, 2, 2, 3],
        ']' => [6, 2, 2, 2, 6],
        '(' => [1, 2, 2, 2, 1],
        ')' => [4, 2, 2, 2, 4],
        '!' => [2, 2, 2, 0, 2],
        ',' => [0, 0, 0, 2, 4],
        '\'' => [2, 2, 0, 0, 0],
        '<' => [1, 2, 4, 2, 1],
        '>' => [4, 2, 1, 2, 4],
        '?' => [7, 1, 2, 0, 2],
        ';' => [0, 2, 0, 2, 4],
        '"' => [5, 5, 0, 0, 0],
        _ => [0; 5],
    }
}

/// Writes `s` at (x, y) in the 3 by 5 font `scale` times its size, in
/// `colour` on a box of `back` when there is one, clipped; returns the
/// width drawn.
pub(super) fn text(image: &mut Image, x: i32, y: i32, s: &str, colour: u32, back: Option<u32>, scale: i32) -> i32 {
    let (w, h) = (s.chars().count() as i32 * 4 * scale + scale, 7 * scale);
    let mut put = |px: i32, py: i32, c: u32| {
        if px >= 0 && py >= 0 && (px as usize) < image.width && (py as usize) < image.height {
            image.pixels[py as usize * image.width + px as usize] = c;
        }
    };
    if let Some(b) = back {
        for by in 0..h {
            for bx in 0..w {
                put(x + bx, y + by, b);
            }
        }
    }
    for (i, ch) in s.chars().enumerate() {
        for (r, bits) in glyph(ch).iter().enumerate() {
            for c in 0..3 {
                if bits & (4 >> c) != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            put(x + scale + (i as i32 * 4 + c) * scale + dx, y + scale + r as i32 * scale + dy, colour);
                        }
                    }
                }
            }
        }
    }
    w
}

/// Tints the 16-pixel cells `cells` (map cells, x and z) of a map picture
/// whose top left is the map's (ox, oz) pixels, each drawn `lift` pixels
/// higher for its ground's height, and outlines each in `colour`.
pub(super) fn mark_cells(image: &mut Image, (ox, oz): (i32, i32), cells: &[(i32, i32, i32)], colour: u32, share: f64) {
    for &(cx, cz, lift) in cells {
        let (sx, sy) = (cx * 16 - ox, cz * 16 - oz - lift);
        for y in sy.max(0)..(sy + 16).min(image.height as i32) {
            for x in sx.max(0)..(sx + 16).min(image.width as i32) {
                let at = &mut image.pixels[y as usize * image.width + x as usize];
                *at = super::guide_more::mix(*at, colour, share);
            }
        }
        outline(image, sx, sy, 16, 16, colour);
    }
}

/// The map of the room the field entered, the leader hidden, from the
/// moment `settled`, with its top left on the map in pixels.
pub(super) fn room_map(field: &mut Field, settled: &super::field::Saved) -> Result<(Image, (i32, i32)), String> {
    let r = field.room();
    let image = field.stitch(settled, [r[0] << 16, r[1] << 16, r[2] << 16, r[3] << 16], false)?;
    Ok((image, (r[0], r[1])))
}

/// Each cell holding collision code `code` on the leader's layer, with the
/// height its ground is drawn up by.
pub(super) fn code_cells(field: &mut Field, code: u16) -> Result<Vec<(i32, i32, i32)>, String> {
    let mut out = Vec::new();
    for (layer, cx, cz) in field.cells_with_code(code)? {
        let lift = field.ground(layer, cx * 16 + 8, cz * 16 + 8)? >> 16;
        out.push((cx, cz, lift));
    }
    Ok(out)
}

/// A horizontal line across `image` at row `y`, two pixels thick.
fn hline(image: &mut Image, y: i32, colour: u32) {
    outline(image, 0, y, image.width as i32, 2, colour);
}

/// The Tret Tree branches overlay: MapId_TretTreeBranch.
const TRET_TREE_BRANCH: u16 = 46;

impl<'a> Guide<'a> {
    /// Out on Tret's branches, standing and walking west and back: the
    /// screen beside BG3 alone with a red line at the scanline the overlay
    /// splits it at (sSplitLine, 192 less the ground layer's vertical
    /// scroll). BG3 follows the map's horizontal scroll on both sides; above
    /// the line it also drifts right a pixel every four frames, as
    /// `tret-tree-branch-split.gif`.
    pub fn tret_tree_branch_split(&mut self) -> Result<(), String> {
        let split = self.overlay_sym("gTretTreeBranch_WarpDests", "sSplitLine")?;
        // The entrance on the branch west of the trunk at 248 pixels down,
        // where the split is near the screen's top: walking along the
        // branch moves the map under the line and not over it.
        let probe = self.enter((TRET_TREE_BRANCH, 0))?;
        let top = probe.entrances().iter().find(|e| e.number > 0 && e.z == 248).map(|e| e.number as u16).ok_or("no entrance")?;
        drop(probe);
        let mut field = self.enter((TRET_TREE_BRANCH, top))?;
        let (w, h) = (2 * WIDTH + 12, HEIGHT + 8);
        let mut frames = Vec::new();
        for (keys, n) in [(0u32, 60u32), (KEY_LEFT, 90), (0, 60), (KEY_RIGHT, 90), (0, 60)] {
            field.hold(keys);
            for _ in 0..n {
                field.frames(1)?;
                let line = field.read32(split) as i32;
                let mut c = canvas(w, h, WHITE);
                paste(&mut c, &screen(&field.picture()), 4, 4, 1);
                let mut bg3 = alone(&mut field, 0x800)?;
                if (0..HEIGHT as i32).contains(&line) {
                    hline(&mut bg3, line - 1, RED);
                }
                paste(&mut c, &bg3, 8 + WIDTH, 4, 1);
                frames.push(c.pixels);
            }
        }
        field.hold(0);
        let written = animation::write(&self.out.join("tret-tree-branch-split.gif"), &frames, w)?;
        self.written.push(("tret-tree-branch-split.gif".to_string(), written.bytes, format!("Tret's branches beside BG3 alone and its split line; {} frames", written.frames)));
        Ok(())
    }
}

/// Kolima Forest's last map, where Tret stands: MapId_KolimaForestTree,
/// Tret's stand-in (LOCALID_TRET_STAND_IN) and Hermes' Water (ITEM_HERMES_WATER).
const KOLIMA_FOREST_TREE: u16 = 44;
const TRET_STAND_IN: u16 = 12;
const HERMES_WATER: u16 = 0xb8;
/// struct Unit's inventory, 15 halfwords; struct FieldWork's item
/// request and the inventory slot it passes (map_header.h).
const UNIT_INVENTORY: u32 = 0xd8;
const WORK_ITEM: u32 = 0x180;
const WORK_ITEM_SLOT: u32 = 0x19a;
/// Palette RAM: the background palettes 0 to 13 that KolimaPalette_Curse
/// changes are its first 224 colours.
const PALETTE_RAM: u32 = 0x0500_0000;
const CURSED_PALETTES: u32 = 14;

/// The 16 colours of each of the first `rows` background palettes as a grid
/// of `cell` pixel squares, a palette to a row.
pub(super) fn palette_grid(field: &Field, first: u32, rows: u32, cell: usize) -> Image {
    let mut image = canvas(16 * cell, rows as usize * cell, WHITE);
    for i in 0..rows * 16 {
        let colour = animation::rgb(field.read16(PALETTE_RAM + 2 * (first + i)));
        let (x, y) = ((i % 16) as usize * cell, (i / 16) as usize * cell);
        for dy in 0..cell - 1 {
            for dx in 0..cell - 1 {
                image.pixels[(y + dy) * image.width + x + dx] = colour;
            }
        }
    }
    image
}

impl<'a> Guide<'a> {
    /// Hermes' Water used on Tret after the Tret Tree (FLAG_TRET_DEFEATED),
    /// from the message that it has seeped into him until Tret speaks: the
    /// screen beside background palettes 0 to 13, the ones the curse
    /// changes, as KolimaForestTree_RestoreTret fades them to the healthy
    /// colours, pulses them between the two saved sets
    /// (KolimaForestTree_UpdateHealingPalette) and settles on the healthy
    /// ones, as `kolima-forest-tree-healing.gif`.
    pub fn kolima_forest_tree_healing(&mut self) -> Result<(), String> {
        let defeated = self.flag("FLAG_TRET_DEFEATED")?;
        let (mut entered, _) = self.stand_for((KOLIMA_FOREST_TREE, 0), super::story::Trigger::Talk(TRET_STAND_IN), TRET_STAND_IN)?;
        let field = &mut entered.field;
        field.set_flag(defeated, true);
        let unit = field.unit(0)?;
        let slot = (0..15).find(|&s| field.read16(unit + UNIT_INVENTORY + 2 * s) == 0).ok_or("Isaac's bag is full")?;
        field.write16(unit + UNIT_INVENTORY + 2 * slot, HERMES_WATER);
        let work = field.field_work();
        field.write16(work + WORK_ITEM_SLOT, slot as u16);
        field.write16(work + WORK_ITEM, HERMES_WATER);
        let cell = 10;
        let (w, h) = (WIDTH + 16 * cell + 12, HEIGHT + 8);
        let mut frames = Vec::new();
        // The pages: that Isaac uses the water and that it has seeped into
        // Tret are dismissed, and Tret's first line ends the picture.
        let mut pages = 0;
        let mut waited = false;
        for _ in 0..3000 {
            field.frames(1)?;
            if field.waiting() {
                if !waited {
                    pages += 1;
                }
                waited = true;
                if pages >= 3 {
                    break;
                }
                field.press(3)?;
                continue;
            }
            waited = false;
            if pages == 2 {
                let mut c = canvas(w, h, WHITE);
                paste(&mut c, &screen(&field.picture()), 4, 4, 1);
                let grid = palette_grid(field, 0, CURSED_PALETTES, cell);
                paste(&mut c, &grid, 8 + WIDTH, 4 + (HEIGHT - grid.height) / 2, 1);
                frames.push(c.pixels);
            }
        }
        if pages < 3 {
            return Err(format!("{pages} pages before the frames ran out"));
        }
        let written = animation::write(&self.out.join("kolima-forest-tree-healing.gif"), &frames, w)?;
        self.written.push(("kolima-forest-tree-healing.gif".to_string(), written.bytes, format!("the forest's colours healed beside its palettes; {} frames", written.frames)));
        Ok(())
    }
}

/// Altmiller Cave's way in: MapId_AltmillerCaveEntrance at its first entrance, where entering from the
/// world map, and the touch codes 60 to 65 that set its darkness levels 0
/// to 5 (AltmillerCave_DarkenToLevel0 to 5), the level kept in gMapLocalState
/// byte 4. AltmillerCave_ApplyFadeBlend gives BG1 no weight and every other
/// layer these sixteenths at each level.
const ALTMILLER_CAVE_ENTRANCE: (u16, u16) = (146, 1);
const DARKNESS_FIRST_CODE: u16 = 60;
const DARKNESS_WEIGHTS: [u32; 6] = [16, 14, 12, 10, 8, 6];
const SCENE_STATE_LEVEL: u32 = 4;

/// Runs the field until the camera has stood still for 8 frames.
pub(super) fn settle_view(field: &mut Field) -> Result<(), String> {
    let (mut last, mut still) = (field.view(), 0);
    for _ in 0..300 {
        field.frames(1)?;
        let now = field.view();
        still = if now == last { still + 1 } else { 0 };
        last = now;
        if still >= 8 {
            return Ok(());
        }
    }
    Err("the camera never settled".to_string())
}

/// struct FieldWork's Psynergy request (map_header.h).
const WORK_PSYNERGY: u32 = 0x17e;

/// Casts `ability` as Isaac from where the leader stands, as the field menu
/// asks for it: the request the field loop answers next.
pub(super) fn cast(field: &mut Field, ability: u16) {
    let work = field.field_work();
    field.write16(work + WORK_PSYNERGY, ability);
}

impl<'a> Guide<'a> {
    /// Altmiller Cave's way in, its map with the touch cells of each darkness
    /// level tinted and labelled with it, above one moment drawn at each level
    /// (the level written, then AltmillerCave_ApplyFadeBlend called), labelled
    /// with the level and the weight the layers under BG1 keep
    /// outside the light, as `altmiller-cave-darkness.png`.
    pub fn altmiller_cave_darkness(&mut self) -> Result<(), String> {
        let state = self.sym("gMapLocalState")?;
        let apply = self.overlay_sym("AltmillerCave_GetWarpDests", "AltmillerCave_ApplyFadeBlend")?;
        let mut entered = self.enter_with(ALTMILLER_CAVE_ENTRANCE, &[])?;
        let field = &mut entered.field;
        let settled = field.save();
        let (mut map, origin) = room_map(field, &settled)?;
        let mut shots = Vec::new();
        for level in 0..6u16 {
            field.load(&settled);
            let code = DARKNESS_FIRST_CODE + level;
            let cells = code_cells(field, code)?;
            let colour = super::guide_more::hue(level as usize);
            mark_cells(&mut map, origin, &cells, colour, 0.55);
            for &(cx, cz, lift) in &cells {
                text(&mut map, cx * 16 - origin.0 + 4, cz * 16 - origin.1 - lift + 1, &level.to_string(), WHITE, Some(colour), 2);
            }
            // The same moment at each level, as crossing its cell leaves it:
            // the level kept, then the blend it gives.
            field.write8(state + SCENE_STATE_LEVEL, level as u8);
            let leader = field.leader().ok_or("no leader")?;
            field.call(apply, [leader.x << 16, leader.z << 16])?;
            field.frames(2)?;
            shots.push(screen(&field.picture()));
        }
        let cols = 3;
        let shots_w = cols * (WIDTH + 4) + 4;
        let w = map.width.max(shots_w) + 8;
        let h = map.height + 8 + 2 * (HEIGHT + 4 + 18) + 4;
        let mut out = canvas(w, h, WHITE);
        paste(&mut out, &map, 4 + (w - 8 - map.width) / 2, 4, 1);
        for (i, shot) in shots.iter().enumerate() {
            let (x, y) = (4 + (i % cols) * (WIDTH + 4) + (w - 8 - shots_w) / 2, map.height + 8 + (i / cols) * (HEIGHT + 22));
            text(&mut out, x as i32, y as i32, &format!("{i}: {}/16", DARKNESS_WEIGHTS[i]), WHITE, Some(super::guide_more::hue(i)), 2);
            paste(&mut out, shot, x, y + 16, 1);
        }
        self.guide_png("altmiller-cave-darkness.png", &out, "Altmiller Cave's darkness levels and the touch cells that set them")
    }
}

/// Bilibin Cave's ice room: MapId_BilibinCaveIce, its fourth torch
/// (LOCALID_FOURTH_TORCH), the cell a torch melts the third puddle from, and
/// the cells north and south of the puddle that BilibinCave_UpdateThirdPuddleFromTorches
/// rewrites (MapGrid_CopyCellAttributes to (36, 22) and (36, 24)).
const BILIBIN_CAVE_ICE: u16 = 47;
const FOURTH_TORCH: u16 = 12;
const MELT_CELL: (i32, i32) = (35, 23);
const PUDDLE_SIDE_CELLS: [(i32, i32); 2] = [(36, 22), (36, 24)];
const KEY_RIGHT_PUSH: u32 = 0x10;

impl<'a> Guide<'a> {
    /// An entrance of `scene` whose room holds (x, z) pixels.
    pub(super) fn entrance_holding(&self, scene: u16, x: i32, z: i32) -> Result<u16, String> {
        let probe = self.enter((scene, 0))?;
        let header = probe.header();
        let found = probe.entrances().iter().find(|e| super::field::box_holds(e.camera, header, x, z)).map(|e| e.number as u16);
        found.ok_or_else(|| format!("no room of scene {scene} holds ({x}, {z})"))
    }

    /// The fourth torch in Bilibin Cave's ice room, put a cell west of the
    /// one beside the frozen third puddle and moved east onto it with Move:
    /// the puddle melts and the cells north and south of it change. Each
    /// frame outlines the torch's cell in yellow and the two rewritten cells
    /// in red, each labelled with what it does (guide_cells::meaning), over
    /// a key, as `bilibin-cave-third-puddle.gif`.
    pub fn bilibin_cave_third_puddle(&mut self) -> Result<(), String> {
        let (tx, tz) = (MELT_CELL.0 * 16 + 8, MELT_CELL.1 * 16 + 8);
        let entrance = self.entrance_holding(BILIBIN_CAVE_ICE, tx, tz)?;
        let mut entered = self.enter_with((BILIBIN_CAVE_ICE, entrance), &[])?;
        let field = &mut entered.field;
        field.set_actor_position(FOURTH_TORCH, tx - 16, tz);
        field.frames(2)?;
        let saved = field.save();
        let trigger = super::story::Trigger::Psynergy(super::story::MOVE, FOURTH_TORCH);
        let spot = *field.spots(trigger)?.iter().find(|s| s.walk == KEY_RIGHT_PUSH).ok_or("nowhere to move the torch east from")?;
        field.load(&saved);
        field.put_leader(&spot);
        settle_view(field)?;
        cast(field, super::story::MOVE);
        // Move waits for the d-pad: east, until the torch has moved.
        field.hold(spot.walk);
        let layer = spot.layer;
        let names = self.cell_names("BilibinCave_GetEvents", "BilibinCaveIce")?;
        let legend = [
            (0xff_e0_30, "Yellow outline: the cell beside the ice pillar; a torch standing on it melts the pillar".to_string()),
            (RED, "Red outline: the cells north and south of the pillar, which melting rewrites; each labelled with what it does".to_string()),
        ];
        let key = super::guide_cells::key(&legend, 2 * WIDTH - 8, None);
        let mut frames = Vec::new();
        for _ in 0..420 {
            field.frames(1)?;
            let (vx, vy) = field.view();
            let mut image = super::guide_more::canvas(2 * WIDTH, 2 * HEIGHT + 8 + key.height, WHITE);
            paste(&mut image, &screen(&field.picture()), 0, 0, 2);
            outline(&mut image, 2 * (MELT_CELL.0 * 16 - vx), 2 * (MELT_CELL.1 * 16 - vy), 32, 32, 0xff_e0_30);
            for &(cx, cz) in &PUDDLE_SIDE_CELLS {
                let code = field.cell(layer, cx, cz);
                let words = super::guide_cells::meaning(field, &names, &[], code)?.map_or_else(|| "Open ground".to_string(), |m| m.words);
                let (sx, sy) = (2 * (cx * 16 - vx), 2 * (cz * 16 - vy));
                outline(&mut image, sx, sy, 32, 32, RED);
                outline(&mut image, sx + 1, sy + 1, 30, 30, RED);
                text(&mut image, sx + 36, sy + 9, &words.to_uppercase(), WHITE, Some(RED), 2);
            }
            paste(&mut image, &key, 4, 2 * HEIGHT + 4, 1);
            frames.push(image.pixels);
            if field.actor(FOURTH_TORCH).is_some_and(|a| a.x != tx - 16) {
                field.hold(0);
            }
        }
        let written = animation::write(&self.out.join("bilibin-cave-third-puddle.gif"), &frames, 2 * WIDTH)?;
        self.written.push(("bilibin-cave-third-puddle.gif".to_string(), written.bytes, format!("a torch moved beside the third puddle; {} frames", written.frames)));
        Ok(())
    }
}

/// A colour for a collision code (enum MetatileBehavior): none for open ground,
/// red for a wall, yellow for the scene's own events, blue for the codes
/// every map shares.
pub(super) fn code_colour(code: i32) -> Option<u32> {
    match code {
        0 => None,
        255 => Some(0xe0_20_20),
        1..=239 => Some(0xf0_d0_20),
        _ => Some(0x30_80_f0),
    }
}

/// Draws what the cells from (c0, z0) to (c1, z1) inclusive do over
/// `image`, a picture of the map at `scale` times its size whose top left
/// is the map's (ox, oz) pixels: each cell with a collision code tinted by
/// its meaning's colour in `palette` (guide_cells::meaning, its rows put in
/// `words`), and outlined in white where its whole word (picture, code and
/// ground) differs from `before`'s. Returns whether any cell was outlined.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_codes(
    image: &mut Image,
    field: &Field,
    layer: u8,
    (ox, oz): (i32, i32),
    scale: i32,
    (c0, z0, c1, z1): (i32, i32, i32, i32),
    before: Option<&dyn Fn(i32, i32) -> i32>,
    (names, words, palette): (&super::guide_cells::Names, super::guide_cells::Words, &mut super::guide_cells::Palette),
) -> Result<bool, String> {
    let mut changed = false;
    for cz in z0..=z1 {
        for cx in c0..=c1 {
            let code = field.cell(layer, cx, cz);
            let (sx, sy) = (scale * (cx * 16 - ox), scale * (cz * 16 - oz));
            let side = 16 * scale;
            if let Some(m) = super::guide_cells::meaning(field, names, words, code)? {
                let colour = palette.colour(&m);
                for y in sy.max(0)..(sy + side).min(image.height as i32) {
                    for x in sx.max(0)..(sx + side).min(image.width as i32) {
                        let at = &mut image.pixels[y as usize * image.width + x as usize];
                        *at = super::guide_more::mix(*at, colour, 0.5);
                    }
                }
            }
            outline(image, sx, sy, side, side, 0x50_50_50);
            if before.is_some_and(|f| f(cx, cz) != field.cell_word(layer, cx, cz)) {
                outline(image, sx, sy, side, side, WHITE);
                outline(image, sx + 1, sy + 1, side - 2, side - 2, WHITE);
                changed = true;
            }
        }
    }
    Ok(changed)
}

/// A part of `image`: the rectangle (x, y, w, h), clipped.
pub(super) fn crop(image: &Image, x: i32, y: i32, w: usize, h: usize) -> Image {
    let mut out = canvas(w, h, 0);
    for dy in 0..h as i32 {
        for dx in 0..w as i32 {
            let (sx, sy) = (x + dx, y + dy);
            if sx >= 0 && sy >= 0 && (sx as usize) < image.width && (sy as usize) < image.height {
                out.pixels[dy as usize * w + dx as usize] = image.pixels[sy as usize * image.width + sx as usize];
            }
        }
    }
    out
}

/// Fuchin Falls Cave's first room: MapId_FuchinFallsCaveEntrance, the
/// flags of its three logs' stops (FLAG_MAPSEC_TEMP_01 to 03), and the cells
/// of their tracks that FuchinFallsCave_RebuildEntranceLogs rewrites
/// (columns 8 to 22, rows 29 to 33).
const FUCHIN_FIRST_ROOM: u16 = 62;
const FUCHIN_LOG_FLAGS: [&str; 3] = ["FLAG_MAPSEC_TEMP_01", "FLAG_MAPSEC_TEMP_02", "FLAG_MAPSEC_TEMP_03"];
const FUCHIN_TRACKS: (i32, i32, i32, i32) = (8, 29, 22, 33);

impl<'a> Guide<'a> {
    /// Fuchin Falls Cave's first room entered with none, then each in turn,
    /// of its logs' stop flags set: the track's part of the map with what
    /// each cell does drawn over it, the cells that differ from the room
    /// with no flag set outlined in white, a panel per state, as
    /// `fuchin-falls-cave-logs.png`.
    pub fn fuchin_falls_cave_logs(&mut self) -> Result<(), String> {
        const MOVED: [&str; 4] = ["No log moved", "First log pushed east", "Second log pushed north too", "Third log pushed north too"];
        let states: Vec<(String, Vec<&str>)> = (0..=3)
            .map(|set| {
                let name = if set == 0 { MOVED[0].to_string() } else { format!("{} ({})", MOVED[set], FUCHIN_LOG_FLAGS[set - 1]) };
                (name, FUCHIN_LOG_FLAGS[..set].to_vec())
            })
            .collect();
        let names = self.cell_names("ImilFuchinFalls_GetEvents", "FuchinFallsCaveEntrance")?;
        let out = self.code_panels(FUCHIN_FIRST_ROOM, None, FUCHIN_TRACKS, &states, &names, super::guide_cells::FUCHIN_FIRST_ROOM_WORDS)?;
        self.guide_png("fuchin-falls-cave-logs.png", &out, "Fuchin Falls Cave's first room, its log track's cells as each log's flag is set")
    }

    /// The cells from (c0, z0) to (c1, z1) of `scene`, a cell round them
    /// too, entered at `entrance` (else one whose room holds them) once for
    /// each of `states`, a label and the flags set: the map there at twice
    /// its size with what each cell does drawn over it (draw_codes), the
    /// cells that differ from the first state's outlined in white, a
    /// labelled panel per state, one under the other, and a key under them
    /// saying in words what each colour means.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn code_panels(
        &self,
        scene: u16,
        entrance: Option<u16>,
        (c0, z0, c1, z1): (i32, i32, i32, i32),
        states: &[(String, Vec<&str>)],
        names: &super::guide_cells::Names,
        words: super::guide_cells::Words,
    ) -> Result<Image, String> {
        let mut palette = super::guide_cells::Palette::default();
        let mut changed = false;
        let entrance = match entrance {
            Some(e) => e,
            None => self.entrance_holding(scene, (c0 + c1) * 8, (z0 + z1) * 8)?,
        };
        let scale = 2;
        let margin = 1;
        let (ox, oz) = ((c0 - margin) * 16, (z0 - margin) * 16);
        let (w, h) = (((c1 - c0 + 1 + 2 * margin) * 16 * scale) as usize, ((z1 - z0 + 1 + 2 * margin) * 16 * scale) as usize);
        let mut panels = Vec::new();
        let mut first: Option<Vec<i32>> = None;
        for (name, flags) in states {
            let mut entered = self.enter_with((scene, entrance), flags)?;
            let field = &mut entered.field;
            let layer = field.leader().map_or(0, |l| l.layer.min(2));
            let settled = field.save();
            let (map, origin) = room_map(field, &settled)?;
            field.load(&settled);
            let part = crop(&map, ox - origin.0, oz - origin.1, w / scale as usize, h / scale as usize);
            let mut big = canvas(w, h, 0);
            paste(&mut big, &part, 0, 0, scale as usize);
            let codes: Vec<i32> = (z0 - margin..=z1 + margin).flat_map(|cz| (c0 - margin..=c1 + margin).map(move |cx| (cx, cz))).map(|(cx, cz)| field.cell_word(layer, cx, cz)).collect();
            let base = first.get_or_insert(codes).clone();
            let row = (c1 - c0 + 1 + 2 * margin) as usize;
            let was = move |cx: i32, cz: i32| base[((cz - z0 + margin) as usize) * row + (cx - c0 + margin) as usize];
            changed |= draw_codes(&mut big, field, layer, (ox, oz), scale, (c0 - margin, z0 - margin, c1 + margin, z1 + margin), Some(&was), (names, words, &mut palette))?;
            panels.push((name.clone(), big));
        }
        let mut legend = palette.entries.clone();
        if changed {
            legend.push((WHITE, format!("White outline: changed from \"{}\"", states[0].0)));
        }
        let colours = super::guide_cells::key(&legend, w, Some("Colours"));
        let mut out = canvas(w + 8, panels.len() * (h + 22) + 8 + colours.height, WHITE);
        for (i, (name, panel)) in panels.iter().enumerate() {
            let y = 4 + i * (h + 22);
            text(&mut out, 4, y as i32, &name.to_uppercase(), WHITE, Some(0x40_40_40), 2);
            paste(&mut out, panel, 4, y + 16, 1);
        }
        paste(&mut out, &colours, 4, 4 + panels.len() * (h + 22), 1);
        Ok(out)
    }

    /// Mercury Lighthouse's pool: the cells Ply rewrites
    /// (MercuryLighthouseEntrance_ActivateWithPly copies 13 by 7 cells to
    /// (6, 32)), entered by the goddess's room before Ply and after it
    /// (FLAG_MERCURY_LIGHTHOUSE_HEART_HONORED), which puts the same cells back
    /// on entry, as `mercury-lighthouse-pool.png`.
    pub fn mercury_lighthouse_pool(&mut self) -> Result<(), String> {
        let states = vec![("Before Ply".to_string(), vec![]), ("After Ply (FLAG_MERCURY_LIGHTHOUSE_HEART_HONORED)".to_string(), vec!["FLAG_MERCURY_LIGHTHOUSE_HEART_HONORED"])];
        let names = self.cell_names("MercuryLighthouseEntrance_GetEvents", "MercuryLighthouseEntrance")?;
        let out = self.code_panels(MERCURY_LIGHTHOUSE_ENTRANCE, Some(MERCURY_GODDESS_FROM_SIX_STATUES), MERCURY_POOL, &states, &names, super::guide_cells::MERCURY_LIGHTHOUSE_ENTRANCE_WORDS)?;
        self.guide_png("mercury-lighthouse-pool.png", &out, "Mercury Lighthouse's pool cells before and after Ply")
    }
}

/// Lunpa Fortress's upper floor (MapId_LunpaFortressUpperFloor), entered
/// into its west passage (LUNPA_FORTRESS_UPPER_FLOOR_WEST_PASSAGE_FROM_NORTH_PASSAGE),
/// and its patrolling pair's rounds (gFirstPatrolRoute and
/// gSecondPatrolRoute): across from 88 to 192 pixels, at 192 and 288 down.
const LUNPA_FORTRESS_UPPER_FLOOR: (u16, u16) = (161, 2);
const PATROL_ROUND_X: (i32, i32) = (88, 192);
const PATROL_ROUND_Z: [i32; 2] = [192, 288];

/// Whether LunpaFortress_IsLeaderInPatrolSafeZone holds for a leader in
/// cell (cx, cz): rows 5 to 7 at column 10 or less, or columns 8 and 9 below
/// row 22.
fn patrol_safe(cx: i32, cz: i32) -> bool {
    ((5..=7).contains(&cz) && cx <= 10) || ((8..=9).contains(&cx) && cz > 22)
}

/// Whether a guard in cell (gx, gz) sees a leader in cell (cx, cz), as
/// LunpaFortress_CanGuardSeeLeader tests it once the guard is on screen: in
/// his column up to six rows away, or within a row of his up to six columns
/// away.
fn guard_sees(gx: i32, gz: i32, cx: i32, cz: i32) -> bool {
    (cx == gx && (cz - gz).abs() <= 6) || ((cz - gz).abs() <= 1 && (cx - gx).abs() <= 6)
}

impl<'a> Guide<'a> {
    /// Lunpa Fortress's west passage on the upper floor with its patrolling
    /// pair's rounds drawn in yellow, every cell either guard sees from some
    /// point of his round tinted red, and the cells where
    /// LunpaFortress_IsLeaderInPatrolSafeZone stops the pair looking tinted
    /// green, as `lunpa-fortress-patrol.png`.
    pub fn lunpa_fortress_patrol(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with(LUNPA_FORTRESS_UPPER_FLOOR, &[])?;
        let field = &mut entered.field;
        let settled = field.save();
        let (map, (ox, oz)) = room_map(field, &settled)?;
        let mut image = canvas(2 * map.width, 2 * map.height, 0);
        paste(&mut image, &map, 0, 0, 2);
        let (cols, rows) = (map.width as i32 / 16, map.height as i32 / 16);
        let (c0, z0) = (ox / 16, oz / 16);
        let guards: Vec<(i32, i32)> = PATROL_ROUND_Z
            .iter()
            .flat_map(|&z| (PATROL_ROUND_X.0 / 16..=PATROL_ROUND_X.1 / 16).map(move |x| (x, z / 16)))
            .collect();
        for cz in z0..z0 + rows {
            for cx in c0..c0 + cols {
                let tint = if patrol_safe(cx, cz) {
                    Some(0x30_e0_60)
                } else if guards.iter().any(|&(gx, gz)| guard_sees(gx, gz, cx, cz)) {
                    Some(0xf0_30_30)
                } else {
                    None
                };
                let Some(t) = tint else { continue };
                let (sx, sy) = (2 * (cx * 16 - ox), 2 * (cz * 16 - oz));
                for y in sy.max(0)..(sy + 32).min(image.height as i32) {
                    for x in sx.max(0)..(sx + 32).min(image.width as i32) {
                        let at = &mut image.pixels[y as usize * image.width + x as usize];
                        *at = super::guide_more::mix(*at, t, 0.4);
                    }
                }
                outline(&mut image, sx, sy, 32, 32, super::guide_more::mix(t, 0, 0.3));
            }
        }
        for &z in &PATROL_ROUND_Z {
            let (x0, x1, y) = (2 * (PATROL_ROUND_X.0 - ox), 2 * (PATROL_ROUND_X.1 - ox), 2 * (z - oz));
            outline(&mut image, x0, y - 2, x1 - x0, 4, 0xff_e0_30);
            for x in [x0, x1] {
                outline(&mut image, x - 4, y - 4, 8, 8, 0xff_e0_30);
            }
        }
        self.guide_png("lunpa-fortress-patrol.png", &image, "Lunpa Fortress's west passage, its patrol's sight and the cells it ignores")
    }

}

/// Mercury Lighthouse's ground floor (MapId_MercuryLighthouseEntrance) and
/// the pool's cells that Ply rewrites: columns 6 to 18, rows 32 to 38.
const MERCURY_LIGHTHOUSE_ENTRANCE: u16 = 53;
const MERCURY_POOL: (i32, i32, i32, i32) = (6, 32, 18, 38);
/// MERCURY_LIGHTHOUSE_ENTRANCE_GODDESS_FROM_SIX_STATUES (scene_links.h).
const MERCURY_GODDESS_FROM_SIX_STATUES: u16 = 3;

/// Tolbi Spring (MapId_TolbiSpring) and the search code of its spring
/// (TOLBI_SPRING_SPRING); the toss's centre (TOLBI_SPRING_CENTER_X and _Z),
/// the squared distances that bound its five rings and what a coin wins in
/// each (gTolbiSpring_CoinPrizes); struct SpringTossMotion's launch frame
/// and first trail point.
const TOLBI_SPRING: u16 = 189;
const TOLBI_SPRING_SPRING: u16 = 80;
/// TOLBI_SPRING_FROM_TOLBI: only this entrance starts the toss simulation.
const TOLBI_SPRING_FROM_TOLBI: u16 = 1;
const SPRING_CENTER: (i32, i32) = (120, 71);
const SPRING_RINGS: [i32; 4] = [224, 624, 1088, 1680];
const SPRING_COIN_PRIZES: [i32; 5] = [20, 10, 2, 1, 0];
const TOSS_LAUNCH_FRAME: u32 = 2;
const TOSS_TRAIL_X: u32 = 4;
const TOSS_TRAIL_Z: u32 = 12;

/// Draws a circle of radius `r` about (cx, cy), a pixel wide, clipped.
fn circle(image: &mut Image, cx: f64, cy: f64, r: f64, colour: u32) {
    let steps = (r * 8.0) as i32 + 16;
    for i in 0..steps {
        let a = f64::from(i) / f64::from(steps) * std::f64::consts::TAU;
        let (x, y) = ((cx + r * a.cos()).round() as i32, (cy + r * a.sin()).round() as i32);
        if x >= 0 && y >= 0 && (x as usize) < image.width && (y as usize) < image.height {
            image.pixels[y as usize * image.width + x as usize] = colour;
        }
    }
}

impl<'a> Guide<'a> {
    /// A coin tossed into Tolbi Spring, from the throw until it settles: the
    /// screen at twice its size with the five prize rings drawn about the
    /// toss's centre at the distances TolbiSpring_UpdateTossSimulation
    /// measures, each labelled with what a coin wins there, and the token's
    /// path so far (sToss's first trail point, on the water) in yellow, as
    /// `tolbi-spring-toss.gif`.
    pub fn tolbi_spring_toss(&mut self) -> Result<(), String> {
        let toss = self.overlay_sym("gTolbiSpring_WarpDests", "sToss")?;
        let ended = self.overlay_sym("gTolbiSpring_WarpDests", "sTossEnded")?;
        let (mut entered, _) = self.stand_for((TOLBI_SPRING, TOLBI_SPRING_FROM_TOLBI), super::story::Trigger::Search(TOLBI_SPRING_SPRING), 0)?;
        let field = &mut entered.field;
        field.set_coins(100);
        field.press(2)?;
        // A through the spring's rising, the question and the toss menu,
        // until the token is thrown.
        let launched = |field: &Field| (field.read16(toss + TOSS_LAUNCH_FRAME) as i16) >= 0;
        // RunToss marks the token unthrown (-1) before it throws it.
        let mut armed = false;
        let mut n = 0;
        while !(armed && launched(field)) {
            armed |= !launched(field);
            if n > 3000 {
                return Err("the token was never thrown".to_string());
            }
            if n % 40 == 39 {
                field.press(2)?;
            } else {
                field.frames(1)?;
            }
            n += 1;
        }
        let mut path: Vec<(i32, i32)> = Vec::new();
        let mut frames = Vec::new();
        let mut after = 0;
        for _ in 0..1200 {
            field.frames(1)?;
            let (vx, vy) = field.view();
            let (x, z) = (field.read32(toss + TOSS_TRAIL_X) as i32 >> 16, field.read32(toss + TOSS_TRAIL_Z) as i32 >> 16);
            if (field.read16(toss + TOSS_LAUNCH_FRAME) as i16) > 31 {
                path.push((x, z));
            }
            let mut image = canvas(2 * WIDTH, 2 * HEIGHT, 0);
            paste(&mut image, &screen(&field.picture()), 0, 0, 2);
            let (cx, cy) = (2.0 * f64::from(SPRING_CENTER.0 - vx), 2.0 * f64::from(SPRING_CENTER.1 - vy));
            let mut inner = 0.0;
            for (i, &d2) in SPRING_RINGS.iter().enumerate() {
                let r = f64::from(d2).sqrt();
                circle(&mut image, cx, cy, 2.0 * r, WHITE);
                // Each ring's prize at its middle, east and west by turns.
                let mid = if i % 2 == 0 { inner + r } else { -(inner + r) };
                text(&mut image, (cx + mid) as i32 - 6, cy as i32 - 6, &SPRING_COIN_PRIZES[i].to_string(), WHITE, Some(0x20_40_a0), 2);
                inner = r;
            }
            text(&mut image, (cx + 2.0 * inner) as i32 + 4, cy as i32 - 6, &SPRING_COIN_PRIZES[4].to_string(), WHITE, Some(0x20_40_a0), 2);
            for w in path.windows(2) {
                let ((x0, z0), (x1, z1)) = (w[0], w[1]);
                for t in 0..=8 {
                    let (px, pz) = (x0 + (x1 - x0) * t / 8, z0 + (z1 - z0) * t / 8);
                    outline(&mut image, 2 * (px - vx), 2 * (pz - vy), 2, 2, 0xff_e0_30);
                }
            }
            frames.push(image.pixels);
            if field.read32(ended) == 1 {
                after += 1;
                if after > 60 {
                    break;
                }
            }
        }
        let written = animation::write(&self.out.join("tolbi-spring-toss.gif"), &frames, 2 * WIDTH)?;
        self.written.push(("tolbi-spring-toss.gif".to_string(), written.bytes, format!("a coin tossed into Tolbi Spring and its prize rings; {} frames", written.frames)));
        Ok(())
    }
}

/// Kolima Forest's way in (MapId_KolimaForestEntrance) from the world map
/// (KOLIMA_FOREST_ENTRANCE_FROM_WORLD_MAP), where the curse falls the first
/// time; its four Psynergy fields (LOCALID_LEADER_PSYNERGY_FIELD to
/// LOCALID_GARET_PSYNERGY_FIELD), where KolimaForestEntrance_WitnessForestCurse
/// puts them, and the boxes KolimaForestEntrance_BurstOnPsynergyField tests a
/// landing sparkle against, in pixels (x0, x1, z0, z1), with Mia in the party
/// and without her.
const KOLIMA_FOREST_ENTRANCE: (u16, u16) = (40, 4);
const PSYNERGY_FIELDS: [(u16, &str, i32, i32); 4] = [(13, "ISAAC", 253, 603), (14, "IVAN", 233, 629), (15, "MIA", 207, 609), (16, "GARET", 227, 580)];
const FIELD_BOXES_WITH_MIA: [(i32, i32, i32, i32); 2] = [(192, 274, 566, 612), (202, 255, 549, 632)];
const FIELD_BOXES_WITHOUT_MIA: [(i32, i32, i32, i32); 3] = [(192, 244, 549, 584), (244, 274, 571, 604), (211, 255, 596, 632)];
/// The party flag of Mia (FLAG_IN_PARTY(MIA)).
const FLAG_MIA_IN_PARTY: u16 = 3;

impl<'a> Guide<'a> {
    /// The curse falling on Kolima Forest's way in, once without Mia and
    /// once with her: two frames running, cropped to the Psynergy fields,
    /// each field's place labelled with whose it is, the boxes a sparkle
    /// bursts in outlined in yellow, as `kolima-forest-entrance-fields.png`.
    pub fn kolima_forest_entrance_fields(&mut self) -> Result<(), String> {
        let scale = 3;
        let (x0, z0, side) = (170, 530, 130);
        let mut rows = Vec::new();
        for mia in [false, true] {
            let mut set = vec![self.flag("FLAG_VALE_PARTY_LEFT")?];
            if mia {
                set.push(FLAG_MIA_IN_PARTY);
            }
            let clear = vec![self.flag("FLAG_VALE_STORM")?, self.flag("FLAG_CHILDHOOD")?];
            let setup = super::field::Setup { scene: KOLIMA_FOREST_ENTRANCE.0, entrance: KOLIMA_FOREST_ENTRANCE.1, set, clear };
            let mut field = Field::enter(self.rom, self.symbols, &setup)?;
            // Through the curse's lines until the fields stand where the
            // scene puts them and have grown.
            let mut placed = None;
            for n in 0..6000 {
                field.frames(1)?;
                if field.waiting() {
                    field.press(3)?;
                }
                let at = field.actor(PSYNERGY_FIELDS[0].0).map(|a| (a.x, a.z));
                if placed.is_none() && at == Some((PSYNERGY_FIELDS[0].2, PSYNERGY_FIELDS[0].3)) {
                    placed = Some(n);
                }
                if placed.is_some_and(|p| n >= p + 90) {
                    break;
                }
            }
            if placed.is_none() {
                return Err("the Psynergy fields never rose".to_string());
            }
            let boxes: &[(i32, i32, i32, i32)] = if mia { &FIELD_BOXES_WITH_MIA } else { &FIELD_BOXES_WITHOUT_MIA };
            let mut pair = Vec::new();
            for _ in 0..2 {
                field.frames(1)?;
                let (vx, vy) = field.view();
                let mut part = crop(&screen(&field.picture()), x0 - vx, z0 - vy, side as usize, side as usize);
                let mut big = canvas(part.width * scale, part.height * scale, 0);
                paste(&mut big, &part, 0, 0, scale);
                part = big;
                let s = scale as i32;
                for &(bx0, bx1, bz0, bz1) in boxes {
                    outline(&mut part, s * (bx0 - x0), s * (bz0 - z0), s * (bx1 - bx0), s * (bz1 - bz0), 0xff_e0_30);
                }
                for (i, &(id, name, x, z)) in PSYNERGY_FIELDS.iter().enumerate() {
                    if !mia && id == 15 {
                        continue;
                    }
                    let shown = field.actor(id).is_some_and(|a| a.y >> 16 < 100);
                    let colour = if shown { super::guide_more::hue(i) } else { 0x50_50_50 };
                    text(&mut part, s * (x - x0) - 12, s * (z - z0) - 8, name, WHITE, Some(colour), 2);
                }
                pair.push(part);
            }
            rows.push((if mia { "WITH MIA" } else { "WITHOUT MIA" }, pair));
        }
        let cell = side * scale;
        let mut out = canvas(2 * cell + 12, 2 * (cell + 20) + 8, WHITE);
        for (r, (name, pair)) in rows.iter().enumerate() {
            let y = 4 + r * (cell + 20);
            text(&mut out, 4, y as i32, name, WHITE, Some(0x40_40_40), 2);
            for (c, part) in pair.iter().enumerate() {
                paste(&mut out, part, 4 + c * (cell + 4), y + 16, 1);
            }
        }
        self.guide_png("kolima-forest-entrance-fields.png", &out, "the Psynergy fields on two frames running, and where a sparkle bursts on them")
    }

    /// The whole map of Tret Tree's inside (MapId_TretTreeInteriors), all
    /// six rooms where the map lays them out: each cocoon's touch cells
    /// (codes 21 to 29, 31, 32 and 50 to 59) outlined and labelled with the
    /// code, and the entrance of the same number, where breaking through it
    /// drops the leader, marked with a ring and the same label in the same
    /// colour, as `tret-tree-interiors-cocoons.png`.
    pub fn tret_tree_interiors_cocoons(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with((TRET_TREE_INTERIORS, 0), &[])?;
        let field = &mut entered.field;
        let settled = field.save();
        let h = field.header();
        let map = field.stitch(&settled, h, false)?;
        field.load(&settled);
        let (ox, oz) = (h[0] >> 16, h[1] >> 16);
        let mut image = map;
        let entrances = field.entrances();
        for (i, &code) in TRET_COCOONS.iter().enumerate() {
            let colour = super::guide_more::hue(i);
            let cells = code_cells(field, code)?;
            mark_cells(&mut image, (ox, oz), &cells, colour, 0.5);
            for &(cx, cz, lift) in cells.iter().take(1) {
                text(&mut image, cx * 16 - ox - 2, cz * 16 - oz - lift - 15, &code.to_string(), WHITE, Some(colour), 2);
            }
            if let Some(e) = entrances.iter().find(|e| e.number as u16 == code) {
                let (x, z) = (i32::from(e.x) - ox, i32::from(e.z) - oz);
                circle(&mut image, f64::from(x), f64::from(z), 7.0, colour);
                circle(&mut image, f64::from(x), f64::from(z), 8.0, colour);
                text(&mut image, x + 9, z - 7, &code.to_string(), colour, Some(WHITE), 2);
            }
        }
        self.guide_png("tret-tree-interiors-cocoons.png", &image, "Tret Tree's rooms, each cocoon and the entrance it drops the leader to")
    }
}

/// Colosseum Final 1 (MapId_ColosseumAzartMatch) at the race
/// (COLOSSEUM_AZART_MATCH_RACE_FROM_LEND_A_HAND), and Azart (LOCALID_OPPONENT).
const COLOSSEUM_AZART_RACE: (u16, u16) = (143, 1);
/// Colosseum Final 3 (MapId_ColosseumNavampaMatch), the touch that steps
/// onto the Scales of Justice and its two pans.
const COLOSSEUM_NAVAMPA_MATCH: u16 = 145;
const STEP_ON_SCALES: u16 = 15;
const SCALE_PAN_WEST: u16 = 10;
const SCALE_PAN_EAST: u16 = 11;
const AZART: u16 = 8;
/// Colosseum Final 2 (MapId_ColosseumSatrageMatch) at the tour
/// (COLOSSEUM_SATRAGE_MATCH_FROM_COLOSSEUM_WEST), and where the leader is put,
/// hidden, for the camera to centre on the lifts.
const COLOSSEUM_SATRAGE_TOUR: (u16, u16) = (144, 3);
/// Vale entered by day (scene 4, entrance 1) and Garet's sister there.
const VALE_SCENE: (u16, u16) = (4, 1);
/// The world map's frame work's heap slot (HEAP_SLOT_WORLD_MAP_FRAME).
const HEAP_SLOT_WORLD_MAP_FRAME: u32 = 7;
/// Goma Cave's pillar room, as `grid.png` enters it, and the cell words.
const GOMA_CAVE_ROOM: (u16, u16) = (27, 5);
const MAP_CELL_WORDS: u32 = 0x0201_0000;
/// struct FieldWork's twelve requests, by offset, in the order
/// Map_Run tests them.
const REQUESTS: [(u32, &str); 12] = [
    (0x170, "WARP"),
    (0x182, "RAISEDEVENT"),
    (0x17c, "BATTLE"),
    (0x16c, "COORDEVENT"),
    (0x16e, "PUSHCELL"),
    (0x174, "EXAMINE"),
    (0x172, "MENU"),
    (0x178, "TALK"),
    (0x17a, "SEARCHCELL"),
    (0x17e, "PSYNERGY"),
    (0x180, "ITEM"),
    (0x176, "START"),
];
/// A door of Vale's, a touch cell whose event leaves the scene.
const VALE_DOOR: u16 = 7;
/// Bilibin's inn, a room of BilibinInteriors (scene_links.h), and its
/// innkeeper.
const BILIBIN_INN_ROOM: (u16, u16) = (31, 6);
const BILIBIN_INNKEEPER_ACTOR: u16 = 19;
/// The window members of struct ItemMenuWork (item_menu.h), by
/// offset.
const ITEM_SCREEN_WINDOWS: [(u32, &str); 10] = [
    (0x10, "mainWindow"),
    (0x20, "itemWindow"),
    (0x24, "statusWindow"),
    (0x28, "buttonHelpWindow"),
    (0x2c, "descriptionWindow"),
    (0x30, "equipWindow"),
    (0x34, "listWindow"),
    (0x3c, "modalWindow"),
    (0x10c, "questionWindow"),
    (0x38, "unopenedWindow"),
];
/// Sprite attribute memory and the sprites' tile memory.
const OAM_BASE: u32 = 0x0700_0000;
const OBJ_VRAM_BASE: u32 = 0x0601_0000;
const GARETS_SISTER_ACTOR: u16 = 18;
/// The Lighthouse Aerie (MapId_VenusLighthouseAerie) after the second
/// battle (VENUS_LIGHTHOUSE_AERIE_AFTER_SECOND_BATTLE), and the beam's lowest
/// segment (LOCALID_BEAM_BOTTOM; the other four follow it).
const VENUS_LIGHTHOUSE_AERIE: (u16, u16) = (187, 3);
const LOCALID_BEAM_BOTTOM: u16 = 24;
/// The Tunnel Ruins' balance room (MapId_TunnelRuinsLadyStatues), its two
/// entrances (TUNNEL_RUINS_LADY_STATUES_FROM_TUNNEL_RUINS_GREAT_STATUE_NORTH
/// and TUNNEL_RUINS_LADY_STATUES_BALANCE_SOUTH), its west platform
/// (LOCALID_BALANCE_WEST; the east one and the carved stones A to C follow),
/// where the hidden leader is put for the camera to centre on the balance,
/// and struct ObjectEvent's timer, a platform's weight.
const TUNNEL_RUINS_LADY_STATUES: u16 = 173;
const BALANCE_FROM_GREAT_STATUE: u16 = 4;
const BALANCE_SOUTH: u16 = 5;
const BALANCE_WEST: u16 = 10;
const BALANCE_WATCH: (i32, i32) = (168, 200);
const OBJ_TIMER: u32 = 0x64;
const LIFT_WATCH: (i32, i32) = (984, 216);
/// COLOSSEUM_SATRAGE_MATCH_LIFT_CYCLE_FRAMES: the frames between the lifts' changes.
const LIFT_CYCLE_FRAMES: u32 = 120;
/// The tour of the course (COLOSSEUM_AZART_MATCH_FROM_COLOSSEUM), and the
/// finish across which Azart's place mirrors his recorded path
/// (Colosso_StartRaceGauge's finishX).
const COLOSSEUM_AZART_TOUR: u16 = 3;
const AZART_FINISH_X: i32 = 1304;
/// The race gauge's rows at the top of the screen.
const GAUGE_ROWS: usize = 16;

impl<'a> Guide<'a> {
    /// The first finals race with Isaac left standing at the start: the
    /// whole course with the path Azart plays back (Colosso_StartRival),
    /// which runs east of the finish, mirrored back across it onto the course
    /// Isaac runs and drawn in red, a dot every five seconds labelled with the seconds, and
    /// under it the race gauge (Colosso_StartRaceGauge) at the top of the
    /// screen at four moments, as `colosseum-azart-race.png`.
    pub fn colosseum_azart_race(&mut self) -> Result<(), String> {
        // The course from the tour, which has no gauge or lettering on it.
        let mut tour = self.enter_with((COLOSSEUM_AZART_RACE.0, COLOSSEUM_AZART_TOUR), &[])?;
        let settled = tour.field.save();
        let h = tour.field.header();
        let mut map = tour.field.stitch(&settled, h, false)?;
        drop(tour);
        let mut entered = self.enter_with(COLOSSEUM_AZART_RACE, &[])?;
        let field = &mut entered.field;
        let (ox, oz) = (h[0] >> 16, h[1] >> 16);
        let start = field.leader().map(|a| (a.x, a.z)).ok_or("no leader")?;
        let mut path = Vec::new();
        let mut gauges = Vec::new();
        let mut still = 0;
        for n in 0..7200u32 {
            field.frames(1)?;
            let Some(a) = field.actor(AZART) else { break };
            if path.last() == Some(&(2 * AZART_FINISH_X - a.x, a.z)) {
                still += 1;
            } else {
                still = 0;
            }
            path.push((2 * AZART_FINISH_X - a.x, a.z));
            if n % 600 == 0 {
                gauges.push((n / 60, crop(&screen(&field.picture()), 0, 0, WIDTH, GAUGE_ROWS)));
            }
            if still > 180 {
                break;
            }
        }
        gauges.push(((path.len() / 60) as u32, crop(&screen(&field.picture()), 0, 0, WIDTH, GAUGE_ROWS)));
        for w in path.windows(2) {
            let ((x0, z0), (x1, z1)) = (w[0], w[1]);
            for t in 0..=4 {
                let (px, pz) = (x0 + (x1 - x0) * t / 4, z0 + (z1 - z0) * t / 4);
                outline(&mut map, px - ox - 1, pz - oz - 1, 2, 2, RED);
            }
        }
        for (i, &(x, z)) in path.iter().enumerate().step_by(300) {
            outline(&mut map, x - ox - 3, z - oz - 3, 6, 6, WHITE);
            text(&mut map, x - ox + 4, z - oz - 14, &format!("{}", i / 60), WHITE, Some(RED), 2);
        }
        let tall = map.height as i32;
        outline(&mut map, AZART_FINISH_X - ox, 0, 1, tall, WHITE);
        text(&mut map, AZART_FINISH_X - ox + 3, 2, "FINISH", WHITE, Some(0x40_40_40), 2);
        circle(&mut map, f64::from(start.0 - ox), f64::from(start.1 - oz), 8.0, 0x30_80_f0);
        text(&mut map, start.0 - ox + 10, start.1 - oz - 7, "ISAAC", WHITE, Some(0x30_80_f0), 2);
        // Four of the gauges kept: evenly through the run, the last at its end.
        let picks: Vec<&(u32, Image)> = {
            let n = gauges.len();
            let mut idx: Vec<usize> = (0..4).map(|k| k * (n - 1) / 3).collect();
            idx.dedup();
            idx.into_iter().map(|k| &gauges[k]).collect()
        };
        let gauge_h = 2 * GAUGE_ROWS + 4;
        let w = map.width.max(2 * WIDTH + 80) + 8;
        let mut out = canvas(w, map.height + 8 + picks.len() * (gauge_h + 4) + 4, WHITE);
        paste(&mut out, &map, 4, 4, 1);
        for (i, (secs, g)) in picks.iter().enumerate() {
            let y = map.height + 8 + i * (gauge_h + 4);
            text(&mut out, 4, y as i32 + 8, &format!("{secs} S"), WHITE, Some(0x40_40_40), 2);
            paste(&mut out, g, 72, y, 2);
        }
        self.guide_png("colosseum-azart-race.png", &out, "Azart's recorded path through the first finals course and the race gauge")
    }

    /// The leader stepping onto the western pan of the Scales of Justice in
    /// Colosseum Final 3 (touch 15, EventScript_StepOnScales): the screen at twice
    /// its size with each pan (SCALE_PAN_WEST and SCALE_PAN_EAST) marked at
    /// its place and labelled with its height in pixels as it moves, as
    /// `colosseum-navampa-scales.gif`.
    pub fn colosseum_navampa_scales(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with((COLOSSEUM_NAVAMPA_MATCH, 0), &[])?;
        let field = &mut entered.field;
        // The pans lie past stages the run does not play first.
        field.relaxed = true;
        let spot = *field.spots(super::story::Trigger::Touch(STEP_ON_SCALES))?.first().ok_or("nowhere to step onto the scales from")?;
        field.stand(&entered.placed, &spot)?;
        settle_view(field)?;
        let mut frames = Vec::new();
        for i in 0..240 {
            field.hold(if i < 40 { spot.walk } else { 0 });
            field.frames(1)?;
            let (vx, vy) = field.view();
            let mut image = canvas(2 * WIDTH, 2 * HEIGHT, 0);
            paste(&mut image, &screen(&field.picture()), 0, 0, 2);
            for (id, name) in [(SCALE_PAN_WEST, "WEST"), (SCALE_PAN_EAST, "EAST")] {
                let Some(a) = field.actor(id) else { continue };
                let height = a.y >> 16;
                let (sx, sy) = (2 * (a.x - vx), 2 * (a.z - height - vy));
                outline(&mut image, sx - 3, sy - 3, 6, 6, 0xff_e0_30);
                text(&mut image, sx - 20, sy + 6, &format!("{name} {height}"), WHITE, Some(0x40_40_40), 2);
            }
            frames.push(image.pixels);
        }
        field.hold(0);
        let written = animation::write(&self.out.join("colosseum-navampa-scales.gif"), &frames, 2 * WIDTH)?;
        self.written.push(("colosseum-navampa-scales.gif".to_string(), written.bytes, format!("stepping onto the Scales of Justice; {} frames", written.frames)));
        Ok(())
    }

    /// The shifting floor stage of Colosseum Final 2 entered for the tour,
    /// the leader put among the lifts and hidden so the camera centres on
    /// them, over two of the lifts' cycles: the screen at twice its size with
    /// the cells ColosseumSatrageMatch_UpdateLifts copies each frame (row 13
    /// from column 58 to 64, and cell (58, 11)) outlined and tinted by what
    /// each does, over a key, and the lifts' phase in words and the frames to
    /// its next change (gColosseumSatrageMatch_LiftPhase and _LiftTimer)
    /// written above, as `colosseum-satrage-lifts.gif`.
    pub fn colosseum_satrage_lifts(&mut self) -> Result<(), String> {
        let timer = self.overlay_sym("gColosseumSatrageMatch_LiftTimer", "gColosseumSatrageMatch_LiftTimer")?;
        let phase = self.overlay_sym("gColosseumSatrageMatch_LiftTimer", "gColosseumSatrageMatch_LiftPhase")?;
        let mut entered = self.enter_with(COLOSSEUM_SATRAGE_TOUR, &[])?;
        let field = &mut entered.field;
        let spot = Self::spot_at(field, (LIFT_WATCH.0, LIFT_WATCH.1, 0x8000))?;
        field.stand(&entered.placed, &spot)?;
        let leader = field.read32(self.sym("gGameState")? + 0x1f4) as u16;
        field.hide_actor(leader);
        settle_view(field)?;
        // The pool's ground is 32 pixels up, and a rider on a lift is drawn
        // that far above his place on the map: the cells are drawn there
        // too, so they sit under the lifts.
        let lift = field.ground(0, LIFT_WATCH.0, LIFT_WATCH.1)? >> 16;
        let mut cells: Vec<(i32, i32)> = (58..=64).map(|x| (x, 13)).collect();
        cells.push((58, 11));
        let names = self.cell_names("gColosseumSatrageMatch_LiftTimer", "ColosseumSatrageMatch")?;
        // Every meaning the cells take over the two cycles, so the key is
        // the same in every frame.
        let mut palette = super::guide_cells::Palette::default();
        let start = field.save();
        for _ in 0..2 * LIFT_CYCLE_FRAMES + 20 {
            field.frames(1)?;
            for &(cx, cz) in &cells {
                if let Some(m) = super::guide_cells::meaning(field, &names, super::guide_cells::SATRAGE_WORDS, field.cell(0, cx, cz))? {
                    palette.colour(&m);
                }
            }
        }
        field.load(&start);
        let mut legend = palette.entries.clone();
        legend.push((WHITE, "White outline: a cell the lifts' task copies collision into; with no tint, it is open ground".to_string()));
        let key = super::guide_cells::key(&legend, 2 * WIDTH - 8, Some("Colours"));
        let mut frames = Vec::new();
        for _ in 0..2 * LIFT_CYCLE_FRAMES + 20 {
            field.frames(1)?;
            let (vx, vy) = field.view();
            let mut image = canvas(2 * WIDTH, 2 * HEIGHT + 8 + key.height, WHITE);
            paste(&mut image, &screen(&field.picture()), 0, 0, 2);
            for &(cx, cz) in &cells {
                let (sx, sy) = (2 * (cx * 16 - vx), 2 * (cz * 16 - vy - lift));
                if let Some(m) = super::guide_cells::meaning(field, &names, super::guide_cells::SATRAGE_WORDS, field.cell(0, cx, cz))? {
                    super::guide_cells::tint_cell(&mut image, sx, sy, 32, palette.colour(&m), 0.55);
                }
                outline(&mut image, sx, sy, 32, 32, WHITE);
                outline(&mut image, sx + 1, sy + 1, 30, 30, WHITE);
            }
            // In phase 0 the lifts come into line across the water, in phase
            // 1 they part, two up and two down (ColosseumSatrageMatch_UpdateLifts,
            // as the frames show).
            let lifts = if field.read32(phase) != 0 { "Lifts parting" } else { "Lifts lining up" };
            let left = LIFT_CYCLE_FRAMES.saturating_sub(field.read32(timer));
            let line = format!("{lifts}; next change in {left} frames");
            text(&mut image, 4, 4, &line.to_uppercase(), WHITE, Some(0x40_40_40), 2);
            paste(&mut image, &key, 4, 2 * HEIGHT + 4, 1);
            frames.push(image.pixels);
        }
        let written = animation::write(&self.out.join("colosseum-satrage-lifts.gif"), &frames, 2 * WIDTH)?;
        self.written.push(("colosseum-satrage-lifts.gif".to_string(), written.bytes, format!("the shifting floor's lifts over two cycles; {} frames", written.frames)));
        Ok(())
    }

    /// The Tunnel Ruins' balance as each of its two entrances sets it up: from
    /// the Great Statue's north (no carved stone on it, both platforms'
    /// weights 3) and from the balance's south (all three set, one on the
    /// west platform and two stacked on the east, weights 2 and 4). The screen
    /// at twice its size, each platform labelled with its weight (its timer)
    /// and the height in pixels gTunnelRuins_BalanceHeights gives it, and
    /// each carved stone with its letter, a panel per entrance, as
    /// `tunnel-ruins-balance.png`.
    pub fn tunnel_ruins_balance(&mut self) -> Result<(), String> {
        let mut panels = Vec::new();
        for (entrance, name) in [(BALANCE_FROM_GREAT_STATUE, "FROM THE GREAT STATUE: NO STONES"), (BALANCE_SOUTH, "BALANCE SOUTH: THREE STONES SET")] {
            let mut entered = self.enter_with((TUNNEL_RUINS_LADY_STATUES, entrance), &[])?;
            let field = &mut entered.field;
            let spot = Self::spot_at(field, (BALANCE_WATCH.0, BALANCE_WATCH.1, 0xc000))?;
            field.stand(&entered.placed, &spot)?;
            let leader = field.read32(self.sym("gGameState")? + 0x1f4) as u16;
            field.hide_actor(leader);
            settle_view(field)?;
            field.frames(30)?;
            let (vx, vy) = field.view();
            let mut image = canvas(2 * WIDTH, 2 * HEIGHT, 0);
            paste(&mut image, &screen(&field.picture()), 0, 0, 2);
            for (id, label) in [(BALANCE_WEST, "W"), (BALANCE_WEST + 1, "E")] {
                let Some(a) = field.actor(id) else { continue };
                let weight = field.read16(field.actor_address(id) + OBJ_TIMER) as i16;
                let (sx, sy) = (2 * (a.x - vx), 2 * (a.z - (a.y >> 16) - vy));
                text(&mut image, sx - 34, sy + 20, &format!("{label} {weight}: {}", a.y >> 16), WHITE, Some(0x40_40_40), 2);
            }
            for (k, letter) in ["A", "B", "C"].iter().enumerate() {
                let Some(a) = field.actor(BALANCE_WEST + 2 + k as u16) else { continue };
                let (sx, sy) = (2 * (a.x - vx), 2 * (a.z - (a.y >> 16) - vy));
                text(&mut image, sx + 14, sy - 30, letter, WHITE, Some(RED), 2);
            }
            panels.push((name, image));
        }
        let mut out = canvas(2 * WIDTH + 8, panels.len() * (2 * HEIGHT + 22) + 4, WHITE);
        for (i, (name, panel)) in panels.iter().enumerate() {
            let y = 4 + i * (2 * HEIGHT + 22);
            text(&mut out, 4, y as i32, name, WHITE, Some(0x40_40_40), 2);
            paste(&mut out, panel, 4, y + 16, 1);
        }
        self.guide_png("tunnel-ruins-balance.png", &out, "the Tunnel Ruins' balance empty and with its three carved stones set")
    }

    /// Venus Lighthouse's beacon erupting, played from the aerie's entrance
    /// after the second battle with A pressed through the lines: from a
    /// second before the beam stacks its five segments (FLAG_MAP_TEMP_36)
    /// for eight seconds, the screen with each
    /// beam segment (LOCALID_BEAM_BOTTOM to LOCALID_BEAM_TOP) numbered 1 to 5 from
    /// the bottom at its place, as `venus-lighthouse-aerie-eruption.gif`;
    /// and the moment five and a half seconds in, the split open and the
    /// colours back, pulled apart into its layers, as
    /// `venus-lighthouse-aerie-layers-3d.gif`.
    pub fn venus_lighthouse_aerie_eruption(&mut self) -> Result<(), String> {
        let stacked = self.flag("FLAG_MAP_TEMP_36")?;
        let set = vec![self.flag("FLAG_VALE_PARTY_LEFT")?];
        let clear = vec![self.flag("FLAG_VALE_STORM")?, self.flag("FLAG_CHILDHOOD")?];
        let mut field = Field::enter(self.rom, self.symbols, &super::field::Setup { scene: VENUS_LIGHTHOUSE_AERIE.0, entrance: VENUS_LIGHTHOUSE_AERIE.1, set, clear })?;
        let scene = field.scene();
        let mut kept: std::collections::VecDeque<Vec<u32>> = std::collections::VecDeque::new();
        let mut frames = Vec::new();
        let mut since: Option<u32> = None;
        for _ in 0..12000 {
            field.frames(1)?;
            if field.waiting() {
                field.press(3)?;
            }
            if field.scene() != scene {
                break;
            }
            let (vx, vy) = field.view();
            let mut image = canvas(WIDTH, HEIGHT, 0);
            paste(&mut image, &screen(&field.picture()), 0, 0, 1);
            if field.flag_is_set(stacked) {
                for (k, id) in (LOCALID_BEAM_BOTTOM..LOCALID_BEAM_BOTTOM + 5).enumerate() {
                    let Some(a) = field.actor(id) else { continue };
                    let (sx, sy) = (a.x - vx, a.z - (a.y >> 16) - vy);
                    text(&mut image, sx + 6, sy - 7, &(k + 1).to_string(), WHITE, Some(RED), 2);
                }
            }
            match since {
                None if field.flag_is_set(stacked) => {
                    since = Some(0);
                    frames.extend(kept.drain(..));
                    frames.push(image.pixels);
                }
                None => {
                    kept.push_back(image.pixels);
                    if kept.len() > 60 {
                        kept.pop_front();
                    }
                }
                Some(n) => {
                    frames.push(image.pixels);
                    since = Some(n + 1);
                    if n + 1 == 330 {
                        self.explode_now(&mut field, "venus-lighthouse-aerie-layers-3d.gif", "Venus Lighthouse's beacon erupting")?;
                    }
                    if n > 480 {
                        break;
                    }
                }
            }
        }
        if since.is_none() {
            return Err("the beacon never erupted".to_string());
        }
        let written = animation::write(&self.out.join("venus-lighthouse-aerie-eruption.gif"), &frames, WIDTH)?;
        self.written.push(("venus-lighthouse-aerie-eruption.gif".to_string(), written.bytes, format!("Venus Lighthouse's beacon erupting; {} frames", written.frames)));
        Ok(())
    }

    /// The credits' names scrolling, beside the sprites' tile memory (32 KB
    /// at OBJ_VRAM, 32 tiles to a row, a tile drawn at its own 8 by 8 pixels
    /// in the colours of the names' sprites): each row of tiles outlined in
    /// red for the 16 frames after its tiles change, as
    /// Credits_DrawEnteringLine draws the line entering, as
    /// `credits-tile-ring.gif`.
    pub fn credits_tile_ring(&mut self) -> Result<(), String> {
        let setup = super::field::Setup { scene: 0, entrance: 9, set: Vec::new(), clear: Vec::new() };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        // From the first frame not black, past the 300-frame lead.
        let mut n = 0;
        while !field.picture().iter().any(|&c| c & 0x7fff != 0) {
            field.frames(1)?;
            n += 1;
            if n > 2000 {
                return Err("the credits never showed".to_string());
            }
        }
        field.frames(280)?;
        let (w, h) = (WIDTH + 8 + 256, 256usize);
        let mut last: Vec<Vec<u8>> = Vec::new();
        let mut marked = [0u32; 32];
        let mut frames = Vec::new();
        for _ in 0..480 {
            field.frames(1)?;
            // The names' palette: that of the first 32 by 8 sprite shown.
            let mut bank = 0usize;
            for i in 0..128u32 {
                let (a0, a1, a2) = (field.read16(OAM_BASE + 8 * i), field.read16(OAM_BASE + 8 * i + 2), field.read16(OAM_BASE + 8 * i + 4));
                if (a0 >> 8) & 3 != 2 && a0 >> 14 == 1 && a1 >> 14 == 1 {
                    bank = usize::from(a2 >> 12);
                    break;
                }
            }
            let colours = super::guide_more::palettes(&field);
            let pal: Vec<u32> = colours[256 + bank * 16..256 + bank * 16 + 16].to_vec();
            let mut c = canvas(w, h, 0x20_20_28);
            paste(&mut c, &screen(&field.picture()), 0, (h - HEIGHT) / 2, 1);
            let mut rows = Vec::new();
            for row in 0..32u32 {
                let bytes: Vec<u8> = (0..1024).map(|k| field.read8(OBJ_VRAM_BASE + row * 1024 + k)).collect();
                if last.get(row as usize).is_some_and(|b| *b != bytes) {
                    marked[row as usize] = 16;
                }
                rows.push(bytes);
                for t in 0..32u32 {
                    let pixels = super::guide_more::tile(&field, OBJ_VRAM_BASE + (row * 32 + t) * 32, false, &pal, 0x20_20_28);
                    super::guide_more::put_tile(&mut c, WIDTH + 8 + t as usize * 8, row as usize * 8, &pixels, false, false);
                }
            }
            for (row, m) in marked.iter_mut().enumerate() {
                if *m > 0 {
                    outline(&mut c, WIDTH as i32 + 8, row as i32 * 8, 256, 8, RED);
                    *m -= 1;
                }
            }
            last = rows;
            frames.push(c.pixels);
        }
        let written = animation::write(&self.out.join("credits-tile-ring.gif"), &frames, w)?;
        self.written.push(("credits-tile-ring.gif".to_string(), written.bytes, format!("the credits beside the sprites' tile memory; {} frames", written.frames)));
        Ok(())
    }

    /// The title intro as it plays after the prologue (the title scene's
    /// entrance 0, Title_ShowIntro without its prompt), 140 frames after the
    /// screen first shows anything, while the backgrounds scroll down and the
    /// sprites rise: pulled apart into its layers, as
    /// `title-intro-layers-3d.gif`.
    pub fn title_intro_exploded(&mut self) -> Result<(), String> {
        let setup = super::field::Setup { scene: 0, entrance: 0, set: Vec::new(), clear: Vec::new() };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        let mut n = 0;
        while !field.picture().iter().any(|&c| c & 0x7fff != 0) {
            field.frames(1)?;
            n += 1;
            if n > 2000 {
                return Err("the intro never showed".to_string());
            }
        }
        field.frames(140)?;
        self.explode_now(&mut field, "title-intro-layers-3d.gif", "the title intro")
    }

    /// The Item screen opened from the field menu in Vale, at twice the GBA's
    /// size at two moments: Isaac's items listed, and an item chosen with its
    /// commands up. Each window the screen's work (struct
    /// ItemMenuWork, in gMenuWork) holds is outlined at its place and
    /// size (struct Window's x, y, width and height, in tiles) and labelled
    /// with the member that holds it, as `item-screen-windows.png`.
    pub fn item_screen_windows(&mut self) -> Result<(), String> {
        let menu_work = self.sym("gMenuWork")?;
        let mut field = self.enter(VALE_SCENE)?;
        let mut panels = Vec::new();
        let steps: [&[(u32, u32)]; 2] = [
            &[(0, 20), (0x1, 4), (0, 40), (0x10, 4), (0, 30), (0x1, 4), (0, 60), (0x1, 4), (0, 60)],
            &[(0x1, 4), (0, 90)],
        ];
        for path in steps {
            for &(keys, n) in path {
                field.hold(keys);
                field.frames(n)?;
            }
            field.hold(0);
            let work = field.read32(menu_work);
            let mut image = canvas(2 * WIDTH, 2 * HEIGHT, 0);
            paste(&mut image, &screen(&field.picture()), 0, 0, 2);
            for (i, &(offset, name)) in ITEM_SCREEN_WINDOWS.iter().enumerate() {
                let window = field.read32(work + offset);
                if window == 0 {
                    continue;
                }
                let (w, h, x, y) = (field.read16(window + 8), field.read16(window + 10), field.read16(window + 12), field.read16(window + 14));
                if w == 0 || h == 0 {
                    continue;
                }
                let colour = super::guide_more::hue(i);
                let (px, py, pw, ph) = (16 * i32::from(x), 16 * i32::from(y), 16 * i32::from(w), 16 * i32::from(h));
                outline(&mut image, px, py, pw, ph, colour);
                outline(&mut image, px + 1, py + 1, pw - 2, ph - 2, colour);
                text(&mut image, px + 3, py + 3, name, WHITE, Some(colour), 2);
            }
            panels.push(image);
        }
        let mut out = canvas(2 * (2 * WIDTH) + 12, 2 * HEIGHT + 8, WHITE);
        for (i, panel) in panels.iter().enumerate() {
            paste(&mut out, panel, 4 + i * (2 * WIDTH + 4), 4, 1);
        }
        self.guide_png("item-screen-windows.png", &out, "the Item screen's windows, each labelled with the ItemMenuWork member that holds it")
    }

    /// A night at Bilibin's inn, the party given 500 coins and A pressed
    /// through the innkeeper's lines as `inn.gif` plays them: a chart
    /// with a column a frame and a row for each track of the eight players
    /// (gMPlayTable: the fanfare player's 4, the music player's 10,
    /// then 2 each), each cell lit while the track is active and has notes
    /// sounding, as bright as its output volume (struct MusicPlayerTrack's
    /// outVolumeA and outVolumeB), as `sound-players.png`.
    pub fn sound_players(&mut self) -> Result<(), String> {
        let slots = self.sym("gMPlayTable")?;
        let (mut entered, _) = self.stand_where(BILIBIN_INN_ROOM, super::story::Trigger::Talk(BILIBIN_INNKEEPER_ACTOR), BILIBIN_INNKEEPER_ACTOR, |s| s.facing == 0xc000)?;
        let field = &mut entered.field;
        field.set_coins(500);
        // The players' tracks: (player, track address, count).
        let players: Vec<(u32, u32)> = (0..8).map(|i| (field.read32(slots + 12 * i + 4), u32::from(field.read8(slots + 12 * i + 8)))).collect();
        let rows: u32 = players.iter().map(|p| p.1).sum();
        // A to talk, then through the innkeeper's lines and the night, as
        // inn.gif plays them: (frame, keys) presses.
        let presses = [20u32, 93, 186, 279, 882];
        let mut columns: Vec<Vec<u32>> = Vec::new();
        let mut shots = Vec::new();
        let total = 1500;
        for f in 0..total {
            field.hold(if presses.iter().any(|&p| (p..p + 3).contains(&f)) { 0x1 } else { 0 });
            field.frames(1)?;
            let mut column = Vec::new();
            for (p, &(tracks, count)) in players.iter().enumerate() {
                for t in 0..count {
                    let track = tracks + 80 * t;
                    let flags = field.read8(track);
                    let sounding = field.read32(track + 0x20) != 0;
                    let level = (u32::from(field.read8(track + 16)) + u32::from(field.read8(track + 17))) / 2;
                    column.push(if flags & 0x80 != 0 && sounding { super::guide_more::mix(0x18_18_20, super::guide_more::hue(p), (f64::from(level) / 96.0).min(1.0)) } else { 0x18_18_20 });
                }
            }
            columns.push(column);
            if f % 30 == 0 {
                shots.push((f, screen(&field.picture())));
            }
        }
        // Three frames to a pixel column, each row as bright as the
        // brightest of the three, so the chart reads at a page's width.
        let per_column = 3usize;
        let brightness = |c: u32| (c >> 16) + ((c >> 8) & 0xff) + (c & 0xff);
        let narrowed: Vec<Vec<u32>> = columns.chunks(per_column).map(|chunk| (0..rows as usize).map(|r| chunk.iter().map(|c| c[r]).max_by_key(|&c| brightness(c)).unwrap_or(0x18_18_20)).collect()).collect();
        let cell = 6usize;
        let label_w = 100usize;
        let gap = 4usize;
        let chart_w = narrowed.len();
        let chart_h = rows as usize * cell + 8 * gap;
        // The screen at a few moments: before talking, the innkeeper's
        // lines, the night, the morning and after.
        let moments = [0u32, 180, 450, 750, 1200];
        let shot_w = WIDTH / 2;
        let shots_w = moments.len() * (shot_w + gap);
        let w = (label_w + chart_w).max(shots_w) + 2 * gap;
        let h = gap + chart_h + 20 + HEIGHT / 2 + 20;
        let mut out = canvas(w, h, WHITE);
        let mut y = gap;
        let mut row = 0usize;
        for (p, &(_, count)) in players.iter().enumerate() {
            let name = match p {
                0 => "0 FANFARE".to_string(),
                1 => "1 MUSIC".to_string(),
                _ => format!("{p}"),
            };
            text(&mut out, gap as i32, y as i32 - 1, &name, 0x20_20_20, None, 2);
            for t in 0..count as usize {
                for (x, column) in narrowed.iter().enumerate() {
                    let c = column[row + t];
                    for dy in 0..cell - 1 {
                        out.pixels[(y + t * cell + dy) * out.width + label_w + x] = c;
                    }
                }
            }
            row += count as usize;
            y += count as usize * cell + gap;
        }
        // A tick every second along the bottom, numbered every five.
        for x in (0..total as usize).step_by(60) {
            let px = (label_w + x / per_column) as i32;
            outline(&mut out, px, y as i32, 1, 5, 0x20_20_20);
            if x % 300 == 0 {
                text(&mut out, px + 2, y as i32 + 2, &format!("{} S", x / 60), 0x20_20_20, None, 2);
            }
        }
        for (i, &at) in moments.iter().enumerate() {
            let shot = &shots.iter().find(|(f, _)| *f == at).ok_or("a moment without its screen")?.1;
            let mut small = canvas(shot_w, HEIGHT / 2, 0);
            for sy in 0..HEIGHT / 2 {
                for sx in 0..shot_w {
                    small.pixels[sy * shot_w + sx] = shot.pixels[2 * sy * WIDTH + 2 * sx];
                }
            }
            let (x, yy) = (gap + i * (shot_w + gap), gap + chart_h + 20);
            paste(&mut out, &small, x, yy, 1);
            text(&mut out, x as i32, (yy + HEIGHT / 2 + 2) as i32, &format!("{} S", at / 60), 0x20_20_20, None, 2);
        }
        self.guide_png("sound-players.png", &out, "the eight sound players' tracks through a night at Bilibin's inn")
    }

    /// The leader walking into a door in Vale until the scene ends: the
    /// twelve request fields of struct FieldWork, in the order the scene
    /// loop tests them, at each change of any of them (several can fall in
    /// one frame), a column a change headed by its frame, nonzero values red;
    /// under the table, the screen at each frame named, as
    /// `field-requests.png`.
    pub fn field_requests(&mut self) -> Result<(), String> {
        let (mut entered, spot) = self.stand_for(VALE_SCENE, super::story::Trigger::Touch(VALE_DOOR), 0)?;
        let field = &mut entered.field;
        let work = field.field_work();
        let addresses: Vec<u32> = REQUESTS.iter().map(|&(offset, _)| work + offset).collect();
        field.hold(spot.walk);
        let changes = field.watch_halfwords(&addresses, 120)?;
        field.hold(0);
        // Every frame while the leader pushes against the door the loop
        // takes `action` and clears it again within the frame: the first
        // three changes show that once, and the last four the door's event,
        // its exit and the scene's end.
        let n = changes.len();
        let kept: Vec<_> = changes.into_iter().enumerate().filter(|&(i, _)| i < 3 || i + 4 >= n).map(|(_, c)| c).collect();
        let mut shots: Vec<(u32, Image)> = Vec::new();
        let mut columns: Vec<(u32, Vec<i16>)> = Vec::new();
        for (f, values, picture) in kept {
            columns.push((f, values.iter().map(|&v| v as i16).collect()));
            // A frame's screen, once: the last picture of it.
            shots.retain(|(g, _)| *g != f);
            shots.push((f, screen(&picture)));
        }
        let gap = 4usize;
        let label_w = 160usize;
        let col_w = 76usize;
        let row_h = 16usize;
        let table_h = 20 + REQUESTS.len() * row_h;
        let table_w = label_w + columns.len() * col_w;
        let shots_w = shots.len() * (WIDTH + gap) - gap;
        let w = table_w.max(shots_w) + 2 * gap;
        let h = gap + table_h + gap + HEIGHT + 20;
        let mut out = canvas(w, h, WHITE);
        for (r, &(_, name)) in REQUESTS.iter().enumerate() {
            text(&mut out, gap as i32, (gap + 20 + r * row_h) as i32, name, 0x20_20_20, None, 2);
        }
        for (c, (f, values)) in columns.iter().enumerate() {
            let x = gap + label_w + c * col_w;
            text(&mut out, x as i32, gap as i32, &format!("FRAME {f}"), 0x20_20_20, None, 2);
            for (r, &v) in values.iter().enumerate() {
                let y = gap + 20 + r * row_h;
                let colour = if v != 0 { RED } else { 0xc0_c0_c0 };
                text(&mut out, x as i32, y as i32, &v.to_string(), colour, None, 2);
            }
        }
        for (i, (f, shot)) in shots.iter().enumerate() {
            let (x, y) = (gap + i * (WIDTH + gap), gap + table_h + gap);
            paste(&mut out, shot, x, y, 1);
            text(&mut out, x as i32, (y + HEIGHT + 2) as i32, &format!("FRAME {f}"), 0x20_20_20, None, 2);
        }
        self.guide_png("field-requests.png", &out, "the scene loop's request fields frame by frame as the leader walks through a door in Vale")
    }

    /// Goma Cave's grid as Tile_ConvertMapGrid leaves it, 128 by 128 cell words
    /// at Ram_MapGridCellBuffer, taken apart into the three planes the words
    /// weave together, each read back out of the words a cell to three
    /// pixels and cropped to the part the map's three layer windows use
    /// (outlined: the ground layer white, the middle green, the overhead
    /// orange, as `grid.png` draws them): the collision codes (byte 2,
    /// coloured as `cells.png` colours them, open ground dark), the shapes
    /// (byte 3, a colour a shape) and the blocks (bits 11..0, a colour a
    /// block); and a stretch of four cells by three around the leader with
    /// each word written in hex over the picture. Two panels a row, so the
    /// picture reads at a page's width, as `grid-planes.png`.
    pub fn grid_planes(&mut self) -> Result<(), String> {
        let mut entered = self.enter_goma_cave(GOMA_CAVE_ROOM)?;
        let field = &mut entered.field;
        let words: Vec<u32> = (0..128 * 128).map(|i| field.read32(MAP_CELL_WORDS + 4 * i)).collect();
        // Each layer's window: its first cell in the grid, and the header's
        // span in cells.
        let header = field.header();
        let (span_x, span_z) = ((header[2] - header[0]) >> 20, (header[3] - header[1]) >> 20);
        let work = field.map_work();
        let windows: Vec<(i32, i32)> = (0..3u32)
            .filter_map(|layer| {
                let cells = field.read32(work + 0x130 + 0x30 * layer);
                let index = (cells.checked_sub(MAP_CELL_WORDS)? / 4) as i32;
                Some((index % 128, index / 128))
            })
            .collect();
        let (gx1, gz1) = windows.iter().fold((0, 0), |(x, z), w| (x.max(w.0 + span_x), z.max(w.1 + span_z)));
        let (gx1, gz1) = (gx1.min(128), gz1.min(128));
        let per = 3usize;
        let (pw, ph) = (gx1 as usize * per, gz1 as usize * per);
        let window_colours = [WHITE, 0x40_e0_60, 0xff_a0_30];
        let plane = |f: &dyn Fn(u32) -> Option<u32>| {
            let mut image = canvas(pw, ph, 0x10_10_14);
            for z in 0..gz1 as usize {
                for x in 0..gx1 as usize {
                    if let Some(c) = f(words[z * 128 + x]) {
                        for dy in 0..per {
                            for dx in 0..per {
                                image.pixels[(z * per + dy) * pw + x * per + dx] = c;
                            }
                        }
                    }
                }
            }
            for (i, &(wx, wz)) in windows.iter().enumerate() {
                for d in 0..2 {
                    outline(&mut image, wx * per as i32 + d, wz * per as i32 + d, span_x * per as i32 - 2 * d, span_z * per as i32 - 2 * d, window_colours[i]);
                }
            }
            image
        };
        let codes = plane(&|w| code_colour(((w >> 16) & 0xff) as i32).or(Some(0x30_30_38)));
        let shapes = plane(&|w| Some(super::guide_more::hue(((w >> 24) & 0xff) as usize)).filter(|_| w >> 24 != 0));
        let blocks = plane(&|w| Some(super::guide_more::hue((w & 0xfff) as usize)).filter(|_| w & 0xfff != 0));
        // The stretch: around the leader, from the map's picture.
        let settled = field.save();
        let (map, origin) = room_map(field, &settled)?;
        field.load(&settled);
        let leader = field.leader().ok_or("no leader")?;
        let (across, down, zoom_by) = (4, 3, 5);
        let (c0, z0) = ((leader.x >> 4) - 2, (leader.z >> 4) - 1);
        let part = crop(&map, c0 * 16 - origin.0, z0 * 16 - origin.1, 16 * across as usize, 16 * down as usize);
        let side = 16 * zoom_by;
        let mut zoom = canvas(side as usize * across as usize, side as usize * down as usize, 0);
        paste(&mut zoom, &part, 0, 0, zoom_by as usize);
        for dz in 0..down {
            for dx in 0..across {
                let w = words[((z0 + dz) * 128 + c0 + dx) as usize];
                let (x, y) = (dx * side, dz * side);
                outline(&mut zoom, x, y, side, side, WHITE);
                text(&mut zoom, x + 3, y + 3, &format!("{:08X}", w), WHITE, Some(0x20_20_20), 2);
            }
        }
        let panels: [(&str, &Image); 4] = [("CODES - BYTE 2", &codes), ("SHAPES - BYTE 3", &shapes), ("BLOCKS - BITS 11 TO 0", &blocks), ("CELL WORDS", &zoom)];
        let (gap, label) = (12usize, 22usize);
        let col_w = [pw.max(zoom.width), pw.max(zoom.width)];
        let row_h = [ph, ph.max(zoom.height)];
        let mut out = canvas(gap + col_w[0] + gap + col_w[1] + gap, gap + label + row_h[0] + gap + label + row_h[1] + gap, WHITE);
        for (i, (name, image)) in panels.iter().enumerate() {
            let x = gap + (i % 2) * (col_w[0] + gap);
            let y = gap + (i / 2) * (label + row_h[0] + gap);
            text(&mut out, x as i32, y as i32, name, 0x20_20_20, None, 2);
            paste(&mut out, image, x, y + label, 1);
        }
        self.guide_png("grid-planes.png", &out, "Goma Cave's grid taken apart into its three planes, and a stretch of its cell words")
    }

    /// Vale where Garet's sister stands, beside the ground layer's character
    /// block (the 512 tiles BG3CNT points at, 32 to a row, each tile at its
    /// own 8 by 8 pixels in grey by colour number), every tile the tile
    /// animation rewrites outlined in red for the 8 frames after it changes,
    /// over four seconds, as `tile-animation.gif`.
    pub fn tile_animation(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.stand_for(VALE_SCENE, super::story::Trigger::Talk(GARETS_SISTER_ACTOR), GARETS_SISTER_ACTOR)?;
        let field = &mut entered.field;
        let control = field.read16(0x0400_000e);
        let base = 0x0600_0000 + u32::from((control >> 2) & 3) * 0x4000;
        let grey: Vec<u32> = (0..16).map(|i| { let v = i * 17; v << 16 | v << 8 | v }).collect();
        let (w, h) = (WIDTH + 8 + 256, 160usize.max(128));
        let mut last: Vec<Vec<u8>> = Vec::new();
        let mut marked = vec![0u32; 512];
        let mut frames = Vec::new();
        for _ in 0..240 {
            field.frames(1)?;
            let mut c = canvas(w, h, 0x20_20_28);
            paste(&mut c, &screen(&field.picture()), 0, 0, 1);
            let mut tiles = Vec::new();
            for t in 0..512u32 {
                let bytes: Vec<u8> = (0..32).map(|k| field.read8(base + t * 32 + k)).collect();
                if last.get(t as usize).is_some_and(|b| *b != bytes) {
                    marked[t as usize] = 8;
                }
                tiles.push(bytes);
                let pixels = super::guide_more::tile(field, base + t * 32, false, &grey, 0);
                super::guide_more::put_tile(&mut c, WIDTH + 8 + (t as usize % 32) * 8, 16 + (t as usize / 32) * 8, &pixels, false, false);
            }
            for (t, m) in marked.iter_mut().enumerate() {
                if *m > 0 {
                    outline(&mut c, (WIDTH + 8 + (t % 32) * 8) as i32, (16 + (t / 32) * 8) as i32, 8, 8, RED);
                    *m -= 1;
                }
            }
            last = tiles;
            frames.push(c.pixels);
        }
        let written = animation::write(&self.out.join("tile-animation.gif"), &frames, w)?;
        self.written.push(("tile-animation.gif".to_string(), written.bytes, format!("Vale beside its ground layer's tiles as the tile animation rewrites them; {} frames", written.frames)));
        Ok(())
    }
}

impl<'a> Guide<'a> {
    /// The world map as the party first leaves Vale, after a short walk
    /// south as `world-map-layers-3d.gif` takes it: the screen at twice its
    /// size beside each scanline's width scale from the map frame's work
    /// (struct WorldMapFrameWork in HEAP_SLOT_WORLD_MAP_FRAME, its WorldMapScanline.scale,
    /// 16.16 fixed point) drawn as a bar on the same line, 0 above the
    /// horizon, as `world-map-scanlines.png`.
    pub fn world_map_scanlines(&mut self) -> Result<(), String> {
        let slots = self.sym("gHeapSlots")?;
        let mut field = self.enter((2, 1))?;
        field.hold(0x80);
        field.frames(50)?;
        field.hold(0);
        field.frames(2)?;
        let frame_work = field.read32(slots + 4 * HEAP_SLOT_WORLD_MAP_FRAME);
        let scales: Vec<i32> = (0..HEIGHT as u32).map(|y| field.read32(frame_work + 20 * y) as i32).collect();
        let most = scales.iter().copied().max().unwrap_or(1).max(1);
        let bars = 240usize;
        let mut out = canvas(2 * WIDTH + 12 + bars, 2 * HEIGHT + 8, WHITE);
        paste(&mut out, &screen(&field.picture()), 4, 4, 2);
        let x0 = 2 * WIDTH + 8;
        for (y, &s) in scales.iter().enumerate() {
            let len = (i64::from(s.max(0)) * bars as i64 / i64::from(most)) as usize;
            for dy in 0..2 {
                for x in 0..len {
                    out.pixels[(4 + 2 * y + dy) * out.width + x0 + x] = 0x30_60_c0;
                }
            }
        }
        text(&mut out, x0 as i32 + 4, 8, "WIDTH SCALE", WHITE, None, 2);
        for (y, s) in [(0usize, scales[0]), (HEIGHT - 1, scales[HEIGHT - 1])] {
            let label = format!("{:.2}", f64::from(s) / 65536.0);
            text(&mut out, x0 as i32 + 4, (4 + 2 * y) as i32 - if y == 0 { -24 } else { 18 }, &label, WHITE, None, 2);
        }
        self.guide_png("world-map-scanlines.png", &out, "the world map beside each scanline's width scale")
    }
}

/// Tret Tree's inside (MapId_TretTreeInteriors) and its cocoons' touch
/// codes, each the number of the entrance breaking through it lands on.
const TRET_TREE_INTERIORS: u16 = 45;
const TRET_COCOONS: [u16; 21] = [21, 22, 23, 24, 25, 26, 27, 28, 29, 31, 32, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59];
// (end)
