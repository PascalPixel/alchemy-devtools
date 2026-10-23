//! Name provenance for the humanization report: whether the names of game
//! things (scenes, actors, flags, scene links, items, abilities and the like)
//! come from evidence or were made up (Alchemy's S3). Three checks, all read
//! from the source and the game's English text (`text/en.po`):
//!
//! - words the game never uses: each name is split into words, and a word is
//!   accounted for when the game's text uses it (after crude stemming) or it
//!   is on the short programming vocabulary below;
//! - names without evidence: a name is verified by a `★ Named from ...`
//!   line in its own comment whose quote or area name the text confirms;
//!   short of that, evidence in its prose (a quote the text holds, a message
//!   symbol, an area-name citation, a note on its render) is told apart from
//!   none at all;
//! - names checked mechanically, which need no line: an enumerator of a
//!   table-indexed enum (`ItemId`) against the game's name for its index,
//!   and a scene link whose words are verified scenes, door words and the
//!   room words of those scenes.
//!
//! The README's "Humanization report" section sets the rules out.

use regex::Regex;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

/// Words a name may use that are the program's, not the game's: what code
/// does (`Get`, `Update`), how it is organized (`Table`, `Entry`, `Handler`)
/// and the report's own kind prefixes (`SceneId`, `ACTOR`, `FLAG`). Ordinary
/// English the text already uses never needs a place here; anything else
/// stays flagged, so the list errs short.
pub const VOCABULARY: &[&str] = &[
    // Kinds and prefixes.
    "scene", "id", "ids", "actor", "actors", "flag", "flags", "event", "events", "script", "scripts", "handler",
    "handlers", "callback", "table", "tables", "entry", "entries", "index", "count", "num", "interiors", "interior",
    "temp", "tmp", "msg", "obj", "npc", "fx", "se", "mus", "bg", "gfx", "anim", "oam", "vram", "sprite", "sprites",
    "tile", "tiles", "tilemap", "palette", "pal", "cell", "cells", "region", "regions", "layer", "rect", "ptr",
    "buf", "buffer", "data", "state", "mode", "kind", "type", "init", "initialize", "update", "updates", "get",
    "set", "run", "runs", "handle", "reset", "clear", "check", "load", "draw", "render", "spawn", "tick", "frame",
    "frames", "timer", "pos", "max", "min", "prev", "cur", "unused", "debug", "util", "func", "sub", "args", "arg",
    "src", "dst", "len", "info", "ctx", "cfg", "config", "rng", "random", "rom", "ram", "iwram", "ewram", "dma",
    "irq", "params", "param", "struct", "enum", "macro", "lut", "offset", "addr", "ov", "alloc", "mem", "var", "cb",
    "attr", "byte", "clr", "ui", "setup", "cached", "engine", "scheduler", "dispatch", "op", "noop", "placement",
    // How the engine draws and moves things, not what the player sees.
    "particle", "transition", "animation", "animate", "blend", "fade", "overlay", "channel", "angle", "collision",
    "trigger", "marker", "cycle", "blank", "hblank", "vblank", "alpha", "vector", "tint", "darken", "hue", "flicker",
    "dither", "contrast", "zoom", "rotation", "backdrop", "foreground", "background", "visibility", "occluding",
    "collidable", "pixel", "scanline", "hofs", "canvas", "glyph", "graphics", "rgb", "radial", "orbit", "pulse",
    "squash", "jitter", "emitter", "emission", "spawner", "cue", "audio",
    // What code does with its data.
    "pointer", "sync", "synchronize", "toggle", "variant", "default", "definition", "install", "queued", "pending",
    "remap", "suspend", "compute", "clamp", "allocate", "address", "header", "heap", "storage", "descriptor",
    "packet", "query", "probe", "validate", "invalid", "unset", "insert", "cleanup", "runtime", "snapshot",
    "fallback", "override", "refresh", "redraw", "repaint", "context", "configure", "enable", "activate",
    "resource", "lz", "hex", "dpad", "interpolation", "linear", "inverse", "magnitude", "ratio", "remainder", "sine",
    "negative", "positive", "relative", "length", "width", "grid", "node", "cursor", "selector", "periodic",
    "frequency",
    // Positions.
    "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth", "eleventh",
    "twelfth", "last", "north",
    "south", "east", "west", "northeast", "northwest", "southeast", "southwest", "upper", "lower", "left", "right",
];

/// A game's English text, as the checks read it.
#[derive(Default)]
pub struct GameText {
    /// Every word of every message, lowercase.
    words: HashSet<String>,
    /// Their stems.
    stems: HashSet<String>,
    /// Each message: its symbol (`None` for `message`) and its text with
    /// commands taken out and lines joined.
    messages: Vec<(Option<String>, String)>,
    /// The messages joined by U+0001, for quotes.
    joined: String,
    symbols: HashMap<String, usize>,
    /// Each message's place by its number (`msgid`).
    numbers: HashMap<u32, usize>,
    /// The area names (`gScene_AreaNameRules`, from `MsgDebugEntryName`).
    areas: Vec<String>,
}

