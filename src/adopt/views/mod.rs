//! `adopt merge --keep-views`: a run's sources joined into one without
//! settling a single disagreement between them. Where two parts see a name
//! two ways, the part that disagrees keeps its own view of it: a routine or
//! variable under a private name bound to the same symbol
//! (`extern int Helper__s0b84(struct Thing *) __asm__("Helper");`), a tag,
//! typedef, enumerator, static or macro under its own suffixed name. A
//! part's macros are undefined where it ends, and an identical repeat is
//! deleted. Each view is marked on the line above it:
//!
//! `/* VIEW(<part>): <kind> <name> as <this view>; <the other> has <its view>. */`
//!
//! The compiler is the judge throughout. The join is compiled in every
//! edition and each message that names a disagreement becomes a view, until
//! it compiles clean; then every function is held against the build's own
//! assembly of the file it came from. A function that moved has the name
//! whose meaning changed for it found (its routines' declarations as
//! `-aux-info` reports them, its macros as `-dM` lists them) and kept as a
//! view too. A part that cannot be reconciled is left out, and its run
//! becomes the runs on either side of it.

pub(crate) mod assembly;
mod diagnostics;
pub(crate) mod text;

use diagnostics::{Declared, Message, Preprocessed};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use text::{insert_before, rename, statement_at, statements, Space};

/// The kinds of view, as the marker names them and the counts list them.
pub(crate) const KINDS: [&str; 7] = ["routine", "variable", "tag", "typedef", "enumerator", "static", "macro"];

/// The marker's opening, as `check tags` and the humanity count find it.
pub(crate) const MARKER: &str = "VIEW(";

/// A comment read as a view's marker: its kind, or why it is not in the
/// marker's form; None for a comment that is no marker.
pub(crate) fn view_kind(comment: &str) -> Option<Result<&'static str, String>> {
    let body = comment.strip_prefix("/*").map_or(comment, |body| body.strip_suffix("*/").unwrap_or(body));
    let body = body.strip_prefix("//").unwrap_or(body).trim();
    let rest = body.strip_prefix(MARKER)?;
    let form = "/* VIEW(<part>): <kind> <name> as <this view>; <the other> has <its view>. */";
    let Some((part, rest)) = rest.split_once("): ") else { return Some(Err(format!("not the view marker's form: {form}"))) };
    if part.is_empty() || !part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Some(Err(format!("{part} is not a part's file stem")));
    }
    let word = rest.split_whitespace().next().unwrap_or("");
    let Some(kind) = KINDS.iter().find(|kind| **kind == word) else { return Some(Err(format!("unknown view kind {word}: {}", KINDS.join(", ")))) };
    if !rest.contains(" as ") || !rest.contains(" has ") || !rest.ends_with('.') {
        return Some(Err(format!("not the view marker's form: {form}")));
    }
    Some(Ok(kind))
}

/// How many times a join is compiled before it is given up on.
const ROUNDS: usize = 2000;

/// One source of a run as it is joined: its object stem, the suffix its
/// views take (its file's old stem) and its text as it now reads.
#[derive(Clone, Debug)]
pub(crate) struct Part {
    pub stem: String,
    pub suffix: String,
    pub text: String,
    /// The names this part renamed, so a declaration written for it later
    /// says its types the way its text now does.
    renames: Vec<(String, String, Space)>,
    /// The private names its statics took: their symbols change, their code not.
    statics: Vec<String>,
    /// Lines after its macros are undefined, giving later parts back a
    /// macro this part saw its own way.
    restore: Vec<String>,
}

impl Part {
    pub fn new(stem: &str, text: String) -> Part {
        let suffix = stem.rsplit('/').next().unwrap_or(stem).chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
        Part { stem: stem.to_string(), suffix, text, renames: Vec::new(), statics: Vec::new(), restore: Vec::new() }
    }

    fn private(&self, name: &str) -> String {
        format!("{name}__{}", self.suffix)
    }
}

/// What a compile is asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Assembly,
    Preprocess,
    Macros,
    Declarations,
}

/// What a compile gave: whether it succeeded, what it wrote (the assembly,
/// the preprocessed text, the macro listing or the `-aux-info` file) and
/// its messages.
pub(crate) struct Compiled {
    pub ok: bool,
    pub output: String,
    pub stderr: String,
}

/// The build's compiler as a run's object takes it, in each edition.
pub(crate) trait Compiler: Sync {
    fn editions(&self) -> Vec<String>;
    /// `source` compiled in `edition` by the run's own recipe.
    fn compile(&self, edition: &str, source: &Path, mode: Mode) -> Result<Compiled, String>;
    /// The assembly the build made of `stem` in `edition`.
    fn built(&self, edition: &str, stem: &str) -> Result<String, String>;
    /// The checkout, which the compiler's own names for headers are under.
    fn root(&self) -> &Path;
}

/// A run settled: its parts' stems, its joined text and its views by kind.
#[derive(Debug)]
pub(crate) struct Settled {
    pub stems: Vec<String>,
    pub text: String,
    pub views: BTreeMap<&'static str, usize>,
}

/// What a run became: the runs joined, and each part left out with why.
#[derive(Debug, Default)]
pub(crate) struct Reconciled {
    pub joined: Vec<Settled>,
    pub alone: Vec<String>,
    pub left_out: Vec<(String, String)>,
}

/// What makes a group's parts: the object they join into, and their texts.
pub(crate) type Prepare<'a> = dyn Fn(&[(String, String)]) -> (String, Vec<Part>) + 'a;

/// Joins `sources` (stem and text) with views, in order. `prepare` makes a
/// group's parts' texts for the object they join into (its includes reached
/// from its folder), and names that object; a group that keeps a stubborn
/// part out is joined as the groups either side of it.
pub(crate) fn reconcile(sources: &[(String, String)], prepare: &Prepare, compiler: &dyn Compiler) -> Reconciled {
    let mut reconciled = Reconciled::default();
    let mut groups: Vec<Vec<(String, String)>> = vec![sources.to_vec()];
    while !groups.is_empty() {
        let group = groups.remove(0);
        if group.len() < 2 {
            reconciled.alone.extend(group.into_iter().map(|(stem, _)| stem));
            continue;
        }
        let (into, parts) = prepare(&group);
        match settle(parts, &into, compiler) {
            Ok(settled) => reconciled.joined.push(settled),
            Err((index, why)) => {
                // The stubborn part stays its own object; its neighbours
                // either side are joined without it.
                reconciled.left_out.push((group[index].0.clone(), why));
                reconciled.alone.push(group[index].0.clone());
                groups.insert(0, group[index + 1..].to_vec());
                groups.insert(0, group[..index].to_vec());
                groups.retain(|group| !group.is_empty());
            }
        }
    }
    reconciled.alone.sort_by_key(|stem| sources.iter().position(|(s, _)| s == stem));
    reconciled.joined.sort_by_key(|settled| sources.iter().position(|(stem, _)| *stem == settled.stems[0]));
    reconciled
}

