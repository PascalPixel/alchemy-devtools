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
mod field;
#[cfg(feature = "render")]
mod front;
#[cfg(feature = "render")]
mod guide;
#[cfg(feature = "render")]
mod mgba;
#[cfg(feature = "render")]
mod places;
mod plan;
// Read only by the feature's commands; without it they say how to build them.
#[cfg_attr(not(feature = "render"), allow(dead_code))]
mod story;

use plan::{Cast, Command, Plan, Tables};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: alchemy-devtools render psynergy [--ability N|NAME]... [--all] [--djinn] [--summons] [--list]
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
        Some(kind @ ("place" | "event")) => scenes(kind, &arguments[1..]),
        Some("story") => story_pictures(&arguments[1..]),
        Some("guide") => guide_pictures(&arguments[1..]),
        Some("-h" | "--help") => {
            println!("{COMMANDS}");
            Ok(())
        }
        _ => Err(COMMANDS.to_string()),
    }
}

struct Options {
    abilities: Vec<String>,
    all: bool,
    djinn: bool,
    summons: bool,
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
        djinn: false,
        summons: false,
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
            "--djinn" => o.djinn = true,
            "--summons" => o.summons = true,
            "--list" => o.list = true,
            "--verbose" => o.verbose = true,
            "--out" => o.out = Some(PathBuf::from(value("--out")?)),
            "--edition" => o.edition = value("--edition")?,
            "--backdrop" => o.backdrop = value("--backdrop")?,
            other => return Err(format!("unexpected argument {other:?}\n{USAGE}")),
        }
    }
    if o.abilities.is_empty() && !o.all && !o.djinn && !o.summons {
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
    let mut chosen: Vec<&Cast> = plan
        .casts
        .iter()
        .filter(|c| match c.command {
            Command::Psynergy => o.all,
            Command::Djinni { .. } => o.djinn,
            Command::Summon { .. } => o.summons,
        })
        .collect();
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
    let edition = &o.edition;
    let scene = backdrop::scene(&o.backdrop)?;
    let build = root.join("out").join(edition);
    let map_path = build.join(format!("{edition}.map"));
    let rom_path = build.join(format!("{edition}.gba"));
    let map = std::fs::read(&map_path).map_err(|e| format!("cannot read {}: {e} (make {edition})", map_path.display()))?;
    let map = crate::ld_map::Map::parse(&String::from_utf8_lossy(&map));
    let rom = std::fs::read(&rom_path).map_err(|e| format!("cannot read {}: {e} (make {edition})", rom_path.display()))?;
    note_modified(root, edition, &rom);
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
            if recording.battle_over { ", the battle ended" } else if recording.capped { ", stopped at 25 seconds" } else { "" }
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

const COMMANDS: &str = "usage: alchemy-devtools render psynergy|place|event [options]
  psynergy  each battle Psynergy the party casts, as a looping GIF
  place     each scene's entrance photo and a map of each room, as PNG
  event     the message pages of each scene's events, as PNG
  story     the front page's pictures: the title, Vale's farewell, the lit
            lighthouse and the world map
Run any of them with --help for its options.";

const SCENES_USAGE: &str = "usage: alchemy-devtools render place|event [--scene N|NAME]... [--all]
         --places places.tsv --events events.tsv --flags flags.tsv [--messages messages.tsv]
         [--event NAME]... [--out DIR] [--edition tbs-en]
Plays a built ☀️ edition (make tbs-en) under libmgba from a new game's
defaults straight into each scene, and writes PNG files at the GBA's size
into DIR/<scene>/ (DIR defaults to out/EDITION/places), with DIR/index.tsv
listing every image (scene, number, area, variant, kind, entrance, event,
page, the page's message, the event's messages, file) and DIR/skipped.tsv
what was left out and why. --all takes every scene but the title, the
clear-data screen and the debug test rooms. The tables are the story tools'
places.tsv (scenes), events.tsv (each event's trigger, flags and messages)
and flags.tsv (flag names).

place: the main entrance's photo once the player has control
(entrance.png; the main entrance is the table's first, which the scene
falls back to), and a map of each room, one camera box of the scene's
entrances, named by the first entrance that reaches it (map-<entrance>.png),
stitched from one moment with the leader hidden. A scene whose
initialization tests FLAG_KOLIMA_CURSE_LIFTED is shown cursed and cured,
in DIR/<scene>_cursed/ and DIR/<scene>_cured/.

event: every talk, search, touch and Psynergy-on-actor event of the scenes
(--event names some; rows of one name are one event), each played once,
from just before its trigger until the player has control again, as
DIR/<scene>/events/<event>.gif, with its first four pages that wait for
the button as <event>.png, <event>-2.png and on. An event plays at any
scene its row lists, so a scene's overlay brings its other scenes along.
The leader stands where the player would to start it, on ground he could
walk to from where he enters, in the room the camera shows: beside the
actor, on its map layer and level with it, facing it; facing the cell it
searches from any of eight ways; a cell from the cell it touches, walking
onto it, or on the cell pushing the way its row needs. Move is steered
with the d-pad once cast. An event counts only when the field loop
dispatches it at the leader's spot, it runs a script, and it shows
itself: one of its messages when it has messages, else (a touch, a Move)
something that moves, leaves, fights or changes the map. Each of its
triggers is tried in turn, by the entrances whose events answer it, with
the flags its row needs, then with each story flag its scene's setup
tests or its scene's events set; one that does not play is tried again
after each of the scene's other events. Pages are held 45 frames; an exit
ends as the next scene starts loading, and an event that runs past three
minutes gets no GIF, noted in skipped.tsv. --messages is accepted and not
read.


Needs the `render` feature: cargo build --release --features render, with
libmgba and its headers installed (MGBA_PREFIX names their prefix).";

struct SceneOptions {
    scenes: Vec<String>,
    events: Vec<String>,
    all: bool,
    out: Option<PathBuf>,
    edition: String,
    places_tsv: Option<PathBuf>,
    events_tsv: Option<PathBuf>,
    flags_tsv: Option<PathBuf>,
}

fn scenes(kind: &str, arguments: &[String]) -> Result<(), String> {
    let mut o = SceneOptions {
        scenes: Vec::new(),
        events: Vec::new(),
        all: false,
        out: None,
        edition: "tbs-en".to_string(),
        places_tsv: None,
        events_tsv: None,
        flags_tsv: None,
    };
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        let mut value =
            |name: &str| arguments.next().cloned().ok_or_else(|| format!("{name} needs a value\n{SCENES_USAGE}"));
        match argument.as_str() {
            "-h" | "--help" => {
                println!("{SCENES_USAGE}");
                return Ok(());
            }
            "--scene" => o.scenes.push(value("--scene")?),
            "--event" => o.events.push(value("--event")?),
            "--all" => o.all = true,
            "--out" => o.out = Some(PathBuf::from(value("--out")?)),
            "--edition" => o.edition = value("--edition")?,
            "--places" => o.places_tsv = Some(PathBuf::from(value("--places")?)),
            "--events" => o.events_tsv = Some(PathBuf::from(value("--events")?)),
            "--flags" => o.flags_tsv = Some(PathBuf::from(value("--flags")?)),
            "--messages" => {
                value("--messages")?;
            }
            other => return Err(format!("unexpected argument {other:?}\n{SCENES_USAGE}")),
        }
    }
    crate::targets::parse_decomp_target(&o.edition)?;
    if !o.edition.starts_with("tbs-") {
        return Err(format!("{}: only the ☀️ editions are read here", o.edition));
    }
    let need = |p: &Option<PathBuf>, name: &str| p.clone().ok_or_else(|| format!("{name} is needed\n{SCENES_USAGE}"));
    let scene_rows = story::read_scenes(&need(&o.places_tsv, "--places")?)?;
    let events = story::read_events(&need(&o.events_tsv, "--events")?)?;
    let flags = story::Flags::read(&need(&o.flags_tsv, "--flags")?)?;
    if o.scenes.is_empty() && !o.all && o.events.is_empty() {
        return Err(format!("name a --scene or ask for --all\n{SCENES_USAGE}"));
    }
    let chosen: Vec<&story::Scene> = scene_rows
        .iter()
        .filter(|s| {
            o.all
                || o.scenes.iter().any(|w| {
                    w.parse::<u16>().ok() == Some(s.number)
                        || s.symbol.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(w))
                        || s.slug() == *w
                })
                || (o.scenes.is_empty() && !o.events.is_empty())
        })
        .collect();
    // An event plays at any scene its row lists, so each overlay's other
    // scenes (the other floors of Mercury Lighthouse) come too; only the
    // events of the scenes asked for are played.
    let only: Vec<u16> = if kind == "event" && !o.all && !o.scenes.is_empty() { chosen.iter().map(|s| s.number).collect() } else { Vec::new() };
    let chosen: Vec<&story::Scene> = if kind == "event" {
        scene_rows.iter().filter(|s| chosen.iter().any(|c| c.file == s.file)).collect()
    } else {
        chosen
    };
    let root = crate::compile::root();
    let out = o.out.clone().unwrap_or_else(|| root.join("out").join(&o.edition).join("places"));
    play_scenes(root, &o.edition, kind, &chosen, &only, &events, &o.events, &flags, &out)
}

