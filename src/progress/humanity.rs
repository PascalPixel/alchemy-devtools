//! The humanization report: numbers for the work of making the matched C
//! readable (S3), meant to fall the way DONE rose. Each game's tracked C and
//! headers under `src` and `include` are lexed (never preprocessed) and
//! counted by simple rules the README sets out: magic numbers, placeholder
//! and numbered names, compiler-steering devices, edition tests, undocumented
//! declarations, long files, duplicated code and, with the game's English
//! text, where the names of game things come from (`provenance`), and whether
//! the names can be guessed from one another (`naming`). Nothing here counts
//! toward DONE (C2); the figures are for people.

use super::clex::{self, Kind, Token};
use super::naming::{self, NameKind};
use super::provenance::{self, Analysis, Definition, GameText, Origin, Thing};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::OnceLock;

pub const REPORT: &str = "out/reports/humanity.tsv";

/// S2's devices, in its order.
pub const DEVICES: [&str; 8] = ["wrapper", "volatile", "temporary", "dead", "one-pass", "barrier", "pin", "signature"];

/// A function body this many tokens long or more takes part in the duplicate search.
const FUNCTION_TOKENS: usize = 24;
/// A table initializer this many tokens long or more takes part in the duplicate search.
const TABLE_TOKENS: usize = 16;
/// Files longer than this are listed (S3's split threshold).
const LONG_FILE: usize = 3000;
/// How many files and groups each top list keeps in the TSV.
const TOP: usize = 10;

const KEYWORDS: &[&str] = &[
    "auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else", "enum", "extern",
    "float", "for", "goto", "if", "inline", "int", "long", "register", "return", "short", "signed", "sizeof",
    "static", "struct", "switch", "typedef", "union", "unsigned", "void", "volatile", "while", "asm", "__asm__",
    "__attribute__", "__inline__", "__volatile__", "typeof", "__typeof__", "_Bool", "__extension__", "defined",
];

/// Word stems before a digit run that name hardware or a convention, not a
/// meaning still to find: `BG2`, `Dma3`, `TM0`, `DISPCNT_MODE_1`, `TBS_PATCH_1`.
const NUMBERED_STEMS: &[&str] = &[
    "bg", "dma", "tm", "timer", "win", "obj", "oam", "tgt", "mode", "fifo", "sound", "snd", "ch", "key", "sio",
    "joy", "irq", "attr", "patch", "tan", "vec", "vector", "lz", "utf", "crc", "sha", "base", "rgb", "vram",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Brace {
    /// An initializer list, `= { ... }`: table data.
    Init,
    Enum,
    /// A struct or union body: field layout.
    Struct,
    /// A function body.
    Func,
    /// Any other block inside a function.
    Block,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Group {
    pub tokens: usize,
    /// `path:name` of each copy.
    pub members: Vec<String>,
}

#[derive(Default)]
struct Declared {
    documented: bool,
    line: usize,
    /// Where it is defined, else where it is first declared.
    file: String,
    defined: bool,
    /// Declared in a far-call header: an entry point another bank reaches.
    entry: bool,
}

/// A unit H4 judges: a map overlay or a folder guide, how many of its
/// open questions are `- XXX:` items, and whether it
/// has the list at all (`/// Open questions:`, `## Evidence and open
/// questions`).
pub struct Unit {
    pub file: String,
    pub open: u64,
    pub listed: bool,
}

/// One game's counts.
#[derive(Default)]
pub struct Report {
    pub files: usize,
    pub lines: usize,
    /// metric -> file -> count.
    pub tallies: BTreeMap<String, BTreeMap<String, u64>>,
    /// Distinct placeholder and numbered names.
    pub names: BTreeMap<&'static str, BTreeSet<String>>,
    pub long_files: Vec<(String, usize)>,
    /// Each unit H4 judges (a map overlay, a folder guide) and its open
    /// questions.
    pub units: Vec<Unit>,
    pub function_groups: Vec<Group>,
    pub table_groups: Vec<Group>,
    /// Each counted thing, for `--list`.
    pub items: Vec<Item>,
    functions: BTreeMap<String, Declared>,
    globals: BTreeMap<String, Declared>,
    /// Definitions that may name game things, for the provenance checks.
    pub things: Vec<Thing>,
    /// Each map overlay's opening comment, raw: its place list.
    pub places: Vec<(String, String)>,
    /// The provenance checks, when the game has its English text.
    pub provenance: Option<Analysis>,
    /// Object-like macros whose value is a number or a simple sum, for the
    /// tables' first numbers (`ENEMY_ID_FIRST`).
    pub macro_values: HashMap<String, i64>,
    /// Each lone argument in code, settled once every declaration is read.
    arguments: Vec<Argument>,
    /// Each declared function's parameter names.
    parameters: HashMap<String, Vec<Option<String>>>,
    /// Each function-like macro's parameters and the call each is passed to.
    macro_parameters: HashMap<String, Vec<(String, Option<(String, usize)>)>>,
    function_bodies: HashMap<String, Group>,
    table_bodies: HashMap<String, Group>,
    /// Every name the source defines or declares, for the naming checks.
    pub naming: naming::Names,
}

/// A lone number passed to a call, with each call around it and the
/// argument it fills (from 0), innermost first, as far as the lexer can tell.
#[derive(Clone, Debug)]
struct Argument {
    file: String,
    line: usize,
    literal: String,
    calls: Vec<(String, usize)>,
}

/// The words of parameter names that hold data S3 lets stay a number: a
/// coordinate (a cell's row and column included), a duration in frames, a
/// size or a count. A speed, an angle, a weight, a mode or a value is not
/// here: its number may have a meaning to name.
const DATA_PARAMETERS: &[(&str, &[&str])] = &[
    ("coordinate", &["x", "y", "z", "dx", "dy", "dz", "row", "column"]),
    ("frames", &["frames", "duration", "delay", "ticks"]),
    ("size", &["width", "height", "w", "h", "size", "length", "radius", "distance"]),
    ("count", &["count"]),
];

/// Words that make a parameter a rate, whatever else it says: `stepX`,
/// `initial_velocity_y`.
const RATE_WORDS: &[&str] = &["velocity", "speed", "step", "freq", "frequency", "acceleration"];

/// What kind of data a parameter's name says it holds, from any of its words
/// with their digits dropped: `srcX`, `fixed_x` and `xOffset` a coordinate,
/// `frames` a duration.
pub fn data_parameter(name: &str) -> Option<&'static str> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut previous = '_';
    for c in name.chars() {
        if c == '_' || c.is_ascii_digit() || (c.is_ascii_uppercase() && previous.is_ascii_lowercase()) {
            words.push(std::mem::take(&mut word));
        }
        if c.is_ascii_alphabetic() {
            word.push(c.to_ascii_lowercase());
        }
        previous = c;
    }
    words.push(word);
    if words.iter().any(|word| RATE_WORDS.contains(&word.as_str())) {
        return None;
    }
    DATA_PARAMETERS.iter().find(|(_, list)| words.iter().any(|word| list.contains(&word.as_str()))).map(|(kind, _)| *kind)
}

/// The callee of the call whose argument list holds the token at `at`, and
/// which argument it is in (from 0), reading back no further than `floor`: `ObjectEvent_WalkTo`
/// and 1 for the `257` of `ObjectEvent_WalkTo(LOCALID_PARTY_LEADER, 257, 420)`.
/// Parentheses that only group (`(x) << 16`) are looked through.
fn call_site(tokens: &[Token], at: usize, floor: usize) -> Option<(usize, usize)> {
    let directive = tokens[at].pp;
    let (mut depth, mut position) = (0, 0);
    let mut index = at;
    while index > floor {
        index -= 1;
        let token = &tokens[index];
        if token.pp != directive || token.kind == Kind::Comment {
            continue;
        }
        match token.text {
            ")" | "]" => depth += 1,
            "[" if depth > 0 => depth -= 1,
            "(" if depth > 0 => depth -= 1,
            "(" => {
                let callee = (index > floor).then(|| &tokens[index - 1]);
                match callee {
                    Some(name) if name.kind == Kind::Ident && !is_keyword(name.text) => return Some((index - 1, position)),
                    // A grouping parenthesis: the argument is the whole group.
                    _ => position = 0,
                }
            }
            "," if depth == 0 => position += 1,
            "{" | "}" | ";" | "[" => return None,
            _ => {}
        }
    }
    None
}

/// One counted thing: where it is and what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub metric: String,
    pub file: String,
    pub line: usize,
    pub what: String,
}

impl Report {
    fn add(&mut self, metric: &str, file: &str, count: u64) {
        if count > 0 {
            *self.tallies.entry(metric.to_string()).or_default().entry(file.to_string()).or_default() += count;
        }
    }

    fn note(&mut self, metric: &str, file: &str, line: usize, what: &str) {
        self.add(metric, file, 1);
        self.items.push(Item { metric: metric.to_string(), file: file.to_string(), line, what: what.to_string() });
    }

    pub fn total(&self, metric: &str) -> u64 {
        self.tallies.get(metric).map_or(0, |files| files.values().sum())
    }

    /// The files with the most of a metric, most first.
    pub fn top(&self, metric: &str, count: usize) -> Vec<(&str, u64)> {
        let mut files: Vec<(&str, u64)> =
            self.tallies.get(metric).map_or(Vec::new(), |files| files.iter().map(|(f, n)| (f.as_str(), *n)).collect());
        files.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        files.truncate(count);
        files
    }

    pub fn distinct(&self, kind: &str) -> usize {
        self.names.get(kind).map_or(0, BTreeSet::len)
    }

    fn declare(&mut self, function: bool, name: &str, file: &str, line: usize, documented: bool, defined: bool) {
        let map = if function { &mut self.functions } else { &mut self.globals };
        let entry = map.entry(name.to_string()).or_insert_with(|| Declared { file: file.to_string(), line, ..Declared::default() });
        entry.documented |= documented;
        entry.entry |= file.contains("/include/farcall/");
        if defined && !entry.defined {
            entry.defined = true;
            entry.file = file.to_string();
            entry.line = line;
        }
    }

    /// The parameter an argument fills, by name, and the kind of data that
    /// name says it holds. A macro's parameter takes its own name's kind, else
    /// that of the parameter it is passed to (`WAIT(n)` is
    /// `FieldScript_WaitFrames(s32 frames)`).
    fn parameter(&self, callee: &str, position: usize, depth: usize) -> Option<(String, Option<&'static str>)> {
        if let Some(names) = self.parameters.get(callee) {
            let name = names.get(position)?.clone()?;
            let kind = data_parameter(&name);
            return Some((name, kind));
        }
        let (name, call) = self.macro_parameters.get(callee)?.get(position)?;
        let inner = call.as_ref().filter(|_| depth < 8).and_then(|(callee, at)| self.parameter(callee, *at, depth + 1));
        let kind = data_parameter(name).or_else(|| inner.as_ref().and_then(|(_, kind)| *kind));
        Some((name.clone(), kind))
    }

