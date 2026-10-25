//! render: the game's own code at work, recorded from a built edition run in
//! libmgba. `render psynergy` plays each battle Psynergy the party casts and
//! writes it as a looping GIF. Nothing here takes part in a build, and the
//! ROM is only read: the pictures are what it draws.

#[cfg(feature = "render")]
mod animation;
#[cfg(feature = "render")]
mod arrivals;
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
mod guide_engine;
#[cfg(feature = "render")]
mod guide_front;
#[cfg(feature = "render")]
mod guide_apart;
#[cfg(feature = "render")]
mod guide_battle;
#[cfg(feature = "render")]
mod guide_cells;
#[cfg(feature = "render")]
mod guide_docs_world;
#[cfg(feature = "render")]
mod guide_effect;
#[cfg(feature = "render")]
mod guide_fight;
#[cfg(feature = "render")]
mod guide_menu;
#[cfg(feature = "render")]
mod guide_menus;
#[cfg(feature = "render")]
mod guide_more;
#[cfg(feature = "render")]
mod guide_planes;
#[cfg(feature = "render")]
mod guide_places;
#[cfg(feature = "render")]
mod guide_take;
#[cfg(feature = "render")]
mod guide_window;
#[cfg(feature = "render")]
mod guide_world;
#[cfg(feature = "render")]
mod guide_world_play;
mod jobs;
#[cfg(feature = "render")]
mod mgba;
#[cfg(feature = "render")]
mod party;
#[cfg(feature = "render")]
mod places;
#[cfg(feature = "render")]
mod probe;
#[cfg(feature = "render")]
mod probe_places;
#[cfg(feature = "render")]
mod source;
mod plan;
// Read only by the feature's commands and by plan.rs.
#[cfg_attr(not(feature = "render"), allow(dead_code))]
mod tables;
// Read only by the feature's commands; without it they say how to build them.
#[cfg_attr(not(feature = "render"), allow(dead_code))]
mod story;

use plan::{Cast, Command, Plan, Skip, Tables};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: alchemy-devtools render psynergy [--ability N|NAME]... [--all] [--djinn] [--summons] [--list] [--unlearned]
         [--out DIR] [--edition tbs-en] [--backdrop gradient|game|PICTURE.png] [--verbose] [--jobs N]
Plays battle Psynergy in a built ☀️ edition (make tbs-en) under libmgba and
writes each cast as DIR/<ability>.gif: the whole 240 by 160 screen at 30
frames a second, looping, from the cast's message to the next round. DIR/index.tsv
lists each file's ability id, name, caster, target and frames at 30 a
second. DIR defaults to out/EDITION/psynergy. --all casts every ability usable
in battle that the party learns (its classes and the items that teach one),
and lists the rest with why they are left out; --ability casts one by id or
English name. --list prints the casts instead of playing them; --verbose
prints each run's setup: the party, the units' classes and the sprites
BattleObject_Make picks. --unlearned lets --ability name one no party list teaches,
or one only enemies use (Fusion Dragon's Outer Space and Dragon Driver),
cast by its element's character like the rest.

--jobs N runs N processes of this program, each with its own libmgba
(default: the number of CPUs; 1 plays them here), hands each the next
cast as it finishes one, summons and Djinn first, and joins their rows
into one index.tsv, the same as one process writes.

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
        Some("probe") => probe_command(&arguments[1..]),
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
    unlearned: bool,
    verbose: bool,
    out: Option<PathBuf>,
    edition: String,
    backdrop: String,
    jobs: jobs::Jobs,
}

fn options(arguments: &[String]) -> Result<Option<Options>, String> {
    let mut o = Options {
        abilities: Vec::new(),
        all: false,
        djinn: false,
        summons: false,
        list: false,
        unlearned: false,
        verbose: false,
        out: None,
        edition: "tbs-en".to_string(),
        backdrop: "gradient".to_string(),
        jobs: jobs::Jobs::new(),
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
            "--unlearned" => o.unlearned = true,
            "--verbose" => o.verbose = true,
            "--out" => o.out = Some(PathBuf::from(value("--out")?)),
            "--edition" => o.edition = value("--edition")?,
            "--backdrop" => o.backdrop = value("--backdrop")?,
            other => {
                if !o.jobs.option(other, &mut value)? {
                    return Err(format!("unexpected argument {other:?}\n{USAGE}"));
                }
            }
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
    let mut plan = Tables::read(root)?.plan()?;
    if o.unlearned {
        // Abilities no party list teaches, and those only enemies use, cast
        // by their element's character as the others are.
        let wanted = |a: &plan::Ability| o.abilities.iter().any(|w| w.parse::<u16>().ok() == Some(a.id) || a.name.eq_ignore_ascii_case(w));
        let extra: Vec<Cast> = plan
            .skipped
            .iter()
            .filter(|(a, skip)| matches!(skip, Skip::Other | Skip::Enemy) && wanted(a))
            .filter_map(|(a, _)| plan::cast(a.clone(), false))
            .collect();
        plan.casts.extend(extra);
    }
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
    if o.all && !o.jobs.child {
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
    if o.jobs.child {
        // One cast a piece, by its file's name.
        return jobs::serve(|slug| {
            let cast = casts.iter().copied().find(|c| c.slug == slug).ok_or_else(|| format!("no cast {slug}"))?;
            let (rows, total) = play(root, &o, &[cast], &out)?;
            let mut records: Vec<String> = rows.iter().map(|r| format!("row\t{}\t{}", r.id, r.line)).collect();
            records.push(format!("bytes\t{total}"));
            Ok(records)
        });
    }
    if o.jobs.split(casts.len()) {
        // A summon plays longest, then a Djinni.
        let pieces: Vec<(String, u64)> = casts
            .iter()
            .map(|c| (c.slug.clone(), match c.command {
                Command::Summon { .. } => 3,
                Command::Djinni { .. } => 2,
                Command::Psynergy => 1,
            }))
            .collect();
        let joined = jobs::run("psynergy", arguments, &pieces, o.jobs.count)?;
        let (mut rows, mut total) = (Vec::new(), 0);
        for record in &joined.records {
            match record.split_once('\t') {
                Some(("row", line)) => {
                    if let Some((id, line)) = line.split_once('\t').and_then(|(id, line)| Some((id.parse().ok()?, line))) {
                        rows.push(Row { id, line: line.to_string() });
                    }
                }
                Some(("bytes", n)) => total += n.parse::<u64>().unwrap_or(0),
                _ => {}
            }
        }
        eprintln!("{} files, {} KiB, in {}, from {} processes", rows.len(), total.div_ceil(1024), out.display(), o.jobs.count.min(pieces.len()));
        write_index(&out.join("index.tsv"), rows)?;
        return if joined.failed.is_empty() { Ok(()) } else { Err(joined.failed.join("\n")) };
    }
    let (rows, total) = play(root, &o, &casts, &out)?;
    eprintln!("{} files, {} KiB, in {}", rows.len(), total.div_ceil(1024), out.display());
    write_index(&out.join("index.tsv"), rows)
}

/// One line of index.tsv.
struct Row {
    id: u16,
    line: String,
}

#[cfg(not(feature = "render"))]
fn play(_: &Path, _: &Options, _: &[&Cast], _: &Path) -> Result<(Vec<Row>, u64), String> {
    Err("alchemy-devtools was built without the render feature: cargo build --release --features render (needs libmgba)".to_string())
}

#[cfg(feature = "render")]
fn play(root: &Path, o: &Options, casts: &[&Cast], out: &Path) -> Result<(Vec<Row>, u64), String> {
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
            ) + "\t",
        });
    }
    Ok((rows, total))
}

