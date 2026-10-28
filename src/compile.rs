//! A C draft compiled as the Makefile compiles the file it would replace:
//! the same compiler, options and edition for its route (K1, K2).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Locate the checkout at runtime, including when Alchemy Devtools is a submodule.
fn checkout_root(folder: &Path) -> Option<PathBuf> {
    folder.ancestors().find(|folder| {
        folder.join("rom.sha1").is_file()
            && folder.join("games").is_dir()
            && folder.join("Makefile").is_file()
    }).map(Path::to_path_buf)
}

/// A standalone Alchemy Devtools checkout can target Alchemy through ALCHEMY_ROOT.
pub fn root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let folder = std::env::var_os("ALCHEMY_ROOT").map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok());
        if let Some(found) = folder.as_deref().and_then(checkout_root) {
            return found.canonicalize().unwrap_or(found);
        }
        eprintln!("alchemy-devtools: run inside an Alchemy checkout or set ALCHEMY_ROOT to its path");
        std::process::exit(1)
    })
}

#[cfg(test)]
mod checkout_tests {
    use super::*;

    #[test]
    fn finds_the_game_checkout_above_a_submodule() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir(root.join("games")).unwrap();
        std::fs::write(root.join("rom.sha1"), "").unwrap();
        std::fs::write(root.join("Makefile"), "").unwrap();
        let nested = root.join("alchemy-devtools/src");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(checkout_root(&nested).as_deref(), Some(root));
        std::fs::remove_file(root.join("rom.sha1")).unwrap();
        assert!(checkout_root(&nested).is_none());
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompilerTarget {
    Tbs,
    Tla,
}

impl CompilerTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            CompilerTarget::Tbs => "tbs",
            CompilerTarget::Tla => "tla",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompilerFamily {
    Game,
    Agbcc,
    AgbccFlash,
    GameArm,
}

/// The Makefile's SOUND_C, FLASH_C and ARM_C.
const SOUND_C: &[&str] = &[
    "games/tbs/src/sound/audio.c",
    "games/tbs/src/sound/tables.c",
    "games/tbs/src/sound/engine.c",
    "games/tla/src/sound/sound.c",
    "games/tla/src/sound/tables.c",
];
const FLASH_C: &[&str] = &[
    "games/tbs/src/cartridge/save/flash.c",
    "games/tbs/src/cartridge/save/sst.c",
    "games/tbs/src/cartridge/save/atmel.c",
    "games/tbs/src/cartridge/save/program_flash_sector.c",
    "games/tla/src/system/save/flash.c",
    "games/tla/src/system/save/sst.c",
    "games/tla/src/system/save/atmel.c",
    "games/tla/src/system/save/program_flash_sector.c",
];
const ARM_C: &[&str] = &[
    "games/tbs/src/window/glyph_arm.c",
    "games/tbs/src/field/update.c",
    "games/tbs/src/field/vertices.c",
    "games/tla/src/graphics/tile/metatile.c",
];

fn relative(source: &str) -> &str {
    let root = root().to_string_lossy();
    source.strip_prefix(root.as_ref()).map_or(source, |rest| rest.trim_start_matches('/'))
}

/// The route the Makefile gives a source: by its path, as the file it would
/// replace when it is a draft named after one.
pub fn family_for_source(_target: CompilerTarget, source: &str) -> CompilerFamily {
    let source = relative(source);
    let tla_sound = source.starts_with("games/tla/src/sound/")
        && (source.ends_with("/engine.c") || source.ends_with("/tail.c") || source.contains("/music_track_"));
    if SOUND_C.contains(&source) || tla_sound {
        CompilerFamily::Agbcc
    } else if FLASH_C.contains(&source) {
        CompilerFamily::AgbccFlash
    } else if ARM_C.contains(&source) {
        CompilerFamily::GameArm
    } else {
        CompilerFamily::Game
    }
}

const INCLUDE_MODULES: [&str; 8] = ["battle", "field", "game", "graphics", "lib", "menu", "sound", "system"];

/// The game's include folders, and a branch's recon/<game>/include where
/// headers only drafts read have moved.
fn includes(target: CompilerTarget) -> Vec<String> {
    let mut flags = Vec::new();
    for base in ["games", "recon"] {
        let folder = root().join(base).join(target.as_str()).join("include");
        if base == "recon" && !folder.is_dir() {
            continue;
        }
        flags.push(format!("-I{}", folder.display()));
        flags.extend(INCLUDE_MODULES.iter().map(|module| format!("-I{}", folder.join(module).display())));
    }
    flags
}

fn tool(name: &str) -> String {
    root().join("tools/out/compilers").join(name).to_string_lossy().into_owned()
}

/// The checkout's Makefile's own value of `name` (`NAME := value`, continued
/// over lines ending in `\`), with the `$(VAR)`s it uses expanded the same
/// way, so a draft compiles as the build compiles on that branch.
fn makefile_var(name: &str) -> Option<String> {
    static MAKEFILE: OnceLock<String> = OnceLock::new();
    let text = MAKEFILE.get_or_init(|| std::fs::read_to_string(root().join("Makefile")).unwrap_or_default());
    makefile_value(text, name)
}

fn makefile_value(text: &str, name: &str) -> Option<String> {
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let Some(rest) = line.strip_prefix(name) else { continue };
        if !rest.starts_with([' ', ':', '=']) {
            continue;
        }
        let rest = rest.trim_start();
        let Some(value) = rest.strip_prefix(":=").or_else(|| rest.strip_prefix('=')) else { continue };
        let mut value = value.trim().to_string();
        while value.ends_with('\\') {
            value.pop();
            value.push(' ');
            value.push_str(lines.next().unwrap_or("").trim());
        }
        let mut expanded = String::new();
        let mut rest = value.as_str();
        while let Some(at) = rest.find("$(") {
            expanded.push_str(&rest[..at]);
            let inner = &rest[at + 2..];
            let Some(end) = inner.find(')') else { break };
            let reference = &inner[..end];
            expanded.push_str(&makefile_value(text, reference).unwrap_or_else(|| format!("$({reference})")));
            rest = &inner[end + 1..];
        }
        expanded.push_str(rest);
        return Some(expanded.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    None
}

/// The game's C compiler as the Makefile runs it (GAME_XGCC_<game>, else
/// XGCC), with its paths made absolute.
fn game_compiler(target: CompilerTarget) -> Vec<String> {
    let words = makefile_var(&format!("GAME_XGCC_{}", target.as_str()))
        .or_else(|| makefile_var("XGCC"))
        .unwrap_or_else(|| format!("{} -B{}/", tool("xgcc"), tool("")));
    words
        .split_whitespace()
        .map(|word| match word.strip_prefix("-B") {
            Some(path) => format!("-B{}", root().join(path).display()),
            None if word.contains('/') => root().join(word).display().to_string(),
            None => word.to_string(),
        })
        .collect()
}

/// The game's C options, the Makefile's CFLAGS_tbs or CFLAGS_tla on this
/// checkout's branch.
pub fn cflags(target: CompilerTarget) -> Vec<String> {
    makefile_var(&format!("CFLAGS_{}", target.as_str()))
        .unwrap_or_default()
        .split_whitespace()
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod makefile_tests {
    use super::makefile_value;

    #[test]
    fn reads_continued_and_nested_values() {
        let text = "COMPILER := tools/out/c\nXGCC := $(COMPILER)/xgcc -B$(COMPILER)/\nCFLAGS_tla := -O2 \\\n  -mthumb\nCFLAGS_tla304 := -Os\n";
        assert_eq!(makefile_value(text, "XGCC").as_deref(), Some("tools/out/c/xgcc -Btools/out/c/"));
        assert_eq!(makefile_value(text, "CFLAGS_tla").as_deref(), Some("-O2 -mthumb"));
        assert_eq!(makefile_value(text, "GAME_XGCC_tla"), None);
    }
}

/// ARM C's options (GCC 2.95.2).
pub fn arm_cflags() -> Vec<String> {
    ["-O2", "-mthumb-interwork", "-fomit-frame-pointer", "-fcall-used-r4"].map(String::from).to_vec()
}

const CPPFLAGS: &[&str] = &[
    "-lang-c", "-nostdinc", "-Acpu(arm)", "-Amachine(arm)", "-D__CHAR_UNSIGNED__", "-D__OPTIMIZE__",
    "-D__ARM_ARCH_4T__", "-D__APCS_32__", "-D__ARMEL__", "-Darm_elf", "-D__ELF__",
];

/// The commands that compile `source` into `object`, through its own route
/// or `family`: the edition's define and its build folder (for the message
/// ids) come from `define` and `build`.
pub fn steps(
    target: CompilerTarget,
    define: &str,
    build: &Path,
    source: &Path,
    object: &Path,
    family: Option<CompilerFamily>,
) -> Vec<Vec<String>> {
    let text = |path: &Path| path.to_string_lossy().into_owned();
    let (source_text, assembly, preprocessed) = (text(source), text(&object.with_extension("s")), text(&object.with_extension("i")));
    let family = family.unwrap_or_else(|| family_for_source(target, &source_text));
    let edition = [format!("-D{define}=1"), format!("-I{}", build.display())];
    let dumpbase = source.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let mut steps = Vec::new();
    match family {
        CompilerFamily::Game => {
            let mut compile = game_compiler(target);
            compile.extend(cflags(target));
            compile.extend(includes(target));
            compile.extend(edition);
            compile.extend(["-S".into(), "-o".into(), assembly.clone(), source_text]);
            steps.push(compile);
        }
        CompilerFamily::Agbcc | CompilerFamily::AgbccFlash => {
            let mut preprocess = vec![tool("cpp0")];
            preprocess.extend(CPPFLAGS.iter().map(|flag| flag.to_string()));
            preprocess.push(format!("-I{}/games/{}/include", root().display(), target.as_str()));
            preprocess.extend(["-D__GNUC__=2", "-D__GNUC_MINOR__=9", "-D__GNUC_PATCHLEVEL__=0", "-D__THUMBEL__", "-Dthumb", "-D__thumb__"].map(String::from));
            preprocess.extend(edition);
            preprocess.extend([source_text, preprocessed.clone()]);
            steps.push(preprocess);
            let level = if family == CompilerFamily::AgbccFlash { "-O" } else { "-O2" };
            steps.push(vec![tool("agbcc/old_agbcc"), preprocessed, "-dumpbase".into(), dumpbase, "-mthumb-interwork".into(), level.into(), "-o".into(), assembly.clone()]);
        }
        CompilerFamily::GameArm => {
            let mut preprocess = vec![tool("cpp0")];
            preprocess.extend(CPPFLAGS.iter().map(|flag| flag.to_string()));
            preprocess.extend(includes(target));
            preprocess.extend(["-D__GNUC__=2", "-D__GNUC_MINOR__=95", "-D__GNUC_PATCHLEVEL__=2", "-Darm", "-D__arm__"].map(String::from));
            preprocess.extend(edition);
            preprocess.extend([source_text, preprocessed.clone()]);
            steps.push(preprocess);
            let mut compile = vec![tool("gcc2952_arm"), preprocessed, "-dumpbase".into(), dumpbase];
            compile.extend(arm_cflags());
            compile.extend(["-o".into(), assembly.clone()]);
            steps.push(compile);
        }
    }
    let mut assemble = vec![tool("as"), "-marm7tdmi".into()];
    if family != CompilerFamily::GameArm {
        assemble.push("-mthumb".into());
    }
    assemble.extend(["-mthumb-interwork".into(), "-o".into(), text(object), assembly]);
    steps.push(assemble);
    steps
}
