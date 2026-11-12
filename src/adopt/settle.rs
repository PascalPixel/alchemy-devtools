//! `adopt settle`: the views a join with views kept, taken away wherever
//! the code does not need them. Each view is one part's own way of seeing a
//! name the rest of its object sees otherwise (`adopt merge --keep-views`).
//! A routine's or variable's is tried as the definition's declaration (or,
//! where the definition is assembly or not C, as the declaration most files
//! write) and as whatever the object already declares; a tag's, typedef's,
//! enumerator's, static's or macro's as the definition the rest of the
//! object has. The compiler judges: every object holding views is compiled
//! in every edition with a view gone, and each function held against the
//! build's own assembly of that object (`.L` labels renumbered). A view
//! whose removal moves nothing goes, with its marker; one that moves code
//! stays, its marker saying what moved and by how much, so what is left is
//! the list of real disagreements.
//!
//! An object's views are first tried all at once, then by halves, so an
//! object whose views all go costs one compile an edition; a symbol that
//! moves code is tried view by view. Objects settle in parallel.

use super::views::{self, assembly, text, Compiler, Mode};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

const USAGE: &str = r#"usage: alchemy-devtools adopt settle --game GAME [--dry-run] [--no-modern] [OBJECT.c...]
Takes away the views adopt merge --keep-views wrote, wherever the code does
not need them. Every object of GAME holding a view marker
  /* VIEW(PART): KIND NAME as THIS; OTHER has THAT. */
(or only the OBJECT.c files named) is settled by symbol. A routine or
variable's view is tried as the definition's own declaration, written in
place of the view's bound one under the plain name (where the definition is
assembly or absent, the declaration most files write), then with no
declaration of its own, as the object sees the name elsewhere; a tag,
typedef, enumerator, static or macro's view is tried as the definition the
rest of the object has: the part's own definition deleted and its name
written plainly again. A view whose definition holds a FAKEMATCH tag is not
tried. The compiler judges: the object is compiled in every edition that
links it by the build's own recipe, and every function and datum is held
against the build's assembly of that object (out/EDITION/obj/OBJECT.s, .L
labels renumbered), which must be newer than its source. An object's views
are tried all at once, then by halves; a symbol that moves code is tried
view by view, and each choice in turn. A view that moves nothing is deleted
with its marker; one that moves code stays, its marker ending
  ...; tried as the definition's DECLARATION, FUNCTION moves: N of M instructions in EDITION. */
Objects settle in parallel; the report lists the symbols settled, the views
deleted and the views left, by symbol, with the functions that moved.
Each try is also compiled as make modern compiles the object, and must have
no error; once written, make modern-GAME must pass (--no-modern skips both).
Scratch files are written outside the checkout; --dry-run writes nothing.
Otherwise the objects are written with a journal under out/adopt/ that
adopt --undo reverses. Run make compare-editions after: settle compiles
each object as the build does but does not link."#;

/// A view as its marker names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct View {
    /// The marker's line, trimmed, as the source writes it.
    pub marker: String,
    /// The line that finds the view again after other edits: its marker,
    /// or while it is settled a line no other view has (two views of one
    /// name in one part have the same marker).
    pub key: String,
    pub suffix: String,
    pub kind: &'static str,
    /// The name the view is of (a tag's without `struct`).
    pub name: String,
}

impl View {
    fn private(&self) -> String {
        format!("{}__{}", self.name, self.suffix)
    }
}

/// A view's marker read from its line, or None for any other line.
pub(crate) fn marker(line: &str) -> Option<View> {
    let line = line.trim();
    if !line.starts_with("/* ") || !line.ends_with("*/") {
        return None;
    }
    let kind = views::view_kind(line)?.ok()?;
    let rest = line.split_once(views::MARKER)?.1;
    let (suffix, rest) = rest.split_once("): ")?;
    let rest = rest.strip_prefix(kind)?.trim_start();
    let named = rest.split_once(" as ")?.0;
    let name = ["struct ", "union ", "enum "].iter().find_map(|tag| named.strip_prefix(tag)).unwrap_or(named);
    Some(View { marker: line.to_string(), key: line.to_string(), suffix: suffix.to_string(), kind, name: name.to_string() })
}

/// A view's own line while it is settled: `/* VIEW#N */` for the view at
/// index N, a comment the compiler reads as nothing.
fn key_of(index: usize) -> String {
    format!("/* VIEW#{index} */")
}

/// The text with each view's marker written as its key, and the views
/// keyed: every view found by its own line, whatever its marker repeats.
fn keyed(text: &str, views: &[View]) -> (String, Vec<View>) {
    let mut keyed: Vec<View> = views.to_vec();
    let mut next = 0;
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let (content, newline) = line.strip_suffix('\n').map_or((line, ""), |content| (content, "\n"));
        match keyed.get(next).filter(|view| content.trim() == view.marker) {
            Some(_) => {
                let indent = &content[..content.len() - content.trim_start().len()];
                keyed[next].key = key_of(next);
                out.push_str(&format!("{indent}{}{newline}", keyed[next].key));
                next += 1;
            }
            None => out.push_str(line),
        }
    }
    (out, keyed)
}

/// The text with every key still standing written as its view's marker.
fn unkeyed(text: &str, views: &[View]) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let (content, newline) = line.strip_suffix('\n').map_or((line, ""), |content| (content, "\n"));
        match views.iter().find(|view| view.key != view.marker && content.trim() == view.key) {
            Some(view) => {
                let indent = &content[..content.len() - content.trim_start().len()];
                out.push_str(&format!("{indent}{}{newline}", view.marker));
            }
            None => out.push_str(line),
        }
    }
    out
}

/// What becomes of a view: deleted, the object seeing the name as it does
/// elsewhere, or its bound declaration written plainly as `declaration`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Remove,
    Declare { declaration: String, whose: String },
    /// A tag's view deleted, the part's uses of its members written with
    /// the names the other definition gives the same layout.
    Members { renames: Vec<(String, String)> },
}

impl Choice {
    /// How a kept marker says what was tried.
    fn said(&self, view: &View) -> String {
        match self {
            Choice::Declare { declaration, whose } => format!("as {whose} {}", brief(declaration)),
            Choice::Remove if matches!(view.kind, "routine" | "variable") => "as the rest of the object declares it".into(),
            Choice::Remove => "with the definition the rest of the object has".into(),
            Choice::Members { renames } => {
                let renames: Vec<String> = renames.iter().map(|(from, to)| format!("{from} as {to}")).collect();
                format!("with the definition the rest of the object has, its members read {}", renames.join(", "))
            }
        }
    }
}

