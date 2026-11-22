//! What `render event` is told to stage around an event, beyond what it
//! finds itself: story flags set or cleared (`--set`, `--clear`), items
//! given (`--give`), the way in (`--entrance`), the cell the leader starts
//! on and the way he faces (`--at`, `--face`), and keys held before the
//! trigger and once the player has control again (`--hold`, `--then`). And
//! the flags an event's own function needs before it does anything (its
//! guard), read from its source.

use std::collections::HashMap;

/// The KEYINPUT bits a pad script names.
const KEY_NAMES: [(&str, u32); 10] = [
    ("a", 1),
    ("b", 2),
    ("select", 4),
    ("start", 8),
    ("right", 0x10),
    ("left", 0x20),
    ("up", 0x40),
    ("down", 0x80),
    ("r", 0x100),
    ("l", 0x200),
];

/// One step of a pad script: keys held (KEYINPUT bits, 0 for none) for a
/// number of frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Held {
    pub keys: u32,
    pub frames: u32,
}

/// What `render event`'s staging options ask for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stage {
    /// Story flags set and cleared for every try, by name or 0x number.
    pub set: Vec<String>,
    pub clear: Vec<String>,
    /// Items put in the leader's bag, by name (ITEM_ or not) or number.
    pub give: Vec<String>,
    /// The only entrance the scene is entered by.
    pub entrance: Option<u16>,
    /// The cell the leader starts on, and its map layer when given.
    pub at: Option<(i32, i32, Option<u8>)>,
    /// The way he faces there (a facing angle, 0 east, 0x4000 south).
    pub face: Option<u32>,
    /// Keys held before the trigger is given: for a touch or a raised
    /// trigger they are what gives it.
    pub hold: Vec<Held>,
    /// Keys held once the player has control again.
    pub then: Vec<Held>,
}

impl Stage {
    /// Whether the leader's start is staged by hand: he starts at `--at`,
    /// or the trigger comes from `--hold`, and the walk-up is not planned.
    pub fn staged(&self) -> bool {
        self.at.is_some() || !self.hold.is_empty()
    }

