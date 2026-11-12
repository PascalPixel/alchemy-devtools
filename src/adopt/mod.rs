//! adopt: gathers a game's objects into one, editing its linker script, and
//! keeps a journal that undoes it, whole or one item at a time. The build
//! still judges what a merge did (`make compare-editions`), `check rom`
//! traces a failure to the object that placed the first differing byte, and
//! `--undo JOURNAL ITEM` sends that merge back while the rest of the batch
//! stays.

use crate::alchemy::built;
use crate::alchemy::layout::Layout;
use crate::alchemy::link::{self, statement_stem};
use crate::alchemy::targets::DecompTargetId;
use crate::compile::root;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod merge;
mod runs;
mod settle;
pub(crate) mod views;

const USAGE: &str = r#"usage: alchemy-devtools adopt merge [--keep-views] INTO.c=OBJECT.c,OBJECT.c... [--verify | --dry-run] [--no-modern]
       alchemy-devtools adopt merge --keep-views --game GAME [--verify | --dry-run] [--no-modern]
       alchemy-devtools adopt --undo JOURNAL [ITEM...]
       alchemy-devtools adopt settle --game GAME [--dry-run] [--no-modern] [OBJECT.c...]  (adopt settle --help)
--keep-views joins without settling any disagreement between the sources:
each becomes a part of INTO after a `// ---- SOURCE.c ----` line, and where
a part sees a name otherwise than one before it, it keeps its own view: a
routine or variable under a private name bound to the same symbol
(`extern int Helper__s0b84(struct Thing *) __asm__("Helper");`; where the
later part holds the definition, the earlier takes the view), a tag,
typedef, enumerator, static or macro under its own `__PART` name (a
header's around the #include that first enters it). A part's macros are
undefined where it ends; identical repeats of a type, typedef or inline
routine are deleted. Each view is marked on the line above it, for check
tags and progress --humanity to count:
  /* VIEW(PART): KIND NAME as THIS; OTHER has THAT. */
KIND is routine, variable, tag, typedef, enumerator, static or macro. The
compiler judges: the join is compiled in every edition that links it until
its messages name nothing more, then every function and datum is held
against the build's own assembly of its file (out/EDITION/obj/SOURCE.s,
.L labels renumbered). For one that moved, the name whose meaning changed
for it is looked for (a routine declared otherwise than -aux-info saw it in
its own file, a macro defined otherwise than -dM listed it) and kept as a
view too; a part nothing reconciles is left out, saying why, and the
objects either side of it join without it. --game GAME takes every run of
the game's linker scripts: C objects on the game's route whose statements
stand together in every section they place, compiled by one command in
every edition, never across a bank's start or an overlay, never a file the
Makefile names; each joins into its scene's own name where it is that
scene's code and the name is free, else into its first object. Runs settle
in parallel and are written as one batch, whose journal records the runs
joined.
merge: the objects become one object built from INTO.c, a new file or one of
the objects' own. Its source is theirs one after another, in the order given:
an #include of a header an earlier source includes goes where neither is
inside an #if, a later source's /// @file comment becomes a // comment, and
a relative #include still reaches its file from INTO's folder; a #define or
static of one source reaches the sources after it. In the game's linker scripts
(games/GAME/ld_script.ld and the overlay.ld it includes) every statement
that places one of the objects is one of a run: statements on lines next to
each other (blank lines aside) that place the same sections, the objects in
the order given. A run's first statement places INTO's object, and the rest
go. An object a run leaves out must place no bytes in its sections, by its
built object in every edition that links it. Refused: an object of assembly
or with no statement; INTO already linked or built from another source; a
run's objects apart (a line of any other kind between, as a bank's start, an
#if or a comment); objects that some edition links into the main image and
another only into overlays, or that the Makefile compiles with different
commands in some edition (a route or an option; The Lost Age's C linked only
into overlays builds for size); a new INTO whose name the Makefile names
or a $(wildcard) of it would take, or whose own recipe, as make gives its
name, is not the objects' (an ARM_C, SOUND_C or FLASH_C object merged into a
name on no route, or game C into one on a route).
A merge changes no compiler, option or route, but it can move bytes: the
pool and alignment between functions that were apart, a static's order. The
build judges: make compare-editions, then check rom. --verify has the build
judge at once: it makes every edition of the batch's games (make -k
compare-EDITION), and while one differs, sends back the item that wrote the
object placing the first byte it differs in (as --undo JOURNAL ITEM does),
or the item whose file make's errors name for an edition that did not build,
and builds again. An edition that links nothing the batch wrote is judged by
the image it kept. It stops, naming the journal, when no edition's failure
can be blamed on an item: one did not build and make names no item's file,
or one differs first in something no item wrote.
A join with views is also compiled as make modern compiles its object, in
every edition, and must have no error; a part whose code a current GCC
refuses is left out. Its scratch files are written outside the checkout.
Once a batch is written (and verified), make modern-GAME builds each of its
games, and an item whose file its errors name goes back, as --verify sends
one back; --no-modern skips both.
--dry-run prints what would change. Otherwise the changes are written with a
journal under out/adopt/ that --undo reverses: a file changed since (a later
merge's edits to the same script) has only the journal's own change taken
out, by a three-way merge of its lines, and the undo refuses, writing
nothing, where the two changed the same lines. Journals undo in any order.
--undo JOURNAL ITEM... backs out only those items (an item, its INTO or one
of its objects): the journal is undone, each edition's object list and the
objects it put back are made again, and its merges run again without them,
under a new journal. A merge reads the lists and objects the last make
wrote, and refuses one older than what it is made from. One adopt or undo runs at a time (out/adopt/lock),
and none writes over a file that changed after it read it."#;

#[derive(Debug)]
struct Options {
    items: Vec<String>,
    /// Whether the build judges the batch after it is written.
    verify: bool,
    dry_run: bool,
    /// Whether make modern judges the batch once it is written (not `--no-modern`).
    modern: bool,
    /// Joins that keep each disagreement as a view (`--keep-views`).
    keep_views: bool,
    /// A game whose every run is joined (`--game GAME`, with `--keep-views`).
    game: Option<String>,
}

enum Request {
    Adopt(Options),
    /// A journal to undo, and the items to back out of it alone.
    Undo(PathBuf, Vec<String>),
}

fn parse(arguments: &[String]) -> Result<Request, String> {
    let mut form = None;
    let mut items = Vec::new();
    let mut verify = false;
    let mut dry_run = false;
    let mut modern = true;
    let mut keep_views = false;
    let mut game = None;
    let mut skip = false;
    for (i, value) in arguments.iter().map(String::as_str).enumerate() {
        if std::mem::take(&mut skip) {
            continue;
        }
        match value {
            "--dry-run" => dry_run = true,
            "--no-modern" => modern = false,
            "--keep-views" => keep_views = true,
            "--game" => {
                game = Some(arguments.get(i + 1).filter(|g| !g.starts_with('-')).ok_or_else(|| USAGE.to_string())?.clone());
                skip = true;
            }
            "--verify" => verify = true,
            "--undo" => {
                let journal = arguments.get(i + 1).filter(|_| i == 0);
                let items = arguments.get(2..).unwrap_or_default().to_vec();
                if items.iter().any(|item| item.starts_with('-')) {
                    return Err(USAGE.into());
                }
                return journal.map(|j| Request::Undo(PathBuf::from(j), items)).ok_or_else(|| USAGE.to_string());
            }
            "-h" | "--help" => return Err(USAGE.into()),
            _ if value.starts_with('-') => return Err(format!("unknown adopt option: {value}\n{USAGE}")),
            _ if form.is_none() => {
                if value != "merge" {
                    return Err(format!("unknown adopt form: {value}\n{USAGE}"));
                }
                form = Some(value);
            }
            _ => items.push(value.to_string()),
        }
    }
    let wanted = items.is_empty() != game.is_some();
    if form.is_none() || wanted || (verify && dry_run) || (game.is_some() && !keep_views) {
        return Err(USAGE.into());
    }
    Ok(Request::Adopt(Options { items, verify, dry_run, modern, keep_views, game }))
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    if arguments.first().map(String::as_str) == Some("settle") {
        return settle::run(&arguments[1..]);
    }
    if arguments.iter().any(|argument| matches!(argument.as_str(), "-h" | "--help")) {
        println!("{USAGE}");
        return Ok(());
    }
    match parse(arguments)? {
        Request::Undo(journal, items) if items.is_empty() => undo(root(), &journal),
        Request::Undo(journal, items) => back_out(root(), &journal, &items),
        Request::Adopt(options) => adopt(root(), &options, &command_line(arguments), &Checkout(root())),
    }
}

/// The command a journal records, which `back_out` reads back as words.
fn command_line(arguments: &[String]) -> String {
    format!("adopt {}", arguments.join(" "))
}

/// Whether `given` names `item`: the item itself, its INTO or one of its
/// objects, with or without the folders before it.
fn names(item: &str, given: &str) -> bool {
    item == given || item.split([',', ' ', '=']).any(|part| part == given || part.ends_with(&format!("/{given}")) || given.ends_with(&format!("/{part}")))
}

/// Backs `items` out of a journal's merges: the journal is undone and the
/// merges it records run again without them, so the rest stay.
fn back_out(root: &Path, journal: &Path, items: &[String]) -> Result<(), String> {
    back_out_batch(root, journal, items, &Checkout(root)).map(|_| ())
}

/// `back_out`, giving the batch written again, if any item is left.
fn back_out_batch(root: &Path, journal: &Path, items: &[String], build: &dyn Build) -> Result<Option<Batch>, String> {
    let path = [std::env::current_dir().map(|d| d.join(journal)).ok(), Some(root.join(journal))]
        .into_iter()
        .flatten()
        .find(|path| path.is_file())
        .ok_or_else(|| format!("cannot read {}", journal.display()))?;
    let bytes = std::fs::read(&path).map_err(|error| format!("cannot read {}: {error}", journal.display()))?;
    let command = read_journal(&bytes)?.0.ok_or("the journal records no command")?;
    let words: Vec<String> = command.split_whitespace().skip(1).map(str::to_string).collect();
    let Request::Adopt(mut options) = parse(&words)? else { return Err("the journal records an undo".into()) };
    if let Some(unknown) = items.iter().find(|given| !options.items.iter().any(|item| names(item, given))) {
        return Err(format!("{unknown} is not among the journal's items: {}", options.items.join(" ")));
    }
    options.items.retain(|item| !items.iter().any(|given| names(item, given)));
    undo(root, journal)?;
    if options.items.is_empty() {
        return Ok(None);
    }
    // The last build listed the batch's merged objects, not the ones the undo
    // put back: the lists and those objects are made again before the rest
    // are judged by them.
    let mut games: Vec<String> = Vec::new();
    let mut objects: Vec<String> = Vec::new();
    for item in &options.items {
        let (into, stems) = item_parts(item)?;
        let game = Layout::of_path(&into).ok_or_else(|| format!("{into} names no game"))?.game;
        if !games.contains(&game) {
            games.push(game);
        }
        objects.extend(stems);
    }
    build.refresh(&games, &objects)?;
    // The rest are verified by whoever called for the back-out.
    let words: Vec<String> = words.into_iter().filter(|word| !items.iter().any(|given| names(word, given)) && word != "--verify").collect();
    options.verify = false;
    adopt_batch(root, &options, &command_line(&words), build)
}

/// A batch written: its journal, its games, and each item with the source
/// stems it wrote under games/.
struct Batch {
    journal: String,
    games: Vec<String>,
    items: Vec<(String, Vec<String>)>,
}

/// Whether make's errors say `edition`'s own build failed: make names a
/// target it gave up on as `[tla-en]` (3.81) or `[Makefile:55: tla-en]`.
fn failed(error: &str, edition: &str) -> bool {
    error.lines().filter(|line| line.contains("*** [")).any(|line| line.contains(&format!("[{edition}]")) || line.contains(&format!(" {edition}]")))
}

fn adopt(root: &Path, options: &Options, command: &str, build: &dyn Build) -> Result<(), String> {
    let Some(mut batch) = adopt_batch(root, options, command, build)? else { return Ok(()) };
    if options.verify {
        match verify(root, batch, build)? {
            Some(rest) => batch = rest,
            None => return Ok(()),
        }
    }
    if options.modern {
        modern(root, batch, build)?;
    }
    Ok(())
}

/// `make modern-GAME`: every edition's C of `game` through a current GCC,
/// which must have no error (the old compilers forgive what it refuses).
fn modern_gate(root: &Path, game: &str) -> Result<(), String> {
    let jobs = std::thread::available_parallelism().map_or(1, |n| n.get());
    let arguments = vec!["make".to_string(), "-k".into(), format!("-j{jobs}"), format!("modern-{game}")];
    println!("modern: {}", arguments.join(" "));
    crate::gba::process::run(&arguments, root).map(|_| ())
}

/// Builds the batch's games with `make modern`, backing out each item whose
/// file its errors name, until it passes; an error no item's file holds
/// stops it, naming the journal.
fn modern(root: &Path, mut batch: Batch, build: &dyn Build) -> Result<(), String> {
    loop {
        let Err(error) = build.modern(&batch.games) else {
            println!("modern: every {} edition's C compiles with a current GCC", batch.games.join(" and "));
            return Ok(());
        };
        let blamed: Vec<String> = batch.items.iter().filter(|(_, stems)| stems.iter().any(|stem| error.contains(&format!("{stem}.c:")))).map(|(item, _)| item.clone()).collect();
        if blamed.is_empty() {
            let tail: Vec<&str> = error.lines().filter(|line| line.contains("error")).take(6).collect();
            return Err(format!(
                "make modern failed in no file this batch wrote:\n  {}\nundo the batch with adopt --undo {}",
                tail.join("\n  "),
                batch.journal
            ));
        }
        for item in &blamed {
            println!("modern: {item} does not compile with a current GCC: it goes back");
        }
        match back_out_batch(root, Path::new(&batch.journal), &blamed, build)? {
            Some(rest) => batch = rest,
            None => {
                println!("modern: every item went back");
                return Ok(());
            }
        }
    }
}

/// Builds every edition of the batch's game and backs out the item the build
/// blames, by what placed the first byte an edition differs in or by the file
/// make's errors name, until every edition compares.
fn verify(root: &Path, mut batch: Batch, build: &dyn Build) -> Result<Option<Batch>, String> {
    let ids = built::editions(&batch.games)?;
    loop {
        let Err(error) = build.compare(&ids) else {
            println!("verified: every {} edition matches its ROM", batch.games.join(" and "));
            return Ok(Some(batch));
        };
        let written = std::fs::metadata(root.join(&batch.journal)).and_then(|meta| meta.modified()).map_err(|error| format!("{}: {error}", batch.journal))?;
        let wrote: Vec<&String> = batch.items.iter().flat_map(|(_, stems)| stems).collect();
        let mut blamed: Vec<String> = Vec::new();
        let mut blame = |item: &str| {
            if !blamed.iter().any(|known| known == item) {
                blamed.push(item.to_string());
            }
        };
        // What no item can be blamed for, edition by edition.
        let mut unblamed: Vec<String> = Vec::new();
        for &id in &ids {
            let lists = build.linked(id).ok();
            // An edition the batch changed relinks after its journal was
            // written; one that links nothing the batch wrote keeps its image
            // and is judged by it, as one make did not fail on.
            let changed = lists.as_ref().is_none_or(|lists| wrote.iter().any(|stem| lists.links(stem)));
            if failed(&error, id.as_str()) || (changed && !build.relinked(id, written)) {
                // A merged source that does not compile names itself.
                let named: Vec<&String> = batch
                    .items
                    .iter()
                    .filter(|(_, stems)| stems.iter().any(|stem| error.contains(&format!("{stem}.c:"))))
                    .map(|(item, _)| item)
                    .collect();
                if named.is_empty() {
                    unblamed.push(format!("{id} did not build, and make's errors name no item's file"));
                }
                for item in named {
                    println!("verify: {id} did not build: {item} goes back");
                    blame(item);
                }
                continue;
            }
            let Some(difference) = build.difference(id)? else { continue };
            // The object that placed the byte, or an overlay whose stream the
            // ROM lacks that links what the item wrote.
            let lists = lists.unwrap_or_default();
            let culprit = batch.items.iter().find(|(_, stems)| {
                difference.object.as_ref().is_some_and(|object| stems.contains(object))
                    || difference.overlays.iter().filter_map(|overlay| lists.overlay(overlay)).any(|objects| stems.iter().any(|stem| objects.contains(stem)))
            });
            match culprit {
                Some((item, _)) => {
                    println!("verify: {id} first differs at {}: {item} goes back", difference.label);
                    blame(item);
                }
                None => unblamed.push(format!("{id} first differs at {}, which no item of this batch wrote", difference.label)),
            }
        }
        if blamed.is_empty() {
            if unblamed.is_empty() {
                return Err(format!("make failed, yet every {} edition matches: {error}", batch.games.join(" and ")));
            }
            let tail: Vec<&str> = error.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();
            return Err(format!(
                "{}:\n  {}\nback an item out with adopt --undo {journal} ITEM, or undo the batch with adopt --undo {journal}",
                unblamed.join("\n"),
                tail.join("\n  "),
                journal = batch.journal
            ));
        }
        for left in &unblamed {
            println!("verify: {left}; judged again once the blamed items go back");
        }
        match back_out_batch(root, Path::new(&batch.journal), &blamed, build)? {
            Some(rest) => batch = rest,
            None => {
                println!("verify: every item went back");
                return Ok(None);
            }
        }
    }
}

fn adopt_batch(root: &Path, options: &Options, command: &str, build: &dyn Build) -> Result<Option<Batch>, String> {
    // Held from the first read to the last write, so no other adoption
    // edits the files this one reads; a dry run writes nothing.
    let _lock = if options.dry_run { None } else { Some(Lock::take(root)?) };
    let mut files = Files::new(root);
    let mut adopted = 0;
    let mut refused = 0;
    let mut written: Vec<(String, Vec<String>)> = Vec::new();
    let mut games: Vec<String> = Vec::new();
    let items = match &options.game {
        Some(game) => runs::items(root, game)?,
        None => options.items.clone(),
    };
    // With views, each item becomes the runs its views settled, each with
    // its text: compiled and judged first, in parallel, writing nothing.
    let planned: Vec<(String, Option<String>)> = match options.keep_views {
        true => runs::settle(root, &mut files, &items, options.modern)?,
        false => items.into_iter().map(|item| (item, None)).collect(),
    };
    for (item, text) in &planned {
        let saved = files.now.clone();
        match merge_objects(&mut files, item, build, text.clone()) {
            Ok((merged_game, lines)) => {
                adopted += 1;
                for line in lines {
                    println!("{line}");
                }
                // What the item wrote under games/: the objects the build
                // can blame it by.
                let stems: Vec<String> = files
                    .now
                    .iter()
                    .filter(|(path, now)| saved.get(*path) != Some(*now) && now.is_some() && path.starts_with("games/"))
                    .filter_map(|(path, _)| path.strip_suffix(".c"))
                    .map(str::to_string)
                    .collect();
                written.push((item.clone(), stems));
                if !games.contains(&merged_game) {
                    games.push(merged_game);
                }
            }
            Err(why) => {
                files.now = saved;
                refused += 1;
                println!("refused {item}: {why}");
            }
        }
    }
    if games.is_empty() {
        return Err(format!("nothing adopted ({refused} refused)"));
    }
    if options.dry_run {
        println!("dry run: {adopted} would be merged, {refused} refused; nothing written");
        return Ok(None);
    }
    // A batch with views records the runs it joined, which a back-out
    // settles again without the items it names.
    let command = match options.keep_views {
        true => format!("adopt merge --keep-views {}", written.iter().map(|(item, _)| item.as_str()).collect::<Vec<_>>().join(" ")),
        false => command.to_string(),
    };
    let journal = files.commit(&command, &journal_name(&written))?;
    println!("{adopted} merged, {refused} refused; journal {journal}");
    if !options.verify {
        println!("judge with make compare-editions, or adopt --verify; undo with alchemy-devtools adopt --undo {journal} [ITEM...]");
    }
    Ok(Some(Batch { journal, games, items: written }))
}

/// What a journal is named after: its form and first item's INTO.
fn journal_name(items: &[(String, Vec<String>)]) -> String {
    let first = items[0].0.split([' ', ',', '=']).next().unwrap_or("");
    let first: String = first
        .rsplit('/')
        .next()
        .unwrap_or(first)
        .trim_end_matches(".c")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '-' })
        .collect();
    format!("merge-{first}")
}