/// A field cast's row also names the scene it is cast in; a battle cast's
/// scene is empty.
const INDEX_HEADER: &str = "ability\tname\tcaster\ttarget\tframes\tfile\tscene";

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
         [--event NAME]... [--out DIR] [--edition tbs-en] [--jobs N]
Plays a built ☀️ edition (make tbs-en) under libmgba from a new game's
defaults straight into each scene, and writes PNG files at the GBA's size
into DIR/<scene>/ (DIR defaults to out/EDITION/places), with DIR/index.tsv
listing every image (scene, number, area, variant, kind, entrance, event,
page, the page's message, frames, leads_to, note, the event's messages,
file) and DIR/skipped.tsv
what was left out and why. --all takes every scene but the title, the
clear-data screen and the debug test rooms. The tables are the story tools'
places.tsv (scenes), events.tsv (each event's trigger, flags and messages)
and flags.tsv (flag names).

place: the photo of the party arriving, once the player has control
(entrance.png): entered by the lowest-numbered warp destination another
map's warp leads to (MAP_WARP in the source's warp tables), the Sanctum's,
the menus' and unused maps' warps after every other, so the game's own
arrival places the party and the camera. The next way in is tried when an
arrival goes on to another map before the player has control, or leaves
the leader without a sprite, on a wall cell, off his ground, outside his
room or the screen, in black, on another actor or where he can walk almost
nowhere; when every way does, the first such photo is kept and skipped.tsv
says why, and when every way goes on to another map, the photo is of the
first arrival's scene, half a second before its first message.
And a map of each room, one camera box of the scene's entrances, named
by the first entrance that reaches it (map-<entrance>.png),
stitched from one moment with the leader hidden. Then the same at each
moment of the scene's story: each sets one more of the named story flags
its initialization tests, in the order the events' conditions give, so
each is a state the player could see (the stormy night's fallen rocks,
Garet joining, the Boulder falling, Felix in the river), with a period
cleared once a later flag shows it over (FLAG_VALE_STORM once the party
has left Vale). A moment draws only the rooms whose placed actors or
map cells differ from every state drawn before, into
DIR/<scene>_<moment>/, the moment named by its flag without FLAG_,
lowercase with hyphens (vale-storm_vale-storm-saw-river-rescue/); one
that changes nothing is dropped, and at most 12 are drawn a scene
(skipped.tsv names the rest). A scene whose initialization tests
FLAG_KOLIMA_CURSE_LIFTED is drawn cursed at the new game's state, in
DIR/<scene>_cursed/, and cured at that moment, in DIR/<scene>_cured/.
index.tsv's variant column gives each moment's name and flags.

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
after each of the scene's other events. Pages are held 45 frames. An exit
is followed into the scene it leads to: the fade out, the load, that
scene's arrival (its pages answered, none kept) and its fade in, until the
player has control there as at a scene's start and half a second more,
with each run of a blank screen cut to its first 8 frames so the GIF
never sits on black. An arrival is followed for at most 10 seconds from
the load; one that runs longer, or that begins a battle, ends there, and
index.tsv's note column says so. index.tsv's leads_to column names the scene an event places the
party in next by its SceneId symbol (its own scene for a staircase to
another of its entrances). An event that runs past four minutes gets no
GIF, noted in skipped.tsv. --messages is accepted and not read.

--jobs N runs N processes of this program, each with its own libmgba
(default: the number of CPUs; 1 plays it all here), and hands each the
next piece of work as it finishes one, the costliest first: a scene for
place, weighed by its story moments, and for event one event of an
overlay, played against all of the overlay's scenes as one process would,
weighed by its triggers, the scenes it may play at and the frames its
pictures took in an earlier run's index.tsv. The scenes' old event
pictures are cleared first. The parent joins their index.tsv, skipped.tsv
and variant-flags.tsv lines into the files one process would write.


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
    jobs: jobs::Jobs,
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
        jobs: jobs::Jobs::new(),
    };
    let mut options = arguments.iter();
    while let Some(argument) = options.next() {
        let mut value =
            |name: &str| options.next().cloned().ok_or_else(|| format!("{name} needs a value\n{SCENES_USAGE}"));
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
            other => {
                if !o.jobs.option(other, &mut value)? {
                    return Err(format!("unexpected argument {other:?}\n{SCENES_USAGE}"));
                }
            }
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
    let request = SceneRequest { kind, scenes: &scene_rows, chosen: &chosen, only: &only, events: &events, wanted: &o.events, flags: &flags };
    play_scenes(root, &o.edition, &request, &out, &o.jobs, arguments)
}

/// What one `render place` or `render event` plays.
#[cfg_attr(not(feature = "render"), allow(dead_code))]
struct SceneRequest<'a> {
    kind: &'a str,
    /// Every scene of the table, to name the scene an exit leads to.
    scenes: &'a [story::Scene],
    chosen: &'a [&'a story::Scene],
    only: &'a [u16],
    events: &'a [story::Event],
    wanted: &'a [String],
    flags: &'a story::Flags,
}

