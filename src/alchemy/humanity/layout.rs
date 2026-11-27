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
    stem(name).split('_').any(|segment| {
        segment.bytes().any(|b| b.is_ascii_digit()) && !convention().is_match(segment)
    })
}

/// A frame of one set: a picture `NAME_NN.png` beside `NAME_00.png`, which the
/// build joins into `NAME.bitmaps` (the Makefile's `PICTURE_RUN`). Its number
/// is its place in the animation.
fn is_frame(path: &str, tracked: &HashSet<&str>) -> bool {
    let Some(base) = path.strip_suffix(".png") else {
        return false;
    };
    let Some((name, number)) = base.rsplit_once('_') else {
        return false;
    };
    number.len() == 2
        && number.bytes().all(|b| b.is_ascii_digit())
        && tracked.contains(format!("{name}_00.png").as_str())
}

/// Whether an `its number` row names the thing a file or folder is named
/// for: `status_grim_reaper_1.png` is `SPRITE_STATUS_GRIM_REAPER_1`'s
/// picture, `test_3` the debug area `SceneIdTest3`.
fn by_row(name: &str, rows: &[String]) -> bool {
    let flat = |text: &str| {
        text.chars()
            .filter(|c| *c != '_')
            .collect::<String>()
            .to_ascii_lowercase()
    };
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
    let ident =
        IDENT.get_or_init(|| Regex::new(r"[A-Za-z_][A-Za-z0-9_]*[0-9][A-Za-z0-9_]*").unwrap());
    let mut symbols: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
    for (file, (path, text)) in texts.iter().enumerate() {
        let text = if path.ends_with(".c") || path.ends_with(".h") {
            without_comments(text)
        } else {
            text.clone()
        };
        for (line, content) in text.lines().enumerate() {
            for found in ident.find_iter(content) {
                let before = content[..found.start()].chars().next_back();
                if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                    continue;
                }
                symbols
                    .entry(found.as_str().to_string())
                    .or_default()
                    .push((file, line));
            }
        }
    }
    symbols
}

/// Whether a numbered file is one nothing uses: it has a symbol in the
/// sources, its stem's words in capitals (`SE_120` for `se_120.mid`) or in
/// mixed case after a prefix (`SoundWave50` for `wave_50.pcm8.wav`), and
/// each of its symbols is written only where it is defined, on one line or
/// two together (`SE_120,` in `sounds.h`; `.global X` and `X:`). A file
/// with no symbol (a sprite kept by its row's number) cannot be shown
/// unused this way, and stays.
fn unused(name: &str, symbols: &HashMap<String, Vec<(usize, usize)>>) -> bool {
    let mut places: Vec<(usize, usize)> = symbols
        .iter()
        .filter(|(symbol, _)| writes(symbol, name))
        .flat_map(|(_, at)| at.iter().copied())
        .collect();
    places.sort_unstable();
    let Some(&(file, first)) = places.first() else {
        return false;
    };
    places
        .iter()
        .all(|&(other, line)| other == file && line <= first + 1)
}

/// What the game's text and songs show of the samples and maps it uses
/// (`uses`): each voice row (its group, its place in it, the sample or
/// instrument it names, and its file and line among the texts), each song
/// (its file name, the voice groups it names and the programs it selects),
/// the folder of each row of `gMaps` in order, and the rows a scene names.
#[derive(Default)]
pub struct Uses {
    voices: Vec<(String, usize, String, (usize, usize))>,
    songs: Vec<(String, BTreeSet<String>, BTreeSet<u8>)>,
    maps: Vec<String>,
    scene_maps: HashSet<usize>,
    /// The songs something plays, by file name.
    played: HashSet<String>,
}

