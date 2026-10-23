//! Take-apart pictures for the battle, effect, field and shop guides: a
//! battle action's event queue beside the frames it plays, the battlefield's
//! places, an effect's canvas and particle pools, a sprite's animation
//! layers and a shop's stock row.

use super::battle::{self, Scene};
use super::field::Image;
use super::guide::Guide;
use super::guide_more::{canvas, mix, paste, screen};
use super::guide_take::text;
use super::mgba::{Gba, HEIGHT, WIDTH};
use super::plan::{self, Cast, Tables};
use std::collections::HashMap;

/// struct BattleWork's event state (battle_work.h): the queue's opcodes,
/// operands, next target and count, then the playback's phase and next
/// event.
const WORK_EVENTS: u32 = 0x6b8;
const QUEUE_OPERANDS: u32 = 0x40;
const QUEUE_TARGET: u32 = 0x140;
const QUEUE_COUNT: u32 = 0x144;
const EVENTS_PHASE: u32 = 0x148;
const EVENTS_INDEX: u32 = 0x14c;
const QUEUE_CAPACITY: u32 = 64;
/// BattleWork.turn (struct BattleTurn) and its target count.
const WORK_PLAN: u32 = 0x654;

/// enum BattlePlaybackCommand's names, as the panel prints them.
const OPCODES: [&str; 15] = [
    "UNIT_NAME",
    "NUMBER",
    "ITEM_NAME",
    "ABILITY_NAME",
    "MESSAGE",
    "MESSAGE_NO_WAIT",
    "MARK",
    "CLEAR_ARGUMENTS",
    "HIT",
    "DEFEAT",
    "DRAW_STATUS",
    "REFRESH_UNIT",
    "REMOVE_UNIT",
    "DJINN_BONUS",
    "SOUND",
];
const UNIT_NAME: u8 = 0;
const NUMBER: u8 = 1;
const MESSAGE: u8 = 4;
const MESSAGE_NO_WAIT: u8 = 5;
const HIT: u8 = 8;
const DEFEAT: u8 = 9;
const REFRESH_UNIT: u8 = 11;
const REMOVE_UNIT: u8 = 12;
const SOUND: u8 = 14;
/// The first enemy unit id.
const FIRST_ENEMY_UNIT: u32 = 128;

/// The playback's phase names (enum BattlePlaybackPhase and playback.c's
/// waits).
fn phase_name(phase: u32) -> &'static str {
    match phase {
        0 => "IDLE",
        1 => "RESOLVE_TARGET",
        2 => "DISPATCH",
        3 => "PLAY_MESSAGE",
        4 => "READY",
        5 => "PLAY_ADVANCE",
        10 => "PLAY_HIT",
        11 => "PLAY_FELLED",
        13 => "PLAY_MESSAGE_ADD",
        _ => "?",
    }
}

/// The event state after one frame.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Events {
    phase: u32,
    index: u32,
    /// Targets resolved so far: the one playing is `target - 1`.
    target: u32,
    targets: u32,
    queue: Vec<(u8, u32)>,
}

fn read_events(gba: &Gba, work_slot: u32) -> Option<Events> {
    let work = gba.read32(work_slot);
    if work == 0 {
        return None;
    }
    let e = work + WORK_EVENTS;
    let count = gba.read32(e + QUEUE_COUNT).min(QUEUE_CAPACITY);
    let queue = (0..count).map(|i| (gba.read8(e + i), gba.read32(e + QUEUE_OPERANDS + 4 * i))).collect();
    Some(Events {
        phase: gba.read32(e + EVENTS_PHASE),
        index: gba.read32(e + EVENTS_INDEX),
        target: gba.read32(e + QUEUE_TARGET),
        targets: u32::from(gba.read8(work + WORK_PLAN + PLAN_TARGET_COUNT)),
        queue,
    })
}

/// BattleTurn.targetCount's offset in the plan.
const PLAN_TARGET_COUNT: u32 = 0x01;

/// The messages' symbols and texts by number, from the story tools'
/// messages.tsv beside flags.tsv.
fn messages(dir: &std::path::Path) -> HashMap<u32, (String, String)> {
    let mut out = HashMap::new();
    let Ok(text) = std::fs::read_to_string(dir.join("messages.tsv")) else {
        return out;
    };
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if let (Some(n), Some(symbol), Some(t)) = (f.first().and_then(|n| n.parse().ok()), f.get(1), f.get(2)) {
            out.insert(n, (symbol.to_string(), t.to_string()));
        }
    }
    out
}

/// The sounds' names by number, from songs.h.
fn sounds() -> HashMap<u32, String> {
    let path = crate::compile::root().join("games/tbs/include/songs.h");
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            (w.next() == Some("#define")).then_some(())?;
            let name = w.next()?;
            let value = w.next()?.parse().ok()?;
            name.starts_with("SE_").then(|| (value, name.to_string()))
        })
        .collect()
}

/// A message's text as the panel shows it: its arguments as <UNIT> and
/// <N>.
fn template(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let close = rest[open..].find('}').map_or(rest.len(), |c| open + c + 1);
        let field = &rest[open..close];
        out.push_str(if field.contains("name") { "<UNIT>" } else if field.contains("number") { "<N>" } else { "" });
        rest = &rest[close..];
    }
    out.push_str(rest);
    out
}

const PANEL_BACK: u32 = 0x20_22_28;
const PANEL_TEXT: u32 = 0xe8_e8_e8;
const PANEL_DIM: u32 = 0x80_84_8c;
const PANEL_MARK: u32 = 0xff_d8_40;
const PANEL_HEAD: u32 = 0x9c_c8_ff;