/// What a scene's work is part of: the scene itself for places, its
/// overlay's source for events, whose rows bring the overlay's other scenes
/// along.
#[cfg(feature = "render")]
fn unit_key(kind: &str, scene: &story::Scene) -> String {
    if kind == "place" { scene.number.to_string() } else { scene.file.clone() }
}

#[cfg(not(feature = "render"))]
fn play_scenes(_: &Path, _: &str, _: &SceneRequest, _: &Path, _: &jobs::Jobs, _: &[String]) -> Result<(), String> {
    Err("alchemy-devtools was built without the render feature: cargo build --release --features render (needs libmgba)".to_string())
}

/// What a run of `render place` or `render event` recorded, before it is
/// written into index.tsv and skipped.tsv: a child's part, or the sum of
/// them.
#[cfg(feature = "render")]
#[derive(Default)]
struct Outcome {
    /// index.tsv's lines, by file.
    rows: Vec<(String, String)>,
    skipped: Vec<places::Skip>,
    cleared: Vec<(String, String)>,
    bytes: u64,
    images: usize,
    gifs: usize,
    gif_bytes: u64,
    /// Story moments drawn, the scenes with any, the scenes capped.
    moments: [usize; 3],
}

#[cfg(feature = "render")]
impl Outcome {
    fn add(&mut self, context: places::Context) {
        self.rows.extend(context.rows.iter().map(|r| (r.file.clone(), r.line())));
        self.skipped.extend(context.skipped);
        self.cleared.extend(context.cleared);
        self.bytes += context.bytes;
        self.images += context.images;
        self.gifs += context.gifs;
        self.gif_bytes += context.gif_bytes;
        for (total, n) in self.moments.iter_mut().zip([context.moments, context.moment_scenes, context.capped]) {
            *total += n;
        }
    }

    /// The part file's records: `row`, `skip`, `clear` and one `count`.
    fn records(&self) -> Vec<String> {
        let mut records: Vec<String> = self.rows.iter().map(|(_, line)| format!("row\t{line}")).collect();
        records.extend(self.skipped.iter().map(|s| format!("skip\t{}\t{}\t{}", s.scene, s.what, s.reason)));
        records.extend(self.cleared.iter().map(|(scene, what)| format!("clear\t{scene}\t{what}")));
        let [m, s, c] = self.moments;
        records.push(format!("count\t{}\t{}\t{}\t{}\t{m}\t{s}\t{c}", self.bytes, self.images, self.gifs, self.gif_bytes));
        records
    }