/// A file a join is compiled from, beside the object it joins into so its
/// relative includes reach what they did; it goes when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(root: &Path, into: &str) -> Scratch {
        static SERIAL: AtomicUsize = AtomicUsize::new(0);
        let folder = root.join(into).parent().map(Path::to_path_buf).unwrap_or_else(|| root.to_path_buf());
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        Scratch(folder.join(format!(".views-{}-{serial}.c", std::process::id())))
    }

    fn write(&self, text: &str) -> Result<(), String> {
        std::fs::write(&self.0, text).map_err(|error| format!("cannot write {}: {error}", self.0.display()))
    }

    fn name(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Where a declaration is: a line of a part's text, or a line of a header
/// that a part's `#include` (at `include`, a line of its text) entered first.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Site {
    Part { part: usize, line: usize },
    Header { file: String, line: usize, part: usize, include: usize },
}

impl Site {
    fn part(&self) -> usize {
        match self {
            Site::Part { part, .. } | Site::Header { part, .. } => *part,
        }
    }
}

/// The other side of a disagreement, as a marker names it: who has it and
/// how it reads.
#[derive(Clone, Debug)]
struct Other {
    owner: String,
    text: String,
}

/// One change a message asks of a part.
#[derive(Clone, Debug)]
enum Fix {
    /// A routine or variable seen another way: a private name bound to the
    /// same symbol.
    /// `expanded` is how the preprocessor wrote a declaration whose text
    /// never spells the name.
    Alias { name: String, site: Site, kind: &'static str, other: Other, expanded: Option<Read> },
    /// A tag, typedef, enumerator, static or macro defined another way: the
    /// part's own name for it.
    Rename { name: String, site: Site, space: Space, kind: &'static str, other: Other },
    /// A routine the part called with no declaration, now declared: the part
    /// keeps calling it unprototyped.
    Implicit { part: usize, name: String, other: Other },
    /// A routine the part saw declared as `declaration`, now declared
    /// otherwise for it.
    Declared { part: usize, name: String, declaration: String, other: Other },
    /// A macro the part saw as `original` (or not at all), now otherwise.
    Macro { part: usize, name: String, original: Option<String>, joined: Option<String> },
    /// A macro a header the part enters at line `include` defines otherwise
    /// than one before it: undefined before that #include, and the joined
    /// definition (`joined`) given back where the part ends.
    Unmacro { part: usize, include: usize, name: String, joined: String },
    /// An identical repeat of a type, typedef or inline routine: deleted.
    Delete { part: usize, line: usize, name: String, tag: bool },
    /// A header view's bound declaration (at `line`) that cannot be read
    /// before its #include: moved after it.
    After { part: usize, line: usize },
}

impl Fix {
    fn part(&self) -> usize {
        match self {
            Fix::Alias { site, .. } | Fix::Rename { site, .. } => site.part(),
            Fix::Implicit { part, .. } | Fix::Declared { part, .. } | Fix::Macro { part, .. } | Fix::Unmacro { part, .. } | Fix::Delete { part, .. } | Fix::After { part, .. } => *part,
        }
    }
}

/// A run's parts being joined.
struct Join<'a> {
    parts: Vec<Part>,
    compiler: &'a dyn Compiler,
    /// Macros a part may define as its headers already do: not undefined
    /// where it ends, so later parts keep them.
    shared: BTreeMap<String, String>,
    views: BTreeMap<&'static str, usize>,
}

/// Each part's lines in the joined text: (first joined line, part, first
/// part line, count).
type Segments = Vec<(usize, usize, usize, usize)>;

/// A marker's text, kept to one readable line.
fn brief(text: &str) -> String {
    let text = text.replace("*/", "* /").split_whitespace().collect::<Vec<_>>().join(" ");
    match text.char_indices().nth(140) {
        Some((at, _)) => format!("{}...", &text[..at]),
        None => text,
    }
}

fn marker(part: &Part, kind: &str, name: &str, this: &str, other: &Other) -> String {
    format!("/* {MARKER}{}): {kind} {name} as {}; {} has {}. */", part.suffix, brief(this.trim_end_matches(';')), other.owner, brief(other.text.trim_end_matches(';')))
}

impl<'a> Join<'a> {
    /// Where a header view bound a declaration before its #include, at
    /// `line` of a part: that line and the `#undef` after the include.
    fn bound_before(&self, part: usize, line: usize) -> Option<(usize, usize)> {
        let lines: Vec<&str> = self.parts[part].text.lines().collect();
        let declaration = lines.get(line.checked_sub(1)?)?;
        let symbol = declaration.strip_prefix("extern ")?.rsplit_once("__asm__(\"")?.1.strip_suffix("\");")?;
        let define = format!("#define {symbol} ");
        let after = lines[line..].iter().position(|l| l.starts_with(&define))? + line;
        let include = after + 1;
        lines.get(include).filter(|l| l.trim_start().starts_with("#include"))?;
        let undefine = lines[include..].iter().position(|l| *l == format!("#undef {symbol}"))? + include;
        Some((line, undefine + 1))
    }

    /// Every static's private name, which the assembly writes for its own.
    fn statics(&self) -> Vec<String> {
        self.parts.iter().flat_map(|part| part.statics.iter().cloned()).collect()
    }

    fn render(&self) -> (String, Segments) {
        self.render_until(self.parts.len())
    }

    /// The parts before `end` joined, and the part at `end` without what
    /// follows its text: the macros in force where its code is.
    fn render_until(&self, end: usize) -> (String, Segments) {
        let mut out = String::new();
        let mut segments = Vec::new();
        let mut line = 1;
        let push = |out: &mut String, text: &str, line: &mut usize| {
            out.push_str(text);
            if !text.is_empty() && !text.ends_with('\n') {
                out.push('\n');
            }
            *line += text.lines().count();
        };
        for (index, part) in self.parts.iter().enumerate().take(end + 1) {
            // The first part's `/// @file` block stays the file's opening.
            let opening = if index == 0 { part.text.lines().take_while(|l| l.starts_with("///")).count() } else { 0 };
            let start = text::line_start(&part.text, opening + 1);
            if opening > 0 {
                segments.push((line, index, 1, opening));
                push(&mut out, &part.text[..start], &mut line);
            }
            if !out.is_empty() && !out.ends_with("\n\n") {
                push(&mut out, "\n", &mut line);
            }
            push(&mut out, &format!("// ---- {}.c ----\n", part.stem), &mut line);
            let rest = &part.text[start..];
            segments.push((line, index, opening + 1, rest.lines().count()));
            push(&mut out, rest, &mut line);
            if index == end {
                break;
            }
            let mut undefined: Vec<String> = Vec::new();
            for (name, definition, _) in text::macros(&part.text) {
                // A header view's wrapper is undefined after its #include already.
                let wrapper = definition == format!("{name} {}", part.private(&name));
                if !wrapper && self.shared.get(&name) != Some(&definition) && !undefined.contains(&name) {
                    undefined.push(name);
                }
            }
            for name in &undefined {
                push(&mut out, &format!("#undef {name}\n"), &mut line);
            }
            for restored in &part.restore {
                push(&mut out, &format!("{restored}\n"), &mut line);
            }
        }
        (out, segments)
    }

