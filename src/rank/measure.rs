//! Every piece measured as diff measures it, in this process: each draft
//! compiled once in each edition by the build's own recipe, on several
//! threads, then compared with its listing by diff's comparison and skeleton
//! gap. A listing nested in a neighbour's draft is found by measurement, and
//! overlay copies of one shape, or editions that pose one problem, are
//! counted as one family.

use super::inventory::{Kind, Piece, Reference};
use crate::alchemy::overlay;
use crate::alchemy::targets::{DecompTarget, DecompTargetId};
use crate::compile::root;
use crate::diff::{self, MainImage};
use crate::gba::process::run as command;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// A draft's comparison with its listing, as diff prints it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Measure {
    pub listing: usize,
    pub draft: usize,
    pub differ: usize,
    /// The skeleton gap: removed and added once registers, stack and pool
    /// offsets, branch distances and register lists are set aside.
    pub gap: usize,
}

pub(crate) fn compare(listing: &[String], draft: &[String]) -> Measure {
    let (common, _) = diff::compare_counts(listing, draft);
    let (removed, added) = diff::skeleton::gap(listing, draft);
    Measure { listing: listing.len(), draft: draft.len(), differ: listing.len() + draft.len() - 2 * common, gap: removed + added }
}

/// A piece measured: its size in the listing's instructions, as diff counts
/// them; the places of its family, its own first; and its comparison, None
/// when it has no draft.
pub(crate) struct Row {
    pub piece: Piece,
    pub size: usize,
    pub places: Vec<String>,
    pub measured: Option<Result<Measure, String>>,
}

/// What measuring found besides the rows.
#[derive(Debug, Default)]
pub(crate) struct Summary {
    pub compiled: usize,
    /// Listings with no instructions and no draft (a function's padding),
    /// left out of the rows, by place.
    pub padding: Vec<String>,
    /// Each listing found nested in a neighbour's draft, and that draft's address.
    pub nested: Vec<(String, String)>,
}

/// `work` on every item, `jobs` at a time, in the items' order.
pub(crate) fn parallel<T: Sync, R: Send>(items: &[T], jobs: usize, work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let results: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..jobs.clamp(1, items.len().max(1)) {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(item) = items.get(index) else { break };
                *results[index].lock().unwrap() = Some(work(item));
            });
        }
    });
    results.into_iter().map(|result| result.into_inner().unwrap().expect("every item ran")).collect()
}

/// A draft compiled for one edition, by one route.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Job {
    draft: String,
    edition: &'static str,
    overlay: Option<String>,
    arm: bool,
}

struct Compiled {
    object: PathBuf,
    /// The whole object's instructions, as diff reads a draft compared with listings.
    whole: Vec<String>,
    defined: Vec<String>,
}

/// The folders the drafts compile in, one an edition under its build's
/// `obj/drafts`, removed when measuring ends.
struct Folders(Vec<PathBuf>);

impl Drop for Folders {
    fn drop(&mut self) {
        for folder in &self.0 {
            let _ = std::fs::remove_dir_all(folder);
        }
    }
}

fn job_of(piece: &Piece) -> Option<Job> {
    let draft = piece.draft.clone().filter(|_| piece.unmeasurable.is_none())?;
    let overlay = match &piece.reference {
        Reference::Overlay(overlay) => Some(overlay.clone()),
        _ => None,
    };
    Some(Job { draft, edition: piece.target.id.as_str(), overlay, arm: piece.arm })
}

/// Where job `index` compiles its draft: its object in the edition's folder.
fn object_of(job: &Job, folder: &Path, index: usize) -> PathBuf {
    let stem = Path::new(&job.draft).file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
    folder.join(format!("{index}-{stem}.o"))
}

/// Every job's compile steps, as diff compiles its draft, from one `make -n`
/// an edition and route rather than one a draft.
fn steps_of(work: &[Job], target: impl Fn(&str) -> DecompTarget) -> Vec<Result<Vec<Vec<String>>, String>> {
    let mut steps: Vec<Option<Result<Vec<Vec<String>>, String>>> = vec![None; work.len()];
    let editions: BTreeSet<&str> = work.iter().map(|job| job.edition).collect();
    for edition in editions {
        let members: Vec<usize> = (0..work.len()).filter(|&index| work[index].edition == edition).collect();
        let paths: Vec<(PathBuf, PathBuf)> =
            members.iter().map(|&index| (root().join(&work[index].draft), object_of(&work[index], &folder(target(edition)), index))).collect();
        let drafts: Vec<(&Path, &Path, Option<&str>, bool)> = members
            .iter()
            .zip(&paths)
            .map(|(&index, (draft, object))| (draft.as_path(), object.as_path(), work[index].overlay.as_deref(), work[index].arm))
            .collect();
        for (&index, found) in members.iter().zip(diff::steps_many(target(edition), &drafts)) {
            steps[index] = Some(found);
        }
    }
    steps.into_iter().map(|found| found.unwrap_or_else(|| Err("no steps".into()))).collect()
}

