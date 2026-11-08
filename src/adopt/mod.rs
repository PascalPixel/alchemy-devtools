//! adopt: moves matched drafts into the build in every edition, and keeps a
//! journal that undoes it, whole or one item at a time. Each draft must
//! first measure differ 0 as diff measures it; the build still judges what
//! adoption did (`make compare-editions`), `check rom` traces a failure to
//! its function, and `--undo JOURNAL ITEM` sends that function back to its
//! draft while the rest of the batch stays.

use crate::alchemy::layout::{
    aligns, edition_only, ends_with_padding, globals, has_edition_forms, overlay_draft, overlay_section, pieces,
    incbin, rom_offset, rom_section, sections, Layout, ObjectList, Piece, Row, Section, LANGUAGES,
};
use crate::alchemy::built::object_stem;
use crate::compile::root;
use crate::gba::ld_map::Map;
use crate::gba::clex::{lex, Kind, Token};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

mod edition;
mod merge;

use edition::{incbin_line, Edition};

const USAGE: &str = r#"usage: alchemy-devtools adopt main ADDRESS[,ADDRESS...]... [--editions LIST] [--game GAME] [--unmeasured] [--verify | --dry-run]
       alchemy-devtools adopt overlay DRAFT.c... [--edition-forms] [--unmeasured] [--verify | --dry-run]
       alchemy-devtools adopt edition OBJECT... --editions LIST [--verify | --dry-run]
       alchemy-devtools adopt replace FILE.c DRAFT.c [--unmeasured] [--verify | --dry-run]
       alchemy-devtools adopt --undo JOURNAL [ITEM...]
main: the English draft recon/GAME/en/main/ADDRESS.c becomes
games/GAME/src/FOLDER/fADDRESS.c, FOLDER being the nearest object's in the
object list (not gamepak or sound). Further addresses name listings the
draft also replaces, nested functions or labels: the draft must define the
first listing's names only. The module takes the earliest listing's row and
the others' rows go. In every other edition of LIST (default: all six) the
rom.s section whose .global names the function goes; its row becomes the
module, joining the module's row unless a row that edition links lies
between. The draft is compiled for each built edition, and the module takes
its size there from its label: bytes before the label stay in the section,
bytes after it (code no label names, a pad) become a section of its own
with the labels they hold, after the module's row, and a later section the
module's bytes run on into (its pool, its last instruction) goes or is
trimmed. An edition not built is cut at labels alone: a section that also
holds another function keeps it as a section of its own when the two
divide at its label, else the edition stays on its scaffold. Code whose
listing ends with a halfword's padding makes what follows it align itself.
overlay: each draft (resource_ID_c_ADDRESS.c or resource_ID/ADDRESS.c)
becomes s<offset>.c in the scene or debug folder its link script names;
every scaffold section it defines goes, and the first one's link line
places the module. Refused: more asm than FAKEMATCH tags, PERM_ macros, a
section with edition forms (unless --edition-forms: the draft covers each
edition's form), a draft defining none of its section's names (unless it
includes a .c file).
edition: an adopted object (games/..., with or without its extension) is
linked in each edition of LIST in place of the rom.s section naming it,
divided by its size as main divides it.
replace: an overlay draft that matches in every edition replaces FILE, an
adopted scene file whose Japanese form stayed on the scaffold; the scaffold
sections the draft defines, and the edition-only sections linked beside
FILE, go with their link lines.
Each draft is first measured as diff measures it (main: in English with its
addresses; overlay: in English; replace: in every edition) and refused
unless it differs in nothing. A measured differ 0 is not yet a match: pools,
padding and an edition's own bytes show only in the linked ROM, so the build
still judges (make compare-editions, then check rom). --unmeasured adopts a
draft diff cannot measure and leaves the build alone to judge it. --verify
has the build judge at once: it makes every edition of the game
(make -k compare-EDITION), and while one differs, sends back to its draft
the item that wrote the object placing the first byte it differs in, or
whose overlay's stream the ROM lacks (as --undo JOURNAL ITEM does), and
builds again; it stops, naming the journal, when an edition does not link
or differs first in something no item wrote.
Drafts lose their leading // header and are clang-formatted. --dry-run
prints what would change. Otherwise the changes are written with a journal
under out/adopt/ that --undo reverses exactly, provided nothing it wrote
has changed since. --undo JOURNAL ITEM... backs out only those items (an
address list, its first address, or a draft or object path): the journal is
undone and its adoption run again without them, under a new journal. One
adopt or undo runs at a time (out/adopt/lock), and none writes over a file
that changed after it read it. LIST: en,de,es,fr,it,ja, or edition names
(tla-en)."#;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Form {
    Main,
    Overlay,
    Edition,
    Replace,
}

#[derive(Debug)]
struct Options {
    form: Form,
    items: Vec<String>,
    editions: Option<Vec<String>>,
    game: Option<String>,
    edition_forms: bool,
    /// Whether each draft is measured before it is adopted.
    measured: bool,
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
    let mut editions = None;
    let mut game = None;
    let mut edition_forms = false;
    let mut measured = true;
    let mut verify = false;
    let mut dry_run = false;
    let mut i = 0;
    while i < arguments.len() {
        let value = arguments[i].as_str();
        match value {
            "--dry-run" => dry_run = true,
            "--edition-forms" => edition_forms = true,
            "--unmeasured" => measured = false,
            "--verify" => verify = true,
            "--undo" => {
                let journal = arguments.get(i + 1).filter(|_| i == 0);
                let items = arguments.get(2..).unwrap_or_default().to_vec();
                if items.iter().any(|item| item.starts_with('-')) {
                    return Err(USAGE.into());
                }
                return journal.map(|j| Request::Undo(PathBuf::from(j), items)).ok_or_else(|| USAGE.to_string());
            }
            "--editions" => {
                i += 1;
                editions = Some(parse_editions(arguments.get(i).ok_or_else(|| USAGE.to_string())?)?);
            }
            "--game" => {
                i += 1;
                game = Some(arguments.get(i).ok_or_else(|| USAGE.to_string())?.clone());
            }
            "-h" | "--help" => return Err(USAGE.into()),
            _ if value.starts_with('-') => return Err(format!("unknown adopt option: {value}\n{USAGE}")),
            _ if form.is_none() => {
                form = Some(match value {
                    "main" => Form::Main,
                    "overlay" => Form::Overlay,
                    "edition" => Form::Edition,
                    "replace" => Form::Replace,
                    _ => return Err(format!("unknown adopt form: {value}\n{USAGE}")),
                })
            }
            _ => items.push(value.to_string()),
        }
        i += 1;
    }
    let form = form.ok_or_else(|| USAGE.to_string())?;
    let fits = match form {
        Form::Main | Form::Overlay => !items.is_empty(),
        Form::Edition => !items.is_empty() && editions.is_some(),
        Form::Replace => items.len() == 2,
    };
    let misplaced = (editions.is_some() && matches!(form, Form::Overlay | Form::Replace))
        || (edition_forms && form != Form::Overlay)
        || (game.is_some() && form != Form::Main)
        || (!measured && form == Form::Edition)
        || (verify && dry_run);
    if !fits || misplaced {
        return Err(USAGE.into());
    }
    if form == Form::Replace {
        // One replacement: FILE and the draft that replaces it.
        items = vec![items.join(" ")];
    }
    Ok(Request::Adopt(Options { form, items, editions, game, edition_forms, measured, verify, dry_run }))
}

fn parse_editions(text: &str) -> Result<Vec<String>, String> {
    let mut editions = Vec::new();
    for name in text.split([',', ' ']).filter(|name| !name.is_empty()) {
        let language = name.rsplit('-').next().unwrap_or(name);
        if !LANGUAGES.contains(&language) {
            return Err(format!("unknown edition {name:?}; expected {}", LANGUAGES.join(",")));
        }
        if !editions.iter().any(|e| e == language) {
            editions.push(language.to_string());
        }
    }
    editions.sort_by_key(|e| LANGUAGES.iter().position(|l| l == e));
    Ok(editions)
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    if arguments.iter().any(|argument| matches!(argument.as_str(), "-h" | "--help")) {
        println!("{USAGE}");
        return Ok(());
    }
    match parse(arguments)? {
        Request::Undo(journal, items) if items.is_empty() => undo(root(), &journal),
        Request::Undo(journal, items) => back_out(root(), &journal, &items),
        Request::Adopt(options) => adopt(root(), &options, &command_line(arguments)),
    }
}

/// The command a journal records, which `back_out` reads back as words.
fn command_line(arguments: &[String]) -> String {
    format!("adopt {}", arguments.join(" "))
}

/// Backs `items` out of a journal's adoption: the journal is undone and the
/// adoption it records run again without them, so the rest stay adopted.
fn back_out(root: &Path, journal: &Path, items: &[String]) -> Result<(), String> {
    back_out_batch(root, journal, items).map(|_| ())
}

/// `back_out`, giving the batch written again, if any item is left.
fn back_out_batch(root: &Path, journal: &Path, items: &[String]) -> Result<Option<Batch>, String> {
    let path = [std::env::current_dir().map(|d| d.join(journal)).ok(), Some(root.join(journal))]
        .into_iter()
        .flatten()
        .find(|path| path.is_file())
        .ok_or_else(|| format!("cannot read {}", journal.display()))?;
    let bytes = std::fs::read(&path).map_err(|error| format!("cannot read {}: {error}", journal.display()))?;
    let command = read_journal(&bytes)?.0.ok_or("the journal records no command")?;
    let words: Vec<String> = command.split_whitespace().skip(1).map(str::to_string).collect();
    let Request::Adopt(mut options) = parse(&words)? else { return Err("the journal records an undo".into()) };
    let named = |item: &str, given: &str| {
        item == given || item.split([',', ' ']).any(|part| part == given || part.ends_with(&format!("/{given}")) || given.ends_with(&format!("/{part}")))
    };
    if let Some(unknown) = items.iter().find(|given| !options.items.iter().any(|item| named(item, given))) {
        return Err(format!("{unknown} is not among the journal's items: {}", options.items.join(" ")));
    }
    options.items.retain(|item| !items.iter().any(|given| named(item, given)));
    undo(root, journal)?;
    if options.items.is_empty() {
        return Ok(None);
    }
    // The rest measured as they were when the journal was written, and
    // verified by whoever called for the back-out.
    let words: Vec<String> = words.into_iter().filter(|word| !items.iter().any(|given| named(word, given)) && word != "--verify").collect();
    options.verify = false;
    adopt_batch(root, &options, &command_line(&words))
}