    fn locate(segments: &Segments, line: usize) -> Option<(usize, usize)> {
        segments.iter().find(|(first, _, _, count)| (*first..*first + count).contains(&line)).map(|(first, part, part_first, _)| (*part, part_first + line - first))
    }

    fn site(&self, segments: &Segments, scratch: &str, pre: Option<&Preprocessed>, file: &str, line: usize) -> Option<Site> {
        if file == scratch {
            let (part, line) = Join::locate(segments, line)?;
            return Some(Site::Part { part, line });
        }
        let include = *pre?.entered.get(file)?;
        let (part, include) = Join::locate(segments, include)?;
        Some(Site::Header { file: file.to_string(), line, part, include })
    }

    /// The text a site's line is in.
    fn source(&self, site: &Site) -> Option<String> {
        match site {
            Site::Part { part, .. } => Some(self.parts[*part].text.clone()),
            Site::Header { file, .. } => {
                let path = if Path::new(file).is_absolute() { PathBuf::from(file) } else { self.compiler.root().join(file) };
                std::fs::read_to_string(path).ok()
            }
        }
    }

    fn line(site: &Site) -> usize {
        match site {
            Site::Part { line, .. } | Site::Header { line, .. } => *line,
        }
    }

    /// Who has a site's view, for a marker.
    fn owner(&self, site: &Site, defines: bool) -> String {
        if defines {
            return "the definition".into();
        }
        match site {
            Site::Part { part, .. } => self.parts[*part].suffix.clone(),
            Site::Header { file, .. } => file.rsplit('/').next().unwrap_or(file).to_string(),
        }
    }

    /// What a declaration at a site of `name` is: (squashed text, typedef,
    /// enumerator, static, inline, routine, defines).
    fn read(&self, site: &Site, name: &str) -> Option<Read> {
        let text = self.source(site)?;
        let all = statements(&text);
        let statement = statement_at(&all, Join::line(site), name)?;
        Some(Read::of(statement, name))
    }

    /// A declaration read as `read` reads it, or where its text never spells
    /// the name (a shared file's `void EFFECT_SPAWN(...)` with `EFFECT_SPAWN`
    /// a part's macro), as the preprocessor wrote it at the place the
    /// compiler named.
    fn read_at(&self, site: &Site, name: &str, place: Option<&Message>, pre: Option<&Preprocessed>) -> Option<Read> {
        self.read(site, name).or_else(|| {
            let (place, pre) = (place?, pre?);
            let all = statements(&pre.text);
            let at = |line: usize| pre.origin.get(line.wrapping_sub(1)).is_some_and(|(file, n)| *file == place.file && *n == place.line);
            let statement = all.iter().find(|s| s.find(name).is_some() && (s.first_line..=s.last_line).any(at))?;
            Some(Read::of(statement, name))
        })
    }

    /// The fixes a compile's messages ask for: one a part, up to the first
    /// error no fix answers. An error no fix answers before any that one
    /// does is the part's own: it is given back as `Err`.
    fn plan(&self, messages: &[Message], segments: &Segments, scratch: &str, pre: Option<&Preprocessed>) -> Result<Vec<Fix>, (usize, String)> {
        let mut fixes: Vec<Fix> = Vec::new();
        let mut taken: BTreeSet<usize> = BTreeSet::new();
        let mut at = 0;
        while at < messages.len() {
            let message = &messages[at];
            let previous = messages.get(at + 1).filter(|next| is_previous(&next.text));
            at += if previous.is_some() { 2 } else { 1 };
            if is_previous(&message.text) {
                continue;
            }
            let later = self.site(segments, scratch, pre, &message.file, message.line);
            let earlier = previous.and_then(|previous| self.site(segments, scratch, pre, &previous.file, previous.line));
            match self.answer(message, previous, later.as_ref(), earlier.as_ref(), pre, (segments, scratch)) {
                Answer::Fix(fix) => {
                    if taken.insert(fix.part()) {
                        fixes.push(fix);
                    }
                }
                Answer::Nothing => {}
                Answer::Unknown(why) => {
                    if fixes.is_empty() {
                        let part = later.as_ref().map_or(0, Site::part);
                        let place = later.as_ref().map_or_else(|| format!("{}:{}", message.file, message.line), |site| self.describe(site));
                        return Err((part, format!("{place}: {}{why}", message.text)));
                    }
                    break;
                }
            }
        }
        // A repeat is deleted only while what it repeats stands: where this
        // round also gives the name another meaning (the first of two
        // identical inlines renamed for a header's), the next round judges.
        let renamed: BTreeSet<String> = fixes.iter().filter_map(|fix| match fix {
            Fix::Alias { name, .. } | Fix::Rename { name, .. } => Some(name.clone()),
            _ => None,
        }).collect();
        fixes.retain(|fix| !matches!(fix, Fix::Delete { name, .. } if renamed.contains(name)));
        Ok(fixes)
    }

    fn describe(&self, site: &Site) -> String {
        match site {
            Site::Part { part, line } => format!("{}.c:{line}", self.parts[*part].stem),
            Site::Header { file, line, part, .. } => format!("{file}:{line} (from {}.c)", self.parts[*part].stem),
        }
    }

