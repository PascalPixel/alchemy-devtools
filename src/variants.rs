//! variants: named rewrites of a draft, each measured as diff measures the
//! draft and listed best first; with --keep the best is written over the
//! draft when it narrows the gap. The draft itself is never written while
//! they run: each rewrite compiles from a copy, by the draft's own recipe.

use crate::diff::{self, skeleton, Reference};
use crate::gba::process::run as command;
use crate::permute::{stop_on_signals, INTERRUPTED};
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const USAGE: &str = r#"usage: alchemy-devtools variants DRAFT.c VARIANTS [ADDRESS[,ADDRESS...]] [--symbol NAME] [--overlay resource_XXX] [--game GAME] [--edition EDITION] [--arm] [--combine N] [--jobs J] [--keep]
Measures named rewrites of DRAFT.c as diff measures the draft, with diff's
ADDRESS, --symbol, --overlay, --game, --edition and --arm: each rewrite is
compiled from a copy by the recipe make gives the draft, asked once, and
compared with the same listing. Prints a table, best first: the skeleton
gap, then the instructions that differ, then the rewrite's instruction
count; (draft) is the draft as written. A rewrite with a FIND that does not
occur, or that does not compile, follows with the reason.
--combine N also measures every combination of 2 to N variants, each
applied in the file's order; a combination in which a FIND no longer occurs
is counted and left out. --jobs J compiles J at a time (default: cores - 2).
--keep writes the best rewrite over DRAFT.c when it narrows the draft's
skeleton gap, or keeps the gap with fewer instructions that differ; nothing
else writes the draft, so after an error or an interrupt it is as it was.
The copies compile in out/EDITION/obj/drafts/run-PID, removed when done.