/// A message's text without its `{...}` commands, on one line.
fn flatten(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0;
    for c in text.chars() {
        match c {
            '{' => depth += 1,
            '}' if depth > 0 => {
                depth -= 1;
                out.push(' ');
            }
            _ if depth > 0 => {}
            '\n' => out.push(' '),
            _ => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A PO string's contents with its escapes undone.
fn unquote(text: &str) -> String {
    let inner = text.trim().strip_prefix('"').and_then(|t| t.strip_suffix('"')).unwrap_or("");
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `McCoy's` is `mccoy`; `Storms` is `storm`; `moving` and `move` meet at
/// `mov`. Crude, and applied alike to the text and the names.
pub fn stem(word: &str) -> String {
    let mut w = word.to_ascii_lowercase();
    if let Some(base) = w.strip_suffix("'s") {
        w = base.to_string();
    }
    let undouble = |w: &mut String| {
        let b = w.as_bytes();
        if b.len() >= 2 && b[b.len() - 1] == b[b.len() - 2] && !matches!(b[b.len() - 1], b'l' | b's' | b'z') {
            w.pop();
        }
    };
    if w.len() > 4 && w.ends_with("ies") {
        w.truncate(w.len() - 3);
        w.push('y');
    } else if w.len() > 3 && w.ends_with("es") && ["s", "x", "z", "ch", "sh"].iter().any(|e| w[..w.len() - 2].ends_with(e)) {
        w.truncate(w.len() - 2);
    } else if w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") && !w.ends_with("us") {
        w.pop();
    }
    if w.len() > 5 && w.ends_with("ing") {
        w.truncate(w.len() - 3);
        undouble(&mut w);
    } else if w.len() > 4 && w.ends_with("ied") {
        w.truncate(w.len() - 3);
        w.push('y');
    } else if w.len() > 4 && w.ends_with("ed") {
        w.truncate(w.len() - 2);
        undouble(&mut w);
    }
    if w.len() > 3 && w.ends_with('e') {
        w.pop();
    }
    w
}

impl GameText {
    /// Read `en.po`; `areas` is how many area names follow `MsgDebugEntryName`
    /// (the highest index in `gScene_AreaNameRules` plus one), or all the
    /// messages up to the next named one.
    pub fn parse(po: &str, areas: Option<usize>) -> GameText {
        let mut text = GameText::default();
        let (mut ctxt, mut msgstr, mut in_str) = (None::<String>, None::<String>, false);
        let mut number = None::<u32>;
        let flush = |ctxt: &mut Option<String>, msgstr: &mut Option<String>, number: &mut Option<u32>, text: &mut GameText| {
            if let Some(body) = msgstr.take() {
                let symbol = ctxt.take().filter(|c| c != "message");
                if let Some(number) = number.take().filter(|_| !body.starts_with("Content-Type:")) {
                    text.numbers.insert(number, text.messages.len());
                }
                text.messages.push((symbol, flatten(&body)));
            }
            *ctxt = None;
            *number = None;
        };
        for line in po.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("msgctxt ") {
                flush(&mut ctxt, &mut msgstr, &mut number, &mut text);
                ctxt = Some(unquote(rest));
                in_str = false;
            } else if let Some(rest) = line.strip_prefix("msgid ") {
                number = unquote(rest).parse().ok();
                in_str = false;
            } else if let Some(rest) = line.strip_prefix("msgstr ") {
                msgstr = Some(unquote(rest));
                in_str = true;
            } else if line.starts_with('"') && in_str {
                if let Some(body) = msgstr.as_mut() {
                    body.push_str(&unquote(line));
                }
            } else if line.is_empty() || line.starts_with('#') {
                in_str = false;
            }
        }
        flush(&mut ctxt, &mut msgstr, &mut number, &mut text);
        // The header entry has no context and no game text.
        if let Some(header) = text.messages.iter().position(|(_, body)| body.starts_with("Content-Type:")) {
            text.messages.remove(header);
            for at in text.numbers.values_mut().filter(|at| **at > header) {
                *at -= 1;
            }
        }
        for (at, (symbol, _)) in text.messages.iter().enumerate() {
            if let Some(symbol) = symbol {
                text.symbols.entry(symbol.clone()).or_insert(at);
            }
        }
        for (_, body) in &text.messages {
            for word in body.split(|c: char| !(c.is_alphanumeric() || c == '\'')) {
                let word = word.trim_matches('\'').to_lowercase();
                if word.is_empty() {
                    continue;
                }
                for part in std::iter::once(word.as_str()).chain(word.split('\'')) {
                    if !part.is_empty() {
                        text.stems.insert(stem(part));
                        text.words.insert(part.to_string());
                    }
                }
            }
        }
        text.joined = text.messages.iter().map(|(_, body)| body.as_str()).collect::<Vec<_>>().join("\u{1}");
        if let Some(&start) = text.symbols.get("MsgDebugEntryName") {
            let end = match areas {
                Some(count) => (start + count).min(text.messages.len()),
                None => (start + 1..text.messages.len()).find(|&at| text.messages[at].0.is_some()).unwrap_or(text.messages.len()),
            };
            let mut seen = HashSet::new();
            for (_, body) in &text.messages[start..end] {
                if seen.insert(body.clone()) {
                    text.areas.push(body.clone());
                }
            }
        }
        text
    }

    /// The game's text uses the word, or a word with its stem.
    pub fn uses(&self, word: &str) -> bool {
        let lower = word.to_lowercase();
        self.words.contains(&lower) || self.stems.contains(&stem(&lower))
    }

    /// Some message holds the quote, commands aside.
    pub fn holds(&self, quote: &str) -> bool {
        let quote = flatten(quote);
        !quote.is_empty() && self.joined.contains(&quote)
    }

    /// The message a symbol names, or one of the unnamed messages after it
    /// (a table's base, `MsgItemName`), holds the quote. `None` for an
    /// unknown symbol.
    fn symbol_holds(&self, symbol: &str, quote: &str) -> Option<bool> {
        let &start = self.symbols.get(symbol)?;
        let quote = flatten(quote);
        let end = (start + 1..self.messages.len()).find(|&at| self.messages[at].0.is_some()).unwrap_or(self.messages.len());
        Some(!quote.is_empty() && self.messages[start..end].iter().any(|(_, body)| body.contains(&quote)))
    }

    /// The message `offset` numbers after a table's first (`MsgItemName` + 5),
    /// commands taken out.
    pub fn table_entry(&self, symbol: &str, offset: i64) -> Option<&str> {
        let &base = self.symbols.get(symbol)?;
        let at = match self.numbers.iter().find(|(_, &at)| at == base) {
            Some((&number, _)) => *self.numbers.get(&u32::try_from(i64::from(number) + offset).ok()?)?,
            None => usize::try_from(base as i64 + offset).ok()?,
        };
        self.messages.get(at).map(|(_, body)| body.as_str())
    }

    /// The longest area name the text starts with, ending the sentence or
    /// followed by punctuation or a parenthesis, never by another word.
    fn area_at_start(&self, text: &str) -> Option<&str> {
        self.areas
            .iter()
            .filter(|area| {
                text.strip_prefix(area.as_str()).is_some_and(|rest| {
                    rest.is_empty() || rest.starts_with(" (") || rest.starts_with(|c: char| c.is_ascii_punctuation())
                })
            })
            .max_by_key(|area| area.len())
            .map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// The area names, as `gScene_AreaNameRules` reads them.
    pub fn areas(&self) -> &[String] {
        &self.areas
    }
}

/// A name's words as it writes them: `SceneId_SolSanctumLunaSolRooms` is
/// Scene, Id, Sol, Sanctum, Luna, Sol, Rooms; `ACTOR_TREE_MAN` is ACTOR, TREE,
/// MAN. Digits, hex letters after them and single letters (`g`, `s`)
/// are left out.
pub fn pieces(name: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for segment in name.split('_') {
        let bytes = segment.as_bytes();
        let mut start = 0;
        // Hex digits after a number belong to it: `Ov6aaRoom1`.
        let mut after_digits = false;
        for at in 1..=bytes.len() {
            let boundary = at == bytes.len() || {
                let (a, b) = (bytes[at - 1], bytes[at]);
                (a.is_ascii_lowercase() && b.is_ascii_uppercase())
                    || (a.is_ascii_digit() != b.is_ascii_digit())
                    || (a.is_ascii_uppercase()
                        && b.is_ascii_uppercase()
                        && bytes.get(at + 1).is_some_and(u8::is_ascii_lowercase))
            };
            if boundary {
                let piece = &segment[start..at];
                let digits = piece.bytes().all(|b| b.is_ascii_digit());
                let hex = after_digits && piece.len() <= 2 && piece.bytes().all(|b| b.is_ascii_hexdigit());
                if piece.len() > 1 && !digits && !hex {
                    out.push(piece);
                }
                after_digits = digits;
                start = at;
            }
        }
    }
    out
}

/// On the vocabulary, plurals and verb forms included.
fn in_vocabulary(lower: &str) -> bool {
    let stemmed = stem(lower);
    VOCABULARY.iter().any(|word| *word == lower || stem(word) == stemmed)
}

/// The words of a name the game's text and the vocabulary leave unexplained,
/// lowercase. Two pieces the text uses as one word are one (`McCoy`).
pub fn unused_words(name: &str, text: &GameText) -> Vec<String> {
    let pieces = pieces(name);
    let mut out: Vec<String> = Vec::new();
    let mut at = 0;
    while at < pieces.len() {
        if let Some(next) = pieces.get(at + 1) {
            let joined = format!("{}{}", pieces[at], next).to_lowercase();
            if text.words.contains(&joined) {
                at += 2;
                continue;
            }
        }
        let lower = pieces[at].to_lowercase();
        if !in_vocabulary(&lower) && !text.uses(&lower) && !out.contains(&lower) {
            out.push(lower);
        }
        at += 1;
    }
    out
}

/// What a game thing's comments give for its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Evidence {
    /// A `★ Named from ...` line the text confirms.
    Verified(&'static str),
    /// An enumerator of a table-indexed enum whose words are the game's name
    /// for its index: the table's first message and the offset, and whether
    /// it took a duplicate's number (`ENEMY_SLIME_2`) to match.
    Table { symbol: &'static str, offset: i64, numbered: bool },
    /// The same, but its words differ from the game's name, which it gives.
    Mismatch { symbol: &'static str, offset: i64, game: String },
    /// A scene link whose words are all verified scenes and door words.
    Rooms,
    /// The same line, confirmed, without its ★.
    Unstarred(&'static str),
    /// A `Named from ...` line the text does not confirm, and why.
    Failed(String),
    /// No line, but its prose quotes the game, cites a message or the area
    /// names, or describes its render.
    Prose(Vec<&'static str>),
    None,
}

impl Evidence {
    pub fn metric(&self) -> &'static str {
        match self {
            Evidence::Verified(_) => "provenance_verified",
            Evidence::Table { .. } => "provenance_table",
            Evidence::Mismatch { .. } => "provenance_table_mismatch",
            Evidence::Rooms => "provenance_rooms",
            Evidence::Unstarred(_) => "provenance_unstarred",
            Evidence::Failed(_) => "provenance_failed",
            Evidence::Prose(_) => "provenance_prose",
            Evidence::None => "provenance_none",
        }
    }

    /// The worklist's evidence column.
    pub fn describe(&self) -> String {
        match self {
            Evidence::Verified(kind) => format!("verified {kind}"),
            Evidence::Table { symbol, offset, numbered } => {
                format!("table {symbol}+{offset}{}", if *numbered { " numbered" } else { "" })
            }
            Evidence::Mismatch { symbol, offset, game } => format!("mismatch: {symbol}+{offset} is \"{game}\""),
            Evidence::Rooms => "rooms".to_string(),
            Evidence::Unstarred(kind) => format!("unstarred {kind}"),
            Evidence::Failed(why) => format!("failed: {why}"),
            Evidence::Prose(kinds) => format!("prose {}", kinds.join("+")),
            Evidence::None => "none".to_string(),
        }
    }
}


/// A comment's words, its markers taken off, lines joined by spaces.
pub fn comment_words(raw: &str) -> String {
    comment_lines(raw).join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A comment's lines without `///<`, `///`, `//`, `/*`, `*/` or a leading `*`.
pub fn comment_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .map(|line| {
            let mut line = line.trim();
            for marker in ["///<", "//!<", "///", "//!", "//", "/**<", "/**", "/*!", "/*"] {
                if let Some(rest) = line.strip_prefix(marker) {
                    line = rest;
                    break;
                }
            }
            let line = line.trim_end().strip_suffix("*/").unwrap_or(line).trim();
            line.strip_prefix('*').map_or(line, str::trim).to_string()
        })
        .collect()
}

fn line_regexes() -> &'static [Regex; 4] {
    static SET: OnceLock<[Regex; 4]> = OnceLock::new();
    SET.get_or_init(|| {
        [
            Regex::new(r#"(★\s*)?Named from the game:\s*"([^"]+)"\s*\(\s*(Msg[A-Za-z0-9_]+)[^)]*\)"#).unwrap(),
            Regex::new(r"(★\s*)?Named from its area name:\s*(.+)").unwrap(),
            Regex::new(r"(★\s*)?Named from what the player sees:\s*(.*?)\(its render\)").unwrap(),
            Regex::new(r"Named from (?:the game|its area name|what the player sees)").unwrap(),
        ]
    })
}

/// The evidence lines in a name's own comment, judged against the text.
fn evidence_line(comment: &str, text: &GameText) -> Option<Evidence> {
    let [game, area, sees, any] = line_regexes();
    let mut found: Vec<(bool, &'static str)> = Vec::new();
    let mut failed = None;
    for capture in game.captures_iter(comment) {
        match text.symbol_holds(&capture[3], &capture[2]) {
            Some(true) => found.push((capture.get(1).is_some(), "quote")),
            Some(false) => failed = Some(format!("{} does not hold \"{}\"", &capture[3], &capture[2])),
            None => failed = Some(format!("no message {}", &capture[3])),
        }
    }
    for capture in area.captures_iter(comment) {
        match text.area_at_start(capture[2].trim()) {
            Some(_) => found.push((capture.get(1).is_some(), "area")),
            None => failed = Some(format!("no area name at \"{}\"", capture[2].trim())),
        }
    }
    for capture in sees.captures_iter(comment) {
        if capture[2].trim().is_empty() {
            failed = Some("no description of what the player sees".to_string());
        } else {
            found.push((capture.get(1).is_some(), "render"));
        }
    }
    if let Some(&(_, kind)) = found.iter().find(|(starred, _)| *starred) {
        return Some(Evidence::Verified(kind));
    }
    if let Some(&(_, kind)) = found.first() {
        return Some(Evidence::Unstarred(kind));
    }
    if failed.is_none() && any.is_match(comment) {
        failed = Some("a Named from line out of form".to_string());
    }
    failed.map(Evidence::Failed)
}

fn prose_regexes() -> &'static [Regex; 4] {
    static SET: OnceLock<[Regex; 4]> = OnceLock::new();
    SET.get_or_init(|| {
        [
            Regex::new(r#""([^"]{3,})"|`([^`]{3,})`|“([^”]{3,})”"#).unwrap(),
            Regex::new(r"\bMsg[A-Z][A-Za-z0-9_]*").unwrap(),
            Regex::new(r"(?i)area name|gScene_AreaNameRules|MsgDebugEntryName|Scene_GetAreaNameIndex").unwrap(),
            Regex::new(r"(?i)\brender|what the player sees|\brecording|\bseen in\b").unwrap(),
        ]
    })
}

/// The kinds of evidence a comment's prose gives.
fn prose(comment: &str, text: &GameText, quotes: &mut HashMap<String, bool>) -> Vec<&'static str> {
    let [quote, message, area, render] = prose_regexes();
    let mut kinds = Vec::new();
    let quoted = quote.captures_iter(comment).any(|capture| {
        let inner = capture.get(1).or(capture.get(2)).or(capture.get(3)).map_or("", |m| m.as_str());
        *quotes.entry(inner.to_string()).or_insert_with(|| text.holds(inner))
    });
    if quoted {
        kinds.push("quote");
    }
    if message.find_iter(comment).any(|m| text.symbols.contains_key(m.as_str())) {
        kinds.push("message");
    }
    if area.is_match(comment) {
        kinds.push("area");
    }
    if render.is_match(comment) {
        kinds.push("render");
    }
    kinds
}

/// A name defined in the source that may name a game thing.
#[derive(Clone, Debug, Default)]
pub struct Thing {
    pub name: String,
    pub kind: &'static str,
    pub file: String,
    pub line: usize,
    /// Its own `///` comment and `///<` notes.
    pub own: String,
    /// Its enum's comment, for an enumerator.
    pub context: String,
    /// For an enumerator, its enum's tag (`ItemId`), if it has one.
    pub tag: Option<String>,
    /// For an enumerator, its value, when it is a number or simple sum.
    pub value: Option<i64>,
    /// For an enumerator, whether it is its enum's only one, so that the
    /// enum's comment is its own.
    pub alone: bool,
}

impl Thing {
    /// Names of code mechanisms (functions, tables) are only checked for words.
    pub fn needs_evidence(&self) -> bool {
        !matches!(self.kind, "function" | "global")
    }
}

/// The family prefixes: the kind of game thing each names, and the word the
/// tags of its own enums end in (`enum XianActor` holds actors).
pub const FAMILIES: &[(&str, &str, &str)] = &[
    ("SceneId_", "scene", "Scene"),
    ("ACTOR_", "actor", "Actor"),
    ("FLAG_", "flag", "Flag"),
    ("ITEM_", "item", "Item"),
    ("ABILITY_", "ability", "Ability"),
    ("PSYNERGY_", "psynergy", "Psynergy"),
    ("DJINNI_", "djinni", "Djinni"),
    ("ENEMY_", "enemy", "Enemy"),
    ("MONSTER_", "enemy", "Monster"),
    ("SUMMON_", "summon", "Summon"),
    ("CHARACTER_", "character", "Character"),
    ("CLASS_", "class", "Class"),
    ("PERSON_", "person", "Person"),
    ("AREA_", "place", "Area"),
];

/// Words that, right after a family prefix, name a part of the family's
/// mechanism rather than one member of it, for the names no enum tag
/// classifies: `ITEM_USE_CONSUMED` is what using an item does to it,
/// `ACTOR_ACTION_IDLE` an action an actor runs, `FLAG_MENU_OPEN` a flag of
/// the menus, `PSYNERGY_EV_ANY` a Psynergy event's control word. Like the
/// vocabulary, it errs short.
pub const ASPECTS: &[&str] = &[
    "kind", "type", "effect", "mode", "step", "phase", "state", "action", "command", "use", "range", "index", "menu",
    "debug", "event", "ev",
];

/// Words that, right after `ACTOR_`, make an actor a part of its scene's
/// machinery rather than a person or thing of the game, even among the
/// actors a scene numbers in one enum: a blank actor draws nothing the player
/// sees and is there for a script to steer or to stand in the way
/// (`ACTOR_BLANK_ROCK_GUIDE`, which the falling rock is held above), and a
/// mask hides the parts of another sprite it covers (`ACTOR_MASK_FOR_STUMP`).
pub const STAND_INS: &[&str] = &["blank", "mask"];

/// Words that, last in a name, make it a quantity, a bound, a bit field or a
/// place on the screen: `ITEM_STACK_MAX`, `ENEMY_CLASS_FIRST`, `SUMMON_END`,
/// `PSYNERGY_SHORTCUT_ID_MASK`, `ITEM_LIST_NAME_X`.
pub const MEASURES: &[&str] = &[
    "count", "num", "max", "min", "first", "last", "end", "mask", "shift", "x", "y", "width", "height", "size",
    "tile", "tiles", "slots", "words",
];

/// A definition `kind` classifies.
#[derive(Clone, Copy, Debug)]
pub struct Definition<'a> {
    pub name: &'a str,
    pub file: &'a str,
    pub origin: Origin,
    /// For an enumerator, its enum's tag.
    pub tag: Option<&'a str>,
    /// Its value, when it is a number the walk could read.
    pub value: Option<i64>,
}

impl<'a> Definition<'a> {
    pub fn new(name: &'a str, file: &'a str, origin: Origin) -> Definition<'a> {
        Definition { name, file, origin, tag: None, value: None }
    }
}

/// What kind of game thing a definition names, if any: a scene link by its
/// header, a function or global of the scene and object folders as a
/// mechanism whose words are checked, and otherwise a definition whose
/// family prefix (`FAMILIES`) says what it names, unless what it is shows
/// it to be a part of the family's mechanism (`is_mechanism`). `macros` are
/// the source's numeric macros read so far.
pub fn kind(definition: Definition, macros: &HashMap<String, i64>) -> Option<&'static str> {
    static TEMP: OnceLock<Regex> = OnceLock::new();
    let temp = TEMP.get_or_init(|| Regex::new(r"^FLAG_(?:SCENE|PLACE)_TEMP_[0-9A-Fa-f]+$").unwrap());
    let Definition { name, file, origin, .. } = definition;
    if super::humanity::is_placeholder(name) || temp.is_match(name) {
        return None;
    }
    let family = FAMILIES.iter().find(|(prefix, ..)| name.starts_with(prefix) && name.len() > prefix.len());
    if let Some(&(prefix, kind, word)) = family {
        return (!is_mechanism(&definition, prefix, word, macros)).then_some(kind);
    }
    if origin == Origin::Enumerator && file.ends_with("scene_links.h") {
        return Some("link");
    }
    let mechanism_folder = file.contains("/src/scene/") || file.contains("/src/object/");
    match origin {
        Origin::Function if mechanism_folder => Some("function"),
        Origin::Global if mechanism_folder => Some("global"),
        _ => None,
    }
}

/// A family-prefixed definition that names a part of the family's
/// mechanism (a kind, an effect, a mode, a step, a phase, a bit, a bound, a
/// layout value) rather than one member of the family (an item, an actor, a
/// flag the story sets), from what it is:
///
/// - a function-like macro computes a value (`FLAG_IN_PARTY(character)`,
///   `PSYNERGY_EV(phase)`);
/// - an actor whose first word is a stand-in's (`STAND_INS`), whatever its
///   enum, is a blank actor or a mask;
/// - an enumerator of a tagged enum is what the tag's last word says: an id
///   (`enum ItemId`) or the family's own word (`enum XianActor`) holds
///   members, and any other word the values of a mechanism
///   (`enum AbilityKind`, `enum ActorMotionFlag`, `enum PsynergyPhase`);
/// - its first word after the prefix names an aspect of the family
///   (`ASPECTS`), or its last word a measure (`MEASURES`);
/// - a flag numbered in the range the source names as the engine's own,
///   `FLAG_SYSTEM_FIRST` to `FLAG_SYSTEM_LAST`, is what the field, battles
///   and menus keep about their own working, not a fact of the story.
fn is_mechanism(definition: &Definition, prefix: &str, family: &str, macros: &HashMap<String, i64>) -> bool {
    if definition.origin == Origin::FunctionMacro {
        return true;
    }
    let rest = &definition.name[prefix.len()..];
    let first = rest.split('_').next().unwrap_or("").to_ascii_lowercase();
    if prefix == "ACTOR_" && STAND_INS.contains(&first.as_str()) {
        return true;
    }
    if let Some(last) = definition.tag.and_then(|tag| pieces(tag).last().copied()) {
        return last != "Id" && last != family;
    }
    let last = rest.rsplit('_').next().unwrap_or("").to_ascii_lowercase();
    if ASPECTS.contains(&first.as_str()) || MEASURES.contains(&last.as_str()) {
        return true;
    }
    if prefix == "FLAG_" {
        let range = (macros.get("FLAG_SYSTEM_FIRST"), macros.get("FLAG_SYSTEM_LAST"));
        if let (Some(value), (Some(&low), Some(&high))) = (definition.value, range) {
            return (low..=high).contains(&value);
        }
    }
    false
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Enumerator,
    /// An object-like macro.
    Macro,
    /// A function-like macro.
    FunctionMacro,
    Global,
    Function,
}

/// An enum whose values index one of the game's name tables.
pub struct Table {
    /// The enum's tag.
    pub tag: &'static str,
    /// The prefix its enumerators share, left off before comparing.
    pub prefix: &'static str,
    /// The table's first message: the name of index 0.
    pub symbol: &'static str,
    /// A macro whose value the enum's numbers start from (an enemy's class id
    /// is its row plus `ENEMY_CLASS_FIRST`), if any.
    pub first: Option<&'static str>,
}

/// The table-indexed enums, as `games/tbs/src/game/README.md` sets out their
/// messages: an item's name is `MsgItemName` + id, an ability's
/// `MsgAbilityName` + id, an enemy's `MsgEnemyName` + its row in
/// `enemies.tsv`, a Djinni's `MsgDjinnName` + its number.
pub const TABLES: &[Table] = &[
    Table { tag: "ItemId", prefix: "ITEM_", symbol: "MsgItemName", first: None },
    Table { tag: "AbilityId", prefix: "ABILITY_", symbol: "MsgAbilityName", first: None },
    Table { tag: "EnemyClassId", prefix: "ENEMY_", symbol: "MsgEnemyName", first: Some("ENEMY_CLASS_FIRST") },
    Table { tag: "DjinniId", prefix: "DJINNI_", symbol: "MsgDjinnName", first: None },
];

/// A name's words for comparing with the game's: lowercase, apostrophes
/// dropped, everything else not a letter or digit a space (`Bandit's
/// Sword` and `BANDITS_SWORD` are both `bandits sword`).
pub fn table_words(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '\'' | '’'))
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A table-indexed enumerator against the game's name for its index, or
/// `None` when it isn't one or its index isn't known.
fn table_check(thing: &Thing, text: &GameText, macros: &HashMap<String, i64>) -> Option<Evidence> {
    let tag = thing.tag.as_deref()?;
    let table = TABLES.iter().find(|table| table.tag == tag)?;
    let first = match table.first {
        Some(name) => *macros.get(name)?,
        None => 0,
    };
    let offset = thing.value? - first;
    let game = text.table_entry(table.symbol, offset)?;
    let words = table_words(thing.name.strip_prefix(table.prefix).unwrap_or(&thing.name));
    let wanted = table_words(game);
    let symbol = table.symbol;
    if words == wanted {
        return Some(Evidence::Table { symbol, offset, numbered: false });
    }
    // Later rows sharing a name take `_2` onwards.
    let base = words.rsplit_once(' ').filter(|(_, n)| n.bytes().all(|b| b.is_ascii_digit())).map(|(base, _)| base);
    if base == Some(wanted.as_str()) {
        return Some(Evidence::Table { symbol, offset, numbered: true });
    }
    Some(Evidence::Mismatch { symbol, offset, game: game.to_string() })
}

/// One row of the worklist.
#[derive(Clone, Debug)]
pub struct Row {
    pub thing: Thing,
    pub words: Vec<String>,
    /// `None` for a mechanism's name.
    pub evidence: Option<Evidence>,
    /// For a scene link not verified otherwise, the words that are neither a
    /// verified scene nor door vocabulary.
    pub door_words: Vec<String>,
}

/// Both checks over a game's things.
#[derive(Default)]
pub struct Analysis {
    pub rows: Vec<Row>,
    /// Each flagged word and the rows that use it, most used first.
    pub words: Vec<(String, Vec<usize>)>,
    /// Each unresolved door word and the links that use it, most used first.
    pub door_words: Vec<(String, Vec<usize>)>,
}

/// The name as prose writes it: `SceneId_Bilibin` is `Bilibin`.
fn spoken(name: &str) -> &str {
    name.strip_prefix("SceneId_").unwrap_or(name)
}

/// `word` stands alone in `text`, between characters no name holds.
fn mentions(text: &str, word: &str) -> bool {
    let is_name = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        !text[..at].ends_with(is_name) && !text[at + word.len()..].starts_with(is_name)
    })
}

