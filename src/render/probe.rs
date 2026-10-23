//! `render probe`: questions only the running game can settle, asked of a
//! built edition: what an actor slot holds, whether the leader can walk
//! through an actor, a map's cells before and after an arrival, a pushed
//! log's path, a render with one function stubbed out, and the heap's
//! high-water marks. Each probe prints what it read.

use super::field::{Field, Spot};
use super::guide::Guide;
use super::mgba::Gba;
use super::story::{Trigger, MOVE};
use crate::ld_map::Map;
use std::cell::RefCell;
use std::rc::Rc;

const KEY_A: u32 = 0x1;
const KEY_B: u32 = 0x2;
const KEY_RIGHT: u32 = 0x10;
const KEY_LEFT: u32 = 0x20;
const KEY_UP: u32 = 0x40;
const KEY_DOWN: u32 = 0x80;

pub const PROBES: &[&str] = &["suhalla", "lunpa", "gondowan", "gondowan-walk", "gondowan-look", "gondowan-layers", "gondowan-move", "gondowan-set", "fuchin", "venus", "heap", "heap-battle"];

const NORTH: u32 = 0xc000;
const SOUTH: u32 = 0x4000;
const EAST: u32 = 0x0000;
const WEST: u32 = 0x8000;

fn word(field: &Field, a: u32) -> i32 {
    field.read32(a) as i32
}

/// One actor's object as the engine keeps it (struct ObjectEvent).
pub fn describe(field: &Field, id: u16) -> String {
    let a = field.actor_address(id);
    if a == 0 {
        return format!("actor {id}: none");
    }
    format!(
        "actor {id} @{a:08x}: x {} z {} y {} radius {} terrain {} priority {:#04x} scale {:#x}/{:#x} kind {} motion {:#04x} collision {:#04x} sprites {:?}",
        word(field, a + 8) >> 16,
        word(field, a + 0x10) >> 16,
        word(field, a + 0x0c) as f64 / 65536.0,
        field.read16(a + 0x20),
        field.read8(a + 0x22),
        field.read8(a + 0x23),
        field.read32(a + 0x18),
        field.read32(a + 0x1c),
        field.read8(a + 0x54) & 15,
        field.read8(a + 0x55),
        field.read8(a + 0x59),
        field.sprite_ids(a)
    )
}

pub(super) fn actors(field: &Field, ids: std::ops::Range<u16>) {
    for id in ids {
        println!("  {}", describe(field, id));
    }
}

/// The cells of layer `layer` from (x0, z0) to (x1, z1) as their collision
/// codes, and their whole words.
pub(super) fn cells(field: &Field, layer: u8, (x0, z0): (i32, i32), (x1, z1): (i32, i32)) {
    println!("  layer {layer} codes, columns {x0}..={x1}:");
    for z in z0..=z1 {
        let row: Vec<String> = (x0..=x1).map(|x| format!("{:3}", field.cell(layer, x, z))).collect();
        println!("   {z:3}: {}", row.join(" "));
    }
}

pub(super) fn words(field: &Field, layer: u8, (x0, z0): (i32, i32), (x1, z1): (i32, i32)) {
    println!("  layer {layer} words, columns {x0}..={x1}:");
    for z in z0..=z1 {
        let row: Vec<String> = (x0..=x1).map(|x| format!("{:08x}", field.cell_word(layer, x, z) as u32)).collect();
        println!("   {z:3}: {}", row.join(" "));
    }
}

/// Puts the leader at (x, z) on `layer` facing `facing`, then holds `keys`
/// for `frames` frames; where he started and where he ended up.
pub(super) fn walk(field: &mut Field, placed: &super::field::Saved, (x, z): (i32, i32), layer: u8, facing: u32, keys: u32, frames: u32) -> Result<String, String> {
    let height = field.ground(layer, x, z)?;
    let spot = Spot { x, z, layer, height, facing, walk: 0, cell: None };
    field.stand(placed, &spot)?;
    let start = field.leader().ok_or("no leader")?;
    field.hold(keys);
    let mut track = Vec::new();
    for i in 0..frames {
        field.frames(1)?;
        if i % 8 == 7 {
            let l = field.leader().ok_or("no leader")?;
            track.push(format!("({}, {})", l.x, l.z));
        }
    }
    field.hold(0);
    field.frames(4)?;
    let end = field.leader().ok_or("no leader")?;
    Ok(format!("from ({}, {}) layer {} to ({}, {}) layer {}; scene {}; {}", start.x, start.z, start.layer, end.x, end.z, end.layer, field.scene(), track.join(" ")))
}

