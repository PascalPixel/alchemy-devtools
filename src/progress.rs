//! Progress: DONE code and data from the twelve editions' linker maps
//! (calcrom), its publication, and the humanization report. The count and
//! its policy are `alchemy::done`, publication `alchemy::publish`, the
//! report `alchemy::humanity`; this is the command line that drives them.

use crate::alchemy::done::{self, Game, GameDone, GAMES};
use crate::alchemy::humanity;
use crate::alchemy::publish::{self, ci, decomp};
use std::path::Path;

const USAGE: &str = "usage: alchemy-devtools progress [--game GAME OUTPUT | --publish ARCHIVE COMMIT DIRECTORY MEASURED | --archive FILE | --fetch DIRECTORY | --volumes | --pre-push REMOTE | --humanity [--top N] [--list METRIC]]
With no option, counts DONE code and data in each edition built and matching
rom.sha1 (make compare-editions), prints a line for each, and writes the
measurement and decomp.dev's report under out/reports/decomp; a game without
all six editions is pending. Stage source changes before measuring: the
receipt must describe the source the build actually read. --pre-push sends
that measurement to the archive for the tip git pushes to main, when REMOTE
is this repository's. The archive is the decomp-reports release and its
numbered successors (decomp-reports-2 and on), since GitHub holds 1,000
assets in a release: --volumes lists them in order, and --archive adds FILE
to the newest under its own name, opening the next when it is full and
refusing a file a volume already holds changed. --fetch brings DIRECTORY level
with the archive's .tsv and .pb files: since no archived file ever changes, a
copy kept from an earlier run fetches only the files it lacks, and drops any
the archive no longer holds.
--game GAME OUTPUT counts one game's six editions into OUTPUT with no receipt
and no look at git: CI's own count, and a quick one on unstaged work.
--publish is CI's too: with MEASURED, CI's count of The Broken Seal, it
admits COMMIT after its successful build, checking its receipt's Broken Seal
row against MEASURED; without a receipt it admits MEASURED and The Lost
Age's figures from the nearest ancestor with a receipt, as that measurement
where everything The Lost Age's measurement reads is unchanged, else as its
last measurement. It rebuilds history from ARCHIVE's immutable baseline and
accepted records and draws the chart in DIRECTORY. Measurement dates are UTC.
--humanity counts what is left to humanize in each game's tracked C and
headers, from the source alone, prints a table and writes
out/reports/humanity.tsv; --top N also lists the N files leading each count,
and --list METRIC prints each counted thing of one of the table's metrics
instead (--list provenance is the name provenance worklist, --list
unused_word the words the game's text never uses, --list door_word the
words of exit names nothing resolves yet, and --list synonym,
synonym_referent, abbreviation, family_shape or prefix_twin the names a
reader could not guess from their siblings). FAKEMATCH devices and
Camelot's leftovers (the LEFTOVER tags and the // DEAD: marks) are counted
on rows of their own and never added together; --list fakematch and --list
leftover take every kind of each.";

/// Both games' DONE parts, where measured.
fn combined(games: &[Result<Game, String>; 2]) -> (Option<GameDone>, Option<GameDone>) {
    let combined = |game: &Result<Game, String>| game.as_ref().ok().map(Game::combined);
    (combined(&games[0]), combined(&games[1]))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn measure(root: &Path) -> Result<String, String> {
    let tree = publish::source_tree(root)?;
    let games = std::thread::scope(|scope| {
        GAMES
            .map(|(game, _, _)| scope.spawn(move || done::measure_game(root, game)))
            .map(|thread| thread.join().expect("measurement thread"))
    });
    let (sun, anchor) = combined(&games);
    let decomp = decomp::write(root, &games)?;
    let path = root.join(publish::MEASUREMENT);
    std::fs::create_dir_all(path.parent().expect("measurement directory"))
        .map_err(|error| format!("{}: {error}", publish::MEASUREMENT))?;
    write(&path, &done::measurement_text(sun, anchor))?;
    if sun.is_some() && anchor.is_some() {
        publish::prepare(root, &tree)?;
    }
    Ok(format!(
        "{}measured {} {decomp} measurement={}",
        done::lines_of(&games, &[0, 1]),
        done::status_line(sun, anchor),
        publish::MEASUREMENT
    ))
}

/// One game's six editions, which CI builds and measures for itself: the
/// measurement holds that game's row alone and no receipt, since CI's own
/// build is what it vouches for.
fn measure_one(root: &Path, game: &str, output: &Path) -> Result<String, String> {
    let at = GAMES
        .iter()
        .position(|(id, _, _)| *id == game)
        .ok_or_else(|| format!("--game takes tbs or tla, not {game}"))?;
    let measured = done::measure_game(root, game);
    let counted = measured.as_ref().map(Game::combined).map_err(|why| format!("{game} is not measured: {why}"))?;
    let mut games = [Err(String::from("not measured")), Err(String::from("not measured"))];
    games[at] = measured;
    let mut only = [None, None];
    only[at] = Some(counted);
    write(output, &done::measurement_text(only[0], only[1]))?;
    Ok(format!("{}measurement={}", done::lines_of(&games, &[at]), output.display()))
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
        [flag, archive, commit, directory, measured] if flag == "--publish" => ci::publish(
            root,
            Path::new(archive),
            commit,
            Path::new(directory),
            Path::new(measured),
        )?,
        [flag, game, output] if flag == "--game" => measure_one(root, game, Path::new(output))?,
        [flag, path] if flag == "--archive" => {
            publish::archive(root, Path::new(path))?;
            String::new()
        }
        [flag, options @ ..] if flag == "--humanity" => humanity_options(root, options)?,
        [flag, directory] if flag == "--fetch" => publish::fetch(root, Path::new(directory))?,
        [flag] if flag == "--volumes" => publish::volumes(root)?.join("\n"),
        [flag, remote] if flag == "--pre-push" => {
            pre_push(root, remote)?;
            String::new()
        }
        _ => return Err(format!("unrecognized arguments: {}\n{USAGE}", argv.join(" "))),
    };
    println!("{line}");
    Ok(())
}