#[cfg(not(feature = "render"))]
fn play_scenes(
    _: &Path,
    _: &str,
    _: &str,
    _: &[&story::Scene],
    _: &[u16],
    _: &[story::Event],
    _: &[String],
    _: &story::Flags,
    _: &Path,
) -> Result<(), String> {
    Err("alchemy-devtools was built without the render feature: cargo build --release --features render (needs libmgba)".to_string())
}

#[cfg(feature = "render")]
fn play_scenes(
    root: &Path,
    edition: &str,
    kind: &str,
    chosen: &[&story::Scene],
    only: &[u16],
    events: &[story::Event],
    wanted: &[String],
    flags: &story::Flags,
    out: &Path,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    let build = root.join("out").join(edition);
    let map_path = build.join(format!("{edition}.map"));
    let rom_path = build.join(format!("{edition}.gba"));
    let map = std::fs::read(&map_path).map_err(|e| format!("cannot read {}: {e} (make {edition})", map_path.display()))?;
    let map = crate::ld_map::Map::parse(&String::from_utf8_lossy(&map));
    let rom = std::fs::read(&rom_path).map_err(|e| format!("cannot read {}: {e} (make {edition})", rom_path.display()))?;
    note_modified(root, edition, &rom);
    let symbols = field::Symbols::find(&map)?;
    std::fs::create_dir_all(out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    // Work in parallel, a scene (place) or the scenes of one overlay
    // (event, whose rows list those scenes) to each job; each job runs its
    // own machines.
    let jobs: Vec<Vec<&story::Scene>> = if kind == "place" {
        chosen.iter().map(|s| vec![*s]).collect()
    } else {
        let mut by_file: BTreeMap<&str, Vec<&story::Scene>> = BTreeMap::new();
        for s in chosen {
            by_file.entry(s.file.as_str()).or_default().push(*s);
        }
        by_file.into_values().collect()
    };
    let queue = std::sync::Mutex::new(jobs.into_iter());
    let results = std::sync::Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(2).clamp(1, 12));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let Some(job) = queue.lock().unwrap().next() else {
                    break;
                };
                let mut context = places::Context {
                    rom: &rom,
                    symbols: &symbols,
                    flags,
                    events,
                    out: out.to_path_buf(),
                    rows: Vec::new(),
                    skipped: Vec::new(),
                    bytes: 0,
                    images: 0,
                    gifs: 0,
                    gif_bytes: 0,
                    wanted,
                    only,
                    cleared: Vec::new(),
                };
                if kind == "place" {
                    for scene in &job {
                        context.place(scene);
                    }
                } else {
                    context.events(&job);
                }
                eprintln!("{}: {} images", job.iter().map(|s| s.name()).collect::<Vec<_>>().join(", "), context.images);
                results.lock().unwrap().push((context.rows, context.skipped, context.bytes, context.images, context.gifs, context.gif_bytes, context.cleared));
            });
        }
    });
    let (mut rows, mut skipped, mut bytes, mut images, mut gifs, mut gif_bytes) = (Vec::new(), Vec::new(), 0, 0, 0, 0);
    let mut cleared = Vec::new();
    for (r, s, b, i, g, gb, c) in results.into_inner().unwrap() {
        cleared.extend(c);
        rows.extend(r);
        skipped.extend(s);
        bytes += b;
        images += i;
        gifs += g;
        gif_bytes += gb;
    }
    if kind == "place" {
        let candidates = places::variant_candidates(events, flags);
        let mut text = String::from("scene\tnumber\tflags\n");
        for (number, list) in &candidates {
            let name = chosen.iter().find(|s| s.number == *number).map_or_else(|| format!("#{number}"), |s| s.name());
            text.push_str(&format!("{name}\t{number}\t{}\n", list.join(", ")));
        }
        let path = out.join("variant-flags.tsv");
        std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    places::write_index(out, &rows)?;
    let path = out.join("skipped.tsv");
    // The lines already there for other scenes and things are kept.
    let mut lines: BTreeMap<(String, String), String> = BTreeMap::new();
    for line in std::fs::read_to_string(&path).unwrap_or_default().lines().skip(1) {
        let mut fields = line.split('\t');
        if let (Some(scene), Some(what)) = (fields.next(), fields.next()) {
            lines.insert((scene.to_string(), what.to_string()), line.to_string());
        }
    }
    // What played this time is no longer left out.
    for key in &cleared {
        lines.remove(key);
    }
    for s in &skipped {
        lines.insert((s.scene.clone(), s.what.clone()), format!("{}\t{}\t{}", s.scene, s.what, s.reason));
    }
    let mut text = String::from("scene\twhat\treason\n");
    for line in lines.values() {
        text.push_str(line);
        text.push('\n');
    }
    std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    eprintln!(
        "{} images ({} GIFs, {} KiB), {} KiB, {} left out, in {:.0} s, in {}",
        images,
        gifs,
        gif_bytes.div_ceil(1024),
        bytes.div_ceil(1024),
        skipped.len(),
        started.elapsed().as_secs_f64(),
        out.display()
    );
    Ok(())
}

