//! The humanization report: numbers for the work of making the matched C
//! readable (S3), meant to fall the way DONE rose. Each game's tracked C and
//! headers under `src` and `include` are lexed (never preprocessed) and
//! counted by simple rules, each set out where it is counted: magic numbers,
//! placeholder and numbered names, fake-match devices (S2), edition tests,
//! undocumented declarations, long files, duplicated code and, with the
//! game's English text, where the names of game things come from
//! (`provenance`), and whether the names can be guessed from one another
//! (`naming`). Nothing here counts toward DONE (C2); the figures are a
//! worklist for people (H5).

mod duplicates;
mod layout;
mod naming;
mod numbers;
mod provenance;

use crate::alchemy::done::commas;
use crate::alchemy::evidence;
use crate::gba::clex::{self, Kind, Token};
use naming::NameKind;
use provenance::{Analysis, Definition, GameText, Origin, Thing};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::OnceLock;

pub const REPORT: &str = "out/reports/humanity.tsv";

/// S2's fake-match kinds, in its ranking, worst first. A once-only block no
/// macro expands to is a `dead` device; `one-pass` is no longer a kind, and a
/// register pin is none: no game takes one.
pub const DEVICES: [&str; 6] = ["signature", "volatile", "barrier", "wrapper", "temporary", "dead"];

/// Each kind's weight in a game's fake-match score, in `DEVICES`' order, each
/// twice the next (S2, 20 November 2026): the score is the sum of a game's
/// tags' weights.
pub const WEIGHTS: [usize; 6] = [64, 32, 16, 8, 4, 2];

/// S2's leftover kinds: Camelot's own code that does nothing in an edition
/// and still shapes it, tagged `LEFTOVER(<kind>)` (12 November 2026). They
/// are counted beside the devices, never with them; the `// DEAD:` marks are
/// counted with them, on a row of their own, as `dead`. `form` is no longer a
/// kind: a device The Lost Age's ROM also needs is a fake match.
pub const LEFTOVERS: [&str; 2] = ["frame", "template"];

/// A function body this many tokens long or more takes part in the duplicate search.
const FUNCTION_TOKENS: usize = 24;
/// A table initializer this many tokens long or more takes part in the
/// duplicate search: more than 24, about ten numbers, since shorter tables
/// (a creature's gravity, a picture's widths) are equal to others by chance.
const TABLE_TOKENS: usize = 25;
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

/// The macros standard C or its compilers give every program, which a
/// game's headers write out because it builds without the C library: their
/// names are their documentation.
const STANDARD_MACROS: &[&str] = &["NULL", "TRUE", "FALSE", "offsetof", "UNUSED"];

/// Whether a routine is the compiler's, the C library's or the BIOS's,
/// named as those name it (`__divsi3`, `_call_via_r0`, `abs`, `Bios_CpuSet`):
/// its documentation is theirs.
fn is_library_routine(name: &str) -> bool {
    name.starts_with("__")
        || name.starts_with("_call_via_")
        || name.starts_with("Bios_")
        || matches!(name, "abs" | "labs" | "memcpy" | "memset" | "memcmp" | "memmove" | "strlen" | "strcmp" | "strcpy")
}

/// Whether a word names a type: C's own, or the game's integer, fixed-point
/// and boolean types (`u8`, `vs16`, `FIXED`, `bool8`).
fn is_type_word(text: &str) -> bool {
    static TYPE: OnceLock<Regex> = OnceLock::new();
    let pattern = TYPE.get_or_init(|| Regex::new(r"^(?:v?[us](?:8|16|32|64)|FIXED|bool(?:8|16|32)?|size_t)$").unwrap());
    pattern.is_match(text) || matches!(text, "struct" | "union" | "enum" | "void" | "const" | "volatile" | "unsigned" | "signed" | "char" | "int" | "short" | "long")
}

/// The name an object-like macro's body views, and how many casts it
/// takes to: `gWindowWork` of `((struct WindowWork *)gWindowWork[0])`,
/// `gHeapSlots` of `(*(struct BattleMenuWork **)&gHeapSlots[HEAP_SLOT_BATTLE_MENU])`,
/// `gWaitForFlashWrite` of `gWaitForFlashWrite`. Any other body, a
/// number or a computation, views nothing.
fn view_target<'t>(body: &[&'t str]) -> Option<(&'t str, usize)> {
    let close_of = |tokens: &[&str]| -> Option<usize> {
        let mut depth = 0;
        for (at, text) in tokens.iter().enumerate() {
            match *text {
                "(" => depth += 1,
                ")" => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(at);
                    }
                }
                _ => {}
            }
        }
        None
    };
    let is_name = |text: &str| text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && !is_keyword(text) && !is_type_word(text);
    let mut tokens = body;
    let mut casts = 0;
    loop {
        match tokens.first().copied() {
            Some("*" | "&") => tokens = &tokens[1..],
            Some("(") => {
                let close = close_of(tokens)?;
                if close == tokens.len() - 1 {
                    tokens = &tokens[1..close];
                    continue;
                }
                let inner = &tokens[1..close];
                let typed = inner.first().is_some_and(|first| is_type_word(first) || first.starts_with(|c: char| c.is_ascii_uppercase()));
                let plain = inner.iter().all(|text| matches!(*text, "*" | "(" | ")" | ",") || text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_'));
                if !(typed && plain) {
                    return None;
                }
                casts += 1;
                tokens = &tokens[close + 1..];
            }
            _ => break,
        }
    }
    let (&target, mut rest) = tokens.split_first()?;
    if !is_name(target) {
        return None;
    }
    while let Some((&first, after)) = rest.split_first() {
        match first {
            "->" | "." if after.first().is_some_and(|name| is_name(name)) => rest = &after[1..],
            "[" => {
                let mut depth = 0;
                let close = rest.iter().position(|text| {
                    match *text {
                        "[" => depth += 1,
                        "]" => depth -= 1,
                        _ => {}
                    }
                    depth == 0
                })?;
                rest = &rest[close + 1..];
            }
            _ => return None,
        }
    }
    Some((target, casts))
}

/// The routine a function body is one call to, its result returned or
/// not, its arguments calling nothing (`return Party_ListMembers(owners);`).
fn one_call<'t>(body: &[&'t str]) -> Option<&'t str> {
    let body = body.strip_prefix(&["return"]).unwrap_or(body);
    let [callee, "(", arguments @ .., ")", ";"] = body else { return None };
    if !callee.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') || is_keyword(callee) {
        return None;
    }
    let mut depth = 0;
    for (at, text) in arguments.iter().enumerate() {
        match *text {
            "(" => {
                // A call among the arguments: more than one call.
                if at > 0 && arguments[at - 1].starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && !is_keyword(arguments[at - 1]) {
                    return None;
                }
                depth += 1;
            }
            ")" => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            _ => {}
        }
    }
    (depth == 0).then_some(*callee)
}

/// The text's arguments of a call, split at its top-level commas, from just
/// after its `(`: `ACTOR_PLACEMENT(SPRITE_STAKE, ..., 0, 0, 0, ...)`.
fn call_arguments(text: &str) -> Vec<String> {
    let mut arguments = vec![String::new()];
    let mut depth = 0;
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => break,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                arguments.push(String::new());
                continue;
            }
            _ => {}
        }
        arguments.last_mut().expect("one").push(c);
    }
    arguments.into_iter().map(|argument| argument.split_whitespace().collect::<Vec<_>>().join(" ")).collect()
}

/// The actors a scene file places at its map's origin, (0, 0), where no
/// camera of the map looks: each enum of its placed actors (`=
/// ACTOR_FIRST_PLACED`) is matched to the placement table whose name's
/// words its tag holds most of (`gMarsLighthouseEntrance_FuguePlacements`
/// for `enum MarsLighthouseEntranceFugueActorId`), and its n-th actor to the
/// table's n-th row, the first branch of an `#if` read. A row placed at a
/// scene point instead (`ACTOR_PLACEMENT_AT_SCENE_POINT`) is not at the
/// origin.
pub fn placed_at_origin(text: &str) -> Vec<String> {
    static ENUM: OnceLock<Regex> = OnceLock::new();
    static TABLE: OnceLock<Regex> = OnceLock::new();
    let enums = ENUM.get_or_init(|| Regex::new(r"enum\s+(\w+)\s*\{([^}]*)\}").unwrap());
    let tables = TABLE.get_or_init(|| Regex::new(r"struct\s+ActorPlacement\s+(\w+)\s*\[\s*\]\s*=\s*\{").unwrap());
    let words = |name: &str| -> BTreeSet<String> {
        provenance::pieces(name)
            .into_iter()
            .map(str::to_ascii_lowercase)
            .filter(|word| !matches!(word.as_str(), "g" | "actor" | "id" | "placements"))
            .collect()
    };
    // Each table's rows, as (x, z), and the words of its name.
    let mut rows: Vec<(BTreeSet<String>, Vec<Option<(String, String)>>)> = Vec::new();
    for found in tables.captures_iter(text) {
        let body_start = found.get(0).expect("match").end();
        let body = &text[body_start..text[body_start..].find("};").map_or(text.len(), |end| body_start + end)];
        let mut placed = Vec::new();
        let mut skipping: Vec<bool> = Vec::new();
        let mut line_start = 0;
        for line in body.split_inclusive('\n') {
            let at = line_start;
            line_start += line.len();
            let directive = line.trim_start().strip_prefix('#').map(str::trim_start);
            match directive {
                Some(rest) if rest.starts_with("if") => skipping.push(false),
                Some(rest) if rest.starts_with("el") => {
                    if let Some(last) = skipping.last_mut() {
                        *last = true;
                    }
                }
                Some(rest) if rest.starts_with("endif") => {
                    skipping.pop();
                }
                _ if skipping.iter().any(|skip| *skip) => {}
                _ => {
                    for (offset, _) in line.match_indices("ACTOR_PLACEMENT(") {
                        let arguments = call_arguments(&body[at + offset + "ACTOR_PLACEMENT(".len()..]);
                        let at_point = arguments.last().is_some_and(|flags| flags.contains("SCENE_POINT"));
                        placed.push((arguments.len() > 5 && !at_point).then(|| (arguments[3].clone(), arguments[5].clone())));
                    }
                }
            }
        }
        rows.push((words(&found[1]), placed));
    }
    let mut out = Vec::new();
    for found in enums.captures_iter(text) {
        let tag = words(&found[1]);
        let Some((_, placed)) = rows.iter().filter(|(table, _)| !table.is_empty() && table.is_subset(&tag)).max_by_key(|(table, _)| table.len()) else {
            continue;
        };
        let body: String = found[2].lines().map(|line| line.split("//").next().unwrap_or("")).collect::<Vec<_>>().join(" ");
        let mut index: Option<usize> = None;
        for entry in body.split(',') {
            let (name, value) = entry.split_once('=').map_or((entry.trim(), None), |(name, value)| (name.trim(), Some(value.trim())));
            if name.is_empty() {
                continue;
            }
            index = match (index, value) {
                (_, Some("ACTOR_FIRST_PLACED")) => Some(0),
                (Some(_), Some(_)) | (None, _) => None,
                (Some(at), None) => Some(at + 1),
            };
            let Some(at) = index else { continue };
            if placed.get(at).cloned().flatten().is_some_and(|(x, z)| x == "0" && z == "0") {
                out.push(name.to_string());
            }
        }
    }
    out
}

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
    /// The row that sets it apart when it has no `///` of its own (D9's
    /// rules: `undocumented_run` and the rest).
    apart: Option<&'static str>,
    /// The routine its body is one call to, for a forwarder.
    forward: Option<String>,
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
    /// Each unit H4 judges (a map overlay, a folder guide), by its file, and
    /// how many `XXX:` questions its files hold (`XXX (no evidence):` ones
    /// aside).
    pub units: BTreeMap<String, u64>,
    /// Each file's `XXX:` questions, by line, for its unit.
    questions: BTreeMap<String, Vec<(usize, String)>>,
    pub function_groups: Vec<Group>,
    pub table_groups: Vec<Group>,
    /// One body under several names the ROM keeps apart, said so in its
    /// comments (`duplicates`).
    pub said_groups: Vec<Group>,
    /// The groups someone judged not to be one text, by a row of
    /// `evidence/<game>-duplicates.tsv`, each with its row's kind and why.
    pub judged_groups: Vec<(Group, String, String)>,
    /// Those rows, by the group's key (`group_key`).
    duplicate_rows: HashMap<String, (String, String)>,
    /// Each file and what it `#include`s, as written.
    includes: Vec<(String, String)>,
    /// Each file's identifiers and the words of its comments, for a shared
    /// file's parameters: which names it uses, and which it says.
    file_words: HashMap<String, (HashSet<String>, HashSet<String>)>,
    /// Each undocumented macro defined right before an `#include` (file,
    /// line, name, the include as written, and the name it views), settled
    /// once every file is read: a parameter of the included file where that
    /// file uses the name (`shared_parameters`).
    parameter_candidates: Vec<(String, usize, String, String, Option<(String, usize)>)>,
    /// Every file read.
    paths: BTreeSet<String>,
    /// The externs an id macro takes the address of (`extern char
    /// SceneId_Vale;` beside `#define SCENE_VALE ((s32)&SceneId_Vale)`).
    id_externs: HashSet<String>,
    /// The identifiers the macros' bodies and the assembly (a far-call
    /// veneer's export row) use, for which routines and tables are referred
    /// to.
    macro_words: HashSet<String>,
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
    /// Each number in code, settled once every declaration is read.
    candidates: Vec<Candidate>,
    /// Each type of pointer to functions whose parameters are named
    /// (`typedef s32 (*BattleEffectDrawRectangle)(..., s32 x, ...)`), and
    /// each name a typedef gives another type (`DrawRectangle`).
    function_types: HashMap<String, Vec<(String, Vec<Option<String>>)>>,
    type_aliases: HashMap<String, Vec<(String, String)>>,
    /// The named types each variable, parameter or member is declared with
    /// (`DrawRectangle draw[2];`), for a call through it.
    typed_variables: HashMap<String, BTreeSet<(String, String)>>,
    /// For each function that calls a pointer it is passed with its own
    /// parameters (`Call1(f, a0) { f(a0); }`): for each parameter, the
    /// pointer parameter it is passed to and in which place.
    forwards: HashMap<String, Vec<Option<(usize, usize)>>>,
    /// For each function defined, the parameters it turns into 16.16 itself
    /// (`velocityY = toFIXED(speed)`), by place.
    converted: HashMap<String, HashSet<usize>>,
    /// Each declared function's parameter names.
    parameters: HashMap<String, Vec<Option<String>>>,
    /// Each function-like macro's parameters and the call each is passed to.
    macro_parameters: HashMap<String, Vec<(String, Option<(String, usize)>)>>,
    /// Function-like macros that only compose a number from their parameters
    /// (`PIXELS`, `RGB`, `DMA_COPY_WORDS`): S3's composed forms.
    compositions: HashSet<String>,
    /// Function-like macros that bring a coordinate down to the map grid's
    /// cells or tiles (`FIELD_CELL(coordinate)`, `CELL_HEIGHT_STEPS(height)`).
    grid_conversions: HashSet<String>,
    /// Object-like macros that stand for a routine, each with the routines it
    /// stands for in the editions (`MENU_DRAW_STATUS_LABEL`).
    aliases: HashMap<String, BTreeSet<String>>,
    /// Names written beside a bitwise operator anywhere in the game
    /// (`flags & PLACE_ENEMY_SIDE`, `~CELL_CODE_BITS`), and those written
    /// beside arithmetic or an ordering (`CANVAS_SIZE + WIPE_LAG_MAX`).
    bitwise_names: HashSet<String>,
    arithmetic_names: HashSet<String>,
    /// Each hex number a `#define` or enumerator gives a name alone that no
    /// rule of its file keeps, settled once every use of its name is read:
    /// file, line, number, name and the definition it is in (`clex::owners`).
    hex_definitions: Vec<(String, usize, String, String, Option<String>)>,
    /// Every function body and table initializer, for the duplicate search.
    copies: Vec<duplicates::Copy>,
    /// Every name the source defines or declares, for the naming checks.
    pub naming: naming::Names,
    /// Numbered names whose number is their meaning, by a row of kind `its
    /// number` in `evidence/<game>-names.tsv`: counted apart (`numbered_honest`).
    numbered_by_row: HashSet<String>,
    /// Numbers in code someone judged by reading them, by file, routine and
    /// the number as written: `true` honest, `false` unsettled
    /// (`evidence/<game>-numbers.tsv`).
    judged: HashMap<(String, String, String), bool>,
    /// Each `.c` file's own function-like macros that number a route's steps
    /// or count from a named first, by what each parameter numbers
    /// (`own_macro_kinds`).
    own_macros: HashMap<String, HashMap<String, Vec<Option<&'static str>>>>,
    /// The functions whose parameters a definition named, which a prototype
    /// elsewhere does not rename.
    defined_parameters: HashSet<String>,
    /// Every declaration's parameter names, by function, for a parameter the
    /// chosen one names vaguely (`rate` where a prototype says `percent`).
    parameter_lists: HashMap<String, Vec<Vec<Option<String>>>>,
    /// Message symbols whose own text says `unknown` (`MsgDetailsUnknown`):
    /// the game's word, no placeholder.
    text_unknown: HashSet<String>,
    /// Every message's symbol in the game's text (`msgctxt "MsgCanYouImagine"`):
    /// a message declared in C is said by its text, as a string is.
    message_symbols: HashSet<String>,
    /// Each macro, whether it has its `///` or a rule sets it apart that
    /// leaves nothing to mark, for what a view views.
    macro_docs: HashMap<String, bool>,
    /// Each undocumented object-like macro that views one name (file, line,
    /// name, the name it views, how many casts), settled once every
    /// declaration is read.
    views: Vec<(String, usize, String, String, usize)>,
    /// Each routine a scene's overlay defines (file, name, marked `UNUSED`),
    /// and how many times each scene file writes each name, for the routines
    /// nothing runs.
    scene_routines: Vec<(String, String, bool)>,
    scene_names: HashMap<String, HashMap<String, usize>>,
    /// The routines nothing runs: marked `UNUSED`, or a scene's that its
    /// overlay writes only where it defines it (file, name).
    pub never_run: HashSet<(String, String)>,
    /// Each routine's copies of the same body in other files (file, name),
    /// by its file and name.
    pub same_body: HashMap<(String, String), Vec<(String, String)>>,
    /// The actors a scene places at its map's origin and no routine of its
    /// file names, so never moved into any camera's sight (file, name).
    pub at_origin: HashSet<(String, String)>,
    /// The names a scene file's routines write, for the actors no routine
    /// moves.
    moved: HashSet<(String, String)>,
}

/// A number in code, with each call around it and the argument it fills
/// (from 0), innermost first, as far as the lexer can tell, and what its
/// tokens show of its kind (`numbers`).
#[derive(Clone, Debug)]
struct Candidate {
    file: String,
    line: usize,
    literal: String,
    calls: Vec<(String, usize)>,
    /// A lone argument (`Event_Wait(40)`), which the parameter it fills may
    /// make data.
    lone: bool,
    /// The honest kind its own tokens show.
    local: Option<&'static str>,
    /// The innermost call's arguments that are one name each, for a
    /// call layer (`Call1(Event_Wait, 15)`), and whether a random roll is
    /// among them.
    siblings: Vec<Option<String>>,
    random_arguments: bool,
    /// A call through a pointer it is an argument of (`draw[0](...)`), and
    /// whether the name is the type the pointer is cast to
    /// (`((DrawRectangle)draw[0])(...)`).
    pointer: Option<(String, usize, bool)>,
    /// The right operand of a shift: an amount of bits, never a position.
    shift: bool,
    /// Inside a function-like macro's own definition, which names it
    /// (`#define FIELD_HALF_CELL(v) ((v) >> 19)`).
    definition: bool,
    /// The calls it is compared with or offsets at expression level
    /// (`FIELD_CELL` of `FIELD_CELL(x) == 15`).
    operand_calls: Vec<String>,
    /// Reckoned in a group of its own inside its argument (`(frame - 486) *
    /// STEP`), before the argument becomes what its parameter takes.
    grouped: bool,
    /// The hardware register the innermost call writes it to, by address
    /// (`REG_BLDY` of `Io_Write16(31, &REG_BLDY)`).
    register: Option<String>,
    /// The routine or definition it is in (`clex::owners`), for its row of
    /// judged numbers.
    routine: Option<String>,
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
/// which argument it is in (from 0), reading back no further than `floor`: `Actor_WalkTo`
/// and 1 for the `257` of `Actor_WalkTo(ACTOR_PARTY_LEADER, 257, 420)`.
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

    /// How a number someone judged by reading it was judged: `Some(true)`
    /// honest, `Some(false)` unsettled, by its file, routine and text.
    fn judged(&self, file: &str, routine: Option<&str>, number: &str) -> Option<bool> {
        self.judged.get(&(file.to_string(), routine?.to_string(), number.to_string())).copied()
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

    /// Keep a routine or global's declaration, with the row that sets it
    /// apart if it has no `///`.
    #[allow(clippy::too_many_arguments)]
    fn declare_apart(&mut self, function: bool, name: &str, file: &str, line: usize, documented: bool, defined: bool, apart: Option<&'static str>) {
        let map = if function { &mut self.functions } else { &mut self.globals };
        let entry = map.entry(name.to_string()).or_insert_with(|| Declared { file: file.to_string(), line, ..Declared::default() });
        entry.documented |= documented;
        if entry.apart.is_none() {
            entry.apart = apart;
        }
        entry.entry |= file.contains("/include/gamepak/far_call/") || file.contains("/include/farcall/");
        if defined && !entry.defined {
            // Counted where it is defined, so set apart as it is there, else
            // as it is where first declared.
            entry.apart = apart.or(entry.apart);
            entry.defined = true;
            entry.file = file.to_string();
            entry.line = line;
        }
    }

    /// The parameter an argument fills, by name, and the kind of data that
    /// name says it holds. A macro's parameter takes its own name's kind, else
    /// that of the parameter it is passed to (`WAIT(n)` is
    /// `Event_Wait(s32 frames)`).
    fn parameter(&self, callee: &str, position: usize, depth: usize) -> Option<(String, Option<&'static str>)> {
        // A far call, `XFar`, is X itself: X's definition names its parameters.
        if let Some(routine) = callee.strip_suffix("Far").filter(|routine| self.defined_parameters.contains(*routine) && depth < 8) {
            if let Some(found) = self.parameter(routine, position, depth + 1) {
                return Some(found);
            }
        }
        if let Some(names) = self.parameters.get(callee) {
            let name = names.get(position)?.clone()?;
            let kind = data_parameter(&name);
            return Some((name, kind));
        }
        let Some(list) = self.macro_parameters.get(callee) else {
            // An alias (`#define MENU_DRAW_STATUS_LABEL Text_DrawStringInWindowFar`)
            // takes its routines' parameter, where every edition's agrees on
            // its kind: `x` in one and `offsetX` in another are coordinates.
            let mut found = self.aliases.get(callee)?.iter().filter(|_| depth < 8).map(|target| self.parameter(target, position, depth + 1));
            let first = found.next()??;
            return found.all(|other| other.is_some_and(|(_, kind)| kind == first.1)).then_some(first);
        };
        let (name, call) = list.get(position)?;
        let inner = call.as_ref().filter(|_| depth < 8).and_then(|(callee, at)| self.parameter(callee, *at, depth + 1));
        let kind = data_parameter(name).or_else(|| inner.as_ref().and_then(|(_, kind)| *kind));
        Some((name.clone(), kind))
    }

    /// The kind of data a lone argument is, from the calls around it: the
    /// parameter it fills, or, where that is a macro that only converts its
    /// value (`INTtoFIXED(value)`, `((value) << 16)`), the parameter the macro's
    /// result fills.
    /// A number composed by one of S3's forms (`INTtoFIXED(24)`, `RGB(31, 0, 0)`)
    /// is a plain quantity in the unit its macro names: it is data, unless a
    /// parameter further out says which kind (a coordinate, a size).
    fn argument_kind(&self, calls: &[(String, usize)]) -> Option<&'static str> {
        let mut composed = None;
        for (callee, at) in calls {
            let Some((_, kind)) = self.parameter(callee, *at, 0) else { return composed };
            if kind.is_some() {
                return kind;
            }
            if self.compositions.contains(callee) {
                composed = Some("composed");
            }
            let converts = self.macro_parameters.get(callee).and_then(|list| list.get(*at)).is_some_and(|(_, call)| call.is_none());
            if !converts {
                return composed;
            }
        }
        composed
    }

    /// Whether a lone decimal argument is a plain speed, as S3 has a call take
    /// one (`Actor_Jump(ACTOR_RIME, 11, 0)`, `FieldCamera_SetSpeed(16)`): it
    /// fills a parameter named for a speed or a velocity that the routine or
    /// macro itself turns into 16.16 with `toFIXED()` or `INTtoFIXED()`. A
    /// speed the callee keeps as it comes (an animation's, a step's) is no
    /// plain speed in pixels.
    fn plain_speed(&self, candidate: &Candidate) -> bool {
        let Some((callee, at)) = candidate.calls.first().filter(|_| numbers::is_decimal(&candidate.literal)) else { return false };
        let Some((name, _)) = self.parameter(callee, *at, 0) else { return false };
        let words = numbers::words(&name);
        let has = |list: &[&str]| words.iter().any(|word| list.contains(&word.as_str()));
        if !has(&["speed", "velocity"]) || has(&["animation", "anim", "step", "steps", "rate", "scale", "acceleration"]) {
            return false;
        }
        let by_macro = self
            .macro_parameters
            .get(callee)
            .and_then(|list| list.get(*at))
            .and_then(|(_, call)| call.as_ref())
            .is_some_and(|(inner, place)| {
                (*place == 0 && matches!(inner.as_str(), "toFIXED" | "INTtoFIXED")) || self.converted.get(inner).is_some_and(|places| places.contains(place))
            });
        by_macro || self.converted.get(callee).is_some_and(|places| places.contains(at))
    }

