//! What is left of a game, piece by piece, read from the tree and the built
//! objects without compiling anything: every English listing of the main
//! image, every overlay scaffold section English still assembles, and what
//! another edition still takes from a scaffold, each with the draft that is to
//! replace it.

use crate::alchemy::built;
use crate::alchemy::layout::{self, Layout};
use crate::alchemy::targets::{target_for, DecompTarget, DecompTargetId};
use crate::gba::elf::{self, Elf};
use regex::Regex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Main,
    Arm,
    Overlay,
    Edition,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Main => "main",
            Kind::Arm => "main-arm",
            Kind::Overlay => "overlay",
            Kind::Edition => "edition",
        }
    }

    /// The kind `--kind` names.
    pub fn named(word: &str) -> Option<Kind> {
        [Kind::Main, Kind::Arm, Kind::Overlay, Kind::Edition].into_iter().find(|kind| match kind {
            Kind::Arm => word == "arm",
            kind => kind.name() == word,
        })
    }
}

/// What a piece's draft is compared with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reference {
    /// English listings, measured together in this order.
    Listings(Vec<String>),
    /// The function of this overlay's scaffold object in the piece's edition.
    Overlay(String),
    /// A function the edition's main-image scaffold places, read from its
    /// reference ROM; `sized_by`, a built object that defines it, sizes the
    /// piece when no scaffold section names it.
    Scaffold { sized_by: Option<PathBuf> },
}

#[derive(Debug, Clone)]
pub(crate) struct Piece {
    pub kind: Kind,
    /// The edition the piece is measured in.
    pub target: DecompTarget,
    pub name: String,
    /// Where the piece is: its listing's address, its overlay and section,
    /// or its edition and scaffold section.
    pub place: String,
    pub draft: Option<String>,
    /// Where its draft is written when there is none yet.
    pub planned: Option<String>,
    pub reference: Reference,
    pub arm: bool,
    /// Why it cannot be measured, when that is known before compiling.
    pub unmeasurable: Option<String>,
}

/// Whether a function symbol of a relocatable object is ARM code.
fn is_arm(symbol: &elf::Symbol) -> bool {
    symbol.kind == 2 && symbol.value & 1 == 0
}

/// Every `.c` under a folder, with its `//` comments removed.
fn sources(folder: &Path) -> Vec<(PathBuf, String)> {
    let mut found: Vec<(PathBuf, String)> = walkdir::WalkDir::new(folder)
        .into_iter()
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "c"))
        .filter_map(|entry| {
            let text = std::fs::read_to_string(entry.path()).ok()?;
            let code: Vec<&str> = text.lines().map(|line| line.split("//").next().unwrap_or("")).collect();
            Some((entry.path().to_path_buf(), code.join("\n")))
        })
        .collect();
    found.sort();
    found
}

/// The first source that defines `name` as a function.
fn defining<'a>(sources: &'a [(PathBuf, String)], name: &str) -> Option<&'a Path> {
    let definition = Regex::new(&format!(r"\b{}\s*\([^;{{]*\)\s*\{{", regex::escape(name))).ok()?;
    sources.iter().find(|(_, code)| definition.is_match(code)).map(|(path, _)| path.as_path())
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned()
}

/// The listings a draft's header says to measure it with: the first run of
/// comma-separated addresses in its opening comment that names its own
/// (`0819284c,08192894,081929fc,08192a4c,08192c9c`), in the header's order.
pub(crate) fn measured_together(text: &str, address: &str) -> Option<Vec<String>> {
    let run = Regex::new(r"\b[0-9a-f]{8}(?:,[0-9a-f]{8})+\b").unwrap();
    for line in text.lines().map(str::trim).take_while(|line| line.is_empty() || line.starts_with("//")) {
        for found in run.find_iter(line) {
            let list: Vec<String> = found.as_str().split(',').map(str::to_string).collect();
            if list.iter().any(|named| named == address) {
                return Some(list);
            }
        }
    }
    None
}

