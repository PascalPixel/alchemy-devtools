//! adopt: moves matched drafts into the build in every edition, and keeps a
//! journal that undoes it. It reads no ROM: the build judges what it did
//! (`make compare-editions`), and `check` traces a failure to its function.

use crate::alchemy::layout::{
    aligns, edition_only, ends_with_padding, globals, has_edition_forms, overlay_draft, overlay_section, pieces,
    rom_offset, rom_section, sections, Layout, ObjectList, Row, Section, LANGUAGES,
};
use crate::compile::root;
use crate::gba::ld_map::Map;
use crate::gba::clex::{lex, Kind, Token};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const USAGE: &str = r#"usage: alchemy-devtools adopt main ADDRESS[,ADDRESS...]... [--editions LIST] [--game GAME] [--dry-run]
       alchemy-devtools adopt overlay DRAFT.c... [--edition-forms] [--dry-run]
       alchemy-devtools adopt edition OBJECT... --editions LIST [--dry-run]
       alchemy-devtools adopt replace FILE.c DRAFT.c [--dry-run]
       alchemy-devtools adopt --undo JOURNAL
main: the English draft recon/GAME/en/main/ADDRESS.c becomes
games/GAME/src/FOLDER/fADDRESS.c, FOLDER being the nearest object's in the
object list (not gamepak or sound). Further addresses name listings the
draft also replaces, nested functions or labels: the draft must define the
first listing's names only. The module takes the earliest listing's row and
the others' rows go. In every other edition of LIST (default: all six) the
rom.s section whose .global names the function goes; its row becomes the
module, joining the module's row unless a row that edition links lies
between. A section that also holds another function keeps it as a section
of its own when the two divide at its label, else the edition stays on its
scaffold. Code whose listing ends with a halfword's padding makes what
follows it align itself.
overlay: each draft (resource_ID_c_ADDRESS.c or resource_ID/ADDRESS.c)
becomes s<offset>.c in the scene or debug folder its link script names;
every scaffold section it defines goes, and the first one's link line
places the module. Refused: more asm than FAKEMATCH tags, PERM_ macros, a
section with edition forms (unless --edition-forms: the draft covers each
edition's form), a draft defining none of its section's names (unless it
includes a .c file).
edition: an adopted object (games/..., with or without its extension) is
linked in each edition of LIST in place of the rom.s section naming it.
replace: an overlay draft that matches in every edition replaces FILE, an
adopted scene file whose Japanese form stayed on the scaffold; the scaffold
sections the draft defines, and the edition-only sections linked beside
FILE, go with their link lines.
Drafts lose their leading // header and are clang-formatted. --dry-run
prints what would change. Otherwise the changes are written with a journal
under out/adopt/ that --undo reverses exactly, provided nothing it wrote
has changed since. One adopt or undo runs at a time (out/adopt/lock), and
none writes over a file that changed after it read it. LIST: en,de,es,fr,it,ja, or edition names (tla-en)."#;

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
    dry_run: bool,
}

enum Request {
    Adopt(Options),
    Undo(PathBuf),
}

