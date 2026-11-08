//! The fake-match tags against S2: each comment that opens with FAKEMATCH
//! must read `FAKEMATCH(<kind>): the form kept; as <plain form>, N of M
//! instructions differ in <edition>.`, or one of the spellings `judge` takes,
//! inside the function (or macro) whose device it tags; and a function or macro with more asm statements than
//! tags has a device no tag names.

use crate::alchemy::humanity::DEVICES;
use crate::alchemy::targets::parse_decomp_target;
use crate::gba::clex::{lex, Kind, Token};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Kinds S2 once had and now writes as ordinary C, with no tag.
const ORDINARY: [&str; 2] = ["volatile", "one-pass"];
/// The kinds The Broken Seal refuses (S2).
const NOT_IN_TBS: [&str; 2] = ["pin", "signature"];

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
fn judge(text: &str, broken_seal: bool) -> Option<String> {
    static KIND: OnceLock<Regex> = OnceLock::new();
    static FORM: OnceLock<Regex> = OnceLock::new();
    let kind = KIND.get_or_init(|| Regex::new(r"^FAKEMATCH(\(([^)]*)\))?:").unwrap());
    let form = FORM.get_or_init(|| {
        Regex::new(r"^FAKEMATCH\([^)]*\): (.+)[;.] (?i:as|without|with) (.+?), (\d+) of (\d+) instructions(?: of [^,;]+?)? differ in ([a-z]+-[a-z]+)(?:[ ,][^.]*)?\.$").unwrap()
    });
    const FORM_TEXT: &str = "FAKEMATCH(<kind>): the form kept; as <plain form>, N of M instructions differ in <edition>.";
    let Some(captures) = kind.captures(text) else {
        return Some(format!("not S2's form: {FORM_TEXT}"));
    };
    let Some(kind) = captures.get(2).map(|kind| kind.as_str()) else {
        return Some(format!("no kind: {FORM_TEXT}"));
    };
    if ORDINARY.contains(&kind) {
        return Some(format!("{kind} is ordinary C under S2: it takes no tag"));
    }
    if !DEVICES.contains(&kind) {
        return Some(format!("unknown kind {kind}: {}", DEVICES.join(", ")));
    }
    if broken_seal && NOT_IN_TBS.contains(&kind) {
        return Some(format!("The Broken Seal takes no {kind} (S2)"));
    }
    let Some(captures) = form.captures(text) else {
        return Some(format!("not S2's form: {FORM_TEXT}"));
    };
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

/// Every tag in one C file judged, and each unit with more asm than tags.
pub fn findings(path: &str, source: &str) -> (usize, Vec<Finding>) {
    let broken_seal = path.contains("games/tbs/");
    let tokens = lex(source);
    let mut found = Vec::new();
    let mut tags = 0;
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
        if token.kind == Kind::Comment && plain(&[token.text]).starts_with("FAKEMATCH") {
            // A tag goes on through the comments right after it until its
            // sentence ends: a macro's tag is one comment a line.
            let mut comments = vec![token.text];
            while !plain(&comments).ends_with('.') && tokens.get(at + 1).is_some_and(|t| t.kind == Kind::Comment) {
                at += 1;
                comments.push(tokens[at].text);
            }
            tags += 1;
            if token.text.starts_with("///") || token.text.starts_with("/**") {
                found.push(Finding { line: token.line, message: "a tag is a // or /* */ comment, not documentation".to_string() });
            }
            if let Some(message) = judge(&plain(&comments), broken_seal) {
                found.push(Finding { line: token.line, message });
            }
            match unit {
                Some(unit) => units[unit].tags += 1,
                None => found.push(Finding { line: token.line, message: "a tag outside any function".to_string() }),
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

pub fn run(arguments: &[String]) -> Result<(), String> {
    if let Some(flag) = arguments.iter().find(|argument| argument.starts_with('-')) {
        return Err(format!("unknown option {flag}"));
    }
    let root = crate::compile::root();
    let paths = if arguments.is_empty() { vec!["games".to_string()] } else { arguments.to_vec() };
    let (mut tags, mut files, mut count) = (0, 0, 0);
    for file in sources(root, &paths)? {
        let source = String::from_utf8_lossy(&std::fs::read(&file).map_err(|error| format!("{}: {error}", file.display()))?).into_owned();
        let name = file.strip_prefix(root).unwrap_or(&file).display().to_string();
        let (found_tags, found) = findings(&name, &source);
        tags += found_tags;
        files += 1;
        count += found.len();
        for finding in found {
            println!("{name}:{}: {}", finding.line, finding.message);
        }
    }
    println!("{tags} tags in {files} files; {count} findings");
    match count {
        0 => Ok(()),
        n => Err(format!("{n} tags or asm statements break S2")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(judge(&ok.replace("barrier", "volatile"), false).unwrap().contains("ordinary C"));
        assert!(judge(&ok.replace("barrier", "frame"), false).unwrap().starts_with("unknown kind frame"));
        assert!(judge(&ok.replace("(barrier)", ""), false).unwrap().starts_with("no kind"));
        assert!(judge("FAKEMATCH(volatile): read afresh (and more).", false).unwrap().contains("ordinary C"));
        assert!(judge("FAKEMATCH: forwarding through the pointer.", false).unwrap().starts_with("no kind"));
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
        assert_eq!(tags, 4);
        assert_eq!(
            found,
            [
                Finding { line: 7, message: "Untagged: 2 asm statements, 0 tags".to_string() },
                Finding { line: 32, message: "a tag outside any function".to_string() },
            ]
        );
    }
}
