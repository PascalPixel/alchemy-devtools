//! Progress: DONE code and data from the twelve editions' linker maps
//! (calcrom), and the humanization report. The count and its policy are
//! `alchemy::done`, the report `alchemy::humanity`; this is the command line
//! that drives them. Both games reached 100%; the chart of how is README's
//! record, no longer drawn or published from here.

use crate::alchemy::done::{self, Game, GAMES};
use crate::alchemy::humanity;
use std::path::Path;

const USAGE: &str = "usage: alchemy-devtools progress [--humanity [--top N] [--list METRIC]]
With no option, counts DONE code and data in each edition built and matching
rom.sha1 (make compare-editions) and prints a line for each; a game without
all six editions is pending. Nothing is written, sent or published.
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

/// Both games' DONE, a line for each edition and one for the games.
fn measure(root: &Path) -> String {
    let games: [Result<Game, String>; 2] = std::thread::scope(|scope| {
        GAMES
            .map(|(game, _, _)| scope.spawn(move || done::measure_game(root, game)))
            .map(|thread| thread.join().expect("measurement thread"))
    });
    let combined = |game: &Result<Game, String>| game.as_ref().ok().map(Game::combined);
    format!("{}measured {}", done::lines_of(&games, &[0, 1]), done::status_line(combined(&games[0]), combined(&games[1])))
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

pub(crate) fn run(argv: &[String]) -> Result<(), String> {
    let root = crate::compile::root();
    let line = match argv {
        [flag] if flag == "-h" || flag == "--help" => USAGE.to_string(),
        [] => measure(root),
        [flag, options @ ..] if flag == "--humanity" => humanity_options(root, options)?,
        _ => return Err(format!("unrecognized arguments: {}\n{USAGE}", argv.join(" "))),
    };
    println!("{line}");
    Ok(())
}