    /// What one message (and the one naming the earlier place, if any) asks.
    fn answer(&self, message: &Message, previous: Option<&Message>, later: Option<&Site>, earlier: Option<&Site>, pre: Option<&Preprocessed>, joined: (&Segments, &str)) -> Answer {
        let places = Places { later: message, earlier: previous, pre, segments: joined.0, scratch: joined.1 };
        let text = message.text.as_str();
        if let (Some(Site::Part { part, line }), false) = (later, message.warning) {
            if self.bound_before(*part, *line).is_some() {
                return Answer::Fix(Fix::After { part: *part, line: *line });
            }
        }
        let Some(name) = message.name().map(str::to_string) else {
            return if message.warning { Answer::Nothing } else { Answer::Unknown(String::new()) };
        };
        let Some(later) = later else {
            return if message.warning { Answer::Nothing } else { Answer::Unknown(" (in no part)".into()) };
        };
        let is = |prefix: &str| text.starts_with(prefix);
        if is("conflicting types for") || is("redeclaration of `") && !is("redeclaration of `enum") || text.ends_with("redeclared as different kind of symbol") {
            let Some(earlier) = earlier else { return Answer::Unknown(" (no earlier declaration named)".into()) };
            return self.disagree(&name, later, earlier, &places);
        }
        if is("redefinition of `struct ") || is("redefinition of `union ") || is("redeclaration of `enum ") {
            let tag = name.split_whitespace().nth(1).unwrap_or(&name).to_string();
            return self.retag(&tag, later, &places);
        }
        if is("redefinition of") {
            let Some(earlier) = earlier else { return Answer::Unknown(" (no earlier definition named)".into()) };
            return self.redefined(&name, later, earlier, &places);
        }
        if is("static declaration for") || text.contains("declared `extern' and later `static'") {
            let Some(read) = self.read(later, &name) else { return Answer::Unknown(" (its declaration was not found)".into()) };
            let other = Other { owner: earlier.map_or_else(|| "another part".into(), |e| self.owner(e, false)), text: "it external".into() };
            return Answer::Fix(Fix::Rename { name, site: later.clone(), space: Space::Ordinary, kind: "static", other: Other { text: format!("{} ({})", other.text, read.text), ..other } });
        }
        if is("type mismatch with previous implicit declaration") {
            let Some(earlier) = earlier else { return Answer::Nothing };
            if earlier.part() == later.part() {
                return Answer::Nothing;
            }
            let Some(read) = self.read(later, &name) else { return Answer::Nothing };
            let other = Other { owner: self.owner(later, read.defines), text: read.declaration };
            return Answer::Fix(Fix::Implicit { part: earlier.part(), name, other });
        }
        if is("too many arguments to function") || is("too few arguments to function") {
            // A part that declared the routine in a way C finds compatible
            // (`void F();` after `void F(int a) {...}`) calls it as it did.
            let Site::Part { part, .. } = later else { return Answer::Unknown(" (called in a header)".into()) };
            let own = statements(&self.parts[*part].text).into_iter().find(|s| s.body.is_none() && s.routine(&name)).map(|s| s.first_line);
            let Some(line) = own else { return Answer::Unknown(" (the part has no declaration of its own)".into()) };
            let other = Other { owner: "the parts before it".into(), text: "another declaration".into() };
            return Answer::Fix(Fix::Alias { name, site: Site::Part { part: *part, line }, kind: "routine", other, expanded: None });
        }
        if text.ends_with("redefined") && message.warning {
            // A macro: the part that defines it keeps its own.
            let site = match (later, earlier) {
                (Site::Part { .. }, _) => later.clone(),
                (_, Some(earlier @ Site::Part { .. })) => earlier.clone(),
                (Site::Header { part, include, .. }, Some(earlier @ Site::Header { .. })) => {
                    if earlier.part() == *part {
                        return Answer::Nothing;
                    }
                    // Both in headers: the later header's is read as its own file read it.
                    let joined = self.source(earlier).and_then(|text| {
                        text::macros(&text).into_iter().filter(|(n, _, _)| *n == name).min_by_key(|(_, _, line)| line.abs_diff(Join::line(earlier))).map(|(_, definition, _)| definition)
                    });
                    let Some(joined) = joined else { return Answer::Unknown(" (the earlier definition was not found)".into()) };
                    return Answer::Fix(Fix::Unmacro { part: *part, include: *include, name, joined });
                }
                _ => return Answer::Unknown(" (no earlier definition named)".into()),
            };
            if earlier.is_some_and(|earlier| earlier.part() == later.part()) {
                return Answer::Nothing;
            }
            let other = Other { owner: "another part or header".into(), text: "another definition".into() };
            return Answer::Fix(Fix::Rename { name, site, space: Space::Ordinary, kind: "macro", other });
        }
        if message.warning {
            Answer::Nothing
        } else {
            Answer::Unknown(String::new())
        }
    }

    /// Two declarations of one ordinary name that disagree.
    fn disagree(&self, name: &str, later: &Site, earlier: &Site, places: &Places) -> Answer {
        if later.part() == earlier.part() {
            return Answer::Unknown(" (both in one part)".into());
        }
        let (Some(late), Some(early)) = (self.read_at(later, name, Some(places.later), places.pre), self.read_at(earlier, name, places.earlier, places.pre)) else {
            return Answer::Unknown(" (a declaration was not found)".into());
        };
        // A part's own site renames; failing that, the later one's include.
        let renamed = |kind: &'static str, mine: &Read, theirs: &Read, other_site: &Site| {
            let site = match (later, earlier) {
                (Site::Part { .. }, _) | (Site::Header { .. }, Site::Header { .. }) => later.clone(),
                (_, Site::Part { .. }) => earlier.clone(),
            };
            let (this, that) = if site == *later { (mine, theirs) } else { (theirs, mine) };
            let _ = this;
            let other_site = if site == *later { other_site } else { later };
            Answer::Fix(Fix::Rename { name: name.to_string(), site, space: Space::Ordinary, kind, other: Other { owner: self.owner(other_site, false), text: that.text.clone() } })
        };
        if late.typedef || early.typedef {
            return renamed("typedef", &late, &early, earlier);
        }
        if late.enumerator || early.enumerator {
            return renamed("enumerator", &late, &early, earlier);
        }
        if late.is_static || early.is_static {
            let (site, this, that, other_site) = if late.is_static { (later, &late, &early, earlier) } else { (earlier, &early, &late, later) };
            let _ = this;
            return Answer::Fix(Fix::Rename { name: name.to_string(), site: site.clone(), space: Space::Ordinary, kind: "static", other: Other { owner: self.owner(other_site, that.defines), text: that.declaration.clone() } });
        }
        // The definition is the truth: the other side is the view. Between two
        // declarations a part's own takes it rather than a header's, which the
        // parts after it may read only through the include it guards.
        let own_earlier = !early.defines && matches!((later, earlier), (Site::Header { .. }, Site::Part { .. }));
        let (site, this, that, other_site) = if late.defines || own_earlier { (earlier, &early, &late, later) } else { (later, &late, &early, earlier) };
        let expanded = self.read(site, name).is_none().then(|| this.clone());
        let kind = if late.routine || early.routine { "routine" } else { "variable" };
        let shown = if that.routine && that.defines { that.declaration.clone() } else { that.text.clone() };
        Answer::Fix(Fix::Alias { name: name.to_string(), site: site.clone(), kind, other: Other { owner: self.owner(other_site, that.defines), text: shown }, expanded })
    }

    /// A tag defined twice: deleted when the two are the same, else renamed.
    fn retag(&self, tag: &str, later: &Site, places: &Places) -> Answer {
        let Some(pre) = places.pre else { return Answer::Unknown(" (not preprocessed)".into()) };
        let definitions = tag_definitions(pre, tag);
        // The later definition: the one at the place the message names (the
        // joined file's line, or a header's); the earlier, the first.
        let late = definitions.iter().position(|(file, at, end, _)| *file == places.later.file && (*at..=*end).contains(&places.later.line));
        let (Some(late), Some(first)) = (late, definitions.first()) else { return Answer::Unknown(" (its definitions were not found)".into()) };
        if late == 0 {
            return Answer::Unknown(" (its earlier definition was not found)".into());
        }
        if definitions[late].3 == first.3 {
            if let Site::Part { part, line } = later {
                return Answer::Fix(Fix::Delete { part: *part, line: *line, name: tag.to_string(), tag: true });
            }
        }
        // A header's tag against a part's own: the part's is renamed, so the
        // parts after it that read the header see it as theirs did.
        if let (Site::Header { file, .. }, true) = (later, first.0 == places.scratch) {
            if let Some((part, line)) = Join::locate(places.segments, first.1) {
                let owner = file.rsplit('/').next().unwrap_or(file).to_string();
                return Answer::Fix(Fix::Rename { name: tag.to_string(), site: Site::Part { part, line }, space: Space::Tag, kind: "tag", other: Other { owner, text: "another definition".into() } });
            }
        }
        let owner = first.0.rsplit('/').next().unwrap_or(&first.0).to_string();
        let owner = if owner.starts_with(".views-") { "an earlier part".to_string() } else { owner };
        Answer::Fix(Fix::Rename { name: tag.to_string(), site: later.clone(), space: Space::Tag, kind: "tag", other: Other { owner, text: "another definition".into() } })
    }

