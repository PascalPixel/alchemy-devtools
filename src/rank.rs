//! rank: every unfinished piece of a game, furthest first, the worklist a
//! round of matching takes its head from. `inventory` finds the pieces in
//! the tree and the built objects, `measure` compiles and compares their
//! drafts as diff does; this orders, filters and prints them.

mod inventory;
mod measure;

use crate::alchemy::built;
use crate::alchemy::done::commas;
use crate::alchemy::layout::Layout;
use crate::alchemy::overlay;
use crate::alchemy::targets::{self, target_for, DecompTarget};
use crate::editions::quote;
use inventory::{Kind, Reference};
use measure::{Measure, Row};
use std::path::Path;

const USAGE: &str = r#"usage: alchemy-devtools rank [--game GAME] [--kind main|arm|overlay|edition]... [--state STATE]... [--limit N] [--jobs N] [--json | --out FILE]
Lists every unfinished piece of a game, furthest first: each English listing
of the main image (kind main, or main-arm for ARM), each section of an
overlay scaffold English still assembles (overlay), and each function
another edition still takes from its scaffold while English links it as C,
or an overlay section it assembles to bytes of its own (edition). Build
every edition first: the listings, scaffolds and recipes are its build's.
Each draft compiles once in each edition it is measured in, as diff
compiles it, on --jobs threads (default: the processors less two), and is
compared with its listing as diff compares them: differ, and the skeleton
gap. A listing with no draft of its own is measured with the draft that
holds it: one whose opening comment names it among the addresses to
measure together (0819284c,08192894,...), or a drafted neighbour within
five listings whose comparison comes at least 70% of the listing's size
closer with it added. Overlay copies of one shape (the same instructions,
calling and reading the same names and constants) are one family, measured
once from the copy with a draft, and so are the editions whose comparison
is the same instruction for instruction.
Pieces with no draft come first, by size times copies; then drafts that do
not measure; then drafts by weight, the gap times copies. Columns, tab-
separated: rank weight gap copies size differ kind state where name draft
measure. size is the listing's instructions as diff counts them; a piece
not measured weighs twice its size; measure is the diff command that
measures the piece, where its draft is to be written when there is none.
--kind and --state keep the pieces of those kinds and states (a state's
first words: "no draft", "does not measure", "draft", "ready"); --limit
keeps the first N. --json prints the rows as one JSON array on stdout
instead of the table; --out writes the table to FILE. The totals by kind
and state, and the run's time, go to stderr. With no --game, every game
that still has recon/<game>."#;

struct Options {
    games: Vec<&'static str>,
    kinds: Vec<Kind>,
    states: Vec<String>,
    limit: Option<usize>,
    jobs: usize,
    json: bool,
    out: Option<String>,
}

fn parse(arguments: &[String]) -> Result<Options, String> {
    let mut options = Options {
        games: Vec::new(),
        kinds: Vec::new(),
        states: Vec::new(),
        limit: None,
        jobs: std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(2).max(1)),
        json: false,
        out: None,
    };
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        let mut value = || words.next().ok_or_else(|| format!("{word} takes a value\n{USAGE}"));
        match word.as_str() {
            "--game" => options.games.push(targets::game(value()?)?),
            "--kind" => {
                for kind in value()?.split(',') {
                    options.kinds.push(Kind::named(kind).ok_or_else(|| format!("--kind takes main, arm, overlay or edition, not {kind}"))?);
                }
            }
            "--state" => options.states.push(value()?.to_ascii_lowercase()),
            "--limit" => options.limit = Some(value()?.parse().map_err(|_| format!("--limit takes a number\n{USAGE}"))?),
            "--jobs" => {
                options.jobs = value()?.parse().ok().filter(|&jobs| jobs > 0).ok_or_else(|| format!("--jobs takes a number above 0\n{USAGE}"))?
            }
            "--json" => options.json = true,
            "--out" => options.out = Some(value()?.clone()),
            other => return Err(format!("unknown rank option: {other}\n{USAGE}")),
        }
    }
    if options.json && options.out.is_some() {
        return Err("--json prints to stdout only; --out writes the table".into());
    }
    if options.kinds.is_empty() {
        options.kinds = vec![Kind::Main, Kind::Arm, Kind::Overlay, Kind::Edition];
    }
    Ok(options)
}

/// A row's state, gap and weight, as rank.ts wrote them.
fn state(row: &Row) -> String {
    match &row.measured {
        None => "no draft".into(),
        Some(Err(error)) => format!("does not measure: {error}"),
        Some(Ok(Measure { differ: 0, .. })) => "ready to adopt".into(),
        Some(Ok(_)) => "draft".into(),
    }
}

fn gap(row: &Row) -> usize {
    match &row.measured {
        Some(Ok(measure)) => measure.gap,
        _ => row.size * 2,
    }
}