/// A marker's text kept to one readable line.
fn brief(text: &str) -> String {
    let text = text.replace("*/", "* /").split_whitespace().collect::<Vec<_>>().join(" ");
    let text = text.trim_end_matches(';').to_string();
    match text.char_indices().nth(140) {
        Some((at, _)) => format!("{}...", &text[..at]),
        None => text,
    }
}

/// Whether a line opens a part of a joined object (`// ---- STEM.c ----`).
fn separates(line: &str) -> bool {
    line.starts_with("// ---- ") && line.ends_with(".c ----")
}

/// The lines of the part that holds line `at`: from its separator to the next.
fn region(lines: &[String], at: usize) -> (usize, usize) {
    let start = lines[..=at].iter().rposition(|line| separates(line)).unwrap_or(0);
    let end = lines[at + 1..].iter().position(|line| separates(line)).map_or(lines.len(), |n| at + 1 + n);
    (start, end)
}

fn word(name: &str) -> Regex {
    Regex::new(&format!(r"\b{}\b", regex::escape(name))).unwrap()
}

/// The 0-based lines a statement covers in `lines`, with the `///` comment
/// above it.
fn documented(lines: &[String], statement: &text::Statement) -> (usize, usize) {
    let mut first = statement.first_line - 1;
    while first > 0 && lines[first - 1].trim_start().starts_with("///") {
        first -= 1;
    }
    (first, statement.last_line)
}

/// A part's lines as one text.
fn joined(lines: &[String]) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

fn holds_tag(lines: &[String], range: (usize, usize)) -> bool {
    lines[range.0..range.1].iter().any(|line| line.contains("FAKEMATCH"))
}

/// The text with one view taken away as `choice` says.
pub(crate) fn apply(text: &str, view: &View, choice: &Choice) -> Result<String, String> {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let at = lines.iter().position(|line| line.trim() == view.key).ok_or_else(|| format!("{}'s marker was not found", view.name))?;
    let (start, end) = region(&lines, at);
    let mut part = lines[start..end].to_vec();
    edit(&mut part, at - start, view, choice)?;
    lines.splice(start..end, part);
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

/// Where a view's own lines end in a part, from line `at` (where its marker
/// was): at the next view's marker, or the part's end.
fn own_end(part: &[String], at: usize) -> usize {
    part[at.min(part.len())..].iter().position(|line| line.trim_start().starts_with("/* VIEW")).map_or(part.len(), |n| at + n)
}

/// Removes, in a part's lines from `at` to the next view, `#define NAME
/// PRIVATE` and the `#undef NAME` after it: a header view's wrapper. True
/// if there was one.
fn unwrap_header(part: &mut Vec<String>, at: usize, name: &str, private: &str) -> bool {
    let define = format!("#define {name} {private}");
    let end = own_end(part, at);
    let Some(found) = part[at.min(end)..end].iter().position(|line| line.trim() == define).map(|n| at + n) else { return false };
    part.remove(found);
    let undefine = format!("#undef {name}");
    if let Some(n) = part[found..].iter().position(|line| line.trim() == undefine) {
        part.remove(found + n);
    }
    true
}

fn edit(part: &mut Vec<String>, at: usize, view: &View, choice: &Choice) -> Result<(), String> {
    let private = view.private();
    let name = view.name.as_str();
    part.remove(at);
    let wrapped = unwrap_header(part, at, name, &private);
    match view.kind {
        "routine" | "variable" => {
            if wrapped {
                // The tags the view declared for its bound declaration.
                while part.get(at).is_some_and(|line| forward(line)) {
                    part.remove(at);
                }
            }
            let text = joined(part);
            let all = text::statements(&text);
            // Its own bound declaration: the first after its marker, before the
            // next view's; else the part's first.
            let end = own_end(part, at);
            let binds = |s: &&text::Statement| s.find(&private).is_some() && s.find("__asm__").is_some() && !s.several();
            let bound = all.iter().filter(binds).find(|s| s.first_line > at && s.first_line <= end).or_else(|| all.iter().find(binds));
            match bound {
                Some(statement) => {
                    let (first, last) = (statement.first_line - 1, statement.last_line);
                    let range = match choice {
                        Choice::Declare { .. } if !wrapped => (first, last),
                        _ => documented(part, statement),
                    };
                    if holds_tag(part, range) {
                        return Err(format!("{name}'s declaration holds a FAKEMATCH tag"));
                    }
                    let written: Vec<String> = match choice {
                        Choice::Declare { declaration, .. } if !wrapped => vec![declaration.clone()],
                        _ => Vec::new(),
                    };
                    part.splice(range.0..range.1, written);
                }
                None if wrapped => {}
                None => return Err(format!("{name}'s bound declaration was not found")),
            }
        }
        "tag" => {
            let text = joined(part);
            let all = text::statements(&text);
            let found = all.iter().find_map(|s| {
                let at = s.words.windows(3).position(|w| matches!(w[0].text, "struct" | "union" | "enum") && w[1].text == private && w[2].text == "{" && w[0].depth == 0)?;
                Some((s, at))
            });
            match found {
                Some((statement, at)) => {
                    let close = statement.words[at + 2..].iter().position(|w| w.text == "}" && w.depth == 0).map(|n| at + 2 + n).ok_or("its definition has no end")?;
                    let alone = at == 0 && statement.words.len() == close + 2 && statement.words[close + 1].text == ";";
                    let range = documented(part, statement);
                    if holds_tag(part, range) {
                        return Err(format!("struct {name}'s definition holds a FAKEMATCH tag"));
                    }
                    if alone {
                        part.splice(range.0..range.1, Vec::new());
                    } else {
                        // `typedef struct X { ... } T;` keeps its declarators.
                        let (from, to) = (statement.words[at + 2].at, statement.words[close].at + 1);
                        let from = text[..from].trim_end().len();
                        let mut text = text.clone();
                        text.replace_range(from..to, "");
                        *part = text.lines().map(str::to_string).collect();
                    }
                }
                None if wrapped => {}
                None => return Err(format!("struct {private} was not found")),
            }
        }
        "static" | "typedef" | "enumerator" => {
            let text = joined(part);
            let all = text::statements(&text);
            let top = |s: &text::Statement| s.words.iter().any(|w| w.depth == 0 && w.text == private);
            let found = all.iter().find(|s| match view.kind {
                "enumerator" => s.enumerates(&private),
                "typedef" => s.has("typedef") && top(s),
                _ => top(s) && (s.has("static") || s.routine(&private)),
            });
            match found {
                Some(statement) => {
                    let range = documented(part, statement);
                    if holds_tag(part, range) {
                        return Err(format!("{name}'s definition holds a FAKEMATCH tag"));
                    }
                    part.splice(range.0..range.1, Vec::new());
                }
                // An enum's other enumerators took it already.
                None if wrapped || view.kind == "enumerator" => {}
                None => return Err(format!("{private} was not found")),
            }
        }
        "macro" => {
            if view.marker.contains(&format!(" as {private};")) {
                remove_defines(part, &private);
                let undefine = format!("#undef {private}");
                part.retain(|line| line.trim() != undefine);
            } else {
                // Its own definition after the marker, and the joined one
                // given back where the part ends.
                let undefine = format!("#undef {name}");
                if part.get(at).is_some_and(|line| line.trim() == undefine) {
                    part.remove(at);
                    if part.get(at).is_some_and(|line| defines(line, name)) {
                        remove_define_at(part, at);
                    }
                }
                let tail = part.iter().rposition(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#')).map_or(0, |n| n + 1);
                if let Some(n) = part[tail..].iter().rposition(|line| line.trim() == undefine) {
                    part.remove(tail + n);
                    if part.get(tail + n).is_some_and(|line| defines(line, name)) {
                        remove_define_at(part, tail + n);
                    }
                }
            }
        }
        _ => return Err(format!("no way to settle a {} view", view.kind)),
    }
    // Where another view of the part still binds the private name (one
    // around each of two includes), the part's code keeps calling it by
    // that name: only the last view of it taken away writes the plain name.
    if matches!(view.kind, "routine" | "variable") {
        let define = format!("#define {name} {private}");
        let text = joined(part);
        let still = part.iter().any(|line| line.trim() == define) || text::statements(&text).iter().any(|s| s.find(&private).is_some() && s.find("__asm__").is_some());
        if still {
            return Ok(());
        }
    }
    let pattern = word(&private);
    for line in part.iter_mut() {
        if marker(line).is_none() && pattern.is_match(line) {
            *line = pattern.replace_all(line, name).into_owned();
        }
    }
    if let Choice::Members { renames } = choice {
        let map: BTreeMap<&str, &str> = renames.iter().map(|(from, to)| (from.as_str(), to.as_str())).collect();
        let alternatives: Vec<String> = map.keys().map(|from| regex::escape(from)).collect();
        let access = Regex::new(&format!(r"(\.|->)(\s*)({})\b", alternatives.join("|"))).unwrap();
        for line in part.iter_mut() {
            if marker(line).is_none() && !line.trim_start().starts_with("//") {
                *line = access.replace_all(line, |found: &regex::Captures| format!("{}{}{}", &found[1], &found[2], map[&found[3]])).into_owned();
            }
        }
    }
    Ok(())
}

/// The members of a `struct` or `union TAG { ... }` a text defines at file
/// scope, by their tokens inside the braces; None where it defines none.
fn body_of<'t>(text: &'t str, tag: &str) -> Option<Vec<&'t str>> {
    for statement in text::statements(text) {
        let words = &statement.words;
        let Some(at) = words.windows(3).position(|w| w[0].depth == 0 && matches!(w[0].text, "struct" | "union") && w[1].text == tag && w[2].text == "{") else { continue };
        let close = words[at + 2..].iter().position(|w| w.text == "}" && w.depth == 0)? + at + 2;
        return Some(words[at + 3..close].iter().map(|w| w.text).collect());
    }
    None
}