fn compile(job: &Job, steps: &Result<Vec<Vec<String>>, String>, folder: &Path, index: usize) -> Result<Compiled, String> {
    let draft = root().join(&job.draft);
    let text = std::fs::read_to_string(&draft).map_err(|error| format!("{}: {error}", job.draft))?;
    if crate::permute::perm::has_macros(&text) {
        return Err("the draft uses PERM macros".into());
    }
    let object = object_of(job, folder, index);
    for step in steps.as_ref().map_err(Clone::clone)? {
        command(step, folder)?;
    }
    Ok(Compiled { whole: diff::disassemble(&object, None)?, defined: diff::text_symbols(&object)?, object })
}

/// An error's line that says most, as a state shows it: a compiler's first
/// diagnostic that is not a warning or the context before one, else the
/// first line; paths from the checkout.
fn reason(error: &str) -> String {
    let error = error.replace(&format!("{}/", root().display()), "");
    let context = |line: &&str| line.contains("In function") || line.contains("In file included") || line.contains("warning:") || line.trim_start().starts_with("from ");
    let line = error.lines().find(|line| !context(line)).or_else(|| error.lines().next()).unwrap_or("").trim();
    line.chars().take(90).collect()
}

/// A listing's instructions, and for an overlay's, the family it belongs to:
/// the same instructions calling and reading the same names and constants.
struct Listed {
    rows: Result<Vec<String>, String>,
    size: usize,
    shape: Option<String>,
}

/// An overlay function's shape: diff's rows, then the names each relocation
/// reaches (without offsets, which move with the overlay) and the value of
/// every pool word no relocation fills.
fn shape(objdump: &str) -> String {
    let mut relocated = BTreeSet::new();
    let mut names = Vec::new();
    for line in objdump.lines() {
        let Some((address, rest)) = line.trim_start().split_once(": R_") else { continue };
        relocated.extend(u32::from_str_radix(address, 16).ok());
        let target = rest.split_whitespace().nth(1).unwrap_or("");
        names.push(target.split(['+', '-']).next().unwrap_or("").to_string());
    }
    let words = objdump.lines().filter_map(|line| {
        let (address, rest) = line.trim_start().split_once(":\t")?;
        let at = u32::from_str_radix(address, 16).ok()?;
        let word = rest.split_once(".word")?.1.trim();
        (!relocated.contains(&at)).then(|| word.to_string())
    });
    let mut key = diff::normalize(objdump);
    key.extend(names.into_iter().map(|name| format!("@{name}")));
    key.extend(words.map(|word| format!("={word}")));
    key.join("\n")
}

fn listed(piece: &Piece, address_rows: &HashMap<String, Result<Vec<String>, String>>, images: &HashMap<DecompTargetId, Result<MainImage, String>>) -> Listed {
    let sized = |rows: Result<Vec<String>, String>, fallback: usize| {
        let size = rows.as_ref().map_or(fallback, Vec::len);
        Listed { rows, size, shape: None }
    };
    match &piece.reference {
        Reference::Listings(list) => {
            let mut rows = Vec::new();
            for address in list {
                match &address_rows[address] {
                    Ok(found) => rows.extend(found.iter().cloned()),
                    Err(error) => return sized(Err(error.clone()), 0),
                }
            }
            sized(Ok(rows), 0)
        }
        Reference::Overlay(overlay) => {
            let object = overlay::file(&root().join(piece.target.output_dir), overlay, "_overlay.o");
            match diff::disassemble_text(&object, Some(&piece.name)) {
                Ok(text) => Listed { shape: Some(shape(&text)), ..sized(Ok(diff::normalize(&text)), 0) },
                Err(error) => sized(Err(error), 0),
            }
        }
        Reference::Scaffold { sized_by } => {
            let own = || sized_by.as_ref().and_then(|object| diff::disassemble(object, Some(&piece.name)).ok()).map_or(0, |rows| rows.len());
            if let Some(why) = &piece.unmeasurable {
                return sized(Err(why.clone()), own());
            }
            let rows = match &images[&piece.target.id] {
                Ok(image) => diff::scaffold_rows(image, &piece.name, piece.arm).map(|(rows, ..)| rows),
                Err(error) => Err(error.clone()),
            };
            let rows = rows.and_then(|rows| if rows.is_empty() { Err("the selected reference function contains no instructions".into()) } else { Ok(rows) });
            let fallback = if rows.is_err() { own() } else { 0 };
            sized(rows, fallback)
        }
    }
}

