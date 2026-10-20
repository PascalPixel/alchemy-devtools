//! render: the game's own code at work, recorded from a built edition run in
//! libmgba. `render psynergy` plays each battle Psynergy the party casts and
//! writes it as a looping GIF. Nothing here takes part in a build, and the
//! ROM is only read: the pictures are what it draws.

#[cfg(feature = "render")]
mod animation;
#[cfg(feature = "render")]
mod backdrop;
#[cfg(feature = "render")]
mod battle;
#[cfg(feature = "render")]
mod mgba;
mod plan;

use plan::{Cast, Plan, Tables};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: alchemy-devtools render psynergy [--ability N|NAME]... [--all] [--list]
         [--out DIR] [--edition tbs-en] [--backdrop gradient|game|PICTURE.png] [--verbose]
Plays battle Psynergy in a built ☀️ edition (make tbs-en) under libmgba and
writes each cast as DIR/<ability>.gif: the whole 240 by 160 screen at 30
frames a second, looping, from the cast's message to the next round. DIR/index.tsv
lists each file's ability id, name, caster, target and frames at 30 a
second. DIR defaults to out/EDITION/psynergy. --all casts every ability usable
in battle that the party learns (classes.tsv and the items that teach one),
and lists the rest with why they are left out; --ability casts one by id or
English name. --list prints the casts instead of playing them; --verbose
prints each run's setup: the party, the units' classes and the sprites
BattleObjMake picks.

--backdrop sets the battlefield behind the fighters: `gradient` (the default)
clears the battle background and draws a grey a line as the GBA's backdrop
colour; `game` keeps the battle's own background; a 256 by 120 PNG, a
battle backdrop's size, takes the background's place in its own format (128
colours in BG palettes 6 to 13, 256-colour tiles), and the game draws,
scrolls, darkens and blends it as its own. The windows are drawn grey at
their colours' lightness.

The caster follows the element: Venus Isaac, Mercury Mia, Mars Jenna for
her own Flare Psynergy and Garet for the rest, Jupiter Ivan: the four from
debug party preset 0, at level 35 with their grown-up sprites, and Jenna as
the debug menu leaves her. Offensive Psynergy strikes the opposed element's
character (Venus and Jupiter, Mars and Mercury), copied onto the enemy side
as a link battle copies the opponent's party, with 999 HP; one that reaches
past its aim strikes three. Healing and support go to that character as the
caster's ally, in a battle against a Vermin: hurt for healing, downed for
Revive, poisoned for Cure Poison, deluded for Restore. A caster short of PP
is given enough, and foes facing instant death enough luck.

Needs the `render` feature: cargo build --release --features render, with
libmgba and its headers installed (MGBA_PREFIX names their prefix).";


pub fn run(arguments: &[String]) -> Result<(), String> {
    match arguments.first().map(String::as_str) {
        Some("psynergy") => psynergy(&arguments[1..]),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(USAGE.to_string()),
    }
}

struct Options {
    abilities: Vec<String>,
    all: bool,
    list: bool,
    verbose: bool,
    out: Option<PathBuf>,
    edition: String,
    backdrop: String,
}

fn options(arguments: &[String]) -> Result<Option<Options>, String> {
    let mut o = Options {
        abilities: Vec::new(),
        all: false,
        list: false,
        verbose: false,
        out: None,
        edition: "tbs-en".to_string(),
        backdrop: "gradient".to_string(),
    };
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        let mut value = |name: &str| arguments.next().cloned().ok_or_else(|| format!("{name} needs a value\n{USAGE}"));
        match argument.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            "--ability" => o.abilities.push(value("--ability")?),
            "--all" => o.all = true,
            "--list" => o.list = true,
            "--verbose" => o.verbose = true,
            "--out" => o.out = Some(PathBuf::from(value("--out")?)),
            "--edition" => o.edition = value("--edition")?,
            "--backdrop" => o.backdrop = value("--backdrop")?,
            other => return Err(format!("unexpected argument {other:?}\n{USAGE}")),
        }
    }
    if o.abilities.is_empty() && !o.all {
        return Err(format!("name an --ability or ask for --all\n{USAGE}"));
    }
    crate::targets::parse_decomp_target(&o.edition)?;
    if !o.edition.starts_with("tbs-") {
        return Err(format!("{}: only the ☀️ editions have this battle setup", o.edition));
    }
    Ok(Some(o))
}

/// The casts asked for, in id order.
fn select<'a>(plan: &'a Plan, o: &Options) -> Result<Vec<&'a Cast>, String> {
    if o.all {
        return Ok(plan.casts.iter().collect());
    }
    let mut chosen: Vec<&Cast> = Vec::new();
    for wanted in &o.abilities {
        let matches = |id: u16, name: &str| wanted.parse::<u16>().ok() == Some(id) || name.eq_ignore_ascii_case(wanted);
        let found: Vec<&Cast> = plan.casts.iter().filter(|c| matches(c.ability.id, &c.ability.name)).collect();
        if found.is_empty() {
            return Err(match plan.skipped.iter().find(|(a, _)| matches(a.id, &a.name)) {
                Some((a, skip)) => format!("{} ({}) is left out: {}", a.name, a.id, skip.reason()),
                None => format!("no battle Psynergy {wanted:?}"),
            });
        }
        chosen.extend(found);
    }
    chosen.sort_by_key(|c| c.ability.id);
    chosen.dedup_by_key(|c| c.ability.id);
    Ok(chosen)
}