fn weight(row: &Row) -> usize {
    gap(row) * row.places.len()
}

/// No draft first, by size times copies; then what does not measure; then
/// drafts by weight; larger first, then by size, then as found.
fn order(rows: &mut [Row]) {
    let tier = |row: &Row| match &row.measured {
        None => 0,
        Some(Err(_)) => 1,
        Some(Ok(_)) => 2,
    };
    rows.sort_by(|a, b| {
        let key = |row: &Row| if tier(row) == 0 { row.size * row.places.len() } else { weight(row) };
        tier(a).cmp(&tier(b)).then(key(b).cmp(&key(a))).then(b.size.cmp(&a.size))
    });
}

/// The diff command that measures a row's piece, from the checkout.
fn measure_command(row: &Row, program: &str) -> String {
    let piece = &row.piece;
    let Some(draft) = piece.draft.as_ref().or(piece.planned.as_ref()) else { return String::new() };
    let mut words = vec![program.to_string(), "diff".into(), draft.clone()];
    let stem = Path::new(draft).file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
    match &piece.reference {
        Reference::Listings(list) if list.len() > 1 || list.first() != Some(&stem) => words.push(list.join(",")),
        Reference::Listings(_) => {}
        Reference::Overlay(named) => {
            if piece.kind == Kind::Edition {
                words.extend(["--edition".into(), piece.target.id.to_string()]);
            }
            if overlay::named_in(draft).as_ref() != Some(named) {
                words.extend(["--overlay".into(), named.clone()]);
            }
            words.extend(["--symbol".into(), piece.name.clone()]);
        }
        Reference::Scaffold { .. } => words.extend(["--edition".into(), piece.target.id.to_string(), "--symbol".into(), piece.name.clone()]),
    }
    if piece.arm {
        words.push("--arm".into());
    }
    words.join(" ")
}

/// This program as a command run from the checkout: its path there, or its
/// whole path when it lives elsewhere.
fn program(root: &Path) -> String {
    let Ok(exe) = std::env::current_exe() else { return "alchemy-devtools".into() };
    let exe = exe.canonicalize().unwrap_or(exe);
    exe.strip_prefix(root).unwrap_or(&exe).to_string_lossy().into_owned()
}

fn differ(row: &Row) -> Option<usize> {
    row.measured.as_ref().and_then(|measured| measured.as_ref().ok()).map(|measure| measure.differ)
}

fn table(rows: &[Row], program: &str) -> String {
    let mut text = String::from("rank\tweight\tgap\tcopies\tsize\tdiffer\tkind\tstate\twhere\tname\tdraft\tmeasure\n");
    for (n, row) in rows.iter().enumerate() {
        let fields = [
            (n + 1).to_string(),
            weight(row).to_string(),
            gap(row).to_string(),
            row.places.len().to_string(),
            row.size.to_string(),
            differ(row).map(|differ| differ.to_string()).unwrap_or_default(),
            row.piece.kind.name().into(),
            state(row),
            row.piece.place.clone(),
            row.piece.name.clone(),
            row.piece.draft.clone().unwrap_or_default(),
            measure_command(row, program),
        ];
        text += &fields.join("\t");
        text.push('\n');
    }
    text
}

fn json(rows: &[Row], program: &str) -> String {
    let optional = |text: Option<&String>| text.map_or("null".into(), |text| quote(text));
    let list = |items: &[String]| format!("[{}]", items.iter().map(|item| quote(item)).collect::<Vec<_>>().join(","));
    let rows: Vec<String> = rows
        .iter()
        .enumerate()
        .map(|(n, row)| {
            let listings = match &row.piece.reference {
                Reference::Listings(addresses) => list(addresses),
                _ => "null".into(),
            };
            format!(
                "{{\"rank\":{},\"weight\":{},\"gap\":{},\"copies\":{},\"size\":{},\"differ\":{},\"kind\":{},\"state\":{},\"where\":{},\"name\":{},\"draft\":{},\"measure\":{},\"edition\":{},\"places\":{},\"listings\":{listings}}}",
                n + 1,
                weight(row),
                gap(row),
                row.places.len(),
                row.size,
                differ(row).map_or("null".into(), |differ| differ.to_string()),
                quote(row.piece.kind.name()),
                quote(&state(row)),
                quote(&row.piece.place),
                quote(&row.piece.name),
                optional(row.piece.draft.as_ref()),
                quote(&measure_command(row, program)),
                quote(row.piece.target.id.as_str()),
                list(&row.places),
            )
        })
        .collect();
    format!("[{}]\n", rows.join(",\n"))
}