    /// An ordinary name defined twice: an identical typedef or inline
    /// routine deleted, any other static renamed.
    fn redefined(&self, name: &str, later: &Site, earlier: &Site, places: &Places) -> Answer {
        if later.part() == earlier.part() {
            return Answer::Unknown(" (both in one part)".into());
        }
        let (Some(late), Some(early)) = (self.read_at(later, name, Some(places.later), places.pre), self.read_at(earlier, name, places.earlier, places.pre)) else {
            return Answer::Unknown(" (a definition was not found)".into());
        };
        let same = late.text == early.text;
        if same && (late.typedef || late.inline) {
            if let Site::Part { part, line } = later {
                return Answer::Fix(Fix::Delete { part: *part, line: *line, name: name.to_string(), tag: false });
            }
        }
        let kind = if late.typedef { "typedef" } else if late.is_static || early.is_static { "static" } else { return Answer::Unknown(" (two external definitions)".into()) };
        let site = if matches!(later, Site::Part { .. }) || matches!(earlier, Site::Header { .. }) { later } else { earlier };
        let other_site = if site == later { earlier } else { later };
        let other_read = if site == later { &early } else { &late };
        Answer::Fix(Fix::Rename { name: name.to_string(), site: site.clone(), space: Space::Ordinary, kind, other: Other { owner: self.owner(other_site, false), text: other_read.text.clone() } })
    }

    /// Makes a fix: the part's text changed, or why it cannot be.
    fn apply(&mut self, fix: &Fix) -> Result<(), String> {
        match fix {
            Fix::Alias { name, site, kind, other, expanded } => self.alias(name, site, kind, other, expanded.as_ref()),
            Fix::Rename { name, site, space, kind, other } => self.rename(name, site, *space, kind, other),
            Fix::Implicit { part, name, other } => {
                let declaration = format!("extern int {name}();");
                self.declare(*part, name, &declaration, "routine", other, &format!("int {name}() unprototyped, as this part called it"))
            }
            Fix::Declared { part, name, declaration, other } => self.declare(*part, name, declaration, "routine", other, declaration),
            Fix::Macro { part, name, original, joined } => {
                let other = Other { owner: "the parts before it".into(), text: joined.clone().map_or_else(|| "it undefined".into(), |j| format!("#define {j}")) };
                let this = original.clone().map_or_else(|| "undefined".into(), |o| format!("#define {o}"));
                let p = &mut self.parts[*part];
                let mut lines = vec![marker(p, "macro", name, &this, &other), format!("#undef {name}")];
                lines.extend(original.iter().map(|o| format!("#define {o}")));
                p.text = insert_before(&p.text, first_code_line(&p.text), &lines);
                p.restore.push(format!("#undef {name}"));
                p.restore.extend(joined.iter().map(|j| format!("#define {j}")));
                *self.views.entry("macro").or_default() += 1;
                Ok(())
            }
            Fix::Unmacro { part, include, name, joined } => {
                let other = Other { owner: "the parts before it".into(), text: format!("#define {joined}") };
                let p = &mut self.parts[*part];
                let note = marker(p, "macro", name, "its header's own definition", &other);
                p.text = insert_before(&p.text, *include, &[note, format!("#undef {name}")]);
                p.restore.push(format!("#undef {name}"));
                p.restore.push(format!("#define {joined}"));
                *self.views.entry("macro").or_default() += 1;
                Ok(())
            }
            Fix::After { part, line } => {
                let (declaration, undefine) = self.bound_before(*part, *line).ok_or("no bound declaration there")?;
                let p = &mut self.parts[*part];
                let mut lines: Vec<String> = p.text.lines().map(str::to_string).collect();
                let moved = lines.remove(declaration - 1);
                lines.insert(undefine - 1, moved);
                p.text = lines.join("\n") + "\n";
                Ok(())
            }
            Fix::Delete { part, line, name, tag } => {
                let p = &mut self.parts[*part];
                let all = statements(&p.text);
                let statement = statement_at(&all, *line, name).ok_or_else(|| format!("{name}'s repeat was not found"))?;
                let (start, mut end);
                end = statement.end;
                let only_tag = *tag && statement.words.len() >= 4 && statement.words[1].text == name && statement.words[2].text == "{" && statement.words[statement.words.len() - 2].text == "}";
                if *tag && !only_tag {
                    // `typedef struct X { ... } T;` keeps its declarators.
                    let open = statement.find("{").ok_or("no body")?;
                    let close = statement.words.iter().rposition(|w| w.text == "}" && w.depth == 0).ok_or("no body")?;
                    start = statement.words[open].at;
                    end = statement.words[close].at + 1;
                } else {
                    // Its documentation goes with it.
                    let mut first = statement.first_line;
                    while first > 1 && p.text[text::line_start(&p.text, first - 1)..].trim_start().starts_with("///") {
                        first -= 1;
                    }
                    start = text::line_start(&p.text, first);
                    if p.text[end..].starts_with('\n') {
                        end += 1;
                    }
                }
                p.text.replace_range(start..end, "");
                Ok(())
            }
        }
    }

    fn alias(&mut self, name: &str, site: &Site, kind: &'static str, other: &Other, expanded: Option<&Read>) -> Result<(), String> {
        let read = self.read(site, name).or_else(|| expanded.cloned()).ok_or_else(|| format!("{name}'s declaration was not found"))?;
        let part = site.part();
        let private = self.parts[part].private(name);
        let p = &mut self.parts[part];
        let this = if read.routine && read.defines { read.declaration.clone() } else { read.text.clone() };
        let note = marker(p, kind, name, &this, other);
        match site {
            Site::Part { line, .. } => {
                let renamed = rename(&p.text, name, &private, Space::Ordinary);
                let all = statements(&renamed);
                let statement = statement_at(&all, *line, &private).ok_or_else(|| format!("{name}'s declaration was not found"))?;
                if statement.several() || statement.body.is_some() || statement.words.iter().any(|w| w.depth == 0 && w.text == "=") {
                    return Err(format!("{}.c:{line}: {name} is declared with other names or defined there, so no view of it can be written", p.stem));
                }
                let semicolon = statement.semicolon().ok_or_else(|| format!("{name}'s declaration has no ;"))?;
                let (start, first_line, external) = (statement.start, statement.first_line, statement.has("extern"));
                let mut text = renamed.clone();
                text.insert_str(semicolon, &format!(" __asm__(\"{name}\")"));
                if !external {
                    text.insert_str(start, "extern ");
                }
                p.text = insert_before(&text, first_line, &[note]);
            }
            Site::Header { include, .. } => {
                // Bound before the #include: the header's own inline code uses the
                // name as it is read, before any later label could take hold.
                // The tags it names are declared first, so they are file scope's.
                let declaration = rename(&read.declaration_for(name), name, &private, Space::Ordinary);
                let mut lines = vec![note];
                lines.extend(tags_named(&declaration).into_iter().map(|tag| format!("{tag};")));
                lines.push(external(&declaration, name));
                lines.push(format!("#define {name} {private}"));
                let text = rename(&p.text, name, &private, Space::Ordinary);
                let text = insert_before(&text, include + 1, &[format!("#undef {name}")]);
                p.text = insert_before(&text, *include, &lines);
            }
        }
        p.renames.push((name.to_string(), private, Space::Ordinary));
        *self.views.entry(kind).or_default() += 1;
        Ok(())
    }