/// Says so when the built ROM is not the one rom.sha1 names: a modified
/// build, whose pictures are its own.
#[cfg(feature = "render")]
fn note_modified(root: &Path, edition: &str, rom: &[u8]) {
    use sha1::{Digest, Sha1};

    let listed = std::fs::read_to_string(root.join("rom.sha1")).unwrap_or_default();
    let digest = format!("{:x}", Sha1::digest(rom));
    if !listed.lines().any(|l| l.starts_with(&digest) && l.ends_with(&format!("{edition}.gba"))) {
        eprintln!("note: out/{edition}/{edition}.gba is not the ROM rom.sha1 names: a modified build");
    }
}

const STORY_USAGE: &str = "usage: alchemy-devtools render story --flags flags.tsv [--only NAME]... [--out DIR] [--edition tbs-en]
Plays a built ☀️ edition (make tbs-en) for The Broken Seal's front page and
writes into DIR (default out/EDITION/story):
  title       title.gif and title.png: the Golden Sun logo card the title
              shows after the ending (its entrance 10), coming up and held
  farewell    vale-farewell.gif and vale-farewell.png: Vale's send-off,
              Event_DepartForQuest (ValeHome's entrance 12), through the
              town's farewell, and the town gathered
  lighthouse  venus-lighthouse-lit.gif and .png: the world map cut at its
              entrance 91 after Venus Lighthouse's beacon erupts
--only picks some; flags.tsv is the story tools' flag names.";

fn story_pictures(arguments: &[String]) -> Result<(), String> {
    let (mut only, mut out, mut edition, mut flags_tsv) = (Vec::new(), None, "tbs-en".to_string(), None);
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        let mut value = |name: &str| arguments.next().cloned().ok_or_else(|| format!("{name} needs a value\n{STORY_USAGE}"));
        match argument.as_str() {
            "-h" | "--help" => {
                println!("{STORY_USAGE}");
                return Ok(());
            }
            "--only" => only.push(value("--only")?),
            "--out" => out = Some(PathBuf::from(value("--out")?)),
            "--edition" => edition = value("--edition")?,
            "--flags" => flags_tsv = Some(PathBuf::from(value("--flags")?)),
            other => return Err(format!("unexpected argument {other:?}\n{STORY_USAGE}")),
        }
    }
    let flags = story::Flags::read(&flags_tsv.ok_or_else(|| format!("--flags is needed\n{STORY_USAGE}"))?)?;
    let root = crate::compile::root();
    let out = out.unwrap_or_else(|| root.join("out").join(&edition).join("story"));
    play_story(root, &edition, &only, &flags, &out)
}