    fn read(&mut self, records: &[String]) {
        for record in records {
            let fields: Vec<&str> = record.splitn(4, '\t').collect();
            match fields[..] {
                ["row", ..] => {
                    let line = &record["row\t".len()..];
                    self.rows.push((line.rsplit('\t').next().unwrap_or_default().to_string(), line.to_string()));
                }
                ["skip", scene, what, reason] => {
                    self.skipped.push(places::Skip { scene: scene.to_string(), what: what.to_string(), reason: reason.to_string() })
                }
                ["clear", scene, what] => self.cleared.push((scene.to_string(), what.to_string())),
                ["count", ..] => {
                    let n: Vec<u64> = record.split('\t').skip(1).map(|n| n.parse().unwrap_or(0)).collect();
                    if let [bytes, images, gifs, gif_bytes, m, s, c] = n[..] {
                        self.bytes += bytes;
                        self.images += images as usize;
                        self.gifs += gifs as usize;
                        self.gif_bytes += gif_bytes;
                        for (total, n) in self.moments.iter_mut().zip([m, s, c]) {
                            *total += n as usize;
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Plays the scenes asked for, in this process or shared out among --jobs
/// children, and writes index.tsv, skipped.tsv and for places
/// variant-flags.tsv; a child plays the pieces it is handed instead.
#[cfg(feature = "render")]
fn play_scenes(root: &Path, edition: &str, r: &SceneRequest, out: &Path, jobs: &jobs::Jobs, arguments: &[String]) -> Result<(), String> {
    let started = std::time::Instant::now();
    std::fs::create_dir_all(out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    // A place's work is a scene, an event's the scenes of one overlay (an
    // event plays at every scene its row lists).
    let mut units: BTreeMap<String, Vec<&story::Scene>> = BTreeMap::new();
    let mut order = Vec::new();
    for s in r.chosen {
        let key = unit_key(r.kind, s);
        if !units.contains_key(&key) {
            order.push(key.clone());
        }
        units.entry(key).or_default().push(*s);
    }
    if r.kind == "event" {
        order.sort();
    }
    if jobs.child {
        // A piece is a scene by number, or an overlay's event by its source
        // and name; the overlay's scans are kept for its next events.
        let machines = Machines::load(root, edition, r.chosen, false)?;
        let mut kept: std::collections::HashMap<String, places::Job> = std::collections::HashMap::new();
        return jobs::serve(|key| {
            let mut context = machines.context(r, out);
            if r.kind == "place" {
                let scene = r.chosen.iter().find(|s| s.number.to_string() == key).ok_or_else(|| format!("no scene {key}"))?;
                context.place(scene);
                eprintln!("{}: {} images", scene.name(), context.images);
            } else {
                let (file, name) = key.split_once('\t').ok_or_else(|| format!("no event {key:?}"))?;
                let scenes = units.get(file).ok_or_else(|| format!("no overlay {file}"))?;
                context.play_events(scenes, Some(name), kept.entry(file.to_string()).or_default());
                eprintln!("{name}: {} images", context.images);
            }
            let mut piece = Outcome::default();
            piece.add(context);
            Ok(piece.records())
        });
    }
    let candidates = places::variant_candidates(r.events, r.flags);
    let mut outcome = Outcome::default();
    let mut failed = Vec::new();
    let mut processes = 1;
    let pieces: Vec<(String, u64)> = if jobs.count > 1 { scene_pieces(r, &units, &order, &candidates, out) } else { Vec::new() };
    if jobs.split(pieces.len()) {
        if r.kind == "event" {
            for key in &order {
                outcome.cleared.extend(places::clear_events(out, &units[key], r.wanted, r.only));
            }
        }
        let rom_path = root.join("out").join(edition).join(format!("{edition}.gba"));
        let rom = std::fs::read(&rom_path).map_err(|e| format!("cannot read {}: {e} (make {edition})", rom_path.display()))?;
        note_modified(root, edition, &rom);
        processes = jobs.count.min(pieces.len());
        let joined = jobs::run(r.kind, arguments, &pieces, jobs.count)?;
        outcome.read(&joined.records);
        failed = joined.failed;
    } else {
        let machines = Machines::load(root, edition, r.chosen, true)?;
        for key in &order {
            let job = &units[key];
            let mut context = machines.context(r, out);
            if r.kind == "place" {
                for scene in job {
                    context.place(scene);
                }
            } else {
                context.events(job);
            }
            eprintln!("{}: {} images", job.iter().map(|s| s.name()).collect::<Vec<_>>().join(", "), context.images);
            outcome.add(context);
        }
    }
    if r.kind == "place" {
        eprintln!(
            "{} story moments drawn in {} scenes; {} scenes hit the cap of {} moments (skipped.tsv)",
            outcome.moments[0],
            outcome.moments[1],
            outcome.moments[2],
            places::MAX_MOMENTS
        );
        let mut text = String::from("scene\tnumber\tflags\n");
        for (number, list) in &candidates {
            let name = r.chosen.iter().find(|s| s.number == *number).map_or_else(|| format!("#{number}"), |s| s.name());
            text.push_str(&format!("{name}\t{number}\t{}\n", list.join(", ")));
        }
        let path = out.join("variant-flags.tsv");
        std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    places::write_index(out, &outcome.rows)?;
    let path = out.join("skipped.tsv");
    // The lines already there for other scenes and things are kept.
    let mut lines: BTreeMap<(String, String), String> = BTreeMap::new();
    for line in std::fs::read_to_string(&path).unwrap_or_default().lines().skip(1) {
        let mut fields = line.split('\t');
        if let (Some(scene), Some(what)) = (fields.next(), fields.next()) {
            lines.insert((scene.to_string(), what.to_string()), line.to_string());
        }
    }
    // What played this time is no longer left out; a scene whose events
    // were all played again keeps none of its old event lines.
    for (scene, what) in &outcome.cleared {
        if what == "event *" {
            lines.retain(|(s, w), _| s != scene || !w.starts_with("event "));
        } else {
            lines.remove(&(scene.clone(), what.clone()));
        }
    }
    for s in &outcome.skipped {
        lines.insert((s.scene.clone(), s.what.clone()), format!("{}\t{}\t{}", s.scene, s.what, s.reason));
    }
    let mut text = String::from("scene\twhat\treason\n");
    for line in lines.values() {
        text.push_str(line);
        text.push('\n');
    }
    std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    eprintln!(
        "{} images ({} GIFs, {} KiB), {} KiB, {} left out, in {:.0} s from {processes} process{}, in {}",
        outcome.images,
        outcome.gifs,
        outcome.gif_bytes.div_ceil(1024),
        outcome.bytes.div_ceil(1024),
        outcome.skipped.len(),
        started.elapsed().as_secs_f64(),
        if processes == 1 { "" } else { "es" },
        out.display()
    );
    if failed.is_empty() { Ok(()) } else { Err(failed.join("\n")) }
}

/// What every scene's machines are made from: the ROM, the field's symbols
/// from the linker map, and each scene's functions by name, from its
/// overlay's ELF in the build (an event named by its function is told from
/// the other rows of its trigger by that function's address).
#[cfg(feature = "render")]
struct Machines {
    rom: Vec<u8>,
    symbols: field::Symbols,
    functions: std::collections::HashMap<u16, std::collections::HashMap<String, u32>>,
    /// Each map's arrivals from other maps, by map number, from the source.
    arrivals: std::collections::HashMap<u16, Vec<arrivals::Arrival>>,
}

#[cfg(feature = "render")]
impl Machines {
    fn load(root: &Path, edition: &str, chosen: &[&story::Scene], note: bool) -> Result<Machines, String> {
        let build = root.join("out").join(edition);
        let map_path = build.join(format!("{edition}.map"));
        let rom_path = build.join(format!("{edition}.gba"));
        let map = std::fs::read(&map_path).map_err(|e| format!("cannot read {}: {e} (make {edition})", map_path.display()))?;
        let map = crate::ld_map::Map::parse(&String::from_utf8_lossy(&map));
        let rom = std::fs::read(&rom_path).map_err(|e| format!("cannot read {}: {e} (make {edition})", rom_path.display()))?;
        if note {
            note_modified(root, edition, &rom);
        }
        let mut symbols = field::Symbols::find(&map)?;
        symbols.read_defeat(&build.join(format!("{edition}.elf")))?;
        let mut functions = std::collections::HashMap::new();
        for s in chosen {
            let Some(id) = s.overlay else {
                continue;
            };
            let path = build.join("overlays").join(format!("resource_{id:03x}.elf"));
            let Some(elf) = std::fs::read(&path).ok().and_then(|data| crate::elf::Elf::parse(&data).ok()) else {
                continue;
            };
            let named = elf.symbols.iter().filter(|y| y.kind == 2 && !y.name.is_empty()).map(|y| (y.name.clone(), y.value & !1)).collect();
            functions.insert(s.number, named);
        }
        let arrivals = arrivals::read(root)?;
        Ok(Machines { rom, symbols, functions, arrivals })
    }

    /// A fresh context for one piece of work.
    fn context<'a>(&'a self, r: &'a SceneRequest, out: &Path) -> places::Context<'a> {
        places::Context {
            rom: &self.rom,
            scenes: r.scenes,
            symbols: &self.symbols,
            flags: r.flags,
            events: r.events,
            out: out.to_path_buf(),
            rows: Vec::new(),
            skipped: Vec::new(),
            bytes: 0,
            images: 0,
            gifs: 0,
            gif_bytes: 0,
            wanted: r.wanted,
            only: r.only,
            cleared: Vec::new(),
            functions: &self.functions,
            arrivals: &self.arrivals,
            moments: 0,
            moment_scenes: 0,
            capped: 0,
            relaxed: std::cell::Cell::new(false),
        }
    }
}

/// The pieces --jobs hands out, each with its estimated cost in frames: a
/// place's scene by its story moments, and each event of an overlay by its
/// triggers times the scenes it may play at, plus the frames its pictures
/// took in the index an earlier run left in `out`.
#[cfg(feature = "render")]
fn scene_pieces(
    r: &SceneRequest,
    units: &BTreeMap<String, Vec<&story::Scene>>,
    order: &[String],
    candidates: &BTreeMap<u16, Vec<String>>,
    out: &Path,
) -> Vec<(String, u64)> {
    const ATTEMPT: u64 = 600;
    if r.kind == "place" {
        return order
            .iter()
            .map(|key| {
                let moments = candidates.get(&units[key][0].number).map_or(0, |list| list.len().min(places::MAX_MOMENTS));
                (key.clone(), ATTEMPT * (1 + moments as u64))
            })
            .collect();
    }
    // index.tsv's frames by scene number and event.
    let mut frames: std::collections::HashMap<(u16, String), u64> = std::collections::HashMap::new();
    for line in std::fs::read_to_string(out.join("index.tsv")).unwrap_or_default().lines().skip(1) {
        let fields: Vec<&str> = line.split('\t').collect();
        if let (Some(number), Some(event), Some(n)) = (fields.get(1), fields.get(6), fields.get(12)) {
            if let (Ok(number), Ok(n)) = (number.parse(), n.parse::<u64>()) {
                *frames.entry((number, event.to_string())).or_default() += n;
            }
        }
    }
    let mut pieces = Vec::new();
    for key in order {
        let scenes = &units[key];
        for event in places::job_events(r.events, r.wanted, r.only, scenes) {
            let at: Vec<u16> = scenes.iter().map(|s| s.number).filter(|n| event.scenes.contains(n)).collect();
            if at.is_empty() {
                continue;
            }
            let before: u64 = at.iter().map(|&n| frames.get(&(n, event.name.clone())).copied().unwrap_or(0)).sum();
            pieces.push((format!("{key}\t{}", event.name), ATTEMPT * (event.alternatives.len().max(1) * at.len()) as u64 + before));
        }
    }
    pieces
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
              EventScript_DepartForQuest (ValeHome's entrance 12), through the
              town's farewell, and the town gathered
  lighthouse  venus-lighthouse-lit.gif and .png: the world map cut at its
              entrance 91 after Venus Lighthouse's beacon erupts
  mercury     mercury-lighthouse-lit.gif and .png: the world map cut at its
              entrance 90 after Mercury Lighthouse's beacon is lit, and
              mercury-lighthouse-aerie-arrival.gif: the Lighthouse Aerie it
              goes on to, until the player has control there
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

const PROBE_USAGE: &str = "usage: alchemy-devtools render probe --flags flags.tsv --only NAME... [--out DIR] [--edition tbs-en]
Plays a built ☀️ edition and prints what it reads for open questions only the
running game settles.";

fn probe_command(arguments: &[String]) -> Result<(), String> {
    let (mut only, mut out, mut edition, mut flags_tsv) = (Vec::new(), None, "tbs-en".to_string(), None);
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        let mut value = |name: &str| arguments.next().cloned().ok_or_else(|| format!("{name} needs a value\n{PROBE_USAGE}"));
        match argument.as_str() {
            "--only" => only.push(value("--only")?),
            "--out" => out = Some(PathBuf::from(value("--out")?)),
            "--edition" => edition = value("--edition")?,
            "--flags" => flags_tsv = Some(PathBuf::from(value("--flags")?)),
            other => return Err(format!("unexpected argument {other:?}\n{PROBE_USAGE}")),
        }
    }
    let flags = story::Flags::read(&flags_tsv.ok_or_else(|| format!("--flags is needed\n{PROBE_USAGE}"))?)?;
    let root = crate::compile::root();
    let out = out.unwrap_or_else(|| root.join("out").join(&edition).join("probe"));
    play_probe(root, &edition, &only, &flags, &out)
}

#[cfg(not(feature = "render"))]
fn play_probe(_: &Path, _: &str, _: &[String], _: &story::Flags, _: &Path) -> Result<(), String> {
    Err("alchemy-devtools was built without the render feature".to_string())
}

#[cfg(feature = "render")]
fn play_probe(root: &Path, edition: &str, only: &[String], flags: &story::Flags, out: &Path) -> Result<(), String> {
    let build = root.join("out").join(edition);
    let map = std::fs::read(build.join(format!("{edition}.map"))).map_err(|e| format!("cannot read the map: {e} (make {edition})"))?;
    let map = crate::ld_map::Map::parse(&String::from_utf8_lossy(&map));
    let rom = std::fs::read(build.join(format!("{edition}.gba"))).map_err(|e| format!("cannot read the ROM: {e} (make {edition})"))?;
    let symbols = field::Symbols::find(&map)?;
    std::fs::create_dir_all(out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    let mut guide = guide::Guide { rom: &rom, symbols: &symbols, flags, out, map: &map, written: Vec::new(), casts: Vec::new() };
    let mut failed = Vec::new();
    for name in probe::PROBES.iter().chain(probe_places::PROBES).filter(|p| only.is_empty() || only.iter().any(|o| o == *p)) {
        if let Err(e) = guide.probe(name, &map) {
            eprintln!("{name}: {e}");
            failed.push(*name);
        }
    }
    for (file, bytes, what) in &guide.written {
        eprintln!("{}\t{} KiB\t{what}", out.join(file).display(), bytes.div_ceil(1024));
    }
    if failed.is_empty() { Ok(()) } else { Err(format!("{} failed", failed.join(", "))) }
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
        let update = map.symbol("WorldMap_UpdateCurrentTileBlock").map(|(_, a)| a).ok_or("the map has no WorldMap_UpdateCurrentTileBlock")?;
        if let Err(e) = front.world(update) {
            eprintln!("world: {e}");
            failed.push("world");
        }
    }
    for name in ["title", "farewell", "lighthouse", "mercury"] {
        if wanted(name) {
            let result = match name {
                "title" => front.title(),
                "farewell" => front.farewell(4607, 4613, 4628),
                "mercury" => front.mercury_lighthouse(),
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

const GUIDE_USAGE: &str = "usage: alchemy-devtools render guide --flags flags.tsv [--only NAME]... [--cast NAME]...
         [--out DIR] [--edition tbs-en] [--jobs N]
Plays a built ☀️ edition (make tbs-en) with the pad held as a player would,
for the folder guides, and writes into DIR (default out/EDITION/guide):
  walk        walk-run.gif: Vale, walking right, then running back with B
  snow        snow.gif: Imil, the leader's footprints in the snow
  layers      layers.gif: Vale built up layer by layer, BG3 to BG1
  layers-3d   layers-3d.gif: Vale's layers pulled apart into sheets in 3D,
              and planes/field/: each plane alone as a transparent PNG with
              planes.tsv, the order the registers give (guide_planes.rs)
  battle-layers-3d, world-map-layers-3d, menu-layers-3d, talk-layers-3d,
  shop-layers-3d
              a Goma Cave battle, the world map, the field menu, a message and
              Bilibin's item shop asking what to buy,
              pulled apart the same way, each as NAME.gif, with its
              planes in planes/NAME/
  scene-actors scene-actors.png: Vale at twice its size, each actor labelled
              with its actor number
  debug-rooms debug-level-room.png, debug-menu-room.png: the debug level room
              and menu test room, their actors labelled
  debug-room-talks
              debug-level-room-N.png and .gif: each placed actor N of the debug
              level room talked to, and the screen two seconds after A
  moment      moment.gif, moment-F.png: a scene entered at MOMENT_PLACE
              (scene,entrance) with MOMENT_SET's flags and left to play for
              MOMENT_FRAMES, every MOMENT_EVERY-th frame from MOMENT_FROM
              labelled; A every MOMENT_A frames; for each of MOMENT_ACTORS,
              what it adds to the screen (moment-F-actor-N.png and -pair.png:
              with it and moved away); MOMENT_OAM prints blended sprites;
              MOMENT_PLANES writes those frames' planes in planes/moment-F/;
              MOMENT_CELLS prints where the map holds those cell codes;
              MOMENT_STAND (x,z) stands the leader there and MOMENT_HOLD
              (keys,frames) then holds the pad, as a player walks
  status-layers-3d
              status-layers-3d.gif: the Status screen pulled apart
  world-map   world-map-walk.gif: the world map's perspective view, walking
  menus, menu-start, items, boot
              the field menu, Start closing it, the Item screen, power-on
  field-psynergy
              <name>.gif for Reveal, Move, Avoid, Mind Read, Catch, Retreat,
              Force, Lift, Carry, Cloak and Halt, each cast where a scene
              answers it, with their rows in DIR/index.tsv (--cast picks some)
  talk        talk.gif and window-open.png: a villager's message window
  djinn-room  djinn-room.gif: Flint given out in the Djinn test room
  shop, inn   shop.gif and inn.gif: Bilibin's item shop and inn
  menu-psynergy
              menu-psynergy.gif: Move chosen from the menu, then cast
  credits     credits.gif and credits-layers.png: the credits' crossfade
  sleep       sleep.gif: L and R held until the screen turns white
  oam         oam.png: a frame's sprite list entries outlined
  vram        vram.png: video memory as tiles, and the palettes
  cells       cells.png: a screen's ground cells, collision and heights
  grid        grid.png: a map's whole cell grid and its layers' windows
  random-battle
              random-battle.gif: walking until a random battle opens
  world-map-picture
              world-map-picture.gif: the world map's still picture (R)
  overlay     overlay.gif: walking into the Sanctum beside the overlay area
Take-apart pictures for places and guides, each as its mode's name with
.png or .gif (guide_take.rs):
  tret-tree-branch-split, kolima-forest-tree-healing, altmiller-cave-darkness,
  bilibin-cave-third-puddle, fuchin-falls-cave-logs, mercury-lighthouse-pool,
  lunpa-fortress-patrol, tolbi-spring-toss, kolima-forest-entrance-fields,
  tret-tree-interiors-cocoons, colosseum-azart-race, colosseum-navampa-scales,
  colosseum-satrage-lifts, tunnel-ruins-balance, venus-lighthouse-aerie-eruption
  (and venus-lighthouse-aerie-layers-3d.gif), scene-event-cells,
  credits-tile-ring, title-intro-layers-3d, item-screen-windows, sound-players,
  field-requests, grid-planes, tile-animation, world-map-scanlines,
  world-map-footprint
Take-apart pictures for the battle, effect, field and shop guides
(guide_apart.rs):
  round-events      round-events.gif: Quake's battle events one frame each,
                    beside the event queue
  battle-places     battle-places.gif: aiming Quake through the command menu,
                    beside the battlefield's places seen from above
  effect-canvas     effect-canvas.gif: Heat Wave beside its 128 by 128 canvas
  effect-transfer   effect-transfer.gif: Heat Wave's canvas under each
                    canvas transfer mode, forced
  effect-particles  effect-particles.gif: Heat Wave beside its particle pools
  sprite-layers     sprite-layers.gif: Isaac's battle sprite, each layer
                    composed alone and together
  shop-counters     shop-counters.png: Bilibin's weapon shop before and after
                    Vault's, its Buy and Artifacts lists
Take-apart pictures for the window and shop guides (guide_window.rs; work-block
also reads out/tbs-ja, make tbs-ja):
  work-block        work-block.gif: the window work block byte by byte in the
                    Japanese and international layouts as a message shows
  text-ring         text-ring.gif: a message decoded into the ring, typed out
  render-outputs    render-outputs.gif: a message's render outputs outlined
  area-name         area-name.gif: Bilibin's area name and its countdown
  sanctum           sanctum.gif: the Vale Sanctum's Revive and its members
  item-picker       item-picker.gif: a Nut given with every bag full
  menu-slot         menu-slot.png: the menu work slot as a counter and as the
                    Item screen read it
Pictures for the battle guide's rules (guide_battle.rs):
  enemy-actions     enemy-actions.png: a Gryphon's eight actions, the
                    weights its strategy picks them by and the place each
                    of the 256 rolls takes
  first-strikes     first-strikes.png: a random encounter opened with a first
                    strike and with a surprise, through the first round
  element-multiplier
                    element-multiplier.png: gElement_PowerResistCurve as its
                    line, with Quake's and an attack's targets on it
  plan-rolls        plan-rolls.png: a miss, a critical hit and a triple blow
                    beside the plan's rolls
  staging           staging.gif: Quake under each outcome, STAGING_CAST to
                    STAGING_FULL_SCREEN
  added-effects     added-effects.png: Impair, Delude and Guard's stages and
                    counters over nine rounds
  after-target      after-target.png: a sleeper woken by Quake, and a haunted
                    caster's evil spirit taking its share
Take-apart pictures for the battle effects guide (guide_effect.rs):
  effect-variants   effect-variants.gif: Quake, Earthquake and Quake Sphere
                    side by side, beside their plan words
  djinn-attacks     djinn-attacks.gif: Venus, Mercury, Mars and Jupiter
                    beside their flames and motes
  spark-groups      spark-groups.gif: Supernova beside its flashes and sparks
  stat-reductions   stat-reductions.gif: Debilitate beside each target's shards
  target-strikes    target-strikes.gif: Twin Beaks and Acid Bite beside
                    their parked smoke, motes and sparks
Recordings for the battle pages, the game alone (guide_fight.rs), each
into battle/ or battle_anim/ under DIR:
  fight-opening, fight-commands, fight-aim, fight-round, fight-turn-order,
  fight-first-strike, fight-flee, fight-gryphons, fight-twin-beaks,
  fight-unleash, fight-rolls, fight-wake, fight-won, fight-bone-charge,
  fight-ally-search
Take-apart pictures for the menu guide (guide_menu.rs), in Xian:
  psynergy-steps    psynergy-steps.png: Cure used on Garet, a panel for each
                    step of the Psynergy screen
  djinn-preview     djinn-preview.png: Flint put on standby, the preview beside
                    Isaac's unit before and after
  status-pages      status-pages.png: the Status screen's member row and pages,
                    the four Djinn figures outlined
  status-panels     status-panels.png: the status panel once for each kind
  item-sorting      item-sorting.png: Isaac's items in each of the four orders
Take-apart pictures for Vale, Sol Sanctum and the Sanctum (guide_places.rs):
  sanctum-account, star-room-rising, eruption-departures, statue-sky-rows,
  vale-opening, vale-gate-display, vale-interiors-sleep, vale-kraden-rockslide,
  vale-cave-kite
Take-apart pictures for the engine's guides, each as its mode's name with
.png or .gif (guide_engine.rs):
  menu-test-setup, palette-fade, scheduler, io-write-queue,
  resource-directory, memory-maps, map-header, placeholders, markers,
  world-map-squares, colosso-path, player-probes, item-row, scene-script,
  temporary-flags, overlay-common
Recordings for the front page, the engine page and the runtime, graphics,
sound and debug guides, each under its guide folder (guide_front.rs):
  front-door, front-area-name, front-warp-rows, front-story,
  front-chest, engine-waiting, engine-debug-select, runtime-soft-reset,
  runtime-save, graphics-title-fade, graphics-credits-names,
  sound-inn-fanfare, debug-map-select, debug-level-editor,
  debug-felix-facing
Recordings for the world guides (guide_world_play.rs), each as
FOLDER/NAME.gif under DIR, the guide it opens a section of:
  world-door, world-wall-slide, world-wooden-box, world-behind-roof,
  world-map-battle (world/), field-talk-walk, field-well-route,
  field-step-height, field-world-map-zoom (field/), overworld-battle-return,
  overworld-move-nothing (overworld/), layout-ledge, layout-waterfall
  (layout/), map-arrive, map-talk-twice (map/), layout-haze,
  layout-psynergy-stone (layout/), map-colosso-race, map-venus-star-thrown
  (map/, guide_docs_world.rs); overworld-battle-return also
  writes field/battle-swing.gif
Recordings for the menu, window, shop and game guides, one GIF each
(guide_menus.rs):
  windows-zoom, message-speed, message-blanks, pause-menu, save-game,
  option-menu, naming-screen, area-name-arrival, field-menu-icons,
  psynergy-cure, item-use, djinn-standby, summary-screen, status-panel,
  item-sort, shop-counter, shop-long-sword, game-ticket, shop-sell,
  shop-repair, shop-artifacts, sanctum-revive, item-picker-full,
  inventory-slots
--only picks some; flags.tsv is the story tools' flag names. --jobs N hands the
modes, and field-psynergy's casts one by one, to N processes of this program
as each finishes one (default: the number of CPUs; 1 plays them here),
joining their index.tsv rows.";


fn guide_pictures(arguments: &[String]) -> Result<(), String> {
    let (mut only, mut out, mut edition, mut flags_tsv) = (Vec::new(), None, "tbs-en".to_string(), None);
    let mut casts = Vec::new();
    let mut jobs = jobs::Jobs::new();
    let mut options = arguments.iter();
    while let Some(argument) = options.next() {
        let mut value = |name: &str| options.next().cloned().ok_or_else(|| format!("{name} needs a value\n{GUIDE_USAGE}"));
        match argument.as_str() {
            "-h" | "--help" => {
                println!("{GUIDE_USAGE}");
                return Ok(());
            }
            "--cast" => casts.push(value("--cast")?),
            "--only" => only.push(value("--only")?),
            "--out" => out = Some(PathBuf::from(value("--out")?)),
            "--edition" => edition = value("--edition")?,
            "--flags" => flags_tsv = Some(PathBuf::from(value("--flags")?)),
            other => {
                if !jobs.option(other, &mut value)? {
                    return Err(format!("unexpected argument {other:?}\n{GUIDE_USAGE}"));
                }
            }
        }
    }
    let flags = story::Flags::read(&flags_tsv.ok_or_else(|| format!("--flags is needed\n{GUIDE_USAGE}"))?)?;
    let root = crate::compile::root();
    let out = out.unwrap_or_else(|| root.join("out").join(&edition).join("guide"));
    play_guide(root, &edition, &only, &casts, &flags, &out, &jobs, arguments)
}

#[cfg(not(feature = "render"))]
#[allow(clippy::too_many_arguments)]
fn play_guide(_: &Path, _: &str, _: &[String], _: &[String], _: &story::Flags, _: &Path, _: &jobs::Jobs, _: &[String]) -> Result<(), String> {
    Err("alchemy-devtools was built without the render feature: cargo build --release --features render (needs libmgba)".to_string())
}

/// The prefix of a share key that names one field Psynergy cast.
#[cfg(feature = "render")]
const FIELD_CAST_KEY: &str = "field-psynergy=";

#[cfg(feature = "render")]
#[allow(clippy::too_many_arguments)]
fn play_guide(
    root: &Path,
    edition: &str,
    only: &[String],
    casts: &[String],
    flags: &story::Flags,
    out: &Path,
    jobs: &jobs::Jobs,
    arguments: &[String],
) -> Result<(), String> {
    let wanted = |name: &str| only.is_empty() || only.iter().any(|o| o == name);
    // The work in pieces: each mode, and field-psynergy's casts one by one.
    let mut units: Vec<String> = Vec::new();
    for &name in guide::MODES.iter().chain(guide_engine::MODES).chain(guide_front::MODES).chain(guide_world_play::MODES).chain(guide_fight::MODES).chain(guide_menus::MODES).filter(|&&name| wanted(name)) {
        if name == "field-psynergy" {
            let cast_wanted = |c: &str| casts.is_empty() || casts.iter().any(|w| w.eq_ignore_ascii_case(c));
            units.extend(guide::field_cast_names()?.into_iter().filter(|c| cast_wanted(c)).map(|c| format!("{FIELD_CAST_KEY}{c}")));
        } else {
            units.push(name.to_string());
        }
    }
    let mut written: Vec<(String, u64, String)> = Vec::new();
    let mut rows: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    let mut children_failed = Vec::new();
    if jobs.split(units.len()) {
        let pieces: Vec<(String, u64)> = units.iter().map(|u| (u.clone(), 1)).collect();
        let rom = std::fs::read(root.join("out").join(edition).join(format!("{edition}.gba")))
            .map_err(|e| format!("cannot read the ROM: {e} (make {edition})"))?;
        note_modified(root, edition, &rom);
        std::fs::create_dir_all(out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
        let joined = jobs::run("guide", arguments, &pieces, jobs.count)?;
        children_failed = joined.failed;
        for record in &joined.records {
            match record.split_once('\t') {
                Some(("written", rest)) => {
                    let fields: Vec<&str> = rest.splitn(3, '\t').collect();
                    if let [file, bytes, what] = fields[..] {
                        written.push((file.to_string(), bytes.parse().unwrap_or(0), what.to_string()));
                    }
                }
                Some(("cast", line)) => rows.push(line.to_string()),
                Some(("failed", name)) => failed.push(name.to_string()),
                _ => {}
            }
        }
        // Failures in the order the modes play.
        failed.sort_by_key(|name| guide::MODES.iter().chain(guide_engine::MODES).chain(guide_front::MODES).chain(guide_world_play::MODES).chain(guide_fight::MODES).chain(guide_menus::MODES).position(|m| m == name));
        failed.dedup();
    } else {
        let build = root.join("out").join(edition);
        let map = std::fs::read(build.join(format!("{edition}.map"))).map_err(|e| format!("cannot read the map: {e} (make {edition})"))?;
        let map = crate::ld_map::Map::parse(&String::from_utf8_lossy(&map));
        let rom = std::fs::read(build.join(format!("{edition}.gba"))).map_err(|e| format!("cannot read the ROM: {e} (make {edition})"))?;
        if !jobs.child {
            note_modified(root, edition, &rom);
        }
        let symbols = field::Symbols::find(&map)?;
        std::fs::create_dir_all(out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
        let mut guide = guide::Guide { rom: &rom, symbols: &symbols, flags, out, map: &map, written: Vec::new(), casts: Vec::new() };
        if jobs.child {
            // A piece is a mode, or one of field-psynergy's casts.
            return jobs::serve(|unit| {
                let (name, these) = match unit.strip_prefix(FIELD_CAST_KEY) {
                    Some(cast) => ("field-psynergy", vec![cast.to_string()]),
                    None => (unit, casts.to_vec()),
                };
                let mut records = Vec::new();
                if let Err(e) = guide.run(name, &these) {
                    eprintln!("{name}: {e}");
                    records.push(format!("failed\t{name}"));
                }
                records.extend(std::mem::take(&mut guide.written).iter().map(|(file, bytes, what)| format!("written\t{file}\t{bytes}\t{what}")));
                records.extend(std::mem::take(&mut guide.casts).iter().map(|line| format!("cast\t{line}")));
                Ok(records)
            });
        }
        for &name in guide::MODES.iter().chain(guide_engine::MODES).chain(guide_front::MODES).chain(guide_world_play::MODES).chain(guide_fight::MODES).chain(guide_menus::MODES) {
            if wanted(name) {
                if let Err(e) = guide.run(name, casts) {
                    eprintln!("{name}: {e}");
                    failed.push(name.to_string());
                }
            }
        }
        written = guide.written;
        rows = guide.casts;
    }
    for (file, bytes, what) in &written {
        eprintln!("{}\t{} KiB\t{what}", out.join(file).display(), bytes.div_ceil(1024));
    }
    if !rows.is_empty() {
        let rows = rows.iter().filter_map(|line| Some(Row { id: line.split('\t').next()?.parse().ok()?, line: line.clone() })).collect();
        write_index(&out.join("index.tsv"), rows)?;
    }
    let mut problems = children_failed;
    if !failed.is_empty() {
        problems.insert(0, format!("{} failed", failed.join(", ")));
    }
    if problems.is_empty() { Ok(()) } else { Err(problems.join("\n")) }
}

#[cfg(all(test, feature = "render"))]
mod tests {
    use super::*;

    /// Every mode `render guide` plays is named in its usage, so a new one
    /// (`shop-layers-3d`) or a renamed one (`status-layers-3d`) can be found
    /// from `--help`.
    #[test]
    fn every_guide_mode_is_in_the_usage() {
        let words: Vec<&str> = GUIDE_USAGE.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).collect();
        for &mode in guide::MODES.iter().chain(guide_engine::MODES).chain(guide_front::MODES).chain(guide_world_play::MODES).chain(guide_fight::MODES).chain(guide_menus::MODES) {
            assert!(words.contains(&mode), "{mode} is not in GUIDE_USAGE");
        }
        assert!(guide::MODES.contains(&"shop-layers-3d"));
        assert!(!guide::MODES.contains(&"item-layers-3d"));
    }
}
