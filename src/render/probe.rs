//! `render probe`: questions only the running game can settle, asked of a
//! built edition, kept for the probes The Broken Seal's source and pages cite
//! as evidence: what an actor slot holds, a map's cells before and after an
//! arrival, a pushed log's path, a render with one function stubbed out.
//! Each probe prints what it read and writes its pictures into the probe's
//! folder. This file holds the shared readings and `venus`, the occluding
//! passage with and without VenusLighthouse_CreateOccludingPassageEffect.

use super::field::{Field, Image, Spot};
use super::guide::Guide;
use super::mgba::Gba;
use super::story::{Trigger, MOVE};
use crate::gba::ld_map::Map;
use std::cell::RefCell;
use std::rc::Rc;

pub const PROBES: &[&str] = &["venus"];

const SOUTH: u32 = 0x4000;

fn word(field: &Field, a: u32) -> i32 {
    field.read32(a) as i32
}

/// One actor as the field keeps it (struct Actor).
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
        match name {
            "venus" => self.venus(map),
            other if super::probe_units::PROBES.contains(&other) => self.probe_unit(other, map),
            other => self.probe_place(other, map),
        }
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
            super::animation::write_evidence(&self.out.join(format!("venus-sink-{}.gif", if stub { "without" } else { "with" })), &rgb, super::mgba::WIDTH)?;
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
            super::animation::write_evidence(&self.out.join(format!("venus-sink-{}.gif", if stub { "without" } else { "with" })), &rgb, super::mgba::WIDTH)?;
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

// ---- the probes' pictures ----

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

/// The map of the room the field entered, the leader hidden, from the
/// moment `settled`, with its top left on the map in pixels.
pub(super) fn room_map(field: &mut Field, settled: &super::field::Saved) -> Result<(Image, (i32, i32)), String> {
    let r = field.room();
    let image = field.stitch(settled, [r[0] << 16, r[1] << 16, r[2] << 16, r[3] << 16], false)?;
    Ok((image, (r[0], r[1])))
}


/// struct FieldWork's Psynergy request (field_work.h).
const WORK_PSYNERGY: u32 = 0x17e;

/// Casts `ability` as Isaac from where the leader stands, as the field menu
/// asks for it: the request the field sequence answers next.
pub(super) fn cast(field: &mut Field, ability: u16) {
    let work = field.field_work();
    field.write16(work + WORK_PSYNERGY, ability);
}
