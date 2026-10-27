//! How the party arrives in each map: the entrances other maps' exits lead
//! to, read from the source's exit tables (each overlay's SCENE_EXIT rows,
//! their names numbered in scene_exits.h; MAP_WARP rows and map_warps.h
//! before The Broken Seal's rename) and map numbers
//! (field/scenes.s). `render place` photographs a map as the party
//! arrives by one of them, rather than by the table's first row, which no
//! exit may use (ENTRANCE_DEFAULT is where the debug map select starts).

use regex::Regex;
use std::collections::HashMap;
use std::path::Path;

/// One exit into a map from another map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrival {
    /// The entrance it arrives at.
    pub dest: u16,
    /// The map whose exit table holds it (`GomaCaveEntrance`).
    pub from: String,
}

/// Maps an exit from which is no way the story walks in: the Sanctum's
/// revival, the title and the main menu, and maps the game never reaches.
fn aside(from: &str) -> bool {
    from == "Sanctum" || from == "Title" || from == "MainMenu" || from.starts_with("Unused")
}

/// The source without its comments.
fn plain(text: &str) -> String {
    let block = Regex::new(r"(?s)/\*.*?\*/").unwrap();
    let text = block.replace_all(text, " ");
    text.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n")
}

/// The enumerators `NAME = N` of a source, by name.
fn numbers(text: &str) -> HashMap<String, u16> {
    let pattern = Regex::new(r"\b([A-Z][A-Z0-9_]+)\s*=\s*(\d+)\b").unwrap();
    pattern.captures_iter(&plain(text)).filter_map(|c| Some((c[1].to_string(), c[2].parse().ok()?))).collect()
}

/// The scene names of scenes.s in row order: a row's number is its
/// SceneId_. A row without a name is an empty string.
fn map_names(text: &str) -> Vec<String> {
    let pattern = Regex::new(r"^\s*scene\s+(\w*)\s*,").unwrap();
    text.lines().filter_map(|l| pattern.captures(l).map(|c| c[1].to_string())).collect()
}

/// The exits of one source file, as (the map whose table holds it, the map
/// it leads to, its entrance's name or number), in table order.
fn warps(text: &str) -> Vec<(String, String, String)> {
    let pattern = Regex::new(r"(?:SCENE_EXITS|MAP_WARPS)\(\s*(\w+)\s*\)|(?:SCENE_EXIT|MAP_WARP)(?:_IF)?\(\s*\w+\s*,\s*(\w+)\s*,\s*(\w+)").unwrap();
    let mut from = None;
    let mut out = Vec::new();
    for c in pattern.captures_iter(&plain(text)) {
        if let Some(map) = c.get(1) {
            from = Some(map.as_str().to_string());
        } else if let Some(from) = &from {
            out.push((from.clone(), c[2].to_string(), c[3].to_string()));
        }
    }
    out
}

fn sources(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // The debug test rooms' exits lead where the story never goes.
            if path.file_name().is_some_and(|n| n != "debug") {
                sources(&path, out)?;
            }
        } else if path.extension().is_some_and(|e| e == "c") {
            out.push(path);
        }
    }
    Ok(())
}

/// Every map's arrivals from other maps, by map number, from the ☀️ source
/// at `root`.
pub fn read(root: &Path) -> Result<HashMap<u16, Vec<Arrival>>, String> {
    let game = root.join("games/tbs");
    let read = |path: &Path| std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()));
    let names = map_names(&read(&game.join("src/field/scenes.s"))?);
    let shared = numbers(&read(&game.join("include/scene_exits.h")).or_else(|_| read(&game.join("include/map_warps.h")))?);
    let mut files = Vec::new();
    sources(&game.join("src"), &mut files)?;
    files.sort();
    let mut out: HashMap<u16, Vec<Arrival>> = HashMap::new();
    for path in files {
        let text = read(&path)?;
        if !text.contains("SCENE_EXITS(") && !text.contains("MAP_WARPS(") {
            continue;
        }
        // An overlay may number a destination of its own (an enum beside
        // its entrance table).
        let own = numbers(&text);
        for (from, to, dest) in warps(&text) {
            let Some(number) = names.iter().position(|n| *n == to) else {
                continue;
            };
            let dest = dest.parse().ok().or_else(|| shared.get(&dest).or_else(|| own.get(&dest)).copied());
            let Some(dest) = dest else {
                continue;
            };
            if from != to {
                out.entry(number as u16).or_default().push(Arrival { dest, from });
            }
        }
    }
    Ok(out)
}

