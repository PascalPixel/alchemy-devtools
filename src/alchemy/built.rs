//! What a built edition leaves under `out/`, as the inventory commands read it
//! (`where`, `editions`, `check`, `regress`): its linker maps and the sections
//! they place, its objects, packed overlays and ROM. Nothing here builds; a
//! missing file is reported as not built.

use super::done::LANGUAGES;
use super::layout::Layout;
use super::targets::{self, target_for, DecompTargetId};
use crate::gba::count::{self, Image};
use crate::gba::ld_map::{Map, Placed};
use std::path::{Path, PathBuf};

/// Every edition, each game's six together, the games in rom.sha1's order and
/// the languages in the measurement's (`ja en de es fr it`).
fn all() -> Result<Vec<DecompTargetId>, String> {
    let mut ids = targets::ids()?;
    let games = targets::games()?;
    let language = |id: DecompTargetId| LANGUAGES.iter().position(|&language| language == id.language());
    ids.sort_by_key(|&id| (games.iter().position(|&game| game == id.game()), language(id)));
    Ok(ids)
}

/// The editions a command line names: an edition (`tla-en`) or a game (`tla`,
/// its six). None names all twelve.
pub fn editions(words: &[String]) -> Result<Vec<DecompTargetId>, String> {
    let all = all()?;
    if words.is_empty() {
        return Ok(all);
    }
    let mut chosen = Vec::new();
    for word in words {
        let named: Vec<_> = all.iter().copied().filter(|&id| id.as_str() == word || id.game() == word).collect();
        if named.is_empty() {
            let names: Vec<&str> = all.iter().map(|id| id.as_str()).collect();
            return Err(format!(
                "{word} is neither an edition nor a game: expected {} or one of {}",
                targets::games()?.join(" or "),
                names.join(" ")
            ));
        }
        for id in named {
            if !chosen.contains(&id) {
                chosen.push(id);
            }
        }
    }
    Ok(chosen)
}

/// The edition of `game` in `language`.
pub fn edition(game: &str, language: &str) -> Option<DecompTargetId> {
    targets::ids().ok()?.into_iter().find(|&id| id.game() == game && id.language() == language)
}

pub fn output(root: &Path, id: DecompTargetId) -> PathBuf {
    root.join(target_for(id).output_dir)
}

pub fn read_text(path: &Path) -> Result<String, String> {
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// The object a source stem builds in an edition.
pub fn object(root: &Path, id: DecompTargetId, stem: &str) -> PathBuf {
    output(root, id).join("obj").join(format!("{stem}.o"))
}

/// The source an object stem is built from, as the Makefile's `source` picks
/// it: `NAME.c`, `NAME.s`, `NAME.S` or `NAME.mid`.
pub fn source(root: &Path, stem: &str) -> Option<String> {
    ["c", "s", "S", "mid"].iter().map(|extension| format!("{stem}.{extension}")).find(|path| root.join(path).is_file())
}

/// An edition's scaffold for its main image, when it has one: the source
/// (`recon/<game>/<language>/rom.s`) and the object it builds.
pub fn scaffold(root: &Path, id: DecompTargetId) -> Option<(String, PathBuf)> {
    let stem = Layout::new(id.game()).scaffold(id.language());
    root.join(format!("{stem}.s")).is_file().then(|| (format!("{stem}.s"), object(root, id, &stem)))
}

/// The source stem of an object path a map names, when the build compiled
/// it from the checkout: `out/<edition>/obj/X.o` is `X` (`games/tla/src/...`,
/// `recon/tla/ja/rom`). This is the one reading of the build's object folder;
/// the generic map reader leaves paths as the map writes them.
pub fn object_stem(path: &str) -> Option<&str> {
    let (_, tail) = path.split_once("/obj/")?;
    Some(tail.strip_suffix(".o").unwrap_or(tail))
}

/// The source stem behind an object path a map names: `out/<edition>/obj/X.o`
/// is `X`, and an overlay's own scaffold, `out/<edition>/overlays/resource_<id>_overlay.o`,
/// is the source the Makefile assembles it from (The Broken Seal's assemble
/// nothing and place no bytes). Anything else (the compiler library, the
/// linker's stubs) is named as the map names it.
pub fn stem(id: DecompTargetId, path: &str) -> String {
    if let Some(stem) = object_stem(path) {
        return stem.to_string();
    }
    let marker = format!("{}/overlays/", target_for(id).output_dir);
    if let Some((_, tail)) = path.split_once(&marker) {
        return format!("{}/{}", Layout::new(id.game()).overlays(), tail.strip_suffix(".o").unwrap_or(tail));
    }
    path.to_string()
}

/// Each overlay's file with `extension` in an edition's build, by overlay
/// name (`resource_64b`), sorted.
pub fn overlay_files(root: &Path, id: DecompTargetId, extension: &str) -> Result<Vec<(String, PathBuf)>, String> {
    let folder = output(root, id).join("overlays");
    let entries = std::fs::read_dir(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    let mut files: Vec<(String, PathBuf)> = entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_name()?.to_str()?.strip_suffix(&format!(".{extension}"))?.to_string();
            let id = name.strip_prefix("resource_")?;
            (!id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit())).then_some((name, path))
        })
        .collect();
    files.sort();
    Ok(files)
}