impl<'a> Guide<'a> {
    /// Isaac's Quake, the four's level-35 preset against copies of Garet,
    /// Ivan and Mia with Ivan aimed, recorded from the battle test menu as
    /// `render psynergy` records it: one frame for each event the playback
    /// task plays, beside that target's queue with the event playing marked,
    /// as `round-events.gif`. The queue and the playback's phase are read
    /// from BattleWork.events after each frame; an event's frame is the last
    /// before the next event plays, or, for a hit, the frame the unit turns
    /// white.
    pub fn round_events(&mut self) -> Result<(), String> {
        let root = crate::compile::root();
        let plan = Tables::read(root)?.plan()?;
        let cast: Cast = plan.casts.into_iter().find(|c| c.ability.name == "Quake").ok_or("no Quake cast")?;
        let symbols = battle::Symbols::find(self.map)?;
        let work_slot = self.sym("gBattleWork")?;
        let mut states: Vec<Option<Events>> = Vec::new();
        let recording = battle::record_watching(self.rom, &symbols, &cast, &Scene::Game, false, None, &[], &mut |gba, _| states.push(read_events(gba, work_slot)))?;
        let frames = recording.frames;
        let names = messages(&self.flags_dir());
        let sounds = sounds();
        let unit = |u: u32| -> String {
            if u >= FIRST_ENEMY_UNIT {
                cast.foes.get((u - FIRST_ENEMY_UNIT) as usize).map_or(format!("UNIT {u}"), |&o| format!("{} (UNIT {u})", plan::character_name(o)))
            } else {
                plan::character_name(u as u8).to_string()
            }
        };
        let describe = |(op, operand): (u8, u32)| -> String {
            let name = OPCODES.get(op as usize).copied().unwrap_or("?");
            let what = match op {
                UNIT_NAME | HIT | DEFEAT | REFRESH_UNIT | REMOVE_UNIT => unit(operand),
                NUMBER => format!("{}", operand as i32),
                MESSAGE | MESSAGE_NO_WAIT => names.get(&operand).map_or(format!("{operand}"), |(_, t)| template(t)),
                SOUND => sounds.get(&operand).cloned().unwrap_or(format!("SE_{operand}")),
                _ => String::new(),
            };
            format!("{name} {what}")
        };

        // When each (target, event) began: the first frame whose state has
        // played it (the next index past it, or a later target).
        let mut queues: Vec<Vec<(u8, u32)>> = Vec::new();
        let mut began: Vec<(usize, usize, usize)> = Vec::new();
        let mut last = (0u32, 0u32);
        for (f, state) in states.iter().enumerate() {
            let Some(s) = state else { continue };
            if s.target == 0 {
                continue;
            }
            let t = s.target as usize - 1;
            if queues.len() <= t {
                queues.resize(t + 1, Vec::new());
                queues[t] = s.queue.clone();
            }
            // Past the waits, a finished queue has played every event.
            let played = if s.phase == 1 || s.phase == 4 { s.queue.len() as u32 } else { s.index };
            let played = if s.target as usize - 1 > last.0 as usize { played } else { played.max(last.1) };
            // Events of earlier targets not yet seen began in this frame.
            for (pt, queue) in queues.iter().enumerate().take(t + 1) {
                let upto = if pt < t { queue.len() } else { played as usize };
                for i in 0..upto {
                    if !began.iter().any(|&(bt, bi, _)| bt == pt && bi == i) {
                        began.push((pt, i, f));
                    }
                }
            }
            last = (s.target - 1, played);
        }
        eprintln!("round-events: {} frames, {} targets", frames.len(), queues.len());
        for (t, queue) in queues.iter().enumerate() {
            for (i, &e) in queue.iter().enumerate() {
                let at = began.iter().find(|&&(bt, bi, _)| bt == t && bi == i).map(|b| b.2);
                eprintln!("  target {t} event {i}: {}  began at frame {at:?}", describe(e));
            }
        }

        // The frame each event shows: up to the next event's start.
        let mut order: Vec<(usize, usize, usize)> = began.clone();
        order.sort_by_key(|&(t, i, f)| (f, t, i));
        let mut shots: Vec<Image> = Vec::new();
        let mut holds: Vec<usize> = Vec::new();
        let first_event = order.first().map_or(0, |b| b.2);
        // The effect alone before its blow lands: playback idle.
        if first_event > 0 {
            let at = first_event * 3 / 4;
            shots.push(self.queue_panel(&frames[at], None, &[], "", &describe, states[at].as_ref(), true));
            holds.push(90);
        }
        for (k, &(t, i, f)) in order.iter().enumerate() {
            let next = order.get(k + 1).map_or(frames.len(), |b| b.2);
            let (op, _) = queues[t][i];
            let at = match op {
                HIT => (f + 3).min(frames.len() - 1),
                MESSAGE if next > f => next - 1,
                _ => f,
            };
            let aimed = FIRST_ENEMY_UNIT + cast.target_foe() as u32;
            let head = format!("TARGET {} OF {}: {}", t + 1, queues.len(), unit_of(&queues[t]).map_or(String::new(), |u| if u == aimed { format!("{}, AIMED", unit(u)) } else { unit(u) }));
            holds.push(if matches!(op, HIT | MESSAGE) { 60 } else { 36 });
            shots.push(self.queue_panel(&frames[at], Some(i), &queues[t], &head, &describe, states[at].as_ref(), matches!(op, HIT | MESSAGE)));
        }
        // Each shot held for 0.8 seconds, the first and last longer.
        let mut gif: Vec<Vec<u32>> = Vec::new();
        for (n, shot) in shots.iter().enumerate() {
            let hold = if n + 1 == shots.len() { 90 } else { holds[n] };
            gif.extend(std::iter::repeat_n(shot.pixels.clone(), hold));
        }
        let width = shots[0].width;
        let written = super::animation::write(&self.out.join("round-events.gif"), &gif, width)?;
        self.written.push(("round-events.gif".to_string(), written.bytes, format!("Quake's events one by one beside their queue; {} frames", written.frames)));
        Ok(())
    }

    /// The directory flags.tsv was read from, where the story tools write
    /// messages.tsv beside it.
    fn flags_dir(&self) -> std::path::PathBuf {
        self.flags.dir.clone()
    }

    /// One battle frame at twice its size beside a target's queue, the event
    /// `playing` marked, those before it dimmed.
    #[allow(clippy::too_many_arguments)]
    fn queue_panel(&self, frame: &[u16], playing: Option<usize>, queue: &[(u8, u32)], head: &str, describe: &dyn Fn((u8, u32)) -> String, state: Option<&Events>, waits: bool) -> Image {
        const PANEL: usize = 400;
        let mut image = canvas(2 * WIDTH + PANEL, 2 * HEIGHT, PANEL_BACK);
        paste(&mut image, &screen(frame), 0, 0, 2);
        let x = 2 * WIDTH as i32 + 10;
        let mut y = 8;
        text(&mut image, x, y, "BATTLEWORK.EVENTS.QUEUE", PANEL_HEAD, None, 2);
        y += 18;
        let line = match state {
            Some(s) if waits => format!("PHASE {}", phase_name(s.phase)),
            _ => "PLAYED IN THE SAME FRAME, NO WAIT".to_string(),
        };
        text(&mut image, x, y, &line, PANEL_DIM, None, 2);
        y += 18;
        if playing.is_none() {
            text(&mut image, x, y, "THE EFFECT PLAYS ALONE:", PANEL_TEXT, None, 2);
            text(&mut image, x, y + 18, "NO TARGET RESOLVED YET", PANEL_TEXT, None, 2);
            return image;
        }
        text(&mut image, x, y, head, PANEL_TEXT, None, 2);
        y += 22;
        for (i, &e) in queue.iter().enumerate() {
            let line = format!("{i:2} {}", describe(e));
            let line: String = line.chars().take(48).collect();
            let (colour, back) = match playing {
                Some(p) if p == i => (0x10_10_10, Some(PANEL_MARK)),
                Some(p) if i < p => (PANEL_DIM, None),
                _ => (PANEL_TEXT, None),
            };
            text(&mut image, x - 4, y, &line, colour, back, 2);
            y += 16;
        }
        image
    }
}

/// The unit a target's queue is about: its first hit's.
fn unit_of(queue: &[(u8, u32)]) -> Option<u32> {
    queue.iter().find(|&&(op, _)| op == HIT || op == DEFEAT).map(|&(_, u)| u)
}

/// struct BattleWork's unit lists and battle objects (battle_work.h,
/// battle_session.h): partyUnits and enemyIds end with 0xff; each unit's
/// object is slots[placement[unit]], placement indexed by party unit id or
/// by 8 + n for enemy unit 128 + n; an object's anchor is 16.16 pixels.
const WORK_PARTY_UNITS: u32 = 0x58;
const WORK_ENEMY_IDS: u32 = 0x66;
const WORK_SLOTS: u32 = 0x74;
const WORK_PLACEMENT: u32 = 0x2dc;
const SLOT_SIZE: u32 = 0x2c;
const SLOT_ANCHOR_X: u32 = 0x0c;
const SLOT_ANCHOR_Z: u32 = 0x10;
const UNIT_LIST_END: u16 = 0xff;
/// BattleHighlight_SetModes' mode for the units a target covers.
const HIGHLIGHT_TARGETED: u32 = 1;

/// The keys that open Isaac's Psynergy list from the battle's first menu,
/// choose Quake (the second row, under Ragnarok) and then move the aim
/// right three times, the selection wrapping round: (keys, frames).
const KEY_A: u32 = 0x1;
const KEY_B: u32 = 0x2;
const KEY_RIGHT: u32 = 0x10;
const KEY_DOWN: u32 = 0x80;
const AIM_FRAMES: u32 = 90;
const QUAKE_MENU: [(u32, u32); 17] = [
    (0, 60),
    (KEY_A, 4),
    (0, 40),
    (KEY_RIGHT, 4),
    (0, 30),
    (KEY_A, 4),
    (0, 40),
    (KEY_DOWN, 4),
    (0, 30),
    (KEY_A, 4),
    (0, AIM_FRAMES),
    (KEY_RIGHT, 4),
    (0, AIM_FRAMES),
    (KEY_RIGHT, 4),
    (0, AIM_FRAMES),
    (KEY_RIGHT, 4),
    (0, AIM_FRAMES),
];

/// A unit on the battlefield plane.
struct Placed {
    unit: u16,
    /// Its place in its side's list.
    place: usize,
    x: i32,
    z: i32,
}

