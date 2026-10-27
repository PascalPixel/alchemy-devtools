//! `render probe` questions from the place overlays of Vale, Sol Sanctum,
//! Vault, Fuchin Falls Cave and Lunpa Fortress: what a cell becomes, who
//! reads a flag, what an invisible actor blocks. Each probe prints what it
//! read and writes its pictures into the probe's directory.

use super::field::{Field, Setup, Spot};
use super::guide::{Entered, Guide};
use super::guide_more::{canvas, outline, paste, screen};
use super::guide_take::{room_map, text};
use super::mgba::{Gba, HEIGHT, WIDTH};
use super::probe::{actors, cells, describe, walk};
use crate::ld_map::Map;
use std::cell::RefCell;
use std::rc::Rc;

const KEY_RIGHT: u32 = 0x10;
const KEY_LEFT: u32 = 0x20;
const KEY_UP: u32 = 0x40;
const KEY_DOWN: u32 = 0x80;
const NORTH: u32 = 0xc000;
const SOUTH: u32 = 0x4000;
const EAST: u32 = 0x0000;
const WEST: u32 = 0x8000;

const RED: u32 = 0xff_30_30;
const YELLOW: u32 = 0xff_e0_30;
const WHITE: u32 = 0xff_ff_ff;

pub const PROBES: &[&str] = &[
    "fuchin-24",
    "sol-entrance",
    "flag-reads",
    "vale-gate",
    "vault",
    "log-rooms-a",
    "vault-revisit",
    "log-rooms-b",
    "vault-bridge",
    "vale-cave",
    "vault-ivy",
    "maze-exit",
    "kraden-lift",
    "vale-inn",
];