// ---- merge ----

/// What a merge asks of the built checkout.
trait Build {
    /// Whether `stem`'s built object places no bytes in `sections` (a
    /// statement's list, `*` matching any tail) in any edition that links it.
    fn empty(&self, stem: &str, sections: &str) -> Result<bool, String>;
    /// Ok when every edition of `game` builds `stems` alike: all in its main
    /// image or none, by the same commands but for the file names; and a new
    /// `into` by those commands too, its name on their route.
    fn alike(&self, game: &str, stems: &[String], into: &str) -> Result<(), String>;
    /// Makes each of `games`' editions' lists, and the objects of `stems`
    /// each links, as the tree now is.
    fn refresh(&self, games: &[String], stems: &[String]) -> Result<(), String>;
    /// Makes and compares every edition of `ids` (`make -k`): make's
    /// errors when one fails.
    fn compare(&self, ids: &[DecompTargetId]) -> Result<(), String>;
    /// Makes every edition of `games` with `make modern`: make's errors
    /// when one fails.
    fn modern(&self, games: &[String]) -> Result<(), String>;
    /// The objects an edition's last build listed.
    fn linked(&self, id: DecompTargetId) -> Result<link::Linked, String>;
    /// Whether the edition's image was written at `since` or after.
    fn relinked(&self, id: DecompTargetId, since: std::time::SystemTime) -> bool;
    /// The first byte the edition's image differs from its ROM in, and what
    /// placed it.
    fn difference(&self, id: DecompTargetId) -> Result<Option<crate::check::rom::Difference>, String>;
}