fn placed(gba: &Gba, work: u32, list: u32, capacity: u32) -> Vec<Placed> {
    let mut out = Vec::new();
    for i in 0..capacity {
        let unit = gba.read16(work + list + 2 * i);
        if unit == UNIT_LIST_END {
            break;
        }
        let index = if u32::from(unit) >= FIRST_ENEMY_UNIT { 8 + u32::from(unit) - FIRST_ENEMY_UNIT } else { u32::from(unit) };
        let slot = work + WORK_SLOTS + SLOT_SIZE * u32::from(gba.read8(work + WORK_PLACEMENT + index));
        out.push(Placed { unit, place: i as usize, x: gba.read32(slot + SLOT_ANCHOR_X) as i32 >> 16, z: gba.read32(slot + SLOT_ANCHOR_Z) as i32 >> 16 });
    }
    out
}

fn disc(image: &mut Image, cx: i32, cy: i32, r: i32, colour: u32, ring: bool) {
    for y in -r..=r {
        for x in -r..=r {
            let d = x * x + y * y;
            if d <= r * r && (!ring || d >= (r - 2) * (r - 2)) {
                let (px, py) = (cx + x, cy + y);
                if px >= 0 && py >= 0 && (px as usize) < image.width && (py as usize) < image.height {
                    image.pixels[py as usize * image.width + px as usize] = colour;
                }
            }
        }
    }
}

const PLANE_GRID: u32 = 0x30_33_3a;
const PLANE_AXIS: u32 = 0x50_55_60;
const COVERED: u32 = 0xff_90_30;
const PARTY: u32 = 0x60_b0_ff;

impl<'a> Guide<'a> {
    /// Isaac aiming Quake (range 2) through the real command menu against
    /// copies of Garet, Mia, Ivan and Jenna, the aim moved right three times:
    /// the screen with the game's cursor and range markers beside the
    /// battlefield seen from above, each unit at its battle object's anchor
    /// (BattleUnit_RefreshPlacement, BattlePlacement_LayoutEnemies), the
    /// covered units ringed with their steps from the aimed place, as
    /// `battle-places.gif`. Which units are covered is read where the
    /// selection marks them, at BattleHighlight_SetModes' entry
    /// with mode 1: its list, the aimed unit first.
    pub fn battle_places(&mut self) -> Result<(), String> {
        let root = crate::compile::root();
        let plan = Tables::read(root)?.plan()?;
        let mut cast: Cast = plan.casts.into_iter().find(|c| c.ability.name == "Quake").ok_or("no Quake cast")?;
        cast.foes = vec![plan::GARET, plan::MIA, plan::IVAN, plan::JENNA];
        let range = cast.ability.range as usize;
        let symbols = battle::Symbols::find(self.map)?;
        let work_slot = self.sym("gBattleWork")?;
        let modes = self.sym("BattleHighlight_SetModes")?;
        // (frame, covered units, aimed first) at each marking; the places.
        let mut marks: Vec<(usize, Vec<u16>)> = Vec::new();
        let mut frame = 0usize;
        let mut field: (Vec<Placed>, Vec<Placed>) = (Vec::new(), Vec::new());
        let recording = battle::record_watching(self.rom, &symbols, &cast, &Scene::Game, false, Some(&QUAKE_MENU), &[modes], &mut |gba, hook| {
            let work = gba.read32(work_slot);
            match hook {
                Some(_) if gba.register(1) == HIGHLIGHT_TARGETED => {
                    let list = gba.register(0);
                    let units: Vec<u16> = (0..14).map(|i| gba.read16(list + 2 * i)).take_while(|&u| u != UNIT_LIST_END).collect();
                    // The acting unit marked for the command menu is not a target.
                    if units.first().is_some_and(|&u| u32::from(u) >= FIRST_ENEMY_UNIT) {
                        marks.push((frame, units));
                    }
                    field = (placed(gba, work, WORK_PARTY_UNITS, 7), placed(gba, work, WORK_ENEMY_IDS, 7));
                }
                Some(_) => {}
                None => frame += 1,
            }
        })?;
        {
            let frames = recording.frames;
            let (party, enemies) = &field;
            let name = |u: u16| -> String {
                if u32::from(u) >= FIRST_ENEMY_UNIT {
                    cast.foes.get(usize::from(u) - FIRST_ENEMY_UNIT as usize).map_or("?", |&o| plan::character_name(o)).to_string()
                } else {
                    plan::character_name(u as u8).to_string()
                }
            };
            eprintln!("battle-places: {} frames; party {:?}; enemies {:?}", frames.len(), party.iter().map(|p| (name(p.unit), p.x, p.z)).collect::<Vec<_>>(), enemies.iter().map(|p| (name(p.unit), p.place, p.x, p.z)).collect::<Vec<_>>());
            let mut out: Vec<Vec<u32>> = Vec::new();
            for (k, (start, covered)) in marks.iter().enumerate() {
                let end = marks.get(k + 1).map_or(frames.len(), |m| m.0);
                eprintln!("  frames {start}..{end}: covered {:?}", covered.iter().map(|&u| name(u)).collect::<Vec<_>>());
                let aimed = enemies.iter().find(|p| Some(&p.unit) == covered.first()).map(|p| p.place).unwrap_or(0);
                let plane = Self::plane(party, enemies, covered, aimed, range, &name);
                for f in &frames[*start..end] {
                    let mut image = canvas(4 * WIDTH, 2 * HEIGHT, PANEL_BACK);
                    paste(&mut image, &screen(f), 0, 0, 2);
                    paste(&mut image, &plane, 2 * WIDTH, 0, 1);
                    out.push(image.pixels);
                }
            }
            let written = super::animation::write(&self.out.join("battle-places.gif"), &out, 4 * WIDTH)?;
            self.written.push(("battle-places.gif".to_string(), written.bytes, format!("Quake aimed at each enemy beside the battlefield's places; {} frames", written.frames)));
        }
        Ok(())
    }

    /// The battlefield from above, as the camera faces it: the enemies'
    /// line (x -80) at the top, the party's (x 80) at the bottom, z running
    /// right to left: 2 picture pixels a battlefield pixel across, 1.1 down.
    fn plane(party: &[Placed], enemies: &[Placed], covered: &[u16], aimed: usize, range: usize, name: &dyn Fn(u16) -> String) -> Image {
        const SCALE: f64 = 2.0;
        const DEPTH: f64 = 1.1;
        let (w, h) = (2 * WIDTH, 2 * HEIGHT);
        let mut image = canvas(w, h, PANEL_BACK);
        let at = |x: i32, z: i32| ((w as f64 / 2.0 - f64::from(z) * SCALE) as i32, (180.0 + f64::from(x) * DEPTH) as i32);
        for g in (-128..=128).step_by(16) {
            let (gx, _) = at(0, g);
            let (_, gy) = at(g, 0);
            for i in 0..h as i32 {
                if gx >= 0 && (gx as usize) < w {
                    image.pixels[i as usize * w + gx as usize] = if g == 0 { PLANE_AXIS } else { PLANE_GRID };
                }
            }
            for i in 0..w as i32 {
                if gy >= 0 && (gy as usize) < h {
                    image.pixels[gy as usize * w + i as usize] = if g == 0 { PLANE_AXIS } else { PLANE_GRID };
                }
            }
        }
        text(&mut image, 6, 4, "THE BATTLEFIELD FROM ABOVE, PIXELS", PANEL_HEAD, Some(PANEL_BACK), 2);
        text(&mut image, 6, 22, &format!("QUAKE, RANGE {range}: THE AIMED PLACE AND {} EACH SIDE", range - 1), PANEL_TEXT, Some(PANEL_BACK), 2);
        text(&mut image, 6, h as i32 - 18, "Z ACROSS, X DOWN (X DRAWN SHORTER)", PANEL_DIM, Some(PANEL_BACK), 2);
        for p in party {
            let (px, py) = at(p.x, p.z);
            disc(&mut image, px, py, 5, PARTY, false);
            let label = name(p.unit);
            text(&mut image, px - 4 * label.len() as i32, py + 8, &label.to_uppercase(), PARTY, None, 2);
        }
        for p in enemies {
            let (px, py) = at(p.x, p.z);
            let step = p.place.abs_diff(aimed);
            let in_range = covered.contains(&p.unit);
            let colour = if step == 0 { PANEL_MARK } else if in_range { COVERED } else { PANEL_DIM };
            disc(&mut image, px, py, 5, colour, false);
            if in_range {
                disc(&mut image, px, py, 11, colour, true);
            }
            let label = name(p.unit).to_uppercase();
            text(&mut image, px - 4 * label.len() as i32, py - 32, &label, colour, None, 2);
            let place = format!("PLACE {}", p.place);
            text(&mut image, px - 4 * place.len() as i32, py + 14, &place, PANEL_TEXT, None, 2);
            let note = if step == 0 { "AIMED".to_string() } else { format!("STEP {step}") };
            text(&mut image, px - 4 * note.len() as i32, py + 30, &note, colour, None, 2);
            let pos = format!("{},{}", p.x, p.z);
            text(&mut image, px - 4 * pos.len() as i32, py + 46, &pos, PANEL_DIM, None, 2);
        }
        image
    }
}

