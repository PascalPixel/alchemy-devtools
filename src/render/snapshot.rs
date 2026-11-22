//! A private copy of the built edition a run reads (out/EDITION's ROM, ELF,
//! linker map and overlays), taken once as the run starts, so that a build
//! writing out/EDITION meanwhile (a landing's `make compare-tla`) cannot
//! hand the run half a file. The copy is taken only once out/EDITION has
//! lain still for a moment, is checked whole (every ELF an ELF, the linker
//! map ended), and is shared with the run's --jobs children through the
//! environment; the process that took it removes it as the run ends.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

/// How long out/EDITION must have lain still before it is copied.
const STILL: Duration = Duration::from_secs(5);
/// How long a run waits for a build to finish before it gives up.
const PATIENCE: Duration = Duration::from_secs(900);

/// The snapshots this process took, by edition, to remove as it ends.
static TAKEN: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
/// The snapshot each edition is read from, once known.
static DIRS: Mutex<Option<HashMap<String, PathBuf>>> = Mutex::new(None);

/// The environment variable a run's children find edition `edition`'s
/// snapshot by.
fn variable(edition: &str) -> String {
    format!("ALCHEMY_RENDER_BUILD_{}", edition.to_ascii_uppercase().replace('-', "_"))
}

/// The directory edition `edition`'s built files are read from: a private
/// copy of `root`/out/EDITION, taken the first time it is asked for (or
/// the one the parent of a --jobs child took). Where out/EDITION does not
/// exist, out/EDITION itself, so the plain "make EDITION" errors stand.
pub fn build(root: &Path, edition: &str) -> Result<PathBuf, String> {
    let mut dirs = DIRS.lock().map_err(|_| "the snapshot lock is poisoned".to_string())?;
    let dirs = dirs.get_or_insert_with(HashMap::new);
    if let Some(dir) = dirs.get(edition) {
        return Ok(dir.clone());
    }
    let source = root.join("out").join(edition);
    let dir = match std::env::var_os(variable(edition)).map(PathBuf::from).filter(|d| d.is_dir()) {
        Some(dir) => dir,
        None if !source.join(format!("{edition}.gba")).exists() => source,
        None => {
            let dir = take(&source, edition)?;
            std::env::set_var(variable(edition), &dir);
            TAKEN.lock().map_err(|_| "the snapshot lock is poisoned".to_string())?.push(dir.clone());
            dir
        }
    };
    dirs.insert(edition.to_string(), dir.clone());
    Ok(dir)
}

/// Removes the snapshots this process took (not a child's parent's).
pub fn release() {
    if let Ok(mut taken) = TAKEN.lock() {
        for dir in taken.drain(..) {
            let _ = std::fs::remove_dir_all(dir.parent().unwrap_or(&dir));
        }
    }
}

/// The files of out/EDITION a run reads, relative to it.
fn wanted(source: &Path, edition: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = ["gba", "elf", "map"].iter().map(|x| PathBuf::from(format!("{edition}.{x}"))).collect();
    let mut overlays: Vec<PathBuf> = std::fs::read_dir(source.join("overlays"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("resource_") && (n.ends_with(".map") || (n.ends_with(".elf") && !n.ends_with(".symbols.elf"))))
        .map(|n| PathBuf::from("overlays").join(n))
        .collect();
    overlays.sort();
    files.extend(overlays);
    files
}

/// Each file's length and time of last change, None for one missing.
fn stamps(source: &Path, files: &[PathBuf]) -> Vec<Option<(u64, SystemTime)>> {
    files.iter().map(|f| std::fs::metadata(source.join(f)).ok().and_then(|m| Some((m.len(), m.modified().ok()?)))).collect()
}

/// Copies out/EDITION's files into a new directory once they have lain
/// still for STILL and are whole, waiting up to PATIENCE for a build
/// writing them to finish.
fn take(source: &Path, edition: &str) -> Result<PathBuf, String> {
    sweep();
    let started = std::time::Instant::now();
    let base = std::env::temp_dir().join("alchemy-render").join(format!("{}-{edition}", std::process::id()));
    let dir = base.join(edition);
    let mut said = false;
    loop {
        let files = wanted(source, edition);
        let before = stamps(source, &files);
        let newest = before.iter().flatten().map(|s| s.1).max().unwrap_or(SystemTime::UNIX_EPOCH);
        let still = SystemTime::now().duration_since(newest).unwrap_or_default() >= STILL;
        let mut why = None;
        if still {
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("overlays")).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
            for f in &files {
                if let Err(e) = std::fs::copy(source.join(f), dir.join(f)) {
                    why = Some(format!("{} could not be copied ({e})", f.display()));
                    break;
                }
            }
            if why.is_none() && stamps(source, &files) != before {
                why = Some("its files changed while they were copied".to_string());
            }
            if why.is_none() {
                why = whole(&dir, &files).err();
            }
            if why.is_none() {
                return Ok(dir);
            }
        }
        let why = why.unwrap_or_else(|| "a file in it changed in the last few seconds".to_string());
        if started.elapsed() >= PATIENCE {
            let _ = std::fs::remove_dir_all(&base);
            return Err(format!(
                "{} is being rebuilt or is incomplete ({why}), and stayed so for {} minutes: wait for the build writing it to finish (make {edition}) and run again",
                source.display(),
                PATIENCE.as_secs() / 60
            ));
        }
        if !said {
            eprintln!("{} is being rebuilt ({why}): waiting for it to lie still", source.display());
            said = true;
        }
        std::thread::sleep(Duration::from_secs(3));
    }
}

