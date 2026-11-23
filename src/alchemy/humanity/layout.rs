//! The game's files and folders as targets of their own: how many source
//! files and folders there are, which headers lie loose in `include/`, which
//! files and folders are named by number, and which objects keep their parts
//! in a folder beside them (`src/field/field.c` and `src/field/field/`), and
//! which numbered files nothing in the game uses.

use super::Report;
use regex::Regex;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;

/// The extensions of source files: C, headers and assembly.
const SOURCE: &[&str] = &["c", "h", "s", "inc", "S"];

/// Segments of a name whose digits are a unit or a convention, not a number
/// standing for a meaning: `4bpp` (bits per pixel), `q16` and `q14` (a fixed
/// point format), `m4a` (pret's name for the sound driver), `vector3` (a
/// dimension).
fn convention() -> &'static Regex {
    static SET: OnceLock<Regex> = OnceLock::new();
    SET.get_or_init(|| Regex::new(r"^(?:[0-9]+bpp|q[0-9]+|m4a|vector[0-9]|vec[0-9])$").unwrap())
}

/// A file's or folder's name without its extensions: `star_dot_00` for
/// `star_dot_00.png`, `vale` for `vale.c`.
fn stem(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

/// Whether a name holds a digit run outside the conventions: `map_108`,
/// `jupiter_lighthouse_room_5`, `unnamed_28`, `set_03`, not `decompress_4bpp`
/// or `mul_q16`.
pub fn named_by_number(name: &str) -> bool {
    stem(name).split('_').any(|segment| segment.bytes().any(|b| b.is_ascii_digit()) && !convention().is_match(segment))
}

/// A frame of one set: a picture `NAME_NN.png` beside `NAME_00.png`, which the
/// build joins into `NAME.bitmaps` (the Makefile's `PICTURE_RUN`). Its number
/// is its place in the animation.
fn is_frame(path: &str, tracked: &HashSet<&str>) -> bool {
    let Some(base) = path.strip_suffix(".png") else { return false };
    let Some((name, number)) = base.rsplit_once('_') else { return false };
    number.len() == 2 && number.bytes().all(|b| b.is_ascii_digit()) && tracked.contains(format!("{name}_00.png").as_str())
}

/// Whether an `its number` row names the thing a file or folder is named
/// for: `status_grim_reaper_1.png` is `SPRITE_STATUS_GRIM_REAPER_1`'s
/// picture, `test_3` the debug area `SceneId_Test3`.
fn by_row(name: &str, rows: &[String]) -> bool {
    let flat = |text: &str| text.chars().filter(|c| *c != '_').collect::<String>().to_ascii_lowercase();
    let name = flat(stem(name));
    rows.iter().any(|row| flat(row).ends_with(&name))
}

/// The extensions of the tracked text that may use a file's symbol: C,
/// headers and assembly. A table's row (`ICON_42` in `icons.tsv`) is no
/// symbol: its file is reached by its row's number, which nothing here reads.
pub const SYMBOL_TEXT: &[&str] = &["c", "h", "s", "inc", "S"];

/// C's comments taken out of a text, lines kept: a comment naming a sound
/// (`/// SE_528's notes at a faster tempo`) does not play it.
fn without_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('/') {
        out.push_str(&rest[..at]);
        let after = &rest[at..];
        if after.starts_with("//") {
            let end = after.find('\n').unwrap_or(after.len());
            rest = &after[end..];
        } else if after.starts_with("/*") {
            let end = after[2..].find("*/").map_or(after.len(), |end| end + 4);
            out.extend(after[..end].chars().filter(|&c| c == '\n'));
            rest = &after[end..];
        } else {
            out.push('/');
            rest = &after[1..];
        }
    }
    out.push_str(rest);
    out
}

