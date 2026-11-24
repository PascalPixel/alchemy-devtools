//! The events of a game the story tools write no events.tsv for (The Lost
//! Age), read from the running game: each scene is entered by each of its
//! entrances, and each row of the trigger table its field then holds
//! (FieldState.triggers) becomes an event, named by the function it runs
//! (the overlay's symbol) or the message it shows (the edition's `Msg`
//! symbol, or the message's number). Rows of one name are one event, at
//! every scene that has them, with each trigger and condition its rows
//! give. What an event tests, sets and clears, and the messages a
//! function shows, are not known: an event counts once its own function
//! has run and shown itself (places.rs).
//!
//! The table is written as events.tsv is (`triggers.tsv` in the output
//! folder), so the processes of --jobs and a reader share it.

use super::field::{result_key, Field, Setup};
use super::story::Scene;
use std::collections::HashMap;

/// A row's kinds (enum TriggerKind): the A button on an actor, a step, the
/// A button on a cell, a field Psynergy, and a trigger raised by code.
const TALK: u32 = 0;
const STEP: u32 = 2;
const EXAMINE: u32 = 3;
const PSYNERGY: u32 = 5;
const RAISED: u32 = 6;
/// A Psynergy row on the actor the leader faces (TRIGGER_PSYNERGY_ON_ACTOR);
/// its ability in control bits 15..8.
const ON_ACTOR: u32 = 0x10;
/// A Psynergy row for any target (TRIGGER_PSYNERGY_ANYWHERE: Reveal cast
/// anywhere in the room).
const ANYWHERE: u32 = 0x80;
/// A condition's flag must be set (CONDITION_FLAG_SET); -1 is none.
const CONDITION_FLAG_SET: i16 = 0x1000;

/// One row as the field reads it: its control word, its target, its
/// condition and its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub control: u32,
    pub target: u16,
    pub condition: i16,
    pub result: u32,
}

/// What starts a row's event, as events.tsv writes a trigger; None for a
/// row no event of the renderer's answers (an exit, an item).
fn trigger_text(row: &Row) -> Option<String> {
    let target = row.target & 0xff;
    match row.control & 15 {
        TALK => Some(format!("talk {target}")),
        STEP => Some(format!("touch {target}")),
        EXAMINE => Some(format!("search {target}")),
        PSYNERGY => {
            let ability = (row.control >> 8) & 0xff;
            if row.control & ANYWHERE != 0 {
                Some(format!("psynergy {ability} anywhere"))
            } else if row.control & ON_ACTOR != 0 {
                Some(format!("psynergy {ability} on {}", row.target))
            } else {
                Some(format!("psynergy {ability} on cell {target}"))
            }
        }
        RAISED => Some(format!("raised {}", row.target as i16)),
        _ => None,
    }
}

/// A row's condition as events.tsv writes one: `0x123 set`, `0x123 clear`,
/// or nothing.
fn condition_text(condition: i16) -> Option<String> {
    match condition {
        -1 => None,
        c if c & CONDITION_FLAG_SET != 0 => Some(format!("{:#05x} set", c & 0xfff)),
        c => Some(format!("{:#05x} clear", c & 0xfff)),
    }
}

/// Each scene's trigger rows, entered by each of its entrances (its
/// table's and its map's), as one list a scene with no duplicates.
fn scene_rows(rom: &[u8], symbols: &super::field::Symbols, scene: u16) -> Result<Vec<Row>, String> {
    let setup = Setup { scene, entrance: 0, set: Vec::new(), clear: Vec::new() };
    let first = Field::enter(rom, symbols, &setup)?;
    let mut numbers: Vec<u16> = vec![0];
    for e in first.entrances() {
        if e.number >= 0 && !numbers.contains(&(e.number as u16)) {
            numbers.push(e.number as u16);
        }
    }
    drop(first);
    let mut rows: Vec<Row> = Vec::new();
    for entrance in numbers {
        let setup = Setup { scene, entrance, set: Vec::new(), clear: Vec::new() };
        let Ok(mut field) = Field::enter(rom, symbols, &setup) else {
            continue;
        };
        if field.until_control().is_err() || field.scene() != scene {
            continue;
        }
        for row in field.trigger_rows() {
            if !rows.contains(&row) {
                rows.push(row);
            }
        }
    }
    Ok(rows)
}