/// One piece a listing, in address order, ARM by its `.arm` or `.code 32`.
/// A listing a draft's header measures with it is that draft's.
pub(crate) fn main(root: &Path, layout: &Layout, english: DecompTarget) -> Result<Vec<Piece>, String> {
    let folder = root.join(layout.listings());
    let entries = std::fs::read_dir(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let mut addresses: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.strip_suffix(".s").map(str::to_string))
        .filter(|stem| stem.len() == 8 && stem.chars().all(|c| c.is_ascii_hexdigit()))
        .collect();
    addresses.sort();
    let drafted = |address: &str| root.join(layout.main_draft(address)).is_file();
    let mut pieces = Vec::new();
    for address in &addresses {
        let text = built::read_text(&folder.join(format!("{address}.s")))?;
        let lines: Vec<&str> = text.lines().collect();
        let arm = lines.iter().any(|line| matches!(line.trim(), ".arm" | ".code 32"));
        let draft = layout.main_draft(address);
        let mut reference = vec![address.clone()];
        if drafted(address) {
            let together = measured_together(&built::read_text(&root.join(&draft))?, address).unwrap_or_default();
            let others: Vec<String> = together.into_iter().filter(|named| named == address || (addresses.contains(named) && !drafted(named))).collect();
            if others.len() > 1 {
                reference = others;
            }
        }
        pieces.push(Piece {
            kind: if arm { Kind::Arm } else { Kind::Main },
            target: english,
            name: layout::globals(&lines).into_iter().next().unwrap_or_else(|| address.clone()),
            place: address.clone(),
            draft: drafted(address).then(|| draft.clone()),
            planned: Some(draft),
            reference: Reference::Listings(reference),
            arm,
            unmeasurable: None,
        });
    }
    let taken: BTreeSet<String> = pieces
        .iter()
        .filter_map(|piece| match &piece.reference {
            Reference::Listings(list) if piece.draft.is_some() => Some(list.iter().filter(|named| **named != piece.place).cloned()),
            _ => None,
        })
        .flatten()
        .collect();
    pieces.retain(|piece| piece.draft.is_some() || !taken.contains(&piece.place));
    Ok(pieces)
}

/// A function section of an overlay's scaffold object: its address, bytes
/// and the function it defines.
struct Section {
    address: u32,
    bytes: Vec<u8>,
    function: Option<String>,
}

/// The `.text.x<address>` sections an overlay's scaffold object assembles.
fn scaffold_sections(path: &Path, target: DecompTarget) -> Result<Vec<Section>, String> {
    let data = std::fs::read(path).map_err(|error| format!("{}: {error}; build {} first", path.display(), target.id))?;
    let headers = elf::headers(&data).map_err(|error| format!("{}: {error}", path.display()))?;
    let symbols = Elf::parse(&data).map_err(|error| format!("{}: {error}", path.display()))?.symbols;
    let mut sections = Vec::new();
    for (index, header) in headers.iter().enumerate() {
        let Some(address) = header.name.strip_prefix(".text.x").and_then(|hex| u32::from_str_radix(hex, 16).ok()) else { continue };
        let Some(bytes) = data.get(header.offset..header.offset + header.size).filter(|bytes| !bytes.is_empty()) else { continue };
        let function = symbols
            .iter()
            .filter(|symbol| usize::from(symbol.section) == index && symbol.bind == 1 && symbol.is_function())
            .min_by_key(|symbol| symbol.value & !1)
            .map(|symbol| symbol.name.clone());
        // A section of zeros that names nothing pads the next to its alignment.
        if function.is_none() && bytes.iter().all(|&byte| byte == 0) {
            continue;
        }
        sections.push(Section { address, bytes: bytes.to_vec(), function });
    }
    sections.sort_by_key(|section| section.address);
    Ok(sections)
}