/// Pieces, instructions (size times copies) and weight, by kind and by state.
fn totals(rows: &[Row]) -> String {
    let kind = |row: &Row| row.piece.kind.name().to_string();
    let first_words = |row: &Row| state(row).split(':').next().unwrap_or("").to_string();
    let mut text = String::new();
    for (title, key) in [("kind", &kind as &dyn Fn(&Row) -> String), ("state", &first_words)] {
        let mut sums: Vec<(String, usize, usize, usize)> = Vec::new();
        for row in rows {
            let name = key(row);
            let at = match sums.iter().position(|(seen, ..)| *seen == name) {
                Some(at) => at,
                None => {
                    sums.push((name, 0, 0, 0));
                    sums.len() - 1
                }
            };
            sums[at].1 += 1;
            sums[at].2 += row.size * row.places.len();
            sums[at].3 += weight(row);
        }
        let parts: Vec<String> = sums
            .iter()
            .map(|(name, pieces, size, weight)| format!("{name} {pieces} ({} instructions, weight {})", commas(*size as i64), commas(*weight as i64)))
            .collect();
        text += &format!("by {title}: {}\n", parts.join("; "));
    }
    text
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    if arguments.iter().any(|argument| matches!(argument.as_str(), "-h" | "--help")) {
        println!("{USAGE}");
        return Ok(());
    }
    let started = std::time::Instant::now();
    let options = parse(arguments)?;
    let root = crate::compile::root();
    let games: Vec<&str> = match options.games.is_empty() {
        true => targets::games()?.into_iter().filter(|game| root.join("recon").join(game).is_dir()).collect(),
        false => options.games.clone(),
    };
    let wants = |kind: Kind| options.kinds.contains(&kind);
    let mut rows = Vec::new();
    let mut notes = String::new();
    for game in games {
        let layout = Layout::new(game);
        let ids = built::editions(&[game.to_string()])?;
        let english = built::edition(game, "en").ok_or_else(|| format!("{game} has no English edition"))?;
        let mut editions: Vec<DecompTarget> = vec![target_for(english)];
        editions.extend(ids.iter().copied().filter(|&id| id != english).map(target_for));
        let mut pieces = Vec::new();
        if (wants(Kind::Main) || wants(Kind::Arm)) && root.join(layout.listings()).is_dir() {
            pieces.extend(inventory::main(root, &layout, editions[0])?);
        }
        if (wants(Kind::Overlay) || wants(Kind::Edition)) && root.join(layout.overlays()).is_dir() {
            pieces.extend(inventory::overlays(root, &layout, &editions)?);
        }
        if wants(Kind::Edition) {
            pieces.extend(inventory::editions(root, game, english, &ids)?);
        }
        let (measured, summary) = measure::measure(pieces, &editions, options.jobs)?;
        rows.extend(measured.into_iter().filter(|row| wants(row.piece.kind)));
        for (listing, draft) in &summary.nested {
            notes += &format!("{game}: {listing} measured with the draft of {draft}, which holds it\n");
        }
        notes += &format!("{game}: {} drafts compiled; {} listings with no instructions and no draft left out\n", summary.compiled, summary.padding);
    }
    if !options.states.is_empty() {
        rows.retain(|row| {
            let state = state(row);
            options.states.iter().any(|wanted| state.starts_with(wanted.as_str()))
        });
    }
    order(&mut rows);
    if let Some(limit) = options.limit {
        rows.truncate(limit);
    }
    let program = program(root);
    if options.json {
        print!("{}", json(&rows, &program));
    } else if let Some(out) = &options.out {
        std::fs::write(out, table(&rows, &program)).map_err(|error| format!("{out}: {error}"))?;
    } else {
        print!("{}", table(&rows, &program));
    }
    eprint!("{notes}{}", totals(&rows));
    eprintln!("{} rows in {:.1} s on {} jobs", rows.len(), started.elapsed().as_secs_f64(), options.jobs);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alchemy::targets::decomp_target;
    use inventory::Piece;

    fn row(kind: Kind, place: &str, size: usize, copies: usize, measured: Option<Result<Measure, String>>) -> Row {
        Row {
            piece: Piece {
                kind,
                target: decomp_target(Some("tla-en")).unwrap(),
                name: format!("Func_{place}"),
                place: place.into(),
                draft: measured.is_some().then(|| format!("recon/tla/en/main/{place}.c")),
                planned: Some(format!("recon/tla/en/main/{place}.c")),
                reference: Reference::Listings(vec![place.into()]),
                arm: false,
                unmeasurable: None,
            },
            size,
            places: vec![place.to_string(); copies],
            measured,
        }
    }

    fn measured(differ: usize, gap: usize) -> Option<Result<Measure, String>> {
        Some(Ok(Measure { listing: 0, draft: 0, differ, gap }))
    }

    #[test]
    fn no_draft_comes_first_then_what_does_not_measure_then_the_gap() {
        let mut rows = vec![
            row(Kind::Main, "08000010", 50, 1, measured(10, 4)),
            row(Kind::Main, "08000020", 10, 1, None),
            row(Kind::Main, "08000030", 30, 1, Some(Err("xgcc failed".into()))),
            row(Kind::Main, "08000040", 20, 3, None),
            row(Kind::Main, "08000050", 90, 1, measured(0, 0)),
            row(Kind::Main, "08000060", 40, 2, measured(6, 3)),
        ];
        order(&mut rows);
        let places: Vec<&str> = rows.iter().map(|row| row.piece.place.as_str()).collect();
        assert_eq!(places, ["08000040", "08000020", "08000030", "08000060", "08000010", "08000050"]);
        assert_eq!(rows.iter().map(weight).collect::<Vec<_>>(), [120, 20, 60, 6, 4, 0]);
        assert_eq!(rows.iter().map(state).collect::<Vec<_>>()[2..], ["does not measure: xgcc failed", "draft", "draft", "ready to adopt"]);
    }

    #[test]
    fn each_row_names_the_command_that_measures_it() {
        let mut nested = row(Kind::Main, "081843fc", 4580, 1, measured(22, 16));
        nested.piece.reference = Reference::Listings(vec!["081843fc".into(), "081848ec".into()]);
        assert_eq!(measure_command(&nested, "devtools"), "devtools diff recon/tla/en/main/081843fc.c 081843fc,081848ec");
        let mut arm = row(Kind::Arm, "080212dc", 95, 1, None);
        arm.piece.arm = true;
        assert_eq!(measure_command(&arm, "devtools"), "devtools diff recon/tla/en/main/080212dc.c --arm");
        let mut copy = row(Kind::Overlay, "resource_6aa 0200a63c", 236, 1, None);
        copy.piece.planned = Some("recon/tla/en/overlays/resource_6aa_c_0200263c.c".into());
        copy.piece.name = "Func_0200263c".into();
        copy.piece.reference = Reference::Overlay("resource_6aa".into());
        assert_eq!(measure_command(&copy, "devtools"), "devtools diff recon/tla/en/overlays/resource_6aa_c_0200263c.c --symbol Func_0200263c");
        copy.piece.kind = Kind::Edition;
        copy.piece.target = decomp_target(Some("tla-ja")).unwrap();
        copy.piece.draft = Some("games/tla/src/scene/islets/s263c.c".into());
        assert_eq!(
            measure_command(&copy, "devtools"),
            "devtools diff games/tla/src/scene/islets/s263c.c --edition tla-ja --overlay resource_6aa --symbol Func_0200263c"
        );
        let mut edition = row(Kind::Edition, "tla-ja .rom.0016fca8", 1152, 5, measured(0, 0));
        edition.piece.target = decomp_target(Some("tla-ja")).unwrap();
        edition.piece.name = "BattleEffect_RunMegaera".into();
        edition.piece.draft = Some("games/tla/src/battle_effect/f170364.c".into());
        edition.piece.reference = Reference::Scaffold { sized_by: None };
        assert_eq!(
            measure_command(&edition, "devtools"),
            "devtools diff games/tla/src/battle_effect/f170364.c --edition tla-ja --symbol BattleEffect_RunMegaera"
        );
        let shown = table(std::slice::from_ref(&edition), "devtools");
        assert_eq!(
            shown.lines().nth(1).unwrap().split('\t').take(9).collect::<Vec<_>>(),
            ["1", "0", "0", "5", "1152", "0", "edition", "ready to adopt", "tla-ja .rom.0016fca8"]
        );
        assert!(json(&[edition], "devtools").starts_with("[{\"rank\":1,\"weight\":0,\"gap\":0,\"copies\":5,\"size\":1152,\"differ\":0,\"kind\":\"edition\""));
    }

    #[test]
    fn options_name_kinds_states_and_one_output() {
        let args = |items: &[&str]| items.iter().map(|item| item.to_string()).collect::<Vec<_>>();
        let options = parse(&args(&["--kind", "main,arm", "--kind", "edition", "--state", "Draft", "--limit", "5", "--jobs", "3"])).unwrap();
        assert_eq!(options.kinds, [Kind::Main, Kind::Arm, Kind::Edition]);
        assert_eq!((options.states, options.limit, options.jobs), (vec!["draft".to_string()], Some(5), 3));
        assert!(parse(&args(&["--json", "--out", "rank.tsv"])).is_err());
        assert!(parse(&args(&["--kind", "data"])).is_err());
        assert!(parse(&args(&["--jobs", "0"])).is_err());
        assert!(parse(&args(&["--limit"])).is_err());
        assert_eq!(parse(&[]).unwrap().kinds.len(), 4);
    }
}