/// Each identifier holding a digit in `texts` (path, text), with the files
/// and lines it is written on.
pub fn numbered_symbols(texts: &[(String, String)]) -> HashMap<String, Vec<(usize, usize)>> {
    static IDENT: OnceLock<Regex> = OnceLock::new();
    let ident = IDENT.get_or_init(|| Regex::new(r"[A-Za-z_][A-Za-z0-9_]*[0-9][A-Za-z0-9_]*").unwrap());
    let mut symbols: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
    for (file, (path, text)) in texts.iter().enumerate() {
        let text = if path.ends_with(".c") || path.ends_with(".h") { without_comments(text) } else { text.clone() };
        for (line, content) in text.lines().enumerate() {
            for found in ident.find_iter(content) {
                let before = content[..found.start()].chars().next_back();
                if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                    continue;
                }
                symbols.entry(found.as_str().to_string()).or_default().push((file, line));
            }
        }
    }
    symbols
}

/// Whether a numbered file is one nothing uses: it has a symbol in the
/// sources, its stem's words in capitals (`SE_120` for `se_120.mid`) or in
/// mixed case after a prefix (`Sound_Wave50` for `wave_50.pcm8.wav`), and
/// each of its symbols is written only where it is defined, on one line or
/// two together (`SE_120,` in `sounds.h`; `.global X` and `X:`). A file
/// with no symbol (a sprite kept by its row's number) cannot be shown
/// unused this way, and stays.
fn unused(name: &str, symbols: &HashMap<String, Vec<(usize, usize)>>) -> bool {
    let stem = stem(name);
    let upper = stem.to_ascii_uppercase();
    let mixed: String = stem
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or(String::new(), |first| first.to_ascii_uppercase().to_string() + chars.as_str())
        })
        .collect();
    let mut places: Vec<(usize, usize)> = Vec::new();
    for (symbol, at) in symbols {
        if *symbol == upper || *symbol == mixed || symbol.strip_suffix(mixed.as_str()).is_some_and(|prefix| prefix.ends_with('_')) {
            places.extend(at);
        }
    }
    places.sort_unstable();
    let Some(&(file, first)) = places.first() else { return false };
    places.iter().all(|&(other, line)| other == file && line <= first + 1)
}