/// Words a scene link uses for how it joins scenes: the directions, its
/// side of the door, and the markers for links no exit takes.
pub const DOOR_WORDS: &[&str] = &[
    "to", "from", "north", "south", "east", "west", "northeast", "northwest", "southeast", "southwest", "unused",
    "debug", "battle", "after", "before",
];

/// A scene link's words, uppercase, split as `pieces` splits them.
fn link_words(name: &str) -> Vec<String> {
    pieces(name).iter().map(|piece| piece.to_ascii_uppercase()).collect()
}

/// The scenes a link's words name, longest first at each place, and the
/// words left over, lowercase, in order. Words are compared run together, so
/// `MCCOY_PALACE` is `SceneId_McCoyPalace`.
fn resolve(words: &[String], scenes: &HashMap<String, usize>, longest: usize) -> Vec<Result<usize, String>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < words.len() {
        let found = (1..=longest.min(words.len() - at)).rev().find_map(|len| scenes.get(&words[at..at + len].concat()).map(|&row| (row, len)));
        match found {
            Some((row, len)) => {
                out.push(Ok(row));
                at += len;
            }
            None => {
                out.push(Err(words[at].to_ascii_lowercase()));
                at += 1;
            }
        }
    }
    out
}

/// The words of a comment's ★ line onwards, as stems.
fn star_stems(comment: &str) -> impl Iterator<Item = String> + '_ {
    let words = comment_words(comment);
    let rest = words.split_once('★').map_or(String::new(), |(_, rest)| rest.to_string());
    table_words(&rest).split(' ').filter(|w| !w.is_empty()).map(stem).collect::<Vec<_>>().into_iter()
}

