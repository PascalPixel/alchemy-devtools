//! Name provenance for the humanization report: whether the names of game
//! things (scenes, actors, flags, scene links, items, abilities and the like)
//! come from evidence or were made up (Alchemy's S3). Two checks, both read
//! from the source and the game's English text (`text/en.po`):
//!
//! - words the game never uses: each name is split into words, and a word is
//!   accounted for when the game's text uses it (after crude stemming) or it
//!   is on the short programming vocabulary below;
//! - names without evidence: a name is verified only by a `★ Named from ...`
//!   line in its own comment whose quote or area name the text confirms;
//!   short of that, evidence in its prose (a quote the text holds, a message
//!   symbol, an area-name citation, a note on its render) is told apart from
//!   none at all.
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
    "trigger", "marker", "cycle", "blank", "hblank", "vblank", "alpha", "vector",
    // Positions.
    "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth", "last", "north",
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
        let flush = |ctxt: &mut Option<String>, msgstr: &mut Option<String>, text: &mut GameText| {
            if let Some(body) = msgstr.take() {
                let symbol = ctxt.take().filter(|c| c != "message");
                text.messages.push((symbol, flatten(&body)));
            }
            *ctxt = None;
        };
        for line in po.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("msgctxt ") {
                flush(&mut ctxt, &mut msgstr, &mut text);
                ctxt = Some(unquote(rest));
                in_str = false;
            } else if line.starts_with("msgid ") {
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
        flush(&mut ctxt, &mut msgstr, &mut text);
        // The header entry has no context and no game text.
        text.messages.retain(|(_, body)| !body.starts_with("Content-Type:"));
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
#[derive(Clone, Debug)]
pub struct Thing {
    pub name: String,
    pub kind: &'static str,
    pub file: String,
    pub line: usize,
    /// Its own `///` comment and `///<` notes.
    pub own: String,
    /// Its enum's comment, for an enumerator.
    pub context: String,
}

impl Thing {
    /// Names of code mechanisms (functions, tables) are only checked for words.
    pub fn needs_evidence(&self) -> bool {
        !matches!(self.kind, "function" | "global")
    }
}

/// What kind of game thing a definition names, if any: by prefix, or a scene
/// link by its header, or a function or global of the scene and object folders.
pub fn kind(name: &str, file: &str, origin: Origin) -> Option<&'static str> {
    static TEMP: OnceLock<Regex> = OnceLock::new();
    let temp = TEMP.get_or_init(|| Regex::new(r"^FLAG_(?:SCENE|PLACE)_TEMP_[0-9A-Fa-f]+$").unwrap());
    if super::humanity::is_placeholder(name) || temp.is_match(name) {
        return None;
    }
    const PREFIXES: &[(&str, &str)] = &[
        ("SceneId_", "scene"),
        ("ACTOR_", "actor"),
        ("FLAG_", "flag"),
        ("ITEM_", "item"),
        ("ABILITY_", "ability"),
        ("PSYNERGY_", "psynergy"),
        ("DJINNI_", "djinni"),
        ("ENEMY_", "enemy"),
        ("MONSTER_", "enemy"),
        ("SUMMON_", "summon"),
        ("CHARACTER_", "character"),
        ("CLASS_", "class"),
        ("PERSON_", "person"),
        ("AREA_", "place"),
    ];
    if let Some(&(_, kind)) = PREFIXES.iter().find(|(prefix, _)| name.starts_with(prefix) && name.len() > prefix.len()) {
        return Some(kind);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Enumerator,
    Macro,
    Global,
    Function,
}

/// One row of the worklist.
#[derive(Clone, Debug)]
pub struct Row {
    pub thing: Thing,
    pub words: Vec<String>,
    /// `None` for a mechanism's name.
    pub evidence: Option<Evidence>,
}

/// Both checks over a game's things.
#[derive(Default)]
pub struct Analysis {
    pub rows: Vec<Row>,
    /// Each flagged word and the rows that use it, most used first.
    pub words: Vec<(String, Vec<usize>)>,
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

/// Run both checks. `places` are the opening comments of the scene overlays
/// (raw); a paragraph of one that mentions a name counts as its prose.
pub fn analyze(things: &[Thing], places: &[(String, String)], text: &GameText) -> Analysis {
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
            let own = comment_words(&thing.own);
            if let Some(line) = evidence_line(&own, text) {
                return line;
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
        analysis.rows.push(Row { thing: thing.clone(), words: flagged, evidence });
    }
    analysis.words = words.into_iter().collect();
    analysis.words.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
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
        }
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
        let analysis = analyze(&things, &places, &text);
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
        let analysis = analyze(&things, &[], &text);
        assert_eq!(analysis.words[0], ("warehouse".to_string(), vec![0, 1]));
        assert!(analysis.rows[1].evidence.is_none(), "a mechanism's name needs no evidence");
        assert_eq!(kind("SceneId_Vale", "a.h", Origin::Global), Some("scene"));
        assert_eq!(kind("FLAG_SCENE_TEMP_01", "a.h", Origin::Macro), None);
        assert_eq!(kind("VALE_FROM_WORLD_MAP", "games/tbs/include/scene_links.h", Origin::Enumerator), Some("link"));
        assert_eq!(kind("Vale_Init", "games/tbs/src/scene/vale.c", Origin::Function), Some("function"));
        assert_eq!(kind("Battle_Init", "games/tbs/src/battle/x.c", Origin::Function), None);
        assert_eq!(kind("ACTOR_unk_14", "a.h", Origin::Enumerator), None);
    }
}