    /// What a parameter's name says beyond `data_parameter`'s words, for a
    /// number that would otherwise want a name: a duration counted in VBlanks
    /// or by a timer (`ScreenFade_ToBlack(s32 vblanks)`), a damping out of
    /// DAMPING_SCALE, a percentage (`BattleEffect_StartActorMotion(...,
    /// s32 percent)`), a fade's level or a colour channel's
    /// (`Palette_BrightenSavedBattleBg(s32 red, s32 green, s32 blue)`), a
    /// shake's amplitude (`MapGrid_SetShake(horizontal, vertical, decay)`),
    /// an edge (`PagedList_DrawPageNumbers(..., s32 rightEdge)`), and the
    /// edges of a rectangle in pixels, where the routine takes both a left or
    /// right and a top or bottom edge (`Window_ClearInteriorTiles(window,
    /// left, top, right, bottom)`; a `BLEND(top, bottom)` is no rectangle).
    fn parameter_words(&self, callee: &str, name: &str) -> Option<&'static str> {
        let words = numbers::words(name);
        let has = |list: &[&str]| words.iter().any(|word| list.contains(&word.as_str()));
        if has(&["velocity", "speed", "step", "acceleration"]) {
            return None;
        }
        if has(&["vblank", "vblanks", "timer", "countdown", "wait"]) {
            return Some("duration");
        }
        if has(&["damping"]) {
            return Some("damping");
        }
        // A percentage, a level or an edge, unless another word makes the
        // parameter a scale, an angle, a phase, a mode or an id
        // (`redScale`, `redPhase`, `fadeMode`).
        let other = has(&[
            "scale", "angle", "rotation", "zoom", "shift", "flip", "freq", "phase", "mode", "kind", "style", "state", "id", "index",
        ]);
        if !other && has(&["percent"]) {
            return Some("decimal");
        }
        // A sheet's own animation number: `Actor_FallOverWithAnimationById(id,
        // angle, s32 animation)`; its speed or step is a rate.
        if !other && words.last().is_some_and(|word| word == "animation") {
            return Some("animation");
        }
        if !other && has(&["fade", "shade", "brightness", "red", "green", "blue"]) {
            return Some("level");
        }
        // A level a canvas, palette or fade call fills or fades to:
        // `CANVAS_FILL_WORD(level)`.
        let toned = numbers::words(callee).iter().any(|word| matches!(word.as_str(), "canvas" | "fill" | "fade" | "palette" | "brightness"));
        if !other && toned && words.last().is_some_and(|word| word == "level") {
            return Some("level");
        }
        let shake = numbers::words(callee).iter().any(|word| word == "shake");
        if !other && shake && has(&["horizontal", "vertical", "amplitude"]) {
            return Some("level");
        }
        if matches!(name.to_ascii_lowercase().as_str(), "edge" | "leftedge" | "rightedge" | "topedge" | "bottomedge") {
            return Some("position");
        }
        let names: Vec<String> = match self.parameters.get(callee) {
            Some(names) => names.iter().flatten().cloned().collect(),
            None => self.macro_parameters.get(callee).map(|list| list.iter().map(|(name, _)| name.clone()).collect()).unwrap_or_default(),
        };
        let edge = |list: &[&str]| names.iter().any(|name| numbers::words(name).iter().any(|word| list.contains(&word.as_str())));
        (has(&["left", "top", "right", "bottom"]) && edge(&["left", "right"]) && edge(&["top", "bottom"])).then_some("position")
    }

    /// The routine a call layer passes an argument on to, and in which
    /// place: `C1(Event_Wait, 15)` is a macro for `Call1((void (*)())f, a)`,
    /// and `Call1(f, a0)` calls `f(a0)`, so its 15 is `Event_Wait`'s first.
    /// `siblings` are the call's arguments that are one name each.
    fn forward(&self, callee: &str, position: usize, siblings: &[Option<String>]) -> Option<(String, usize)> {
        if let Some(list) = self.forwards.get(callee) {
            let (function, at) = (*list.get(position)?)?;
            return Some((siblings.get(function)?.clone()?, at));
        }
        let parameters = self.macro_parameters.get(callee)?;
        let (inner, inner_at) = parameters.get(position)?.1.clone()?;
        let (function, at) = (*self.forwards.get(&inner)?.get(inner_at)?)?;
        let feeding = parameters.iter().position(|(_, call)| call.as_ref().is_some_and(|(name, place)| *name == inner && *place == function))?;
        Some((siblings.get(feeding)?.clone()?, at))
    }

    /// The parameter a call through a variable fills, from the one type of
    /// pointer to functions the variable is declared with: `draw[0](canvas,
    /// picture, 48, 24, ...)` with `DrawRectangle draw[2];` fills `x` with 48.
    /// A typedef or declaration is seen where C sees it: in the calling file,
    /// else in a header; two that disagree, or a typedef that names no
    /// parameter, resolve nothing. A pointer cast to a type (`cast`) is called
    /// as that type: `((DrawRectangle)draw[0])(...)`.
    fn pointer_parameter(&self, variable: &str, position: usize, cast: bool, file: &str) -> Option<String> {
        // What `file` sees of a name declared in several places.
        fn seen<'a, T: PartialEq>(entries: &'a [(String, T)], file: &str) -> Option<&'a T> {
            let own: Vec<&T> = entries.iter().filter(|(at, _)| at == file).map(|(_, value)| value).collect();
            let shared: Vec<&T> = entries.iter().filter(|(at, _)| at.ends_with(".h")).map(|(_, value)| value).collect();
            let visible = if own.is_empty() { shared } else { own };
            let first = *visible.first()?;
            visible.iter().all(|value| *value == first).then_some(first)
        }
        let resolve = |name: &str| -> Option<&Vec<Option<String>>> {
            let mut name = name;
            for _ in 0..8 {
                // A typedef another file keeps to itself is not seen here.
                if let Some(list) = self.function_types.get(name).and_then(|entries| seen(entries, file)) {
                    return Some(list);
                }
                name = seen(self.type_aliases.get(name)?, file)?;
            }
            None
        };
        if cast {
            return resolve(variable)?.get(position)?.clone();
        }
        let declared: Vec<(String, String)> = self.typed_variables.get(variable)?.iter().map(|(ty, at)| (at.clone(), ty.clone())).collect();
        let own = declared.iter().any(|(at, _)| at == file);
        let mut lists = declared.iter().filter(|(at, _)| if own { at == file } else { at.ends_with(".h") }).map(|(_, ty)| resolve(ty));
        let first = lists.next()??;
        if !lists.all(|list| list == Some(first)) {
            return None;
        }
        first.get(position)?.clone()
    }

    /// The honest kind of a number by the calls it stands in, whatever their
    /// parameters are named: a place or size on the map grid in a map-copy
    /// call (`MapGrid_CopyCells(32, 24, 2, 2, 30, 24)`) or beside a cell the
    /// grid's own macro gives (`FIELD_CELL(x) == 15`), a place in an OAM word
    /// (`OBJ_X(560 - digits * 10)`), and the animation an actor's sheet
    /// numbers for itself (`Actor_SetAnimation(ACTOR_ALEX, 13)`).
    /// Decimal only, never a shift's amount, as S3 writes these.
    fn honest_by_call(&self, candidate: &Candidate) -> Option<&'static str> {
        if !numbers::is_decimal(&candidate.literal) || candidate.shift {
            return None;
        }
        if candidate.calls.iter().any(|(callee, _)| matches!(callee.as_str(), "OBJ_X" | "OBJ_Y")) {
            return Some("position");
        }
        // Degrees reckoned for DEGtoANG, S3's composed angle: `DEGtoANG(i * 60)`.
        if candidate.calls.first().is_some_and(|(callee, _)| callee == "DEGtoANG") {
            return Some("composed");
        }
        // A colour channel's level, offsetting or compared with the channel
        // RGB555's own macro takes out: `RGB_BLUE(value) - 20`.
        if candidate.operand_calls.iter().any(|callee| matches!(callee.as_str(), "RGB_RED" | "RGB_GREEN" | "RGB_BLUE")) {
            return Some("level");
        }
        // A level written to the brightness register (`Io_Write16(31,
        // &REG_BLDY)`), GBATEK's fade coefficient.
        if candidate.register.as_deref().is_some_and(|register| numbers::named_kind([register]) == Some("level")) {
            return Some("level");
        }
        // Compared with a cell or tile of the map grid, or offsetting one:
        // `FIELD_CELL(actor->x.fixed) == 15`, `FIELD_CELL(x) - 42`.
        if candidate.operand_calls.iter().any(|callee| self.grid_conversions.contains(callee)) {
            return Some("position");
        }
        let (callee, at) = candidate.calls.first()?;
        if callee.starts_with("MapGrid_Copy") || callee.starts_with("Map_Copy") {
            return Some("position");
        }
        let words = numbers::words(callee);
        let animation = words.iter().any(|word| word == "animation") && words.iter().all(|word| !matches!(word.as_str(), "speed" | "rate"));
        (animation && words.iter().any(|word| word == "set") && *at == 1).then_some("animation")
    }

    /// The honest kind of a number by the parameter it fills, where its own
    /// tokens showed none: the argument of a random roll; a parameter whose
    /// words `parameter_words` reads; a decimal offset in a position or
    /// duration argument (`attendantX - 16`, never a shift's amount); and
    /// data passed through a pointer or a call layer. A lone argument takes
    /// any literal there, as `data_argument` does; anything else a decimal.
    fn honest_by_parameter(&self, candidate: &Candidate) -> Option<&'static str> {
        let plain = candidate.lone || (numbers::is_decimal(&candidate.literal) && !candidate.shift);
        let data = |callee: &str, name: &str| data_parameter(name).is_some() || self.parameter_words(callee, name).is_some();
        // Through S3's conversions to the parameter their result fills:
        // `toFIXED(118 - offset)` as `BattleCamera_SetRange`'s `centerY`.
        let mut calls = candidate.calls.iter().peekable();
        while calls.next_if(|(callee, _)| numbers::CONVERSIONS.contains(&callee.as_str()) && !candidate.lone).is_some() {}
        if let Some((callee, at)) = calls.next().or(candidate.calls.first()) {
            if numbers::random_call(callee, candidate.random_arguments) {
                return Some("random");
            }
            match self.parameter(callee, *at, 0) {
                Some((name, kind)) => {
                    if let Some(kind) = self.parameter_words(callee, &name).filter(|_| plain) {
                        return Some(kind);
                    }
                    if !candidate.lone && plain {
                        match kind {
                            Some("coordinate" | "size") => return Some("position"),
                            Some("frames") => return Some("duration"),
                            _ => {}
                        }
                    }
                }
                // `draw(canvas, ...)` with `DrawRectangle draw`.
                None => {
                    if self.pointer_parameter(callee, *at, false, &candidate.file).is_some_and(|name| data(callee, &name)) && plain {
                        return Some("pointer");
                    }
                }
            }
            if let Some((target, place)) = self.forward(callee, *at, &candidate.siblings) {
                if self.parameter(&target, place, 0).is_some_and(|(name, kind)| kind.is_some() || data(&target, &name)) && plain {
                    return Some("pointer");
                }
            }
        }
        let (variable, at, cast) = candidate.pointer.as_ref()?;
        (plain && self.pointer_parameter(variable, *at, *cast, &candidate.file).is_some_and(|name| data(variable, &name))).then_some("pointer")
    }

    /// Settle what needs the whole game: numbers in code by the parameters
    /// they fill, documentation across declarations, and the duplicate groups.
    /// A lone argument whose parameter holds data is data, as it always was;
    /// every other number counts among all the numbers in code
    /// (`magic_code_all`), and is honest (`honest_<kind>`) where its tokens or
    /// the parameter it fills show it to be, else a number to name
    /// (`magic_code`).
    fn finish(&mut self) {
        // A parameter the chosen declaration names without saying what it
        // holds takes the name another declaration gives it that does say:
        // `BattleEffect_SetApproachMotion(..., s32 rate)`, written where it
        // divides by 100, is the header's `percent`.
        for (function, lists) in std::mem::take(&mut self.parameter_lists) {
            let Some(chosen) = self.parameters.get(&function).cloned() else { continue };
            let says = |report: &Report, name: &str| data_parameter(name).is_some() || report.parameter_words(&function, name).is_some() || numbers::is_other(name);
            let mut merged = chosen.clone();
            for (position, name) in chosen.iter().enumerate() {
                if name.as_deref().is_some_and(|name| says(self, name)) {
                    continue;
                }
                let other = lists.iter().filter(|list| list.len() == chosen.len()).filter_map(|list| list[position].clone()).find(|other| says(self, other));
                if let Some(other) = other {
                    merged[position] = Some(other);
                }
            }
            self.parameters.insert(function, merged);
        }
        // A hex number named alone is a mask or a bit, which S3 keeps hex,
        // where its name says so (`CELL_CODE_BITS`, `TRIGGER_KIND_MASK`), or
        // where it is written beside `&`, `|`, `^` or `~` (`flags &
        // PLACE_ENEMY_SIDE`) and never reckoned with as a quantity
        // (`front <= CANVAS_SIZE + WIPE_LAG_MAX` is a distance).
        for (file, line, text, name, routine) in std::mem::take(&mut self.hex_definitions) {
            let said = numbers::words(&name).iter().any(|word| matches!(word.as_str(), "mask" | "bit" | "bits"));
            // A single bit stays a bit where it is also reckoned with
            // (`SCOOPED_COINS + 1`, `EPILOGUE_NO_PORTRAIT - 1`, the bits
            // below it).
            let single = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).and_then(|digits| u64::from_str_radix(digits.trim_end_matches(['u', 'U', 'l', 'L']), 16).ok()).is_some_and(u64::is_power_of_two);
            let used = self.bitwise_names.contains(&name) && (single || !self.arithmetic_names.contains(&name));
            if said || used {
                self.add("hex_mask", &file, 1);
                continue;
            }
            // An instruction's encoding or a CRC's polynomial, by its name;
            // else as someone who read it judged it, by its row.
            match self.judged(&file, routine.as_deref(), &text) {
                _ if is_encoding(&name) => self.add("hex_encoding", &file, 1),
                Some(true) => self.note("hex_judged", &file, line, &text),
                Some(false) => {
                    self.note("hex_other", &file, line, &text);
                    self.note("hex_unsettled", &file, line, &text);
                }
                None => self.note("hex_other", &file, line, &text),
            }
        }
        for candidate in std::mem::take(&mut self.candidates) {
            let (file, line, literal) = (&candidate.file, candidate.line, &candidate.literal);
            let what = match candidate.calls.first() {
                Some((callee, at)) => match self.parameter(callee, *at, 0) {
                    Some((name, _)) => format!("{literal}\t{callee} {name}"),
                    None => format!("{literal}\t{callee} #{}", at + 1),
                },
                None => format!("{literal}\t-"),
            };
            if candidate.lone {
                if let Some(kind) = self.argument_kind(&candidate.calls).or_else(|| self.plain_speed(&candidate).then_some("speed")) {
                    self.note("data_argument", file, line, &what);
                    self.note(&format!("data_argument_{kind}"), file, line, &what);
                    continue;
                }
            }
            let what = if candidate.lone { what } else { literal.clone() };
            self.note("magic_code_all", file, line, &what);
            // Judged by someone who read it, by its row: honest, or, where no
            // rule finds it honest, still to name and unsettled, counted
            // apart among those.
            let judged = self.judged(file, candidate.routine.as_deref(), literal);
            // A position or duration reckoned in an argument whose parameter
            // is a rate, scale or angle is that, not a position: `Trig_Sin(row
            // * 3553)`, `SceneTransform_ApplyScale3(frame * 1500 - ...)`; one
            // reckoned in a group of its own first stays what it is
            // (`Trig_Sin((frame - 486) * STEP)`).
            let parameter = candidate.calls.first().and_then(|(callee, at)| self.parameter(callee, *at, 0));
            let vetoed = !candidate.grouped && parameter.is_some_and(|(name, _)| numbers::is_other(&name));
            let local = candidate.local.filter(|kind| !(vetoed && matches!(*kind, "position" | "duration")));
            // A mask in an OAM word's place is the field io_reg.h names
            // (`OBJ_X_MASK`).
            let oam = candidate.calls.iter().any(|(callee, _)| matches!(callee.as_str(), "OBJ_X" | "OBJ_Y"));
            let local = local.filter(|kind| !(oam && *kind == "mask"));
            // A number in a function-like macro's own body is the name's
            // definition: the macro is the name S3 asks for.
            let definition = candidate.definition.then_some("definition");
            // An amount in a debug room's own code (`FieldUnit_AddHp(ISAAC,
            // -100)`, `coins = 777777`): a test's figure, nothing the game
            // plays.
            let debug = (numbers::is_decimal(literal) && is_debug_room(file)).then_some("debug");
            let honest = match judged {
                Some(true) => Some("judged"),
                _ => local.or_else(|| self.honest_by_call(&candidate)).or_else(|| self.honest_by_parameter(&candidate)).or(definition).or(debug),
            };
            match honest {
                Some(kind) => self.note(&format!("honest_{kind}"), file, line, &what),
                None => {
                    // The callee and the parameter it fills, so the next
                    // reader sees why it is counted: `ShipRooms_RunEventCommand
                    // opt`, `F #3 (unnamed)`.
                    let described = match candidate.calls.first() {
                        Some((callee, at)) => match self.parameter(callee, *at, 0) {
                            Some((name, _)) => format!("{literal}\t{callee} {name}"),
                            None => format!("{literal}\t{callee} #{} (unnamed)", at + 1),
                        },
                        None => literal.clone(),
                    };
                    self.note("magic_code", file, line, &described);
                    if candidate.lone {
                        self.note("magic_argument", file, line, &what);
                    }
                    if judged == Some(false) {
                        self.note("magic_unsettled", file, line, &described);
                    }
                }
            }
        }
        self.shared_parameters();
        // Each list in file and line order, as the files were read.
        self.items.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
        // A far call, `XFar`, is documented by X's comment.
        let far = |functions: &BTreeMap<String, Declared>, name: &str| {
            name.strip_suffix("Far").and_then(|routine| functions.get(routine)).is_some_and(|routine| routine.documented)
        };
        // A routine is said where it has its `///`, as a far call by its
        // target's, or as the compiler's, the C library's or the BIOS's.
        let said = |functions: &BTreeMap<String, Declared>, name: &str| {
            functions.get(name).is_some_and(|routine| routine.documented) || far(functions, name) || is_library_routine(name)
        };
        // The files another includes as text, whose macro-named declarations
        // are the names their includers give them.
        let by_suffix = self.suffixes();
        let included: HashSet<String> = self.includes.iter().filter_map(|(file, spec)| self.resolve(&by_suffix, file, spec)).map(str::to_string).collect();
        drop(by_suffix);
        let families = self.numbered_families();
        for (function, kind) in [(true, "undocumented_function"), (false, "undocumented_global")] {
            let missing: Vec<(String, usize, String, &str)> = (if function { &self.functions } else { &self.globals })
                .iter()
                .filter(|(_, declared)| !declared.documented)
                .map(|(name, declared)| {
                    let apart = if function && far(&self.functions, name) { Some("undocumented_far") } else { declared.apart };
                    let apart = apart.or_else(|| (!function && self.id_externs.contains(name)).then_some("undocumented_id"));
                    let apart = apart.or_else(|| (function && is_library_routine(name)).then_some("undocumented_library"));
                    // A forwarder: its body one call to a routine said
                    // elsewhere (`return Party_ListMembers(owners);`).
                    let forwards = declared.forward.as_deref().is_some_and(|target| target != name && said(&self.functions, target));
                    let apart = apart.or_else(|| (function && forwards).then_some("undocumented_forward"));
                    // A message's extern (`extern u8 MsgCanYouImagine[];`): its
                    // text in the game's .po files says it, as a string's
                    // initializer does.
                    let apart = apart.or_else(|| (!function && self.message_symbols.contains(name)).then_some("undocumented_string"));
                    // A shared file's parameter (`extern s16 WAVE_SCROLL_LINES[];`):
                    // a name a macro gives, declared in a file another
                    // includes.
                    let shared = apart.is_none() && is_macro_name(name) && self.macro_docs.contains_key(name) && included.contains(&declared.file);
                    // A member of a numbered family (`Call1` to `Call11`): in
                    // its head's run where the head has its `///`, else the
                    // family counted once, at its head.
                    let family = families.get(&(function, name.clone())).filter(|_| apart.is_none() && !shared);
                    match (shared, family) {
                        (true, _) => {
                            let (metric, what) = self.parameter_said(name, &declared.file);
                            (declared.file.clone(), declared.line, what, if metric == "undocumented_macro" { kind } else { metric })
                        }
                        (false, Some((true, _))) => (declared.file.clone(), declared.line, name.clone(), "undocumented_run"),
                        (false, Some((false, members))) if members.first() == Some(name) => {
                            (declared.file.clone(), declared.line, format!("{name}\ta family without a head: {}", members.join(", ")), kind)
                        }
                        (false, Some((false, _))) => (declared.file.clone(), declared.line, name.clone(), "undocumented_family"),
                        (false, None) => (declared.file.clone(), declared.line, name.clone(), apart.unwrap_or(kind)),
                    }
                })
                .collect();
            for (file, line, name, kind) in missing {
                self.note(kind, &file, line, &name);
            }
        }
        // A typed view of one name (`((struct WindowWork *)gWindowWork[0])`),
        // that name said where it is declared: through a cast, any routine,
        // global or macro; as it is, a global (`#define gLzPackWork
        // gWaitForFlashWrite`), since a routine's bare name is a parameter of
        // what it is passed to.
        for (file, line, name, target, casts) in std::mem::take(&mut self.views) {
            let global = self.globals.get(&target).is_some_and(|global| {
                global.documented || global.apart.is_some_and(|apart| !matches!(apart, "undocumented_plain" | "undocumented_note"))
            });
            // A routine viewed under its own name (`#define Actor_StartIdleFar
            // ((void (*)(struct MotionObject *))Actor_StartIdleFar)`) is the
            // routine's.
            let other = said(&self.functions, &target) || (target != name && self.macro_docs.get(&target).copied().unwrap_or(false));
            let viewed = (target != name && global) || (casts > 0 && other);
            self.note(if viewed { "undocumented_view" } else { "undocumented_macro" }, &file, line, &name);
        }
        // D9 asks every entry point for its summary; any other declaration only
        // where it has something to say, which no count can tell.
        let entries: Vec<(String, usize, String)> = self
            .functions
            .iter()
            .filter(|(name, declared)| declared.entry && !declared.documented && !far(&self.functions, name))
            .map(|(name, declared)| (declared.file.clone(), declared.line, name.clone()))
            .collect();
        for (file, line, name) in entries {
            self.note("undocumented_entry", &file, line, &name);
        }
        self.unused_placeholders();
        let objects = self.objects();
        self.never_run(&objects);
        let moved = std::mem::take(&mut self.moved);
        self.at_origin.retain(|actor| !moved.contains(actor));
        // Each routine's copies of the same body, its parameters and locals
        // renamed aside, in other files.
        let mut bodies: HashMap<&[String], Vec<(String, String)>> = HashMap::new();
        for copy in self.copies.iter().filter(|copy| copy.function && !copy.tokens.is_empty()) {
            bodies.entry(&copy.tokens[..]).or_default().push((copy.file.clone(), copy.name.clone()));
        }
        for members in bodies.values().filter(|members| members.len() > 1) {
            for member in members {
                let others: Vec<(String, String)> = members.iter().filter(|other| other.0 != member.0).cloned().collect();
                if !others.is_empty() {
                    self.same_body.entry(member.clone()).or_default().extend(others);
                }
            }
        }
        let referred = std::mem::take(&mut self.macro_words);
        (self.function_groups, self.table_groups, self.said_groups) =
            duplicates::groups(&std::mem::take(&mut self.copies), FUNCTION_TOKENS, TABLE_TOKENS, &objects, &referred);
        // A group someone judged, by its row, is counted apart.
        for groups in [&mut self.function_groups, &mut self.table_groups] {
            let mut kept = Vec::new();
            for group in std::mem::take(groups) {
                match self.duplicate_rows.get(&group_key(&group)) {
                    Some((kind, why)) => self.judged_groups.push((group, kind.clone(), why.clone())),
                    None => kept.push(group),
                }
            }
            *groups = kept;
        }
        self.long_files.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    }

    /// Sets apart a placeholder nothing uses (`placeholder_unused`): its one
    /// use in the C and headers is where it is defined (an enumerator, a
    /// macro, a global, a function), and no assembly or macro body writes
    /// it. Its table row and its file list are that definition and the
    /// linker's (`SE_120,` in `sounds.h`, a sound nothing plays): S3 keeps
    /// an unknown a placeholder, and with nothing using it nothing will
    /// settle it.
    fn unused_placeholders(&mut self) {
        let Some(names) = self.names.get("placeholder").cloned() else { return };
        let mut uses: HashMap<&str, Vec<usize>> = HashMap::new();
        for (at, item) in self.items.iter().enumerate() {
            if item.metric == "placeholder" {
                uses.entry(item.what.as_str()).or_default().push(at);
            }
        }
        let defined: HashSet<(&str, &str, usize)> = self
            .naming
            .names
            .iter()
            .filter(|name| {
                matches!(name.kind, NameKind::Enumerator | NameKind::Macro | NameKind::FunctionMacro | NameKind::Global | NameKind::Function)
            })
            .map(|name| (name.name.as_str(), name.file.as_str(), name.line))
            .collect();
        let mut unused = Vec::new();
        for name in &names {
            let Some(&[at]) = uses.get(name.as_str()).map(Vec::as_slice) else { continue };
            let item = &self.items[at];
            if defined.contains(&(name.as_str(), item.file.as_str(), item.line)) && !self.macro_words.contains(name) {
                unused.push((name.clone(), at));
            }
        }
        for (name, at) in unused {
            let file = self.items[at].file.clone();
            self.items[at].metric = "placeholder_unused".to_string();
            if let Some(files) = self.tallies.get_mut("placeholder") {
                if let Some(count) = files.get_mut(&file) {
                    *count -= 1;
                    if *count == 0 {
                        files.remove(&file);
                    }
                }
            }
            self.add("placeholder_unused", &file, 1);
            self.names.entry("placeholder").or_default().remove(&name);
            self.names.entry("placeholder_unused").or_default().insert(name);
        }
    }

    /// The routines nothing runs (`never_run`): marked `UNUSED`, or one a
    /// scene's overlay writes only where it defines it, in all the files it
    /// includes, and that no macro or assembly names.
    fn never_run(&mut self, objects: &HashMap<String, BTreeSet<String>>) {
        for (file, name, unused) in std::mem::take(&mut self.scene_routines) {
            let written: usize = objects
                .iter()
                .filter(|(_, includers)| includers.contains(&file))
                .filter_map(|(part, _)| self.scene_names.get(part).and_then(|names| names.get(&name)))
                .sum();
            if unused || (is_map_overlay(&file) && written == 1 && !self.macro_words.contains(&name)) {
                self.never_run.insert((file, name));
            }
        }
        self.scene_names.clear();
    }

    /// The objects each file is part of: the files that include it, through
    /// others or not, and itself. An include is found beside the file that
    /// writes it, else under the game's `src` or `include`.
    fn objects(&self) -> HashMap<String, BTreeSet<String>> {
        let by_suffix = self.suffixes();
        let mut includers: HashMap<&str, Vec<&str>> = HashMap::new();
        for (file, spec) in &self.includes {
            if let Some(found) = self.resolve(&by_suffix, file, spec) {
                includers.entry(found).or_default().push(file);
            }
        }
        let mut objects = HashMap::new();
        for path in &self.paths {
            let mut seen = BTreeSet::from([path.clone()]);
            let mut next = vec![path.as_str()];
            while let Some(file) = next.pop() {
                for &by in includers.get(file).into_iter().flatten() {
                    if seen.insert(by.to_string()) {
                        next.push(by);
                    }
                }
            }
            objects.insert(path.clone(), seen);
        }
        objects
    }

    /// The numbered families of routines and of globals: two or more names
    /// one stem and a number apart (`Call1`, `Call2`, ..., `Call11`), declared
    /// in one file with no other routine or global between them. Each
    /// member, by whether it is a routine and its name, with whether the
    /// first (its head) has its `///`, and the members in file order.
    fn numbered_families(&self) -> HashMap<(bool, String), (bool, Vec<String>)> {
        let mut groups: BTreeMap<(bool, &str, &str), Vec<(usize, &str, bool)>> = BTreeMap::new();
        let mut declared: HashMap<&str, Vec<usize>> = HashMap::new();
        for (function, map) in [(true, &self.functions), (false, &self.globals)] {
            for (name, entry) in map {
                declared.entry(entry.file.as_str()).or_default().push(entry.line);
                let stem = name.trim_end_matches(|c: char| c.is_ascii_digit());
                if stem.len() < name.len() && stem.ends_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
                    groups.entry((function, entry.file.as_str(), stem)).or_default().push((entry.line, name.as_str(), entry.documented));
                }
            }
        }
        let mut families = HashMap::new();
        for ((function, file, _), mut members) in groups {
            members.sort();
            let (first, last) = (members[0].0, members[members.len() - 1].0);
            let between = declared.get(file).map_or(0, |lines| lines.iter().filter(|line| (first..=last).contains(*line)).count());
            if members.len() < 2 || between != members.len() {
                continue;
            }
            let names: Vec<String> = members.iter().map(|(_, name, _)| name.to_string()).collect();
            for name in &names {
                families.insert((function, name.clone()), (members[0].2, names.clone()));
            }
        }
        families
    }

    /// Every file read, by each tail of its path (`field/call.h`, `call.h`),
    /// for `resolve`.
    fn suffixes(&self) -> HashMap<&str, Vec<&str>> {
        let mut by_suffix: HashMap<&str, Vec<&str>> = HashMap::new();
        for path in &self.paths {
            for (at, _) in path.match_indices('/') {
                by_suffix.entry(&path[at + 1..]).or_default().push(path);
            }
        }
        by_suffix
    }

    /// The file an `#include` of `spec` in `file` reads: beside `file`, else
    /// under the game's `src` or `include`.
    fn resolve<'s>(&'s self, by_suffix: &HashMap<&str, Vec<&'s str>>, file: &str, spec: &str) -> Option<&'s str> {
        let beside = file.rsplit_once('/').map_or(spec.to_string(), |(folder, _)| normalized(&format!("{folder}/{spec}")));
        self.paths.get(&beside).map(String::as_str).or_else(|| {
            let candidates = by_suffix.get(spec)?;
            let game = file.split('/').take(2).collect::<Vec<_>>().join("/");
            let mut own = candidates.iter().filter(|path| [format!("{game}/src/{spec}"), format!("{game}/include/{spec}")].iter().any(|full| **path == full));
            own.next().copied()
        })
    }

    /// Settles each macro defined right before an `#include`: where the
    /// included file uses its name, a parameter of that file, set apart
    /// (`undocumented_shared`) when that file's comments name it and else
    /// counted with that file given as where to say it; any other, as an
    /// undocumented macro is.
    fn shared_parameters(&mut self) {
        let candidates = std::mem::take(&mut self.parameter_candidates);
        let by_suffix = self.suffixes();
        let mut settled = Vec::new();
        for (file, line, name, spec, view) in candidates {
            let shared = self.resolve(&by_suffix, &file, &spec).filter(|shared| self.file_words.get(*shared).is_some_and(|(idents, _)| idents.contains(&name)));
            settled.push(match shared {
                Some(shared) => Ok((file, line, self.parameter_said(&name, shared))),
                None => Err((file, line, name, view)),
            });
        }
        for found in settled {
            match found {
                Ok((file, line, (metric, what))) => self.note(metric, &file, line, &what),
                Err((file, line, name, Some((target, casts)))) => self.views.push((file, line, name, target, casts)),
                Err((file, line, name, None)) => self.note("undocumented_macro", &file, line, &name),
            }
        }
    }

    /// How a parameter of a shared file is counted: apart where the shared
    /// file's comments name it, else to review, the shared file given as
    /// where to say it.
    fn parameter_said(&self, name: &str, shared: &str) -> (&'static str, String) {
        if self.file_words.get(shared).is_some_and(|(_, said)| said.contains(name)) {
            ("undocumented_shared", name.to_string())
        } else {
            ("undocumented_macro", format!("{name}\ta parameter of {shared}, whose comments do not name it"))
        }
    }

    /// Copies past the first in each group.
    pub fn copies(groups: &[Group]) -> usize {
        groups.iter().map(|group| group.members.len() - 1).sum()
    }

    /// The rows of `evidence/<game>-duplicates.tsv` that judge no group the
    /// count finds: a member gone, or the members no longer one group. Each
    /// with what is wrong.
    pub fn stale_duplicates(&self) -> Vec<(String, String)> {
        let found: HashSet<String> =
            self.function_groups.iter().chain(&self.table_groups).chain(&self.said_groups).chain(self.judged_groups.iter().map(|(group, ..)| group)).map(group_key).collect();
        let mut out = Vec::new();
        for (members, (kind, _)) in &self.duplicate_rows {
            if found.contains(members) {
                continue;
            }
            let gone: Vec<&str> = members.split(' ').filter(|name| !self.functions.contains_key(*name) && !self.globals.contains_key(*name)).collect();
            let what = if gone.is_empty() { "no longer one group".to_string() } else { format!("{} gone", gone.join(", ")) };
            out.push((members.clone(), format!("{kind}: {what}")));
        }
        out.sort();
        out
    }
}

/// A duplicate group's key, as its row of `evidence/<game>-duplicates.tsv`
/// writes it: its members' names, without their files, sorted and joined
/// with a space.
pub fn group_key(group: &Group) -> String {
    let mut names: Vec<&str> = group.members.iter().map(|member| member.rsplit_once(':').map_or(member.as_str(), |(_, name)| name)).collect();
    names.sort_unstable();
    names.join(" ")
}

/// Whether a line is a comment's (`//`, `/*`, a block's inner or closing
/// line), a `clang-format` switch aside.
fn is_comment_line(line: &str) -> bool {
    let line = line.trim();
    (line.starts_with("//") || line.starts_with("/*") || line.starts_with('*')) && !line.contains("clang-format")
}

/// Whether a line is an `#if`, `#ifdef`, `#ifndef`, `#elif`, `#else` or
/// `#endif`.
fn is_condition_line(line: &str) -> bool {
    let rest = line.trim_start().strip_prefix('#').map(str::trim_start).unwrap_or("");
    ["if", "elif", "else", "endif"].iter().any(|word| rest.starts_with(word))
}

/// The comment heading a run of declarations: `///`, a `//` to be marked
/// `///`, or a `//` note D9 keeps as it is (`// UB:`, `// BUG:`, `// DEAD:`,
/// an `XXX` question, a FAKEMATCH tag), which heads nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Head {
    Doc,
    Plain,
    Note,
}

/// Whether a comment is one D9 keeps as `//` over a declaration: a note of
/// undefined behaviour, a bug or dead code, an open question or a fake
/// match's tag.
fn is_kept_note(comment: &str) -> bool {
    ["// UB:", "// BUG:", "// DEAD:", "XXX", "FAKEMATCH("].iter().any(|mark| comment.contains(mark))
}

/// The first line of the logical line `at` is part of, past the lines a
/// `\` continues.
fn logical_start(lines: &[&str], mut at: usize) -> usize {
    while at > 0 && lines[at - 1].trim_end().ends_with('\\') {
        at -= 1;
    }
    at
}

/// The comment block right above line `at` (from 0), past `#if` lines (a
/// condition written over several lines too) and `clang-format` switches:
/// its kind, if there is one.
fn head_above(lines: &[&str], mut above: usize) -> Option<Head> {
    while above > 0 {
        let start = logical_start(lines, above - 1);
        if is_condition_line(lines[start]) || lines[start].contains("clang-format") {
            above = start;
        } else {
            break;
        }
    }
    let mut head = None;
    while above > 0 && is_comment_line(lines[above - 1]) {
        above -= 1;
        let line = lines[above].trim_start();
        let own = if line.starts_with("///") || line.starts_with("/**") {
            Head::Doc
        } else if is_kept_note(line) {
            Head::Note
        } else {
            Head::Plain
        };
        head = Some(match (head, own) {
            (Some(Head::Doc), _) | (_, Head::Doc) => Head::Doc,
            (Some(Head::Note), _) | (_, Head::Note) => Head::Note,
            _ => Head::Plain,
        });
    }
    head
}

/// The comment over the run of `#define`s each `#define` line belongs to,
/// by line (from 1). A run is consecutive `#define`s, their continuation
/// lines and `#if` lines between them; a blank line, a comment or any other
/// line ends it. Its comment is the one right above it, past `#if` lines
/// (a `///` over an `#if` heads the run inside it) and `clang-format` lines.
fn define_heads(text: &str) -> HashMap<usize, Head> {
    let lines: Vec<&str> = text.lines().collect();
    let is_define = |line: &str| line.trim_start().strip_prefix('#').is_some_and(|rest| rest.trim_start().starts_with("define"));
    let mut heads = HashMap::new();
    let mut at = 0;
    while at < lines.len() {
        if !is_define(lines[at]) {
            at += 1;
            continue;
        }
        let head = head_above(&lines, at);
        let mut members = Vec::new();
        while at < lines.len() && (is_define(lines[at]) || is_condition_line(lines[at])) {
            if is_define(lines[at]) {
                members.push(at + 1);
            }
            while at < lines.len() && lines[at].trim_end().ends_with('\\') {
                at += 1;
            }
            at += 1;
        }
        if let Some(head) = head {
            heads.extend(members.into_iter().map(|line| (line, head)));
        }
    }
    heads
}