/// One section an edition's images place in the cartridge, with the source
/// stem of the object that placed it.
pub struct Piece {
    pub stem: String,
    pub placed: Placed,
}

impl Piece {
    /// Code as C2 counts it: a `.text` section, as `done` reads the maps.
    pub fn is_code(&self) -> bool {
        self.placed.name.starts_with(".text")
    }

    /// A scaffold's `.rom.*` section, its code and data not yet told apart;
    /// data to C2.
    pub fn is_scaffold(&self) -> bool {
        self.placed.name.starts_with(".rom.")
    }

    /// The stem, with its section unless that is plain `.text`.
    pub fn label(&self) -> String {
        match self.placed.name.as_str() {
            ".text" => self.stem.clone(),
            name => format!("{}({name})", self.stem),
        }
    }
}

/// Every section an edition places, as calcrom counts it: the main map's ROM
/// image, then each overlay map's own.
pub fn pieces(root: &Path, id: DecompTargetId) -> Result<Vec<Piece>, String> {
    let piece = |placed: &Placed| Piece { stem: stem(id, &placed.path), placed: placed.clone() };
    let main = Map::parse(&read_text(&output(root, id).join(format!("{id}.map")))?);
    let mut pieces: Vec<Piece> = count::placed(&main, Image::Rom).map(piece).collect();
    for (_, path) in overlay_files(root, id, "map")? {
        let map = Map::parse(&read_text(&path)?);
        pieces.extend(count::placed(&map, Image::Linked(super::overlay::SECTIONS)).map(piece));
    }
    Ok(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> DecompTargetId {
        targets::parse_decomp_target(name).unwrap()
    }

    #[test]
    fn games_and_editions_are_named_on_the_command_line() {
        let words = |items: &[&str]| items.iter().map(|item| item.to_string()).collect::<Vec<_>>();
        assert_eq!(editions(&[]).unwrap().len(), 12);
        let named: Vec<&str> = editions(&words(&["tla-de", "tla", "tbs-en"])).unwrap().iter().map(|id| id.as_str()).collect();
        assert_eq!(named, ["tla-de", "tla-ja", "tla-en", "tla-es", "tla-fr", "tla-it", "tbs-en"]);
        assert!(editions(&words(&["tla-xx"])).is_err());
                assert_eq!((id("tla-de").game(), id("tla-de").language()), ("tla", "de"));
        assert_eq!(edition("tbs", "it"), Some(id("tbs-it")));
    }

    #[test]
    fn object_paths_name_their_sources() {
        assert_eq!(stem(id("tla-en"), "out/tla-en/obj/recon/tla/raw/0815f16c.o"), "recon/tla/raw/0815f16c");
        assert_eq!(stem(id("tla-ja"), "out/tla-ja/obj/recon/tla/ja/rom.o"), "recon/tla/ja/rom");
        assert_eq!(
            stem(id("tla-en"), "out/tla-en/overlays/resource_64c_overlay.o"),
            "recon/tla/raw/overlays/resource_64c_overlay"
        );
        assert_eq!(stem(id("tla-en"), "tools/out/compiler-runtime/libgcc.a(_call_via_rX.o)"), "tools/out/compiler-runtime/libgcc.a(_call_via_rX.o)");
    }
}