/// A body's members as (layout, name): each member's tokens with its name
/// written `#`. None for a member this cannot read (two names in one, a
/// nameless one).
fn members(body: &[&str]) -> Option<Vec<(String, String)>> {
    let mut found = Vec::new();
    let mut member: Vec<&str> = Vec::new();
    let mut depth = 0i32;
    for &token in body {
        match token {
            "{" | "(" | "[" => depth += 1,
            "}" | ")" | "]" => depth -= 1,
            _ => {}
        }
        if token == ";" && depth == 0 {
            found.push(member_of(&member)?);
            member.clear();
        } else {
            if token == "," && depth == 0 {
                return None;
            }
            member.push(token);
        }
    }
    member.is_empty().then_some(found)
}

fn member_of(tokens: &[&str]) -> Option<(String, String)> {
    let ident = |t: &&str| t.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    // A pointer to a routine: the name after `(*`; else the last name
    // before a `[`, a `:` or the end.
    let at = match tokens.windows(3).position(|w| w[0] == "(" && w[1] == "*" && ident(&w[2])) {
        Some(at) => at + 2,
        None => {
            let end = tokens.iter().position(|t| *t == "[" || *t == ":").unwrap_or(tokens.len());
            tokens[..end].iter().rposition(ident)?
        }
    };
    let mut layout: Vec<&str> = tokens.to_vec();
    let name = layout[at].to_string();
    layout[at] = "#";
    Some((layout.join(" "), name))
}

/// For a tag's view whose definition has the other's layout under other
/// member names: the names to write, each part member to the other's.
/// None where the layouts differ, the names agree, or an old name is a
/// member of another tag the object defines.
pub(crate) fn member_renames(object: &str, view: &View, other: &str) -> Option<Vec<(String, String)>> {
    let theirs = members(&body_of(other, &view.name)?)?;
    let ours = members(&body_of(object, &view.private())?)?;
    if theirs.len() != ours.len() || theirs.iter().zip(&ours).any(|(a, b)| a.0 != b.0) {
        return None;
    }
    let renames: Vec<(String, String)> = ours.iter().zip(&theirs).filter(|(a, b)| a.1 != b.1).map(|(a, b)| (a.1.clone(), b.1.clone())).collect();
    if renames.is_empty() {
        return None;
    }
    // The member names of every other tag the object defines.
    let mut elsewhere: BTreeSet<String> = BTreeSet::new();
    let private = view.private();
    for statement in text::statements(object) {
        let words = &statement.words;
        for (at, w) in words.windows(3).enumerate() {
            if matches!(w[0].text, "struct" | "union") && w[2].text == "{" && w[1].text != private && w[1].text != view.name {
                let Some(close) = words[at + 2..].iter().position(|x| x.text == "}" && x.depth == w[0].depth).map(|n| n + at + 2) else { continue };
                let body: Vec<&str> = words[at + 3..close].iter().map(|x| x.text).collect();
                elsewhere.extend(members(&body).unwrap_or_default().into_iter().map(|(_, name)| name));
            }
        }
    }
    (!renames.iter().any(|(from, _)| elsewhere.contains(from))).then_some(renames)
}