/// The effect's heap slots (heap.h): gHeapSlots[HEAP_SLOT_BATTLE_ANIM]
/// holds struct BattleAnimWork (battle_anim_work.h).
const HEAP_SLOT_BATTLE_ANIM: u32 = 39;
const FX_PARTICLES: u32 = 0x7080;
const FX_TRANSFER_MODE: u32 = 0x7780;
const FX_TRANSFER_VALUE: u32 = 0x7784;
const FX_TRANSFER_PENDING: u32 = 0x7824;
const HEAP_SLOT_BATTLE_CANVAS: u32 = 40;
/// struct Particle: x, y, z, three velocities, then variant.
const STEP_SIZE: u32 = 28;
const STEP_X: u32 = 0;
const STEP_VARIANT: u32 = 24;
const WORK_PARTICLE_COUNT: u32 = 64;
/// PROJECTILE_VOLLEY_PARTICLE_COUNT and its ranges (projectiles.c): hit
/// flashes from 0, sparks from 100, fireball trails from 512.
const VOLLEY_PARTICLES: u32 = 1024;
const VOLLEY_FIRST_SPARK: usize = 100;
const VOLLEY_FIRST_TRAIL: usize = 512;
/// The canvas as BG2 shows it: character block 1, 16 by 16 tiles of 8 by 8
/// pixels, a byte a pixel; each byte a level, an index into the first 64
/// background colours.
const CANVAS_VRAM: u32 = 0x0600_4000;
const CANVAS_SIDE: usize = 128;
const LEVELS: u32 = 64;
const BG_PALETTE: u32 = 0x0500_0000;
/// enum CanvasTransferMode.
const CANVAS_COPY: u32 = 0;
const CANVAS_COPY_AND_CLEAR: u32 = 1;
const CANVAS_COPY_AND_FADE: u32 = 2;
const CANVAS_COPY_AND_DARKEN: u32 = 3;
const CANVAS_COPY_AND_BRIGHTEN: u32 = 4;
/// What a level-0 canvas pixel is drawn as: transparent to the hardware.
const CANVAS_EMPTY: u32 = 0x10_10_14;

/// One frame of a battle effect: the screen and what the effect keeps.
struct FxFrame {
    screen: Vec<u16>,
    /// Whether the effect's work block is allocated.
    running: bool,
    canvas: Vec<u8>,
    /// The first 64 background colours, the canvas's levels.
    ramp: Vec<u32>,
    mode: u32,
    value: u32,
    /// work->particles' (x, variant).
    shots: Vec<(i32, i32)>,
    /// PARTICLES' variants.
    pool: Vec<i32>,
}

fn read_fx(gba: &Gba, work_slots: u32, pool: u32) -> FxFrame {
    let work = gba.read32(work_slots + 4 * HEAP_SLOT_BATTLE_ANIM);
    let running = work != 0;
    let mut canvas = vec![0u8; CANVAS_SIDE * CANVAS_SIDE];
    for (i, px) in canvas.iter_mut().enumerate() {
        let (x, y) = (i % CANVAS_SIDE, i / CANVAS_SIDE);
        let tile = (y / 8) * 16 + x / 8;
        *px = gba.read8(CANVAS_VRAM + (tile * 64 + (y % 8) * 8 + x % 8) as u32);
    }
    let ramp = (0..LEVELS).map(|i| super::animation::rgb(gba.read16(BG_PALETTE + 2 * i))).collect();
    let (mode, value, shots) = if running {
        let shots = (0..WORK_PARTICLE_COUNT)
            .map(|i| {
                let s = work + FX_PARTICLES + STEP_SIZE * i;
                (gba.read32(s + STEP_X) as i32, gba.read32(s + STEP_VARIANT) as i32)
            })
            .collect();
        (gba.read32(work + FX_TRANSFER_MODE), gba.read32(work + FX_TRANSFER_VALUE), shots)
    } else {
        (0, 0, Vec::new())
    };
    let pool = (0..VOLLEY_PARTICLES).map(|i| gba.read32(pool + STEP_SIZE * i + STEP_VARIANT) as i32).collect();
    FxFrame { screen: gba.picture(), running, canvas, ramp, mode, value, shots, pool }
}

fn canvas_image(frame: &FxFrame) -> Image {
    let pixels = frame.canvas.iter().map(|&l| if l == 0 { CANVAS_EMPTY } else { frame.ramp[usize::from(l) % LEVELS as usize] }).collect();
    Image { width: CANVAS_SIDE, height: CANVAS_SIDE, pixels }
}

fn mode_name(mode: u32, value: u32) -> String {
    match mode {
        CANVAS_COPY => "CANVAS_COPY".to_string(),
        CANVAS_COPY_AND_CLEAR => format!("CANVAS_COPY_AND_CLEAR {value}"),
        CANVAS_COPY_AND_FADE => format!("CANVAS_COPY_AND_FADE {value}"),
        CANVAS_COPY_AND_DARKEN => format!("CANVAS_COPY_AND_DARKEN {value:#010x}"),
        CANVAS_COPY_AND_BRIGHTEN => format!("CANVAS_COPY_AND_BRIGHTEN {value:#010x}"),
        _ => format!("MODE {mode}"),
    }
}

/// Frames kept before the effect's work block is taken and after it is
/// released.
const FX_MARGIN: usize = 10;

impl<'a> Guide<'a> {
    /// Garet's Heat Wave at Mia, as `render psynergy` casts it, with each
    /// frame's effect state; `force`, when given, is written over
    /// work->transferMode and transferValue each time
    /// BattleAnim_ProcessPendingGraphicsTransfer runs. Only the frames from
    /// shortly before the effect's work block is taken to shortly after it
    /// is released are kept.
    fn heat_wave(&self, force: Option<(u32, u32)>) -> Result<Vec<FxFrame>, String> {
        self.heat_wave_copies(force).map(|(frames, _)| frames)
    }