/// A path with its `..` steps taken: `src/scene/../field/a.c` is `src/field/a.c`.
fn normalized(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            "." => {}
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

fn regexes() -> &'static [Regex; 3] {
    static SET: OnceLock<[Regex; 3]> = OnceLock::new();
    SET.get_or_init(|| {
        [
            // Placeholders.
            Regex::new(concat!(
                r"^(?:(?:unk|Unk|UNK|[gs]Unk|[gs]UNK)(?:nown)?(?:$|[_0-9A-Z]))",
                r"|^(?:sub|func|Func|data|loc|off|byte|word|dword|nullsub|D)_[0-9A-Fa-f]{2,}",
                r"|^field_[0-9A-Fa-f]+",
                r"|[_a-z0-9]Unk(?:nown)?(?:$|[_0-9A-Z])|_unk(?:$|[_0-9])|_UNK(?:$|[_0-9])",
                r"|_0[23689][0-9A-Fa-f]{6}(?:$|_)",
                r"|^(?:SE|MUS|SPRITE|ITEM|ABILITY|ENEMY|DJINNI)_[0-9]+$|^nullsub",
            ))
            .unwrap(),
            // Conventions that keep their number: temporary flags, registers and
            // the compiler library (`__divsi3`).
            Regex::new(r"^FLAG_(?:SCENE|MAPSEC|AREA)_TEMP_[0-9A-F]+$|^REG_|^__").unwrap(),
            // Edition names, which belong in linker scripts and assembly.
            Regex::new(r"^(?:TBS|TLA)_EDITION_[A-Z]+$").unwrap(),
        ]
    })
}

/// `unk_14`, `sub_08012345`, `gUnknown_0203`, `field_1C`, `SE_138`, the id
/// headers' pret-style row placeholders (`ITEM_034`): a name that says it is
/// not known yet.
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
/// `Vale_LeaveByExit10`, `ACTOR_8`. Placeholders, hardware and convention
/// names (`BG2`, `Dma3`, `REG_DMA3`, `u16`, `Random16`, `FLAG_SCENE_TEMP_01`)
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

/// Whether a file is a debug room's code: a test scene under `src/debug/`
/// (`test.c`, `psynergy_test.c`, `psynergy_test_2.c`, `shop_test.c`), not its
/// tables of scenes or rows (`test_scenes.c`) or the level tool.
fn is_debug_room(path: &str) -> bool {
    let Some((folder, name)) = path.rsplit_once('/') else { return false };
    let stem = name.strip_suffix(".c").unwrap_or("");
    let stem = stem.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end_matches('_');
    folder.ends_with("/src/debug") && (stem == "test" || stem.ends_with("_test"))
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

/// Whether a function-like macro's body only composes a number from its
/// parameters, as S3's composed forms do: parameters, numbers and constants
/// joined by shifts, ors and multiplies (`((value) << 16)`,
/// `(((b) << 10) | ((g) << 5) | (r))`, `((DMA_ENABLE | DMA_32BIT) << 16 | (n))`).
/// SGL's `toFIXED` and `DEGtoANG` are compositions too: a multiply typed
/// `s32`, and a multiply divided by a constant.
/// An offset (`0x180 + (group)`, an id's range), a pointer cast, an index, a
/// call or an assignment is not a composition: what it is passed may be an id.
fn is_composition<'a>(parameters: &[&str], body: impl Iterator<Item = &'a str>) -> bool {
    let (mut operator, mut used, mut name) = (false, false, false);
    for word in body.filter(|&word| word != "\\") {
        let was_name = std::mem::replace(&mut name, false);
        match word {
            // A `(` after a name is a call.
            "(" if was_name => return false,
            "(" | ")" => {}
            "<<" | "|" | "*" => operator = true,
            // A divisor (`(degrees) * TRIG_TURN / 360`) and the whole number's type
            // (`(s32)((value) * FIXED_ONE)`) change nothing of what is composed.
            "/" | "s32" => {}
            _ if parameters.contains(&word) => used = true,
            _ if word.starts_with(|c: char| c.is_ascii_digit()) => {}
            // A constant: `DMA_ENABLE`.
            _ if is_macro_name(word) && word.starts_with(|c: char| c.is_ascii_alphabetic()) => name = true,
            _ => return false,
        }
    }
    operator && used
}

/// Whether a function-like macro brings a coordinate down to the map grid:
/// its name says cell or tile, its one parameter a coordinate, position or
/// height, and its body only scales that down by a shift or a divisor
/// (`FIELD_CELL(coordinate) ((coordinate) >> FIELD_CELL_SHIFT)`,
/// `TILE(position) ((position) / toFIXED(16))`, `CELL_HEIGHT_STEPS(height)`).
/// A cell's code or a packed word's tile (`CELL_CODE(attributes)`) is no
/// place.
fn is_grid_conversion<'a>(name: &str, parameters: &[&str], body: impl Iterator<Item = &'a str>) -> bool {
    let named = numbers::words(name).iter().any(|word| matches!(word.as_str(), "cell" | "tile"));
    let [parameter] = parameters else { return false };
    let coordinate = numbers::words(parameter).iter().any(|word| matches!(word.as_str(), "coordinate" | "position" | "height"));
    if !named || !coordinate {
        return false;
    }
    let (mut scaled, mut used) = (false, false);
    for word in body.filter(|&word| word != "\\") {
        match word {
            ">>" | "/" => scaled = true,
            // A shift less one: a half cell's (`FIELD_CELL_SHIFT - 1`).
            "(" | ")" | "u32" | "s32" | "toFIXED" | "INTtoFIXED" | "-" | "+" => {}
            _ if word == *parameter => used = true,
            _ if word.starts_with(|c: char| c.is_ascii_digit()) => {}
            _ if is_macro_name(word) => {}
            _ => return false,
        }
    }
    scaled && used
}

/// A hex number token's value.
/// What each parameter of a file's own function-like macro numbers, from its
/// body: a step of a route the macro writes as a `case` (`#define WALK(state_,
/// x, z) case state_: ... *state = state_ + 1;`), or the step it goes on to
/// (`ARRIVE(state_, next)`'s `*state = next;`), and the index of a macro that
/// counts from a named first (`#define STAKE(n) (ACTOR_FIRST_PLACED + (n))`).
fn own_macro_kinds(names: &[&str], body: &[&str]) -> Vec<Option<&'static str>> {
    let cased = body.contains(&"case");
    let indexed = |parameter: &str| {
        let base = |text: &str| text.starts_with(|c: char| c.is_ascii_uppercase()) && is_macro_name(text);
        match body {
            ["(", first, "+", "(", p, ")", ")"] | [first, "+", "(", p, ")"] | ["(", first, "+", p, ")"] | [first, "+", p] => base(first) && *p == parameter,
            _ => false,
        }
    };
    names
        .iter()
        .map(|&parameter| {
            let step = body.windows(3).any(|w| (w[0] == "case" && w[1] == parameter && w[2] == ":") || (cased && w[0] == "=" && w[1] == parameter && w[2] == ";"));
            if step {
                Some("step")
            } else if names.len() == 1 && indexed(parameter) {
                Some("index")
            } else {
                None
            }
        })
        .collect()
}

fn hex_value(token: &Token) -> Option<u64> {
    let digits = token.text.strip_prefix("0x").or_else(|| token.text.strip_prefix("0X"))?;
    (token.kind == Kind::Number).then(|| u64::from_str_radix(digits.trim_end_matches(['u', 'U', 'l', 'L']), 16).unwrap_or(u64::MAX))
}

/// Whether a hex number's name says it is an instruction's encoding
/// (`THUMB_BL_HIGH`, an instruction set's name first) or a CRC's polynomial
/// (`PASSWORD_CRC_POLYNOMIAL`): fields and bits S3 keeps hex.
fn is_encoding(name: &str) -> bool {
    let words = numbers::words(name);
    words.first().is_some_and(|word| matches!(word.as_str(), "thumb" | "arm")) && name.bytes().all(|b| !b.is_ascii_lowercase())
        || words.iter().any(|word| word == "polynomial")
        || (words.iter().any(|word| word == "crc") && words.last().is_some_and(|word| matches!(word.as_str(), "poly" | "generator")))
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
    /// Every comment, `//` or `///`, between each token and the one before.
    comment_text: Vec<String>,
}

