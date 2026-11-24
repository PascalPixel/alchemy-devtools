//! Whether names can be guessed from one another. Alchemy's bar for a
//! humanized name is that a reader who knows one name of a family can predict
//! the others: one word for each idea, one shape for each family, and a small
//! vocabulary of prefixes (S3). These checks read
//! every name the source defines or declares (functions, globals, macros,
//! enumerators, tags, typedefs and struct members, and the variables declared
//! with a struct type), never the ROM, and report where
//! a name disagrees with its siblings:
//!
//! - `synonym`: a name that spells an idea with a second word where the game
//!   or the code has settled on one (`SPELLINGS`);
//! - `synonym_referent`: one struct type that its variables call by
//!   different spellings of its own name (`obj` and `object` for
//!   `struct Object`);
//! - `abbreviation`: a shortened word (`Cnt`, `Idx`, `Obj`) whose full word
//!   other names spell out (`ABBREVIATIONS`);
//! - `family_shape`: a member of a prefix family or an enum whose shape its
//!   siblings don't share (its case, a missing part of the family's prefix,
//!   a family prefix without its `_`, a place that isn't first where the
//!   family puts it first, or a count or bound on the other side from the
//!   rest of the game);
//! - `prefix_twin`: one idea under two prefixes (`ANIM_` and `ANIMATION_`).
//!
//! Each rule measures a name against what the rest of the game does, so a
//! convention the game keeps everywhere is never flagged, and the conventions
//! AGENTS.md sets (pret's `gName` and `sName`, message symbols, the
//! hardware's register names, the temporary flags, a function taking the
//! prefix of a thing the code names elsewhere) are left alone.

use super::provenance::{self, GameText};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// What a name names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NameKind {
    Function,
    Global,
    Macro,
    FunctionMacro,
    Enumerator,
    Tag,
    Typedef,
    Field,
    /// A function declared but not defined in C.
    Prototype,
    /// A global declared `extern`.
    Extern,
}

/// One name where the source defines or declares it.
#[derive(Clone, Debug)]
pub struct Name {
    pub name: String,
    pub kind: NameKind,
    pub file: String,
    pub line: usize,
    /// An enumerator's enum or a member's struct.
    pub group: Option<String>,
}

/// A variable, parameter or member declared with a struct or union type.
#[derive(Clone, Debug, Default)]
pub struct Typed {
    pub tag: String,
    pub name: String,
    pub file: String,
    pub line: usize,
    /// A struct or union member, which names its place in the record rather
    /// than the thing it holds.
    pub member: bool,
    /// An array, which names a set of the things rather than one.
    pub array: bool,
    /// The declaration it belongs to in its file (the token that starts a
    /// function, for its parameters and locals), to tell which names a
    /// function already uses.
    pub scope: Option<usize>,
}

/// What the checks read: every name, the struct-typed variables, the
/// identifiers pret's flash library uses (`PRET_FLASH`) and those the m4a
/// sound driver's code uses (`M4A`), every name each function gives its
/// parameters and locals, and the file stems of the game's pictures.
#[derive(Default)]
pub struct Names {
    pub names: Vec<Name>,
    pub typed: Vec<Typed>,
    pub pret: HashSet<String>,
    /// The parameters and locals of each function, of any type, by its file
    /// and the token that starts it (`Typed::scope`).
    pub scopes: HashMap<(String, usize), HashSet<String>>,
    /// The stems of the game's pictures (`crates` of `sprite/crates.png`),
    /// lowercase.
    pub assets: HashSet<String>,
}

/// Names pret spells with a plural prefix beside a singular one
/// (pokeemerald's `PALETTES_ALL` beside `PALETTE_`, its `TRACKS_ALL`), which
/// the games keep as pret writes them.
pub const PRET_PLURALS: &[&str] = &[
    "PALETTES_ALL",
    "PALETTES_BG",
    "PALETTES_OBJECTS",
    "TRACKS_ALL",
];

/// The files of pret's flash library, the Makefile's `FLASH_C`, which
/// pret's `old_agbcc` builds. Their names are pret's (`gFlashNumRemainingBytes`,
/// `maxTime`), which AGENTS.md keeps as pret writes them, so every
/// identifier they use is left out of the checks.
pub const PRET_FLASH: &[&str] = &[
    "tbs/src/gamepak/flash/flash.c",
    "tbs/src/gamepak/flash/sst.c",
    "tbs/src/gamepak/flash/atmel.c",
    "tbs/src/gamepak/flash/program_flash_sector.c",
    "tbs/src/cartridge/save/flash.c",
    "tbs/src/cartridge/save/sst.c",
    "tbs/src/cartridge/save/atmel.c",
    "tbs/src/cartridge/save/program_flash_sector.c",
    "tla/src/gamepak/flash/flash.c",
    "tla/src/gamepak/flash/sst.c",
    "tla/src/gamepak/flash/atmel.c",
    "tla/src/gamepak/flash/program_flash_sector.c",
    "tla/src/system/save/flash.c",
    "tla/src/system/save/sst.c",
    "tla/src/system/save/atmel.c",
    "tla/src/system/save/program_flash_sector.c",
];

/// Whether a file is one of pret's flash library's.
pub fn is_pret_flash(path: &str) -> bool {
    PRET_FLASH.iter().any(|file| path.ends_with(file))
}

/// The files of the m4a sound driver, the third-party library S3 keeps under
/// its own names (`SoundMain`, `MPlayMainNext`, `numTracks`,
/// `MAX_DIRECTSOUND_CHANNELS`): the C the Makefile's `SOUND_C` routes
/// through pret's `old_agbcc`, and the driver's own headers. Every
/// identifier their code uses (comments and strings aside, which name the
/// game's routines that call them) is left out of the checks.
pub const M4A: &[&str] = &[
    "tbs/src/sound/audio.c",
    "tbs/src/sound/tables.c",
    "tbs/src/sound/driver.c",
    "tla/src/sound/sound.c",
    "tla/src/sound/tables.c",
    "tla/src/sound/player_work_copy.c",
    "tla/src/sound/driver.c",
    "/include/sound/sound_driver.h",
    "/include/sound/sound_driver_types.h",
    "/include/sound/sound_driver_symbols.h",
];

/// Whether a file is one of the m4a sound driver's.
pub fn is_m4a(path: &str) -> bool {
    M4A.iter().any(|file| path.ends_with(file))
}

/// The identifiers a file's code uses: its comments, strings and character
/// constants left out.
pub fn code_identifiers(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let (mut at, mut start) = (0, None);
    while at < bytes.len() {
        let b = bytes[at];
        if b.is_ascii_alphanumeric() || b == b'_' {
            start.get_or_insert(at);
            at += 1;
            continue;
        }
        if let Some(from) = start.take() {
            out.push(&text[from..at]);
        }
        match (b, bytes.get(at + 1)) {
            (b'/', Some(b'/')) => at = text[at..].find('\n').map_or(bytes.len(), |end| at + end),
            (b'/', Some(b'*')) => {
                at = text[at + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |end| at + 2 + end + 2)
            }
            (b'"' | b'\'', _) => {
                at += 1;
                while at < bytes.len() && bytes[at] != b {
                    at += if bytes[at] == b'\\' { 2 } else { 1 };
                }
                at += 1;
            }
            _ => at += 1,
        }
    }
    if let Some(from) = start {
        out.push(&text[from..]);
    }
    out.retain(|word| word.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_'));
    out
}

/// The identifiers a file's text uses, comments and strings included.
pub fn identifiers(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| word.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_'))
}

/// One name that disagrees with its siblings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub metric: &'static str,
    pub file: String,
    pub line: usize,
    /// The name, a tab, and what it disagrees with or the form expected.
    pub what: String,
}

/// Shortened words and the words they shorten. A name is flagged for one
/// only where another name spells the full word out, so the game already
/// has both spellings of one idea, and never where the game's own text uses
/// the short form (`Psy`, `HP`). The short forms pret itself writes
/// (`PRET_WORDS`: `num`, a count wherever this code uses it, as in
/// `NUM_ELEMENTS` and pret's `songNum`) are here only so that each is one
/// idea with its full word, and are never flagged. Words the hardware or AGENTS.md gives the code
/// (`BG`, `OAM`, `attr`, `Msg` for a message symbol, `Temp` for the
/// temporary flags) are not here.
pub const ABBREVIATIONS: &[(&str, &str)] = &[
    ("cnt", "count"),
    ("num", "count"),
    ("idx", "index"),
    ("tmp", "temp"),
    ("ptr", "pointer"),
    ("buf", "buffer"),
    ("len", "length"),
    ("pos", "position"),
    ("obj", "object"),
    ("anim", "animation"),
    ("init", "initialize"),
    ("pal", "palette"),
    ("gfx", "graphics"),
    ("msg", "message"),
    ("calc", "calculate"),
    ("val", "value"),
    ("addr", "address"),
    ("src", "source"),
    ("dst", "destination"),
    ("dest", "destination"),
    ("cur", "current"),
    ("curr", "current"),
    ("prev", "previous"),
    ("ctr", "counter"),
    ("btn", "button"),
    ("spr", "sprite"),
    ("fx", "effect"),
    ("sel", "selection"),
    ("cmd", "command"),
    ("mgr", "manager"),
    ("ofs", "offset"),
    ("elem", "element"),
    ("arg", "argument"),
    ("param", "parameter"),
    ("ctx", "context"),
    ("coord", "coordinate"),
    ("dir", "direction"),
    ("rot", "rotation"),
    ("vel", "velocity"),
    ("accel", "acceleration"),
    ("tbl", "table"),
    ("lvl", "level"),
    ("img", "image"),
    ("desc", "description"),
];

/// Shortened words pret's map layer writes, which the source kept as pret
/// does until The Broken Seal's rename to entrances and step triggers, each
/// beside the word it shortens with: a warp destination was a `WarpDest`, an
/// event the party triggers by stepping on its cell a `CoordEvent`
/// (`COORD_EVENT_FLAG_WALKING`, `MAP_EVENT_COORD`). The short word elsewhere
/// is still measured.
pub const PRET_TERMS: &[(&str, &str)] = &[("warp", "dest"), ("coord", "event"), ("event", "coord")];

/// Shortened words pret writes as its own spelling, kept as pret does
/// wherever they appear (`StartSpriteAnim`, `gBattleAnimArgs`, `InitHeap`,
/// `affineParam`, `songNum`): never flagged as an abbreviation, and the
/// spelling `prefix_twin` prefers.
pub const PRET_WORDS: &[&str] = &["anim", "anims", "args", "init", "param", "num"];

/// Whether a word, as written or in the singular, is one of `PRET_WORDS`.
fn is_pret_word(word: &str) -> bool {
    PRET_WORDS.contains(&word) || PRET_WORDS.contains(&singular(word).as_str())
}

/// Whether the word at `at` of a name's words is half of a `PRET_TERMS` pair.
fn in_pret_term(words: &[String], at: usize) -> bool {
    let pair = |first: &str, second: &str| PRET_TERMS.contains(&(first, second));
    (at > 0 && pair(&words[at - 1], &words[at]))
        || words.get(at + 1).is_some_and(|next| pair(&words[at], next))
}

