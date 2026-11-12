//! The fake-match tags against S2: each comment that opens with FAKEMATCH
//! must read `FAKEMATCH(<kind>): the form kept; as <plain form>, N of M
//! instructions differ in <edition>.`, or one of the spellings `judge` takes,
//! inside the function (or macro) whose device it tags; and a function or macro with more asm statements than
//! tags has a device no tag names. A LEFTOVER tag, Camelot's own idle code,
//! is held to the same form with its own kinds and must name its evidence;
//! it never covers an asm statement, since an asm is always a device.

use crate::alchemy::humanity::{DEVICES, LEFTOVERS};
use crate::alchemy::targets::parse_decomp_target;
use crate::gba::clex::{lex, Kind, Token};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The kind S2 once had for a once-only block: one no macro expands to is
/// now a `dead` device, and a macro's is ordinary C with no tag.
const RETIRED: &str = "one-pass";
/// The kinds The Broken Seal refuses (S2).
const NOT_IN_TBS: [&str; 2] = ["pin", "signature"];
/// What a LEFTOVER tag's first part names as its evidence, one of S2's
/// three: another edition or The Lost Age uses the thing (an edition, a
/// language or a game named), the shape repeats down sibling routines, or a
/// reconstructed macro or helper explains its family; for `frame`, the
/// ROM's own frame. Only that the evidence is named is seen, not that it holds.
const EVIDENCE: &str = r"(?i)\b(t(bs|la)-(ja|en|de|es|fr|it)|editions?|japanese|english|german|spanish|french|italian|the lost age|the broken seal|siblings?|family|macro|helper|frame)\b";

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
/// way with its own kinds, and its first part must also name its evidence
/// (see `EVIDENCE`): a claim without it is a fake match.
fn judge(text: &str, broken_seal: bool) -> Option<String> {
    static KIND: OnceLock<Regex> = OnceLock::new();
    static FORM: OnceLock<Regex> = OnceLock::new();
    static NAMED: OnceLock<Regex> = OnceLock::new();
    let kind = KIND.get_or_init(|| Regex::new(r"^(FAKEMATCH|LEFTOVER)(\(([^)]*)\))?:").unwrap());
    let form = FORM.get_or_init(|| {
        Regex::new(r"^[A-Z]+\([^)]*\): (.+)[;.] (?i:as|without|with) (.+?), (\d+) of (\d+) instructions(?: of [^,;]+?)? differ in ([a-z]+-[a-z]+)(?:[ ,][^.]*)?\.$").unwrap()
    });
    let named = NAMED.get_or_init(|| Regex::new(EVIDENCE).unwrap());
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
    if leftover && !named.is_match(&captures[1]) {
        return Some(
            "a leftover names its evidence before its plain form (S2): another edition or The Lost Age using it, \
             the shape its sibling routines share, its frame, or its family's macro or helper; without it, it is a fake match"
                .to_string(),
        );
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

/// Every tag in one C file judged, and each unit with more asm than tags.
/// The tags one file holds, each word counted apart (S2: never summed).
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub struct Tags {
    pub fakematch: usize,
    pub leftover: usize,
}

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
    let (mut tags, mut files, mut count) = (Tags::default(), 0, 0);
    for file in sources(root, &paths)? {
        let source = String::from_utf8_lossy(&std::fs::read(&file).map_err(|error| format!("{}: {error}", file.display()))?).into_owned();
        let name = file.strip_prefix(root).unwrap_or(&file).display().to_string();
        let (found_tags, found) = findings(&name, &source);
        tags.fakematch += found_tags.fakematch;
        tags.leftover += found_tags.leftover;
        files += 1;
        count += found.len();
        for finding in found {
            println!("{name}:{}: {}", finding.line, finding.message);
        }
    }
    println!("{} FAKEMATCH tags and {} LEFTOVER tags in {files} files; {count} findings", tags.fakematch, tags.leftover);
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
        let ok = "LEFTOVER(frame): an idle buffer the Japanese edition still fills; as no buffer, 4 of 90 instructions differ in tbs-en.";
        assert_eq!(judge(ok, true), None);
        for evidenced in [
            "LEFTOVER(template): a second blitter, idle here, as its sibling routines use it; as one blitter, 6 of 80 instructions differ in tbs-en.",
            "LEFTOVER(form): the record through a pointer, which The Lost Age's twin needs, measured in tla-en; as direct, 2 of 40 instructions differ in tbs-en.",
            "LEFTOVER(form): a constant in a named local, as the family's macro writes it; without it, 3 of 52 instructions differ in tbs-en.",
            "LEFTOVER(frame): an unused local the ROM's frame reserves; as none, 2 of 30 instructions differ in tbs-en.",
        ] {
            assert_eq!(judge(evidenced, true), None, "{evidenced}");
        }
        // The same strictness as FAKEMATCH: a kind of its own list, the
        // measured ending, more than 0 instructions, a real edition.
        assert!(judge(&ok.replace("(frame)", ""), true).unwrap().starts_with("unfinished, no kind"));
        let unknown = judge(&ok.replace("frame", "dead"), true).unwrap();
        assert!(unknown.starts_with("unknown kind dead") && unknown.ends_with(&LEFTOVERS.join(", ")), "{unknown}");
        assert!(judge(&ok.replace("(frame)", "(barrier)"), true).unwrap().starts_with("unknown kind barrier"));
        assert!(judge(&ok.replace("4 of", "0 of"), true).unwrap().contains("no device"));
        assert!(judge(&ok.replace("tbs-en", "tbs-xx"), true).unwrap().contains("not an edition"));
        let form = judge("LEFTOVER(form): kept.", true).unwrap();
        assert!(form.starts_with("not S2's form") && form.contains("what remains and the evidence"), "{form}");
        // The evidence is named before the plain form; the edition the
        // measurement names is not evidence.
        let bare = judge("LEFTOVER(form): a constant in a named local; as the constant, 2 of 9 instructions differ in tbs-en.", true).unwrap();
        assert!(bare.starts_with("a leftover names its evidence"), "{bare}");
        // A leftover's kinds are not devices, and the reverse.
        assert!(judge(&ok.replace("LEFTOVER", "FAKEMATCH"), true).unwrap().starts_with("unknown kind frame"));
        assert_eq!(judge(&ok.replace("LEFTOVER(frame)", "LEFTOVER(template)"), true), None);
    }

    #[test]
    fn a_leftover_is_counted_apart_and_covers_no_asm() {
        let source = "\
void Kept(void) {
  /* LEFTOVER(frame): an idle buffer the Japanese edition fills; as
   * no buffer, 4 of 90 instructions differ in tbs-en. */
  u8 buffer[16];
  asm(\"\");
}

void Unproven(void) {
  // LEFTOVER(form): a constant in a named local; as the constant, 2 of 9 instructions differ in tbs-en.
  int n = 4;
}

/* LEFTOVER(template): as its siblings; as one, 1 of 2 instructions differ in tbs-en. */
static const int table[] = { 1, 2 };
";
        let (tags, found) = findings("games/tbs/src/x.c", source);
        assert_eq!(tags, Tags { fakematch: 0, leftover: 3 });
        let lines: Vec<(usize, &str)> = found.iter().map(|finding| (finding.line, finding.message.split(':').next().unwrap())).collect();
        assert_eq!(
            lines,
            [
                (1, "Kept"),
                (9, "a leftover names its evidence before its plain form (S2)"),
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
        assert_eq!(tags, Tags { fakematch: 4, leftover: 0 });
        assert_eq!(
            found,
            [
                Finding { line: 7, message: "Untagged: 2 asm statements, 0 tags".to_string() },
                Finding { line: 32, message: "a tag outside any function".to_string() },
            ]
        );
    }
}
