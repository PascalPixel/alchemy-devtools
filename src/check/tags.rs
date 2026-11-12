//! The fake-match tags against S2: each comment that opens with FAKEMATCH
//! must read `FAKEMATCH(<kind>): the form kept; as <plain form>, N of M
//! instructions differ in <edition>.`, or one of the spellings `judge` takes,
//! inside the function (or macro) whose device it tags; and a function or macro with more asm statements than
//! tags has a device no tag names. A LEFTOVER tag, Camelot's own idle code,
//! is held to the same form with its own kinds and must state the evidence
//! its kind rests on; it never covers an asm statement, since an asm is
//! always a device. A `// DEAD:` mark, Camelot's own code the ROM keeps,
//! must say what the ROM keeps (S3).

use crate::alchemy::humanity::{DEVICES, LEFTOVERS};
use crate::alchemy::targets::parse_decomp_target;
use crate::gba::clex::{lex, Kind, Token};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The kind S2 once had for a once-only block: one no macro expands to is
/// now a `dead` device, and a macro's is ordinary C with no tag.
const RETIRED: &str = "one-pass";
/// The leftover kind S2 had for a device The Lost Age's ROM also needs
/// (removed 12 November 2026): one compiler meeting one reconstruction
/// twice is no evidence that Camelot wrote it.
const RETIRED_LEFTOVER: &str = "form";
/// The kinds The Broken Seal refuses (S2).
const NOT_IN_TBS: [&str; 2] = ["pin", "signature"];
/// A measurement inside a tag ("4 of 549 instructions differing in tla-en"),
/// taken out of a leftover's evidence before it is read: a measured edition
/// is no evidence that the edition uses the thing.
const MEASUREMENT: &str = r"(?i)\d+ of \d+(?: instructions)?(?: [a-z]+)*? differ(?:ing)?(?: in| there)?(?: t(?:bs|la)-[a-z]{2})?";
/// Forms S2 keeps fake matches whatever the evidence: none is ever a leftover.
const FORBIDDEN: &str = r"(?i)\b(self-assignments?|cancel(?:s|ling|led)?|both arms|barriers?|asm|empty (?:block|loop|statement)s?|once-only|runs once)\b";
/// A `frame` leftover's evidence, the frame itself: the bytes it reserves
/// (`sub sp, #N`, or a byte count)...
const RESERVED: &str = r"(?i)\b(?:sub|add) sp\b|\b\d+ bytes\b";
/// ...and that no instruction touches them.
const UNTOUCHED: &str = r"(?i)\buntouched\b|\bnever touch(?:es|ed)?\b|\bno instruction (?:addresses|reads|touches)\b";
/// A routine named as evidence: ours (`Module_Name`) or a placeholder.
const ROUTINE: &str = r"\b(?:[A-Z][A-Za-z0-9]*_[A-Z][A-Za-z0-9]*|(?:Func|sub)_[0-9a-f]{8})\b";
/// A reconstructed family's macro, named as evidence.
const FAMILY: &str = r"(?i:\b(?:macro|helper)\b).*\b[A-Z][A-Z0-9]*_[A-Z0-9_]+\b|\b[A-Z][A-Z0-9]*_[A-Z0-9_]+\b.*(?i:\b(?:macro|helper)\b)";
/// A claim about another edition, by word...
const EDITION_CLAIM: &str = r"(?i)\b(?:editions?|japanese|english|german|spanish|french|italian)\b";
/// ...which must name the edition.
const EDITION: &str = r"\bt(?:bs|la)-(?:ja|en|de|es|fr|it)\b";
/// A claim about The Lost Age, which must name its routine.
const LOST_AGE: &str = r"(?i)\bthe lost age\b";

fn regex(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).unwrap())
}

/// What is missing from a leftover's evidence, its first part read with any
/// measurement taken out (S2, 12 November 2026). A `frame` leftover states
/// the bytes the frame reserves and that no instruction touches them; a
/// `template` names the sibling routine whose shape it shares, or the
/// family's macro; a claim about another edition names it, and one about The
/// Lost Age names its routine. A form S2 keeps a fake match is never one.
fn evidence(kind: &str, first: &str) -> Option<String> {
    static CELLS: [OnceLock<Regex>; 9] = [const { OnceLock::new() }; 9];
    let text = regex(&CELLS[0], MEASUREMENT).replace_all(first, "");
    if let Some(form) = regex(&CELLS[1], FORBIDDEN).find(&text) {
        return Some(format!(
            "a leftover never names {}: S2 keeps that form a fake match, whatever the evidence",
            form.as_str()
        ));
    }
    let routine = regex(&CELLS[2], ROUTINE).is_match(&text);
    match kind {
        "frame" if !(regex(&CELLS[3], RESERVED).is_match(&text) && regex(&CELLS[4], UNTOUCHED).is_match(&text)) => {
            return Some(
                "a frame leftover states its evidence before its plain form (S2): the bytes the frame reserves \
                 (sub sp, #N, or a byte count) and that no instruction touches them; without it, it is a fake match"
                    .to_string(),
            );
        }
        "template" if !routine && !regex(&CELLS[5], FAMILY).is_match(&text) => {
            return Some(
                "a template leftover names before its plain form the sibling routine whose shape it shares, or its \
                 family's macro (S2); without it, it is a fake match"
                    .to_string(),
            );
        }
        _ => {}
    }
    if regex(&CELLS[6], EDITION_CLAIM).is_match(&text) && !regex(&CELLS[7], EDITION).is_match(&text) {
        return Some("a leftover's claim about another edition names that edition (tbs-ja and so on) outside its measurement (S2)".to_string());
    }
    if regex(&CELLS[8], LOST_AGE).is_match(&text) && !routine {
        return Some("a leftover's claim about The Lost Age names the routine there (S2)".to_string());
    }
    None
}

