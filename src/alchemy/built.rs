//! What a built edition leaves under `out/`, as the inventory commands read it
//! (`where`, `editions`, `check`, `regress`): its linker maps and the sections
//! they place, its objects, packed overlays and ROM, and the object list
//! (`games/<game>/objects.tsv`) it was linked from. Nothing here builds; a
//! missing file is reported as not built.

use super::targets::{target_for, DecompTargetId, TARGET_IDS};
use crate::gba::ld_map::{Map, Placed, ROM};
use std::path::{Path, PathBuf};

/// The end of the cartridge's ROM window: a main image's section counts as
/// placed when its load address lies in `ROM..ROM_END`.
pub const ROM_END: u32 = 0x0a00_0000;

pub fn game(id: DecompTargetId) -> &'static str {
    id.as_str().split_once('-').map_or(id.as_str(), |(game, _)| game)
}

pub fn language(id: DecompTargetId) -> &'static str {
    id.as_str().split_once('-').map_or("", |(_, language)| language)
}

/// Every game, in the targets' order.
pub fn games() -> Vec<&'static str> {
    let mut games: Vec<&str> = TARGET_IDS.iter().map(|&id| game(id)).collect();
    games.dedup();
    games
}

/// The editions a command line names: an edition (`tla-en`) or a game (`tla`,
/// its six). None names all twelve.
pub fn editions(words: &[String]) -> Result<Vec<DecompTargetId>, String> {
    if words.is_empty() {
        return Ok(TARGET_IDS.to_vec());
    }
    let mut chosen = Vec::new();
    for word in words {
        let named: Vec<_> = TARGET_IDS.into_iter().filter(|&id| id.as_str() == word || game(id) == word).collect();
        if named.is_empty() {
            return Err(format!(
                "{word} is neither an edition nor a game: expected {} or one of {}",
                games().join(" or "),
                TARGET_IDS.map(|id| id.as_str()).join(" ")
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
    TARGET_IDS.into_iter().find(|&id| self::game(id) == game && self::language(id) == language)
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
    let stem = format!("recon/{}/{}/rom", game(id), language(id));
    root.join(format!("{stem}.s")).is_file().then(|| (format!("{stem}.s"), object(root, id, &stem)))
}

/// The source stem behind an object path a map names: `out/<edition>/obj/X.o`
/// is `X`, and an overlay's own scaffold, `out/<edition>/overlays/resource_<id>_overlay.o`,
/// is the source the Makefile assembles it from. Anything else (the compiler
/// library, the linker's stubs) is named as the map names it.
pub fn stem(id: DecompTargetId, path: &str) -> String {
    let target = target_for(id);
    let built = |folder: &str| {
        let marker = format!("{}/{folder}/", target.output_dir);
        path.split_once(&marker).map(|(_, tail)| tail.strip_suffix(".o").unwrap_or(tail).to_string())
    };
    if let Some(stem) = built("obj") {
        return stem;
    }
    if let Some(stem) = built("overlays") {
        return format!("{}/{stem}", target.overlay_dir);
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
    /// A `.text` section, or a scaffold's `.rom.*` section of code and data
    /// not yet told apart.
    pub fn is_code(&self) -> bool {
        self.placed.name.starts_with(".text") || self.placed.name.starts_with(".rom.")
    }

    /// The stem, with its section unless that is plain `.text`.
    pub fn label(&self) -> String {
        match self.placed.name.as_str() {
            ".text" => self.stem.clone(),
            name => format!("{}({name})", self.stem),
        }
    }
}

/// Whether a section places bytes: loaded, not the linker's fill, not empty.
fn places(placed: &Placed) -> bool {
    placed.loaded && !placed.is_fill() && placed.size > 0
}

/// Every section an edition places: the main map's that load into the ROM
/// window, then each overlay map's. An overlay's map also lists its
/// unallocated sections (attributes, comments) at address 0, which place
/// nothing.
pub fn pieces(root: &Path, id: DecompTargetId) -> Result<Vec<Piece>, String> {
    let main = read_text(&output(root, id).join(format!("{id}.map")))?;
    let mut pieces: Vec<Piece> = Map::parse(&main)
        .placed
        .into_iter()
        .filter(|placed| places(placed) && (ROM..ROM_END).contains(&placed.load))
        .map(|placed| Piece { stem: stem(id, &placed.path), placed })
        .collect();
    for (_, path) in overlay_files(root, id, "map")? {
        let map = Map::parse(&read_text(&path)?);
        pieces.extend(
            map.placed
                .into_iter()
                .filter(|placed| places(placed) && placed.address != 0)
                .map(|placed| Piece { stem: stem(id, &placed.path), placed }),
        );
    }
    Ok(pieces)
}

/// One row of a game's object list: the image and place it is linked into,
/// the object, its sections, and the languages that link it (none listed:
/// every edition).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub image: String,
    pub place: String,
    pub object: String,
    pub sections: String,
    pub languages: Vec<String>,
}

impl Row {
    pub fn links(&self, language: &str) -> bool {
        self.languages.is_empty() || self.languages.iter().any(|l| l == language)
    }
}

pub fn parse_object_list(text: &str) -> Vec<Row> {
    text.lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with("image\t"))
        .map(|line| {
            let mut columns = line.split('\t');
            let mut next = || columns.next().unwrap_or("").to_string();
            let (image, place, object, sections, languages) = (next(), next(), next(), next(), next());
            let languages = languages.split_whitespace().map(str::to_string).collect();
            Row { image, place, object, sections, languages }
        })
        .collect()
}

pub fn object_list(root: &Path, game: &str) -> Result<Vec<Row>, String> {
    Ok(parse_object_list(&read_text(&root.join(format!("games/{game}/objects.tsv")))?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use DecompTargetId::*;

    #[test]
    fn games_and_editions_are_named_on_the_command_line() {
        let words = |items: &[&str]| items.iter().map(|item| item.to_string()).collect::<Vec<_>>();
        assert_eq!(editions(&[]).unwrap().len(), 12);
        assert_eq!(editions(&words(&["tla-de", "tla", "tbs-en"])).unwrap(), [TlaDe, TlaJa, TlaEn, TlaEs, TlaFr, TlaIt, TbsEn]);
        assert!(editions(&words(&["tla-xx"])).is_err());
        assert_eq!(games(), ["tbs", "tla"]);
        assert_eq!((game(TlaDe), language(TlaDe)), ("tla", "de"));
        assert_eq!(edition("tbs", "it"), Some(TbsIt));
    }

    #[test]
    fn object_paths_name_their_sources() {
        assert_eq!(stem(TlaEn, "out/tla-en/obj/recon/tla/raw/0815f16c.o"), "recon/tla/raw/0815f16c");
        assert_eq!(stem(TlaJa, "out/tla-ja/obj/recon/tla/ja/rom.o"), "recon/tla/ja/rom");
        assert_eq!(
            stem(TlaEn, "out/tla-en/overlays/resource_64c_overlay.o"),
            "recon/tla/raw/overlays/resource_64c_overlay"
        );
        assert_eq!(stem(TlaEn, "tools/out/compiler-runtime/libgcc.a(_call_via_rX.o)"), "tools/out/compiler-runtime/libgcc.a(_call_via_rX.o)");
    }

    #[test]
    fn object_list_rows_keep_their_languages() {
        let rows = parse_object_list(
            "image\tplace\tobject\tsections\teditions\nmain\tiwram\tgames/tla/src/system/iwram\t.sym_iwram\nmain\trom_main\tgames/tla/src/window/f039864\t.text\ten\n",
        );
        assert_eq!(rows.len(), 2);
        assert!(rows[0].links("ja") && rows[0].languages.is_empty());
        assert!(rows[1].links("en") && !rows[1].links("ja"));
        assert_eq!(rows[1].sections, ".text");
    }
}