/// The functions the overlay scaffolds still hold, overlay by overlay: one
/// piece for each section English assembles, measured in English; and one
/// for each section another edition assembles to other bytes, or English
/// not at all, measured in that edition. `editions` lists English first.
pub(crate) fn overlays(root: &Path, layout: &Layout, editions: &[DecompTarget]) -> Result<Vec<Piece>, String> {
    let folder = root.join(layout.overlays());
    let entries = std::fs::read_dir(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let mut ids: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.strip_prefix("resource_")?.strip_suffix("_overlay.s").map(str::to_string))
        .collect();
    ids.sort();
    let drafts = sources(&root.join(layout.drafts("en", "overlays")));
    let mut pieces = Vec::new();
    for id in ids {
        let overlay = format!("resource_{id}");
        let assembled = editions
            .iter()
            .map(|&target| scaffold_sections(&crate::alchemy::overlay::file(&root.join(target.output_dir), &overlay, "_overlay.o"), target))
            .collect::<Result<Vec<_>, String>>()?;
        let Some((english, others)) = assembled.split_first() else { continue };
        // A draft of several functions names its overlay; the overlay's own
        // C serves a function another edition still takes from its scaffold.
        let ours: Vec<(PathBuf, String)> = drafts.iter().filter(|(path, _)| path.to_string_lossy().contains(overlay.as_str())).cloned().collect();
        let mut linked: Option<Vec<(PathBuf, String)>> = None;
        let mut draft_of = |section: &Section, name: &str, edition: bool| -> Option<String> {
            let named = layout.overlay_drafts(&id, section.address).into_iter().find(|path| root.join(path).is_file());
            let found = named.or_else(|| defining(&ours, name).map(|path| relative(root, path)));
            if found.is_some() || !edition {
                return found;
            }
            let linked = linked.get_or_insert_with(|| overlay_sources(root, layout, &id));
            defining(linked, name).map(|path| relative(root, path))
        };
        for (n, sections) in std::iter::once(english).chain(others).enumerate() {
            let target = editions[n];
            for section in sections {
                if n > 0 && english.iter().any(|ours| ours.address == section.address && ours.bytes == section.bytes) {
                    continue;
                }
                let name = section.function.clone().unwrap_or_else(|| format!("{:08x}", section.address));
                let draft = draft_of(section, &name, n > 0);
                let place = format!("{overlay} {:08x}", section.address);
                pieces.push(Piece {
                    kind: if n == 0 { Kind::Overlay } else { Kind::Edition },
                    target,
                    place: if n == 0 { place } else { format!("{} {place}", target.id) },
                    planned: Some(layout.overlay_drafts(&id, section.address)[0].clone()),
                    reference: Reference::Overlay(overlay.clone()),
                    arm: false,
                    unmeasurable: section.function.is_none().then(|| format!("{overlay} defines no function at {:08x}", section.address)),
                    name,
                    draft,
                });
            }
        }
    }
    Ok(pieces)
}

/// The C the overlay's link script names.
fn overlay_sources(root: &Path, layout: &Layout, id: &str) -> Vec<(PathBuf, String)> {
    let script = std::fs::read_to_string(root.join(layout.overlay_script(id))).unwrap_or_default();
    let mut found = Vec::new();
    for line in script.lines() {
        let Some(object) = line.trim().strip_prefix("\"*/").and_then(|rest| rest.split_once(".o\"")).map(|(stem, _)| stem) else { continue };
        let path = root.join(format!("{object}.c"));
        if let Ok(text) = std::fs::read_to_string(&path) {
            let code: Vec<&str> = text.lines().map(|line| line.split("//").next().unwrap_or("")).collect();
            found.push((path, code.join("\n")));
        }
    }
    found
}