/// What is wrong with a `// DEAD:` mark's plain text, if anything: S3's form
/// is `// DEAD: <what>; the ROM keeps <its instructions>.`, since S2's
/// physical test makes code the original's only where its own instructions
/// are in the ROM.
fn judge_dead(text: &str) -> Option<String> {
    static KEEPS: OnceLock<Regex> = OnceLock::new();
    let keeps = regex(&KEEPS, r"^DEAD: \S.*\bthe ROM keeps \S[^.]*\.$");
    (!keeps.is_match(text)).then(|| "not S3's form: // DEAD: <what>; the ROM keeps <its instructions>.".to_string())
}

/// S2's form of a tag.
fn form_text(word: &str) -> String {
    match word {
        "LEFTOVER" => "LEFTOVER(<kind>): what remains and the evidence; as <plain form>, N of M instructions differ in <edition>.".to_string(),
        _ => format!("{word}(<kind>): the form kept; as <plain form>, N of M instructions differ in <edition>."),
    }
}

/// A tag's text with its comment marks and line breaks taken out.
fn plain(comments: &[&str]) -> String {
    let mut words = String::new();
    for comment in comments {
        let body = comment
            .strip_prefix("//")
            .or_else(|| comment.strip_prefix("/*").map(|body| body.strip_suffix("*/").unwrap_or(body)))
            .unwrap_or(comment);
        for line in body.lines() {
            // A macro's comment ends each line with the directive's `\`.
            let line = line.trim().trim_end_matches('\\').trim_end();
            let line = line.strip_prefix('*').filter(|rest| !rest.starts_with('/')).unwrap_or(line);
            for word in line.split_whitespace() {
                if !words.is_empty() {
                    words.push(' ');
                }
                words += word;
            }
        }
    }
    words
}

/// What is wrong with a tag's plain text, if anything. S2's parts are what
/// is judged: a kind, the form kept, the plain form, and N of M measured in
/// an edition. As The Broken Seal's finished tags write them, the plain form
/// may open a sentence of its own and be named by what it lacks ("Without
/// them", "with no arm"), the instructions may be a nested function's, and
/// a parenthesis or a clause after the edition may say more.
///
/// A LEFTOVER tag, Camelot's own code idle in an edition, is judged the same
/// way with its own kinds, and its first part must also state the evidence
/// its kind rests on (see `evidence`): a claim without it is a fake match.
fn judge(text: &str, broken_seal: bool) -> Option<String> {
    static KIND: OnceLock<Regex> = OnceLock::new();
    static FORM: OnceLock<Regex> = OnceLock::new();
    let kind = KIND.get_or_init(|| Regex::new(r"^(FAKEMATCH|LEFTOVER)(\(([^)]*)\))?:").unwrap());
    let form = FORM.get_or_init(|| {
        Regex::new(r"^[A-Z]+\([^)]*\): (.+)[;.] (?i:as|without|with) (.+?), (\d+) of (\d+) instructions(?: of [^,;]+?)? differ in ([a-z]+-[a-z]+)(?:[ ,][^.]*)?\.$").unwrap()
    });
    let Some(captures) = kind.captures(text) else {
        return Some(format!("not S2's form: {}", form_text("FAKEMATCH")));
    };
    let word = captures.get(1).map_or("FAKEMATCH", |word| word.as_str());
    let leftover = word == "LEFTOVER";
    let Some(kind) = captures.get(3).map(|kind| kind.as_str()) else {
        return Some(format!("unfinished, no kind: give it its kind or remove it with its form (S2): {}", form_text(word)));
    };
    if !leftover && kind == RETIRED {
        return Some(format!(
            "{RETIRED} is no kind under S2: a once-only block no macro expands to is FAKEMATCH(dead); a macro's takes no tag"
        ));
    }
    if leftover && kind == RETIRED_LEFTOVER {
        return Some(format!(
            "{RETIRED_LEFTOVER} is no kind under S2: a device The Lost Age's ROM also needs is one compiler meeting one \
             reconstruction twice, a fake match with its device's kind"
        ));
    }
    let kinds: &[&str] = if leftover { &LEFTOVERS } else { &DEVICES };
    if !kinds.contains(&kind) {
        return Some(format!("unknown kind {kind}: {}", kinds.join(", ")));
    }
    if !leftover && broken_seal && NOT_IN_TBS.contains(&kind) {
        return Some(format!("The Broken Seal takes no {kind} (S2)"));
    }
    let Some(captures) = form.captures(text) else {
        return Some(format!("not S2's form: {}", form_text(word)));
    };
    if leftover {
        if let Some(message) = evidence(kind, &captures[1]) {
            return Some(message);
        }
    }
    if captures[3].parse::<u64>().unwrap_or(0) == 0 {
        return Some("0 instructions differ: the plain form matches, so this is no device".to_string());
    }
    if parse_decomp_target(&captures[5]).is_err() {
        return Some(format!("{} is not an edition", &captures[5]));
    }
    None
}

