//! DONE code and data from the linker maps, as pret's calcrom counts them
//! (C2): every byte each image places, the ROM and each overlay in its
//! unpacked form, sorted by the folder of the object that placed it. Code
//! is the bytes of `.text` sections; from games/ it is done, as C when its
//! source is C, as assembly otherwise (8-byte far-call stubs and the
//! compiler library counted apart); from recon/ it is not yet. Every other
//! byte is data: done from games/, from the compiler library or as the
//! linker's fill, not yet from recon/. An overlay's packed stream in the
//! ROM is outside the totals. An unfinished edition's code total is the
//! larger of its own and the English build's: its code not yet matched may
//! still be bytes of its scaffold, which the English build has as listings.
//! An edition counts only when its ROM matches rom.sha1.

use super::targets;
use crate::gba::count::{self, Image};
use crate::gba::ld_map::Map;
use std::path::Path;

/// The two games: their id, mark and title.
pub(crate) const GAMES: [(&str, &str, &str); 2] = [("tbs", "☀️", "The Broken Seal"), ("tla", "⚓️", "The Lost Age")];

pub(crate) const LANGUAGES: [&str; 6] = ["ja", "en", "de", "es", "fr", "it"];

/// One game's DONE parts, summed over its six editions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GameDone {
    /// Code from the tree both games once shared: none since each game owns
    /// its copy, kept so the measurements already stored still read.
    pub common_asm: i64,
    pub common_c: i64,
    /// Assembly from the game's folder, with the stubs and the library.
    pub game_asm: i64,
    pub game_c: i64,
    pub executable: i64,
    pub veneers: i64,
    /// DONE data: non-code bytes placed from games/, the library or fill,
    /// out of every non-code byte the images place.
    pub data: i64,
    pub data_total: i64,
}

impl GameDone {
    pub fn parts(&self) -> (f64, f64, f64) {
        (
            floor_percent(self.common_c + self.game_c, self.executable),
            floor_percent(self.common_asm + self.game_asm - self.veneers, self.executable),
            floor_percent(self.veneers, self.executable),
        )
    }
    pub fn bytes(&self) -> i64 {
        self.common_asm + self.common_c + self.game_asm + self.game_c
    }
    pub fn percent(&self) -> f64 {
        floor_percent(self.bytes(), self.executable)
    }
    pub fn data_percent(&self) -> f64 {
        floor_percent(self.data, self.data_total)
    }
    /// A game is 100% only when both its code and its data are.
    pub fn complete(&self) -> bool {
        self.executable > 0 && self.bytes() == self.executable && self.data == self.data_total
    }
}

impl std::ops::AddAssign for GameDone {
    fn add_assign(&mut self, other: GameDone) {
        self.common_asm += other.common_asm;
        self.common_c += other.common_c;
        self.game_asm += other.game_asm;
        self.game_c += other.game_c;
        self.executable += other.executable;
        self.veneers += other.veneers;
        self.data += other.data;
        self.data_total += other.data_total;
    }
}