/// Words that name one idea, as the code has used them, each set marked by
/// whose words they are. A game thing's word is the game's (H3): where its
/// text uses exactly one word of a set, that is the word; where it uses
/// several, the game itself tells them apart (`gold` and `coins`, `monster`
/// and `enemy`), so the set doesn't apply, to words or prefixes; where it uses none, the
/// word most names use. A code idea's word is the one most names use, its
/// shortened forms counted with it. Words that only look alike, or that the
/// code uses for different things (`player`, the person playing, and the
/// leader the player controls; `draw` and `render`, two stages; `destroy`
/// an object and `delete` a save; the ivy Growth raises and a vine), are not
/// here.
pub const SPELLINGS: &[(Words, &[&str])] = &[
    (
        Words::Game,
        &["merchant", "shopkeeper", "shopkeep", "clerk", "vendor"],
    ),
    (Words::Game, &["enemy", "monster", "foe"]),
    (Words::Game, &["rival", "opponent"]),
    (Words::Game, &["coin", "gold", "money"]),
    (Words::Code, &["start", "begin"]),
    (Words::Code, &["calculate", "compute"]),
];

/// Whose words a `SPELLINGS` set holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Words {
    /// A game thing's: the game's text decides.
    Game,
    /// A code idea's: the names decide.
    Code,
}

/// Words that say how many or how far, which the game writes on one side of
/// the thing they measure: `ITEM_COUNT`, not `NUM_ITEMS`.
pub const MEASURE_WORDS: &[&str] = &["count", "num", "total", "max", "min", "size", "limit"];

/// A family is measured once it has this many members.
const FAMILY: usize = 3;

/// A name's words, lowercase, as `provenance::pieces` splits them: pret's
/// `g` and `s`, single letters and digits are dropped.
fn words(name: &str) -> Vec<String> {
    provenance::pieces(name)
        .into_iter()
        .map(str::to_ascii_lowercase)
        .collect()
}

/// A word in the singular: `entries` is `entry`, `objects` `object`. Only
/// plurals come off, never `-ing` or `-ed`, which make other words
/// (`FLASHING_` is not `FLASH_`, nor `STATE` `STAT`). `-es` comes off only
/// after a hissing ending (`classes`, `statuses`, `boxes`), so `poses` is
/// `pose`, not `pos`.
fn singular(word: &str) -> String {
    let w = word.to_ascii_lowercase();
    let n = w.len();
    if n > 4 && w.ends_with("ies") {
        format!("{}y", &w[..n - 3])
    } else if n > 3
        && w.ends_with("es")
        && ["ss", "us", "x", "z", "ch", "sh"]
            .iter()
            .any(|end| w[..n - 2].ends_with(end))
    {
        w[..n - 2].to_string()
    } else if n > 3 && w.ends_with('s') && !["ss", "us", "is"].iter().any(|end| w.ends_with(end)) {
        w[..n - 1].to_string()
    } else {
        w
    }
}

/// A word in the plural, for a suggested rename: `enemy` is `enemies`.
fn plural(word: &str) -> String {
    match word.strip_suffix('y') {
        Some(base) if !base.ends_with(['a', 'e', 'o', 'u']) => format!("{base}ies"),
        _ if word.ends_with(['s', 'x']) || word.ends_with("ch") || word.ends_with("sh") => {
            format!("{word}es")
        }
        _ => format!("{word}s"),
    }
}

/// What one word means to these checks: its full form, and for a word of a
/// `SPELLINGS` set, the set's first word, singular.
fn meaning(word: &str) -> String {
    let word = singular(word);
    let full = ABBREVIATIONS
        .iter()
        .find(|(short, _)| *short == word)
        .map_or(word.as_str(), |(_, full)| *full);
    SPELLINGS
        .iter()
        .find(|(_, set)| set.contains(&full))
        .map_or(full, |(_, set)| set[0])
        .to_string()
}

/// Names the checks leave alone: placeholders, the compiler's and the
/// hardware's names (`__`, `REG_`, `io_reg.h`), message symbols (`Msg...`,
/// AGENTS.md's S3), the temporary flags, the m4a driver's music players
/// (`gMPlayInfo_BGM`, `gMPlayInfo_SE5`: the driver's own scheme, wherever
/// declared), and the `OWN_` guards, which
/// repeat the name of the symbol they guard (`OWN_gFlash`) and are
/// measured there.
fn exempt(name: &Name) -> bool {
    let n = name.name.as_str();
    super::is_placeholder(n)
        || n.starts_with("__")
        || n.starts_with("REG_")
        || n.starts_with("OWN_")
        || n.starts_with("gMPlayInfo_")
        || n.starts_with("FLAG_SCENE_TEMP_")
        || n.starts_with("FLAG_MAPSEC_TEMP_")
        || n.starts_with("FLAG_AREA_TEMP_")
        || name.file.ends_with("/io_reg.h")
        || (n.starts_with("Msg") && matches!(name.kind, NameKind::Global | NameKind::Extern))
        || n.len() < 2
}

/// The first segment of a name with `_`, without pret's `g` or `s`: the
/// family prefix `Battle` of `Battle_Run` and `gBattle_State`,
/// `FLAG` of `FLAG_CHILDHOOD`.
fn prefix(name: &str) -> Option<&str> {
    let (first, rest) = name.split_once('_')?;
    if first.is_empty() || rest.is_empty() {
        return None;
    }
    let bytes = first.as_bytes();
    let pret = bytes.len() > 1 && matches!(bytes[0], b'g' | b's') && bytes[1].is_ascii_uppercase();
    Some(if pret { &first[1..] } else { first })
}

/// After the family prefix: `Run` of `Battle_Run`.
fn rest(name: &str) -> &str {
    name.split_once('_').map_or(name, |(_, rest)| rest)
}

/// How a name is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Style {
    /// `CAPITALS_WITH_UNDERSCORES`.
    Upper,
    /// `PascalCase`.
    Pascal,
    /// `Pascal_Segments`.
    Segments,
    /// `camelCase`.
    Camel,
    /// `snake_case`.
    Snake,
    /// One word in small letters: `count`, either camel or snake case.
    Word,
    /// Fewer than two letters (`X`, `2`): a coordinate or a number, which
    /// has no case to compare.
    Any,
    Mixed,
}

impl Style {
    fn of(text: &str) -> Style {
        // A segment without letters (`8_8`, a Q format) has no case.
        let lettered: Vec<&str> = text
            .split('_')
            .filter(|segment| segment.bytes().any(|b| b.is_ascii_alphabetic()))
            .collect();
        let joined = lettered.join("_");
        let text = joined.as_str();
        if text.bytes().filter(u8::is_ascii_alphabetic).count() < 2 {
            return Style::Any;
        }
        let upper = text.bytes().any(|b| b.is_ascii_uppercase());
        let lower = text.bytes().any(|b| b.is_ascii_lowercase());
        let first = text.bytes().find(u8::is_ascii_alphabetic).unwrap_or(b'_');
        let underscored = text.trim_matches('_').contains('_');
        match (upper, lower, underscored) {
            (true, false, _) => Style::Upper,
            (false, true, true) => Style::Snake,
            (false, _, false) => Style::Word,
            (true, true, false) if first.is_ascii_uppercase() => Style::Pascal,
            (true, true, false) => Style::Camel,
            (true, true, true)
                if text
                    .split('_')
                    .all(|s| s.bytes().next().is_none_or(|b| !b.is_ascii_lowercase())) =>
            {
                Style::Segments
            }
            _ => Style::Mixed,
        }
    }

    fn agrees(self, other: Style) -> bool {
        self == other
            || self == Style::Any
            || other == Style::Any
            || matches!(
                (self, other),
                (Style::Word, Style::Camel | Style::Snake)
                    | (Style::Camel | Style::Snake, Style::Word)
            )
    }

    fn describe(self) -> &'static str {
        match self {
            Style::Upper => "CAPITALS",
            Style::Pascal => "PascalCase",
            Style::Segments => "Pascal_Segments",
            Style::Camel => "camelCase",
            Style::Snake => "snake_case",
            Style::Word => "one small word",
            Style::Any => "a letter or number",
            Style::Mixed => "mixed case",
        }
    }
}

/// A name rewritten with one word replaced, in the name's own case.
fn replace_word(name: &str, word: &str, with: &str) -> String {
    let pieces = provenance::pieces(name);
    let mut out = String::new();
    let mut at = 0;
    for piece in pieces {
        let Some(found) = name[at..].find(piece).map(|offset| at + offset) else {
            continue;
        };
        out.push_str(&name[at..found]);
        if piece.eq_ignore_ascii_case(word) {
            let bytes = piece.as_bytes();
            if bytes.iter().all(|b| !b.is_ascii_lowercase()) {
                out.push_str(&with.to_ascii_uppercase());
            } else if bytes[0].is_ascii_uppercase() {
                out.push(with.as_bytes()[0].to_ascii_uppercase() as char);
                out.push_str(&with[1..]);
            } else {
                out.push_str(with);
            }
        } else {
            out.push_str(piece);
        }
        at = found + piece.len();
    }
    out.push_str(&name[at..]);
    out
}

/// Each distinct name once, where it is defined if it is (a function before
/// its prototypes, a global before its `extern`s), else first declared; the
/// names the checks leave alone are out.
fn distinct(names: &[Name]) -> Vec<&Name> {
    let mut best: BTreeMap<&str, &Name> = BTreeMap::new();
    for name in names.iter().filter(|name| !exempt(name)) {
        let entry = best.entry(name.name.as_str()).or_insert(name);
        if name.kind < entry.kind {
            *entry = name;
        }
    }
    let mut out: Vec<&Name> = best.into_values().collect();
    out.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.line.cmp(&b.line))
            .then(a.name.cmp(&b.name))
    });
    out
}

fn finding(metric: &'static str, name: &Name, what: String) -> Finding {
    Finding {
        metric,
        file: name.file.clone(),
        line: name.line,
        what: format!("{}\t{what}", name.name),
    }
}

/// Run every check on one game's names.
pub fn check(names: &Names, text: Option<&GameText>) -> Vec<Finding> {
    let all: Vec<&Name> = distinct(&names.names)
        .into_iter()
        .filter(|name| !names.pret.contains(&name.name))
        .collect();
    let mut out = Vec::new();
    let usage = Usage::new(&all);
    let families = collect_families(&all);
    let runs = Runs::new(&all);
    let hardware = hardware_segments(&names.names);
    let twins = twin_prefixes(&all, &hardware, text, &names.assets, &mut out);
    words_of_one_idea(&all, &usage, &runs, &twins, &hardware, text, &mut out);
    referents(names, &mut out);
    shapes(&all, &families, text, &mut out);
    measures(&all, &mut out);
    out
}

/// A name's `_` segments, pret's `g` or `s` taken off the first.
fn segments(name: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = name.split('_').collect();
    let several = parts.len() > 1;
    if let Some(first) = parts.first_mut() {
        let bytes = first.as_bytes();
        if several
            && bytes.len() > 1
            && matches!(bytes[0], b'g' | b's')
            && bytes[1].is_ascii_uppercase()
        {
            *first = &first[1..];
        }
    }
    parts
}

/// How many names start with each run of leading segments (`BATTLE`,
/// `BATTLE_FX`), the runs a family's members share.
struct Runs(HashMap<String, usize>);

impl Runs {
    fn new(all: &[&Name]) -> Runs {
        let mut counts = HashMap::new();
        for name in all {
            let parts = segments(&name.name);
            for depth in 1..parts.len() {
                *counts.entry(parts[..depth].join("_")).or_default() += 1;
            }
        }
        Runs(counts)
    }

