//! `render probe scene`: what one scene shows, in either game, read from the
//! running game for naming it (H3): the scene entered by an entrance (its
//! table's or, in The Lost Age, its map's), once the player has control, as
//! a photo, with its entrances and their camera boxes, each actor on stage
//! (where it stands, its sprites) and each row of the trigger table its
//! field holds, named by the function it runs or the message it shows.

use super::field::{result_key, Field, Setup, Symbols, SETTLE};
use super::story::Scene;
use std::collections::HashMap;
use std::path::Path;

/// What the probe is asked: the scene, the entrance (the first way in when
/// none), and the flags to set first.
pub struct Ask<'a> {
    pub scene: &'a Scene,
    pub entrance: Option<u16>,
    pub set: Vec<u16>,
}

/// Plays the scene and prints what it shows; writes `<scene>-<entrance>.png`
/// into `out` and returns its file name.
pub fn scene(rom: &[u8], symbols: &Symbols, ask: &Ask, functions: &HashMap<String, u32>, messages: &HashMap<u32, String>, out: &Path) -> Result<String, String> {
    let s = ask.scene;
    let first = Field::enter(rom, symbols, &Setup { scene: s.number, entrance: 0, set: ask.set.clone(), clear: Vec::new() })?;
    let entrances = first.entrances();
    drop(first);
    println!("{} (scene {}, {}):", s.name(), s.number, s.file);
    for e in &entrances {
        let camera = if e.camera == [-1; 4] { "none".to_string() } else { format!("{:?}", e.camera) };
        println!("  entrance {}: ({}, {}), camera box {camera}{}", e.number, e.x, e.z, if e.required_flag >= 0 { format!(", needs flag {:#05x}", e.required_flag) } else { String::new() });
    }
    let entrance = ask.entrance.or_else(|| entrances.iter().find(|e| e.number > 0).or(entrances.first()).map(|e| e.number as u16)).unwrap_or(0);
    let mut field = Field::enter(rom, symbols, &Setup { scene: s.number, entrance, set: ask.set.clone(), clear: Vec::new() })?;
    field.until_control()?;
    field.frames(SETTLE)?;
    if field.scene() != s.number {
        println!("  entered by entrance {entrance}, the game went on to scene {}", field.scene());
    }
    let leader = field.leader().map_or("none".to_string(), |l| format!("({}, {}) layer {}", l.x, l.z, l.layer));
    println!("  entered by entrance {entrance}; the leader at {leader}; the view's top left {:?}", field.view());
    for id in 0..symbols.l.field_actor_count as u16 {
        if field.present(id).is_ok() {
            println!("  {}", super::probe::describe(&field, id));
        }
    }
    let by_address: HashMap<u32, &String> = functions.iter().map(|(n, &a)| (result_key(a), n)).collect();
    for row in field.trigger_rows() {
        let key = result_key(row.result);
        let name = match by_address.get(&key) {
            // An exit row's value is the exit kind its scene's exit table
            // matches.
            _ if row.control & 15 == 1 => format!("exit {}", row.result),
            Some(name) => (*name).clone(),
            None if key < 0x10000 => messages.get(&key).map_or_else(|| format!("message {key:05}"), |m| format!("{m} ({key:05})")),
            None => format!("{:#010x}", row.result),
        };
        let condition = match row.condition {
            -1 => String::new(),
            c if c & 0x1000 != 0 => format!(" while flag {:#05x} is set", c & 0xfff),
            c => format!(" while flag {c:#05x} is clear"),
        };
        println!("  trigger kind {} control {:#06x} target {}{condition}: {name}", row.control & 15, row.control, row.target);
    }
    let file = format!("{}-{entrance}.png", s.slug());
    super::places::write_png(&out.join(&file), &field.screen())?;
    Ok(file)
}