/// One edition's count, with the compiler library apart for the report.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Counted {
    pub done: GameDone,
    pub library: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct Game {
    pub editions: Vec<(&'static str, Counted)>,
}

impl Game {
    pub fn combined(&self) -> GameDone {
        let mut combined = GameDone::default();
        for (_, edition) in &self.editions {
            combined += edition.done;
        }
        combined
    }
}

/// A share in hundredths of a percent, rounded down.
pub fn floor_percent(n: i64, d: i64) -> f64 {
    if d == 0 {
        0.0
    } else {
        (n as i128 * 10_000 / d as i128) as f64 / 100.0
    }
}

/// A count with a comma every three digits.
pub fn commas(value: i64) -> String {
    let text = value.to_string();
    let (sign, digits) = text.strip_prefix('-').map_or(("", text.as_str()), |value| ("-", value));
    let mut out = String::from(sign);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[derive(Default)]
struct Tally {
    span: i64,
    streams: i64,
    code: i64,
    data_todo: i64,
    counted: Counted,
}

/// An 8-byte far-call stub: a far-call bank, or an overlay's entry or
/// import table.
fn stub(stem: &str) -> bool {
    stem.contains("/far_call/")
        || stem.contains("/entry/")
        || stem.contains("/import/")
        || stem.ends_with("/entry")
        || stem.ends_with("/import")
}

/// Counts one linker map by the folder of each object that placed bytes.
fn tally(root: &Path, map: &str, image: Image, tally: &mut Tally) {
    let map = Map::parse(map);
    for placed in count::placed(&map, image) {
        let (name, size, object) = (placed.name.as_str(), i64::from(placed.size), placed.path.as_str());
        if name == ".overlays" {
            tally.streams += size;
            continue;
        }
        let stem = super::built::object_stem(object);
        let games = stem.is_some_and(|stem| stem.starts_with("games/"));
        let library = object.contains("libgcc.a");
        let own = object.contains("/overlays/resource_");
        let done = &mut tally.counted.done;
        if !name.starts_with(".text") {
            if !(games || library || own) {
                tally.data_todo += size;
            }
            continue;
        }
        tally.code += size;
        match stem {
            _ if library => {
                done.game_asm += size;
                tally.counted.library += size;
            }
            Some(stem) if games => {
                if root.join(format!("{stem}.c")).is_file() {
                    done.game_c += size;
                } else {
                    done.game_asm += size;
                    if stub(stem) {
                        done.veneers += size;
                    }
                }
            }
            _ => {}
        }
    }
    tally.span += count::span(&map, image);
}

/// Whether an edition's ROM is the one rom.sha1 names.
pub(crate) fn verified(root: &Path, edition: &str) -> Result<(), String> {
    let target = targets::decomp_target(Some(edition))?;
    let rom = target.image();
    let bytes = std::fs::read(root.join(&rom)).map_err(|_| format!("{rom} is not built: make compare-{edition}"))?;
    if !target.matches(&bytes) {
        return Err(format!("{rom} does not match rom.sha1"));
    }
    Ok(())
}

pub(crate) fn measure_edition(root: &Path, edition: &str) -> Result<Counted, String> {
    verified(root, edition)?;
    count_edition(root, edition)
}

/// One edition counted from its maps alone, whatever its ROM: what `where
/// --done` shows of a tree in progress. A measurement is `measure_edition`.
pub(crate) fn count_edition(root: &Path, edition: &str) -> Result<Counted, String> {
    let mut counted = Tally::default();
    let main = root.join(format!("out/{edition}/{edition}.map"));
    let text = std::fs::read_to_string(&main).map_err(|error| format!("{}: {error}", main.display()))?;
    tally(root, &text, Image::Rom, &mut counted);
    let mut maps: Vec<_> = std::fs::read_dir(root.join(format!("out/{edition}/overlays")))
        .map_err(|error| format!("out/{edition}/overlays: {error}"))?
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "map"))
        .collect();
    maps.sort();
    for map in maps {
        let text = std::fs::read_to_string(&map).map_err(|error| format!("{}: {error}", map.display()))?;
        tally(root, &text, Image::Linked(super::overlay::SECTIONS), &mut counted);
    }
    let done = &mut counted.counted.done;
    done.executable = counted.code;
    done.data_total = counted.span - counted.code - counted.streams;
    done.data = done.data_total - counted.data_todo;
    Ok(counted.counted)
}

/// A game's six editions, or why it is pending.
pub(crate) fn measure_game(root: &Path, game: &str) -> Result<Game, String> {
    count_game(root, game, measure_edition)
}

/// A game's six editions, each counted by `count`.
pub(crate) fn count_game(
    root: &Path,
    game: &str,
    count: fn(&Path, &str) -> Result<Counted, String>,
) -> Result<Game, String> {
    let mut editions = Vec::new();
    for language in LANGUAGES {
        editions.push((language, count(root, &format!("{game}-{language}"))?));
    }
    let english = editions[1].1.done.executable;
    for (_, counted) in &mut editions {
        if !counted.done.complete() {
            counted.done.executable = counted.done.executable.max(english);
        }
    }
    Ok(Game { editions })
}

