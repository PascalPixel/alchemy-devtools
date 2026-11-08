//! Camelot's overlays: `resource_XXX`, linked by the build into
//! `out/<edition>/overlays/`, and in the ROM a packed stream the resource
//! directory points at, unpacked here as the game's loader unpacks it.

use crate::gba::ld_map::{Map, ROM};
use std::path::{Path, PathBuf};

/// The overlay a path names (`resource_` and three hex digits), in lower case.
pub(crate) fn named_in(path: &str) -> Option<String> {
    let at = path.find("resource_")?;
    let id = path.get(at + 9..at + 12)?;
    id.chars().all(|c| c.is_ascii_hexdigit()).then(|| format!("resource_{}", id.to_ascii_lowercase()))
}

/// The address a draft is named after: its file stem, or the stem's last
/// `_`-separated part (`resource_XXX_c_ADDRESS`), when that is 8 hex digits.
pub(crate) fn draft_address(stem: &str) -> Option<String> {
    let tail = stem.rsplit('_').next()?;
    (tail.len() == 8 && tail.chars().all(|c| c.is_ascii_hexdigit())).then(|| tail.to_ascii_lowercase())
}

/// Whether `name` is named after `address` (`Func_0200263c`).
pub(crate) fn named_after(name: &str, address: Option<&str>) -> bool {
    address.is_some_and(|address| name.to_ascii_lowercase().ends_with(address))
}

/// The overlay function an overlay draft is compared with: the one of the
/// overlay's functions (`ours`) the draft also defines; among several, or
/// none, the one named after the draft's address.
pub(crate) fn function(ours: &[String], defined: &[String], stem: &str) -> Option<String> {
    let address = draft_address(stem);
    let mut shared: Vec<&String> = ours.iter().filter(|name| defined.contains(name)).collect();
    if shared.len() != 1 {
        let among = if shared.is_empty() { ours.iter().collect() } else { shared };
        shared = among.into_iter().filter(|name| named_after(name, address.as_deref())).collect();
    }
    match shared.as_slice() {
        [one] => Some(one.to_string()),
        _ => None,
    }
}

/// The overlay's own build file: `suffix` is `.map`, `.elf` or `_overlay.o`
/// (its scaffold's object).
pub(crate) fn file(output: &Path, overlay: &str, suffix: &str) -> PathBuf {
    output.join("overlays").join(format!("{overlay}{suffix}"))
}

/// A C source the overlay links and the main image does not, as the edition's
/// object list (`objects.mk`) names them: its recipe is the overlay's.
pub(crate) fn linked_source(root: &Path, output: &Path, overlay: &str) -> Result<String, String> {
    let path = output.join("objects.mk");
    let text = std::fs::read_to_string(root.join(&path)).map_err(|error| format!("{}: {error}", path.display()))?;
    let list = |name: &str| -> Vec<String> {
        let mut objects = Vec::new();
        let mut inside = false;
        for line in text.lines() {
            if !inside {
                inside = line.split_once(":=").is_some_and(|(variable, _)| variable.trim() == name);
            } else if !line.starts_with('\t') {
                break;
            }
            if inside {
                objects.extend(line.split_whitespace().filter_map(|word| word.strip_prefix("$(BUILD)/obj/")).map(str::to_string));
            }
        }
        objects
    };
    let id = overlay.strip_prefix("resource_").unwrap_or(overlay);
    let main = list("MAIN_OBJS");
    list(&format!("OVERLAY_OBJS_{id}"))
        .into_iter()
        .filter(|object| !main.contains(object))
        .filter_map(|object| object.strip_suffix(".o").map(|base| format!("{base}.c")))
        .find(|source| root.join(source).is_file())
        .ok_or_else(|| format!("{}: {overlay} links no C of its own", path.display()))
}