/// `struct X;` or `union X;`, a view's forward declaration.
fn forward(line: &str) -> bool {
    let line = line.trim();
    let Some(rest) = line.strip_prefix("struct ").or_else(|| line.strip_prefix("union ")) else { return false };
    rest.strip_suffix(';').is_some_and(|tag| !tag.is_empty() && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

fn defines(line: &str, name: &str) -> bool {
    line.trim_start().strip_prefix("#define ").and_then(|rest| rest.strip_prefix(name)).is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t', '(']))
}

fn remove_define_at(part: &mut Vec<String>, at: usize) {
    while at < part.len() {
        let continued = part[at].trim_end().ends_with('\\');
        part.remove(at);
        if !continued {
            break;
        }
    }
}

fn remove_defines(part: &mut Vec<String>, name: &str) {
    while let Some(at) = part.iter().position(|line| defines(line, name)) {
        remove_define_at(part, at);
    }
}

/// How a test of an object went wrong: the edition, and the message that
/// stopped the compile or each function that moved (its name, instructions
/// differing and instructions it had).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Failure {
    pub edition: String,
    pub error: Option<String>,
    pub moved: Vec<(String, usize, usize)>,
}

impl Failure {
    fn said(&self) -> String {
        if let Some(error) = &self.error {
            return format!("it does not compile in {}: {}", self.edition, brief(error));
        }
        let mut shown: Vec<String> = self.moved.iter().take(3).map(|(name, n, m)| format!("{name} moves: {n} of {m} instructions")).collect();
        if self.moved.len() > 3 {
            shown.push(format!("{} more", self.moved.len() - 3));
        }
        format!("{} in {}", shown.join(", "), self.edition)
    }
}

/// What judges an object's text: None when it compiles to the build's own
/// assembly in every edition, read with `renamed`'s symbols (each private
/// name a static view gave, of a view the text takes away) written as the
/// plain name; every other symbol as the build wrote it.
pub(crate) trait Judge: Sync {
    fn judge(&self, text: &str, renamed: &[(String, String)]) -> Result<Option<Failure>, String>;
}

/// One symbol's views in one object, and the choices to try, in order.
#[derive(Clone, Debug)]
pub(crate) struct Unit {
    pub views: Vec<usize>,
    pub choices: Vec<Choice>,
}

/// What an object became: its text, the views deleted and those kept with
/// what was tried first and what it did.
#[derive(Debug, Default)]
pub(crate) struct Solved {
    pub text: String,
    pub deleted: Vec<usize>,
    pub kept: Vec<(usize, String, Vec<String>)>,
}

struct Solver<'a> {
    original: &'a str,
    views: &'a [View],
    units: &'a [Unit],
    judge: &'a dyn Judge,
    accepted: Vec<(usize, Choice)>,
    solved: Solved,
}

impl Solver<'_> {
    fn text(&self, extra: &[(usize, Choice)]) -> Result<String, String> {
        let mut text = self.original.to_string();
        for (view, choice) in self.accepted.iter().chain(extra) {
            text = apply(&text, &self.views[*view], choice)?;
        }
        Ok(text)
    }

    /// None when `extra` on what is accepted judges clean.
    fn test(&self, extra: &[(usize, Choice)]) -> Result<Option<Failure>, String> {
        // A static's view taken away gives its symbol the plain name back.
        let renamed: Vec<(String, String)> =
            self.accepted.iter().chain(extra).map(|(view, _)| &self.views[*view]).filter(|view| view.kind == "static").map(|view| (view.private(), view.name.clone())).collect();
        match self.text(extra) {
            Ok(text) => self.judge.judge(&text, &renamed),
            Err(why) => Ok(Some(Failure { edition: "the source".into(), error: Some(why), moved: Vec::new() })),
        }
    }

    fn accept(&mut self, edits: Vec<(usize, Choice)>) {
        self.solved.deleted.extend(edits.iter().map(|(view, _)| *view));
        self.accepted.extend(edits);
    }

    fn first(&self, units: &[usize]) -> Vec<(usize, Choice)> {
        units.iter().flat_map(|&unit| self.units[unit].views.iter().map(move |&view| (view, self.units[unit].choices[0].clone()))).collect()
    }

    fn settle(&mut self, units: &[usize]) -> Result<(), String> {
        if units.is_empty() {
            return Ok(());
        }
        let edits = self.first(units);
        let Some(failure) = self.test(&edits)? else {
            self.accept(edits);
            return Ok(());
        };
        if units.len() == 1 {
            return self.leaf(units[0], failure);
        }
        let (left, right) = units.split_at(units.len() / 2);
        self.settle(left)?;
        self.settle(right)
    }

    /// One symbol whose first choice moved code: its other choices, then
    /// each view on its own.
    fn leaf(&mut self, unit: usize, failure: Failure) -> Result<(), String> {
        let Unit { views, choices } = self.units[unit].clone();
        for choice in &choices[1..] {
            let edits: Vec<(usize, Choice)> = views.iter().map(|&view| (view, choice.clone())).collect();
            if self.test(&edits)?.is_none() {
                self.accept(edits);
                return Ok(());
            }
        }
        if views.len() == 1 {
            self.keep(views[0], &choices, &failure);
            return Ok(());
        }
        for &view in &views {
            let mut first: Option<Failure> = None;
            let mut done = false;
            for choice in &choices {
                let edit = vec![(view, choice.clone())];
                match self.test(&edit)? {
                    None => {
                        self.accept(edit);
                        done = true;
                        break;
                    }
                    Some(failed) => {
                        first.get_or_insert(failed);
                    }
                }
            }
            if !done {
                self.keep(view, &choices, first.as_ref().unwrap_or(&failure));
            }
        }
        Ok(())
    }

    /// Where the object already declares the name as the definition does, the
    /// first try was the definition's, written by nothing new.
    fn keep(&mut self, view: usize, choices: &[Choice], failure: &Failure) {
        let tried = match choices {
            [Choice::Remove, Choice::Declare { declaration, whose }, ..] if whose == "the definition's" => format!("as the definition's {}, which the object already declares", brief(declaration)),
            _ => choices[0].said(&self.views[view]),
        };
        let note = format!("{tried}, {}", failure.said());
        let functions = failure.moved.iter().map(|(name, ..)| name.clone()).collect();
        self.solved.kept.push((view, note, functions));
    }
}