/// rank.ts's rule for a listing of 8 instructions or more with no draft of
/// its own: it is nested in a drafted neighbour within five listings when
/// adding it to that draft's comparison brings the listing's and the draft's
/// instruction counts at least 70% of its size closer. That is how 081848ec
/// was found inside Catastrophe's draft, 081843fc. Nearer neighbours are
/// tried first, the one after before the one before. Returns, for each
/// piece, whether a neighbour's draft took it in.
fn nest(pieces: &mut [Piece], sizes: &mut [usize], address_rows: &HashMap<String, Result<Vec<String>, String>>, whole: &HashMap<usize, usize>, nested: &mut Vec<(String, String)>) -> Vec<bool> {
    let count = |address: &String| address_rows.get(address).and_then(|rows| rows.as_ref().ok()).map_or(0, Vec::len);
    let mut order: Vec<String> = pieces
        .iter()
        .filter_map(|piece| match &piece.reference {
            Reference::Listings(list) => Some(list.iter().cloned()),
            _ => None,
        })
        .flatten()
        .collect();
    order.sort();
    let mut gone = vec![false; pieces.len()];
    for at in 0..order.len() {
        let Some(index) = pieces.iter().position(|piece| matches!(piece.kind, Kind::Main | Kind::Arm) && piece.place == order[at] && piece.draft.is_none()) else { continue };
        let size = sizes[index];
        if size < 8 || gone[index] {
            continue;
        }
        for step in [1, -1, 2, -2, 3, -3, 4, -4, 5, -5] {
            let Some(neighbour) = at.checked_add_signed(step).and_then(|n| order.get(n)) else { continue };
            let Some(parent) = pieces.iter().position(|piece| piece.place == *neighbour && piece.draft.is_some()) else { continue };
            let (Some(&draft), Reference::Listings(list)) = (whole.get(&parent), &pieces[parent].reference) else { continue };
            let alone: usize = list.iter().map(count).sum();
            let apart = |listing: usize| (listing as f64 - draft as f64).abs();
            if apart(alone + size) < apart(alone) - size as f64 * 0.7 {
                let mut list = list.clone();
                let sorted = list.windows(2).all(|pair| pair[0] <= pair[1]);
                list.push(order[at].clone());
                if sorted {
                    list.sort();
                }
                pieces[parent].reference = Reference::Listings(list);
                sizes[parent] += size;
                gone[index] = true;
                nested.push((order[at].clone(), neighbour.clone()));
                break;
            }
        }
    }
    gone
}

/// `items` without those `gone` marks.
fn without<T>(items: Vec<T>, gone: &[bool]) -> Vec<T> {
    items.into_iter().zip(gone).filter(|(_, gone)| !**gone).map(|(item, _)| item).collect()
}

/// The folder an edition's drafts compile in for this run.
fn folder(target: DecompTarget) -> PathBuf {
    root().join(target.output_dir).join("obj/drafts").join(format!("rank-{}", std::process::id()))
}

