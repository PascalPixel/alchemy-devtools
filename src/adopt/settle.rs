//! `adopt settle`: the views a join with views kept, taken away wherever
//! the code does not need them. Each view is one part's own way of seeing a
//! name the rest of its object sees otherwise (`adopt merge --keep-views`).
//! A routine's or variable's is tried as the definition's declaration (or,
//! where the definition is assembly or not C, as the declaration most files
//! write) and as whatever the object already declares; a tag's, typedef's,
//! enumerator's, static's or macro's as the definition the rest of the
//! object has. The compiler judges: every object holding views, or
//! including a file that holds them (a part, `<object>/<part>.c`, a shared
//! file a scene includes, an internal header), is compiled in every edition
//! with a view gone, and each function held against the build's own
//! assembly of that object (`.L` labels renumbered). A view whose removal
//! moves nothing goes, with its marker; one that moves code stays, its
//! marker saying what moved and by how much, so what is left is the list of
//! real disagreements.
//!
//! An object's views are first tried all at once, then by halves, so an
//! object whose views all go costs one compile an edition; a symbol that
//! moves code is tried view by view. Objects settle in parallel; objects
//! that include one file holding views settle as one.

mod bundle;

use super::views::{self, assembly, text, Compiler, Mode};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

const USAGE: &str = r#"usage: alchemy-devtools adopt settle --game GAME [--dry-run] [--no-modern] [FILE.c...]
Takes away the views adopt merge --keep-views wrote, wherever the code does
not need them. Every view marker of GAME
  /* VIEW(PART): KIND NAME as THIS; OTHER has THAT. */
on one line or wrapped by clang-format over several (or only those in the
FILE.c files named and the files they include) is settled by symbol, in the
objects that hold it or include its file: a part of a large object
(<object>/<part>.c, included from <object>.c directly or through another
included file), a shared file scenes include, an internal header. A routine
or variable's view is tried as the definition's own declaration, written in
place of the view's bound one under the plain name (where the definition is
assembly or absent, the declaration most files write), then with no
declaration of its own, as the object sees the name elsewhere; a tag,
typedef, enumerator, static or macro's view is tried as the definition the
rest of the object has: the part's own definition deleted and its name
written plainly again in every file of the object that uses it, and an
#undef of a wrapper the including file writes deleted with it. A view whose
definition holds a FAKEMATCH tag is not tried. The compiler judges: each
object that includes a file the try changed is compiled in every edition
that links it by the build's own recipe, from scratch copies of its files,
and every function and datum is held against the build's assembly of that
object (out/EDITION/obj/OBJECT.s, .L labels renumbered), which must be newer
than the object and every file it includes. Objects that include one file
holding views or using a view's name are settled together. Their views are
tried all at once, then by halves; a symbol that moves code is tried view by
view, and each choice in turn. A view that moves nothing is deleted with its
marker; one that moves code stays, its marker ending
  ...; tried as the definition's DECLARATION, FUNCTION moves: N of M instructions in EDITION. */
written as clang-format wraps it, so the formatter leaves it and the next
settle reads it again. Settles run in parallel; the report lists the symbols
settled, the views deleted and the views left, by symbol, with the
functions that moved, then the views tried and deleted in linked objects
and in the files they include.
Each try is also compiled as make modern compiles the object, and must have
no error; once written, make modern-GAME must pass (--no-modern skips both).
Scratch files are written outside the checkout; --dry-run writes nothing.
Otherwise the files are written with a journal under out/adopt/ that
adopt --undo reverses. Run make compare-editions after: settle compiles
each object as the build does but does not link."#;

/// A view as its marker names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct View {
    /// The marker read as one line, trimmed: a marker the formatter wrapped
    /// over several comment lines has them joined by single spaces, their
    /// leading `*` taken off.
    pub marker: String,
    /// The marker as the source writes it, every line with its indent.
    pub written: String,
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

/// A view's marker read from one line, or None for any other line.
pub(crate) fn marker(line: &str) -> Option<View> {
    let written = line;
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
    Some(View { marker: line.to_string(), written: written.to_string(), key: line.to_string(), suffix: suffix.to_string(), kind, name: name.to_string() })
}

/// The longest a marker may run over lines before it is taken for no marker.
const MARKER_LINES: usize = 64;

