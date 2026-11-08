//! What adoption reads about one edition before it relinks a module there:
//! the module compiled as that edition's build compiles it (its size and
//! the names it calls and points at), the names the edition's last link
//! defines, and its reference ROM, where the edition's own calls and pool
//! words say where a callee is. Nothing here writes; the ROM is only read.

use crate::alchemy::built;
use crate::alchemy::layout::{incbin, Layout};
use crate::alchemy::targets::target_for;
use crate::gba::elf::{self, Elf, Relocation, R_ARM_ABS32, R_ARM_THM_CALL};
use crate::gba::ld_map::Map;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The ROM's base in the GBA's address space.
const ROM: u32 = 0x0800_0000;

/// The module as one edition compiles it.
pub(super) struct Compiled {
    pub(super) text: Vec<u8>,
    /// Its functions' offsets in `.text`, Thumb bit cleared.
    pub(super) functions: BTreeMap<String, u32>,
    pub(super) relocations: Vec<Relocation>,
}

impl Compiled {
    pub(super) fn size(&self) -> u32 {
        self.text.len() as u32
    }
}

/// One edition as adoption reads it.
pub(super) struct Edition {
    pub(super) compiled: Compiled,
    /// Every name the edition's last link defines, with its value.
    pub(super) defined: BTreeMap<String, u32>,
    /// The names the last link placed through the edition's scaffold: one the
    /// scaffold no longer labels is no longer defined.
    pub(super) scaffold_names: BTreeSet<String>,
    /// The source stem of every object the last link placed.
    pub(super) linked_objects: BTreeSet<String>,
    pub(super) rom: Vec<u8>,
}

/// Whether a name is a message's or a resource's id, which the catalogue and
/// the resource directory define per edition.
pub(super) fn is_id(name: &str) -> bool {
    name.starts_with("Msg") || name.starts_with("ResourceId_")
}

impl Edition {
    /// Compiles `source` (a draft or an adopted file, on disk) for the
    /// edition and reads the edition's last link, its catalogue and its
    /// reference ROM.
    pub(super) fn read(root: &Path, game: &str, language: &str, source: &str) -> Result<Edition, String> {
        let id = built::edition(game, language).ok_or_else(|| format!("no edition {game}-{language}"))?;
        let target = target_for(id);
        let linked = built::output(root, id).join(format!("{id}.elf"));
        let linked = std::fs::read(&linked).map_err(|error| format!("{}: {error}; build {id} first", linked.display()))?;
        let defined = Elf::parse(&linked)?
            .symbols
            .into_iter()
            .filter(|symbol| symbol.section != 0 && symbol.bind != 0 && !symbol.name.is_empty())
            .map(|symbol| (symbol.name, symbol.value))
            .collect();
        let scaffold = Layout::new(game).scaffold(language);
        let map = built::read_text(&built::output(root, id).join(format!("{id}.map")))?;
        let map = Map::parse(&map);
        let linked_objects = map.placed.iter().map(|placed| built::stem(id, &placed.path)).collect();
        let scaffold_names = map
            .placed
            .into_iter()
            .filter(|placed| built::stem(id, &placed.path) == scaffold)
            .flat_map(|placed| placed.symbols.into_iter().map(|(_, name)| name))
            .collect();
        let rom = std::fs::read(root.join(target.rom)).map_err(|error| format!("{}: {error}", target.rom))?;
        target.verify_reference(&rom)?;
        let work = crate::diff::Work::new(target)?;
        let object = work.folder.join("adopted.o");
        for step in crate::compile::steps(target, &root.join(source), &object, None, false)? {
            crate::gba::process::run(&step, &work.folder).map_err(|error| format!("{source} does not compile for {id}: {error}"))?;
        }
        let data = std::fs::read(&object).map_err(|error| format!("{}: {error}", object.display()))?;
        let (text, relocations) = elf::relocated_section(&data, ".text")?.ok_or_else(|| format!("{source} compiles to no .text"))?;
        let text_index = elf::headers(&data)?.iter().position(|h| h.name == ".text").unwrap_or(0) as u16;
        let functions = Elf::parse(&data)?
            .symbols
            .into_iter()
            .filter(|symbol| symbol.section == text_index && symbol.is_function())
            .map(|symbol| (symbol.name, symbol.value & !1))
            .collect();
        Ok(Edition { compiled: Compiled { text, functions, relocations }, defined, scaffold_names, linked_objects, rom })
    }