impl<'a> Guide<'a> {
    pub fn probe(&mut self, name: &str, map: &Map) -> Result<(), String> {
        let at = |s: &str| map.symbol(s).map(|(_, a)| a & !1).ok_or_else(|| format!("the map has no {s}"));
        match name {
            "suhalla" => {
                // SuhallaGateDew (scene 170) from each of its entrances.
                for entrance in 0..5u16 {
                    let mut e = self.enter_with((170, entrance), &[])?;
                    println!("SuhallaGateDew entrance {entrance}:");
                    actors(&e.field, 0..20);
                    let work = e.field.read32(at("gFieldWork")?);
                    println!("  slot 15 word (gFieldWork+0x14+60): {:08x}", e.field.read32(work + 0x14 + 60));
                    if entrance == 0 {
                        // Again from the party's placing, watching every
                        // actor lookup of slot 15 as the scene starts.
                        let get = at("ObjectEvent_GetByLocalId")?;
                        let ew_sym = at("gFieldWork")?;
                        e.field.load(&e.placed);
                        e.field.probe_pcs = vec![get];
                        let seen = Rc::new(RefCell::new(Vec::new()));
                        let keep = seen.clone();
                        e.field.probe = Some(Box::new(move |gba: &Gba, pc: u32, frame: u32| {
                            if pc != 0 && gba.register(0) == 15 {
                                let work = gba.read32(ew_sym);
                                keep.borrow_mut().push(format!("frame {frame}: ObjectEvent_GetByLocalId(15) from {:08x}, slot word {:08x}", gba.register(14), gba.read32(work + 0x14 + 60)));
                            }
                        }));
                        e.field.until_control()?;
                        e.field.probe = None;
                        for s in seen.borrow().iter().take(10) {
                            println!("  {s}");
                        }
                        println!("  word at 0x1c after: {:08x}", e.field.read32(0x1c));
                        e.field.frames(60)?;
                        println!("  a second later:");
                        actors(&e.field, 14..17);
                    }
                }
                Ok(())
            }
            "lunpa" => {
                // LunpaFortressCave (162) and LunpaFortressToadonpa (163).
                for (scene, ids) in [(162u16, 8u16..13u16), (163, 8..16)] {
                    let e = self.enter_with((scene, 0), &[])?;
                    println!("scene {scene} entrance 0:");
                    actors(&e.field, ids);
                }
                let mut e = self.enter_with((162, 0), &[])?;
                let placed = e.placed_clone();
                println!("cave cells around (504, 168) = cell (31, 10):");
                for layer in 0..3 {
                    cells(&e.field, layer, (27, 7), (35, 13));
                }
                for (from, facing, keys) in [((504, 200), NORTH, KEY_UP), ((472, 168), EAST, KEY_RIGHT), ((536, 168), WEST, KEY_LEFT), ((504, 136), SOUTH, KEY_DOWN)] {
                    for layer in 0..2u8 {
                        match walk(&mut e.field, &placed, from, layer, facing, keys, 48) {
                            Ok(s) => println!("  walk {keys:#x} {s}"),
                            Err(err) => println!("  walk {keys:#x} from {from:?} layer {layer}: {err}"),
                        }
                    }
                }
                // Again with the invisible actor taken away.
                let mut e = self.enter_with((162, 0), &[])?;
                let a = e.field.actor_address(11);
                e.field.write32(a + 8, 0);
                e.field.write32(a + 0x10, 0);
                let placed = e.field.save();
                println!("  without actor 11:");
                for (from, facing, keys) in [((504, 200), NORTH, KEY_UP), ((472, 168), EAST, KEY_RIGHT), ((536, 168), WEST, KEY_LEFT)] {
                    for layer in 0..2u8 {
                        match walk(&mut e.field, &placed, from, layer, facing, keys, 48) {
                            Ok(s) => println!("  walk {keys:#x} {s}"),
                            Err(err) => println!("  walk {keys:#x} from {from:?} layer {layer}: {err}"),
                        }
                    }
                }
                let mut e = self.enter_with((163, 0), &[])?;
                let placed = e.placed_clone();
                println!("Toadonpa cells around (56..216, 72), rows 2..7:");
                for layer in 0..3 {
                    cells(&e.field, layer, (1, 2), (15, 7));
                }
                for x in [56, 136, 216] {
                    for (from, facing, keys) in [((x, 104), NORTH, KEY_UP), ((x - 32, 72), EAST, KEY_RIGHT), ((x + 32, 72), WEST, KEY_LEFT)] {
                        for layer in 0..2u8 {
                            match walk(&mut e.field, &placed, from, layer, facing, keys, 48) {
                                Ok(s) => println!("  walk {keys:#x} {s}"),
                                Err(err) => println!("  walk {keys:#x} from {from:?} layer {layer}: {err}"),
                            }
                        }
                    }
                }
                let mut e = self.enter_with((163, 0), &[])?;
                for id in 8..11u16 {
                    let a = e.field.actor_address(id);
                    e.field.write32(a + 8, 0);
                    e.field.write32(a + 0x10, 0);
                }
                let placed = e.field.save();
                println!("  without actors 8 to 10:");
                for x in [56, 136, 216] {
                    for (from, facing, keys) in [((x, 104), NORTH, KEY_UP), ((x - 32, 72), EAST, KEY_RIGHT)] {
                        for layer in 0..2u8 {
                            match walk(&mut e.field, &placed, from, layer, facing, keys, 48) {
                                Ok(s) => println!("  walk {keys:#x} {s}"),
                                Err(err) => println!("  walk {keys:#x} from {from:?} layer {layer}: {err}"),
                            }
                        }
                    }
                }
                Ok(())
            }
            "gondowan-walk" => {
                // Jumping the gap at row 17 (touch cells 10 and 11) from each
                // side, after each arrival, with layer 2's cell (43, 17) (raw
                // grid (107, 17)) as the arrival left it and then swapped.
                let base = at("gMapGridCellBuffer")?;
                let cell = base + (107 + 17 * 128) * 4;
                for entrance in [1u16, 2, 3, 6] {
                    for swap in [false, true] {
                        let mut e = self.enter_with((158, entrance), &[])?;
                        let was = e.field.read32(cell);
                        if swap {
                            e.field.write32(cell, if was == 0x0600_0001 { 0x0000_0001 } else { 0x0600_0001 });
                        }
                        let now = e.field.read32(cell);
                        let saved = e.field.save();
                        for (from, facing, keys) in [((680, 280), EAST, KEY_RIGHT), ((712, 280), WEST, KEY_LEFT), ((696, 264), SOUTH, KEY_DOWN), ((696, 296), NORTH, KEY_UP)] {
                            for layer in [0u8, 2] {
                                match walk(&mut e.field, &saved, from, layer, facing, keys, 64) {
                                    Ok(s) => println!("entrance {entrance} cell {now:08x} (was {was:08x}) walk {keys:#x} {s}"),
                                    Err(err) => println!("entrance {entrance} cell {now:08x} walk {keys:#x} from {from:?} layer {layer}: {err}"),
                                }
                            }
                        }
                    }
                }
                Ok(())
            }
            "gondowan-layers" => {
                // Each layer's ground (the word's top byte) and collision code
                // over the cave's north half, after the east arrival.
                let e = self.enter_with((158, 2), &[])?;
                for layer in 0..3u8 {
                    println!("layer {layer}: ground byte / code, columns 14..=54, rows 2..=30");
                    for z in 2..=30 {
                        let row: String = (14..=54).map(|x| {
                            let w = e.field.cell_word(layer, x, z) as u32;
                            let (g, c) = (w >> 24, (w >> 16) & 0xff);
                            if c == 255 { " ##".to_string() } else if c != 0 { format!("{c:3}") } else { format!("{g:3x}") }
                        }).collect();
                        println!("  {z:2}:{row}");
                    }
                }
                Ok(())
            }
            "gondowan-move" => {
                // Move cast east on the pillar, from column 41 and from 42,
                // after each arrival: where it ends up, whether it drops.
                let flag = self.flags.resolve("FLAG_GONDOWAN_CAVE_PILLAR_DROPPED", None).ok_or("no flag")?;
                let base = at("gMapGridCellBuffer")?;
                let cell = base + (107 + 17 * 128) * 4;
                let flags_at = at("gGameFlagBytes")?;
                for entrance in [1u16, 2] {
                    for column in [41, 42] {
                        let mut e = self.enter_with((158, entrance), &[])?;
                        let a = e.field.actor_address(9);
                        e.field.write32(a + 8, ((column * 16 + 8) << 16) as u32);
                        e.field.frames(2)?;
                        e.placed = e.field.save();
                        let x = column * 16 + 8 - 24;
                        let height = e.field.ground(0, x, 280)?;
                        let spot = Spot { x, z: 280, layer: 0, height, facing: EAST, walk: 0, cell: None };
                        let result = Self::record_trigger(&mut e, Trigger::Psynergy(MOVE, 9), Some(spot), 90);
                        let dropped = e.field.read8(flags_at + u32::from(flag >> 3)) >> (flag & 7) & 1;
                        println!(
                            "entrance {entrance}, pillar from column {column}: {} frames; pillar {}; dropped pillar {}; flag {dropped}; cell {:08x}",
                            result.as_ref().map(|f| f.len() as i64).unwrap_or(-1),
                            describe(&e.field, 9),
                            describe(&e.field, 10),
                            e.field.read32(cell)
                        );
                        if let Err(err) = &result {
                            println!("   {err}");
                        }
                        if let Ok(frames) = result {
                            let rgb: Vec<Vec<u32>> = frames.iter().step_by(2).map(|f| f.iter().map(|&c| super::animation::rgb(c)).collect()).collect();
                            super::animation::write(&self.out.join(format!("gondowan-move-e{entrance}-c{column}.gif")), &rgb, super::mgba::WIDTH)?;
                        }
                    }
                }
                Ok(())
            }
            "gondowan-set" => {
                // The pillar (actor 9) set down at columns 41 to 44 of row
                // 17, after each arrival: the height it settles at.
                for entrance in [1u16, 2] {
                    for column in [41, 42, 43, 44] {
                        let mut e = self.enter_with((158, entrance), &[])?;
                        let a = e.field.actor_address(9);
                        e.field.write32(a + 8, ((column * 16 + 8) << 16) as u32);
                        e.field.frames(90)?;
                        println!("entrance {entrance} column {column}: {}", describe(&e.field, 9));
                    }
                }
                Ok(())
            }
            "gondowan-look" => {
                // The dropped pillar (actor 10) with layer 2's cell (43, 17)
                // as the map loads it and as the drop leaves it, seen from
                // the leader standing west of the crevice.
                let base = at("gMapGridCellBuffer")?;
                let cell = base + (107 + 17 * 128) * 4;
                let flag = self.flags.resolve("FLAG_GONDOWAN_CAVE_PILLAR_DROPPED", None).ok_or("no flag")?;
                for (entrance, drop) in [(2u16, true), (1, true), (2, false), (1, false)] {
                    for value in [0x0600_0001u32, 0x0000_0001] {
                        let set: Vec<u16> = if drop { vec![flag] } else { Vec::new() };
                        let mut e = self.enter_flags((158, entrance), &set)?;
                        let placed = e.placed_clone();
                        let height = e.field.ground(0, 664, 264)?;
                        e.field.stand(&placed, &Spot { x: 664, z: 264, layer: 0, height, facing: EAST, walk: 0, cell: None })?;
                        e.field.write32(cell, value);
                        e.field.frames(30)?;
                        println!("entrance {entrance} dropped {drop} cell {value:08x}: {}", describe(&e.field, 10));
                        println!("   leader {}", describe(&e.field, 0));
                        let image = e.field.screen();
                        super::places::write_png(&self.out.join(format!("gondowan-e{entrance}-d{}-c{value:x}.png", drop as u8)), &image)?;
                    }
                }
                Ok(())
            }
            "gondowan" => {
                // GondowanCave (158): east arrival (entrance 0), the others.
                for entrance in [0u16, 1, 2, 5] {
                    let e = self.enter_with((158, entrance), &[])?;
                    let work = e.field.map_work();
                    let base = at("gMapGridCellBuffer")?;
                    let ptrs: Vec<String> = (0..3).map(|l| {
                        let p = e.field.read32(work + 0x130 + 0x30 * l);
                        format!("{p:08x} (+{} words: column {}, row {})", (p.wrapping_sub(base)) / 4, ((p.wrapping_sub(base)) / 4) % 128, ((p.wrapping_sub(base)) / 4) / 128)
                    }).collect();
                    println!("GondowanCave entrance {entrance}: leader {:?}; layer windows {}", e.field.leader().map(|l| (l.x, l.z, l.layer)), ptrs.join(", "));
                    let raw = |x: i32, z: i32| e.field.read32(base + ((x + z * 128) * 4) as u32);
                    for (x, z) in [(43, 17), (44, 17), (107, 17), (108, 17), (43, 18), (42, 17)] {
                        println!("  raw grid ({x}, {z}) = {:08x}", raw(x, z));
                    }
                    for layer in 0..3 {
                        cells(&e.field, layer, (38, 13), (48, 21));
                    }
                    for layer in 0..2 {
                        words(&e.field, layer, (40, 15), (46, 19));
                    }
                    actors(&e.field, 8..16);
                }
                Ok(())
            }
            "fuchin" => {
                // FuchinFallsCaveDragonsEye (64): where trigger 24 is, in each
                // state of the fourth log's flags.
                let states: [(&str, &[u16]); 6] = [
                    ("no flags", &[]),
                    ("FLAG_MAPSEC_TEMP_0F", &[0x30f]),
                    ("FLAG_MAPSEC_TEMP_10", &[0x310]),
                    ("FLAG_MAPSEC_TEMP_11", &[0x311]),
                    ("FLAG_MAPSEC_TEMP_12", &[0x312]),
                    ("FLAG_MAPSEC_TEMP_08", &[0x308]),
                ];
                for entrance in 0..4u16 {
                    for (label, set) in states {
                        let Ok(mut e) = self.enter_flags((64, entrance), set) else {
                            println!("entrance {entrance} {label}: cannot enter");
                            continue;
                        };
                        let mut found = Vec::new();
                        for code in 20..=24u16 {
                            let c = e.field.cells_with_code(code)?;
                            found.push(format!("{code}: {:?}", c));
                        }
                        println!("entrance {entrance} {label}: leader {:?}; fourth log {}", e.field.leader().map(|l| (l.x, l.z)), describe(&e.field, 11));
                        for f in found {
                            println!("  {f}");
                        }
                        if entrance == 0 && set.is_empty() {
                            actors(&e.field, 8..14);
                            for layer in 0..2 {
                                cells(&e.field, layer, (10, 18), (20, 34));
                            }
                        }
                    }
                }
                Ok(())
            }
            "heap" => self.heap(&at("gHeapSlots")?, map),
            "heap-battle" => self.heap_battle(map),
            "venus" => self.venus(map),
            other => self.probe_place(other, map),
        }
    }