/// The overlay's code as the game loads it: its stream in the reference ROM,
/// found through the main image's `Resource_DirectoryTable`, unpacked, and
/// its calls restored.
pub(crate) fn unpacked(rom: &[u8], main_map: &Map, overlay: &str) -> Result<Vec<u8>, String> {
    let index = usize::from_str_radix(overlay.strip_prefix("resource_").ok_or("invalid overlay id")?, 16)
        .map_err(|_| "invalid overlay id")?;
    let (section, address) = main_map.symbol("Resource_DirectoryTable").ok_or("the main map has no Resource_DirectoryTable")?;
    if !section.loaded || section.load < ROM {
        return Err("resource directory has no ROM placement".into());
    }
    let table = section
        .load
        .checked_sub(ROM)
        .and_then(|offset| offset.checked_add(address - section.address))
        .ok_or("resource directory offset overflows")? as usize;
    let word = |at: usize| {
        rom.get(at..at + 4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .ok_or("resource directory is truncated")
    };
    if word(table + 4)? != address {
        return Err("reference resource directory disagrees with the main map".into());
    }
    let table_end = word(table + 8)?.checked_sub(ROM).ok_or("resource directory end is outside the ROM")? as usize;
    if table_end <= table + 12 || table_end > rom.len() || !(table_end - table).is_multiple_of(4) {
        return Err("reference resource directory has invalid bounds".into());
    }
    let count = (table_end - table) / 4;
    if index >= count {
        return Err(format!("{overlay} is outside the reference resource directory"));
    }
    let start = word(table + 4 * index)?.checked_sub(ROM).ok_or("overlay stream is outside the ROM")? as usize;
    if start >= rom.len() {
        return Err("overlay stream is outside the ROM".into());
    }
    let end = (0..count)
        .filter_map(|index| word(table + 4 * index).ok()?.checked_sub(ROM).map(|offset| offset as usize))
        .filter(|&offset| offset > start && offset <= rom.len())
        .min()
        .unwrap_or(rom.len());
    let decoded = match rom[start] {
        0 => crate::alchemy::lz::decode_general(rom, start, end, 0x100000),
        1 => crate::alchemy::lz::decode_palette(rom, start + 1, end, 0x100000),
        kind => return Err(format!("overlay stream has unsupported kind {kind}")),
    };
    let (mut decoded, _) = decoded?;
    restore_thumb_calls(&mut decoded);
    Ok(decoded)
}

/// The packer stores each `bl`'s displacement plus the offset of its second
/// halfword in the overlay; the loader takes that offset off again.
fn restore_thumb_calls(block: &mut [u8]) {
    let half = |block: &[u8], at: usize| u32::from(u16::from_le_bytes([block[at], block[at + 1]]));
    let mut low_at = 2;
    while low_at + 2 <= block.len() {
        let (low, high) = (half(block, low_at), half(block, low_at - 2));
        if low >> 11 == 31 && high >> 11 == 30 {
            let stored = ((high & 0x7ff) << 12) | ((low & 0x7ff) << 1);
            let call = stored.wrapping_sub(low_at as u32) & 0x7f_fffe;
            block[low_at - 2..low_at].copy_from_slice(&((0xf000 | (call >> 12)) as u16).to_le_bytes());
            block[low_at..low_at + 2].copy_from_slice(&((0xf800 | ((call >> 1) & 0x7ff)) as u16).to_le_bytes());
        }
        low_at += 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gba::ld_map::Placed;

    fn halves(words: &[u16]) -> Vec<u8> {
        words.iter().flat_map(|h| h.to_le_bytes()).collect()
    }

    #[test]
    fn the_overlay_comes_from_the_path() {
        assert_eq!(named_in("recon/tbs/en/overlays/resource_383_c_020048c8.c").as_deref(), Some("resource_383"));
        assert_eq!(named_in("recon/tbs/en/overlays/resource_3cd/02008768.c").as_deref(), Some("resource_3cd"));
        assert_eq!(named_in("recon/tbs/en/main/08011bf4.c"), None);
        assert_eq!(named_in("x/resource_3c"), None);
        assert_eq!(named_in("resource_6AA_c_0200263c.c").as_deref(), Some("resource_6aa"));
        assert_eq!(draft_address("resource_6aa_c_0200263C").as_deref(), Some("0200263c"));
        assert_eq!(draft_address("080cbc0c").as_deref(), Some("080cbc0c"));
        assert_eq!(draft_address("draft_link"), None);
    }

    #[test]
    fn a_draft_is_compared_with_the_function_it_shares_or_is_named_after() {
        let names = |items: &[&str]| items.iter().map(|item| item.to_string()).collect::<Vec<_>>();
        let ours = names(&["Func_02000100", "Func_0200263c", "Scene_Init"]);
        let stem = "resource_6aa_c_0200263c";
        // The one the draft defines, whatever its name.
        assert_eq!(function(&ours, &names(&["Scene_Init", "Helper"]), stem).as_deref(), Some("Scene_Init"));
        // None, or several: the one named after the address.
        assert_eq!(function(&ours, &names(&["Helper"]), stem).as_deref(), Some("Func_0200263c"));
        assert_eq!(function(&ours, &names(&["Func_02000100", "Func_0200263c"]), stem).as_deref(), Some("Func_0200263c"));
        assert_eq!(function(&ours, &names(&["Func_02000100", "Scene_Init"]), stem), None);
        assert_eq!(function(&ours, &names(&["Helper"]), "draft"), None);
    }

    #[test]
    fn an_overlay_takes_the_route_of_c_it_alone_links() {
        let work = tempfile::tempdir().unwrap();
        let root = work.path();
        for source in ["src/shared.c", "src/scene/a.c", "src/scene/b.c"] {
            std::fs::create_dir_all(root.join(source).parent().unwrap()).unwrap();
            std::fs::write(root.join(source), "").unwrap();
        }
        std::fs::create_dir_all(root.join("out/x")).unwrap();
        std::fs::write(root.join("out/x/objects.mk"), "MAIN_OBJS := \\\n\t$(BUILD)/obj/src/shared.o \\\n\nOVERLAY_OBJS_6aa := \\\n\t$(BUILD)/obj/src/shared.o \\\n\t$(BUILD)/obj/src/scene/data.o \\\n\t$(BUILD)/obj/src/scene/a.o \\\n\nOVERLAY_OBJS_6ab := \\\n\t$(BUILD)/obj/src/shared.o \\\n").unwrap();
        assert_eq!(linked_source(root, Path::new("out/x"), "resource_6aa").unwrap(), "src/scene/a.c");
        assert!(linked_source(root, Path::new("out/x"), "resource_6ab").is_err());
    }

    #[test]
    fn loader_restoration_inverts_stored_calls_including_backwards_and_wrap() {
        for displacement in [0, 2, 0x1234, 0x3ffffe, 0x400000, 0x7ffffe] {
            let original = halves(&[0x2001, (0xf000 | (displacement >> 12)) as u16, (0xf800 | ((displacement >> 1) & 0x7ff)) as u16, 0x4770]);
            let stored = (displacement + 4) & 0x7ffffe;
            let mut packed = halves(&[0x2001, (0xf000 | (stored >> 12)) as u16, (0xf800 | ((stored >> 1) & 0x7ff)) as u16, 0x4770]);
            restore_thumb_calls(&mut packed);
            assert_eq!(packed, original);
        }
        let mut unrelated = halves(&[0x2001, 0xf800, 0xf000]);
        unrelated.push(0xab);
        let original = unrelated.clone();
        restore_thumb_calls(&mut unrelated);
        assert_eq!(unrelated, original);
    }

    #[test]
    fn unpacking_uses_directory_bounds_with_duplicate_and_unsorted_rows() {
        let map = Map {
            placed: vec![Placed {
                name: ".rodata".into(),
                path: "games/tbs/directory.o".into(),
                address: ROM + 0x20,
                size: 24,
                load: ROM + 0x20,
                loaded: true,
                symbols: vec![(ROM + 0x20, "Resource_DirectoryTable".into())],
                output: None,
            }],
            outputs: Vec::new(),
        };
        let payload = halves(&[0x2001, 0x4770, 0x2002, 0x4770]);
        // The payload packed once as a general and as a palette stream by the encoders
        // that alchemy-devtools once carried, kept as bytes so this test needs no encoder.
        let packed = [0x00, 0x03, 0x82, 0x84, 0x7b, 0x54, 0x40, 0x87, 0x0f, 0x00];
        let palette = [0x01, 0x06, 0x01, 0x20, 0x70, 0x47, 0x02, 0x02, 0x04, 0x00, 0x00];
        let mut rom = vec![0; 0x100];
        let directory = [ROM, ROM + 0x20, ROM + 0x38, ROM + 0xc0, ROM + 0x80, ROM + 0x80];
        for (index, word) in directory.iter().enumerate() {
            rom[0x20 + index * 4..0x24 + index * 4].copy_from_slice(&word.to_le_bytes());
        }
        rom[0x80..0x80 + packed.len()].copy_from_slice(&packed);
        assert_eq!(unpacked(&rom, &map, "resource_004").unwrap(), payload);
        assert_eq!(unpacked(&rom, &map, "resource_005").unwrap(), payload);
        rom[0x80..0x80 + palette.len()].copy_from_slice(&palette);
        assert_eq!(unpacked(&rom, &map, "resource_004").unwrap(), payload);
        assert!(unpacked(&rom, &map, "resource_006").is_err());
        rom[0x24] ^= 1;
        assert!(unpacked(&rom, &map, "resource_004").is_err());
    }
}