/// The diff commands that measure an item's draft, from the checkout, or
/// none for a form with no draft.
fn measures(root: &Path, options: &Options, item: &str) -> Result<Vec<Vec<String>>, String> {
    let path = |path: &str| root.join(path).to_string_lossy().into_owned();
    Ok(match options.form {
        Form::Main => {
            let layout = Layout::new(&main_game(root, options)?);
            let first = item.split(',').next().unwrap_or(item);
            vec![vec![path(&layout.main_draft(first)), item.to_string()]]
        }
        Form::Overlay => vec![vec![path(item)]],
        Form::Replace => {
            let draft = item.split_once(' ').map_or(item, |(_, draft)| draft);
            let layout = Layout::of_path(draft).ok_or_else(|| format!("{draft} is not under recon/GAME"))?;
            LANGUAGES.iter().map(|language| vec![path(draft), "--edition".into(), format!("{}-{language}", layout.game)]).collect()
        }
        Form::Edition => Vec::new(),
    })
}

/// Measures an item's draft as diff does: Ok when it differs in nothing.
fn measure(root: &Path, options: &Options, item: &str) -> Result<(), String> {
    for arguments in measures(root, options, item)? {
        let (listing, draft, differ) = crate::diff::measure(&arguments)
            .map_err(|error| format!("diff cannot measure it ({error}); --unmeasured leaves the build to judge"))?;
        if differ != 0 {
            let edition = arguments.iter().skip_while(|word| *word != "--edition").nth(1).map_or(String::new(), |e| format!(" in {e}"));
            return Err(format!("it measures differ {differ}{edition} ({listing} listing, {draft} draft instructions): adopt what matches"));
        }
    }
    Ok(())
}

/// A batch written: its journal, its game, and each item with the source
/// stems it wrote under games/ and the overlay its draft belongs to.
struct Batch {
    journal: String,
    game: String,
    items: Vec<(String, Vec<String>, Option<String>)>,
}

fn adopt(root: &Path, options: &Options, command: &str) -> Result<(), String> {
    let batch = adopt_batch(root, options, command)?;
    match batch.filter(|_| options.verify) {
        Some(batch) => verify(root, batch),
        None => Ok(()),
    }
}

/// Builds every edition of the batch's game and backs out the item the build
/// blames, by what placed the first byte an edition differs in or by the
/// overlay whose stream the ROM lacks, until every edition compares.
fn verify(root: &Path, mut batch: Batch) -> Result<(), String> {
    let ids = crate::alchemy::built::editions(std::slice::from_ref(&batch.game))?;
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
        // An edition the batch changed relinks after its journal was written;
        // one that did not link keeps an older image.
        let written = std::fs::metadata(root.join(&batch.journal)).and_then(|meta| meta.modified()).map_err(|error| format!("{}: {error}", batch.journal))?;
        let mut blamed: Vec<String> = Vec::new();
        for &id in &ids {
            let image = root.join(crate::alchemy::targets::target_for(id).image());
            let linked = std::fs::metadata(&image).and_then(|meta| meta.modified()).is_ok_and(|time| time >= written);
            if !linked {
                let error = made.as_ref().err().map_or("", |error| error.as_str());
                let tail: Vec<&str> = error.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();
                return Err(format!(
                    "{id} did not link, so no byte can be blamed:\n  {}\nback an item out with adopt --undo {} ITEM",
                    tail.join("\n  "),
                    batch.journal
                ));
            }
            let Some(difference) = crate::check::rom::first_difference(root, id)? else { continue };
            let culprit = batch.items.iter().find(|(_, stems, overlay)| {
                difference.object.as_ref().is_some_and(|object| stems.contains(object))
                    || overlay.as_ref().is_some_and(|overlay| difference.overlays.contains(overlay))
            });
            match culprit {
                Some((item, ..)) => {
                    println!("verify: {id} first differs at {}: {item} goes back to its draft", difference.label);
                    if !blamed.contains(item) {
                        blamed.push(item.clone());
                    }
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
            return Err(format!("make failed, yet every {} edition matches: {}", batch.game, made.err().unwrap_or_default()));
        }
        match back_out_batch(root, Path::new(&batch.journal), &blamed)? {
            Some(rest) => batch = rest,
            None => {
                println!("verify: every item went back to its draft");
                return Ok(());
            }
        }
    }
}

fn adopt_batch(root: &Path, options: &Options, command: &str) -> Result<Option<Batch>, String> {
    // Held from the first read to the last write, so no other adoption
    // edits the files this one reads; a dry run writes nothing.
    let _lock = if options.dry_run { None } else { Some(Lock::take(root)?) };
    let mut files = Files::new(root);
    let mut adopted = 0;
    let mut refused = 0;
    let mut written: Vec<(String, Vec<String>, Option<String>)> = Vec::new();
    for item in &options.items {
        if options.measured {
            if let Err(why) = measure(root, options, item) {
                refused += 1;
                println!("refused {item}: {why}");
                continue;
            }
        }
        let saved = files.now.clone();
        let result = match options.form {
            Form::Main => main_game(root, options).and_then(|game| adopt_main(&mut files, &Layout::new(&game), item, options)),
            Form::Overlay => adopt_overlay(&mut files, item, options.edition_forms),
            Form::Edition => adopt_edition(&mut files, item, options),
            Form::Replace => {
                let (file, draft) = item.split_once(' ').unwrap_or((item, ""));
                adopt_replace(&mut files, file, draft)
            }
        };
        match result {
            Ok(lines) => {
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
                    .filter_map(|(path, _)| [".c", ".s", ".S"].iter().find_map(|extension| path.strip_suffix(extension)))
                    .map(str::to_string)
                    .collect();
                written.push((item.clone(), stems, crate::alchemy::overlay::named_in(item)));
            }
            Err(why) => {
                files.now = saved;
                refused += 1;
                println!("refused {item}: {why}");
            }
        }
    }
    if adopted == 0 {
        return Err(format!("nothing adopted ({refused} refused)"));
    }
    if options.dry_run {
        println!("dry run: {adopted} would be adopted, {refused} refused; nothing written");
        return Ok(None);
    }
    let journal = files.commit(command, &journal_name(options))?;
    println!("{adopted} adopted, {refused} refused; journal {journal}");
    if !options.verify {
        println!("judge with make compare-editions, or adopt --verify; undo with alchemy-devtools adopt --undo {journal} [ITEM...]");
    }
    let game = match options.form {
        Form::Main => main_game(root, options)?,
        _ => {
            let path = options.items[0].split(' ').next().unwrap_or("");
            Layout::of_path(path).map(|layout| layout.game).ok_or_else(|| format!("{path} names no game"))?
        }
    };
    Ok(Some(Batch { journal, game, items: written }))
}

/// The game `adopt main` works in: --game, else the one game with English
/// main-image drafts.
fn main_game(root: &Path, options: &Options) -> Result<String, String> {
    if let Some(game) = &options.game {
        return Ok(game.clone());
    }
    let mut games: Vec<String> = std::fs::read_dir(root.join("recon"))
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().join("en/main").is_dir())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    games.sort();
    match games.as_slice() {
        [game] => Ok(game.clone()),
        [] => Err("no game has English main-image drafts".into()),
        _ => Err(format!("drafts in {}: give --game", games.join(" and "))),
    }
}

/// What a journal is named after: its form and first item.
fn journal_name(options: &Options) -> String {
    let form = format!("{:?}", options.form).to_lowercase();
    let first = options.items[0].split([' ', ',']).next().unwrap_or("");
    let first: String = first
        .rsplit('/')
        .next()
        .unwrap_or(first)
        .trim_end_matches(".c")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '-' })
        .collect();
    format!("{form}-{first}")
}

// ---- main ----

fn adopt_main(files: &mut Files, layout: &Layout, item: &str, options: &Options) -> Result<Vec<String>, String> {
    let addresses: Vec<&str> = item.split(',').collect();
    if let Some(bad) = addresses.iter().find(|a| a.len() != 8 || !a.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))) {
        return Err(format!("{bad:?} is not an address (8 lowercase hex digits)"));
    }
    let editions = options.editions.clone().unwrap_or(LANGUAGES.map(String::from).to_vec());
    if !editions.iter().any(|e| e == "en") {
        return Err("adopt main replaces the English listing: --editions must hold en (adopt edition links the others)".into());
    }
    let draft_path = layout.main_draft(addresses[0]);
    let draft = files.read(&draft_path).map_err(|_| format!("no draft {draft_path}"))?;
    check_draft(&draft)?;
    let defined = definitions(&draft);
    let mut listings = Vec::new();
    for address in &addresses {
        let object = layout.listing(address);
        let text = files.read(&format!("{object}.s")).map_err(|_| format!("no listing {object}.s"))?;
        listings.push((object, text));
    }
    let first_names = globals(&lines(&listings[0].1).0);
    let missing: Vec<&String> = first_names.iter().filter(|name| !defined.contains(*name)).collect();
    if !missing.is_empty() {
        return Err(format!("the draft does not define {}", join(&missing)));
    }
    // Further listings may hold labels inside the function or nested functions
    // under other names: their names go with it, defined or not.
    let mut names: Vec<String> = Vec::new();
    for (_, text) in &listings {
        for name in globals(&lines(text).0) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    let known: BTreeSet<String> = defined.iter().cloned().chain(names.iter().cloned()).collect();

    let objects = layout.objects();
    let mut list = ObjectList::parse(&files.read(&objects)?);
    let mut rows = Vec::new();
    for (object, _) in &listings {
        let row = list.rows.iter().position(|row| row.object() == object && row.links("en"));
        rows.push(row.ok_or_else(|| format!("{object} has no row in {objects}"))?);
    }
    let first = *rows.iter().min().unwrap();
    let last = *rows.iter().max().unwrap();
    if let Some(between) = (first..=last).find(|i| !rows.contains(i) && list.rows[*i].links("en")) {
        return Err(format!("{} links in en between the listings", list.rows[between].object()));
    }
    let folder = neighbour_folder(&list, first, layout).ok_or("no games/ object near its row")?;
    let module = layout.main_module(&folder, addresses[0]);
    let target = format!("{module}.c");
    if files.exists(&target) {
        return Err(format!("{target} exists"));
    }
    let last_listing = &listings[rows.iter().position(|&r| r == last).unwrap()].1;
    let halfword = ends_with_padding(&lines(last_listing).0);

    let mut report = vec![format!("main {item} -> {target}")];
    let listing = &list.rows[first];
    let linked = listing.editions().unwrap_or(LANGUAGES.to_vec());
    list.rows[first] = Row::new(listing.image(), listing.place(), &module, ".text", &linked);
    let mut gone: Vec<usize> = rows.iter().copied().filter(|&r| r != first).collect();
    gone.sort_unstable();
    for &r in gone.iter().rev() {
        list.rows.remove(r);
    }
    let listed: Vec<&str> = listings.iter().map(|(object, _)| object.as_str()).collect();
    let verb = if listed.len() > 1 { "become" } else { "becomes" };
    let mut line = format!("  en  {} {verb} the module", listed.join(" "));
    if halfword {
        if let Some(note) = align_next(files, layout, &list, first, "en", &module)? {
            line.push_str(&format!("; {note}"));
        }
    }
    report.push(line);
    for language in editions.iter().filter(|e| *e != "en") {
        let (edition, unknown) = match Edition::read(files.root(), &layout.game, language, &draft_path) {
            Ok(edition) => (Some(edition), None),
            Err(why) => (None, Some(why)),
        };
        match relink(files, layout, &mut list, language, &module, &names, &known, Some(halfword), edition.as_ref()) {
            Ok(done) => report.push(format!("  {language}  {done}")),
            Err(why) => report.push(format!("  {language}  stays on its scaffold: {why}")),
        }
        if let Some(why) = unknown {
            report.push(format!("      cut by its labels alone, its size there unknown: {why}"));
        }
    }
    files.write(&objects, list.render());
    let text = moved(files, &draft, &draft_path, &target, &mut report);
    files.write(&target, text);
    files.remove(&draft_path);
    for (object, _) in &listings {
        files.remove(&format!("{object}.s"));
    }
    Ok(report)
}