/// Verify the scene links that a ★ line doesn't by their words: each must
/// be a ★-verified scene, a door word, a room word of one of the scenes it
/// names (from the scene's ★ line or a ★-verified link of that scene), or
/// after AFTER or BEFORE a game word or a word of a ★-verified name.
fn check_doors(analysis: &mut Analysis, text: &GameText) {
    let rows = &mut analysis.rows;
    let verified = |row: &Row| matches!(row.evidence, Some(Evidence::Verified(_)));
    let mut scenes: HashMap<String, usize> = HashMap::new();
    let mut longest = 0;
    let mut rooms: HashMap<usize, HashSet<String>> = HashMap::new();
    for (at, row) in rows.iter().enumerate().filter(|(_, row)| row.thing.kind == "scene") {
        let words = link_words(&row.thing.name);
        if words.len() > 2 {
            scenes.entry(words[2..].concat()).or_insert(at);
            longest = longest.max(words.len() - 2);
        }
        if verified(row) {
            rooms.entry(at).or_default().extend(star_stems(&row.thing.own));
        }
    }
    let door = |word: &str| DOOR_WORDS.contains(&word);
    let mut events: HashSet<String> = HashSet::new();
    for row in rows.iter().filter(|row| verified(row)) {
        if row.thing.kind == "link" {
            let parts = resolve(&link_words(&row.thing.name), &scenes, longest);
            let named: Vec<usize> = parts.iter().filter_map(|part| part.as_ref().ok().copied()).collect();
            for word in parts.iter().filter_map(|part| part.as_ref().err()).filter(|word| !door(word)) {
                for scene in &named {
                    rooms.entry(*scene).or_default().insert(stem(word));
                }
            }
        } else if row.thing.kind != "scene" {
            events.extend(link_words(&row.thing.name).iter().skip(1).map(|word| stem(word)));
        }
    }
    let mut words: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for at in 0..rows.len() {
        let row = &rows[at];
        if row.thing.kind != "link" || matches!(row.evidence, Some(Evidence::Verified(_) | Evidence::Failed(_))) {
            continue;
        }
        let parts = resolve(&link_words(&row.thing.name), &scenes, longest);
        let named: Vec<usize> = parts.iter().filter_map(|part| part.as_ref().ok().copied()).collect();
        let mut unresolved: Vec<String> = Vec::new();
        for &scene in &named {
            if !verified(&rows[scene]) {
                unresolved.push(rows[scene].thing.name.clone());
            }
        }
        let mut after = false;
        for word in parts.iter().filter_map(|part| part.as_ref().err()) {
            let stemmed = stem(word);
            let room = named.iter().any(|scene| rooms.get(scene).is_some_and(|set| set.contains(&stemmed)));
            let event = after && (text.uses(word) || events.contains(&stemmed));
            if door(word) {
                after |= word == "after" || word == "before";
            } else if !room && !event {
                unresolved.push(word.clone());
            }
        }
        let mut seen = HashSet::new();
        unresolved.retain(|word| seen.insert(word.clone()));
        let row = &mut rows[at];
        if unresolved.is_empty() && !named.is_empty() {
            row.evidence = Some(Evidence::Rooms);
        } else {
            for word in &unresolved {
                words.entry(word.clone()).or_default().push(at);
            }
            row.door_words = unresolved;
        }
    }
    analysis.door_words = words.into_iter().collect();
    analysis.door_words.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
}