/// Settles an object's units: its text with every view gone that moves
/// nothing, and each kept one's marker saying what was tried.
pub(crate) fn solve(original: &str, views: &[View], units: &[Unit], judge: &dyn Judge) -> Result<Solved, String> {
    // Each view found by a line of its own, whatever its marker repeats.
    let (original, views) = keyed(original, views);
    let (original, views) = (original.as_str(), views.as_slice());
    let mut solver = Solver { original, views, units, judge, accepted: Vec::new(), solved: Solved::default() };
    // A unit no choice can even be written for stays as it is.
    let mut tried = Vec::new();
    for (index, unit) in units.iter().enumerate() {
        let edits: Vec<(usize, Choice)> = unit.views.iter().map(|&view| (view, unit.choices[0].clone())).collect();
        let mut text = original.to_string();
        let written = edits.iter().try_for_each(|(view, choice)| apply(&text, &views[*view], choice).map(|t| text = t));
        match written {
            Ok(()) => tried.push(index),
            Err(why) => {
                for &view in &unit.views {
                    solver.solved.kept.push((view, format!("not tried: {}", brief(&why)), Vec::new()));
                }
            }
        }
    }
    solver.settle(&tried)?;
    solver.solved.text = solver.text(&[])?;
    let kept = std::mem::take(&mut solver.solved.kept);
    for (view, note, _) in &kept {
        solver.solved.text = annotate(&solver.solved.text, &views[*view], note);
    }
    solver.solved.text = unkeyed(&solver.solved.text, views);
    solver.solved.kept = kept;
    solver.solved.deleted.sort_unstable();
    Ok(solver.solved)
}

/// What a kept marker's note opens with, so a second settle replaces it.
const TRIED: &str = "; tried ";

/// What a marker that could not be tried says, the same way.
const UNTRIED: &str = "; not tried: ";

/// The text with a kept view's marker ending in what was tried.
fn annotate(text: &str, view: &View, note: &str) -> String {
    let body = view.marker.strip_suffix("*/").unwrap_or(&view.marker).trim_end();
    let body = body.strip_suffix('.').unwrap_or(body);
    let body = [TRIED, UNTRIED].iter().find_map(|note| body.split_once(note)).map_or(body, |(before, _)| before);
    // A view no choice could be written for says so in place of what was tried.
    let rewritten = match note.strip_prefix("not tried: ") {
        Some(why) => format!("{body}{UNTRIED}{}. */", why.trim_end_matches('.')),
        None => format!("{body}{TRIED}{}. */", note.strip_prefix("tried ").unwrap_or(note).trim_end_matches('.')),
    };
    let mut out = String::with_capacity(text.len() + rewritten.len());
    for line in text.split_inclusive('\n') {
        let (content, newline) = line.strip_suffix('\n').map_or((line, ""), |content| (content, "\n"));
        if content.trim() == view.key {
            let indent = &content[..content.len() - content.trim_start().len()];
            out.push_str(indent);
            out.push_str(&rewritten);
            out.push_str(newline);
        } else {
            out.push_str(line);
        }
    }
    out
}

/// The units of an object's views: a routine's, variable's or other name's
/// views together, an enum's enumerators together.
pub(crate) fn units(views: &[View], lines: &[&str], choices: &dyn Fn(&View) -> Vec<Choice>) -> Vec<Unit> {
    let mut keyed: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let line_of = |view: &View| lines.iter().position(|line| line.trim() == view.key).unwrap_or(0);
    let mut previous: Option<(usize, String)> = None;
    for (index, view) in views.iter().enumerate() {
        let line = line_of(view);
        let key = match view.kind {
            "routine" | "variable" => format!("symbol {}", view.name),
            "enumerator" => match &previous {
                Some((at, key)) if *at + 1 == line && key.starts_with("enumerator ") => key.clone(),
                _ => format!("enumerator {} {}", view.suffix, view.name),
            },
            // A macro's and a tag's views are each part's own: their
            // choices are worked out from that part's definition.
            kind => format!("{kind} {} {}", view.name, if matches!(kind, "macro" | "tag") { view.suffix.as_str() } else { "" }),
        };
        previous = Some((line, key.clone()));
        if !keyed.contains_key(&key) {
            order.push(key.clone());
        }
        keyed.entry(key).or_default().push(index);
    }
    order
        .into_iter()
        .map(|key| {
            let views_of = keyed.remove(&key).unwrap_or_default();
            let choices = choices(&views[views_of[0]]);
            Unit { views: views_of, choices }
        })
        .collect()
}

/// The declarations a game's C and headers write of the names its views
/// are of: each name's definition (as a declaration), the declarations by
/// how many statements write them, and those a header writes.
#[derive(Debug, Default)]
pub(crate) struct Declarations {
    pub definition: BTreeMap<String, String>,
    pub declared: BTreeMap<String, BTreeMap<String, usize>>,
    pub in_headers: BTreeSet<String>,
}

/// Whether the word at `at` of a statement declares a variable: at file
/// scope outside any parentheses, followed by what ends a declarator.
fn declarator(statement: &text::Statement, at: usize) -> bool {
    let words = &statement.words;
    if words[at].depth != 0 || at > 0 && matches!(words[at - 1].text, "struct" | "union" | "enum" | "." | "->") {
        return false;
    }
    let mut parens = 0i32;
    for word in &words[..at] {
        match word.text {
            "(" | "[" => parens += 1,
            ")" | "]" => parens -= 1,
            _ => {}
        }
    }
    parens == 0 && words.get(at + 1).is_some_and(|next| matches!(next.text, ";" | "[" | "=" | "__asm__" | "__attribute__"))
}

/// A statement's declaration of `plain`, written plainly: a definition's
/// head, an initializer left off, a view's private name and binding taken
/// out, a variable made `extern`.
fn declaration_of(statement: &text::Statement, written: &str, plain: &str, routine: bool) -> String {
    let mut words: Vec<&str> = Vec::new();
    let mut skip = 0;
    for (index, word) in statement.words.iter().enumerate() {
        if let Some((open, _)) = statement.body {
            if word.at >= open {
                break;
            }
        }
        if skip > 0 {
            skip -= 1;
            continue;
        }
        if word.depth == 0 && (word.text == ";" || !routine && word.text == "=") {
            break;
        }
        if word.text == "__asm__" && statement.words.get(index + 1).is_some_and(|w| w.text == "(") {
            skip = 3;
            continue;
        }
        words.push(if word.text == written { plain } else { word.text });
    }
    if routine && words.first() == Some(&"extern") {
        words.remove(0);
    } else if !routine && words.first() != Some(&"extern") {
        words.insert(0, "extern");
    }
    format!("{};", text::squash(words.into_iter()))
}