impl<'a> Code<'a> {
    fn new(text: &'a str) -> (Code<'a>, Vec<Token<'a>>) {
        let all = clex::lex(text);
        let (mut tokens, mut comments, mut doc_before, mut doc_after) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let (mut doc_text, mut trailing_text, mut comment_text) = (Vec::new(), Vec::<String>::new(), Vec::new());
        let mut pending_comments = String::new();
        let mut pending_doc = false;
        let mut pending_text = String::new();
        // The last comment was a `///<` note kept with the last token.
        let mut trailing_open = false;
        // Inside an `#if`, `#else` or `#endif` line, which a `///` above
        // a declaration reads past (`/// ...` over `#ifdef UBFIX`).
        let mut conditional = false;
        for (index, token) in all.iter().cloned().enumerate() {
            if token.kind == Kind::PpStart {
                let word = all[index + 1..].iter().find(|next| next.kind != Kind::Comment).map_or("", |next| next.text);
                conditional = matches!(word, "if" | "ifdef" | "ifndef" | "elif" | "else" | "endif");
            }
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
                    // A `//` line between a `///` block and its declaration
                    // (`// UB:`, `// clang-format off`) does not end the block.
                    let doc = clex::is_doc(token.text) && !token.pp;
                    if doc && !pending_doc {
                        pending_text.clear();
                    }
                    if doc {
                        pending_text.push_str(token.text);
                        pending_text.push('\n');
                    }
                    pending_doc |= doc;
                    trailing_open = false;
                    pending_comments.push_str(token.text);
                    pending_comments.push('\n');
                }
                comments.push(token);
                continue;
            }
            doc_before.push(pending_doc);
            doc_after.push(false);
            doc_text.push(if pending_doc { pending_text.clone() } else { String::new() });
            trailing_text.push(String::new());
            comment_text.push(std::mem::take(&mut pending_comments));
            trailing_open = false;
            if token.kind != Kind::PpEnd && !conditional {
                pending_doc = false;
                pending_text.clear();
            }
            if token.kind == Kind::PpEnd {
                conditional = false;
            }
            tokens.push(token);
        }
        (Code { tokens, doc_before, doc_after, doc_text, trailing_text, comment_text }, comments)
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
    /// The routine or definition each token is in (`clex::owners`).
    owners: Vec<Option<&'a str>>,
    stack: Vec<Brace>,
    /// For each `#if`: the open braces at it and at the end of its first branch.
    conditions: Vec<(Snapshot, Option<Snapshot>)>,
    /// `[` depth outside every brace: a declaration's array size.
    square: i32,
    statement: Vec<usize>,
    braces: HashMap<usize, Opened>,
    open: Vec<usize>,
    /// Each macro name: documented at any definition, its first line, and
    /// the row that sets it apart without a `///`.
    macros: BTreeMap<String, (bool, usize, Option<&'static str>)>,
    /// The file's lines.
    lines: Vec<&'a str>,
    /// The comment over each `#define` line's run of `#define`s: `///`
    /// (true) or `//` (false).
    heads: HashMap<usize, Head>,
    /// The name the last directive, an `#ifndef`, tested: an include guard
    /// if the next defines it.
    guard: Option<String>,
    /// The last declaration's last line, the comment over its run, and the
    /// routine its body calls if it is one call (a run of such bodies may
    /// keep a blank line between them).
    last_global: Option<(usize, Option<Head>, Option<String>)>,
    /// The last struct or union this file defined outside a declaration of
    /// anything else: its tag, whether a `///` documents it, and its last
    /// line, for a global declared right under it.
    last_type: Option<(String, bool, usize)>,
    /// Within an old-style definition's parameter declarations (`void f(x,
    /// y) s32 x; s16 y; {`), which declare no global.
    old_style: bool,
    /// Each object-like macro of this file that views one name through a
    /// cast or as it is, and how many casts (`view_target`).
    views: HashMap<String, (String, usize)>,
    /// Where each macro is first defined: its directive's first token.
    macro_starts: HashMap<String, usize>,
    /// A file-scope definer's invocation (`DEFINE_LIFT_ROCK(Name, ...)`,
    /// no semicolon): its closing parenthesis, the routine it defines, and
    /// whether a `///` documents it.
    definer: Option<(usize, String, bool)>,
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
    /// Where the directive being read starts and ends.
    directive_start: usize,
    directive_end: usize,
    /// The routine last measured: its opening and closing braces.
    routine: Option<(usize, usize)>,
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
        self.report.things.push(Thing { name: name.to_string(), kind, file, line, own, context, tag, value, alone: false, evidence: Vec::new() });
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
        if in_map_folder(self.file) {
            *self.report.scene_names.entry(self.file.to_string()).or_default().entry(name.to_string()).or_default() += 1;
            if self.stack.contains(&Brace::Func) {
                self.report.moved.insert((self.file.to_string(), name.to_string()));
            }
        }
        for (kind, test) in [("placeholder", is_placeholder as fn(&str) -> bool), ("numbered", is_numbered)] {
            if !test(name) || (kind == "placeholder" && self.report.text_unknown.contains(name)) {
                continue;
            }
            let kind = if kind == "numbered" && self.report.numbered_by_row.contains(name) { "numbered_honest" } else { kind };
            self.report.note(kind, self.file, line, name);
            self.report.names.entry(kind).or_default().insert(name.to_string());
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
        if metric == "magic_code" {
            // Whether it is data or honest may depend on the parameter it
            // fills, which may be declared in a file read later.
            let floor = if function_macro.is_some() { self.directive_start } else { 0 };
            // How far on its local may be read: its routine's end, or its
            // directive's.
            let limit = if function_macro.is_some() { self.directive_end } else { self.routine_end() };
            let tokens = &self.code.tokens;
            // The calls around it, innermost first: `PIXELS` and then `Actor_SetPosition`.
            let mut calls = Vec::new();
            let mut inside = at;
            let mut innermost = None;
            while let Some((callee, position)) = call_site(tokens, inside, floor) {
                innermost.get_or_insert(callee + 1);
                calls.push((self.code.text(callee).to_string(), position));
                inside = callee;
            }
            let (siblings, random_arguments) = innermost.map_or_else(|| (Vec::new(), false), |open| numbers::arguments(tokens, open));
            // A local array's size is a count of its own elements.
            let own = if numbers::is_array_size(tokens, at, floor) {
                Some("position")
            } else {
                numbers::local(tokens, at, floor, limit).or_else(|| numbers::is_hardware_address(literal).then_some("address"))
            };
            let own = own.or_else(|| self.own_macro_number(&calls, at, floor, limit)).or_else(|| self.chance_returned(at));
            // An index or stride continuing a routine's own, in order.
            let routine_open = self.stack.iter().position(|kind| *kind == Brace::Func).and_then(|at| self.open.get(at).copied());
            let own = own.or_else(|| {
                routine_open.filter(|_| function_macro.is_none()).filter(|&open| numbers::continues_indices(&self.code.tokens, at, open, limit)).map(|_| "index")
            });
            let tokens = &self.code.tokens;
            let candidate = Candidate {
                file: self.file.to_string(),
                line,
                literal: literal.to_string(),
                calls,
                lone: self.is_argument(at),
                local: own,
                siblings,
                random_arguments,
                pointer: numbers::pointer_call(tokens, at, floor),
                shift: at > floor && matches!(self.code.text(at - 1), "<<" | ">>"),
                definition: function_macro == Some(true),
                operand_calls: numbers::operand_calls(tokens, at, floor),
                grouped: numbers::in_group(tokens, at, floor),
                register: innermost.and_then(|open| numbers::written_register(tokens, open)).map(str::to_string),
                routine: self.owners.get(at).copied().flatten().map(str::to_string),
            };
            self.report.candidates.push(candidate);
            return;
        }
        self.report.note(metric, self.file, line, literal);
    }

    /// What a number in this file's own macros numbers: the lone argument a
    /// macro takes as a route's step or an index from its named first
    /// (`WALK(16, 232, 744)`, `STAKE(10)`, also through another macro,
    /// `STAKE_ROW(10)`), or a step of the switch those macros write (`case
    /// 17:`, `*state = 18;`).
    fn own_macro_number(&self, calls: &[(String, usize)], at: usize, floor: usize, limit: usize) -> Option<&'static str> {
        let own = self.report.own_macros.get(self.file)?;
        let mut call = calls.first().filter(|_| self.is_argument(at)).cloned();
        for _ in 0..4 {
            let Some((callee, position)) = call else { break };
            if let Some(kind) = own.get(&callee).and_then(|kinds| kinds.get(position).copied().flatten()) {
                let _ = kind;
                return Some("own_macro");
            }
            call = self.report.macro_parameters.get(&callee).and_then(|list| list.get(position)).and_then(|(_, inner)| inner.clone());
        }
        let steps = |name: &str| own.get(name).is_some_and(|kinds| kinds.contains(&Some("step")));
        numbers::route_step(&self.code.tokens, at, floor, limit, &steps).then_some("own_macro")
    }

    /// The routine being read: its name, as its definition's declarator
    /// names it (`s32 Battle_GetEffectBaseSuccessRate(s32 effectId) {`), and
    /// its braces.
    fn routine_name(&mut self) -> Option<(&'a str, usize, usize)> {
        let open = self.stack.iter().position(|kind| *kind == Brace::Func).and_then(|at| self.open.get(at).copied())?;
        let close = self.routine_end();
        let tokens = &self.code.tokens;
        let mut index = open.checked_sub(1).filter(|&index| tokens[index].text == ")")?;
        let mut depth = 0;
        loop {
            match tokens[index].text {
                ")" => depth += 1,
                "(" => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
            index = index.checked_sub(1)?;
        }
        let name = index.checked_sub(1).map(|at| &tokens[at]).filter(|t| t.kind == Kind::Ident)?;
        Some((name.text, open, close))
    }

    /// A percentage a routine named for a chance returns: the whole value it
    /// returns, or gives a local it returns, in a routine whose name holds
    /// `chance`, `percent` or a success `rate` (`Battle_GetEffectBaseSuccessRate`'s
    /// `return 70;`, `fixedChance = 60; ... return -fixedChance;`). A rate
    /// alone may be a speed, and stays.
    fn chance_returned(&mut self, at: usize) -> Option<&'static str> {
        let (name, open, close) = self.routine_name()?;
        let words = numbers::words(name);
        let has = |word: &str| words.iter().any(|w| w == word);
        let chance = has("chance") || has("percent") || (has("success") && has("rate"));
        (chance && numbers::is_decimal(self.code.text(at)) && numbers::returned_whole(&self.code.tokens, at, open, close)).then_some("decimal")
    }

    /// A literal standing alone between `(` or `,` and `,` or `)`, its sign
    /// included (`toFIXED(-32)`): an argument or list element.
    fn is_argument(&self, at: usize) -> bool {
        let text = |offset: usize| at.checked_sub(offset).map_or("", |at| self.code.text(at));
        let before = if text(1) == "-" { text(2) } else { text(1) };
        let after = self.code.tokens.get(at + 1).map_or("", |t| t.text);
        matches!(before, "(" | ",") && matches!(after, ")" | ",")
    }

    /// The last token of the routine being read: its closing brace, or the
    /// file's end outside one.
    fn routine_end(&mut self) -> usize {
        let last = self.code.tokens.len() - 1;
        let Some(open) = self.stack.iter().position(|kind| *kind == Brace::Func).and_then(|at| self.open.get(at).copied()) else {
            return last;
        };
        if let Some(&(known, close)) = self.routine.as_ref() {
            if known == open {
                return close;
            }
        }
        let mut depth = 0;
        let close = (open..=last)
            .find(|&index| {
                match self.code.text(index) {
                    "{" => depth += 1,
                    "}" => depth -= 1,
                    _ => {}
                }
                depth == 0
            })
            .unwrap_or(last);
        self.routine = Some((open, close));
        close
    }

    fn directive(&mut self, start: usize, end: usize) {
        let tokens: Vec<Token<'a>> = self.code.tokens[start + 1..end].to_vec();
        let Some(word) = tokens.first().map(|t| t.text) else { return };
        let idents: Vec<&'a str> = tokens.iter().filter(|t| t.kind == Kind::Ident).map(|t| t.text).collect();
        let line = self.code.tokens[start].line;
        let condition = tokens.iter().map(|t| t.text).filter(|t| !t.starts_with("/")).collect::<Vec<_>>().join(" ");
        let tested = self.guard.take();
        if word == "ifndef" {
            self.guard = idents.get(1).map(|name| name.to_string());
        }
        match word {
            "include" => {
                // `#include "shared/staged_actor.c"`: what this file holds.
                if let Some(spec) = tokens.get(1).map(|t| t.text).filter(|text| text.starts_with('"')) {
                    self.report.includes.push((self.file.to_string(), spec.trim_matches('"').to_string()));
                }
                return;
            }
            "include_next" | "error" | "warning" | "pragma" | "line" => return,
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
                self.report.macro_words.extend(idents.iter().skip(1).map(|word| word.to_string()));
                let function_like = tokens.get(2).is_some_and(|t| t.text == "(" && t.glued);
                if function_like {
                    self.macro_parameters(name.text, start, end);
                }
                // `#define MENU_DRAW_STATUS_LABEL Text_DrawStringInWindowFar`.
                let body: Vec<&Token> = tokens[2..].iter().filter(|t| t.kind != Kind::Comment).collect();
                if let [routine] = body[..] {
                    if routine.kind == Kind::Ident && routine.text.bytes().any(|b| b.is_ascii_lowercase()) {
                        self.report.aliases.entry(name.text.to_string()).or_default().insert(routine.text.to_string());
                    }
                }
                self.directive_start = start;
                self.directive_end = end;
                for at in start + 3..end {
                    if self.code.tokens[at].kind == Kind::Number {
                        self.number(at, Some(function_like));
                    }
                }
                // An include guard (`#ifndef X` then `#define X`) is no declaration.
                let guard = body.is_empty() && (name.text.ends_with("_H") || name.text.ends_with("_H_") || tested.as_deref() == Some(name.text));
                if guard {
                    self.report.note("undocumented_guard", self.file, line, name.text);
                } else {
                    let documented = self.code.doc_before[start] || (start..=end).any(|at| self.code.doc_after[at]);
                    // `#define SCENE_VALE ((s32)&SceneId_Vale)`: one declaration with its extern.
                    let texts: Vec<&str> = body.iter().map(|t| t.text).collect();
                    let id = match texts[..] {
                        ["(", "(", "s32", ")", "&", target, ")"] => Some(target),
                        _ => None,
                    };
                    if let Some(target) = id {
                        self.report.id_externs.insert(target.to_string());
                    }
                    // A macro a routine defines for its own body is no
                    // declaration, and its comments stay `//` (D9).
                    let local = self.stack.contains(&Brace::Func);
                    let apart = if local {
                        Some("undocumented_local")
                    } else if name.text.starts_with("REG_") {
                        Some("undocumented_register")
                    } else if id.is_some() {
                        Some("undocumented_id")
                    } else if STANDARD_MACROS.contains(&name.text) {
                        Some("undocumented_standard")
                    } else {
                        self.heads.get(&line).map(|head| match head {
                            Head::Doc => "undocumented_run",
                            Head::Plain => "undocumented_plain",
                            Head::Note => "undocumented_note",
                        })
                    };
                    // One cast of a name (`#define WINDOW_WORK ((struct
                    // WindowWork *)gWindowWork[0])`), settled once every
                    // declaration is read.
                    if apart.is_none() && !function_like {
                        if let Some((target, casts)) = view_target(&texts) {
                            self.views.insert(name.text.to_string(), (target.to_string(), casts));
                        }
                    }
                    self.macro_starts.entry(name.text.to_string()).or_insert(start);
                    let entry = self.macros.entry(name.text.to_string()).or_insert((false, line, None));
                    entry.0 |= documented;
                    if entry.2.is_none() {
                        entry.2 = apart;
                    }
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
    /// is passed to: `WAIT(n)`'s `n` is `Event_Wait`'s first argument.
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
        let words = || (body..end).filter(|&at| self.code.tokens[at].kind != Kind::Comment).map(|at| self.code.text(at));
        if is_composition(&names, words()) {
            self.report.compositions.insert(name.to_string());
        }
        if is_grid_conversion(name, &names, words()) {
            self.report.grid_conversions.insert(name.to_string());
        }
        let body: Vec<&str> = words().collect();
        // `#define EFFECT_BLITTER(slot) ((DrawRectangle)gHeapSlots[slot])`: a
        // call through it is a call through the type it casts to.
        if let ["(", "(", ty, ")", ..] = body[..] {
            if ty.starts_with(|c: char| c.is_ascii_uppercase()) && !is_macro_name(ty) {
                self.report.type_aliases.entry(name.to_string()).or_default().push((self.file.to_string(), ty.to_string()));
            }
        }
        if self.file.ends_with(".c") {
            let kinds = own_macro_kinds(&names, &body);
            if kinds.iter().any(Option::is_some) {
                self.report.own_macros.entry(self.file.to_string()).or_default().insert(name.to_string(), kinds);
            }
        }
    }

    /// The parameter names of the function a declaration names, `None` where
    /// a parameter is unnamed: `void Map_CopyCells(s32 srcX, ...)`. A
    /// definition's names stand over a prototype's (`travelDivisor` in a
    /// header, `frames` where the routine is written); among prototypes, or
    /// among definitions, the one that names most.
    fn parameters(&mut self, name: &str, statement: &[usize], definition: bool) {
        let Some(open) = statement.windows(2).find(|pair| self.code.text(pair[0]) == name && self.code.text(pair[1]) == "(")
        else {
            return;
        };
        let open = statement.iter().position(|&at| at == open[1]).expect("its own token");
        let lists = self.parameter_tokens(statement, open);
        let mut names: Vec<Option<String>> = lists.iter().map(|tokens| self.parameter_name(tokens)).collect();
        if names.iter().all(Option::is_none) {
            // A definition of the old style names its parameters alone and
            // declares them after the list: `void Garoh_PopQuestionMark(x, y,
            // z, rotation, frames) s32 x, y, z; ...`. A prototype's unnamed
            // types (`void F(s32, u8);`) have no declarations after.
            let close = lists.iter().flatten().max().and_then(|&last| statement.iter().position(|&at| at == last)).map(|at| at + 1);
            let declared = close.and_then(|close| statement.get(close + 1)).is_some_and(|&at| {
                let token = &self.code.tokens[at];
                token.kind == Kind::Ident && !matches!(token.text, "__attribute__" | "asm" | "__asm__")
            });
            let alone = lists.iter().all(|tokens| matches!(tokens[..], [at] if self.code.tokens[at].kind == Kind::Ident && !is_keyword(self.code.text(at))));
            if !(declared && alone) {
                return;
            }
            names = lists.iter().map(|tokens| Some(self.code.text(tokens[0]).to_string())).collect();
        }
        let named = |list: &[Option<String>]| list.iter().filter(|n| n.is_some()).count();
        let defined = self.report.defined_parameters.contains(name);
        self.report.parameter_lists.entry(name.to_string()).or_default().push(names.clone());
        let entry = self.report.parameters.entry(name.to_string()).or_default();
        let named_definition = definition && named(&names) > 0;
        if (named_definition && !defined) || (named_definition == defined && named(&names) > named(entry)) {
            *entry = names;
        }
        if named_definition {
            self.report.defined_parameters.insert(name.to_string());
        }
    }

    /// The tokens of each parameter in the list whose `(` is `statement[open]`.
    fn parameter_tokens(&self, statement: &[usize], open: usize) -> Vec<Vec<usize>> {
        let (mut list, mut current, mut depth) = (Vec::new(), Vec::new(), 0);
        for &at in &statement[open + 1..] {
            match self.code.text(at) {
                "(" => depth += 1,
                ")" if depth == 0 => break,
                ")" => depth -= 1,
                "," if depth == 0 => {
                    list.push(std::mem::take(&mut current));
                    continue;
                }
                _ => {}
            }
            current.push(at);
        }
        list.push(current);
        list
    }

    /// The name of a parameter that is a pointer to functions: `f` in
    /// `void (*f)()`.
    fn pointer_name(&self, tokens: &[usize]) -> Option<String> {
        tokens.windows(4).find_map(|window| {
            let text = |at: usize| self.code.text(window[at]);
            (text(0) == "(" && text(1) == "*" && self.code.tokens[window[2]].kind == Kind::Ident && text(3) == ")").then(|| text(2).to_string())
        })
    }

    /// A function that calls a pointer it is passed, with its own parameters
    /// as arguments (`static __inline__ void Call1(void (*f)(), s32 a0) {
    /// f(a0); }`), forwards those parameters: kept for the numbers its
    /// callers pass (`Call1(Event_Wait, 15)`).
    fn forwards(&mut self, name: &str, statement: &[usize], open: usize, close: usize) {
        let Some(list) = statement.windows(2).find(|pair| self.code.text(pair[0]) == name && self.code.text(pair[1]) == "(") else {
            return;
        };
        let list = statement.iter().position(|&at| at == list[1]).expect("its own token");
        let parameters = self.parameter_tokens(statement, list);
        let names: Vec<Option<String>> = parameters.iter().map(|tokens| self.parameter_name(tokens).or_else(|| self.pointer_name(tokens))).collect();
        let mut forwards = vec![None; names.len()];
        for at in open + 1..close {
            let token = &self.code.tokens[at];
            let Some(function) = names.iter().position(|name| name.as_deref() == Some(token.text)) else { continue };
            if token.kind != Kind::Ident || self.code.tokens.get(at + 1).is_none_or(|t| t.text != "(") {
                continue;
            }
            let (arguments, _) = numbers::arguments(&self.code.tokens, at + 1);
            for (place, argument) in arguments.iter().enumerate() {
                if let Some(parameter) = argument.as_ref().and_then(|argument| names.iter().position(|name| name.as_ref() == Some(argument))) {
                    forwards[parameter] = Some((function, place));
                }
            }
        }
        if forwards.iter().any(Option::is_some) {
            self.report.forwards.insert(name.to_string(), forwards);
        }
    }

    /// The parameters a function's body between `open` and `close` turns
    /// into 16.16 itself, each passed alone to `toFIXED()` or `INTtoFIXED()`
    /// (`actor->velocityY = toFIXED(speed)`), as S3 has a call take plain
    /// pixels and speeds.
    fn conversions(&mut self, name: &str, open: usize, close: usize) {
        let Some(names) = self.report.parameters.get(name) else { return };
        let text = |at: usize| self.code.tokens.get(at).map_or("", |t| t.text);
        let places: HashSet<usize> = (open + 1..close)
            .filter(|&at| matches!(text(at), "toFIXED" | "INTtoFIXED") && text(at + 1) == "(" && text(at + 3) == ")")
            .filter_map(|at| names.iter().position(|parameter| parameter.as_deref() == Some(text(at + 2))))
            .collect();
        if !places.is_empty() {
            self.report.converted.entry(name.to_string()).or_default().extend(places);
        }
    }

    /// The name one parameter's tokens end in, when they name it: not `s32`,
    /// `struct Actor *` or a pointer to functions.
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
            // An old-style definition's body, after its parameters' declarations.
            None if text(prev) == ";" && self.old_style => Brace::Func,
            _ => Brace::Block,
        }
    }


    /// The hex number a `#define NAME 0x...` or an enumerator `NAME = 0x...`
    /// gives a name alone, with that name.
    fn hex_definition(&self, at: usize) -> Option<&'a str> {
        let text = |offset: isize| at.checked_add_signed(offset).and_then(|index| self.code.tokens.get(index));
        let (before, after) = (text(-1)?, text(1)?);
        if before.kind == Kind::Ident && text(-2)?.text == "define" && after.kind == Kind::PpEnd {
            return Some(before.text);
        }
        let name = text(-2)?;
        (before.text == "=" && name.kind == Kind::Ident && matches!(after.text, "," | "}") && !after.pp).then_some(name.text)
    }

    /// The variable whose brace initializer holds the token at `at`:
    /// `gContigo_SparkleTiles` of `const u32 gContigo_SparkleTiles[] = {
    /// 0x00007777, ... }`, through nested braces.
    fn initialized(&self, at: usize) -> Option<&'a str> {
        let tokens = &self.code.tokens;
        let mut depth = 0usize;
        for index in (0..at).rev() {
            match tokens[index].text {
                "}" => depth += 1,
                "{" if depth > 0 => depth -= 1,
                "{" => {
                    if index == 0 || tokens[index - 1].text != "=" {
                        continue;
                    }
                    let mut name = index.checked_sub(2)?;
                    while tokens[name].text == "]" {
                        name = (0..name).rev().find(|&open| tokens[open].text == "[")?.checked_sub(1)?;
                    }
                    return (tokens[name].kind == Kind::Ident).then_some(tokens[name].text);
                }
                ";" if depth == 0 => return None,
                _ => {}
            }
        }
        None
    }

    /// The names a file defines as bits: in a family of defines or
    /// enumerators sharing the name's prefix (`SPEAKER_` of `SPEAKER_FLAGS`),
    /// two or more of which are a single bit, each whose bits the family's
    /// single bits cover (the single bits themselves, and masks of them).
    fn bit_families(&self) -> BTreeSet<&'a str> {
        let mut families: BTreeMap<&str, Vec<(&str, u64)>> = BTreeMap::new();
        for at in 0..self.code.tokens.len() {
            let token = &self.code.tokens[at];
            let Some(value) = hex_value(token) else { continue };
            let Some(name) = self.hex_definition(at) else { continue };
            if let Some((prefix, _)) = name.rsplit_once('_') {
                families.entry(prefix).or_default().push((name, value));
            }
        }
        let mut bits = BTreeSet::new();
        // A packed field's values, within the mask a name of their prefix
        // gives it: `EXIT_STYLE_SILENT = 0x10` beside `EXIT_STYLE_MASK =
        // 0x70`, the field's value 0 too.
        let all: Vec<(&str, u64)> = families.values().flatten().copied().collect();
        for &(mask_name, mask) in &all {
            let Some(prefix) = mask_name.strip_suffix("_MASK") else { continue };
            bits.extend(
                all.iter()
                    .filter(|&&(name, value)| name != mask_name && name.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('_')) && value & !mask == 0)
                    .map(|&(name, _)| name),
            );
        }
        for members in families.values() {
            let singles: Vec<u64> = members.iter().map(|&(_, value)| value).filter(|value| value.is_power_of_two()).collect();
            let covered = singles.iter().fold(0, |all, value| all | value);
            if singles.len() >= 2 {
                bits.extend(members.iter().filter(|&&(_, value)| value != 0 && value & !covered == 0).map(|&(name, _)| name));
            }
        }
        bits
    }

    /// The call a number is a whole argument of: `GameFlag_Set` of
    /// `GameFlag_Set(0x9eb)`.
    fn argument_of(&self, at: usize) -> Option<&'a str> {
        let tokens = &self.code.tokens;
        if !matches!(tokens.get(at + 1).map(|t| t.text), Some(")" | ",")) || !matches!(tokens.get(at.checked_sub(1)?).map(|t| t.text), Some("(" | ",")) {
            return None;
        }
        let mut depth = 0;
        for index in (0..at).rev() {
            match tokens[index].text {
                ")" => depth += 1,
                "(" if depth == 0 => return tokens.get(index.checked_sub(1)?).filter(|t| t.kind == Kind::Ident).map(|t| t.text),
                "(" => depth -= 1,
                ";" | "{" | "}" => return None,
                _ => {}
            }
        }
        None
    }

    /// The call a number is a whole argument of, and which argument it is:
    /// `(SCENE_ITEM, 2)` of `SCENE_ITEM(CONTAINER_CHEST, ITEM_HERB, 0x9eb)`.
    fn argument_position(&self, at: usize) -> Option<(&'a str, usize)> {
        let call = self.argument_of(at)?;
        let tokens = &self.code.tokens;
        let (mut depth, mut position) = (0, 0);
        for index in (0..at).rev() {
            match tokens[index].text {
                ")" | "]" | "}" => depth += 1,
                "(" if depth == 0 => return Some((call, position)),
                "(" | "[" | "{" => depth -= 1,
                "," if depth == 0 => position += 1,
                _ => {}
            }
        }
        None
    }

    /// Whether a function's or function-like macro's parameter at `position`
    /// is named for a flag (`flag`, `doneFlag`), as far as the files read so
    /// far declare it: a header is read before the code that includes it.
    fn fills_flag_parameter(&self, call: &str, position: usize) -> bool {
        let is_flag = |name: &str| numbers::words(name).iter().any(|word| word == "flag");
        let report = &self.report;
        report.parameters.get(call).and_then(|names| names.get(position)).is_some_and(|name| name.as_deref().is_some_and(is_flag))
            || report.macro_parameters.get(call).and_then(|names| names.get(position)).is_some_and(|(name, _)| is_flag(name))
    }

    /// Counts the file's hex numbers, by what S3 lets stay hex: flag numbers
    /// (flags.h, a FLAG_ name's, a flag call's argument), addresses and values
    /// from 0x02000000 up, masks and bits (a number beside a bitwise operator
    /// or a shift, io_reg.h's register values, a name in a family of bits)
    /// and the sound driver's. The rest are listed, to be judged: a packed
    /// field stays, an offset, a count or a 16.16 value is written otherwise.
    fn hex(&mut self) {
        const BITWISE: &[&str] = &["&", "|", "^", "~", "<<", ">>", "&=", "|=", "^=", "<<=", ">>="];
        // A name beside `&`, `|`, `^` or `~` may be a mask or a bit wherever
        // it is defined, one beside a shift is an amount of bits, and one
        // beside arithmetic or an ordering is a quantity.
        const MASKING: &[&str] = &["&", "|", "^", "~", "&=", "|=", "^="];
        const RECKONING: &[&str] = &["+", "-", "*", "/", "%", "<", "<=", ">", ">=", "+=", "-=", "*=", "/=", "%="];
        for at in 0..self.code.tokens.len() {
            let token = &self.code.tokens[at];
            let beside = |list: &[&str]| {
                let near = |index: Option<usize>| index.and_then(|index| self.code.tokens.get(index)).is_some_and(|t| list.contains(&t.text));
                near(at.checked_sub(1)) || near(Some(at + 1))
            };
            if token.kind == Kind::Ident && beside(MASKING) {
                self.report.bitwise_names.insert(token.text.to_string());
            }
            if token.kind == Kind::Ident && beside(RECKONING) {
                self.report.arithmetic_names.insert(token.text.to_string());
            }
        }
        let bits = self.bit_families();
        // A hardware header's register values and addresses: the console's,
        // and the flash chip's commands and unlock addresses.
        let registers = self.file.ends_with("/io_reg.h") || self.file.ends_with("/gamepak/flash.h");
        for at in 0..self.code.tokens.len() {
            let token = self.code.tokens[at].clone();
            let Some(value) = hex_value(&token) else { continue };
            let beside = |offset: usize, back: bool| {
                let index = if back { at.checked_sub(offset) } else { at.checked_add(offset) };
                index.and_then(|index| self.code.tokens.get(index)).map_or("", |t| t.text)
            };
            let shift = |a: &str, b: &str| (a == "<" && b == "<") || (a == ">" && b == ">");
            let mask = BITWISE.contains(&beside(1, true))
                || BITWISE.contains(&beside(1, false))
                || shift(beside(2, true), beside(1, true))
                || shift(beside(1, false), beside(2, false));
            let name = self.hex_definition(at);
            // An all-ones marker with a name, as S3 writes one
            // (`PASSWORD_CRC_SEED 0xffff`): a byte's, halfword's or word's.
            let all_ones = name.is_some() && matches!(value, 0xff | 0xffff | 0xffff_ffff);
            // A flag number: flags.h's, a FLAG_ name's, or the whole argument
            // of a call or macro whose name says it takes a flag.
            let flag = self.file.ends_with("/flags.h")
                || name.is_some_and(|name| name.starts_with("FLAG_"))
                || self.argument_of(at).is_some_and(|call| call.to_ascii_lowercase().contains("flag"))
                || self.argument_position(at).is_some_and(|(call, position)| self.fills_flag_parameter(call, position));
            // A picture's pixel words, a hex digit or byte a pixel
            // (`gContigo_SparkleTiles`): packed fields S3 keeps hex.
            let pixels = self.initialized(at).is_some_and(|table| numbers::words(table).last().is_some_and(|word| matches!(word.as_str(), "tiles" | "pixels")));
            let metric = if flag {
                "hex_flag"
            } else if self.file.contains("/sound/") {
                "hex_driver"
            } else if value >= 0x0200_0000 {
                "hex_address"
            } else if mask || registers || all_ones || name.is_some_and(|name| bits.contains(name)) {
                "hex_mask"
            } else if pixels {
                "hex_pixels"
            } else {
                "hex_other"
            };
            self.report.add("hex", self.file, 1);
            // A function-like macro's own body defines its name: S3's hex.
            let metric = if metric == "hex_other" && self.in_function_macro(at) { "hex_definition" } else { metric };
            // Judged by someone who read it, by its row.
            let routine = self.owners.get(at).copied().flatten();
            // A named one's uses are read first (`finish`).
            let judged = if metric == "hex_other" && name.is_none() { self.report.judged(self.file, routine, token.text) } else { None };
            if let Some(honest) = judged {
                if honest {
                    self.report.note("hex_judged", self.file, token.line, token.text);
                } else {
                    self.report.note("hex_other", self.file, token.line, token.text);
                    self.report.note("hex_unsettled", self.file, token.line, token.text);
                }
            } else if let Some(name) = name.filter(|_| metric == "hex_other") {
                // Its name's uses, in any file, may show it a mask or a bit.
                self.report.hex_definitions.push((self.file.to_string(), token.line, token.text.to_string(), name.to_string(), routine.map(str::to_string)));
            } else if metric == "hex_other" {
                self.report.note(metric, self.file, token.line, token.text);
            } else {
                self.report.add(metric, self.file, 1);
            }
        }
    }

    /// Whether the token at `at` is in a function-like macro's definition
    /// (`#define PIXELS_4BPP_HALFWORD(color) ((color) * 0x1111)`).
    fn in_function_macro(&self, at: usize) -> bool {
        let tokens = &self.code.tokens;
        if !tokens[at].pp {
            return false;
        }
        let Some(start) = (0..at).rev().find(|&index| tokens[index].kind == Kind::PpStart) else { return false };
        let words: Vec<&Token> = tokens[start + 1..at].iter().filter(|t| t.kind != Kind::Comment).collect();
        matches!(words[..], [define, name, open, ..] if define.text == "define" && name.kind == Kind::Ident && open.text == "(" && open.glued)
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
                if self.statement.len() == 1 && token.kind == Kind::Ident && is_macro_name(token.text) {
                    self.definer = self.definer_at(at);
                }
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
                    // `DrawRectangle draw[2];`: a variable of a named type.
                    let mut after = at + 1;
                    while self.code.tokens.get(after).is_some_and(|t| t.text == "*") {
                        after += 1;
                    }
                    let next = self.code.tokens.get(after);
                    let named_type = name.starts_with(|c: char| c.is_ascii_uppercase()) && !is_macro_name(name);
                    if named_type
                        && next.is_some_and(|t| t.kind == Kind::Ident && !is_keyword(t.text))
                        && self.code.tokens.get(after + 1).is_some_and(|t| matches!(t.text, ";" | "[" | "," | "=" | ")"))
                    {
                        let variable = next.map_or("", |t| t.text).to_string();
                        self.report.typed_variables.entry(variable).or_default().insert((name.to_string(), self.file.to_string()));
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
            // A definer's invocation ends at its closing parenthesis, and
            // defines the routine it names.
            if let Some((close, name, documented)) = self.definer.take() {
                if close == at {
                    let first = self.statement.first().copied().unwrap_or(at);
                    let line = self.code.tokens[first].line;
                    let (_, run) = self.run_head(first, at, documented, None);
                    let apart = (run == Some(Head::Doc)).then_some("undocumented_run");
                    self.report.declare_apart(true, &name, self.file, line, documented, true, apart);
                    self.named(&name, NameKind::Function, line, None);
                    self.statement.clear();
                } else {
                    self.definer = Some((close, name, documented));
                }
            }
            prev2 = prev;
            prev = Some(at);
            at += 1;
        }
        if !self.statement.is_empty() {
            self.declaration();
        }
        let file = self.file;
        for (name, (documented, line, apart)) in std::mem::take(&mut self.macros) {
            // What a view may view: a macro with its `///`, or one set apart
            // by a rule that is not a `//` to mark.
            let said = documented || apart.is_some_and(|apart| !matches!(apart, "undocumented_plain" | "undocumented_note" | "undocumented_local"));
            *self.report.macro_docs.entry(name.clone()).or_default() |= said;
            if documented {
                continue;
            }
            let view = self.views.remove(&name);
            if apart.is_none() {
                if let Some(spec) = self.parameter_of(&name) {
                    self.report.parameter_candidates.push((file.to_string(), line, name, spec, view));
                    continue;
                }
            }
            match (apart, view) {
                (None, Some((target, casts))) => self.report.views.push((file.to_string(), line, name, target, casts)),
                (apart, _) => self.report.note(apart.unwrap_or("undocumented_macro"), file, line, &name),
            }
        }
    }

    /// The file a macro is a parameter of, as its `#include` writes it: the
    /// macro's first definition is followed, past comments and the other
    /// `#define`s it is set with, by that `#include`, and this file uses the
    /// name nowhere else or `#undef`s it right after (`#define
    /// EFFECT_SET_MODE Actor_SelectAnimation` over `#include
    /// "../field/field_effect_spawn.c"`). Whether the included file uses it
    /// is settled once every file is read.
    fn parameter_of(&self, name: &str) -> Option<String> {
        let tokens = &self.code.tokens;
        let start = *self.macro_starts.get(name)?;
        let directive_end = |from: usize| (from..tokens.len()).find(|&at| tokens[at].kind == Kind::PpEnd).unwrap_or(tokens.len());
        let words = |from: usize| -> Vec<&str> { tokens[from + 1..directive_end(from)].iter().filter(|t| t.kind != Kind::Comment).map(|t| t.text).collect() };
        let next = |from: usize| (directive_end(from) + 1..tokens.len()).find(|&at| tokens[at].kind != Kind::Comment);
        let mut at = next(start)?;
        let spec = loop {
            if tokens[at].kind != Kind::PpStart {
                return None;
            }
            match words(at)[..] {
                ["define", ..] => at = next(at)?,
                ["include", spec] if spec.starts_with('"') => break spec.trim_matches('"').to_string(),
                _ => return None,
            }
        };
        let mut undone = false;
        let mut after = next(at);
        while let Some(undef) = after.filter(|&at| tokens[at].kind == Kind::PpStart && words(at).first() == Some(&"undef")) {
            undone |= words(undef).get(1) == Some(&name);
            after = next(undef);
        }
        // Its uses outside its `#define`s and `#undef`s.
        let mut inside = false;
        let mut directive = "";
        let mut elsewhere = false;
        for (index, token) in tokens.iter().enumerate() {
            match token.kind {
                Kind::PpStart => {
                    inside = true;
                    directive = tokens.get(index + 1).map_or("", |t| t.text);
                }
                Kind::PpEnd => inside = false,
                Kind::Ident if token.text == name && !(inside && matches!(directive, "define" | "undef")) => elsewhere = true,
                _ => {}
            }
        }
        (undone || !elsewhere).then_some(spec)
    }

    /// The tokens between a brace pair, preprocessor lines aside, a
    /// function's parameters and locals bound, for the duplicate search
    /// (`duplicates`).
    /// A table's type is what its declaration says before its name, storage
    /// and qualifiers aside (`struct SceneEntrance`, `u16`); a function's,
    /// how many parameters it takes. The comments above it go with it.
    fn duplicate(&mut self, statement: &[usize], function: bool, open: usize, name: &str) {
        let close = self.braces[&open].close;
        let tokens: Vec<&str> = self.code.tokens[open + 1..close].iter().filter(|token| !token.pp).map(|token| token.text).collect();
        let comment = statement.first().map_or(String::new(), |&first| self.code.comment_text[first].clone());
        let kind;
        let tokens = match function {
            true => {
                let open = statement.windows(2).position(|pair| self.code.text(pair[0]) == name && self.code.text(pair[1]) == "(");
                let listed = open.map(|open| self.parameter_tokens(statement, open + 1)).unwrap_or_default();
                let parameters: Vec<String> = listed.iter().filter_map(|tokens| self.parameter_name(tokens)).collect();
                // How many it takes; `(void)` takes none.
                let void = listed.len() == 1 && listed[0].iter().all(|&at| self.code.text(at) == "void");
                kind = format!("({})", if void { 0 } else { listed.len() });
                // Every name the function gives a parameter or local, for
                // whether a rename would clash (`naming`).
                let scope = (self.file.to_string(), statement[0]);
                let names = duplicates::bound(&parameters, &tokens).into_keys().map(str::to_string);
                self.report.naming.scopes.entry(scope).or_default().extend(names);
                duplicates::bind(&parameters, &tokens)
            }
            false => {
                // A section or attribute macro (`LATE_DATA`) before the type is no part of it.
                let words: Vec<&str> = statement
                    .iter()
                    .map(|&at| self.code.text(at))
                    .take_while(|text| *text != name)
                    .filter(|text| !matches!(*text, "static" | "extern" | "const" | "volatile" | "UNUSED"))
                    .collect();
                let last = words.len().saturating_sub(1);
                let words: Vec<&str> = words.iter().enumerate().filter(|(at, text)| *at == last || !is_macro_name(text)).map(|(_, text)| *text).collect();
                kind = words.join(" ");
                tokens.iter().map(|text| text.to_string()).collect()
            }
        };
        let (file, name) = (self.file.to_string(), name.to_string());
        self.report.copies.push(duplicates::Copy { function, file, name, tokens, kind, comment });
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
            self.function_type(&statement);
            self.types(&opened, second_last, line, documented);
            return;
        }
        let old_style = std::mem::take(&mut self.old_style);
        if let Some(&(open, _)) = opened.iter().find(|(_, kind)| *kind == Brace::Func) {
            if let Some(Declarator::Function(name)) = self.declarator(&statement) {
                let name = name.to_string();
                self.parameters(&name, &statement, true);
                let close = self.braces[&open].close;
                let body: Vec<&str> = self.code.tokens[open + 1..close].iter().filter(|token| !token.pp).map(|token| token.text).collect();
                let call = one_call(&body).filter(|callee| !self.declares(&statement, callee)).map(str::to_string);
                let (_, run) = self.run_head(first, last, documented, call.clone());
                let apart = if body.is_empty() {
                    Some("undocumented_empty")
                } else if run == Some(Head::Doc) {
                    Some("undocumented_run")
                } else {
                    None
                };
                self.report.declare_apart(true, &name, file, line, documented, true, apart);
                if let Some(call) = call {
                    self.report.functions.get_mut(&name).expect("declared").forward.get_or_insert(call);
                }
                self.forwards(&name, &statement, open, close);
                self.conversions(&name, open, close);
                self.named(&name, NameKind::Function, line, None);
                self.duplicate(&statement, true, open, &name);
                if in_map_folder(file) {
                    let unused = statement.iter().any(|&at| self.code.text(at) == "UNUSED");
                    self.report.scene_routines.push((file.to_string(), name.clone(), unused));
                }
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
            // `struct BilibinRippleTimer { ... };`, for a global declared
            // right under it.
            if lead != "enum" && after_body.is_some() {
                let tag = self.code.tokens.get(first + 1).filter(|t| t.kind == Kind::Ident).map(|t| t.text.to_string());
                self.last_type = tag.map(|tag| (tag, documented, self.code.tokens[last].line));
            }
            return;
        }
        let defined = lead != "extern";
        match self.declarator(&statement) {
            Some(Declarator::Function(name)) => {
                let name = name.to_string();
                self.parameters(&name, &statement, false);
                let (_, run) = self.run_head(first, last, documented, None);
                let apart = (run == Some(Head::Doc)).then_some("undocumented_run");
                self.report.declare_apart(true, &name, file, line, documented, false, apart);
                self.named(&name, NameKind::Prototype, line, None);
                // `void f(x, y) s32 x;`: an old-style definition, whose
                // parameters' declarations follow up to its body.
                let close = statement.iter().position(|&at| self.code.text(at) == name).and_then(|open| {
                    let mut depth = 0;
                    statement[open + 1..]
                        .iter()
                        .position(|&at| {
                            match self.code.text(at) {
                                "(" => depth += 1,
                                ")" => depth -= 1,
                                _ => {}
                            }
                            depth == 0
                        })
                        .map(|close| open + 1 + close)
                });
                self.old_style = close.is_some_and(|close| close + 2 < statement.len() && is_type_word(self.code.text(statement[close + 1])));
            }
            Some(Declarator::Global(_)) if old_style => self.old_style = true,
            Some(Declarator::Global(name)) => {
                let name = name.to_string();
                let apart = self.global_apart(&statement, &name, documented);
                self.report.declare_apart(false, &name, file, line, documented, defined, apart);
                self.named(&name, if defined { NameKind::Global } else { NameKind::Extern }, line, None);
                // A mechanism's table counts where it is defined; a game thing
                // (`extern char SceneId_Vale;`) where it is declared.
                if defined || provenance::kind(Definition::new(&name, file, Origin::Global), &self.report.macro_values) != Some("global") {
                    let own = self.code.doc_text[first].clone() + &self.code.notes(last, last);
                    self.thing(&name, Origin::Global, line, own, String::new(), (None, None));
                }
                if let Some(&(open, _)) = opened.iter().find(|(_, kind)| *kind == Brace::Init) {
                    self.duplicate(&statement, false, open, &name);
                }
            }
            None => {}
        }
    }

    /// Whether a function's declaration (its tokens before its body) names
    /// `name`, as a parameter it calls (`Call1(void (*f)(), s32 a0) { f(a0); }`).
    fn declares(&self, statement: &[usize], name: &str) -> bool {
        statement.iter().take_while(|&&at| self.code.text(at) != "{").any(|&at| self.code.text(at) == name)
    }

    /// A file-scope invocation of a definer macro at `at`, which takes no
    /// semicolon (`DEFINE_LIFT_ROCK(MagmaRock_LiftRock, ...)` and the next
    /// declaration on a line of its own): its closing parenthesis, the
    /// routine its first argument names, and whether a `///` above it
    /// documents that routine.
    fn definer_at(&self, at: usize) -> Option<(usize, String, bool)> {
        let tokens = &self.code.tokens;
        if tokens.get(at + 1).is_none_or(|t| t.text != "(" || t.pp) {
            return None;
        }
        let mut depth = 0;
        let close = (at + 1..tokens.len()).filter(|&i| !tokens[i].pp).find(|&i| {
            match tokens[i].text {
                "(" => depth += 1,
                ")" => depth -= 1,
                _ => {}
            }
            depth == 0
        })?;
        let next = (close + 1..tokens.len()).find(|&i| !tokens[i].pp || tokens[i].kind == Kind::PpStart).map(|i| &tokens[i]);
        let ends = next.is_none_or(|next| next.line > tokens[close].line && !matches!(next.text, ";" | "{" | "," | ")" | "=" | "("));
        // A definer may take the routine's type first (`UNIT_DEFINE_ADD(s32,
        // Unit_AddHp, hp, hpMax)`).
        let first = if tokens.get(at + 2).is_some_and(|t| is_type_word(t.text)) && tokens.get(at + 3).is_some_and(|t| t.text == ",") { at + 4 } else { at + 2 };
        let name = tokens.get(first).filter(|t| t.kind == Kind::Ident && t.text.bytes().any(|b| b.is_ascii_lowercase()));
        let named = name.is_some() && tokens.get(first + 1).is_some_and(|t| matches!(t.text, "," | ")"));
        (ends && named).then(|| (close, name.map_or(String::new(), |t| t.text.to_string()), self.code.doc_before[at]))
    }

    /// The row that sets a global apart when it has no `///` (D9's rules,
    /// as both games' samples found them): one of a run of declarations,
    /// no blank line between, under one `///` (`undocumented_run`); a
    /// scene's own table (its entrances, exits, step triggers, actor
    /// placements, regions, by its type or name) or an actor script of `SC_` commands, said by
    /// its type and name (`undocumented_scene`); a string whose initializer
    /// is its text (`undocumented_string`); one under a `//` comment, to be
    /// marked `///` (`undocumented_plain`).
    fn global_apart(&mut self, statement: &[usize], name: &str, documented: bool) -> Option<&'static str> {
        let (first, last) = (statement[0], statement[statement.len() - 1]);
        let (own, run) = self.run_head(first, last, documented, None);
        let first_line = self.code.tokens[first].line;
        let equals = statement.iter().position(|&at| self.code.text(at) == "=");
        let value = equals.and_then(|at| statement.get(at + 1)).map(|&at| &self.code.tokens[at]);
        let script = equals.is_some_and(|at| {
            self.code.tokens[statement[at]..last].iter().find(|t| t.kind == Kind::Ident).is_some_and(|t| t.text.starts_with("SC_"))
        });
        let typed = statement.windows(2).any(|pair| {
            self.code.text(pair[0]) == "struct"
                && matches!(self.code.text(pair[1]), "SceneEntrance" | "Trigger" | "ActorPlacement" | "SceneRegion" | "SceneExitRegion")
        });
        let scene = (self.file.contains("/src/scene/") || self.file.contains("/src/debug/"))
            && (typed || ["Entrances", "Exits", "Triggers", "ActorPlacements", "Regions"].iter().any(|kind| name.contains(kind)));
        // Declared right under its type, whose `///` says what it holds
        // (`struct BilibinRippleTimer gBilibin_RippleTimer;` under the
        // struct's), nothing but blank lines between.
        let under_type = self.last_type.as_ref().is_some_and(|(tag, said, end)| {
            *said
                && statement.windows(2).any(|pair| matches!(self.code.text(pair[0]), "struct" | "union") && self.code.text(pair[1]) == tag)
                && (*end..first_line.saturating_sub(1)).all(|line| self.lines.get(line).is_some_and(|text| text.trim().is_empty()))
        });
        self.last_type = None;
        if documented {
            None
        } else if run == Some(Head::Doc) {
            Some("undocumented_run")
        } else if scene || script {
            Some("undocumented_scene")
        } else if value.is_some_and(|t| t.kind == Kind::Str) {
            Some("undocumented_string")
        } else if under_type && own.is_none() {
            Some("undocumented_typed")
        } else {
            match own.or(run) {
                Some(Head::Plain) => Some("undocumented_plain"),
                Some(Head::Note) => Some("undocumented_note"),
                _ => None,
            }
        }
    }

    /// The comment a declaration from token `first` to `last` has right
    /// above it (`///`, or a `//` to mark or a note D9 keeps), and the one
    /// over the run it continues: the declaration ending on the line before,
    /// or, where both are one call to the same routine (`call`), the one
    /// before a blank line (`Test_ReceiveFizz` after `Test_ReceiveFlint`).
    /// A `///<` note beside it is its own.
    fn run_head(&mut self, first: usize, last: usize, documented: bool, call: Option<String>) -> (Option<Head>, Option<Head>) {
        let (first_line, last_line) = (self.code.tokens[first].line, self.code.tokens[last].line);
        let own = if self.code.doc_before[first] {
            Some(Head::Doc)
        } else if documented {
            None
        } else {
            let mut above = first_line.saturating_sub(1);
            while above > 0 && self.lines.get(above - 1).is_some_and(|line| line.contains("clang-format")) {
                above -= 1;
            }
            let mut head = None;
            while above > 0 && self.lines.get(above - 1).is_some_and(|line| is_comment_line(line)) {
                above -= 1;
                let kept = is_kept_note(self.lines[above]);
                head = Some(if kept || head == Some(Head::Note) { Head::Note } else { Head::Plain });
            }
            head
        };
        let blank = |from: usize, to: usize, lines: &[&str]| (from..to.saturating_sub(1)).all(|line| lines.get(line).is_some_and(|text| text.trim().is_empty()));
        let run = self
            .last_global
            .as_ref()
            .filter(|(end, _, before)| {
                end + 1 == first_line || (call.is_some() && *before == call && *end < first_line && blank(*end, first_line, &self.lines))
            })
            .and_then(|(_, head, _)| *head);
        // A note D9 keeps heads no run, nor a declaration a `///<` beside
        // it documents.
        let head = match own.or(run) {
            Some(Head::Note) => None,
            _ if documented && own.is_none() => None,
            head => head,
        };
        self.last_global = Some((last_line, head, call));
        (own, run)
    }

    /// A typedef of a pointer to functions with named parameters
    /// (`typedef s32 (*BattleEffectDrawRectangle)(void *destination, ...,
    /// s32 x, s32 y, s32 width, s32 height);`), or another name for a type
    /// (`typedef BattleEffectDrawRectangle DrawRectangle;`).
    fn function_type(&mut self, statement: &[usize]) {
        let text = |at: usize| self.code.text(statement[at]);
        // `typedef DrawRectangle BlitterPair[2];` is a pair of the same.
        let array = statement.len() > 4 && text(3) == "[" && text(statement.len() - 2) == "]";
        if (statement.len() == 4 || array) && (1..3).all(|at| self.code.tokens[statement[at]].kind == Kind::Ident) && text(statement.len() - 1) == ";" {
            let alias = (self.file.to_string(), text(1).to_string());
            self.report.type_aliases.entry(text(2).to_string()).or_default().push(alias);
            return;
        }
        // The type's own declarator, before any struct body it holds.
        let body = (0..statement.len()).find(|&at| text(at) == "{").unwrap_or(statement.len());
        let Some(at) = (0..body.saturating_sub(4)).find(|&at| {
            text(at) == "(" && text(at + 1) == "*" && self.code.tokens[statement[at + 2]].kind == Kind::Ident && text(at + 3) == ")" && text(at + 4) == "("
        }) else {
            return;
        };
        let names: Vec<Option<String>> = self.parameter_tokens(statement, at + 4).iter().map(|tokens| self.parameter_name(tokens)).collect();
        // One that names no parameter is kept too: it hides another of its name.
        self.report.function_types.entry(text(at + 2).to_string()).or_default().push((self.file.to_string(), names));
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
            // An anonymous enum of constants is documented by its first
            // constant's comment, or by a `///<` note on every constant.
            if kind == Brace::Enum && tag.is_none() && typedef.is_empty() {
                let close = self.braces[&open].close;
                let mut members = vec![open + 1];
                members.extend((open + 1..close).filter(|&at| self.code.text(at) == "," && at + 1 < close).map(|at| at + 1));
                let noted = |member: usize| {
                    let end = (member..close).find(|&at| self.code.text(at) == ",").unwrap_or(close);
                    (member..=end.min(close - 1)).any(|at| self.code.doc_after[at]) || self.code.doc_after[end]
                };
                if self.code.doc_before.get(open + 1) == Some(&true) || (close > open + 1 && members.iter().all(|&member| noted(member))) {
                    continue;
                }
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
                        // A routine a shared file names by its includer's
                        // macro, right after its type (`struct Actor
                        // *EFFECT_CREATE(s32 type, ...)`, `void EFFECT_SPAWN(`).
                        Some(name)
                            if name.kind == Kind::Ident
                                && index >= 2
                                && (tokens[index - 2].text == "*" || is_type_word(tokens[index - 2].text)) =>
                        {
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
    report.paths.insert(path.to_string());
    let idents = code.tokens.iter().filter(|t| t.kind == Kind::Ident).map(|t| t.text.to_string()).collect();
    let said = comments.iter().flat_map(|c| c.text.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')).filter(|w| !w.is_empty()).map(str::to_string).collect();
    report.file_words.insert(path.to_string(), (idents, said));
    let lines = text.lines().count();
    report.lines += lines;
    // S3 splits only an object holding more than one family: never a header, a
    // map overlay (laid out by kind, so one file), a part (one family
    // already) or a data object (one file).
    let object = path.ends_with(".c") && !in_map_folder(path) && !comments.iter().any(|c| c.text.starts_with("// Part of "));
    let long = object && lines > LONG_FILE;
    static TAG: OnceLock<Regex> = OnceLock::new();
    let tag = TAG.get_or_init(|| Regex::new(r"(FAKEMATCH|LEFTOVER)\(([a-z-]+)\)").unwrap());
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
            let (word, kind) = (&capture[1], &capture[2]);
            if word == "FAKEMATCH" && DEVICES.contains(&kind) {
                report.note(&format!("fakematch_{kind}"), path, comment.line, kind);
            } else if word == "FAKEMATCH" {
                // A tag is a fake match whatever its word: one S2 does not
                // name (`FAKEMATCH(type)`) is counted, to be given its kind.
                report.note("fakematch_other", path, comment.line, kind);
            } else if word == "LEFTOVER" && LEFTOVERS.contains(&kind) {
                report.note(&format!("leftover_{kind}"), path, comment.line, kind);
            } else if word == "LEFTOVER" {
                // As with FAKEMATCH: a leftover of a kind S2 does not name is
                // counted, to be given its kind.
                report.note("leftover_other", path, comment.line, kind);
            }
        }
        if comment.text.starts_with("// DEAD:") {
            report.note("leftover_dead", path, comment.line, "dead");
        }
        if let Some(Ok(kind)) = crate::adopt::views::view_kind(comment.text) {
            report.note("views", path, comment.line, kind);
        }
    }
    // A question with the `//` lines that go on with it, for its whole text.
    for (at, comment) in comments.iter().enumerate() {
        if !comment.text.contains("XXX:") {
            continue;
        }
        let mut text = comment.text.to_string();
        let mut last = comment.line;
        for next in &comments[at + 1..] {
            if next.line != last + 1 || !next.text.starts_with("//") || next.text.starts_with("///") || next.text.contains("XXX") {
                break;
            }
            text.push('\n');
            text.push_str(next.text);
            last = next.line;
        }
        questions(report, path, comment.line, &text);
    }
    let owners = clex::owners(&code.tokens);
    let mut walk = Walk {
        report,
        file: path,
        code,
        owners,
        stack: Vec::new(),
        conditions: Vec::new(),
        square: 0,
        statement: Vec::new(),
        braces: HashMap::new(),
        open: Vec::new(),
        macros: BTreeMap::new(),
        lines: text.lines().collect(),
        heads: define_heads(text),
        guard: None,
        last_global: None,
        last_type: None,
        old_style: false,
        views: HashMap::new(),
        macro_starts: HashMap::new(),
        definer: None,
        enum_docs: HashMap::new(),
        defined: BTreeSet::new(),
        enum_next: HashMap::new(),
        enum_values: HashMap::new(),
        enum_members: HashMap::new(),
        directive_start: 0,
        directive_end: 0,
        routine: None,
        functions: 0,
    };
    walk.walk();
    walk.hex();
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
    game_with_text(files, None, &[])
}

/// Count a game, and with its English text check its names' provenance,
/// `names` being the game's rows of `evidence/<game>-names.tsv` (H8).
#[cfg(test)]
pub fn game_with_text<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>, text: Option<&GameText>, names: &[evidence::Name]) -> Report {
    game_judged(files, text, names, &[])
}

