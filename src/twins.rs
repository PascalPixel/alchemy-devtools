//! twins: for each function a game has not yet matched, the matched function
//! of another game most like it in shape (`gba::shape`), so a draft can start
//! from C the other game already has. Both games are read from their English
//! builds: what an edition places from recon/ is unfinished, what it places
//! from games/ is matched.

use crate::alchemy::built;
use crate::alchemy::done::commas;
use crate::alchemy::targets::{self, DecompTargetId};
use crate::gba::elf::Elf;
use crate::gba::ld_map::Map;
use crate::gba::shape::{self, Index, Instruction, GRAM};
use std::path::Path;

const USAGE: &str = "usage: alchemy-devtools twins GAME [--all] [--min N] [--reference GAME] [--min-dice D] [--ratio LOW HIGH] [--limit N]
For each function GAME still places from recon/ (its listings and its
overlays' scaffold sections) with 12 or more instructions, the function the
reference game places from games/ most like it in shape; with --all, each
function GAME places from games/ instead, so a finished game's functions
find their counterparts (a function is never its own twin, so a game may be
its own reference). --min N reads functions of N or more instructions in
place of 12. Each Thumb
instruction reads as its mnemonic with its immediate, or the stack, literal
pool or pc it addresses; registers, branch targets, literal pools and jump
tables are set aside. What one compiler writes one way and another another
reads alike: a call through a register (a _call_via_rN veneer or a lone bl
suffix), a return (pop {pc}, or pop {rN} and bx rN), and a constant (read
from the pool, or built in its register by mov, lsl and add). Two functions
are as alike as the Dice coefficient of their sets of 5-instruction runs.
Each game is read from its English edition's ELFs and linker maps, the main
image and every overlay, so another edition's own forms are not: build both
first (make compare-tla-en compare-tbs-en). A game may be its own reference.
--reference defaults to the other game rom.sha1 lists. --ratio LOW HIGH
takes the twin among functions LOW to HIGH times as long; --min-dice D
leaves out functions whose twin is less alike than D; --limit N prints the
first N. Prints a header, then a tab-separated row a function, most alike
first: the function, its listing or scaffold, its length, its twin, the
twin's source, its length, and their Dice coefficient; - where no function
shares a run with it. A length counts instructions as they are read here. A
count of what was read goes to stderr.";

/// An unfinished function shorter than this is left out.
const MIN_LENGTH: usize = 12;

struct Options {
    game: &'static str,
    reference: &'static str,
    /// Read the functions GAME places from games/, not from recon/.
    all: bool,
    /// The fewest instructions a function read from GAME has.
    min_length: usize,
    min_dice: f64,
    ratio: Option<(f64, f64)>,
    limit: Option<usize>,
}

/// Whether an object of the game being read is one twins reads: from
/// games/ with --all, else from recon/.
fn reads(all: bool) -> impl Fn(&str) -> bool {
    let folder = if all { "games/" } else { "recon/" };
    move |stem: &str| stem.starts_with(folder)
}

fn number<T: std::str::FromStr>(value: Option<&String>, flag: &str) -> Result<T, String> {
    value.and_then(|value| value.parse().ok()).ok_or_else(|| format!("{flag} takes a number\n{USAGE}"))
}

/// The options, or None when help is asked for.
fn parse(arguments: &[String]) -> Result<Option<Options>, String> {
    let (mut game, mut reference, mut min_dice, mut ratio, mut limit) = (None, None, 0.0, None, None);
    let (mut all, mut min_length) = (false, MIN_LENGTH);
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-h" | "--help" => return Ok(None),
            "--all" => all = true,
            "--min" => min_length = number(arguments.next(), "--min")?,
            "--reference" => reference = Some(targets::game(arguments.next().ok_or_else(|| USAGE.to_string())?)?),
            "--min-dice" => min_dice = number(arguments.next(), "--min-dice")?,
            "--ratio" => {
                let (low, high): (f64, f64) = (number(arguments.next(), "--ratio")?, number(arguments.next(), "--ratio")?);
                if !(low > 0.0 && low <= high) {
                    return Err(format!("--ratio takes LOW and HIGH with 0 < LOW <= HIGH\n{USAGE}"));
                }
                ratio = Some((low, high));
            }
            "--limit" => limit = Some(number(arguments.next(), "--limit")?),
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}\n{USAGE}")),
            word if game.is_none() => game = Some(targets::game(word)?),
            _ => return Err(USAGE.into()),
        }
    }
    let game = game.ok_or_else(|| USAGE.to_string())?;
    let reference = match reference {
        Some(reference) => reference,
        None => match targets::games()?.into_iter().filter(|&other| other != game).collect::<Vec<_>>()[..] {
            [other] => other,
            _ => return Err(format!("name the reference game with --reference\n{USAGE}")),
        },
    };
    Ok(Some(Options { game, reference, all, min_length, min_dice, ratio, limit }))
}