impl<'a> Guide<'a> {
    /// Enters `place` from a new game with the named flags set and cleared,
    /// none of enter_with's own (FLAG_VALE_PARTY_LEFT): the field with the
    /// player in control, and the moment the party was placed.
    pub(super) fn enter_story(&self, place: (u16, u16), set: &[&str], clear: &[&str]) -> Result<Entered<'a>, String> {
        let resolve = |names: &[&str]| -> Result<Vec<u16>, String> {
            names.iter().map(|n| match n.strip_prefix("0x") {
                Some(hex) => u16::from_str_radix(hex, 16).map_err(|e| e.to_string()),
                None => self.flag(n),
            }).collect()
        };
        let setup = Setup { scene: place.0, entrance: place.1, set: resolve(set)?, clear: resolve(clear)? };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        let placed = field.save();
        field.until_control()?;
        field.frames(20)?;
        Ok(Entered { field, placed })
    }

    /// The room the field stands in, its cells in `marks` outlined in their
    /// colour and labelled, as `file`.
    fn marked_room(&mut self, e: &mut Entered, file: &str, marks: &[(i32, i32, u32, String)], what: &str) -> Result<(), String> {
        let settled = e.field.save();
        let (mut map, (ox, oz)) = room_map(&mut e.field, &settled)?;
        e.field.load(&settled);
        for (cx, cz, colour, label) in marks {
            let (sx, sy) = (cx * 16 - ox, cz * 16 - oz);
            outline(&mut map, sx, sy, 16, 16, *colour);
            if !label.is_empty() {
                text(&mut map, sx + 2, sy + 2, label, WHITE, Some(*colour), 1);
            }
        }
        self.guide_png(file, &map, what)
    }

    /// The leader put at `from` on `layer` facing `facing`, then `keys` held
    /// for `frames` frames: each frame at twice its size with the cells in
    /// `marks` outlined and labelled with their code and ground byte on
    /// layer 0, as `file`; prints the leader's track with his height.
    #[allow(clippy::too_many_arguments)]
    fn walk_gif(&mut self, e: &mut Entered, file: &str, from: (i32, i32), layer: u8, facing: u32, keys: u32, frames: u32, marks: &[(i32, i32)]) -> Result<(), String> {
        let placed = e.placed_clone();
        let height = e.field.ground(layer, from.0, from.1)?;
        e.field.stand(&placed, &Spot { x: from.0, z: from.1, layer, height, facing, walk: 0, cell: None })?;
        e.field.hold(keys);
        let mut pictures = Vec::new();
        let mut track = Vec::new();
        for i in 0..frames {
            e.field.frames(1)?;
            let (vx, vy) = e.field.view();
            let mut image = canvas(2 * WIDTH, 2 * HEIGHT, 0);
            paste(&mut image, &screen(&e.field.picture()), 0, 0, 2);
            for &(cx, cz) in marks {
                let word = e.field.cell_word(0, cx, cz) as u32;
                let (code, ground) = ((word >> 16) & 0xff, word >> 24);
                let (sx, sy) = (2 * (cx * 16 - vx), 2 * (cz * 16 - vy));
                let colour = if ground != 0 { RED } else { YELLOW };
                outline(&mut image, sx, sy, 32, 32, colour);
                text(&mut image, sx + 2, sy + 2, &format!("{code}/{ground}"), WHITE, Some(colour), 1);
            }
            pictures.push(image.pixels);
            if i % 8 == 7 {
                if let Some(l) = e.field.leader() {
                    track.push(format!("({}, {}, y {}, layer {})", l.x, l.z, l.y >> 16, l.layer));
                }
            }
        }
        e.field.hold(0);
        println!("{file}: {}", track.join(" "));
        let written = super::animation::write(&self.out.join(file), &pictures.iter().step_by(2).cloned().collect::<Vec<_>>(), 2 * WIDTH)?;
        self.written.push((file.to_string(), written.bytes, format!("{} frames", written.frames)));
        Ok(())
    }

    pub fn probe_place(&mut self, name: &str, map: &Map) -> Result<(), String> {
        let at = |s: &str| map.symbol(s).map(|(_, a)| a & !1).ok_or_else(|| format!("the map has no {s}"));
        match name {
            "fuchin-24" => {
                // FuchinFallsCaveDragonsEye (64): the fourth log put at its
                // lower stop (FLAG_AREA_TEMP_12, which no script sets) with
                // the third at its north end (FLAG_AREA_TEMP_0B), the
                // leader on it walking north and then south.
                for keys in [KEY_UP, KEY_DOWN] {
                    let mut e = self.enter_story((64, 2), &["FLAG_VALE_PARTY_LEFT", "0x312", "0x30b"], &[])?;
                    let placed = e.placed_clone();
                    println!("fourth log before: {}", describe(&e.field, 11));
                    cells(&e.field, 0, (13, 20), (17, 33));
                    let facing = if keys == KEY_UP { NORTH } else { SOUTH };
                    match walk(&mut e.field, &placed, (248, 456), 0, facing, keys, 120) {
                        Ok(s) => println!("walk {keys:#x}: {s}"),
                        Err(err) => println!("walk {keys:#x}: {err}"),
                    }
                    e.field.frames(120)?;
                    println!("fourth log after: {}", describe(&e.field, 11));
                    println!("leader after: {}", describe(&e.field, 0));
                    let image = e.field.screen();
                    super::places::write_png(&self.out.join(format!("fuchin-24-{keys:x}.png")), &image)?;
                }
                Ok(())
            }
            "sol-entrance" => {
                // SolEntrance (19): the cells before the door with and
                // without FLAG_VALE_RETURNED_FROM_MT_ALEPH, and walking north
                // into the door from below them.
                for set in [vec!["FLAG_SOL_SANCTUM_ERUPTED"], vec!["FLAG_SOL_SANCTUM_ERUPTED", "FLAG_VALE_RETURNED_FROM_MT_ALEPH"]] {
                    let mut e = self.enter_story((19, 0), &set, &["FLAG_VALE_PARTY_LEFT"])?;
                    println!("{set:?}: leader {}", describe(&e.field, 0));
                    for layer in 0..2 {
                        cells(&e.field, layer, (0, 0), (14, 16));
                    }
                    let mut marks = Vec::new();
                    for cz in 0..17 {
                        for cx in 0..15 {
                            let code = e.field.cell(0, cx, cz);
                            if code != 0 && code != 255 {
                                marks.push((cx, cz, if (5..=6).contains(&cz) && (6..=8).contains(&cx) { RED } else { YELLOW }, code.to_string()));
                            }
                        }
                    }
                    let tag = if set.len() == 2 { "returned" } else { "before" };
                    self.marked_room(&mut e, &format!("sol-entrance-{tag}.png"), &marks, "Sol Entrance's codes")?;
                    let placed = e.placed_clone();
                    for x in [104, 120, 136] {
                        match walk(&mut e.field, &placed, (x, 120), 0, NORTH, KEY_UP, 150) {
                            Ok(s) => println!("  walk north from ({x}, 120): {s}"),
                            Err(err) => println!("  walk north from ({x}, 120): {err}"),
                        }
                    }
                }
                Ok(())
            }
            "flag-reads" => {
                // Every read of flags 0x100 and 0x1ff (a bit test, or the
                // byte or nibble that holds it) while scenes after the
                // eruption and after the chase in Vault's inn play.
                let tests = [at("GameFlag_Get")?, at("GameFlag_Toggle")?, at("GameFlag_GetByte")?, at("GameFlag_GetNibble")?, at("GameFlag_IncrementByte")?, at("GameFlag_DecrementByte")?];
                let names = ["GameFlag_Get", "GameFlag_Toggle", "GameFlag_GetByte", "GameFlag_GetNibble", "GameFlag_IncrementByte", "GameFlag_DecrementByte"];
                let places: [(u16, u16, &[&str]); 9] = [
                    (11, 0, &["FLAG_SOL_SANCTUM_ERUPTED", "FLAG_SOL_SANCTUM_STAR_ROOM_COLLAPSED"]),
                    (10, 0, &["FLAG_SOL_SANCTUM_ERUPTED", "FLAG_SOL_SANCTUM_STAR_ROOM_COLLAPSED"]),
                    (4, 0, &["FLAG_SOL_SANCTUM_ERUPTED", "FLAG_SOL_SANCTUM_STAR_ROOM_COLLAPSED", "FLAG_VALE_RETURNED_FROM_MT_ALEPH"]),
                    (9, 0, &["FLAG_SOL_SANCTUM_ERUPTED", "FLAG_VALE_RETURNED_FROM_MT_ALEPH"]),
                    (5, 0, &["FLAG_SOL_SANCTUM_ERUPTED", "FLAG_VALE_RETURNED_FROM_MT_ALEPH"]),
                    (2, 0, &["FLAG_VALE_PARTY_LEFT"]),
                    (20, 0, &["FLAG_VALE_PARTY_LEFT"]),
                    (21, 0, &["FLAG_VALE_PARTY_LEFT", "FLAG_VAULT_IVAN_JOINED_ROD_SEARCH", "FLAG_VAULT_SUSPECTS_APPROACHED"]),
                    (27, 5, &["FLAG_VALE_PARTY_LEFT"]),
                ];
                for (scene, entrance, set) in places {
                    let mut all: Vec<&str> = set.to_vec();
                    all.push("0x100");
                    let set: Vec<u16> = all.iter().map(|n| match n.strip_prefix("0x") {
                        Some(hex) => u16::from_str_radix(hex, 16).unwrap_or(0),
                        None => self.flag(n).unwrap_or(0),
                    }).collect();
                    let setup = Setup { scene, entrance, set, clear: Vec::new() };
                    let Ok(field) = Field::enter(self.rom, self.symbols, &setup) else {
                        println!("scene {scene}: cannot enter");
                        continue;
                    };
                    struct Held<'b> {
                        field: Field<'b>,
                    }
                    let mut e = Held { field };
                    let seen = Rc::new(RefCell::new(Vec::new()));
                    let keep = seen.clone();
                    let count = Rc::new(RefCell::new(0u32));
                    let counted = count.clone();
                    e.field.probe_pcs = tests.to_vec();
                    e.field.probe = Some(Box::new(move |gba: &Gba, pc: u32, frame: u32| {
                        let Some(i) = tests.iter().position(|&t| t == pc) else { return };
                        *counted.borrow_mut() += 1;
                        let flag = gba.register(0) & 0xfff;
                        let byte = flag >> 3;
                        let hit = if i <= 1 { flag == 0x100 || flag == 0x1ff } else { byte == 0x20 || byte == 0x3f };
                        if hit {
                            keep.borrow_mut().push(format!("frame {frame}: {}({flag:#x}) from {:08x}", names[i], gba.register(14)));
                        }
                    }));
                    // The scene's setup, then a walk a little way in each
                    // direction, so per-frame and per-step tests run too.
                    if let Err(err) = e.field.until_control() {
                        println!("scene {scene}: {err}");
                    }
                    for keys in [KEY_UP, KEY_LEFT, KEY_DOWN, KEY_RIGHT, 0] {
                        e.field.hold(keys);
                        e.field.frames(90)?;
                    }
                    e.field.hold(0);
                    e.field.probe = None;
                    e.field.probe_pcs.clear();
                    println!("scene {scene} entrance {entrance}: {} flag reads, of 0x100 or 0x1ff: {:?}", count.borrow(), seen.borrow());
                }
                Ok(())
            }
            "vale-gate" => {
                // ValePlaza (5) on the stormy night: the rock, its spot and
                // the boy; walking into the spot with it and without it.
                let storm = ["FLAG_VALE_STORM", "FLAG_CHILDHOOD"];
                let mut e = self.enter_story((5, 0), &storm, &["FLAG_VALE_PARTY_LEFT"])?;
                actors(&e.field, 8..32);
                let placed = e.placed_clone();
                for (from, facing, keys) in [((56, 760), NORTH, KEY_UP), ((24, 729), EAST, KEY_RIGHT), ((88, 729), WEST, KEY_LEFT), ((56, 700), SOUTH, KEY_DOWN)] {
                    match walk(&mut e.field, &placed, from, 0, facing, keys, 60) {
                        Ok(s) => println!("  walk {keys:#x}: {s}"),
                        Err(err) => println!("  walk {keys:#x} from {from:?}: {err}"),
                    }
                }
                for id in [21u16, 22] {
                    let mut e = self.enter_story((5, 0), &storm, &["FLAG_VALE_PARTY_LEFT"])?;
                    let a = e.field.actor_address(id);
                    e.field.write32(a + 8, 0);
                    e.field.write32(a + 0x10, 0);
                    let placed = e.field.save();
                    println!("  without actor {id}:");
                    for (from, facing, keys) in [((56, 760), NORTH, KEY_UP), ((24, 729), EAST, KEY_RIGHT), ((88, 729), WEST, KEY_LEFT)] {
                        match walk(&mut e.field, &placed, from, 0, facing, keys, 60) {
                            Ok(s) => println!("  walk {keys:#x}: {s}"),
                            Err(err) => println!("  walk {keys:#x} from {from:?}: {err}"),
                        }
                    }
                }
                // The sprites alone over the rock's spot, with the spot's
                // actor and without it.
                let mut e = self.enter_story((5, 0), &storm, &["FLAG_VALE_PARTY_LEFT"])?;
                let placed = e.placed_clone();
                let height = e.field.ground(0, 56, 776)?;
                e.field.stand(&placed, &Spot { x: 56, z: 776, layer: 0, height, facing: NORTH, walk: 0, cell: None })?;
                e.field.frames(30)?;
                let with = super::guide_take::alone(&mut e.field, 0x1000)?;
                let whole = screen(&e.field.picture());
                let a = e.field.actor_address(21);
                e.field.write32(a + 8, 0);
                e.field.write32(a + 0x10, 0);
                e.field.frames(2)?;
                let without = super::guide_take::alone(&mut e.field, 0x1000)?;
                let mut c = canvas(3 * WIDTH + 16, HEIGHT + 8, WHITE);
                paste(&mut c, &whole, 4, 4, 1);
                paste(&mut c, &with, 8 + WIDTH, 4, 1);
                paste(&mut c, &without, 12 + 2 * WIDTH, 4, 1);
                self.guide_png("vale-gate-rock-spot.png", &c, "the stormy night's small rock: the screen, its sprites, its sprites without the spot")?;
                // Cell code 11 in each state the scene has.
                for (label, set, clear) in [
                    ("storm", vec!["FLAG_VALE_STORM", "FLAG_CHILDHOOD"], vec!["FLAG_VALE_PARTY_LEFT"]),
                    ("day", vec![], vec!["FLAG_VALE_PARTY_LEFT"]),
                    ("left", vec!["FLAG_VALE_PARTY_LEFT"], vec![]),
                    ("kalay", vec!["FLAG_VALE_PARTY_LEFT", "FLAG_KALAY_REACHED"], vec![]),
                ] {
                    let Ok(mut e) = self.enter_story((5, 0), &set, &clear) else {
                        println!("{label}: cannot enter");
                        continue;
                    };
                    let mut found = Vec::new();
                    for code in [11u16, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12] {
                        found.push(format!("{code}: {:?}", e.field.cells_with_code(code)?));
                    }
                    println!("{label}: {}", found.join("; "));
                }
                Ok(())
            }
            "vault" => {
                // Vault (20): actor 22 (the ivy at (0, 0)) and every actor;
                // the passage cells at (23, 26) as they are and after each
                // touch code; the cells with codes 1 and 2.
                let mut e = self.enter_story((20, 0), &["FLAG_VALE_PARTY_LEFT"], &[])?;
                actors(&e.field, 8..30);
                println!("code 1: {:?}", e.field.cells_with_code(1)?);
                println!("code 2: {:?}", e.field.cells_with_code(2)?);
                for layer in 0..3 {
                    cells(&e.field, layer, (19, 21), (30, 31));
                    super::probe::words(&e.field, layer, (21, 23), (28, 29));
                }
                for layer in 0..3 {
                    super::probe::words(&e.field, layer, (53, 24), (60, 29));
                }
                let mut marks = Vec::new();
                for code in [1u16, 2] {
                    for (_, cx, cz) in e.field.cells_with_code(code)? {
                        marks.push((cx, cz, if code == 1 { RED } else { YELLOW }, code.to_string()));
                    }
                }
                for cz in 26..28 {
                    for cx in 23..27 {
                        marks.push((cx, cz, 0x30_80_f0, String::new()));
                    }
                }
                self.marked_room(&mut e, "vault-passage.png", &marks, "Vault's passage cells")?;
                Ok(())
            }
            "vault-bridge" => {
                // Vault (20): across the rope bridge from each end, and
                // north along the path below it, the passage cells and the
                // touch cells outlined with their code and ground.
                let mut marks = vec![(21, 25), (21, 26), (21, 27), (22, 26), (27, 26)];
                for cz in 26..28 {
                    for cx in 23..27 {
                        marks.push((cx, cz));
                    }
                }
                let mut e = self.enter_story((20, 0), &["FLAG_VALE_PARTY_LEFT"], &[])?;
                for (file, from, facing, keys) in [
                    ("vault-bridge-west.gif", (456, 424), WEST, KEY_LEFT),
                    ("vault-bridge-east.gif", (312, 424), EAST, KEY_RIGHT),
                    ("vault-bridge-north.gif", (392, 488), NORTH, KEY_UP),
                    ("vault-bridge-south.gif", (392, 376), SOUTH, KEY_DOWN),
                ] {
                    if let Err(err) = self.walk_gif(&mut e, file, from, 0, facing, keys, 150, &marks) {
                        println!("{file}: {err}");
                    }
                }
                Ok(())
            }
            "kraden-lift" => {
                // ValeKradensCottage (6): Lift cast on the boulder by the fence
                // (actor 8), in the day's table and after the party left.
                for (label, set, clear) in [("day", vec![], vec!["FLAG_VALE_PARTY_LEFT"]), ("left", vec!["FLAG_VALE_PARTY_LEFT"], vec![])] {
                    let mut e = self.enter_story((6, 0), &set, &clear)?;
                    println!("{label}: {}", describe(&e.field, 8));
                    let before = e.field.last_message();
                    let spot = Self::spot_at(&mut e.field, (72, 236, SOUTH))?;
                    match Self::record_trigger(&mut e, super::story::Trigger::Psynergy(0x8f, 8), Some(spot), 60) {
                        Ok(frames) => {
                            println!("  {} frames; last message {:#x} (was {before:#x}); boulder after {}", frames.len(), e.field.last_message(), describe(&e.field, 8));
                            let rgb: Vec<Vec<u32>> = frames.iter().map(|f| f.iter().map(|&c| super::animation::rgb(c)).collect()).collect();
                            let file = format!("kraden-lift-{label}.gif");
                            let written = super::animation::write(&self.out.join(&file), &rgb, WIDTH)?;
                            self.written.push((file, written.bytes, "Lift on the fence boulder".to_string()));
                        }
                        Err(err) => println!("  {err}"),
                    }
                }
                Ok(())
            }
            "maze-exit" => {
                // SolSanctumMaze (14): the south exit's cells (code 1), and
                // the leader walking onto them from each side, recorded until
                // the next scene is in.
                for entrance in [1u16, 2] {
                    let mut e = self.enter_story((14, entrance), &["FLAG_SOL_SANCTUM_ENTERED"], &["FLAG_VALE_PARTY_LEFT"])?;
                    let c1 = e.field.cells_with_code(1)?;
                    let c2 = e.field.cells_with_code(2)?;
                    println!("entrance {entrance}: leader {}; code 1 {c1:?}; code 2 {c2:?}", describe(&e.field, 0));
                }
                let mut e = self.enter_story((14, 1), &["FLAG_SOL_SANCTUM_ENTERED"], &["FLAG_VALE_PARTY_LEFT"])?;
                let c1 = e.field.cells_with_code(1)?;
                let Some(&(_, cx, cz)) = c1.first() else { return Err("no code 1".into()) };
                for (dir, (dx, dz), facing, keys) in [("north", (0, 1), NORTH, KEY_UP), ("south", (0, -1), SOUTH, KEY_DOWN), ("east", (-1, 0), EAST, KEY_RIGHT), ("west", (1, 0), WEST, KEY_LEFT)] {
                    let (x, z) = ((cx + dx) * 16 + 8, (cz + dz) * 16 + 8);
                    if e.field.cell(0, cx + dx, cz + dz) == 255 {
                        continue;
                    }
                    let placed = e.placed_clone();
                    let height = e.field.ground(0, x, z)?;
                    if e.field.stand(&placed, &Spot { x, z, layer: 0, height, facing, walk: 0, cell: None }).is_err() {
                        continue;
                    }
                    e.field.hold(keys);
                    let mut frames = Vec::new();
                    let start = e.field.scene();
                    let mut left = None;
                    for i in 0..400 {
                        e.field.frames(1)?;
                        if i == 20 {
                            e.field.hold(0);
                        }
                        frames.push(e.field.picture());
                        if left.is_none() && e.field.scene() != start {
                            left = Some(i);
                        }
                        if left.is_some_and(|l| i > l + 120) {
                            break;
                        }
                    }
                    println!("from the {dir} ({x}, {z}): scene {} after {:?} frames", e.field.scene(), left);
                    let rgb: Vec<Vec<u32>> = frames.iter().map(|f| f.iter().map(|&c| super::animation::rgb(c)).collect()).collect();
                    let file = format!("maze-exit-{dir}.gif");
                    let written = super::animation::write(&self.out.join(&file), &rgb, WIDTH)?;
                    self.written.push((file, written.bytes, "the maze's south exit".to_string()));
                    break;
                }
                Ok(())
            }
            "vault-ivy" => {
                // Vault (20) and VaultRevisit (23): every lookup of the ivy
                // placed at (0, 0) (actor 22 and 20) from the scene's start,
                // walking, ringing the bell and casting Whirlwind.
                let get = at("Actor_Lookup").or_else(|_| at("ObjectEvent_GetByLocalId"))?;
                for (scene, id, set) in [(20u16, 22u32, vec!["FLAG_VALE_PARTY_LEFT"]), (23, 20, vec!["FLAG_VALE_PARTY_LEFT", "FLAG_KALAY_REACHED"])] {
                    let flags: Vec<u16> = set.iter().map(|n| self.flag(n).unwrap_or(0)).collect();
                    let setup = Setup { scene, entrance: 0, set: flags, clear: Vec::new() };
                    let mut field = Field::enter(self.rom, self.symbols, &setup)?;
                    let placed = field.save();
                    let seen = Rc::new(RefCell::new(Vec::new()));
                    let keep = seen.clone();
                    let phase = Rc::new(RefCell::new("setup"));
                    let now = phase.clone();
                    field.probe_pcs = vec![get];
                    field.probe = Some(Box::new(move |gba: &Gba, pc: u32, frame: u32| {
                        if pc == get && gba.register(0) == id {
                            keep.borrow_mut().push(format!("{} frame {frame}: from {:08x}", now.borrow(), gba.register(14)));
                        }
                    }));
                    field.until_control()?;
                    println!("scene {scene}: actor {id}: {}", describe(&field, id as u16));
                    *phase.borrow_mut() = "walk";
                    for keys in [KEY_UP, KEY_LEFT, KEY_DOWN, KEY_RIGHT] {
                        field.hold(keys);
                        field.frames(60)?;
                    }
                    field.hold(0);
                    let mut e = Entered { field, placed };
                    *phase.borrow_mut() = "bell";
                    match Self::record_trigger(&mut e, super::story::Trigger::Search(30), None, 30) {
                        Ok(f) => println!("  bell: {} frames", f.len()),
                        Err(err) => println!("  bell: {err}"),
                    }
                    *phase.borrow_mut() = "whirlwind";
                    super::guide_take::cast(&mut e.field, 0x4e);
                    e.field.frames(240)?;
                    e.field.probe = None;
                    println!("  lookups of actor {id}: {:?}", seen.borrow());
                }
                Ok(())
            }
            "vale-cave" => {
                // ValeCaveKite (94): the still crate at (120, 232) and the
                // still chest at (642, 552), each with the sprites alone.
                for (entrance, (x, z), file) in [(1u16, (120, 232), "vale-cave-crate.png"), (8, (642, 552), "vale-cave-chest.png")] {
                    let mut e = self.enter_story((94, entrance), &["FLAG_VALE_PARTY_LEFT"], &[])?;
                    for a in (0x0200_0000u32..0x0204_0000).step_by(4) {
                        if e.field.read32(a) == (x as u32) << 16 && e.field.read32(a + 8) == (z as u32) << 16 {
                            let o = a - 8;
                            println!("{file}: object at {o:08x}: kind {} sprites {:?} priority {:#04x} collision {:#04x}", e.field.read8(o + 0x54) & 15, e.field.sprite_ids(o), e.field.read8(o + 0x23), e.field.read8(o + 0x59));
                        }
                    }
                    let placed = e.placed_clone();
                    let (sx, sz) = (x + 8, z + 40);
                    let height = e.field.ground(0, sx, sz).unwrap_or(0);
                    if let Err(err) = e.field.stand(&placed, &Spot { x: sx, z: sz, layer: 0, height, facing: NORTH, walk: 0, cell: None }) {
                        println!("{file}: {err}");
                    }
                    e.field.frames(30)?;
                    println!("{file}: view {:?}", e.field.view());
                    let mut c = canvas(4 * WIDTH + 12, 2 * HEIGHT + 8, WHITE);
                    paste(&mut c, &screen(&e.field.picture()), 4, 4, 2);
                    let objs = super::guide_take::alone(&mut e.field, 0x1000)?;
                    paste(&mut c, &objs, 8 + 2 * WIDTH, 4, 2);
                    self.guide_png(file, &c, "the still object and the sprites alone")?;
                }
                Ok(())
            }
            "log-rooms-a" => {
                // LogRoomsA (21), the guests' room upstairs at the inn:
                // the cells with LOG_ROOMS_A_CELL_GUESTS_ROOM (10) and
                // the row the events rewrite, in each state of the chase.
                for (label, set) in [
                    ("before", vec!["FLAG_VALE_PARTY_LEFT", "FLAG_VAULT_IVAN_JOINED_ROD_SEARCH"]),
                    ("approached", vec!["FLAG_VALE_PARTY_LEFT", "FLAG_VAULT_IVAN_JOINED_ROD_SEARCH", "FLAG_VAULT_SUSPECTS_APPROACHED"]),
                ] {
                    let entrance = 12u16;
                    let Ok(mut e) = self.enter_story((21, entrance), &set, &[]) else {
                        println!("{label}: cannot enter");
                        continue;
                    };
                    println!("{label} (entrance {entrance}): code 10 at {:?}", e.field.cells_with_code(10)?);
                    for layer in 0..2 {
                        cells(&e.field, layer, (10, 40), (20, 51));
                    }
                }
                Ok(())
            }
            "vault-revisit" => {
                // VaultRevisit (23): configured object 223 at (128, 420) and
                // the 3-by-3 block copied from (45, 41) to (8, 45), the room
                // before the copy and after it.
                let mut e = self.enter_story((23, 0), &["FLAG_VALE_PARTY_LEFT", "FLAG_KALAY_REACHED"], &[])?;
                println!("header {:?} room {:?}", e.field.header(), e.field.room());
                for a in (0x0200_0000u32..0x0204_0000).step_by(4) {
                    if e.field.read32(a) == 128 << 16 && e.field.read32(a + 8) == 420 << 16 {
                        let o = a - 8;
                        println!(
                            "object at {o:08x}: kind {} sprite {:08x} sprites {:?} priority {:#04x} collision {:#04x} words {:08x} {:08x} {:08x} {:08x}",
                            e.field.read8(o + 0x54) & 15,
                            e.field.read32(o + 0x50),
                            e.field.sprite_ids(o),
                            e.field.read8(o + 0x23),
                            e.field.read8(o + 0x59),
                            e.field.read32(o),
                            e.field.read32(o + 4),
                            e.field.read32(o + 0x18),
                            e.field.read32(o + 0x1c)
                        );
                    }
                }
                {
                    // The well, with the sprites alone beside the screen.
                    let placed = e.placed_clone();
                    let height = e.field.ground(0, 160, 470)?;
                    e.field.stand(&placed, &Spot { x: 160, z: 470, layer: 0, height, facing: NORTH, walk: 0, cell: None })?;
                    let mut frames = Vec::new();
                    for _ in 0..180 {
                        e.field.frames(1)?;
                        let mut c = canvas(4 * WIDTH + 12, 2 * HEIGHT + 8, WHITE);
                        paste(&mut c, &screen(&e.field.picture()), 4, 4, 2);
                        let objs = super::guide_take::alone(&mut e.field, 0x1000)?;
                        paste(&mut c, &objs, 8 + 2 * WIDTH, 4, 2);
                        frames.push(c.pixels);
                    }
                    let written = super::animation::write(&self.out.join("vault-revisit-well.gif"), &frames.iter().step_by(2).cloned().collect::<Vec<_>>(), 4 * WIDTH + 12)?;
                    self.written.push(("vault-revisit-well.gif".to_string(), written.bytes, "the well and its sprites".to_string()));
                    println!("view {:?}", e.field.view());
                    for i in 0..128u32 {
                        let a0 = e.field.read16(0x0700_0000 + 8 * i);
                        let a1 = e.field.read16(0x0700_0000 + 8 * i + 2);
                        let a2 = e.field.read16(0x0700_0000 + 8 * i + 4);
                        if a0 & 0x300 == 0x200 {
                            continue;
                        }
                        println!("  oam {i}: mode {} y {} x {} shape {} size {} tile {} priority {} palette {}", (a0 >> 10) & 3, a0 & 0xff, a1 & 0x1ff, a0 >> 14, a1 >> 14, a2 & 0x3ff, (a2 >> 10) & 3, a2 >> 12);
                    }
                    let pal: Vec<String> = (0..16).map(|k| format!("{:04x}", e.field.read16(0x0500_0200 + 15 * 32 + 2 * k))).collect();
                    println!("  OBJ palette 15: {}", pal.join(" "));
                    let nonzero = (0..32 * 32u32).filter(|k| e.field.read32(0x0601_0000 + 420 * 32 + 4 * k) != 0).count();
                    println!("  tiles 420.. (32 tiles): {nonzero} of 1024 words non-zero; DISPCNT {:04x} WININ {:04x} WINOUT {:04x} BLDCNT {:04x}", e.field.read16(0x0400_0000), e.field.read16(0x0400_0048), e.field.read16(0x0400_004a), e.field.read16(0x0400_0050));
                }
                {
                    let work = e.field.map_work();
                    let base = at("gMapGridCellBuffer")?;
                    for l in 0..3u32 {
                        let p = e.field.read32(work + 0x130 + 0x30 * l);
                        let w = p.wrapping_sub(base) / 4;
                        println!("layer {l} window: column {}, row {}", w % 128, w / 128);
                    }
                }
                {
                    let settled = e.field.save();
                    let below = e.field.stitch(&settled, [0, 512 << 16, 512 << 16, 1024 << 16], false)?;
                    e.field.load(&settled);
                    self.guide_png("vault-revisit-below.png", &below, "the grid below Vault's view")?;
                }
                for layer in 0..3 {
                    super::probe::words(&e.field, layer, (6, 43), (12, 49));
                    super::probe::words(&e.field, layer, (43, 39), (49, 45));
                }
                let mut marks = Vec::new();
                for cz in 45..48 {
                    for cx in 8..11 {
                        marks.push((cx, cz, RED, String::new()));
                    }
                }
                marks.push((8, 26, YELLOW, "223".to_string()));
                self.marked_room(&mut e, "vault-revisit-after.png", &marks, "Vault after Kalay, the copied block outlined")?;
                // Before: put the source's old cells back by entering Vault
                // (20), whose initialization does not copy them.
                let mut v = self.enter_story((20, 0), &["FLAG_VALE_PARTY_LEFT"], &[])?;
                for layer in 0..3 {
                    super::probe::words(&v.field, layer, (6, 43), (12, 49));
                }
                self.marked_room(&mut v, "vault-before.png", &marks, "Vault before Kalay, the same cells outlined")?;
                Ok(())
            }
            "log-rooms-b" => {
                // LogRoomsB (24): the gem owner's house, its
                // cells at (8, 67) and the copy's source at (0, 120).
                let e = self.enter_story((24, 0), &["FLAG_VALE_PARTY_LEFT", "FLAG_KALAY_REACHED"], &[])?;
                let entrance = {
                    let ents = e.field.entrances();
                    ents.iter().map(|x| (x.number, x.x, x.z)).collect::<Vec<_>>()
                };
                println!("entrances: {entrance:?}");
                drop(e);
                let mut e = self.enter_story((24, 5), &["FLAG_VALE_PARTY_LEFT", "FLAG_KALAY_REACHED"], &[])?;
                println!("leader {}", describe(&e.field, 0));
                {
                    let work = e.field.map_work();
                    let base = at("gMapGridCellBuffer")?;
                    for l in 0..3u32 {
                        let p = e.field.read32(work + 0x130 + 0x30 * l);
                        let w = p.wrapping_sub(base) / 4;
                        println!("layer {l} window: column {}, row {}", w % 128, w / 128);
                    }
                }
                actors(&e.field, 8..20);
                for layer in 0..3 {
                    super::probe::words(&e.field, layer, (6, 65), (13, 71));
                    super::probe::words(&e.field, layer, (0, 119), (4, 123));
                }
                let mut marks = Vec::new();
                for cz in 67..70 {
                    for cx in 8..12 {
                        marks.push((cx, cz, RED, String::new()));
                    }
                }
                self.marked_room(&mut e, "log-rooms-b-after.png", &marks, "the gem owner's house after Kalay")?;
                // The same room before Kalay (LogRoomsA, 21).
                let entrance = 1u16;
                let mut v = self.enter_story((21, entrance), &["FLAG_VALE_PARTY_LEFT"], &[])?;
                for layer in 0..3 {
                    super::probe::words(&v.field, layer, (6, 65), (13, 71));
                }
                self.marked_room(&mut v, "log-rooms-a-before.png", &marks, "the gem owner's house before Kalay")?;
                Ok(())
            }
            "vale-inn" => {
                // CottageRoomsB (8) after Kalay: ACTOR_INVISIBLE_AFTER_KALAY
                // and its neighbours, and walking into it.
                let ents = {
                    let e = self.enter_story((8, 0), &["FLAG_VALE_PARTY_LEFT", "FLAG_KALAY_REACHED"], &[])?;
                    e.field.entrances().iter().map(|x| (x.number, x.x, x.z)).collect::<Vec<_>>()
                };
                println!("entrances: {ents:?}");
                let after = ["FLAG_VALE_PARTY_LEFT", "FLAG_KALAY_REACHED"];
                let entrance = 5u16;
                {
                    let mut e = self.enter_story((8, entrance), &after, &[])?;
                    let mut marks = Vec::new();
                    for cz in 17..30 {
                        for cx in 9..20 {
                            let code = e.field.cell(0, cx, cz);
                            if code != 0 {
                                marks.push((cx, cz, if code == 255 { RED } else { YELLOW }, code.to_string()));
                            }
                        }
                    }
                    for id in 13u16..18 {
                        if let Some(a) = e.field.actor(id) {
                            marks.push((a.x >> 4, a.z >> 4, 0x30_80_f0, format!("A{id}")));
                        }
                    }
                    self.marked_room(&mut e, "vale-inn-codes.png", &marks, "Vale's inn after Kalay, its codes")?;
                }
                for remove in [false, true] {
                    let mut e = self.enter_story((8, entrance), &after, &[])?;
                    let tag = if remove { "without" } else { "with" };
                    if remove {
                        let a = e.field.actor_address(17);
                        e.field.write32(a + 8, 0);
                        e.field.write32(a + 0x10, 0);
                        e.placed = e.field.save();
                    }
                    let marks = [(15, 23), (13, 23), (16, 23), (15, 22), (15, 24), (14, 23)];
                    for (dir, from, facing, keys) in [("north", (258, 398), NORTH, KEY_UP), ("northwest", (266, 400), NORTH, KEY_UP | KEY_LEFT), ("south", (252, 345), SOUTH, KEY_DOWN)] {
                        let file = format!("vale-inn-{tag}-{dir}.gif");
                        if let Err(err) = self.walk_gif(&mut e, &file, from, 0, facing, keys, 120, &marks) {
                            println!("{file}: {err}");
                        }
                    }
                }
                for remove in [false, true] {
                    let mut e = self.enter_story((8, entrance), &after, &[])?;
                    if remove {
                        let a = e.field.actor_address(17);
                        e.field.write32(a + 8, 0);
                        e.field.write32(a + 0x10, 0);
                        e.placed = e.field.save();
                    } else {
                        println!("entrance {entrance}:");
                        actors(&e.field, 8..20);
                        cells(&e.field, 0, (11, 20), (19, 27));
                        let marks = vec![(15, 23, RED, "B".to_string()), (13, 23, YELLOW, "W".to_string()), (16, 23, YELLOW, "M".to_string())];
                        self.marked_room(&mut e, "vale-inn-blank.png", &marks, "Vale's inn after Kalay")?;
                    }
                    let placed = e.placed_clone();
                    println!("  without the blank actor: {remove}");
                    for (from, facing, keys) in [((252, 420), NORTH, KEY_UP), ((252, 340), SOUTH, KEY_DOWN), ((240, 410), NORTH, KEY_UP), ((240, 340), SOUTH, KEY_DOWN), ((230, 378), EAST, KEY_RIGHT), ((290, 378), WEST, KEY_LEFT)] {
                        match walk(&mut e.field, &placed, from, 0, facing, keys, 60) {
                            Ok(s) => println!("  walk {keys:#x}: {s}"),
                            Err(err) => println!("  walk {keys:#x} from {from:?}: {err}"),
                        }
                    }
                }
                Ok(())
            }
            other => Err(format!("no probe {other}")),
        }
    }
}