/// Each game's code and data shares, or pending.
pub(crate) fn status_line(sun: Option<GameDone>, anchor: Option<GameDone>) -> String {
    let show = |done: Option<GameDone>| {
        done.map_or("pending".to_string(), |d| format!("code {:.2}% data {:.2}%", d.percent(), d.data_percent()))
    };
    format!("{} {} · {} {}", GAMES[0].1, show(sun), GAMES[1].1, show(anchor))
}

/// The lines of the games at `which`: each game's total, then a line for
/// each edition.
pub(crate) fn lines_of(games: &[Result<Game, String>; 2], which: &[usize]) -> String {
    let mut text = String::new();
    for ((_, mark, name), game) in which.iter().map(|&at| (GAMES[at], &games[at])) {
        let Ok(game) = game else {
            text += &format!("{mark} {name}: pending ({})\n", game.as_ref().err().unwrap());
            continue;
        };
        let all = game.combined();
        let (c, assembly, stubs) = all.parts();
        text += &format!(
            "{mark} {name} DONE {}: code {:.2}% ({} / {} bytes: C {c:.2} + assembly {assembly:.2} + stubs {stubs:.2}) · data {:.2}% ({} / {} bytes), six editions\n",
            if all.complete() { "100%" } else { "not yet" },
            all.percent(),
            commas(all.bytes()),
            commas(all.executable),
            all.data_percent(),
            commas(all.data),
            commas(all.data_total),
        );
        for (language, counted) in &game.editions {
            let d = counted.done;
            text += &format!(
                "{mark}   {language} code {:.2}% ({} / {}) · data {:.2}% ({} / {})\n",
                d.percent(),
                commas(d.bytes()),
                commas(d.executable),
                d.data_percent(),
                commas(d.data),
                commas(d.data_total),
            );
        }
    }
    text
}

const MEASUREMENT_HEADER: &str = "game\tcommon_asm\tcommon_c\tgame_asm\tgame_c\texecutable\tveneers\tdata\tdata_total";

/// The measurement a count writes and the archive keeps: a row for each
/// game measured.
pub(crate) fn measurement_text(sun: Option<GameDone>, anchor: Option<GameDone>) -> String {
    let mut text = format!("{MEASUREMENT_HEADER}\n");
    for ((game, _, _), done) in GAMES.into_iter().zip([sun, anchor]) {
        if let Some(d) = done {
            text += &format!(
                "{game}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                d.common_asm, d.common_c, d.game_asm, d.game_c, d.executable, d.veneers, d.data, d.data_total
            );
        }
    }
    text
}