/// The functions `editions` (crate::editions) finds an edition still taking
/// from its main-image scaffold while English links them as C: one piece a
/// function and edition, its draft the edition's own form when
/// `recon/<game>/<language>/main` holds one, else the C English links.
pub(crate) fn editions(root: &Path, game: &str, english: DecompTargetId, chosen: &[DecompTargetId]) -> Result<Vec<Piece>, String> {
    let layout = Layout::new(game);
    let (objects, _) = crate::editions::survey(root, game, chosen)?;
    let scanned: Vec<DecompTargetId> = chosen.iter().copied().filter(|&id| id != english && built::scaffold(root, id).is_some()).collect();
    let mut forms: Vec<(DecompTargetId, Vec<(PathBuf, String)>)> = Vec::new();
    let mut pieces = Vec::new();
    for object in &objects {
        // The object as some edition built it, English first: which names are functions, and in which instruction set.
        let built: Option<(PathBuf, Vec<elf::Symbol>)> = std::iter::once(english)
            .chain(chosen.iter().copied())
            .map(|id| built::object(root, id, &object.object))
            .find_map(|path| Some((path.clone(), Elf::parse(&std::fs::read(&path).ok()?).ok()?.symbols)));
        let Some((sized_by, symbols)) = built else { continue };
        let source = object.file.ends_with(".c").then(|| object.file.clone());
        for (name, places) in &object.names {
            let Some(symbol) = symbols.iter().find(|symbol| symbol.name == *name && symbol.is_function()) else { continue };
            let mut wanting: Vec<(DecompTargetId, Option<String>)> = places.iter().map(|held| (held.edition, Some(held.section.clone()))).collect();
            for &id in object.missing.iter().filter(|id| scanned.contains(id)) {
                if places.iter().all(|held| held.edition != id) {
                    wanting.push((id, None));
                }
            }
            for (id, section) in wanting {
                if !forms.iter().any(|(language, _)| *language == id) {
                    forms.push((id, sources(&root.join(layout.drafts(id.language(), "main")))));
                }
                let own = forms.iter().find(|(language, _)| *language == id).and_then(|(_, found)| defining(found, name));
                let draft = own.map(|path| relative(root, path)).or_else(|| source.clone());
                pieces.push(Piece {
                    kind: Kind::Edition,
                    target: target_for(id),
                    name: name.clone(),
                    place: format!("{id} {}", section.as_deref().unwrap_or("(no section names it)")),
                    planned: draft.clone(),
                    draft,
                    reference: Reference::Scaffold { sized_by: Some(sized_by.clone()) },
                    arm: is_arm(symbol),
                    unmeasurable: section.is_none().then(|| format!("no {id} scaffold section names {name}")),
                });
            }
        }
    }
    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_names_the_listings_its_draft_holds() {
        let iris = "// Differ 34 of 5364 (tla-en), measured with all five addresses below\n// Measure in address order,\n// 0819284c,08192894,081929fc,08192a4c,08192c9c: the parent alone gives 549\n#include \"types.h\"\n// 08000000,08000004 after the header is not read\n";
        assert_eq!(measured_together(iris, "08192c9c").unwrap(), ["0819284c", "08192894", "081929fc", "08192a4c", "08192c9c"]);
        assert_eq!(measured_together("// Measure 081843fc,081848ec together.\n", "081843fc").unwrap(), ["081843fc", "081848ec"]);
        // A run that does not name the draft's own address, or one after the header, is another subject.
        assert!(measured_together("// like 0802dd70,0802e000\nint x;\n// 081843fc,081848ec\n", "081843fc").is_none());
        assert!(measured_together("// 081843fc alone\n", "081843fc").is_none());
    }

    #[test]
    fn kinds_read_as_rank_writes_them() {
        assert_eq!(Kind::named("arm").map(Kind::name), Some("main-arm"));
        assert_eq!(Kind::named("edition"), Some(Kind::Edition));
        assert!(Kind::named("main-arm").is_none());
        let sources = vec![(PathBuf::from("a.c"), "static void Helper(void);\nvoid Helper(void)\n{\n}\n".to_string())];
        assert_eq!(defining(&sources, "Helper"), Some(Path::new("a.c")));
        assert_eq!(defining(&sources, "Help"), None);
    }

    /// The tree's own pieces: Iris's header takes its four nested listings,
    /// an overlay section no edition assembles is no piece, and an edition
    /// still on its scaffold is measured in that edition.
    #[test]
    fn the_checkout_lists_what_is_left() {
        let root = crate::compile::root();
        let Some(english) = built::edition("tla", "en").map(target_for) else { return };
        let layout = Layout::new("tla");
        if !root.join(layout.listings()).is_dir() || !root.join(english.output_dir).is_dir() {
            return;
        }
        let pieces = main(root, &layout, english).unwrap();
        if let Some(iris) = pieces.iter().find(|piece| piece.place == "08192c9c") {
            if let Reference::Listings(list) = &iris.reference {
                assert!(list.len() > 1 && list.last().map(String::as_str) == Some("08192c9c"), "{list:?}");
                assert!(list.iter().all(|address| !pieces.iter().any(|piece| piece.place == *address && piece.draft.is_none())));
            }
        }
        let editions: Vec<DecompTarget> = built::editions(&["tla".to_string()]).unwrap().into_iter().map(target_for).collect();
        let mut ordered = vec![english];
        ordered.extend(editions.into_iter().filter(|target| *target != english));
        let overlays = overlays(root, &layout, &ordered).unwrap();
        assert!(overlays.iter().all(|piece| piece.kind != Kind::Overlay || piece.target == english));
        // resource_686's 02008418 is kept for no edition: its function is C in s0418.c.
        assert!(!overlays.iter().any(|piece| piece.place.ends_with("resource_686 02008418")));
        assert!(overlays.iter().filter(|piece| piece.kind == Kind::Edition).all(|piece| piece.place.starts_with(piece.target.id.as_str())));
    }
}