/// A function body or a macro, with the tags and asm statements inside it.
struct Unit {
    name: String,
    line: usize,
    tags: usize,
    asm: usize,
    /// CAMELOT_ASM marks inside: each a proven handwritten asm statement (S1).
    proofs: usize,
    /// Marked CAMELOT_ASM before its definition: a proven helper whole.
    proven: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Finding {
    pub line: usize,
    pub message: String,
}

fn is_asm(token: &Token) -> bool {
    token.kind == Kind::Ident && matches!(token.text, "asm" | "__asm__" | "__asm")
}

/// The name a function body's `{` belongs to: the identifier before the
/// parameter list that the `)` at `close` ends.
fn function_name(tokens: &[&Token], close: usize) -> Option<String> {
    let mut depth = 0;
    for at in (0..=close).rev() {
        match tokens[at].text {
            ")" => depth += 1,
            "(" => {
                depth -= 1;
                if depth == 0 {
                    return at.checked_sub(1).map(|before| tokens[before]).filter(|t| t.kind == Kind::Ident).map(|t| t.text.to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// The tags and marks one file holds, each word counted apart (S2: never
/// summed). `dead` counts the `// DEAD:` marks; `dead_unformed` those of
/// them not yet in S3's form where they are counted rather than refused.
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub struct Tags {
    pub fakematch: usize,
    pub leftover: usize,
    pub dead: usize,
    pub dead_unformed: usize,
}

impl Tags {
    fn add(&mut self, other: Tags) {
        self.fakematch += other.fakematch;
        self.leftover += other.leftover;
        self.dead += other.dead;
        self.dead_unformed += other.dead_unformed;
    }
}

/// Every tag and `// DEAD:` mark in one C file judged, and each unit with
/// more asm than tags. The Broken Seal's marks are held to S3's form; The
/// Lost Age's are counted where they miss it, for that game to rewrite.
pub fn findings(path: &str, source: &str) -> (Tags, Vec<Finding>) {
    let broken_seal = path.contains("games/tbs/");
    let tokens = lex(source);
    let mut found = Vec::new();
    let mut tags = Tags::default();
    let mut units: Vec<Unit> = Vec::new();
    // The line span of each function body, with its unit. Each branch of a
    // conditional starts from the braces open at its #if, since each opens
    // its own; after #endif the first branch's stand.
    let mut bodies: Vec<(usize, usize, usize)> = Vec::new();
    let mut code: Vec<&Token> = Vec::new();
    let mut depth = 0usize;
    let mut open: Option<(usize, usize)> = None;
    // Where the declaration being read began: after the last `;` or `}` at file scope.
    let mut declaration = 0;
    type State = (usize, Option<(usize, usize)>);
    let mut branches: Vec<(State, Option<State>)> = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        if token.kind == Kind::PpStart {
            match tokens.get(at + 1).map_or("", |t| t.text) {
                "if" | "ifdef" | "ifndef" => branches.push(((depth, open), None)),
                "elif" | "else" => {
                    if let Some((start, first)) = branches.last_mut() {
                        first.get_or_insert((depth, open));
                        (depth, open) = *start;
                    }
                }
                "endif" => {
                    if let Some((_, Some(first))) = branches.pop() {
                        (depth, open) = first;
                    }
                }
                _ => {}
            }
            continue;
        }
        if token.pp || token.kind == Kind::Comment {
            continue;
        }
        if token.kind == Kind::Punct && token.text == "{" {
            if depth == 0 && code.last().is_some_and(|last| last.text == ")") {
                units.push(Unit {
                    name: function_name(&code, code.len() - 1).unwrap_or_else(|| "a function".to_string()),
                    line: token.line,
                    tags: 0,
                    asm: 0,
                    proofs: 0,
                    proven: code[declaration.min(code.len())..].iter().any(|t| t.text == "CAMELOT_ASM"),
                });
                open = Some((units.len() - 1, token.line));
            }
            depth += 1;
        } else if token.kind == Kind::Punct && token.text == "}" {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                if let Some((unit, line)) = open.take() {
                    bodies.push((line, token.line, unit));
                }
                declaration = code.len() + 1;
            }
        } else if depth == 0 && token.text == ";" {
            declaration = code.len() + 1;
        }
        code.push(token);
    }
    let unit_at = |line: usize| bodies.iter().find(|(start, end, _)| (*start..=*end).contains(&line)).map(|(_, _, unit)| *unit);
    // Directives: a #define is a unit of its own.
    let mut macro_unit: Option<usize> = None;
    // Inside an extern declaration, where asm only names the symbol.
    let mut external = false;
    // Each open conditional on MODERN: which branch is `make modern`'s (true
    // for #ifdef MODERN, false for #ifndef MODERN) and whether #else is being
    // read. There asm serves a current GCC, not a match.
    let mut conditionals: Vec<(Option<bool>, bool)> = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        let token = &tokens[at];
        if token.kind == Kind::PpStart {
            macro_unit = None;
            let directive: Vec<&str> = tokens[at + 1..].iter().take_while(|t| t.kind != Kind::PpEnd).map(|t| t.text).collect();
            let tests_modern = directive.contains(&"MODERN");
            match directive.first().copied().unwrap_or("") {
                "ifdef" | "if" => conditionals.push((tests_modern.then_some(!directive.contains(&"!")), false)),
                "ifndef" => conditionals.push((tests_modern.then_some(false), false)),
                "else" => {
                    if let Some((_, otherwise)) = conditionals.last_mut() {
                        *otherwise = true;
                    }
                }
                "elif" => {
                    if let Some((modern, _)) = conditionals.last_mut() {
                        *modern = None;
                    }
                }
                "endif" => {
                    conditionals.pop();
                }
                _ => {}
            }
            if tokens.get(at + 1).is_some_and(|t| t.text == "define") {
                let name = tokens.get(at + 2).map_or("a macro", |t| t.text).to_string();
                units.push(Unit { name, line: token.line, tags: 0, asm: 0, proofs: 0, proven: false });
                macro_unit = Some(units.len() - 1);
            }
        } else if token.kind == Kind::PpEnd {
            macro_unit = None;
        }
        let unit = if token.pp { macro_unit } else { unit_at(token.line) };
        let modern = conditionals.iter().any(|(modern, otherwise)| *modern == Some(!otherwise));
        match token.text {
            ";" | "{" | "}" if token.kind == Kind::Punct => external = false,
            "extern" if token.kind == Kind::Ident => external = true,
            "CAMELOT_ASM" if token.kind == Kind::Ident => {
                if let Some(unit) = unit {
                    units[unit].proofs += 1;
                }
            }
            _ => {}
        }
        let opening = plain(&[token.text]);
        let leftover = opening.starts_with("LEFTOVER");
        if token.kind == Kind::Comment && (leftover || opening.starts_with("FAKEMATCH")) {
            // A tag goes on through the comments right after it until its
            // sentence ends: a macro's tag is one comment a line.
            let mut comments = vec![token.text];
            while !plain(&comments).ends_with('.') && tokens.get(at + 1).is_some_and(|t| t.kind == Kind::Comment) {
                at += 1;
                comments.push(tokens[at].text);
            }
            if leftover {
                tags.leftover += 1;
            } else {
                tags.fakematch += 1;
            }
            if token.text.starts_with("///") || token.text.starts_with("/**") {
                found.push(Finding { line: token.line, message: "a tag is a // or /* */ comment, not documentation".to_string() });
            }
            if let Some(message) = judge(&plain(&comments), broken_seal) {
                found.push(Finding { line: token.line, message });
            }
            match unit {
                // A leftover is never what an asm statement needs.
                Some(_) if leftover => {}
                Some(unit) => units[unit].tags += 1,
                None => found.push(Finding { line: token.line, message: "a tag outside any function".to_string() }),
            }
        } else if token.kind == Kind::Comment && token.text.starts_with("// DEAD:") {
            // A mark goes on through the `//` comments right after it until
            // its sentence ends.
            let mut comments = vec![token.text];
            while !plain(&comments).ends_with('.')
                && tokens.get(at + 1).is_some_and(|t| t.kind == Kind::Comment && t.text.starts_with("//") && t.line == tokens[at].line + 1)
            {
                at += 1;
                comments.push(tokens[at].text);
            }
            tags.dead += 1;
            let mut problems: Vec<String> = judge_dead(&plain(&comments)).into_iter().collect();
            if unit.is_none() {
                problems.push("a // DEAD: mark outside any function".to_string());
            }
            if !problems.is_empty() {
                if broken_seal {
                    found.extend(problems.into_iter().map(|message| Finding { line: token.line, message }));
                } else {
                    tags.dead_unformed += 1;
                }
            }
        } else if is_asm(token) && !external && !modern {
            match unit {
                Some(unit) => units[unit].asm += 1,
                None => found.push(Finding { line: token.line, message: "asm outside any function".to_string() }),
            }
        }
        at += 1;
    }
    for unit in units.iter().filter(|unit| !unit.proven && unit.asm > unit.tags + unit.proofs) {
        let proofs = match unit.proofs {
            0 => String::new(),
            n => format!(" and {n} CAMELOT_ASM"),
        };
        found.push(Finding {
            line: unit.line,
            message: format!("{}: {} asm statements, {} tags{proofs}", unit.name, unit.asm, unit.tags),
        });
    }
    found.sort_by_key(|finding| finding.line);
    (tags, found)
}

/// The views a C file keeps (`adopt merge --keep-views`): each marker's kind,
/// and a finding for a marker not in its form. A view is a disagreement no
/// one has tested yet, not a device, so it is counted apart from the tags.
pub fn views(source: &str) -> (Vec<&'static str>, Vec<Finding>) {
    let mut kinds = Vec::new();
    let mut found = Vec::new();
    for token in lex(source).iter().filter(|token| token.kind == Kind::Comment) {
        match crate::adopt::views::view_kind(token.text) {
            Some(Ok(kind)) => kinds.push(kind),
            Some(Err(message)) => found.push(Finding { line: token.line, message }),
            None => {}
        }
    }
    (kinds, found)
}

/// The C sources and headers under each path, sorted.
fn sources(root: &Path, paths: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for path in paths {
        let full = root.join(path);
        if !full.exists() {
            return Err(format!("{path}: no such file or folder"));
        }
        for entry in walkdir::WalkDir::new(&full).sort_by_file_name() {
            let entry = entry.map_err(|error| error.to_string())?;
            if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == "c" || e == "h") {
                files.push(entry.into_path());
            }
        }
    }
    files.dedup();
    Ok(files)
}

/// One game's count: its tags and marks, views, files and findings.
#[derive(Debug, Default)]
struct Game {
    tags: Tags,
    views: BTreeMap<&'static str, usize>,
    files: usize,
    findings: usize,
}

impl Game {
    /// The game's lines: fake matches, leftovers, `// DEAD:` marks and
    /// unsettled views each on its own line, never summed (S2).
    fn lines(&self, name: &str) -> Vec<String> {
        let unformed = match self.tags.dead_unformed {
            0 => String::new(),
            n => format!(" ({n} do not yet say what the ROM keeps, S3; counted, not refused)"),
        };
        let views: usize = self.views.values().sum();
        let kinds: Vec<String> = self.views.iter().map(|(kind, n)| format!("{n} {kind}")).collect();
        let kinds = if kinds.is_empty() { String::new() } else { format!(" ({})", kinds.join(", ")) };
        vec![
            format!("{name}: {} files; {} findings", self.files, self.findings),
            format!("{name}: {} FAKEMATCH tags", self.tags.fakematch),
            format!("{name}: {} LEFTOVER tags", self.tags.leftover),
            format!("{name}: {} // DEAD: marks{unformed}", self.tags.dead),
            format!("{name}: {views} VIEW markers{kinds}"),
        ]
    }
}

/// The game a source path belongs to (`games/tbs`), or its first folder.
fn game_of(path: &str) -> String {
    let mut parts = path.split('/');
    match (parts.next(), parts.next()) {
        (Some("games"), Some(game)) if parts.next().is_some() => format!("games/{game}"),
        (Some(first), _) => first.to_string(),
        _ => path.to_string(),
    }
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    if let Some(flag) = arguments.iter().find(|argument| argument.starts_with('-')) {
        return Err(format!("unknown option {flag}"));
    }
    let root = crate::compile::root();
    let paths = if arguments.is_empty() { vec!["games".to_string()] } else { arguments.to_vec() };
    let mut games: BTreeMap<String, Game> = BTreeMap::new();
    for file in sources(root, &paths)? {
        let source = String::from_utf8_lossy(&std::fs::read(&file).map_err(|error| format!("{}: {error}", file.display()))?).into_owned();
        let name = file.strip_prefix(root).unwrap_or(&file).display().to_string();
        let (found_tags, mut found) = findings(&name, &source);
        let (kinds, malformed) = self::views(&source);
        found.extend(malformed);
        let game = games.entry(game_of(&name)).or_default();
        game.tags.add(found_tags);
        for kind in kinds {
            *game.views.entry(kind).or_default() += 1;
        }
        game.files += 1;
        game.findings += found.len();
        for finding in found {
            println!("{name}:{}: {}", finding.line, finding.message);
        }
    }
    for (name, game) in &games {
        for line in game.lines(name) {
            println!("{line}");
        }
    }
    let count: usize = games.values().map(|game| game.findings).sum();
    match count {
        0 => Ok(()),
        n => Err(format!("{n} tags or asm statements break S2")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_are_counted_apart_and_their_markers_judged() {
        let source = "/* VIEW(s0b84): routine Helper as int Helper(struct Thing *t); the definition has void Helper(int a, int b). */\nextern int Helper__s0b84(struct Thing *t) __asm__(\"Helper\");\n/* VIEW(s0b84): tag struct Thing as struct Thing__s0b84; s0674 has another definition. */\n// VIEW(s0b84): widget Foo as Foo__s0b84; x has y.\n/* VIEW(): routine F as G; x has y. */\n/* A VIEW( in prose is no marker. */\n";
        let (kinds, found) = views(source);
        assert_eq!(kinds, ["routine", "tag"]);
        assert_eq!(found.iter().map(|f| f.line).collect::<Vec<_>>(), [4, 5]);
        assert!(found[0].message.starts_with("unknown view kind widget"), "{}", found[0].message);
        // A view's asm label names a symbol in an extern declaration: no device.
        assert!(findings("games/tla/src/x.c", source).1.is_empty());
    }

    #[test]
    fn a_tag_is_read_across_its_lines() {
        assert_eq!(
            plain(&["/* FAKEMATCH(dead): an unused array; as\n   * the function without it, 4 of 91 instructions differ in tla-en. */"]),
            "FAKEMATCH(dead): an unused array; as the function without it, 4 of 91 instructions differ in tla-en."
        );
        assert_eq!(plain(&["// FAKEMATCH(pin): x"]), "FAKEMATCH(pin): x");
    }

    #[test]
    fn tags_are_judged_by_s2() {
        let ok = "FAKEMATCH(barrier): dmg held; as no asm, 2 of 3451 instructions differ in tla-en.";
        assert_eq!(judge(ok, false), None);
        assert_eq!(judge(&ok.replace("barrier", "temporary"), true), None);
        assert!(judge(&ok.replace("barrier", "pin"), true).unwrap().contains("Broken Seal"));
        assert!(judge(&ok.replace("barrier", "signature"), true).unwrap().contains("Broken Seal"));
        assert!(judge(&ok.replace("barrier", "signature"), false).is_none());
        // Revised S2 (11 November 2026): volatile is a kind, in both games.
        assert_eq!(judge(&ok.replace("barrier", "volatile"), false), None);
        assert_eq!(judge(&ok.replace("barrier", "volatile"), true), None);
        assert!(judge("FAKEMATCH(volatile): read afresh (and more).", false).unwrap().starts_with("not S2's form"));
        // A once-only block is dead now; one-pass is no kind.
        let retired = judge(&ok.replace("barrier", "one-pass"), false).unwrap();
        assert!(retired.starts_with("one-pass is no kind") && retired.contains("FAKEMATCH(dead)"), "{retired}");
        assert!(judge(&ok.replace("barrier", "frame"), false).unwrap().starts_with("unknown kind frame"));
        assert!(judge(&ok.replace("barrier", "frame"), false).unwrap().ends_with(&DEVICES.join(", ")));
        assert!(judge(&ok.replace("(barrier)", ""), false).unwrap().starts_with("unfinished, no kind"));
        assert!(judge("FAKEMATCH: forwarding through the pointer.", false).unwrap().starts_with("unfinished, no kind"));
        assert!(judge("FAKEMATCH(): kept.", false).is_some());
        assert!(judge(&ok.replace("2 of", "0 of"), false).unwrap().contains("no device"));
        assert!(judge(&ok.replace("tla-en", "tla-xx"), false).unwrap().contains("not an edition"));
        assert!(judge("FAKEMATCH(dead): kept.", false).unwrap().starts_with("not S2's form"));
        // The Broken Seal's finished tags: the plain form named by what it
        // lacks, and the function measured in a parenthesis.
        for finished in [
            "FAKEMATCH(dead): updates that cancel out end CSE; without them, 2 of 43 instructions differ in tbs-en.",
            "FAKEMATCH(dead): a never-taken null arm; with no arm, 2 of 174 instructions differ in tbs-en.",
            "FAKEMATCH(dead): state set again; as the one assignment above, 14 of 62 instructions differ in tbs-en (BattleArena_ExchangePartyRecords).",
            "FAKEMATCH(dead): two self-tests of offset keep their branch. Without them, 22 of 228 instructions differ in tbs-en.",
            "FAKEMATCH(dead): the pair keeps r9; without the pair, 8 of 80 instructions of the nested DjinnMenu_DrawStatArrow differ in tbs-en.",
            "FAKEMATCH(dead): dead stores restore the frame; without them, 65 of 1585 instructions differ in tbs-en, each a spill offset.",
        ] {
            assert_eq!(judge(finished, true), None, "{finished}");
        }
        assert!(judge("FAKEMATCH(dead): kept; without them, 2 of 43 differ in tbs-en.", true).unwrap().starts_with("not S2's form"));
        assert!(judge("FAKEMATCH(dead): kept, 2 of 43 instructions differ in tbs-en.", true).unwrap().starts_with("not S2's form"));
    }

    #[test]
    fn leftovers_are_judged_by_s2_with_their_evidence() {
        let ok = "LEFTOVER(frame): an unused local the ROM's frame reserves and never touches: of the 36 bytes it reserves (sub sp, #36), no instruction addresses the 28 at sp+8 to sp+35; as without it, 4 of 246 instructions differ in tbs-en.";
        assert_eq!(judge(ok, true), None);
        for evidenced in [
            "LEFTOVER(template): a second blitter, idle here, as its sibling BattleEffect_RunBolt uses it; as one blitter, 6 of 80 instructions differ in tbs-en.",
            "LEFTOVER(template): a constant in a named local, as the family's macro FIELD_PARTICLES_SET_ACTIVE_STATE writes it; without it, 3 of 52 instructions differ in tbs-en.",
            "LEFTOVER(frame): the buffer tbs-ja's loop fills; the other editions keep its 256 bytes (sub sp, #256), which stay untouched; as declared only with the loop, 4 of 232 instructions differ in tbs-en.",
            "LEFTOVER(frame): an unused local: no instruction addresses the 8 bytes at sp+8, and The Lost Age's Message_OpenDecoded reserves the same 8; as without it, 4 of 42 instructions differ in tbs-en.",
        ] {
            assert_eq!(judge(evidenced, true), None, "{evidenced}");
        }
        // The same strictness as FAKEMATCH: a kind of its own list, the
        // measured ending, more than 0 instructions, a real edition.
        assert!(judge(&ok.replace("(frame)", ""), true).unwrap().starts_with("unfinished, no kind"));
        let unknown = judge(&ok.replace("(frame)", "(dead)"), true).unwrap();
        assert!(unknown.starts_with("unknown kind dead") && unknown.ends_with(&LEFTOVERS.join(", ")), "{unknown}");
        assert!(judge(&ok.replace("(frame)", "(barrier)"), true).unwrap().starts_with("unknown kind barrier"));
        assert!(judge(&ok.replace("4 of", "0 of"), true).unwrap().contains("no device"));
        assert!(judge(&ok.replace("tbs-en.", "tbs-xx."), true).unwrap().contains("not an edition"));
        let kept = judge("LEFTOVER(frame): kept.", true).unwrap();
        assert!(kept.starts_with("not S2's form") && kept.contains("what remains and the evidence"), "{kept}");
        // A leftover's kinds are not devices, and the reverse.
        assert!(judge(&ok.replace("LEFTOVER", "FAKEMATCH"), true).unwrap().starts_with("unknown kind frame"));
    }

    #[test]
    fn form_is_no_kind_of_leftover() {
        let form = judge(
            "LEFTOVER(form): the record through a pointer, which The Lost Age's Func_08049a30 needs too; as direct, 2 of 40 instructions differ in tbs-en.",
            true,
        )
        .unwrap();
        assert!(form.starts_with("form is no kind") && form.contains("fake match"), "{form}");
    }

    #[test]
    fn each_kind_states_its_own_evidence() {
        // A frame states the bytes reserved and that none is touched; the
        // word frame alone, or an animation frame, is no evidence.
        for unevidenced in [
            "LEFTOVER(frame): an unused local the ROM's frame reserves; as none, 2 of 30 instructions differ in tbs-en.",
            "LEFTOVER(frame): a constant in a named local, kept from frame 152 on; as the constant, 2 of 9 instructions differ in tbs-en.",
            "LEFTOVER(frame): an unused local for 12 bytes of the frame (sub sp, #12); as none, 4 of 42 instructions differ in tbs-en.",
        ] {
            let message = judge(unevidenced, true).unwrap();
            assert!(message.starts_with("a frame leftover states its evidence"), "{unevidenced}: {message}");
        }
        // A template names its sibling routine or its family's macro.
        for unevidenced in [
            "LEFTOVER(template): a second blitter, idle here, as its sibling routines use it; as one blitter, 6 of 80 instructions differ in tbs-en.",
            "LEFTOVER(template): the sibling loop above holds the size the same way; as 2, 69 of 415 instructions differ in tbs-en.",
        ] {
            let message = judge(unevidenced, true).unwrap();
            assert!(message.starts_with("a template leftover names"), "{unevidenced}: {message}");
        }
        // A claim about another edition names it outside the measurement;
        // one about The Lost Age names the routine there.
        let edition = judge(
            "LEFTOVER(frame): a buffer the Japanese edition fills, whose 256 bytes (sub sp, #256) stay untouched here; as none, 4 of 232 instructions differ in tbs-en.",
            true,
        )
        .unwrap();
        assert!(edition.contains("names that edition"), "{edition}");
        let measured = judge(
            "LEFTOVER(frame): a buffer the Japanese edition fills, 4 of 230 instructions differing in tbs-ja without it, whose 256 bytes (sub sp, #256) stay untouched here; as none, 4 of 232 instructions differ in tbs-en.",
            true,
        )
        .unwrap();
        assert!(measured.contains("names that edition"), "the measurement's edition is no evidence: {measured}");
        let lost_age = judge(
            "LEFTOVER(template): the shape The Lost Age's twin uses, as BattleEffect_RunBolt's; as none, 2 of 9 instructions differ in tbs-en.",
            true,
        );
        assert_eq!(lost_age, None, "a named routine carries the claim");
        let unnamed = judge(
            "LEFTOVER(frame): 8 bytes (sub sp, #16) no instruction addresses, which The Lost Age reserves too; as none, 4 of 42 instructions differ in tbs-en.",
            true,
        )
        .unwrap();
        assert!(unnamed.contains("names the routine there"), "{unnamed}");
    }

    #[test]
    fn a_leftover_never_names_a_form_s2_keeps_a_fake_match() {
        for (text, form) in [
            ("LEFTOVER(template): a self-assignment, as in BattleEffect_RunBolt; as none, 2 of 9 instructions differ in tbs-en.", "self-assignment"),
            ("LEFTOVER(template): cancelling updates, the helper HELPER_X's habit; as none, 2 of 9 instructions differ in tbs-en.", "cancelling"),
            ("LEFTOVER(template): both arms alike, as in Battle_Run; as one arm, 2 of 9 instructions differ in tbs-en.", "both arms"),
            ("LEFTOVER(frame): an empty block before sub sp, #12, the 12 bytes untouched; as none, 2 of 9 instructions differ in tbs-en.", "empty block"),
            ("LEFTOVER(template): a loop that runs once, as Battle_Run's; as its body, 2 of 9 instructions differ in tbs-en.", "runs once"),
        ] {
            let message = judge(text, true).unwrap();
            assert!(message.starts_with(&format!("a leftover never names {form}")), "{text}: {message}");
        }
    }

    #[test]
    fn dead_marks_say_what_the_rom_keeps() {
        assert_eq!(judge_dead("DEAD: nothing reads velocityZ; the ROM keeps the store."), None);
        assert_eq!(judge_dead("DEAD: the depth is clamped to -60..60 but never read; the ROM keeps the clamp and the move."), None);
        for unformed in ["DEAD: x++ then x--.", "DEAD: both fields are set again before anything reads them.", "DEAD: never true; the source keeps the test.", "DEAD:", "DEAD: never true; the ROM keeps the test"] {
            assert!(judge_dead(unformed).unwrap().starts_with("not S3's form"), "{unformed}");
        }
        let source = "\
void F(int x) {
  // DEAD: never true, as x is 5 here; the ROM keeps
  // the compare.
  if (x > 9)
    x = 9;
  // DEAD: x++ then x--.
  x++;
  // DEAD: a read whose value goes unused;
  // the ROM keeps the load.
  // An ordinary comment after it.
  (void)x;
}

// DEAD: outside; the ROM keeps nothing.
";
        let (tags, found) = findings("games/tbs/src/x.c", source);
        assert_eq!((tags.dead, tags.dead_unformed), (4, 0));
        let lines: Vec<(usize, &str)> = found.iter().map(|finding| (finding.line, finding.message.as_str())).collect();
        assert_eq!(
            lines,
            [(6, "not S3's form: // DEAD: <what>; the ROM keeps <its instructions>."), (14, "a // DEAD: mark outside any function")],
            "{found:?}"
        );
        // The Lost Age's marks are counted where they miss the form, not refused.
        let (tags, found) = findings("games/tla/src/x.c", source);
        assert_eq!((tags.dead, tags.dead_unformed, found.len()), (4, 2, 0), "{found:?}");
    }

    #[test]
    fn each_game_has_its_own_line() {
        assert_eq!(game_of("games/tbs/src/battle/run.c"), "games/tbs");
        assert_eq!(game_of("games/tla/include/x.h"), "games/tla");
        assert_eq!(game_of("tools/x.c"), "tools");
        let game = Game { tags: Tags { fakematch: 169, leftover: 48, dead: 33, dead_unformed: 0 }, views: BTreeMap::new(), files: 832, findings: 0 };
        assert_eq!(
            game.lines("games/tbs"),
            [
                "games/tbs: 832 files; 0 findings",
                "games/tbs: 169 FAKEMATCH tags",
                "games/tbs: 48 LEFTOVER tags",
                "games/tbs: 33 // DEAD: marks",
                "games/tbs: 0 VIEW markers"
            ]
        );
        let views = BTreeMap::from([("routine", 3), ("tag", 2)]);
        let game = Game { tags: Tags { fakematch: 1, leftover: 0, dead: 60, dead_unformed: 40 }, views, files: 2, findings: 0 };
        let lines = game.lines("games/tla");
        assert_eq!(lines[3], "games/tla: 60 // DEAD: marks (40 do not yet say what the ROM keeps, S3; counted, not refused)");
        assert_eq!(lines[4], "games/tla: 5 VIEW markers (3 routine, 2 tag)");
    }

    #[test]
    fn a_leftover_is_counted_apart_and_covers_no_asm() {
        let source = "\
void Kept(void) {
  /* LEFTOVER(frame): an idle buffer: of the 16 bytes the frame reserves (sub sp, #16), no instruction addresses any; as
   * no buffer, 4 of 90 instructions differ in tbs-en. */
  u8 buffer[16];
  asm(\"\");
}

void Unproven(void) {
  // LEFTOVER(template): a constant in a named local; as the constant, 2 of 9 instructions differ in tbs-en.
  int n = 4;
}

/* LEFTOVER(template): as its sibling Battle_Run; as one, 1 of 2 instructions differ in tbs-en. */
static const int table[] = { 1, 2 };
";
        let (tags, found) = findings("games/tbs/src/x.c", source);
        assert_eq!(tags, Tags { fakematch: 0, leftover: 3, dead: 0, dead_unformed: 0 });
        let lines: Vec<(usize, &str)> = found.iter().map(|finding| (finding.line, finding.message.split(':').next().unwrap())).collect();
        assert_eq!(
            lines,
            [
                (1, "Kept"),
                (9, "a template leftover names before its plain form the sibling routine whose shape it shares, or its family's macro (S2); without it, it is a fake match"),
                (13, "a tag outside any function"),
            ],
            "{found:?}"
        );
    }

    #[test]
    fn asm_needs_a_tag_in_its_own_function_or_macro() {
        let source = "\
void Tagged(void) {
  /* FAKEMATCH(barrier): held; as no asm, 2 of 9 instructions differ in tla-en. */
  asm(\"\" : \"+r\"(x));
  if (x) { CAMELOT_ASM(\"rrx\"); }
}

void Untagged(int y) {
  register int i asm(\"r10\") = y;
  asm volatile(\"\");
}

#define PINNED(a)                                                             \\
  {                                                                          \\
    /* FAKEMATCH(pin): the emote pinned; as a plain local, 4 of 30          \\
     * instructions */                                                       \\
    /* differ in tla-en. */                                                  \\
    register int a_ __asm__(\"r1\") = (a);                                   \\
  }

CAMELOT_ASM(\"stmia of three registers, which no pattern emits\")
static __inline__ void Proven(void) {
  register int r __asm__(\"r3\") = 0;
  __asm__ volatile(\"stmia %0!, {r0, r1, r2}\" : \"+r\"(r));
}

void Named(void) {
  extern const char gAlias[] __asm__(\"gName\");
  CAMELOT_ASM(\"mov from sp\")
  __asm__ volatile(\"mov %0, sp\" : \"=r\"(sp));
}

/* FAKEMATCH(dead): stray; as nothing, 1 of 2 instructions differ in tla-en. */
static const int table[] = { 1, 2 };

#if !PATCH
void Old(int id) {
  if (id) {
#else
void New(int id, int cue) {
  /* FAKEMATCH(pin): r pinned; as a plain local, 2 of 9 instructions differ in tla-en. */
  register int r asm(\"r2\") = cue;
  if (id && r) {
#endif
    id++;
  }
}

#ifdef MODERN
#define LINKED(x) ({ __asm__(\"\" : \"+r\"(x)); x; })
#else
#define LINKED(x) (x)
#endif
";
        let (tags, found) = findings("games/tla/src/x.c", source);
        assert_eq!(tags, Tags { fakematch: 4, ..Tags::default() });
        assert_eq!(
            found,
            [
                Finding { line: 7, message: "Untagged: 2 asm statements, 0 tags".to_string() },
                Finding { line: 32, message: "a tag outside any function".to_string() },
            ]
        );
    }
}