    /// As `heat_wave`, with each canvas the effect presented, read from its
    /// heap block as the transfer callback starts to copy it
    /// (work->transferPending set), one an effect frame, before the mode
    /// treats it.
    fn heat_wave_copies(&self, force: Option<(u32, u32)>) -> Result<(Vec<FxFrame>, Vec<Image>), String> {
        let root = crate::compile::root();
        let plan = Tables::read(root)?.plan()?;
        let cast: Cast = plan.casts.into_iter().find(|c| c.ability.name == "Heat Wave").ok_or("no Heat Wave cast")?;
        let symbols = battle::Symbols::find(self.map)?;
        let work_slots = self.sym("gHeapSlots")?;
        let pool = self.sym("gMapGridCellBuffer")?;
        let transfer = self.sym("BattleAnim_ProcessPendingGraphicsTransfer")?;
        let mut copies: Vec<Image> = Vec::new();
        let mut copy_frames: Vec<usize> = Vec::new();
        let mut frames: Vec<FxFrame> = Vec::new();
        battle::record_watching(self.rom, &symbols, &cast, &Scene::Game, false, None, &[transfer], &mut |gba, hook| match hook {
            Some(_) => {
                let work = gba.read32(work_slots + 4 * HEAP_SLOT_BATTLE_ANIM);
                if work != 0 && gba.read32(work + FX_TRANSFER_PENDING) != 0 {
                    let block = gba.read32(work_slots + 4 * HEAP_SLOT_BATTLE_CANVAS);
                    let mut frame = read_fx(gba, work_slots, pool);
                    for (i, px) in frame.canvas.iter_mut().enumerate() {
                        let (x, y) = (i % CANVAS_SIDE, i / CANVAS_SIDE);
                        *px = gba.read8(block + ((y / 8) * 16 * 64 + (x / 8) * 64 + (y % 8) * 8 + x % 8) as u32);
                    }
                    copies.push(canvas_image(&frame));
                    copy_frames.push(frames.len());
                }
                if let Some((mode, value)) = force {
                    let work = gba.read32(work_slots + 4 * HEAP_SLOT_BATTLE_ANIM);
                    if work != 0 {
                        gba.write32(work + FX_TRANSFER_MODE, mode);
                        gba.write32(work + FX_TRANSFER_VALUE, value);
                    }
                }
            }
            None => frames.push(read_fx(gba, work_slots, pool)),
        })?;
        if let (Some(a), Some(b)) = (copy_frames.first(), copy_frames.last()) {
            eprintln!("  {} canvases presented over screen frames {a}..={b}", copy_frames.len());
        }
        let first = frames.iter().position(|f| f.running).ok_or("the effect never ran")?;
        let last = frames.iter().rposition(|f| f.running).unwrap_or(first);
        let (from, to) = (first.saturating_sub(FX_MARGIN), (last + FX_MARGIN).min(frames.len() - 1));
        Ok((frames.drain(from..=to).collect(), copies))
    }

    /// Heat Wave's canvas beside the battle it is blended over, as
    /// `effect-canvas.gif`: the screen at twice its size, and the 128 by 128
    /// canvas as character block 1 holds it, each byte drawn in the colour
    /// its level names, which BG2 shows stretched to twice its width.
    pub fn effect_canvas(&mut self) -> Result<(), String> {
        let frames = self.heat_wave(None)?;
        let mut out = Vec::new();
        for f in &frames {
            let mut image = canvas(2 * WIDTH + 2 * CANVAS_SIDE + 24, 2 * HEIGHT + 24, PANEL_BACK);
            paste(&mut image, &screen(&f.screen), 0, 0, 2);
            let x = 2 * WIDTH + 12;
            text(&mut image, x as i32, 4, "THE CANVAS, 128 X 128", PANEL_HEAD, None, 2);
            text(&mut image, x as i32, 20, "A BYTE A PIXEL: A LEVEL", PANEL_DIM, None, 2);
            paste(&mut image, &canvas_image(f), x, 40, 2);
            for (l, &c) in f.ramp.iter().enumerate() {
                for dy in 0..10 {
                    for dx in 0..4 {
                        let (px, py) = (x + 4 * l + dx, 40 + 2 * CANVAS_SIDE + 6 + dy);
                        image.pixels[py * image.width + px] = if l == 0 { CANVAS_EMPTY } else { c };
                    }
                }
            }
            text(&mut image, x as i32, (40 + 2 * CANVAS_SIDE + 18) as i32, "LEVELS 0-63", PANEL_DIM, None, 2);
            text(&mut image, 4, (2 * HEIGHT + 6) as i32, "THE SCREEN: BG2 SHOWS THE CANVAS TWICE AS WIDE, BLENDED", PANEL_DIM, None, 2);
            out.push(image.pixels);
        }
        let width = 2 * WIDTH + 2 * CANVAS_SIDE + 24;
        let written = super::animation::write(&self.out.join("effect-canvas.gif"), &out, width)?;
        self.written.push(("effect-canvas.gif".to_string(), written.bytes, format!("Heat Wave beside its canvas; {} frames", written.frames)));
        Ok(())
    }

    /// Heat Wave played six times, its canvas transfer forced each time to
    /// another mode of enum CanvasTransferMode, the six canvases side by side
    /// frame for frame, as `effect-transfer.gif`. Its own is
    /// CANVAS_COPY_AND_FADE 50. The forced runs draw the same pictures in the
    /// same places, since the transfer draws no random numbers.
    pub fn effect_transfer(&mut self) -> Result<(), String> {
        let modes: [(u32, u32); 6] = [
            (CANVAS_COPY, 0),
            (CANVAS_COPY_AND_CLEAR, 0),
            (CANVAS_COPY_AND_FADE, 50),
            (CANVAS_COPY_AND_FADE, 75),
            (CANVAS_COPY_AND_DARKEN, 0x0404_0404),
            (CANVAS_COPY_AND_BRIGHTEN, 0x0101_0101),
        ];
        let natural = self.heat_wave(None)?;
        let own = natural.iter().find(|f| f.running && f.mode != 0).map(|f| (f.mode, f.value));
        // Each run's canvases one an effect frame, so the runs stay in step
        // even where a slower transfer makes an effect frame take longer.
        eprintln!("effect-transfer: Heat Wave's own transfer {own:?}");
        let mut runs = Vec::new();
        for &m in &modes {
            let (_, run) = self.heat_wave_copies(Some(m))?;
            eprintln!("  forced {}: {} frames", mode_name(m.0, m.1), run.len());
            runs.push(run);
        }
        let n = runs.iter().map(Vec::len).min().unwrap_or(0);
        const PANEL_W: usize = 2 * CANVAS_SIDE + 12;
        const PANEL_H: usize = 2 * CANVAS_SIDE + 38;
        let (w, h) = (3 * PANEL_W + 12, 2 * PANEL_H + 12);
        let mut out = Vec::new();
        for i in 0..n {
            let mut image = canvas(w, h, PANEL_BACK);
            for (k, (run, &(mode, value))) in runs.iter().zip(&modes).enumerate() {
                let (x, y) = (12 + (k % 3) * PANEL_W, 8 + (k / 3) * PANEL_H);
                let colour = if Some((mode, value)) == own { PANEL_MARK } else { PANEL_TEXT };
                let name = mode_name(mode, value);
                let (first, second) = name.split_once(' ').unwrap_or((&name, ""));
                text(&mut image, x as i32 - 4, y as i32, first, colour, None, 2);
                let second = if Some((mode, value)) == own { format!("{second}  HEAT WAVE'S OWN") } else { second.to_string() };
                text(&mut image, x as i32 - 4, y as i32 + 16, &second, colour, None, 2);
                paste(&mut image, &run[i], x, y + 34, 2);
            }
            // An effect frame lasts two of the screen's.
            out.push(image.pixels.clone());
            out.push(image.pixels);
        }
        let written = super::animation::write(&self.out.join("effect-transfer.gif"), &out, w)?;
        self.written.push(("effect-transfer.gif".to_string(), written.bytes, format!("Heat Wave's canvas under each transfer mode; {} frames", written.frames)));
        Ok(())
    }