#[cfg(not(feature = "render"))]
fn play_story(_: &Path, _: &str, _: &[String], _: &story::Flags, _: &Path) -> Result<(), String> {
    Err("alchemy-devtools was built without the render feature: cargo build --release --features render (needs libmgba)".to_string())
}

#[cfg(feature = "render")]
fn play_story(root: &Path, edition: &str, only: &[String], flags: &story::Flags, out: &Path) -> Result<(), String> {
    let build = root.join("out").join(edition);
    let map = std::fs::read(build.join(format!("{edition}.map"))).map_err(|e| format!("cannot read the map: {e} (make {edition})"))?;
    let map = crate::ld_map::Map::parse(&String::from_utf8_lossy(&map));
    let rom = std::fs::read(build.join(format!("{edition}.gba"))).map_err(|e| format!("cannot read the ROM: {e} (make {edition})"))?;
    note_modified(root, edition, &rom);
    let symbols = field::Symbols::find(&map)?;
    std::fs::create_dir_all(out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    let mut front = front::Front { rom: &rom, symbols: &symbols, flags, out, written: Vec::new() };
    let wanted = |name: &str| only.is_empty() || only.iter().any(|o| o == name);
    let mut failed = Vec::new();
    if wanted("world") {
        let update = map.symbol("Map_UpdateCurrentTileBlock").map(|(_, a)| a).ok_or("the map has no Map_UpdateCurrentTileBlock")?;
        if let Err(e) = front.world(update) {
            eprintln!("world: {e}");
            failed.push("world");
        }
    }
    for name in ["title", "farewell", "lighthouse"] {
        if wanted(name) {
            let result = match name {
                "title" => front.title(),
                "farewell" => front.farewell(4607, 4613, 4628),
                _ => front.lighthouse(),
            };
            if let Err(e) = result {
                eprintln!("{name}: {e}");
                failed.push(name);
            }
        }
    }
    for (file, bytes, what) in &front.written {
        eprintln!("{}\t{} KiB\t{what}", out.join(file).display(), bytes.div_ceil(1024));
    }
    if failed.is_empty() { Ok(()) } else { Err(format!("{} failed", failed.join(", "))) }
}

const GUIDE_USAGE: &str = "usage: alchemy-devtools render guide --flags flags.tsv [--only NAME]... [--out DIR] [--edition tbs-en]
Plays a built ☀️ edition (make tbs-en) with the pad held as a player would,
for the folder guides, and writes into DIR (default out/EDITION/guide):
  walk       walk-run.gif: Vale, walking right, then running back with B
  snow       snow.gif: Imil, the leader's footprints in the snow
  layers     layers.gif: Vale built up layer by layer, BG3 to BG1
  layers-3d  layers-3d.gif: Vale's layers pulled apart into sheets in 3D
  world-map  world-map-walk.gif: the world map's perspective view, walking
--only picks some; flags.tsv is the story tools' flag names.";

fn guide_pictures(arguments: &[String]) -> Result<(), String> {
    let (mut only, mut out, mut edition, mut flags_tsv) = (Vec::new(), None, "tbs-en".to_string(), None);
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        let mut value = |name: &str| arguments.next().cloned().ok_or_else(|| format!("{name} needs a value\n{GUIDE_USAGE}"));
        match argument.as_str() {
            "-h" | "--help" => {
                println!("{GUIDE_USAGE}");
                return Ok(());
            }
            "--only" => only.push(value("--only")?),
            "--out" => out = Some(PathBuf::from(value("--out")?)),
            "--edition" => edition = value("--edition")?,
            "--flags" => flags_tsv = Some(PathBuf::from(value("--flags")?)),
            other => return Err(format!("unexpected argument {other:?}\n{GUIDE_USAGE}")),
        }
    }
    let flags = story::Flags::read(&flags_tsv.ok_or_else(|| format!("--flags is needed\n{GUIDE_USAGE}"))?)?;
    let root = crate::compile::root();
    let out = out.unwrap_or_else(|| root.join("out").join(&edition).join("guide"));
    play_guide(&root, &edition, &only, &flags, &out)
}

