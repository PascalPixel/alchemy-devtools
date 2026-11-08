//! The built images against the reference ROMs: each overlay's packed stream
//! looked for in its edition's ROM, and each ROM compared byte for byte with
//! every difference named by what placed it.

use crate::alchemy::built;
use crate::alchemy::layout::rom_offset;
use crate::alchemy::done::commas;
use crate::alchemy::overlay;
use crate::alchemy::targets::{target_for, DecompTargetId};
use crate::gba::count::CARTRIDGE;
use crate::gba::elf::Elf;
use crate::gba::ld_map::{Map, Placed, ROM};
use std::collections::HashMap;
use std::path::Path;

/// How many differing ranges `check rom` lists in each edition.
const LISTED: usize = 40;
/// Differing bytes this close together are one range.
const JOIN: usize = 4;

/// An edition's reference ROM, checked against its approved digest.
pub(super) fn reference(root: &Path, id: DecompTargetId) -> Result<Vec<u8>, String> {
    let target = target_for(id);
    let rom = std::fs::read(root.join(target.rom)).map_err(|error| format!("{}: {error}", target.rom))?;
    target.verify_reference(&rom)?;
    Ok(rom)
}

/// Where each needle first occurs in `haystack`, found in one pass over it.
fn find_all(haystack: &[u8], needles: &[Vec<u8>]) -> Vec<Option<usize>> {
    let mut found = vec![None; needles.len()];
    let mut starts: HashMap<[u8; 4], Vec<usize>> = HashMap::new();
    // The first two bytes of every needle, so most offsets cost one lookup.
    let mut first = vec![false; 1 << 16];
    for (index, needle) in needles.iter().enumerate() {
        match needle.get(..4) {
            Some(&[a, b, c, d]) => {
                first[usize::from(u16::from_le_bytes([a, b]))] = true;
                starts.entry([a, b, c, d]).or_default().push(index);
            }
            _ => found[index] = haystack.windows(needle.len().max(1)).position(|window| needle.is_empty() || window == needle),
        }
    }
    for at in 0..haystack.len().saturating_sub(3) {
        if !first[usize::from(u16::from_le_bytes([haystack[at], haystack[at + 1]]))] {
            continue;
        }
        let key = [haystack[at], haystack[at + 1], haystack[at + 2], haystack[at + 3]];
        for &index in starts.get(&key).map_or(&[][..], Vec::as_slice) {
            if found[index].is_none() && haystack[at..].starts_with(&needles[index]) {
                found[index] = Some(at);
            }
        }
    }
    found
}

/// The overlay id an argument names: `64b`, `resource_64b`, or a path with
/// `resource_64b` in it.
fn overlay_id(argument: &str) -> Option<String> {
    overlay::named_in(argument).or_else(|| overlay::named_in(&format!("resource_{argument}")).filter(|_| argument.len() == 3))
}

pub fn overlays(arguments: &[String]) -> Result<(), String> {
    let (mut words, mut wanted) = (Vec::new(), Vec::new());
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        if argument == "--edition" {
            words.push(arguments.next().ok_or("--edition takes an edition or a game")?.clone());
        } else if argument.starts_with('-') {
            return Err(format!("unknown option {argument}"));
        } else {
            wanted.push(overlay_id(argument).ok_or_else(|| format!("{argument} names no overlay"))?);
        }
    }
    let root = crate::compile::root();
    let (mut checked, mut missing, mut unbuilt) = (0, 0, 0);
    let (editions, mut not_built) = (built::editions(&words)?, Vec::new());
    for &id in &editions {
        let Ok(streams) = built::overlay_files(root, id, "lz") else {
            println!("{id}: not built; make compare-{id}");
            not_built.push(id);
            continue;
        };
        let streams: Vec<_> = streams.into_iter().filter(|(name, _)| wanted.is_empty() || wanted.contains(name)).collect();
        for name in wanted.iter().filter(|name| !streams.iter().any(|(built, _)| built == *name)) {
            println!("{id}: {name} is not built");
            unbuilt += 1;
        }
        if streams.is_empty() {
            continue;
        }
        let packed = streams
            .iter()
            .map(|(_, path)| std::fs::read(path).map_err(|error| format!("{}: {error}", path.display())))
            .collect::<Result<Vec<_>, _>>()?;
        let found = find_all(&reference(root, id)?, &packed);
        let lost: Vec<&str> = streams.iter().zip(&found).filter(|(_, at)| at.is_none()).map(|((name, _), _)| name.as_str()).collect();
        checked += streams.len();
        missing += lost.len();
        let rom = target_for(id).rom;
        match lost.is_empty() {
            true => println!("{id}: {} overlays, each found in {rom}", streams.len()),
            false => println!("{id}: {} overlays; {} not found in {rom}: {}", streams.len(), lost.len(), lost.join(" ")),
        }
    }
    match (missing, unbuilt) {
        (0, 0) => checked_all(!words.is_empty(), editions.len(), &not_built),
        (0, unbuilt) => Err(format!("{unbuilt} overlays asked for are not built")),
        (missing, _) => Err(format!("{missing} of {checked} overlay streams are not in their reference ROMs")),
    }
}