    /// The kind of data a lone argument is, from the calls around it: the
    /// parameter it fills, or, where that is a macro that only converts its
    /// value (`PIXELS(value)`, `((value) << 16)`), the parameter the macro's
    /// result fills.
    fn argument_kind(&self, calls: &[(String, usize)]) -> Option<&'static str> {
        for (callee, at) in calls {
            let (_, kind) = self.parameter(callee, *at, 0)?;
            if kind.is_some() {
                return kind;
            }
            let converts = self.macro_parameters.get(callee).and_then(|list| list.get(*at)).is_some_and(|(_, call)| call.is_none());
            if !converts {
                return None;
            }
        }
        None
    }

    /// Settle what needs the whole game: lone arguments by the parameters
    /// they fill, documentation across declarations, and the duplicate groups.
    fn finish(&mut self) {
        for argument in std::mem::take(&mut self.arguments) {
            let (file, line, literal) = (&argument.file, argument.line, &argument.literal);
            let what = match argument.calls.first() {
                Some((callee, at)) => match self.parameter(callee, *at, 0) {
                    Some((name, _)) => format!("{literal}\t{callee} {name}"),
                    None => format!("{literal}\t{callee} #{}", at + 1),
                },
                None => format!("{literal}\t-"),
            };
            match self.argument_kind(&argument.calls) {
                Some(kind) => {
                    self.note("data_argument", file, line, &what);
                    self.note(&format!("data_argument_{kind}"), file, line, &what);
                }
                None => {
                    self.note("magic_code", file, line, literal);
                    self.note("magic_argument", file, line, &what);
                }
            }
        }
        // Each list in file and line order, as the files were read.
        self.items.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
        for (function, kind) in [(true, "undocumented_function"), (false, "undocumented_global")] {
            let missing: Vec<(String, usize, String)> = (if function { &self.functions } else { &self.globals })
                .iter()
                .filter(|(_, declared)| !declared.documented)
                .map(|(name, declared)| (declared.file.clone(), declared.line, name.clone()))
                .collect();
            for (file, line, name) in missing {
                self.note(kind, &file, line, &name);
            }
        }
        // D9 asks every entry point for its summary; any other declaration only
        // where it has something to say, which no count can tell.
        let entries: Vec<(String, usize, String)> = self
            .functions
            .iter()
            .filter(|(_, declared)| declared.entry && !declared.documented)
            .map(|(name, declared)| (declared.file.clone(), declared.line, name.clone()))
            .collect();
        for (file, line, name) in entries {
            self.note("undocumented_entry", &file, line, &name);
        }
        let groups = |bodies: &mut HashMap<String, Group>| {
            let mut groups: Vec<Group> = bodies.drain().map(|(_, group)| group).filter(|g| g.members.len() > 1).collect();
            for group in &mut groups {
                group.members.sort();
            }
            groups.sort_by(|a, b| {
                (b.tokens * (b.members.len() - 1)).cmp(&(a.tokens * (a.members.len() - 1))).then(a.members.cmp(&b.members))
            });
            groups
        };
        self.function_groups = groups(&mut self.function_bodies);
        self.table_groups = groups(&mut self.table_bodies);
        self.long_files.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    }

    /// Copies past the first in each group.
    pub fn copies(groups: &[Group]) -> usize {
        groups.iter().map(|group| group.members.len() - 1).sum()
    }
}

fn regexes() -> &'static [Regex; 3] {
    static SET: OnceLock<[Regex; 3]> = OnceLock::new();
    SET.get_or_init(|| {
        [
            // Placeholders.
            Regex::new(concat!(
                r"^(?:[gs]?(?:unk|Unk|UNK)(?:nown)?(?:$|[_0-9A-Z]))",
                r"|^(?:sub|func|Func|data|loc|off|byte|word|dword|nullsub|D)_[0-9A-Fa-f]{2,}",
                r"|^field_[0-9A-Fa-f]+",
                r"|[_a-z0-9]Unk(?:nown)?(?:$|[_0-9A-Z])|_unk(?:$|[_0-9])|_UNK(?:$|[_0-9])",
                r"|_0[23689][0-9A-Fa-f]{6}(?:$|_)",
                r"|^(?:SE|MUS)_[0-9]+$|^nullsub",
            ))
            .unwrap(),
            // Conventions that keep their number: temporary flags, registers and
            // the compiler library (`__divsi3`).
            Regex::new(r"^FLAG_(?:MAP|MAPSEC)_TEMP_[0-9A-F]+$|^REG_|^__").unwrap(),
            // Edition names, which belong in linker scripts and assembly.
            Regex::new(r"^(?:TBS|TLA)_EDITION_[A-Z]+$").unwrap(),
        ]
    })
}

/// `unk_14`, `sub_08012345`, `gUnknown_0203`, `field_1C`, `SE_138`: a name
/// that says it is not known yet.
pub fn is_placeholder(name: &str) -> bool {
    regexes()[0].is_match(name)
}

/// The last word of a name: `Script` in `gEffectScript`, `BG` in `BLDCNT_TGT1_BG`.
fn last_word(text: &str) -> &str {
    let text = text.trim_end_matches('_');
    let segment = &text[text.rfind('_').map_or(0, |at| at + 1)..];
    let bytes = segment.as_bytes();
    // Camel case: from the last capital that follows a small letter or digit.
    let mut start = 0;
    for at in 1..bytes.len() {
        if bytes[at].is_ascii_uppercase() && (bytes[at - 1].is_ascii_lowercase() || bytes[at - 1].is_ascii_digit()) {
            start = at;
        }
    }
    &segment[start..]
}

/// A name that carries a number in place of a meaning: `ColosseumFinal1`,
/// `EventScript_LeaveByWarp10`, `LOCALID_8`. Placeholders, hardware and convention
/// names (`BG2`, `Dma3`, `REG_DMA3`, `u16`, `Random16`, `FLAG_MAP_TEMP_01`)
/// are not.
pub fn is_numbered(name: &str) -> bool {
    let stem_end = name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if stem_end == name.len() || stem_end == 0 || is_placeholder(name) || regexes()[1].is_match(name) {
        return false;
    }
    let (before, digits) = name.split_at(stem_end);
    let separated = before.ends_with('_');
    let mut rest = before;
    let mut stem = last_word(rest);
    // Skip number segments: `Q` in `Q_8_8`.
    while !stem.is_empty() && stem.bytes().all(|b| b.is_ascii_digit()) {
        rest = rest.trim_end_matches('_');
        rest = &rest[..rest.len() - stem.len()];
        stem = last_word(rest);
    }
    let stem = stem.trim_end_matches(|c: char| c.is_ascii_digit());
    if stem.len() <= 1 || (!separated && matches!(digits, "8" | "16" | "32" | "64")) {
        return false;
    }
    let lower = stem.to_ascii_lowercase();
    // A width after a type stem: `vu16`, `bool8`.
    if matches!(lower.as_str(), "u" | "s" | "vu" | "vs" | "f" | "bool" | "int" | "uint" | "intptr") {
        return false;
    }
    !NUMBERED_STEMS.contains(&lower.as_str())
}

/// A number that wants a name: hex from 0x10, decimal from 10.
pub fn is_magic(literal: &str) -> bool {
    let digits = literal.trim_end_matches(['u', 'U', 'l', 'L']);
    if let Some(hex) = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).is_ok_and(|value| value >= 0x10);
    }
    digits.parse::<u64>().is_ok_and(|value| value >= 10)
}

/// A C integer literal's value, suffixes aside.
fn literal(text: &str) -> Option<i64> {
    let digits = text.trim_end_matches(['u', 'U', 'l', 'L']);
    match digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
        Some(hex) => i64::from_str_radix(hex, 16).ok(),
        None if digits.len() > 1 && digits.starts_with('0') => i64::from_str_radix(&digits[1..], 8).ok(),
        None => digits.parse().ok(),
    }
}

/// The value of an enumerator's or macro's tokens, when they are numbers and
/// known names joined by `+`, `-`, `|`, `*` and `<<` in parentheses or not;
/// anything else (a cast, a call) is unknown.
fn evaluate(words: &[&str], lookup: &dyn Fn(&str) -> Option<i64>) -> Option<i64> {
    fn sum(words: &[&str], at: &mut usize, lookup: &dyn Fn(&str) -> Option<i64>) -> Option<i64> {
        let mut value = product(words, at, lookup)?;
        while let Some(&op) = words.get(*at).filter(|w| matches!(**w, "+" | "-" | "|")) {
            *at += 1;
            let right = product(words, at, lookup)?;
            value = match op {
                "+" => value.checked_add(right)?,
                "-" => value.checked_sub(right)?,
                _ => value | right,
            };
        }
        Some(value)
    }
    fn product(words: &[&str], at: &mut usize, lookup: &dyn Fn(&str) -> Option<i64>) -> Option<i64> {
        let mut value = factor(words, at, lookup)?;
        while let Some(&op) = words.get(*at).filter(|w| matches!(**w, "*" | "<<")) {
            *at += 1;
            let right = factor(words, at, lookup)?;
            value = if op == "*" { value.checked_mul(right)? } else { value.checked_shl(u32::try_from(right).ok()?)? };
        }
        Some(value)
    }
    fn factor(words: &[&str], at: &mut usize, lookup: &dyn Fn(&str) -> Option<i64>) -> Option<i64> {
        let word = *words.get(*at)?;
        *at += 1;
        match word {
            "(" => {
                let value = sum(words, at, lookup)?;
                (words.get(*at) == Some(&")")).then(|| *at += 1)?;
                Some(value)
            }
            "-" => factor(words, at, lookup)?.checked_neg(),
            _ if word.starts_with(|c: char| c.is_ascii_digit()) => literal(word),
            _ => lookup(word),
        }
    }
    let mut at = 0;
    let value = sum(words, &mut at, lookup)?;
    (at == words.len()).then_some(value)
}

fn is_keyword(text: &str) -> bool {
    KEYWORDS.contains(&text)
}

/// A name written in capitals, as macros are: `UNUSED`, `ALIGNED`.
fn is_macro_name(text: &str) -> bool {
    text.bytes().any(|b| b.is_ascii_uppercase()) && !text.bytes().any(|b| b.is_ascii_lowercase())
}

/// One file's tokens with what the counts need around them.
struct Code<'a> {
    tokens: Vec<Token<'a>>,
    /// The comment right before each token is documentation.
    doc_before: Vec<bool>,
    /// A trailing `///<` note follows each token on its line.
    doc_after: Vec<bool>,
    /// The documentation comments right before each token, raw.
    doc_text: Vec<String>,
    /// The `///<` notes after each token, its continuation lines included, raw.
    trailing_text: Vec<String>,
}

impl<'a> Code<'a> {
    fn new(text: &'a str) -> (Code<'a>, Vec<Token<'a>>) {
        let all = clex::lex(text);
        let (mut tokens, mut comments, mut doc_before, mut doc_after) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let (mut doc_text, mut trailing_text) = (Vec::new(), Vec::<String>::new());
        let mut pending_doc = false;
        let mut pending_text = String::new();
        // The last comment was a `///<` note kept with the last token.
        let mut trailing_open = false;
        for token in all {
            if token.kind == Kind::Comment {
                if clex::is_trailing_doc(token.text) {
                    if let (Some(last), Some(flag), Some(note)) = (tokens.last(), doc_after.last_mut(), trailing_text.last_mut()) {
                        let last: &Token = last;
                        *flag |= last.line == token.line;
                        if last.line == token.line || trailing_open {
                            note.push_str(token.text);
                            note.push('\n');
                            trailing_open = true;
                        }
                    }
                    pending_doc = false;
                    pending_text.clear();
                } else {
                    let doc = clex::is_doc(token.text) && !token.pp;
                    if !doc || !pending_doc {
                        pending_text.clear();
                    }
                    if doc {
                        pending_text.push_str(token.text);
                        pending_text.push('\n');
                    }
                    pending_doc = doc;
                    trailing_open = false;
                }
                comments.push(token);
                continue;
            }
            doc_before.push(pending_doc);
            doc_after.push(false);
            doc_text.push(if pending_doc { pending_text.clone() } else { String::new() });
            trailing_text.push(String::new());
            trailing_open = false;
            if token.kind != Kind::PpEnd {
                pending_doc = false;
                pending_text.clear();
            }
            tokens.push(token);
        }
        (Code { tokens, doc_before, doc_after, doc_text, trailing_text }, comments)
    }

    /// The `///<` notes after the tokens from `start` to `end`.
    fn notes(&self, start: usize, end: usize) -> String {
        self.trailing_text[start..=end.min(self.tokens.len() - 1)].concat()
    }

    fn text(&self, at: usize) -> &'a str {
        self.tokens[at].text
    }
}

/// Where a `{` was opened and closed, and what it holds.
struct Opened {
    kind: Brace,
    close: usize,
}

/// The kinds of the open braces and where each opened.
type Snapshot = (Vec<Brace>, Vec<usize>);

struct Walk<'r, 'a> {
    report: &'r mut Report,
    file: &'a str,
    code: Code<'a>,
    stack: Vec<Brace>,
    /// For each `#if`: the open braces at it and at the end of its first branch.
    conditions: Vec<(Snapshot, Option<Snapshot>)>,
    /// `[` depth outside every brace: a declaration's array size.
    square: i32,
    statement: Vec<usize>,
    braces: HashMap<usize, Opened>,
    open: Vec<usize>,
    /// Each macro name: documented at any definition, and its first line.
    macros: BTreeMap<String, (bool, usize)>,
    /// The comment of each enum, by its opening brace.
    enum_docs: HashMap<usize, String>,
    /// Game-thing names this file has defined.
    defined: BTreeSet<String>,
    /// Each enum's tag and the value its next enumerator takes, by its brace.
    enum_next: HashMap<usize, (Option<String>, Option<i64>)>,
    /// This file's enumerators with known values.
    enum_values: HashMap<String, i64>,
    /// Each enum's enumerators, by its brace: how many, and the game things
    /// among them (their places in the report).
    enum_members: HashMap<usize, (usize, Vec<usize>)>,
    /// Where the directive being read starts.
    directive_start: usize,
    /// How many functions this file defines.
    functions: usize,
}

