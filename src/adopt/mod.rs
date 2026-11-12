//! adopt: gathers a game's objects into one, editing its linker script, and
//! keeps a journal that undoes it, whole or one item at a time. The build
//! still judges what a merge did (`make compare-editions`), `check rom`
//! traces a failure to the object that placed the first differing byte, and
//! `--undo JOURNAL ITEM` sends that merge back while the rest of the batch
//! stays.

use crate::alchemy::built;
use crate::alchemy::layout::Layout;
use crate::alchemy::link::{self, statement_stem};
use crate::compile::root;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod merge;

const USAGE: &str = r#"usage: alchemy-devtools adopt merge INTO.c=OBJECT.c,OBJECT.c... [--verify | --dry-run]
       alchemy-devtools adopt --undo JOURNAL [ITEM...]
merge: the objects become one object built from INTO.c, a new file or one of
the objects' own. Its source is theirs one after another, in the order given:
an #include of a header an earlier source includes goes, a later source's
/// @file comment becomes a // comment, and a relative #include still reaches
its file from INTO's folder. In the game's linker scripts
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
judge at once: it makes every edition of the game (make -k compare-EDITION),
and while one differs, sends back the item that wrote the object placing the
first byte it differs in (as --undo JOURNAL ITEM does), or the item whose
file an edition that did not link names in make's errors, and builds again;
it stops, naming the journal, when an edition does not link or differs first
in something no item wrote.
--dry-run prints what would change. Otherwise the changes are written with a
journal under out/adopt/ that --undo reverses: a file changed since (a later
merge's edits to the same script) has only the journal's own change taken
out, by a three-way merge of its lines, and the undo refuses, writing
nothing, where the two changed the same lines. Journals undo in any order.
--undo JOURNAL ITEM... backs out only those items (an item, its INTO or one
of its objects): the journal is undone and its merges run again without
them, under a new journal. One adopt or undo runs at a time (out/adopt/lock),
and none writes over a file that changed after it read it."#;

#[derive(Debug)]
struct Options {
    items: Vec<String>,
    /// Whether the build judges the batch after it is written.
    verify: bool,
    dry_run: bool,
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
    for (i, value) in arguments.iter().map(String::as_str).enumerate() {
        match value {
            "--dry-run" => dry_run = true,
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
    if form.is_none() || items.is_empty() || (verify && dry_run) {
        return Err(USAGE.into());
    }
    Ok(Request::Adopt(Options { items, verify, dry_run }))
}

pub fn run(arguments: &[String]) -> Result<(), String> {
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
    // The rest are verified by whoever called for the back-out.
    let words: Vec<String> = words.into_iter().filter(|word| !items.iter().any(|given| names(word, given)) && word != "--verify").collect();
    options.verify = false;
    adopt_batch(root, &options, &command_line(&words), build)
}

/// A batch written: its journal, its game, and each item with the source
/// stems it wrote under games/.
struct Batch {
    journal: String,
    game: String,
    items: Vec<(String, Vec<String>)>,
}

fn adopt(root: &Path, options: &Options, command: &str, build: &dyn Build) -> Result<(), String> {
    let batch = adopt_batch(root, options, command, build)?;
    match batch.filter(|_| options.verify) {
        Some(batch) => verify(root, batch, build),
        None => Ok(()),
    }
}

/// Builds every edition of the batch's game and backs out the item the build
/// blames, by what placed the first byte an edition differs in or by the file
/// make's errors name, until every edition compares.
fn verify(root: &Path, mut batch: Batch, build: &dyn Build) -> Result<(), String> {
    let ids = built::editions(std::slice::from_ref(&batch.game))?;
    let jobs = std::thread::available_parallelism().map_or(1, |n| n.get());
    loop {
        let mut arguments = vec!["make".to_string(), "-k".into(), format!("-j{jobs}")];
        arguments.extend(ids.iter().map(|id| format!("compare-{id}")));
        println!("verify: make {}", arguments[1..].join(" "));
        let made = crate::gba::process::run(&arguments, root);
        if made.is_ok() {
            println!("verified: every {} edition matches its ROM", batch.game);
            return Ok(());
        }
        let error = made.as_ref().err().map_or("", |error| error.as_str());
        // An edition the batch changed relinks after its journal was written;
        // one that did not link keeps an older image.
        let written = std::fs::metadata(root.join(&batch.journal)).and_then(|meta| meta.modified()).map_err(|error| format!("{}: {error}", batch.journal))?;
        let mut blamed: Vec<String> = Vec::new();
        let mut blame = |item: &str| {
            if !blamed.iter().any(|known| known == item) {
                blamed.push(item.to_string());
            }
        };
        for &id in &ids {
            let image = root.join(crate::alchemy::targets::target_for(id).image());
            let linked = std::fs::metadata(&image).and_then(|meta| meta.modified()).is_ok_and(|time| time >= written);
            if !linked {
                // A merged source that does not compile names itself.
                let named: Vec<&String> = batch
                    .items
                    .iter()
                    .filter(|(_, stems)| stems.iter().any(|stem| error.contains(&format!("{stem}.c:"))))
                    .map(|(item, _)| item)
                    .collect();
                if named.is_empty() {
                    let tail: Vec<&str> = error.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();
                    return Err(format!(
                        "{id} did not link, so no byte can be blamed:\n  {}\nback an item out with adopt --undo {} ITEM",
                        tail.join("\n  "),
                        batch.journal
                    ));
                }
                for item in named {
                    println!("verify: {id} did not build: {item} goes back");
                    blame(item);
                }
                continue;
            }
            let Some(difference) = crate::check::rom::first_difference(root, id)? else { continue };
            // The object that placed the byte, or an overlay whose stream the
            // ROM lacks that links what the item wrote.
            let lists = link::linked(root, id).unwrap_or_default();
            let culprit = batch.items.iter().find(|(_, stems)| {
                difference.object.as_ref().is_some_and(|object| stems.contains(object))
                    || difference.overlays.iter().filter_map(|overlay| lists.overlay(overlay)).any(|objects| stems.iter().any(|stem| objects.contains(stem)))
            });
            match culprit {
                Some((item, _)) => {
                    println!("verify: {id} first differs at {}: {item} goes back", difference.label);
                    blame(item);
                }
                None => {
                    return Err(format!(
                        "{id} first differs at {}, which no item of this batch wrote; adopt --undo {} undoes it",
                        difference.label, batch.journal
                    ))
                }
            }
        }
        if blamed.is_empty() {
            return Err(format!("make failed, yet every {} edition matches: {error}", batch.game));
        }
        match back_out_batch(root, Path::new(&batch.journal), &blamed, build)? {
            Some(rest) => batch = rest,
            None => {
                println!("verify: every item went back");
                return Ok(());
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
    let mut game = None;
    for item in &options.items {
        let saved = files.now.clone();
        match merge_objects(&mut files, item, build) {
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
                game.get_or_insert(merged_game);
            }
            Err(why) => {
                files.now = saved;
                refused += 1;
                println!("refused {item}: {why}");
            }
        }
    }
    let Some(game) = game else { return Err(format!("nothing adopted ({refused} refused)")) };
    if options.dry_run {
        println!("dry run: {adopted} would be merged, {refused} refused; nothing written");
        return Ok(None);
    }
    let journal = files.commit(command, &journal_name(options))?;
    println!("{adopted} merged, {refused} refused; journal {journal}");
    if !options.verify {
        println!("judge with make compare-editions, or adopt --verify; undo with alchemy-devtools adopt --undo {journal} [ITEM...]");
    }
    Ok(Some(Batch { journal, game, items: written }))
}

/// What a journal is named after: its form and first item's INTO.
fn journal_name(options: &Options) -> String {
    let first = options.items[0].split([' ', ',', '=']).next().unwrap_or("");
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
}

/// The checkout's build, as the last make left it.
struct Checkout<'a>(&'a Path);

/// Whether a section name matches a linker-script pattern: whole, or with a
/// `*` matching the rest.
fn matches(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        Some((head, tail)) => name.len() >= head.len() + tail.len() && name.starts_with(head) && name.ends_with(tail),
        None => pattern == name,
    }
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
        let name = into.rsplit('/').next().unwrap_or(into);
        let anew = !stems.iter().any(|stem| format!("{stem}.c") == into);
        if anew && makefile.contains(name) {
            return Err(format!("the Makefile names {name}, so {into} may take another route"));
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
/// text already includes goes, and its `/// @file` comment becomes `//`.
fn joined(text: &str, included: &[String]) -> String {
    let mut out = String::new();
    let mut file_comment = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
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
        if include_of(trimmed).is_some_and(|header| header.ends_with(".h\"") && included.iter().any(|known| known == trimmed)) {
            continue;
        }
        out.push_str(line);
    }
    out
}

/// The quoted file an `#include` line names.
fn include_of(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('#')?.trim_start().strip_prefix("include")?.trim();
    (rest.starts_with('"') || rest.starts_with('<')).then_some(rest)
}

/// Merges one item, `INTO.c=OBJECT.c,OBJECT.c...`: its game and report.
fn merge_objects(files: &mut Files, item: &str, build: &dyn Build) -> Result<(String, Vec<String>), String> {
    let (into, objects) = item.split_once('=').ok_or("an item is INTO.c=OBJECT.c,OBJECT.c...")?;
    let stem = |path: &str| {
        let path = path.trim_start_matches("./");
        path.strip_suffix(".c").unwrap_or(path).to_string()
    };
    let into = stem(into);
    let objects: Vec<String> = objects.split(',').filter(|o| !o.is_empty()).map(stem).collect();
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

    // The sources, one after another.
    let mut text = String::new();
    let mut included: Vec<String> = Vec::new();
    for object in &objects {
        let source = format!("{object}.c");
        let own = rewrite_includes(&files.read(&source)?, parent(&source), parent(&target), |path| files.exists(path));
        let part = if text.is_empty() { own } else { joined(&own, &included).trim_start_matches('\n').to_string() };
        included.extend(part.lines().map(str::trim).filter(|line| include_of(line).is_some()).map(str::to_string));
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

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// `#include "../..."` lines that reach a file from the draft's folder reach
/// it from the module's.
fn rewrite_includes(text: &str, from: &str, to: &str, exists: impl Fn(&str) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let rewritten = (|| {
            let rest = line.trim_start().strip_prefix('#')?.trim_start().strip_prefix("include")?.trim_start();
            let path = rest.strip_prefix('"')?.split('"').next()?;
            if !path.starts_with("../") && !path.starts_with("./") {
                return None;
            }
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
    struct Stub {
        placing: Vec<(&'static str, &'static str)>,
        apart: bool,
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
        let stub = Stub { placing: vec![], apart: false };
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
        let stub = Stub { placing: vec![], apart: false };
        let mut files = Files::new(root);
        merge_objects(&mut files, "games/tla/src/field/a.c=games/tla/src/field/a.c,games/tla/src/field/b.c", &stub).unwrap();
        assert!(files.read("games/tla/ld_script.ld").unwrap().contains("    {\n        \"*/games/tla/src/field/a.o\"(.text)\n\n        \"*/games/tla/src/field/c.o\"(.text)"));
        assert!(files.read("games/tla/src/field/a.c").unwrap().ends_with("void B(void) {}\n"));
        assert!(!files.exists("games/tla/src/field/b.c"));
        let mut files = Files::new(root);
        merge_objects(&mut files, "games/tla/src/field/map/ab.c=games/tla/src/field/a.c,games/tla/src/field/b.c", &stub).unwrap();
        assert!(files.read("games/tla/src/field/map/ab.c").unwrap().contains("#include \"../../field.h\"\n"));
    }

    #[test]
    fn a_merge_refuses_objects_apart_or_built_apart() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        tree(root);
        let alike = Stub { placing: vec![("field/c", ".text"), ("field/e", ".text"), ("field/f", ".text"), ("field/g", ".text"), ("field/a", ".text")], apart: false };
        let merge = |item: &str, build: &dyn Build| merge_objects(&mut Files::new(root), item, build).unwrap_err();
        let field = |names: &str| names.split(',').map(|name| format!("games/tla/src/field/{name}.c")).collect::<Vec<_>>().join(",");
        // A blank line is no bar.
        assert!(merge_objects(&mut Files::new(root), &format!("games/tla/src/field/x.c={}", field("b,c")), &alike).is_ok());
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
        let apart = Stub { placing: vec![], apart: true };
        assert!(merge(&format!("games/tla/src/field/x.c={}", field("a,b")), &apart).contains("only into overlays"));
        // An object a run leaves out with nothing in its sections joins it.
        let empty = Stub { placing: vec![], apart: false };
        assert!(merge_objects(&mut Files::new(root), &format!("games/tla/src/field/x.c={}", field("f,g")), &empty).is_ok());
    }

    /// Two merges in one journal; the build blames the second, which goes
    /// back while the first stays merged.
    #[test]
    fn one_item_backs_out_of_a_batch_and_the_rest_stay() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        tree(root);
        let stub = Stub { placing: vec![], apart: false };
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
    fn the_checkout_refuses_a_merge_that_changes_a_route() {
        let build = Checkout(root());
        let field = |name: &str| format!("games/tla/src/field/{name}");
        let arm = vec![field("f021470"), field("f021828")];
        // Two ARM_C objects into a name the Makefile does not route: Thumb C.
        let error = build.alike("tla", &arm, &field("arm_pair.c")).unwrap_err();
        assert!(error.contains("INTO's route"), "{error}");
        // Into one of their own names, the route stays.
        build.alike("tla", &arm, &field("f021828.c")).unwrap();
        // Game C into a name the sound library's wildcard would take.
        let scene = |name: &str| format!("games/tla/src/scene/kandorean_temple/{name}");
        let thumb = vec![scene("s00b8"), scene("events_a")];
        let error = build.alike("tla", &thumb, "games/tla/src/sound/music_track_new.c").unwrap_err();
        assert!(error.contains("music_track_*.c"), "{error}");
        // C linked only into overlays builds for size, and a new INTO placed
        // where it is does too.
        build.alike("tla", &thumb, &scene("events.c")).unwrap();
        // An ARM_C object and game C.
        assert!(build.alike("tla", &[field("f021470"), scene("s00b8")], &field("mixed.c")).unwrap_err().contains("only into overlays"));
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
        let text = "/// @file\n#include \"../a/a.h\"\n#include \"types.h\"\n";
        let moved = rewrite_includes(text, "games/tla/src/scene/c", "games/tla/src/field/x", |p| p == "games/tla/src/scene/a/a.h");
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