    /// Heat Wave beside its particle pools, as `effect-particles.gif`: the
    /// 64 of work->particles, which hold the seven fireballs, and the first
    /// 1,024 of PARTICLES, the volley's hit flashes from 0, sparks from 100
    /// and fireball trails from 512, each slot lit while its variant is
    /// above 0 (a flash, a spark's or trail's life) and dark when free. The
    /// pool is drawn from the frame the volley frees it, before which it
    /// holds the map's cells.
    pub fn effect_particles(&mut self) -> Result<(), String> {
        let frames = self.heat_wave(None)?;
        let start = frames.iter().position(|f| f.running && f.pool.iter().all(|&v| v == 0)).ok_or("the volley never freed its pool")?;
        let mut out = Vec::new();
        let mut first_x: Vec<Option<i32>> = vec![None; WORK_PARTICLE_COUNT as usize];
        const CELL: usize = 8;
        let (w, h) = (2 * WIDTH + 32 * CELL + 24, 2 * HEIGHT + 24);
        for (i, f) in frames.iter().enumerate() {
            let mut image = canvas(w, h, PANEL_BACK);
            paste(&mut image, &screen(&f.screen), 0, 0, 2);
            let x0 = 2 * WIDTH + 12;
            let live = i >= start && f.running;
            text(&mut image, x0 as i32, 4, "WORK->PARTICLES, 64", PANEL_HEAD, None, 2);
            for s in 0..WORK_PARTICLE_COUNT as usize {
                let (cx, cy) = (x0 + (s % 32) * CELL, 22 + (s / 32) * CELL);
                let colour = match f.shots.get(s) {
                    Some(&(x, v)) if live && s < 7 => {
                        let moved = first_x[s].is_some_and(|f0| f0 != x);
                        if first_x[s].is_none() {
                            first_x[s] = Some(x);
                        }
                        if v == 1 { 0xff_ff_ff } else if moved { 0xff_70_20 } else { 0x60_40_30 }
                    }
                    _ => 0x2a_2c_33,
                };
                fill(&mut image, cx, cy, CELL - 1, CELL - 1, colour);
            }
            let y0 = 22 + 2 * CELL + 10;
            text(&mut image, x0 as i32, y0 as i32, "PARTICLES, 1024", PANEL_HEAD, None, 2);
            for s in 0..VOLLEY_PARTICLES as usize {
                let (cx, cy) = (x0 + (s % 32) * CELL, y0 + 18 + (s / 32) * CELL);
                let v = if live { f.pool[s] } else { 0 };
                let colour = if v <= 0 {
                    if s < VOLLEY_FIRST_SPARK { 0x2a_2c_33 } else if s < VOLLEY_FIRST_TRAIL { 0x30_30_28 } else { 0x33_2a_2a }
                } else if s < VOLLEY_FIRST_SPARK {
                    0xff_ff_ff
                } else if s < VOLLEY_FIRST_TRAIL {
                    mix(0x60_50_10, 0xff_e0_40, (f64::from(v) / 23.0).min(1.0))
                } else {
                    mix(0x60_10_10, 0xff_40_30, (f64::from(v) / 39.0).min(1.0))
                };
                fill(&mut image, cx, cy, CELL - 1, CELL - 1, colour);
            }
            let used = |r: std::ops::Range<usize>| if live { f.pool[r].iter().filter(|&&v| v > 0).count() } else { 0 };
            let legend = format!("FLASHES {}  SPARKS {}  TRAILS {}", used(0..VOLLEY_FIRST_SPARK), used(VOLLEY_FIRST_SPARK..VOLLEY_FIRST_TRAIL), used(VOLLEY_FIRST_TRAIL..VOLLEY_PARTICLES as usize));
            text(&mut image, 4, (2 * HEIGHT + 6) as i32, &legend, PANEL_TEXT, None, 2);
            if live && i % 4 == 0 {
                eprintln!("  frame {i}: {legend}");
            }
            out.push(image.pixels);
        }
        let written = super::animation::write(&self.out.join("effect-particles.gif"), &out, w)?;
        self.written.push(("effect-particles.gif".to_string(), written.bytes, format!("Heat Wave beside its particle pools; {} frames", written.frames)));
        Ok(())
    }
}

fn fill(image: &mut Image, x: usize, y: usize, w: usize, h: usize, colour: u32) {
    for py in y..(y + h).min(image.height) {
        for px in x..(x + w).min(image.width) {
            image.pixels[py * image.width + px] = colour;
        }
    }
}

/// struct MotionObject's sprite (motobj.h) and struct Sprite and
/// SpriteAnim (animspr.h).
const MOTION_RECORDS: u32 = 0x50;
const MOTION_RECORD_KIND: u32 = 0x54;
const OBJECT_SPRITES_ONE: u8 = 1;
const SPRITE_IMAGE_ATTR2: u32 = 0x08;
const SPRITE_WIDTH: u32 = 0x20;
const SPRITE_HEIGHT: u32 = 0x21;
const SPRITE_DIRTY: u32 = 0x25;
const SPRITE_COUNT: u32 = 0x27;
const SPRITE_ENTRIES: u32 = 0x28;
const ENTRY_ANIM_ID: u32 = 0x00;
const ENTRY_PRIORITY: u32 = 0x06;
const ENTRY_STEP: u32 = 0x15;
const ENTRY_FRAME: u32 = 0x16;
/// A layer whose priority is above 3 is not drawn (Sprite_ComposeFrame).
const HIDDEN_PRIORITY: u8 = 9;
const OBJ_TILES: u32 = 0x0601_0000;
const OBJ_PALETTE: u32 = 0x0500_0200;
/// What a sprite's colour 0, transparent to the hardware, is drawn as.
const SPRITE_BACK: u32 = 0x58_5e_6a;

/// One layer of a sprite: its sprite id, its frame (255 for none) and its
/// priority.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Layer {
    anim: i16,
    frame: u8,
    priority: u8,
}

/// The composed buffer of a 256-colour sprite as video memory holds it:
/// `width` by `height` pixels in 8 by 8 tiles of 64 bytes, row by row.
fn sprite_tiles(gba: &Gba, sprite: u32) -> Image {
    let (w, h) = (usize::from(gba.read8(sprite + SPRITE_WIDTH)), usize::from(gba.read8(sprite + SPRITE_HEIGHT)));
    let tile = u32::from(gba.read16(sprite + SPRITE_IMAGE_ATTR2) & 0x3ff);
    let base = OBJ_TILES + (tile << 5);
    let palette: Vec<u32> = (0..256).map(|i| super::animation::rgb(gba.read16(OBJ_PALETTE + 2 * i))).collect();
    let mut image = canvas(w, h, SPRITE_BACK);
    for y in 0..h {
        for x in 0..w {
            let at = ((y / 8) * (w / 8) + x / 8) * 64 + (y % 8) * 8 + x % 8;
            let index = usize::from(gba.read8(base + at as u32));
            if index != 0 {
                image.pixels[y * w + x] = palette[index];
            }
        }
    }
    image
}

fn layers(gba: &Gba, sprite: u32) -> Vec<Option<Layer>> {
    (0..u32::from(gba.read8(sprite + SPRITE_COUNT)).min(4))
        .map(|i| {
            let e = gba.read32(sprite + SPRITE_ENTRIES + 4 * i);
            (e != 0).then(|| Layer { anim: gba.read16(e + ENTRY_ANIM_ID) as i16, frame: gba.read8(e + ENTRY_FRAME), priority: gba.read8(e + ENTRY_PRIORITY) })
        })
        .collect()
}

/// A sprite taken apart: each drawn layer composed alone, and all of them.
struct SpriteApart {
    layers: Vec<Option<Layer>>,
    alone: Vec<Option<Image>>,
    composed: Image,
}

/// Runs the machine to the game's next call of Sprite_ComposeFrame
/// for `sprite` and there lets it draw only layer `only` (the others'
/// priority raised past 3), its scripts held still (every layer's step 0);
/// reads the buffer back from video memory when the call returns, and puts
/// the machine back as it was. None if no call comes within two frames.
fn compose_alone(gba: &mut Gba, sprite: u32, only: u32, compose: u32) -> Option<Image> {
    let saved = gba.save();
    let entries: Vec<u32> = (0..u32::from(gba.read8(sprite + SPRITE_COUNT)).min(4)).map(|i| gba.read32(sprite + SPRITE_ENTRIES + 4 * i)).collect();
    let mut back = 0;
    let mut image = None;
    let start = gba.step().frame;
    loop {
        let step = gba.step();
        if step.frame > start + 2 {
            break;
        }
        if back == 0 && step.pc == compose && gba.register(0) == sprite {
            // The call for this sprite: hide the other layers, hold the
            // scripts still and make it redraw.
            for (i, &e) in entries.iter().enumerate() {
                if e != 0 {
                    gba.write8(e + ENTRY_STEP, 0);
                    if i as u32 != only {
                        gba.write8(e + ENTRY_PRIORITY, HIDDEN_PRIORITY);
                    }
                }
            }
            gba.write8(sprite + SPRITE_DIRTY, 1);
            back = gba.register(super::mgba::LR) & !1;
        } else if back != 0 && step.pc == back {
            image = Some(sprite_tiles(gba, sprite));
            break;
        }
    }
    gba.load(&saved);
    image
}