/// An enumerator's tag and value, where known.
type Enumerated = (Option<String>, Option<i64>);

impl<'r, 'a> Walk<'r, 'a> {
    /// Keep a name for the naming checks.
    fn named(&mut self, name: &str, kind: NameKind, line: usize, group: Option<String>) {
        let file = self.file.to_string();
        self.report.naming.names.push(naming::Name { name: name.to_string(), kind, file, line, group });
    }

    /// Keep a definition whose name may name a game thing, once per file.
    fn thing(&mut self, name: &str, origin: Origin, line: usize, own: String, context: String, enumerated: Enumerated) {
        let (tag, value) = enumerated;
        let definition = Definition { tag: tag.as_deref(), value, ..Definition::new(name, self.file, origin) };
        let Some(kind) = provenance::kind(definition, &self.report.macro_values) else { return };
        if !self.defined.insert(name.to_string()) {
            return;
        }
        let file = self.file.to_string();
        self.report.things.push(Thing { name: name.to_string(), kind, file, line, own, context, tag, value, alone: false });
    }

    /// An enumerator at `at`: its own comment and notes, up to its `,` or `}`.
    fn enumerator(&mut self, at: usize) {
        let mut depth = 0;
        let mut end = at;
        while end + 1 < self.code.tokens.len() {
            let text = self.code.text(end + 1);
            match text {
                "(" => depth += 1,
                ")" => depth -= 1,
                "," if depth == 0 => {
                    end += 1;
                    break;
                }
                "}" if depth == 0 => break,
                _ => {}
            }
            end += 1;
        }
        let own = self.code.doc_text[at].clone() + &self.code.notes(at, end);
        let context = self.open.last().and_then(|open| self.enum_docs.get(open)).cloned().unwrap_or_default();
        let token = &self.code.tokens[at];
        let (name, line) = (token.text, token.line);
        // Its value: what follows `=`, or one past the enumerator before it.
        let (tag, next) = self.open.last().and_then(|open| self.enum_next.get(open)).cloned().unwrap_or_default();
        let value = if self.code.tokens.get(at + 1).is_some_and(|t| t.text == "=") {
            let last = if self.code.text(end) == "," { end } else { end + 1 };
            let words: Vec<&str> = (at + 2..last).map(|t| self.code.text(t)).collect();
            let (values, macros) = (&self.enum_values, &self.report.macro_values);
            evaluate(&words, &|name| values.get(name).or_else(|| macros.get(name)).copied())
        } else {
            next
        };
        if let Some(value) = value {
            self.enum_values.insert(name.to_string(), value);
        }
        if let Some(&open) = self.open.last() {
            self.enum_next.insert(open, (tag.clone(), value.map(|value| value + 1)));
        }
        let group = self.open.last().map(|open| format!("{}#{open}", self.file));
        self.named(name, NameKind::Enumerator, line, group);
        let before = self.report.things.len();
        self.thing(name, Origin::Enumerator, line, own, context, (tag, value));
        if let Some(&open) = self.open.last() {
            let members = self.enum_members.entry(open).or_default();
            members.0 += 1;
            members.1.extend(before..self.report.things.len());
        }
    }

    fn name(&mut self, name: &str, line: usize) {
        if is_keyword(name) {
            return;
        }
        for (kind, test) in [("placeholder", is_placeholder as fn(&str) -> bool), ("numbered", is_numbered)] {
            if test(name) {
                self.report.note(kind, self.file, line, name);
                self.report.names.entry(kind).or_default().insert(name.to_string());
            }
        }
    }

    /// A struct or union member's name at `at`: an identifier its type
    /// precedes and `;`, `,`, `[` or `:` follows, or a pointer to functions
    /// (`void (*draw)(void);`).
    fn field(&mut self, at: usize, prev: Option<usize>, prev2: Option<usize>) {
        let text = |at: Option<usize>| at.map_or("", |at| self.code.text(at));
        let name = self.code.text(at);
        let after = self.code.tokens.get(at + 1).map_or("", |t| t.text);
        let member = matches!(after, ";" | "," | "[" | ":") && !matches!(text(prev), "struct" | "union" | "enum" | "{" | ";");
        let pointer = after == ")" && text(prev) == "*" && text(prev2) == "(";
        if is_keyword(name) || !(member || pointer) {
            return;
        }
        let Some(&open) = self.open.last() else { return };
        let tag = open.checked_sub(1).map(|before| &self.code.tokens[before]).filter(|t| t.kind == Kind::Ident && !is_keyword(t.text));
        let group = tag.map_or_else(|| format!("{}#{open}", self.file), |tag| tag.text.to_string());
        let line = self.code.tokens[at].line;
        self.named(name, NameKind::Field, line, Some(group));
    }

    /// A variable, parameter or member declared with a struct or union type
    /// (`struct Unit *unit`), for the names one type goes by.
    fn typed(&mut self, at: usize) {
        let tokens = &self.code.tokens;
        let Some(tag) = tokens.get(at + 1).filter(|t| t.kind == Kind::Ident && !is_keyword(t.text)) else { return };
        let mut next = at + 2;
        while tokens.get(next).is_some_and(|t| matches!(t.text, "*" | "const" | "volatile")) {
            next += 1;
        }
        let Some(name) = tokens.get(next).filter(|t| t.kind == Kind::Ident && !is_keyword(t.text)) else { return };
        if !tokens.get(next + 1).is_some_and(|t| matches!(t.text, "," | ")" | ";" | "=" | "[")) {
            return;
        }
        let member = self.stack.last() == Some(&Brace::Struct);
        let array = tokens.get(next + 1).is_some_and(|t| t.text == "[");
        // The declaration being read: a function's, from its declarator to its
        // closing brace, for its parameters and locals alike.
        let scope = self.statement.first().copied();
        let (tag, name, file, line) = (tag.text.to_string(), name.text.to_string(), self.file.to_string(), name.line);
        let typed = naming::Typed { tag, name, file, line, member, array, scope };
        self.report.naming.typed.push(typed);
    }

    /// `(w * h)` written with literals.
    fn is_size(&self, at: usize) -> bool {
        let text = |offset: isize| {
            let index = at as isize + offset;
            (index >= 0 && (index as usize) < self.code.tokens.len()).then(|| &self.code.tokens[index as usize])
        };
        let is = |offset: isize, value: &str| text(offset).is_some_and(|t| t.text == value);
        let number = |offset: isize| text(offset).is_some_and(|t| t.kind == Kind::Number);
        (is(-1, "(") && is(1, "*") && number(2) && is(3, ")")) || (is(-3, "(") && number(-2) && is(-1, "*") && is(1, ")"))
    }

    fn number(&mut self, at: usize, function_macro: Option<bool>) {
        if !is_magic(self.code.text(at)) || self.is_size(at) {
            return;
        }
        let metric = match function_macro {
            // An object-like macro names its number; other directives test editions.
            Some(false) => return,
            Some(true) => "magic_code",
            None => match self.stack.last() {
                Some(Brace::Init) => "magic_table",
                Some(Brace::Func | Brace::Block) => "magic_code",
                // Enum values, field layout and declarations outside functions.
                _ => return,
            },
        };
        let line = self.code.tokens[at].line;
        let literal = self.code.text(at);
        if metric == "magic_code" && self.is_argument(at) {
            // Whether it is data depends on the parameter it fills, which may be
            // declared in a file read later.
            let floor = if function_macro.is_some() { self.directive_start } else { 0 };
            // The calls around it, innermost first: `PIXELS` and then `ObjectEvent_SetPosition`.
            let mut calls = Vec::new();
            let mut inside = at;
            while let Some((callee, position)) = call_site(&self.code.tokens, inside, floor) {
                calls.push((self.code.text(callee).to_string(), position));
                inside = callee;
            }
            let (file, literal) = (self.file.to_string(), literal.to_string());
            self.report.arguments.push(Argument { file, line, literal, calls });
            return;
        }
        self.report.note(metric, self.file, line, literal);
    }

    /// A literal standing alone between `(` or `,` and `,` or `)`: an argument or list element.
    fn is_argument(&self, at: usize) -> bool {
        let before = at.checked_sub(1).map_or("", |at| self.code.text(at));
        let after = self.code.tokens.get(at + 1).map_or("", |t| t.text);
        matches!(before, "(" | ",") && matches!(after, ")" | ",")
    }

    fn directive(&mut self, start: usize, end: usize) {
        let tokens: Vec<Token<'a>> = self.code.tokens[start + 1..end].to_vec();
        let Some(word) = tokens.first().map(|t| t.text) else { return };
        let idents: Vec<&'a str> = tokens.iter().filter(|t| t.kind == Kind::Ident).map(|t| t.text).collect();
        let line = self.code.tokens[start].line;
        let condition = tokens.iter().map(|t| t.text).filter(|t| !t.starts_with("/")).collect::<Vec<_>>().join(" ");
        match word {
            "include" | "include_next" | "error" | "warning" | "pragma" | "line" => return,
            "if" | "ifdef" | "ifndef" | "elif" => {
                if word == "elif" {
                    self.branch();
                } else {
                    self.conditions.push(((self.stack.clone(), self.open.clone()), None));
                }
                let language = idents.contains(&"LANGUAGE");
                let patch = idents.iter().any(|i| i.contains("_PATCH_") && (i.starts_with("TBS_") || i.starts_with("TLA_")));
                // `edition.h` and `version.h` are where edition names become LANGUAGE and
                // the patch levels (S3).
                let bridge = self.file.ends_with("edition.h") || self.file.ends_with("version.h");
                let edition = !bridge && idents.iter().any(|i| regexes()[2].is_match(i));
                for (metric, hit) in [("if_language", language), ("if_patch", patch), ("if_edition_name", edition)] {
                    if hit {
                        self.report.note(metric, self.file, line, &condition);
                    }
                }
                if (language || patch || edition) && self.stack.contains(&Brace::Func) {
                    self.report.note("if_in_function", self.file, line, &condition);
                }
            }
            "else" => self.branch(),
            "endif" => {
                if let Some((_, Some((stack, open)))) = self.conditions.pop() {
                    self.stack = stack;
                    self.open = open;
                }
            }
            "define" => {
                let Some(name) = tokens.get(1).filter(|t| t.kind == Kind::Ident) else { return };
                let function_like = tokens.get(2).is_some_and(|t| t.text == "(" && t.glued);
                if function_like {
                    self.macro_parameters(name.text, start, end);
                }
                self.directive_start = start;
                for at in start + 3..end {
                    if self.code.tokens[at].kind == Kind::Number {
                        self.number(at, Some(function_like));
                    }
                }
                let guard = name.text.ends_with("_H") || name.text.ends_with("_H_");
                if !guard {
                    let documented = self.code.doc_before[start] || (start..=end).any(|at| self.code.doc_after[at]);
                    self.macros.entry(name.text.to_string()).or_insert((false, line)).0 |= documented;
                    let own = self.code.doc_text[start].clone() + &self.code.notes(start, end);
                    let mut value = None;
                    if !function_like {
                        let words: Vec<&str> = (start + 3..end).map(|t| self.code.text(t)).collect();
                        let macros = &self.report.macro_values;
                        value = evaluate(&words, &|name| macros.get(name).copied());
                        if let Some(value) = value {
                            self.report.macro_values.insert(name.text.to_string(), value);
                        }
                    }
                    let kind = if function_like { NameKind::FunctionMacro } else { NameKind::Macro };
                    self.named(name.text, kind, line, None);
                    let origin = if function_like { Origin::FunctionMacro } else { Origin::Macro };
                    self.thing(name.text, origin, line, own, String::new(), (None, value));
                }
            }
            _ => {}
        }
        for ident in idents.iter().skip(1) {
            self.name(ident, line);
        }
    }

    /// A function-like macro's parameters, each with the call in its body it
    /// is passed to: `WAIT(n)`'s `n` is `FieldScript_WaitFrames`'s first argument.
    fn macro_parameters(&mut self, name: &str, start: usize, end: usize) {
        // `#`, `define`, the name, `(`, then the parameters up to `)`.
        let mut names = Vec::new();
        let mut at = start + 4;
        while at < end && self.code.text(at) != ")" {
            if self.code.tokens[at].kind == Kind::Ident {
                names.push(self.code.text(at));
            }
            at += 1;
        }
        let body = at + 1;
        let parameters = names
            .iter()
            .map(|&parameter| {
                let call = (body..end)
                    .filter(|&use_at| self.code.text(use_at) == parameter)
                    .find_map(|use_at| call_site(&self.code.tokens, use_at, body))
                    .map(|(callee, position)| (self.code.text(callee).to_string(), position));
                (parameter.to_string(), call)
            })
            .collect();
        self.report.macro_parameters.entry(name.to_string()).or_insert(parameters);
    }