    /// The shortest run of a name's leading segments that holds its segment
    /// `at` and that at least `FAMILY` names share, with how many: the lead
    /// a word there is written under (`BATTLE_FX` for the `FX` of
    /// `BATTLE_FX_NOVA`).
    fn lead(&self, name: &str, at: usize) -> Option<(String, usize)> {
        let parts = segments(name);
        (at + 1 < parts.len())
            .then(|| parts[..=at].join("_"))
            .and_then(|run| {
                let count = self.0.get(&run).copied().unwrap_or(0);
                (count >= FAMILY).then_some((run, count))
            })
    }
}

/// The segments of the hardware's register names (`io_reg.h`): `BG1CNT`,
/// `OBJ`, `DISPCNT`, which a name may repeat as they are.
fn hardware_segments(names: &[Name]) -> HashSet<String> {
    names
        .iter()
        .filter(|name| name.file.ends_with("/io_reg.h"))
        .flat_map(|name| name.name.split('_').map(str::to_string))
        .collect()
}

/// Which names use each word, by meaning-free spelling.
struct Usage<'a> {
    /// word -> the names using it, in file order.
    by_word: HashMap<String, Vec<&'a Name>>,
    /// singular -> how many names use the word, singular or plural.
    by_singular: HashMap<String, usize>,
}

impl<'a> Usage<'a> {
    fn new(all: &[&'a Name]) -> Usage<'a> {
        let (mut by_word, mut by_singular) = (HashMap::<String, Vec<&Name>>::new(), HashMap::new());
        for &name in all {
            let mut seen = HashSet::new();
            for word in words(&name.name) {
                if seen.insert(word.clone()) {
                    *by_singular.entry(singular(&word)).or_default() += 1;
                    by_word.entry(word).or_default().push(name);
                }
            }
        }
        Usage {
            by_word,
            by_singular,
        }
    }

    /// How many names spell a word, singular or plural.
    fn count(&self, word: &str) -> usize {
        self.by_singular.get(&singular(word)).copied().unwrap_or(0)
    }

    /// The names using a word, singular or plural.
    fn users(&self, word: &str) -> Vec<&'a Name> {
        let target = singular(word);
        let mut users: Vec<&Name> = self
            .by_word
            .iter()
            .filter(|(w, _)| singular(w) == target)
            .flat_map(|(_, names)| names.iter().copied())
            .collect();
        users.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
        users
    }
}

/// A sibling to cite: one in the same family if there is one, else the
/// first name in the game that isn't a struct member.
fn sibling<'a>(name: &Name, users: &[&'a Name]) -> Option<&'a Name> {
    let family = prefix(&name.name);
    let others = || users.iter().filter(|other| other.name != name.name);
    others()
        .find(|other| family.is_some() && prefix(&other.name) == family)
        .or_else(|| others().find(|other| other.kind != NameKind::Field))
        .or_else(|| others().next())
        .copied()
}

/// Whether a word is SGL's `FX` suffix, which S3 sets for a routine's or a
/// value's 16.16 fixed-point form (`Math_MulFX`, `Actor_SetPositionFX`,
/// `IRIS_HALF_PIXELS_FX`), not a shortened `effect`: `FX` in capitals in a
/// name of mixed case, or a constant's last segment. A constant's `FX`
/// elsewhere (`BATTLE_FX_NOVA`), and `Fx`, are still measured.
fn is_sgl_suffix(piece: &str, upper: bool, segment: &str, last: bool) -> bool {
    piece == "FX" && (!upper || (last && segment == "FX"))
}

/// `abbreviation` and `synonym`: a word with a second spelling in the game.
/// A word in a family's lead (its prefix, or its enum's shared leading
/// segments) is reported once for the lead, at its first name, rather than
/// at each of its members, and not at all where `prefix_twin` already
/// reports the prefix.
fn words_of_one_idea(
    all: &[&Name],
    usage: &Usage,
    runs: &Runs,
    twins: &HashSet<String>,
    hardware: &HashSet<String>,
    text: Option<&GameText>,
    out: &mut Vec<Finding>,
) {
    // The word each SPELLINGS set prefers, where its names use two or more.
    let mut preferred: HashMap<String, &str> = HashMap::new();
    // How many names spell a word, its shortened forms included.
    let spelled = |word: &str| {
        usage.count(word)
            + ABBREVIATIONS
                .iter()
                .filter(|(_, full)| *full == word)
                .map(|(short, _)| usage.count(short))
                .sum::<usize>()
    };
    for &(whose, set) in SPELLINGS {
        let used: Vec<&str> = set
            .iter()
            .copied()
            .filter(|word| usage.count(word) > 0)
            .collect();
        if used.len() < 2 {
            continue;
        }
        let spoken: Vec<&str> = set
            .iter()
            .copied()
            .filter(|word| text.is_some_and(|text| text.uses(word)))
            .collect();
        let pool = match (whose, spoken.len()) {
            (Words::Game, 1) => &spoken,
            (Words::Game, 0) | (Words::Code, _) => &used,
            (Words::Game, _) => continue,
        };
        let best = pool
            .iter()
            .copied()
            .max_by_key(|word| (spelled(word), std::cmp::Reverse(*word)))
            .expect("a word");
        for word in used.iter().filter(|word| **word != best) {
            preferred.insert(singular(word), best);
        }
    }
    let mut leads_done = HashSet::new();
    for &name in all {
        // Each word with the segment it is in, and as the name writes it.
        let segments_of = segments(&name.name);
        let last = segments_of.len() - 1;
        let placed: Vec<(usize, &str, &str, String)> = segments_of
            .into_iter()
            .enumerate()
            .flat_map(|(at, segment)| {
                provenance::pieces(segment)
                    .into_iter()
                    .map(move |piece| (at, segment, piece, piece.to_ascii_lowercase()))
            })
            .collect();
        let singulars: Vec<String> = placed
            .iter()
            .map(|(_, _, _, word)| singular(word))
            .collect();
        let upper = Style::of(&name.name) == Style::Upper;
        for (index, (at, segment, piece, word)) in placed.into_iter().enumerate() {
            let (metric, full) = if let Some(&(_, full)) = ABBREVIATIONS
                .iter()
                .find(|(short, _)| *short == singular(&word))
            {
                if in_pret_term(&singulars, index)
                    || is_pret_word(&word)
                    || is_sgl_suffix(piece, upper, segment, at == last)
                    || text.is_some_and(|text| text.uses(&word))
                    || usage.count(full) == 0
                    || hardware.contains(segment)
                {
                    continue;
                }
                ("abbreviation", full)
            } else if let Some(&best) = preferred.get(&singular(&word)) {
                ("synonym", best)
            } else {
                continue;
            };
            let spelled = if singular(&word) == word {
                full.to_string()
            } else {
                plural(full)
            };
            let users = usage.users(full);
            let example = sibling(name, &users)
                .map_or(String::new(), |other| format!(", as {} does", other.name));
            if let Some((family, members)) = runs.lead(&name.name, at) {
                if (at == 0 && twins.contains(&family))
                    || !leads_done.insert((family.clone(), word.clone()))
                {
                    continue;
                }
                let expected = replace_word(&family, &word, &spelled);
                let what = format!(
                    "{word} -> {full}: {expected}_ for the {members} names under {family}_ ({} names spell {full}{example})",
                    users.len()
                );
                out.push(Finding {
                    metric,
                    file: name.file.clone(),
                    line: name.line,
                    what: format!("{family}_\t{what}"),
                });
                continue;
            }
            let what = format!(
                "{word} -> {full}: {} ({} names spell {full}{example})",
                replace_word(&name.name, &word, &spelled),
                users.len()
            );
            out.push(finding(metric, name, what));
        }
    }
}

/// `synonym_referent`: the variables of one struct type, called by
/// different spellings of the type's own name. A variable spells its type
/// when each of its words means one of the type's (`object` and `obj` for
/// `struct Object`, `state` and `fieldState` for `struct FieldState`),
/// or it is the type's initials; names for a role (`leader`, `target`) are
/// not spellings and are left alone, and so are struct members, arrays and
/// plurals, as `prefix_twin` leaves out members: a member names its place in
/// a record, and an array or a pointer named in the plural (`objects`,
/// indexed as the whole battle's objects) a set, not one of the type. A
/// spelling another type owns (it is that type's most used, and that type
/// uses it more: `state`, which `struct FieldState` gives most of its
/// variables and more variables than any other type) is that type's,
/// neither proposed nor measured here, since a `struct FieldEffectState`
/// called `state` would read as the field's. The spelling most variables use
/// is the type's, and each other spelling is listed once, at its first use
/// in a function that doesn't already give that name to another variable,
/// parameter or local of any type.
fn referents(names: &Names, out: &mut Vec<Finding>) {
    let mut by_tag: BTreeMap<&str, Vec<&Typed>> = BTreeMap::new();
    // The names each declaration (a function, with its parameters) uses.
    let mut scopes: HashMap<(&str, usize), Vec<&str>> = HashMap::new();
    for ((file, scope), all) in &names.scopes {
        scopes
            .entry((file.as_str(), *scope))
            .or_default()
            .extend(all.iter().map(String::as_str));
    }
    for typed in &names.typed {
        if let Some(scope) = typed.scope {
            scopes
                .entry((typed.file.as_str(), scope))
                .or_default()
                .push(typed.name.as_str());
        }
        let bytes = typed.name.as_bytes();
        let pret =
            bytes.len() > 1 && matches!(bytes[0], b'g' | b's') && bytes[1].is_ascii_uppercase();
        let plural = words(&typed.name)
            .last()
            .is_some_and(|word| singular(word) != *word);
        if typed.member
            || typed.array
            || plural
            || pret
            || super::is_placeholder(&typed.name)
            || !bytes[0].is_ascii_lowercase()
        {
            continue;
        }
        by_tag.entry(typed.tag.as_str()).or_default().push(typed);
    }
    // Each type's spellings (its words' meanings) and the variables using each.
    let mut spellings_of: BTreeMap<&str, BTreeMap<String, Vec<&Typed>>> = BTreeMap::new();
    for (tag, variables) in by_tag {
        let type_words: HashSet<String> = words(tag).iter().map(|word| meaning(word)).collect();
        let initials: String = words(tag)
            .iter()
            .filter_map(|word| word.chars().next())
            .collect();
        // Each spelling (its words' meanings) and the variables using it.
        let mut spellings: BTreeMap<String, Vec<&Typed>> = BTreeMap::new();
        for typed in variables {
            let name_words = words(&typed.name);
            let spells = if name_words.is_empty() || typed.name.eq_ignore_ascii_case(&initials) {
                typed.name.len() > 1 && typed.name.eq_ignore_ascii_case(&initials)
            } else {
                name_words
                    .iter()
                    .all(|word| type_words.contains(&meaning(word)))
            };
            if spells {
                let key: Vec<String> = provenance::pieces(&typed.name)
                    .iter()
                    .map(|word| singular(word))
                    .collect();
                let key = if key.is_empty() {
                    typed.name.to_ascii_lowercase()
                } else {
                    key.join(" ")
                };
                spellings.entry(key).or_default().push(typed);
            }
        }
        spellings_of.insert(tag, spellings);
    }
    // The type each spelling is the most used of, with how many use it there.
    let mut owners: HashMap<String, (&str, usize)> = HashMap::new();
    for (tag, spellings) in &spellings_of {
        if let Some((spelling, uses)) = spellings
            .iter()
            .max_by_key(|(spelling, uses)| (uses.len(), std::cmp::Reverse(spelling.len())))
        {
            let owner = owners.entry(spelling.clone()).or_insert((tag, uses.len()));
            if uses.len() > owner.1 {
                *owner = (tag, uses.len());
            }
        }
    }
    for (tag, mut spellings) in spellings_of {
        spellings.retain(|spelling, uses| {
            owners
                .get(spelling)
                .is_none_or(|&(owner, count)| owner == tag || count <= uses.len())
        });
        if spellings.len() < 2 {
            continue;
        }
        let (_, most) = spellings
            .iter()
            .max_by_key(|(spelling, uses)| (uses.len(), std::cmp::Reverse(spelling.len())))
            .expect("two");
        // Its most used written form, the singular where the counts tie.
        let mut forms: BTreeMap<&str, usize> = BTreeMap::new();
        for typed in most {
            *forms.entry(typed.name.as_str()).or_default() += 1;
        }
        let best = forms
            .iter()
            .max_by_key(|(form, count)| (**count, std::cmp::Reverse(form.len())))
            .map(|(form, _)| form.to_string())
            .expect("a form");
        let most = most.len();
        if most < FAMILY {
            continue;
        }
        for uses in spellings.values() {
            if uses.len() >= most {
                continue;
            }
            // Where its function already calls another variable `best`, the
            // rename would clash.
            let taken = |typed: &Typed| {
                typed
                    .scope
                    .and_then(|scope| scopes.get(&(typed.file.as_str(), scope)))
                    .is_some_and(|others| others.contains(&best.as_str()))
            };
            let Some(first) = uses.iter().copied().find(|typed| !taken(typed)) else {
                continue;
            };
            let what = format!("struct {tag}: {} uses -> {best} ({most} uses)", uses.len());
            out.push(Finding {
                metric: "synonym_referent",
                file: first.file.clone(),
                line: first.line,
                what: format!("{}\t{what}", first.name),
            });
        }
    }
}