/// A Thumb function an edition's build defines, with the file that placed it.
struct Found {
    name: String,
    file: String,
    instructions: Vec<Instruction>,
}

/// The English edition of `game`, and each image its build links, read: the
/// main image's ELF and map, then each overlay's.
fn images(root: &Path, game: &str) -> Result<(DecompTargetId, Vec<(Elf, Map)>), String> {
    let id = built::edition(game, "en").ok_or_else(|| format!("rom.sha1 lists no English edition of {game}"))?;
    let output = built::output(root, id);
    let unbuilt = |error: String| format!("{error}; build {id} first (make compare-{id})");
    let mut paths = vec![(output.join(format!("{id}.elf")), output.join(format!("{id}.map")))];
    for (name, elf) in built::overlay_files(root, id, "elf").map_err(unbuilt)? {
        paths.push((elf, output.join("overlays").join(format!("{name}.map"))));
    }
    let mut images = Vec::new();
    for (elf, map) in paths {
        let data = std::fs::read(&elf).map_err(|error| unbuilt(format!("{}: {error}", elf.display())))?;
        let parsed = Elf::parse(&data).map_err(|error| format!("{}: {error}", elf.display()))?;
        images.push((parsed, Map::parse(&built::read_text(&map).map_err(unbuilt)?)));
    }
    Ok((id, images))
}

/// Every Thumb function the English edition of `game` places from an object
/// whose source stem `keep` admits: the main image's, then each overlay's.
fn read(root: &Path, game: &str, keep: impl Fn(&str) -> bool) -> Result<(DecompTargetId, Vec<Found>), String> {
    let (id, images) = images(root, game)?;
    let mut found = Vec::new();
    for (elf, map) in &images {
        for function in shape::functions(elf, map, |placed| keep(&built::stem(id, &placed.path))) {
            let stem = built::stem(id, &function.placed.path);
            let file = built::source(root, &stem).unwrap_or(stem);
            found.push(Found { name: function.name, file, instructions: function.instructions });
        }
    }
    Ok((id, found))
}

/// One function's row: the function, and its twin with their Dice coefficient.
struct Row<'a> {
    function: &'a Found,
    twin: Option<(&'a Found, f64)>,
}

impl Row<'_> {
    fn dice(&self) -> f64 {
        self.twin.map_or(0.0, |(_, dice)| dice)
    }

    fn text(&self) -> String {
        let Found { name, file, instructions } = self.function;
        let twin = match self.twin {
            Some((twin, dice)) => format!("{}\t{}\t{}\t{dice:.3}", twin.name, twin.file, twin.instructions.len()),
            None => "-\t-\t-\t-".to_string(),
        };
        format!("{name}\t{file}\t{}\t{twin}", instructions.len())
    }
}