    /// The parameter names of the function a declaration names, `None` where
    /// a parameter is unnamed: `void Map_CopyCells(s32 srcX, ...)`.
    fn parameters(&mut self, name: &str, statement: &[usize]) {
        let Some(open) = statement.windows(2).find(|pair| self.code.text(pair[0]) == name && self.code.text(pair[1]) == "(")
        else {
            return;
        };
        let open = statement.iter().position(|&at| at == open[1]).expect("its own token");
        let (mut names, mut current, mut depth) = (Vec::new(), Vec::new(), 0);
        for &at in &statement[open + 1..] {
            let text = self.code.text(at);
            match text {
                "(" => depth += 1,
                ")" if depth == 0 => break,
                ")" => depth -= 1,
                "," if depth == 0 => {
                    names.push(self.parameter_name(&current));
                    current.clear();
                    continue;
                }
                _ => {}
            }
            current.push(at);
        }
        names.push(self.parameter_name(&current));
        if names.iter().all(Option::is_none) {
            return;
        }
        let named = |list: &[Option<String>]| list.iter().filter(|n| n.is_some()).count();
        let entry = self.report.parameters.entry(name.to_string()).or_default();
        if named(&names) > named(entry) {
            *entry = names;
        }
    }

    /// The name one parameter's tokens end in, when they name it: not `s32`,
    /// `struct ObjectEvent *` or a pointer to functions.
    fn parameter_name(&self, tokens: &[usize]) -> Option<String> {
        let (&last, rest) = tokens.split_last()?;
        let token = &self.code.tokens[last];
        let before = rest.last().map_or("", |&at| self.code.text(at));
        let typed = rest.iter().any(|&at| self.code.tokens[at].kind == Kind::Ident);
        (token.kind == Kind::Ident && !is_keyword(token.text) && typed && !matches!(before, "struct" | "union" | "enum"))
            .then(|| token.text.to_string())
            .filter(|_| !rest.iter().any(|&at| self.code.text(at) == "("))
    }

    fn branch(&mut self) {
        if let Some(((stack, open), first)) = self.conditions.last_mut() {
            first.get_or_insert_with(|| (self.stack.clone(), self.open.clone()));
            self.stack = stack.clone();
            self.open = open.clone();
        }
    }

    fn brace_kind(&self, prev: Option<usize>, prev2: Option<usize>) -> Brace {
        let text = |at: Option<usize>| at.map_or("", |at| self.code.text(at));
        let ident = |at: Option<usize>| at.is_some_and(|at| self.code.tokens[at].kind == Kind::Ident);
        let tagged = |words: &[&str]| words.contains(&text(prev)) || (ident(prev) && words.contains(&text(prev2)));
        match self.stack.last() {
            Some(Brace::Init) => Brace::Init,
            _ if text(prev) == "=" => Brace::Init,
            _ if tagged(&["enum"]) => Brace::Enum,
            _ if tagged(&["struct", "union"]) => Brace::Struct,
            None if text(prev) == ")" => Brace::Func,
            _ => Brace::Block,
        }
    }

    fn walk(&mut self) {
        let (mut prev, mut prev2) = (None, None);
        let mut at = 0;
        while at < self.code.tokens.len() {
            let token = self.code.tokens[at].clone();
            if token.kind == Kind::PpStart {
                let end = (at..self.code.tokens.len()).find(|&e| self.code.tokens[e].kind == Kind::PpEnd).unwrap_or(self.code.tokens.len() - 1);
                self.directive(at, end);
                at = end + 1;
                continue;
            }
            if self.stack.is_empty() {
                self.statement.push(at);
            }
            match (token.kind, token.text) {
                (Kind::Ident, "goto") => self.report.note("goto", self.file, token.line, "goto"),
                (Kind::Ident, name) => {
                    self.name(name, token.line);
                    let after = self.code.tokens.get(at + 1).map_or("", |t| t.text);
                    if self.stack.last() == Some(&Brace::Enum)
                        && matches!(prev.map(|p| self.code.text(p)), Some("{" | ","))
                        && matches!(after, "=" | "," | "}")
                    {
                        self.enumerator(at);
                    }
                    if self.stack.last() == Some(&Brace::Struct) {
                        self.field(at, prev, prev2);
                    }
                    if matches!(name, "struct" | "union") {
                        self.typed(at);
                    }
                }
                (Kind::Number, _) if self.stack.is_empty() && self.square > 0 => {}
                (Kind::Number, _) => self.number(at, None),
                (_, "[") if self.stack.is_empty() => self.square += 1,
                (_, "]") if self.stack.is_empty() => self.square -= 1,
                (_, "{") => {
                    let kind = self.brace_kind(prev, prev2);
                    if kind == Brace::Enum && self.stack.is_empty() {
                        if let Some(&first) = self.statement.first() {
                            self.enum_docs.insert(at, self.code.doc_text[first].clone());
                        }
                        // `enum Tag {`.
                        let tag = prev2
                            .filter(|&p| self.code.text(p) == "enum")
                            .and(prev)
                            .filter(|&p| self.code.tokens[p].kind == Kind::Ident)
                            .map(|p| self.code.text(p).to_string());
                        self.enum_next.insert(at, (tag, Some(0)));
                    }
                    self.stack.push(kind);
                    self.open.push(at);
                    self.braces.insert(at, Opened { kind, close: at });
                }
                (_, "}") => {
                    self.stack.pop();
                    if let Some(open) = self.open.pop() {
                        if let Some(opened) = self.braces.get_mut(&open) {
                            opened.close = at;
                        }
                        if self.stack.is_empty() {
                            self.statement.push(at);
                            if self.braces[&open].kind == Brace::Func {
                                self.functions += 1;
                                self.declaration();
                            }
                        }
                    }
                }
                (_, ";") if self.stack.is_empty() => self.declaration(),
                _ => {}
            }
            prev2 = prev;
            prev = Some(at);
            at += 1;
        }
        if !self.statement.is_empty() {
            self.declaration();
        }
        let file = self.file;
        for (name, (documented, line)) in std::mem::take(&mut self.macros) {
            if !documented {
                self.report.note("undocumented_macro", file, line, &name);
            }
        }
    }

    /// Normalized tokens between a brace pair, for the duplicate search.
    fn body(&self, open: usize, keep_names: bool) -> (String, usize) {
        let close = self.braces[&open].close;
        let mut key = String::new();
        let mut count = 0;
        for token in &self.code.tokens[open + 1..close] {
            if token.pp {
                continue;
            }
            let text = match token.kind {
                Kind::Ident if !keep_names && !is_keyword(token.text) => "$",
                Kind::Str if !keep_names => "\"",
                _ => token.text,
            };
            key.push_str(text);
            key.push('\u{1f}');
            count += 1;
        }
        (key, count)
    }

    fn duplicate(&mut self, function: bool, open: usize, name: &str) {
        let (key, tokens) = self.body(open, !function);
        if tokens < if function { FUNCTION_TOKENS } else { TABLE_TOKENS } {
            return;
        }
        let bodies = if function { &mut self.report.function_bodies } else { &mut self.report.table_bodies };
        let group = bodies.entry(key).or_insert_with(|| Group { tokens, members: Vec::new() });
        group.members.push(format!("{}:{name}", self.file));
    }

    /// One statement outside every brace: a function, prototype, global,
    /// struct, enum or typedef.
    fn declaration(&mut self) {
        let statement = std::mem::take(&mut self.statement);
        let (Some(&first), Some(&last)) = (statement.first(), statement.last()) else { return };
        let documented = self.code.doc_before[first] || self.code.doc_after[last];
        let opened: Vec<(usize, Brace)> =
            statement.iter().filter_map(|&at| self.braces.get(&at).map(|opened| (at, opened.kind))).collect();
        let file = self.file;
        let lead = self.code.text(first);
        let second_last = statement.len().checked_sub(2).map_or("", |at| self.code.text(statement[at]));
        if matches!(lead, "asm" | "__asm__") {
            return;
        }
        let line = self.code.tokens[first].line;
        self.tags(&opened, if lead == "typedef" { second_last } else { "" }, line);
        if lead == "typedef" {
            self.types(&opened, second_last, line, documented);
            return;
        }
        if let Some(&(open, _)) = opened.iter().find(|(_, kind)| *kind == Brace::Func) {
            if let Some(Declarator::Function(name)) = self.declarator(&statement) {
                let name = name.to_string();
                self.parameters(&name, &statement);
                self.report.declare(true, &name, file, line, documented, true);
                self.named(&name, NameKind::Function, line, None);
                self.duplicate(true, open, &name);
                let own = self.code.doc_text[first].clone();
                self.thing(&name, Origin::Function, line, own, String::new(), (None, None));
            }
            return;
        }
        self.types(&opened, "", line, documented);
        // A bare definition (`struct S { ... };`) or forward declaration (`struct S;`).
        let after_body = opened.iter().find(|(_, kind)| matches!(kind, Brace::Struct | Brace::Enum)).and_then(|(open, _)| {
            let close = self.braces[open].close;
            statement.iter().position(|&at| at == close).and_then(|at| statement.get(at + 1)).map(|&at| self.code.text(at))
        });
        if matches!(lead, "struct" | "union" | "enum") && (matches!(after_body, Some(";" | "__attribute__")) || statement.len() == 3) {
            return;
        }
        let defined = lead != "extern";
        match self.declarator(&statement) {
            Some(Declarator::Function(name)) => {
                let name = name.to_string();
                self.parameters(&name, &statement);
                self.report.declare(true, &name, file, line, documented, false);
                self.named(&name, NameKind::Prototype, line, None);
            }
            Some(Declarator::Global(name)) => {
                let name = name.to_string();
                self.report.declare(false, &name, file, line, documented, defined);
                self.named(&name, if defined { NameKind::Global } else { NameKind::Extern }, line, None);
                // A mechanism's table counts where it is defined; a game thing
                // (`extern char MapId_Vale;`) where it is declared.
                if defined || provenance::kind(Definition::new(&name, file, Origin::Global), &self.report.macro_values) != Some("global") {
                    let own = self.code.doc_text[first].clone() + &self.code.notes(last, last);
                    self.thing(&name, Origin::Global, line, own, String::new(), (None, None));
                }
                if let Some(&(open, _)) = opened.iter().find(|(_, kind)| *kind == Brace::Init) {
                    self.duplicate(false, open, &name);
                }
            }
            None => {}
        }
    }

    /// The structs and enums a statement defines, named by their tag or else
    /// their typedef name.
    fn types(&mut self, opened: &[(usize, Brace)], typedef: &str, line: usize, documented: bool) {
        if documented {
            return;
        }
        for &(open, kind) in opened {
            let metric = match kind {
                Brace::Struct => "undocumented_struct",
                Brace::Enum => "undocumented_enum",
                _ => continue,
            };
            let tag = open.checked_sub(1).map(|at| &self.code.tokens[at]).filter(|t| t.kind == Kind::Ident && !is_keyword(t.text));
            // An anonymous enum of constants is documented by its first constant's note.
            if kind == Brace::Enum && tag.is_none() && typedef.is_empty() && self.code.doc_before.get(open + 1) == Some(&true) {
                continue;
            }
            let name = tag.map_or(if typedef.is_empty() { "(anonymous)" } else { typedef }, |t| t.text);
            self.report.note(metric, self.file, line, name);
        }
    }