/// A MIDI file's voice groups, from its first text event's header
/// (`SoundSeVoices`), and the programs its tracks select; `None` for a
/// file that is not MIDI.
pub fn midi_programs(bytes: &[u8]) -> Option<(BTreeSet<String>, BTreeSet<u8>)> {
    fn number(bytes: &[u8], at: &mut usize) -> Option<usize> {
        let mut value = 0;
        loop {
            let byte = *bytes.get(*at)?;
            *at += 1;
            value = (value << 7) | usize::from(byte & 0x7f);
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
    }
    static BANK: OnceLock<Regex> = OnceLock::new();
    let bank = BANK.get_or_init(|| Regex::new(r"Sound\w+Voices").unwrap());
    if bytes.get(..4)? != b"MThd" {
        return None;
    }
    let header = u32::from_be_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
    let mut at = 8 + header;
    let (mut banks, mut programs, mut text) = (BTreeSet::new(), BTreeSet::new(), None);
    while at + 8 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[at + 4..at + 8].try_into().ok()?) as usize;
        let body = bytes.get(at + 8..at + 8 + length)?;
        let track = &bytes[at..at + 4] == b"MTrk";
        at += 8 + length;
        if !track {
            continue;
        }
        let (mut i, mut running) = (0, 0u8);
        while i < body.len() {
            number(body, &mut i)?;
            let status = *body.get(i)?;
            if status == 0xff {
                let meta = *body.get(i + 1)?;
                i += 2;
                let size = number(body, &mut i)?;
                if meta == 1 && text.is_none() {
                    text = Some(String::from_utf8_lossy(body.get(i..i + size)?).into_owned());
                }
                i += size;
                continue;
            }
            if status == 0xf0 || status == 0xf7 {
                i += 1;
                let size = number(body, &mut i)?;
                i += size;
                continue;
            }
            if status & 0x80 != 0 {
                running = status;
                i += 1;
            }
            let kind = running & 0xf0;
            if kind == 0xc0 {
                programs.insert(*body.get(i)?);
            }
            i += if matches!(kind, 0xc0 | 0xd0) { 1 } else { 2 };
        }
    }
    banks.extend(
        bank.find_iter(text.as_deref().unwrap_or(""))
            .map(|found| found.as_str().to_string()),
    );
    Some((banks, programs))
}

/// What a game's texts (`symbol_texts`, as `numbered_symbols` numbers them)
/// and its songs (each file's path and bytes) show of its samples and maps:
/// the voice rows of `sound/voice_groups.s`, the songs' voice groups and
/// programs (a song in assembly that plays any track takes every program of
/// its group), the rows of `gMaps` by the folder each names
/// (`src/field/map_grid_data.c`) and the rows the scene table names
/// (`src/field/scenes.s`).
pub fn uses(texts: &[(String, String)], songs: &[(String, Vec<u8>)]) -> Uses {
    static VOICE: OnceLock<Regex> = OnceLock::new();
    static MAP: OnceLock<Regex> = OnceLock::new();
    let voice = VOICE.get_or_init(|| Regex::new(r"^\s*voice\s+(.*)$").unwrap());
    let map = MAP.get_or_init(|| Regex::new(r"^\s*\{.*\},\s*//\s*(\w+)\s*$").unwrap());
    let mut out = Uses::default();
    for (file, (path, text)) in texts.iter().enumerate() {
        if path.ends_with("sound/voice_groups.s") {
            let (mut group, mut place) = (String::new(), 0);
            for (line, content) in text.lines().enumerate() {
                if let Some(label) = content
                    .strip_suffix(':')
                    .filter(|label| !label.starts_with(['\t', ' ', '.']))
                {
                    (group, place) = (label.to_string(), 0);
                } else if let Some(found) = voice.captures(content) {
                    for target in found[1]
                        .split(',')
                        .map(str::trim)
                        .filter(|cell| cell.starts_with(|c: char| c.is_ascii_alphabetic()))
                    {
                        out.voices
                            .push((group.clone(), place, target.to_string(), (file, line)));
                    }
                    place += 1;
                }
            }
        } else if path.ends_with("src/field/map_grid_data.c") {
            out.maps.extend(
                text.lines()
                    .filter_map(|line| map.captures(line))
                    .map(|found| found[1].to_string()),
            );
        } else if path.ends_with("src/field/scenes.s") {
            for line in text
                .lines()
                .filter_map(|line| line.trim_start().strip_prefix("scene "))
            {
                if let Some(row) = line
                    .split(',')
                    .nth(4)
                    .and_then(|cell| cell.trim().parse().ok())
                {
                    out.scene_maps.insert(row);
                }
            }
        } else if path.contains("/sound/songs/") && path.ends_with(".s") {
            // A song written in assembly: its voice group, and every program
            // of it where it plays a track (its header's first byte).
            let tracks = text
                .lines()
                .find_map(|line| line.trim().strip_prefix(".byte "))
                .and_then(|bytes| bytes.split(',').next()?.trim().parse::<u32>().ok());
            let banks: BTreeSet<String> = text
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .filter(|word| word.starts_with("Sound") && word.ends_with("Voices"))
                .map(str::to_string)
                .collect();
            let programs = if tracks.unwrap_or(1) > 0 {
                (0..=255).collect()
            } else {
                BTreeSet::new()
            };
            out.songs.push((
                path.rsplit('/').next().unwrap_or(path).to_string(),
                banks,
                programs,
            ));
        }
    }
    for (path, bytes) in songs {
        if let Some((banks, programs)) = midi_programs(bytes) {
            out.songs.push((
                path.rsplit('/').next().unwrap_or(path).to_string(),
                banks,
                programs,
            ));
        }
    }
    // The songs something plays: a song's symbol (`SE_FLASH` for
    // `se_flash.mid`) written anywhere but where it is defined.
    static IDENT: OnceLock<Regex> = OnceLock::new();
    let ident = IDENT.get_or_init(|| Regex::new(r"[A-Za-z_][A-Za-z0-9_]*").unwrap());
    let wanted: HashMap<String, &str> = out
        .songs
        .iter()
        .map(|(song, ..)| (stem(song).to_ascii_uppercase(), song.as_str()))
        .collect();
    let mut places: HashMap<&str, Vec<(usize, usize)>> = HashMap::new();
    for (file, (path, text)) in texts.iter().enumerate() {
        let text = if path.ends_with(".c") || path.ends_with(".h") {
            without_comments(text)
        } else {
            text.clone()
        };
        for (line, content) in text.lines().enumerate() {
            for found in ident.find_iter(content) {
                if let Some(song) = wanted.get(found.as_str()) {
                    places.entry(song).or_default().push((file, line));
                }
            }
        }
    }
    out.played = places
        .into_iter()
        .filter(|(_, at)| {
            let (file, first) = at[0];
            !at.iter()
                .all(|&(other, line)| other == file && line <= first + 1)
        })
        .map(|(song, _)| song.to_string())
        .collect();
    out
}

