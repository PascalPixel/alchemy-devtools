//! Progress: DONE code and data from the twelve editions' linker maps
//! (calcrom), the report decomp.dev reads, and the figures CI draws.

pub(crate) mod boxtree;
pub(crate) mod archive;
pub(crate) mod calcrom;
pub(crate) mod clex;
pub(crate) mod decomp;
pub(crate) mod figure;
pub(crate) mod history;
pub(crate) mod humanity;
pub(crate) mod jsnum;
pub(crate) mod naming;
pub(crate) mod provenance;
pub(crate) mod letters;
pub(crate) mod model;
pub(crate) mod palette;
pub(crate) mod publish;
pub(crate) mod raster;

use calcrom::{Game, GameDone};
use std::path::Path;

const USAGE: &str = "usage: alchemy-devtools progress [--game GAME OUTPUT | --publish ARCHIVE COMMIT DIRECTORY MEASURED | --upload COMMIT REMOTE | --pre-push REMOTE | --humanity [--top N] [--list METRIC]]
With no option, counts DONE code and data in each edition built and matching
rom.sha1 (make compare-editions), prints a line for each, and writes the
measurement and decomp.dev's report under out/reports/decomp; a game without
all six editions is pending. --upload sends that measurement to the
decomp-reports release for COMMIT, when REMOTE is this repository's;
--pre-push does so for the tip git pushes to main.
Stage source changes before measuring: the receipt must describe the source
the build actually read. --game GAME OUTPUT is CI's own count of one game's
six editions, written to OUTPUT with no receipt. --publish is CI's too: with
MEASURED, CI's count of The Broken Seal, it admits COMMIT after its
successful build, checking its receipt's Broken Seal row against MEASURED;
without a receipt it admits MEASURED and The Lost Age's figures from the
nearest ancestor with a receipt, as that measurement where everything The
Lost Age's measurement reads is unchanged, else as its last measurement. It
rebuilds history from ARCHIVE's immutable baseline and accepted records and
draws both figures in DIRECTORY. Measurement dates are UTC.
--humanity counts what is left to humanize in each game's tracked C and
headers, from the source alone (README), prints a table and writes
out/reports/humanity.tsv; --top N also lists the N files leading each count,
and --list METRIC prints each counted thing of one TSV metric instead
(--list provenance is the name provenance worklist, --list unused_word the
words the game's text never uses, and --list synonym, synonym_referent,
abbreviation, family_shape or prefix_twin the names a
reader could not guess from their siblings).";