/// Whether a check that found nothing wrong checked what it was asked to:
/// every edition named, or with none named at least one edition built.
pub(super) fn checked_all(named: bool, editions: usize, not_built: &[DecompTargetId]) -> Result<(), String> {
    let names = || not_built.iter().map(|id| id.as_str()).collect::<Vec<_>>().join(" ");
    match not_built.len() {
        0 => Ok(()),
        _ if named => Err(format!("not built, so not checked: {}", names())),
        count if count == editions => Err("no edition is built, so nothing was checked".into()),
        _ => Ok(()),
    }
}

/// The ranges where two images differ, as offsets: bytes closer than `JOIN`
/// form one range, and a longer image's tail is a range of its own.
fn differences(built: &[u8], reference: &[u8]) -> Vec<(usize, usize)> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for at in (0..built.len().min(reference.len())).filter(|&at| built[at] != reference[at]) {
        match ranges.last_mut() {
            Some(last) if at - last.1 < JOIN => last.1 = at + 1,
            _ => ranges.push((at, at + 1)),
        }
    }
    if built.len() != reference.len() {
        ranges.push((built.len().min(reference.len()), built.len().max(reference.len())));
    }
    ranges
}

/// What placed each byte of an edition's ROM: its main map's sections by
/// load address, and the symbols its ELF defines in them.
struct Placement {
    edition: DecompTargetId,
    sections: Vec<Placed>,
    /// (address, name), sorted
    symbols: Vec<(u32, String)>,
}

impl Placement {
    fn read(root: &Path, id: DecompTargetId) -> Result<Placement, String> {
        let output = built::output(root, id);
        let map = Map::parse(&built::read_text(&output.join(format!("{id}.map")))?);
        let mut sections: Vec<Placed> = map
            .placed
            .into_iter()
            .filter(|placed| placed.loaded && placed.size > 0 && CARTRIDGE.contains(&placed.load))
            .collect();
        sections.sort_by_key(|placed| placed.load);
        let path = output.join(format!("{id}.elf"));
        let data = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let elf = Elf::parse(&data).map_err(|error| format!("{}: {error}", path.display()))?;
        let mut symbols: Vec<(u32, String)> = elf
            .symbols
            .into_iter()
            .filter(|symbol| {
                symbol.kind <= 2
                    && symbol.section != 0
                    && symbol.section < 0xff00
                    && !symbol.name.is_empty()
                    && !symbol.name.starts_with(['$', '.'])
            })
            .map(|symbol| (if symbol.kind == 2 { symbol.value & !1 } else { symbol.value }, symbol.name))
            .collect();
        symbols.sort();
        Ok(Placement { edition: id, sections, symbols })
    }

    /// The symbol and object that placed the byte at a cartridge address.
    fn at(&self, address: u32) -> String {
        let after = self.sections.partition_point(|placed| placed.load <= address);
        let Some(placed) = after.checked_sub(1).map(|at| &self.sections[at]).filter(|placed| address < placed.load + placed.size)
        else {
            return "no section".to_string();
        };
        if placed.is_fill() {
            return "the linker's fill".to_string();
        }
        let label = match placed.name.as_str() {
            ".text" => built::stem(self.edition, &placed.path),
            name => format!("{}({name})", built::stem(self.edition, &placed.path)),
        };
        let vma = placed.address + (address - placed.load);
        let before = self.symbols.partition_point(|(value, _)| *value <= vma);
        match before.checked_sub(1).map(|at| &self.symbols[at]).filter(|(value, _)| *value >= placed.address) {
            Some((value, name)) if *value == vma => format!("{name} in {label}"),
            Some((value, name)) => format!("{name}+{:#x} in {label}", vma - value),
            None => label,
        }
    }

