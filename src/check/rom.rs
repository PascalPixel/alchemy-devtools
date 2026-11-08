//! The built images against the reference ROMs: each overlay's packed stream
//! looked for in its edition's ROM, and each ROM compared byte for byte with
//! every difference named by what placed it.

use crate::alchemy::built;
use crate::alchemy::targets::{target_for, DecompTargetId};
use crate::gba::elf::Elf;
use crate::gba::ld_map::{Map, Placed, ROM};
use crate::progress::commas;
use std::collections::HashMap;
use std::path::Path;

/// How many differing ranges `check rom` lists in each edition.
const LISTED: usize = 40;
/// Differing bytes this close together are one range.
const JOIN: usize = 4;

/// An edition's reference ROM, checked against its approved digest.
fn reference(root: &Path, id: DecompTargetId) -> Result<Vec<u8>, String> {
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
    let id = match argument.rfind("resource_") {
        Some(at) => &argument[at + "resource_".len()..],
        None => argument,
    };
    let id: String = id.chars().take_while(char::is_ascii_hexdigit).collect();
    (!id.is_empty()).then(|| format!("resource_{}", id.to_ascii_lowercase()))
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
    for id in built::editions(&words)? {
        let Ok(streams) = built::overlay_files(root, id, "lz") else {
            println!("{id}: not built; make compare-{id}");
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
        (0, 0) => Ok(()),
        (0, unbuilt) => Err(format!("{unbuilt} overlays asked for are not built")),
        (missing, _) => Err(format!("{missing} of {checked} overlay streams are not in their reference ROMs")),
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
            .filter(|placed| placed.loaded && placed.size > 0 && (ROM..built::ROM_END).contains(&placed.load))
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
}

pub fn roms(arguments: &[String]) -> Result<(), String> {
    if let Some(flag) = arguments.iter().find(|argument| argument.starts_with('-')) {
        return Err(format!("unknown option {flag}"));
    }
    let root = crate::compile::root();
    let mut differing = Vec::new();
    for id in built::editions(arguments)? {
        let path = built::output(root, id).join(format!("{id}.gba"));
        let Ok(image) = std::fs::read(&path) else {
            println!("{id}: not built; make compare-{id}");
            continue;
        };
        let rom = target_for(id).rom;
        let ranges = differences(&image, &reference(root, id)?);
        if ranges.is_empty() {
            println!("{id}: identical to {rom}");
            continue;
        }
        differing.push(id.as_str());
        let bytes: usize = ranges.iter().map(|(start, end)| end - start).sum();
        println!("{id}: {} bytes in {} ranges differ from {rom}", commas(bytes as i64), ranges.len());
        let placement = Placement::read(root, id)?;
        for &(start, end) in ranges.iter().take(LISTED) {
            let at = ROM + start as u32;
            println!("  {at:#010x}..{:#010x} {:>9}  {}", ROM + end as u32, commas((end - start) as i64), placement.at(at));
        }
        if ranges.len() > LISTED {
            println!("  and {} ranges more", ranges.len() - LISTED);
        }
    }
    match differing.as_slice() {
        [] => Ok(()),
        [edition] => Err(format!("{edition} differs from its reference ROM")),
        editions => Err(format!("{} differ from their reference ROMs", editions.join(" "))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn a_byte_is_named_by_the_symbol_and_object_that_placed_it() {
        let map = Map::parse(crate::gba::ld_map::tests::MAP);
        let mut sections: Vec<Placed> = map.placed.into_iter().filter(|p| p.loaded && p.size > 0).collect();
        sections.sort_by_key(|placed| placed.load);
        let symbols = vec![(0x0300_0000, "IwramIrqMain".to_string()), (0x0800_0010, "Input_Handle".to_string()), (0x0800_0018, "Input_Poll".to_string())];
        let placement = Placement { edition: DecompTargetId::TlaEn, sections, symbols };
        assert_eq!(placement.at(0x0800_001a), "Input_Poll+0x2 in /x/build/obj/src/system/io.o");
        assert_eq!(placement.at(0x0800_0032), "IwramIrqMain+0x2 in /x/build/obj/src/system/iwram_routines.o");
        assert_eq!(placement.at(0x0800_001e), "the linker's fill");
        assert_eq!(placement.at(0x0800_0004), "/x/build/obj/asm/rom.o(.rom.00000000)");
    }
}