/// Each view marker of a text with the 0-based lines it covers, first and
/// last: a marker on one line, or one clang-format wrapped over several
/// (` * ` before each line after the first, `*/` maybe on a line of its own).
pub(crate) fn spans(text: &str) -> Vec<(usize, usize, View)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let first = lines[at].trim();
        if !first.strip_prefix("/* ").is_some_and(|rest| rest.starts_with(views::MARKER)) {
            at += 1;
            continue;
        }
        let Some(end) = (at..lines.len().min(at + MARKER_LINES)).find(|&n| lines[n].contains("*/")) else {
            at += 1;
            continue;
        };
        let mut words = vec![first];
        for line in &lines[at + 1..=end] {
            let line = line.trim();
            let line = if line.starts_with("*/") { line } else { line.strip_prefix('*').unwrap_or(line).trim_start() };
            if !line.is_empty() {
                words.push(line);
            }
        }
        match marker(&words.join(" ")) {
            Some(mut view) => {
                view.written = lines[at..=end].join("\n");
                found.push((at, end, view));
                at = end + 1;
            }
            None => at += 1,
        }
    }
    found
}

/// Each view of a text, in order, whatever lines its marker covers.
pub(crate) fn markers(text: &str) -> Vec<View> {
    spans(text).into_iter().map(|(.., view)| view).collect()
}

/// The column clang-format's LLVM style wraps at.
const COLUMNS: usize = 80;

/// A one-line block comment written as clang-format's LLVM style wraps it:
/// filled to 80 columns, each line after the first opened with ` * `, the
/// closing `*/` on the last line where it fits, else on its own.
pub(crate) fn wrapped(comment: &str, indent: &str) -> String {
    let body = comment.trim().strip_prefix("/*").map_or(comment, |body| body.strip_suffix("*/").unwrap_or(body)).trim();
    let width = |text: &str| text.chars().count();
    let mut lines: Vec<String> = Vec::new();
    let mut line = format!("{indent}/*");
    let mut filled = false;
    for word in body.split(' ').filter(|word| !word.is_empty()) {
        if filled && width(&line) + 1 + width(word) > COLUMNS {
            lines.push(std::mem::replace(&mut line, format!("{indent} *")));
        }
        line.push(' ');
        line.push_str(word);
        filled = true;
    }
    if width(&line) + 3 <= COLUMNS {
        line.push_str(" */");
    } else {
        lines.push(std::mem::replace(&mut line, format!("{indent} */")));
    }
    lines.push(line);
    lines.join("\n")
}

/// A view's own line while it is settled: `/* VIEW#N */` for the view at
/// index N, a comment the compiler reads as nothing.
fn key_of(index: usize) -> String {
    format!("/* VIEW#{index} */")
}

/// What stands for each further line of a wrapped marker while it is
/// settled, so every other line keeps its number and a compiler's message
/// names the file's own line.
const CONTINUED: &str = "/* continued VIEW */";

fn continued(line: &str) -> bool {
    line.trim() == CONTINUED
}

/// A text's lines, each with its newline.
fn lines_of(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

fn indent_of(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// The text with each view's marker written as its key on one line, and
/// the views keyed: every view found by its own line, whatever its marker
/// repeats or however many lines it covers.
fn keyed(text: &str, views: &[View]) -> (String, Vec<View>) {
    let mut keyed: Vec<View> = views.to_vec();
    let lines = lines_of(text);
    let starts: BTreeMap<usize, (usize, View)> = spans(text).into_iter().map(|(first, last, view)| (first, (last, view))).collect();
    let mut next = 0;
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < lines.len() {
        match starts.get(&at).filter(|(_, view)| keyed.get(next).is_some_and(|wanted| wanted.marker == view.marker)) {
            Some((last, view)) => {
                let newline = if lines[at].ends_with('\n') { "\n" } else { "" };
                keyed[next].key = key_of(next);
                keyed[next].written = view.written.clone();
                out.push_str(&format!("{}{}{newline}", indent_of(lines[at]), keyed[next].key));
                for line in &lines[at + 1..=*last] {
                    out.push_str(&format!("{}{CONTINUED}{}", indent_of(line), if line.ends_with('\n') { "\n" } else { "" }));
                }
                next += 1;
                at = last + 1;
            }
            None => {
                out.push_str(lines[at]);
                at += 1;
            }
        }
    }
    (out, keyed)
}

/// The text with every key still standing written as its view's marker was.
fn unkeyed(text: &str, views: &[View]) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        if continued(line) {
            continue;
        }
        let (content, newline) = line.strip_suffix('\n').map_or((line, ""), |content| (content, "\n"));
        match views.iter().find(|view| view.key != view.marker && content.trim() == view.key) {
            Some(view) => out.push_str(&format!("{}{newline}", view.written)),
            None => out.push_str(line),
        }
    }
    out
}