/// The checkout's build, as the last make left it: a list or object older
/// than what it is made from is refused, not read.
struct Checkout<'a>(&'a Path);

/// Whether a section name matches a linker-script pattern: whole, or with a
/// `*` matching the rest.
fn matches(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        Some((head, tail)) => name.len() >= head.len() + tail.len() && name.starts_with(head) && name.ends_with(tail),
        None => pattern == name,
    }
}

/// Every C source a makefile names by its path (`games/tla/src/x.c`), its
/// `$(addprefix FOLDER/,a.c b.c)` lists written out and its `$(wildcard)`
/// patterns kept as patterns: what may route a file, matched by path, so a
/// scene's own `flash.c` is not FLASH_C's `gamepak/flash/flash.c`.
fn named_sources(makefile: &str) -> Vec<String> {
    let text = makefile.replace("\\\n", " ");
    let mut expanded = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(at) = rest.find("$(addprefix ") {
        expanded.push_str(&rest[..at]);
        let inside = &rest[at + "$(addprefix ".len()..];
        let Some(end) = inside.find(')') else {
            rest = &rest[at..];
            break;
        };
        match inside[..end].split_once(',') {
            Some((prefix, list)) => {
                for word in list.split_whitespace() {
                    expanded.push_str(&format!(" {}{word} ", prefix.trim()));
                }
            }
            None => expanded.push_str(&inside[..end]),
        }
        rest = &inside[end + 1..];
    }
    expanded.push_str(rest);
    let mut found: Vec<String> = Vec::new();
    for word in expanded.split(|c: char| c.is_whitespace() || matches!(c, ',' | '(' | ')')) {
        if word.ends_with(".c") && word.contains('/') && !word.contains('$') && !found.iter().any(|known| known == word) {
            found.push(word.to_string());
        }
    }
    found
}

/// Whether a makefile names `source` (a path from the root) by its path or
/// a pattern of it.
fn makefile_names(makefile: &str, source: &str) -> bool {
    named_sources(makefile).iter().any(|named| matches(named, source))
}

/// Whether a C text includes another C file as text (`#include "x.c"`):
/// such a file is more than its own text, so no view of it can be written
/// in its own text alone.
fn includes_source(text: &str) -> bool {
    text.lines().filter_map(|line| include_of(line.trim_start())).any(|named| named.strip_prefix('"').and_then(|n| n.split('"').next()).is_some_and(|n| n.ends_with(".c")))
}

/// The patterns of a makefile's `$(wildcard ...)` calls that name C files.
fn wildcard_patterns(makefile: &str) -> Vec<String> {
    let mut patterns = Vec::new();
    for (at, call) in makefile.match_indices("$(wildcard ") {
        let inside = makefile[at + call.len()..].split(')').next().unwrap_or("");
        patterns.extend(inside.split_whitespace().filter(|word| word.contains('*') && word.ends_with(".c")).map(str::to_string));
    }
    patterns
}

/// Whether two recipes are one but for -O2 in `unplaced` where `placed`
/// builds for size (-Os), as a Makefile builds C linked only into overlays.
fn same_but_size(unplaced: &[Vec<String>], placed: &[Vec<String>]) -> bool {
    unplaced.len() == placed.len()
        && unplaced.iter().zip(placed).all(|(a, b)| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x == y || (x == "-O2" && y == "-Os")))
}

impl Build for Checkout<'_> {
    fn empty(&self, stem: &str, sections: &str) -> Result<bool, String> {
        let game = Layout::of_path(stem).ok_or_else(|| format!("{stem} names no game"))?.game;
        let mut read = 0;
        for id in built::editions(&[game])? {
            if !link::linked(self.0, id)?.links(stem) {
                continue;
            }
            let path = built::object(self.0, id, stem);
            let data = std::fs::read(&path).map_err(|_| format!("{} is not built: make {id} first", path.display()))?;
            let modified = |path: &Path| std::fs::metadata(path).and_then(|meta| meta.modified()).ok();
            if let Some(source) = built::source(self.0, stem).filter(|source| modified(&self.0.join(source)) > modified(&path)) {
                return Err(format!("{} is older than {source}: make {id} first", path.display()));
            }
            let headers = crate::gba::elf::headers(&data).map_err(|error| format!("{}: {error}", path.display()))?;
            read += 1;
            let placed = headers.iter().any(|header| header.size > 0 && sections.split_whitespace().any(|pattern| matches(pattern, &header.name)));
            if placed {
                return Ok(false);
            }
        }
        if read == 0 {
            return Err(format!("no edition links {stem}"));
        }
        Ok(true)
    }

    fn alike(&self, game: &str, stems: &[String], into: &str) -> Result<(), String> {
        let makefile = built::read_text(&self.0.join("Makefile"))?;
        let anew = !stems.iter().any(|stem| format!("{stem}.c") == into);
        if anew && makefile_names(&makefile, into) {
            return Err(format!("the Makefile names {into}, so it may take another route"));
        }
        // A $(wildcard) names only the files there when make reads it, so its
        // pattern would catch INTO once it is written.
        if let Some(pattern) = anew.then(|| wildcard_patterns(&makefile).into_iter().find(|pattern| matches(pattern, into))).flatten() {
            return Err(format!("the Makefile's {pattern} would take {into}, so it may take another route"));
        }
        for id in built::editions(&[game.to_string()])? {
            let linked = link::linked(self.0, id)?;
            let main: Vec<&String> = stems.iter().filter(|stem| linked.main.contains(stem)).collect();
            if !main.is_empty() && main.len() < stems.len() {
                let other = stems.iter().find(|stem| !main.contains(stem)).unwrap();
                return Err(format!("{id} links {} into its main image and {other} only into overlays", main[0]));
            }
            let overlays_only = main.is_empty() && stems.iter().any(|stem| linked.links(stem));
            let target = crate::alchemy::targets::target_for(id);
            let mut wanted: Vec<(String, String)> = stems.iter().map(|stem| (format!("{stem}.c"), format!("{}/obj/{stem}.o", target.output_dir))).collect();
            // A new INTO's own recipe, as make gives its name before any list
            // places it: its route is the one its name takes.
            if anew {
                let stem = into.strip_suffix(".c").unwrap_or(into);
                wanted.push((into.to_string(), format!("{}/obj/{stem}.o", target.output_dir)));
            }
            let recipes = crate::gba::recipe::recipes(self.0, Path::new(target.output_dir), &[format!("EDITION={id}")], &wanted)?;
            let mut first: Option<(String, Vec<Vec<String>>)> = None;
            for ((source, object), recipe) in wanted.iter().zip(recipes) {
                let recipe = recipe.map_err(|error| format!("{source}: {error}"))?;
                let steps = crate::gba::recipe::retarget(&recipe, self.0, source, object, Path::new("/merge/object.c"), Path::new("/merge/object.o"));
                match &first {
                    None => first = Some((source.clone(), steps)),
                    // INTO is placed where the objects are, so where they
                    // build for size and it, unplaced, does not, that is the
                    // one difference allowed.
                    Some(_) if anew && source == into && overlays_only && first.as_ref().is_some_and(|(_, theirs)| same_but_size(&steps, theirs)) => {}
                    Some((other, theirs)) if *theirs != steps => {
                        let mine = steps.concat();
                        let theirs = theirs.concat();
                        let differ = mine.iter().find(|word| !theirs.contains(word)).or_else(|| theirs.iter().find(|word| !mine.contains(word)));
                        let route = if source == into { " (INTO's route)" } else { "" };
                        return Err(format!("{id} compiles {source} and {other} differently{route} ({})", differ.map_or("in order", String::as_str)));
                    }
                    Some(_) => {}
                }
            }
        }
        Ok(())
    }

    fn refresh(&self, games: &[String], stems: &[String]) -> Result<(), String> {
        for id in built::editions(games)? {
            let output = crate::alchemy::targets::target_for(id).output_dir;
            let make = |targets: Vec<String>| {
                let mut arguments = vec!["make".to_string(), format!("EDITION={id}")];
                arguments.extend(targets);
                crate::gba::process::run(&arguments, self.0).map(|_| ())
            };
            make(vec![format!("{output}/objects.mk")])?;
            let linked = link::linked(self.0, id)?;
            let objects: Vec<String> = stems.iter().filter(|stem| linked.links(stem)).map(|stem| format!("{output}/obj/{stem}.o")).collect();
            if !objects.is_empty() {
                println!("back-out: {id} makes {} objects again", objects.len());
                make(objects)?;
            }
        }
        Ok(())
    }

    fn compare(&self, ids: &[DecompTargetId]) -> Result<(), String> {
        let jobs = std::thread::available_parallelism().map_or(1, |n| n.get());
        let mut arguments = vec!["make".to_string(), "-k".into(), format!("-j{jobs}")];
        arguments.extend(ids.iter().map(|id| format!("compare-{id}")));
        println!("verify: make {}", arguments[1..].join(" "));
        crate::gba::process::run(&arguments, self.0).map(|_| ())
    }

    fn modern(&self, games: &[String]) -> Result<(), String> {
        games.iter().try_for_each(|game| modern_gate(self.0, game))
    }

    fn linked(&self, id: DecompTargetId) -> Result<link::Linked, String> {
        link::linked(self.0, id)
    }

    fn relinked(&self, id: DecompTargetId, since: std::time::SystemTime) -> bool {
        let image = self.0.join(crate::alchemy::targets::target_for(id).image());
        std::fs::metadata(image).and_then(|meta| meta.modified()).is_ok_and(|time| time >= since)
    }

    fn difference(&self, id: DecompTargetId) -> Result<Option<crate::check::rom::Difference>, String> {
        crate::check::rom::first_difference(self.0, id)
    }
}

/// One statement of a linker script: the line it is on, the object it
/// places and the sections it names.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Statement {
    line: usize,
    stem: String,
    sections: String,
}

fn statement(index: usize, line: &str) -> Option<Statement> {
    let stem = statement_stem(line)?;
    let rest = line.split_once(&format!("\"*/{stem}.o\"("))?.1;
    let sections = rest.rsplit_once(')')?.0.trim().to_string();
    Some(Statement { line: index, stem: stem.to_string(), sections })
}