    /// Reads one staging option and its value; false when `option` is none.
    pub fn option(&mut self, option: &str, value: &mut dyn FnMut(&str) -> Result<String, String>) -> Result<bool, String> {
        match option {
            "--set" => self.set.push(value(option)?),
            "--clear" => self.clear.push(value(option)?),
            "--give" => self.give.push(value(option)?),
            "--entrance" => {
                let v = value(option)?;
                self.entrance = Some(v.parse().map_err(|_| format!("--entrance takes a number, not {v:?}"))?);
            }
            "--at" => self.at = Some(parse_cell(&value(option)?)?),
            "--face" => self.face = Some(parse_face(&value(option)?)?),
            "--hold" => self.hold.extend(parse_script(&value(option)?)?),
            "--then" => self.then.extend(parse_script(&value(option)?)?),
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// The options again, as a child of --jobs is given them.
    pub fn is_option(option: &str) -> bool {
        matches!(option, "--set" | "--clear" | "--give" | "--entrance" | "--at" | "--face" | "--hold" | "--then")
    }
}

/// `X,Y` or `X,Y,LAYER`: a map cell, and its layer.
pub fn parse_cell(text: &str) -> Result<(i32, i32, Option<u8>), String> {
    let parts: Vec<&str> = text.split(',').map(str::trim).collect();
    let bad = || format!("--at takes a cell as X,Y or X,Y,LAYER, not {text:?}");
    match parts[..] {
        [x, z] => Ok((x.parse().map_err(|_| bad())?, z.parse().map_err(|_| bad())?, None)),
        [x, z, layer] => Ok((x.parse().map_err(|_| bad())?, z.parse().map_err(|_| bad())?, Some(layer.parse().map_err(|_| bad())?))),
        _ => Err(bad()),
    }
}

/// A way to face, by its compass name (north, north-east and so on, or
/// n, ne, ...), as a facing angle.
pub fn parse_face(text: &str) -> Result<u32, String> {
    let ways = [
        (["east", "e"], 0x0000),
        (["south-east", "se"], 0x2000),
        (["south", "s"], 0x4000),
        (["south-west", "sw"], 0x6000),
        (["west", "w"], 0x8000),
        (["north-west", "nw"], 0xa000),
        (["north", "n"], 0xc000),
        (["north-east", "ne"], 0xe000),
    ];
    let lower = text.trim().to_ascii_lowercase().replace('_', "-");
    ways.iter()
        .find(|(names, _)| names.contains(&lower.as_str()) || names[0].replace('-', "") == lower)
        .map(|(_, angle)| *angle)
        .ok_or_else(|| format!("--face takes north, north-east, east and so on (or n, ne, e ...), not {text:?}"))
}

/// A pad script: steps `KEYS:FRAMES` apart by commas, KEYS key names joined
/// by `+` (up, down, left, right, a, b, l, r, start, select), or `wait` for
/// none (`up:40,wait:20,a:2`).
pub fn parse_script(text: &str) -> Result<Vec<Held>, String> {
    let mut out = Vec::new();
    for step in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (keys, frames) = step.split_once(':').ok_or_else(|| format!("a pad step is KEYS:FRAMES (up:40), not {step:?}"))?;
        let frames: u32 = frames.trim().parse().map_err(|_| format!("{step:?} holds its keys for no number of frames"))?;
        let mut bits = 0;
        for key in keys.split('+').map(|k| k.trim().to_ascii_lowercase()) {
            if key == "wait" || key == "none" {
                continue;
            }
            bits |= KEY_NAMES.iter().find(|(n, _)| *n == key).map(|(_, b)| *b).ok_or_else(|| format!("{key:?} is no key (up, down, left, right, a, b, l, r, start, select, wait)"))?;
        }
        out.push(Held { keys: bits, frames });
    }
    Ok(out)
}

/// The keys a script holds in each frame, in order.
pub fn script_frames(script: &[Held]) -> Vec<u32> {
    script.iter().flat_map(|h| std::iter::repeat_n(h.keys, h.frames as usize)).collect()
}

/// The script as `--hold` writes it, for a note.
pub fn describe(script: &[Held]) -> String {
    script
        .iter()
        .map(|h| {
            let keys: Vec<&str> = KEY_NAMES.iter().filter(|(_, b)| h.keys & b != 0).map(|(n, _)| *n).collect();
            format!("{}:{}", if keys.is_empty() { "wait".to_string() } else { keys.join("+") }, h.frames)
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(feature = "render")]
/// An item's number from the game's item_ids.h (`enum ItemId`), by its name
/// with or without ITEM_, or a number.
pub fn item_id(header: &str, name: &str) -> Result<u16, String> {
    let name = name.trim();
    if let Some(n) = name.strip_prefix("0x").map_or_else(|| name.parse().ok(), |h| u16::from_str_radix(h, 16).ok()) {
        return Ok(n);
    }
    let wanted = if name.starts_with("ITEM_") { name.to_string() } else { format!("ITEM_{}", name.to_ascii_uppercase()) };
    super::source::enums(header, &HashMap::new())?
        .into_iter()
        .flat_map(|(_, values)| values)
        .find(|(n, _)| *n == wanted)
        .and_then(|(_, v)| v)
        .map(|v| v as u16)
        .ok_or_else(|| format!("item_ids.h has no {wanted}"))
}

/// The flags function `name` of C source `text` needs before it does
/// anything, as (flag, set) pairs: the condition of an `if` its whole body
/// lies in (after its declarations), or the opposite of the one that
/// returns at once, when the condition is flag tests joined by `&&` (an
/// early return, by `||`). A flag test is `GameFlag_Get(F)`,
/// `Event_GetFlag(F)` or `Flag_IsSet(F)`, with `!` or compared with 0.
/// Nothing for anything else.
pub fn guard(text: &str, name: &str) -> Vec<(String, bool)> {
    let Some(body) = function_body(text, name) else {
        return Vec::new();
    };
    let plain = strip_comments(&body);
    let mut rest = plain.trim();
    // Declarations first (`struct Actor *leader = Actor_Get(...);`).
    let declaration = regex::Regex::new(r"^(?:const\s+)?(?:struct\s+\w+\s*\*?|s8|u8|s16|u16|s32|u32|bool8|bool32|int)\s*\*?\s*\w+(?:\s*=[^;]*)?;").expect("a declaration pattern");
    while let Some(found) = declaration.find(rest) {
        rest = rest[found.end()..].trim_start();
    }
    let Some(after_if) = rest.strip_prefix("if") else {
        return Vec::new();
    };
    let Some((condition, after)) = parenthesised(after_if.trim_start()) else {
        return Vec::new();
    };
    let after = after.trim_start();
    // `if (C) return;`: the body needs C false.
    let returns = |t: &str| t.strip_prefix("return").is_some_and(|r| r.trim_start().starts_with(';'));
    let braced_return = after.strip_prefix('{').map(str::trim_start).is_some_and(|t| returns(t) && t.trim_start_matches("return").trim_start().trim_start_matches(';').trim_start().starts_with('}'));
    if returns(after) || braced_return {
        let terms: Vec<&str> = split_top(&condition, "||");
        return terms.iter().map(|t| flag_test(t).map(|(f, on)| (f, !on))).collect::<Option<Vec<_>>>().unwrap_or_default();
    }
    // `if (C) { ... }` closing the body, with no else.
    let Some(block) = after.strip_prefix('{') else {
        return Vec::new();
    };
    let Some(end) = matching_brace(block) else {
        return Vec::new();
    };
    if !block[end + 1..].trim().is_empty() {
        return Vec::new();
    }
    let terms: Vec<&str> = split_top(&condition, "&&");
    terms.iter().map(|t| flag_test(t)).collect::<Option<Vec<_>>>().unwrap_or_default()
}

/// The map copies whose rectangle's destination comes before its size
/// (`(sourceX, sourceY, destinationX, destinationY, width, height)`); the
/// others give the size first. Those that copy pictures, then those that
/// copy only what the cells do.
const PICTURE_COPIES: [(&str, bool); 7] = [
    ("MapGrid_CopyCellsTo", true),
    ("MapGrid_CopyCellPictureIndicesRect", true),
    ("MapGrid_CopyCells", false),
    ("MapGrid_CopyCellsLayered", false),
    ("MapGrid_CopyCellPictureCellsRect", false),
    ("FieldTileCopies_CopyTouchedCells", false),
    ("TileCopies_CopyTouchedCells", false),
];
const BEHAVIOUR_COPIES: [(&str, bool); 3] = [("MapGrid_CopyCellAttributes", false), ("Map_CopyCellAttributeRect", false), ("Map_CopyCellsRect", false)];

/// The map cells function `name` of C source `text` changes or puts
/// something on, what a Psynergy cast for it must reach to show it: the
/// destinations of its map copies with numbers for arguments (those that
/// copy pictures, else those that copy what cells do), and the cells of the
/// pixels an actor is put at (`Actor_SetPosition(id, 657, 246)`).
pub fn drawn_cells(text: &str, name: &str) -> Vec<(i32, i32)> {
    let Some(body) = function_body(text, name) else {
        return Vec::new();
    };
    let plain = strip_comments(&body);
    let numbers = |args: &str| -> Option<Vec<i32>> { args.split(',').map(|a| a.trim().parse::<i32>().ok()).collect() };
    let calls = |table: &[(&str, bool)]| -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for &(function, to_first) in table {
            let pattern = regex::Regex::new(&format!(r"\b{function}\s*\(([^;]*?)\)\s*;")).expect("a call pattern");
            for found in pattern.captures_iter(&plain) {
                let Some(n) = numbers(&found[1]) else {
                    continue;
                };
                if n.len() < 6 {
                    continue;
                }
                let (x, z, w, h) = if to_first { (n[2], n[3], n[4], n[5]) } else { (n[4], n[5], n[2], n[3]) };
                for dz in 0..h.clamp(1, 8) {
                    for dx in 0..w.clamp(1, 8) {
                        if !out.contains(&(x + dx, z + dz)) {
                            out.push((x + dx, z + dz));
                        }
                    }
                }
            }
        }
        out
    };
    let mut out = calls(&PICTURE_COPIES);
    let placed = regex::Regex::new(r"\bActor_SetPosition(?:AndFacing)?\s*\(\s*\w+\s*,\s*(\d+)\s*,\s*(\d+)\s*[,)]").expect("a placement pattern");
    for found in placed.captures_iter(&plain) {
        if let (Ok(x), Ok(z)) = (found[1].parse::<i32>(), found[2].parse::<i32>()) {
            out.push((x / 16, z / 16));
        }
    }
    if out.is_empty() {
        out = calls(&BEHAVIOUR_COPIES);
    }
    out
}

/// One flag test: the flag's name and whether it must be set.
fn flag_test(term: &str) -> Option<(String, bool)> {
    let mut t = term.trim();
    while let Some(inner) = t.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        t = inner.trim();
    }
    let mut on = true;
    if let Some(r) = t.strip_prefix('!') {
        if !r.starts_with('=') {
            on = false;
            t = r.trim();
        }
    }
    for (suffix, flips) in [("== 0", true), ("== FALSE", true), ("!= 0", false), ("== 1", false), ("!= FALSE", false)] {
        if let Some(r) = t.strip_suffix(suffix) {
            t = r.trim();
            on ^= flips;
            break;
        }
    }
    for getter in ["GameFlag_Get(", "Event_GetFlag(", "Flag_IsSet("] {
        if let Some(r) = t.strip_prefix(getter).and_then(|r| r.strip_suffix(')')) {
            let flag = r.trim();
            if !flag.is_empty() && flag.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Some((flag.to_string(), on));
            }
        }
    }
    None
}