/// Count a game's tracked files under `games/<game>/` (`paths`, each from
/// the checkout's root) into its report. `rows` are the names its
/// `evidence/<game>-names.tsv` gives the kind `its number`; `symbols` the
/// numbered identifiers its tracked text writes (`numbered_symbols`), by
/// which a numbered file nothing uses is set apart.
pub fn count(report: &mut Report, game: &str, paths: &[String], rows: &[String], symbols: &HashMap<String, Vec<(usize, usize)>>) {
    let root = format!("games/{game}/");
    let tracked: HashSet<&str> = paths.iter().map(String::as_str).collect();
    let mut folders = BTreeSet::new();
    for path in paths {
        let Some(inside) = path.strip_prefix(&root) else { continue };
        let mut at = 0;
        while let Some(slash) = inside[at..].find('/') {
            folders.insert(&path[..root.len() + at + slash]);
            at += slash + 1;
        }
        let name = &path[path.rfind('/').map_or(0, |slash| slash + 1)..];
        let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
        if SOURCE.contains(&extension) {
            report.add("source_files", path, 1);
            if inside.starts_with("include/") && !inside["include/".len()..].contains('/') && extension == "h" {
                report.note("loose_headers", path, 0, name);
            }
        }
        if named_by_number(name) && !is_frame(path, &tracked) {
            let metric = if by_row(name, rows) {
                "numbered_files_honest"
            } else if unused(name, symbols) {
                "numbered_files_unused"
            } else {
                "numbered_files"
            };
            report.note(metric, path, 0, name);
        }
    }
    for folder in &folders {
        report.add("folders", folder, 1);
        let name = &folder[folder.rfind('/').map_or(0, |slash| slash + 1)..];
        if named_by_number(name) {
            let metric = if by_row(name, rows) { "numbered_files_honest" } else { "numbered_files" };
            report.note(metric, &format!("{folder}/"), 0, name);
        }
        // `src/field/field/` beside `src/field/field.c`: an object's parts.
        if tracked.contains(format!("{folder}.c").as_str()) {
            report.note("part_folders", &format!("{folder}/"), 0, name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_and_folders_by_name() {
        for name in ["map_108", "jupiter_lighthouse_room_5.c", "unnamed_28.c", "set_03", "spr_12.tsv", "keyboard_j2.png", "a9_motion.s"] {
            assert!(named_by_number(name), "{name}");
        }
        for name in ["decompress_4bpp.s", "mul_fx.h", "iwram_ratio_q14.s", "m4a_interfaces.h", "vector3.h", "vale.c", "field"] {
            assert!(!named_by_number(name), "{name}");
        }
        let paths: Vec<String> = [
            "games/tla/include/actor.h",
            "games/tla/include/field/field.h",
            "games/tla/src/field/field.c",
            "games/tla/src/field/field/sprite_draw.c",
            "games/tla/src/field/field_internal.h",
            "games/tla/src/graphics/star_dot_00.png",
            "games/tla/src/graphics/star_dot_01.png",
            "games/tla/src/graphics/spr_12.png",
            "games/tla/src/graphics/status_grim_reaper_1.png",
            "games/tla/src/graphics/set_03/tiles.png",
            "games/tla/src/scene/jupiter_lighthouse_room_5.c",
            "games/tla/sound/songs/se_104.s",
        ]
        .map(String::from)
        .to_vec();
        let texts: Vec<(String, String)> = [
            ("games/tla/include/sounds.h", "enum SoundId {\n  SE_104,\n  /// SE_104's notes, higher.\n  SE_105,\n};\n"),
            ("games/tla/src/scene/vale.c", "  Sound_Play(SE_105);\n"),
            ("games/tla/sound/samples.s", "\t.global Sound_Wave50\nSound_Wave50:\n\t.global Sound_Wave52\nSound_Wave52:\n"),
            ("games/tla/sound/voices.s", "\tvoice 0, Sound_Wave52\n"),
        ]
        .map(|(path, text)| (path.to_string(), text.to_string()))
        .to_vec();
        let mut report = Report::default();
        let mut paths = paths;
        paths.extend(["games/tla/sound/songs/se_105.s", "games/tla/sound/samples/wave_50.pcm8.wav", "games/tla/sound/samples/wave_52.pcm8.wav"].map(String::from));
        count(&mut report, "tla", &paths, &["SPRITE_STATUS_GRIM_REAPER_1".to_string()], &numbered_symbols(&texts));
        let listed = |metric: &str| -> Vec<&str> { report.items.iter().filter(|item| item.metric == metric).map(|item| item.file.as_str()).collect() };
        assert_eq!(report.total("source_files"), 8);
        // include, include/field, src, src/field, src/field/field, src/graphics,
        // src/graphics/set_03, src/scene, sound, sound/songs, sound/samples.
        assert_eq!(report.total("folders"), 11);
        assert_eq!(listed("loose_headers"), ["games/tla/include/actor.h"]);
        assert_eq!(
            listed("numbered_files"),
            [
                "games/tla/src/graphics/spr_12.png",
                "games/tla/src/scene/jupiter_lighthouse_room_5.c",
                "games/tla/sound/songs/se_105.s",
                "games/tla/sound/samples/wave_52.pcm8.wav",
                "games/tla/src/graphics/set_03/",
            ],
            "star_dot_00 and _01 are frames of one set; a sprite has no symbol; SE_105 is played and Sound_Wave52 is a voice's"
        );
        assert_eq!(
            listed("numbered_files_unused"),
            ["games/tla/sound/songs/se_104.s", "games/tla/sound/samples/wave_50.pcm8.wav"],
            "written only where each is defined"
        );
        assert_eq!(listed("numbered_files_honest"), ["games/tla/src/graphics/status_grim_reaper_1.png"]);
        assert_eq!(listed("part_folders"), ["games/tla/src/field/field/"]);
        let listing = super::super::list(&[("tla", &report)], "part_folders");
        assert_eq!(listing, "tla\tgames/tla/src/field/field/\tfield\n", "a folder is listed by its path");
        assert!(super::super::listable("loose_headers") && super::super::listable("numbered_files"));
    }
}