/// Each function's twin among `matched`, most alike first, then the
/// longest, then by name. A function is never its own twin: one of the same
/// name from the same file is passed over.
fn rows<'a>(unfinished: &'a [Found], matched: &'a [Found], ratio: Option<(f64, f64)>) -> Vec<Row<'a>> {
    let index = Index::new(matched.iter().map(|found| found.instructions.as_slice()));
    let mut rows: Vec<Row> = unfinished
        .iter()
        .map(|function| {
            let length = function.instructions.len() as f64;
            let admits = |twin: usize| {
                let twin = &matched[twin];
                let own = twin.name == function.name && twin.file == function.file;
                let size = twin.instructions.len() as f64;
                !own && ratio.is_none_or(|(low, high)| size >= low * length && size <= high * length)
            };
            let twin = index.nearest(&function.instructions, admits).map(|(twin, dice)| (&matched[twin], dice));
            Row { function, twin }
        })
        .collect();
    rows.sort_by(|a, b| {
        b.dice()
            .total_cmp(&a.dice())
            .then(b.function.instructions.len().cmp(&a.function.instructions.len()))
            .then(a.function.name.cmp(&b.function.name))
            .then(a.function.file.cmp(&b.function.file))
    });
    rows
}

/// Each function `game` has not finished, by name, and its twin among the
/// other game's matched functions: the twin's name and source, and their Dice
/// coefficient. For rank's twin column; empty when rom.sha1 lists one game.
pub(crate) fn index(root: &Path, game: &str) -> Result<std::collections::HashMap<String, (String, String, f64)>, String> {
    let others: Vec<&str> = targets::games()?.into_iter().filter(|&other| other != game).collect();
    let [reference] = others[..] else { return Ok(Default::default()) };
    let (_, mut unfinished) = read(root, game, |stem| stem.starts_with("recon/"))?;
    unfinished.retain(|function| function.instructions.len() >= MIN_LENGTH);
    let (_, matched) = read(root, reference, |stem| stem.starts_with("games/"))?;
    Ok(rows(&unfinished, &matched, None)
        .into_iter()
        .filter_map(|row| row.twin.map(|(twin, dice)| (row.function.name.clone(), (twin.name.clone(), twin.file.clone(), dice))))
        .collect())
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let Some(options) = parse(arguments)? else {
        println!("{USAGE}");
        return Ok(());
    };
    let root = crate::compile::root();
    let (game, mut unfinished) = read(root, options.game, reads(options.all))?;
    let read_all = unfinished.len();
    unfinished.retain(|function| function.instructions.len() >= options.min_length);
    let (reference, matched) = read(root, options.reference, |stem| stem.starts_with("games/"))?;
    let shaped = matched.iter().filter(|function| function.instructions.len() >= GRAM).count();
    eprintln!(
        "{game}: {} {} functions, {} of {} or more instructions; {reference}: {} matched functions indexed, {} of {GRAM} or more instructions",
        commas(read_all as i64),
        if options.all { "matched" } else { "unfinished" },
        commas(unfinished.len() as i64),
        options.min_length,
        commas(matched.len() as i64),
        commas(shaped as i64),
    );
    println!("function\tfile\tlength\ttwin\tsource\tlength\tdice");
    let rows = rows(&unfinished, &matched, options.ratio);
    let shown = rows.iter().filter(|row| options.min_dice <= 0.0 || row.dice() >= options.min_dice);
    for row in shown.take(options.limit.unwrap_or(usize::MAX)) {
        println!("{}", row.text());
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn the_reference_defaults_to_the_other_game() {
        let options = parse(&words("tla")).unwrap().unwrap();
        assert_eq!((options.game, options.reference, options.ratio, options.limit), ("tla", "tbs", None, None));
        let options = parse(&words("tbs --reference tbs --ratio 0.75 1.33 --min-dice 0.1 --limit 5")).unwrap().unwrap();
        assert_eq!((options.game, options.reference, options.ratio, options.min_dice, options.limit), ("tbs", "tbs", Some((0.75, 1.33)), 0.1, Some(5)));
        for wrong in ["", "gs", "tla tbs", "tla --ratio 2 1", "tla --ratio 1", "tla --limit", "tla --min", "tla --reference gs"] {
            assert!(parse(&words(wrong)).is_err(), "{wrong:?}");
        }
        assert!(parse(&words("tla --help")).unwrap().is_none());
        let options = parse(&words("tla")).unwrap().unwrap();
        assert_eq!((options.all, options.min_length), (false, MIN_LENGTH));
        let options = parse(&words("tla --all --min 30")).unwrap().unwrap();
        assert_eq!((options.all, options.min_length), (true, 30));
        assert!(parse(&words("tla --all --min")).is_err());
        assert!(parse(&words("tla --min many")).is_err());
    }

    /// With --all the functions a game places from games/ are read, and
    /// with the game its own reference none is its own twin.
    #[test]
    fn all_reads_the_matched_functions_and_none_is_its_own_twin() {
        let root = crate::compile::root();
        let (_, mut lost) = read(root, "tla", reads(true)).unwrap();
        lost.retain(|function| function.instructions.len() >= 40);
        assert!(lost.len() > 1_000, "{} functions of 40 or more instructions", lost.len());
        assert!(lost.iter().all(|function| function.file.starts_with("games/")));
        let (_, matched) = read(root, "tbs", reads(true)).unwrap();
        let across = rows(&lost, &matched, Some((0.75, 1.33)));
        let thor = across.iter().find(|row| row.function.name == "BattleEffect_RunThor").unwrap();
        assert_eq!(thor.twin.unwrap().0.name, "BattleEffect_RunThor");
        let own = rows(&matched, &matched, None);
        assert!(own.iter().filter(|row| row.twin.is_some()).count() > 4_000);
        for row in own {
            let twin = row.twin.map(|(twin, _)| (twin.name.as_str(), twin.file.as_str()));
            assert_ne!(twin, Some((row.function.name.as_str(), row.function.file.as_str())));
        }
    }

    /// Every Thumb function the reference build places from games/ is read
    /// with its instructions (twin.ts once read 133 of The Broken Seal's),
    /// and each is its own twin.
    #[test]
    fn every_matched_function_is_read_and_is_its_own_twin() {
        let root = crate::compile::root();
        let (id, images) = images(root, "tbs").unwrap();
        let mut defined = 0;
        for (elf, map) in &images {
            let mut starts: Vec<u32> = elf
                .symbols
                .iter()
                .filter(|symbol| symbol.kind == 2 && symbol.value & 1 != 0 && !matches!(symbol.section, 0 | 0xfff1))
                .filter(|symbol| !symbol.name.starts_with(['$', '.']))
                .map(|symbol| symbol.value & !1)
                .filter(|&start| map.at_address(start).is_some_and(|placed| built::stem(id, &placed.path).starts_with("games/")))
                .collect();
            starts.sort();
            starts.dedup();
            defined += starts.len();
        }
        let (_, matched) = read(root, "tbs", |stem| stem.starts_with("games/")).unwrap();
        assert_eq!(matched.len(), defined);
        let long = matched.iter().filter(|function| function.instructions.len() >= MIN_LENGTH).count();
        assert!(long > 4_000, "{long} of {defined} with {MIN_LENGTH} or more instructions");
        let index = Index::new(matched.iter().map(|found| found.instructions.as_slice()));
        for function in matched.iter().filter(|function| function.instructions.len() >= GRAM).step_by(10) {
            let dice = index.nearest(&function.instructions, |_| true).map(|(_, dice)| dice);
            assert_eq!(dice, Some(1.0), "{} in {}", function.name, function.file);
        }
    }

    /// The Lost Age's Thor, matched since, finds The Broken Seal's with
    /// nothing else near.
    #[test]
    fn thor_finds_its_twin_in_the_other_game() {
        let root = crate::compile::root();
        let (_, lost) = read(root, "tla", |_| true).unwrap();
        let (_, matched) = read(root, "tbs", |stem| stem.starts_with("games/")).unwrap();
        let thor = lost.iter().find(|function| function.name == "BattleEffect_RunThor").unwrap();
        let (twin, dice) = rows(std::slice::from_ref(thor), &matched, Some((0.75, 1.33)))[0].twin.unwrap();
        assert_eq!((twin.name.as_str(), twin.file.as_str()), ("BattleEffect_RunThor", "games/tbs/src/battle_effect/battle_effect.c"));
        assert!(dice > 0.3, "{dice}");
        let others: Vec<Found> = matched.into_iter().filter(|function| function.name != "BattleEffect_RunThor").collect();
        let next = rows(std::slice::from_ref(thor), &others, None)[0].dice();
        assert!(next < dice / 4.0, "{next} against {dice}");
    }
}