/// `game_with_text`, with the game's rows of `evidence/<game>-numbers.tsv`:
/// the numbers in code someone judged by reading them.
#[cfg(test)]
pub fn game_judged<'a>(
    files: impl IntoIterator<Item = (&'a str, &'a str)>,
    text: Option<&GameText>,
    names: &[evidence::Name],
    judged: &[evidence::Judged],
) -> Report {
    game_all(files, text, names, judged, &[])
}

/// `game_judged`, with the game's rows of `evidence/<game>-duplicates.tsv`:
/// the duplicate groups someone judged not to be one text.
pub fn game_all<'a>(
    files: impl IntoIterator<Item = (&'a str, &'a str)>,
    text: Option<&GameText>,
    names: &[evidence::Name],
    judged: &[evidence::Judged],
    duplicates: &[evidence::JudgedGroup],
) -> Report {
    let mut report = Report::default();
    report.duplicate_rows = duplicates.iter().map(|row| (row.members.clone(), (row.kind.clone(), row.why.clone()))).collect();
    report.numbered_by_row = names.iter().filter(|row| row.kind == "its number").map(|row| row.name.clone()).collect();
    report.judged = judged.iter().map(|row| ((row.file.clone(), row.routine.clone(), row.number.clone()), row.kind == "honest")).collect();
    if let Some(text) = text {
        report.text_unknown = text.symbols_saying("unknown").map(str::to_string).collect();
        report.message_symbols = text.symbols().map(str::to_string).collect();
    }
    let mut units = Vec::new();
    for (path, text) in files {
        if path.ends_with("/README.md") {
            questions(&mut report, path, 1, text);
            units.push(path);
            continue;
        }
        // Assembly: the names it refers to, for which routines are called.
        if path.ends_with(".s") {
            report.macro_words.extend(naming::code_identifiers(text).into_iter().map(str::to_string));
            continue;
        }
        // A picture: its stem, for the names taken from it.
        if let Some(picture) = path.strip_suffix(".png") {
            report.naming.assets.insert(picture.rsplit('/').next().unwrap_or(picture).to_ascii_lowercase());
            continue;
        }
        file(&mut report, path, text);
        if is_map_overlay(path) {
            units.push(path);
            report.at_origin.extend(placed_at_origin(text).into_iter().map(|name| (path.to_string(), name)));
        }
        if naming::is_pret_flash(path) {
            report.naming.pret.extend(naming::identifiers(text).map(str::to_string));
        }
        if naming::is_m4a(path) {
            report.naming.pret.extend(naming::code_identifiers(text).into_iter().map(str::to_string));
        }
    }
    for path in units {
        unit(&mut report, path);
    }
    report.finish();
    let text = text.filter(|text| !text.is_empty());
    if let Some(text) = text {
        report.check_provenance(text, names);
    }
    let mut findings = naming::check(&report.naming, text);
    findings.sort_by(|a, b| a.metric.cmp(b.metric).then(a.file.cmp(&b.file)).then(a.line.cmp(&b.line)).then(a.what.cmp(&b.what)));
    for finding in findings {
        report.note(finding.metric, &finding.file, finding.line, &finding.what);
    }
    report
}

