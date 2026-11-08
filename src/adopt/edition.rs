//! What adoption reads about one edition before it relinks a module there:
//! the module compiled as that edition's build compiles it, for its size.
//! Nothing here writes.

use crate::alchemy::built;
use crate::alchemy::targets::target_for;
use crate::gba::elf;
use std::path::Path;

/// The module as one edition compiles it.
pub(super) struct Compiled {
    pub(super) text: Vec<u8>,
}

impl Compiled {
    pub(super) fn size(&self) -> u32 {
        self.text.len() as u32
    }
}

/// One edition as adoption reads it.
pub(super) struct Edition {
    pub(super) compiled: Compiled,
}

impl Edition {
    /// Compiles `source` (a draft or an adopted file, on disk) for the
    /// edition, which must have been built.
    pub(super) fn read(root: &Path, game: &str, language: &str, source: &str) -> Result<Edition, String> {
        let id = built::edition(game, language).ok_or_else(|| format!("no edition {game}-{language}"))?;
        let target = target_for(id);
        let linked = built::output(root, id).join(format!("{id}.elf"));
        if !linked.is_file() {
            return Err(format!("{} is missing; build {id} first", linked.display()));
        }
        let work = crate::diff::Work::new(target)?;
        let object = work.folder.join("adopted.o");
        for step in crate::compile::steps(target, &root.join(source), &object, None, false)? {
            crate::gba::process::run(&step, &work.folder).map_err(|error| format!("{source} does not compile for {id}: {error}"))?;
        }
        let data = std::fs::read(&object).map_err(|error| format!("{}: {error}", object.display()))?;
        let text = elf::headers(&data)?
            .into_iter()
            .find(|header| header.name == ".text")
            .and_then(|header| data.get(header.offset..header.offset + header.size).map(<[u8]>::to_vec))
            .ok_or_else(|| format!("{source} compiles to no .text"))?;
        Ok(Edition { compiled: Compiled { text } })
    }
}

pub(super) fn incbin_line(offset: u32, length: u32) -> String {
    format!("\t.incbin \"baserom.gba\", {offset:#010x}, {length:#010x}")
}