/// Runs of the statements placing `objects` in one script's lines: each run
/// statements on lines next to each other (blank lines aside), all naming the
/// same sections, their objects in `objects`' order.
fn runs(lines: &[&str], objects: &[String]) -> Vec<Vec<Statement>> {
    let mut runs: Vec<Vec<Statement>> = Vec::new();
    let mut open = false;
    for (index, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let Some(found) = statement(index, line).filter(|found| objects.contains(&found.stem)) else {
            open = false;
            continue;
        };
        let rank = |stem: &str| objects.iter().position(|object| object == stem);
        let joins = open
            && runs.last().and_then(|run| run.last()).is_some_and(|last| last.sections == found.sections && rank(&last.stem) < rank(&found.stem));
        if joins {
            runs.last_mut().unwrap().push(found);
        } else {
            runs.push(vec![found]);
        }
        open = true;
    }
    runs
}

/// A later source as it joins the merged text: an `#include` of a header the
/// text already includes, both outside any `#if`, goes, and its `/// @file`
/// comment becomes `//`.
fn joined(text: &str, included: &[String]) -> String {
    let mut out = String::new();
    let mut file_comment = false;
    let mut depth = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        let outside = depth == 0;
        depth = conditional_depth(trimmed, depth);
        if trimmed == "/// @file" {
            file_comment = true;
            continue;
        }
        if file_comment {
            if let Some(rest) = line.strip_prefix("///") {
                out.push_str("//");
                out.push_str(rest);
                continue;
            }
            file_comment = false;
        }
        if outside && include_of(trimmed).is_some_and(|header| header.ends_with(".h\"") && included.iter().any(|known| known == trimmed)) {
            continue;
        }
        out.push_str(line);
    }
    out
}

/// The `#if` depth after a line, from the depth before it.
fn conditional_depth(line: &str, depth: usize) -> usize {
    let Some(directive) = line.strip_prefix('#').map(str::trim_start) else { return depth };
    if ["if", "ifdef", "ifndef"].iter().any(|word| directive.split_whitespace().next() == Some(word)) {
        depth + 1
    } else if directive.starts_with("endif") {
        depth.saturating_sub(1)
    } else {
        depth
    }
}

/// The `#include` lines of a text outside any `#if`.
fn top_level_includes(text: &str) -> Vec<String> {
    let mut depth = 0;
    let mut found = Vec::new();
    for line in text.lines().map(str::trim) {
        if depth == 0 && include_of(line).is_some() {
            found.push(line.to_string());
        }
        depth = conditional_depth(line, depth);
    }
    found
}

/// The quoted file an `#include` line names.
fn include_of(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('#')?.trim_start().strip_prefix("include")?.trim();
    (rest.starts_with('"') || rest.starts_with('<')).then_some(rest)
}

/// An item's INTO and objects, as stems.
fn item_parts(item: &str) -> Result<(String, Vec<String>), String> {
    let (into, objects) = item.split_once('=').ok_or("an item is INTO.c=OBJECT.c,OBJECT.c...")?;
    let stem = |path: &str| {
        let path = path.trim_start_matches("./");
        path.strip_suffix(".c").unwrap_or(path).to_string()
    };
    Ok((stem(into), objects.split(',').filter(|o| !o.is_empty()).map(stem).collect()))
}

/// Merges one item, `INTO.c=OBJECT.c,OBJECT.c...`: its game and report.
fn merge_objects(files: &mut Files, item: &str, build: &dyn Build, joined_text: Option<String>) -> Result<(String, Vec<String>), String> {
    let (into, objects) = item_parts(item)?;
    if objects.len() < 2 {
        return Err("a merge takes two objects or more".into());
    }
    if let Some(twice) = objects.iter().enumerate().find_map(|(i, o)| objects[..i].contains(o).then_some(o)) {
        return Err(format!("{twice} is named twice"));
    }
    let layout = Layout::of_path(&into).filter(|layout| into.starts_with(&layout.sources())).ok_or_else(|| format!("{into} is not under games/GAME/src"))?;
    if let Some(other) = objects.iter().find(|o| !o.starts_with(&layout.sources())) {
        return Err(format!("{other} is not under {}", layout.sources()));
    }
    for object in &objects {
        if !files.exists(&format!("{object}.c")) {
            let assembly = [".s", ".S"].iter().any(|extension| files.exists(&format!("{object}{extension}")));
            return Err(match assembly {
                true => format!("{object} is assembly, which stays its own object"),
                false => format!("{object} has no C source"),
            });
        }
    }
    let included = included_sources(&files.root, &layout.game);
    if let Some((source, by)) = objects.iter().find_map(|object| included.get_key_value(&format!("{object}.c"))) {
        return Err(format!("{by} includes {source}, which stays the file it is"));
    }
    // A view is written in a part's own text: one that includes C as text
    // would leave what it includes seeing the names as before.
    if joined_text.is_some() {
        for object in &objects {
            if includes_source(&files.read(&format!("{object}.c"))?) {
                return Err(format!("{object}.c includes C files as text, which a join with views cannot reach"));
            }
        }
    }
    let target = format!("{into}.c");
    let anew = !objects.contains(&into);
    if anew {
        if let Some(found) = [".c", ".s", ".S", ".mid"].iter().map(|x| format!("{into}{x}")).find(|path| files.exists(path)) {
            return Err(format!("{found} exists"));
        }
    }

    // Every statement placing the objects, as runs of each script.
    let mut edits: Vec<(String, Vec<String>, bool)> = Vec::new();
    let mut placed = vec![0; objects.len()];
    let mut report = vec![format!("merge {} -> {target}", objects.join(" "))];
    for script in layout.linker_scripts() {
        if !files.exists(&script) {
            continue;
        }
        let text = files.read(&script)?;
        let (lines, newline) = lines(&text);
        if anew && lines.iter().filter_map(|line| statement_stem(line)).any(|found| found == into) {
            return Err(format!("{script} already places {into}"));
        }
        let mut out: Vec<String> = lines.iter().map(|line| line.to_string()).collect();
        let mut gone: Vec<usize> = Vec::new();
        for run in runs(&lines, &objects) {
            for (index, object) in objects.iter().enumerate() {
                if run.iter().any(|s| s.stem == *object) {
                    placed[index] += 1;
                } else if !build.empty(object, &run[0].sections)? {
                    return Err(format!(
                        "{object} places bytes in ({}) that the statements from {script}:{} leave out: the objects must lie together, in the order given, wherever they are placed",
                        run[0].sections,
                        run[0].line + 1
                    ));
                }
            }
            let first = &run[0];
            out[first.line] = lines[first.line].replacen(&format!("\"*/{}.o\"", first.stem), &format!("\"*/{into}.o\""), 1);
            gone.extend(run[1..].iter().map(|s| s.line));
            report.push(format!("  {script}:{}  ({}) places {} in one statement", first.line + 1, first.sections, run.len()));
        }
        if gone.is_empty() && out.iter().zip(&lines).all(|(a, b)| a == b) {
            continue;
        }
        gone.sort_unstable();
        for line in gone.into_iter().rev() {
            out.remove(line);
        }
        edits.push((script, out, newline));
    }
    if let Some(index) = placed.iter().position(|&count| count == 0) {
        return Err(format!("no linker script places {}", objects[index]));
    }
    build.alike(&layout.game, &objects, &target)?;
    for (script, out, newline) in edits {
        files.write(&script, join_lines(&out, newline));
    }

    // The sources, one after another, unless views joined them already.
    let mut text = String::new();
    if let Some(joined_text) = joined_text {
        for object in objects.iter().filter(|object| **object != into) {
            files.remove(&format!("{object}.c"));
        }
        files.write(&target, joined_text);
        return Ok((layout.game, report));
    }
    let mut included: Vec<String> = Vec::new();
    for object in &objects {
        let source = format!("{object}.c");
        let own = rewrite_includes(&files.read(&source)?, parent(&source), parent(&target), |path| files.exists(path));
        let part = if text.is_empty() { own } else { joined(&own, &included).trim_start_matches('\n').to_string() };
        included.extend(top_level_includes(&part));
        if !text.is_empty() && !text.ends_with("\n\n") {
            text.push('\n');
        }
        text.push_str(&part);
        if !text.ends_with('\n') {
            text.push('\n');
        }
    }
    for object in objects.iter().filter(|object| **object != into) {
        files.remove(&format!("{object}.c"));
    }
    files.write(&target, text);
    Ok((layout.game, report))
}

// ---- sources ----

/// Each C source of `game` that another file includes by a quoted path
/// beside it (`#include "world_map/s3600.c"`), with the file that includes
/// it: such a source is read by more than its own object, so no merge takes
/// it away or changes what it holds.
fn included_sources(root: &Path, game: &str) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    let top = root.join("games").join(game);
    for entry in walkdir::WalkDir::new(&top).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if !matches!(path.extension().and_then(|e| e.to_str()), Some("c" | "h")) {
            continue;
        }
        let (Ok(relative), Ok(text)) = (path.strip_prefix(root), std::fs::read_to_string(path)) else { continue };
        let file = relative.to_string_lossy().into_owned();
        for line in text.lines() {
            let Some(included) = include_of(line.trim_start()).and_then(|named| named.strip_prefix('"')?.split('"').next()).filter(|named| named.ends_with(".c")) else { continue };
            let Some(target) = normalize(&format!("{}/{included}", parent(&file))) else { continue };
            if root.join(&target).is_file() {
                found.entry(target).or_insert_with(|| file.clone());
            }
        }
    }
    found
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// Quoted `#include` lines that reach a file beside the draft (`"../x.h"`,
/// `"local.h"`) reach it from the module's folder.
fn rewrite_includes(text: &str, from: &str, to: &str, exists: impl Fn(&str) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let rewritten = (|| {
            let rest = line.trim_start().strip_prefix('#')?.trim_start().strip_prefix("include")?.trim_start();
            let path = rest.strip_prefix('"')?.split('"').next()?;
            // A quoted include is looked for beside its file first: one that
            // is there is reached from INTO's folder too.
            let target = normalize(&format!("{from}/{path}"))?;
            if !exists(&target) {
                return None;
            }
            let relative = relative(to, &target);
            (relative != path).then(|| line.replacen(&format!("\"{path}\""), &format!("\"{relative}\""), 1))
        })();
        out.push_str(rewritten.as_deref().unwrap_or(line));
    }
    out
}

fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

fn relative(folder: &str, path: &str) -> String {
    let from: Vec<&str> = folder.split('/').filter(|p| !p.is_empty()).collect();
    let to: Vec<&str> = path.split('/').collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    parts.extend(&to[common..]);
    parts.join("/")
}

// ---- text ----

/// A file's lines, and whether it ends with a newline.
fn lines(text: &str) -> (Vec<&str>, bool) {
    match text.strip_suffix('\n') {
        Some(body) => (body.split('\n').collect(), true),
        None if text.is_empty() => (Vec::new(), false),
        None => (text.split('\n').collect(), false),
    }
}

fn join_lines<S: AsRef<str>>(lines: &[S], newline: bool) -> String {
    let mut text = lines.iter().map(AsRef::as_ref).collect::<Vec<_>>().join("\n");
    if newline && !lines.is_empty() {
        text.push('\n');
    }
    text
}

// ---- files and the journal ----