    fn rename(&mut self, name: &str, site: &Site, space: Space, kind: &'static str, other: &Other) -> Result<(), String> {
        let part = site.part();
        let private = self.parts[part].private(name);
        let p = &mut self.parts[part];
        let shown = if space == Space::Tag { format!("struct {name}") } else { name.to_string() };
        let shown_private = if space == Space::Tag { format!("struct {private}") } else { private.clone() };
        let note = marker(p, kind, &shown, &shown_private, other);
        match site {
            Site::Part { line, .. } => {
                let renamed = rename(&p.text, name, &private, space);
                let first = if kind == "macro" {
                    text::macros(&renamed).into_iter().filter(|(n, _, _)| *n == private).map(|(_, _, line)| line).min_by_key(|at| at.abs_diff(*line))
                } else {
                    let all = statements(&renamed);
                    statement_at(&all, *line, &private).map(|statement| statement.first_line)
                };
                let first = first.ok_or_else(|| format!("{}.c:{line}: {name} was not found to rename", p.stem))?;
                if renamed == p.text {
                    return Err(format!("{}.c:{line}: renaming {name} changed nothing", p.stem));
                }
                p.text = insert_before(&renamed, first, &[note]);
            }
            Site::Header { include, .. } => {
                let text = rename(&p.text, name, &private, space);
                let text = insert_before(&text, include + 1, &[format!("#undef {name}"), note]);
                p.text = insert_before(&text, *include, &[format!("#define {name} {private}")]);
            }
        }
        if kind == "static" {
            p.statics.push(private.clone());
        }
        p.renames.push((name.to_string(), private, space));
        *self.views.entry(kind).or_default() += 1;
        Ok(())
    }

    /// A routine the part sees as `declaration`: its uses renamed, and the
    /// declaration bound to the symbol above its first use.
    fn declare(&mut self, part: usize, name: &str, declaration: &str, kind: &'static str, other: &Other, this: &str) -> Result<(), String> {
        let p = &mut self.parts[part];
        let private = p.private(name);
        let renamed = rename(&p.text, name, &private, Space::Ordinary);
        if renamed == p.text {
            return Err(format!("{}.c does not use {name}", p.stem));
        }
        let mut declaration = declaration.to_string();
        for (from, to, space) in &p.renames {
            if *space == Space::Tag {
                declaration = rename(&declaration, from, to, Space::Tag);
            }
        }
        let declaration = external(&rename(&declaration, name, &private, Space::Ordinary), name);
        let all = statements(&renamed);
        let first = all.iter().find(|s| s.find(&private).is_some()).map(|s| s.first_line).unwrap_or_else(|| first_code_line(&renamed));
        let note = marker(p, kind, name, this, other);
        p.text = insert_before(&renamed, first, &[note, declaration]);
        p.renames.push((name.to_string(), private, Space::Ordinary));
        *self.views.entry(kind).or_default() += 1;
        Ok(())
    }
}

/// The `struct X` and `union X` a declaration names, each once.
fn tags_named(declaration: &str) -> Vec<String> {
    let words: Vec<&str> = text::identifiers_in_order(declaration);
    let mut tags: Vec<String> = Vec::new();
    for pair in words.windows(2) {
        if matches!(pair[0], "struct" | "union") {
            let tag = format!("{} {}", pair[0], pair[1]);
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
    }
    tags
}

/// What `Join::read` found of a declaration.
#[derive(Clone, Debug)]
struct Read {
    text: String,
    declaration: String,
    typedef: bool,
    enumerator: bool,
    is_static: bool,
    inline: bool,
    routine: bool,
    defines: bool,
}

impl Read {
    fn of(statement: &text::Statement, name: &str) -> Read {
        Read {
            text: statement.squashed(),
            declaration: statement.declaration(),
            typedef: statement.has("typedef"),
            enumerator: statement.enumerates(name),
            is_static: statement.has("static"),
            inline: statement.is_inline(),
            routine: statement.routine(name),
            defines: statement.defines(name),
        }
    }

    /// The declaration to write for a view of `_name`: a definition's head,
    /// or the statement itself.
    fn declaration_for(&self, _name: &str) -> String {
        if self.routine && self.defines {
            self.declaration.clone()
        } else {
            self.text.clone()
        }
    }
}

/// Where a message and the one naming its earlier place point, and the
/// preprocessed join, for a declaration only the expansion spells.
struct Places<'m> {
    later: &'m Message,
    earlier: Option<&'m Message>,
    pre: Option<&'m Preprocessed>,
    segments: &'m Segments,
    scratch: &'m str,
}

enum Answer {
    Fix(Fix),
    /// Nothing to do (a warning the join does not change).
    Nothing,
    /// An error no view answers, and what more is known.
    Unknown(String),
}

fn is_previous(text: &str) -> bool {
    text.starts_with("previous declaration of")
        || text.starts_with("previous definition of")
        || text.starts_with("previous implicit declaration of")
        || text.ends_with("previously defined here")
        || text.ends_with("previously declared here")
        || text.ends_with("was previously implicitly declared to return `int'")
        || text == "this is the location of the previous definition"
}

/// A declaration made `extern` and bound to `symbol`.
fn external(declaration: &str, symbol: &str) -> String {
    let declaration = declaration.trim().trim_end_matches(';').trim_end();
    let declaration = declaration.strip_prefix("static ").unwrap_or(declaration);
    let declaration = if declaration.starts_with("extern ") { declaration.to_string() } else { format!("extern {declaration}") };
    format!("{declaration} __asm__(\"{symbol}\");")
}

/// The first line of a text that is neither a comment nor blank nor a
/// directive: where a part's own declarations may begin.
fn first_code_line(text: &str) -> usize {
    statements(text).first().map_or(text.lines().count() + 1, |s| s.first_line)
}

/// Each definition of `struct|union|enum TAG { ... }` in a preprocessed
/// file: its file, the lines of its tag and of its closing brace there, and
/// its tokens.
fn tag_definitions(pre: &Preprocessed, tag: &str) -> Vec<(String, usize, usize, Vec<String>)> {
    use crate::gba::clex::{lex, Kind};
    let tokens: Vec<_> = lex(&pre.text).into_iter().filter(|t| !t.pp && t.kind != Kind::Comment).collect();
    let mut found = Vec::new();
    for at in 0..tokens.len().saturating_sub(2) {
        if matches!(tokens[at].text, "struct" | "union" | "enum") && tokens[at + 1].text == tag && tokens[at + 2].text == "{" {
            let mut depth = 0;
            let mut words = Vec::new();
            let mut last = tokens[at + 1].line;
            for token in &tokens[at + 2..] {
                words.push(token.text.to_string());
                last = token.line;
                match token.text {
                    "{" => depth += 1,
                    "}" => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let (file, line) = pre.origin.get(tokens[at + 1].line - 1).cloned().unwrap_or_default();
            let end = pre.origin.get(last - 1).filter(|(f, _)| *f == file).map_or(line, |(_, end)| *end);
            found.push((file, line, end, words));
        }
    }
    found
}

/// The declaration of `name` that `function` saw: one made implicitly in
/// its body, else the last before it.
fn visible<'d>(declared: &'d [Declared], function: &[String], name: &[String]) -> Option<&'d Declared> {
    let at = declared.iter().position(|d| function.contains(&d.name) && d.how.ends_with('F'))?;
    let end = declared[at + 1..].iter().position(|d| d.how.ends_with('F')).map_or(declared.len(), |n| at + 1 + n);
    declared[at + 1..end]
        .iter()
        .find(|d| name.contains(&d.name) && d.how.starts_with('I'))
        .or_else(|| declared[..at].iter().rev().find(|d| name.contains(&d.name)))
}