    /// Enters as enter_with does, with these flags (numbers) set too.
    pub(super) fn enter_flags(&self, place: (u16, u16), set: &[u16]) -> Result<super::guide::Entered<'a>, String> {
        let names: Vec<String> = set.iter().map(|f| format!("0x{f:x}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        self.enter_with(place, &refs)
    }

    fn heap(&mut self, work: &u32, map: &Map) -> Result<(), String> {
        use super::field::{HEAP_EWRAM_HIGH, HEAP_IWRAM_HIGH, HEAP_PCS, HEAP_WORK};
        use std::sync::atomic::Ordering::Relaxed;
        let at = |s: &str| map.symbol(s).map(|(_, a)| a & !1).ok_or_else(|| format!("the map has no {s}"));
        for (slot, name) in HEAP_PCS.iter().zip(["Free", "FreeHeapSlot", "InitHeap"]) {
            slot.store(at(name)?, Relaxed);
        }
        HEAP_WORK.store(*work, Relaxed);
        let mut highest = (0u32, 0u32);
        let mut report = |what: &str| {
            let (iw, ew) = (HEAP_IWRAM_HIGH.swap(0, Relaxed), HEAP_EWRAM_HIGH.swap(0, Relaxed));
            highest = (highest.0.max(iw), highest.1.max(ew));
            println!("{what}: IWRAM next up to {iw:08x}, EWRAM next up to {ew:08x}");
        };
        // A random battle in Goma Cave, fought by pressing A.
        {
            let mut e = self.enter_with((27, 5), &[])?;
            let mut fought = false;
            'walk: for leg in 0..300 {
                for keys in [if leg % 2 == 0 { KEY_UP } else { KEY_DOWN }, if leg % 2 == 0 { KEY_LEFT } else { KEY_RIGHT }] {
                    e.field.hold(keys | KEY_B);
                    for _ in 0..70 {
                        e.field.frames(1)?;
                        if e.field.scene() == 0x1fe {
                            fought = true;
                            break 'walk;
                        }
                    }
                }
            }
            report(&format!("Goma Cave walked (battle came: {fought})"));
            for _ in 0..600 {
                e.field.hold(KEY_A);
                e.field.frames(3)?;
                e.field.hold(0);
                e.field.frames(9)?;
            }
            report(&format!("the battle fought with A, then scene {}", e.field.scene()));
        }
        for mode in ["menus", "items", "menu-psynergy", "field-psynergy", "shop", "inn", "world-map", "world-map-picture", "talk", "djinn-room", "credits", "sleep", "random-battle", "walk", "snow"] {
            HEAP_IWRAM_HIGH.store(0, Relaxed);
            HEAP_EWRAM_HIGH.store(0, Relaxed);
            match self.run(mode, &[]) {
                Ok(()) => report(mode),
                Err(e) => report(&format!("{mode} (failed: {e})")),
            }
        }
        HEAP_WORK.store(0, Relaxed);
        println!("highest: IWRAM {:08x}, EWRAM {:08x}; the mixer at {:08x}", highest.0, highest.1, at("SoundDriver_Mixer")?);
        Ok(())
    }