impl<'a> Guide<'a> {
    /// Isaac attacking from the battle test menu (Fight, Attack, the aimed
    /// enemy), his battle sprite taken apart each frame one of its layers
    /// changes: the body (entries[0], BattleObject.resource) and the weapon
    /// (entries[1], BattleObject.overlay) each composed alone by the game's own
    /// Sprite_ComposeFrame, the other layer's priority raised past
    /// 3 and the scripts held still, and the buffer it composes from both,
    /// read back from the sprite's tiles in video memory, beside the screen,
    /// as `sprite-layers.gif`.
    pub fn sprite_layers(&mut self) -> Result<(), String> {
        let root = crate::compile::root();
        let plan = Tables::read(root)?.plan()?;
        let mut cast: Cast = plan.casts.into_iter().find(|c| c.ability.name == "Quake").ok_or("no Quake cast")?;
        cast.foes = vec![plan::IVAN];
        let symbols = battle::Symbols::find(self.map)?;
        let compose = self.sym("Sprite_ComposeFrame")?;
        let work_slot = self.sym("gBattleWork")?;
        let script = [(0, 20), (KEY_A, 4), (0, 30), (KEY_A, 4), (0, 30), (KEY_A, 4), (0, 170)];
        let mut aparts: Vec<SpriteApart> = Vec::new();
        // Each recorded frame: the screen and the take-apart it shows.
        let mut timeline: Vec<(Vec<u16>, usize)> = Vec::new();
        let mut last: Option<Vec<Option<Layer>>> = None;
        battle::record_watching(self.rom, &symbols, &cast, &Scene::Game, false, Some(&script), &[], &mut |gba, hook| {
            if hook.is_some() {
                return;
            }
            let work = gba.read32(work_slot);
            if work == 0 {
                return;
            }
            // Isaac is party unit 0: his battle object's single sprite.
            let slot = work + WORK_SLOTS + SLOT_SIZE * u32::from(gba.read8(work + WORK_PLACEMENT));
            let object = gba.read32(slot);
            if object == 0 || gba.read8(object + MOTION_RECORD_KIND) & 15 != OBJECT_SPRITES_ONE {
                return;
            }
            let sprite = gba.read32(object + MOTION_RECORDS);
            let now = layers(gba, sprite);
            if last.as_ref() != Some(&now) {
                last = Some(now.clone());
                let composed = sprite_tiles(gba, sprite);
                let alone = now.iter().enumerate().map(|(i, l)| l.filter(|l| l.priority <= 3 && l.frame != 255).and_then(|_| compose_alone(gba, sprite, i as u32, compose))).collect();
                aparts.push(SpriteApart { layers: now, alone, composed });
            }
            timeline.push((gba.picture(), aparts.len() - 1));
        })?;
        eprintln!("sprite-layers: {} frames, {} changes", timeline.len(), aparts.len());
        for a in &aparts {
            eprintln!("  {:?} {}x{} alone {:?}", a.layers, a.composed.width, a.composed.height, a.alone.iter().map(Option::is_some).collect::<Vec<_>>());
        }
        // Which BattleObject field each layer came from, in the order
        // BattleObj_SpawnObjectsForList registers them for a one-part body
        // with no status icon.
        const ROLES: [&str; 3] = ["RESOURCE", "OVERLAY", "EFFECT"];
        const SCALE: usize = 2;
        const TOP: usize = 60;
        let side = aparts.iter().map(|a| a.composed.width.max(a.composed.height)).max().unwrap_or(64);
        let cell = side * SCALE + 16;
        let count = aparts.iter().map(|a| a.layers.len()).max().unwrap_or(1);
        let (w, h) = (2 * WIDTH + (count + 1) * cell + 16, 2 * HEIGHT);
        let panels_of = |a: &SpriteApart| -> Image {
            let mut image = canvas(w, h, PANEL_BACK);
            let mut panels: Vec<([String; 3], Option<&Image>)> = Vec::new();
            for (i, l) in a.layers.iter().enumerate() {
                let role = ROLES.get(i).map_or(String::new(), |r| format!(".{r}"));
                let lines = match l {
                    Some(l) if l.frame != 255 => [format!("ENTRIES[{i}]"), format!("SPRITE {} {role}", l.anim), format!("FRAME {} PRIORITY {}", l.frame, l.priority)],
                    Some(l) => [format!("ENTRIES[{i}]"), format!("SPRITE {} {role}", l.anim), "NO FRAME".to_string()],
                    None => [format!("ENTRIES[{i}]"), "EMPTY".to_string(), String::new()],
                };
                panels.push((lines, a.alone.get(i).and_then(Option::as_ref)));
            }
            panels.push((["COMPOSED".to_string(), format!("{} X {} PIXELS", a.composed.width, a.composed.height), "READ FROM VRAM".to_string()], Some(&a.composed)));
            for (k, (lines, picture)) in panels.iter().enumerate() {
                let x = 2 * WIDTH + 12 + k * cell;
                text(&mut image, x as i32, 6, &lines[0], if k == count { PANEL_MARK } else { PANEL_HEAD }, None, 2);
                text(&mut image, x as i32, 22, &lines[1], PANEL_DIM, None, 1);
                text(&mut image, x as i32, 34, &lines[2], PANEL_DIM, None, 1);
                if let Some(p) = picture {
                    paste(&mut image, p, x, TOP, SCALE);
                }
                if k < count {
                    text(&mut image, (x + cell - 14) as i32, (TOP + side * SCALE / 2) as i32, if k + 1 < count { "+" } else { "=" }, PANEL_TEXT, None, 2);
                }
            }
            image
        };
        let backs: Vec<Image> = aparts.iter().map(panels_of).collect();
        let mut out = Vec::new();
        for (picture, k) in &timeline {
            let mut image = Image { width: w, height: h, pixels: backs[*k].pixels.clone() };
            paste(&mut image, &screen(picture), 0, 0, 2);
            out.push(image.pixels);
        }
        let written = super::animation::write(&self.out.join("sprite-layers.gif"), &out, w)?;
        self.written.push(("sprite-layers.gif".to_string(), written.bytes, format!("Isaac's battle sprite taken apart; {} frames", written.frames)));
        Ok(())
    }
}

/// Bilibin's weapon and armor shop (MapId_BilibinInteriors, entrance
/// BILIBIN_INTERIORS_WEAPON_AND_ARMOR_SHOP) and its weapon merchant
/// (BILIBIN_WEAPON_MERCHANT), whose counter opens stock row 7; Vault's
/// interiors (MapId_VaultInteriors, from its east-southeast door) and its
/// weapon merchant (LOCALID_WEAPON_MERCHANT), whose counter opens row 4.
const BILIBIN_WEAPON_SHOP: (u16, u16) = (31, 4);
const BILIBIN_WEAPON_MERCHANT: u16 = 16;
const BILIBIN_WEAPON_ROW: usize = 7;
const VAULT_SHOPS: (u16, u16) = (21, 5);
const VAULT_WEAPON_MERCHANT: u16 = 19;
/// Facing north, 1/65536 turns: across a counter.
const FACING_NORTH: u32 = 0xc000;
/// gItemCounters (ram_buffer.h): a byte for each artifact's counter.
const ITEM_COUNTERS: u32 = 128;

impl<'a> Guide<'a> {
    /// Stands across `merchant`'s counter in `place` with 2,000 coins, after
    /// `counters` (when given) are written over gItemCounters, and talks to
    /// him; returns the field once `path` has been pressed, every frame kept.
    fn at_counter(&self, place: (u16, u16), merchant: u16, counters: Option<&[u8]>, path: &[(u32, u32)]) -> Result<(super::field::Field<'a>, Vec<Vec<u16>>, Vec<u8>), String> {
        let (mut entered, _) = self.stand_where(place, super::story::Trigger::Talk(merchant), merchant, |s| s.facing == FACING_NORTH)?;
        entered.field.set_coins(2000);
        if let Some(c) = counters {
            let base = self.sym("gItemCounters")?;
            for (i, &v) in c.iter().enumerate() {
                entered.field.write8(base + i as u32, v);
            }
        }
        let before = self.counters(&entered.field)?;
        let frames = Guide::press_path(&mut entered.field, path)?;
        Ok((entered.field, frames, before))
    }