fn read(path: &Path) -> Result<String, String> {
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

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

pub fn floor_percent(n: i64, d: i64) -> f64 {
    if d == 0 {
        0.0
    } else {
        (n as i128 * 10_000 / d as i128) as f64 / 100.0
    }
}

fn done(games: &[Result<Game, String>; 2]) -> (Option<GameDone>, Option<GameDone>) {
    let done = |game: &Result<Game, String>| game.as_ref().ok().map(Game::combined);
    (done(&games[0]), done(&games[1]))
}

fn status_line(sun: Option<GameDone>, anchor: Option<GameDone>) -> String {
    let show = |done: Option<GameDone>| {
        done.map_or("pending".to_string(), |d| format!("code {:.2}% data {:.2}%", d.percent(), d.data_percent()))
    };
    format!("☀️ {} · ⚓️ {}", show(sun), show(anchor))
}

/// A line for each edition, then each game's total.
fn lines(games: &[Result<Game, String>; 2]) -> String {
    lines_of(games, &[0, 1])
}

/// The lines of the games at `which`.
fn lines_of(games: &[Result<Game, String>; 2], which: &[usize]) -> String {
    let mut text = String::new();
    let names = [("☀️", "The Broken Seal"), ("⚓️", "The Lost Age")];
    for ((mark, name), game) in which.iter().map(|&at| (names[at], &games[at])) {
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
const CODE_ONLY_HEADER: &str = "game\tcommon_asm\tcommon_c\tgame_asm\tgame_c\texecutable\tveneers";

fn measurement_text(sun: Option<GameDone>, anchor: Option<GameDone>) -> String {
    let mut text = format!("{MEASUREMENT_HEADER}\n");
    for (game, done) in [("tbs", sun), ("tla", anchor)] {
        if let Some(d) = done {
            text += &format!(
                "{game}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                d.common_asm, d.common_c, d.game_asm, d.game_c, d.executable, d.veneers, d.data, d.data_total
            );
        }
    }
    text
}

fn parse_measurement(text: &str) -> Result<[Option<GameDone>; 2], String> {
    let mut lines = text.lines();
    let width = match lines.next() {
        Some(MEASUREMENT_HEADER) => 8,
        Some(CODE_ONLY_HEADER) => 6,
        _ => return Err("measurement has an unexpected header".into()),
    };
    let mut games = [None, None];
    for line in lines {
        let fields = line.split('\t').collect::<Vec<_>>();
        let at = match fields.first() {
            Some(&"tbs") => 0,
            Some(&"tla") => 1,
            _ => return Err(format!("measurement row names no game: {line}")),
        };
        let values = fields[1..]
            .iter()
            .map(|field| field.parse::<i64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| format!("measurement row is not numeric: {line}"))?;
        if values.len() != width {
            return Err(format!("measurement row has the wrong width: {line}"));
        }
        let code = values[..4].iter().map(|value| *value as i128).sum::<i128>();
        if values.iter().any(|value| *value < 0)
            || values[4] == 0
            || code > values[4] as i128
            || values[5] as i128 > values[0] as i128 + values[2] as i128
        {
            return Err(format!("measurement row has impossible code: {line}"));
        }
        let (data, data_total) = (values.get(6).copied().unwrap_or_default(), values.get(7).copied().unwrap_or_default());
        if data < 0 || data > data_total {
            return Err(format!("measurement row has impossible data: {line}"));
        }
        if games[at].is_some() {
            return Err(format!("measurement names {} twice", fields[0]));
        }
        games[at] = Some(GameDone {
            common_asm: values[0],
            common_c: values[1],
            game_asm: values[2],
            game_c: values[3],
            executable: values[4],
            veneers: values[5],
            data,
            data_total,
        });
    }
    Ok(games)
}

fn measure(root: &Path) -> Result<String, String> {
    let tree = publish::source_tree(root)?;
    let games = std::thread::scope(|scope| {
        ["tbs", "tla"]
            .map(|game| scope.spawn(move || calcrom::measure_game(root, game)))
            .map(|thread| thread.join().expect("measurement thread"))
    });
    let (sun, anchor) = done(&games);
    let decomp = decomp::write(root, [("tbs", &games[0]), ("tla", &games[1])])?;
    let path = root.join(publish::MEASUREMENT);
    std::fs::create_dir_all(path.parent().expect("measurement directory"))
        .map_err(|error| format!("{}: {error}", publish::MEASUREMENT))?;
    write(&path, &measurement_text(sun, anchor))?;
    if sun.is_some() && anchor.is_some() {
        publish::prepare(root, &tree)?;
    }
    Ok(format!("{}measured {} {decomp} measurement={}", lines(&games), status_line(sun, anchor), publish::MEASUREMENT))
}

/// One game's six editions, which CI builds and measures for itself: the
/// measurement holds that game's row alone and no receipt, since CI's own
/// build is what it vouches for.
fn measure_one(root: &Path, game: &str, output: &Path) -> Result<String, String> {
    let at = match game {
        "tbs" => 0,
        "tla" => 1,
        _ => return Err(format!("--game takes tbs or tla, not {game}")),
    };
    let measured = calcrom::measure_game(root, game);
    let done = measured.as_ref().map(Game::combined).map_err(|why| format!("{game} is not measured: {why}"))?;
    let mut games = [Err(String::from("not measured")), Err(String::from("not measured"))];
    games[at] = measured;
    let mut only = [None, None];
    only[at] = Some(done);
    write(output, &measurement_text(only[0], only[1]))?;
    Ok(format!("{}measurement={}", lines_of(&games, &[at]), output.display()))
}

/// `--humanity`'s options: `--top N` and `--list METRIC`.
fn humanity_options(root: &Path, options: &[String]) -> Result<String, String> {
    let (mut top, mut metric) = (0, None);
    let mut options = options.iter();
    while let Some(option) = options.next() {
        let value = options.next().ok_or_else(|| format!("{option} takes a value\n{USAGE}"))?;
        match option.as_str() {
            "--top" => top = value.parse().map_err(|_| format!("--top takes a number, not {value}"))?,
            "--list" => metric = Some(value.as_str()),
            _ => return Err(format!("unrecognized --humanity option: {option}\n{USAGE}")),
        }
    }
    humanity::run(root, top, metric)
}

/// The pre-push hook: the tip pushed to main carries its measurement,
/// uploaded from the lines git writes on the hook's input (C3).
fn pre_push(root: &Path, remote: &str) -> Result<(), String> {
    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input).map_err(|error| error.to_string())?;
    for line in input.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if let [_, commit, "refs/heads/main", _] = fields.as_slice() {
            if commit.bytes().any(|byte| byte != b'0') {
                publish::upload(root, commit, remote)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn run(argv: &[String]) -> Result<(), String> {
    let root = crate::compile::root();
    let line = match argv {
        [flag] if flag == "-h" || flag == "--help" => USAGE.to_string(),
        [] => measure(root)?,
        [flag, archive, commit, directory, measured] if flag == "--publish" => archive::publish(
            root,
            Path::new(archive),
            commit,
            Path::new(directory),
            Path::new(measured),
        )?,
        [flag, game, output] if flag == "--game" => measure_one(root, game, Path::new(output))?,
        [flag, commit, remote] if flag == "--upload" => {
            publish::upload(root, commit, remote)?;
            String::new()
        }
        [flag, options @ ..] if flag == "--humanity" => humanity_options(root, options)?,
        [flag, remote] if flag == "--pre-push" => {
            pre_push(root, remote)?;
            String::new()
        }
        _ => return Err(format!("unrecognized arguments: {}\n{USAGE}", argv.join(" "))),
    };
    println!("{line}");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::{measurement_text, parse_measurement, status_line};
    use crate::progress::calcrom::GameDone;

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
        // A code-only measurement from before DONE data still reads.
        let old = "game\tcommon_asm\tcommon_c\tgame_asm\tgame_c\texecutable\tveneers\n\
                   tla\t1\t2\t3\t4\t100\t1\n";
        let [None, Some(anchor)] = parse_measurement(old).unwrap() else {
            panic!("a code-only measurement reads")
        };
        assert_eq!((anchor.bytes(), anchor.data_total), (10, 0));
    }
}