/// Settles one group: its joined text, or the index of the part it cannot
/// reconcile and why.
fn settle(parts: Vec<Part>, into: &str, compiler: &dyn Compiler) -> Result<Settled, (usize, String)> {
    let scratch = Scratch::new(compiler.root(), into);
    let editions = compiler.editions();
    let first = editions.first().cloned().ok_or((0, "no edition links the run".to_string()))?;
    let mut join = Join { parts, compiler, shared: BTreeMap::new(), views: BTreeMap::new() };
    join.shared = shared_macros(&join, &scratch, &first).map_err(|why| (0, why))?;
    let originals: Vec<Vec<Vec<assembly::Piece>>> = editions
        .iter()
        .map(|edition| join.parts.iter().map(|part| compiler.built(edition, &part.stem).map(|asm| assembly::pieces(&asm, &[]))).collect::<Result<Vec<_>, String>>())
        .collect::<Result<_, String>>()
        .map_err(|why| (0, why))?;
    let mut rounds = 0;
    'settle: loop {
        for edition in &editions {
            compile_clean(&mut join, &scratch, edition, &mut rounds)?;
        }
        for (index, edition) in editions.iter().enumerate() {
            let moved = verify(&join, &scratch, edition, &originals[index])?;
            let Some((part, function)) = moved.first().cloned() else { continue };
            rounds += 1;
            if rounds > ROUNDS {
                return Err((part, format!("{function} still differs in {edition} after {ROUNDS} compiles")));
            }
            if search(&mut join, &scratch, edition, &originals[index], part, &function, moved.len())? {
                continue 'settle;
            }
            let others: Vec<&str> = moved.iter().filter(|(p, _)| *p == part).map(|(_, f)| f.as_str()).collect();
            return Err((part, format!("{} compiles differently joined in {edition}, and no name's meaning was found changed for it", others.join(", "))));
        }
        let (text, _) = join.render();
        return Ok(Settled { stems: join.parts.iter().map(|p| p.stem.clone()).collect(), text, views: join.views });
    }
}

/// The macros every part's headers define, read from their includes alone.
fn shared_macros(join: &Join, scratch: &Scratch, edition: &str) -> Result<BTreeMap<String, String>, String> {
    let mut includes = String::new();
    for part in &join.parts {
        for line in part.text.lines().filter(|line| line.trim_start().starts_with("#include")) {
            includes.push_str(line);
            includes.push('\n');
        }
    }
    scratch.write(&includes)?;
    let listing = join.compiler.compile(edition, &scratch.0, Mode::Macros)?;
    Ok(text::listed_macros(&listing.output))
}

/// Compiles the join in one edition, fixing what its messages name, until
/// it compiles with nothing to fix.
fn compile_clean(join: &mut Join, scratch: &Scratch, edition: &str, rounds: &mut usize) -> Result<(), (usize, String)> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    loop {
        *rounds += 1;
        if *rounds > ROUNDS {
            return Err((0, format!("still not clean after {ROUNDS} compiles")));
        }
        let (text, segments) = join.render();
        scratch.write(&text).map_err(|why| (0, why))?;
        let compiled = join.compiler.compile(edition, &scratch.0, Mode::Assembly).map_err(|why| (0, why))?;
        let messages = diagnostics::messages(&compiled.stderr);
        let needs_place = messages.iter().any(|m| m.file != scratch.name());
        let pre = if needs_place || !compiled.ok {
            let output = join.compiler.compile(edition, &scratch.0, Mode::Preprocess).map_err(|why| (0, why))?;
            Some(diagnostics::preprocessed(&output.output, &scratch.name()))
        } else {
            None
        };
        let fixes = join.plan(&messages, &segments, &scratch.name(), pre.as_ref())?;
        if fixes.is_empty() {
            if compiled.ok {
                return Ok(());
            }
            let first = messages.iter().find(|m| !m.warning).map_or_else(|| compiled.stderr.lines().next().unwrap_or("").to_string(), |m| m.text.clone());
            return Err((0, format!("{edition}: {first}")));
        }
        for fix in fixes {
            let key = format!("{fix:?}");
            if !seen.insert(key) {
                return Err((fix.part(), format!("{edition}: the same disagreement came back after its view was written: {fix:?}")));
            }
            join.apply(&fix).map_err(|why| (fix.part(), why))?;
        }
    }
}

/// What moved in one edition: each function or datum that compiles
/// differently joined, with its part.
fn verify(join: &Join, scratch: &Scratch, edition: &str, originals: &[Vec<assembly::Piece>]) -> Result<Vec<(usize, String)>, (usize, String)> {
    let (text, _) = join.render();
    scratch.write(&text).map_err(|why| (0, why))?;
    let compiled = join.compiler.compile(edition, &scratch.0, Mode::Assembly).map_err(|why| (0, why))?;
    if !compiled.ok {
        return Err((0, format!("{edition}: the join no longer compiles: {}", compiled.stderr.lines().next().unwrap_or(""))));
    }
    Ok(assembly::moved(originals, &assembly::pieces(&compiled.output, &join.statics())))
}