impl Declarations {
    /// Reads `files` (path and text) for `names`.
    pub fn read(files: &[(String, String)], names: &BTreeSet<String>) -> Declarations {
        let mut found = Declarations::default();
        for (path, source) in files {
            let header = path.ends_with(".h");
            for statement in text::statements(source) {
                if statement.has("static") || statement.has("typedef") || statement.several() {
                    continue;
                }
                let mut seen: BTreeSet<&str> = BTreeSet::new();
                for (at, word) in statement.words.iter().enumerate() {
                    if word.depth != 0 {
                        continue;
                    }
                    let plain = if names.contains(word.text) {
                        word.text
                    } else {
                        match word.text.split_once("__") {
                            Some((plain, _)) if names.contains(plain) && statement.find("__asm__").is_some() => plain,
                            _ => continue,
                        }
                    };
                    let routine = statement.routine(word.text);
                    if !routine && !declarator(&statement, at) || !seen.insert(plain) {
                        continue;
                    }
                    let defines = if routine { statement.body.is_some() } else { !statement.has("extern") };
                    let declaration = declaration_of(&statement, word.text, plain, routine);
                    if defines {
                        found.definition.entry(plain.to_string()).or_insert(declaration);
                    } else {
                        *found.declared.entry(plain.to_string()).or_default().entry(declaration.clone()).or_default() += 1;
                        if header {
                            found.in_headers.insert(declaration);
                        }
                    }
                }
            }
        }
        found
    }

    /// The choices for a routine's or variable's view in an object whose
    /// text is `object`: the definition's declaration (else the one most
    /// written) and the next most written, and the object's own. Where the
    /// first is already written in a header or the object, the object's own
    /// comes first, so no repeat is added.
    pub fn choices(&self, name: &str, object: &str) -> Vec<Choice> {
        let mut written: Vec<(String, String)> = Vec::new();
        if let Some(definition) = self.definition.get(name) {
            written.push((definition.clone(), "the definition's".into()));
        }
        let mut declared: Vec<(&String, &usize)> = self.declared.get(name).map(|d| d.iter().collect()).unwrap_or_default();
        // The most written first; of as many, a header's.
        declared.sort_by(|a, b| b.1.cmp(a.1).then(self.in_headers.contains(b.0).cmp(&self.in_headers.contains(a.0))).then(a.0.cmp(b.0)));
        for (declaration, count) in declared {
            if written.len() >= 2 {
                break;
            }
            if !written.iter().any(|(d, _)| d == declaration) {
                let whose = if *count == 1 { "the one declaration".to_string() } else { format!("the {count} declarations'") };
                written.push((declaration.clone(), whose));
            }
        }
        let mut choices: Vec<Choice> = written.into_iter().map(|(declaration, whose)| Choice::Declare { declaration, whose }).collect();
        let repeat = match choices.first() {
            Some(Choice::Declare { declaration, .. }) => self.in_headers.contains(declaration) || object_declares(object, declaration),
            _ => true,
        };
        choices.insert(if repeat { 0 } else { choices.len().min(1) }, Choice::Remove);
        choices
    }
}

/// Whether a statement of `object` declares or defines exactly `declaration`.
fn object_declares(object: &str, declaration: &str) -> bool {
    text::statements(object).iter().any(|statement| statement.declaration() == *declaration || statement.squashed() == *declaration)
}

/// Bounds how many compilers run at once across every object.
struct Gate {
    free: Mutex<usize>,
    wake: Condvar,
}

impl Gate {
    fn new(slots: usize) -> Gate {
        Gate { free: Mutex::new(slots.max(1)), wake: Condvar::new() }
    }

    fn hold<T>(&self, work: impl FnOnce() -> T) -> T {
        {
            let mut free = self.free.lock().unwrap_or_else(|e| e.into_inner());
            while *free == 0 {
                free = self.wake.wait(free).unwrap_or_else(|e| e.into_inner());
            }
            *free -= 1;
        }
        let result = work();
        *self.free.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        self.wake.notify_one();
        result
    }
}

/// An object judged by the build's compiler against the build's assembly,
/// and by `make modern`'s compiler for errors.
struct Built<'a> {
    root: &'a Path,
    stem: String,
    compiler: views::Recipes,
    /// The build's assembly of the object, each edition's, as it reads.
    reference: Vec<String>,
    gate: &'a Gate,
}

/// A function's instructions: its lines that are no label or directive.
fn instructions(piece: &assembly::Piece) -> Vec<&str> {
    piece.lines.iter().map(String::as_str).filter(|line| !line.ends_with(':') && !line.starts_with('.')).collect()
}

/// How many instructions of two listings differ: the longer's length less
/// their longest common run.
pub(crate) fn differing(a: &[&str], b: &[&str]) -> usize {
    let mut row = vec![0usize; b.len() + 1];
    for x in a {
        let mut diagonal = 0;
        for (j, y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if x == y { diagonal + 1 } else { above.max(row[j]) };
            diagonal = above;
        }
    }
    a.len().max(b.len()) - row[b.len()]
}

/// What moved in one edition, each with its instructions differing and had.
pub(crate) fn moved_by(reference: &[assembly::Piece], now: &[assembly::Piece]) -> Vec<(String, usize, usize)> {
    assembly::moved(&[reference.to_vec()], now)
        .into_iter()
        .map(|(_, name)| {
            let plain = name.strip_suffix(" (new)").unwrap_or(&name);
            let theirs = reference.iter().find(|piece| piece.name == plain);
            let ours = now.iter().find(|piece| piece.name == plain);
            match (theirs, ours) {
                (Some(theirs), Some(ours)) => {
                    let (a, b) = (instructions(theirs), instructions(ours));
                    (name.clone(), differing(&a, &b), a.len())
                }
                (Some(theirs), None) => (format!("{name} (gone)"), instructions(theirs).len(), instructions(theirs).len()),
                (None, Some(ours)) => (name.clone(), instructions(ours).len(), 0),
                (None, None) => (name.clone(), 0, 0),
            }
        })
        .collect()
}

/// The first error a compile printed, its scratch file's name read as the object's.
fn first_error(stderr: &str, scratch: &str, object: &str, root: &Path) -> String {
    let placed = Regex::new(r":\d+: ").unwrap();
    let line = stderr.lines().find(|line| !line.contains("warning:") && placed.is_match(line)).or_else(|| stderr.lines().next()).unwrap_or("");
    // Every path from the checkout, a header the build writes too.
    let line = line.replace(scratch, object).replace(&format!("{}/", root.display()), "");
    line.split_once("/out/").map_or(line.clone(), |(_, rest)| format!("out/{rest}"))
}