fn names(owners: &[u8]) -> String {
    owners.iter().map(|&o| plan::character_name(o)).collect::<Vec<_>>().join(", ")
}

fn psynergy(arguments: &[String]) -> Result<(), String> {
    let Some(o) = options(arguments)? else {
        return Ok(());
    };
    let root = crate::compile::root();
    let plan = Tables::read(root)?.plan()?;
    let casts = select(&plan, &o)?;
    if o.list {
        println!("ability\tname\telement\tcaster\ttarget\tparty\tfoes\tfile");
        for c in &casts {
            let a = &c.ability;
            let foes = if c.foes.is_empty() { "Vermin".to_string() } else { names(&c.foes) };
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{foes}\t{}.gif",
                a.id,
                a.name,
                plan::element_name(a.element),
                plan::character_name(c.caster()),
                c.target_name(),
                names(&c.party),
                c.slug
            );
        }
    }
    if o.all {
        for (a, skip) in &plan.skipped {
            eprintln!("left out: {}\t{}\t{}", a.id, a.name, skip.reason());
        }
        eprintln!("{} casts, {} left out", plan.casts.len(), plan.skipped.len());
    }
    if o.list {
        return Ok(());
    }
    let out = o.out.clone().unwrap_or_else(|| root.join("out").join(&o.edition).join("psynergy"));
    std::fs::create_dir_all(&out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    let rows = play(root, &o, &casts, &out)?;
    write_index(&out.join("index.tsv"), rows)
}

/// One line of index.tsv.
struct Row {
    id: u16,
    line: String,
}

#[cfg(not(feature = "render"))]
fn play(_: &Path, _: &Options, _: &[&Cast], _: &Path) -> Result<Vec<Row>, String> {
    Err("alchemy-devtools was built without the render feature: cargo build --release --features render (needs libmgba)".to_string())
}

#[cfg(feature = "render")]
fn play(root: &Path, o: &Options, casts: &[&Cast], out: &Path) -> Result<Vec<Row>, String> {
    use sha1::{Digest, Sha1};

    let edition = &o.edition;
    let scene = backdrop::scene(&o.backdrop)?;
    let build = root.join("out").join(edition);
    let map_path = build.join(format!("{edition}.map"));
    let rom_path = build.join(format!("{edition}.gba"));
    let map = std::fs::read(&map_path).map_err(|e| format!("cannot read {}: {e} (make {edition})", map_path.display()))?;
    let map = crate::ld_map::Map::parse(&String::from_utf8_lossy(&map));
    let rom = std::fs::read(&rom_path).map_err(|e| format!("cannot read {}: {e} (make {edition})", rom_path.display()))?;
    let listed = std::fs::read_to_string(root.join("rom.sha1")).unwrap_or_default();
    let digest = format!("{:x}", Sha1::digest(&rom));
    if !listed.lines().any(|l| l.starts_with(&digest) && l.ends_with(&format!("{edition}.gba"))) {
        eprintln!("note: out/{edition}/{edition}.gba is not the ROM rom.sha1 names: a modified build");
    }
    let symbols = battle::Symbols::find(&map)?;
    let mut rows = Vec::new();
    let mut total = 0;
    for cast in casts {
        let a = &cast.ability;
        let failed = |e: String| format!("{} ({}): {e}", a.name, a.id);
        let recording = battle::record(&rom, &symbols, cast, &scene, o.verbose).map_err(failed)?;
        let frames: Vec<Vec<u32>> =
            recording.frames.iter().map(|f| f.iter().map(|&c| animation::rgb(c)).collect()).collect();
        let file = format!("{}.gif", cast.slug);
        let written = animation::write(&out.join(&file), &frames, mgba::WIDTH)?;
        total += written.bytes;
        eprintln!(
            "{}\t{}\t{} -> {}\t{} frames ({} stored), {} colours, {} KiB{}",
            a.id,
            a.name,
            plan::character_name(cast.caster()),
            cast.target_name(),
            written.frames,
            written.stored,
            written.colours,
            written.bytes.div_ceil(1024),
            if recording.battle_over { ", the battle ended" } else { "" }
        );
        rows.push(Row {
            id: a.id,
            line: format!(
                "{}\t{}\t{}\t{}\t{}\t{file}",
                a.id,
                a.name,
                plan::character_name(cast.caster()),
                cast.target_name(),
                written.frames
            ),
        });
    }
    eprintln!("{} files, {} KiB, in {}", rows.len(), total.div_ceil(1024), out.display());
    Ok(rows)
}

const INDEX_HEADER: &str = "ability\tname\tcaster\ttarget\tframes\tfile";

/// Writes `rows` into index.tsv, keeping the rows already there for other
/// abilities, in id order.
fn write_index(path: &Path, rows: Vec<Row>) -> Result<(), String> {
    let mut lines: BTreeMap<u16, String> = BTreeMap::new();
    if let Ok(text) = std::fs::read_to_string(path) {
        for line in text.lines().skip(1) {
            if let Some(id) = line.split('\t').next().and_then(|id| id.parse().ok()) {
                lines.insert(id, line.to_string());
            }
        }
    }
    for row in rows {
        lines.insert(row.id, row.line);
    }
    let mut text = format!("{INDEX_HEADER}\n");
    for line in lines.values() {
        text.push_str(line);
        text.push('\n');
    }
    std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}