/// Run both checks. `places` are the opening comments of the scene overlays
/// (raw); a paragraph of one that mentions a name counts as its prose.
/// `macros` are the source's numeric macros, for a table's first number.
pub fn analyze(things: &[Thing], places: &[(String, String)], text: &GameText, macros: &HashMap<String, i64>) -> Analysis {
    let paragraphs: Vec<String> = places
        .iter()
        .flat_map(|(_, raw)| {
            comment_lines(raw)
                .split(|line| line.is_empty())
                .map(|lines| lines.join(" "))
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
        })
        .collect();
    let mut quotes = HashMap::new();
    let mut analysis = Analysis::default();
    let mut words: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for thing in things {
        let flagged = unused_words(&thing.name, text);
        let evidence = thing.needs_evidence().then(|| {
            if let Some(table) = table_check(thing, text, macros) {
                return table;
            }
            let own = comment_words(&thing.own);
            if let Some(line) = evidence_line(&own, text) {
                return line;
            }
            if thing.alone {
                if let Some(line) = evidence_line(&comment_words(&thing.context), text) {
                    return line;
                }
            }
            let mut kinds = prose(&own, text, &mut quotes);
            let mut more = |comment: &str, kinds: &mut Vec<&'static str>| {
                for kind in prose(comment, text, &mut quotes) {
                    if !kinds.contains(&kind) {
                        kinds.push(kind);
                    }
                }
            };
            more(&comment_words(&thing.context), &mut kinds);
            for paragraph in paragraphs.iter().filter(|p| mentions(p, spoken(&thing.name))) {
                more(paragraph, &mut kinds);
            }
            if kinds.is_empty() {
                Evidence::None
            } else {
                Evidence::Prose(kinds)
            }
        });
        for word in &flagged {
            words.entry(word.clone()).or_default().push(analysis.rows.len());
        }
        analysis.rows.push(Row { thing: thing.clone(), words: flagged, evidence, door_words: Vec::new() });
    }
    analysis.words = words.into_iter().collect();
    analysis.words.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    check_doors(&mut analysis, text);
    analysis
}