/// The text with a view's marker on one line, where it covers several.
fn collapsed(text: &str, view: &View) -> String {
    if text.lines().any(|line| line.trim() == view.key) {
        return text.to_string();
    }
    let lines = lines_of(text);
    let Some((first, last, _)) = spans(text).into_iter().find(|(.., found)| found.marker == view.marker) else { return text.to_string() };
    let newline = if lines[last].ends_with('\n') { "\n" } else { "" };
    let mut out: String = lines[..first].concat();
    out.push_str(&format!("{}{}{newline}", indent_of(lines[first]), view.key));
    out.push_str(&lines[last + 1..].concat());
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

/// Whether a line opens a part of a joined object (`// ---- STEM.c ----`)
/// or a file of an object's files read as one text.
fn separates(line: &str) -> bool {
    line.starts_with("// ---- ") && line.ends_with(".c ----") || line.starts_with(bundle::FILE)
}

/// The lines of the part that holds line `at`: from its separator to the next.
fn region(lines: &[String], at: usize) -> (usize, usize) {
    let start = lines[..=at].iter().rposition(|line| separates(line)).unwrap_or(0);
    let end = lines[at + 1..].iter().position(|line| separates(line)).map_or(lines.len(), |n| at + 1 + n);
    (start, end)
}

/// Every part's lines, in order: from each separator to the next.
fn regions(lines: &[String]) -> Vec<(usize, usize)> {
    let mut starts: Vec<usize> = (0..lines.len()).filter(|&n| n == 0 || separates(&lines[n])).collect();
    starts.dedup();
    starts.iter().enumerate().map(|(n, &start)| (start, starts.get(n + 1).copied().unwrap_or(lines.len()))).collect()
}

/// Whether a line is a view's marker or key, which no rename touches.
fn marks(line: &str) -> bool {
    line.trim_start().strip_prefix("/* ").is_some_and(|rest| rest.starts_with(views::MARKER) || rest.starts_with("VIEW#"))
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

/// The text with one view taken away as `choice` says. The view's own
/// lines are its part's; its private name is written plainly wherever the
/// text uses it, since the parts an object was cut into share their names
/// (a view of `event_commands` binds a name its sibling files call).
pub(crate) fn apply(text: &str, view: &View, choice: &Choice) -> Result<String, String> {
    let text = collapsed(text, view);
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let at = lines.iter().position(|line| line.trim() == view.key).ok_or_else(|| format!("{}'s marker was not found", view.name))?;
    let private = view.private();
    let name = view.name.as_str();
    let pattern = word(&private);
    // The parts that see the private name, by their order.
    let seeing: BTreeSet<usize> = regions(&lines).iter().enumerate().filter(|(_, (start, end))| lines[*start..*end].iter().any(|line| pattern.is_match(line))).map(|(n, _)| n).collect();
    let (start, end) = region(&lines, at);
    let mut part = lines[start..end].to_vec();
    let wrapped = edit(&mut part, at - start, view, choice)?;
    lines.splice(start..end, part);
    if view.kind == "macro" && view.marker.contains(&format!(" as {private};")) {
        remove_defines(&mut lines, &private);
        let undefine = format!("#undef {private}");
        lines.retain(|line| line.trim() != undefine);
    }
    // A wrapper's `#undef` the including file writes after a part goes
    // once nothing defines the name.
    if wrapped && !lines.iter().any(|line| defines(line, name)) {
        let undefine = format!("#undef {name}");
        lines.retain(|line| line.trim() != undefine);
    }
    if renames(&lines, view) {
        for line in lines.iter_mut() {
            if !marks(line) && pattern.is_match(line) {
                *line = pattern.replace_all(line, name).into_owned();
            }
        }
    }
    if let Choice::Members { renames } = choice {
        let map: BTreeMap<&str, &str> = renames.iter().map(|(from, to)| (from.as_str(), to.as_str())).collect();
        let alternatives: Vec<String> = map.keys().map(|from| regex::escape(from)).collect();
        let access = Regex::new(&format!(r"(\.|->)(\s*)({})\b", alternatives.join("|"))).unwrap();
        for (n, (start, end)) in regions(&lines).into_iter().enumerate() {
            if !seeing.contains(&n) {
                continue;
            }
            for line in &mut lines[start..end] {
                if !marks(line) && !line.trim_start().starts_with("//") {
                    *line = access.replace_all(line, |found: &regex::Captures| format!("{}{}{}", &found[1], &found[2], map[&found[3]])).into_owned();
                }
            }
        }
    }
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

/// Whether the view's private name is written plainly now it is taken
/// away: not while another view still binds it (one around each of two
/// includes), whose code keeps calling it by that name; only the last view
/// of it taken away writes the plain name.
fn renames(lines: &[String], view: &View) -> bool {
    if !matches!(view.kind, "routine" | "variable") {
        return true;
    }
    let private = view.private();
    let define = format!("#define {} {private}", view.name);
    let text = joined(lines);
    let still = defined_as(lines, &view.name, &private).is_some() || lines.iter().any(|line| line.trim() == define) || text::statements(&text).iter().any(|s| s.find(&private).is_some() && s.find("__asm__").is_some());
    !still
}

/// The first line of `#define NAME PRIVATE` in `lines`, however many
/// continued lines clang-format wrote it over, with the line after it.
fn defined_as(lines: &[String], name: &str, private: &str) -> Option<(usize, usize)> {
    let wanted = format!("#define {name} {private}");
    let mut at = 0;
    while at < lines.len() {
        let mut end = at;
        let mut logical = String::new();
        loop {
            let line = lines[end].trim_end();
            match line.strip_suffix('\\') {
                Some(line) if end + 1 < lines.len() => {
                    logical.push_str(line);
                    logical.push(' ');
                    end += 1;
                }
                _ => {
                    logical.push_str(line);
                    break;
                }
            }
        }
        if logical.split_whitespace().collect::<Vec<_>>().join(" ") == wanted {
            return Some((at, end + 1));
        }
        at = end + 1;
    }
    None
}

/// Where a view's own lines end in a part, from line `at` (where its marker
/// was): at the next view's marker, or the part's end.
fn own_end(part: &[String], at: usize) -> usize {
    part[at.min(part.len())..].iter().position(|line| line.trim_start().starts_with("/* VIEW")).map_or(part.len(), |n| at + n)
}

/// Removes, in a part's lines from `at` to the next view, `#define NAME
/// PRIVATE` (on one line or continued over several) and the `#undef NAME`
/// after it in the part: a header view's wrapper. True if there was one.
fn unwrap_header(part: &mut Vec<String>, at: usize, name: &str, private: &str) -> bool {
    let end = own_end(part, at);
    let from = at.min(end);
    let Some((first, last)) = defined_as(&part[from..end], name, private) else { return false };
    let found = from + first;
    part.drain(found..from + last);
    let undefine = format!("#undef {name}");
    if let Some(n) = part[found..].iter().position(|line| line.trim() == undefine) {
        part.remove(found + n);
    }
    true
}

/// Takes a view away in its part's lines; true where it wrapped a header.
fn edit(part: &mut Vec<String>, at: usize, view: &View, choice: &Choice) -> Result<bool, String> {
    let private = view.private();
    let name = view.name.as_str();
    part.remove(at);
    while part.get(at).is_some_and(|line| continued(line)) {
        part.remove(at);
    }
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
    Ok(wrapped)
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

/// The text with a kept view's marker ending in what was tried, written as
/// clang-format wraps it, so the formatter leaves it be and the next settle
/// reads it again.
fn annotate(text: &str, view: &View, note: &str) -> String {
    let text = collapsed(text, view);
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
            out.push_str(&wrapped(&rewritten, indent_of(content)));
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
                // Markers one after another, a wrapped one's further lines between.
                Some((at, key)) if *at < line && lines[*at + 1..line].iter().all(|line| continued(line)) && key.starts_with("enumerator ") => key.clone(),
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

/// One object a joined text is compiled as: its stem, the joined files it
/// reaches, its compiler, and the build's assembly of it in each edition.
struct Member {
    stem: String,
    reaches: BTreeSet<String>,
    compiler: views::Recipes,
    reference: Vec<String>,
}

/// A joined text judged by the build's compiler against the build's
/// assembly, as each object that reaches a file the try changed, and by
/// `make modern`'s compiler for errors.
struct Built<'a> {
    root: &'a Path,
    /// The joined files as they were read, each marker written as its key.
    original: Vec<(String, String)>,
    members: Vec<Member>,
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
#[cfg(test)]
fn first_error(stderr: &str, scratch: &str, object: &str, root: &Path) -> String {
    first_error_of(stderr, &[(scratch.to_string(), object.to_string())], root)
}

/// The first error a compile printed, each scratch copy's name (`names`,
/// with the file it stands for) read as its file's.
fn first_error_of(stderr: &str, names: &[(String, String)], root: &Path) -> String {
    let placed = Regex::new(r":\d+: ").unwrap();
    let line = stderr.lines().find(|line| !line.contains("warning:") && placed.is_match(line)).or_else(|| stderr.lines().next()).unwrap_or("");
    let mut line = line.to_string();
    for (copy, file) in names {
        line = line.replace(copy, file);
    }
    // Every path from the checkout, a header the build writes too.
    let line = line.replace(&format!("{}/", root.display()), "");
    line.split_once("/out/").map_or(line.clone(), |(_, rest)| format!("out/{rest}"))
}

impl Judge for Built<'_> {
    fn judge(&self, text: &str, renamed: &[(String, String)]) -> Result<Option<Failure>, String> {
        let files = bundle::split(text);
        let changed: BTreeSet<&str> = files.iter().filter(|(path, now)| self.original.iter().find(|(p, _)| p == path).is_none_or(|(_, was)| was != now)).map(|(path, _)| path.as_str()).collect();
        let several = self.members.len() > 1;
        // Each object that sees a change, compiled from copies of its files.
        let mut copies: Vec<(&Member, bundle::Copies)> = Vec::new();
        for member in self.members.iter().filter(|member| member.reaches.iter().any(|path| changed.contains(path.as_str()))) {
            let object = format!("{}.c", member.stem);
            let own: Vec<(&str, &str)> = files.iter().filter(|(path, _)| member.reaches.contains(path)).map(|(path, text)| (path.as_str(), text.as_str())).collect();
            copies.push((member, bundle::Copies::write(self.root, &object, &own)?));
        }
        let tasks: Vec<(usize, usize)> = copies.iter().enumerate().flat_map(|(n, (member, _))| (0..member.compiler.editions().len()).map(move |e| (n, e))).collect();
        let named = |n: usize, edition: &str| match several {
            true => format!("{edition} ({}.c)", copies[n].0.stem.rsplit('/').next().unwrap_or(&copies[n].0.stem)),
            false => edition.to_string(),
        };
        let check = |(n, e): (usize, usize), compiled: views::Compiled| -> Option<Failure> {
            let (member, copy) = &copies[n];
            let edition = named(n, &member.compiler.editions()[e]);
            if !compiled.ok {
                return Some(Failure { edition, error: Some(first_error_of(&compiled.stderr, &copy.names, self.root)), moved: Vec::new() });
            }
            let reference = assembly::pieces(&assembly::renamed(&member.reference[e], renamed));
            let moved = moved_by(&reference, &assembly::pieces(&compiled.output));
            (!moved.is_empty()).then_some(Failure { edition, error: None, moved })
        };
        let compile = |(n, e): (usize, usize)| {
            let (member, copy) = &copies[n];
            self.gate.hold(|| member.compiler.compile(&member.compiler.editions()[e], &copy.object, Mode::Assembly))
        };
        // The first edition alone, then the rest together: most tries that
        // fail are seen failing in the first.
        let Some(&first) = tasks.first() else { return Ok(None) };
        if let Some(failure) = check(first, compile(first)?) {
            return Ok(Some(failure));
        }
        let rest: Vec<Result<views::Compiled, String>> = std::thread::scope(|scope| {
            let handles: Vec<_> = tasks[1..].iter().map(|&task| scope.spawn(move || compile(task))).collect();
            handles.into_iter().map(|handle| handle.join().unwrap_or_else(|_| Err("a compile panicked".into()))).collect()
        });
        for (&task, compiled) in tasks[1..].iter().zip(rest) {
            if let Some(failure) = check(task, compiled?) {
                return Ok(Some(failure));
            }
        }
        // As make modern compiles it: a current GCC must take it too.
        for &(n, e) in &tasks {
            let (member, copy) = &copies[n];
            let edition = &member.compiler.editions()[e];
            let Some(compiled) = self.gate.hold(|| member.compiler.modern(edition, &copy.object))? else { continue };
            if !compiled.ok {
                let error = first_error_of(&compiled.stderr, &copy.names, self.root);
                return Ok(Some(Failure { edition: format!("{} (make modern)", named(n, edition)), error: Some(error), moved: Vec::new() }));
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

/// One settle: the objects that reach a set of files holding views, read
/// as one text.
struct Job {
    /// Each object compiled, and the joined files it reaches.
    members: Vec<(String, BTreeSet<String>)>,
    /// The joined files as they were read.
    files: Vec<(String, String)>,
    text: String,
    views: Vec<View>,
    /// The file each view is in.
    holders: Vec<String>,
    /// The views asked for, by index; the rest stay as they are.
    selected: Vec<usize>,
    units: Vec<Unit>,
}

impl Job {
    /// How the report names it: its first object, and how many more.
    fn name(&self) -> String {
        let first = self.members.first().map_or("", |(stem, _)| stem.as_str());
        match self.members.len() {
            0 | 1 => format!("{first}.c"),
            n => format!("{first}.c and {} more objects", n - 1),
        }
    }
}

/// Which of `count` items are joined, by the pairs given: each item's
/// group's first item.
fn grouped(count: usize, pairs: &[(usize, usize)]) -> Vec<usize> {
    let mut leader: Vec<usize> = (0..count).collect();
    fn find(leader: &mut [usize], at: usize) -> usize {
        let mut at = at;
        while leader[at] != at {
            leader[at] = leader[leader[at]];
            at = leader[at];
        }
        at
    }
    for &(a, b) in pairs {
        let (a, b) = (find(&mut leader, a), find(&mut leader, b));
        leader[a.max(b)] = a.min(b);
    }
    (0..count).map(|n| find(&mut leader, n)).collect()
}

/// A regex matching any of `words` as a word, or None for none.
fn any_word(words: &BTreeSet<String>) -> Option<Regex> {
    (!words.is_empty()).then(|| Regex::new(&format!(r"\b(?:{})\b", words.iter().map(|w| regex::escape(w)).collect::<Vec<_>>().join("|"))).unwrap())
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
    let texts: BTreeMap<&str, &str> = all.iter().map(|(path, text)| (path.as_str(), text.as_str())).collect();
    let editions = crate::alchemy::built::editions(std::slice::from_ref(&request.game))?;
    let mut linked = Vec::new();
    for &id in &editions {
        linked.push((id, crate::alchemy::link::linked(root, id)?));
    }

    // The files holding views, and those a settle may change: a file that
    // uses a view's private name or undefines a view's name.
    let held: BTreeMap<&str, Vec<View>> = all.iter().map(|(path, text)| (path.as_str(), markers(text))).filter(|(_, views)| !views.is_empty()).collect();
    if held.is_empty() {
        return Err(format!("no file of games/{} holds a view", request.game));
    }
    let privates: BTreeSet<String> = held.values().flatten().map(View::private).collect();
    let plain: BTreeSet<String> = held.values().flatten().map(|view| view.name.clone()).collect();
    let private_word = any_word(&privates);
    let undefines = any_word(&plain).map(|names| Regex::new(&format!(r"(?m)^\s*#\s*undef\s+{}\s*$", names.as_str())).unwrap());
    let relevant: BTreeSet<&str> = all
        .iter()
        .filter(|(path, text)| held.contains_key(path.as_str()) || private_word.as_ref().is_some_and(|r| r.is_match(text)) || undefines.as_ref().is_some_and(|r| r.is_match(text)))
        .map(|(path, _)| path.as_str())
        .collect();

    // Every object some edition links, with the files of its tree it needs.
    let read = |path: &str| texts.get(path).map(|text| text.to_string());
    let mut stems: BTreeSet<&str> = BTreeSet::new();
    for (_, linked) in &linked {
        stems.extend(linked.all().into_iter().filter(|stem| texts.contains_key(format!("{stem}.c").as_str())));
    }
    let mut objects: Vec<(String, BTreeSet<String>, BTreeSet<String>)> = Vec::new();
    for stem in stems {
        let object = format!("{stem}.c");
        let parents = bundle::tree(root, &object, &read);
        if !parents.keys().any(|path| held.contains_key(path.as_str())) {
            continue;
        }
        let needed = bundle::needed(&object, &parents, &|path| relevant.contains(path));
        let reached: BTreeSet<String> = parents.into_keys().collect();
        objects.push((stem.to_string(), needed, reached));
    }
    // Objects that need one changeable file are settled together.
    let mut first_of: BTreeMap<&str, usize> = BTreeMap::new();
    let mut pairs = Vec::new();
    for (n, (_, needed, _)) in objects.iter().enumerate() {
        for path in needed.iter().filter(|path| relevant.contains(path.as_str())) {
            match first_of.get(path.as_str()) {
                Some(&other) => pairs.push((other, n)),
                None => {
                    first_of.insert(path, n);
                }
            }
        }
    }
    let leaders = grouped(objects.len(), &pairs);
    // The views the request names: those of a file named, or of a file one
    // it names includes.
    let named: Option<BTreeSet<String>> = (!request.objects.is_empty()).then(|| {
        let mut named = BTreeSet::new();
        for asked in &request.objects {
            for path in [format!("{asked}.c"), asked.clone()].into_iter().filter(|path| texts.contains_key(path.as_str())) {
                named.extend(bundle::tree(root, &path, &read).into_keys());
            }
        }
        named
    });
    let held_views = held.values().flatten().count();
    let held_by_objects: usize = held.iter().filter(|(path, _)| path.strip_suffix(".c").is_some_and(|stem| linked.iter().any(|(_, l)| l.links(stem)))).map(|(_, views)| views.len()).sum();
    let mut reached_views = 0;

    let mut names: BTreeSet<String> = BTreeSet::new();
    let mut jobs: Vec<Job> = Vec::new();
    // Each file as it was read, for the journal that undoes the settle.
    let mut files = super::Files::new(root);
    for leader in leaders.iter().copied().collect::<BTreeSet<usize>>() {
        let group: Vec<usize> = (0..objects.len()).filter(|&n| leaders[n] == leader).collect();
        // The objects first, then the files they include.
        let mut paths: Vec<String> = group.iter().map(|&n| format!("{}.c", objects[n].0)).collect();
        let mut rest: BTreeSet<String> = group.iter().flat_map(|&n| objects[n].1.iter().cloned()).collect();
        rest.retain(|path| !paths.contains(path));
        paths.extend(rest);
        let mut joined_files = Vec::new();
        let mut views = Vec::new();
        let mut holders = Vec::new();
        for path in &paths {
            let text = files.read(path)?;
            for view in markers(&text) {
                views.push(view);
                holders.push(path.clone());
            }
            joined_files.push((path.clone(), text));
        }
        reached_views += views.len();
        let selected: Vec<bool> = holders.iter().map(|holder| named.as_ref().is_none_or(|named| named.contains(holder))).collect();
        if !selected.contains(&true) {
            continue;
        }
        names.extend(views.iter().zip(&selected).filter(|(v, &s)| s && matches!(v.kind, "routine" | "variable")).map(|(v, _)| v.name.clone()));
        let members = group.iter().map(|&n| (objects[n].0.clone(), objects[n].1.clone())).collect();
        let text = bundle::join(&joined_files);
        let selected = selected.iter().enumerate().filter(|(_, &s)| s).map(|(n, _)| n).collect();
        jobs.push(Job { members, files: joined_files, text, views, holders, selected, units: Vec::new() });
    }
    if jobs.is_empty() {
        return Err(format!("no object of games/{} reaches a view it was asked for", request.game));
    }
    // The headers the build writes (the message ids), as the first edition has them.
    let mut read_all = all.clone();
    if let Some(&first) = editions.first() {
        let out = crate::alchemy::built::output(root, first);
        let folder = out.strip_prefix(root).unwrap_or(&out).to_string_lossy().into_owned();
        read_all.extend(sources(root, &folder).into_iter().filter(|(path, _)| path.ends_with(".h")));
    }
    let declarations = Declarations::read(&read_all, &names);
    for job in &mut jobs {
        let selected = job.selected.clone();
        let (keyed_text, keyed_views) = keyed(&job.text, &job.views);
        let lines: Vec<&str> = keyed_text.lines().collect();
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
        let chosen: Vec<View> = selected.iter().map(|&n| keyed_views[n].clone()).collect();
        job.units = units(&chosen, &lines, &choices)
            .into_iter()
            .map(|unit| Unit { views: unit.views.into_iter().map(|n| selected[n]).collect(), choices: unit.choices })
            .collect();
    }
    // The largest first, so the longest is not left for last.
    jobs.sort_by_key(|job| std::cmp::Reverse(job.text.len() * job.units.len() * job.members.len()));
    let total: usize = jobs.iter().map(|job| job.units.iter().map(|unit| unit.views.len()).sum::<usize>()).sum();
    println!(
        "settle: {total} views in {} settles of {} objects; games/{} holds {held_views}, {held_by_objects} in linked objects, {} in files they include, {} reached by no linked object",
        jobs.len(),
        jobs.iter().map(|job| job.members.len()).sum::<usize>(),
        request.game,
        held_views - held_by_objects,
        held_views.saturating_sub(reached_views)
    );

    // Each edition's recipes from one make -n (the build's and make
    // modern's), and the build's assembly.
    let mut recipes: BTreeMap<String, Vec<EditionRecipe>> = BTreeMap::new();
    let mut objects_of: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut moderns: BTreeMap<String, Vec<views::ModernRecipe>> = BTreeMap::new();
    let mut reference: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let members: BTreeMap<&str, &BTreeSet<String>> = jobs.iter().flat_map(|job| job.members.iter().map(|(stem, reaches)| (stem.as_str(), reaches))).collect();
    let reached_of: BTreeMap<&str, &BTreeSet<String>> = objects.iter().map(|(stem, _, reached)| (stem.as_str(), reached)).collect();
    for (id, linked) in &linked {
        let target = crate::alchemy::targets::target_for(*id);
        let linking: Vec<&str> = members.keys().copied().filter(|stem| linked.links(stem)).collect();
        if linking.is_empty() {
            continue;
        }
        let mut wanted: Vec<(String, String)> = linking.iter().map(|stem| (format!("{stem}.c"), format!("{}/obj/{stem}.o", target.output_dir))).collect();
        wanted.extend(linking.iter().map(|stem| (format!("{stem}.c"), format!("{}/modern/{stem}.o", target.output_dir))));
        let mut found = crate::gba::recipe::recipes(root, Path::new(target.output_dir), &[format!("EDITION={id}")], &wanted)?;
        let modern_found = found.split_off(linking.len());
        for (((source, object), recipe), ((_, modern_object), modern_recipe)) in wanted.iter().zip(found).zip(wanted[linking.len()..].iter().zip(modern_found)) {
            let stem = source.strip_suffix(".c").unwrap_or(source).to_string();
            let recipe = recipe.map_err(|why| format!("{source} in {id}: {why}"))?;
            let modern_recipe = modern_recipe.map_err(|why| format!("{source} in {id} (make modern): {why}"))?;
            let built = crate::alchemy::built::output(root, *id).join("obj").join(format!("{stem}.s"));
            let modified = |path: &Path| std::fs::metadata(path).and_then(|meta| meta.modified()).ok();
            let made = modified(&built);
            // Every file of games/ the object reaches, its parts too.
            let newer = reached_of.get(stem.as_str()).into_iter().flat_map(|reached| reached.iter()).find(|path| made.is_none() || modified(&root.join(path)) > made);
            if let Some(newer) = newer {
                return Err(format!("{} is missing or older than {newer}: make {id} first", built.display()));
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
                    let mut members = Vec::new();
                    for (stem, reaches) in &job.members {
                        let recipe = recipes.get(stem).cloned().ok_or_else(|| format!("{stem} is linked by no edition"))?;
                        let compiler = views::Recipes::of(root, stem, recipe, objects_of.get(stem).cloned().unwrap_or_default())?;
                        let compiler = if request.modern { compiler.with_modern(moderns.get(stem).cloned().unwrap_or_default()) } else { compiler };
                        members.push(Member { stem: stem.clone(), reaches: reaches.clone(), compiler, reference: reference.get(stem).cloned().unwrap_or_default() });
                    }
                    // As solve compiles it: each marker written as its key.
                    let original = bundle::split(&keyed(&job.text, &job.views).0);
                    let built = Built { root, original, members, gate: &gate };
                    solve(&job.text, &job.views, &job.units, &built)
                })();
                let asked: usize = job.units.iter().map(|unit| unit.views.len()).sum();
                match &result {
                    Ok(solved) => println!("settle: {}: {} of {asked} views deleted ({:.0} s)", job.name(), solved.deleted.len(), begun.elapsed().as_secs_f64()),
                    Err(why) => println!("settle: {} not settled: {why}", job.name()),
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
    /// Views asked for, tried and deleted, in linked objects and in the
    /// files they include.
    #[derive(Default)]
    struct Count {
        views: usize,
        tried: usize,
        deleted: usize,
    }
    let mut symbols: BTreeMap<String, Symbol> = BTreeMap::new();
    let (mut deleted, mut kept, mut written) = (0, 0, 0);
    let mut counts: [Count; 2] = Default::default();
    for (at, result) in &results {
        let job = &jobs[*at];
        let class = |view: usize| usize::from(!job.members.iter().any(|(stem, _)| job.holders[view] == format!("{stem}.c")));
        let asked: Vec<usize> = job.units.iter().flat_map(|unit| unit.views.iter().copied()).collect();
        for &view in &asked {
            counts[class(view)].views += 1;
        }
        let Ok(solved) = result else {
            for &view in &asked {
                let view_of = &job.views[view];
                symbols.entry(format!("{} {}", view_of.kind, view_of.name)).or_default().kept.push(format!("{} ({}): not settled", job.holders[view], view_of.suffix));
            }
            kept += asked.len();
            continue;
        };
        for &view in &solved.deleted {
            symbols.entry(format!("{} {}", job.views[view].kind, job.views[view].name)).or_default().deleted += 1;
            counts[class(view)].tried += 1;
            counts[class(view)].deleted += 1;
        }
        for (view, note, _) in &solved.kept {
            if !note.starts_with("not tried: ") {
                counts[class(*view)].tried += 1;
            }
            let holder = &job.holders[*view];
            let view = &job.views[*view];
            symbols.entry(format!("{} {}", view.kind, view.name)).or_default().kept.push(format!("{holder} ({}): {note}", view.suffix));
        }
        deleted += solved.deleted.len();
        kept += solved.kept.len();
        for ((path, now), (_, was)) in bundle::split(&solved.text).into_iter().zip(&job.files) {
            let now = bundle::like(was, now);
            if now != *was {
                files.write(&path, now);
                written += 1;
            }
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
    for (count, held) in counts.iter().zip(["linked objects", "files they include"]) {
        println!("settle: in {held}, {} views: {} tried, {} deleted", count.views, count.tried, count.deleted);
    }
    let action = if request.dry_run { "would be written" } else { "written" };
    println!(
        "settle: {total} views; {deleted} deleted, {kept} left; {settled} of {} symbols settled; {written} files {action} ({:.0} s)",
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
