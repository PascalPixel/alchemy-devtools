//! Which objects each image of an edition links, in link order, as the build
//! reads them: the Makefile runs `games/<game>/ld_script.ld` through the C
//! preprocessor once for the main image and once for each overlay, and
//! writes the objects each script names to `out/<edition>/objects.mk`. That
//! list is read here, and refused when the scripts or headers it is made
//! from changed after it: an edition whose list is missing or stale is made
//! first.
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

/// An edition's objects from what its build wrote, `objects.mk`, once it is
/// as new as the scripts and headers it is made from.
pub(crate) fn linked(root: &Path, id: DecompTargetId) -> Result<Linked, String> {
    let output = built::output(root, id);
    if !output.join("objects.mk").is_file() {
        return Err(format!("{id} is not built (no objects.mk): make {id} first"));
    }
    from_objects_mk(root, &output)
}

/// The objects of `output`'s `objects.mk` (`out/<edition>`, relative to
/// `root` or not): the build's own list, refused when it is older than what
/// it is made from.
pub(crate) fn from_objects_mk(root: &Path, output: &Path) -> Result<Linked, String> {
    let path = root.join(output).join("objects.mk");
    let text = std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    fresh(root, output, &text)?;
    Ok(parse_objects_mk(&text))
}

/// Ok when every image's list was written after its script and the headers
/// it reads last changed. The Makefile rewrites a preprocessed script and
/// `objects.mk` only when their text changes, and touches the script's stamp
/// (`ld/main.stamp`, `ld/overlay_<id>.stamp`) whenever it runs the
/// preprocessor: the later of the stamp and `objects.mk` is when the list
/// was last made.
fn fresh(root: &Path, output: &Path, text: &str) -> Result<(), String> {
    let output = root.join(output);
    let edition = output.file_name().and_then(|name| name.to_str()).unwrap_or("");
    let game = edition.split('-').next().unwrap_or(edition);
    let modified = |path: &Path| std::fs::metadata(path).and_then(|meta| meta.modified()).ok();
    let written = modified(&output.join("objects.mk"));
    // The Makefile's prerequisites of each script: the one script, the
    // overlays' part it includes, and LD_HEADERS.
    let script = format!("games/{game}/ld_script.ld");
    let overlays = format!("games/{game}/src/system/overlay/overlay.ld");
    let headers = [format!("games/{game}/include/edition.h"), format!("games/{game}/include/version.h")];
    let check = |stamp: String, sources: Vec<&String>| -> Result<(), String> {
        let made = modified(&output.join("ld").join(stamp)).max(written);
        let newest = sources.into_iter().filter_map(|source| modified(&root.join(source)).map(|time| (time, source))).max();
        match newest {
            Some((time, source)) if made.is_none_or(|made| made < time) => {
                Err(format!("{}/objects.mk is older than {source}: make {edition} first", output.strip_prefix(root).unwrap_or(&output).display()))
            }
            _ => Ok(()),
        }
    };
    check("main.stamp".into(), [&script].into_iter().chain(&headers).collect())?;
    for overlay in variable(text, "OVERLAYS") {
        check(format!("overlay_{overlay}.stamp"), [&script, &overlays].into_iter().chain(&headers).collect())?;
    }
    Ok(())
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
    }

    /// A list is as new as its scripts by its own time or its stamp's, and
    /// refused once a script or header it is made from changed after both.
    #[test]
    fn a_list_older_than_its_scripts_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let at = |path: &str, seconds: u64| {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            if !path.exists() {
                std::fs::write(&path, "\n").unwrap();
            }
            let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 + seconds);
            std::fs::File::options().write(true).open(&path).unwrap().set_modified(time).unwrap();
        };
        at("games/tla/ld_script.ld", 10);
        at("games/tla/include/version.h", 0);
        at("games/tla/src/system/overlay/overlay.ld", 10);
        std::fs::create_dir_all(root.join("out/tla-en")).unwrap();
        std::fs::write(root.join("out/tla-en/objects.mk"), "MAIN_OBJS := \\\n\t$(BUILD)/obj/games/tla/src/a.o\nOVERLAYS := 649\nOVERLAY_OBJS_649 :=\n").unwrap();
        at("out/tla-en/objects.mk", 20);
        let read = || from_objects_mk(root, Path::new("out/tla-en"));
        assert_eq!(read().unwrap().main, ["games/tla/src/a"]);
        at("games/tla/src/system/overlay/overlay.ld", 30);
        assert!(read().unwrap_err().contains("older than games/tla/src/system/overlay/overlay.ld: make tla-en first"));
        // The preprocessor ran again and wrote the same list.
        at("out/tla-en/ld/overlay_649.stamp", 40);
        read().unwrap();
        at("games/tla/include/version.h", 50);
        assert!(read().unwrap_err().contains("version.h"));
        at("out/tla-en/ld/main.stamp", 60);
        assert!(read().unwrap_err().contains("version.h"), "each overlay's stamp counts for its own script");
        at("out/tla-en/ld/overlay_649.stamp", 60);
        read().unwrap();
    }
}