/// The folder of the nearest object under the game's sources, gamepak and
/// sound aside (they hold libraries).
fn neighbour_folder(list: &ObjectList, at: usize, layout: &Layout) -> Option<String> {
    let sources = layout.sources();
    for distance in 1..list.rows.len() {
        for index in [at.checked_sub(distance), Some(at + distance)].into_iter().flatten() {
            let Some(rest) = list.rows.get(index).and_then(|row| row.object().strip_prefix(&sources)) else { continue };
            let (folder, _) = rest.rsplit_once('/').unwrap_or(("", rest));
            if !folder.is_empty() && !["gamepak", "sound"].contains(&folder.split('/').next().unwrap_or("")) {
                return Some(format!("{sources}{folder}"));
            }
        }
    }
    None
}

/// What goes from one rom.s section: all of it, or the module's stretch, the
/// rest staying: lines before the module's first label in the section
/// itself, what follows as a section of its own. With the module's size in
/// the edition known, the stretch is its bytes: code after it that no label
/// names stays, and sections its bytes run on into go or are trimmed.
#[derive(Debug)]
struct Cut {
    section: Section,
    row: usize,
    /// The line the module's part starts at, when lines before it stay.
    head: Option<usize>,
    /// What stays after it, as a section of its own: its ROM offset and lines.
    tail: Option<(u32, Vec<String>)>,
    kept: Vec<String>,
    /// The ROM offset of the module's first labelled function, and its name.
    start: Option<(u32, String)>,
    /// Later sections the module's bytes run on into: each goes (None) or
    /// keeps what follows them (its new offset and lines).
    covered: Vec<(Section, Option<(u32, Vec<String>)>)>,
}