    /// The struct, union and enum tags a statement defines, and the name a
    /// typedef gives.
    fn tags(&mut self, opened: &[(usize, Brace)], typedef: &str, line: usize) {
        for &(open, kind) in opened {
            if !matches!(kind, Brace::Struct | Brace::Enum) {
                continue;
            }
            let tag = open.checked_sub(1).map(|at| &self.code.tokens[at]).filter(|t| t.kind == Kind::Ident && !is_keyword(t.text));
            if let Some(tag) = tag {
                let (name, line) = (tag.text, tag.line);
                self.named(name, NameKind::Tag, line, None);
            }
        }
        if !typedef.is_empty() && !is_keyword(typedef) && typedef.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            self.named(typedef, NameKind::Typedef, line, None);
        }
    }

    /// The name a statement declares, read from its tokens outside braces.
    fn declarator(&self, statement: &[usize]) -> Option<Declarator<'a>> {
        let tokens: Vec<&Token<'a>> = statement.iter().map(|&at| &self.code.tokens[at]).collect();
        let mut depth = 0;
        let mut last_name = None;
        let mut index = 0;
        while index < tokens.len() {
            let token = tokens[index];
            match token.text {
                "(" if depth == 0 => {
                    let before = index.checked_sub(1).map(|at| tokens[at]);
                    let after = tokens.get(index + 1).map(|t| t.text);
                    if after == Some("*") {
                        // `void (*gHandler)(void)`: a pointer to functions.
                        return tokens[index + 1..]
                            .iter()
                            .find(|t| t.kind == Kind::Ident && !is_keyword(t.text))
                            .map(|t| Declarator::Global(t.text));
                    }
                    match before {
                        Some(name) if name.kind == Kind::Ident && !is_keyword(name.text) && !is_macro_name(name.text) => {
                            return Some(Declarator::Function(name.text));
                        }
                        _ => depth += 1,
                    }
                }
                "(" => depth += 1,
                ")" => depth -= 1,
                "[" | "=" | ";" | "," | ":" | "{" if depth == 0 => break,
                // A capitalized name is a macro only when it is invoked: `ALIGNED(4)`.
                _ if depth == 0
                    && token.kind == Kind::Ident
                    && !is_keyword(token.text)
                    && !(is_macro_name(token.text) && tokens.get(index + 1).is_some_and(|t| t.text == "(")) =>
                {
                    last_name = Some(token.text);
                }
                _ => {}
            }
            index += 1;
        }
        last_name.map(Declarator::Global)
    }
}

enum Declarator<'a> {
    Function(&'a str),
    Global(&'a str),
}

/// Count one file into its game's report.
/// A header written in assembly (`ed_asm.h`) is not counted.
pub fn file(report: &mut Report, path: &str, text: &str) {
    let (code, comments) = Code::new(text);
    if code.tokens.first().is_some_and(|t| t.text == ".") {
        return;
    }
    report.files += 1;
    let lines = text.lines().count();
    report.lines += lines;
    // S3 splits only an object holding more than one family: never a header, a
    // map overlay (laid out by kind, so one file), a part (one family
    // already) or a data object (one file).
    let object = path.ends_with(".c") && !in_map_folder(path) && !comments.iter().any(|c| c.text.starts_with("// Part of "));
    let long = object && lines > LONG_FILE;
    static TAG: OnceLock<Regex> = OnceLock::new();
    let tag = TAG.get_or_init(|| Regex::new(r"FAKEMATCH\(([a-z-]+)\)").unwrap());
    if in_map_folder(path) {
        if let Some(opening) = comments.iter().find(|c| clex::is_doc(c.text) && c.text.contains("@file")) {
            // The whole `///` block that holds `@file`.
            let block: Vec<&str> = comments
                .iter()
                .skip_while(|c| c.line < opening.line)
                .scan(opening.line, |next, c| {
                    (c.line == *next && clex::is_doc(c.text)).then(|| {
                        *next += 1;
                        c.text
                    })
                })
                .collect();
            report.places.push((path.to_string(), block.join("\n")));
        }
    }
    for comment in &comments {
        for capture in tag.captures_iter(comment.text) {
            if DEVICES.contains(&&capture[1]) {
                report.note(&format!("fakematch_{}", &capture[1]), path, comment.line, &capture[1]);
            }
        }
    }
    let mut walk = Walk {
        report,
        file: path,
        code,
        stack: Vec::new(),
        conditions: Vec::new(),
        square: 0,
        statement: Vec::new(),
        braces: HashMap::new(),
        open: Vec::new(),
        macros: BTreeMap::new(),
        enum_docs: HashMap::new(),
        defined: BTreeSet::new(),
        enum_next: HashMap::new(),
        enum_values: HashMap::new(),
        enum_members: HashMap::new(),
        directive_start: 0,
        functions: 0,
    };
    walk.walk();
    // An enum of one enumerator documents it: its comment is that name's own.
    for (count, things) in walk.enum_members.values() {
        if *count == 1 {
            for &at in things {
                walk.report.things[at].alone = true;
            }
        }
    }
    if long && walk.functions > 0 {
        walk.report.long_files.push((path.to_string(), lines));
    }
}

/// Count a game from `(path, text)` pairs.
#[cfg(test)]
pub fn game<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> Report {
    game_with_text(files, None)
}

/// Count a game, and with its English text check its names' provenance.
pub fn game_with_text<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>, text: Option<&GameText>) -> Report {
    let mut report = Report::default();
    for (path, text) in files {
        if path.ends_with("/README.md") {
            unit(&mut report, path, text);
            continue;
        }
        file(&mut report, path, text);
        if is_map_overlay(path) {
            unit(&mut report, path, text);
        }
        if naming::is_pret_flash(path) {
            report.naming.pret.extend(naming::identifiers(text).map(str::to_string));
        }
    }
    report.finish();
    let text = text.filter(|text| !text.is_empty());
    if let Some(text) = text {
        report.check_provenance(text);
    }
    let mut findings = naming::check(&report.naming, text);
    findings.sort_by(|a, b| a.metric.cmp(b.metric).then(a.file.cmp(&b.file)).then(a.line.cmp(&b.line)).then(a.what.cmp(&b.what)));
    for finding in findings {
        report.note(finding.metric, &finding.file, finding.line, &finding.what);
    }
    report
}

impl Report {
    fn check_provenance(&mut self, text: &GameText) {
        // A map id is declared once for the game, wherever it is repeated.
        let mut maps = BTreeSet::new();
        self.things.retain(|thing| thing.kind != "map" || maps.insert(thing.name.clone()));
        let analysis = provenance::analyze(&self.things, &self.places, text, &self.macro_values);
        for row in &analysis.rows {
            let (file, line, name) = (&row.thing.file, row.thing.line, &row.thing.name);
            if let Some(evidence) = &row.evidence {
                self.note("provenance_names", file, line, name);
                match evidence {
                    provenance::Evidence::Mismatch { game, .. } => {
                        self.note(evidence.metric(), file, line, &format!("{name}\t{game}"))
                    }
                    _ => self.note(evidence.metric(), file, line, name),
                }
            }
            if !row.words.is_empty() {
                self.note("unused_word_name", file, line, name);
            }
            if !row.door_words.is_empty() {
                self.note("door_word_link", file, line, &format!("{name}\t{}", row.door_words.join(",")));
            }
        }
        self.provenance = Some(analysis);
    }
}

/// How many area names `gMapSec_NameRules` reads: its highest name index
/// plus one, from `map_rules.s`.
pub fn area_count(rules: &str) -> Option<usize> {
    rules
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("map_name ") || line.starts_with("group_name "))
        .filter_map(|line| line.split(',').nth(2)?.split('@').next()?.trim().parse::<usize>().ok())
        .max()
        .map(|max| max + 1)
}

/// A game's English text, if git tracks one.
fn game_text(root: &Path, game: &str) -> Option<GameText> {
    let path = format!("games/{game}/text/en.po");
    let tracked = std::process::Command::new("git").arg("-C").arg(root).args(["ls-files", "--"]).arg(&path).output().ok()?;
    if tracked.stdout.is_empty() {
        return None;
    }
    let po = std::fs::read(root.join(&path)).ok()?;
    let rules = std::fs::read(root.join(format!("games/{game}/src/overworld/map_rules.s"))).ok();
    let areas = rules.and_then(|rules| area_count(&String::from_utf8_lossy(&rules)));
    Some(GameText::parse(&String::from_utf8_lossy(&po), areas))
}

/// A place's overlay, `games/<game>/src/map/<place>.c` (⚓️, not yet
/// renamed, keeps its overlays in `src/scene`).
fn is_map_overlay(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    parts.len() == 5 && matches!(parts[3], "map" | "scene") && path.ends_with(".c")
}

/// Under a game's map overlays, `src/map` (⚓️'s `src/scene`).
fn in_map_folder(path: &str) -> bool {
    path.contains("/src/map/") || path.contains("/src/scene/")
}

/// A folder guide, `games/<game>/src/<folder>/README.md`.
fn is_folder_guide(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    parts.len() == 5 && parts[2] == "src" && parts[4] == "README.md"
}

/// Notes a unit's open questions (H4): the `- XXX:` items of a map
/// overlay's `Open questions:` list or of a folder guide's `## Evidence and
/// open questions` section. `- XXX (no evidence):` items leave a unit done.
fn unit(report: &mut Report, path: &str, text: &str) {
    let guide = path.ends_with("/README.md");
    let (mut on, mut listed, mut open) = (false, false, 0);
    for (i, line) in text.lines().enumerate() {
        let item = if guide {
            if let Some(heading) = line.strip_prefix("## ") {
                on = heading == "Evidence and open questions";
                listed |= on;
                continue;
            }
            line.strip_prefix("- ")
        } else {
            if line == "/// Open questions:" {
                (on, listed) = (true, true);
                continue;
            }
            // A place whose last question was answered keeps the line, so it
            // counts as done rather than never started.
            if line == "/// Open questions: none." {
                (on, listed) = (false, true);
                continue;
            }
            if !line.starts_with("///") {
                on = false;
            }
            line.strip_prefix("/// - ")
        };
        if let Some(item) = item.filter(|item| on && item.starts_with("XXX:")) {
            open += 1;
            report.note("unit_open", path, i + 1, item);
        }
    }
    report.units.push(Unit { file: path.to_string(), open, listed });
}