impl Judge for Built<'_> {
    fn judge(&self, text: &str, renamed: &[(String, String)]) -> Result<Option<Failure>, String> {
        // Compiled in a private folder, as a file in the object's folder.
        let scratch = views::Scratch::new(self.root, &format!("{}.c", self.stem))?;
        scratch.write(text)?;
        let editions = self.compiler.editions();
        let object = format!("{}.c", self.stem);
        let check = |index: usize, compiled: views::Compiled| -> Option<Failure> {
            let edition = editions[index].clone();
            if !compiled.ok {
                let error = first_error(&compiled.stderr, &scratch.name(), &object, self.root);
                return Some(Failure { edition, error: Some(error), moved: Vec::new() });
            }
            let reference = assembly::pieces(&assembly::renamed(&self.reference[index], renamed));
            let moved = moved_by(&reference, &assembly::pieces(&compiled.output));
            (!moved.is_empty()).then_some(Failure { edition, error: None, moved })
        };
        // The first edition alone, then the rest together: most tries that
        // fail are seen failing in the first.
        let first = self.gate.hold(|| self.compiler.compile(&editions[0], &scratch.path, Mode::Assembly))?;
        if let Some(failure) = check(0, first) {
            return Ok(Some(failure));
        }
        let rest: Vec<Result<views::Compiled, String>> = std::thread::scope(|scope| {
            let handles: Vec<_> = editions[1..].iter().map(|edition| scope.spawn(|| self.gate.hold(|| self.compiler.compile(edition, &scratch.path, Mode::Assembly)))).collect();
            handles.into_iter().map(|handle| handle.join().unwrap_or_else(|_| Err("a compile panicked".into()))).collect()
        });
        for (index, compiled) in rest.into_iter().enumerate() {
            if let Some(failure) = check(index + 1, compiled?) {
                return Ok(Some(failure));
            }
        }
        // As make modern compiles it: a current GCC must take it too.
        for edition in &editions {
            let Some(compiled) = self.gate.hold(|| self.compiler.modern(edition, &scratch.path))? else { continue };
            if !compiled.ok {
                let error = first_error(&compiled.stderr, &scratch.name(), &object, self.root);
                return Ok(Some(Failure { edition: format!("{edition} (make modern)"), error: Some(error), moved: Vec::new() }));
            }
        }
        Ok(None)
    }
}

#[derive(Debug)]
struct Request {
    game: String,
    dry_run: bool,
    /// Whether make modern judges too (each try, and the game once written).
    modern: bool,
    objects: Vec<String>,
}

fn parse(arguments: &[String]) -> Result<Request, String> {
    let mut game = None;
    let mut dry_run = false;
    let mut modern = true;
    let mut objects = Vec::new();
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--game" => game = Some(words.next().filter(|g| !g.starts_with('-')).ok_or_else(|| USAGE.to_string())?.clone()),
            "--dry-run" => dry_run = true,
            "--no-modern" => modern = false,
            other if other.starts_with('-') => return Err(format!("unknown settle option: {other}\n{USAGE}")),
            other => objects.push(other.strip_suffix(".c").unwrap_or(other).to_string()),
        }
    }
    let game = game.ok_or_else(|| USAGE.to_string())?;
    Ok(Request { game, dry_run, modern, objects })
}

/// Every `.c` and `.h` under a folder, by its path from the root.
fn sources(root: &Path, folder: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for entry in walkdir::WalkDir::new(root.join(folder)).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        let named = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if named.starts_with('.') || !(named.ends_with(".c") || named.ends_with(".h")) || !entry.file_type().is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned();
        found.push((relative, text));
    }
    found.sort();
    found
}

/// An edition and the commands that build an object in it.
type EditionRecipe = (String, Vec<Vec<String>>);

/// One object to settle.
struct Job {
    stem: String,
    text: String,
    views: Vec<View>,
    units: Vec<Unit>,
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    if arguments.iter().any(|a| matches!(a.as_str(), "-h" | "--help")) {
        println!("{USAGE}");
        return Ok(());
    }
    let request = parse(arguments)?;
    let root = crate::compile::root();
    let _lock = if request.dry_run { None } else { Some(super::Lock::take(root)?) };
    let started = std::time::Instant::now();
    let all = sources(root, &format!("games/{}", request.game));
    let mut names: BTreeSet<String> = BTreeSet::new();
    let mut jobs: Vec<Job> = Vec::new();
    // Each object as it was read, for the journal that undoes the settle.
    let mut files = super::Files::new(root);
    for (path, text) in &all {
        let Some(stem) = path.strip_suffix(".c") else { continue };
        if !request.objects.is_empty() && !request.objects.iter().any(|o| o == stem) {
            continue;
        }
        let views: Vec<View> = text.lines().filter_map(marker).collect();
        if views.is_empty() {
            continue;
        }
        names.extend(views.iter().filter(|v| matches!(v.kind, "routine" | "variable")).map(|v| v.name.clone()));
        let text = files.read(path)?;
        jobs.push(Job { stem: stem.to_string(), text, views, units: Vec::new() });
    }
    if jobs.is_empty() {
        return Err(format!("no object of games/{} holds a view", request.game));
    }
    let editions = crate::alchemy::built::editions(std::slice::from_ref(&request.game))?;
    // The headers the build writes (the message ids), as the first edition has them.
    let mut read = all.clone();
    if let Some(&first) = editions.first() {
        let out = crate::alchemy::built::output(root, first);
        let folder = out.strip_prefix(root).unwrap_or(&out).to_string_lossy().into_owned();
        read.extend(sources(root, &folder).into_iter().filter(|(path, _)| path.ends_with(".h")));
    }
    let declarations = Declarations::read(&read, &names);
    for job in &mut jobs {
        let lines: Vec<&str> = job.text.lines().collect();
        let text = job.text.clone();
        let choices = |view: &View| match view.kind {
            "routine" | "variable" => declarations.choices(&view.name, &text),
            "tag" => {
                let other = body_of(&text, &view.name).map(|_| text.as_str()).or_else(|| all.iter().find(|(path, source)| path.ends_with(".h") && body_of(source, &view.name).is_some()).map(|(_, source)| source.as_str()));
                match other.and_then(|other| member_renames(&text, view, other)) {
                    Some(renames) => vec![Choice::Members { renames }, Choice::Remove],
                    None => vec![Choice::Remove],
                }
            }
            _ => vec![Choice::Remove],
        };
        job.units = units(&job.views, &lines, &choices);
    }
    // The largest first, so the longest is not left for last.
    jobs.sort_by_key(|job| std::cmp::Reverse(job.text.len() * job.units.len()));
    let total: usize = jobs.iter().map(|job| job.views.len()).sum();
    println!("settle: {total} views in {} objects", jobs.len());