    /// The source stem of the object that placed the byte at an address.
    fn object(&self, address: u32) -> Option<String> {
        let after = self.sections.partition_point(|placed| placed.load <= address);
        let placed = after.checked_sub(1).map(|at| &self.sections[at]).filter(|placed| address < placed.load + placed.size)?;
        (!placed.is_fill()).then(|| built::stem(self.edition, &placed.path))
    }
}

/// Where a built edition first differs from its reference ROM, for adopt's
/// verifying, which blames what placed it: the first differing byte's label
/// and the source stem of its object (code that grows or shrinks moves every
/// byte after it, so only the first range names a culprit), and the overlays
/// whose packed stream the ROM lacks. None when the edition is identical.
pub(crate) struct Difference {
    pub(crate) label: String,
    pub(crate) object: Option<String>,
    pub(crate) overlays: Vec<String>,
}

pub(crate) fn first_difference(root: &Path, id: DecompTargetId) -> Result<Option<Difference>, String> {
    let path = built::output(root, id).join(format!("{id}.gba"));
    let image = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let rom = reference(root, id)?;
    let Some(&(start, _)) = differences(&image, &rom).first() else { return Ok(None) };
    let placement = Placement::read(root, id)?;
    let address = ROM + start as u32;
    let mut overlays = Vec::new();
    if let Ok(streams) = built::overlay_files(root, id, "lz") {
        let packed = streams.iter().map(|(_, path)| std::fs::read(path).unwrap_or_default()).collect::<Vec<_>>();
        for ((name, _), at) in streams.iter().zip(find_all(&rom, &packed)) {
            if at.is_none() {
                overlays.push(name.clone());
            }
        }
    }
    Ok(Some(Difference { label: placement.at(address), object: placement.object(address), overlays }))
}

/// Where an object places its bytes in the reference layout, as its name
/// says: a scaffold section `.rom.OFFSET`, an English listing
/// `recon/GAME/raw/ADDRESS`, or (in English) a module named for its address,
/// `fADDRESS` (`f0f94a4` for 0x080f94a4). None for anything else.
fn expected(id: DecompTargetId, piece: &Placed, stem: &str) -> Option<u32> {
    if let Some(offset) = rom_offset(&piece.name) {
        return Some(ROM + offset);
    }
    let name = stem.rsplit('/').next().unwrap_or(stem);
    let hex = |text: &str| u32::from_str_radix(text, 16).ok();
    if name.len() == 8 && name.starts_with("08") && stem.contains("/raw/") {
        return hex(name);
    }
    let address = name.strip_prefix('f').filter(|rest| rest.len() == 6 && id.language() == "en" && stem.starts_with("games/")).and_then(hex)?;
    Some(ROM + address)
}

/// How many objects the origin names between its anchors.
const ORIGIN_LISTED: usize = 12;

/// Where an edition's layout first moves: the first object named for its
/// place that is placed elsewhere, the last before it that is placed right,
/// and the objects from that one up to it, each marked when its bytes differ from the
/// reference there. Everything after a size change differs, so this, not
/// the first differing byte, is where to look when sizes changed. None
/// when every such object is in place.
fn origin(placement: &Placement, image: &[u8], reference: &[u8]) -> Option<Vec<String>> {
    let id = placement.edition;
    let pieces: Vec<&Placed> = placement.sections.iter().filter(|placed| !placed.is_fill()).collect();
    let mut last_right: Option<usize> = None;
    for (index, piece) in pieces.iter().enumerate() {
        let stem = built::stem(id, &piece.path);
        let Some(expected) = expected(id, piece, &stem) else { continue };
        if expected == piece.load {
            last_right = Some(index);
            continue;
        }
        let label = |piece: &Placed| match piece.name.as_str() {
            ".text" => built::stem(id, &piece.path),
            name => format!("{}({name})", built::stem(id, &piece.path)),
        };
        let shift = i64::from(piece.load) - i64::from(expected);
        let mut lines = vec![format!(
            "origin: {} is placed at {:#010x}, {} bytes {} its place; {}",
            label(piece),
            piece.load,
            commas(shift.abs()),
            if shift > 0 { "after" } else { "before" },
            match last_right {
                Some(at) => format!("the layout last holds at {} ({:#010x}). From there:", label(pieces[at]), pieces[at].load),
                None => "nothing before it is placed by name. Before it:".to_string(),
            }
        )];
        let between = &pieces[last_right.unwrap_or(0)..index];
        for piece in between.iter().take(ORIGIN_LISTED) {
            let at = (piece.load - ROM) as usize;
            let end = at + piece.size as usize;
            let differs = image.get(at..end) != reference.get(at..end);
            lines.push(format!(
                "  {:#010x} {:>7} {}{}",
                piece.load,
                format!("{:#x}", piece.size),
                label(piece),
                if differs { "  (differs)" } else { "" }
            ));
        }
        if between.len() > ORIGIN_LISTED {
            lines.push(format!("  and {} objects more", between.len() - ORIGIN_LISTED));
        }
        return Some(lines);
    }
    None
}