    /// A Goma Cave battle fought with A: every allocation whose IWRAM block
    /// would reach past the mixer's start, who asked for it, and whether the
    /// mixer's bytes change.
    fn heap_battle(&mut self, map: &Map) -> Result<(), String> {
        let at = |s: &str| map.symbol(s).map(|(_, a)| a & !1).ok_or_else(|| format!("the map has no {s}"));
        let work = at("gHeapSlots")?;
        let mixer = at("SoundDriver_Mixer")?;
        let allocators = vec![at("AllocHeapSlot")?, at("AllocHeapSlotEwram")?, at("Alloc")?, at("AllocEwram")?, at("Free")?, at("FreeHeapSlot")?];
        let names = ["AllocHeapSlot", "AllocHeapSlotEwram", "Alloc", "AllocEwram", "Free", "FreeHeapSlot"];
        let mut e = self.enter_with((27, 5), &[])?;
        let snapshot: Vec<u32> = (0..233).map(|i| e.field.read32(mixer + 4 * i)).collect();
        let log = Rc::new(RefCell::new(Vec::<String>::new()));
        let keep = log.clone();
        let mut pcs = allocators.clone();
        pcs.push(mixer);
        let tick = at("AudioEngine_RunMixerTick")?;
        pcs.push(tick);
        let enter = at("SoundDriver_EnterMixer")?;
        pcs.push(enter);
        let enters = Rc::new(RefCell::new(0u32));
        let keep_enters = enters.clone();
        let ticks = Rc::new(RefCell::new(0u32));
        let keep_ticks = ticks.clone();
        let changed = Rc::new(RefCell::new(None::<u32>));
        let keep_changed = changed.clone();
        let damaged = Rc::new(RefCell::new(false));
        let runs_damaged = Rc::new(RefCell::new((0u32, 0u32)));
        let keep_runs = runs_damaged.clone();
        e.field.probe_pcs = pcs.clone();
        e.field.probe = Some(Box::new(move |gba: &Gba, pc: u32, frame: u32| {
            if pc == 0 {
                let first = (0..233u32).find(|&i| gba.read32(mixer + 4 * i) != snapshot[i as usize]);
                let now = first.is_some();
                if now != *damaged.borrow() {
                    keep.borrow_mut().push(format!("frame {frame}: the mixer is {}", match first { Some(i) => format!("overwritten from +{:#x}", 4 * i), None => "intact again".to_string() }));
                    *damaged.borrow_mut() = now;
                    if now && keep_changed.borrow().is_none() {
                        *keep_changed.borrow_mut() = Some(frame);
                    }
                }
                return;
            }
            if pc == enter {
                *keep_enters.borrow_mut() += 1;
                return;
            }
            if pc == tick {
                *keep_ticks.borrow_mut() += 1;
                return;
            }
            if pc == mixer {
                let mut r = keep_runs.borrow_mut();
                if *damaged.borrow() { r.1 += 1 } else { r.0 += 1 }
                return;
            }
            let Some(k) = pcs.iter().position(|&p| p == pc) else { return };
            let iw = gba.read32(work + 4);
            let ew = gba.read32(work);
            let (r0, r1, lr) = (gba.register(0), gba.register(1), gba.register(14));
            let line = match k {
                0 | 1 => format!("frame {frame}: {}(slot {r0}, {r1:#x} bytes) from {lr:08x}; slot holds {:08x}; next IWRAM {iw:08x}, EWRAM {ew:08x}", names[k], gba.read32(work + 4 * r0)),
                2 | 3 => format!("frame {frame}: {}({r0:#x} bytes) from {lr:08x}; next IWRAM {iw:08x}, EWRAM {ew:08x}", names[k]),
                _ => format!("frame {frame}: {}({r0:#x}) from {lr:08x}; next IWRAM {iw:08x}, EWRAM {ew:08x}", names[k]),
            };
            let size = if k <= 1 { r1 } else { r0 };
            let crosses = k <= 3 && ((k == 0 || k == 2) && iw + size > mixer && iw < mixer + 0x800 || iw > mixer);
            if crosses || iw > 0x0300_6800 {
                keep.borrow_mut().push(line);
            }
        }));
        let mut fought = false;
        'walk: for leg in 0..300 {
            for keys in [if leg % 2 == 0 { KEY_UP } else { KEY_DOWN }, if leg % 2 == 0 { KEY_LEFT } else { KEY_RIGHT }] {
                e.field.hold(keys | KEY_B);
                for _ in 0..70 {
                    e.field.frames(1)?;
                    if e.field.scene() == 0x1fe {
                        fought = true;
                        break 'walk;
                    }
                }
            }
        }
        println!("battle came: {fought}");
        for _ in 0..600 {
            e.field.hold(KEY_A);
            e.field.frames(3)?;
            e.field.hold(0);
            e.field.frames(9)?;
        }
        for line in log.borrow().iter().take(80) {
            println!("{line}");
        }
        println!("{} lines; the mixer changed: {:?}; mixer entries while intact, while overwritten: {:?}; ticks {}; SoundDriver_EnterMixer entries {}; sound work ident {:08x}", log.borrow().len(), changed.borrow(), runs_damaged.borrow(), ticks.borrow(), enters.borrow(), e.field.read32(at("gSound_Work")?));
        for line in log.borrow().iter().filter(|l| l.contains("the mixer is")) {
            println!("{line}");
        }
        Ok(())
    }

    /// Venus Lighthouse's second floor (scene 184), entered by its gate room
    /// (entrance 9): the gate pillar (actor 9) set a cell north of its socket
    /// (46, 45), and Move cast on it from the north, so that it sinks with
    /// the leader north of it; played as the game does it, and again with
    /// VenusLighthouse_CreateOccludingPassageEffect (at `VENUS_OCCLUDER`, an
    /// address in the overlay) returning at once.
    fn venus(&mut self, _map: &Map) -> Result<(), String> {
        let occluder = std::env::var("VENUS_OCCLUDER").ok().and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok()).ok_or("set VENUS_OCCLUDER")?;
        let watch: Vec<u32> = std::env::var("VENUS_WATCH").unwrap_or_default().split(',').filter_map(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok()).collect();
        let pillar: Vec<i32> = std::env::var("VENUS_PILLAR").unwrap_or_else(|_| "744,712".into()).split(',').filter_map(|v| v.parse().ok()).collect();
        let leader: Vec<i32> = std::env::var("VENUS_LEADER").unwrap_or_else(|_| "744,696".into()).split(',').filter_map(|v| v.parse().ok()).collect();
        let prepare = |me: &Self, stub: bool| -> Result<(super::guide::Entered<'a>, Spot, Rc<RefCell<Vec<u32>>>), String> {
            let mut e = me.enter_with((184, 9), &[])?;
            let a = e.field.actor_address(9);
            e.field.write32(a + 8, (pillar[0] << 16) as u32);
            e.field.write32(a + 0x10, (pillar[1] << 16) as u32);
            e.field.frames(2)?;
            e.placed = e.field.save();
            if stub {
                e.field.returns = vec![(occluder, 0)];
            }
            // VENUS_ARRIVE: at that address (the event's entry), the pillar is
            // put at this z, as if the push had brought it there.
            if let Ok(v) = std::env::var("VENUS_ARRIVE") {
                let parts: Vec<&str> = v.split(',').collect();
                if let (Some(at), Some(z)) = (parts.first().and_then(|p| u32::from_str_radix(p, 16).ok()), parts.get(1).and_then(|p| p.parse::<i32>().ok())) {
                    e.field.pokes = vec![(at, a + 0x10, (z << 16) as u32)];
                }
            }
            let mut pcs = watch.clone();
            pcs.push(occluder);
            e.field.probe_pcs = pcs.clone();
            let hits = Rc::new(RefCell::new(vec![0u32; pcs.len()]));
            let keep = hits.clone();
            e.field.probe = Some(Box::new(move |_: &Gba, pc: u32, _: u32| {
                if let Some(i) = pcs.iter().position(|&p| p == pc) {
                    keep.borrow_mut()[i] += 1;
                }
            }));
            let height = e.field.ground(0, leader[0], leader[1])?;
            Ok((e, Spot { x: leader[0], z: leader[1], layer: 0, height, facing: SOUTH, walk: 0, cell: None }, hits))
        };
        let mut runs = Vec::new();
        for stub in [false, true] {
            let (mut e, spot, hits) = prepare(self, stub)?;
            let frames = Self::record_trigger(&mut e, Trigger::Psynergy(MOVE, 9), Some(spot), 60)?;
            println!("stub {stub}: {} frames, hits {:?} (watched, then the occluder); pillar {}", frames.len(), hits.borrow(), describe(&e.field, 9));
            let rgb: Vec<Vec<u32>> = frames.iter().map(|f| f.iter().map(|&c| super::animation::rgb(c)).collect()).collect();
            super::animation::write(&self.out.join(format!("venus-sink-{}.gif", if stub { "without" } else { "with" })), &rgb, super::mgba::WIDTH)?;
            runs.push(frames);
        }
        if std::env::var("VENUS_SEARCH").is_err() {
            return compare_runs(self, &runs);
        }
        let mut found = None;
        'search: for (scene, triggers) in [
            (184u16, vec![Trigger::Psynergy(MOVE, 9), Trigger::Touch(36)]),
            (185, vec![Trigger::Psynergy(MOVE, 9), Trigger::Psynergy(MOVE, 10), Trigger::Psynergy(MOVE, 11), Trigger::Touch(35)]),
            (186, vec![Trigger::Psynergy(MOVE, 15), Trigger::Psynergy(MOVE, 16), Trigger::Psynergy(MOVE, 17), Trigger::Psynergy(MOVE, 18), Trigger::Touch(45)]),
        ] {
            for entrance in 1..14u16 {
                for &trigger in &triggers {
                    let Ok(mut e) = self.enter_with((scene, entrance), &[]) else { continue };
                    e.field.probe_pcs = vec![occluder];
                    let hits = Rc::new(RefCell::new(0u32));
                    let keep = hits.clone();
                    e.field.probe = Some(Box::new(move |_: &Gba, pc: u32, _: u32| {
                        if pc != 0 {
                            *keep.borrow_mut() += 1;
                        }
                    }));
                    let r = Self::record_trigger(&mut e, trigger, None, 0);
                    println!("scene {scene} entrance {entrance} {trigger:?}: {} frames, occluder {}", r.as_ref().map(|f| f.len() as i64).unwrap_or(-1), hits.borrow());
                    if *hits.borrow() > 0 {
                        found = Some((scene, entrance, trigger));
                        break 'search;
                    }
                }
            }
        }
        let (scene, entrance, trigger) = found.ok_or("no sinking pillar made the occluder")?;
        let mut runs = Vec::new();
        for stub in [false, true] {
            let mut e = self.enter_with((scene, entrance), &[])?;
            if stub {
                e.field.returns = vec![(occluder, 0)];
            }
            e.field.probe_pcs = vec![occluder];
            let hits = Rc::new(RefCell::new(0u32));
            let keep = hits.clone();
            e.field.probe = Some(Box::new(move |_: &Gba, pc: u32, _: u32| {
                if pc != 0 {
                    *keep.borrow_mut() += 1;
                }
            }));
            let frames = Self::record_trigger(&mut e, trigger, None, 60)?;
            println!("stub {stub}: scene {scene} entrance {entrance} {trigger:?}: {} frames, occluder entered {} times; leader {}", frames.len(), hits.borrow(), describe(&e.field, 0));
            let rgb: Vec<Vec<u32>> = frames.iter().map(|f| f.iter().map(|&c| super::animation::rgb(c)).collect()).collect();
            super::animation::write(&self.out.join(format!("venus-sink-{}.gif", if stub { "without" } else { "with" })), &rgb, super::mgba::WIDTH)?;
            runs.push(frames);
        }
        compare_runs(self, &runs)
    }
}