/// A family's members and the part of each the checks compare.
struct Family<'a> {
    label: String,
    /// An enum's or a struct's group key; `None` for a prefix family.
    group: Option<&'a str>,
    members: Vec<(&'a Name, &'a str)>,
}

/// Prefix families (functions by `Prefix_`, constants by their first
/// segment) and each enum's enumerators and each struct's members.
fn collect_families<'a>(all: &[&'a Name]) -> Vec<Family<'a>> {
    let mut by_prefix: BTreeMap<(bool, &str), Vec<(&Name, &str)>> = BTreeMap::new();
    let mut by_group: BTreeMap<(&str, bool), Vec<(&Name, &str)>> = BTreeMap::new();
    for &name in all {
        let function = matches!(name.kind, NameKind::Function | NameKind::Prototype);
        let constant = matches!(
            name.kind,
            NameKind::Macro
                | NameKind::FunctionMacro
                | NameKind::Enumerator
                | NameKind::Extern
                | NameKind::Global
        );
        if let Some(family) = prefix(&name.name).filter(|_| function || constant) {
            if !MEASURE_WORDS.contains(&family.to_ascii_lowercase().as_str()) {
                by_prefix
                    .entry((function, family))
                    .or_default()
                    .push((name, rest(&name.name)));
            }
        }
        if let Some(group) = &name.group {
            by_group
                .entry((group.as_str(), name.kind == NameKind::Field))
                .or_default()
                .push((name, name.name.as_str()));
        }
    }
    let mut families = Vec::new();
    for ((function, family), members) in by_prefix {
        let kind = if function { "functions" } else { "names" };
        families.push(Family {
            label: format!("{family}_ {kind}"),
            group: None,
            members,
        });
    }
    for ((group, field), members) in by_group {
        let what = if field { "struct" } else { "enum" };
        let name = group.rsplit('/').next().unwrap_or(group);
        families.push(Family {
            label: format!("{what} {name}"),
            group: Some(group),
            members,
        });
    }
    families.retain(|family| family.members.len() >= FAMILY);
    families
}

/// The longest run of a family's leading segments that all its members but
/// one in ten (or one, in a small family) share: `FLAG_VALE` for an enum of
/// Vale's flags. It stops before a door word (`TO`, `FROM`, `AFTER`, a
/// compass point), where a link's own part begins (`VALE_FROM_` and
/// `VALE_TO_` are both Vale's links).
fn common_prefix(members: &[(&Name, &str)]) -> Option<String> {
    let allowed = (members.len() / 10).max(1);
    let mut best = None;
    for depth in 1.. {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for (_, part) in members {
            let segments: Vec<&str> = part.split('_').collect();
            let door = segments.get(depth - 1).is_some_and(|segment| {
                provenance::DOOR_WORDS.contains(&segment.to_ascii_lowercase().as_str())
            });
            if segments.len() > depth && !door {
                *counts.entry(segments[..depth].join("_")).or_default() += 1;
            }
        }
        match counts
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
        {
            // Past the first segment, a run needs five members to be a pattern.
            Some((common, count))
                if count >= if depth == 1 { FAMILY } else { 5 }
                    && count + allowed >= members.len() =>
            {
                best = Some(common)
            }
            _ => return best,
        }
    }
    best
}