    /// Where the edition places the relocation's target, read from its own
    /// code with the module's `function` at ROM offset `start`: the BL's
    /// destination, or the pool word less the addend the object holds. A
    /// pointer keeps its Thumb bit; None when the ROM holds no such call.
    fn target(&self, relocation: &Relocation, start: u32, function: u32) -> Option<u32> {
        let site = (start + relocation.offset).checked_sub(function)? as usize;
        match relocation.kind {
            R_ARM_THM_CALL => elf::thumb_bl_target(&self.rom, site).map(|offset| offset + ROM),
            R_ARM_ABS32 => {
                let word = u32::from_le_bytes(self.rom.get(site..site + 4)?.try_into().ok()?);
                let at = relocation.offset as usize;
                let addend = u32::from_le_bytes(self.compiled.text.get(at..at + 4)?.try_into().ok()?);
                Some(word.wrapping_sub(addend))
            }
            _ => None,
        }
    }

    /// The module's offset of the function the scaffold labels first.
    fn function(&self, name: &str) -> u32 {
        self.compiled.functions.get(name).copied().unwrap_or(0)
    }

    /// Labels in the scaffold's lines each function the module calls or
    /// points at that the edition defines nowhere, where the edition's own
    /// call or pool word lands; Err names the first that lands in no
    /// scaffold bytes. `start` is the ROM offset of the module's `first`
    /// function; `adopted` holds the names objects linked in the edition
    /// since its last link define. Returns a note for each label made.
    pub(super) fn label_callees(&self, lines: &mut Vec<String>, start: u32, first: &str, adopted: &BTreeSet<String>) -> Result<Vec<String>, String> {
        let function = self.function(first);
        let mut notes = Vec::new();
        let mut done: Vec<&str> = Vec::new();
        for relocation in &self.compiled.relocations {
            let name = relocation.symbol.as_str();
            if relocation.defined || name.is_empty() || is_id(name) || done.contains(&name) {
                continue;
            }
            if lines.iter().any(|line| line.trim() == format!(".global {name}")) {
                continue;
            }
            if adopted.contains(name) || (self.defined.contains_key(name) && !self.scaffold_names.contains(name)) {
                continue;
            }
            let kind = if relocation.kind == R_ARM_THM_CALL { "calls" } else { "points at" };
            let address = self
                .target(relocation, start, function)
                .ok_or_else(|| format!("{kind} {name}, which the edition's code at that place does not"))?;
            if !(ROM..ROM + self.rom.len() as u32).contains(&(address & !1)) {
                return Err(format!("{kind} {name}, which the edition places at {address:#x}, outside the cartridge"));
            }
            let thumb = relocation.kind == R_ARM_THM_CALL || address & 1 == 1;
            let offset = (address & !1) - ROM;
            if !label(lines, name, offset, thumb) {
                return Err(format!("{kind} {name}, which the edition places at {:#x}, in no bytes of its scaffold: label it there", address & !1));
            }
            notes.push(format!("{name} labelled at {offset:#x}, where its {} lands", if kind == "calls" { "call" } else { "pointer" }));
            done.push(name);
        }
        Ok(notes)
    }

}

/// Puts a label for `name` at ROM offset `offset` in a scaffold's lines,
/// splitting the `.incbin` that holds it; false when no incbin does.
pub(super) fn label(lines: &mut Vec<String>, name: &str, offset: u32, thumb: bool) -> bool {
    let header: Vec<String> = if thumb {
        vec![format!("\t.global {name}"), format!("\t.type {name}, %function"), "\t.thumb_func".into(), format!("{name}:")]
    } else {
        vec![format!("\t.global {name}"), format!("{name}:")]
    };
    for index in 0..lines.len() {
        let Some((start, length)) = incbin(&lines[index]) else { continue };
        if offset < start || offset >= start + length {
            continue;
        }
        let mut replacement = Vec::new();
        if offset > start {
            replacement.push(incbin_line(start, offset - start));
        }
        replacement.extend(header);
        replacement.push(incbin_line(offset, start + length - offset));
        lines.splice(index..=index, replacement);
        return true;
    }
    false
}

pub(super) fn incbin_line(offset: u32, length: u32) -> String {
    format!("\t.incbin \"baserom.gba\", {offset:#010x}, {length:#010x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_splits_the_incbin_that_holds_its_place() {
        let mut lines: Vec<String> = ["\t.section .rom.00000100, \"ax\"", "\t.incbin \"baserom.gba\", 0x00000100, 0x00000020"].map(String::from).into();
        assert!(label(&mut lines, "Func_08000110", 0x110, true));
        assert_eq!(
            lines,
            [
                "\t.section .rom.00000100, \"ax\"",
                "\t.incbin \"baserom.gba\", 0x00000100, 0x00000010",
                "\t.global Func_08000110",
                "\t.type Func_08000110, %function",
                "\t.thumb_func",
                "Func_08000110:",
                "\t.incbin \"baserom.gba\", 0x00000110, 0x00000010",
            ]
        );
        assert!(label(&mut lines, "Data_08000100", 0x100, false));
        assert_eq!(lines[1..3], ["\t.global Data_08000100", "Data_08000100:"]);
        assert!(!label(&mut lines, "Func_08000200", 0x200, true));
    }
}