/// The checkout as adoption sees it: what each touched file held on disk and
/// what it holds now. Nothing is written until commit.
struct Files {
    root: PathBuf,
    before: BTreeMap<String, Option<String>>,
    now: BTreeMap<String, Option<String>>,
}

/// A journal: `was` and `now` each a file's whole text (version 1 kept only
/// `now`'s digest, so it undoes only a file nothing changed since).
const JOURNAL_HEADER: &str = "alchemy-devtools adopt journal 2";
const JOURNAL_HEADER_1: &str = "alchemy-devtools adopt journal 1";

impl Files {
    fn new(root: &Path) -> Files {
        Files { root: root.to_path_buf(), before: BTreeMap::new(), now: BTreeMap::new() }
    }

    fn load(&mut self, path: &str) {
        if !self.now.contains_key(path) {
            let text = std::fs::read_to_string(self.root.join(path)).ok();
            self.before.insert(path.to_string(), text.clone());
            self.now.insert(path.to_string(), text);
        }
    }

    fn read(&mut self, path: &str) -> Result<String, String> {
        self.load(path);
        self.now[path].clone().ok_or_else(|| format!("cannot read {path}"))
    }

    fn exists(&self, path: &str) -> bool {
        match self.now.get(path) {
            Some(text) => text.is_some(),
            None => self.root.join(path).is_file(),
        }
    }

    fn write(&mut self, path: &str, text: String) {
        self.load(path);
        self.now.insert(path.to_string(), Some(text));
    }

    fn remove(&mut self, path: &str) {
        self.load(path);
        self.now.insert(path.to_string(), None);
    }