#[cfg(not(feature = "render"))]
fn play_guide(_: &Path, _: &str, _: &[String], _: &story::Flags, _: &Path) -> Result<(), String> {
    Err("alchemy-devtools was built without the render feature: cargo build --release --features render (needs libmgba)".to_string())
}

#[cfg(feature = "render")]
fn play_guide(root: &Path, edition: &str, only: &[String], flags: &story::Flags, out: &Path) -> Result<(), String> {
    let build = root.join("out").join(edition);
    let map = std::fs::read(build.join(format!("{edition}.map"))).map_err(|e| format!("cannot read the map: {e} (make {edition})"))?;
    let map = crate::ld_map::Map::parse(&String::from_utf8_lossy(&map));
    let rom = std::fs::read(build.join(format!("{edition}.gba"))).map_err(|e| format!("cannot read the ROM: {e} (make {edition})"))?;
    note_modified(root, edition, &rom);
    let symbols = field::Symbols::find(&map)?;
    std::fs::create_dir_all(out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    let mut guide = guide::Guide { rom: &rom, symbols: &symbols, flags, out, written: Vec::new() };
    let wanted = |name: &str| only.is_empty() || only.iter().any(|o| o == name);
    let mut failed = Vec::new();
    for name in ["walk", "snow", "layers", "layers-3d", "world-map", "menus", "menu-start", "boot", "items"] {
        if wanted(name) {
            let result = match name {
                "walk" => guide.walk(),
                "snow" => guide.snow(),
                "layers" => guide.layers(),
                "layers-3d" => guide.exploded(),
                "menus" => guide.menus(),                "menu-start" => guide.menu_start(),                "boot" => guide.boot(),                "items" => guide.items(),
                _ => guide.world_map(),
            };
            if let Err(e) = result {
                eprintln!("{name}: {e}");
                failed.push(name);
            }
        }
    }
    for (file, bytes, what) in &guide.written {
        eprintln!("{}\t{} KiB\t{what}", out.join(file).display(), bytes.div_ceil(1024));
    }
    if failed.is_empty() { Ok(()) } else { Err(format!("{} failed", failed.join(", "))) }
}