/// Compares two recordings frame by frame, and writes the frame where they
/// differ most from each.
fn compare_runs(g: &Guide, runs: &[Vec<Vec<u16>>]) -> Result<(), String> {
    {
        let (a, b) = (&runs[0], &runs[1]);
        let mut differ = Vec::new();
        for (i, (fa, fb)) in a.iter().zip(b.iter()).enumerate() {
            let n = fa.iter().zip(fb.iter()).filter(|(x, y)| x != y).count();
            if n > 0 {
                differ.push((i, n));
            }
        }
        println!("frames compared: {}; frames that differ: {} {:?}", a.len().min(b.len()), differ.len(), &differ[..differ.len().min(40)]);
        if let Some(&(i, _)) = differ.iter().max_by_key(|(_, n)| *n) {
            for (name, run) in [("with", a), ("without", b)] {
                let image = super::field::Image { width: super::mgba::WIDTH, height: super::mgba::HEIGHT, pixels: run[i].iter().map(|&c| super::animation::rgb(c)).collect() };
                super::places::write_png(&g.out.join(format!("venus-frame-{i}-{name}.png")), &image)?;
            }
        }
        Ok(())
    }
}

impl<'a> super::guide::Entered<'a> {
    pub fn placed_clone(&mut self) -> super::field::Saved {
        let now = self.field.save();
        self.field.load(&self.placed);
        let placed = self.field.save();
        self.field.load(&now);
        placed
    }
}