/// The text of function `name`'s body in `text`, braces left out.
fn function_body(text: &str, name: &str) -> Option<String> {
    let pattern = regex::Regex::new(&format!(r"(?m)^[\w\s\*]*\b{}\s*\([^)]*\)\s*\{{", regex::escape(name))).ok()?;
    let start = pattern.find(text)?.end();
    let end = matching_brace(&text[start..])?;
    Some(text[start..start + end].to_string())
}

/// Where the brace closing an opened block lies in `text` (which starts
/// inside it).
fn matching_brace(text: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' if depth == 0 => return Some(i),
            '}' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// `(inside) rest`: the parenthesised text at the start and what follows.
fn parenthesised(text: &str) -> Option<(String, &str)> {
    let inner = text.strip_prefix('(')?;
    let mut depth = 0i32;
    for (i, c) in inner.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return Some((inner[..i].to_string(), &inner[i + 1..])),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// `text` split at `op` outside parentheses.
fn split_top<'t>(text: &'t str, op: &str) -> Vec<&'t str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ if depth == 0 && text[i..].starts_with(op) => {
                out.push(&text[start..i]);
                i += op.len();
                start = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&text[start..]);
    out
}

fn strip_comments(text: &str) -> String {
    regex::Regex::new(r"(?s)/\*.*?\*/|//[^\n]*").map_or_else(|_| text.to_string(), |r| r.replace_all(text, "").into_owned())
}