    /// Writes the journal that undoes every change, then the changes; returns
    /// the journal's path. Nothing is written when a file it changes no longer
    /// holds what adoption read.
    fn commit(&self, command: &str, name: &str) -> Result<String, String> {
        let changed: Vec<(&String, &Option<String>, &Option<String>)> = self
            .now
            .iter()
            .filter(|(path, now)| self.before[*path] != **now)
            .map(|(path, now)| (path, &self.before[path], now))
            .collect();
        let moved: Vec<&str> = changed
            .iter()
            .filter(|(path, before, _)| std::fs::read_to_string(self.root.join(path)).ok() != **before)
            .map(|(path, ..)| path.as_str())
            .collect();
        if !moved.is_empty() {
            return Err(format!("changed since adopt read them, so nothing was written:\n  {}", moved.join("\n  ")));
        }
        let mut folders: Vec<String> = Vec::new();
        for (path, _, now) in &changed {
            if now.is_none() {
                continue;
            }
            let mut folder = parent(path);
            let mut missing = Vec::new();
            while !folder.is_empty() && !self.root.join(folder).is_dir() {
                missing.push(folder.to_string());
                folder = parent(folder);
            }
            for folder in missing.into_iter().rev() {
                if !folders.contains(&folder) {
                    folders.push(folder);
                }
            }
        }
        let mut journal = format!("{JOURNAL_HEADER}\ncommand {command}\n").into_bytes();
        for folder in &folders {
            journal.extend(format!("folder {folder}\n").bytes());
        }
        for (path, before, now) in &changed {
            journal.extend(format!("file {path}\n").bytes());
            match before {
                Some(text) => {
                    journal.extend(format!("was {}\n", text.len()).bytes());
                    journal.extend(text.bytes());
                    journal.push(b'\n');
                }
                None => journal.extend(b"was absent\n"),
            }
            // What it holds after, whole, so an undo can take its own change
            // out of a file later adoptions changed too.
            match now {
                Some(text) => {
                    journal.extend(format!("now {}\n", text.len()).bytes());
                    journal.extend(text.bytes());
                    journal.push(b'\n');
                }
                None => journal.extend(b"now absent\n"),
            }
        }
        // Numbered after the journals already there, so they list in the
        // order they were written and are undone in the reverse.
        let folder = self.root.join("out/adopt");
        std::fs::create_dir_all(&folder).map_err(|error| format!("cannot create out/adopt: {error}"))?;
        let last = std::fs::read_dir(&folder)
            .map_err(|error| format!("cannot list out/adopt: {error}"))?
            .flatten()
            .filter_map(|entry| entry.file_name().to_str()?.split('-').next()?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        let journal_path = format!("out/adopt/{:04}-{name}.journal", last + 1);
        std::fs::write(self.root.join(&journal_path), &journal).map_err(|error| format!("cannot write {journal_path}: {error}"))?;
        let put = |path: &str, text: &Option<String>| -> Result<(), String> {
            match text {
                Some(text) => std::fs::write(self.root.join(path), text).map_err(|error| format!("cannot write {path}: {error}")),
                None => std::fs::remove_file(self.root.join(path)).map_err(|error| format!("cannot remove {path}: {error}")),
            }
        };
        let mut written = 0;
        let mut failed = folders
            .iter()
            .find_map(|folder| std::fs::create_dir_all(self.root.join(folder)).err().map(|error| format!("cannot create {folder}: {error}")));
        if failed.is_none() {
            for (path, _, now) in &changed {
                if let Err(error) = put(path, now) {
                    failed = Some(error);
                    break;
                }
                written += 1;
            }
        }
        let Some(error) = failed else { return Ok(journal_path) };
        // A write failed partway: what was written goes back, so the tree is
        // as adoption found it and the journal has nothing to undo.
        let stuck: Vec<&str> =
            changed[..written].iter().rev().filter(|(path, before, _)| put(path, before).is_err()).map(|(path, ..)| path.as_str()).collect();
        for folder in folders.iter().rev() {
            let _ = std::fs::remove_dir(self.root.join(folder));
        }
        if stuck.is_empty() {
            let _ = std::fs::remove_file(self.root.join(&journal_path));
            return Err(format!("{error}; nothing was changed"));
        }
        Err(format!("{error}; {} could not be put back: adopt --undo {journal_path} restores them", stuck.join(" ")))
    }
}

/// One adoption or undo at a time: `out/adopt/lock` holds the running one's
/// process id and goes when it ends. A lock whose process has ended is taken
/// over; journals are numbered while it is held, so no two share a number.
#[derive(Debug)]
struct Lock(PathBuf);

impl Lock {
    fn take(root: &Path) -> Result<Lock, String> {
        let folder = root.join("out/adopt");
        std::fs::create_dir_all(&folder).map_err(|error| format!("cannot create out/adopt: {error}"))?;
        let path = folder.join("lock");
        for _ in 0..2 {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    use std::io::Write;
                    writeln!(file, "{}", std::process::id()).map_err(|error| format!("cannot write out/adopt/lock: {error}"))?;
                    return Ok(Lock(path));
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let holder = std::fs::read_to_string(&path).ok().and_then(|text| text.trim().parse::<u32>().ok());
                    match holder {
                        Some(pid) if crate::gba::process::gone(pid) => {
                            let _ = std::fs::remove_file(&path);
                        }
                        Some(pid) => return Err(format!("another adopt (process {pid}) holds out/adopt/lock; run one at a time")),
                        None => return Err("out/adopt/lock names no process: remove it if no adopt is running".into()),
                    }
                }
                Err(error) => return Err(format!("cannot create out/adopt/lock: {error}")),
            }
        }
        Err("cannot take out/adopt/lock".into())
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn fingerprint(text: Option<&str>) -> String {
    use sha1::{Digest, Sha1};
    match text {
        Some(text) => format!("sha1:{:x}", Sha1::digest(text.as_bytes())),
        None => "absent".into(),
    }
}

/// What a journal says a file held after its adoption: nothing, its text
/// (version 2), or only its digest (version 1).
enum After {
    Absent,
    Text(String),
    Digest(String),
}

impl After {
    fn fingerprint(&self) -> String {
        match self {
            After::Absent => fingerprint(None),
            After::Text(text) => fingerprint(Some(text)),
            After::Digest(digest) => digest.clone(),
        }
    }
}

struct Entry {
    path: String,
    was: Option<String>,
    now: After,
}

/// A journal's command, the folders it made and its files.
fn read_journal(bytes: &[u8]) -> Result<(Option<String>, Vec<String>, Vec<Entry>), String> {
    let mut at = 0;
    let line = |at: &mut usize| -> Option<String> {
        let end = bytes[*at..].iter().position(|&b| b == b'\n')? + *at;
        let text = String::from_utf8_lossy(&bytes[*at..end]).into_owned();
        *at = end + 1;
        Some(text)
    };
    let header = line(&mut at);
    if header.as_deref() != Some(JOURNAL_HEADER) && header.as_deref() != Some(JOURNAL_HEADER_1) {
        return Err("not an adopt journal".into());
    }
    let broken = || "the journal is cut short".to_string();
    // A text written whole: its length on the line, then its bytes and a newline.
    let text = |at: &mut usize, length: &str| -> Result<String, String> {
        let length: usize = length.parse().map_err(|_| broken())?;
        let text = bytes.get(*at..*at + length).ok_or_else(broken)?;
        *at += length + 1;
        String::from_utf8(text.to_vec()).map_err(|_| broken())
    };
    let mut command = None;
    let mut folders = Vec::new();
    let mut entries = Vec::new();
    while let Some(next) = line(&mut at) {
        if let Some(text) = next.strip_prefix("command ") {
            command = Some(text.to_string());
            continue;
        }
        if let Some(folder) = next.strip_prefix("folder ") {
            folders.push(folder.to_string());
            continue;
        }
        let path = next.strip_prefix("file ").ok_or_else(broken)?.to_string();
        let was = match line(&mut at).and_then(|l| l.strip_prefix("was ").map(str::to_string)).ok_or_else(broken)?.as_str() {
            "absent" => None,
            length => Some(text(&mut at, length)?),
        };
        let now = match line(&mut at).and_then(|l| l.strip_prefix("now ").map(str::to_string)).ok_or_else(broken)?.as_str() {
            "absent" => After::Absent,
            digest if digest.starts_with("sha1:") => After::Digest(digest.to_string()),
            length => After::Text(text(&mut at, length)?),
        };
        entries.push(Entry { path, was, now });
    }
    Ok((command, folders, entries))
}

/// Puts back what a journal's adoption changed. A file later work changed
/// too takes the adoption's change out by a three-way merge of the lines; the
/// undo refuses, writing nothing, when the two changed the same lines (or the
/// journal, from version 1, kept no text to merge with). A file already as it
/// was before (an adoption whose writing stopped partway) is left alone.
fn undo(root: &Path, journal: &Path) -> Result<(), String> {
    let _lock = Lock::take(root)?;
    let journal_path = [std::env::current_dir().map(|d| d.join(journal)).ok(), Some(root.join(journal))]
        .into_iter()
        .flatten()
        .find(|path| path.is_file())
        .ok_or_else(|| format!("cannot read {}", journal.display()))?;
    let bytes = std::fs::read(&journal_path).map_err(|error| format!("cannot read {}: {error}", journal.display()))?;
    let (_, folders, entries) = read_journal(&bytes)?;
    // A journal from before recon/ went would bring it back.
    if let Some(entry) = entries.iter().find(|entry| entry.path.starts_with("recon/")) {
        return Err(format!("the journal would restore {}, and Alchemy has no recon/ any more", entry.path));
    }
    // Each file's text after the undo: None removes it.
    let mut writes: Vec<(&Entry, Option<String>)> = Vec::new();
    let mut clashes: Vec<&str> = Vec::new();
    let mut merged = 0;
    for entry in &entries {
        let current = std::fs::read_to_string(root.join(&entry.path)).ok();
        let state = fingerprint(current.as_deref());
        if state == entry.now.fingerprint() {
            writes.push((entry, entry.was.clone()));
        } else if state == fingerprint(entry.was.as_deref()) {
            continue;
        } else if let (After::Text(after), Some(was), Some(current)) = (&entry.now, &entry.was, &current) {
            match merge::merge(after, current, was) {
                Some(text) => {
                    merged += 1;
                    writes.push((entry, Some(text)));
                }
                None => clashes.push(&entry.path),
            }
        } else {
            clashes.push(&entry.path);
        }
    }
    if !clashes.is_empty() {
        return Err(format!(
            "changed since the adoption where it changed them, so nothing was undone:\n  {}",
            clashes.join("\n  ")
        ));
    }
    for (entry, text) in &writes {
        let full = root.join(&entry.path);
        match text {
            Some(text) => {
                if let Some(folder) = full.parent() {
                    std::fs::create_dir_all(folder).map_err(|error| format!("cannot create {}: {error}", folder.display()))?;
                }
                std::fs::write(&full, text).map_err(|error| format!("cannot write {}: {error}", entry.path))?;
            }
            None => std::fs::remove_file(&full).map_err(|error| format!("cannot remove {}: {error}", entry.path))?,
        }
    }
    for folder in folders.iter().rev() {
        // Only a folder left empty goes.
        let _ = std::fs::remove_dir(root.join(folder));
    }
    std::fs::remove_file(&journal_path).map_err(|error| format!("cannot remove {}: {error}", journal_path.display()))?;
    let merged = if merged > 0 { format!(" ({merged} merged with later changes)") } else { String::new() };
    println!("undone: {} files restored from {}{merged}", writes.len(), journal.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// A build that says every object outside `placing` places nothing in a
    /// statement's sections, and builds everything alike unless told not to.
    /// Its editions link `listed` but for those in `untouched`, relink but
    /// for those in `unlinked`, and differ where `differs` says; each
    /// compare gives the next of `compares`, then succeeds.
    #[derive(Default)]
    struct Stub {
        placing: Vec<(&'static str, &'static str)>,
        apart: bool,
        listed: Vec<String>,
        untouched: Vec<&'static str>,
        unlinked: Vec<&'static str>,
        differs: Vec<(&'static str, &'static str)>,
        compares: std::cell::RefCell<Vec<String>>,
        refreshed: std::cell::RefCell<Vec<String>>,
        moderns: std::cell::RefCell<Vec<String>>,
    }

    impl Build for Stub {
        fn empty(&self, stem: &str, sections: &str) -> Result<bool, String> {
            Ok(!self.placing.iter().any(|(s, section)| stem.ends_with(s) && sections.split_whitespace().any(|p| matches(p, section))))
        }
        fn alike(&self, _: &str, _: &[String], _: &str) -> Result<(), String> {
            if self.apart {
                return Err("tla-en links a into its main image and b only into overlays".into());
            }
            Ok(())
        }
        fn refresh(&self, games: &[String], stems: &[String]) -> Result<(), String> {
            self.refreshed.borrow_mut().push(format!("{} {}", games.join(" "), stems.join(" ")));
            Ok(())
        }
        fn compare(&self, _: &[DecompTargetId]) -> Result<(), String> {
            let mut compares = self.compares.borrow_mut();
            if compares.is_empty() {
                return Ok(());
            }
            Err(compares.remove(0))
        }
        fn modern(&self, _: &[String]) -> Result<(), String> {
            let mut moderns = self.moderns.borrow_mut();
            if moderns.is_empty() {
                return Ok(());
            }
            Err(moderns.remove(0))
        }
        fn linked(&self, id: DecompTargetId) -> Result<link::Linked, String> {
            let main = if self.untouched.contains(&id.as_str()) { Vec::new() } else { self.listed.clone() };
            Ok(link::Linked { main, overlays: Vec::new() })
        }
        fn relinked(&self, id: DecompTargetId, _: std::time::SystemTime) -> bool {
            !self.unlinked.contains(&id.as_str()) && !self.untouched.contains(&id.as_str())
        }
        fn difference(&self, id: DecompTargetId) -> Result<Option<crate::check::rom::Difference>, String> {
            Ok(self.differs.iter().find(|(edition, _)| *edition == id.as_str()).map(|(_, object)| crate::check::rom::Difference {
                label: format!("{object}.o"),
                object: Some(object.to_string()),
                overlays: Vec::new(),
            }))
        }
    }

    const SCRIPT: &str = "SECTIONS\n{\n    . = ABSOLUTE(0x08001000);\n    .rom :\n    {\n        \"*/games/tla/src/field/a.o\"(.text)\n        \"*/games/tla/src/field/b.o\"(.text)\n\n        \"*/games/tla/src/field/c.o\"(.text)\n        \"*/games/tla/src/field/d.o\"(.text)\n        \"*libgcc.a:_udivsi3.o\"(.text)\n#if LANGUAGE == LANGUAGE_JAPANESE\n        \"*/games/tla/src/field/e.o\"(.text)\n#endif\n        \"*/games/tla/src/field/f.o\"(.text .rodata)\n        \"*/games/tla/src/field/g.o\"(.text)\n        \"*/games/tla/src/field/a.o\"(.rodata)\n        \"*/games/tla/src/field/b.o\"(.rodata)\n        \"*/games/tla/src/field/c.o\"(.rodata)\n    }\n}\n";

    fn tree(root: &Path) {
        let files = [
            ("games/tla/ld_script.ld", SCRIPT),
            ("games/tla/src/field/a.c", "/// @file\n/// A's guide.\n\n#include \"global.h\"\n#include \"../field.h\"\n\nvoid A(void) {}\n"),
            ("games/tla/src/field/b.c", "/// @file\n/// B's guide.\n\n#include \"global.h\"\n#include \"b.h\"\n\nvoid B(void) {}\n"),
            ("games/tla/src/field/c.c", "#include \"global.h\"\n\nvoid C(void) {}\n"),
            ("games/tla/src/field/d.s", "\t.thumb\n"),
            ("games/tla/src/field/e.c", "void E(void) {}\n"),
            ("games/tla/src/field/f.c", "void F(void) {}\n"),
            ("games/tla/src/field/g.c", "void G(void) {}\n"),
            ("games/tla/src/field.h", "\n"),
        ];
        for (path, text) in files {
            std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            std::fs::write(root.join(path), text).unwrap();
        }
    }

    #[test]
    fn the_merge_form_parses_and_refuses_what_it_cannot_take() {
        let Ok(Request::Adopt(o)) = parse(&args(&["merge", "games/tla/src/field/ab.c=games/tla/src/field/a.c,games/tla/src/field/b.c", "--dry-run"])) else {
            panic!("merge did not parse")
        };
        assert_eq!((o.items.len(), o.dry_run, o.verify), (1, true, false));
        assert!(parse(&args(&["merge"])).is_err());
        assert!(parse(&args(&["main", "080e2264"])).is_err());
        assert!(parse(&args(&["merge", "x.c=a.c,b.c", "--verify", "--dry-run"])).is_err());
        assert!(parse(&args(&["merge", "x.c=a.c,b.c", "--editions", "en"])).is_err());
        assert!(matches!(parse(&args(&["--undo", "j"])), Ok(Request::Undo(_, items)) if items.is_empty()));
        assert!(matches!(parse(&args(&["--undo", "j", "a.c"])), Ok(Request::Undo(_, items)) if items == ["a.c"]));
        assert!(names("games/tla/src/field/ab.c=games/tla/src/field/a.c,games/tla/src/field/b.c", "field/b.c"));
        assert!(!names("games/tla/src/field/ab.c=games/tla/src/field/a.c", "c.c"));
    }

    /// Every statement placing the objects is one of a run: the first takes
    /// the merged object, the rest go; the sources join one after another,
    /// and the journal undoes it all.
    #[test]
    fn a_merge_rewrites_each_run_and_joins_the_sources() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        tree(root);
        let stub = Stub { placing: vec![], apart: false, ..Stub::default() };
        let words = args(&["merge", "games/tla/src/field/abc.c=games/tla/src/field/a.c,games/tla/src/field/b.c,games/tla/src/field/c.c"]);
        let Ok(Request::Adopt(options)) = parse(&words) else { panic!("merge did not parse") };
        adopt(root, &options, &command_line(&words), &stub).unwrap();
        let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
        let script = read("games/tla/ld_script.ld").unwrap();
        assert!(script.contains("    {\n        \"*/games/tla/src/field/abc.o\"(.text)\n\n        \"*/games/tla/src/field/d.o\"(.text)\n"), "{script}");
        assert!(script.contains("        \"*/games/tla/src/field/abc.o\"(.rodata)\n    }\n"), "{script}");
        assert!(!script.contains("field/a.o") && !script.contains("field/b.o") && !script.contains("field/c.o"));
        assert_eq!(
            read("games/tla/src/field/abc.c").unwrap(),
            "/// @file\n/// A's guide.\n\n#include \"global.h\"\n#include \"../field.h\"\n\nvoid A(void) {}\n\n// B's guide.\n\n#include \"b.h\"\n\nvoid B(void) {}\n\nvoid C(void) {}\n"
        );
        assert!(read("games/tla/src/field/a.c").is_none() && read("games/tla/src/field/c.c").is_none());
        let journal = std::fs::read_dir(root.join("out/adopt")).unwrap().flatten().next().unwrap().path();
        assert!(journal.file_name().unwrap().to_str().unwrap().starts_with("0001-merge-abc"));
        undo(root, &journal).unwrap();
        assert_eq!(read("games/tla/ld_script.ld").as_deref(), Some(SCRIPT));
        assert!(read("games/tla/src/field/abc.c").is_none());
        assert!(read("games/tla/src/field/b.c").unwrap().starts_with("/// @file\n/// B's guide."));
    }

    /// A merge into one of the objects keeps its name; a merge into another
    /// folder keeps relative includes reaching their files.
    #[test]
    fn a_merge_may_keep_an_object_s_name_or_move_its_folder() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        tree(root);
        let stub = Stub { placing: vec![], apart: false, ..Stub::default() };
        let mut files = Files::new(root);
        merge_objects(&mut files, "games/tla/src/field/a.c=games/tla/src/field/a.c,games/tla/src/field/b.c", &stub, None).unwrap();
        assert!(files.read("games/tla/ld_script.ld").unwrap().contains("    {\n        \"*/games/tla/src/field/a.o\"(.text)\n\n        \"*/games/tla/src/field/c.o\"(.text)"));
        assert!(files.read("games/tla/src/field/a.c").unwrap().ends_with("void B(void) {}\n"));
        assert!(!files.exists("games/tla/src/field/b.c"));
        let mut files = Files::new(root);
        merge_objects(&mut files, "games/tla/src/field/map/ab.c=games/tla/src/field/a.c,games/tla/src/field/b.c", &stub, None).unwrap();
        assert!(files.read("games/tla/src/field/map/ab.c").unwrap().contains("#include \"../../field.h\"\n"));
    }