#[cfg(test)]
mod tests {
    use super::*;

    const PO: &str = concat!(
        "msgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n\n",
        "msgctxt \"MsgSignMcCoy\"\nmsgid \"00001\"\nmsgstr \"\"\n\"McCoy's Hidden\\n\"\n\"Treasury{end_wait}\"\n\n",
        "msgctxt \"message\"\nmsgid \"00002\"\nmsgstr \"The trees are moving.{end_now}\"\n\n",
        "msgctxt \"MsgDebugEntryName\"\nmsgid \"00003\"\nmsgstr \"Bilibin Barricade\"\n\n",
        "msgctxt \"message\"\nmsgid \"00004\"\nmsgstr \"Bilibin\"\n\n",
        "msgctxt \"MsgOther\"\nmsgid \"00005\"\nmsgstr \"Kolima Forest\"\n",
    );

    #[test]
    fn text_words_stems_and_areas() {
        let text = GameText::parse(PO, Some(2));
        assert!(text.uses("Treasury") && text.uses("tree") && text.uses("Move") && text.uses("McCoy"));
        assert!(!text.uses("Warehouse") && !text.uses("charset"));
        assert!(text.holds("McCoy's Hidden Treasury") && !text.holds("Hidden Warehouse"));
        assert_eq!(text.areas, vec!["Bilibin Barricade", "Bilibin"]);
        assert_eq!(GameText::parse(PO, None).areas.len(), 2, "up to the next named message");
        assert_eq!(stem("Storms"), "storm");
        assert_eq!(stem("moving"), stem("move"));
        assert_eq!(stem("bodies"), "body");
    }

    #[test]
    fn words_of_names() {
        assert_eq!(pieces("SceneId_SolSanctumLunaSolRooms"), ["Scene", "Id", "Sol", "Sanctum", "Luna", "Sol", "Rooms"]);
        assert_eq!(pieces("gBilibinEvents1"), ["Bilibin", "Events"]);
        assert_eq!(pieces("ACTOR_TREE_MAN_2"), ["ACTOR", "TREE", "MAN"]);
        assert_eq!(pieces("HTMLParser"), ["HTML", "Parser"]);
        assert_eq!(pieces("SceneId_Ov6aaRoom1"), ["Scene", "Id", "Ov", "Room"]);
        let text = GameText::parse(PO, Some(2));
        assert_eq!(unused_words("Bilibin_UpdateWarehouseApproach", &text), ["warehouse", "approach"]);
        assert_eq!(unused_words("ACTOR_MCCOY_TREES", &text), Vec::<String>::new());
        assert_eq!(unused_words("Event_McCoyTreasury", &text), Vec::<String>::new(), "Mc and Coy are one word");
    }

    fn thing(name: &str, own: &str, context: &str) -> Thing {
        Thing {
            name: name.to_string(),
            kind: "actor",
            file: "games/tbs/src/scene/bilibin.c".to_string(),
            line: 1,
            own: own.to_string(),
            context: context.to_string(),
            ..Thing::default()
        }
    }

    #[test]
    fn a_lone_enumerator_takes_its_enums_line() {
        let text = GameText::parse(PO, Some(2));
        let line = "/// The barricade is down.\n/// ★ Named from its area name: Bilibin Barricade";
        let lone = Thing { alone: true, ..thing("FLAG_BARRICADE_DOWN", "", line) };
        let shared = thing("FLAG_BARRICADE_UP", "", line);
        let analysis = analyze(&[lone, shared], &[], &text, &HashMap::new());
        let evidence: Vec<String> = analysis.rows.iter().map(|row| row.evidence.as_ref().unwrap().describe()).collect();
        assert_eq!(evidence, ["verified area", "prose area"], "an enum of several keeps its line its own");
    }

