//! The game's files and folders as targets of their own: how many source
//! files and folders there are, which headers lie loose in `include/`, which
//! files and folders are named by number, and which objects keep their parts
//! in a folder beside them (`src/field/field.c` and `src/field/field/`).

use super::Report;
use regex::Regex;
use std::collections::{BTreeSet, HashSet};
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

/// Count a game's tracked files under `games/<game>/` (`paths`, each from
/// the checkout's root) into its report. `rows` are the names its
/// `evidence/<game>-names.tsv` gives the kind `its number`.
pub fn count(report: &mut Report, game: &str, paths: &[String], rows: &[String]) {
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
            let metric = if by_row(name, rows) { "numbered_files_honest" } else { "numbered_files" };
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
        let mut report = Report::default();
        count(&mut report, "tla", &paths, &["SPRITE_STATUS_GRIM_REAPER_1".to_string()]);
        let listed = |metric: &str| -> Vec<&str> { report.items.iter().filter(|item| item.metric == metric).map(|item| item.file.as_str()).collect() };
        assert_eq!(report.total("source_files"), 7);
        // include, include/field, src, src/field, src/field/field, src/graphics,
        // src/graphics/set_03, src/scene, sound, sound/songs.
        assert_eq!(report.total("folders"), 10);
        assert_eq!(listed("loose_headers"), ["games/tla/include/actor.h"]);
        assert_eq!(
            listed("numbered_files"),
            [
                "games/tla/src/graphics/spr_12.png",
                "games/tla/src/scene/jupiter_lighthouse_room_5.c",
                "games/tla/sound/songs/se_104.s",
                "games/tla/src/graphics/set_03/",
            ],
            "star_dot_00 and _01 are frames of one set"
        );
        assert_eq!(listed("numbered_files_honest"), ["games/tla/src/graphics/status_grim_reaper_1.png"]);
        assert_eq!(listed("part_folders"), ["games/tla/src/field/field/"]);
        let listing = super::super::list(&[("tla", &report)], "part_folders");
        assert_eq!(listing, "tla\tgames/tla/src/field/field/\tfield\n", "a folder is listed by its path");
        assert!(super::super::listable("loose_headers") && super::super::listable("numbered_files"));
    }
}