/// The entrances to photograph a map by, best first, of those in its
/// table (`dests`) that another map's exit leads to: lowest-numbered first,
/// Camelot numbering a map's ways in from 1 in the order of its own table,
/// and exits from the Sanctum, the menus and unused maps after every other.
/// An exit to a destination the table lacks lands on the table's first row
/// (Scene_ApplyEntrance), so that row is then a way in too, after the rest.
/// Empty when no exit leads in at all.
pub fn ranked(arrivals: &[Arrival], dests: &[i16]) -> Vec<u16> {
    let mut ways: Vec<(bool, u16)> = Vec::new();
    for a in arrivals {
        let dest = if dests.contains(&(a.dest as i16)) {
            a.dest
        } else if let Some(&row) = dests.first() {
            row as u16
        } else {
            continue;
        };
        ways.push((aside(&a.from) || !dests.contains(&(a.dest as i16)), dest));
    }
    ways.sort();
    let mut out: Vec<u16> = Vec::new();
    for (_, dest) in ways {
        if !out.contains(&dest) {
            out.push(dest);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warp_tables_are_read_by_their_maps() {
        let text = "u32 gTable[] = {\n    SCENE_EXITS(GomaCaveStream), // a note\n    SCENE_EXIT(GOMA_CAVE_STREAM_TO_GOMA_CAVE_ENTRANCE, GomaCaveEntrance,\n             GOMA_CAVE_ENTRANCE_FROM_GOMA_CAVE_STREAM),\n    /* SCENE_EXIT(X, Vale, 3), */\n    SCENE_EXITS(GomaCaveBridge),\n    MAP_WARP_IF(GOMA_CAVE_BRIDGE_TO_WORLD_MAP, WorldMap, 40, FLAG_X),\n    SCENE_EXITS_END,\n};";
        assert_eq!(
            warps(text),
            vec![
                ("GomaCaveStream".to_string(), "GomaCaveEntrance".to_string(), "GOMA_CAVE_ENTRANCE_FROM_GOMA_CAVE_STREAM".to_string()),
                ("GomaCaveBridge".to_string(), "WorldMap".to_string(), "40".to_string()),
            ]
        );
    }

    #[test]
    fn map_numbers_are_their_rows() {
        let text = "\t.macro scene name, code:req\n\t.endm\ngScenes:\n\tscene Title, 0x36f, 0x00, 1, 0x00\n\tscene , 0x370, 0x01, 1, 0x00\n\tscene WorldMap, 0x371, 0x02, 1, 0x01\n";
        assert_eq!(map_names(text), vec!["Title", "", "WorldMap"]);
    }

    #[test]
    fn enumerators_may_wrap_onto_the_next_line() {
        let text = "enum {\n  VALE_FROM_WORLD_MAP = 1, ///< a note\n  VALE_FROM_INN =\n      12, ///< another\n};";
        let n = numbers(text);
        assert_eq!(n.get("VALE_FROM_WORLD_MAP"), Some(&1));
        assert_eq!(n.get("VALE_FROM_INN"), Some(&12));
    }

    #[test]
    fn goma_cave_entrance_is_entered_from_the_world_map() {
        let arrivals = read(crate::compile::root()).unwrap();
        let names = map_names(&std::fs::read_to_string(crate::compile::root().join("games/tbs/src/field/scenes.s")).unwrap());
        let number = names.iter().position(|n| n == "GomaCaveEntrance").unwrap() as u16;
        assert!(arrivals[&number].contains(&Arrival { dest: 1, from: "WorldMap".to_string() }), "{:?}", arrivals[&number]);
        assert_eq!(ranked(&arrivals[&number], &[0, 1, 2]), vec![1, 2]);
    }

    #[test]
    fn ways_in_are_ranked_lowest_first() {
        let a = |dest, from: &str| Arrival { dest, from: from.to_string() };
        // Goma Cave Entrance: the default row 0 is in the table, but no exit leads there.
        assert_eq!(ranked(&[a(2, "GomaCaveStream"), a(1, "WorldMap")], &[0, 1, 2]), vec![1, 2]);
        // The Sanctum's revival after every other way in.
        assert_eq!(ranked(&[a(1, "Sanctum"), a(2, "SolEntrance")], &[0, 1, 2]), vec![2, 1]);
        // An exit to a destination the table lacks lands on its first row.
        assert_eq!(ranked(&[a(10, "SolSanctumLunaSolRooms")], &[3, 4]), vec![3]);
        assert_eq!(ranked(&[a(7, "Vale"), a(4, "Vale")], &[3, 4]), vec![4, 3]);
        assert!(ranked(&[], &[0, 1]).is_empty());
    }
}