    // Each edition's recipes from one make -n (the build's and make
    // modern's), and the build's assembly.
    let mut recipes: BTreeMap<String, Vec<EditionRecipe>> = BTreeMap::new();
    let mut objects_of: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut moderns: BTreeMap<String, Vec<views::ModernRecipe>> = BTreeMap::new();
    let mut reference: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for &id in &editions {
        let linked = crate::alchemy::link::linked(root, id)?;
        let target = crate::alchemy::targets::target_for(id);
        let linking: Vec<&Job> = jobs.iter().filter(|job| linked.links(&job.stem)).collect();
        if linking.is_empty() {
            continue;
        }
        let mut wanted: Vec<(String, String)> = linking.iter().map(|job| (format!("{}.c", job.stem), format!("{}/obj/{}.o", target.output_dir, job.stem))).collect();
        wanted.extend(linking.iter().map(|job| (format!("{}.c", job.stem), format!("{}/modern/{}.o", target.output_dir, job.stem))));
        let mut found = crate::gba::recipe::recipes(root, Path::new(target.output_dir), &[format!("EDITION={id}")], &wanted)?;
        let modern_found = found.split_off(linking.len());
        for (((source, object), recipe), ((_, modern_object), modern_recipe)) in wanted.iter().zip(found).zip(wanted[linking.len()..].iter().zip(modern_found)) {
            let stem = source.strip_suffix(".c").unwrap_or(source).to_string();
            let recipe = recipe.map_err(|why| format!("{source} in {id}: {why}"))?;
            let modern_recipe = modern_recipe.map_err(|why| format!("{source} in {id} (make modern): {why}"))?;
            let built = crate::alchemy::built::output(root, id).join("obj").join(format!("{stem}.s"));
            let modified = |path: &Path| std::fs::metadata(path).and_then(|meta| meta.modified()).ok();
            if modified(&built).is_none() || modified(&root.join(source)) > modified(&built) {
                return Err(format!("{} is missing or older than {source}: make {id} first", built.display()));
            }
            recipes.entry(stem.clone()).or_default().push((id.as_str().to_string(), recipe));
            objects_of.entry(stem.clone()).or_default().push(object.clone());
            moderns.entry(stem.clone()).or_default().push((modern_recipe, modern_object.clone()));
            reference.entry(stem).or_default().push(crate::alchemy::built::read_text(&built)?);
        }
    }

    let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
    let gate = Gate::new(workers);
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, Result<Solved, String>)>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..workers.min(jobs.len()) {
            scope.spawn(|| loop {
                let at = next.fetch_add(1, Ordering::Relaxed);
                let Some(job) = jobs.get(at) else { break };
                let begun = std::time::Instant::now();
                let result = (|| -> Result<Solved, String> {
                    let recipe = recipes.get(&job.stem).cloned().ok_or_else(|| format!("{} is linked by no edition", job.stem))?;
                    let compiler = views::Recipes::of(root, &job.stem, recipe, objects_of.get(&job.stem).cloned().unwrap_or_default())?;
                    let compiler = if request.modern { compiler.with_modern(moderns.get(&job.stem).cloned().unwrap_or_default()) } else { compiler };
                    let built = Built { root, stem: job.stem.clone(), compiler, reference: reference.get(&job.stem).cloned().unwrap_or_default(), gate: &gate };
                    solve(&job.text, &job.views, &job.units, &built)
                })();
                match &result {
                    Ok(solved) => println!("settle: {}.c: {} of {} views deleted ({:.0} s)", job.stem, solved.deleted.len(), job.views.len(), begun.elapsed().as_secs_f64()),
                    Err(why) => println!("settle: {}.c not settled: {why}", job.stem),
                }
                results.lock().unwrap_or_else(|e| e.into_inner()).push((at, result));
            });
        }
    });
    let mut results = results.into_inner().unwrap_or_else(|e| e.into_inner());
    results.sort_by_key(|(at, _)| *at);

    // The report, by symbol.
    #[derive(Default)]
    struct Symbol {
        deleted: usize,
        kept: Vec<String>,
    }
    let mut symbols: BTreeMap<String, Symbol> = BTreeMap::new();
    let (mut deleted, mut kept, mut written) = (0, 0, 0);
    for (at, result) in &results {
        let job = &jobs[*at];
        let Ok(solved) = result else {
            for view in &job.views {
                symbols.entry(format!("{} {}", view.kind, view.name)).or_default().kept.push(format!("{}.c ({}): not settled", job.stem, view.suffix));
            }
            kept += job.views.len();
            continue;
        };
        for &view in &solved.deleted {
            symbols.entry(format!("{} {}", job.views[view].kind, job.views[view].name)).or_default().deleted += 1;
        }
        for (view, note, _) in &solved.kept {
            let view = &job.views[*view];
            symbols.entry(format!("{} {}", view.kind, view.name)).or_default().kept.push(format!("{}.c ({}): {note}", job.stem, view.suffix));
        }
        deleted += solved.deleted.len();
        kept += solved.kept.len();
        if solved.text != job.text {
            files.write(&format!("{}.c", job.stem), solved.text.clone());
            written += 1;
        }
    }
    let settled = symbols.values().filter(|s| s.kept.is_empty()).count();
    for (symbol, found) in &symbols {
        if found.kept.is_empty() {
            println!("settled {symbol}: {} views deleted", found.deleted);
        }
    }
    for (symbol, found) in &symbols {
        if !found.kept.is_empty() {
            println!("left {symbol}: {} kept, {} deleted", found.kept.len(), found.deleted);
            for line in &found.kept {
                println!("  {line}");
            }
        }
    }
    let action = if request.dry_run { "would be written" } else { "written" };
    println!(
        "settle: {total} views; {deleted} deleted, {kept} left; {settled} of {} symbols settled; {written} objects {action} ({:.0} s)",
        symbols.len(),
        started.elapsed().as_secs_f64()
    );
    if request.dry_run || written == 0 {
        return Ok(());
    }
    // Written through a journal, as a merge is, so adopt --undo takes it back.
    let journal = files.commit(&format!("adopt settle {}", arguments.join(" ")), &format!("settle-{}", request.game))?;
    println!("settle: journal {journal}; undo with alchemy-devtools adopt --undo {journal}");
    if request.modern {
        super::modern_gate(root, &request.game).map_err(|why| format!("{why}\nthe settle is written; undo it with alchemy-devtools adopt --undo {journal}"))?;
    }
    println!("judge with make compare-editions: settle compiles each object as the build does but does not link");
    Ok(())
}

#[cfg(test)]
mod tests;