pub fn roms(arguments: &[String]) -> Result<(), String> {
    let mut words = Vec::new();
    let (mut all, mut origin_only) = (false, false);
    for argument in arguments {
        match argument.as_str() {
            "--all" => all = true,
            "--origin" => origin_only = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}")),
            word => words.push(word.to_string()),
        }
    }
    let root = crate::compile::root();
    let mut differing = Vec::new();
    let (editions, mut not_built) = (built::editions(&words)?, Vec::new());
    for &id in &editions {
        let path = built::output(root, id).join(format!("{id}.gba"));
        let Ok(image) = std::fs::read(&path) else {
            println!("{id}: not built; make compare-{id}");
            not_built.push(id);
            continue;
        };
        let rom = target_for(id).rom;
        let reference = reference(root, id)?;
        let ranges = differences(&image, &reference);
        if ranges.is_empty() {
            println!("{id}: identical to {rom}");
            continue;
        }
        differing.push(id.as_str());
        let bytes: usize = ranges.iter().map(|(start, end)| end - start).sum();
        println!("{id}: {} bytes in {} ranges differ from {rom}", commas(bytes as i64), ranges.len());
        let placement = Placement::read(root, id)?;
        match origin(&placement, &image, &reference) {
            Some(lines) => lines.iter().for_each(|line| println!("  {line}")),
            None => println!("  origin: every object named for its place is in place; the first range below is where to look"),
        }
        if origin_only {
            continue;
        }
        let listed = if all { ranges.len() } else { LISTED };
        for &(start, end) in ranges.iter().take(listed) {
            let at = ROM + start as u32;
            println!("  {at:#010x}..{:#010x} {:>9}  {}", ROM + end as u32, commas((end - start) as i64), placement.at(at));
        }
        if ranges.len() > listed {
            println!("  and {} ranges more (--all lists them)", ranges.len() - listed);
        }
    }
    match differing.as_slice() {
        [] => checked_all(!words.is_empty(), editions.len(), &not_built),
        [edition] => Err(format!("{edition} differs from its reference ROM")),
        editions => Err(format!("{} differ from their reference ROMs", editions.join(" "))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> DecompTargetId {
        crate::alchemy::targets::parse_decomp_target(name).unwrap()
    }

    #[test]
    fn needles_are_found_in_one_pass() {
        let haystack = b"..abcdefgh..abcdXYZ..xy";
        let needles = [b"abcdX".to_vec(), b"abcdefgh".to_vec(), b"xy".to_vec(), b"abcdQ".to_vec(), Vec::new()];
        assert_eq!(find_all(haystack, &needles), [Some(12), Some(2), Some(21), None, Some(0)]);
    }

    #[test]
    fn overlay_ids_come_from_any_naming() {
        assert_eq!(overlay_id("64B").as_deref(), Some("resource_64b"));
        assert_eq!(overlay_id("recon/tla/en/overlays/resource_6aa_c_0200263c.c").as_deref(), Some("resource_6aa"));
        assert_eq!(overlay_id("out/tla-en/overlays/resource_649.lz").as_deref(), Some("resource_649"));
        assert_eq!(overlay_id("tla-en"), None);
    }

    #[test]
    fn an_edition_named_or_every_edition_unbuilt_fails() {
        let unbuilt = [id("tla-ja")];
        assert!(checked_all(false, 6, &[]).is_ok());
        assert!(checked_all(false, 6, &unbuilt).is_ok(), "with none named, what is built is checked");
        assert!(checked_all(true, 6, &unbuilt).unwrap_err().contains("tla-ja"));
        assert!(checked_all(false, 1, &unbuilt).unwrap_err().contains("nothing was checked"));
    }

    #[test]
    fn differing_bytes_close_together_are_one_range() {
        let reference = [0u8; 32];
        let mut built = reference;
        built[2] = 1;
        built[4] = 1;
        built[20] = 1;
        assert_eq!(differences(&built, &reference), [(2, 5), (20, 21)]);
        assert_eq!(differences(&built[..30], &reference), [(2, 5), (20, 21), (30, 32)]);
        assert!(differences(&reference, &reference).is_empty());
    }

    /// A module 2 bytes long moves everything after it: the origin names the
    /// scaffold section it pushed, the last in place, and the module between.
    #[test]
    fn the_origin_is_the_first_object_placed_away_from_its_name() {
        let placed = |name: &str, path: &str, load: u32, size: u32| Placed {
            name: name.into(),
            path: path.into(),
            address: load,
            size,
            load,
            loaded: true,
            symbols: Vec::new(),
            output: None,
        };
        let de = id("tla-de");
        let sections = vec![
            placed(".rom.00000000", "out/tla-de/obj/recon/tla/de/rom.o", 0x0800_0000, 0x10),
            placed(".text", "out/tla-de/obj/games/tla/src/menu/f000010.o", 0x0800_0010, 0x12),
            placed(".rom.00000020", "out/tla-de/obj/recon/tla/de/rom.o", 0x0800_0022, 0x8),
        ];
        let placement = Placement { edition: de, sections, symbols: Vec::new() };
        let reference = vec![0u8; 0x2a];
        let mut image = reference.clone();
        image[0x20] = 1;
        let lines = origin(&placement, &image, &reference).unwrap();
        assert!(lines[0].starts_with("origin: recon/tla/de/rom(.rom.00000020) is placed at 0x08000022, 2 bytes after its place"), "{}", lines[0]);
        assert!(lines[0].contains("last holds at recon/tla/de/rom(.rom.00000000)"));
        assert_eq!(lines[1..], ["  0x08000000    0x10 recon/tla/de/rom(.rom.00000000)", "  0x08000010    0x12 games/tla/src/menu/f000010  (differs)"]);
        assert_eq!(expected(id("tla-en"), &placement.sections[1], "games/tla/src/menu/f0f94a4"), Some(0x080f_94a4));
        assert_eq!(expected(de, &placement.sections[1], "games/tla/src/menu/f0f94a4"), None, "modules are named by English addresses");
        assert_eq!(expected(id("tla-en"), &placement.sections[1], "recon/tla/raw/080f9644"), Some(0x080f_9644));
        image[0x20] = 0;
        let mut right = placement.sections.clone();
        right[2].load = 0x0800_0020;
        assert!(origin(&Placement { edition: de, sections: right, symbols: Vec::new() }, &image, &reference).is_none());
    }

    #[test]
    fn a_byte_is_named_by_the_symbol_and_object_that_placed_it() {
        let map = Map::parse(crate::gba::ld_map::tests::MAP);
        let mut sections: Vec<Placed> = map.placed.into_iter().filter(|p| p.loaded && p.size > 0).collect();
        sections.sort_by_key(|placed| placed.load);
        let symbols = vec![(0x0300_0000, "IwramIrqMain".to_string()), (0x0800_0010, "Input_Handle".to_string()), (0x0800_0018, "Input_Poll".to_string())];
        let placement = Placement { edition: id("tla-en"), sections, symbols };
        assert_eq!(placement.at(0x0800_001a), "Input_Poll+0x2 in /x/build/src/system/io.o");
        assert_eq!(placement.at(0x0800_0032), "IwramIrqMain+0x2 in /x/build/src/system/iwram_routines.o");
        assert_eq!(placement.at(0x0800_001e), "the linker's fill");
        assert_eq!(placement.at(0x0800_0004), "/x/build/asm/rom.o(.rom.00000000)");
    }
}