/// Links `module` in `language` in place of the edition's rom.s sections
/// that name it. On Err the edition is left as it was: every file it edited
/// is put back, and `list` is untouched.
#[allow(clippy::too_many_arguments)]
fn relink(
    files: &mut Files,
    layout: &Layout,
    list: &mut ObjectList,
    language: &str,
    module: &str,
    names: &[String],
    known: &BTreeSet<String>,
    halfword: Option<bool>,
    edition: Option<&Edition>,
) -> Result<String, String> {
    let saved = files.now.clone();
    let result = relink_edition(files, layout, list, language, module, names, known, halfword, edition);
    if result.is_err() {
        files.now = saved;
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn relink_edition(
    files: &mut Files,
    layout: &Layout,
    list: &mut ObjectList,
    language: &str,
    module: &str,
    names: &[String],
    known: &BTreeSet<String>,
    halfword: Option<bool>,
    edition: Option<&Edition>,
) -> Result<String, String> {
    if list.rows.iter().any(|row| row.object() == module && row.links(language)) {
        return Err("it already links the module".into());
    }
    let scaffold = layout.scaffold(language);
    let path = format!("{scaffold}.s");
    let text = files.read(&path)?;
    let (all, newline) = lines(&text);
    let every = sections(&all);
    let naming: Vec<&Section> = every
        .iter()
        .filter(|s| s.name.starts_with(".rom.") && globals(&all[s.start..s.end]).iter().any(|name| names.contains(name)))
        .collect();
    let mut cuts = Vec::new();
    for &section in &naming {
        let row = list
            .find(&scaffold, &section.name, language)
            .ok_or_else(|| format!("{} has no row in the object list", section.name))?;
        cuts.push(match (edition, naming.len()) {
            (Some(edition), 1) => cut_sized(&all, &every, section.clone(), row, known, edition.compiled.size())?,
            _ => cut(&all, section.clone(), row, known)?,
        });
    }
    if cuts.is_empty() {
        let mut why = format!("no section of {path} names {}", names.join(" "));
        if let Some(section) = unlabelled_beside(list, &all, module, &scaffold, language) {
            why.push_str(&format!(" ({section} beside the module's row has no label: label it, then adopt edition)"));
        }
        return Err(why);
    }
    let sized = edition.is_some() && cuts.len() == 1;
    let halfword = if sized { edition.map(|edition| edition.compiled.size() % 4 == 2) } else { halfword };
    cuts.sort_by_key(|cut| cut.row);
    let rows: Vec<usize> = cuts.iter().map(|cut| cut.row).collect();
    let divided = cuts.iter().any(|cut| cut.head.is_some() || cut.tail.is_some() || !cut.covered.is_empty());
    if cuts.len() > 1 {
        if divided {
            return Err("its sections also hold other functions".into());
        }
        let (first, last) = (rows[0], rows[rows.len() - 1]);
        if let Some(between) = (first..=last).find(|i| !rows.contains(i) && list.rows[*i].links(language)) {
            return Err(format!("{} lies between its sections", list.rows[between].object()));
        }
    }
    let at = rows[0];
    let place = list.rows[at].place().to_string();
    let image = list.rows[at].image().to_string();

    // Join the module's row when nothing this edition links lies between: a
    // head stays before it and a tail follows it. A module whose bytes run on
    // into later sections keeps a row of its own.
    let mut blocked = None;
    let (head, tail) = (cuts[0].head.is_some(), cuts[0].tail.as_ref());
    let joined = if !cuts[0].covered.is_empty() {
        None
    } else {
        let mut candidates: Vec<usize> = (0..list.rows.len())
            .filter(|&r| list.rows[r].object() == module && list.rows[r].place() == place && !list.rows[r].links(language))
            .filter(|&r| !head || r > at)
            .collect();
        candidates.sort_by_key(|&r| r.abs_diff(at));
        candidates.into_iter().find(|&r| {
            let (low, high) = (r.min(at), r.max(at));
            let between = (low + 1..high).find(|i| !rows.contains(i) && list.rows[*i].links(language));
            if let Some(between) = between {
                let row = &list.rows[between];
                blocked.get_or_insert(if row.object() == scaffold {
                    row.sections().to_string()
                } else {
                    row.object().rsplit('/').next().unwrap_or("").to_string()
                });
            }
            between.is_none()
        })
    };

    let mut edited = list.clone();
    let mut module_row = if let Some(r) = joined {
        edited.rows[r] = edited.rows[r].with_edition(language);
        let mut going: Vec<usize> = if head { Vec::new() } else { rows.clone() };
        if let Some((offset, _)) = tail {
            edited.rows.insert(r + 1, Row::new(&image, &place, &scaffold, &rom_section(*offset), &[language]));
            going.iter_mut().filter(|row| **row > r).for_each(|row| *row += 1);
        }
        going.sort_unstable();
        for &row in going.iter().rev() {
            edited.rows.remove(row);
        }
        r - going.iter().filter(|&&row| row < r).count()
    } else {
        let cut = &cuts[0];
        // The sections the module runs on into follow its row in this
        // edition, one after another; each goes or is trimmed.
        let mut previous = at;
        let mut covered_rows = Vec::new();
        for (section, _) in &cut.covered {
            let row = edited.find(&scaffold, &section.name, language).ok_or_else(|| format!("{} has no row in the object list", section.name))?;
            if edited.next_linked(previous, language) != Some(row) {
                return Err(format!("the module runs on into {}, whose row does not follow its own", section.name));
            }
            covered_rows.push(row);
            previous = row;
        }
        for ((_, keep), &row) in cut.covered.iter().zip(&covered_rows).rev() {
            match keep {
                Some((offset, _)) => edited.rows[row].fields[3] = rom_section(*offset),
                None => {
                    edited.rows.remove(row);
                }
            }
        }
        let mut replacement = Vec::new();
        if cut.head.is_some() {
            replacement.push(edited.rows[at].clone());
        }
        replacement.push(Row::new(&image, &place, module, ".text", &[language]));
        if let Some((offset, _)) = &cut.tail {
            replacement.push(Row::new(&image, &place, &scaffold, &rom_section(*offset), &[language]));
        }
        for &row in rows[1..].iter().rev() {
            edited.rows.remove(row);
        }
        edited.rows.splice(at..at + 1, replacement);
        at + usize::from(cut.head.is_some())
    };
    // A row of its own beside another of the module's rows is that row.
    for neighbour in [module_row.checked_sub(1), Some(module_row + 1)].into_iter().flatten() {
        if let Some(merged) = edited.rows.get(neighbour).and_then(|row| merge_rows(row, &edited.rows[module_row])) {
            edited.rows[neighbour] = merged;
            edited.rows.remove(module_row);
            module_row = neighbour.min(module_row);
            break;
        }
    }

    // The scaffold without what goes.
    let mut out: Vec<String> = all.iter().map(|line| line.to_string()).collect();
    let mut edits: Vec<(usize, usize, Vec<String>)> = Vec::new();
    for cut in &cuts {
        let s = &cut.section;
        let mut keep: Vec<String> = Vec::new();
        if let Some(start) = cut.head {
            keep.extend(all[s.start..start].iter().map(|l| l.to_string()));
        }
        if let Some((offset, lines)) = &cut.tail {
            keep.push(format!("\t.section {}, \"ax\"", rom_section(*offset)));
            keep.extend(lines.iter().cloned());
        }
        edits.push((s.start, s.end, keep));
        for (section, rest) in &cut.covered {
            let mut keep = Vec::new();
            if let Some((offset, lines)) = rest {
                keep.push(format!("\t.section {}, \"ax\"", rom_section(*offset)));
                keep.extend(lines.iter().cloned());
            }
            edits.push((section.start, section.end, keep));
        }
    }
    edits.sort_by_key(|(start, ..)| std::cmp::Reverse(*start));
    for (start, end, keep) in edits {
        out.splice(start..end, keep);
    }
    // What the module calls that the edition never labelled is labelled
    // where the edition's own code says it is.
    let mut notes = Vec::new();
    if let (Some(edition), Some((start, first))) = (edition, cuts[0].start.as_ref()) {
        let adopted = adopted_names(files, layout, list, language, edition);
        notes = edition.label_callees(&mut out, *start, first, &adopted)?;
    }
    files.write(&path, join_lines(&out, newline));
    let cut = &cuts[0];
    let mut done = match &cuts[..] {
        [cut] if sized && (cut.tail.is_some() || cut.head.is_some()) => format!(
            "{} keeps what is not the module's {:#x} bytes{}{}{}",
            cut.section.name,
            edition.map_or(0, |edition| edition.compiled.size()),
            cut.start.as_ref().map(|(offset, _)| format!(" at {offset:#x}")).unwrap_or_default(),
            if cut.kept.is_empty() { String::new() } else { format!(" ({} stay)", join(&cut.kept)) },
            cut.tail.as_ref().map(|(offset, _)| format!(", from {} on", rom_section(*offset))).unwrap_or_default()
        ),
        [cut] if divided && cut.covered.is_empty() => format!(
            "{} divides at the module's labels: {} stay{}",
            cut.section.name,
            join(&cut.kept),
            cut.tail.as_ref().map(|(offset, _)| format!(", from {} on", rom_section(*offset))).unwrap_or_default()
        ),
        [cut] => format!("{} goes", cut.section.name),
        _ => format!("{} go", join(&cuts.iter().map(|cut| &cut.section.name).collect::<Vec<_>>())),
    };
    for (section, rest) in &cut.covered {
        done.push_str(&match rest {
            Some((offset, _)) => format!("; {} becomes {}, the module's bytes reaching into it", section.name, rom_section(*offset)),
            None => format!("; {} goes, the module's bytes covering it", section.name),
        });
    }
    done.push_str(&match (joined, blocked) {
        (Some(_), _) => "; joins the module's row".to_string(),
        (None, Some(object)) => format!("; a row of its own ({object} lies between)"),
        (None, None) => "; a row of its own".to_string(),
    });
    match halfword {
        Some(true) => {
            if let Some(note) = align_next(files, layout, &edited, module_row, language, module)? {
                done.push_str(&format!("; {note}"));
            }
        }
        Some(false) => {}
        None => done.push_str("; no edition's map says whether it ends on a halfword: check what follows"),
    }
    for note in notes {
        done.push_str(&format!("\n      {note}"));
    }
    *list = edited;
    Ok(done)
}

/// The one section of an edition's scaffold that carries no label and whose
/// row stands with the module's, nothing else that edition links between:
/// where a function the edition's scaffold never named most likely is.
fn unlabelled_beside(list: &ObjectList, all: &[&str], module: &str, scaffold: &str, language: &str) -> Option<String> {
    let unlabelled = |name: &str| {
        let section = sections(all).into_iter().find(|s| s.name == name)?;
        pieces(all, &section).iter().all(|piece| piece.names.is_empty()).then(|| name.to_string())
    };
    let mut found = BTreeSet::new();
    for r in (0..list.rows.len()).filter(|&r| list.rows[r].object() == module) {
        let steps: [fn(usize, usize) -> Option<usize>; 2] = [usize::checked_sub, usize::checked_add];
        for step in steps {
            let mut index = r;
            while let Some(next) = step(index, 1).filter(|&i| i < list.rows.len()) {
                index = next;
                let row = &list.rows[next];
                if !row.links(language) {
                    continue;
                }
                match (row.object() == scaffold).then(|| unlabelled(row.sections())).flatten() {
                    Some(name) => found.insert(name),
                    None => break,
                };
            }
        }
    }
    (found.len() == 1).then(|| found.into_iter().next().unwrap())
}

/// The names defined by the sources under games/ that `language` links now
/// but did not at its last link: what adoptions since took from its scaffold.
fn adopted_names(files: &mut Files, layout: &Layout, list: &ObjectList, language: &str, edition: &Edition) -> BTreeSet<String> {
    let sources = layout.sources();
    let objects: BTreeSet<String> = list
        .rows
        .iter()
        .filter(|row| row.links(language) && row.object().starts_with(&sources) && !edition.linked_objects.contains(row.object()))
        .map(|row| row.object().to_string())
        .collect();
    let mut names = BTreeSet::new();
    for object in objects {
        for extension in [".c", ".s", ".S"] {
            let Ok(text) = files.read(&format!("{object}{extension}")) else { continue };
            match extension {
                ".c" => names.extend(definitions(&text)),
                _ => names.extend(globals(&lines(&text).0)),
            }
            break;
        }
    }
    names
}

/// Two rows placing the same sections of one object, one beside the other,
/// as one row linking both rows' editions; None when they place different
/// things or share an edition.
fn merge_rows(a: &Row, b: &Row) -> Option<Row> {
    let same = a.image() == b.image() && a.place() == b.place() && a.object() == b.object() && a.sections() == b.sections();
    let (first, second) = (a.editions().unwrap_or(LANGUAGES.to_vec()), b.editions().unwrap_or(LANGUAGES.to_vec()));
    if !same || first.iter().any(|edition| second.contains(edition)) {
        return None;
    }
    let editions: Vec<&str> = first.into_iter().chain(second).collect();
    Some(Row::new(a.image(), a.place(), a.object(), a.fields.get(3).map_or("", String::as_str), &editions))
}

/// Which of a section's pieces go with the module, by its labels.
fn cut(all: &[&str], section: Section, row: usize, known: &BTreeSet<String>) -> Result<Cut, String> {
    let pieces = pieces(all, &section);
    let theirs = |names: &[String]| !names.is_empty() && names.iter().all(|name| !known.contains(name));
    let ours = |names: &[String]| !names.is_empty() && names.iter().all(|name| known.contains(name));
    let start = pieces.iter().find(|piece| ours(&piece.names)).and_then(|piece| Some((piece.offset?, piece.names[0].clone())));
    if pieces.iter().all(|piece| piece.names.is_empty() || ours(&piece.names)) {
        return Ok(Cut { section, row, head: None, tail: None, kept: Vec::new(), start, covered: Vec::new() });
    }
    if let Some(mixed) = pieces.iter().find(|piece| !piece.names.is_empty() && !theirs(&piece.names) && !ours(&piece.names)) {
        return Err(format!("{} labels {} at one place", section.name, mixed.names.join(" and ")));
    }
    let first = pieces.iter().position(|piece| ours(&piece.names)).unwrap();
    let last = pieces.iter().rposition(|piece| ours(&piece.names)).unwrap();
    if let Some(piece) = pieces[first..=last].iter().find(|piece| theirs(&piece.names)) {
        return Err(format!("{} holds {} amid the module's functions", section.name, piece.names.join(" ")));
    }
    let unnamed = |index: Option<usize>| index.and_then(|i| pieces.get(i)).is_some_and(|piece| piece.names.is_empty());
    if unnamed(first.checked_sub(1)) || unnamed(Some(last + 1)) {
        return Err(format!("{} holds bytes beside the module's that no label names", section.name));
    }
    let tail = match pieces.get(last + 1) {
        Some(piece) => {
            let offset = piece.offset.ok_or_else(|| format!("{} has a label with no bytes", section.name))?;
            Some((offset, all[piece.start..section.end].iter().map(|line| line.to_string()).collect()))
        }
        None => None,
    };
    let kept = pieces.iter().filter(|piece| theirs(&piece.names)).flat_map(|piece| piece.names.clone()).collect();
    Ok(Cut { section, row, head: (first > 0).then_some(pieces[first].start), tail, kept, start, covered: Vec::new() })
}

/// The ROM offset just past a section's last bytes.
fn section_end(all: &[&str], section: &Section) -> Option<u32> {
    all[section.start..section.end].iter().filter_map(|line| incbin(line)).map(|(offset, length)| offset + length).max()
}

/// Which of a section's bytes go with the module, by its size: the module's
/// `size` bytes from its first label. What comes before stays in the
/// section, what follows becomes a section of its own with the labels it
/// holds, and later sections the bytes run on into go or are trimmed.
fn cut_sized(all: &[&str], every: &[Section], section: Section, row: usize, known: &BTreeSet<String>, size: u32) -> Result<Cut, String> {
    let parts = pieces(all, &section);
    let ours = |names: &[String]| !names.is_empty() && names.iter().all(|name| known.contains(name));
    if let Some(mixed) = parts.iter().find(|piece| !piece.names.is_empty() && !ours(&piece.names) && piece.names.iter().any(|n| known.contains(n))) {
        return Err(format!("{} labels {} at one place", section.name, mixed.names.join(" and ")));
    }
    let first = parts.iter().position(|piece| ours(&piece.names)).ok_or_else(|| format!("{} labels none of the module's functions", section.name))?;
    let start = parts[first].offset.ok_or_else(|| format!("{} has a label with no bytes", section.name))?;
    if start % 4 != 0 {
        return Err(format!("{} places the module at {start:#x}, which is not on a word", section.name));
    }
    let end = start + size;
    let mut kept: Vec<String> = parts[..first].iter().flat_map(|piece| piece.names.clone()).collect();
    let tail = clip(all, &parts[first..], end, known, &section.name, &mut kept)?;
    let mut covered = Vec::new();
    let mut reach = section_end(all, &section).ok_or_else(|| format!("{} places no bytes", section.name))?;
    while end > reach {
        let next = every
            .iter()
            .find(|s| rom_offset(&s.name) == Some(reach))
            .ok_or_else(|| format!("the module's {size:#x} bytes run on past {}, and no section starts at {reach:#x}", section.name))?;
        let rest = clip(all, &pieces(all, next), end, known, &next.name, &mut kept)?;
        reach = section_end(all, next).ok_or_else(|| format!("{} places no bytes", next.name))?;
        covered.push((next.clone(), rest));
    }
    let head = (first > 0).then_some(parts[first].start);
    let name = parts[first].names[0].clone();
    Ok(Cut { section, row, head, tail, kept, start: Some((start, name)), covered })
}

/// The lines of `pieces` from ROM offset `end` on, with the labels there
/// (each added to `kept`), and the offset they start at; None when the
/// module's bytes cover them all. A label of another function before `end`
/// lies amid the module, and one of the module's after it past its bytes.
fn clip(all: &[&str], pieces: &[Piece], end: u32, known: &BTreeSet<String>, name: &str, kept: &mut Vec<String>) -> Result<Option<(u32, Vec<String>)>, String> {
    let mut lines = Vec::new();
    let mut at = None;
    for piece in pieces {
        let offset = piece.offset.ok_or_else(|| format!("{name} has a label with no bytes"))?;
        if offset >= end {
            if let Some(ours) = piece.names.iter().find(|n| known.contains(*n)) {
                return Err(format!("{name} labels {ours} past the module's bytes, at {offset:#x}"));
            }
            at.get_or_insert(offset);
            kept.extend(piece.names.iter().cloned());
            lines.extend(all[piece.start..piece.end].iter().map(|line| line.to_string()));
            continue;
        }
        if let Some(other) = piece.names.iter().find(|n| !known.contains(*n)) {
            return Err(format!("{name} labels {other} amid the module's bytes, at {offset:#x}"));
        }
        for line in &all[piece.start..piece.end] {
            let Some((offset, length)) = incbin(line) else { continue };
            if offset + length <= end {
                continue;
            }
            let from = offset.max(end);
            at.get_or_insert(from);
            lines.push(incbin_line(from, offset + length - from));
        }
    }
    Ok(at.map(|at| (at, lines)))
}

/// Code that ends on a halfword leaves what follows it 2 bytes early unless
/// that aligns itself, as its listing's padding did. Makes the next object
/// `language` links after `index` align itself when it starts on a word.
fn align_next(files: &mut Files, layout: &Layout, list: &ObjectList, index: usize, language: &str, module: &str) -> Result<Option<String>, String> {
    let Some(next) = list.next_linked(index, language) else { return Ok(None) };
    let row = &list.rows[next];
    let object = row.object();
    let name = module.rsplit('/').next().unwrap_or(module);
    if object == layout.scaffold(language) {
        let section = row.sections();
        if rom_offset(section).is_none_or(|offset| offset % 4 != 0) {
            return Ok(None);
        }
        let path = format!("{object}.s");
        let text = files.read(&path)?;
        let (all, newline) = lines(&text);
        let Some(found) = sections(&all).into_iter().find(|s| s.name == section) else {
            return Ok(Some(format!("{section} follows {name}, which ends on a halfword, but {path} lacks it")));
        };
        if aligns(&all[found.start..found.end]) {
            return Ok(None);
        }
        let mut out = all.clone();
        out.insert(found.start + 1, "\t.balign 4");
        files.write(&path, join_lines(&out, newline));
        return Ok(Some(format!("{section} aligns itself after {name}, which ends on a halfword")));
    }
    if let Some(address) = layout.listing_address(object) {
        if u32::from_str_radix(address, 16).is_ok_and(|address| address % 4 != 0) {
            return Ok(None);
        }
        let path = format!("{object}.s");
        let text = files.read(&path)?;
        let (all, newline) = lines(&text);
        if aligns(&all) {
            return Ok(None);
        }
        let at = all.iter().position(|line| line.trim() == ".thumb" || line.trim() == ".arm").map_or(1, |i| i + 1);
        let mut out = all.clone();
        out.insert(at.min(out.len()), "\t.balign 4");
        files.write(&path, join_lines(&out, newline));
        return Ok(Some(format!("{address} aligns itself after {name}, which ends on a halfword")));
    }
    if object.starts_with(&layout.sources()) && files.exists(&format!("{object}.c")) {
        return Ok(None);
    }
    Ok(Some(format!("{object} follows {name}, which ends on a halfword: check that it aligns")))
}

// ---- edition ----

fn adopt_edition(files: &mut Files, item: &str, options: &Options) -> Result<Vec<String>, String> {
    let object = item.trim_start_matches("./");
    let object = [".c", ".s", ".S"].iter().find_map(|x| object.strip_suffix(x)).unwrap_or(object);
    let layout = Layout::of_path(object)
        .filter(|layout| object.starts_with(&layout.sources()))
        .ok_or_else(|| format!("{object} is not under games/GAME/src"))?;
    let source = [".c", ".s", ".S"]
        .iter()
        .map(|x| format!("{object}{x}"))
        .find(|path| files.exists(path))
        .ok_or_else(|| format!("{object} has no source"))?;
    let text = files.read(&source)?;
    let defined: BTreeSet<String> =
        if source.ends_with(".c") { definitions(&text) } else { globals(&lines(&text).0).into_iter().collect() };
    let names: Vec<String> = defined.iter().cloned().collect();
    let objects = layout.objects();
    let mut list = ObjectList::parse(&files.read(&objects)?);
    if !list.rows.iter().any(|row| row.object() == object) {
        return Err(format!("{object} has no row in {objects}: adopt its draft first"));
    }
    let halfword = halfword_from_maps(files.root(), &layout, &list, object);
    let mut report = vec![format!("edition {object}")];
    let mut linked = 0;
    for language in options.editions.as_deref().unwrap_or_default() {
        if list.rows.iter().any(|row| row.object() == object && row.links(language)) {
            report.push(format!("  {language}  already links it"));
            continue;
        }
        let (edition, unknown) = match source.ends_with(".c") {
            true => match Edition::read(files.root(), &layout.game, language, &source) {
                Ok(edition) => (Some(edition), None),
                Err(why) => (None, Some(why)),
            },
            false => (None, None),
        };
        match relink(files, &layout, &mut list, language, object, &names, &defined, halfword, edition.as_ref()) {
            Ok(done) => {
                linked += 1;
                report.push(format!("  {language}  {done}"));
            }
            Err(why) => report.push(format!("  {language}  stays on its scaffold: {why}")),
        }
        if let Some(why) = unknown {
            report.push(format!("      cut by its labels alone, its size there unknown: {why}"));
        }
    }
    if linked == 0 {
        return Err(format!("no edition links it anew\n{}", report[1..].join("\n")));
    }
    files.write(&objects, list.render());
    Ok(report)
}

/// Whether an adopted object's code ends on a halfword, from the map of an
/// edition that links it (build output, not the ROM).
fn halfword_from_maps(root: &Path, layout: &Layout, list: &ObjectList, object: &str) -> Option<bool> {
    let linking = LANGUAGES.iter().filter(|language| list.rows.iter().any(|row| row.object() == object && row.links(language)));
    linking.into_iter().find_map(|language| {
        let edition = format!("{}-{language}", layout.game);
        let text = std::fs::read_to_string(root.join(format!("out/{edition}/{edition}.map"))).ok()?;
        let map = Map::parse(&text);
        map.placed.iter().find(|p| p.name == ".text" && object_stem(&p.path) == Some(object)).map(|p| p.size % 4 == 2)
    })
}

// ---- overlays ----

struct Overlay {
    layout: Layout,
    id: String,
    address: u32,
    script_path: String,
    scaffold_path: String,
}

fn overlay(path: &str) -> Result<Overlay, String> {
    let (id, address) = overlay_draft(path)
        .ok_or_else(|| format!("{path} is not an overlay draft (resource_ID_c_ADDRESS.c or resource_ID/ADDRESS.c)"))?;
    let layout = Layout::of_path(path).ok_or_else(|| format!("{path} is not under recon/GAME"))?;
    Ok(Overlay { script_path: layout.overlay_script(&id), scaffold_path: layout.overlay_scaffold(&id), layout, id, address })
}

/// The scaffold sections a draft defines, each with its link line's index,
/// in link order.
fn defined_sections(o: &Overlay, draft: &str, script: &[&str], scaffold: &[&str], edition_forms: bool) -> Result<Vec<(Section, usize)>, String> {
    let defined = definitions(draft);
    let own = overlay_section(o.address);
    let all = sections(scaffold);
    let mut going = Vec::new();
    for section in all.iter().filter(|s| s.name.starts_with(".text.x")) {
        let names = globals(&scaffold[section.start..section.end]);
        let mine = names.iter().filter(|name| defined.contains(*name)).count();
        let included = section.name == own && includes_c(draft);
        if mine == 0 && !included {
            continue;
        }
        if mine < names.len() && !included {
            let others: Vec<&String> = names.iter().filter(|name| !defined.contains(*name)).collect();
            return Err(format!("{} also holds {}", section.name, join(&others)));
        }
        if !edition_forms && has_edition_forms(&scaffold[section.start..section.end]) {
            return Err(format!("{} has edition forms (--edition-forms when the draft covers each)", section.name));
        }
        let line = o.layout.overlay_scaffold_line(&o.id, &section.name);
        let at = script
            .iter()
            .position(|l| l.trim() == line)
            .ok_or_else(|| format!("{} does not link {}", o.script_path, section.name))?;
        going.push((section.clone(), at));
    }
    if !going.iter().any(|(section, _)| section.name == own) {
        let names = all.iter().find(|s| s.name == own).map(|s| globals(&scaffold[s.start..s.end]));
        return Err(match names {
            None => format!("{} has no section {own}", o.scaffold_path),
            Some(names) => format!("the draft does not define {}", names.join(" ")),
        });
    }
    going.sort_by_key(|(_, at)| *at);
    Ok(going)
}

fn drop_sections(scaffold: &[&str], going: &[(Section, usize)]) -> Vec<String> {
    let mut out: Vec<String> = scaffold.iter().map(|l| l.to_string()).collect();
    let mut by_line: Vec<&Section> = going.iter().map(|(section, _)| section).collect();
    by_line.sort_by_key(|s| s.start);
    for section in by_line.iter().rev() {
        out.drain(section.start..section.end);
    }
    out
}

fn adopt_overlay(files: &mut Files, draft_path: &str, edition_forms: bool) -> Result<Vec<String>, String> {
    let draft_path = draft_path.trim_start_matches("./");
    let o = overlay(draft_path)?;
    let draft = files.read(draft_path).map_err(|_| format!("no draft {draft_path}"))?;
    check_draft(&draft)?;
    let script_text = files.read(&o.script_path)?;
    let scaffold_text = files.read(&o.scaffold_path)?;
    let (script, script_newline) = lines(&script_text);
    let (scaffold, scaffold_newline) = lines(&scaffold_text);
    let going = defined_sections(&o, &draft, &script, &scaffold, edition_forms)?;
    let at: Vec<usize> = going.iter().map(|(_, at)| *at).collect();
    if at.windows(2).any(|pair| pair[1] != pair[0] + 1) {
        return Err(format!("its sections are apart in {}", o.script_path));
    }
    let folder = o
        .layout
        .overlay_folder(&script_text)
        .ok_or_else(|| format!("{} names no scene or debug folder", o.script_path))?;
    let module = o.layout.overlay_module(&folder, o.address);
    let target = format!("{module}.c");
    if files.exists(&target) {
        return Err(format!("{target} exists"));
    }
    let (last, _) = &going[going.len() - 1];
    let halfword = ends_with_padding(&scaffold[last.start..last.end]);

    let line = script[at[0]];
    let indent = &line[..line.len() - line.trim_start().len()];
    let mut new_script: Vec<String> = script.iter().map(|l| l.to_string()).collect();
    new_script.splice(at[0]..=at[at.len() - 1], [format!("{indent}\"*/{module}.o\"(.text)")]);
    let mut new_scaffold = drop_sections(&scaffold, &going);
    let names: Vec<&str> = going.iter().map(|(s, _)| s.name.as_str()).collect();
    let verb = if names.len() > 1 { "go" } else { "goes" };
    let mut every = format!("  every edition  {} {verb} from {}", names.join(" "), o.scaffold_path);
    if halfword {
        if let Some(note) = align_overlay_next(&o, &new_script, at[0], &mut new_scaffold, &module) {
            every.push_str(&format!("; {note}"));
        }
    }
    let mut report = vec![format!("overlay {draft_path} -> {target}"), every];
    files.write(&o.script_path, join_lines(&new_script, script_newline));
    files.write(&o.scaffold_path, join_lines(&new_scaffold, scaffold_newline));
    let text = moved(files, &draft, draft_path, &target, &mut report);
    files.write(&target, text);
    files.remove(draft_path);
    Ok(report)
}

/// The overlay counterpart of align_next: the scaffold section the link
/// script places next aligns itself when it starts on a word.
fn align_overlay_next(o: &Overlay, script: &[String], index: usize, scaffold: &mut Vec<String>, module: &str) -> Option<String> {
    let next = script.get(index + 1)?.trim();
    let prefix = format!("\"*/resource_{}_overlay.o\"(", o.id);
    let section = next.strip_prefix(&prefix)?.strip_suffix(')')?;
    let address = u32::from_str_radix(section.strip_prefix(".text.x")?, 16).ok()?;
    if address % 4 != 0 {
        return None;
    }
    let view: Vec<&str> = scaffold.iter().map(String::as_str).collect();
    let found = sections(&view).into_iter().find(|s| s.name == section)?;
    if aligns(&view[found.start..found.end]) {
        return None;
    }
    scaffold.insert(found.start + 1, "\t.balign 4".into());
    let name = module.rsplit('/').next().unwrap_or(module);
    Some(format!("{section} aligns itself after {name}, which ends on a halfword"))
}

fn adopt_replace(files: &mut Files, file: &str, draft_path: &str) -> Result<Vec<String>, String> {
    let (file, draft_path) = (file.trim_start_matches("./"), draft_path.trim_start_matches("./"));
    let o = overlay(draft_path)?;
    let object = file.strip_suffix(".c").ok_or_else(|| format!("{file} is not a C file"))?;
    if !object.starts_with(&o.layout.sources()) || !files.exists(file) {
        return Err(format!("{file} is not an adopted source"));
    }
    let draft = files.read(draft_path).map_err(|_| format!("no draft {draft_path}"))?;
    check_draft(&draft)?;
    let script_text = files.read(&o.script_path)?;
    let scaffold_text = files.read(&o.scaffold_path)?;
    let (script, script_newline) = lines(&script_text);
    let (scaffold, scaffold_newline) = lines(&scaffold_text);
    let own = format!("\"*/{object}.o\"(.text)");
    let file_at = script
        .iter()
        .position(|l| l.trim() == own)
        .ok_or_else(|| format!("{} does not link {object}", o.script_path))?;
    let mut going = defined_sections(&o, &draft, &script, &scaffold, true)?;
    // The form left on the scaffold may run on through sections that name
    // nothing the draft defines: every edition-only section linked beside
    // the file goes with it.
    let all = sections(&scaffold);
    let prefix = format!("\"*/resource_{}_overlay.o\"(", o.id);
    let linked = |index: usize| {
        let name = script.get(index)?.trim().strip_prefix(&prefix)?.strip_suffix(')')?;
        all.iter().find(|s| s.name == name)
    };
    let steps: [fn(usize, usize) -> Option<usize>; 2] = [usize::checked_sub, usize::checked_add];
    for step in steps {
        let mut index = file_at;
        while let Some(next) = step(index, 1) {
            let Some(section) = linked(next) else { break };
            let known = going.iter().any(|(g, _)| g.name == section.name);
            if !known && !edition_only(&scaffold[section.start..section.end]) {
                break;
            }
            if !known {
                going.push((section.clone(), next));
            }
            index = next;
        }
    }
    going.sort_by_key(|(_, at)| *at);
    let mut at: Vec<usize> = going.iter().map(|(_, at)| *at).chain([file_at]).collect();
    at.sort_unstable();
    if at.windows(2).any(|pair| pair[1] != pair[0] + 1) {
        return Err(format!("its sections and {object} are apart in {}", o.script_path));
    }
    let mut new_script: Vec<String> = script.iter().map(|l| l.to_string()).collect();
    for (_, at) in going.iter().rev() {
        new_script.remove(*at);
    }
    let new_scaffold = drop_sections(&scaffold, &going);
    let names: Vec<&str> = going.iter().map(|(s, _)| s.name.as_str()).collect();
    let mut report = vec![
        format!("replace {file} with {draft_path}"),
        format!("  every edition  {} and the lines linking them go from {} and {}", names.join(" "), o.scaffold_path, o.script_path),
    ];
    files.write(&o.script_path, join_lines(&new_script, script_newline));
    files.write(&o.scaffold_path, join_lines(&new_scaffold, scaffold_newline));
    let text = moved(files, &draft, draft_path, file, &mut report);
    files.write(file, text);
    files.remove(draft_path);
    Ok(report)
}

// ---- drafts ----

/// A draft's text as its module: no header, relative includes still
/// reaching their files, clang-formatted.
fn moved(files: &Files, draft: &str, from: &str, to: &str, report: &mut Vec<String>) -> String {
    let text = rewrite_includes(&headerless(draft), parent(from), parent(to), |path| files.exists(path));
    match clang_format(files.root(), to, &text) {
        Ok(formatted) => formatted,
        Err(why) => {
            report.push(format!("  {to} is not formatted: {why}"));
            text
        }
    }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// The draft without its leading `//` header (a `///` comment is the module's
/// own and stays).
fn headerless(text: &str) -> String {
    let mut rest = text;
    while !rest.is_empty() {
        let end = rest.find('\n').map_or(rest.len(), |i| i + 1);
        let line = &rest[..end];
        let header = line.starts_with("//") && !line.starts_with("///");
        if !header && !line.trim().is_empty() {
            break;
        }
        rest = &rest[end..];
    }
    rest.to_string()
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

fn clang_format(root: &Path, path: &str, text: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("clang-format")
        .arg(format!("--assume-filename={}", root.join(path).display()))
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("clang-format: {error}"))?;
    // clang-format reads all its input before it writes.
    child.stdin.take().unwrap().write_all(text.as_bytes()).map_err(|error| format!("clang-format: {error}"))?;
    let output = child.wait_with_output().map_err(|error| format!("clang-format: {error}"))?;
    if !output.status.success() {
        return Err(format!("clang-format: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    String::from_utf8(output.stdout).map_err(|_| "clang-format wrote something other than text".into())
}

/// Refuses what S1 and S2 keep out of games/: more asm than FAKEMATCH tags,
/// and PERM_ macros the permuter left.
fn check_draft(text: &str) -> Result<(), String> {
    let tokens = lex(text);
    let words = || tokens.iter().filter(|t| t.kind == Kind::Ident).map(|t| t.text);
    let asm = words().filter(|w| matches!(*w, "asm" | "__asm" | "__asm__")).count();
    let tags = tokens.iter().filter(|t| t.kind == Kind::Comment && t.text.starts_with("/* FAKEMATCH(")).count();
    if asm > tags {
        return Err(format!("{asm} asm with {tags} FAKEMATCH tags (S2)"));
    }
    if let Some(perm) = words().find(|w| w.starts_with("PERM_")) {
        return Err(format!("a permuter macro is left in ({perm})"));
    }
    Ok(())
}

/// The functions a C file defines, nested ones included: a name after a type
/// or `*`, its parameters, then a body.
fn definitions(text: &str) -> BTreeSet<String> {
    const KEYWORDS: [&str; 14] =
        ["if", "while", "for", "switch", "return", "sizeof", "do", "else", "case", "typeof", "__typeof__", "asm", "__asm__", "__attribute__"];
    let tokens: Vec<Token> = lex(text).into_iter().filter(|t| !t.pp && t.kind != Kind::Comment).collect();
    let punct = |i: usize, text: &str| tokens.get(i).is_some_and(|t| t.kind == Kind::Punct && t.text == text);
    let mut defined = BTreeSet::new();
    for (i, token) in tokens.iter().enumerate() {
        if token.kind != Kind::Ident || KEYWORDS.contains(&token.text) || !punct(i + 1, "(") {
            continue;
        }
        let typed = i.checked_sub(1).and_then(|p| tokens.get(p)).is_some_and(|t| t.kind == Kind::Ident || t.text == "*");
        let mut depth = 0;
        let close = (i + 1..tokens.len()).find(|&j| {
            depth += i32::from(punct(j, "(")) - i32::from(punct(j, ")"));
            depth == 0
        });
        if typed && close.is_some_and(|close| punct(close + 1, "{")) {
            defined.insert(token.text.to_string());
        }
    }
    defined
}

fn includes_c(text: &str) -> bool {
    let tokens = lex(text);
    tokens.windows(3).any(|t| {
        t[0].kind == Kind::PpStart && t[1].text == "include" && t[2].kind == Kind::Str && t[2].text.ends_with(".c\"")
    })
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

fn join<S: AsRef<str>>(names: &[S]) -> String {
    names.iter().map(AsRef::as_ref).collect::<Vec<_>>().join(" ")
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

    fn root(&self) -> &Path {
        &self.root
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

    #[test]
    fn forms_parse_and_refuse_what_they_cannot_take() {
        let Ok(Request::Adopt(o)) = parse(&args(&["main", "080e2264", "0819284c,08192894", "--editions", "tla-ja,en", "--dry-run"])) else {
            panic!("main did not parse")
        };
        assert_eq!((o.form, o.items.len(), o.dry_run), (Form::Main, 2, true));
        assert_eq!(o.editions.unwrap(), ["en", "ja"]);
        assert!(parse(&args(&["edition", "games/tla/src/x"])).is_err());
        assert!(parse(&args(&["overlay", "a.c", "--editions", "en"])).is_err());
        assert!(parse(&args(&["replace", "a.c"])).is_err());
        assert!(parse(&args(&["main", "1", "--editions", "xx"])).is_err());
        assert!(matches!(parse(&args(&["--undo", "j"])), Ok(Request::Undo(_, items)) if items.is_empty()));
        assert!(matches!(parse(&args(&["--undo", "j", "08000100"])), Ok(Request::Undo(_, items)) if items == ["08000100"]));
        assert!(parse(&args(&["edition", "games/tla/src/x", "--editions", "ja", "--unmeasured"])).is_err());
    }

    #[test]
    fn definitions_include_nested_functions_and_skip_calls() {
        let text = "// void Header(void) {\n#define M(x) { x }\nstatic inline s32 Flag_IsClear(s32 flag) { return !GameFlag_Test(flag); }\nstruct Actor *\nFunc_02003df4(s32 type,\n  s32 x)\n{\n  void nested(void) {\n    if (x) {\n      Call(\"f(\");\n    }\n  }\n  else if (y) { }\n  FOO(x) { }\n  return 0;\n}\n";
        let defined: Vec<String> = definitions(text).into_iter().collect();
        assert_eq!(defined, ["Flag_IsClear", "Func_02003df4", "nested"]);
        assert!(includes_c("#include \"../world_map/s36a8.c\"\n"));
        assert!(!includes_c("// #include \"x.c\"\n#include \"x.h\"\n"));
    }

    #[test]
    fn drafts_with_untagged_asm_or_permuter_macros_are_refused() {
        assert!(check_draft("void f(void) { asm(\"\"); }").is_err());
        assert!(check_draft("void f(void) {\n  /* FAKEMATCH(barrier): x; as y, 1 of 2 instructions differ in tla-en. */\n  asm(\"\");\n}").is_ok());
        assert!(check_draft("// asm in a comment\nvoid f(void) { g(\"asm\"); }").is_ok());
        assert!(check_draft("void f(void) { PERM_GENERAL(a, b); }").is_err());
    }

    #[test]
    fn a_header_goes_and_includes_follow_the_move() {
        let text = "// Matches (tla-en)\n// notes\n\n/// @file\n#include \"../../../../games/tla/src/scene/a/a.h\"\n#include \"types.h\"\n";
        let headless = headerless(text);
        assert!(headless.starts_with("/// @file\n"));
        let moved = rewrite_includes(&headless, "recon/tla/en/overlays", "games/tla/src/scene/b", |p| p == "games/tla/src/scene/a/a.h");
        assert!(moved.contains("#include \"../a/a.h\"\n"));
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
        std::fs::write(root.join("objects.tsv"), "a\nb\nc\nd\ne\n").unwrap();
        let mut first = Files::new(root);
        first.write("objects.tsv", "a\nB\nc\nd\ne\n".into());
        let first = first.commit("adopt edition x --editions ja", "first").unwrap();
        let mut second = Files::new(root);
        second.write("objects.tsv", "a\nB\nc\nd\nE\nf\n".into());
        let second = second.commit("adopt edition y --editions ja", "second").unwrap();
        undo(root, Path::new(&first)).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("objects.tsv")).unwrap(), "a\nb\nc\nd\nE\nf\n");
        undo(root, Path::new(&second)).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("objects.tsv")).unwrap(), "a\nb\nc\nd\ne\n");

        // A later change to the same line leaves both journals in place.
        let mut third = Files::new(root);
        third.write("objects.tsv", "a\nB\nc\nd\ne\n".into());
        let third = third.commit("adopt edition x --editions ja", "third").unwrap();
        std::fs::write(root.join("objects.tsv"), "a\nBB\nc\nd\ne\n").unwrap();
        assert!(undo(root, Path::new(&third)).unwrap_err().contains("objects.tsv"));
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
        std::fs::write(root.join("objects.tsv"), "a\n").unwrap();
        let mut first = Files::new(root);
        let mut second = Files::new(root);
        first.write("objects.tsv", "a\nb\n".into());
        second.write("objects.tsv", "a\nc\n".into());
        second.write("made.c", "int c;\n".into());
        first.commit("adopt first", "first").unwrap();
        assert!(second.commit("adopt second", "second").unwrap_err().contains("objects.tsv"));
        assert_eq!(std::fs::read_to_string(root.join("objects.tsv")).unwrap(), "a\nb\n");
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

    /// German's section goes and joins the module's row; Japanese's also holds
    /// the next function, which stays as a section of its own after the
    /// module's joined row; the code ends
    /// on a halfword, so what follows it in each edition aligns itself.
    #[test]
    fn main_adoption_relinks_each_edition_and_undoes() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let tree = [
            ("games/tla/objects.tsv", "image\tplace\tobject\tsections\teditions\nmain\trom\tgames/tla/src/field/a\t.text\nmain\trom\trecon/tla/de/rom\t.rom.00000100\tde\nmain\trom\trecon/tla/raw/08000100\t.text\ten\nmain\trom\trecon/tla/ja/rom\t.rom.00000100\tja\nmain\trom\trecon/tla/raw/08000120\t.text\ten\nmain\trom\trecon/tla/de/rom\t.rom.00000120\tde\nmain\trom\tgames/tla/src/field/b\t.text\n"),
            ("recon/tla/raw/08000100.s", ".syntax unified\n\t.thumb\n\t.global Func_08000100\n\t.thumb_func\nFunc_08000100:\n\tbx lr\n\t.2byte 0x0000\n"),
            ("recon/tla/raw/08000120.s", ".syntax unified\n\t.thumb\n\t.global Func_08000120\n\t.thumb_func\nFunc_08000120:\n\tbx lr\n"),
            ("recon/tla/en/main/08000100.c", "// Matches: differ 0 (tla-en)\nvoid Func_08000100(void) {}\n"),
            ("recon/tla/de/rom.s", "\t.section .rom.00000100, \"ax\"\n\t.global Func_08000100\nFunc_08000100:\n\t.incbin \"baserom.gba\", 0x00000100, 0x00000004\n\t.section .rom.00000120, \"ax\"\n\t.global Func_08000120\nFunc_08000120:\n\t.incbin \"baserom.gba\", 0x00000120, 0x00000002\n"),
            ("recon/tla/ja/rom.s", "\t.section .rom.00000100, \"ax\"\n\t.global Func_08000100\nFunc_08000100:\n\t.incbin \"baserom.gba\", 0x00000100, 0x00000004\n\t.global Func_08000120\nFunc_08000120:\n\t.incbin \"baserom.gba\", 0x00000104, 0x00000002\n"),
        ];
        for (path, text) in tree {
            std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            std::fs::write(root.join(path), text).unwrap();
        }
        let Ok(Request::Adopt(options)) = parse(&args(&["main", "08000100", "--editions", "en,de,ja", "--game", "tla", "--unmeasured"])) else {
            panic!("main did not parse")
        };
        adopt(root, &options, "adopt test").unwrap();
        let read = |path: &str| std::fs::read_to_string(root.join(path)).unwrap();
        assert_eq!(
            read("games/tla/objects.tsv"),
            "image\tplace\tobject\tsections\teditions\nmain\trom\tgames/tla/src/field/a\t.text\nmain\trom\tgames/tla/src/field/f000100\t.text\ten de ja\nmain\trom\trecon/tla/ja/rom\t.rom.00000104\tja\nmain\trom\trecon/tla/raw/08000120\t.text\ten\nmain\trom\trecon/tla/de/rom\t.rom.00000120\tde\nmain\trom\tgames/tla/src/field/b\t.text\n"
        );
        assert_eq!(read("recon/tla/raw/08000120.s"), ".syntax unified\n\t.thumb\n\t.balign 4\n\t.global Func_08000120\n\t.thumb_func\nFunc_08000120:\n\tbx lr\n");
        assert_eq!(read("recon/tla/de/rom.s"), "\t.section .rom.00000120, \"ax\"\n\t.balign 4\n\t.global Func_08000120\nFunc_08000120:\n\t.incbin \"baserom.gba\", 0x00000120, 0x00000002\n");
        assert_eq!(read("recon/tla/ja/rom.s"), "\t.section .rom.00000104, \"ax\"\n\t.balign 4\n\t.global Func_08000120\nFunc_08000120:\n\t.incbin \"baserom.gba\", 0x00000104, 0x00000002\n");
        assert!(read("games/tla/src/field/f000100.c").starts_with("void Func_08000100(void)"));
        assert!(!root.join("recon/tla/raw/08000100.s").exists() && !root.join("recon/tla/en/main/08000100.c").exists());
        let journal = std::fs::read_dir(root.join("out/adopt")).unwrap().flatten().next().unwrap().path();
        assert!(journal.file_name().unwrap().to_str().unwrap().starts_with("0001-main-08000100"));
        undo(root, &journal).unwrap();
        for (path, text) in tree {
            assert_eq!(read(path), text, "{path}");
        }
        assert!(!root.join("games/tla/src/field/f000100.c").exists());
    }

    /// German's next row is a listing with no file, so aligning what follows
    /// fails: German stays on its scaffold with its section where it was.
    #[test]
    fn an_edition_that_stays_on_its_scaffold_keeps_its_section() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let objects = "image\tplace\tobject\tsections\teditions\nmain\trom\tgames/tla/src/field/a\t.text\nmain\trom\trecon/tla/raw/08000100\t.text\ten\nmain\trom\trecon/tla/de/rom\t.rom.00000100\tde\nmain\trom\trecon/tla/raw/08000200\t.text\tde\n";
        let de = "\t.section .rom.00000100, \"ax\"\n\t.global Func_08000100\nFunc_08000100:\n\t.incbin \"baserom.gba\", 0x00000100, 0x00000004\n";
        let tree = [
            ("games/tla/objects.tsv", objects),
            ("recon/tla/raw/08000100.s", ".syntax unified\n\t.thumb\n\t.global Func_08000100\n\t.thumb_func\nFunc_08000100:\n\tbx lr\n\t.2byte 0x0000\n"),
            ("recon/tla/en/main/08000100.c", "void Func_08000100(void) {}\n"),
            ("recon/tla/de/rom.s", de),
        ];
        for (path, text) in tree {
            std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            std::fs::write(root.join(path), text).unwrap();
        }
        let Ok(Request::Adopt(options)) = parse(&args(&["main", "08000100", "--editions", "en,de", "--game", "tla"])) else {
            panic!("main did not parse")
        };
        let mut files = Files::new(root);
        let report = adopt_main(&mut files, &Layout::new("tla"), "08000100", &options).unwrap();
        assert!(report.iter().any(|line| line.contains("de  stays on its scaffold")), "{report:?}");
        assert_eq!(files.read("recon/tla/de/rom.s").unwrap(), de);
        assert!(files.read("games/tla/objects.tsv").unwrap().contains("recon/tla/de/rom\t.rom.00000100\tde\n"));
    }

    /// With the module's size in an edition known, a section keeps the gap
    /// before its label and the code after the module's bytes, and the
    /// section after it is trimmed or goes when the module runs on into it.
    #[test]
    fn a_section_divides_at_the_module_s_size() {
        let text = "\t.section .rom.000000fe, \"ax\"\n\t.incbin \"baserom.gba\", 0x000000fe, 0x00000002\n\t.global F\n\t.thumb_func\nF:\n\t.incbin \"baserom.gba\", 0x00000100, 0x00000040\n\t.section .rom.00000140, \"ax\"\n\t.incbin \"baserom.gba\", 0x00000140, 0x00000004\n";
        let all: Vec<&str> = text.lines().collect();
        let every = sections(&all);
        let known: BTreeSet<String> = ["F".to_string()].into();
        let cut = cut_sized(&all, &every, every[0].clone(), 0, &known, 0x1e).unwrap();
        assert_eq!(cut.head, Some(2), "the gap before F stays in its section");
        assert_eq!(cut.tail, Some((0x11e, vec!["\t.incbin \"baserom.gba\", 0x0000011e, 0x00000022".to_string()])));
        assert!(cut.covered.is_empty());
        assert_eq!(cut.start, Some((0x100, "F".to_string())));
        let cut = cut_sized(&all, &every, every[0].clone(), 0, &known, 0x42).unwrap();
        assert_eq!(cut.tail, None);
        assert_eq!(cut.covered, vec![(every[1].clone(), Some((0x142, vec!["\t.incbin \"baserom.gba\", 0x00000142, 0x00000002".to_string()])))]);
        let cut = cut_sized(&all, &every, every[0].clone(), 0, &known, 0x44).unwrap();
        assert_eq!(cut.covered, vec![(every[1].clone(), None)]);
        assert!(cut_sized(&all, &every, every[0].clone(), 0, &known, 0x48).unwrap_err().contains("no section starts at 0x144"));
    }

    /// Three functions in one section, adopted one by one: each leaves the
    /// next its labels, and the last takes what is left.
    #[test]
    fn functions_sharing_a_section_are_cut_one_after_another() {
        let text = "\t.section .rom.00000100, \"ax\"\n\t.global F\nF:\n\t.incbin \"baserom.gba\", 0x00000100, 0x00000010\n\t.global G\nG:\n\t.incbin \"baserom.gba\", 0x00000110, 0x00000008\n\t.global H\nH:\n\t.incbin \"baserom.gba\", 0x00000118, 0x00000004\n";
        let all: Vec<&str> = text.lines().collect();
        let every = sections(&all);
        let cut = cut_sized(&all, &every, every[0].clone(), 0, &["F".to_string()].into(), 0x10).unwrap();
        let (offset, tail) = cut.tail.unwrap();
        assert_eq!((offset, cut.kept), (0x110, vec!["G".to_string(), "H".to_string()]));
        let rest = [format!("\t.section {}, \"ax\"", rom_section(offset))].into_iter().chain(tail).collect::<Vec<_>>();
        let all: Vec<&str> = rest.iter().map(String::as_str).collect();
        let every = sections(&all);
        let cut = cut_sized(&all, &every, every[0].clone(), 0, &["G".to_string()].into(), 0x8).unwrap();
        assert_eq!(cut.tail.as_ref().map(|(offset, _)| *offset), Some(0x118));
        assert!(cut_sized(&all, &every, every[0].clone(), 0, &["G".to_string()].into(), 0xc).unwrap_err().contains("H amid"));
        assert!(cut_sized(&all, &every, every[0].clone(), 0, &["G".to_string(), "H".to_string()].into(), 0x4).unwrap_err().contains("H past"));
    }

    #[test]
    fn rows_beside_each_other_merge_only_when_they_place_the_same() {
        let a = Row::new("main", "rom", "games/tla/src/x", ".text", &["ja"]);
        let b = Row::new("main", "rom", "games/tla/src/x", ".text", &["en", "de", "es", "fr", "it"]);
        assert_eq!(merge_rows(&a, &b).unwrap().fields.len(), 4, "all six editions write no column");
        assert!(merge_rows(&a, &a).is_none());
        assert!(merge_rows(&a, &Row::new("main", "rom", "games/tla/src/y", ".text", &["en"])).is_none());
    }

    /// Two overlay drafts adopted in one journal; the build blames the second,
    /// which goes back to its draft while the first stays adopted.
    #[test]
    fn one_item_backs_out_of_a_batch_and_the_rest_stay() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let script = "SECTIONS\n{\n    .text 0x2008000 :\n    {\n        \"*/games/tla/src/scene/islets/entry.o\"(.text)\n        \"*/resource_674_overlay.o\"(.text.x02008100)\n        \"*/resource_674_overlay.o\"(.text.x02008110)\n    }\n}\n";
        let scaffold = ".syntax unified\n\t.thumb\n\t.section .text.x02008100,\"ax\",%progbits\n\t.global Func_02000100\n\t.thumb_func\nFunc_02000100:\n\tbx lr\n\t.section .text.x02008110,\"ax\",%progbits\n\t.global Func_02000110\n\t.thumb_func\nFunc_02000110:\n\tbx lr\n";
        let tree = [
            ("recon/tla/raw/overlays/resource_674.ld", script),
            ("recon/tla/raw/overlays/resource_674_overlay.s", scaffold),
            ("recon/tla/en/overlays/resource_674_c_02000100.c", "void Func_02000100(void) {}\n"),
            ("recon/tla/en/overlays/resource_674_c_02000110.c", "void Func_02000110(void) {}\n"),
        ];
        for (path, text) in tree {
            std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            std::fs::write(root.join(path), text).unwrap();
        }
        let words = args(&["overlay", "recon/tla/en/overlays/resource_674_c_02000100.c", "recon/tla/en/overlays/resource_674_c_02000110.c", "--unmeasured"]);
        let Ok(Request::Adopt(options)) = parse(&words) else { panic!("overlay did not parse") };
        adopt(root, &options, &command_line(&words)).unwrap();
        let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
        assert!(read("games/tla/src/scene/islets/s0100.c").is_some() && read("games/tla/src/scene/islets/s0110.c").is_some());
        let journal = Path::new("out/adopt/0001-overlay-resource_674_c_02000100.journal");
        assert!(back_out(root, journal, &["Func_x.c".into()]).unwrap_err().contains("not among"));
        back_out(root, journal, &["resource_674_c_02000110.c".into()]).unwrap();
        assert!(read("games/tla/src/scene/islets/s0100.c").is_some() && read("games/tla/src/scene/islets/s0110.c").is_none());
        assert_eq!(read("recon/tla/en/overlays/resource_674_c_02000110.c").as_deref(), Some(tree[3].1));
        assert!(read("recon/tla/raw/overlays/resource_674_overlay.s").unwrap().contains("Func_02000110:"));
        // The journal written afresh, under the number the undone one freed.
        let second = std::fs::read_dir(root.join("out/adopt")).unwrap().flatten().map(|entry| entry.file_name()).collect::<Vec<_>>();
        assert_eq!(second, ["0001-overlay-resource_674_c_02000100.journal"]);
        let recorded = read("out/adopt/0001-overlay-resource_674_c_02000100.journal").unwrap();
        assert!(recorded.contains("command adopt overlay recon/tla/en/overlays/resource_674_c_02000100.c --unmeasured\n"), "{recorded}");
    }

    /// An overlay draft takes its section's link line; a replacement takes the
    /// Japanese form linked beside the file, a section it names nothing in.
    #[test]
    fn overlay_adoption_and_replacement_edit_the_script_and_scaffold() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let script = "SECTIONS\n{\n    .text 0x2008000 :\n    {\n        \"*/games/tla/src/scene/islets/entry.o\"(.text)\n        \"*/resource_674_overlay.o\"(.text.x02008100)\n        \"*/resource_674_overlay.o\"(.text.x02008120)\n        \"*/resource_674_overlay.o\"(.text.x02008124)\n        \"*/games/tla/src/scene/islets/s0130.o\"(.text)\n    }\n}\n";
        let scaffold = ".syntax unified\n\t.thumb\n\t.section .text.x02008100,\"ax\",%progbits\n\t.global Func_02000100\n\t.thumb_func\nFunc_02000100:\n\tbx lr\n\t.2byte 0x0000\n\t.section .text.x02008120,\"ax\",%progbits\n\t.ifdef TLA_EDITION_JA\n\t.global Func_02000120\nFunc_02000120:\n\t.4byte 0x47704770\n\t.endif\n\t.section .text.x02008124,\"ax\",%progbits\n\t.ifdef TLA_EDITION_JA\n\t.global Func_02000124\nFunc_02000124:\n\t.4byte 0x47704770\n\t.endif\n";
        let tree = [
            ("recon/tla/raw/overlays/resource_674.ld", script),
            ("recon/tla/raw/overlays/resource_674_overlay.s", scaffold),
            ("recon/tla/en/overlays/resource_674_c_02000100.c", "// Matches\nvoid Func_02000100(void) {}\n"),
            ("recon/tla/en/overlays/resource_674_c_02000124.c", "// Matches in every edition\nvoid Func_02000124(void) {}\n"),
            ("games/tla/src/scene/islets/s0130.c", "void Func_02000130(void) {}\n"),
        ];
        for (path, text) in tree {
            std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            std::fs::write(root.join(path), text).unwrap();
        }
        let read = |path: &str| std::fs::read_to_string(root.join(path)).unwrap();
        let mut files = Files::new(root);
        adopt_overlay(&mut files, "recon/tla/en/overlays/resource_674_c_02000100.c", false).unwrap();
        assert!(files.read("recon/tla/raw/overlays/resource_674.ld").unwrap().contains("        \"*/games/tla/src/scene/islets/s0100.o\"(.text)\n        \"*/resource_674_overlay.o\"(.text.x02008120)"));
        let edited = files.read("recon/tla/raw/overlays/resource_674_overlay.s").unwrap();
        assert!(edited.starts_with(".syntax unified\n\t.thumb\n\t.section .text.x02008120,\"ax\",%progbits\n\t.balign 4\n\t.ifdef"));
        assert!(files.exists("games/tla/src/scene/islets/s0100.c") && !files.exists("recon/tla/en/overlays/resource_674_c_02000100.c"));
        assert!(adopt_overlay(&mut Files::new(root), "recon/tla/en/overlays/resource_674_c_02000124.c", false).unwrap_err().contains("edition forms"));

        files = Files::new(root);
        adopt_replace(&mut files, "games/tla/src/scene/islets/s0130.c", "recon/tla/en/overlays/resource_674_c_02000124.c").unwrap();
        let script = files.read("recon/tla/raw/overlays/resource_674.ld").unwrap();
        assert!(script.contains("x02008100)\n        \"*/games/tla/src/scene/islets/s0130.o\"(.text)\n    }"));
        let scaffold = files.read("recon/tla/raw/overlays/resource_674_overlay.s").unwrap();
        assert!(scaffold.ends_with("\t.2byte 0x0000\n"), "{scaffold}");
        assert!(files.read("games/tla/src/scene/islets/s0130.c").unwrap().starts_with("void Func_02000124(void)"));
        assert_eq!(read("games/tla/src/scene/islets/s0130.c"), tree[4].1, "nothing is written before commit");
    }
}