impl Report {
    fn check_provenance(&mut self, text: &GameText, names: &[evidence::Name]) {
        // A map id is declared once for the game, wherever it is repeated.
        let mut maps = BTreeSet::new();
        self.things.retain(|thing| thing.kind != "scene" || maps.insert(thing.name.clone()));
        // Each name's evidence rows, by the name and the file declaring it;
        // a map id's by its name alone, being the game's wherever declared.
        let mut rows: HashMap<(&str, &str), Vec<String>> = HashMap::new();
        let mut by_name: HashMap<&str, Vec<String>> = HashMap::new();
        for row in names {
            rows.entry((row.name.as_str(), row.file.as_str())).or_default().push(row.line());
            by_name.entry(row.name.as_str()).or_default().push(row.line());
        }
        for thing in &mut self.things {
            let own = rows.get(&(thing.name.as_str(), thing.file.as_str()));
            let own = own.or_else(|| by_name.get(thing.name.as_str()).filter(|_| thing.kind == "scene"));
            thing.evidence = own.cloned().unwrap_or_default();
        }
        let lenders = self.lenders();
        let analysis = provenance::analyze_with(&self.things, &self.places, text, &self.macro_values, &lenders);
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

    /// The things whose seen rows may lend each thing its words, by place:
    /// for a routine nothing runs (`never_run`), its namesakes in other files
    /// (by its whole name, or its name after its scene's prefix) and its
    /// copies of the same body; for an actor placed at its map's
    /// origin (`at_origin`), its siblings of the same name with another
    /// letter (`ACTOR_FUGUE_ICE_CONE_B` for `_A`).
    fn lenders(&self) -> HashMap<usize, Vec<usize>> {
        let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
        let mut by_place: HashMap<(&str, &str), usize> = HashMap::new();
        // A routine's name after its scene's or module's prefix
        // (`SlideFromEdge` of `MarsLighthouseRoomF_SlideFromEdge`).
        let unprefixed = |name: &str| name.split_once('_').filter(|(prefix, _)| prefix.starts_with(|c: char| c.is_ascii_uppercase()) && prefix.bytes().any(|b| b.is_ascii_lowercase())).map(|(_, rest)| rest.to_string());
        let mut by_rest: HashMap<String, Vec<usize>> = HashMap::new();
        for (at, thing) in self.things.iter().enumerate() {
            by_name.entry(thing.name.as_str()).or_default().push(at);
            by_place.insert((thing.file.as_str(), thing.name.as_str()), at);
            if let Some(rest) = unprefixed(&thing.name) {
                by_rest.entry(rest).or_default().push(at);
            }
        }
        let mut lenders: HashMap<usize, Vec<usize>> = HashMap::new();
        for (at, thing) in self.things.iter().enumerate() {
            let key = (thing.file.clone(), thing.name.clone());
            let mut found = Vec::new();
            if self.never_run.contains(&key) {
                found.extend(by_name.get(thing.name.as_str()).into_iter().flatten().filter(|&&other| self.things[other].file != thing.file));
                let namesakes = unprefixed(&thing.name).and_then(|rest| by_rest.get(&rest));
                found.extend(namesakes.into_iter().flatten().filter(|&&other| self.things[other].file != thing.file));
                for (file, name) in self.same_body.get(&key).into_iter().flatten() {
                    found.extend(by_place.get(&(file.as_str(), name.as_str())));
                }
            }
            if self.at_origin.contains(&key) {
                let letter = thing.name.rsplit_once('_').filter(|(_, letter)| letter.len() == 1 && letter.bytes().all(|b| b.is_ascii_uppercase()));
                if let Some((stem, own)) = letter {
                    for other in ('A'..='Z').filter(|other| other.to_string() != own) {
                        found.extend(by_place.get(&(thing.file.as_str(), format!("{stem}_{other}").as_str())));
                    }
                }
            }
            if !found.is_empty() {
                lenders.insert(at, found);
            }
        }
        lenders
    }
}

/// The files a game keeps its area-name rules in: The Broken Seal's
/// `gArea_NameRules` in `src/field/scene_rules.s`, The Lost Age's
/// `gScene_AreaNameRules` in `src/field/area_name_rules.s`.
const AREA_RULES: [&str; 2] = ["src/field/scene_rules.s", "src/field/area_name_rules.s"];

/// How many area names the area-name rules read: their highest name index
/// plus one, from the `scene_name`, `area_name` and `group_name` rows.
pub fn area_count(rules: &str) -> Option<usize> {
    rules
        .lines()
        .map(str::trim)
        .filter(|line| ["scene_name ", "area_name ", "group_name "].iter().any(|row| line.starts_with(row)))
        .filter_map(|line| line.split(',').nth(2)?.split('@').next()?.trim().parse::<usize>().ok())
        .max()
        .map(|max| max + 1)
}

/// The game whose text a game's twin words come from: The Lost Age is
/// written as The Broken Seal expanded and takes its words (H6), not the
/// other way round.
const TWIN: [(&str, &str); 1] = [("tla", "tbs")];

/// A game's English text, if git tracks one, with the words its glossary
/// (`games/<game>/glossary.md`, which the docs site prints) defines and its
/// twin's words.
fn game_text(root: &Path, game: &str) -> Result<Option<GameText>, String> {
    let glossary = std::fs::read(root.join(format!("games/{game}/glossary.md"))).ok();
    let text = own_text(root, game)?.map(|text| match &glossary {
        Some(glossary) => text.with_glossary(&String::from_utf8_lossy(glossary)),
        None => text,
    });
    let Some(&(_, twin)) = TWIN.iter().find(|(of, _)| *of == game) else { return Ok(text) };
    Ok(match (text, own_text(root, twin)?) {
        (Some(text), Some(twin)) => Some(text.with_twin(&twin)),
        (text, _) => text,
    })
}

/// A game's own English text, if git tracks one.
fn own_text(root: &Path, game: &str) -> Result<Option<GameText>, String> {
    let path = format!("games/{game}/text/en.po");
    let listed = std::process::Command::new("git").arg("-C").arg(root).args(["ls-files", "--"]).arg(&path).output();
    let Some(po) = listed.ok().filter(|listed| !listed.stdout.is_empty()).and_then(|_| std::fs::read(root.join(&path)).ok()) else {
        return Ok(None);
    };
    let rules = AREA_RULES.iter().find_map(|rules| std::fs::read(root.join(format!("games/{game}/{rules}"))).ok());
    let areas = rules.and_then(|rules| area_count(&String::from_utf8_lossy(&rules)));
    let mut text = GameText::parse(&String::from_utf8_lossy(&po), areas).map_err(|error| format!("{path}: {error}"))?;
    // The other editions' text, for the messages only they show.
    for other in tracked(root, &[format!("games/{game}/text")])?.iter().filter(|other| other.ends_with(".po") && **other != path) {
        let po = std::fs::read(root.join(other)).map_err(|error| format!("cannot read {other}: {error}"))?;
        text = text.with_edition_text(&String::from_utf8_lossy(&po));
    }
    Ok(Some(text))
}

/// A scene's overlay, `games/<game>/src/scene/<scene>.c` (⚓️'s overlays are
/// still folders there, so they are not counted here).
fn is_map_overlay(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    parts.len() == 5 && parts[3] == "scene" && path.ends_with(".c")
}

/// Under a game's scene overlays, `src/scene`.
fn in_map_folder(path: &str) -> bool {
    path.contains("/src/scene/")
}

/// A folder guide, `games/<game>/src/<folder>/README.md`.
fn is_folder_guide(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    parts.len() == 5 && parts[2] == "src" && parts[4] == "README.md"
}

/// Keeps the `XXX:` questions of a comment or a guide that starts at `line`.
/// `XXX (no evidence):` marks a question nothing in reach settles, so it is
/// not kept and leaves its unit done (H4).
fn questions(report: &mut Report, path: &str, line: usize, text: &str) {
    for (offset, _) in text.match_indices("XXX:") {
        let at = line + text[..offset].matches('\n').count();
        // Its whole text, the lines a comment or paragraph goes on with up to
        // the sentence's end, so the next reader sees what would settle it.
        let mut question = String::new();
        for (index, piece) in text[offset..].lines().enumerate() {
            let piece = if index == 0 { piece } else { piece.trim_start().trim_start_matches(['/', '*']) };
            let piece = piece.trim().trim_end_matches("*/").trim_end();
            if piece.is_empty() || (index > 0 && piece.contains("XXX")) {
                break;
            }
            if !question.is_empty() {
                question.push(' ');
            }
            question.push_str(piece);
            if question.ends_with(['.', '?', '!']) {
                break;
            }
        }
        report.questions.entry(path.to_string()).or_default().push((at, question));
    }
}

/// Counts a unit's open questions (H4). A map's unit is its overlay; a
/// folder's is its guide and the C files and headers under its folder, but
/// for the map overlays, which are units of their own.
fn unit(report: &mut Report, path: &str) {
    let folder = path.strip_suffix("README.md").filter(|folder| folder.ends_with('/'));
    let mut found = Vec::new();
    for (file, questions) in &report.questions {
        let mine = file == path || folder.is_some_and(|folder| file.starts_with(folder) && !is_map_overlay(file));
        if mine {
            found.extend(questions.iter().map(|(line, question)| (file.clone(), *line, question.clone())));
        }
    }
    // Listed with the unit each belongs to, so a unit that reopens is named.
    for (file, line, question) in &found {
        report.note("unit_open", file, *line, &format!("{path}\t{question}"));
    }
    report.units.insert(path.to_string(), found.len() as u64);
}

/// The files git tracks under each of `folders`, from the checkout's root.
fn tracked(root: &Path, folders: &[String]) -> Result<Vec<String>, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--"])
        .args(folders)
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err(format!("git ls-files failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).split('\0').filter(|path| !path.is_empty()).map(str::to_string).collect())
}

/// The tracked text of a game that may write a file's symbol
/// (`layout::SYMBOL_TEXT`), for which numbered files nothing uses.
fn symbol_texts(root: &Path, paths: &[String]) -> Result<Vec<(String, String)>, String> {
    let mut files = Vec::new();
    for path in paths {
        let extension = path.rsplit_once('.').map_or("", |(_, extension)| extension);
        if layout::SYMBOL_TEXT.contains(&extension) {
            let bytes = std::fs::read(root.join(path)).map_err(|error| format!("cannot read {path}: {error}"))?;
            files.push((path.clone(), String::from_utf8_lossy(&bytes).into_owned()));
        }
    }
    Ok(files)
}

/// The tracked C, headers and folder guides of a game, never build output,
/// its assembly, and its pictures by path alone (their text empty).
fn sources(root: &Path, game: &str) -> Result<Vec<(String, String)>, String> {
    let mut files = Vec::new();
    for path in tracked(root, &[format!("games/{game}/src"), format!("games/{game}/include")])? {
        if path.ends_with(".png") {
            files.push((path, String::new()));
        } else if path.ends_with(".c") || path.ends_with(".h") || path.ends_with(".s") || is_folder_guide(&path) {
            let bytes = std::fs::read(root.join(&path)).map_err(|error| format!("cannot read {path}: {error}"))?;
            files.push((path, String::from_utf8_lossy(&bytes).into_owned()));
        }
    }
    Ok(files)
}

/// The rows of the summary, as `(metric, label)`; a metric's TSV total is its value. A row
/// with no metric is a heading.
const ROWS: &[(&str, &str)] = &[
    ("magic_code", "numbers in code to name"),
    ("magic_argument", "  of those, lone arguments"),
    ("magic_unsettled", "  of those, unsettled, by row"),
    ("magic_code_all", "numbers in code, all"),
    ("honest_composed", "  honest: composed (S3)"),
    ("honest_decimal", "  honest: tenths, percents, digits"),
    ("honest_random", "  honest: random ranges"),
    ("honest_position", "  honest: positions, sizes"),
    ("honest_duration", "  honest: durations, cues"),
    ("honest_pointer", "  honest: via pointer calls"),
    ("honest_damping", "  honest: dampings"),
    ("honest_animation", "  honest: a sheet's animations"),
    ("honest_level", "  honest: fade, shake levels"),
    ("honest_mask", "  honest: masks of whole fields"),
    ("honest_sign", "  honest: the sign bit"),
    ("honest_packing", "  honest: packed bytes, bit fields"),
    ("honest_index", "  honest: indices of an array filled in order"),
    ("honest_own_macro", "  honest: steps, indices of a file's own macros"),
    ("honest_debug", "  honest: amounts in the debug rooms"),
    ("honest_address", "  honest: hardware addresses"),
    ("honest_definition", "  honest: a macro's own definition"),
    ("honest_scaling", "  honest: shifts scaling a 16.16 product"),
    ("honest_judged", "  honest: judged by row"),
    ("data_argument", "numbers passed as data"),
    ("data_argument_frames", "  frames"),
    ("data_argument_coordinate", "  coordinates"),
    ("data_argument_size", "  sizes"),
    ("data_argument_count", "  counts"),
    ("data_argument_speed", "  plain speeds"),
    ("data_argument_composed", "  composed quantities"),
    ("magic_table", "numbers in table data"),
    ("placeholder_names", "placeholder names"),
    ("placeholder", "  their uses"),
    ("placeholder_unused", "  apart: placeholders nothing uses"),
    ("numbered_names", "numbered names"),
    ("numbered", "  their uses"),
    ("numbered_honest", "  apart: its number, by row"),
    ("fakematch", "FAKEMATCH devices"),
    ("fakematch_signature", "  signature"),
    ("fakematch_volatile", "  volatile"),
    ("fakematch_barrier", "  barrier"),
    ("fakematch_wrapper", "  wrapper"),
    ("fakematch_temporary", "  temporary"),
    ("fakematch_dead", "  dead"),
    ("fakematch_other", "  of a kind S2 does not name"),
    ("leftover", "LEFTOVERs (Camelot's)"),
    ("leftover_frame", "  frame"),
    ("leftover_template", "  template"),
    ("leftover_other", "  of a kind S2 does not name"),
    ("leftover_dead", "  dead (// DEAD: marks)"),
    ("views", "unsettled views (VIEW markers)"),
    ("goto", "goto"),
    ("hex", "hex numbers"),
    ("hex_flag", "  flag numbers"),
    ("hex_address", "  addresses, high values"),
    ("hex_mask", "  masks, bits, registers, all-ones"),
    ("hex_pixels", "  a picture's pixel words"),
    ("hex_driver", "  the sound driver's"),
    ("hex_definition", "  a function-like macro's own definition"),
    ("hex_encoding", "  instruction encodings, CRC polynomials"),
    ("hex_judged", "  honest: judged by row"),
    ("hex_other", "  the rest, to judge"),
    ("hex_unsettled", "    of those, unsettled, by row"),
    ("if_language", "#if on LANGUAGE"),
    ("if_patch", "#if on a patch level"),
    ("if_edition_name", "#if on an edition name"),
    ("if_in_function", "  of those inside functions"),
    ("undocumented_entry", "far-call entries without ///"),
    ("long_file", "objects over 3,000 lines"),
    ("source_files", "source files (C, headers, asm)"),
    ("folders", "folders"),
    ("loose_headers", "  headers loose in include/"),
    ("part_folders", "  part folders beside an object"),
    ("numbered_files", "files, folders named by number"),
    ("numbered_files_honest", "  apart: its number, by row"),
    ("numbered_files_unused", "  unused, kept by number"),
    ("units", "units (places and folder guides)"),
    ("units_done", "  done under H4"),
    ("unit_open", "  XXX: questions in them"),
    ("duplicate_function", "duplicate function groups"),
    ("duplicate_function_copies", "  copies past the first"),
    ("duplicate_table", "duplicate table groups"),
    ("duplicate_table_copies", "  copies past the first"),
    ("duplicate_said", "one body under two names, said so"),
    ("duplicate_judged", "duplicates judged by row"),
    ("provenance_names", "game-thing names"),
    ("provenance_verified", "  verified by row or ★ line"),
    ("provenance_table", "  verified by table"),
    ("provenance_rooms", "  verified by rooms"),
    ("provenance_table_mismatch", "  table name differs"),
    ("provenance_unstarred", "  evidence line without its ★"),
    ("provenance_unchecked", "  evidence by row, unchecked"),
    ("provenance_unseen", "  its code: nothing on screen"),
    ("provenance_failed", "  evidence failing check"),
    ("provenance_prose", "  evidence in prose only"),
    ("provenance_none", "  no evidence"),
    ("unused_words", "words the game never uses"),
    ("unused_word_name", "  names using them"),
    ("seen_words", "words for what the player sees, by a render"),
    ("lent_words", "  of those, from a seen copy or sibling"),
    ("unseen_words", "words of things nothing shows"),
    ("twin_words", "words from The Broken Seal's text"),
    ("glossary_words", "words the glossary defines"),
    ("door_words", "door words unresolved"),
    ("door_word_link", "  links using them"),
    ("", "names to guess (one word per idea):"),
    ("synonym", "  a second word for one idea"),
    ("synonym_referent", "  one type, two variable names"),
    ("abbreviation", "  shortened, spelled elsewhere"),
    ("family_shape", "  off their family's pattern"),
    ("prefix_twin", "  one idea under two prefixes"),
    ("prefix_twin_apart", "  apart: a place's or a group's plural"),
    ("", "to review, not to fill (D9):"),
    ("undocumented_function", "  functions without ///"),
    ("undocumented_struct", "  structs without ///"),
    ("undocumented_enum", "  enums without ///"),
    ("undocumented_macro", "  macros without ///"),
    ("undocumented_global", "  globals without ///"),
    ("undocumented_run", "  apart: in a run under ///"),
    ("undocumented_empty", "  apart: empty functions"),
    ("undocumented_forward", "  apart: one call to a documented routine"),
    ("undocumented_library", "  apart: library and BIOS routines"),
    ("undocumented_standard", "  apart: standard C macros"),
    ("undocumented_view", "  apart: a typed view of a documented name"),
    ("undocumented_typed", "  apart: right under its type's ///"),
    ("undocumented_plain", "  apart: under //, mark ///"),
    ("undocumented_note", "  apart: under a // note D9 keeps"),
    ("undocumented_local", "  apart: macros inside a function"),
    ("undocumented_far", "  apart: XFar, by X's ///"),
    ("undocumented_id", "  apart: ids, by their extern"),
    ("undocumented_register", "  apart: hardware registers"),
    ("undocumented_scene", "  apart: scene tables, scripts"),
    ("undocumented_string", "  apart: strings, their text"),
    ("undocumented_guard", "  apart: include guards"),
    ("undocumented_shared", "  apart: a shared file's parameters"),
    ("undocumented_family", "  apart: a family without a head, past its first"),
];

/// The metrics a sum adds: `fakematch` its devices, `leftover` its leftovers
/// and the `// DEAD:` marks; the two sums stay apart (S2). Any other metric
/// adds none.
fn kinds(metric: &str) -> Vec<String> {
    match metric {
        "fakematch" => DEVICES.iter().chain(&["other"]).map(|kind| format!("fakematch_{kind}")).collect(),
        "leftover" => LEFTOVERS.iter().chain(&["other", "dead"]).map(|kind| format!("leftover_{kind}")).collect(),
        _ => Vec::new(),
    }
}

/// Metrics whose top files are listed.
const LISTED: &[&str] = &[
    "magic_code", "magic_argument", "magic_unsettled", "magic_code_all", "magic_table", "placeholder", "numbered", "fakematch", "leftover", "views", "goto", "hex_other", "hex_unsettled", "if_language", "if_patch",
    "if_in_function", "undocumented_entry", "undocumented_function", "undocumented_struct", "undocumented_enum", "undocumented_macro",
    "undocumented_global", "undocumented_plain", "provenance_none", "provenance_prose", "provenance_table_mismatch", "unused_word_name",
    "door_word_link", "unit_open", "synonym", "synonym_referent", "abbreviation", "family_shape", "prefix_twin",
];

impl Report {
    pub fn value(&self, metric: &str) -> u64 {
        match metric {
            "placeholder_names" => self.distinct("placeholder") as u64,
            "placeholder_unused" => self.distinct("placeholder_unused") as u64,
            "numbered_names" => self.distinct("numbered") as u64,
            "numbered_honest" => self.distinct("numbered_honest") as u64,
            "fakematch" | "leftover" => kinds(metric).iter().map(|kind| self.total(kind)).sum(),
            "long_file" => self.long_files.len() as u64,
            "units" => self.units.len() as u64,
            "units_done" => self.units.values().filter(|open| **open == 0).count() as u64,
            "duplicate_function" => self.function_groups.len() as u64,
            "duplicate_function_copies" => Report::copies(&self.function_groups) as u64,
            "duplicate_table" => self.table_groups.len() as u64,
            "duplicate_table_copies" => Report::copies(&self.table_groups) as u64,
            "duplicate_said" => self.said_groups.len() as u64,
            "duplicate_judged" => self.judged_groups.len() as u64,
            "unused_words" => self.provenance.as_ref().map_or(0, |analysis| analysis.words.len() as u64),
            "seen_words" => self.provenance.as_ref().map_or(0, |analysis| (analysis.seen_words.len() + analysis.lent_words.len()) as u64),
            "lent_words" => self.provenance.as_ref().map_or(0, |analysis| analysis.lent_words.len() as u64),
            "unseen_words" => self.provenance.as_ref().map_or(0, |analysis| analysis.unseen_words.len() as u64),
            "twin_words" => self.provenance.as_ref().map_or(0, |analysis| analysis.twin_words.len() as u64),
            "glossary_words" => self.provenance.as_ref().map_or(0, |analysis| analysis.glossary_words.len() as u64),
            "door_words" => self.provenance.as_ref().map_or(0, |analysis| analysis.door_words.len() as u64),
            _ => self.total(metric),
        }
    }

    /// A sum's kinds (the FAKEMATCH devices, or the leftovers) added per file.
    fn kind_files(&self, metric: &str) -> BTreeMap<String, u64> {
        let mut files = BTreeMap::new();
        for kind in kinds(metric) {
            for (file, count) in self.tallies.get(&kind).into_iter().flatten() {
                *files.entry(file.clone()).or_default() += count;
            }
        }
        files
    }

    fn top_of(&self, metric: &str, count: usize) -> Vec<(String, u64)> {
        if !kinds(metric).is_empty() {
            let mut files: Vec<(String, u64)> = self.kind_files(metric).into_iter().collect();
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
        for (metric, groups) in [
            ("duplicate_function", &report.function_groups),
            ("duplicate_table", &report.table_groups),
            ("duplicate_said", &report.said_groups),
        ] {
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

/// The short table `progress --humanity` prints, and with `top` each metric's
/// leading files.
pub fn summary(games: &[(&str, &Report)], top: usize) -> String {
    let mut text = String::from("Humanization");
    for (game, _) in games {
        let _ = write!(text, "{:>10}", game);
    }
    text.push('\n');
    let _ = write!(text, "  {:<30}", "files (lines)");
    for (_, report) in games {
        let _ = write!(text, "{:>10}", commas(report.files as i64));
    }
    text.push('\n');
    let _ = write!(text, "  {:<30}", "");
    for (_, report) in games {
        let _ = write!(text, "{:>10}", format!("({})", commas(report.lines as i64)));
    }
    text.push('\n');
    for (metric, label) in ROWS {
        if metric.is_empty() {
            let _ = writeln!(text, "  {label}");
            continue;
        }
        let _ = write!(text, "  {label:<30}");
        for (_, report) in games {
            let _ = write!(text, "{:>10}", commas(report.value(metric) as i64));
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
                    let _ = writeln!(text, "  {:>6}  {file}", commas(count as i64));
                }
            }
            if !report.long_files.is_empty() {
                let _ = writeln!(text, "{game} long_file:");
                for (file, lines) in report.long_files.iter().take(top) {
                    let _ = writeln!(text, "  {:>6}  {file}", commas(*lines as i64));
                }
            }
            for (metric, groups) in [
                ("duplicate_function", &report.function_groups),
                ("duplicate_table", &report.table_groups),
                ("duplicate_said", &report.said_groups),
            ] {
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

/// The lists `--list` takes beside the table's metrics: the name provenance
/// worklist, the words the game never uses, those set apart as what the
/// player sees, the twin's text's or the glossary's, and those no door
/// resolves.
const WORD_LISTS: [&str; 8] = ["provenance", "unused_word", "seen_word", "lent_word", "unseen_word", "twin_word", "glossary_word", "door_word"];

/// Whether `--list` knows a metric: one the table counts, or a word list.
fn listable(metric: &str) -> bool {
    WORD_LISTS.contains(&metric) || ROWS.iter().any(|(row, _)| !row.is_empty() && *row == metric)
}

/// Every counted thing of one metric (`fakematch` takes each device kind,
/// `leftover` each leftover kind and the `// DEAD:` marks),
/// one line each: game, `file:line`, what.
pub fn list(games: &[(&str, &Report)], metric: &str) -> String {
    let mut text = String::new();
    for (game, report) in games {
        if WORD_LISTS.contains(&metric) {
            let Some(analysis) = &report.provenance else { continue };
            if metric != "provenance" {
                // Each flagged or unresolved word, how many names use it and
                // where, a flagged word's use marked `(seen)` where its name
                // takes it from what someone saw.
                let words = match metric {
                    "unused_word" => &analysis.words,
                    "seen_word" => &analysis.seen_words,
                    "lent_word" => &analysis.lent_words,
                    "unseen_word" => &analysis.unseen_words,
                    "twin_word" => &analysis.twin_words,
                    "glossary_word" => &analysis.glossary_words,
                    _ => &analysis.door_words,
                };
                for (word, rows) in words {
                    let uses: Vec<String> = rows
                        .iter()
                        .map(|&at| {
                            let row = &analysis.rows[at];
                            let thing = &row.thing;
                            let seen = match metric {
                                "unused_word" if row.seen_uses.contains(word) => " (seen)",
                                "unused_word" if row.lent_uses.contains(word) => " (from a seen copy or sibling)",
                                "unused_word" if row.unseen_uses.contains(word) => " (nothing on screen)",
                                _ => "",
                            };
                            format!("{} {}:{}{seen}", thing.name, thing.file, thing.line)
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
        // A duplicate group: its tokens and every copy.
        let groups = match metric {
            "duplicate_function" | "duplicate_function_copies" => &report.function_groups[..],
            "duplicate_table" | "duplicate_table_copies" => &report.table_groups[..],
            "duplicate_said" => &report.said_groups[..],
            _ => &[],
        };
        for group in groups {
            let _ = writeln!(text, "{game}\t{}\t{}", group.tokens, group.members.join(" "));
        }
        if metric == "duplicate_judged" {
            for (group, kind, why) in &report.judged_groups {
                let _ = writeln!(text, "{game}\t{}\t{}\t{kind}: {why}", group.tokens, group.members.join(" "));
            }
        }
        for item in &report.items {
            if item.metric == metric || kinds(metric).contains(&item.metric) {
                // A file or folder is listed by its path alone.
                if item.line == 0 {
                    let _ = writeln!(text, "{game}\t{}\t{}", item.file, item.what);
                } else {
                    let _ = writeln!(text, "{game}\t{}:{}\t{}", item.file, item.line, item.what);
                }
            }
        }
    }
    text
}

/// The rows of a game's `evidence/<game>-duplicates.tsv` that judge no group
/// its duplicate count finds now, for `check tags`: each row's members and
/// what is wrong.
pub fn stale_duplicates(root: &Path, game: &str) -> Result<Vec<(String, String)>, String> {
    let duplicates = evidence::duplicates(game)?;
    if duplicates.is_empty() {
        return Ok(Vec::new());
    }
    let files = sources(root, game)?;
    let report = game_all(files.iter().map(|(p, t)| (p.as_str(), t.as_str())), None, &[], &[], &duplicates);
    Ok(report.stale_duplicates())
}

/// `progress --humanity [--top N] [--list METRIC]`: count both games, write
/// the TSV and return the table, or with `list` that metric's items.
pub fn run(root: &Path, top: usize, metric: Option<&str>) -> Result<String, String> {
    if let Some(metric) = metric.filter(|metric| !listable(metric)) {
        return Err(format!(
            "--list takes a metric of the table, as humanity.tsv names it, or provenance, unused_word, seen_word, lent_word, unseen_word, twin_word, glossary_word or door_word, not {metric}"
        ));
    }
    let reports = std::thread::scope(|scope| {
        ["tbs", "tla"]
            .map(|game| {
                scope.spawn(move || {
                    let text = game_text(root, game)?;
                    let names = evidence::names(game)?;
                    let judged = evidence::judged(game)?;
                    let duplicates = evidence::duplicates(game)?;
                    let files = sources(root, game)?;
                    let mut report = game_all(files.iter().map(|(p, t)| (p.as_str(), t.as_str())), text.as_ref(), &names, &judged, &duplicates);
                    let rows: Vec<String> = names.iter().filter(|row| row.kind == "its number").map(|row| row.name.clone()).collect();
                    let paths = tracked(root, &[format!("games/{game}")])?;
                    let texts = symbol_texts(root, &paths)?;
                    let symbols = layout::numbered_symbols(&texts);
                    let mut songs = Vec::new();
                    for path in paths.iter().filter(|path| path.contains("/sound/songs/") && path.ends_with(".mid")) {
                        songs.push((path.clone(), std::fs::read(root.join(path)).map_err(|error| format!("cannot read {path}: {error}"))?));
                    }
                    layout::count(&mut report, game, &paths, &rows, &symbols, &layout::uses(&texts, &songs));
                    Ok::<Report, String>(report)
                })
            })
            .map(|thread| thread.join().expect("humanity thread"))
    });
    let [sun, anchor] = reports;
    let (sun, anchor) = (sun?, anchor?);
    let games = [("☀️ tbs", &sun), ("⚓️ tla", &anchor)];
    let path = root.join(REPORT);
    std::fs::create_dir_all(path.parent().expect("report directory")).map_err(|error| format!("{REPORT}: {error}"))?;
    std::fs::write(&path, tsv(&[("tbs", &sun), ("tla", &anchor)]))
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
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
    fn a_unit_is_open_while_its_files_hold_an_xxx() {
        let report = game([
            ("games/tbs/src/shop/README.md", "# Shops\n\n## Evidence\n\nNamed from the counters.\n"),
            ("games/tbs/src/shop/shop/run.c", "// XXX: why three; a probe of the\n// counter would settle it.\nvoid Shop_Run(void) {}\n"),
            ("games/tbs/src/shop/shop/inn.c", "void Inn_Run(void) {\n  // XXX (no evidence): why; only the source says.\n}\n"),
            ("games/tbs/src/scene/README.md", "# Maps\n"),
            ("games/tbs/src/scene/shared/door.h", "/// A door.\nstruct Door { int x; };\n"),
            ("games/tbs/src/scene/vale.c", "/// @file\n/// Vale.\n\nvoid Vale_Run(void) {\n  /* XXX: what this is; its render. */\n}\n"),
            ("games/tbs/src/scene/vault.c", "/// @file\n/// Vault.\n// XXX (no evidence): what the ivy is for; nothing says.\n"),
        ]);
        let open: Vec<(&str, u64)> = report.units.iter().map(|(file, open)| (file.as_str(), *open)).collect();
        assert_eq!(
            open,
            [
                ("games/tbs/src/scene/README.md", 0),
                ("games/tbs/src/scene/vale.c", 1),
                ("games/tbs/src/scene/vault.c", 0),
                ("games/tbs/src/shop/README.md", 1),
            ]
        );
        assert_eq!(report.value("units"), 4);
        assert_eq!(report.value("units_done"), 2);
        let listed: Vec<(&str, usize)> =
            report.items.iter().filter(|item| item.metric == "unit_open").map(|item| (item.file.as_str(), item.line)).collect();
        assert_eq!(listed, [("games/tbs/src/scene/vale.c", 5), ("games/tbs/src/shop/shop/run.c", 1)]);
        assert_eq!(
            list(&[("tbs", &report)], "unit_open"),
            concat!(
                "tbs\tgames/tbs/src/scene/vale.c:5\tgames/tbs/src/scene/vale.c\tXXX: what this is; its render.\n",
                "tbs\tgames/tbs/src/shop/shop/run.c:1\tgames/tbs/src/shop/README.md\tXXX: why three; a probe of the counter would settle it.\n",
            ),
            "each question with its unit"
        );
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
        // 0x10, 100, 1000u and SCALE's 12; 8 * 8 is a size. 100 is assigned
        // to an x and 12 multiplies one: positions.
        assert_eq!(report.total("magic_code_all"), 4);
        assert_eq!((report.total("magic_code"), report.total("honest_position")), (2, 2));
        assert_eq!(report.total("magic_argument"), 1, "1000u");
        // 0x30, 40 and the local table's 99.
        assert_eq!(report.total("magic_table"), 3);
    }

    #[test]
    fn placeholder_and_numbered_names() {
        for name in ["unk_14", "unk14", "unkC", "gUnk_0203", "gUnknown_0203", "sUnk_10", "sub_08012345", "field_1C", "Foo_Unk"]
        {
            assert!(is_placeholder(name), "{name}");
        }
        assert!(is_placeholder("SE_138") && is_placeholder("ITEM_034") && is_placeholder("DJINNI_07") && is_placeholder("Battle_08012345") && is_placeholder("UnkThing"));
        for name in ["Unkempt", "Unknowable", "Trunk", "FunkyUnkle", "SE_JUMP", "sunkRows"] {
            assert!(!is_placeholder(name), "{name}");
        }
        for name in ["ColosseumFinal1", "Event_LeaveByExit10", "ACTOR_8", "gEffectScript0", "VenusLighthouse2", "arg0"] {
            assert!(is_numbered(name), "{name}");
        }
        for name in [
            "u8", "vu16", "s32", "BG2", "Bg2", "Dma3", "REG_DMA3", "Random16", "Dma_Fill32", "MulFX", "Q_8_8",
            "FLAG_SCENE_TEMP_01", "TBS_PATCH_1", "DISPCNT_MODE_1", "BLDCNT_TGT1_BG2", "x1", "Trig_Atan", "attr2",
            "Vector3", "unk_14", "MapGridCell", "SE_138", "FLAG_SCENE_TEMP_0E", "__divsi3", "rgb555", "OBJ_VRAM0",
        ] {
            assert!(!is_numbered(name), "{name}");
        }
        let report = one("void ColosseumFinal1(void) { unk_14 = ColosseumFinal1 + gUnk_0203; } // unk_99 Exit10\n");
        assert_eq!((report.distinct("placeholder"), report.total("placeholder")), (2, 2));
        assert_eq!((report.distinct("numbered"), report.total("numbered")), (1, 2));
    }

    #[test]
    fn placeholders_nothing_uses() {
        let report = game([
            ("games/tla/include/sounds.h", "enum SoundId {\n  SE_JUMP,\n  SE_120,\n  SE_121,\n  SE_122,\n  SE_123,\n};\n#define SE_LATE SE_123\n"),
            ("games/tla/src/a.c", "void Jump(void) { Sound_Play(SE_121); }\n"),
            ("games/tla/src/b.s", "\t.2byte SE_122\n"),
        ]);
        let names = |kind: &str| report.names.get(kind).map(|names| names.iter().cloned().collect::<Vec<_>>()).unwrap_or_default();
        assert_eq!(names("placeholder_unused"), ["SE_120"], "written only in its own row");
        assert_eq!(names("placeholder"), ["SE_121", "SE_122", "SE_123"], "played, in a script, in a macro");
        assert_eq!((report.value("placeholder_names"), report.value("placeholder_unused")), (3, 1));
        assert_eq!(report.total("placeholder"), 5, "SE_121 and SE_123 twice, SE_122 once: a script is no C");
        let listing = list(&[("tla", &report)], "placeholder_unused");
        assert_eq!(listing, "tla\tgames/tla/include/sounds.h:3\tSE_120\n");
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
            "    // FAKEMATCH(volatile): read afresh; as a plain local, 2 of 9 instructions differ in tla-en.\n",
            "    // FAKEMATCH(one-pass): no longer a kind, counted to be given one.\n",
            "    /* VIEW(s0b84): routine Helper as int Helper(int); the definition has void Helper(void). */\n",
            "#if LANGUAGE != LANGUAGE_JAPANESE && TBS_PATCH_2\n",
            "    goto done;\n",
            "#endif\n",
            "done:;\n",
            "}\n",
        ));
        assert_eq!(
            (report.total("fakematch_wrapper"), report.total("fakematch_volatile"), report.total("fakematch_dead"), report.value("fakematch")),
            (1, 1, 1, 4)
        );
        assert_eq!(list(&[("tbs", &report)], "fakematch_other"), "tbs\tf.c:12\tone-pass\n");
        let rows: Vec<&str> = ROWS.iter().filter_map(|(metric, _)| metric.strip_prefix("fakematch_")).collect();
        assert_eq!(rows[..DEVICES.len()], DEVICES, "the table's device rows are S2's, in its order");
        assert_eq!(rows[DEVICES.len()..], ["other"], "then the tags of a kind S2 does not name");
        assert!(listable("fakematch_barrier") && listable("fakematch_volatile") && !listable("fakematch_one-pass") && !listable("xyz"));
        assert_eq!(report.value("leftover"), 0);
        assert_eq!(report.total("views"), 1);
        assert!(listable("views"));
        assert_eq!(report.total("goto"), 1);
        let hex = one("#define MASK 0x3f\nint F(int a) { return (a & 0xff) + (a << 0x10) + 0x1c + *(int *)0x04000130 + 26; }\n");
        assert_eq!((hex.total("hex"), hex.total("hex_mask"), hex.total("hex_address"), hex.total("hex_other")), (5, 3, 1, 1), "MASK by its name");
        assert_eq!(list(&[("tbs", &hex)], "hex_other"), "tbs\tf.c:2\t0x1c\n");
        // Bits by family, flag numbers by name or call, register values by header.
        let kinds = one(concat!(
            "#define SPEAKER_LEFT 0x1000\n#define SPEAKER_RIGHT 0x2000\n#define SPEAKER_FLAGS 0x3000\n#define SPEAKER_SIZE 0x30\n",
            "enum { TILE_HFLIP = 0x400, TILE_VFLIP = 0x800 };\n#define LONE_VALUE 0x80\n#define FLAG_DOOR_OPEN 0x9eb\n",
            "void F(void) { GameFlag_Set(0x8ee); Actor_Move(0x8ee); GameFlag_Set(0x10 + 1); }\n",
        ));
        assert_eq!((kinds.total("hex_mask"), kinds.total("hex_flag"), kinds.total("hex_other")), (5, 2, 4));
        assert_eq!(list(&[("tbs", &kinds)], "hex_other"), "tbs\tf.c:4\t0x30\ntbs\tf.c:6\t0x80\ntbs\tf.c:8\t0x8ee\ntbs\tf.c:8\t0x10\n");
        let registers = game([("games/tbs/include/system/io_reg.h", "#define REG_DISPCNT_MODE_1 0x0001\n#define WINOUT_ALL 0x3f\n")]);
        assert_eq!((registers.total("hex_mask"), registers.total("hex_other")), (2, 0));
        assert_eq!((report.total("if_language"), report.total("if_patch")), (2, 2));
        assert_eq!(report.total("if_in_function"), 1);
    }

    #[test]
    fn leftovers_are_counted_apart_from_devices() {
        let report = one(concat!(
            "void F(void) {\n",
            "    /* LEFTOVER(frame): an idle buffer the Japanese edition fills; as no buffer, 4 of 90 instructions differ in tbs-en. */\n",
            "    /* LEFTOVER(template): a second blitter, as its siblings have; as one, 6 of 80 instructions differ in tbs-en. */\n",
            "    /* LEFTOVER(form): a retired kind, not counted; as direct, 2 of 40 instructions differ in tbs-en. */\n",
            "    // LEFTOVER(dead): no kind of leftover, not counted.\n",
            "    // FAKEMATCH(frame): no kind of device, counted to be given one.\n",
            "    // DEAD: a read whose value goes unused.\n",
            "    /* FAKEMATCH(temporary): a copy; as none, 1 of 9 instructions differ in tbs-en. */\n",
            "}\n",
        ));
        assert_eq!(
            ["leftover_frame", "leftover_template", "leftover_other", "leftover_dead"].map(|metric| report.total(metric)),
            [1, 1, 2, 1]
        );
        assert_eq!((report.value("leftover"), report.value("fakematch")), (5, 2), "never summed together");
        assert_eq!(list(&[("tbs", &report)], "leftover_other"), "tbs\tf.c:4\tform\ntbs\tf.c:5\tdead\n", "a kind S2 does not name, counted to be given one");
        assert_eq!(list(&[("tbs", &report)], "fakematch_other"), "tbs\tf.c:6\tframe\n");
        let rows: Vec<&str> = ROWS.iter().filter_map(|(metric, _)| metric.strip_prefix("leftover_")).collect();
        assert_eq!(rows, ["frame", "template", "other", "dead"], "the table's leftover rows are S2's, those of a kind it does not name, then the DEAD marks");
        assert!(listable("leftover") && listable("leftover_frame") && !listable("leftover_pin"));
        assert_eq!(list(&[("tbs", &report)], "leftover"), "tbs\tf.c:2\tframe\ntbs\tf.c:3\ttemplate\ntbs\tf.c:4\tform\ntbs\tf.c:5\tdead\ntbs\tf.c:7\tdead\n");
        assert_eq!(list(&[("tbs", &report)], "fakematch"), "tbs\tf.c:6\tframe\ntbs\tf.c:8\ttemporary\n");
        let tsv = tsv(&[("tbs", &report)]);
        assert!(tsv.contains("tbs\tleftover\ttotal\t5\n") && !tsv.contains("leftover_form"), "{tsv}");
        assert!(tsv.contains("tbs\tleftover\tf.c\t5\n") && tsv.contains("tbs\tfakematch\tf.c\t2\n"), "{tsv}");
        let table = summary(&[("tbs", &report)], 0);
        let devices = table.find("FAKEMATCH devices").unwrap();
        let leftovers = table.find("LEFTOVERs (Camelot's)").unwrap();
        assert!(devices < leftovers && table[leftovers..].lines().nth(1).unwrap().trim_start().starts_with("frame"), "{table}");
    }

    #[test]
    fn declarations_without_documentation() {
        let header = concat!(
            "#ifndef ALCHEMY_F_H\n",
            "#define ALCHEMY_F_H\n",
            "/// Documented in its header.\n",
            "void Documented(void);\n\n",
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
            "void Plain(void) { Run(); }\n",
            "/// Its own.\n",
            "static void Local(void) {}\n\n",
            "UNUSED static int Helper(int x) { return x; }\n",
            "u8 gTable[] = { 1, 2 };\n",
            "u8 gBare;\n",
        );
        let report = game([("f.h", header), ("f.c", source)]);
        assert_eq!(report.total("undocumented_function"), 2, "Plain and Helper");
        assert_eq!(report.total("undocumented_global"), 3, "gBare, gHandler and CAPITAL_TABLE");
        assert_eq!(report.total("undocumented_macro"), 0, "BARE is in the run NAMED's comment heads");
        assert_eq!(report.total("undocumented_run"), 1);
        assert_eq!(report.total("undocumented_struct"), 2);
        assert_eq!(report.total("undocumented_enum"), 1);
        assert_eq!(report.top("undocumented_function", 5), vec![("f.c", 2)]);
    }

    #[test]
    fn a_shared_file_scenes_include_is_one_copy() {
        let shared = "void Actor_Blink(struct Actor *actor) {\n  switch (gFrameCount & 7) {\n  case 0:\n    Actor_SetColor(actor, 1);\n    break;\n  case 4:\n    Actor_SetColor(actor, 0);\n    break;\n  }\n}\n";
        let scene = "#include \"shared/actor_blink.h\"\n";
        let report = game([
            ("games/tla/src/scene/shared/actor_blink.h", shared),
            ("games/tla/src/scene/garoh.c", scene),
            ("games/tla/src/scene/madra.c", scene),
        ]);
        assert!(report.function_groups.is_empty());
        let copy = format!("{scene}{}", shared.replace("Actor_Blink", "Madra_Blink"));
        let report = game([("games/tla/src/scene/shared/actor_blink.h", shared), ("games/tla/src/scene/madra.c", copy.as_str())]);
        assert_eq!(report.function_groups[0].members, ["games/tla/src/scene/madra.c:Madra_Blink", "games/tla/src/scene/shared/actor_blink.h:Actor_Blink"]);
    }

    #[test]
    fn duplicates_long_files_and_report() {
        let body = "{ int idx; for (idx = 0; idx < 4; idx++) { gA[idx] = Load(idx) + 3; Store(gB, idx); } }\n";
        // A local renamed is the same routine; another table or callee is not.
        let renamed = body.replace("idx", "k");
        let other = body.replace("gA", "gC").replace("Load", "Fetch");
        let table = "= { 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15 };\n";
        let first = format!(
            "void Alpha(void) {body}void Beta(void) {renamed}void Eta(void) {other}void Gamma(void) {{ int i; }}\nconst u8 gT1[] {table}"
        );
        let long = "\n".repeat(3001);
        let second = format!("const u8 gT2[] {table}void Delta(void) {{}}{long}");
        // A map overlay, a part, a header and a data object stay one file however long.
        let part = format!("// Part of battle_effect.c.\nvoid Epsilon(void) {{}}{long}");
        let data = format!("const u8 gT3[] = {{ 0 }};{long}");
        let scene = format!("void Zeta(void) {{}}{long}");
        let report = game([
            ("a.c", first.as_str()),
            ("b.c", second.as_str()),
            ("games/tbs/src/scene/vale.c", scene.as_str()),
            ("games/tbs/src/battle_effect/battle_effect/bolt.c", part.as_str()),
            ("games/tbs/include/scene_exits.h", long.as_str()),
            ("games/tbs/src/field/table.c", data.as_str()),
        ]);
        assert_eq!(report.function_groups.len(), 1);
        assert_eq!(report.function_groups[0].members, vec!["a.c:Alpha", "a.c:Beta"]);
        assert_eq!(report.table_groups.len(), 1);
        assert_eq!(report.table_groups[0].members, vec!["a.c:gT1", "b.c:gT2"]);
        assert_eq!(report.long_files, vec![("b.c".to_string(), 3002)]);
        assert_eq!(list(&[("tbs", &report)], "duplicate_table"), "tbs\t29\ta.c:gT1 b.c:gT2\n");
        let tsv = tsv(&[("tbs", &report)]);
        assert!(tsv.starts_with("game\tmetric\tscope\tcount\ntbs\tfiles\ttotal\t6\n"));
        assert!(tsv.contains("tbs\tduplicate_function\ttotal\t1\n"));
        assert!(tsv.contains("tbs\tduplicate_function\ta.c:Alpha a.c:Beta\t"));
        assert!(tsv.contains("tbs\tlong_file\tb.c\t3002\n"));
        let table = summary(&[("tbs", &report)], 0);
        assert!(table.contains("objects over 3,000 lines") && table.lines().count() == ROWS.len() + 3);
    }

    #[test]
    fn what_needs_no_comment_of_its_own() {
        let header = concat!(
            "#ifndef OWN_GUARD\n#define OWN_GUARD\n\n",
            "/// The sizes.\n#define A 1\n#define B 2\n#if X\n#define C 3\n#endif\n\n",
            "// Plain head.\n#define D 4\n\n",
            "#define REG_X (*(vu16 *)0x4000000)\n#define REG_SOUNDCNT_H (*(vu16 *)0x4000082)\n",
            "extern char SceneId_Vale;\n#define SCENE_VALE ((s32)&SceneId_Vale)\n",
            "#ifndef DEFAULT\n#define DEFAULT 1\n#endif\n#define BARE 5\n#endif\n",
        );
        let scene = concat!(
            "/// The doors.\nu16 gVale_DoorA[] = { 1 };\nu16 gVale_DoorB[] = { 2 };\n\n",
            "struct Trigger gVale_Triggers[] = { {1} };\n\n",
            "s32 gVale_WalkScript[] = { SC_WAIT(1), SC_END };\n\n",
            "struct ActorPlacement gVale_Room4Placements[] = { {1} };\n\n",
            "const char *sGreeting = \"Hello\";\n\n",
            "// Plain.\nu8 gVale_Count;\n\n",
            "/// Its doc.\n// UB: read before it is set.\nu8 gVale_Documented;\n\n",
            "u8 gVale_Bare;\n\n",
            "/// Lifts the rock.\nDEFINE_LIFT_ROCK(Vale_LiftRock, 3)\n\n",
            "/// Its own.\nvoid Vale_Run(void) {}\n\n",
            "enum {\n  E_A, ///< a\n  E_B, ///< b\n};\n",
        );
        let far = "void Vale_RunFar(void);\n";
        let report = game([
            ("games/tbs/include/vale.h", header),
            ("games/tbs/src/scene/vale.c", scene),
            ("games/tbs/include/gamepak/far_call/field.h", far),
        ]);
        let names = |metric: &str| -> Vec<String> {
            report.items.iter().filter(|item| item.metric == metric).map(|item| item.what.clone()).collect()
        };
        assert_eq!(names("undocumented_guard"), ["OWN_GUARD"]);
        assert_eq!(names("undocumented_run"), ["B", "C", "gVale_DoorB"]);
        assert_eq!(names("undocumented_plain"), ["D", "gVale_Count"]);
        assert_eq!(names("undocumented_register"), ["REG_X", "REG_SOUNDCNT_H"]);
        assert_eq!(names("undocumented_id"), ["SCENE_VALE", "SceneId_Vale"]);
        assert_eq!(names("undocumented_macro"), ["DEFAULT", "BARE"]);
        assert_eq!(names("undocumented_scene"), ["gVale_Room4Placements", "gVale_Triggers", "gVale_WalkScript"]);
        assert_eq!(names("undocumented_string"), ["sGreeting"]);
        assert_eq!(names("undocumented_global"), ["gVale_Bare"]);
        assert_eq!(names("undocumented_far"), ["Vale_RunFar"]);
        assert_eq!((report.total("undocumented_function"), report.total("undocumented_entry"), report.total("undocumented_enum")), (0, 0, 0));
    }

    #[test]
    fn declarations_their_neighbours_or_kind_say() {
        let header = concat!(
            "#define NULL ((void *)0)\n#define TRUE 1\n\n",
            "/// The window's work.\nextern struct WindowWork *gWindowWork[2];\n\n",
            "#define WINDOW_WORK ((struct WindowWork *)gWindowWork[0])\n",
            "#define BARE_WORK ((struct WindowWork *)gBare)\n",
            "#define ALIAS gWindowWork\n#define ROUTINE_ALIAS Math_DivideBy\n\n",
            "extern void *gBare;\n\n",
            "/// Picks the width by edition.\n#if LANGUAGE == 1 || \\\n    LANGUAGE == 2\n#define WIDE 1\n#define NARROW 3\n#else\n#define WIDE 2\n#define NARROW 4\n#endif\n\n",
            "/// Divides.\ns32 Math_DivideBy(s32 a, s32 b);\n\n",
            "s32 __divsi3(s32 a, s32 b);\nvoid Bios_CpuSet(void);\n\n",
            "/// Sends, as a far call takes it.\ns32 Send(s32 a);\ns32 SendAgain(s32 a);\n\n",
            "static inline s32 Field_Send(s32 a) { return Send(a); }\n\n",
            "static inline s32 Field_Twice(s32 a) { return Send(Send(a)); }\n\n",
            "static inline void Call1(void (*f)(), s32 a0) { f(a0); }\n",
        );
        let code = concat!(
            "/// Does nothing, twice.\nvoid NoOpA(void) {}\nvoid NoOpB(void) {}\n\n",
            "void Empty(void) {\n}\n\n",
            "/// Gives Flint.\nstatic void Test_ReceiveFlint(void) {\n  Test_Give(1, 2);\n}\n\n",
            "static void Test_ReceiveFizz(void) {\n  Test_Give(3, 4);\n}\n\n",
            "static void Test_Other(void) {\n  Test_Take(3);\n}\n\n",
            "/// Counts frames between ripples.\nstruct Ripple {\n  s16 frames;\n};\n\nstruct Ripple gRipple;\n\n",
            "// UB: the shift overflows.\n#ifdef UBFIX\n#define SHIFTED(x) ((x) * 2)\n#else\n#define SHIFTED(x) ((x) << 1)\n#endif\n\n",
            "/// Builds.\nvoid Build(void) {\n  s32 count = 0;\n#define PUT(word) count++\n  PUT(1);\n#undef PUT\n}\n\n",
            "void Kr(x, y) s32 x;\ns16 y;\n{\n  Test_Give(x, y);\n}\n\n",
            "#define UNIT_DEFINE(type, name) type name(void) { return 0; }\n\n",
            "UNIT_DEFINE(s32, Unit_GetZero)\n",
        );
        let report = game([("games/tbs/include/a.h", header), ("games/tbs/src/a.c", code)]);
        let names = |metric: &str| -> Vec<String> {
            let mut names: Vec<String> = report.items.iter().filter(|item| item.metric == metric).map(|item| item.what.clone()).collect();
            names.sort();
            names
        };
        assert_eq!(names("undocumented_empty"), ["Empty", "NoOpB"], "NoOpB is also in NoOpA's run");
        assert_eq!(names("undocumented_run"), ["NARROW", "SendAgain", "Test_ReceiveFizz"], "a /// over a two-line #if; one call to the same routine past a blank line");
        assert_eq!(names("undocumented_forward"), ["Field_Send"]);
        assert_eq!(names("undocumented_library"), ["Bios_CpuSet", "__divsi3"]);
        assert_eq!(names("undocumented_standard"), ["NULL", "TRUE"]);
        assert_eq!(names("undocumented_view"), ["ALIAS", "WINDOW_WORK"]);
        assert_eq!(names("undocumented_typed"), ["gRipple"]);
        assert_eq!(names("undocumented_note"), ["SHIFTED"]);
        assert_eq!(names("undocumented_local"), ["PUT"]);
        assert_eq!(names("undocumented_macro"), ["BARE_WORK", "ROUTINE_ALIAS", "UNIT_DEFINE"], "gBare is undocumented; a routine's bare name is no view");
        assert_eq!(names("undocumented_function"), ["Call1", "Field_Twice", "Kr", "Test_Other", "Unit_GetZero"]);
        assert_eq!(names("undocumented_global"), ["gBare"], "an old-style definition's parameters are no globals");
    }

    #[test]
    fn a_shared_files_parameters() {
        let shared = concat!(
            "/// @file\n/// The spawner each scene copies: the includer gives SET_MODE and\n/// names the copy SPAWN.\n#include \"types.h\"\n\n",
            "extern s16 LINES[];\n\nvoid SET_MODE(s32 mode);\n\n",
            "void SPAWN(void) { SET_MODE(LINES[UNSAID]); }\n",
        );
        let scene = concat!(
            "#define SET_MODE Actor_Select\n#define UNSAID 3\n#include \"../field/spawn.c\"\n#undef SET_MODE\n\n",
            "#define SPAWN A_Spawn\n#define LINES gA_Lines\n#include \"../field/spawn.c\"\n\n",
            "#define OTHER 4\n#include \"../field/spawn.c\"\n\n",
            "#define NONE 5\n#include \"../field/spawn.c\"\n\n",
            "void A_Run(void) { Run(OTHER); }\n",
        );
        let report = game([("games/tla/src/field/spawn.c", shared), ("games/tla/src/scene/a.c", scene)]);
        let names = |metric: &str| -> Vec<String> {
            let mut names: Vec<String> = report.items.iter().filter(|item| item.metric == metric).map(|item| item.what.clone()).collect();
            names.sort();
            names
        };
        assert_eq!(names("undocumented_shared"), ["SET_MODE", "SET_MODE", "SPAWN", "SPAWN"], "the includer's macros and the shared file's declarations under them");
        assert_eq!(
            names("undocumented_macro"),
            [
                "LINES\ta parameter of games/tla/src/field/spawn.c, whose comments do not name it",
                "NONE",
                "OTHER",
                "UNSAID\ta parameter of games/tla/src/field/spawn.c, whose comments do not name it",
            ],
            "OTHER is used again, NONE not by the shared file"
        );
        assert_eq!(names("undocumented_global"), ["LINES\ta parameter of games/tla/src/field/spawn.c, whose comments do not name it"]);
    }

    /// A numbered family is one run under its head's `///`, and counted once
    /// at its head where it has none.
    #[test]
    fn a_numbered_family_is_one_run_or_one_item() {
        let header = concat!(
            "/// Calls f with that many arguments.\nstatic inline void Pass1(void (*f)(), s32 a0) { f(a0); }\n\n",
            "static inline void Pass2(void (*f)(), s32 a0, s32 a1) { f(a0, a1); }\n\n",
            "static inline void Call1(void (*f)(), s32 a0) { f(a0); }\n\n",
            "static inline void Call2(void (*f)(), s32 a0, s32 a1) { f(a0, a1); }\n\n",
            "static inline void Call11(void (*f)(), s32 a0, s32 a1, s32 a2, s32 a3, s32 a4, s32 a5, s32 a6, s32 a7, s32 a8, s32 a9, s32 a10) {\n",
            "  f(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10);\n}\n\n",
            "s32 Step1(s32 a) { return Walk(a); }\n\ns32 Other(s32 a) { return Walk(a); }\n\ns32 Step2(s32 a) { return Run(a); }\n",
        );
        let report = game([("games/tla/include/field/call.h", header)]);
        let names = |metric: &str| -> Vec<String> { report.items.iter().filter(|item| item.metric == metric).map(|item| item.what.clone()).collect() };
        assert_eq!(names("undocumented_run"), ["Pass2"]);
        assert_eq!(names("undocumented_family"), ["Call11", "Call2"]);
        assert_eq!(
            names("undocumented_function"),
            ["Call1\ta family without a head: Call1, Call2, Call11", "Other", "Step1", "Step2"],
            "Step1 and Step2 have a routine between them"
        );
    }

    /// A message declared in C is said by its text in the game's .po files.
    #[test]
    fn a_message_extern_is_said_by_its_text() {
        let po = "msgctxt \"MsgCanYouImagine\"\nmsgid \"00001\"\nmsgstr \"Can you imagine?\"\n";
        let ja = "msgctxt \"MsgFieldPasswordKana\"\nmsgid \"00002\"\nmsgstr \"kana\"\n";
        let text = GameText::parse(po, None).unwrap().with_edition_text(ja);
        let scene = "extern u8 MsgCanYouImagine[];\n\nextern u8 MsgFieldPasswordKana[];\n\nextern u8 MsgNoSuchText[];\n\nextern u8 gIzumo_Stones[];\n";
        let report = game_with_text([("games/tla/src/scene/izumo.c", scene)], Some(&text), &[]);
        let names = |metric: &str| -> Vec<String> { report.items.iter().filter(|item| item.metric == metric).map(|item| item.what.clone()).collect() };
        assert_eq!(names("undocumented_string"), ["MsgCanYouImagine", "MsgFieldPasswordKana"], "one only another edition's text holds too");
        assert_eq!(names("undocumented_global"), ["MsgNoSuchText", "gIzumo_Stones"], "a symbol the text does not hold stays");
    }

    #[test]
    fn one_body_under_two_names_in_one_object() {
        let body = "{ int i; for (i = 0; i < 4; i++) { gA[i] = Load(i) + 3; Store(gB, i); } }\n";
        let scene = format!(
            "#include \"../field/p.c\"\nvoid A_Run(void) {{ Task_Add(A_Step); Task_Add(P_Step); }}\n\
             /// Steps.\n// The same routine as P_Step, under another name.\nvoid A_Step(void) {body}"
        );
        let part = format!("void P_Step(void) {body}");
        let members = |report: &Report| (report.function_groups.len(), report.said_groups.iter().map(|group| group.members.clone()).collect::<Vec<_>>());
        let report = game([("games/tla/src/scene/a.c", scene.as_str()), ("games/tla/src/field/p.c", part.as_str())]);
        assert_eq!(members(&report), (0, vec![vec!["games/tla/src/field/p.c:P_Step".to_string(), "games/tla/src/scene/a.c:A_Step".to_string()]]));
        let apart = scene.replace("#include \"../field/p.c\"\n", "");
        let report = game([("games/tla/src/scene/a.c", apart.as_str()), ("games/tla/src/field/p.c", part.as_str())]);
        assert_eq!(members(&report), (1, vec![]), "two objects: a copy to write once");
        assert_eq!(normalized("games/tla/src/scene/../field/p.c"), "games/tla/src/field/p.c");
    }

    #[test]
    fn duplicates_judged_by_row() {
        let body = "{ int i; for (i = 0; i < 4; i++) { gA[i] = Load(i) + 3; Store(gB, i); } }\n";
        let (one, two, three) = (format!("void A_Step(void) {body}"), format!("void B_Step(void) {body}"), format!("void C_Walk(void) {body}"));
        let files = [("games/tla/src/a.c", one.as_str()), ("games/tla/src/b.c", two.as_str()), ("games/tla/src/c.c", three.as_str())];
        let rows = evidence::parse_duplicates(concat!(
            "members\tkind\twhy\n",
            "A_Step B_Step C_Walk\tkept apart\tthree main-image objects\n",
            "A_Step B_Step\tdifferent things\tno longer one group\n",
            "D_Gone E_Gone\tdifferent things\tgone\n",
        ))
        .unwrap();
        let report = game_all(files, None, &[], &[], &rows);
        assert_eq!((report.value("duplicate_function"), report.value("duplicate_judged")), (0, 1));
        assert_eq!(report.judged_groups[0].1, "kept apart");
        let listed = list(&[("tla", &report)], "duplicate_judged");
        assert!(listed.ends_with("games/tla/src/c.c:C_Walk\tkept apart: three main-image objects\n"), "{listed}");
        assert_eq!(
            report.stale_duplicates(),
            [
                ("A_Step B_Step".to_string(), "different things: no longer one group".to_string()),
                ("D_Gone E_Gone".to_string(), "different things: D_Gone, E_Gone gone".to_string()),
            ]
        );
        let unjudged = game_all(files, None, &[], &[], &[]);
        assert_eq!((unjudged.value("duplicate_function"), unjudged.value("duplicate_judged")), (1, 0), "a group no one judged stays counted");
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
            "void Event_Wait(s32 frames);\n",
            "#define WAIT(n) Event_Wait(n)\n",
            "#define INTtoFIXED(value) ((value) << 16)\n",
            "#define BLEND(top, bottom) Blend((top) | ((bottom) << 8))\n",
            "void Actor_SetPosition(s32 actor, s32 fixed_x, s32 fixed_z);\n",
            "static inline void Actor_SetSpeed(s32 actor, s32 speed, s32 acceleration) { Engine(actor, speed); }\n",
            "void Old();\n",
            "void Blend(s32);\n",
        );
        let source = concat!(
            "void Run(void) {\n",
            "    WAIT(80);\n",
            "    Event_Wait(\n        40);\n",
            "    Actor_SetPosition(ACTOR_LEADER, INTtoFIXED(312), (416) << 16);\n",
            "    Actor_SetSpeed(ACTOR_LEADER, INTtoFIXED(24), 0x8000);\n",
            "    Old(1, 20);\n",
            "    BLEND(10, 12);\n",
            "    draw[0](canvas, 64, 40);\n",
            "}\n",
        );
        let report = game([("f.h", header), ("f.c", source)]);
        assert_eq!(report.total("data_argument_frames"), 2, "WAIT(80) and 40");
        assert_eq!(report.total("data_argument_coordinate"), 2, "312 and 416, through PIXELS and a group");
        assert_eq!(report.total("data_argument_composed"), 1, "INTtoFIXED(24), a speed in pixels");
        assert_eq!(report.total("data_argument"), 5);
        // 0x8000 (an acceleration), 20, 10 and 12 (BLEND calls Blend), 64 and 40;
        // the shifts' 16, in PIXELS and in Run, are no arguments, and
        // INTtoFIXED's own 16 is its definition.
        assert_eq!((report.total("magic_argument"), report.total("magic_code")), (6, 7));
        assert_eq!(report.total("honest_definition"), 1);
        let listed = list(&[("tbs", &report)], "magic_argument");
        assert!(listed.starts_with("tbs\tf.c:6\t0x8000\tActor_SetSpeed acceleration\ntbs\tf.c:7\t20\tOld #2\n"), "{listed}");
        assert!(listed.ends_with("tbs\tf.c:9\t40\t-\n"), "{listed}");
        assert!(list(&[("tbs", &report)], "data_argument").contains("tbs\tf.c:2\t80\tWAIT n\n"));
    }

    #[test]
    fn hex_flags_by_the_parameter_they_fill() {
        let report = game([
            ("a.h", "#define SCENE_ITEM(kind, contents, flag) {kind, contents, flag}\nvoid Chest_Open(s32 actor, s32 doneFlag);\n"),
            ("b.c", "void Run(void) {\n    Chest_Open(3, 0x9eb);\n    Other(3, 0x9ec);\n}\nstruct Item gItems[] = {\n    SCENE_ITEM(2, 4, 0x010),\n};\n"),
        ]);
        assert_eq!((report.total("hex_flag"), report.total("hex_other")), (2, 1), "a flag parameter's argument, a macro's and a function's, and Other's unknown one");
    }

    #[test]
    fn levels_of_a_canvas_or_a_channel_and_half_cells() {
        let header = concat!(
            "#define PIXELS_8BPP(byte) ((byte) * 0x01010101)\n",
            "#define CANVAS_FILL_WORD(level) PIXELS_8BPP(level)\n",
            "#define RGB_BLUE(color) (((color) >> 10) & 31)\n",
            "#define FIELD_CELL_SHIFT 20\n",
            "#define FIELD_HALF_CELL(coordinate) ((coordinate) >> (FIELD_CELL_SHIFT - 1))\n",
            "void Fill(void *canvas, s32 size, u32 word);\n",
            "void Pick(s32 level);\n",
        );
        let source = concat!(
            "void Run(void) {\n",
            "    Fill(canvas, 64, CANVAS_FILL_WORD(47));\n",
            "    blue = RGB_BLUE(value) - 20;\n",
            "    if (FIELD_HALF_CELL(actor->z.fixed) >= 36) {}\n",
            "    Pick(33);\n",
            "}\n",
        );
        let report = game([("f.h", header), ("f.c", source)]);
        let listed = |metric: &str| -> Vec<String> {
            report.items.iter().filter(|item| item.metric == metric).map(|item| item.what.split('\t').next().unwrap().to_string()).collect()
        };
        assert_eq!(listed("honest_level"), ["47", "20"], "a canvas fill's level, a channel's");
        assert_eq!(listed("honest_position"), ["36"], "a half cell, its shift less one");
        assert_eq!(listed("magic_code"), ["33"], "a level no canvas, palette or fade takes");
    }

    #[test]
    fn numbers_judged_by_their_rows() {
        let source = concat!(
            "#define PIXELS(c) ((c) * 0x1111)\n",
            "#define THUMB_BL_HIGH 0xf000\n",
            "#define PASSWORD_CRC_POLYNOMIAL 0x1021\n",
            "#define TASK_PASS_FRAME 0x480\n",
            "enum { VENUS_TAG = 0x212 };\n",
            "void Run(void) {\n",
            "    GameFlag_Set(0x169);\n",
            "    Wait(20);\n",
            "    Wait(20);\n",
            "    Other(20);\n",
            "    if (frame == 140) {}\n",
            "    x = gTable[37];\n",
            "}\n",
            "void Next(void) { Wait(20); }\n",
        );
        let row = |routine: &str, number: &str, kind: &str| evidence::Judged {
            file: "f.c".to_string(),
            routine: routine.to_string(),
            number: number.to_string(),
            kind: kind.to_string(),
            what: "w".to_string(),
        };
        let rows = [
            row("Run", "20", "honest"),
            row("Run", "0x169", "unsettled"),
            row("Run", "140", "unsettled"),
            row("TASK_PASS_FRAME", "0x480", "honest"),
            row("VENUS_TAG", "0x212", "unsettled"),
        ];
        let report = game_judged([("f.c", source)], None, &[], &rows);
        let listed = |metric: &str| -> Vec<String> {
            report.items.iter().filter(|item| item.metric == metric).map(|item| item.what.split('\t').next().unwrap().to_string()).collect()
        };
        // Every 20 in its routine, and no other routine's; a number a rule
        // finds honest stays so whatever its row says.
        assert_eq!(listed("honest_judged"), ["20", "20", "20"]);
        assert_eq!(listed("magic_code"), ["0x169", "37", "20"]);
        assert_eq!(listed("magic_unsettled"), ["0x169"]);
        assert_eq!(listed("honest_duration"), ["140"]);
        // A function-like macro's own hex, an instruction's encoding and a
        // CRC's polynomial by their names, and the rest by their rows.
        assert_eq!((report.total("hex_definition"), report.total("hex_encoding")), (1, 2));
        assert_eq!(listed("hex_judged"), ["0x480"]);
        assert_eq!((listed("hex_other"), listed("hex_unsettled")), (vec!["0x212".to_string()], vec!["0x212".to_string()]));
        // Without the rows, both are to judge.
        let plain = game([("f.c", source)]);
        assert_eq!((plain.total("hex_other"), plain.total("magic_unsettled"), plain.total("honest_judged")), (2, 0, 0));
    }

    #[test]
    fn honest_numbers_by_the_calls_they_stand_in() {
        let header = concat!(
            "void MapGrid_CopyCellPictureCellsRect_(s32 a0, s32 a1, s32 a2, s32 a3, s32 a4, s32 a5);\n",
            "void Actor_SetAnimation(s32 a0, s32 a1);\n",
            "void Actor_SetAnimationSpeed(void *actor, s32 a1);\n",
            "#define OBJ_X(x) ((x) << 16)\n",
        );
        let source = concat!(
            "void Run(void) {\n",
            "    u16 counts[16];\n",
            "    MapGrid_CopyCellPictureCellsRect_(x, 125, 3, 3, 39, 85);\n",
            "    Actor_SetAnimation(ACTOR_ALEX, 13);\n",
            "    Actor_SetAnimation(12, 25);\n",
            "    Actor_SetAnimationSpeed(leader, 48);\n",
            "    word = OBJ_X((560 - digits * 10) & 0x1ff);\n",
            "    fade = -16;\n",
            "    gTable[12] = 0;\n",
            "    F(a,\n",
            "      x - 32);\n",
            "    F(a, x - 32);\n",
            "}\n",
        );
        let report = game([("f.h", header), ("f.c", source), ("g.h", "void F(void *a, s32 x);\n")]);
        let listed = |metric: &str| -> Vec<String> {
            report.items.iter().filter(|item| item.metric == metric).map(|item| item.what.split('\t').next().unwrap().to_string()).collect()
        };
        // The array's own size, the map grid's places and sizes, an OAM
        // word's place (not its hex mask), and a wrapped call's argument
        // classed as the same call on one line.
        assert_eq!(listed("honest_position"), ["16", "125", "39", "85", "560", "10", "32", "32"]);
        assert_eq!(listed("honest_animation"), ["13", "25"], "the second argument only; a speed is a rate");
        assert_eq!(listed("honest_level"), ["16"]);
        assert_eq!(listed("magic_code"), ["12", "48", "0x1ff", "12"], "an actor id, a speed, a mask and an index");
        assert_eq!(listed("honest_definition"), ["16"], "OBJ_X's own shift is the macro's definition");
    }

    #[test]
    fn honest_numbers_by_the_parameters_they_fill() {
        let header = concat!(
            "typedef s32 (*BattleEffectDrawRectangle)(void *destination, const void *source,\n",
            "                                         s32 x, s32 y, s32 width, s32 height);\n",
            "typedef BattleEffectDrawRectangle DrawRectangle;\n",
            "typedef DrawRectangle BlitterPair[2];\n",
            "static __inline__ void Call1(void (*f)(), s32 a0) { f(a0); }\n",
            "static __inline__ void Call3(void (*f)(), s32 a0, s32 a1, s32 a2) { f(a0, a1, a2); }\n",
            "#define C1(f, a) Call1((void (*)())f, a)\n",
            "void Event_Wait(s32 frames);\n",
            "void Actor_Face(s32 actorId, s32 facing, s32 delay);\n",
            "void Actor_WalkBy(s32 actorId, s32 dx, s32 dz);\n",
            "void ScreenFade_ToBlack(s32 vblanks);\n",
            "void Window_Clear(void *window, s32 left, s32 top, s32 right, s32 bottom);\n",
            "void Blend(s32 top, s32 bottom);\n",
            "void Particle_Advance(void *particle, s32 damping, s32 gravity);\n",
            "u32 Random_Below(u32 n);\n",
            "s32 Math_Remainder(u32 value, u32 divisor);\n",
            "s32 Trig_Sin(s32 angle);\n",
        );
        let source = concat!(
            "void Run(BlitterPair blitters, DrawRectangle *draw) {\n",
            "    blitters[0](canvas, sheet, 48, 24, 104, 120);\n",
            "    draw[1](canvas, sheet, 12, 0, 58, 16);\n",
            "    C1(Event_Wait, 60);\n",
            "    Call3((void (*)())Actor_Face, 30, 2, 20);\n",
            "    Actor_WalkBy(ACTOR_IODEM, -16, 0);\n",
            "    Actor_WalkBy(ACTOR_IODEM, x - 32, z << 16);\n",
            "    ScreenFade_ToBlack(30);\n",
            "    Window_Clear(window, 0, 96, 224, 104);\n",
            "    Blend(10, 12);\n",
            "    Particle_Advance(mote, 60, 0);\n",
            "    if (Random_Below(120) == 0) { gTimer = Math_Remainder(Random_Next(), 90) + 60; }\n",
            "    gSine = Trig_Sin(row * 3553);\n",
            "}\n",
        );
        // A file's own typedef, naming no parameter, hides the header's from
        // that file alone.
        let other = concat!(
            "typedef s32 (*DrawRectangle)(void *, const void *, s32, s32, s32, s32);\n",
            "void Other(void) { DrawRectangle draw; draw(canvas, sheet, 40, 50, 60, 70); }\n",
        );
        let hidden = game([("f.h", header), ("f.c", source), ("g.c", other)]);
        assert_eq!(hidden.items.iter().filter(|item| item.file == "g.c" && item.metric == "magic_code").count(), 4);
        assert_eq!(hidden.total("honest_pointer"), 9);
        let report = game([("f.h", header), ("f.c", source)]);
        let listed = |metric: &str| -> Vec<String> {
            report.items.iter().filter(|item| item.metric == metric).map(|item| item.what.split('\t').next().unwrap().to_string()).collect()
        };
        // Through the pointers' type: x, y, width and height, and the second
        // call's 12, 58 and 16; through the call layers: Event_Wait's frames
        // and Actor_Face's delay, not its actor 30.
        assert_eq!(listed("honest_pointer"), ["48", "24", "104", "120", "12", "58", "16", "60", "20"]);
        // -16 is passed alone, as 16 would be: dx's data. 32 is an offset in
        // dx; 16 is a shift's amount; the window's edges are a rectangle's,
        // Blend's are not.
        assert_eq!(listed("honest_position"), ["32", "96", "224", "104"]);
        assert_eq!(listed("data_argument_coordinate"), ["16"]);
        assert_eq!(listed("honest_duration"), ["30"]);
        assert_eq!(listed("honest_damping"), ["60"]);
        assert_eq!(listed("honest_random"), ["120", "90", "60"]);
        // An angle reckoned from a row is no position.
        assert_eq!(listed("magic_code"), ["30", "16", "10", "12", "3553"]);
        assert_eq!(report.total("magic_code_all"), 23);
    }

    #[test]
    fn places_on_the_map_grid_by_its_own_macros() {
        let header = concat!(
            "#define FIELD_CELL(coordinate) ((coordinate) >> FIELD_CELL_SHIFT)\n",
            "#define CELL_HEIGHT_STEPS(height) ((height) >> 19)\n",
            "#define TILE(position) ((position) / toFIXED(16))\n",
            "#define CELL_CODE(attributes) ((attributes) >> 8)\n",
            "#define FIELD_CELL_ORIGIN(cell) ((cell) << FIELD_CELL_SHIFT)\n",
        );
        let source = concat!(
            "void Run(void) {\n",
            "    if (FIELD_CELL(actor->x.fixed) == 15 && FIELD_CELL(actor->z.fixed) != 37) {}\n",
            "    Set(FIELD_CELL(actor->x.fixed) - 42, 0);\n",
            "    if (CELL_HEIGHT_STEPS(actor->y.fixed) > 12 || TILE(x) <= 45) {}\n",
            "    index = FIELD_CELL(x) + FIELD_CELL(z) * 128;\n",
            "    if (CELL_CODE(cell) == 13) {}\n",
            "    if (FIELD_CELL_ORIGIN(cell) == 20) {}\n",
            "}\n",
        );
        let report = game([("f.h", header), ("f.c", source)]);
        let listed = |metric: &str| -> Vec<String> {
            report.items.iter().filter(|item| item.file == "f.c" && item.metric == metric).map(|item| item.what.clone()).collect()
        };
        assert_eq!(listed("honest_position"), ["15", "37", "42", "12", "45"]);
        // The map's width a row is reckoned by, a cell's code, and a number
        // compared with a coordinate brought up from a cell.
        assert_eq!(listed("magic_code"), ["128", "13", "20"]);
    }

    #[test]
    fn levels_percentages_and_edges_by_the_parameters_they_fill() {
        let header = concat!(
            "void Palette_BrightenSavedBattleBg(s32 red, s32 green, s32 blue);\n",
            "void BattleEffect_StartActorMotion(s32 id, s32 force, s32 percent);\n",
            "void PagedList_DrawPageNumbers(void *window, s32 count, s32 rows, s32 page, s32 rightEdge);\n",
            "#define MapGrid_SetShake(horizontal, vertical, decay) Shake((horizontal), (vertical), (decay))\n",
            "#define CameraShift(horizontal, vertical) Shift((horizontal), (vertical))\n",
            "void Unit_LevelUpTo(s32 owner, s32 level);\n",
            "void Tint(s32 redScale, s32 redPhase, s32 fadeMode, s32 edgeStyle, s32 percentIndex);\n",
        );
        let source = concat!(
            "void Run(void) {\n",
            "    Palette_BrightenSavedBattleBg(0, -20, frame - 52);\n",
            "    BattleEffect_StartActorMotion(id, 12, 150);\n",
            "    BattleEffect_StartActorMotion(id, 12, i * 10 + 110);\n",
            "    PagedList_DrawPageNumbers(window, count, 4, page, 28);\n",
            "    MapGrid_SetShake(16, 16, 1);\n",
            "    CameraShift(16, 16);\n",
            "    Unit_LevelUpTo(ISAAC, 10);\n",
            "    Tint(12, 40, 16, 20, 30);\n",
            "}\n",
        );
        let report = game([("f.h", header), ("f.c", source)]);
        let listed = |metric: &str| -> Vec<String> {
            report.items.iter().filter(|item| item.metric == metric).map(|item| item.what.split('\t').next().unwrap().to_string()).collect()
        };
        assert_eq!(listed("honest_level"), ["20", "16", "16"], "a colour channel's and a shake's");
        assert_eq!(listed("honest_duration"), ["52"], "its own words first: a frame");
        assert_eq!(listed("honest_decimal"), ["150", "10", "110"]);
        assert_eq!(listed("honest_position"), ["28"]);
        // A force, a shift that is no shake, a unit's level, and a scale, a
        // phase, a mode, a count and an index named with a tone's, an edge's
        // or a percent's word.
        assert_eq!(listed("magic_code"), ["12", "12", "16", "16", "10", "12", "40", "16", "20", "30"]);
    }

    #[test]
    fn speeds_masks_levels_and_old_style_parameters_by_the_calls_they_fill() {
        let header = concat!(
            "#define toFIXED(a) ((s32)((a) * FIXED_ONE))\n",
            "#define OBJ_X(x) ((x) << 16)\n",
            "#define Io_Write16(v, reg) (*(reg) = (v))\n",
            "#define FieldCamera_SetSpeed(speed) FieldCamera_SetSpeedFX(toFIXED(speed), toFIXED(speed) / 8)\n",
            "void Actor_Jump(s32 actorId, s32 speed, s32 frames);\n",
            "void Actor_SetAnimationSpeed(s32 actorId, s32 speed);\n",
            "s32 Trig_Sin(s32 angle);\n",
            "void Camera_SetRange(s32 centerX, s32 centerY);\n",
            "void Quiet(s32, s32);\n",
            "typedef s32 (*DrawRectangle)(void *destination, const void *source, s32 x, s32 y, s32 width, s32 height);\n",
            "typedef s32 (*Unnamed)(void *, const void *, s32, s32);\n",
        );
        let source = concat!(
            "void Actor_Jump(s32 actorId, s32 speed, s32 frames) { gActor.velocityY = toFIXED(speed); }\n",
            "void Actor_SetAnimationSpeed(s32 actorId, s32 speed) { gActor.animationSpeed = speed; }\n",
            "void Pop(x, frames) s32 x;\ns32 frames;\n{\n}\n",
            "void Run(void) {\n",
            "    Actor_Jump(ACTOR_RIME, 11, 0);\n",
            "    FieldCamera_SetSpeed(16);\n",
            "    Actor_SetAnimationSpeed(ACTOR_RIME, 12);\n",
            "    word = OBJ_X(x & 0x1ff);\n",
            "    v = (step & 0xff) * STEP;\n",
            "    Io_Write16(31, &REG_BLDY);\n",
            "    Io_Write16(30, &REG_BG0VOFS);\n",
            "    sway = Trig_Sin((frame - 486) * STEP);\n",
            "    sway = Trig_Sin(frame * 487);\n",
            "    Camera_SetRange(toFIXED(120), toFIXED(118 - offset));\n",
            "    Pop(28, 20);\n",
            "    Quiet(29, 21);\n",
            "    ((DrawRectangle)draw[0])(canvas, sheet, px + 101, py + 26, 4, 8);\n",
            "    ((Unnamed)draw[0])(canvas, sheet, px + 102, py + 27);\n",
            "}\n",
        );
        let report = game([("f.h", header), ("f.c", source)]);
        let listed = |metric: &str| -> Vec<String> {
            report.items.iter().filter(|item| item.metric == metric).map(|item| item.what.split('\t').next().unwrap().to_string()).collect()
        };
        // A plain speed the routine or macro turns into 16.16 itself, as S3
        // has a call take one; an animation's speed kept as it comes is not.
        assert_eq!(listed("data_argument_speed"), ["11", "16"]);
        assert_eq!(listed("honest_mask"), ["0xff"], "the OAM word's mask is io_reg.h's OBJ_X_MASK");
        assert_eq!(listed("honest_level"), ["31"], "written to the brightness register");
        assert_eq!(listed("honest_duration"), ["486"], "a moment reckoned in its own group before it turns an angle");
        assert_eq!(listed("honest_position"), ["118"], "through toFIXED to the camera's centerY");
        // An old-style definition's parameters, by their names; a prototype
        // of unnamed types names none.
        assert_eq!(listed("data_argument_coordinate"), ["120", "28"]);
        assert_eq!(listed("data_argument_frames"), ["20"]);
        // Through a pointer cast to a type that names its parameters.
        assert_eq!(listed("honest_pointer"), ["101", "26"]);
        assert_eq!(listed("magic_code"), ["12", "0x1ff", "30", "487", "29", "21", "102", "27"]);
    }

    #[test]
    fn hex_kept_as_a_chips_registers_and_a_named_all_ones() {
        let report = game([
            ("games/tla/include/gamepak/flash.h", "#define FLASH_COMMAND_RESET 0xF0\n#define FLASH_UNLOCK_ADDRESS 0x2AAA\n"),
            ("games/tla/include/field/flash_menu.h", "#define MENU_FLASH_ROW 0x2AAA\n"),
            ("a.h", "#define PASSWORD_CRC_SEED 0xffff\n#define CRC_POLYNOMIAL 0x1021\n"),
            ("b.c", "void F(void) { gSeed = 0xffff; }\n"),
        ]);
        assert_eq!((report.total("hex_mask"), report.total("hex_encoding"), report.total("hex_other")), (3, 1, 2), "a CRC's polynomial by its name");
        assert_eq!(
            list(&[("tla", &report)], "hex_other"),
            "tla\tb.c:1\t0xffff\ntla\tgames/tla/include/field/flash_menu.h:1\t0x2AAA\n",
            "an all-ones value with no name, and a header that is not the chip's"
        );
    }

    #[test]
    fn hex_kept_as_a_fields_values_a_reckoned_bit_and_pixel_words() {
        let header = concat!(
            "enum ExitStyle {\n",
            "  EXIT_STYLE_NORMAL = 0x00,\n",
            "  EXIT_STYLE_SILENT = 0x10,\n",
            "  EXIT_STYLE_HIDDEN_LAYER = 0x50,\n",
            "  EXIT_STYLE_MASK = 0x70,\n",
            "  EXIT_STYLE_WIDE = 0x180\n",
            "};\n",
            "#define SCOOPED_COINS 0x8000\n",
            "#define COIN_LIMIT 0xc000\n",
            "#define HALF_TURN 0x4000\n",
        );
        let source = concat!(
            "const u32 gSparkleTiles[] = { 0x00007777, 0x0000e9eb };\n",
            "const u32 gRoom_Offsets[] = { 0x1234 };\n",
            "void F(void) {\n",
            "    if (item & SCOOPED_COINS) item = SCOOPED_COINS + 1;\n",
            "    v = (v & COIN_LIMIT) + COIN_LIMIT;\n",
            "    n = HALF_TURN + 1;\n",
            "}\n",
        );
        let report = game([("f.h", header), ("f.c", source)]);
        // A field's values within its mask; a single bit also reckoned with;
        // a picture's pixel words. Not: a value past the field's mask, two
        // bits reckoned with, a bit never used as one, a table of offsets.
        assert_eq!(report.total("hex_pixels"), 2);
        assert_eq!(list(&[("tla", &report)], "hex_other"), "tla\tf.c:2\t0x1234\ntla\tf.h:6\t0x180\ntla\tf.h:9\t0xc000\ntla\tf.h:10\t0x4000\n");
    }

    #[test]
    fn hardware_addresses_in_code() {
        let source = "void F(void) {\n    Dma_Set((const void *)0x06000290, (void *)0x0e000000, 16);\n    v = 0x01ffffff;\n    w = 0x10000000;\n    m = 0x0ffff000 & draw;\n}\n";
        let report = game([("f.c", source)]);
        let listed = |metric: &str| -> Vec<String> {
            report.items.iter().filter(|item| item.metric == metric).map(|item| item.what.split('\t').next().unwrap().to_string()).collect()
        };
        assert_eq!(listed("honest_address"), ["0x06000290", "0x0e000000"]);
        assert_eq!(listed("magic_code"), ["16", "0x01ffffff", "0x10000000"], "below EWRAM and past the save memory");
        assert_eq!(listed("honest_mask"), ["0x0ffff000"], "a mask first");
    }

    #[test]
    fn an_alias_takes_its_routines_parameters() {
        let header = concat!(
            "void Text_DrawStringInWindowFar(u8 *text, s32 window, u32 x, u32 y);\n",
            "void Text_DrawStringAtOffsetFar(u8 *text, s32 window, s32 offsetX, s32 offsetY);\n",
            "void Text_DrawCount(u8 *text, s32 window, s32 count, s32 tone);\n",
            "#if LANGUAGE == LANGUAGE_JAPANESE\n#define LABEL Text_DrawStringInWindowFar\n#else\n#define LABEL Text_DrawStringAtOffsetFar\n#endif\n",
            "#if LANGUAGE == LANGUAGE_JAPANESE\n#define MIXED Text_DrawStringInWindowFar\n#else\n#define MIXED Text_DrawCount\n#endif\n",
        );
        let source = "void Run(void) {\n    LABEL(text, window, 40, 16);\n    MIXED(text, window, 40, 16);\n}\n";
        let report = game([("f.h", header), ("f.c", source)]);
        assert_eq!(report.total("data_argument_coordinate"), 2, "x and offsetX agree");
        assert_eq!(list(&[("tbs", &report)], "magic_argument"), "tbs\tf.c:3\t40\tMIXED #3\ntbs\tf.c:3\t16\tMIXED #4\n", "a coordinate in one edition, a count in the other");
    }

    #[test]
    fn a_signed_number_passed_alone_is_an_argument() {
        let header = "#define toFIXED(a) ((s32)((a) * FIXED_ONE))\nvoid Unit_AddPp(s32 unit, s32 amount);\n";
        let source = "void Run(void) {\n    actor->groundHeight = toFIXED(-240);\n    Unit_AddPp(IVAN, -35);\n    F(a - 16);\n}\n";
        let report = game([("f.h", header), ("f.c", source)]);
        assert_eq!(report.total("data_argument_composed"), 1, "toFIXED(-240)");
        assert_eq!(list(&[("tbs", &report)], "magic_argument"), "tbs\tf.c:3\t35\tUnit_AddPp amount\n", "a lone amount, still to name");
        assert_eq!(report.total("magic_code"), 2, "35 and the 16 a difference takes");
    }

    #[test]
    fn steps_and_indices_of_a_files_own_macros() {
        let scene = concat!(
            "#define STAKE(n) (ACTOR_FIRST_PLACED + (n))\n",
            "#define STAKE_ROW(n) STAKE(n), 0\n",
            "#define WALK(state_, x, z) \\\n  case state_: \\\n    Move((s32)actor, x, 0, z); \\\n    *state = state_ + 1; \\\n    break;\n",
            "#define ARRIVE(state_, next) \\\n  case state_: \\\n    if (Arrived(actor)) \\\n      *state = next; \\\n    break;\n",
            "void Move(s32 actor, s32 x, s32 y, s32 z);\n",
            "void Roam(struct Actor *actor) {\n  u8 *state = &actor->riseCounter;\n  switch (*state) {\n",
            "    WALK(16, 232, 744)\n    ARRIVE(17, 18)\n  case 19:\n    *state = 32;\n    break;\n  }\n",
            "  Sink(STAKE(10));\n  Sink(Get(11));\n}\n",
        );
        let shared = "#define FLOOR(n) (FLOOR_FIRST + (n))\nvoid Climb(void) {\n  Go(FLOOR(12));\n}\n";
        let report = game([("games/tla/src/scene/yampi.c", scene), ("games/tla/include/floors.h", shared)]);
        let listed = |metric: &str| -> Vec<String> {
            report.items.iter().filter(|item| item.metric == metric).map(|item| item.what.split('\t').next().unwrap().to_string()).collect()
        };
        assert_eq!(listed("honest_own_macro"), ["16", "17", "18", "19", "32", "10"], "the route's steps, its plain steps too, and the stake's index");
        assert_eq!(listed("magic_code"), ["12", "11"], "a header's macro, which is no file's own, and another call's");
    }

    #[test]
    fn chances_debug_rooms_and_the_parameters_a_definition_names() {
        let header = concat!(
            "void Approach(s32 unit, s32 target, s32 travelDivisor, s32 vy);\n",
            "void ApproachFar(s32 unit, s32 target, s32 travel_divisor, s32 vy);\n",
            "void Push(s32 actor, s32 percent);\n",
            "void FallOver(s32 id, u32 angle, s32 animation);\n",
            "typedef void (*DrawRectangle)(void *canvas, s32 x, s32 y);\n",
            "#define BLITTER(slot) ((DrawRectangle)gSlots[slot])\n",
        );
        let motion = concat!(
            "void Approach(s32 unit, s32 target, s32 frames, s32 vy) {\n  Wait(frames);\n}\n",
            "void Push(s32 actor, s32 rate) {\n  x = rate * d / 100;\n}\n",
            "s32 Battle_GetEffectBaseSuccessRate(s32 effect) {\n  s32 fixedChance;\n  switch (effect) {\n  case A:\n    return 70;\n  default:\n    fixedChance = 100;\n  }\n  return -fixedChance;\n}\n",
            "s32 Battle_GetMultiplier(s32 effect) {\n  return 70;\n}\n",
            "void Run(void) {\n  Approach(a, b, 13, 0);\n  ApproachFar(a, b, 24, 0);\n  Push(a, 20);\n  FallOver(a, 0, 13);\n  BLITTER(2)(canvas, fall + 47, 48);\n  Opt(a, 0, 20);\n}\n",
        );
        let debug = "void Test(void) {\n  FieldUnit_AddHp(ISAAC, -100);\n  coins = 777777;\n  GameFlag_Set(0x169);\n}\n";
        let report = game([("f.h", header), ("motion.c", motion), ("games/tbs/src/debug/test.c", debug), ("games/tbs/src/debug/test_scenes.c", debug)]);
        let listed = |metric: &str| -> Vec<String> {
            report.items.iter().filter(|item| item.metric == metric).map(|item| item.what.split('\t').next().unwrap().to_string()).collect()
        };
        assert_eq!(listed("data_argument_frames"), ["13", "24"], "the definition's frames, for its far call too");
        assert_eq!(listed("honest_decimal"), ["100", "70", "100", "20"], "a routine's own percent, the chances it returns, a vague rate by the header's percent");
        assert_eq!(listed("honest_animation"), ["13"]);
        assert_eq!(listed("honest_pointer"), ["47", "48"], "through the type a macro casts to");
        assert_eq!(listed("honest_debug"), ["100", "777777"], "a debug room's amounts, not its hex flag");
        let magic = list(&[("tbs", &report)], "magic_code");
        assert!(magic.contains("motion.c:18\t70\n"), "not a chance's routine: {magic}");
        assert!(magic.contains("motion.c:26\t20\tOpt #3 (unnamed)\n"), "the callee and its parameter beside the number: {magic}");
        assert!(magic.contains("test_scenes.c:3\t777777\n"), "a debug room's table of scenes is no room: {magic}");
        assert!(magic.contains("test.c:4\t0x169\tGameFlag_Set #1 (unnamed)\n"), "{magic}");
    }

    #[test]
    fn hex_named_alone_is_a_mask_by_its_name_or_its_uses() {
        let report = game([
            ("a.h", "#define CELL_CODE_BITS 0xff00\n#define PLACE_ENEMY_SIDE 0x80\nenum { ICON_PSY_SEAL = 0x08 };\n#define LONE_VALUE 0x80\n#define WIPE_LAG_MAX 0x3f\n"),
            ("b.c", "s32 F(s32 place, s32 icons) { return (place & PLACE_ENEMY_SIDE) + (icons | ICON_PSY_SEAL) + LONE_VALUE; }\n"),
            ("c.c", "void G(void) { delay = Random16() & WIPE_LAG_MAX; while (front <= CANVAS_SIZE + WIPE_LAG_MAX) front++; }\n"),
        ]);
        assert_eq!((report.total("hex_mask"), report.total("hex_other")), (3, 2));
        assert_eq!(list(&[("tbs", &report)], "hex_other"), "tbs\ta.h:4\t0x80\ntbs\ta.h:5\t0x3f\n", "a value nothing masks with, and a distance reckoned with");
    }

    #[test]
    fn the_sound_drivers_numbers_are_to_name() {
        // The Broken Seal names them (SOUND_PAN_RIGHT, NR50_VOLUME_FULL), and
        // The Lost Age takes its names (H6): no kind keeps them.
        let driver = "void TrkVolPitSet(s32 pan) {\n    if (pan > 127)\n        pan = 127;\n    REG_NR50 = 0x77;\n}\n";
        let report = game([("games/tla/src/sound/driver.c", driver)]);
        assert_eq!(list(&[("tla", &report)], "magic_code"), "tla\tgames/tla/src/sound/driver.c:2\t127\ntla\tgames/tla/src/sound/driver.c:3\t127\ntla\tgames/tla/src/sound/driver.c:4\t0x77\n");
    }

    #[test]
    fn composed_numbers_are_data() {
        let composed = |body: &str, parameters: &[&str]| {
            let tokens = clex::lex(body);
            is_composition(parameters, tokens.iter().filter(|t| t.kind != Kind::Comment).map(|t| t.text))
        };
        assert!(composed("((value) << 16)", &["value"]));
        assert!(composed("(((b) << 10) | ((g) << 5) | (r))", &["r", "g", "b"]));
        assert!(composed("((DMA_ENABLE | DMA_32BIT) << 16 | (n)) ///< n words", &["n"]));
        assert!(composed("((value) * (1 << (bits)))", &["value", "bits"]));
        assert!(composed("((s32)((value) * FIXED_ONE))", &["value"]), "toFIXED");
        assert!(composed("((degrees) * TRIG_TURN / 360)", &["degrees"]), "DEGtoANG");
        assert!(!composed("(0x180 + (group))", &["group"]), "an offset into a range of ids");
        assert!(!composed("(BG_PLTT[(palette) * 16 + (color)])", &["palette", "color"]));
        assert!(!composed("((u8 *)VRAM + (n) * BG_SCREEN_SIZE)", &["n"]));
        assert!(!composed("(FLAG_BIT(n) << 1)", &["n"]), "a call");
        assert!(!composed("(16)", &["n"]), "no parameter");
        let header = concat!(
            "#define DMA_ENABLE 0x8000\n",
            "#define RGB(r, g, b) (((b) << 10) | ((g) << 5) | (r))\n",
            "#define DMA_COPY_WORDS(n) ((DMA_ENABLE | 0x400) << 16 | (n)) ///< n words\n",
            "#define FLAG_AREA_VISITED(group) (0x180 + (group))\n",
            "void Palette_Set(s32 index, u16 color);\n",
            "void Dma_Copy(const void *from, void *to, u32 control);\n",
            "void Flag_Set(s32 flag);\n",
        );
        let source = concat!(
            "void Run(void) {\n",
            "    Palette_Set(12, RGB(31, 16, 0));\n",
            "    Dma_Copy(a, b, DMA_COPY_WORDS(512));\n",
            "    Flag_Set(FLAG_AREA_VISITED(37));\n",
            "}\n",
        );
        let report = game([("f.h", header), ("f.c", source)]);
        assert_eq!(report.total("data_argument_composed"), 3, "31, 16 and 512; a 0 is no magic number");
        let listed = list(&[("tbs", &report)], "magic_argument");
        assert_eq!(listed, "tbs\tf.c:2\t12\tPalette_Set index\ntbs\tf.c:4\t37\tFLAG_AREA_VISITED group\n", "{listed}");
    }

    #[test]
    fn entry_points_need_their_summary() {
        let far = "/// Draws a number.\nvoid Text_DrawNumberFar(s32 number);\n\nvoid UiText_ClearFar(void);\n";
        let source = "void UiText_ClearFar(void) { Clear(); }\nvoid Helper(void) { Clear(); }\n";
        let report = game([("games/tbs/include/gamepak/far_call/window.h", far), ("games/tbs/src/window/text.c", source)]);
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
            "void F(void) { gB = 50; }\n",
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
            "msgctxt \"MsgDebugAreaName\"\nmsgid \"00002\"\nmsgstr \"Bilibin\"\n\n",
            "msgctxt \"message\"\nmsgid \"00003\"\nmsgstr \"Bilibin Barricade\"\n",
        );
        let text = GameText::parse(po, area_count("\tarea_name 0, -1, 0\n\tscene_name Bilibin, -1, 1\n")).unwrap();
        // The Lost Age's rules: a macro's definition is no row, a group's is.
        let rules = "\t.macro scene_name scene:req, entrance:req, name:req\n\t.endm\n\tgroup_name 0, -1, 0\n\tscene_name 11, 7, 7\n\tgroup_name 7, -1, 8\n";
        assert_eq!(area_count(rules), Some(9));
        let ids = "/// @file\n/// Scenes.\n\n/// ★ Named from its area name: Bilibin\nextern char SceneId_Bilibin;\nextern char SceneId_BilibinWarehouse;\n";
        let flags = "/// ★ Named from the game: \"Hidden Treasury\" (MsgSign)\n#define FLAG_TREASURY_OPEN 0x10\n#define FLAG_SCENE_TEMP_01 0x200\n#define FLAG_WAREHOUSE_SEEN 0x11 ///< Named from the game: \"Hidden Treasury\" (MsgSign)\n";
        let scene = concat!(
            "/// @file\n/// Bilibin and the Bilibin Barricade.\n///\n/// ACTOR_GUARD stands by `Bilibin Barricade`.\n",
            "#include \"x.h\"\n",
            "/// Its actors.\n",
            "enum {\n",
            "  ACTOR_GUARD, ///< the guard\n",
            "  ACTOR_PILLAR_APPROACH = 3, ///< a stack of crates;\n",
            "                          ///< Named from what the player sees: crates (its render)\n",
            "  /// A man.\n  ACTOR_MAN,\n",
            "};\n",
            "/// Runs the treasury.\n",
            "void Bilibin_UpdateWarehouse(void) { ACTOR_MAN; }\n",
            "static const u8 gBilibinTreasury[] = { 1 };\n",
        );
        let files = [
            ("games/tbs/include/scene_ids.h", ids),
            ("games/tbs/include/flags.h", flags),
            ("games/tbs/src/scene/bilibin.c", scene),
        ];
        let report = game_with_text(files, Some(&text), &[]);
        let names: Vec<&str> = report.things.iter().map(|thing| thing.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "SceneId_Bilibin", "SceneId_BilibinWarehouse", "FLAG_TREASURY_OPEN", "FLAG_WAREHOUSE_SEEN", "ACTOR_GUARD",
                "ACTOR_PILLAR_APPROACH", "ACTOR_MAN", "Bilibin_UpdateWarehouse", "gBilibinTreasury",
            ]
        );
        assert_eq!(report.value("provenance_names"), 7);
        assert_eq!(report.value("provenance_verified"), 2, "SceneId_Bilibin and FLAG_TREASURY_OPEN");
        assert_eq!(report.value("provenance_unstarred"), 2, "FLAG_WAREHOUSE_SEEN and ACTOR_PILLAR_APPROACH");
        assert_eq!(report.value("provenance_prose"), 1, "ACTOR_GUARD, from its place paragraph");
        assert_eq!(report.value("provenance_none"), 2, "SceneId_BilibinWarehouse and ACTOR_MAN");
        assert_eq!(report.value("unused_words"), 7, "warehouse, open, seen, guard, pillar, approach and man");
        let words = list(&[("tbs", &report)], "unused_word");
        assert!(words.starts_with("tbs\twarehouse\t3\tSceneId_BilibinWarehouse games/tbs/include/scene_ids.h:6; "), "{words}");
        let worklist = list(&[("tbs", &report)], "provenance");
        assert!(worklist.contains("tbs\tACTOR_PILLAR_APPROACH\tactor\tgames/tbs/src/scene/bilibin.c:9\tpillar,approach\tunstarred render\n"), "{worklist}");
        assert!(worklist.contains("tbs\tBilibin_UpdateWarehouse\tfunction\tgames/tbs/src/scene/bilibin.c:15\twarehouse\t-\n"));
        assert!(!worklist.contains("gBilibinTreasury"), "a mechanism whose words the game uses is not work");
        assert_eq!(list(&[("tbs", &report)], "provenance_none").lines().count(), 2);
        let tsv = tsv(&[("tbs", &report)]);
        assert!(tsv.contains("tbs\tunused_word\twarehouse\t3\n"));
        assert!(game(files).provenance.is_none(), "no text, no check");
    }

    /// H8: the same names with their ★ lines gone and their evidence in rows
    /// of the game's names table, matched by name and declaring file, a map
    /// id's by its name alone.
    #[test]
    fn names_take_their_evidence_rows() {
        let po = concat!(
            "msgctxt \"MsgSign\"\nmsgid \"00001\"\nmsgstr \"McCoy's Hidden Treasury\"\n\n",
            "msgctxt \"MsgDebugAreaName\"\nmsgid \"00002\"\nmsgstr \"Bilibin\"\n\n",
            "msgctxt \"message\"\nmsgid \"00003\"\nmsgstr \"Bilibin Barricade\"\n",
        );
        let text = GameText::parse(po, Some(2)).unwrap();
        let ids = "extern char SceneId_Bilibin;\nextern char SceneId_BilibinWarehouse;\n";
        let flags = "#define FLAG_TREASURY_OPEN 0x10\n#define FLAG_WAREHOUSE_SEEN 0x11\n#define FLAG_GATE_OPEN 0x12\n";
        let scene = "enum {\n  ACTOR_GUARD,\n  ACTOR_PILLAR,\n  ACTOR_MAN,\n  ACTOR_DOOR,\n  ACTOR_ODD,\n  ACTOR_SHADOW,\n  ACTOR_MARK,\n};\n";
        let files = [
            ("games/tbs/include/scene_ids.h", ids),
            ("games/tbs/include/flags.h", flags),
            ("games/tbs/src/scene/bilibin.c", scene),
        ];
        let names = evidence::parse_names(concat!(
            "name\tkind\tevidence\tfile\n",
            "SceneId_Bilibin\tits area name\tBilibin\tgames/tbs/include/other_ids.h\n",
            "FLAG_TREASURY_OPEN\tthe game\t\"Hidden Treasury\" (MsgSign)\tgames/tbs/include/flags.h\n",
            "FLAG_WAREHOUSE_SEEN\tthe game\t\"Hidden Warehouse\" (MsgSign)\tgames/tbs/include/flags.h\n",
            "FLAG_GATE_OPEN\tthe game\t\"Hidden Treasury\" (MsgSign)\tgames/tbs/include/gates.h\n",
            "ACTOR_GUARD\twhat the player sees\ta guard at the barricade (its render)\tgames/tbs/src/scene/bilibin.c\n",
            "ACTOR_PILLAR\tthe game\t\"no such words\" (MsgSign)\tgames/tbs/src/scene/bilibin.c\n",
            "ACTOR_PILLAR\twhat the player sees\ta stack of crates (its render)\tgames/tbs/src/scene/bilibin.c\n",
            "ACTOR_MAN\twhat the player sees\ta man in green\tgames/tbs/src/scene/bilibin.c\n",
            "ACTOR_DOOR\tits twin\tThe Lost Age's ACTOR_DOOR, the same routine\tgames/tbs/src/scene/bilibin.c\n",
            "ACTOR_ODD\tits vibe\ta feeling\tgames/tbs/src/scene/bilibin.c\n",
            "ACTOR_SHADOW\tits code\tplaced under the coin; it changes nothing on screen\tgames/tbs/src/scene/bilibin.c\n",
            "ACTOR_MARK\tits code\tplaced where the guard stands, drawn zero wide\tgames/tbs/src/scene/bilibin.c\n",
        ))
        .unwrap();
        let report = game_with_text(files, Some(&text), &names);
        let worklist = list(&[("tbs", &report)], "provenance");
        let evidence = |name: &str| worklist.lines().find(|line| line.split('\t').nth(1) == Some(name)).map(|line| line.rsplit('\t').next().unwrap().to_string());
        assert_eq!(evidence("SceneId_Bilibin").as_deref(), Some("verified area"), "a map id, by its name wherever declared");
        assert_eq!(evidence("FLAG_TREASURY_OPEN").as_deref(), Some("verified quote"));
        assert!(evidence("FLAG_WAREHOUSE_SEEN").unwrap().starts_with("failed: MsgSign does not hold"), "a row the text refutes");
        assert_eq!(evidence("FLAG_GATE_OPEN").as_deref(), Some("none"), "a row for a name declared in another file");
        assert_eq!(evidence("ACTOR_GUARD").as_deref(), Some("verified render"));
        assert_eq!(evidence("ACTOR_PILLAR").as_deref(), Some("verified render"), "one verified row of several");
        assert_eq!(evidence("ACTOR_MAN").as_deref(), Some("failed: a Named from line out of form"), "a render row names its render");
        assert_eq!(evidence("SceneId_BilibinWarehouse").as_deref(), Some("none"));
        assert_eq!(evidence("ACTOR_DOOR").as_deref(), Some("unchecked its twin"), "a row no check reads");
        assert_eq!(evidence("ACTOR_ODD").as_deref(), Some("failed: a row of no known kind: its vibe"));
        assert_eq!((report.value("provenance_verified"), report.value("provenance_failed"), report.value("provenance_none")), (4, 3, 2));
        assert_eq!(evidence("ACTOR_SHADOW").as_deref(), Some("unchecked its code: nothing on screen"), "a thing the player never sees");
        assert_eq!(evidence("ACTOR_MARK").as_deref(), Some("unchecked its code"));
        assert_eq!((report.value("provenance_unchecked"), report.value("provenance_unseen")), (2, 1));
        // GUARD and PILLAR are words for what a render showed; MAN's row cites
        // none, and the rest are evidence of no look.
        let seen = list(&[("tbs", &report)], "seen_word");
        assert_eq!(seen, "tbs\tguard\t1\tACTOR_GUARD games/tbs/src/scene/bilibin.c:2\ntbs\tpillar\t1\tACTOR_PILLAR games/tbs/src/scene/bilibin.c:3\n");
        assert_eq!(report.value("seen_words"), 2);
        let flagged = list(&[("tbs", &report)], "unused_word");
        assert!(flagged.contains("\tman\t") && flagged.contains("\twarehouse\t") && !flagged.contains("\tguard\t"), "{flagged}");
    }


    #[test]
    fn words_no_render_can_name() {
        let po = "msgctxt \"MsgDebugAreaName\"\nmsgid \"00001\"\nmsgstr \"Hall\"\n\nmsgctxt \"message\"\nmsgid \"00002\"\nmsgstr \"Hello there.\"\n";
        let text = GameText::parse(po, Some(1)).unwrap();
        let hall = concat!(
            "enum HallActorId {\n  ACTOR_ICE_CONE_A = ACTOR_FIRST_PLACED,\n  ACTOR_ICE_CONE_B,\n  ACTOR_SPARKLE_A,\n  ACTOR_SPARKLE_B,\n};\n\n",
            "struct ActorPlacement gHall_Placements[] = {\n",
            "    ACTOR_PLACEMENT(SPRITE_STAKE, CONDITION_ALWAYS, 0, 0, 0, 0, FACING_SOUTH, 0, 0),\n",
            "#if TLA_PATCH_1\n    ACTOR_PLACEMENT(SPRITE_STAKE, CONDITION_ALWAYS, 0, 136, 0, 424, FACING_SOUTH, 0, 0),\n",
            "#else\n    ACTOR_PLACEMENT(SPRITE_STAKE, CONDITION_ALWAYS, 0, 0, 0, 0, FACING_SOUTH, 0, 0),\n#endif\n",
            "    ACTOR_PLACEMENT(SPRITE_SPARK, CONDITION_ALWAYS, 0, 0, 0, 0, FACING_SOUTH, 0, 0),\n",
            "    ACTOR_PLACEMENT(SPRITE_SPARK, CONDITION_ALWAYS, 0, 8, 0, 8, FACING_SOUTH, 0, 0),\n",
            "    {SCENE_TABLE_END},\n};\n\n",
            "/// Slides on from the ice's edge.\nvoid Hall_SlideFromEdge(void) {\n  Leader_Go();\n}\n\n",
            "/// Lights the sparkle.\nvoid Hall_Light(void) {\n  Actor_Show(ACTOR_SPARKLE_A);\n}\n\n",
            "/// Calls it.\nvoid Hall_Run(void) {\n  Hall_SlideFromEdge();\n}\n",
        );
        let room = concat!(
            "/// Unused: slides on from the edge.\nUNUSED void Room_SlideFromEdge(void) {\n  Leader_Go();\n  Room_Other();\n}\n\n",
            "/// Never called here.\nvoid Hall_Light(void) {\n  Actor_Show(3);\n}\n\n",
            "/// Keeps the drip.\nvoid Room_KeepDrip(void) {\n  Room_Other();\n}\n",
        );
        assert_eq!(placed_at_origin(hall), ["ACTOR_ICE_CONE_A", "ACTOR_SPARKLE_A"], "an #if's first branch read; a row away from the origin");
        let files = [("games/tla/src/scene/hall.c", hall), ("games/tla/src/scene/room.c", room)];
        let names = evidence::parse_names(concat!(
            "name\tkind\tevidence\tfile\n",
            "ACTOR_ICE_CONE_B\twhat the player sees\tan upright cone of ice (its render)\tgames/tla/src/scene/hall.c\n",
            "ACTOR_SPARKLE_B\twhat the player sees\ta sparkle running along the floor (its render)\tgames/tla/src/scene/hall.c\n",
            "Hall_SlideFromEdge\twhat the player sees\tFelix slides across the ice from its edge (its render)\tgames/tla/src/scene/hall.c\n",
            "Hall_Light\twhat the player sees\tthe sparkle lights up (its render)\tgames/tla/src/scene/hall.c\n",
            "Room_KeepDrip\tits code\tnothing on screen: keeps a counter no routine reads\tgames/tla/src/scene/room.c\n",
        ))
        .unwrap();
        let report = game_with_text(files, Some(&text), &names);
        let words = |metric: &str| -> Vec<String> { list(&[("tla", &report)], metric).lines().map(|line| line.split('\t').nth(1).unwrap().to_string()).collect() };
        assert_eq!(
            words("lent_word"),
            ["cone", "edge", "from", "ice", "light", "slide"],
            "cone A off camera beside B; Room's unused copy of Hall's routine; Hall_Light, which nothing in room.c calls"
        );
        assert_eq!(words("unseen_word"), ["drip", "keep"]);
        assert_eq!(words("unused_word"), ["room", "sparkle"], "sparkle A is moved by a routine, so B lends it nothing; Room is no map's name");
        assert_eq!((report.value("lent_words"), report.value("unseen_words")), (6, 2));
        assert!(report.never_run.contains(&("games/tla/src/scene/room.c".to_string(), "Hall_Light".to_string())), "written only where it is defined");
        assert!(!report.never_run.contains(&("games/tla/src/scene/hall.c".to_string(), "Hall_SlideFromEdge".to_string())));
    }

    #[test]
    fn numbered_names_whose_number_is_their_meaning() {
        let po = concat!(
            "msgctxt \"MsgDetailsUnknown\"\nmsgid \"00001\"\nmsgstr \"Details are unknown\"\n\n",
            "msgctxt \"MsgUnknownThing\"\nmsgid \"00002\"\nmsgstr \"A thing\"\n",
        );
        let text = GameText::parse(po, None).unwrap();
        let source = concat!(
            "enum { SCENE_TEST_0, SCENE_TEST_1, ACTOR_8 };\n",
            "void Run(void) { Show(MsgDetailsUnknown); Show(MsgUnknownThing); Go(SCENE_TEST_1); }\n",
        );
        let names = evidence::parse_names(concat!(
            "name\tkind\tevidence\tfile\n",
            "SCENE_TEST_0\tits number\tthe game's text: Test 0 to Test 7\tgames/tbs/include/scene_ids.h\n",
            "SCENE_TEST_1\tits number\tthe game's text: Test 0 to Test 7\tgames/tbs/include/scene_ids.h\n",
        ))
        .unwrap();
        let report = game_with_text([("games/tbs/include/scene_ids.h", source)], Some(&text), &names);
        assert_eq!((report.value("numbered_names"), report.value("numbered")), (1, 1), "ACTOR_8 alone");
        assert_eq!((report.value("numbered_honest"), report.total("numbered_honest")), (2, 3), "the tests, by their rows");
        assert!(listable("numbered_honest"));
        // A message named from its own text's "unknown" is the game's word;
        // one whose text never says it is still a placeholder.
        assert_eq!(report.names.get("placeholder").map(|names| names.iter().cloned().collect::<Vec<_>>()), Some(vec!["MsgUnknownThing".to_string()]));
        assert!(is_placeholder("MsgDetailsUnknown"), "without the text, it looks like one");
    }

    #[test]
    fn every_evidence_row_is_of_a_known_kind() {
        for game in ["tbs", "tla"] {
            for row in evidence::names(game).unwrap() {
                let known = provenance::CHECKED_KINDS.contains(&row.kind.as_str()) || provenance::UNCHECKED_KINDS.contains(&row.kind.as_str());
                assert!(known, "{game}: {} has a row of kind {}", row.name, row.kind);
            }
        }
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
            "enum ActorCollisionFlag { ACTOR_SOLID = 1 };\n",
            "enum BilibinActorId { ACTOR_GUARD = 8, ACTOR_SHOP_END };\n",
            "enum { ACTOR_MAYOR = 10, ACTOR_ID_COUNT = 66 };\n",
            "#define ITEM_TYPE_WEAPON 1\n#define ITEM_LIST_NAME_X 12\n",
        );
        let files = [("games/tbs/include/flags.h", flags), ("games/tbs/include/field/actor.h", actors)];
        let report = game(files);
        let names: Vec<&str> = report.things.iter().map(|thing| thing.name.as_str()).collect();
        assert_eq!(names, ["FLAG_KRADEN_SETS_OFF", "ACTOR_GUARD", "ACTOR_SHOP_END", "ACTOR_MAYOR"]);
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
        let text = GameText::parse(po, None).unwrap();
        let header = concat!(
            "/// The first class id.\n#define ENEMY_ID_FIRST 8\n",
            "/// Classes.\nenum EnemyId {\n",
            "  ENEMY_VERMIN = ENEMY_ID_FIRST + 1,\n  ENEMY_VERMIN_2,\n  ENEMY_RAT = 0xb, ///< a rat\n};\n",
            "enum { ENEMY_NOT_IN_A_TABLE = 9 };\n",
        );
        let report = game_with_text([("games/tbs/include/enemy_ids.h", header)], Some(&text), &[]);
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
        let po = "msgctxt \"MsgDebugAreaName\"\nmsgid \"00001\"\nmsgstr \"Tolbi\"\n";
        let text = GameText::parse(po, Some(1)).unwrap();
        let ids = "/// ★ Named from its area name: Tolbi\nextern char SceneId_Tolbi;\n";
        let links = "enum {\n  TOLBI_FROM_WORLD_MAP_NORTH = 1,\n  TOLBI_FROM_HOUSE = 2,\n  TOLBI_TO_HOUSE = 3,\n};\n";
        let files = [("games/tbs/include/scene_ids.h", ids), ("games/tbs/include/scene_exits.h", links)];
        let report = game_with_text(files, Some(&text), &[]);
        assert_eq!(report.value("provenance_rooms"), 0, "WORLD_MAP is no map here");
        assert_eq!((report.value("door_words"), report.value("door_word_link")), (3, 3), "world, map and house; north is a door word");
        assert_eq!(list(&[("tbs", &report)], "door_word").lines().next(), Some("tbs\thouse\t2\tTOLBI_FROM_HOUSE games/tbs/include/scene_exits.h:3; TOLBI_TO_HOUSE games/tbs/include/scene_exits.h:4"));
        let worklist = list(&[("tbs", &report)], "provenance");
        assert!(worklist.contains("\tTOLBI_TO_HOUSE\texit\tgames/tbs/include/scene_exits.h:4\tto,house\tnone; door: house\n"), "{worklist}");
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
