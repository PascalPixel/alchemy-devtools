//! Which objects each image of an edition links, in link order, as the build
//! reads them: the Makefile runs `games/<game>/ld_script.ld` through the C
//! preprocessor once for the main image and once for each overlay, and
//! writes the objects each script names to `out/<edition>/objects.mk`. That
//! list is read here; an edition whose list is missing has its preprocessed
//! scripts (`out/<edition>/ld/`) read instead, by the Makefile's own pattern.
//! Nothing here runs the preprocessor: only it decides an edition's tests.

use super::built;
use super::targets::DecompTargetId;
use std::path::Path;

/// An edition's objects, each a source stem (`games/<game>/src/...`), first
/// mention first: the main image's, and each overlay's by its number.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Linked {
    pub main: Vec<String>,
    pub overlays: Vec<(String, Vec<String>)>,
}

impl Linked {
    /// Whether any image links `stem`.
    pub fn links(&self, stem: &str) -> bool {
        self.main.iter().chain(self.overlays.iter().flat_map(|(_, objects)| objects)).any(|object| object == stem)
    }

    /// Every object, the main image's first, then each overlay's that the
    /// main image does not link, each once.
    pub fn all(&self) -> Vec<&str> {
        let mut all: Vec<&str> = Vec::new();
        for object in self.main.iter().chain(self.overlays.iter().flat_map(|(_, objects)| objects)) {
            if !all.contains(&object.as_str()) {
                all.push(object);
            }
        }
        all
    }

    /// The objects overlay `id` (`64b`, or `resource_64b`) links.
    pub fn overlay(&self, id: &str) -> Option<&[String]> {
        let id = id.strip_prefix("resource_").unwrap_or(id);
        self.overlays.iter().find(|(overlay, _)| overlay == id).map(|(_, objects)| objects.as_slice())
    }
}

/// One make variable's words from `objects.mk`: `NAME := ...`, continued on
/// tab-indented lines.
fn variable<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let mut words = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if !inside {
            let Some((variable, rest)) = line.split_once(":=") else { continue };
            if variable.trim() != name {
                continue;
            }
            inside = true;
            words.extend(rest.split_whitespace());
        } else if line.starts_with('\t') {
            words.extend(line.split_whitespace());
        } else {
            break;
        }
    }
    words.retain(|word| *word != "\\");
    words
}

/// The stems of a list of `$(BUILD)/obj/<stem>.o` words.
fn stems(words: &[&str]) -> Vec<String> {
    words.iter().filter_map(|word| word.strip_prefix("$(BUILD)/obj/")?.strip_suffix(".o")).map(str::to_string).collect()
}

/// An `objects.mk` the Makefile wrote.
pub(crate) fn parse_objects_mk(text: &str) -> Linked {
    // Each overlay's list, in the order they are written.
    let overlays = text
        .lines()
        .filter_map(|line| line.split_once(":=")?.0.trim().strip_prefix("OVERLAY_OBJS_"))
        .map(|id| (id.to_string(), stems(&variable(text, &format!("OVERLAY_OBJS_{id}")))))
        .collect();
    Linked { main: stems(&variable(text, "MAIN_OBJS")), overlays }
}

/// The source stem one linker-script statement names, by the Makefile's own
/// pattern (`^ *"*/(games/[^"]*)\.o"(`): a statement of the compiler library
/// or a comment names none.
pub(crate) fn statement_stem(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches(' ').strip_prefix("\"*/")?;
    let (path, after) = rest.split_once('"')?;
    if !after.starts_with('(') || !path.starts_with("games/") {
        return None;
    }
    path.strip_suffix(".o")
}

/// The objects a preprocessed script names, first mention first.
fn script_objects(text: &str) -> Vec<String> {
    let mut objects: Vec<String> = Vec::new();
    for stem in text.lines().filter_map(statement_stem) {
        if !objects.iter().any(|object| object == stem) {
            objects.push(stem.to_string());
        }
    }
    objects
}