VARIANTS is plain text. A variant is its name on a line of its own, then
its replacements, applied in order, each a FIND and the text that replaces
every occurrence of it:
  - TEXT   a line of the FIND, found as written
  ~ TEXT   a line of the FIND as a regular expression (Rust's regex crate)
  + TEXT   a line of the replacement; after ~, $1 or ${name} inserts a
           group and $$ is a dollar sign
After the marker and one space a line is taken as written, indentation
included; a marker alone is an empty line. Lines of one kind in a row are
one text, joined by newlines; a FIND with no + lines is deleted. A blank
line ends a replacement, so that a FIND can follow one with no + lines.
Lines starting with # are comments. Each FIND must occur when its turn
comes. For example:
  reuse_dx
  - if (Iwram_MulQ16(dx, dx) + Iwram_MulQ16(dz, dz) < 0x40000) {
  + dx = Iwram_MulQ16(dx, dx) + Iwram_MulQ16(dz, dz);
  +   if (dx < 0x40000) {

  unsigned_counts
  ~ \bs32 (count|total);
  + u32 $1;"#;

struct Options {
    measure: diff::Options,
    variants: PathBuf,
    combine: usize,
    jobs: usize,
    keep: bool,
}

fn parse(arguments: &[String]) -> Result<Options, String> {
    let count = |option: &str, value: &str| match value.parse::<usize>() {
        Ok(n) if n > 0 => Ok(n),
        _ => Err(format!("{option}: a count of at least 1\n{USAGE}")),
    };
    // diff reads the draft, ADDRESS and the measure options; the rest are ours.
    let mut measured = Vec::new();
    let mut positional = Vec::new();
    let mut combine = 1;
    let mut jobs = std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(2).max(1));
    let mut keep = false;
    let mut i = 0;
    while i < arguments.len() {
        match arguments[i].as_str() {
            "--keep" => keep = true,
            "--arm" => measured.push(arguments[i].clone()),
            option @ ("--combine" | "--jobs" | "--symbol" | "--overlay" | "--game" | "--edition") => {
                i += 1;
                let value = arguments.get(i).ok_or_else(|| format!("{option} takes a value\n{USAGE}"))?;
                match option {
                    "--combine" => combine = count(option, value)?,
                    "--jobs" => jobs = count(option, value)?,
                    _ => measured.extend([option.to_string(), value.clone()]),
                }
            }
            value if value.starts_with('-') => return Err(format!("unknown variants option: {value}\n{USAGE}")),
            value => positional.push(value.to_string()),
        }
        i += 1;
    }
    let [draft, variants, address @ ..] = positional.as_slice() else { return Err(USAGE.into()) };
    if address.len() > 1 {
        return Err(USAGE.into());
    }
    measured.splice(0..0, std::iter::once(draft.clone()).chain(address.iter().cloned()));
    // diff's errors name its usage; these name this command's.
    let measure = diff::parse(&measured).map_err(|error| error.replace(diff::USAGE, USAGE))?;
    Ok(Options { measure, variants: PathBuf::from(variants), combine, jobs, keep })
}

/// One replacement: what it finds, and the text that replaces every
/// occurrence.
struct Replacement {
    /// The FIND's first line in the variants file.
    line: usize,
    find: Find,
    replace: String,
}

enum Find {
    Text(String),
    Pattern(Regex),
}

impl Replacement {
    /// `text` with every occurrence replaced, or None when there is none.
    fn apply(&self, text: &str) -> Option<String> {
        match &self.find {
            Find::Text(find) => text.contains(find.as_str()).then(|| text.replace(find.as_str(), &self.replace)),
            Find::Pattern(pattern) => pattern.is_match(text).then(|| pattern.replace_all(text, self.replace.as_str()).into_owned()),
        }
    }
}

struct Variant {
    name: String,
    replacements: Vec<Replacement>,
}

/// A replacement being read: its FIND's lines (a pattern's when marked ~)
/// and its replacement's.
struct Reading {
    line: usize,
    pattern: bool,
    find: Vec<String>,
    replace: Vec<String>,
}

impl Reading {
    fn finish(self) -> Result<Replacement, String> {
        let find = self.find.join("\n");
        if find.is_empty() {
            return Err(format!("line {}: an empty FIND", self.line));
        }
        let find = if self.pattern {
            Find::Pattern(Regex::new(&find).map_err(|error| format!("line {}: {error}", self.line))?)
        } else {
            Find::Text(find)
        };
        Ok(Replacement { line: self.line, find, replace: self.replace.join("\n") })
    }
}

/// The variants a file defines, in its order (USAGE gives the format).
fn read_variants(text: &str) -> Result<Vec<Variant>, String> {
    let mut variants: Vec<Variant> = Vec::new();
    let mut reading: Option<Reading> = None;
    let finish = |reading: &mut Option<Reading>, variants: &mut Vec<Variant>| -> Result<(), String> {
        if let Some(done) = reading.take() {
            variants.last_mut().expect("a FIND follows a name").replacements.push(done.finish()?);
        }
        Ok(())
    };
    let complete = |variants: &[Variant]| match variants.last() {
        Some(variant) if variant.replacements.is_empty() => Err(format!("variant {} has no replacements", variant.name)),
        _ => Ok(()),
    };
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let Some(marker) = line.chars().next().filter(|c| matches!(c, '-' | '~' | '+')) else {
            if line.starts_with('#') {
                continue;
            }
            finish(&mut reading, &mut variants)?;
            let name = line.trim();
            if name.is_empty() {
                continue;
            }
            if name.starts_with(['-', '~', '+']) {
                return Err(format!("line {number}: a marker opens its line, with no space before it"));
            }
            complete(&variants)?;
            if variants.iter().any(|variant| variant.name == name) {
                return Err(format!("line {number}: a second variant named {name}"));
            }
            variants.push(Variant { name: name.to_string(), replacements: Vec::new() });
            continue;
        };
        let content = match &line[1..] {
            "" => "",
            rest => rest.strip_prefix(' ').ok_or_else(|| format!("line {number}: one space follows the {marker}"))?,
        }
        .to_string();
        if marker == '+' {
            reading.as_mut().ok_or_else(|| format!("line {number}: a + line with no FIND before it"))?.replace.push(content);
            continue;
        }
        if variants.is_empty() {
            return Err(format!("line {number}: a FIND before any variant's name"));
        }
        let pattern = marker == '~';
        match &mut reading {
            Some(open) if open.replace.is_empty() => {
                if open.pattern != pattern {
                    return Err(format!("line {number}: a FIND is found as written (-) or as a pattern (~), not both"));
                }
                open.find.push(content);
            }
            _ => {
                finish(&mut reading, &mut variants)?;
                reading = Some(Reading { line: number, pattern, find: vec![content], replace: Vec::new() });
            }
        }
    }
    finish(&mut reading, &mut variants)?;
    complete(&variants)?;
    if variants.is_empty() {
        return Err("no variants".into());
    }
    Ok(variants)
}

/// `text` with the replacements of `variants` applied in order, or the
/// line of the first FIND that does not occur when its turn comes.
fn rewrite(text: &str, variants: &[&Variant]) -> Result<String, usize> {
    let mut text = text.to_string();
    for replacement in variants.iter().flat_map(|variant| &variant.replacements) {
        text = replacement.apply(&text).ok_or(replacement.line)?;
    }
    Ok(text)
}

/// Every choice of 1 to `most` of `count` variants, each in file order:
/// the variants alone, then pairs, and so on.
fn combinations(count: usize, most: usize) -> Vec<Vec<usize>> {
    let mut all = Vec::new();
    let mut choices: Vec<Vec<usize>> = (0..count).map(|i| vec![i]).collect();
    for _ in 0..most.min(count) {
        let next = choices.iter().flat_map(|chosen| (chosen[chosen.len() - 1] + 1..count).map(move |i| [chosen.as_slice(), &[i]].concat())).collect();
        all.append(&mut std::mem::replace(&mut choices, next));
    }
    all
}

/// The draft's compile commands, asked of make once, written for the
/// first job's folder and aimed at each job's copy.
struct Bench {
    draft: PathBuf,
    folder: PathBuf,
    steps: Vec<Vec<String>>,
}

/// A job's copy of the draft, the object it compiles to and the commands.
struct Job {
    copy: PathBuf,
    object: PathBuf,
    steps: Vec<Vec<String>>,
}

impl Bench {
    fn new(options: &diff::Options, draft: &Path, folder: &Path) -> Result<Bench, String> {
        let object = Bench::object(draft, &folder.join("job-0"));
        Ok(Bench { draft: draft.to_path_buf(), folder: folder.to_path_buf(), steps: options.steps(draft, &object)? })
    }

    fn object(draft: &Path, folder: &Path) -> PathBuf {
        folder.join(draft.with_extension("o").file_name().unwrap_or_default())
    }

    /// Job `n`'s copy and commands. A quoted include is looked for first
    /// beside the file that includes it: the copy's includes are looked for
    /// next in the draft's folder, so the copy finds what the draft finds,
    /// and every header still looks in its own folder first.
    fn job(&self, n: usize) -> Result<Job, String> {
        let folder = self.folder.join(format!("job-{n}"));
        std::fs::create_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
        let copy = folder.join(self.draft.file_name().unwrap_or_default());
        let object = Bench::object(&self.draft, &folder);
        let (draft, copied) = (self.draft.to_string_lossy(), copy.to_string_lossy().into_owned());
        let (from, to) = (format!("{}/", self.folder.join("job-0").display()), format!("{}/", folder.display()));
        let beside = format!("-I{}", self.draft.parent().unwrap_or(Path::new("/")).display());
        let steps = self.steps.iter().map(|step| {
            let mut step: Vec<String> = step.iter().map(|argument| {
                if *argument == draft { copied.clone() } else { argument.replace(&from, &to) }
            }).collect();
            if step.contains(&copied) {
                step.insert(1, beside.clone());
            }
            step
        }).collect();
        Ok(Job { copy, object, steps })
    }
}

impl Job {
    fn compile(&self, text: &str) -> Result<(), String> {
        std::fs::write(&self.copy, text).map_err(|error| format!("{}: {error}", self.copy.display()))?;
        let _ = std::fs::remove_file(&self.object);
        let folder = self.copy.parent().unwrap_or(Path::new("/"));
        for step in &self.steps {
            command(step, folder)?;
        }
        Ok(())
    }

    /// The compiled text's figures, as diff prints them.
    fn figures(&self, reference: &Reference) -> Result<Figures, String> {
        let compiled = diff::disassemble(&self.object, reference.symbol(&self.object)?.as_deref())?;
        let listing = &reference.listing;
        let (common, _) = diff::compare_counts(listing, &compiled);
        let (removed, added) = skeleton::gap(listing, &compiled);
        Ok(Figures { gap: removed + added, differ: listing.len() + compiled.len() - 2 * common, count: compiled.len() })
    }

    fn measure(&self, reference: &Reference, text: &str) -> Result<Figures, String> {
        if crate::permute::perm::has_macros(text) {
            return Err("uses PERM macros, which only permute expands".into());
        }
        self.compile(text).map_err(|error| format!("does not compile: {}", self.complaint(&error)))?;
        self.figures(reference)
    }

    /// The compiler's first complaint about a line of the copy, which it
    /// names by the draft's file name; else the error's first line.
    fn complaint(&self, error: &str) -> String {
        let copy = self.copy.to_string_lossy();
        let name = self.copy.file_name().unwrap_or_default().to_string_lossy();
        let at = format!("{name}:");
        let lines: Vec<String> = error.lines().map(|line| line.replace(copy.as_ref(), &name)).collect();
        let complaint = lines.iter().find(|line| {
            !line.contains("warning:") && line.split_once(&at).is_some_and(|(_, rest)| rest.starts_with(|c: char| c.is_ascii_digit()))
        });
        complaint.or(lines.first()).map(|line| line.trim().to_string()).unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Figures {
    gap: usize,
    differ: usize,
    count: usize,
}

impl Figures {
    /// Smaller is better: the skeleton gap, then the instructions that differ.
    fn rank(&self) -> (usize, usize) {
        (self.gap, self.differ)
    }
}

/// A row of the table: the draft as written, a variant or a combination.
struct Row {
    name: String,
    /// How many variants it applies; none for the draft as written.
    size: usize,
    /// Its text's index among those measured, or why it has none.
    text: Result<usize, String>,
}

/// What a run measured: its rows, best first; the distinct texts, the
/// draft's first; and their figures (none for a text an interrupt left).
struct Table {
    rows: Vec<Row>,
    texts: Vec<String>,
    figures: Vec<Option<Result<Figures, String>>>,
}

impl Table {
    fn outcome<'a>(&'a self, row: &'a Row) -> Option<Result<Figures, &'a str>> {
        match &row.text {
            Ok(index) => self.figures[*index].as_ref().map(|result| result.as_ref().copied().map_err(String::as_str)),
            Err(why) => Some(Err(why)),
        }
    }

    /// The first row that improves on the draft as written, with its text.
    fn best(&self) -> Option<(&Row, &str, Figures)> {
        let draft = self.figures[0].clone()?.ok()?;
        self.rows.iter().filter(|row| row.size > 0).find_map(|row| {
            let index = *row.text.as_ref().ok()?;
            let figures = self.figures[index].clone()?.ok()?;
            (figures.rank() < draft.rank()).then_some((row, self.texts[index].as_str(), figures))
        })
    }

    fn print(&self) {
        println!("{:>5} {:>6} {:>12}  variant", "gap", "differ", "instructions");
        for row in &self.rows {
            match self.outcome(row) {
                Some(Ok(figures)) => println!("{:>5} {:>6} {:>12}  {}", figures.gap, figures.differ, figures.count, row.name),
                Some(Err(why)) => println!("{:>5} {:>6} {:>12}  {}: {why}", "-", "-", "-", row.name),
                None => {}
            }
        }
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// Measures the draft as written, which `first` has compiled, then each
/// variant and combination of up to `combine` of them, `jobs` at a time;
/// a text two of them share is compiled once.
fn tabulate(bench: &Bench, first: &Job, reference: &Reference, original: &str, variants: &[Variant], combine: usize, jobs: usize) -> Result<Table, String> {
    let mut texts = vec![original.to_string()];
    let mut index = HashMap::from([(original.to_string(), 0)]);
    let mut rows = vec![Row { name: "(draft)".into(), size: 0, text: Ok(0) }];
    let mut left_out = 0;
    for chosen in combinations(variants.len(), combine) {
        let chosen: Vec<&Variant> = chosen.iter().map(|&i| &variants[i]).collect();
        let name = chosen.iter().map(|variant| variant.name.as_str()).collect::<Vec<_>>().join("+");
        let text = match rewrite(original, &chosen) {
            Ok(text) => Ok(*index.entry(text.clone()).or_insert_with(|| {
                texts.push(text);
                texts.len() - 1
            })),
            Err(_) if chosen.len() > 1 => {
                left_out += 1;
                continue;
            }
            Err(line) => Err(format!("the FIND on line {line} does not occur")),
        };
        rows.push(Row { name, size: chosen.len(), text });
    }
    let combined = rows.iter().filter(|row| row.size > 1).count();
    let jobs = jobs.min(texts.len() - 1).max(1);
    let mut counted = format!("{} variant{}", variants.len(), plural(variants.len()));
    if combine > 1 {
        counted += &format!(" and {combined} combination{} of up to {combine} ({left_out} left out)", plural(combined));
    }
    println!("listing {} instructions; {counted}: {} text{} to compile in {jobs} job{}", reference.listing.len(), texts.len() - 1, plural(texts.len() - 1), plural(jobs));
    let mut figures = vec![None; texts.len()];
    figures[0] = Some(Ok(first.figures(reference)?));
    let figures = Mutex::new(figures);
    let next = AtomicUsize::new(1);
    let workers = (0..jobs).map(|n| bench.job(n)).collect::<Result<Vec<Job>, String>>()?;
    std::thread::scope(|scope| {
        for job in &workers {
            let (texts, figures, next) = (&texts, &figures, &next);
            scope.spawn(move || loop {
                if INTERRUPTED.load(Ordering::Relaxed) {
                    break;
                }
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(text) = texts.get(i) else { break };
                let outcome = job.measure(reference, text);
                figures.lock().unwrap()[i] = Some(outcome);
            });
        }
    });
    let mut table = Table { rows: Vec::new(), texts, figures: figures.into_inner().unwrap() };
    // Measured rows first, best first; among equals, fewer variants, then
    // the file's order.
    rows.sort_by_key(|row| match table.outcome(row) {
        Some(Ok(figures)) => (0, figures.rank(), row.size),
        _ => (1, (0, 0), 0),
    });
    table.rows = rows;
    Ok(table)
}

/// Writes `text` over the draft through a file beside it, renamed into
/// place, so the draft is whole whatever happens; refused when the draft
/// is no longer `original`.
fn keep(draft: &Path, original: &str, text: &str) -> Result<(), String> {
    let now = std::fs::read_to_string(draft).map_err(|error| format!("{}: {error}", draft.display()))?;
    if now != original {
        return Err(format!("{}: changed while the variants ran; nothing written", draft.display()));
    }
    let name = draft.file_name().unwrap_or_default().to_string_lossy();
    let beside = draft.with_file_name(format!(".{name}.variants-{}", std::process::id()));
    let written = std::fs::write(&beside, text)
        .and_then(|()| std::fs::set_permissions(&beside, std::fs::metadata(draft)?.permissions()))
        .and_then(|()| std::fs::rename(&beside, draft));
    if let Err(error) = written {
        let _ = std::fs::remove_file(&beside);
        return Err(format!("{}: {error}; the draft is as it was", draft.display()));
    }
    Ok(())
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    if arguments.iter().any(|argument| matches!(argument.as_str(), "-h" | "--help")) {
        println!("{USAGE}");
        return Ok(());
    }
    let options = parse(arguments)?;
    if options.measure.arm {
        println!("{}", diff::GAME_ARM_NOTICE);
    }
    let draft = std::fs::canonicalize(&options.measure.draft).map_err(|error| format!("{}: {error}", options.measure.draft.display()))?;
    let original = std::fs::read_to_string(&draft).map_err(|error| format!("{}: {error}", draft.display()))?;
    if crate::permute::perm::has_macros(&original) {
        return Err(format!("{}: the draft uses PERM macros, which only alchemy-devtools permute expands", draft.display()));
    }
    let path = &options.variants;
    let file = std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let variants = read_variants(&file).map_err(|error| format!("{}: {error}", path.display()))?;
    stop_on_signals();
    let work = diff::Work::new(options.measure.target)?;
    let bench = Bench::new(&options.measure, &draft, &work.folder)?;
    let first = bench.job(0)?;
    first.compile(&original).map_err(|error| error.replace(first.copy.to_string_lossy().as_ref(), draft.to_string_lossy().as_ref()))?;
    let reference = diff::reference(&options.measure, &first.object)?;
    let table = tabulate(&bench, &first, &reference, &original, &variants, options.combine, options.jobs)?;
    table.print();
    if INTERRUPTED.load(Ordering::Relaxed) {
        let measured = table.figures.iter().filter(|figures| figures.is_some()).count();
        return Err(format!("interrupted after {} of {} texts; the draft is as it was", measured - 1, table.texts.len() - 1));
    }
    if !options.keep {
        return Ok(());
    }
    let Some(Ok(was)) = table.figures[0].clone() else { unreachable!("the draft is measured first") };
    match table.best() {
        Some((row, text, figures)) => {
            keep(&draft, &original, text)?;
            println!("kept {}: gap {}, differ {}, where the draft had gap {}, differ {}", row.name, figures.gap, figures.differ, was.gap, was.differ);
        }
        None => println!("kept nothing: no variant improves on the draft's gap {}, differ {}; the draft is as it was", was.gap, was.differ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn options_split_between_diff_and_variants() {
        let parsed = parse(&args(&["recon/tla/en/main/08171bb0.c", "v.txt", "--combine", "2", "--jobs", "3", "--keep", "--edition", "tla-de"])).unwrap();
        assert_eq!((parsed.combine, parsed.jobs, parsed.keep), (2, 3, true));
        assert_eq!(parsed.variants, PathBuf::from("v.txt"));
        assert_eq!(parsed.measure.target.id.as_str(), "tla-de");
        let parsed = parse(&args(&["x.c", "v.txt", "08000000,08000010", "--arm", "--game", "tbs"])).unwrap();
        assert!(parsed.measure.arm && !parsed.keep && parsed.combine == 1);
        for wrong in [&["x.c"][..], &["x.c", "v.txt", "08000000", "08000010"], &["08000000.c", "v.txt", "--show"], &["08000000.c", "v.txt", "--jobs", "0"], &["08000000.c", "v.txt", "--combine"]] {
            assert!(parse(&args(wrong)).is_err(), "{wrong:?}");
        }
        // diff's own refusals name this command's usage.
        let error = parse(&args(&["draft.c", "v.txt"])).err().unwrap();
        assert!(error.contains("name the listing address") && error.contains("alchemy-devtools variants") && !error.contains("alchemy-devtools diff"), "{error}");
    }

    #[test]
    fn a_file_reads_as_names_finds_and_replacements() {
        let file = "# a comment\nfirst\n- a\n-   b\n+ c\n+\n+  d\n- e\n\n- f\n\nsecond\n~ (x+)y\n# among them\n+ $1\n- gone\n";
        let variants = read_variants(file).unwrap();
        assert_eq!(variants.iter().map(|variant| variant.name.as_str()).collect::<Vec<_>>(), ["first", "second"]);
        let first = &variants[0].replacements;
        assert_eq!(first.len(), 3);
        assert!(matches!(&first[0].find, Find::Text(text) if text == "a\n  b"));
        assert_eq!((first[0].line, first[0].replace.as_str()), (3, "c\n\n d"));
        assert!(matches!(&first[1].find, Find::Text(text) if text == "e") && first[1].replace.is_empty());
        assert!(matches!(&first[2].find, Find::Text(text) if text == "f") && first[2].line == 10);
        let second = &variants[1].replacements;
        assert!(matches!(&second[0].find, Find::Pattern(_)) && second[0].replace == "$1");
        assert!(matches!(&second[1].find, Find::Text(text) if text == "gone") && second[1].replace.is_empty());
        for (file, wrong) in [
            ("- a\n+ b\n", "before any variant's name"),
            ("one\n+ b\n", "with no FIND"),
            ("one\n- a\n~ b\n+ c\n", "not both"),
            ("one\n-a\n+ b\n", "one space follows the -"),
            ("one\n  - a\n", "a marker opens its line"),
            ("one\n- a\none\n- b\n", "a second variant named one"),
            ("one\ntwo\n- a\n", "one has no replacements"),
            ("one\n- a\ntwo\n", "two has no replacements"),
            ("one\n-\n+ b\n", "an empty FIND"),
            ("one\n~ (\n", "line 2"),
            ("# nothing\n\n", "no variants"),
        ] {
            let error = read_variants(file).err().unwrap_or_default();
            assert!(error.contains(wrong), "{file:?}: {error}");
        }
    }

    #[test]
    fn replacements_apply_in_order_to_every_occurrence() {
        let variants = read_variants("swap\n- a\n+ b\n- bb\n+ c\n\nshout\n~ (?m)^(\\w+)$\n+ ${1}!\n\nmissing\n- z\n").unwrap();
        let [swap, shout, missing] = [&variants[0], &variants[1], &variants[2]];
        assert_eq!(rewrite("ab\na", &[swap]).unwrap(), "c\nb");
        assert_eq!(rewrite("ab\na", &[shout]).unwrap(), "ab!\na!");
        assert_eq!(rewrite("ab\na", &[swap, shout]).unwrap(), "c!\nb!");
        assert_eq!(rewrite("ab\na", &[missing]), Err(12));
        // A later FIND sees the text the earlier ones left.
        assert_eq!(rewrite("a", &[swap]), Err(4));
    }

    #[test]
    fn combinations_go_by_size_then_file_order() {
        assert_eq!(combinations(3, 1), [vec![0], vec![1], vec![2]]);
        assert_eq!(combinations(3, 2), [vec![0], vec![1], vec![2], vec![0, 1], vec![0, 2], vec![1, 2]]);
        assert_eq!(combinations(3, 9).len(), 7);
        assert_eq!(combinations(5, 3).len(), 5 + 10 + 10);
    }

    #[test]
    fn variants_measure_as_diff_does_and_the_best_is_kept_only_when_better() {
        let work = tempfile::tempdir().unwrap();
        let draft = work.path().join("draft/08000000.c");
        std::fs::create_dir_all(draft.parent().unwrap()).unwrap();
        // A header beside the draft: the copies find it as the draft does.
        std::fs::write(draft.with_file_name("step.h"), "#define STEP 3\n").unwrap();
        let original = "#include \"step.h\"\nint gX;\nint Sub(int a, int b)\n{\n    gX = a;\n    return a * STEP - b;\n}\n";
        std::fs::write(&draft, original).unwrap();
        // The listing: the matching spelling, compiled as diff compiles drafts.
        let options = diff::parse(&args(&[&draft.to_string_lossy(), "--game", "tbs"])).unwrap();
        let source = work.path().join("reference/08000000.c");
        let object = work.path().join("reference/obj/08000000.o");
        std::fs::create_dir_all(object.parent().unwrap()).unwrap();
        std::fs::write(&source, "int gX;\nint Sub(int a, int b)\n{\n    gX = a;\n    return a - b;\n}\n").unwrap();
        for step in options.steps(&source, &object).unwrap() {
            command(&step, work.path()).unwrap();
        }
        let reference = Reference { listing: diff::disassemble(&object, None).unwrap(), functions: Vec::new() };
        let file = "unscaled\n- a * STEP\n+ a\n\nbroken\n- gX = a;\n+ gX = a\n\nmissing\n- gY\n\nsame\n- int gX;\n+ int gX;\n\nlast\n- return\n+ gX = b;\n+     return\n";
        let variants = read_variants(file).unwrap();
        let bench = Bench::new(&options, &draft, &work.path().join("run")).unwrap();
        let first = bench.job(0).unwrap();
        first.compile(original).unwrap();
        let table = tabulate(&bench, &first, &reference, original, &variants, 2, 3).unwrap();
        let row = |name: &str| table.rows.iter().find(|row| row.name == name).map(|row| table.outcome(row).unwrap());
        // Each figure is diff's: its comparison of the same compile.
        let expected = |text: &str| {
            first.compile(text).unwrap();
            let compiled = diff::disassemble(&first.object, None).unwrap();
            let (common, _) = diff::compare(&reference.listing, &compiled);
            let (removed, added) = skeleton::gap(&reference.listing, &compiled);
            Figures { gap: removed + added, differ: reference.listing.len() + compiled.len() - 2 * common, count: compiled.len() }
        };
        let draft_figures = expected(original);
        assert!(draft_figures.differ > 0);
        assert_eq!(row("(draft)"), Some(Ok(draft_figures)));
        assert_eq!(row("same"), Some(Ok(draft_figures)));
        let matched = Figures { gap: 0, differ: 0, count: reference.listing.len() };
        assert_eq!(row("unscaled"), Some(Ok(matched)));
        assert_eq!(row("unscaled+same"), Some(Ok(matched)));
        assert_eq!(row("last"), Some(Ok(expected(&rewrite(original, &[&variants[4]]).unwrap()))));
        assert!(matches!(row("broken"), Some(Err(why)) if why.starts_with("does not compile: 08000000.c:")), "{:?}", row("broken"));
        assert_eq!(row("missing"), Some(Err("the FIND on line 10 does not occur")));
        // The four combinations with missing never apply; the six others do.
        assert_eq!(table.rows.len(), 1 + 5 + 6);
        assert!(!table.rows.iter().any(|row| row.name.contains("missing+") || row.name.contains("+missing")));
        // Best first, the variant alone before its combinations.
        assert_eq!(table.rows[0].name, "unscaled");
        assert!(table.rows.iter().position(|row| row.name == "(draft)") < table.rows.iter().position(|row| row.name == "same"));
        let (best, text, _) = table.best().unwrap();
        assert_eq!(best.name, "unscaled");
        // Kept only over the text the run read, and nothing left beside it.
        assert!(keep(&draft, "int other;\n", text).is_err());
        assert_eq!(std::fs::read_to_string(&draft).unwrap(), original);
        keep(&draft, original, text).unwrap();
        let kept = original.replace("a * STEP", "a");
        assert_eq!(std::fs::read_to_string(&draft).unwrap(), kept);
        assert_eq!(std::fs::read_dir(draft.parent().unwrap()).unwrap().count(), 2);
        // Nothing improves on a match.
        first.compile(&kept).unwrap();
        let table = tabulate(&bench, &first, &reference, &kept, &read_variants("worse\n- a - b\n+ b - a\n").unwrap(), 1, 1).unwrap();
        assert!(matches!(table.outcome(&table.rows[1]), Some(Ok(figures)) if figures.differ > 0));
        assert!(table.best().is_none());
    }
}