/// Looks for the name whose meaning changed for `function` of `part`: each
/// routine it calls declared otherwise than its own file declared it, each
/// macro it uses defined otherwise. Keeps the first view that brings the
/// function back without moving anything else; true if one did.
fn search(join: &mut Join, scratch: &Scratch, edition: &str, originals: &[Vec<assembly::Piece>], part: usize, function: &str, moved: usize) -> Result<bool, (usize, String)> {
    let fail = |why: String| (part, why);
    let p = &join.parts[part];
    let original_path = join.compiler.root().join(format!("{}.c", p.stem));
    let original_text = std::fs::read_to_string(&original_path).map_err(|e| fail(format!("{}: {e}", original_path.display())))?;
    let own = statements(&original_text);
    let body = own.iter().find(|s| s.body.is_some() && s.routine(function)).map(|s| &original_text[s.start..s.end]).unwrap_or(&original_text);
    let used: BTreeSet<String> = text::identifiers(body).into_iter().filter(|name| *name != function).map(str::to_string).collect();
    let mut candidates: Vec<Fix> = Vec::new();

    // Routines.
    let theirs = join.compiler.compile(edition, &original_path, Mode::Declarations).map_err(fail)?;
    let theirs = diagnostics::declarations(&theirs.output);
    let (text, _) = join.render();
    scratch.write(&text).map_err(fail)?;
    let ours = join.compiler.compile(edition, &scratch.0, Mode::Declarations).map_err(fail)?;
    let ours = diagnostics::declarations(&ours.output);
    let suffix = format!("__{}", p.suffix);
    let plain = |text: &str| -> String {
        let mut text = text.to_string();
        for part in &join.parts {
            text = text.replace(&format!("__{}", part.suffix), "");
        }
        text
    };
    let functions = [function.to_string(), format!("{function}{suffix}")];
    for name in &used {
        let names = [name.clone(), format!("{name}{suffix}")];
        let Some(saw) = visible(&theirs, &functions[..1], &names[..1]) else { continue };
        let now = visible(&ours, &functions, &names);
        if now.is_some_and(|now| plain(&now.text) == saw.text) {
            continue;
        }
        let other = Other { owner: "the parts before it".into(), text: now.map_or_else(|| "no declaration".into(), |now| plain(&now.text)) };
        candidates.push(match saw.how.starts_with('I') {
            true => Fix::Implicit { part, name: name.clone(), other },
            false => Fix::Declared { part, name: name.clone(), declaration: saw.text.clone(), other },
        });
    }

    // Macros: the original file's, against those in force where the part ends.
    let theirs = join.compiler.compile(edition, &original_path, Mode::Macros).map_err(fail)?;
    let theirs = text::listed_macros(&theirs.output);
    let (text, _) = join.render_until(part);
    scratch.write(&text).map_err(fail)?;
    let ours = join.compiler.compile(edition, &scratch.0, Mode::Macros).map_err(fail)?;
    let ours = text::listed_macros(&ours.output);
    let whole: BTreeSet<&str> = text::identifiers(&original_text);
    for name in whole {
        let names = [name.to_string(), format!("{name}{suffix}")];
        let saw = theirs.get(name).cloned();
        let now = names.iter().find_map(|n| ours.get(n)).cloned();
        if saw == now || (saw.is_none() && now.is_none()) {
            continue;
        }
        candidates.push(Fix::Macro { part, name: name.to_string(), original: saw, joined: now });
    }

    // Each alone, those of the function's own names first; then all at once.
    candidates.sort_by_key(|fix| match fix {
        Fix::Implicit { name, .. } | Fix::Declared { name, .. } | Fix::Macro { name, .. } => !used.contains(name),
        _ => true,
    });
    let mut trials: Vec<Vec<Fix>> = candidates.iter().map(|fix| vec![fix.clone()]).collect();
    if candidates.len() > 1 {
        trials.push(candidates.clone());
    }
    for trial in trials {
        let mut tried = Join { parts: join.parts.clone(), compiler: join.compiler, shared: join.shared.clone(), views: join.views.clone() };
        if trial.iter().any(|fix| tried.apply(fix).is_err()) {
            continue;
        }
        let Ok(now) = verify(&tried, scratch, edition, originals) else { continue };
        if !now.iter().any(|(p, f)| *p == part && f == function) && now.len() < moved {
            *join = tried;
            return Ok(true);
        }
    }
    Ok(false)
}

/// The build's compiler for a run, by the recipe make gives its first
/// object in each edition, run on a scratch file in its place.
pub(crate) struct Recipes {
    root: PathBuf,
    /// Each edition with the recipe of the run's first object.
    recipes: Vec<(String, Vec<Vec<String>>)>,
    source: String,
    object_of: Vec<String>,
    scratch: tempfile::TempDir,
}

impl Recipes {
    /// The recipes of `stem`, the run's first object, in each edition of
    /// `editions` that links it.
    pub fn new(root: &Path, stem: &str, editions: &[crate::alchemy::targets::DecompTargetId]) -> Result<Recipes, String> {
        let source = format!("{stem}.c");
        let mut recipes = Vec::new();
        let mut object_of = Vec::new();
        for &id in editions {
            if !crate::alchemy::link::linked(root, id)?.links(stem) {
                continue;
            }
            let target = crate::alchemy::targets::target_for(id);
            let object = format!("{}/obj/{stem}.o", target.output_dir);
            let recipe = crate::gba::recipe::recipe(root, Path::new(target.output_dir), &[format!("EDITION={id}")], &source, &object)?;
            recipes.push((id.as_str().to_string(), recipe));
            object_of.push(object);
        }
        let scratch = tempfile::Builder::new().prefix("alchemy-views-").tempdir().map_err(|e| e.to_string())?;
        Ok(Recipes { root: root.to_path_buf(), recipes, source, object_of, scratch })
    }
}

impl Compiler for Recipes {
    fn editions(&self) -> Vec<String> {
        self.recipes.iter().map(|(edition, _)| edition.clone()).collect()
    }

    fn compile(&self, edition: &str, source: &Path, mode: Mode) -> Result<Compiled, String> {
        static SERIAL: AtomicUsize = AtomicUsize::new(0);
        let index = self.recipes.iter().position(|(e, _)| e == edition).ok_or_else(|| format!("{edition} does not link the run"))?;
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let output = self.scratch.path().join(format!("out{serial}.o"));
        let steps = crate::gba::recipe::retarget(&self.recipes[index].1, &self.root, &self.source, &self.object_of[index], source, &output);
        let mut step = steps.into_iter().find(|step| step.iter().any(|w| w == "-S")).ok_or("the recipe has no -S step")?;
        let assembly = output.with_extension("s").to_string_lossy().into_owned();
        let written = match mode {
            Mode::Assembly => assembly.clone(),
            Mode::Declarations => {
                let aux = output.with_extension("aux").to_string_lossy().into_owned();
                step.extend(["-aux-info".to_string(), aux.clone()]);
                aux
            }
            Mode::Preprocess | Mode::Macros => {
                let preprocessed = output.with_extension("i").to_string_lossy().into_owned();
                for word in &mut step {
                    if word == "-S" {
                        *word = "-E".into();
                    } else if *word == assembly {
                        *word = preprocessed.clone();
                    }
                }
                if mode == Mode::Macros {
                    step.push("-dM".into());
                }
                preprocessed
            }
        };
        let (ok, _, stderr) = crate::gba::process::capture(&step, &self.root)?;
        let text = std::fs::read(&written).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        for extension in ["s", "aux", "i"] {
            let _ = std::fs::remove_file(output.with_extension(extension));
        }
        Ok(Compiled { ok, output: text, stderr })
    }

    fn built(&self, edition: &str, stem: &str) -> Result<String, String> {
        let id = crate::alchemy::targets::parse_decomp_target(edition)?;
        let path = crate::alchemy::built::output(&self.root, id).join("obj").join(format!("{stem}.s"));
        let modified = |path: &Path| std::fs::metadata(path).and_then(|meta| meta.modified()).ok();
        let source = format!("{stem}.c");
        if modified(&path).is_none() || modified(&self.root.join(&source)) > modified(&path) {
            return Err(format!("{} is missing or older than {source}: make {edition} first", path.display()));
        }
        crate::alchemy::built::read_text(&path)
    }

    fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests;