/// An edition's objects from what its build wrote: `objects.mk`, else the
/// preprocessed scripts under `ld/`.
pub(crate) fn linked(root: &Path, id: DecompTargetId) -> Result<Linked, String> {
    let output = built::output(root, id);
    if let Ok(text) = std::fs::read_to_string(output.join("objects.mk")) {
        return Ok(parse_objects_mk(&text));
    }
    let main = std::fs::read_to_string(output.join("ld/main.ld")).map_err(|_| format!("{id} is not built (no objects.mk): make {id} first"))?;
    let mut overlays = Vec::new();
    let mut scripts: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(output.join("ld"))
        .map_err(|error| format!("{}: {error}", output.join("ld").display()))?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            let id = name.strip_prefix("overlay_")?.strip_suffix(".ld")?.to_string();
            Some((id, entry.path()))
        })
        .collect();
    scripts.sort();
    for (overlay, path) in scripts {
        let text = built::read_text(&path)?;
        overlays.push((overlay, script_objects(&text)));
    }
    Ok(Linked { main: script_objects(&main), overlays })
}

/// The objects of `output`'s `objects.mk` (`out/<edition>`), relative to
/// `root`: the build's own list, read by the commands that compile a draft.
pub(crate) fn from_objects_mk(root: &Path, output: &Path) -> Result<Linked, String> {
    let path = output.join("objects.mk");
    std::fs::read_to_string(root.join(&path)).map(|text| parse_objects_mk(&text)).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objects_mk_lists_each_image_first_mention_first() {
        let text = "# Written by the Makefile from the linker script: do not edit.\nMAIN_OBJS := \\\n\t$(BUILD)/obj/games/tla/src/system/ewram.o \\\n\t$(BUILD)/obj/games/tla/src/system/entry.o\nOVERLAYS := 649 64a\nOVERLAY_OBJS_649 := \\\n\t$(BUILD)/obj/games/tla/src/scene/a/a.o \\\n\t$(BUILD)/obj/games/tla/src/system/entry.o\nOVERLAY_OBJS_64a :=\n";
        let linked = parse_objects_mk(text);
        assert_eq!(linked.main, ["games/tla/src/system/ewram", "games/tla/src/system/entry"]);
        assert_eq!(linked.overlay("resource_649").unwrap(), ["games/tla/src/scene/a/a", "games/tla/src/system/entry"]);
        assert_eq!(linked.overlay("64a").unwrap(), Vec::<String>::new().as_slice());
        assert!(linked.links("games/tla/src/scene/a/a") && !linked.links("games/tla/src/scene/b/b"));
        assert_eq!(linked.all(), ["games/tla/src/system/ewram", "games/tla/src/system/entry", "games/tla/src/scene/a/a"]);
    }

    #[test]
    fn a_statement_names_its_object_as_the_makefile_reads_it() {
        assert_eq!(statement_stem("        \"*/games/tla/src/field/f021470.o\"(.text)"), Some("games/tla/src/field/f021470"));
        assert_eq!(statement_stem("\"*/games/tbs/src/system/iwram.o\"(.sym_iwram_slots)"), Some("games/tbs/src/system/iwram"));
        assert_eq!(statement_stem("        \"*libgcc.a:_udivsi3.o\"(.text)"), None);
        assert_eq!(statement_stem("        /* \"*/games/tla/src/x.o\"(.text) */"), None);
        assert_eq!(statement_stem("\t\"*/games/tla/src/x.o\"(.text)"), None, "the Makefile's pattern takes spaces only");
        let script = "  \"*/games/tla/src/a.o\"(.sym_iwram_slots)\n  \"*/games/tla/src/b.o\"(.text)\n  \"*/games/tla/src/a.o\"(.sym_iwram)\n";
        assert_eq!(script_objects(script), ["games/tla/src/a", "games/tla/src/b"]);
    }
}