/// Whether a numbered sample nothing plays: its symbol (`SoundWave52` for
/// `wave_52.pcm8.wav`) is written only where it is defined and in voice
/// rows, and no song the game plays selects any of those rows' programs in
/// their voice group (`SoundSeVoices`' row 94 only in `se_562.mid`, a sound
/// nothing plays).
fn unplayed(name: &str, symbols: &HashMap<String, Vec<(usize, usize)>>, uses: &Uses) -> bool {
    let rows: Vec<&(String, usize, String, (usize, usize))> = uses
        .voices
        .iter()
        .filter(|(_, _, target, _)| writes(target, name))
        .collect();
    if rows.is_empty() {
        return false;
    }
    let mut places: Vec<(usize, usize)> = symbols
        .iter()
        .filter(|(symbol, _)| writes(symbol, name))
        .flat_map(|(_, at)| at.iter().copied())
        .collect();
    places.retain(|place| !rows.iter().any(|row| row.3 == *place));
    places.sort_unstable();
    // What is left is its definition, on one line or two together.
    let defined = places.first().is_none_or(|&(file, first)| {
        places
            .iter()
            .all(|&(other, line)| other == file && line <= first + 1)
    });
    let played = |group: &str, place: usize| {
        uses.songs.iter().any(|(song, banks, programs)| {
            banks.contains(group)
                && programs.contains(&u8::try_from(place).unwrap_or(u8::MAX))
                && uses.played.contains(song)
        })
    };
    defined
        && rows
            .iter()
            .all(|(group, place, _, _)| !played(group, *place))
}

/// Whether `symbol` writes a file's stem: its words in capitals (`SE_120`
/// for `se_120.mid`), or in mixed case after a prefix (`SoundWave50` for
/// `wave_50.pcm8.wav`).
fn writes(symbol: &str, name: &str) -> bool {
    let stem = stem(name);
    let mixed: String = stem
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or(String::new(), |first| {
                first.to_ascii_uppercase().to_string() + chars.as_str()
            })
        })
        .collect();
    symbol == stem.to_ascii_uppercase()
        || symbol == mixed
        || symbol
            .strip_suffix(mixed.as_str())
            .is_some_and(|prefix| {
                prefix.starts_with(|c: char| c.is_ascii_uppercase())
                    && prefix.chars().all(|c| c.is_ascii_alphanumeric())
            })
}