/// The area names' words, for finding a place in a name: `Kolima Forest` is
/// `kolima`, `forest`. Only proper names count, every word capitalized but
/// short ones (`Road to Babi Lighthouse`) and no number, which leaves out the debug rooms' labels (`Test 0`,
/// `Menu test`, `Title screen`). A compass point the text writes as letters
/// is its word (`E Tundaria Islet` is `east`, `tundaria`, `islet`, as
/// `AREA_EAST_TUNDARIA_ISLET` writes it), and an area of one word that ends
/// other areas' names (`Sanctum`, beside `Vale Sanctum` and `Sol Sanctum`)
/// names a kind of place, not one place, so it is left out.
fn area_words(text: Option<&GameText>) -> Vec<Vec<String>> {
    let proper = |area: &str| {
        let capitalized = |word: &str| word.chars().next().is_some_and(|c| c.is_ascii_uppercase());
        let mut words = area.split([' ', '-']);
        words.next().is_some_and(capitalized)
            && words.all(|word| capitalized(word) || word.len() < 4)
            && !area.chars().any(|c| c.is_ascii_digit())
    };
    let areas: BTreeSet<Vec<String>> = text
        .map(|text| {
            text.areas()
                .iter()
                .filter(|area| proper(area))
                .map(|area| {
                    let spelled: Vec<&str> = area
                        .split(' ')
                        .map(|word| compass(word).unwrap_or(word))
                        .collect();
                    words(
                        &spelled
                            .join(" ")
                            .replace(|c: char| !c.is_ascii_alphanumeric(), "_"),
                    )
                })
                .filter(|w| !w.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let kinds: BTreeSet<&String> = areas
        .iter()
        .filter(|area| area.len() == 1)
        .filter(|one| {
            areas
                .iter()
                .any(|area| area.len() > 1 && area.last() == one.last())
        })
        .map(|one| &one[0])
        .collect();
    let mut areas: Vec<Vec<String>> = areas
        .iter()
        .filter(|area| !(area.len() == 1 && kinds.contains(&area[0])))
        .cloned()
        .collect();
    areas.sort_by_key(|area| std::cmp::Reverse(area.len()));
    areas
}

/// The compass point a word of letters writes (`SE`), spelled out.
fn compass(word: &str) -> Option<&'static str> {
    let points = [
        ("N", "North"),
        ("S", "South"),
        ("E", "East"),
        ("W", "West"),
        ("NE", "Northeast"),
        ("NW", "Northwest"),
        ("SE", "Southeast"),
        ("SW", "Southwest"),
    ];
    points
        .iter()
        .find(|(short, _)| *short == word)
        .map(|(_, full)| *full)
}

/// Whether a prefix is a place's name, or the start of one (`Fuchin` of
/// `Fuchin Falls Cave`).
fn is_place(family: &str, areas: &[Vec<String>]) -> bool {
    let family_words = words(family);
    !family_words.is_empty()
        && areas
            .iter()
            .any(|area| area.starts_with(&family_words) || family_words.starts_with(area))
}

/// Where in a name's words an area name starts, if it has one.
fn place_at(name_words: &[String], areas: &[Vec<String>]) -> Option<(usize, String)> {
    (0..name_words.len()).find_map(|at| {
        areas
            .iter()
            .find(|area| name_words[at..].starts_with(area))
            .map(|area| (at, area.join("_").to_ascii_uppercase()))
    })
}

/// Whether a member that lacks its enum's shared lead names a place of its
/// own where the lead names one: a scene file serving two places
/// (`CELL_BEHAVIOR_IMIL_FALLS_CAVE_ICE` beside `CELL_BEHAVIOR_FUCHIN_FALLS_`)
/// starts with its own place, as its siblings start with theirs.
fn own_place(part: &str, common: &str, areas: &[Vec<String>]) -> bool {
    let (part_segments, common_segments): (Vec<&str>, Vec<&str>) =
        (part.split('_').collect(), common.split('_').collect());
    let shared = part_segments
        .iter()
        .zip(&common_segments)
        .take_while(|(a, b)| a == b)
        .count();
    let (theirs, own) = (
        common_segments[shared..].join("_"),
        words(&part_segments[shared..].join("_")),
    );
    shared < common_segments.len()
        && is_place(&theirs, areas)
        && matches!(place_at(&own, areas), Some((0, _)))
}

/// `family_shape`: members whose shape the rest of their family doesn't
/// share. A family has at least `FAMILY` members, and its pattern is what
/// two thirds of them do:
///
/// - case: `SceneId_Vale` beside `SceneId_VALE_CAVE`;
/// - an enum's shared leading segments (`FLAG_VALE_`) that one of its
///   members lacks;
/// - a function named with a family's prefix but no `_` (`BattleObjGet`
///   beside `BattleObj_Init`);
/// - a place: in a family whose members mostly start with an area name
///   (`FLAG_VALE_NUT_CAUGHT`), a member naming its place later
///   (`FLAG_SAW_SHIP_AT_IDEJIMA`).
fn shapes(all: &[&Name], families: &[Family], text: Option<&GameText>, out: &mut Vec<Finding>) {
    let areas = area_words(text);
    let mut flagged: HashSet<(String, &'static str)> = HashSet::new();
    let mut note = |out: &mut Vec<Finding>, name: &Name, rule: &'static str, what: String| {
        if flagged.insert((name.name.clone(), rule)) {
            out.push(finding("family_shape", name, what));
        }
    };
    for family in families {
        let members = &family.members;
        let size = members.len();
        // Case.
        let mut styles: BTreeMap<Style, usize> = BTreeMap::new();
        for (_, part) in members {
            *styles.entry(Style::of(part)).or_default() += 1;
        }
        let (&style, _) = styles
            .iter()
            .max_by_key(|(style, count)| (**count, std::cmp::Reverse(**style)))
            .expect("members");
        let agreeing = members
            .iter()
            .filter(|(_, part)| Style::of(part).agrees(style))
            .count();
        if agreeing * 3 >= size * 2 {
            let example = members
                .iter()
                .find(|(_, part)| Style::of(part) == style)
                .map_or("", |(name, _)| name.name.as_str());
            for (name, part) in members {
                let own = Style::of(part);
                if !own.agrees(style) {
                    let what = format!(
                        "case: {} where the {} are {} ({agreeing} of {size}, as {example})",
                        own.describe(),
                        family.label,
                        style.describe()
                    );
                    note(out, name, "case", what);
                }
            }
        }
        // An enum's shared leading segments. An enum of actor ids numbers
        // one room's actors, each named for what it is, so its family is
        // `ACTOR_` alone, whatever a run of one kind inside it shares
        // (`ACTOR_STONE_FACE_A` to `_F` beside `ACTOR_NUT_CHEST_PILLAR`).
        if family.label.starts_with("enum ") {
            if let Some(common) =
                common_prefix(members).filter(|common| !common.starts_with("ACTOR_"))
            {
                let lead = format!("{common}_");
                let sharing = members
                    .iter()
                    .filter(|(_, part)| part.starts_with(&lead))
                    .count();
                let example = members
                    .iter()
                    .find(|(_, part)| part.starts_with(&lead))
                    .map_or("", |(name, _)| name.name.as_str());
                for (name, part) in members {
                    // The family's base (`CANVAS_COPY` beside `CANVAS_COPY_AND_CLEAR`) has its own shape.
                    let base = common.starts_with(part)
                        && common.as_bytes().get(part.len()) == Some(&b'_');
                    if !part.starts_with(&lead)
                        && *part != common
                        && !base
                        && !own_place(part, &common, &areas)
                    {
                        let what = format!("missing part: {lead}... as {sharing} of the {size} in its enum are (as {example})");
                        note(out, name, "part", what);
                    }
                }
            }
        }
        // A place that isn't first, in a family that puts it first.
        // Only a family whose prefix names a kind (`FLAG_`, `Event_`), not a
        // place (`VALE_`, whose links all start with Vale).
        let kind_prefix = family.group.is_none()
            && !family
                .label
                .split('_')
                .next()
                .is_some_and(|prefix| is_place(prefix, &areas));
        if !areas.is_empty() && kind_prefix {
            let placed: Vec<(usize, &Name, Option<(usize, String)>)> = members
                .iter()
                .map(|(name, part)| {
                    let part_words = words(part);
                    (part_words.len(), *name, place_at(&part_words, &areas))
                })
                .collect();
            let first = placed
                .iter()
                .filter(|(_, _, place)| matches!(place, Some((0, _))))
                .count();
            if first >= 5 && first * 2 >= size {
                let example = placed
                    .iter()
                    .find(|(_, _, place)| matches!(place, Some((0, _))))
                    .map_or("", |(_, name, _)| name.name.as_str());
                for (_, name, place) in &placed {
                    // S3 puts `Unused` right after the prefix (`SceneId_UnusedMercuryLighthouseAerie2`).
                    let unused = rest(&name.name).to_ascii_lowercase().starts_with("unused");
                    if let Some((_, area)) = place.as_ref().filter(|(at, _)| *at > 0 && !unused) {
                        let family_prefix = prefix(&name.name).unwrap_or("");
                        let area = if Style::of(rest(&name.name)) == Style::Upper {
                            format!("{area}_")
                        } else {
                            area.split('_')
                                .map(|word| word[..1].to_string() + &word[1..].to_ascii_lowercase())
                                .collect()
                        };
                        let what = format!(
                            "place last: {family_prefix}_{area}... as {first} of the {size} {} start with their place (as {example})",
                            family.label
                        );
                        note(out, name, "place", what);
                    }
                }
            }
        }
    }
    // A family's prefix written without its `_`.
    let function_prefixes: BTreeMap<&str, usize> = families
        .iter()
        .filter(|family| family.label.ends_with(" functions"))
        .filter_map(|family| {
            family
                .label
                .strip_suffix("_ functions")
                .map(|prefix| (prefix, family.members.len()))
        })
        .collect();
    for &name in all.iter().filter(|name| {
        matches!(name.kind, NameKind::Function | NameKind::Prototype) && !name.name.contains('_')
    }) {
        let n = name.name.as_str();
        // A routine resident in IWRAM, which C only declares, is named by its
        // linked symbol, `IwramName`; `Iwram_Name` is its entry at the fixed
        // address (`IwramMulFX` beside `Iwram_MulFX`), so the two differ on purpose.
        if name.kind == NameKind::Prototype && n.starts_with("Iwram") {
            continue;
        }
        let found = function_prefixes
            .iter()
            .filter(|(prefix, _)| {
                n.len() > prefix.len()
                    && n.starts_with(**prefix)
                    && n.as_bytes()[prefix.len()].is_ascii_uppercase()
            })
            .max_by_key(|(prefix, _)| prefix.len());
        if let Some((prefix, members)) = found {
            let what =
                format!("separator: {prefix}_{}, as the {members} {prefix}_ functions are, or a prefix of its own", &n[prefix.len()..]);
            note(out, name, "separator", what);
        }
    }
}

/// `family_shape`, game-wide: a count or bound (`MEASURE_WORDS`) first in a
/// name where the game's other names put it last, or the other way round.
/// Functions and the macros written as calls are left out, since there
/// `Count` is a verb (`COUNT_ALL_CURES(unit, count)`), and so are a name of
/// two things joined by `and` (`countAndShadow`, which holds a count and
/// then a shadow) and a measure word with a number joined to it, which names
/// an operation (`MAX4_WORDS`, keep the brightest of four, beside
/// `COPY8_WORDS`).
fn measures(all: &[&Name], out: &mut Vec<Finding>) {
    let meanings: BTreeSet<String> = MEASURE_WORDS.iter().map(|word| meaning(word)).collect();
    let operation = |name: &str| {
        name.split('_').any(|segment| {
            let letters = segment.trim_end_matches(|c: char| c.is_ascii_digit());
            letters.len() < segment.len()
                && MEASURE_WORDS.contains(&letters.to_ascii_lowercase().as_str())
        })
    };
    for measure in &meanings {
        let (mut leading, mut trailing) = (Vec::new(), Vec::new());
        for &name in all.iter().filter(|name| {
            !matches!(
                name.kind,
                NameKind::Function | NameKind::Prototype | NameKind::FunctionMacro
            )
        }) {
            let name_words = words(&name.name);
            if name_words.len() < 2
                || name_words.iter().any(|word| word == "and")
                || operation(&name.name)
            {
                continue;
            }
            if meaning(&name_words[0]) == *measure {
                leading.push((name, name_words[0].clone()));
            } else if name_words
                .last()
                .is_some_and(|word| meaning(word) == *measure)
            {
                trailing.push((name, name_words[name_words.len() - 1].clone()));
            }
        }
        let (side, odd, usual) = if trailing.len() >= leading.len() {
            ("last", &leading, &trailing)
        } else {
            ("first", &trailing, &leading)
        };
        if usual.len() < 5 || usual.len() * 3 < (usual.len() + odd.len()) * 2 {
            continue;
        }
        for (name, word) in odd {
            let total = usual.len() + odd.len();
            let what = format!(
                "order: {word} goes {side}, where {} of the game's {total} names with it at an end put it (as {})",
                usual.len(),
                usual[0].0.name
            );
            out.push(finding("family_shape", name, what));
        }
    }
}

/// `prefix_twin`: two prefixes for one idea, their words the same once each
/// shortened word is spelled out, each `SPELLINGS` word is its set's and
/// plurals are singular (`BattleObj_` and `BattleObject_`, `ANIM_` and
/// `ANIMATION_`, `STREAKS_` and `STREAK_`). Prefixes that differ
/// only in case (`Battle_` and `BATTLE_`) are one spelling, struct members
/// are left out, so are macros written as calls, which may start with a
/// verb (`COUNT_ALL_CURES` counts, beside pret's `NUM_MUSIC_PLAYERS`), and so is a shortened word that is a hardware register's
/// segment (`OBJ_` of `OBJ_AFFINE_MATRIX_COUNT`, the hardware's OBJ, not a
/// shortened object). A plural prefix whose names are all pret's
/// (`PRET_PLURALS`: `PALETTES_ALL` beside `PALETTE_`) is pret's set and left
/// out. Where the game's text uses two words of a `SPELLINGS` set of its
/// own (`monster` and `enemy`, `opponent` and `rival`), it tells them apart,
/// so their prefixes are two ideas. A plural is no second spelling where
/// English or the game writes one: a rate (`SECONDS_PER_MINUTE` beside
/// `SECOND_`, whose names `_PER_` are left out of their prefix), a function
/// prefix that walks a list (`Actors_SetCellsOpen` beside `Actor_SetCellOpen`),
/// a picture's own name (`CRATES_` of `sprite/crates.png`) and a proper
/// name's possessive (`ISAACS_`, Isaac's bed, where the text writes Isaac
/// only capitalized); nor where one of the two is a place's name or the
/// start of one (`ISLET_` of Islet Cave beside the Islets' `ISLETS_`), two
/// places. The spelling fewer names use is reported, at its first name;
/// the set of those prefixes is returned.
fn twin_prefixes(
    all: &[&Name],
    hardware: &HashSet<String>,
    text: Option<&GameText>,
    assets: &HashSet<String>,
    out: &mut Vec<Finding>,
) -> HashSet<String> {
    let mut by_prefix: BTreeMap<&str, Vec<&Name>> = BTreeMap::new();
    for &name in all
        .iter()
        .filter(|name| !matches!(name.kind, NameKind::Field | NameKind::FunctionMacro))
    {
        if let Some(family) = prefix(&name.name)
            .filter(|family| family.starts_with(|c: char| c.is_ascii_uppercase()))
            .filter(|family| {
                !(hardware.contains(*family)
                    && ABBREVIATIONS
                        .iter()
                        .any(|(short, _)| family.eq_ignore_ascii_case(short)))
            })
        {
            by_prefix.entry(family).or_default().push(name);
        }
    }
    let rate = |name: &Name| {
        segments(&name.name)
            .get(1)
            .is_some_and(|segment| segment.eq_ignore_ascii_case("per"))
    };
    // The last word of each scene's name (`grounds` of
    // `SceneId_KibomboStatueGrounds`): a place's own word.
    let scene_ends: HashSet<String> = all
        .iter()
        .filter_map(|name| name.name.strip_prefix("SceneId_"))
        .filter_map(|scene| words(scene).last().cloned())
        .collect();
    // Each segment a name writes after its first, by file (`SPARKS` of
    // `WEST_SPARKS_X`).
    let inner: HashSet<(&str, String)> = all
        .iter()
        .flat_map(|name| {
            segments(&name.name)
                .into_iter()
                .skip(1)
                .map(move |segment| (name.file.as_str(), segment.to_ascii_uppercase()))
        })
        .collect();
    // The plurals set apart as a place's or a group's own word, and why.
    let mut kept: Vec<(&str, &Name, usize, &'static str)> = Vec::new();
    by_prefix.retain(|family, members| {
        let plural = words(family)
            .last()
            .is_some_and(|word| singular(word) != *word);
        if plural {
            members.retain(|name| !rate(name));
        }
        let last = words(family).last().cloned().unwrap_or_default();
        let functions = members
            .iter()
            .all(|name| matches!(name.kind, NameKind::Function | NameKind::Prototype));
        let possessive = text.is_some_and(|text| text.is_proper(&singular(&last)));
        // A plural that ends a place's own name, a scene's or one the text
        // writes (`GROUNDS_` of Kibombo Statue Grounds, `FALLS_` of the
        // falls, as "Gaia Falls"): the place, not many of the singular's
        // (`FALL_`, a falling).
        let place = scene_ends.contains(&last) || text.is_some_and(|text| text.ends_a_name(&last));
        // A plural its own file writes inside its siblings' names
        // (`SPARKS_Z` beside `WEST_SPARKS_X`): the group's word, as the file
        // names the group.
        let group = members
            .iter()
            .all(|name| inner.contains(&(name.file.as_str(), family.to_ascii_uppercase())));
        let other = members
            .iter()
            .all(|name| PRET_PLURALS.contains(&name.name.as_str()))
            || functions
            || assets.contains(&last)
            || possessive;
        if plural && !other && !members.is_empty() && (place || group) {
            kept.push((
                *family,
                members[0],
                members.len(),
                if place {
                    "a place's own word"
                } else {
                    "the group's word in its file"
                },
            ));
        }
        !members.is_empty() && !(plural && (other || place || group))
    });
    // The words of a SPELLINGS set the game's own text tells apart.
    let apart: HashSet<&str> = SPELLINGS
        .iter()
        .filter(|(whose, set)| {
            *whose == Words::Game
                && set
                    .iter()
                    .filter(|word| text.is_some_and(|text| text.uses(word)))
                    .count()
                    > 1
        })
        .flat_map(|(_, set)| set.iter().copied())
        .collect();
    let mut ideas: BTreeMap<Vec<String>, Vec<(&str, &Vec<&Name>)>> = BTreeMap::new();
    for (family, members) in &by_prefix {
        let family_words = words(family);
        if family_words.is_empty() {
            continue;
        }
        let idea: Vec<String> = family_words
            .iter()
            .map(|word| {
                if apart.contains(singular(word).as_str()) {
                    singular(word)
                } else {
                    meaning(word)
                }
            })
            .collect();
        ideas.entry(idea).or_default().push((family, members));
    }
    // Counted apart where a singular spelling is there to twin with.
    for (family, first, count, why) in kept {
        let singular_words: Vec<String> = words(family).iter().map(|word| meaning(word)).collect();
        // The other spellings, those differing only in case taken together.
        let others: Vec<&(&str, &Vec<&Name>)> = ideas
            .get(&singular_words)
            .into_iter()
            .flatten()
            .filter(|(other, _)| words(other) != words(family))
            .collect();
        let total: usize = others.iter().map(|(_, members)| members.len()).sum();
        if let Some((other, _)) = others
            .iter()
            .max_by_key(|(other, members)| (members.len(), std::cmp::Reverse(*other)))
        {
            let what = format!("{count} names; {why}, beside {other}_ ({total} names)");
            out.push(Finding {
                metric: "prefix_twin_apart",
                file: first.file.clone(),
                line: first.line,
                what: format!("{family}_\t{what}"),
            });
        }
    }
    let areas = area_words(text);
    let mut twins = HashSet::new();
    for spellings in ideas.values() {
        let mut distinct: BTreeMap<String, (usize, &str, &Name)> = BTreeMap::new();
        for (family, members) in spellings {
            let key = words(family).join(" ");
            let entry = distinct.entry(key).or_insert((0, family, members[0]));
            entry.0 += members.len();
        }
        if distinct.len() < 2 || spellings.iter().any(|(family, _)| is_place(family, &areas)) {
            continue;
        }
        // pret's spelling where one uses a word of pret's (`ANIM_`, not
        // `ANIMATION_`), else the spelled-out one where they differ by a
        // shortened word (`ACTOR_`, not `ACT_`), else the one more names use.
        let pret = |key: &str| key.split(' ').filter(|word| is_pret_word(word)).count();
        let short = |key: &str| {
            key.split(' ')
                .filter(|word| {
                    !is_pret_word(word)
                        && ABBREVIATIONS
                            .iter()
                            .any(|(short, _)| *short == singular(word))
                })
                .count()
        };
        let (_, &(most, best, _)) = distinct
            .iter()
            .max_by_key(|(key, (count, ..))| {
                (
                    pret(key),
                    std::cmp::Reverse(short(key)),
                    *count,
                    std::cmp::Reverse((*key).clone()),
                )
            })
            .expect("two");
        for (other, _) in spellings {
            twins.insert(other.to_string());
        }
        for &(count, family, first) in distinct.values() {
            if family == best {
                continue;
            }
            let what = format!("{count} names; one idea with {best}_ ({most} names)");
            out.push(Finding {
                metric: "prefix_twin",
                file: first.file.clone(),
                line: first.line,
                what: format!("{family}_\t{what}"),
            });
        }
    }
    twins
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(name: &str, kind: NameKind, file: &str, line: usize) -> Name {
        Name {
            name: name.to_string(),
            kind,
            file: file.to_string(),
            line,
            group: None,
        }
    }

    #[test]
    fn styles() {
        assert_eq!(Style::of("FLAG_VALE"), Style::Upper);
        assert_eq!(Style::of("RunEncounter"), Style::Pascal);
        assert_eq!(Style::of("Vale_Cave"), Style::Segments);
        assert_eq!(Style::of("unitCount"), Style::Camel);
        assert_eq!(Style::of("unit_count"), Style::Snake);
        assert!(Style::of("count").agrees(Style::Camel));
        assert!(!Style::of("Count").agrees(Style::Camel));
        assert_eq!(
            replace_word("NUM_ELEMENTS", "num", "count"),
            "COUNT_ELEMENTS"
        );
        assert_eq!(
            replace_word("BattleObj_GetPos", "pos", "position"),
            "BattleObj_GetPosition"
        );
        assert_eq!(replace_word("objCnt", "cnt", "count"), "objCount");
        assert_eq!(prefix("gBattle_State"), Some("Battle"));
        assert_eq!(prefix("FLAG_VALE"), Some("FLAG"));
        assert_eq!(prefix("Vale"), None);
    }

    #[test]
    fn abbreviations_where_the_game_spells_the_word() {
        let file = "games/tbs/src/battle/a.c";
        let names = Names {
            names: vec![
                name("BattleUnit_GetCnt", NameKind::Function, file, 1),
                name("BattleUnit_GetCount", NameKind::Function, file, 2),
                name("Menu_GetIdx", NameKind::Function, file, 3),
                name("gMsgTable", NameKind::Global, file, 4),
                name("MsgItemName", NameKind::Extern, file, 5),
                name("Message_Show", NameKind::Function, file, 6),
                name("BattleObj_Init", NameKind::Function, file, 7),
                name("BattleObj_Free", NameKind::Function, file, 8),
                name("BattleObj_Draw", NameKind::Function, file, 9),
                name("Field_FindObject", NameKind::Function, file, 10),
                name("BATTLE_BG1CNT", NameKind::Macro, file, 11),
                name(
                    "REG_BG1CNT",
                    NameKind::Macro,
                    "games/tbs/include/system/io_reg.h",
                    1,
                ),
            ],
            ..Names::default()
        };
        let findings = check(&names, None);
        let abbreviations: Vec<&str> = findings
            .iter()
            .filter(|f| f.metric == "abbreviation")
            .map(|f| f.what.as_str())
            .collect();
        assert_eq!(
            abbreviations,
            [
                "BattleUnit_GetCnt\tcnt -> count: BattleUnit_GetCount (1 names spell count, as BattleUnit_GetCount does)",
                "gMsgTable\tmsg -> message: gMessageTable (1 names spell message, as Message_Show does)",
                "BattleObj_\tobj -> object: BattleObject_ for the 3 names under BattleObj_ (1 names spell object, as Field_FindObject does)",
            ],
            "Idx: nothing spells index; MsgItemName: a message symbol; BattleObj_ once for its prefix; BG1CNT: the hardware's"
        );
    }

    #[test]
    fn pret_terms_keep_their_short_words() {
        let file = "games/tbs/src/scene/a.c";
        let names = Names {
            names: vec![
                name("Scene_GetWarpDests", NameKind::Function, file, 1),
                name("MAP_EVENT_COORD", NameKind::Macro, file, 2),
                name("Scene_RunCoordEventOrWarp", NameKind::Function, file, 3),
                name("gSerialReceiveDest", NameKind::Global, file, 4),
                name("Event_SetDestination", NameKind::Function, file, 5),
                name("Particle_Coordinates", NameKind::Function, file, 6),
            ],
            ..Names::default()
        };
        let findings = check(&names, None);
        let abbreviations: Vec<&str> = findings
            .iter()
            .filter(|f| f.metric == "abbreviation")
            .map(|f| f.what.as_str())
            .collect();
        assert_eq!(
            abbreviations,
            ["gSerialReceiveDest\tdest -> destination: gSerialReceiveDestination (1 names spell destination, as Event_SetDestination does)"],
            "WarpDest and CoordEvent are pret's; a dest outside them is still measured"
        );
    }

    #[test]
    fn names_the_rules_keep() {
        let file = "games/tbs/src/battle/a.c";
        let flash = "games/tbs/src/gamepak/flash/flash.c";
        let mut names = Names {
            names: vec![
                // pret's flash library keeps pret's names.
                name(
                    "gFlashNumRemainingBytes",
                    NameKind::Extern,
                    "games/tbs/include/gamepak/flash.h",
                    1,
                ),
                name(
                    "maxTime",
                    NameKind::Field,
                    "games/tbs/include/gamepak/flash_data.h",
                    2,
                ),
                // An OWN_ guard repeats the symbol it guards.
                name("OWN_gFlashNumRemainingBytes", NameKind::Macro, file, 3),
                // Poses are not positions.
                name("gCybele_ObjectPoses", NameKind::Global, file, 4),
                // A macro written as a call may start with a verb.
                name("COUNT_ALL_CURES", NameKind::FunctionMacro, file, 5),
                // The hardware's OBJ is not a shortened object.
                name("OBJ_AFFINE_MATRIX_COUNT", NameKind::Enumerator, file, 6),
                name(
                    "OBJ_MODE_NORMAL",
                    NameKind::Macro,
                    "games/tbs/include/system/io_reg.h",
                    7,
                ),
                name("OBJECT_SLOT_COUNT", NameKind::Macro, file, 8),
                name("OBJECT_SLOT_FIRST", NameKind::Macro, file, 9),
                name("gHeapRemainingCount", NameKind::Global, file, 11),
                name("Vector3", NameKind::Tag, file, 12),
            ],
            ..Names::default()
        };
        for (at, count) in [
            "ITEM_COUNT",
            "ENEMY_COUNT",
            "UNIT_COUNT",
            "ACTOR_COUNT",
            "SLOT_COUNT",
            "LIST_MAX",
        ]
        .iter()
        .enumerate()
        {
            names.names.push(name(
                count,
                NameKind::Macro,
                "games/tbs/include/counts.h",
                20 + at,
            ));
        }
        let text = "u16 gFlashNumRemainingBytes; const u16 *maxTime; // pret's names";
        assert!(is_pret_flash(flash) && !is_pret_flash(file));
        names.pret.extend(identifiers(text).map(str::to_string));
        let findings: Vec<String> = check(&names, None)
            .into_iter()
            .map(|f| format!("{} {}", f.metric, f.what))
            .collect();
        assert!(findings.is_empty(), "{findings:?}");
        assert_eq!(singular("poses"), "pose");
        assert_eq!(singular("classes"), "class");
        assert_eq!(singular("statuses"), "status");
    }

    #[test]
    fn synonyms_take_the_games_word() {
        let file = "games/tbs/src/scene/a.c";
        let po = "msgctxt \"MsgShop\"\nmsgid \"00001\"\nmsgstr \"I'm a merchant.\"\n";
        let text = GameText::parse(po, None).unwrap();
        let names = Names {
            names: vec![
                name("ACTOR_SHOPKEEPER", NameKind::Enumerator, file, 1),
                name("ACTOR_MERCHANT", NameKind::Enumerator, file, 2),
                name("ACTOR_OLD_MAN", NameKind::Enumerator, file, 3),
            ],
            ..Names::default()
        };
        let findings = check(&names, Some(&text));
        let synonyms: Vec<&str> = findings
            .iter()
            .filter(|f| f.metric == "synonym")
            .map(|f| f.what.as_str())
            .collect();
        assert_eq!(synonyms, ["ACTOR_SHOPKEEPER\tshopkeeper -> merchant: ACTOR_MERCHANT (1 names spell merchant, as ACTOR_MERCHANT does)"]);
    }

    #[test]
    fn one_type_one_name() {
        let typed = |tag: &str, name: &str, line| Typed {
            tag: tag.to_string(),
            name: name.to_string(),
            file: "f.c".to_string(),
            line,
            ..Typed::default()
        };
        let names = Names {
            typed: vec![
                typed("Object", "object", 1),
                typed("Object", "object", 2),
                typed("Object", "object", 3),
                typed("Object", "obj", 3),
                typed("Object", "leader", 4),
                typed("Object", "gActors", 5),
                typed("FieldState", "state", 6),
                typed("FieldState", "fieldState", 7),
                typed("FieldState", "state", 8),
                typed("FieldState", "state", 8),
                typed("FieldState", "states", 10),
                typed("FieldState", "fs", 9),
            ],
            ..Names::default()
        };
        let findings: Vec<String> = check(&names, None)
            .into_iter()
            .map(|f| format!("{}:{} {}", f.file, f.line, f.what))
            .collect();
        assert_eq!(
            findings,
            [
                "f.c:7 fieldState\tstruct FieldState: 1 uses -> state (3 uses)",
                "f.c:9 fs\tstruct FieldState: 1 uses -> state (3 uses)",
                "f.c:3 obj\tstruct Object: 1 uses -> object (3 uses)",
            ],
            "a role (leader) and a global are not spellings of the type; states, named in the plural, names a set"
        );
    }

    #[test]
    fn one_type_one_name_takes_no_spelling_another_type_owns() {
        let typed = |tag: &str, name: &str, line, scope| Typed {
            tag: tag.to_string(),
            name: name.to_string(),
            file: "f.c".to_string(),
            line,
            scope: Some(scope),
            ..Typed::default()
        };
        let mut names = Names {
            typed: vec![
                // `state` is FieldState's.
                typed("FieldState", "state", 1, 1),
                typed("FieldState", "state", 2, 2),
                typed("FieldState", "state", 3, 3),
                typed("FieldState", "state", 4, 4),
                typed("FieldState", "state", 5, 5),
                typed("FieldEffectState", "state", 6, 6),
                typed("FieldEffectState", "state", 7, 7),
                typed("FieldEffectState", "state", 8, 8),
                typed("FieldEffectState", "state", 9, 9),
                typed("FieldEffectState", "effectState", 10, 10),
                typed("FieldEffectState", "effectState", 11, 11),
                typed("FieldEffectState", "effectState", 12, 12),
                typed("FieldEffectState", "fieldEffectState", 13, 13),
                // A plural pointer names the set it is indexed in.
                typed("BattleObject", "battleObject", 20, 20),
                typed("BattleObject", "battleObject", 21, 21),
                typed("BattleObject", "battleObject", 22, 22),
                typed("BattleObject", "objects", 23, 23),
                // `layer` where the function already has an `s32 layer`.
                typed("MapLayer", "layer", 30, 30),
                typed("MapLayer", "layer", 31, 31),
                typed("MapLayer", "layer", 32, 32),
                typed("MapLayer", "mapLayer", 33, 33),
            ],
            ..Names::default()
        };
        names.scopes.insert(
            ("f.c".to_string(), 33),
            ["layer".to_string(), "mapLayer".to_string()].into(),
        );
        let findings: Vec<String> = check(&names, None)
            .into_iter()
            .map(|f| format!("{}:{} {}", f.file, f.line, f.what))
            .collect();
        assert_eq!(
            findings,
            ["f.c:13 fieldEffectState\tstruct FieldEffectState: 1 uses -> effectState (3 uses)"],
            "FieldEffectState's state is FieldState's, neither proposed nor measured; objects names a set; mapLayer's function has a layer"
        );
    }

    #[test]
    fn the_sgl_suffix_and_the_sound_driver() {
        let file = "games/tbs/src/field/a.c";
        let names = Names {
            names: vec![
                name("Actor_SetAttachedEffect", NameKind::Function, file, 1),
                name("Actor_SetPositionFX", NameKind::FunctionMacro, file, 2),
                name("Math_SqrtFXFar", NameKind::Function, file, 3),
                name("IRIS_HALF_PIXELS_FX", NameKind::Macro, file, 4),
                name("BATTLE_FX_NOVA", NameKind::Macro, file, 5),
                name("SoundTest_FxString", NameKind::Global, file, 6),
                name("gMPlayInfo_SE5", NameKind::Extern, file, 7),
                name("gMPlayInfo_ActionSE", NameKind::Extern, file, 8),
                name("gMPlayInfo_Jingle", NameKind::Extern, file, 9),
            ],
            ..Names::default()
        };
        let findings: Vec<String> = check(&names, None)
            .into_iter()
            .map(|f| format!("{} {}", f.metric, f.what.split('\t').next().unwrap()))
            .collect();
        assert_eq!(
            findings,
            ["abbreviation BATTLE_FX_NOVA", "abbreviation SoundTest_FxString"],
            "FX in a mixed-case name or a constant's last segment is SGL's; the music players are the driver's"
        );
        let code = "void SoundMain(void); // Sound_Play calls it\nchar *s = \"Sound_Stop\"; /* Sound_X */ u8 c = 'a'; MPlayMain(p);";
        assert_eq!(
            code_identifiers(code),
            [
                "void",
                "SoundMain",
                "void",
                "char",
                "s",
                "u8",
                "c",
                "MPlayMain",
                "p"
            ]
        );
        assert!(
            is_m4a("games/tla/src/sound/driver.c")
                && is_m4a("games/tbs/include/sound/sound_driver_types.h")
        );
        assert!(
            !is_m4a("games/tla/src/sound/sound_play.c")
                && !is_m4a("games/tla/include/sound/sound.h")
        );
    }

    #[test]
    fn family_shapes_the_rules_keep() {
        let po = "msgctxt \"MsgDebugAreaName\"\nmsgid \"00001\"\nmsgstr \"E Tundaria Islet\"\n\nmsgid \"00002\"\nmsgstr \"Vale Sanctum\"\n\n\
                  msgid \"00003\"\nmsgstr \"Sanctum\"\n\nmsgid \"00004\"\nmsgstr \"Fuchin Falls\"\n\nmsgid \"00005\"\nmsgstr \"Imil\"\n\n\
                  msgid \"00006\"\nmsgstr \"Vale\"\n\nmsgid \"00007\"\nmsgstr \"Bilibin\"\n\nmsgid \"00008\"\nmsgstr \"Kolima\"\n";
        let text = GameText::parse(po, Some(8)).unwrap();
        assert_eq!(
            area_words(Some(&text))
                .iter()
                .map(|area| area.join(" "))
                .collect::<Vec<_>>()[0],
            "east tundaria islet"
        );
        assert!(
            !area_words(Some(&text)).contains(&vec!["sanctum".to_string()]),
            "Sanctum alone ends other areas' names"
        );
        let flags = "games/tbs/include/flags.h";
        let mut list: Vec<Name> = [
            "FLAG_VALE_A",
            "FLAG_VALE_B",
            "FLAG_BILIBIN_C",
            "FLAG_KOLIMA_D",
            "FLAG_IMIL_E",
            "FLAG_SAVE_RESTARTED_AT_SANCTUM",
        ]
        .iter()
        .enumerate()
        .map(|(at, n)| name(n, NameKind::Macro, flags, at + 1))
        .collect();
        for (at, n) in [
            "AREA_VALE",
            "AREA_BILIBIN",
            "AREA_KOLIMA",
            "AREA_IMIL",
            "AREA_FUCHIN_FALLS",
            "AREA_EAST_TUNDARIA_ISLET",
        ]
        .iter()
        .enumerate()
        {
            list.push(name(
                n,
                NameKind::Enumerator,
                "games/tla/include/area_ids.h",
                10 + at,
            ));
        }
        let scene = "games/tbs/src/scene/imil_fuchin_falls.c";
        for (at, n) in [
            "CB_FUCHIN_FALLS_A",
            "CB_FUCHIN_FALLS_B",
            "CB_FUCHIN_FALLS_C",
            "CB_FUCHIN_FALLS_D",
            "CB_FUCHIN_FALLS_E",
            "CB_IMIL_CAVE_ICE",
        ]
        .iter()
        .enumerate()
        {
            list.push(Name {
                group: Some("falls.c#1".to_string()),
                ..name(n, NameKind::Enumerator, scene, 20 + at)
            });
        }
        for (at, n) in [
            "ACTOR_STONE_FACE_A",
            "ACTOR_STONE_FACE_B",
            "ACTOR_STONE_FACE_C",
            "ACTOR_STONE_FACE_D",
            "ACTOR_STONE_FACE_E",
            "ACTOR_NUT_CHEST",
        ]
        .iter()
        .enumerate()
        {
            list.push(Name {
                group: Some("ruins.c#1".to_string()),
                ..name(n, NameKind::Enumerator, scene, 30 + at)
            });
        }
        let iwram = "games/tla/include/system/iwram_call.h";
        for (at, n) in ["Iwram_CopyWords", "Iwram_FillWords", "Iwram_Sqrt"]
            .iter()
            .enumerate()
        {
            list.push(name(n, NameKind::Function, iwram, 40 + at));
        }
        list.push(name("IwramMulFX", NameKind::Prototype, iwram, 45));
        list.push(name("IwramRun", NameKind::Function, iwram, 46));
        for (at, n) in [
            "ITEM_COUNT",
            "ENEMY_COUNT",
            "UNIT_COUNT",
            "ACTOR_COUNT",
            "SLOT_COUNT",
            "countAndShadow",
            "MAX4_WORDS",
            "LINE_MAX",
        ]
        .iter()
        .enumerate()
        {
            list.push(name(
                n,
                NameKind::Macro,
                "games/tbs/include/counts.h",
                50 + at,
            ));
        }
        let findings = check(
            &Names {
                names: list,
                ..Names::default()
            },
            Some(&text),
        );
        let shapes: Vec<String> = findings
            .iter()
            .filter(|f| f.metric == "family_shape")
            .map(|f| f.what.clone())
            .collect();
        assert_eq!(
            shapes,
            ["IwramRun\tseparator: Iwram_Run, as the 3 Iwram_ functions are, or a prefix of its own"],
            "a Sanctum is no one place; E is east; a cell of Imil's own place; an actor enum's run; IWRAM's linked symbol; two things joined by and; an operation"
        );
    }

    #[test]
    fn prefix_twins_the_rules_keep() {
        let po = "msgctxt \"MsgArena\"\nmsgid \"00001\"\nmsgstr \"Fight the next monster? The enemy waits. Isaac sleeps by Gaia Falls.\"\n\n\
                  msgctxt \"MsgDebugAreaName\"\nmsgid \"00002\"\nmsgstr \"Islet Cave\"\n";
        let text = GameText::parse(po, Some(1)).unwrap();
        let file = "games/tla/src/scene/a.c";
        let mut list = Vec::new();
        let mut add = |names: &[&str], kind: NameKind| {
            for n in names {
                list.push(name(n, kind, file, list.len() + 1));
            }
        };
        add(&["MONSTER_BATTLE", "ENEMY_A", "ENEMY_B"], NameKind::Macro);
        add(
            &["SECONDS_PER_MINUTE", "SECOND_HAND", "SECOND_ROW"],
            NameKind::Macro,
        );
        add(
            &["Actors_SetCellsOpen", "Actor_SetCellOpen", "Actor_Run"],
            NameKind::Function,
        );
        add(&["ACTOR_A", "ACTOR_B"], NameKind::Enumerator);
        add(&["CRATES_LANE", "CRATE_MOVED"], NameKind::Macro);
        add(&["ISAACS_BED", "ISAAC_WALK"], NameKind::Enumerator);
        add(&["ISLET_CAVE_EXIT", "ISLETS_EXIT"], NameKind::Macro);
        add(
            &["STREAKS_HIT", "STREAK_WIDTH", "STREAK_HEIGHT"],
            NameKind::Macro,
        );
        add(
            &["FALLS_PALETTE", "FALL_FRAMES", "FALL_TRAILS"],
            NameKind::Macro,
        );
        add(&["SceneId_KibomboStatueGrounds"], NameKind::Extern);
        add(
            &["GROUNDS_FIRE", "GROUND_LEVEL", "GROUND_HEIGHT"],
            NameKind::Macro,
        );
        add(
            &["SPARKS_Z", "WEST_SPARKS_X", "SPARK_LIFE", "SPARK_SPEED"],
            NameKind::Macro,
        );
        let mut names = Names {
            names: list,
            ..Names::default()
        };
        names.assets.insert("crates".to_string());
        let twins: Vec<String> = check(&names, Some(&text))
            .into_iter()
            .filter(|f| f.metric == "prefix_twin")
            .map(|f| f.what)
            .collect();
        let apart: Vec<String> = check(&names, Some(&text))
            .into_iter()
            .filter(|f| f.metric == "prefix_twin_apart")
            .map(|f| f.what)
            .collect();
        assert_eq!(
            apart,
            [
                "FALLS_\t1 names; a place's own word, beside FALL_ (2 names)",
                "GROUNDS_\t1 names; a place's own word, beside GROUND_ (2 names)",
                "SPARKS_\t1 names; the group's word in its file, beside SPARK_ (2 names)",
            ]
        );
        assert_eq!(
            twins,
            ["STREAKS_\t1 names; one idea with STREAK_ (2 names)"],
            "the text tells monster from enemy; a rate; a routine over a list; a picture; Isaac's; a place; the last word of a place's \
             name in the text or a scene's; a group its file names so inside other names"
        );
    }

    #[test]
    fn family_shapes() {
        let po =
            "msgctxt \"MsgDebugAreaName\"\nmsgid \"00001\"\nmsgstr \"Vale\"\n\nmsgctxt \"message\"\nmsgid \"00002\"\nmsgstr \"Idejima\"\n";
        let text = GameText::parse(po, Some(2)).unwrap();
        let file = "games/tbs/include/flags.h";
        let mut list: Vec<Name> = [
            "FLAG_VALE_A",
            "FLAG_VALE_B",
            "FLAG_VALE_C",
            "FLAG_IDEJIMA_D",
            "FLAG_IDEJIMA_E",
            "FLAG_SAW_SHIP_AT_IDEJIMA",
            "FLAG_Childhood",
        ]
        .iter()
        .enumerate()
        .map(|(at, n)| name(n, NameKind::Macro, file, at + 1))
        .collect();
        let scene = "games/tbs/src/scene/vale.c";
        for (at, n) in ["ACTOR_GUARD", "ACTOR_MAYOR", "ACTOR_DOG", "GUARD_CAPTAIN"]
            .iter()
            .enumerate()
        {
            list.push(Name {
                group: Some("vale.c#3".to_string()),
                ..name(n, NameKind::Enumerator, scene, 10 + at)
            });
        }
        for (at, n) in [
            "BattleObj_Init",
            "BattleObj_Free",
            "BattleObj_Draw",
            "BattleObjGet",
        ]
        .iter()
        .enumerate()
        {
            list.push(name(
                n,
                NameKind::Function,
                "games/tbs/src/battle/obj.c",
                20 + at,
            ));
        }
        for (at, n) in [
            "ITEM_COUNT",
            "ENEMY_COUNT",
            "UNIT_COUNT",
            "ACTOR_COUNT",
            "SLOT_COUNT",
            "NUM_ELEMENTS",
        ]
        .iter()
        .enumerate()
        {
            list.push(name(
                n,
                NameKind::Macro,
                "games/tbs/include/counts.h",
                30 + at,
            ));
        }
        let findings = check(
            &Names {
                names: list,
                ..Names::default()
            },
            Some(&text),
        );
        let shapes: Vec<String> = findings
            .iter()
            .filter(|f| f.metric == "family_shape")
            .map(|f| f.what.clone())
            .collect();
        assert_eq!(
            shapes,
            [
                "FLAG_Childhood\tcase: PascalCase where the FLAG_ names are CAPITALS (6 of 7, as FLAG_VALE_A)",
                "FLAG_SAW_SHIP_AT_IDEJIMA\tplace last: FLAG_IDEJIMA_... as 5 of the 7 FLAG_ names start with their place (as FLAG_VALE_A)",
                "GUARD_CAPTAIN\tmissing part: ACTOR_... as 3 of the 4 in its enum are (as ACTOR_GUARD)",
                "BattleObjGet\tseparator: BattleObj_Get, as the 3 BattleObj_ functions are, or a prefix of its own",
                "NUM_ELEMENTS\torder: num goes last, where 5 of the game's 6 names with it at an end put it (as ITEM_COUNT)",
            ]
        );
    }

    #[test]
    fn pret_words_are_pret_spelling() {
        let file = "games/tbs/src/battle/a.c";
        let names = Names {
            names: vec![
                name("Sprite_StartAnim", NameKind::Function, file, 1),
                name("gBattleAnimArgs", NameKind::Global, file, 2),
                name("InitHeap", NameKind::Function, file, 3),
                name("affineParam", NameKind::Field, file, 4),
                name("songNum", NameKind::Field, file, 5),
                name("Field_RunAnimation", NameKind::Function, file, 6),
                name("Battle_ApplyArgument", NameKind::Function, file, 7),
                name("Actor_StartScript", NameKind::Function, file, 8),
                name("Motion_SetParameter", NameKind::Function, file, 9),
                name("Unit_Count", NameKind::Function, file, 10),
                name("Battle_GetArg", NameKind::Function, file, 11),
                name("ANIM_WALK", NameKind::Macro, file, 12),
                name("ANIM_RUN", NameKind::Macro, file, 13),
                name("ANIMATION_IDLE", NameKind::Macro, file, 14),
                name("ANIMATION_JUMP", NameKind::Macro, file, 15),
                name("ANIMATION_FALL", NameKind::Macro, file, 16),
                // A macro written as a call starts with its verb.
                name("NUM_MUSIC_PLAYERS", NameKind::Macro, file, 17),
                name("COUNT_ALL_CURES", NameKind::FunctionMacro, file, 18),
                name("COUNT_PARTIAL_CURES", NameKind::FunctionMacro, file, 19),
            ],
            ..Names::default()
        };
        let findings = check(&names, None);
        let what = |metric: &str| -> Vec<String> {
            findings
                .iter()
                .filter(|f| f.metric == metric)
                .map(|f| f.what.split('\t').next().unwrap().to_string())
                .collect()
        };
        assert_eq!(
            what("abbreviation"),
            ["Battle_GetArg"],
            "anim, args, init, param and num are pret's; arg alone is not"
        );
        assert_eq!(what("prefix_twin"), ["ANIMATION_"], "pret's ANIM_ is preferred though fewer names use it; COUNT_ALL_CURES is a verb, not a twin of NUM_");
    }

    #[test]
    fn plural_prefixes_pret_writes() {
        let file = "games/tbs/include/a.h";
        let mut list: Vec<Name> = [
            "PALETTES_ALL",
            "PALETTES_BG",
            "PALETTES_OBJECTS",
            "TRACKS_ALL",
            "TRACKS_FEW",
        ]
        .iter()
        .enumerate()
        .map(|(at, n)| name(n, NameKind::Enumerator, file, at + 1))
        .collect();
        for (at, n) in [
            "PALETTE_A",
            "PALETTE_B",
            "PALETTE_C",
            "TRACK_A",
            "TRACK_B",
            "TRACK_C",
        ]
        .iter()
        .enumerate()
        {
            list.push(name(n, NameKind::Enumerator, file, at + 10));
        }
        let names = Names {
            names: list,
            ..Names::default()
        };
        let twins: Vec<String> = check(&names, None)
            .into_iter()
            .filter(|f| f.metric == "prefix_twin")
            .map(|f| f.what)
            .collect();
        assert_eq!(
            twins,
            ["TRACKS_\t2 names; one idea with TRACK_ (3 names)"],
            "a plural prefix is pret's only when all its names are"
        );
    }

    #[test]
    fn one_type_one_name_leaves_out_members_arrays_and_clashes() {
        let typed = |tag: &str, name: &str, line, scope| Typed {
            tag: tag.to_string(),
            name: name.to_string(),
            file: "f.c".to_string(),
            line,
            scope: Some(scope),
            ..Typed::default()
        };
        let names = Names {
            typed: vec![
                typed("Object", "object", 1, 1),
                typed("Object", "object", 2, 2),
                typed("Object", "object", 3, 3),
                Typed {
                    member: true,
                    ..typed("Object", "objects", 4, 4)
                },
                Typed {
                    array: true,
                    ..typed("Object", "objs", 5, 5)
                },
                // `obj` where its function already has another `object`.
                typed("Object", "obj", 6, 6),
                typed("Unit", "object", 6, 6),
                typed("Object", "obj", 7, 7),
                typed("ItemDefinition", "item", 8, 8),
                typed("ItemDefinition", "item", 9, 9),
                typed("ItemDefinition", "item", 10, 10),
                typed("ItemDefinition", "definition", 11, 11),
                typed("Unit", "item", 11, 11),
            ],
            ..Names::default()
        };
        let findings: Vec<String> = check(&names, None)
            .into_iter()
            .map(|f| format!("{}:{} {}", f.file, f.line, f.what))
            .collect();
        assert_eq!(
            findings,
            ["f.c:7 obj\tstruct Object: 2 uses -> object (3 uses)"],
            "a member and an array are left out; obj is reported where object is free, definition, whose one function has an item, not at all"
        );
    }
}