/// Measures every piece: `editions` are the targets pieces name.
pub(crate) fn measure(mut pieces: Vec<Piece>, editions: &[DecompTarget], jobs: usize) -> Result<(Vec<Row>, Summary), String> {
    let target = |id: &str| editions.iter().copied().find(|target| target.id.as_str() == id).expect("a piece's edition is listed");
    let mut summary = Summary::default();

    // The listings: each English listing object once, and each edition's main image once.
    let addresses: Vec<String> = pieces
        .iter()
        .filter_map(|piece| match &piece.reference {
            Reference::Listings(list) => Some(list.iter().cloned()),
            _ => None,
        })
        .flatten()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let english = pieces.iter().find(|piece| matches!(piece.reference, Reference::Listings(_))).map(|piece| piece.target);
    let address_rows: HashMap<String, Result<Vec<String>, String>> = match english {
        Some(english) => {
            let found = parallel(&addresses, jobs, |address| {
                let object = root().join(english.output_dir).join(format!("obj/recon/{}/raw/{address}.o", english.game()));
                diff::disassemble(&object, None)
            });
            addresses.iter().cloned().zip(found).collect()
        }
        None => HashMap::new(),
    };
    let wanted: Vec<&str> = pieces
        .iter()
        .filter(|piece| matches!(piece.reference, Reference::Scaffold { .. }) && piece.unmeasurable.is_none())
        .map(|piece| piece.target.id.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let images: HashMap<DecompTargetId, Result<MainImage, String>> =
        wanted.iter().map(|&id| target(id).id).zip(parallel(&wanted, jobs, |&id| MainImage::load(target(id)))).collect();
    let listings: Vec<Listed> = parallel(&pieces, jobs, |piece| listed(piece, &address_rows, &images));
    drop(images);

    // Every draft once in each edition and route it is measured by.
    let work: Vec<Job> = pieces.iter().filter_map(job_of).collect::<BTreeSet<_>>().into_iter().collect();
    let mut folders = Folders(Vec::new());
    for job in &work {
        let folder = folder(target(job.edition));
        if !folders.0.contains(&folder) {
            // What a killed rank left goes first; a live one's stays.
            if let Some(drafts) = folder.parent() {
                crate::gba::process::sweep(drafts, "rank-");
            }
            let _ = std::fs::remove_dir_all(&folder);
            std::fs::create_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
            folders.0.push(folder);
        }
    }
    let steps = steps_of(&work, |edition| target(edition));
    let indexed: Vec<(usize, &Job)> = work.iter().enumerate().collect();
    let results = parallel(&indexed, jobs, |&(index, job)| compile(job, &steps[index], &folder(target(job.edition)), index));
    let compiled: HashMap<Job, Result<Compiled, String>> = work.into_iter().zip(results).collect();
    summary.compiled = compiled.len();

    // Listings nested in a neighbour's draft, which is then measured with them.
    let mut sizes: Vec<usize> = listings.iter().map(|listed| listed.size).collect();
    let whole: HashMap<usize, usize> = pieces
        .iter()
        .enumerate()
        .filter_map(|(index, piece)| Some((index, compiled.get(&job_of(piece)?)?.as_ref().ok()?.whole.len())))
        .collect();
    let gone = nest(&mut pieces, &mut sizes, &address_rows, &whole, &mut summary.nested);
    let (pieces, sizes) = (without(pieces, &gone), without(sizes, &gone));
    let listings: Vec<Listed> = without(listings, &gone)
        .into_iter()
        .zip(&pieces)
        .map(|(found, piece)| match &piece.reference {
            Reference::Listings(list) if list.len() > 1 => listed(piece, &address_rows, &HashMap::new()),
            _ => found,
        })
        .collect();

    // Overlay copies of one shape are one family: its draft is the first copy's that has one.
    let mut families: Vec<(Option<String>, Vec<usize>)> = Vec::new();
    for (index, piece) in pieces.iter().enumerate() {
        let shape = listings[index].shape.clone().filter(|_| piece.kind == Kind::Overlay);
        match families.iter_mut().find(|(key, _)| key.is_some() && *key == shape) {
            Some((_, members)) => members.push(index),
            None => families.push((shape, vec![index])),
        }
    }

    // Each family's comparison, from its first drafted copy.
    let leaders: Vec<usize> = families
        .iter()
        .map(|(_, members)| members.iter().copied().find(|&index| pieces[index].draft.is_some()).unwrap_or(members[0]))
        .collect();
    let measured: Vec<Option<(Result<Measure, String>, u64)>> = parallel(&leaders, jobs, |&index| {
        let piece = &pieces[index];
        piece.draft.as_ref()?;
        let fingerprint = |listing: &[String], draft: &[String]| {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            (listing, draft).hash(&mut hasher);
            hasher.finish()
        };
        let result = (|| {
            if let Some(why) = &piece.unmeasurable {
                return Err(why.clone());
            }
            let listing = listings[index].rows.as_ref().map_err(Clone::clone)?;
            let compiled = compiled[&job_of(piece).expect("a drafted piece compiles")].as_ref().map_err(Clone::clone)?;
            let draft = match &piece.reference {
                Reference::Listings(_) => compiled.whole.clone(),
                Reference::Overlay(_) if !compiled.defined.contains(&piece.name) => compiled.whole.clone(),
                // diff would compare the whole object; a file of many functions says nothing that way.
                Reference::Scaffold { .. } if !compiled.defined.contains(&piece.name) => {
                    return Err(format!("{} defines no {} in {}", piece.draft.as_deref().unwrap_or(""), piece.name, piece.target.id));
                }
                _ => diff::disassemble(&compiled.object, Some(&piece.name))?,
            };
            if listing.is_empty() {
                return Err("the selected reference function contains no instructions".into());
            }
            Ok((compare(listing, &draft), fingerprint(listing, &draft)))
        })();
        Some(match result {
            Ok((measure, fingerprint)) => (Ok(measure), fingerprint),
            Err(error) => (Err(reason(&error)), 0),
        })
    });

    let mut rows: Vec<Row> = Vec::new();
    let mut groups: HashMap<(String, Option<String>, u64), usize> = HashMap::new();
    for (((_, members), &leader), outcome) in families.iter().zip(&leaders).zip(measured) {
        let piece = &pieces[leader];
        if matches!(piece.kind, Kind::Main | Kind::Arm) && sizes[leader] == 0 && piece.draft.is_none() {
            summary.padding.push(piece.place.clone());
            continue;
        }
        let mut places = vec![piece.place.clone()];
        places.extend(members.iter().filter(|&&index| index != leader).map(|&index| pieces[index].place.clone()));
        let (measured, fingerprint) = match outcome {
            Some((result, fingerprint)) => (Some(result), fingerprint),
            None => (None, 0),
        };
        // Editions that pose one problem, instruction for instruction, are one family.
        if piece.kind == Kind::Edition && measured.as_ref().is_some_and(|result| result.is_ok()) {
            let key = (piece.name.clone(), piece.draft.clone(), fingerprint);
            if let Some(&at) = groups.get(&key) {
                rows[at].places.extend(places);
                continue;
            }
            groups.insert(key, rows.len());
        }
        rows.push(Row { piece: piece.clone(), size: sizes[leader], places, measured });
    }
    Ok((rows, summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alchemy::targets::decomp_target;

    fn rows(text: &str) -> Vec<String> {
        text.split(';').map(str::to_string).collect()
    }

    #[test]
    fn a_comparison_counts_as_diff_prints_it() {
        let listing = rows("push {r4, lr};movs r4, r0;bl;pop {r4, pc}");
        assert_eq!(compare(&listing, &listing), Measure { listing: 4, draft: 4, differ: 0, gap: 0 });
        let renamed = rows("push {r5, lr};movs r5, r0;bl;pop {r5, pc}");
        assert_eq!(compare(&listing, &renamed), Measure { listing: 4, draft: 4, differ: 6, gap: 0 });
        let longer = rows("push {r4, lr};movs r4, r0;bl;bl;pop {r4, pc}");
        assert_eq!(compare(&listing, &longer), Measure { listing: 4, draft: 5, differ: 1, gap: 1 });
    }

    #[test]
    fn parallel_keeps_the_items_order() {
        let items: Vec<usize> = (0..100).collect();
        assert_eq!(parallel(&items, 7, |n| n * 2), items.iter().map(|n| n * 2).collect::<Vec<_>>());
        assert!(parallel(&Vec::<usize>::new(), 4, |n| *n).is_empty());
    }

    #[test]
    fn a_shape_reads_names_and_constants_not_places() {
        let at = |base: u32, callee: &str, constant: &str| {
            format!(
                "{:x}:\tb500      \tpush\t{{lr}}\n{:x}:\tf7ff fffe \tbl\t0 <{callee}>\n\t\t\t{:x}: R_ARM_THM_CALL\t{callee}\n{:x}:\tbd00      \tpop\t{{pc}}\n{:x}:\t{constant} \t.word\t0x{constant}\n{:x}:\t00000000 \t.word\t0x00000000\n\t\t\t{:x}: R_ARM_ABS32\t.bss+0x{:x}\n",
                base, base + 2, base + 2, base + 6, base + 8, base + 12, base + 12, base & 0xff
            )
        };
        assert_eq!(shape(&at(0x100, "Sprite_Load", "12345678")), shape(&at(0x8a4, "Sprite_Load", "12345678")));
        assert_ne!(shape(&at(0x100, "Sprite_Load", "12345678")), shape(&at(0x100, "Sprite_Free", "12345678")));
        assert_ne!(shape(&at(0x100, "Sprite_Load", "12345678")), shape(&at(0x100, "Sprite_Load", "00000010")));
    }

    fn piece(place: &str, draft: bool) -> Piece {
        Piece {
            kind: Kind::Main,
            target: decomp_target(Some("tla-en")).unwrap(),
            name: format!("Func_{place}"),
            place: place.into(),
            draft: draft.then(|| format!("recon/tla/en/main/{place}.c")),
            planned: None,
            reference: Reference::Listings(vec![place.into()]),
            arm: false,
            unmeasurable: None,
        }
    }

    /// Catastrophe: its draft compiles to 4,580 instructions, its listing has
    /// 546 and the next listing, which has no draft, 4,034.
    #[test]
    fn a_listing_its_neighbours_draft_holds_is_nested_in_it() {
        let mut pieces = vec![piece("081843f0", false), piece("081843fc", true), piece("081848ec", false), piece("08186000", false)];
        let mut sizes = vec![4, 546, 4034, 300];
        let address_rows: HashMap<String, Result<Vec<String>, String>> =
            pieces.iter().zip(&sizes).map(|(piece, &size)| (piece.place.clone(), Ok(vec![String::new(); size]))).collect();
        let whole = HashMap::from([(1, 4580)]);
        let mut nested = Vec::new();
        let gone = nest(&mut pieces, &mut sizes, &address_rows, &whole, &mut nested);
        assert_eq!(nested, [("081848ec".to_string(), "081843fc".to_string())]);
        assert_eq!(gone, [false, false, true, false]);
        assert_eq!(pieces[1].reference, Reference::Listings(vec!["081843fc".into(), "081848ec".into()]));
        assert_eq!(without(sizes, &gone), [4, 4580, 300]);
        // A draft whose count is already the listing's takes nothing more.
        let mut pieces = vec![piece("08000000", true), piece("08000100", false)];
        let mut sizes = vec![100, 50];
        let address_rows = pieces.iter().zip(&sizes).map(|(piece, &size)| (piece.place.clone(), Ok(vec![String::new(); size]))).collect();
        assert_eq!(nest(&mut pieces, &mut sizes, &address_rows, &HashMap::from([(0, 104)]), &mut Vec::new()), [false, false]);
    }

    /// With no header to name them, Catastrophe's draft still takes in
    /// 081848ec and Iris's its four nested listings, by measurement alone.
    #[test]
    fn the_checkout_nests_by_measurement_alone() {
        let root = root();
        let Some(english) = crate::alchemy::built::edition("tla", "en").map(crate::alchemy::targets::target_for) else { return };
        let layout = crate::alchemy::layout::Layout::new("tla");
        let built = |address: &str| root.join(english.output_dir).join(format!("obj/recon/tla/raw/{address}.o")).is_file();
        if !["081843fc", "08192c9c"].iter().all(|address| root.join(layout.main_draft(address)).is_file() && built(address)) || !built("081848ec") {
            return;
        }
        let mut alone: Vec<Piece> = Vec::new();
        for piece in super::super::inventory::main(root, &layout, english).unwrap() {
            let Reference::Listings(list) = &piece.reference else { continue };
            for address in list.iter().filter(|address| ("08184000".."08193000").contains(&address.as_str())) {
                let mut own = piece.clone();
                if *address != piece.place {
                    (own.place, own.draft) = (address.clone(), None);
                }
                own.reference = Reference::Listings(vec![address.clone()]);
                alone.push(own);
            }
        }
        alone.sort_by(|a, b| a.place.cmp(&b.place));
        let (rows, summary) = measure(alone, &[english], 4).unwrap();
        let nested = |listing: &str, draft: &str| summary.nested.contains(&(listing.to_string(), draft.to_string()));
        assert!(nested("081848ec", "081843fc"), "{:?}", summary.nested);
        for listing in ["0819284c", "08192894", "081929fc", "08192a4c"] {
            assert!(nested(listing, "08192c9c"), "{listing}: {:?}", summary.nested);
        }
        let catastrophe = rows.iter().find(|row| row.piece.place == "081843fc").unwrap();
        assert!(catastrophe.measured.as_ref().unwrap().as_ref().unwrap().differ < 500);
    }
}