    #[test]
    fn a_merge_refuses_objects_apart_or_built_apart() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        tree(root);
        let alike = Stub { placing: vec![("field/c", ".text"), ("field/e", ".text"), ("field/f", ".text"), ("field/g", ".text"), ("field/a", ".text")], apart: false, ..Stub::default() };
        let merge = |item: &str, build: &dyn Build| merge_objects(&mut Files::new(root), item, build, None).unwrap_err();
        let field = |names: &str| names.split(',').map(|name| format!("games/tla/src/field/{name}.c")).collect::<Vec<_>>().join(",");
        // A blank line is no bar.
        assert!(merge_objects(&mut Files::new(root), &format!("games/tla/src/field/x.c={}", field("b,c")), &alike, None).is_ok());
        // An #if and a line of the compiler library lie between.
        assert!(merge(&format!("games/tla/src/field/x.c={}", field("c,e")), &alike).contains("leave out"));
        // Different sections.
        assert!(merge(&format!("games/tla/src/field/x.c={}", field("f,g")), &alike).contains("leave out"));
        // Out of order.
        assert!(merge(&format!("games/tla/src/field/x.c={}", field("b,a")), &alike).contains("leave out"));
        // Assembly, an existing file, too few objects, the main image and overlays.
        assert!(merge(&format!("games/tla/src/field/x.c={}", field("c,d")), &alike).contains("is assembly"));
        assert!(merge(&format!("games/tla/src/field/g.c={}", field("a,b")), &alike).contains("exists"));
        assert!(merge(&format!("games/tla/src/field/x.c={}", field("a")), &alike).contains("two objects"));
        let apart = Stub { placing: vec![], apart: true, ..Stub::default() };
        assert!(merge(&format!("games/tla/src/field/x.c={}", field("a,b")), &apart).contains("only into overlays"));
        // An object a run leaves out with nothing in its sections joins it.
        let empty = Stub { placing: vec![], apart: false, ..Stub::default() };
        assert!(merge_objects(&mut Files::new(root), &format!("games/tla/src/field/x.c={}", field("f,g")), &empty, None).is_ok());
        // A source another file includes stays the file it is.
        std::fs::write(root.join("games/tla/src/credits.c"), "#include \"field/c.c\"\n").unwrap();
        assert!(merge(&format!("games/tla/src/field/x.c={}", field("b,c")), &alike).contains("games/tla/src/credits.c includes games/tla/src/field/c.c"));
    }

    /// Two merges in one journal; the build blames the second, which goes
    /// back while the first stays merged.
    #[test]
    fn one_item_backs_out_of_a_batch_and_the_rest_stay() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        tree(root);
        let stub = Stub { placing: vec![], apart: false, ..Stub::default() };
        let first = "games/tla/src/field/ab.c=games/tla/src/field/a.c,games/tla/src/field/b.c";
        let second = "games/tla/src/field/fg.c=games/tla/src/field/f.c,games/tla/src/field/g.c";
        let words = args(&["merge", first, second]);
        let Ok(Request::Adopt(options)) = parse(&words) else { panic!("merge did not parse") };
        adopt(root, &options, &command_line(&words), &stub).unwrap();
        let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
        assert!(read("games/tla/src/field/ab.c").is_some() && read("games/tla/src/field/fg.c").is_some());
        let journal = Path::new("out/adopt/0001-merge-ab.journal");
        assert!(back_out_batch(root, journal, &["x.c".into()], &stub).err().unwrap().contains("not among"));
        back_out_batch(root, journal, &["field/fg.c".into()], &stub).unwrap();
        assert!(read("games/tla/src/field/ab.c").is_some() && read("games/tla/src/field/fg.c").is_none());
        assert_eq!(read("games/tla/src/field/g.c").as_deref(), Some("void G(void) {}\n"));
        let script = read("games/tla/ld_script.ld").unwrap();
        assert!(script.contains("field/ab.o\"(.text)") && script.contains("field/f.o\"(.text .rodata)\n        \"*/games/tla/src/field/g.o\"(.text)"));
        let recorded = read("out/adopt/0001-merge-ab.journal").unwrap();
        assert!(recorded.contains(&format!("command adopt merge {first}\n")), "{recorded}");
    }

    /// The checkout's own build: a merge keeps every object on its route and
    /// its side of the -O2/-Os line, INTO's own name included.
    #[test]
    #[ignore = "needs a built Alchemy checkout on main (ALCHEMY_ROOT): it reads make's recipes for scene/kandorean_temple/s00b8"]
    fn the_checkout_refuses_a_merge_that_changes_a_route() {
        let build = Checkout(root());
        let field = |name: &str| format!("games/tla/src/field/{name}");
        let arm = vec![field("f021470"), field("f021828")];
        // Two ARM_C objects into a name the Makefile does not route: Thumb C.
        let error = build.alike("tla", &arm, &field("arm_pair.c")).unwrap_err();
        assert!(error.contains("INTO's route"), "{error}");
        // Into one of their own names, the route stays.
        build.alike("tla", &arm, &field("f021828.c")).unwrap();
        let scene = |name: &str| format!("games/tla/src/scene/kandorean_temple/{name}");
        let thumb = vec![scene("s00b8"), scene("s2ec4")];
        // C linked only into overlays builds for size, and a new INTO placed
        // where it is does too.
        build.alike("tla", &thumb, &scene("events.c")).unwrap();
        // An ARM_C object and game C.
        assert!(build.alike("tla", &[field("f021470"), scene("s00b8")], &field("mixed.c")).unwrap_err().contains("only into overlays"));
    }

    /// The checkout's built objects: what places bytes in a statement's
    /// sections, and what no edition links.
    #[test]
    #[ignore = "needs a built Alchemy checkout on main (ALCHEMY_ROOT): it reads the built scene/kandorean_temple/s00b8.o"]
    fn the_checkout_reads_each_object_s_sections() {
        let build = Checkout(root());
        let stem = "games/tla/src/scene/kandorean_temple/s00b8";
        assert!(!build.empty(stem, ".text").unwrap());
        assert!(build.empty(stem, ".no_such_section.*").unwrap());
        assert!(build.empty("games/tla/src/field/no_such_object", ".text").unwrap_err().contains("no edition links"));
    }

    /// Two merges, the second blamed: by the byte one edition differs in,
    /// while another edition links neither and kept its image, and another
    /// failed with no file named. The untouched edition is no failure, the
    /// one that failed does not stop the blame, and the back-out makes the
    /// lists and the objects it put back before the first merges again.
    #[test]
    #[ignore = "needs an Alchemy checkout (ALCHEMY_ROOT): its editions are read from the checkout"]
    fn verify_blames_only_what_an_edition_the_batch_changed_shows() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        tree(root);
        let ids = built::editions(&["tla".into()]).unwrap();
        let (first_id, second_id, third_id) = (ids[0].as_str(), ids[1].as_str(), ids[2].as_str());
        let field = |name: &str| format!("games/tla/src/field/{name}");
        let stub = Stub {
            listed: vec![field("ab"), field("fg")],
            untouched: vec![first_id],
            unlinked: vec![third_id],
            differs: vec![(second_id, "games/tla/src/field/fg")],
            compares: std::cell::RefCell::new(vec![format!("make failed: make: *** [compare-{second_id}] Error 1\nmake: *** [{third_id}] Error 2")]),
            ..Stub::default()
        };
        let first = "games/tla/src/field/ab.c=games/tla/src/field/a.c,games/tla/src/field/b.c";
        let second = "games/tla/src/field/fg.c=games/tla/src/field/f.c,games/tla/src/field/g.c";
        let words = args(&["merge", first, second, "--verify"]);
        let Ok(Request::Adopt(options)) = parse(&words) else { panic!("merge did not parse") };
        adopt(root, &options, &command_line(&words), &stub).unwrap();
        let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
        assert!(read("games/tla/src/field/ab.c").is_some() && read("games/tla/src/field/fg.c").is_none());
        assert_eq!(read("games/tla/src/field/f.c").as_deref(), Some("void F(void) {}\n"));
        assert_eq!(*stub.refreshed.borrow(), [format!("tla {} {}", field("a"), field("b"))]);

        // Only what no item wrote: the journal is named and nothing goes back.
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        tree(root);
        let stub = Stub {
            listed: vec![field("ab")],
            unlinked: vec![third_id],
            compares: std::cell::RefCell::new(vec![format!("make failed: make: *** [Makefile:55: {third_id}] Error 2")]),
            ..Stub::default()
        };
        let words = args(&["merge", first, "--verify"]);
        let Ok(Request::Adopt(options)) = parse(&words) else { panic!("merge did not parse") };
        let error = adopt(root, &options, &command_line(&words), &stub).unwrap_err();
        assert!(error.contains(&format!("{third_id} did not build")) && error.contains("adopt --undo out/adopt/0001-merge-ab.journal"), "{error}");
        assert!(std::fs::read_to_string(root.join("games/tla/src/field/ab.c")).is_ok());
    }

    /// After a batch is written, make modern judges it: an item whose file
    /// its errors name goes back and the rest stay; an error in no file the
    /// batch wrote names the journal. --no-modern skips it.
    #[test]
    fn make_modern_sends_back_the_item_whose_file_it_refuses() {
        let first = "games/tla/src/field/ab.c=games/tla/src/field/a.c,games/tla/src/field/b.c";
        let second = "games/tla/src/field/fg.c=games/tla/src/field/f.c,games/tla/src/field/g.c";
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        tree(root);
        let stub = Stub { moderns: std::cell::RefCell::new(vec!["make failed: games/tla/src/field/fg.c:3:1: error: unknown type name 'Thing'".into()]), ..Stub::default() };
        let words = args(&["merge", first, second]);
        let Ok(Request::Adopt(options)) = parse(&words) else { panic!("merge did not parse") };
        adopt(root, &options, &command_line(&words), &stub).unwrap();
        let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
        assert!(read("games/tla/src/field/ab.c").is_some() && read("games/tla/src/field/fg.c").is_none());
        assert!(read("games/tla/src/field/f.c").is_some() && read("games/tla/src/field/g.c").is_some());

        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        tree(root);
        let stub = Stub { moderns: std::cell::RefCell::new(vec!["make failed: games/tla/include/x.h:3:1: error: unknown type name".into()]), ..Stub::default() };
        let words = args(&["merge", first]);
        let Ok(Request::Adopt(options)) = parse(&words) else { panic!("merge did not parse") };
        let error = adopt(root, &options, &command_line(&words), &stub).unwrap_err();
        assert!(error.contains("no file this batch wrote") && error.contains("adopt --undo out/adopt/0001-merge-ab.journal"), "{error}");

        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        tree(root);
        let stub = Stub { moderns: std::cell::RefCell::new(vec!["make failed".into()]), ..Stub::default() };
        let words = args(&["merge", first, "--no-modern"]);
        let Ok(Request::Adopt(options)) = parse(&words) else { panic!("merge did not parse") };
        adopt(root, &options, &command_line(&words), &stub).unwrap();
        assert_eq!(stub.moderns.borrow().len(), 1, "not run");
    }

    /// A join with views refuses a source that includes C as text: its
    /// views could not reach the text it includes.
    #[test]
    fn a_join_with_views_refuses_a_source_that_includes_c() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        tree(root);
        std::fs::write(root.join("games/tla/src/field/b.c"), "#include \"global.h\"\n#include \"b_parts/run.c\"\n").unwrap();
        let mut files = Files::new(root);
        let item = "games/tla/src/field/ab.c=games/tla/src/field/a.c,games/tla/src/field/b.c";
        let error = merge_objects(&mut files, item, &Stub::default(), Some("joined".into())).unwrap_err();
        assert!(error.contains("b.c includes C files as text"), "{error}");
        assert!(includes_source("#include \"x/y.c\"\n") && !includes_source("#include \"x.h\"\n#include <y.c>\n"));
        // A plain merge still takes it.
        assert!(merge_objects(&mut Files::new(root), item, &Stub::default(), None).is_ok());
    }

    #[test]
    fn a_makefile_names_its_sources_by_path() {
        let makefile = "FLASH_C := $(addprefix games/tla/src/gamepak/flash/,flash.c sst.c) \\\n    games/tla/src/x.c\nARM_C := $(wildcard games/tla/src/arm/*.c)\n";
        assert_eq!(named_sources(makefile), ["games/tla/src/gamepak/flash/flash.c", "games/tla/src/gamepak/flash/sst.c", "games/tla/src/x.c", "games/tla/src/arm/*.c"]);
        assert!(makefile_names(makefile, "games/tla/src/gamepak/flash/flash.c") && makefile_names(makefile, "games/tla/src/arm/glyph.c"));
        assert!(!makefile_names(makefile, "games/tla/src/scene/mars/flash.c"));
    }

    #[test]
    fn make_names_the_editions_it_gave_up_on() {
        let error = "make: *** [compare-tla-en] Error 1\nmake[1]: *** [out/tla-de/tla-de.elf] Error 1\nmake: *** [Makefile:55: tla-de] Error 2";
        assert!(failed(error, "tla-de") && !failed(error, "tla-en") && !failed(error, "tla-ja"));
        assert!(failed("make: *** [tla-ja] Error 2", "tla-ja"));
    }

    /// An include that sits inside an #if stays, in the later source and as
    /// the one an earlier source had.
    #[test]
    fn a_repeated_include_inside_an_if_stays() {
        let first = "#include \"global.h\"\n#if LANGUAGE == LANGUAGE_GERMAN\n#include \"german.h\"\n#endif\n";
        let included = top_level_includes(first);
        assert_eq!(included, ["#include \"global.h\""]);
        let later = "#include \"global.h\"\n#include \"german.h\"\n#ifdef X\n#include \"global.h\"\n#endif\n";
        assert_eq!(joined(later, &included), "#include \"german.h\"\n#ifdef X\n#include \"global.h\"\n#endif\n");
    }

    #[test]
    fn a_makefile_s_wildcards_and_the_size_option() {
        let makefile = "SOUND_C := a.c $(wildcard games/x/driver.c games/x/track_*.c)\nOTHER := $(wildcard *.s)\n";
        assert_eq!(wildcard_patterns(makefile), ["games/x/track_*.c"]);
        let step = |words: &[&str]| vec![words.iter().map(|w| w.to_string()).collect::<Vec<_>>()];
        assert!(same_but_size(&step(&["cc", "-O2", "x.c"]), &step(&["cc", "-Os", "x.c"])));
        assert!(same_but_size(&step(&["cc", "-O2"]), &step(&["cc", "-O2"])));
        assert!(!same_but_size(&step(&["cc", "-Os"]), &step(&["cc", "-O2"])));
        assert!(!same_but_size(&step(&["cc", "-O2", "-marm"]), &step(&["cc", "-Os", "-mthumb"])));
    }

    #[test]
    fn section_patterns_match_as_the_linker_s() {
        assert!(matches(".text.*", ".text.bit_dispatch") && matches(".text", ".text") && !matches(".text", ".text.x"));
        assert!(!matches(".text.*", ".rodata"));
    }

    #[test]
    fn includes_follow_the_move() {
        let text = "/// @file\n#include \"../a/a.h\"\n#include \"types.h\"\n#include \"local.h\"\n";
        let moved = rewrite_includes(text, "games/tla/src/scene/c", "games/tla/src/field/x", |p| p == "games/tla/src/scene/a/a.h" || p == "games/tla/src/scene/c/local.h");
        assert!(moved.contains("#include \"../../scene/c/local.h\"\n"), "{moved}");
        assert!(moved.contains("#include \"../../scene/a/a.h\"\n"), "{moved}");
        assert!(moved.contains("#include \"types.h\"\n"));
        assert_eq!(relative("games/tla/src/menu/credits", "games/tla/include/x.h"), "../../../include/x.h");
    }

    #[test]
    fn the_journal_undoes_exactly_and_only_when_nothing_changed_since() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::write(root.join("kept.txt"), "before\n").unwrap();
        std::fs::write(root.join("gone.txt"), "old").unwrap();
        let mut files = Files::new(root);
        files.write("kept.txt", "after\n".into());
        files.remove("gone.txt");
        files.write("new/folder/made.c", "int x;\n".into());
        let journal = files.commit("adopt test", "test").unwrap();
        assert_eq!(std::fs::read_to_string(root.join("kept.txt")).unwrap(), "after\n");
        assert!(!root.join("gone.txt").exists());
        std::fs::write(root.join("kept.txt"), "edited\n").unwrap();
        assert!(undo(root, Path::new(&journal)).unwrap_err().contains("kept.txt"));
        assert_eq!(std::fs::read_to_string(root.join("new/folder/made.c")).unwrap(), "int x;\n", "a refused undo writes nothing");
        std::fs::write(root.join("kept.txt"), "after\n").unwrap();
        undo(root, Path::new(&journal)).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("kept.txt")).unwrap(), "before\n");
        assert_eq!(std::fs::read_to_string(root.join("gone.txt")).unwrap(), "old");
        assert!(!root.join("new").exists());
        assert!(!root.join(&journal).exists());
    }

    /// An earlier journal undoes beneath a later one that changed the same
    /// files elsewhere, and the later one still undoes after it.
    #[test]
    fn an_earlier_journal_undoes_beneath_later_changes() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::write(root.join("ld_script.ld"), "a\nb\nc\nd\ne\n").unwrap();
        let mut first = Files::new(root);
        first.write("ld_script.ld", "a\nB\nc\nd\ne\n".into());
        let first = first.commit("adopt merge x.c=a.c,b.c", "first").unwrap();
        let mut second = Files::new(root);
        second.write("ld_script.ld", "a\nB\nc\nd\nE\nf\n".into());
        let second = second.commit("adopt merge y.c=c.c,d.c", "second").unwrap();
        undo(root, Path::new(&first)).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("ld_script.ld")).unwrap(), "a\nb\nc\nd\nE\nf\n");
        undo(root, Path::new(&second)).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("ld_script.ld")).unwrap(), "a\nb\nc\nd\ne\n");

        // A later change to the same line leaves both journals in place.
        let mut third = Files::new(root);
        third.write("ld_script.ld", "a\nB\nc\nd\ne\n".into());
        let third = third.commit("adopt merge x.c=a.c,b.c", "third").unwrap();
        std::fs::write(root.join("ld_script.ld"), "a\nBB\nc\nd\ne\n").unwrap();
        assert!(undo(root, Path::new(&third)).unwrap_err().contains("ld_script.ld"));
        assert!(root.join(&third).exists());
    }

    /// A write that fails partway puts back what was written; a journal whose
    /// adoption stopped partway still undoes what it did write.
    #[test]
    fn a_failed_write_changes_nothing_and_a_partial_adoption_still_undoes() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        std::fs::set_permissions(root.join("b.txt"), std::fs::Permissions::from_mode(0o444)).unwrap();
        let mut files = Files::new(root);
        files.write("a.txt", "a2\n".into());
        files.write("b.txt", "b2\n".into());
        files.write("new/c.txt", "c\n".into());
        let error = files.commit("adopt test", "test").unwrap_err();
        assert!(error.contains("b.txt") && error.ends_with("nothing was changed"), "{error}");
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "a\n");
        assert!(!root.join("new").exists());
        assert_eq!(std::fs::read_dir(root.join("out/adopt")).unwrap().count(), 0);

        std::fs::set_permissions(root.join("b.txt"), std::fs::Permissions::from_mode(0o644)).unwrap();
        let journal = files.commit("adopt test", "test").unwrap();
        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        undo(root, Path::new(&journal)).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "a\n");
        assert_eq!(std::fs::read_to_string(root.join("b.txt")).unwrap(), "b\n");
        assert!(!root.join("new").exists());
    }

    /// Two adoptions that read the same file: the second writes nothing over
    /// the first, and only one holds the lock at a time.
    #[test]
    fn adoptions_take_turns_and_never_write_over_each_other() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::write(root.join("ld_script.ld"), "a\n").unwrap();
        let mut first = Files::new(root);
        let mut second = Files::new(root);
        first.write("ld_script.ld", "a\nb\n".into());
        second.write("ld_script.ld", "a\nc\n".into());
        second.write("made.c", "int c;\n".into());
        first.commit("adopt first", "first").unwrap();
        assert!(second.commit("adopt second", "second").unwrap_err().contains("ld_script.ld"));
        assert_eq!(std::fs::read_to_string(root.join("ld_script.ld")).unwrap(), "a\nb\n");
        assert!(!root.join("made.c").exists());
        assert_eq!(std::fs::read_dir(root.join("out/adopt")).unwrap().count(), 1, "only the first journal");

        let held = Lock::take(root).unwrap();
        assert!(Lock::take(root).unwrap_err().contains(&std::process::id().to_string()));
        drop(held);
        std::fs::write(root.join("out/adopt/lock"), "1073741808\n").unwrap();
        let taken = Lock::take(root).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("out/adopt/lock")).unwrap().trim(), std::process::id().to_string());
        drop(taken);
        assert!(!root.join("out/adopt/lock").exists());
    }
}