    fn counters(&self, field: &super::field::Field) -> Result<Vec<u8>, String> {
        let base = self.sym("gItemCounters")?;
        Ok((0..ITEM_COUNTERS).map(|i| field.read8(base + i)).collect())
    }


    /// The items each counter of gItemCounters belongs to, from the ROM's
    /// gItem_ArtifactSlotTable (an item's counter plus one, 0 for none).
    fn counter_items(&self, field: &super::field::Field) -> Result<Vec<(usize, u16)>, String> {
        let table = self.sym("gItem_ArtifactSlotTable")?;
        Ok((0..ITEM_IDS).filter_map(|item| {
            let c = field.read8(table + item);
            (c != 0).then(|| (usize::from(c) - 1, item as u16))
        }).collect())
    }

    /// Bilibin's weapon shop, stock row 7, opened twice from a new game:
    /// first as it is, then with gItemCounters as they stand after Vault's
    /// weapon shop (row 4) has been opened for the first time in a run of
    /// its own. Each time the Buy list and the Artifacts list are shown, as
    /// `shop-counters.png`, with the row's items and artifacts read from the
    /// shop's work block (ShopWork.stockItemIds) and the counters above
    /// zero.
    pub fn shop_counters(&mut self) -> Result<(), String> {
        let names = item_names()?;
        let name = |id: u16| names.get(usize::from(id)).cloned().unwrap_or(format!("ITEM {id}"));
        // Vault's weapon shop, opened once: its first visit stocks row 4.
        // The merchant stands in the room of one of the interiors' doors.
        let vault = (5..=13u16)
            .find_map(|e| {
                let r = self.at_counter((VAULT_SHOPS.0, e), VAULT_WEAPON_MERCHANT, None, &[(0, 20), (KEY_A, 3), (0, 140)]);
                if let Err(err) = &r {
                    eprintln!("  Vault entrance {e}: {err}");
                }
                r.ok()
            })
            .ok_or("no Vault entrance reaches the weapon merchant")?
            .0;
        let after_vault = self.counters(&vault)?;
        let owners = self.counter_items(&vault)?;
        let stocked = |c: &[u8]| -> Vec<String> {
            c.iter().enumerate().filter(|(_, &v)| v != 0).map(|(i, &v)| {
                let item = owners.iter().find(|(k, _)| *k == i).map_or(format!("COUNTER {i}"), |&(_, id)| name(id));
                format!("{item}: {v}")
            }).collect()
        };
        eprintln!("shop-counters: after Vault's weapon shop {:?}", stocked(&after_vault));
        // Talk; Buy; back out; along to Artifacts; through its line.
        let buy: [(u32, u32); 5] = [(0, 20), (KEY_A, 3), (0, 100), (KEY_A, 3), (0, 60)];
        let artifacts: [(u32, u32); 10] = [(KEY_B, 3), (0, 60), (KEY_RIGHT, 3), (0, 15), (KEY_RIGHT, 3), (0, 15), (KEY_A, 3), (0, 90), (KEY_A, 3), (0, 60)];
        let menu = self.sym("gMenuWork")?;
        struct Visit {
            label: &'static str,
            buy: Vec<u16>,
            artifacts: Vec<u16>,
            before: Vec<String>,
            added: Vec<String>,
            on_sale: Vec<u16>,
            listed: Vec<u16>,
        }
        let mut visits: Vec<Visit> = Vec::new();
        for (label, counters) in [("A NEW GAME", None), ("AFTER VAULT'S WEAPON SHOP", Some(after_vault.as_slice()))] {
            let (mut field, frames, before) = self.at_counter(BILIBIN_WEAPON_SHOP, BILIBIN_WEAPON_MERCHANT, counters, &buy)?;
            let list = |field: &super::field::Field| -> Vec<u16> {
                let work = field.read32(menu);
                (0..64).map(|i| field.read16(work + SHOP_STOCK_ITEMS + 2 * i)).take_while(|&id| id != 0).collect()
            };
            let on_sale = list(&field);
            let opened = self.counters(&field)?;
            let added: Vec<u8> = opened.iter().zip(&before).map(|(a, b)| a.saturating_sub(*b)).collect();
            let buy_screen = frames.last().cloned().ok_or("no frames")?;
            let more = Guide::press_path(&mut field, &artifacts)?;
            let listed = list(&field);
            let artifact_screen = more.last().cloned().ok_or("no frames")?;
            eprintln!(
                "  {label}: before {:?}; the visit adds {:?}; Buy {:?}; Artifacts {:?}",
                stocked(&before),
                stocked(&added),
                on_sale.iter().map(|&i| name(i)).collect::<Vec<_>>(),
                listed.iter().map(|&i| name(i)).collect::<Vec<_>>()
            );
            visits.push(Visit { label, buy: buy_screen, artifacts: artifact_screen, before: stocked(&before), added: stocked(&added), on_sale, listed });
        }
        const SIDE: usize = 360;
        let (w, h) = (SIDE + 4 * WIDTH + 24, 40 + 2 * (2 * HEIGHT + 40));
        let mut image = canvas(w, h, PANEL_BACK);
        text(&mut image, 8, 8, &format!("BILIBIN WEAPONS, STOCK ROW {BILIBIN_WEAPON_ROW}"), PANEL_HEAD, None, 2);
        text(&mut image, (SIDE + 8) as i32, 8, "BUY", PANEL_HEAD, None, 2);
        text(&mut image, (SIDE + 2 * WIDTH + 16) as i32, 8, "ARTIFACTS", PANEL_HEAD, None, 2);
        for (r, v) in visits.iter().enumerate() {
            let y = 40 + r * (2 * HEIGHT + 40);
            paste(&mut image, &screen(&v.buy), SIDE + 8, y, 2);
            paste(&mut image, &screen(&v.artifacts), SIDE + 2 * WIDTH + 16, y, 2);
            let mut ty = y as i32;
            let mut line = |image: &mut Image, indent: i32, s: &str, colour: u32, gap: i32| {
                text(image, 8 + indent, ty, &s.to_uppercase(), colour, None, 2);
                ty += 16 + gap;
            };
            line(&mut image, 0, v.label, PANEL_MARK, 8);
            line(&mut image, 0, "COUNTERS BEFORE TALKING:", PANEL_DIM, 0);
            if v.before.is_empty() {
                line(&mut image, 8, "NONE ABOVE 0", PANEL_TEXT, 0);
            }
            for c in &v.before {
                line(&mut image, 8, c, PANEL_TEXT, 0);
            }
            line(&mut image, 0, "", PANEL_DIM, -8);
            line(&mut image, 0, "THE SHOP'S FIRST VISIT ADDS:", PANEL_DIM, 0);
            for c in &v.added {
                line(&mut image, 8, &format!("{c}  (ROW {BILIBIN_WEAPON_ROW})"), PANEL_TEXT, 0);
            }
            line(&mut image, 0, "", PANEL_DIM, -8);
            line(&mut image, 0, &format!("BUY: THE ROW'S {} ITEMS", v.on_sale.len()), PANEL_DIM, 0);
            line(&mut image, 0, "ARTIFACTS: EACH WEAPON WHOSE", PANEL_DIM, 0);
            line(&mut image, 0, "COUNTER IS ABOVE 0:", PANEL_DIM, 0);
            for &id in &v.listed {
                line(&mut image, 8, &name(id), PANEL_TEXT, 0);
            }
        }
        self.guide_png("shop-counters.png", &image, "Bilibin's weapon shop before and after Vault's")
    }
}

/// ShopWork.stockItemIds (shop.h).
const SHOP_STOCK_ITEMS: u32 = 0x26c;
/// Every 9-bit item id, as Shop_ListAvailableArtifacts tries them.
const ITEM_IDS: u32 = 512;

/// Item names by id, from items.tsv's `item` column.
pub(super) fn item_names() -> Result<Vec<String>, String> {
    let path = crate::compile::root().join("games/tbs/src/game/items.tsv");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut lines = text.lines();
    let column = lines.next().unwrap_or("").split('\t').position(|c| c == "item").ok_or("items.tsv has no item column")?;
    Ok(lines.map(|l| l.split('\t').nth(column).unwrap_or("").to_string()).collect())
}