/// Whether the copied files are whole: each ELF starts as a little-endian
/// ELF32 file and the linker map reaches its OUTPUT line.
fn whole(dir: &Path, files: &[PathBuf]) -> Result<(), String> {
    for f in files {
        let path = dir.join(f);
        let name = f.to_string_lossy();
        if name.ends_with(".elf") {
            let mut head = [0u8; 6];
            let read = std::fs::File::open(&path).and_then(|mut file| std::io::Read::read_exact(&mut file, &mut head));
            if read.is_err() || head != [0x7f, b'E', b'L', b'F', 1, 1] {
                return Err(format!("{name} is not a whole little-endian ELF32 file"));
            }
        } else if name.ends_with(".map") && !f.starts_with("overlays") {
            let text = std::fs::read(&path).map_err(|e| format!("{name}: {e}"))?;
            if !map_is_whole(&text) {
                return Err(format!("{name} stops before its OUTPUT line"));
            }
        } else if name.ends_with(".gba") && std::fs::metadata(&path).map_or(0, |m| m.len()) == 0 {
            return Err(format!("{name} is empty"));
        }
    }
    Ok(())
}

/// Whether a linker map reaches the OUTPUT line ld writes near its end.
fn map_is_whole(text: &[u8]) -> bool {
    String::from_utf8_lossy(text).lines().any(|l| l.starts_with("OUTPUT("))
}

/// Removes snapshots left by runs that no longer run (killed before they
/// could remove their own).
fn sweep() {
    let base = std::env::temp_dir().join("alchemy-render");
    for entry in std::fs::read_dir(&base).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(pid) = name.split('-').next().and_then(|p| p.parse::<u32>().ok()) else {
            continue;
        };
        if pid != std::process::id() && !Path::new(&format!("/proc/{pid}")).exists() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_map_is_whole_once_it_reaches_its_output_line() {
        assert!(map_is_whole(b"Memory map\nLOAD a.o\nOUTPUT(out/tla-en/tla-en.elf elf32-littlearm)\nLOAD linker stubs\n"));
        assert!(!map_is_whole(b"Memory map\nLOAD a.o\n .text 0x0800 MainRoutineWork_Init"));
    }

    #[test]
    fn a_half_written_build_is_named_and_a_whole_one_copied() {
        let source = std::env::temp_dir().join(format!("alchemy-snapshot-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&source);
        std::fs::create_dir_all(source.join("overlays")).unwrap();
        std::fs::write(source.join("x-en.gba"), [1u8; 16]).unwrap();
        std::fs::write(source.join("x-en.elf"), [0x7f, b'E', b'L', b'F', 1, 1, 0, 0]).unwrap();
        std::fs::write(source.join("x-en.map"), "OUTPUT(x)\n").unwrap();
        std::fs::write(source.join("overlays/resource_001.elf"), b"\x7fELF").unwrap();
        std::fs::write(source.join("overlays/resource_001.symbols.elf"), b"").unwrap();
        let files = wanted(&source, "x-en");
        assert_eq!(files.len(), 4, "{files:?}");
        assert_eq!(whole(&source, &files), Err("overlays/resource_001.elf is not a whole little-endian ELF32 file".to_string()));
        std::fs::write(source.join("overlays/resource_001.elf"), [0x7f, b'E', b'L', b'F', 1, 1]).unwrap();
        assert_eq!(whole(&source, &files), Ok(()));
        std::fs::remove_dir_all(&source).unwrap();
    }
}