pub(crate) fn parse_measurement(text: &str) -> Result<[Option<GameDone>; 2], String> {
    let mut lines = text.lines();
    if lines.next() != Some(MEASUREMENT_HEADER) {
        return Err("measurement has an unexpected header".into());
    }
    let mut games = [None, None];
    for line in lines {
        let fields = line.split('\t').collect::<Vec<_>>();
        let at = fields
            .first()
            .and_then(|name| GAMES.iter().position(|(game, _, _)| game == name))
            .ok_or_else(|| format!("measurement row names no game: {line}"))?;
        let values = fields[1..]
            .iter()
            .map(|field| field.parse::<i64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| format!("measurement row is not numeric: {line}"))?;
        let [common_asm, common_c, game_asm, game_c, executable, veneers, data, data_total] = values[..] else {
            return Err(format!("measurement row has the wrong width: {line}"));
        };
        let code = values[..4].iter().map(|value| *value as i128).sum::<i128>();
        if values.iter().any(|value| *value < 0)
            || executable == 0
            || code > executable as i128
            || veneers as i128 > common_asm as i128 + game_asm as i128
        {
            return Err(format!("measurement row has impossible code: {line}"));
        }
        if data > data_total {
            return Err(format!("measurement row has impossible data: {line}"));
        }
        if games[at].is_some() {
            return Err(format!("measurement names {} twice", fields[0]));
        }
        games[at] = Some(GameDone { common_asm, common_c, game_asm, game_c, executable, veneers, data, data_total });
    }
    Ok(games)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: &str = "\
Linker script and memory map

ewram           0x02000000    0x38000
 .sym_ewram     0x02000000    0x38000 out/x/obj/games/tbs/src/system/ewram.o
.rom_start      0x08000000      0x200
 .text          0x08000000      0x100 out/x/obj/games/tbs/src/main.o
 .text          0x08000100       0x40 out/x/obj/games/tbs/src/gamepak/far_call/SYSTEM.o
 .rodata        0x08000140       0x30 out/x/obj/games/tbs/src/main.o
 *fill*         0x08000170       0x10
 .rom.00000180  0x08000180       0x40 out/x/obj/recon/tla/ja/rom.o
 .text          0x080001c0       0x20 out/x/obj/recon/tla/raw/080001c0.o
 .overlays      0x080001e0       0x20 out/x/obj/games/tbs/src/system/overlay/overlays.o
.iwram_resident
                0x03000000       0x20 load address 0x08000200
 .text          0x03000000       0x20 out/x/obj/games/tbs/src/system/runtime.o
";

    #[test]
    fn a_map_counts_by_folder() {
        let mut counted = Tally::default();
        tally(Path::new("/nowhere"), MAP, Image::Rom, &mut counted);
        let done = counted.counted.done;
        // The wrapped .iwram_resident heading still places its code.
        assert_eq!(counted.span, 0x220);
        assert_eq!(counted.code, 0x180);
        assert_eq!((done.game_asm, done.veneers), (0x160, 0x40));
        assert_eq!(counted.streams, 0x20);
        assert_eq!(counted.data_todo, 0x40);
    }

    #[test]
    fn shares_round_down_to_hundredths() {
        assert_eq!(floor_percent(500, 1000), 50.0);
        assert_eq!(floor_percent(632_006, 1_354_302), 46.66);
        assert_eq!(floor_percent(249_266, 1_622_410), 15.36);
        assert_eq!(floor_percent(9_999, 10_000), 99.99);
        assert_eq!(floor_percent(2, 0), 0.0);
        assert_eq!(commas(-1_622_410), "-1,622,410");
    }

    #[test]
    fn status_shows_each_verified_game_or_pending() {
        let sun = GameDone {
            game_c: 250,
            game_asm: 340,
            executable: 1000,
            data: 992,
            data_total: 1000,
            ..GameDone::default()
        };
        assert_eq!(
            status_line(Some(sun), None),
            "☀️ code 59.00% data 99.20% · ⚓️ pending"
        );
        let anchor = GameDone {
            game_c: 10,
            game_asm: 50,
            veneers: 30,
            executable: 1000,
            ..GameDone::default()
        };
        assert_eq!(
            status_line(Some(sun), Some(anchor)),
            "☀️ code 59.00% data 99.20% · ⚓️ code 6.00% data 0.00%"
        );
    }

    #[test]
    fn a_measurement_survives_its_trip_to_ci() {
        let sun = GameDone {
            common_asm: 1,
            common_c: 2,
            game_asm: 3,
            game_c: 4,
            executable: 100,
            veneers: 1,
            data: 7,
            data_total: 9,
        };
        let text = measurement_text(Some(sun), None);
        assert_eq!(parse_measurement(&text).unwrap(), [Some(sun), None]);
        assert!(parse_measurement("game\n").is_err());
        assert!(parse_measurement(&format!("{text}tbs\t1\t2\t3\t4\t100\t1\t0\t0\n")).is_err());
        assert!(parse_measurement(&format!("{text}tla\t1\t2\n")).is_err());
        assert!(parse_measurement(&text.replace("\t7\t9", "\t9\t7")).is_err());
        assert!(parse_measurement(&text.replace("\t100\t", "\t0\t")).is_err());
        assert!(parse_measurement(&text.replace("\t100\t", "\t5\t")).is_err());
        assert!(parse_measurement(&text.replace("tbs\t1\t", "tbs\t-1\t")).is_err());
        // Every archived measurement has its data columns; one without is refused.
        let code_only = "game\tcommon_asm\tcommon_c\tgame_asm\tgame_c\texecutable\tveneers\n\
                         tla\t1\t2\t3\t4\t100\t1\n";
        assert!(parse_measurement(code_only).is_err());
    }
}