    #[test]
    fn evidence_lines_and_prose() {
        let text = GameText::parse(PO, Some(2));
        let things = [
            thing("ACTOR_A", "/// ★ Named from the game: \"McCoy's Hidden\n/// Treasury\" (MsgSignMcCoy)", ""),
            thing("ACTOR_B", "///< Named from its area name: Bilibin Barricade.", ""),
            thing("ACTOR_C", "/// ★ Named from what the player sees: a stack of crates (its render)", ""),
            thing("ACTOR_D", "/// ★ Named from the game: \"Hidden Warehouse\" (MsgSignMcCoy)", ""),
            thing("ACTOR_E", "/// ★ Named from its area name: Bilibin Warehouse", ""),
            thing("ACTOR_F", "/// ★ Named from the game: \"The trees\" (MsgNope)", ""),
            thing("ACTOR_G", "/// Says `The trees are moving.`", ""),
            thing("ACTOR_H", "/// The sign (MsgSignMcCoy).", ""),
            thing("ACTOR_I", "", "/// The actors, as their renders show them."),
            thing("ACTOR_J", "/// A raised warehouse approach.", "/// Says \"warehouse\"."),
            thing("ACTOR_K", "", ""),
            thing("ACTOR_L", "/// ★ Named from the game: \"The trees are moving.\" (MsgSignMcCoy)", ""),
            thing("ACTOR_M", "/// ★ Named from the game: \"Kolima Forest\" (MsgSignMcCoy)", ""),
        ];
        let places = [(
            "games/tbs/src/scene/bilibin.c".to_string(),
            "/// @file\n/// ACTOR_K stands where its area name says.\n///\n/// ACTOR_J says `Treasury`.".to_string(),
        )];
        let analysis = analyze(&things, &places, &text, &HashMap::new());
        let evidence: Vec<String> = analysis.rows.iter().map(|row| row.evidence.as_ref().unwrap().describe()).collect();
        assert_eq!(evidence[0], "verified quote");
        assert_eq!(evidence[1], "unstarred area");
        assert_eq!(evidence[2], "verified render");
        assert!(evidence[3].starts_with("failed: MsgSignMcCoy does not hold"));
        assert!(evidence[4].starts_with("failed: no area name"));
        assert_eq!(evidence[5], "failed: no message MsgNope");
        assert_eq!(evidence[6], "prose quote");
        assert_eq!(evidence[7], "prose message");
        assert_eq!(evidence[8], "prose render");
        assert_eq!(evidence[9], "prose quote", "its place paragraph quotes the game");
        assert_eq!(evidence[10], "prose area");
        assert_eq!(evidence[11], "verified quote", "an unnamed message after the symbol's own");
        assert!(evidence[12].starts_with("failed"), "not past the next named message");
    }

    #[test]
    fn flagged_words_by_use() {
        let text = GameText::parse(PO, Some(2));
        let mut function = thing("Bilibin_RaiseWarehouse", "", "");
        function.kind = "function";
        let things = [thing("ACTOR_WAREHOUSE_GUARD", "", ""), function, thing("ACTOR_PILLAR", "", "")];
        let analysis = analyze(&things, &[], &text, &HashMap::new());
        assert_eq!(analysis.words[0], ("warehouse".to_string(), vec![0, 1]));
        assert!(analysis.rows[1].evidence.is_none(), "a mechanism's name needs no evidence");
        let kind = |name, file, origin| kind(Definition::new(name, file, origin), &HashMap::new());
        assert_eq!(kind("SceneId_Vale", "a.h", Origin::Global), Some("scene"));
        assert_eq!(kind("FLAG_SCENE_TEMP_01", "a.h", Origin::Macro), None);
        assert_eq!(kind("VALE_FROM_WORLD_MAP", "games/tbs/include/scene_links.h", Origin::Enumerator), Some("link"));
        assert_eq!(kind("Vale_Init", "games/tbs/src/scene/vale.c", Origin::Function), Some("function"));
        assert_eq!(kind("Battle_Init", "games/tbs/src/battle/x.c", Origin::Function), None);
        assert_eq!(kind("ACTOR_unk_14", "a.h", Origin::Enumerator), None);
    }

    #[test]
    fn mechanisms_of_a_family() {
        let macros = HashMap::from([("FLAG_SYSTEM_FIRST".to_string(), 0x100), ("FLAG_SYSTEM_LAST".to_string(), 0x1ff)]);
        let of = |name, origin, tag, value| {
            kind(Definition { tag, value, ..Definition::new(name, "games/tbs/include/a.h", origin) }, &macros)
        };
        let enumerator = |name, tag| of(name, Origin::Enumerator, tag, None);
        assert_eq!(enumerator("ITEM_HERB", Some("ItemId")), Some("item"), "an id enum holds members");
        assert_eq!(enumerator("ACTOR_GUARD", Some("BilibinActor")), Some("actor"), "so does one tagged by the family");
        assert_eq!(enumerator("ACTOR_GUARD", None), Some("actor"), "an untagged enum, by its words");
        assert_eq!(enumerator("ACTOR_BLANK_ROCK_GUIDE", Some("WorldMapCaravanActor")), None, "a blank actor");
        assert_eq!(enumerator("ACTOR_MASK_FOR_STUMP", Some("GomaCavePillarActor")), None, "a mask");
        assert_eq!(enumerator("ACTOR_YOUNG_MAN_SAW_MASK", Some("ValeInteriorsInnActor")), Some("actor"), "a mask last is a word");
        assert_eq!(enumerator("ACTOR_MASKED_MAN", Some("ValeInteriorsInnActor")), Some("actor"));
        assert_eq!(enumerator("ACTOR_HIDDEN_PUDDLE", Some("LunpaActor")), Some("actor"), "hidden is not blank");
        assert_eq!(enumerator("ITEM_BLANK", Some("ItemId")), Some("item"), "stand-ins are actors");
        assert_eq!(enumerator("ABILITY_KIND_SUMMON", Some("AbilityKind")), None);
        assert_eq!(enumerator("ACTOR_SOLID", Some("ActorCollisionFlag")), None, "an actor's bits");
        assert_eq!(enumerator("CHARACTER_POISON", Some("CharacterAilment")), None, "a state, not a person");
        assert_eq!(of("FLAG_IN_PARTY", Origin::FunctionMacro, None, None), None, "a computed value");
        assert_eq!(of("ITEM_TYPE_WEAPON", Origin::Macro, None, Some(1)), None, "an aspect");
        assert_eq!(of("PSYNERGY_STEP_TARGET", Origin::Enumerator, None, Some(3)), None);
        assert_eq!(of("ITEM_LIST_NAME_X", Origin::Macro, None, Some(9)), None, "a position on the screen");
        assert_eq!(of("ENEMY_SPRITE_SLOTS", Origin::Macro, None, Some(9)), None, "a measure");
        assert_eq!(of("FLAG_IN_BATTLE", Origin::Macro, None, Some(0x103)), None, "the engine's own flag");
        assert_eq!(of("FLAG_KRADEN_SETS_OFF", Origin::Macro, None, Some(0x801)), Some("flag"), "a story flag");
        assert_eq!(of("FLAG_CHILDHOOD", Origin::Macro, None, Some(0x20)), Some("flag"));
        let unranged = kind(Definition { value: Some(0x103), ..Definition::new("FLAG_IN_BATTLE", "a.h", Origin::Macro) }, &HashMap::new());
        assert_eq!(unranged, Some("flag"), "no range named, no range");
    }