/// `#define NAME VALUE` lines of a C source, and its enumerators given a
/// value (`FLAG_LOG_ROLLED = FLAG_AREA_TEMP_10,`), for flags a scene names
/// itself.
pub fn defines(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let given = regex::Regex::new(r"(?m)^\s*(FLAG_\w+)\s*=\s*([\w()]+)\s*,").expect("an enumerator pattern");
    for found in given.captures_iter(text) {
        out.insert(found[1].to_string(), found[2].to_string());
    }
    for line in text.lines() {
        let mut words = line.split_whitespace();
        if let (Some("#define"), Some(name), Some(value)) = (words.next(), words.next(), words.next()) {
            if !name.contains('(') {
                out.insert(name.to_string(), value.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scene_names_flags_by_define_and_enum() {
        let names = defines("#define FLAG_TOLD 0x8b0 ///< told\nenum {\n  FLAG_LOG_ROLLED = FLAG_AREA_TEMP_10,\n  OTHER = 3,\n};\n#define F(x) (x)\n");
        assert_eq!(names.get("FLAG_TOLD").map(String::as_str), Some("0x8b0"));
        assert_eq!(names.get("FLAG_LOG_ROLLED").map(String::as_str), Some("FLAG_AREA_TEMP_10"));
        assert!(!names.contains_key("OTHER") && !names.contains_key("F(x)"));
    }

    #[test]
    fn a_pad_script_is_keys_held_for_frames() {
        let script = parse_script("up:40, wait:20,a+right:2").unwrap();
        assert_eq!(script, vec![Held { keys: 0x40, frames: 40 }, Held { keys: 0, frames: 20 }, Held { keys: 0x11, frames: 2 }]);
        assert_eq!(script_frames(&script).len(), 62);
        assert_eq!(describe(&script), "up:40,wait:20,a+right:2");
        assert!(parse_script("up").is_err());
        assert!(parse_script("jump:3").is_err());
    }

    #[test]
    fn a_cell_and_a_way_to_face() {
        assert_eq!(parse_cell("12,28").unwrap(), (12, 28, None));
        assert_eq!(parse_cell("12, 28, 1").unwrap(), (12, 28, Some(1)));
        assert!(parse_cell("12").is_err());
        assert_eq!(parse_face("north").unwrap(), 0xc000);
        assert_eq!(parse_face("SW").unwrap(), 0x6000);
        assert_eq!(parse_face("south-east").unwrap(), 0x2000);
        assert_eq!(parse_face("southeast").unwrap(), 0x2000);
        assert!(parse_face("up").is_err());
    }

    #[test]
    fn staging_options_are_read_and_named() {
        let mut stage = Stage::default();
        let mut given = ["FLAG_A".to_string(), "4".to_string(), "3,4".to_string(), "left:16".to_string()].into_iter();
        for option in ["--set", "--entrance", "--at", "--hold"] {
            assert!(stage.option(option, &mut |_| Ok(given.next().unwrap())).unwrap());
            assert!(Stage::is_option(option));
        }
        assert!(!stage.option("--scene", &mut |_| Ok(String::new())).unwrap());
        assert_eq!(stage.set, vec!["FLAG_A"]);
        assert_eq!(stage.entrance, Some(4));
        assert_eq!(stage.at, Some((3, 4, None)));
        assert_eq!(stage.hold, vec![Held { keys: 0x20, frames: 16 }]);
        assert!(stage.staged());
        assert!(!Stage::default().staged());
    }

    #[cfg(feature = "render")]
    #[test]
    fn items_are_found_by_name() {
        let header = "enum ItemId {\n  ITEM_NONE,\n  ITEM_HERB,\n  ITEM_RUIN_KEY = 389,\n  ITEM_MAGMA_BALL\n};";
        assert_eq!(item_id(header, "ITEM_MAGMA_BALL").unwrap(), 390);
        assert_eq!(item_id(header, "ruin_key").unwrap(), 389);
        assert_eq!(item_id(header, "0x10").unwrap(), 16);
        assert!(item_id(header, "ITEM_SWORD").is_err());
    }

    #[test]
    fn what_a_cast_must_reach_is_what_its_function_draws() {
        let text = "void C_Reveal(void) {\n  MapGrid_CopyCellsTo(27, 4, 48, 12, 1, 2);\n  MapGrid_CopyCellAttributes(48, 1, 2, 1, 48, 19);\n}\n\
                    void C_Weeds(void) {\n  FieldTileCopies_CopyTouchedCells(3, 50, 1, 1, 8, 54, 0);\n}\n\
                    void C_Idol(void) {\n  Actor_SetPosition(FIELD_TREASURE_ACTOR, 657, 246);\n}\n\
                    void C_Door(void) {\n  Map_CopyCellAttributeRect(2, 4, 1, 1, 7, 14);\n}\n\
                    void C_Unknown(void) {\n  MapGrid_CopyCellsTo(x, 4, 48, 12, 1, 2);\n}\n";
        assert_eq!(drawn_cells(text, "C_Reveal"), vec![(48, 12), (48, 13)]);
        assert_eq!(drawn_cells(text, "C_Weeds"), vec![(8, 54)]);
        assert_eq!(drawn_cells(text, "C_Idol"), vec![(41, 15)]);
        assert_eq!(drawn_cells(text, "C_Door"), vec![(7, 14)]);
        assert!(drawn_cells(text, "C_Unknown").is_empty());
    }

    #[test]
    fn a_guard_around_the_whole_body_is_what_it_needs() {
        let text = "void A_Stop(void) {\n  if (GameFlag_Get(FLAG_REUNITED) &&\n      !GameFlag_Get(FLAG_SET_OUT)) {\n    Event_Start();\n    if (x) { y(); }\n  }\n}\n\
                    void A_Early(void) {\n  struct Actor *a = Actor_Get(3);\n  // a comment\n  if (Event_GetFlag(FLAG_DONE) != 0 || !Flag_IsSet(FLAG_OPEN))\n    return;\n  Event_Start();\n}\n\
                    void A_Else(void) {\n  if (GameFlag_Get(FLAG_X)) {\n    a();\n  } else {\n    b();\n  }\n}\n\
                    void A_Braced(void) {\n\n  if (!Event_GetFlag(FLAG_WALKED)) {\n    return;\n  }\n  Event_Start();\n}\n\
                    void A_Zero(s32 mode) {\n  if (Event_GetFlag(FLAG_Y) == 0) {\n    a();\n  }\n}\n\
                    void A_Other(void) {\n  if (Actor_Get(3)) {\n    a();\n  }\n}\n";
        assert_eq!(guard(text, "A_Stop"), vec![("FLAG_REUNITED".to_string(), true), ("FLAG_SET_OUT".to_string(), false)]);
        assert_eq!(guard(text, "A_Early"), vec![("FLAG_DONE".to_string(), false), ("FLAG_OPEN".to_string(), true)]);
        assert!(guard(text, "A_Else").is_empty());
        assert_eq!(guard(text, "A_Braced"), vec![("FLAG_WALKED".to_string(), true)]);
        assert_eq!(guard(text, "A_Zero"), vec![("FLAG_Y".to_string(), false)]);
        assert!(guard(text, "A_Other").is_empty());
        assert!(guard(text, "A_Missing").is_empty());
    }
}