/// Count a game's tracked files under `games/<game>/` (`paths`, each from
/// the checkout's root) into its report. `rows` are the names its
/// `evidence/<game>-names.tsv` gives the kind `its number`; `symbols` the
/// numbered identifiers its tracked text writes (`numbered_symbols`), by
/// which a numbered file nothing uses is set apart, and `uses` what its
/// voice rows, songs and scene table show: a sample heard only in sounds
/// nothing plays, and a map no scene names, are set apart too.
pub fn count(
    report: &mut Report,
    game: &str,
    paths: &[String],
    rows: &[String],
    symbols: &HashMap<String, Vec<(usize, usize)>>,
    uses: &Uses,
) {
    let root = format!("games/{game}/");
    let tracked: HashSet<&str> = paths.iter().map(String::as_str).collect();
    let mut folders = BTreeSet::new();
    for path in paths {
        let Some(inside) = path.strip_prefix(&root) else {
            continue;
        };
        let mut at = 0;
        while let Some(slash) = inside[at..].find('/') {
            folders.insert(&path[..root.len() + at + slash]);
            at += slash + 1;
        }
        let name = &path[path.rfind('/').map_or(0, |slash| slash + 1)..];
        let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
        if SOURCE.contains(&extension) {
            report.add("source_files", path, 1);
            if inside.starts_with("include/")
                && !inside["include/".len()..].contains('/')
                && extension == "h"
            {
                report.note("loose_headers", path, 0, name);
            }
        }
        if named_by_number(name) && !is_frame(path, &tracked) {
            let metric = if by_row(name, rows) {
                "numbered_files_honest"
            } else if unused(name, symbols) || unplayed(name, symbols, uses) {
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
            // A map's folder no scene's row of the scene table names.
            let map_unused = folder.contains("/src/map/")
                && uses
                    .maps
                    .iter()
                    .position(|map| map == name)
                    .is_some_and(|row| !uses.scene_maps.contains(&row));
            let metric = if by_row(name, rows) {
                "numbered_files_honest"
            } else if map_unused {
                "numbered_files_unused"
            } else {
                "numbered_files"
            };
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
        for name in [
            "map_108",
            "jupiter_lighthouse_room_5.c",
            "unnamed_28.c",
            "set_03",
            "spr_12.tsv",
            "keyboard_j2.png",
            "a9_motion.s",
        ] {
            assert!(named_by_number(name), "{name}");
        }
        for name in [
            "decompress_4bpp.s",
            "mul_fx.h",
            "iwram_ratio_q14.s",
            "m4a_interfaces.h",
            "vector3.h",
            "vale.c",
            "field",
        ] {
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
            (
                "games/tla/include/sounds.h",
                "enum SoundId {\n  SE_104,\n  /// SE_104's notes, higher.\n  SE_105,\n};\n",
            ),
            ("games/tla/src/scene/vale.c", "  Sound_Play(SE_105);\n"),
            (
                "games/tla/sound/samples.s",
                "\t.global SoundWave50\nSoundWave50:\n\t.global SoundWave52\nSoundWave52:\n",
            ),
            ("games/tla/sound/voices.s", "\tvoice 0, SoundWave52\n"),
        ]
        .map(|(path, text)| (path.to_string(), text.to_string()))
        .to_vec();
        let mut report = Report::default();
        let mut paths = paths;
        paths.extend(
            [
                "games/tla/sound/songs/se_105.s",
                "games/tla/sound/samples/wave_50.pcm8.wav",
                "games/tla/sound/samples/wave_52.pcm8.wav",
            ]
            .map(String::from),
        );
        count(
            &mut report,
            "tla",
            &paths,
            &["SPRITE_STATUS_GRIM_REAPER_1".to_string()],
            &numbered_symbols(&texts),
            &uses(&texts, &[]),
        );
        let listed = |metric: &str| -> Vec<&str> {
            report
                .items
                .iter()
                .filter(|item| item.metric == metric)
                .map(|item| item.file.as_str())
                .collect()
        };
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
            "star_dot_00 and _01 are frames of one set; a sprite has no symbol; SE_105 is played and SoundWave52 is a voice's"
        );
        assert_eq!(
            listed("numbered_files_unused"),
            [
                "games/tla/sound/songs/se_104.s",
                "games/tla/sound/samples/wave_50.pcm8.wav"
            ],
            "written only where each is defined"
        );
        assert_eq!(
            listed("numbered_files_honest"),
            ["games/tla/src/graphics/status_grim_reaper_1.png"]
        );
        assert_eq!(listed("part_folders"), ["games/tla/src/field/field/"]);
        let listing = super::super::list(&[("tla", &report)], "part_folders");
        assert_eq!(
            listing, "tla\tgames/tla/src/field/field/\tfield\n",
            "a folder is listed by its path"
        );
        assert!(
            super::super::listable("loose_headers") && super::super::listable("numbered_files")
        );
    }

    /// A MIDI file of one track whose header names `bank` and that selects
    /// `program`.
    fn midi(bank: &str, program: u8) -> Vec<u8> {
        let text = format!("tones={bank}");
        let mut track = vec![0, 0xff, 1, text.len() as u8];
        track.extend(text.bytes());
        track.extend([0, 0xc0, program, 0, 0x90, 60, 100, 0, 0xff, 0x2f, 0]);
        let mut bytes = b"MThd\0\0\0\x06\0\0\0\x01\0\x18MTrk".to_vec();
        bytes.extend((track.len() as u32).to_be_bytes());
        bytes.extend(track);
        bytes
    }

    #[test]
    fn samples_heard_only_in_sounds_nothing_plays_and_maps_no_scene_names() {
        assert_eq!(
            midi_programs(&midi("SoundSeVoices", 7)),
            Some((
                BTreeSet::from(["SoundSeVoices".to_string()]),
                BTreeSet::from([7])
            ))
        );
        let texts: Vec<(String, String)> = [
            ("games/tla/include/sounds.h", "enum SoundId {\n  SE_FLASH,\n  SE_562,\n};\n"),
            ("games/tla/src/field/flash.c", "void Flash(void) { Sound_Play(SE_FLASH); }\n"),
            ("games/tla/sound/direct_sound_samples.s", "\t.global SoundWave52\nSoundWave52:\n\t.global SoundWave64\nSoundWave64:\n"),
            (
                "games/tla/sound/voice_groups.s",
                "\t.global SoundSeVoices\nSoundSeVoices:\n\tvoice 0x00, 60, 0, 0, SoundWave52, 255, 0, 255, 226\n\tvoice 0x00, 60, 0, 0, SoundWave64, 255, 0, 255, 226\n",
            ),
            ("games/tla/src/field/map_grid_data.c", "struct MapRow gMaps[] = {\n    {1, 2, {3, 4, 5}, 6}, // vale\n    {7, 8, {9, 10, 11}, 12}, // map_290\n};\n"),
            ("games/tla/src/field/scenes.s", "\tscene Vale, 0x649, AREA_VALE, SCENE_TYPE_TOWN, 0, 0\n"),
        ]
        .map(|(path, text)| (path.to_string(), text.to_string()))
        .to_vec();
        let songs = [
            (
                "games/tla/sound/songs/se_562.mid".to_string(),
                midi("SoundSeVoices", 0),
            ),
            (
                "games/tla/sound/songs/se_flash.mid".to_string(),
                midi("SoundSeVoices", 1),
            ),
        ];
        let used = uses(&texts, &songs);
        let paths: Vec<String> = [
            "games/tla/sound/direct_sound_samples/wave_52.pcm8.wav",
            "games/tla/sound/direct_sound_samples/wave_64.pcm8.wav",
            "games/tla/src/map/map_290/map.txt",
        ]
        .map(String::from)
        .to_vec();
        let mut report = Report::default();
        count(
            &mut report,
            "tla",
            &paths,
            &[],
            &numbered_symbols(&texts),
            &used,
        );
        let listed = |metric: &str| -> Vec<&str> {
            report
                .items
                .iter()
                .filter(|item| item.metric == metric)
                .map(|item| item.what.as_str())
                .collect()
        };
        assert_eq!(
            listed("numbered_files_unused"),
            ["wave_52.pcm8.wav", "map_290"],
            "heard only in SE_562, which nothing plays; no scene's map"
        );
        assert_eq!(
            listed("numbered_files"),
            ["wave_64.pcm8.wav"],
            "heard in SE_FLASH"
        );
    }
}