    /// Item and enemy name tables, numbered as the real catalog numbers them.
    const TABLE_PO: &str = concat!(
        "msgctxt \"MsgItemName\"\nmsgid \"00386\"\nmsgstr \"???\"\n\n",
        "msgctxt \"message\"\nmsgid \"00387\"\nmsgstr \"{article_class:2}Bandit's Sword\"\n\n",
        "msgctxt \"message\"\nmsgid \"00388\"\nmsgstr \"Hyper-Boots\"\n\n",
        "msgctxt \"MsgEnemyName\"\nmsgid \"00389\"\nmsgstr \"???\"\n\n",
        "msgctxt \"message\"\nmsgid \"00390\"\nmsgstr \"Slime\"\n\n",
        "msgctxt \"message\"\nmsgid \"00391\"\nmsgstr \"Slime\"\n",
    );

    fn entry(name: &str, kind: &'static str, tag: &str, value: i64) -> Thing {
        Thing { name: name.to_string(), kind, tag: Some(tag.to_string()), value: Some(value), ..Thing::default() }
    }

    #[test]
    fn table_names() {
        let text = GameText::parse(TABLE_PO, None);
        assert_eq!(text.table_entry("MsgItemName", 1), Some("Bandit's Sword"), "by msgid, commands taken out");
        assert_eq!(text.table_entry("MsgEnemyName", 2), Some("Slime"));
        assert_eq!(text.table_entry("MsgNope", 0), None);
        assert_eq!(table_words("{x}Bandit's Sword"), "x bandits sword");
        let macros = HashMap::from([("ENEMY_CLASS_FIRST".to_string(), 8)]);
        let mut unknown = entry("ITEM_MYSTERY", "item", "ItemId", 0);
        unknown.value = None;
        let things = [
            entry("ITEM_BANDITS_SWORD", "item", "ItemId", 1),
            entry("ITEM_HYPER_BOOTS", "item", "ItemId", 2),
            entry("ITEM_SPEED_BOOTS", "item", "ItemId", 2),
            entry("ENEMY_SLIME", "enemy", "EnemyClassId", 9),
            entry("ENEMY_SLIME_2", "enemy", "EnemyClassId", 10),
            entry("ENEMY_OOZE", "enemy", "EnemyClassId", 10),
            Thing { own: "/// ★ Named from the game: \"Slime\" (MsgEnemyName)".to_string(), ..entry("ITEM_X", "item", "Other", 1) },
            unknown,
        ];
        let analysis = analyze(&things, &[], &text, &macros);
        let evidence: Vec<String> = analysis.rows.iter().map(|row| row.evidence.as_ref().unwrap().describe()).collect();
        assert_eq!(evidence[0], "table MsgItemName+1");
        assert_eq!(evidence[1], "table MsgItemName+2", "a hyphen is a space");
        assert_eq!(evidence[2], "mismatch: MsgItemName+2 is \"Hyper-Boots\"");
        assert_eq!(evidence[3], "table MsgEnemyName+1", "an enemy's row is its class id less ENEMY_CLASS_FIRST");
        assert_eq!(evidence[4], "table MsgEnemyName+2 numbered");
        assert_eq!(evidence[5], "mismatch: MsgEnemyName+2 is \"Slime\"");
        assert_eq!(evidence[6], "verified quote", "an enum no table reads keeps its ★ line");
        assert_eq!(evidence[7], "none", "an unknown value is not checked");
        assert_eq!(analysis.rows[2].evidence.as_ref().unwrap().metric(), "provenance_table_mismatch");
        let without = analyze(&things[3..4], &[], &text, &HashMap::new());
        assert_eq!(without.rows[0].evidence, Some(Evidence::None), "no ENEMY_CLASS_FIRST, no check");
    }

    #[test]
    fn door_names() {
        let po = concat!(
            "msgctxt \"MsgDebugEntryName\"\nmsgid \"00001\"\nmsgstr \"Tolbi\"\n\n",
            "msgctxt \"MsgSignInn\"\nmsgid \"00002\"\nmsgstr \"Inn Full\"\n\n",
            "msgctxt \"message\"\nmsgid \"00003\"\nmsgstr \"The ship's crew was attacked.\"\n",
        );
        let text = GameText::parse(po, Some(1));
        let scene = |name: &str, own: &str| Thing {
            name: name.to_string(),
            kind: "scene",
            own: own.to_string(),
            ..Thing::default()
        };
        let link = |name: &str, own: &str| Thing {
            name: name.to_string(),
            kind: "link",
            own: own.to_string(),
            ..Thing::default()
        };
        let things = [
            scene("SceneId_Tolbi", "/// ★ Named from its area name: Tolbi"),
            scene("SceneId_TolbiShip", "/// ★ Named from what the player sees: Tolbi's ship and its well-kept deck (its render)"),
            scene("SceneId_TolbiShipMast", ""),
            Thing { kind: "flag", ..scene("FLAG_SAW_LIGHTHOUSE", "/// ★ Named from the game: \"Inn Full\" (MsgSignInn)") },
            link("TOLBI_TO_INN", "///< ★ Named from the game: \"Inn Full\" (MsgSignInn)"),
            link("TOLBI_FROM_INN", ""),
            link("TOLBI_FROM_TOLBI_SHIP_DECK_NORTH", ""),
            link("TOLBI_SHIP_FROM_TOLBI_SHIP_MAST", ""),
            link("TOLBI_FROM_HOUSE_UPPER", ""),
            link("TOLBI_SHIP_AFTER_CREW_ATTACKED", ""),
            link("TOLBI_SHIP_AFTER_LIGHTHOUSE", "/// Tolbi's dock."),
            link("TOLBI_SHIP_CREW_ATTACKED", ""),
            link("ENTRANCE_DEFAULT", ""),
            link("TOLBI_FROM_INN_BROKEN", "///< ★ Named from the game: \"Inn Empty\" (MsgSignInn)"),
            scene("SceneId_McCoyPalace", "/// ★ Named from its area name: Tolbi"),
            link("TOLBI_FROM_MCCOY_PALACE", ""),
        ];
        let analysis = analyze(&things, &[], &text, &HashMap::new());
        let evidence: Vec<String> = analysis.rows.iter().map(|row| row.evidence.as_ref().unwrap().describe()).collect();
        let doors: Vec<String> = analysis.rows.iter().map(|row| row.door_words.join(",")).collect();
        assert_eq!(evidence[4], "verified quote");
        assert_eq!(evidence[5], "rooms", "INN, from the ★-verified TOLBI_TO_INN");
        assert_eq!(evidence[6], "rooms", "DECK, from SceneId_TolbiShip's ★ line");
        assert_eq!((evidence[7].as_str(), doors[7].as_str()), ("none", "SceneId_TolbiShipMast"), "an unverified scene");
        assert_eq!(doors[8], "house,upper");
        assert_eq!(evidence[9], "rooms", "after AFTER, the game's words");
        assert_eq!(evidence[10], "rooms", "after AFTER, a ★-verified name's words");
        assert_eq!(doors[11], "crew,attacked", "the game's words only after AFTER or BEFORE");
        assert_eq!(doors[12], "entrance,default", "no scene, nothing verified");
        assert!(evidence[13].starts_with("failed") && doors[13].is_empty(), "a failing line stays a finding");
        assert!(analysis.door_words.contains(&("attacked".to_string(), vec![11])));
        assert_eq!(analysis.rows[5].evidence.as_ref().unwrap().metric(), "provenance_rooms");
        assert_eq!(evidence[15], "rooms", "MCCOY is McCoy");
    }
}