fn parse(arguments: &[String]) -> Result<Request, String> {
    let mut form = None;
    let mut items = Vec::new();
    let mut editions = None;
    let mut game = None;
    let mut edition_forms = false;
    let mut dry_run = false;
    let mut i = 0;
    while i < arguments.len() {
        let value = arguments[i].as_str();
        match value {
            "--dry-run" => dry_run = true,
            "--edition-forms" => edition_forms = true,
            "--undo" => {
                let journal = arguments.get(i + 1).filter(|_| arguments.len() == 2 && i == 0);
                return journal.map(|j| Request::Undo(PathBuf::from(j))).ok_or_else(|| USAGE.to_string());
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
        || (game.is_some() && form != Form::Main);
    if !fits || misplaced {
        return Err(USAGE.into());
    }
    if form == Form::Replace {
        // One replacement: FILE and the draft that replaces it.
        items = vec![items.join(" ")];
    }
    Ok(Request::Adopt(Options { form, items, editions, game, edition_forms, dry_run }))
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
    match parse(arguments)? {
        Request::Undo(journal) => undo(root(), &journal),
        Request::Adopt(options) => adopt(root(), &options, &format!("adopt {}", arguments.join(" "))),
    }
}

fn adopt(root: &Path, options: &Options, command: &str) -> Result<(), String> {
    // Held from the first read to the last write, so no other adoption
    // edits the files this one reads; a dry run writes nothing.
    let _lock = if options.dry_run { None } else { Some(Lock::take(root)?) };
    let mut files = Files::new(root);
    let mut adopted = 0;
    let mut refused = 0;
    for item in &options.items {
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
        return Ok(());
    }
    let journal = files.commit(command, &journal_name(options))?;
    println!("{adopted} adopted, {refused} refused; journal {journal}");
    println!("judge with make compare-editions; undo with alchemy-devtools adopt --undo {journal}");
    Ok(())
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
        match relink(files, layout, &mut list, language, &module, &names, &known, Some(halfword)) {
            Ok(done) => report.push(format!("  {language}  {done}")),
            Err(why) => report.push(format!("  {language}  stays on its scaffold: {why}")),
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

/// What goes from one rom.s section: all of it, or the stretch from the
/// module's first label up to the next function's label, the rest staying.
struct Cut {
    section: Section,
    row: usize,
    /// The line the module's part starts at, when lines before it stay.
    head: Option<usize>,
    /// The line and ROM offset of what stays after it, as a section of its own.
    tail: Option<(usize, u32)>,
    kept: Vec<String>,
}

/// Links `module` in `language` in place of the edition's rom.s sections
/// that name it. On Err the edition is left as it was.
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
) -> Result<String, String> {
    if list.rows.iter().any(|row| row.object() == module && row.links(language)) {
        return Err("it already links the module".into());
    }
    let scaffold = layout.scaffold(language);
    let path = format!("{scaffold}.s");
    let text = files.read(&path)?;
    let (all, newline) = lines(&text);
    let mut cuts = Vec::new();
    for section in sections(&all).into_iter().filter(|s| s.name.starts_with(".rom.")) {
        if !globals(&all[section.start..section.end]).iter().any(|name| names.contains(name)) {
            continue;
        }
        let row = list
            .find(&scaffold, &section.name, language)
            .ok_or_else(|| format!("{} has no row in the object list", section.name))?;
        cuts.push(cut(&all, section, row, known)?);
    }
    if cuts.is_empty() {
        let mut why = format!("no section of {path} names {}", names.join(" "));
        if let Some(section) = unlabelled_beside(list, &all, module, &scaffold, language) {
            why.push_str(&format!(" ({section} beside the module's row has no label: label it, then adopt edition)"));
        }
        return Err(why);
    }
    cuts.sort_by_key(|cut| cut.row);
    let rows: Vec<usize> = cuts.iter().map(|cut| cut.row).collect();
    let divided = cuts.iter().any(|cut| cut.head.is_some() || cut.tail.is_some());
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

    // Join the module's row when nothing this edition links lies between.
    let mut blocked = None;
    let joined = if divided {
        None
    } else {
        let mut candidates: Vec<usize> = (0..list.rows.len())
            .filter(|&r| list.rows[r].object() == module && list.rows[r].place() == place && !list.rows[r].links(language))
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
    let module_row = if let Some(r) = joined {
        edited.rows[r] = edited.rows[r].with_edition(language);
        for &row in rows.iter().rev() {
            edited.rows.remove(row);
        }
        r - rows.iter().filter(|&&row| row < r).count()
    } else {
        let cut = &cuts[0];
        let mut replacement = Vec::new();
        if cut.head.is_some() {
            replacement.push(edited.rows[at].clone());
        }
        replacement.push(Row::new(&image, &place, module, ".text", &[language]));
        if let Some((_, offset)) = cut.tail {
            replacement.push(Row::new(&image, &place, &scaffold, &rom_section(offset), &[language]));
        }
        for &row in rows[1..].iter().rev() {
            edited.rows.remove(row);
        }
        edited.rows.splice(at..at + 1, replacement);
        at + usize::from(cut.head.is_some())
    };

    // The scaffold without what goes.
    let mut out: Vec<String> = all.iter().map(|line| line.to_string()).collect();
    let mut by_line: Vec<&Cut> = cuts.iter().collect();
    by_line.sort_by_key(|cut| cut.section.start);
    for cut in by_line.iter().rev() {
        let s = &cut.section;
        let mut keep: Vec<String> = Vec::new();
        if let Some(start) = cut.head {
            keep.extend(all[s.start..start].iter().map(|l| l.to_string()));
        }
        if let Some((start, offset)) = cut.tail {
            keep.push(format!("\t.section {}, \"ax\"", rom_section(offset)));
            keep.extend(all[start..s.end].iter().map(|l| l.to_string()));
        }
        out.splice(s.start..s.end, keep);
    }
    files.write(&path, join_lines(&out, newline));
    let mut done = match &cuts[..] {
        [cut] if divided => format!(
            "{} divides at the module's labels: {} stay{}",
            cut.section.name,
            join(&cut.kept),
            cut.tail.map(|(_, offset)| format!(", from {} on", rom_section(offset))).unwrap_or_default()
        ),
        [cut] => format!("{} goes", cut.section.name),
        _ => format!("{} go", join(&cuts.iter().map(|cut| &cut.section.name).collect::<Vec<_>>())),
    };
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

/// Which of a section's pieces go with the module.
fn cut(all: &[&str], section: Section, row: usize, known: &BTreeSet<String>) -> Result<Cut, String> {
    let pieces = pieces(all, &section);
    let theirs = |names: &[String]| !names.is_empty() && names.iter().all(|name| !known.contains(name));
    let ours = |names: &[String]| !names.is_empty() && names.iter().all(|name| known.contains(name));
    if pieces.iter().all(|piece| piece.names.is_empty() || ours(&piece.names)) {
        return Ok(Cut { section, row, head: None, tail: None, kept: Vec::new() });
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
        Some(piece) => Some((piece.start, piece.offset.ok_or_else(|| format!("{} has a label with no bytes", section.name))?)),
        None => None,
    };
    let kept = pieces.iter().filter(|piece| theirs(&piece.names)).flat_map(|piece| piece.names.clone()).collect();
    Ok(Cut { section, row, head: (first > 0).then_some(pieces[first].start), tail, kept })
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
        match relink(files, &layout, &mut list, language, object, &names, &defined, halfword) {
            Ok(done) => {
                linked += 1;
                report.push(format!("  {language}  {done}"));
            }
            Err(why) => report.push(format!("  {language}  stays on its scaffold: {why}")),
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
    let placed = format!("{object}.o");
    let linking = LANGUAGES.iter().filter(|language| list.rows.iter().any(|row| row.object() == object && row.links(language)));
    linking.into_iter().find_map(|language| {
        let edition = format!("{}-{language}", layout.game);
        let text = std::fs::read_to_string(root.join(format!("out/{edition}/{edition}.map"))).ok()?;
        let map = Map::parse(&text);
        map.placed.iter().find(|p| p.name == ".text" && p.object() == placed).map(|p| p.size % 4 == 2)
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

const JOURNAL_HEADER: &str = "alchemy-devtools adopt journal 1";

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
            journal.extend(format!("now {}\n", fingerprint(now.as_deref())).bytes());
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
        for folder in &folders {
            std::fs::create_dir_all(self.root.join(folder)).map_err(|error| format!("cannot create {folder}: {error}"))?;
        }
        for (path, _, now) in &changed {
            let full = self.root.join(path);
            match now {
                Some(text) => std::fs::write(&full, text).map_err(|error| format!("cannot write {path}: {error}"))?,
                None => std::fs::remove_file(&full).map_err(|error| format!("cannot remove {path}: {error}"))?,
            }
        }
        Ok(journal_path)
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

struct Entry {
    path: String,
    was: Option<String>,
    now: String,
}

fn read_journal(bytes: &[u8]) -> Result<(Vec<String>, Vec<Entry>), String> {
    let mut at = 0;
    let line = |at: &mut usize| -> Option<String> {
        let end = bytes[*at..].iter().position(|&b| b == b'\n')? + *at;
        let text = String::from_utf8_lossy(&bytes[*at..end]).into_owned();
        *at = end + 1;
        Some(text)
    };
    if line(&mut at).as_deref() != Some(JOURNAL_HEADER) {
        return Err("not an adopt journal".into());
    }
    let broken = || "the journal is cut short".to_string();
    let mut folders = Vec::new();
    let mut entries = Vec::new();
    while let Some(next) = line(&mut at) {
        if next.starts_with("command ") {
            continue;
        }
        if let Some(folder) = next.strip_prefix("folder ") {
            folders.push(folder.to_string());
            continue;
        }
        let path = next.strip_prefix("file ").ok_or_else(broken)?.to_string();
        let was = match line(&mut at).as_deref().and_then(|l| l.strip_prefix("was ")).ok_or_else(broken)? {
            "absent" => None,
            length => {
                let length: usize = length.parse().map_err(|_| broken())?;
                let text = bytes.get(at..at + length).ok_or_else(broken)?;
                at += length + 1;
                Some(String::from_utf8(text.to_vec()).map_err(|_| broken())?)
            }
        };
        let now = line(&mut at).and_then(|l| l.strip_prefix("now ").map(str::to_string)).ok_or_else(broken)?;
        entries.push(Entry { path, was, now });
    }
    Ok((folders, entries))
}

/// Puts back what a journal's adoption changed, refusing when any file it
/// wrote has changed since.
fn undo(root: &Path, journal: &Path) -> Result<(), String> {
    let _lock = Lock::take(root)?;
    let journal_path = [std::env::current_dir().map(|d| d.join(journal)).ok(), Some(root.join(journal))]
        .into_iter()
        .flatten()
        .find(|path| path.is_file())
        .ok_or_else(|| format!("cannot read {}", journal.display()))?;
    let bytes = std::fs::read(&journal_path).map_err(|error| format!("cannot read {}: {error}", journal.display()))?;
    let (folders, entries) = read_journal(&bytes)?;
    let changed: Vec<&str> = entries
        .iter()
        .filter(|entry| fingerprint(std::fs::read_to_string(root.join(&entry.path)).ok().as_deref()) != entry.now)
        .map(|entry| entry.path.as_str())
        .collect();
    if !changed.is_empty() {
        return Err(format!("changed since the adoption, so nothing was undone:\n  {}", changed.join("\n  ")));
    }
    for entry in &entries {
        let full = root.join(&entry.path);
        match &entry.was {
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
    println!("undone: {} files restored from {}", entries.len(), journal.display());
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
        assert!(matches!(parse(&args(&["--undo", "j"])), Ok(Request::Undo(_))));
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
        assert!(undo(root, Path::new(&journal)).is_err());
        std::fs::write(root.join("kept.txt"), "after\n").unwrap();
        undo(root, Path::new(&journal)).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("kept.txt")).unwrap(), "before\n");
        assert_eq!(std::fs::read_to_string(root.join("gone.txt")).unwrap(), "old");
        assert!(!root.join("new").exists());
        assert!(!root.join(&journal).exists());
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
    /// the next function, which stays as a section of its own; the code ends
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
        let Ok(Request::Adopt(options)) = parse(&args(&["main", "08000100", "--editions", "en,de,ja", "--game", "tla"])) else {
            panic!("main did not parse")
        };
        adopt(root, &options, "adopt test").unwrap();
        let read = |path: &str| std::fs::read_to_string(root.join(path)).unwrap();
        assert_eq!(
            read("games/tla/objects.tsv"),
            "image\tplace\tobject\tsections\teditions\nmain\trom\tgames/tla/src/field/a\t.text\nmain\trom\tgames/tla/src/field/f000100\t.text\ten de\nmain\trom\tgames/tla/src/field/f000100\t.text\tja\nmain\trom\trecon/tla/ja/rom\t.rom.00000104\tja\nmain\trom\trecon/tla/raw/08000120\t.text\ten\nmain\trom\trecon/tla/de/rom\t.rom.00000120\tde\nmain\trom\tgames/tla/src/field/b\t.text\n"
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