/// The tracked C, headers and folder guides of a game, never build output.
fn sources(root: &Path, game: &str) -> Result<Vec<(String, String)>, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--"])
        .arg(format!("games/{game}/src"))
        .arg(format!("games/{game}/include"))
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err(format!("git ls-files failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let mut files = Vec::new();
    for path in String::from_utf8_lossy(&output.stdout).split('\0') {
        if path.ends_with(".c") || path.ends_with(".h") || is_folder_guide(path) {
            let bytes = std::fs::read(root.join(path)).map_err(|error| format!("cannot read {path}: {error}"))?;
            files.push((path.to_string(), String::from_utf8_lossy(&bytes).into_owned()));
        }
    }
    Ok(files)
}

/// The rows of the summary, as `(metric, label)`; a metric's TSV total is its value. A row
/// with no metric is a heading.
const ROWS: &[(&str, &str)] = &[
    ("magic_code", "magic numbers in code"),
    ("magic_argument", "  of those, lone arguments"),
    ("data_argument", "numbers passed as data"),
    ("data_argument_frames", "  frames"),
    ("data_argument_coordinate", "  coordinates"),
    ("data_argument_size", "  sizes"),
    ("data_argument_count", "  counts"),
    ("magic_table", "numbers in table data"),
    ("placeholder_names", "placeholder names"),
    ("placeholder", "  their uses"),
    ("numbered_names", "numbered names"),
    ("numbered", "  their uses"),
    ("fakematch", "FAKEMATCH devices"),
    ("fakematch_wrapper", "  wrapper"),
    ("fakematch_volatile", "  volatile"),
    ("fakematch_temporary", "  temporary"),
    ("fakematch_dead", "  dead"),
    ("fakematch_one-pass", "  one-pass"),
    ("fakematch_barrier", "  barrier"),
    ("fakematch_pin", "  pin"),
    ("fakematch_signature", "  signature"),
    ("goto", "goto"),
    ("if_language", "#if on LANGUAGE"),
    ("if_patch", "#if on a patch level"),
    ("if_edition_name", "#if on an edition name"),
    ("if_in_function", "  of those inside functions"),
    ("undocumented_entry", "far-call entries without ///"),
    ("long_file", "objects over 3,000 lines"),
    ("units", "units (places and folder guides)"),
    ("units_done", "  done under H4"),
    ("unit_open", "  open questions in them"),
    ("duplicate_function", "duplicate function groups"),
    ("duplicate_function_copies", "  copies past the first"),
    ("duplicate_table", "duplicate table groups"),
    ("duplicate_table_copies", "  copies past the first"),
    ("provenance_names", "game-thing names"),
    ("provenance_verified", "  verified by a ★ line"),
    ("provenance_table", "  verified by table"),
    ("provenance_rooms", "  verified by rooms"),
    ("provenance_table_mismatch", "  table name differs"),
    ("provenance_unstarred", "  evidence line without its ★"),
    ("provenance_failed", "  evidence line failing check"),
    ("provenance_prose", "  evidence in prose only"),
    ("provenance_none", "  no evidence"),
    ("unused_words", "words the game never uses"),
    ("unused_word_name", "  names using them"),
    ("door_words", "door words unresolved"),
    ("door_word_link", "  links using them"),
    ("", "names to guess (one word per idea):"),
    ("synonym", "  a second word for one idea"),
    ("synonym_referent", "  one type, two variable names"),
    ("abbreviation", "  shortened, spelled elsewhere"),
    ("family_shape", "  off their family's pattern"),
    ("prefix_twin", "  one idea under two prefixes"),
    ("", "to review, not to fill (D9):"),
    ("undocumented_function", "  functions without ///"),
    ("undocumented_struct", "  structs without ///"),
    ("undocumented_enum", "  enums without ///"),
    ("undocumented_macro", "  macros without ///"),
    ("undocumented_global", "  globals without ///"),
];

/// Metrics whose top files are listed.
const LISTED: &[&str] = &[
    "magic_code", "magic_argument", "magic_table", "placeholder", "numbered", "fakematch", "goto", "if_language", "if_patch",
    "if_in_function", "undocumented_entry", "undocumented_function", "undocumented_struct", "undocumented_enum", "undocumented_macro",
    "undocumented_global", "provenance_none", "provenance_prose", "provenance_table_mismatch", "unused_word_name",
    "door_word_link", "unit_open", "synonym", "synonym_referent", "abbreviation", "family_shape", "prefix_twin",
];

impl Report {
    pub fn value(&self, metric: &str) -> u64 {
        match metric {
            "placeholder_names" => self.distinct("placeholder") as u64,
            "numbered_names" => self.distinct("numbered") as u64,
            "fakematch" => DEVICES.iter().map(|kind| self.total(&format!("fakematch_{kind}"))).sum(),
            "long_file" => self.long_files.len() as u64,
            "units" => self.units.len() as u64,
            "units_done" => self.units.iter().filter(|unit| unit.listed && unit.open == 0).count() as u64,
            "duplicate_function" => self.function_groups.len() as u64,
            "duplicate_function_copies" => Report::copies(&self.function_groups) as u64,
            "duplicate_table" => self.table_groups.len() as u64,
            "duplicate_table_copies" => Report::copies(&self.table_groups) as u64,
            "unused_words" => self.provenance.as_ref().map_or(0, |analysis| analysis.words.len() as u64),
            "door_words" => self.provenance.as_ref().map_or(0, |analysis| analysis.door_words.len() as u64),
            _ => self.total(metric),
        }
    }

    /// FAKEMATCH devices summed per file.
    fn fakematch_files(&self) -> BTreeMap<String, u64> {
        let mut files = BTreeMap::new();
        for kind in DEVICES {
            for (file, count) in self.tallies.get(&format!("fakematch_{kind}")).into_iter().flatten() {
                *files.entry(file.clone()).or_default() += count;
            }
        }
        files
    }

    fn top_of(&self, metric: &str, count: usize) -> Vec<(String, u64)> {
        if metric == "fakematch" {
            let mut files: Vec<(String, u64)> = self.fakematch_files().into_iter().collect();
            files.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            files.truncate(count);
            return files;
        }
        self.top(metric, count).into_iter().map(|(file, n)| (file.to_string(), n)).collect()
    }
}

/// The TSV: each metric's total, then its top files and groups.
pub fn tsv(games: &[(&str, &Report)]) -> String {
    let mut text = String::from("game\tmetric\tscope\tcount\n");
    for (game, report) in games {
        let _ = writeln!(text, "{game}\tfiles\ttotal\t{}", report.files);
        let _ = writeln!(text, "{game}\tlines\ttotal\t{}", report.lines);
        for (metric, _) in ROWS.iter().filter(|(metric, _)| !metric.is_empty()) {
            let _ = writeln!(text, "{game}\t{metric}\ttotal\t{}", report.value(metric));
        }
        for metric in LISTED {
            for (file, count) in report.top_of(metric, TOP) {
                let _ = writeln!(text, "{game}\t{metric}\t{file}\t{count}");
            }
        }
        for (file, lines) in report.long_files.iter().take(TOP) {
            let _ = writeln!(text, "{game}\tlong_file\t{file}\t{lines}");
        }
        for (metric, groups) in [("duplicate_function", &report.function_groups), ("duplicate_table", &report.table_groups)] {
            for group in groups.iter().take(TOP) {
                let _ = writeln!(text, "{game}\t{metric}\t{}\t{}", group.members.join(" "), group.tokens);
            }
        }
        for (word, rows) in report.provenance.iter().flat_map(|analysis| analysis.words.iter()).take(TOP) {
            let _ = writeln!(text, "{game}\tunused_word\t{word}\t{}", rows.len());
        }
        for (word, rows) in report.provenance.iter().flat_map(|analysis| analysis.door_words.iter()).take(TOP) {
            let _ = writeln!(text, "{game}\tdoor_word\t{word}\t{}", rows.len());
        }
    }
    text
}

/// The short table `make progress` prints, and with `top` each metric's
/// leading files.
pub fn summary(games: &[(&str, &Report)], top: usize) -> String {
    let mut text = String::from("Humanization");
    for (game, _) in games {
        let _ = write!(text, "{:>10}", game);
    }
    text.push('\n');
    let _ = write!(text, "  {:<30}", "files (lines)");
    for (_, report) in games {
        let _ = write!(text, "{:>10}", super::commas(report.files as i64));
    }
    text.push('\n');
    let _ = write!(text, "  {:<30}", "");
    for (_, report) in games {
        let _ = write!(text, "{:>10}", format!("({})", super::commas(report.lines as i64)));
    }
    text.push('\n');
    for (metric, label) in ROWS {
        if metric.is_empty() {
            let _ = writeln!(text, "  {label}");
            continue;
        }
        let _ = write!(text, "  {label:<30}");
        for (_, report) in games {
            let _ = write!(text, "{:>10}", super::commas(report.value(metric) as i64));
        }
        text.push('\n');
    }
    if top > 0 {
        for (game, report) in games {
            for metric in LISTED {
                let files = report.top_of(metric, top);
                if files.is_empty() {
                    continue;
                }
                let _ = writeln!(text, "{game} {metric}:");
                for (file, count) in files {
                    let _ = writeln!(text, "  {:>6}  {file}", super::commas(count as i64));
                }
            }
            if !report.long_files.is_empty() {
                let _ = writeln!(text, "{game} long_file:");
                for (file, lines) in report.long_files.iter().take(top) {
                    let _ = writeln!(text, "  {:>6}  {file}", super::commas(*lines as i64));
                }
            }
            for (metric, groups) in [("duplicate_function", &report.function_groups), ("duplicate_table", &report.table_groups)] {
                if groups.is_empty() {
                    continue;
                }
                let _ = writeln!(text, "{game} {metric} (tokens, copies):");
                for group in groups.iter().take(top) {
                    let more = group.members.len().saturating_sub(3);
                    let shown = group.members[..group.members.len() - more].join(" ");
                    let more = if more > 0 { format!(" and {more} more") } else { String::new() };
                    let _ = writeln!(text, "  {:>6} x{}  {shown}{more}", group.tokens, group.members.len());
                }
            }
            if let Some(analysis) = report.provenance.as_ref().filter(|analysis| !analysis.words.is_empty()) {
                let _ = writeln!(text, "{game} unused_word (names, first):");
                for (word, rows) in analysis.words.iter().take(top) {
                    let _ = writeln!(text, "  {:>6}  {word}  {}", rows.len(), analysis.rows[rows[0]].thing.name);
                }
            }
            if let Some(analysis) = report.provenance.as_ref().filter(|analysis| !analysis.door_words.is_empty()) {
                let _ = writeln!(text, "{game} door_word (links, first):");
                for (word, rows) in analysis.door_words.iter().take(top) {
                    let _ = writeln!(text, "  {:>6}  {word}  {}", rows.len(), analysis.rows[rows[0]].thing.name);
                }
            }
        }
    }
    text
}

/// Every counted thing of one metric (`fakematch` takes each device kind),
/// one line each: game, `file:line`, what.
pub fn list(games: &[(&str, &Report)], metric: &str) -> String {
    let mut text = String::new();
    for (game, report) in games {
        if metric == "provenance" || metric == "unused_word" || metric == "door_word" {
            let Some(analysis) = &report.provenance else { continue };
            if metric != "provenance" {
                // Each flagged or unresolved word, how many names use it and where.
                let words = if metric == "unused_word" { &analysis.words } else { &analysis.door_words };
                for (word, rows) in words {
                    let uses: Vec<String> = rows
                        .iter()
                        .map(|&at| {
                            let thing = &analysis.rows[at].thing;
                            format!("{} {}:{}", thing.name, thing.file, thing.line)
                        })
                        .collect();
                    let _ = writeln!(text, "{game}\t{word}\t{}\t{}", rows.len(), uses.join("; "));
                }
                continue;
            }
            // The worklist: game things, and mechanisms whose words the game never uses.
            for row in &analysis.rows {
                if row.evidence.is_none() && row.words.is_empty() {
                    continue;
                }
                let thing = &row.thing;
                let words = if row.words.is_empty() { "-".to_string() } else { row.words.join(",") };
                let mut evidence = row.evidence.as_ref().map_or("-".to_string(), |evidence| evidence.describe());
                if !row.door_words.is_empty() {
                    let _ = write!(evidence, "; door: {}", row.door_words.join(","));
                }
                let _ = writeln!(text, "{game}\t{}\t{}\t{}:{}\t{words}\t{evidence}", thing.name, thing.kind, thing.file, thing.line);
            }
            continue;
        }
        for item in &report.items {
            if item.metric == metric || (metric == "fakematch" && item.metric.starts_with("fakematch_")) {
                let _ = writeln!(text, "{game}\t{}:{}\t{}", item.file, item.line, item.what);
            }
        }
    }
    text
}

/// `progress --humanity [--top N] [--list METRIC]`: count both games, write
/// the TSV and return the table, or with `list` that metric's items.
pub fn run(root: &Path, top: usize, metric: Option<&str>) -> Result<String, String> {
    let reports = std::thread::scope(|scope| {
        ["tbs", "tla"]
            .map(|game| {
                scope.spawn(move || {
                    let text = game_text(root, game);
                    sources(root, game)
                        .map(|files| game_with_text(files.iter().map(|(p, t)| (p.as_str(), t.as_str())), text.as_ref()))
                })
            })
            .map(|thread| thread.join().expect("humanity thread"))
    });
    let [sun, anchor] = reports;
    let (sun, anchor) = (sun?, anchor?);
    let games = [("☀️ tbs", &sun), ("⚓️ tla", &anchor)];
    let path = root.join(REPORT);
    std::fs::create_dir_all(path.parent().expect("report directory")).map_err(|error| format!("{REPORT}: {error}"))?;
    super::write(&path, &tsv(&[("tbs", &sun), ("tla", &anchor)]))?;
    if let Some(metric) = metric {
        return Ok(list(&[("tbs", &sun), ("tla", &anchor)], metric).trim_end().to_string());
    }
    Ok(format!("{}humanity={REPORT}", summary(&games, top)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str) -> Report {
        game([("f.c", text)])
    }

    #[test]
    fn an_enum_of_one_enumerator_is_its_comment() {
        let report = one(concat!(
            "/// The inn worker has apologized.\n",
            "enum { FLAG_LALIVERO_INN_WORKER_APOLOGIZED = 0x900 };\n",
            "/// Two flags.\n",
            "enum {\n  FLAG_DOOR_OPEN = 0x901,\n  FLAG_GATE_OPEN = 0x902\n};\n",
            "/// One flag and a count that is not a game thing.\n",
            "enum { FLAG_THIRD = 0x903, THIRD_COUNT = 1 };\n",
        ));
        let alone: Vec<(&str, bool)> = report.things.iter().map(|thing| (thing.name.as_str(), thing.alone)).collect();
        assert_eq!(
            alone,
            [("FLAG_LALIVERO_INN_WORKER_APOLOGIZED", true), ("FLAG_DOOR_OPEN", false), ("FLAG_GATE_OPEN", false), ("FLAG_THIRD", false)]
        );
    }

    #[test]
    fn magic_numbers_in_code_and_tables() {
        let report = one(concat!(
            "#define LIMIT 0x40\n",
            "#define SCALE(x) ((x) * 12)\n",
            "enum { KIND_A = 20, KIND_B };\n",
            "struct S { u8 unk[0x20]; u32 bits : 12; };\n",
            "static const u8 sTable[16] = { 1, 0x30, { 40, 9 } };\n",
            "u8 sBuf[(16 * 32)];\n",
            "void F(int x) {\n",
            "    int a[] = { 99 };\n",
            "    if (x == 0x10 || x == 9 || x == 0x0F) { x = 100; } // 55\n",
            "    G(\"77\", (8 * 8), 1000u);\n",
            "}\n",
        ));
        // 0x10, 100, 1000u and SCALE's 12; 8 * 8 is a size.
        assert_eq!(report.total("magic_code"), 4);
        assert_eq!(report.total("magic_argument"), 1, "1000u");
        // 0x30, 40 and the local table's 99.
        assert_eq!(report.total("magic_table"), 3);
    }

    #[test]
    fn placeholder_and_numbered_names() {
        for name in ["unk_14", "unk14", "unkC", "gUnk_0203", "gUnknown_0203", "sub_08012345", "field_1C", "Foo_Unk"]
        {
            assert!(is_placeholder(name), "{name}");
        }
        assert!(is_placeholder("SE_138") && is_placeholder("Battle_08012345") && is_placeholder("UnkThing"));
        for name in ["Unkempt", "Unknowable", "Trunk", "FunkyUnkle", "SE_JUMP"] {
            assert!(!is_placeholder(name), "{name}");
        }
        for name in ["ColosseumFinal1", "Event_LeaveByExit10", "LOCALID_8", "gEffectScript0", "VenusLighthouse2", "arg0"] {
            assert!(is_numbered(name), "{name}");
        }
        for name in [
            "u8", "vu16", "s32", "BG2", "Bg2", "Dma3", "REG_DMA3", "Random16", "Dma_Fill32", "MulQ16", "Q_8_8",
            "FLAG_MAP_TEMP_01", "TBS_PATCH_1", "DISPCNT_MODE_1", "BLDCNT_TGT1_BG2", "x1", "ArcTan2", "attr2",
            "Vector3", "unk_14", "MapGridCell", "SE_138", "FLAG_MAP_TEMP_0E", "__divsi3", "rgb555", "OBJ_VRAM0",
        ] {
            assert!(!is_numbered(name), "{name}");
        }
        let report = one("void ColosseumFinal1(void) { unk_14 = ColosseumFinal1 + gUnk_0203; } // unk_99 Exit10\n");
        assert_eq!((report.distinct("placeholder"), report.total("placeholder")), (2, 2));
        assert_eq!((report.distinct("numbered"), report.total("numbered")), (1, 2));
    }

    #[test]
    fn devices_goto_and_edition_tests() {
        let report = one(concat!(
            "#if TBS_PATCH_1\n",
            "int a;\n",
            "#elif (LANGUAGE == LANGUAGE_GERMAN)\n",
            "int a;\n",
            "#endif\n",
            "#ifdef UBFIX\n",
            "#endif\n",
            "void F(void) {\n",
            "    /* FAKEMATCH(wrapper): kept; as plain, 2 of 9 instructions differ in tbs-en. */\n",
            "    // FAKEMATCH(dead): kept. FAKEMATCH(<kind>) is the form.\n",
            "#if LANGUAGE != LANGUAGE_JAPANESE && TBS_PATCH_2\n",
            "    goto done;\n",
            "#endif\n",
            "done:;\n",
            "}\n",
        ));
        assert_eq!((report.total("fakematch_wrapper"), report.total("fakematch_dead"), report.value("fakematch")), (1, 1, 2));
        assert_eq!(report.total("goto"), 1);
        assert_eq!((report.total("if_language"), report.total("if_patch")), (2, 2));
        assert_eq!(report.total("if_in_function"), 1);
    }

    #[test]
    fn declarations_without_documentation() {
        let header = concat!(
            "#ifndef ALCHEMY_F_H\n",
            "#define ALCHEMY_F_H\n",
            "/// Documented in its header.\n",
            "void Documented(void);\n",
            "void Plain(void);\n",
            "extern u8 gTable[]; ///< Noted.\n",
            "extern u8 gBare;\n",
            "/// A macro.\n",
            "#define NAMED 1\n",
            "#define BARE 2\n",
            "#define NOTED 3 ///< Noted.\n",
            "typedef struct Pair { u8 a; } Pair;\n",
            "/// Kinds.\n",
            "enum Kind { KIND_A };\n",
            "enum { LOOSE_A };\n",
            "enum {\n    /// Noted at its first constant.\n    NOTED_A,\n};\n",
            "struct Forward;\n",
            "void (*gHandler)(void);\n",
            "struct Packed { u8 a : 2; } __attribute__((packed));\n",
            "extern s16 CAPITAL_TABLE[] ALIGNED(4);\n",
            "#endif\n",
        );
        let source = concat!(
            "void Documented(void) {}\n",
            "void Plain(void) {}\n",
            "/// Its own.\n",
            "static void Local(void) {}\n",
            "UNUSED static int Helper(int x) { return x; }\n",
            "u8 gTable[] = { 1, 2 };\n",
            "u8 gBare;\n",
        );
        let report = game([("f.h", header), ("f.c", source)]);
        assert_eq!(report.total("undocumented_function"), 2, "Plain and Helper");
        assert_eq!(report.total("undocumented_global"), 3, "gBare, gHandler and CAPITAL_TABLE");
        assert_eq!(report.total("undocumented_macro"), 1, "BARE");
        assert_eq!(report.total("undocumented_struct"), 2);
        assert_eq!(report.total("undocumented_enum"), 1);
        assert_eq!(report.top("undocumented_function", 5), vec![("f.c", 2)]);
    }

    #[test]
    fn duplicates_long_files_and_report() {
        let body = "{ int i; for (i = 0; i < 4; i++) { gA[i] = Load(i) + 3; Store(gB, i); } }\n";
        let renamed = body.replace("gA", "gC").replace("Load", "Fetch");
        let table = "= { 1, 2, 3, 4, 5, 6, 7, 8, 9, 10 };\n";
        let first = format!("void Alpha(void) {body}void Beta(void) {renamed}void Gamma(void) {{ int i; }}\nconst u8 gT1[] {table}");
        let long = "\n".repeat(3001);
        let second = format!("const u8 gT2[] {table}void Delta(void) {{}}{long}");
        // A map overlay, a part, a header and a data object stay one file however long.
        let part = format!("// Part of battle_anim.c.\nvoid Epsilon(void) {{}}{long}");
        let data = format!("const u8 gT3[] = {{ 0 }};{long}");
        let scene = format!("void Zeta(void) {{}}{long}");
        let report = game([
            ("a.c", first.as_str()),
            ("b.c", second.as_str()),
            ("games/tbs/src/map/vale.c", scene.as_str()),
            ("games/tbs/src/battle_anim/parts/bolt.c", part.as_str()),
            ("games/tbs/include/map_warps.h", long.as_str()),
            ("games/tbs/src/overworld/table.c", data.as_str()),
        ]);
        assert_eq!(report.function_groups.len(), 1);
        assert_eq!(report.function_groups[0].members, vec!["a.c:Alpha", "a.c:Beta"]);
        assert_eq!(report.table_groups.len(), 1);
        assert_eq!(report.table_groups[0].members, vec!["a.c:gT1", "b.c:gT2"]);
        assert_eq!(report.long_files, vec![("b.c".to_string(), 3002)]);
        let tsv = tsv(&[("tbs", &report)]);
        assert!(tsv.starts_with("game\tmetric\tscope\tcount\ntbs\tfiles\ttotal\t6\n"));
        assert!(tsv.contains("tbs\tduplicate_function\ttotal\t1\n"));
        assert!(tsv.contains("tbs\tduplicate_function\ta.c:Alpha a.c:Beta\t"));
        assert!(tsv.contains("tbs\tlong_file\tb.c\t3002\n"));
        let table = summary(&[("tbs", &report)], 0);
        assert!(table.contains("objects over 3,000 lines") && table.lines().count() == ROWS.len() + 3);
    }

    #[test]
    fn arguments_that_are_data() {
        for (name, kind) in [
            ("frames", Some("frames")), ("waitFrames", Some("frames")), ("srcX", Some("coordinate")), ("fixed_z", Some("coordinate")),
            ("xOffset", Some("coordinate")), ("x2", Some("coordinate")), ("sourceRow", Some("coordinate")), ("height", Some("size")),
            ("count", Some("count")), ("stepX", None), ("initial_velocity_y", None), ("speed", None), ("facing", None), ("box", None),
        ] {
            assert_eq!(data_parameter(name), kind, "{name}");
        }
        let header = concat!(
            "void FieldScript_WaitFrames(s32 frames);\n",
            "#define WAIT(n) FieldScript_WaitFrames(n)\n",
            "#define PIXELS(value) ((value) << 16)\n",
            "#define BLEND(top, bottom) Blend((top) | ((bottom) << 8))\n",
            "void ObjectEvent_SetPosition(s32 actor, s32 fixed_x, s32 fixed_z);\n",
            "static inline void ObjectEvent_SetSpeed(s32 actor, s32 speed, s32 acceleration) { Engine(actor, speed); }\n",
            "void Old();\n",
            "void Blend(s32);\n",
        );
        let source = concat!(
            "void Run(void) {\n",
            "    WAIT(80);\n",
            "    FieldScript_WaitFrames(\n        40);\n",
            "    ObjectEvent_SetPosition(LOCALID_LEADER, PIXELS(312), (416) << 16);\n",
            "    ObjectEvent_SetSpeed(LOCALID_LEADER, PIXELS(24), 0x8000);\n",
            "    Old(1, 20);\n",
            "    BLEND(10, 12);\n",
            "    draw[0](canvas, 64, 40);\n",
            "}\n",
        );
        let report = game([("f.h", header), ("f.c", source)]);
        assert_eq!(report.total("data_argument_frames"), 2, "WAIT(80) and 40");
        assert_eq!(report.total("data_argument_coordinate"), 2, "312 and 416, through PIXELS and a group");
        assert_eq!(report.total("data_argument"), 4);
        // 24 and 0x8000 (a speed and an acceleration), 20, 10 and 12, 64 and 40;
        // the shifts' 16, in PIXELS and in Run, are no arguments.
        assert_eq!((report.total("magic_argument"), report.total("magic_code")), (7, 9));
        let listed = list(&[("tbs", &report)], "magic_argument");
        assert!(listed.starts_with("tbs\tf.c:6\t24\tPIXELS value\ntbs\tf.c:6\t0x8000\tObjectEvent_SetSpeed acceleration\ntbs\tf.c:7\t20\tOld #2\n"), "{listed}");
        assert!(listed.ends_with("tbs\tf.c:9\t40\t-\n"), "{listed}");
        assert!(list(&[("tbs", &report)], "data_argument").contains("tbs\tf.c:2\t80\tWAIT n\n"));
    }

    #[test]
    fn entry_points_need_their_summary() {
        let far = "/// Draws a number.\nvoid Text_DrawNumberFar(s32 number);\nvoid UiText_ClearFar(void);\n";
        let source = "void UiText_ClearFar(void) {}\nvoid Helper(void) {}\n";
        let report = game([("games/tbs/include/farcall/window.h", far), ("games/tbs/src/window/text.c", source)]);
        assert_eq!(list(&[("tbs", &report)], "undocumented_entry"), "tbs\tgames/tbs/src/window/text.c:1\tUiText_ClearFar\n");
        assert_eq!(report.total("undocumented_function"), 2, "UiText_ClearFar and Helper, to review");
    }

    #[test]
    fn divergent_branches_keep_the_brace_depth() {
        let report = one(concat!(
            "#if TBS_PATCH_1\n",
            "const u8 gA[] = {\n",
            "#else\n",
            "const u8 gA[] = { 0,\n",
            "#endif\n",
            "    50 };\n",
            "void F(void) { x = 50; }\n",
        ));
        assert_eq!((report.total("magic_table"), report.total("magic_code")), (1, 1));
    }
}

#[cfg(test)]
mod listing_tests {
    use super::*;

    #[test]
    fn items_list_where_each_count_is() {
        let report = game([
            ("tbs_edition.h", "#if defined(TBS_EDITION_EN)\n#endif\n"),
            ("ed_asm.h", "\t.set EDITION_INTERNATIONAL, 0\n\t.ifdef TBS_EDITION_JA\n"),
            ("f.c", "#ifdef TBS_EDITION_EN\n#endif\nvoid Run(void) {\n  x = 0x40; // FAKEMATCH(pin): kept\n}\n"),
        ]);
        assert_eq!(report.total("if_edition_name"), 1, "only outside edition.h");
        assert_eq!(report.files, 2, "an assembly header is not C");
        let listed = list(&[("tbs", &report)], "magic_code");
        assert_eq!(listed, "tbs\tf.c:4\t0x40\n");
        assert_eq!(list(&[("tbs", &report)], "fakematch"), "tbs\tf.c:4\tpin\n");
        assert_eq!(list(&[("tbs", &report)], "undocumented_function"), "tbs\tf.c:3\tRun\n");
    }
}

#[cfg(test)]
mod provenance_tests {
    use super::*;

    #[test]
    fn names_of_game_things_and_their_evidence() {
        let po = concat!(
            "msgctxt \"MsgSign\"\nmsgid \"00001\"\nmsgstr \"McCoy's Hidden Treasury\"\n\n",
            "msgctxt \"MsgDebugEntryName\"\nmsgid \"00002\"\nmsgstr \"Bilibin\"\n\n",
            "msgctxt \"message\"\nmsgid \"00003\"\nmsgstr \"Bilibin Barricade\"\n",
        );
        let text = GameText::parse(po, area_count("\tgroup_name 0, -1, 0\n\tscene_name Bilibin, -1, 1\n"));
        let ids = "/// @file\n/// Scenes.\n\n/// ★ Named from its area name: Bilibin\nextern char MapId_Bilibin;\nextern char MapId_BilibinWarehouse;\n";
        let flags = "/// ★ Named from the game: \"Hidden Treasury\" (MsgSign)\n#define FLAG_TREASURY_OPEN 0x10\n#define FLAG_MAP_TEMP_01 0x200\n#define FLAG_WAREHOUSE_SEEN 0x11 ///< Named from the game: \"Hidden Treasury\" (MsgSign)\n";
        let scene = concat!(
            "/// @file\n/// Bilibin and the Bilibin Barricade.\n///\n/// LOCALID_GUARD stands by `Bilibin Barricade`.\n",
            "#include \"x.h\"\n",
            "/// Its actors.\n",
            "enum {\n",
            "  LOCALID_GUARD, ///< the guard\n",
            "  LOCALID_PILLAR_APPROACH = 3, ///< a stack of crates;\n",
            "                          ///< Named from what the player sees: crates (its render)\n",
            "  /// A man.\n  LOCALID_MAN,\n",
            "};\n",
            "/// Runs the treasury.\n",
            "void Bilibin_UpdateWarehouse(void) { LOCALID_MAN; }\n",
            "static const u8 gBilibinTreasury[] = { 1 };\n",
        );
        let files = [
            ("games/tbs/include/map_ids.h", ids),
            ("games/tbs/include/flags.h", flags),
            ("games/tbs/src/map/bilibin.c", scene),
        ];
        let report = game_with_text(files, Some(&text));
        let names: Vec<&str> = report.things.iter().map(|thing| thing.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "MapId_Bilibin", "MapId_BilibinWarehouse", "FLAG_TREASURY_OPEN", "FLAG_WAREHOUSE_SEEN", "LOCALID_GUARD",
                "LOCALID_PILLAR_APPROACH", "LOCALID_MAN", "Bilibin_UpdateWarehouse", "gBilibinTreasury",
            ]
        );
        assert_eq!(report.value("provenance_names"), 7);
        assert_eq!(report.value("provenance_verified"), 2, "MapId_Bilibin and FLAG_TREASURY_OPEN");
        assert_eq!(report.value("provenance_unstarred"), 2, "FLAG_WAREHOUSE_SEEN and LOCALID_PILLAR_APPROACH");
        assert_eq!(report.value("provenance_prose"), 1, "LOCALID_GUARD, from its place paragraph");
        assert_eq!(report.value("provenance_none"), 2, "MapId_BilibinWarehouse and LOCALID_MAN");
        assert_eq!(report.value("unused_words"), 7, "warehouse, open, seen, guard, pillar, approach and man");
        let words = list(&[("tbs", &report)], "unused_word");
        assert!(words.starts_with("tbs\twarehouse\t3\tMapId_BilibinWarehouse games/tbs/include/map_ids.h:6; "), "{words}");
        let worklist = list(&[("tbs", &report)], "provenance");
        assert!(worklist.contains("tbs\tLOCALID_PILLAR_APPROACH\tobject event\tgames/tbs/src/map/bilibin.c:9\tpillar,approach\tunstarred render\n"), "{worklist}");
        assert!(worklist.contains("tbs\tBilibin_UpdateWarehouse\tfunction\tgames/tbs/src/map/bilibin.c:15\twarehouse\t-\n"));
        assert!(!worklist.contains("gBilibinTreasury"), "a mechanism whose words the game uses is not work");
        assert_eq!(list(&[("tbs", &report)], "provenance_none").lines().count(), 2);
        let tsv = tsv(&[("tbs", &report)]);
        assert!(tsv.contains("tbs\tunused_word\twarehouse\t3\n"));
        assert!(game(files).provenance.is_none(), "no text, no check");
    }

    #[test]
    fn mechanisms_are_not_game_things() {
        let flags = concat!(
            "#define FLAG_IN_PARTY(character) (character)\n",
            "#define FLAG_SYSTEM_FIRST 0x100\n#define FLAG_SYSTEM_LAST 0x1ff\n",
            "#define FLAG_IN_BATTLE 0x103\n#define FLAG_LURE_CAP (FLAG_SYSTEM_FIRST + 0x67)\n",
            "#define FLAG_KRADEN_SETS_OFF 0x801\n",
        );
        let actors = concat!(
            "enum ObjectEventCollisionFlag { OBJ_EVENT_SOLID = 1 };\n",
            "enum BilibinLocalId { LOCALID_GUARD = 8, LOCALID_SHOP_END };\n",
            "enum { LOCALID_MAYOR = 10, OBJ_EVENT_SLOT_COUNT = 66 };\n",
            "#define ITEM_TYPE_WEAPON 1\n#define ITEM_LIST_NAME_X 12\n",
        );
        let files = [("games/tbs/include/flags.h", flags), ("games/tbs/include/object_event.h", actors)];
        let report = game(files);
        let names: Vec<&str> = report.things.iter().map(|thing| thing.name.as_str()).collect();
        assert_eq!(names, ["FLAG_KRADEN_SETS_OFF", "LOCALID_GUARD", "LOCALID_SHOP_END", "LOCALID_MAYOR"]);
    }

    #[test]
    fn enumerator_values_and_table_names() {
        let words = |text: &str| text.split(' ').map(str::to_string).collect::<Vec<_>>();
        let lookup = |name: &str| (name == "FIRST").then_some(8);
        let value = |text: &str| evaluate(&words(text).iter().map(String::as_str).collect::<Vec<_>>(), &lookup);
        assert_eq!(value("0x1a"), Some(26));
        assert_eq!(value("( FIRST + 2 ) * 3"), Some(30));
        assert_eq!(value("1 << 4 | 1"), Some(17));
        assert_eq!(value("- 1"), Some(-1));
        assert_eq!(value("( u8 ) 3"), None, "a cast is unknown");
        assert_eq!(value("FIRST +"), None);
        let po = concat!(
            "msgctxt \"MsgEnemyName\"\nmsgid \"00655\"\nmsgstr \"???\"\n\n",
            "msgctxt \"message\"\nmsgid \"00656\"\nmsgstr \"Vermin\"\n\n",
            "msgctxt \"message\"\nmsgid \"00657\"\nmsgstr \"Vermin\"\n\n",
            "msgctxt \"message\"\nmsgid \"00658\"\nmsgstr \"Mad Vermin\"\n",
        );
        let text = GameText::parse(po, None);
        let header = concat!(
            "/// The first class id.\n#define ENEMY_ID_FIRST 8\n",
            "/// Classes.\nenum EnemyId {\n",
            "  ENEMY_VERMIN = ENEMY_ID_FIRST + 1,\n  ENEMY_VERMIN_2,\n  ENEMY_RAT = 0xb, ///< a rat\n};\n",
            "enum { ENEMY_NOT_IN_A_TABLE = 9 };\n",
        );
        let report = game_with_text([("games/tbs/include/enemy_ids.h", header)], Some(&text));
        let values: Vec<(Option<&str>, Option<i64>)> =
            report.things.iter().map(|thing| (thing.tag.as_deref(), thing.value)).collect();
        assert_eq!(values, [(Some("EnemyId"), Some(9)), (Some("EnemyId"), Some(10)), (Some("EnemyId"), Some(11)), (None, Some(9))]);
        assert_eq!(report.macro_values.get("ENEMY_ID_FIRST"), Some(&8));
        assert_eq!(report.value("provenance_table"), 2, "ENEMY_VERMIN and ENEMY_VERMIN_2");
        assert_eq!(report.value("provenance_table_mismatch"), 1);
        assert_eq!(list(&[("tbs", &report)], "provenance_table_mismatch"), "tbs\tgames/tbs/include/enemy_ids.h:7\tENEMY_RAT\tMad Vermin\n");
        let worklist = list(&[("tbs", &report)], "provenance");
        assert!(worklist.contains("\tENEMY_VERMIN_2\tenemy\tgames/tbs/include/enemy_ids.h:6\tenemy\ttable MsgEnemyName+2 numbered\n"), "{worklist}");
    }

    #[test]
    fn door_words_in_the_report() {
        let po = "msgctxt \"MsgDebugEntryName\"\nmsgid \"00001\"\nmsgstr \"Tolbi\"\n";
        let text = GameText::parse(po, Some(1));
        let ids = "/// ★ Named from its area name: Tolbi\nextern char MapId_Tolbi;\n";
        let links = "enum {\n  TOLBI_FROM_WORLD_MAP_NORTH = 1,\n  TOLBI_FROM_HOUSE = 2,\n  TOLBI_TO_HOUSE = 3,\n};\n";
        let files = [("games/tbs/include/map_ids.h", ids), ("games/tbs/include/map_warps.h", links)];
        let report = game_with_text(files, Some(&text));
        assert_eq!(report.value("provenance_rooms"), 0, "WORLD_MAP is no map here");
        assert_eq!((report.value("door_words"), report.value("door_word_link")), (3, 3), "world, map and house; north is a door word");
        assert_eq!(list(&[("tbs", &report)], "door_word").lines().next(), Some("tbs\thouse\t2\tTOLBI_FROM_HOUSE games/tbs/include/map_warps.h:3; TOLBI_TO_HOUSE games/tbs/include/map_warps.h:4"));
        let worklist = list(&[("tbs", &report)], "provenance");
        assert!(worklist.contains("\tTOLBI_TO_HOUSE\twarp\tgames/tbs/include/map_warps.h:4\tto,house\tnone; door: house\n"), "{worklist}");
        assert!(tsv(&[("tbs", &report)]).contains("tbs\tdoor_word\thouse\t2\n"));
    }
}

#[cfg(test)]
mod naming_tests {
    use super::*;

    #[test]
    fn names_are_read_for_the_naming_checks() {
        let guide = "# Battle\n\n| `Battle_` | the battle |\n";
        let code = concat!(
            "struct Unit { u8 itemCnt; u16 count; void (*draw)(void); };\n",
            "enum Mode { MODE_IDLE, MODE_WALK, MODE_RUN };\n",
            "#define UNIT_COUNT 4\n",
            "void Battle_Run(struct Unit *unit) { struct Unit *target; }\n",
            "void Spoils_Count(void) {}\n",
        );
        let report = game([("games/tbs/src/battle/README.md", guide), ("games/tbs/src/battle/a.c", code)]);
        let named: Vec<(&str, NameKind)> = report.naming.names.iter().map(|name| (name.name.as_str(), name.kind)).collect();
        assert_eq!(
            named,
            [
                ("itemCnt", NameKind::Field),
                ("count", NameKind::Field),
                ("draw", NameKind::Field),
                ("Unit", NameKind::Tag),
                ("MODE_IDLE", NameKind::Enumerator),
                ("MODE_WALK", NameKind::Enumerator),
                ("MODE_RUN", NameKind::Enumerator),
                ("Mode", NameKind::Tag),
                ("UNIT_COUNT", NameKind::Macro),
                ("Battle_Run", NameKind::Function),
                ("Spoils_Count", NameKind::Function),
            ]
        );
        let typed: Vec<(&str, &str)> = report.naming.typed.iter().map(|typed| (typed.tag.as_str(), typed.name.as_str())).collect();
        assert_eq!(typed, [("Unit", "unit"), ("Unit", "target")]);
        assert_eq!(
            list(&[("tbs", &report)], "abbreviation"),
            "tbs\tgames/tbs/src/battle/a.c:1\titemCnt\tcnt -> count: itemCount (3 names spell count, as UNIT_COUNT does)\n"
        );
        assert_eq!(report.value("abbreviation"), 1);
    }
}