/// The events of `scenes`' trigger tables, read from the running game on
/// as many threads as there are CPUs, as events.tsv's text; `functions`
/// names each scene's functions (its overlay's symbols) and `messages`
/// the edition's messages.
pub fn read(
    rom: &[u8],
    symbols: &super::field::Symbols,
    scenes: &[&Scene],
    functions: &HashMap<u16, HashMap<String, u32>>,
    messages: &HashMap<u32, String>,
) -> String {
    let places: Vec<u16> = scenes.iter().filter(|s| super::places::is_place(s).is_ok() && s.map != "world").map(|s| s.number).collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(places.len().max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let found: std::sync::Mutex<Vec<(u16, Vec<Row>)>> = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(&scene) = places.get(i) else {
                    break;
                };
                match scene_rows(rom, symbols, scene) {
                    Ok(rows) => found.lock().unwrap().push((scene, rows)),
                    Err(e) => eprintln!("  triggers of scene {scene}: {e}"),
                }
            });
        }
    });
    let mut found = found.into_inner().unwrap();
    found.sort_by_key(|(scene, _)| *scene);
    // name -> (scenes, alternatives as (trigger, condition), messages)
    let mut events: Vec<(String, Vec<u16>, Vec<(String, Option<String>)>, Vec<u32>)> = Vec::new();
    for (scene, rows) in found {
        let named: HashMap<u32, &String> = functions.get(&scene).map(|f| f.iter().map(|(n, &a)| (result_key(a), n)).collect()).unwrap_or_default();
        for row in rows {
            let Some(trigger) = trigger_text(&row) else {
                continue;
            };
            let key = result_key(row.result);
            // A row whose result is 0 runs nothing.
            if key == 0 {
                continue;
            }
            let (name, message) = match named.get(&key) {
                Some(name) if key >= 0x10000 => ((*name).clone(), None),
                _ if key >= 0x10000 => continue,
                _ => (messages.get(&key).cloned().unwrap_or_else(|| format!("{key:05}")), Some(key)),
            };
            let index = match events.iter().position(|e| e.0 == name) {
                Some(i) => i,
                None => {
                    events.push((name, Vec::new(), Vec::new(), Vec::new()));
                    events.len() - 1
                }
            };
            let event = &mut events[index];
            if !event.1.contains(&scene) {
                event.1.push(scene);
            }
            let alternative = (trigger, condition_text(row.condition));
            if !event.2.contains(&alternative) {
                event.2.push(alternative);
            }
            if let Some(m) = message.filter(|m| !event.3.contains(m)) {
                event.3.push(m);
            }
        }
    }
    to_tsv(&events, scenes)
}

/// The events as events.tsv writes them.
fn to_tsv(events: &[(String, Vec<u16>, Vec<(String, Option<String>)>, Vec<u32>)], scenes: &[&Scene]) -> String {
    let name = |n: u16| scenes.iter().find(|s| s.number == n).map_or_else(|| format!("#{n}"), |s| s.name());
    let mut text = String::from("scenes\tevent\ttrigger\tcondition\trequires\ttests\tmessages\tsets\tclears\n");
    for (event, at, alternatives, messages) in events {
        let scenes: Vec<String> = at.iter().map(|&n| format!("{n} {}", name(n))).collect();
        let mut triggers: Vec<&str> = Vec::new();
        for (t, _) in alternatives {
            if !triggers.contains(&t.as_str()) {
                triggers.push(t);
            }
        }
        let conditions: Vec<String> = alternatives.iter().filter_map(|(t, c)| c.as_ref().map(|c| format!("{t}: {c}"))).collect();
        let messages: Vec<String> = messages.iter().map(|m| format!("{m:05}")).collect();
        text.push_str(&format!("{}\t{event}\t{}\t{}\t\t\t{}\t\t\n", scenes.join(", "), triggers.join("; "), conditions.join(" | "), messages.join(", ")));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_written_as_triggers() {
        let row = |control, target, condition| Row { control, target, condition, result: 0 };
        assert_eq!(trigger_text(&row(0, 10, -1)).as_deref(), Some("talk 10"));
        assert_eq!(trigger_text(&row(0x602, 3, -1)).as_deref(), Some("touch 3"));
        assert_eq!(trigger_text(&row(3, 11, -1)).as_deref(), Some("search 11"));
        assert_eq!(trigger_text(&row(5 | 0x10 | 0x8d << 8, 0x408, -1)).as_deref(), Some("psynergy 141 on 1032"));
        // Reveal for any target (PSYNERGY_ANYWHERE) is no cell's.
        assert_eq!(trigger_text(&row(0x5000_0000 | 5 | 0x80 | 0x90 << 8, 0, -1)).as_deref(), Some("psynergy 144 anywhere"));
        assert_eq!(super::super::story::parse_trigger("psynergy 144 anywhere"), Ok(super::super::story::Trigger::Cast(0x90)));
        assert_eq!(trigger_text(&row(1, 4, -1)), None);
        assert_eq!(condition_text(-1), None);
        assert_eq!(condition_text(0x1000 | 0x823).as_deref(), Some("0x823 set"));
        assert_eq!(condition_text(0x823).as_deref(), Some("0x823 clear"));
    }
}
