//! `render place` and `render eis_story_flag(t, flags)ent`: the photo of a scene's main entrance,
//! a map of each of its rooms, story-state variants, and the message pages
//! of its events, as PNG files at the GBA's own size, with an index.

use super::field::{
    box_holds, result_key, Entrance, EventRun, Field, Image, Saved, Setup, Start, Symbols, EVENT_EXIT, EVENT_LIMIT, EVENT_PSYNERGY, EVENT_SEARCH, EVENT_TALK,
    EVENT_TOUCH, SETTLE,
};
use super::story::{parse_trigger, Alternative, Event, Flags, Scene, Trigger, MOVE};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Pages kept of each event.
const MAX_PAGES: usize = 4;
/// Frames a gauge takes to slide off the screen.
const HUD_FRAMES: u32 = 30;

/// A story flag whose moments have names of their own: scenes whose
/// initialization tests `flag` are shown at the new game's state as
/// `clear` (DIR/<scene>_<clear>/, not DIR/<scene>/), and the moment that sets
/// it is `set`.
struct Variant {
    flag: &'static str,
    clear: &'static str,
    set: &'static str,
}

const VARIANTS: [Variant; 1] = [Variant { flag: "FLAG_KOLIMA_CURSE_LIFTED", clear: "cursed", set: "cured" }];

/// Story moments drawn for one scene at most, besides its new game's state.
pub const MAX_MOMENTS: usize = 12;

/// A story state a scene is drawn in besides the new game's: the flag whose
/// setting brings it about, and every flag set and cleared by then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moment {
    pub flag: String,
    pub set: Vec<String>,
    pub clear: Vec<String>,
}

/// A moment's name, from the flag that brings it about: its name in
/// flags.tsv without `FLAG_`, lowercase, with hyphens
/// (`vale-storm-saw-river-rescue`), or a Variant's own name.
pub fn moment_label(flag: &str) -> String {
    if let Some(v) = VARIANTS.iter().find(|v| v.flag == flag) {
        return v.set.to_string();
    }
    match flag.strip_prefix("FLAG_") {
        Some(name) => name.to_ascii_lowercase().replace('_', "-"),
        None => format!("flag-{}", flag.to_ascii_lowercase()),
    }
}

/// The moments of a scene's story, in story order: `candidates` are the
/// story flags its initialization tests (in the order it tests them), `own`
/// those its initialization sets itself, `number` each flag's number.
///
/// The order comes from the events' own conditions. An event that needs A
/// set to set B puts A before B; one that needs A clear to set B puts B
/// before A, unless some event clears A again (the stormy night's
/// FLAG_VALE_STORM is set and then cleared, so needing it clear says
/// nothing about when). Among flags those leave free, the scene's own
/// arrival comes first; a flag that would end a period waits while another
/// flag tied to that period by some condition can still come (the night
/// goes on before the party leaves Vale); then comes a flag that follows
/// from the last one by needing it set, then the lower number, which the
/// game gives roughly in story order.
///
/// Each moment adds its flag to the ones before it. A flag some event clears
/// is ended, cleared again, by a later flag whose setting needs it clear, or
/// that no event's condition ever sees set together with it while some sees
/// it set with the later flag clear (the storm is over once the party has
/// left).
pub fn story_moments(candidates: &[String], own: &[String], events: &[Event], number: &dyn Fn(&str) -> Option<u16>) -> Vec<Moment> {
    let interval: HashSet<&str> = events.iter().flat_map(|e| e.clears.iter().map(String::as_str)).collect();
    // before[a] holds the flags a comes before; follows[a] those set only
    // once a is set.
    let mut before: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut follows: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in events {
        for b in &e.sets {
            for alternative in &e.alternatives {
                for (a, on) in &alternative.conditions {
                    if a == b {
                        continue;
                    }
                    if *on {
                        before.entry(a).or_default().push(b);
                        follows.entry(a).or_default().push(b);
                    } else if !interval.contains(a.as_str()) {
                        before.entry(b).or_default().push(a);
                    }
                }
            }
        }
    }
    let reach = |graph: &HashMap<&str, Vec<&str>>, from: &str| -> HashSet<String> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut queue = vec![from.to_string()];
        while let Some(f) = queue.pop() {
            for &n in graph.get(f.as_str()).map(Vec::as_slice).unwrap_or(&[]) {
                if seen.insert(n.to_string()) {
                    queue.push(n.to_string());
                }
            }
        }
        seen
    };
    let mut flags: Vec<&String> = Vec::new();
    for c in candidates {
        if !flags.contains(&c) && number(c).is_some() {
            flags.push(c);
        }
    }
    let after: Vec<HashSet<String>> = flags.iter().map(|f| reach(&before, f)).collect();
    let next: Vec<HashSet<String>> = flags.iter().map(|f| reach(&follows, f)).collect();
    // a must come before b: b is after a, and a is not after b.
    let precedes = |a: usize, b: usize| after[a].contains(flags[b].as_str()) && !after[b].contains(flags[a].as_str());
    // The flags tied to a by any condition, either way round: its chapter.
    let mut either: HashMap<&str, Vec<&str>> = HashMap::new();
    for (&a, list) in &before {
        for &b in list {
            either.entry(a).or_default().push(b);
            either.entry(b).or_default().push(a);
        }
    }
    // Whether some event's condition sees a set with b clear, and none sees
    // them both set. An event that sets b says nothing: it needs b clear
    // only because it has not happened yet.
    let exclusive = |a: &str, b: &str| {
        let conditions =
            || events.iter().filter(|e| !e.sets.iter().any(|s| s == b)).flat_map(|e| e.alternatives.iter().map(|alt| &alt.conditions));
        let has = |c: &Vec<(String, bool)>, name: &str, on: bool| c.iter().any(|(n, o)| n == name && *o == on);
        conditions().any(|c| has(c, a, true) && has(c, b, false)) && !conditions().any(|c| has(c, a, true) && has(c, b, true))
    };
    // The flags of `set` that setting b ends.
    let ends = |set: &[String], b: &str| -> Vec<String> {
        let needs_clear = |a: &str| {
            events
                .iter()
                .filter(|e| e.sets.iter().any(|s| s == b))
                .any(|e| e.alternatives.iter().any(|alt| alt.conditions.iter().any(|(n, on)| n == a && !*on)))
        };
        set.iter().filter(|a| interval.contains(a.as_str()) && (needs_clear(a) || exclusive(a, b))).cloned().collect()
    };
    let mut out = Vec::new();
    let (mut set, mut clear): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let mut last: Option<usize> = None;
    let mut left: Vec<usize> = (0..flags.len()).collect();
    while !left.is_empty() {
        let ready: Vec<usize> = left.iter().copied().filter(|&b| !left.iter().any(|&a| a != b && precedes(a, b))).collect();
        let ready = if ready.is_empty() { left.clone() } else { ready };
        // A flag that ends a period waits while another flag of that
        // period's chapter can still come first.
        let ending: Vec<(usize, Vec<String>)> = ready.iter().map(|&i| (i, ends(&set, flags[i]))).collect();
        let chapter: HashSet<String> = ending.iter().flat_map(|(_, e)| e.iter().flat_map(|a| reach(&either, a))).collect();
        let waits = |i: usize| {
            let mine = &ending.iter().find(|(j, _)| *j == i).unwrap().1;
            !mine.is_empty() && ending.iter().any(|(j, e)| *j != i && e.is_empty() && chapter.contains(flags[*j].as_str()))
        };
        let key = |&i: &usize| {
            let follows_last = last.is_some_and(|l| next[l].contains(flags[i].as_str()));
            (!own.contains(flags[i]), waits(i), !follows_last, number(flags[i]), i)
        };
        let pick = *ready.iter().min_by_key(|i| key(i)).unwrap();
        left.retain(|&i| i != pick);
        last = Some(pick);
        let b = flags[pick].as_str();
        let ended = ends(&set, b);
        set.retain(|a| !ended.contains(a));
        for a in ended {
            if !clear.contains(&a) {
                clear.push(a);
            }
        }
        clear.retain(|a| a != b);
        set.push(b.to_string());
        out.push(Moment { flag: b.to_string(), set: set.clone(), clear: clear.clone() });
    }
    out
}

/// Flags a scene's initialization tests that are no story state: the
/// party's kept position and the scene's own temporary flags.
fn is_story_flag(name: &str, flags: &Flags) -> bool {
    if name == "FLAG_KEEP_PARTY_POSITION" || name.starts_with("FLAG_SCENE_TEMP_") {
        return false;
    }
    // 0x200 to 0x3ff are the scene's and the place's own (cleared on entry).
    !matches!(flags.resolve(name, None), Some(0x200..=0x3ff))
}

/// One line of the index.
#[derive(Default)]
pub struct Row {
    pub scene: String,
    pub number: u16,
    pub area: String,
    pub variant: String,
    pub kind: &'static str,
    pub entrance: i16,
    pub event: String,
    pub page: usize,
    pub message: String,
    pub messages: String,
    pub file: String,
    /// A segment's number: the messages before it.
    pub segment: Option<usize>,
    /// A GIF's message before and after it, number and symbol.
    pub after: String,
    pub before: String,
    /// A GIF's frames at 30 a second.
    pub frames: usize,
    /// An event GIF's destination: the scene it leads to, by its SceneId
    /// symbol (`SceneId_WorldMap`, `#N` for an unnamed row).
    pub leads_to: String,
    /// Why an exit's recording ends before the player has control there.
    pub note: String,
}

const INDEX_HEADER: &str =
    "scene\tnumber\tarea\tvariant\tkind\tentrance\tevent\tpage\tsegment\tmessage\tafter\tbefore\tframes\tleads_to\tnote\tmessages\tfile";

impl Row {
    pub fn line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.scene,
            self.number,
            self.area,
            self.variant,
            self.kind,
            self.entrance,
            self.event,
            if self.page > 0 { self.page.to_string() } else { String::new() },
            self.segment.map(|s| s.to_string()).unwrap_or_default(),
            self.message,
            self.after,
            self.before,
            if self.frames > 0 { self.frames.to_string() } else { String::new() },
            self.leads_to,
            self.note,
            self.messages,
            self.file
        )
    }
}

/// A line of an index written with `header` in INDEX_HEADER's columns: one
/// an earlier version wrote gets its columns by name, empty where it had
/// none.
fn same_columns(header: &str, line: &str) -> String {
    if header == INDEX_HEADER {
        return line.to_string();
    }
    let old: HashMap<&str, &str> = header.split('\t').zip(line.split('\t')).collect();
    INDEX_HEADER.split('\t').map(|k| old.get(k).copied().unwrap_or_default()).collect::<Vec<_>>().join("\t")
}

/// Writes `rows` into index.tsv, keeping the lines already there for other
/// files that are still there, sorted by file.
pub fn write_index(out: &Path, rows: &[(String, String)]) -> Result<(), String> {
    let path = out.join("index.tsv");
    let mut lines: BTreeMap<String, String> = BTreeMap::new();
    if let Ok(text) = std::fs::read_to_string(&path) {
        let mut text = text.lines();
        let header = text.next().unwrap_or_default();
        for line in text {
            if let Some(file) = line.rsplit('\t').next() {
                if out.join(file).is_file() {
                    lines.insert(file.to_string(), same_columns(header, line));
                }
            }
        }
    }
    for (file, line) in rows {
        lines.insert(file.clone(), line.clone());
    }
    let mut text = format!("{INDEX_HEADER}\n");
    for line in lines.values() {
        text.push_str(line);
        text.push('\n');
    }
    std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Writes an image as a PNG: indexed when it has at most 256 colours.
pub fn write_png(path: &Path, image: &Image) -> Result<u64, String> {
    let error = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| error(&e))?;
    }
    let file = std::fs::File::create(path).map_err(|e| error(&e))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), image.width as u32, image.height as u32);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Best);
    let mut colours: Vec<u32> = image.pixels.clone();
    colours.sort_unstable();
    colours.dedup();
    let data: Vec<u8> = if colours.len() <= 256 {
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_palette(colours.iter().flat_map(|&c| [(c >> 16) as u8, (c >> 8) as u8, c as u8]).collect::<Vec<u8>>());
        let index: HashMap<u32, u8> = colours.iter().enumerate().map(|(i, &c)| (c, i as u8)).collect();
        image.pixels.iter().map(|c| index[c]).collect()
    } else {
        encoder.set_color(png::ColorType::Rgb);
        image.pixels.iter().flat_map(|&c| [(c >> 16) as u8, (c >> 8) as u8, c as u8]).collect()
    };
    let mut writer = encoder.write_header().map_err(|e| error(&e))?;
    writer.write_image_data(&data).map_err(|e| error(&e))?;
    writer.finish().map_err(|e| error(&e))?;
    Ok(std::fs::metadata(path).map_err(|e| error(&e))?.len())
}

/// Why something was left out.
pub struct Skip {
    pub scene: String,
    pub what: String,
    pub reason: String,
}

pub struct Context<'a> {
    pub rom: &'a [u8],
    /// Every scene of the table, to name the scene an exit leads to.
    pub scenes: &'a [Scene],
    pub symbols: &'a Symbols,
    pub flags: &'a Flags,
    pub events: &'a [Event],
    pub out: PathBuf,
    pub rows: Vec<Row>,
    pub skipped: Vec<Skip>,
    pub bytes: u64,
    pub images: usize,
    pub gifs: usize,
    pub gif_bytes: u64,
    /// The events to play, by name; none for every event of the scenes.
    pub wanted: &'a [String],
    /// The scenes asked for: an event is played when it lists one of them,
    /// at any scene of the job it lists. None for every scene of the job.
    pub only: &'a [u16],
    /// The scenes and things of skipped.tsv that played this time, whose
    /// old lines go. `event *` is every event of the scene, all played again.
    pub cleared: Vec<(String, String)>,
    /// Each scene's functions by name, from its overlay's ELF: an event
    /// named by its function is told by its row's result.
    pub functions: &'a HashMap<u16, HashMap<String, u32>>,
    /// Story moments drawn (besides the new game's state), the scenes that
    /// have any, and the scenes that had more than MAX_MOMENTS.
    pub moments: usize,
    pub moment_scenes: usize,
    pub capped: usize,
    /// Whether the events played now may start the leader where he cannot
    /// walk (Field::relaxed): an event no walkable spot plays is tried once
    /// more that way.
    pub relaxed: std::cell::Cell<bool>,
}

/// Whether a scene is a place: not the title, the clear-data screen or the
/// debug test rooms.
pub fn is_place(scene: &Scene) -> Result<(), String> {
    if scene.file.contains("/debug/") {
        return Err("a debug test room".to_string());
    }
    if scene.file.ends_with("menu/title.c") || scene.file.ends_with("menu/clear.c") {
        return Err("a menu, not a place".to_string());
    }
    Ok(())
}

impl Context<'_> {
    fn skip(&mut self, scene: &Scene, what: &str, reason: String) {
        eprintln!("  left out: {} {what}: {reason}", scene.name());
        self.skipped.push(Skip { scene: scene.name(), what: what.to_string(), reason });
    }

    fn write(&mut self, image: &Image, file: &str) -> Result<(), String> {
        self.bytes += write_png(&self.out.join(file), image)?;
        self.images += 1;
        Ok(())
    }

    /// The scene's entrance table, from a run that enters it by entrance 0
    /// (the table's first entry when no entry has that number).
    fn entrances(&self, scene: &Scene, set: &[u16], clear: &[u16]) -> Result<(Vec<Entrance>, [i32; 4]), String> {
        let setup = Setup { scene: scene.number, entrance: 0, set: set.to_vec(), clear: clear.to_vec() };
        let field = Field::enter(self.rom, self.symbols, &setup)?;
        let entrances = field.entrances();
        if entrances.is_empty() {
            return Err("its entrance table is empty".to_string());
        }
        Ok((entrances, field.header()))
    }

    /// The states a scene is drawn in: the new game's first, then each
    /// moment of its story (story_moments), as (label, the index's variant
    /// column, flags set, flags cleared). The new game's label is empty, or
    /// a Variant's `clear` name when the scene tests its flag.
    fn states(&self, scene: &Scene) -> Vec<(String, String, Vec<u16>, Vec<u16>)> {
        let name = scene.symbol.as_deref();
        let entry: Vec<&Event> =
            self.events.iter().filter(|e| e.scenes.contains(&scene.number) && e.trigger_text.starts_with("entry")).collect();
        let mut candidates: Vec<String> = Vec::new();
        let mut own: Vec<String> = Vec::new();
        for e in &entry {
            // A flag flags.tsv leaves unnamed (0xfd0) names no moment.
            for t in e.tests.iter().filter(|t| is_story_flag(t, self.flags) && !t.starts_with("0x")) {
                if !candidates.contains(t) {
                    candidates.push(t.clone());
                }
            }
            own.extend(e.sets.iter().cloned());
        }
        let resolve = |n: &str| self.flags.resolve(n, name);
        let numbers = |names: &[String]| -> Vec<u16> { names.iter().filter_map(|n| resolve(n)).collect() };
        let mut out = Vec::new();
        match VARIANTS.iter().find(|v| candidates.iter().any(|c| c == v.flag)) {
            Some(v) => out.push((v.clear.to_string(), v.clear.to_string(), Vec::new(), resolve(v.flag).into_iter().collect())),
            None => out.push((String::new(), String::new(), Vec::new(), Vec::new())),
        }
        let (storm, child) = (resolve("FLAG_VALE_STORM"), resolve("FLAG_CHILDHOOD"));
        for m in story_moments(&candidates, &own, self.events, &resolve) {
            let label = moment_label(&m.flag);
            let mut column = format!("{label}: {}", m.set.join(", "));
            if !m.clear.is_empty() {
                column.push_str(&format!("; clear {}", m.clear.join(", ")));
            }
            let (mut set, mut clear) = (numbers(&m.set), numbers(&m.clear));
            // The stormy night is the prologue, when Isaac and Garet are
            // children (as alternative_flags has it); once it is over they
            // are grown.
            if let (Some(storm), Some(child)) = (storm, child) {
                if set.contains(&storm) || name == Some("ValeStorm") {
                    set.push(child);
                } else if clear.contains(&storm) {
                    clear.push(child);
                }
            }
            out.push((label, column, set, clear));
        }
        out
    }

    /// The entrance photo and a map of each room, at the new game's state
    /// and at each moment of the scene's story that changes what a room
    /// shows (Field::signature) from every state drawn before it; only the
    /// rooms it changes are drawn. At most MAX_MOMENTS moments are drawn.
    pub fn place(&mut self, scene: &Scene) {
        if let Err(reason) = is_place(scene) {
            self.skip(scene, "place", reason);
            return;
        }
        let states = self.states(scene);
        let mut seen: HashMap<i16, Vec<Vec<i32>>> = HashMap::new();
        let mut drawn = 0;
        for (i, (label, column, set, clear)) in states.iter().enumerate() {
            if i > 0 && drawn == MAX_MOMENTS {
                let rest: Vec<&str> = states[i..].iter().map(|s| s.0.as_str()).collect();
                self.skip(scene, "place moments", format!("hit the cap of {MAX_MOMENTS} moments; not tried: {}", rest.join(", ")));
                self.capped += 1;
                break;
            }
            match self.place_variant(scene, label, column, set, clear, i == 0, &mut seen) {
                Ok(true) if i > 0 => drawn += 1,
                Ok(_) => {}
                Err(e) => self.skip(scene, &format!("place {label}"), e),
            }
        }
        if drawn > 0 {
            self.moments += drawn;
            self.moment_scenes += 1;
        }
    }

    /// Draws one state of a scene; the new game's (`all`) in every room, a
    /// moment only in the rooms whose signature no state before it had.
    /// Whether it drew anything.
    #[allow(clippy::too_many_arguments)]
    fn place_variant(
        &mut self,
        scene: &Scene,
        variant: &str,
        column: &str,
        set: &[u16],
        clear: &[u16],
        all: bool,
        seen: &mut HashMap<i16, Vec<Vec<i32>>>,
    ) -> Result<bool, String> {
        let (entrances, _) = self.entrances(scene, set, clear)?;
        let folder = if variant.is_empty() { scene.slug() } else { format!("{}_{variant}", scene.slug()) };
        // A room is a camera box; the first entrance that has it names it.
        let mut rooms: Vec<Entrance> = Vec::new();
        for e in &entrances {
            if !rooms.iter().any(|r| r.camera == e.camera) {
                rooms.push(*e);
            }
        }
        let mut named: HashMap<i16, usize> = HashMap::new();
        let mut wrote = false;
        for (i, room) in rooms.iter().enumerate() {
            let mut set = set.to_vec();
            if room.required_flag >= 0 {
                set.push(room.required_flag as u16);
            }
            let setup = Setup { scene: scene.number, entrance: room.number as u16, set, clear: clear.to_vec() };
            let result = (|| -> Result<(), String> {
                let mut field = Field::enter(self.rom, self.symbols, &setup)?;
                field.until_control()?;
                field.frames(SETTLE)?;
                let signature = field.signature();
                let earlier = seen.entry(room.number).or_default();
                let new = !earlier.contains(&signature);
                if new {
                    earlier.push(signature);
                }
                if !all && !new {
                    return Ok(());
                }
                let row = |kind, file: String| Row {
                    scene: scene.slug(),
                    number: scene.number,
                    area: scene.area.clone(),
                    variant: column.to_string(),
                    kind,
                    entrance: room.number,
                    event: String::new(),
                    page: 0,
                    message: String::new(),
                    messages: String::new(),
                    file,
                    ..Row::default()
                };
                if i == 0 {
                    let file = format!("{folder}/entrance.png");
                    self.write(&field.screen(), &file)?;
                    self.rows.push(row("entrance", file));
                    wrote = true;
                }
                if scene.map == "world" {
                    return Ok(());
                }
                // Gauges that hide while an event runs (the heat gauge) slide away
                // under FLAG_CONTACT_BLOCKED before the map's moment.
                if let Some(flag) = self.flags.resolve("FLAG_CONTACT_BLOCKED", None) {
                    field.set_flag(flag, true);
                    field.frames(HUD_FRAMES)?;
                }
                let settled = field.save();
                let bounds = field.bounds();
                let image = field.stitch(&settled, bounds, false)?;
                let count = named.entry(room.number).or_default();
                *count += 1;
                let suffix = if *count > 1 { format!("-{count}") } else { String::new() };
                let file = format!("{folder}/map-{}{suffix}.png", room.number);
                self.write(&image, &file)?;
                self.rows.push(row("map", file));
                wrote = true;
                Ok(())
            })();
            if let Err(e) = result {
                self.skip(scene, &format!("{folder} room of entrance {}", room.number), e);
            }
        }
        if all && scene.map == "world" {
            self.skip(scene, "maps", "the perspective world map has no layered map to stitch".to_string());
        }
        Ok(wrote)
    }
}

/// The places an event is tried at: scenes and entrances whose camera box
/// holds the trigger, in the event's scene order, and at most so many in one
/// scene.
const MAX_PLACES: usize = 4;
const PLACES_PER_SCENE: usize = 3;
/// Entrances of one scene scanned for a trigger at most.
const MAX_ENTRANCES: usize = 24;
/// Spots tried around one trigger at one place.
const MAX_SPOTS: usize = 8;
/// Triggers of an event retried after each of its scene's other events.
const MAX_RETRIED: usize = 2;

/// What a scene shows with given flags, entered by an entrance: its
/// entrances, the header's bounds, where its triggers are, and why each
/// actor it has but does not show is off stage.
struct Scan {
    entrances: Vec<Entrance>,
    header: [i32; 4],
    actors: HashMap<u16, (i32, i32)>,
    absent: HashMap<u16, String>,
    cells: HashMap<u16, Vec<(u8, i32, i32)>>,
    /// The side of its cells in pixels: 16, or the world map's 32.
    size: i32,
    /// The rows of its event table, as (kind, target, result).
    triggers: Vec<(u8, u16, u32)>,
}

type ScanKey = (u16, u16, Vec<u16>, Vec<u16>);

/// Why an event could not be played, and whether it was because its actor
/// was never on stage, which another event of the scene may change.
struct Failure {
    reason: String,
    absent: bool,
}

impl Failure {
    fn new(reason: String) -> Failure {
        Failure { reason, absent: false }
    }
}

/// An event played: where, and what it left.
struct Played {
    setup: Setup,
    run: EventRun,
}

/// What one job keeps between its events: the scans, and the moment each
/// event played without recording left the player in control, for the
/// events that only another event makes playable.
#[derive(Default)]
pub struct Job {
    scans: HashMap<ScanKey, Result<Scan, String>>,
    ends: HashMap<(String, Vec<u16>, Vec<u16>), Option<(Setup, Saved)>>,
}

/// Whether an event is a row of a scene's event table that only shows a
/// message, named by it (MsgValeRockCannotBeMoved): it changes nothing
/// another event could need.
fn is_message_row(name: &str) -> bool {
    name.starts_with("Msg") || name.contains('[') || name.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// The events `render event` plays for one overlay's scenes, in its order:
/// the rows that list one of them (and one of `only`'s, when it names
/// any), of the names `wanted` gives, merged by name.
pub fn job_events(events: &[Event], wanted: &[String], only: &[u16], scenes: &[&Scene]) -> Vec<Event> {
    let numbers: Vec<u16> = scenes.iter().map(|s| s.number).collect();
    let rows: Vec<&Event> = events
        .iter()
        .filter(|e| wanted.is_empty() || wanted.iter().any(|w| *w == e.name))
        .filter(|e| e.scenes.iter().any(|n| numbers.contains(n)))
        .filter(|e| only.is_empty() || e.scenes.iter().any(|n| only.contains(n)))
        .collect();
    merge_rows(&rows)
}

/// Every event of a scene played afresh: what an earlier run wrote for it
/// goes first, so an event renamed since, or one that no longer plays,
/// keeps no picture. Returns skipped.tsv's `event *` lines to clear.
pub fn clear_events(out: &Path, scenes: &[&Scene], wanted: &[String], only: &[u16]) -> Vec<(String, String)> {
    let mut cleared = Vec::new();
    if wanted.is_empty() {
        for scene in scenes.iter().filter(|s| only.is_empty() || only.contains(&s.number)) {
            let folder = out.join(scene.slug()).join("events");
            if folder.is_dir() {
                let _ = std::fs::remove_dir_all(&folder);
            }
            cleared.push((scene.name(), "event *".to_string()));
        }
    }
    cleared
}

/// The rows of `events` that share a name, as one event: the eight rocks of
/// the stormy night are one event with eight triggers, and a helper two
/// scenes include is one event of both.
fn merge_rows(events: &[&Event]) -> Vec<Event> {
    let mut out: Vec<Event> = Vec::new();
    for event in events {
        let Some(group) = out.iter_mut().find(|g| g.name == event.name) else {
            out.push((*event).clone());
            continue;
        };
        for s in &event.scenes {
            if !group.scenes.contains(s) {
                group.scenes.push(*s);
            }
        }
        group.alternatives.extend(event.alternatives.iter().cloned());
        for (list, more) in [(&mut group.tests, &event.tests), (&mut group.sets, &event.sets)] {
            for t in more {
                if !list.contains(t) {
                    list.push(t.clone());
                }
            }
        }
        for m in &event.messages {
            if !group.messages.contains(m) {
                group.messages.push(*m);
            }
        }
    }
    out
}

impl Context<'_> {
    fn scan(&self, scene: u16, entrance: u16, set: &[u16], clear: &[u16]) -> Result<Scan, String> {
        let setup = Setup { scene, entrance, set: set.to_vec(), clear: clear.to_vec() };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        let entrances = field.entrances();
        let header = field.header();
        field.until_control()?;
        field.frames(SETTLE)?;
        let mut actors = HashMap::new();
        let mut absent = HashMap::new();
        for id in 0..66u16 {
            match field.present(id) {
                Ok(a) => {
                    actors.insert(id, (a.x, a.z));
                }
                Err(e) => {
                    absent.insert(id, e);
                }
            }
        }
        let cells = field.cells_by_code()?;
        let triggers = field.triggers();
        if std::env::var_os("ALCHEMY_RENDER_DEBUG").is_some() {
            let mut codes: Vec<_> = cells.iter().map(|(c, v)| (*c, v.len(), v[0])).collect();
            codes.sort();
            eprintln!("    scan {scene} entrance {entrance} set {set:?}: world {} cells {codes:?} triggers {:?} entrances {:?}", field.world(), triggers.iter().filter(|t| t.0 == 3).collect::<Vec<_>>(), entrances.iter().map(|e| (e.number, e.camera)).collect::<Vec<_>>());
        }
        let size = field.cell_size();
        Ok(Scan { entrances, header, actors, absent, cells, size, triggers })
    }

    /// The scan of a scene entered by an entrance with given flags, made
    /// once.
    fn scanned<'s>(
        &self,
        scans: &'s mut HashMap<ScanKey, Result<Scan, String>>,
        scene: u16,
        entrance: u16,
        set: &[u16],
        clear: &[u16],
    ) -> &'s Result<Scan, String> {
        scans.entry((scene, entrance, set.to_vec(), clear.to_vec())).or_insert_with(|| self.scan(scene, entrance, set, clear))
    }

    /// The scene and entrance pairs where `trigger` stands, scanning the
    /// scene by its first entrance and then by the first entrance of each of
    /// its other rooms. Each place is the room holding the trigger (an actor,
    /// or one of the cells with its code), entered by the entrance nearest
    /// it, from which the leader can most likely walk to it.
    fn places(
        &self,
        scenes: &[&Scene],
        trigger: Trigger,
        event: &Event,
        set: &[u16],
        clear: &[u16],
        scans: &mut HashMap<ScanKey, Result<Scan, String>>,
    ) -> Result<Vec<(u16, Entrance)>, Failure> {
        let mut out = Vec::new();
        let mut error = None;
        let mut absent = None;
        // Where the trigger stands in a scan: the actor, or the cells with
        // its code (the ground layer's for a search).
        let at = |scan: &Scan| -> Vec<(i32, i32)> {
            let centre = |&(_, cx, cz): &(u8, i32, i32)| (cx * scan.size + scan.size / 2, cz * scan.size + scan.size / 2);
            match trigger {
                Trigger::Talk(a) | Trigger::Psynergy(_, a) => scan.actors.get(&a).copied().into_iter().collect(),
                Trigger::Search(c) => scan.cells.get(&c).map_or_else(Vec::new, |v| v.iter().filter(|c| c.0 == 0).map(centre).collect()),
                // A world map place is an actor the leader walks into.
                Trigger::Touch(c) if scan.size == 32 && c >= 100 && !scan.cells.contains_key(&c) => scan.actors.get(&(c - 100)).copied().into_iter().collect(),
                Trigger::Touch(c) => scan.cells.get(&c).map_or_else(Vec::new, |v| v.iter().map(centre).collect()),
            }
        };
        // Whether the event table a scan's entrance gives answers the
        // trigger with the event's own row, when its row is known.
        let answers = |scan: &Scan, own: &[u32]| {
            let row = |kind: u8, target: u16| scan.triggers.iter().any(|&(k, t, r)| k == kind && t == target && (own.is_empty() || own.contains(&r)));
            match trigger {
                Trigger::Talk(a) => row(EVENT_TALK, a),
                Trigger::Search(c) => row(EVENT_SEARCH, c),
                Trigger::Touch(c) => row(EVENT_TOUCH, c) || (own.is_empty() && row(EVENT_EXIT, c)),
                Trigger::Psynergy(..) if own.is_empty() => scan.triggers.iter().any(|t| t.0 == EVENT_PSYNERGY),
                Trigger::Psynergy(_, a) => row(EVENT_PSYNERGY, a),
            }
        };
        for scene in scenes.iter().filter(|s| is_place(s).is_ok()) {
            let own = self.own(event, scene.number);
            let first = match self.scanned(scans, scene.number, 0, set, clear) {
                Ok(scan) => scan,
                Err(e) => {
                    error.get_or_insert_with(|| format!("{}: {e}", scene.name()));
                    continue;
                }
            };
            let header = first.header;
            let entrances: Vec<Entrance> = first.entrances.iter().filter(|e| e.number >= 0).copied().collect();
            let first_at = at(first);
            // The entrances to try, best first: those whose room holds the
            // trigger as the scene is entered by its first, nearest it
            // first (a room's entrances may give it different events, as
            // the loft's do in Vault); then the first of each other room.
            let mut near: Vec<(i32, Entrance)> = Vec::new();
            for (x, z) in first_at {
                for e in entrances.iter().filter(|e| box_holds(e.camera, header, x, z)) {
                    near.push(((i32::from(e.x) - x).pow(2) + (i32::from(e.z) - z).pow(2), *e));
                }
            }
            near.sort_by_key(|(d, _)| *d);
            let mut order: Vec<Entrance> = near.into_iter().map(|(_, e)| e).collect();
            for e in &entrances {
                if !order.iter().any(|o| o.camera == e.camera) {
                    order.push(*e);
                }
            }
            let mut tried: Vec<i16> = Vec::new();
            let mut found = 0;
            for e in order {
                if tried.contains(&e.number) || found >= PLACES_PER_SCENE || tried.len() >= MAX_ENTRANCES {
                    continue;
                }
                tried.push(e.number);
                let scan = match self.scanned(scans, scene.number, e.number as u16, set, clear) {
                    Ok(scan) => scan,
                    Err(why) => {
                        error.get_or_insert_with(|| format!("{}: {why}", scene.name()));
                        continue;
                    }
                };
                if let Trigger::Talk(a) | Trigger::Psynergy(_, a) = trigger {
                    if let Some(why) = scan.absent.get(&a) {
                        absent.get_or_insert_with(|| why.clone());
                    }
                }
                // Entered here, the scene must show the trigger in this
                // entrance's room and answer it.
                let here = at(scan);
                if answers(scan, &own) && here.iter().any(|&(x, z)| box_holds(e.camera, scan.header, x, z)) {
                    out.push((scene.number, e));
                    found += 1;
                }
            }
            if out.len() >= MAX_PLACES {
                out.truncate(MAX_PLACES);
                break;
            }
        }
        if out.is_empty() {
            if let Some(why) = absent {
                return Err(Failure { reason: why, absent: true });
            }
            let names = scenes.iter().map(|s| s.name()).collect::<Vec<_>>().join(", ");
            let missing = match trigger {
                Trigger::Search(c) => format!("no entrance of {names} leads to a room with a ground-layer cell with code {c} that its events search"),
                Trigger::Touch(c) => format!("no entrance of {names} leads to a room with a cell with code {c} that its events answer"),
                Trigger::Talk(a) => format!("no entrance of {names} leads to a room with actor {a} that its events talk to"),
                Trigger::Psynergy(_, a) => format!("no entrance of {names} leads to a room with actor {a} and a Psynergy event"),
            };
            return Err(Failure::new(error.unwrap_or(missing)));
        }
        Ok(out)
    }

    /// Plays every chosen event of `scenes` and writes its pages and one GIF
    /// of the whole event. Rows of the scenes with the same name are one
    /// event. Each event is tried by each of its triggers in turn, with its
    /// flags and then with each story flag its scene's setup tests or its
    /// scene's events set added one at a time, at each place that holds the
    /// trigger, from each spot around it, until it plays; an event that does
    /// not play when the scene is entered (its actor is not on stage yet, or
    /// what it needs another event leaves) is tried once each other event
    /// of the scene has played.
    pub fn events(&mut self, scenes: &[&Scene]) {
        self.cleared.extend(clear_events(&self.out, scenes, self.wanted, self.only));
        self.play_events(scenes, None, &mut Job::default());
    }

    /// Plays the events `events` would for these scenes (one overlay's),
    /// or only the one named, keeping what it finds in `job` for the next
    /// events of the same scenes. The old pictures are not cleared first.
    pub fn play_events(&mut self, scenes: &[&Scene], name: Option<&str>, job: &mut Job) {
        for event in job_events(self.events, self.wanted, self.only, scenes).into_iter().filter(|e| name.is_none_or(|n| e.name == n)) {
            let candidates: Vec<&Scene> = scenes.iter().copied().filter(|s| event.scenes.contains(&s.number)).collect();
            let Some(&first) = candidates.first() else {
                continue;
            };
            let label = format!("event {}", event.name);
            // An event played again keeps none of its old pages.
            for scene in &candidates {
                self.remove_event_files(scene, &event.name);
            }
            let mut played = self.play_event(&event, &candidates, job, Some(EVENT_LIMIT as usize), None);
            if played.as_ref().is_err_and(|f| f.reason.contains("cannot walk")) {
                self.relaxed.set(true);
                let relaxed = self.play_event(&event, &candidates, job, Some(EVENT_LIMIT as usize), None);
                self.relaxed.set(false);
                if relaxed.is_ok() {
                    played = relaxed;
                }
            }
            match played {
                Ok(played) => {
                    let scene = candidates.iter().find(|s| s.number == played.setup.scene).copied().unwrap_or(first);
                    for s in &candidates {
                        self.cleared.push((s.name(), label.clone()));
                    }
                    // A GIF shows the whole event or is not written.
                    if played.run.capped {
                        let limit = EVENT_LIMIT / 60;
                        self.skip(scene, &label, format!("it runs past {limit} seconds without the player having control again"));
                    } else if let Err(e) = self.write_event(scene, played.setup.entrance as i16, &event, &played.run) {
                        self.skip(scene, &label, e);
                    }
                }
                Err(e) => self.skip(first, &label, e.reason),
            }
        }
    }

    /// The flags an alternative sets and clears, with the stormy night's
    /// childhood.
    fn alternative_flags(&self, alternative: &Alternative, first: &Scene) -> Result<(Vec<u16>, Vec<u16>), String> {
        let (mut set, mut clear) = (Vec::new(), Vec::new());
        for (name, on) in &alternative.conditions {
            let flag = self.flags.resolve(name, first.symbol.as_deref()).ok_or_else(|| format!("unknown flag {name}"))?;
            set.retain(|&f| f != flag);
            clear.retain(|&f| f != flag);
            if *on { &mut set } else { &mut clear }.push(flag);
        }
        // The stormy night is the prologue: Isaac and Garet are children
        // then, and the party's sprites follow FLAG_CHILDHOOD
        // (Party_RemapCharacterIdByFlags).
        if let (Some(storm), Some(child)) = (self.flags.resolve("FLAG_VALE_STORM", None), self.flags.resolve("FLAG_CHILDHOOD", None)) {
            if (set.contains(&storm) || first.symbol.as_deref() == Some("ValeStorm")) && !set.contains(&child) {
                clear.retain(|&f| f != child);
                set.push(child);
            }
        }
        set.sort_unstable();
        clear.sort_unstable();
        Ok((set, clear))
    }

    /// The story flags tried one at a time when an event's own do not put
    /// its trigger on stage: those its scene's setup tests, then those its
    /// scene's events set.
    fn extra_flags(&self, first: &Scene, set: &[u16], clear: &[u16]) -> Vec<u16> {
        let mut names: Vec<String> = variant_candidates(self.events, self.flags).remove(&first.number).unwrap_or_default();
        // A named variant's flag is tried only where an event sets it.
        names.retain(|n| !VARIANTS.iter().any(|v| v.flag == n));
        for e in self.events.iter().filter(|e| e.scenes.contains(&first.number)) {
            for s in e.sets.iter().filter(|s| is_story_flag(s, self.flags)) {
                if !names.contains(s) {
                    names.push(s.clone());
                }
            }
        }
        let mut out = Vec::new();
        for f in names.iter().filter_map(|n| self.flags.resolve(n, first.symbol.as_deref())) {
            if !set.contains(&f) && !clear.contains(&f) && !out.contains(&f) {
                out.push(f);
            }
        }
        out
    }

    /// `set` with every story flag the scene's other events set that the
    /// event neither sets itself nor needs clear, when that adds more than
    /// one flag (one alone extra_flags tries).
    fn story_so_far(&self, first: &Scene, event: &Event, set: &[u16], clear: &[u16]) -> Option<Vec<u16>> {
        let own: Vec<u16> = event.sets.iter().filter_map(|n| self.flags.resolve(n, first.symbol.as_deref())).collect();
        let mut out = set.to_vec();
        for e in self.events.iter().filter(|e| e.scenes.contains(&first.number) && e.name != event.name) {
            for f in e.sets.iter().filter(|s| is_story_flag(s, self.flags)).filter_map(|n| self.flags.resolve(n, first.symbol.as_deref())) {
                if !out.contains(&f) && !clear.contains(&f) && !own.contains(&f) {
                    out.push(f);
                }
            }
        }
        out.sort_unstable();
        (out.len() > set.len() + 1).then_some(out)
    }

    fn play_event(
        &self,
        event: &Event,
        scenes: &[&Scene],
        job: &mut Job,
        record: Option<usize>,
        within: Option<(&[u16], &[u16])>,
    ) -> Result<Played, Failure> {
        let first = scenes[0];
        // On the way to another event, any run that leaves the player in
        // control will do; an event shown must show itself.
        let strict = within.is_none();
        let mut failure: Option<Failure> = None;
        // The triggers to try again after the scene's other events, with
        // their flags: those that found a place first.
        let mut retry: Vec<(Trigger, Vec<u16>, Vec<u16>)> = Vec::new();
        let mut unplaced: Vec<(Trigger, Vec<u16>, Vec<u16>)> = Vec::new();
        for alternative in &event.alternatives {
            let trigger = match &alternative.trigger {
                Ok(t) => *t,
                Err(e) => {
                    failure.get_or_insert_with(|| Failure::new(e.clone()));
                    continue;
                }
            };
            let (mut set, mut clear) = match self.alternative_flags(alternative, first) {
                Ok(flags) => flags,
                Err(e) => {
                    failure.get_or_insert_with(|| Failure::new(e));
                    continue;
                }
            };
            // Played on the way to another event, by that event's flags.
            if let Some((with_set, with_clear)) = within {
                set.retain(|f| !with_clear.contains(f));
                clear.retain(|f| !with_set.contains(f));
                set.extend(with_set.iter().filter(|f| !set.contains(f)).collect::<Vec<_>>());
                clear.extend(with_clear.iter().filter(|f| !clear.contains(f)).collect::<Vec<_>>());
                set.sort_unstable();
                clear.sort_unstable();
            }
            // On the way to another event, only by its own flags.
            let extras = if within.is_some() { Vec::new() } else { self.extra_flags(first, &set, &clear) };
            // Last, every story flag the scene's other events set, all at
            // once: a room some events open in turn (the crossings each
            // Elemental Star bagged opens to the next pedestal).
            let together = if within.is_some() { None } else { self.story_so_far(first, event, &set, &clear) };
            let flag_sets = std::iter::once(set.clone())
                .chain(extras.iter().map(|&f| {
                    let mut s = set.clone();
                    s.push(f);
                    s.sort_unstable();
                    s
                }))
                .chain(together);
            let mut placed = false;
            for flags in flag_sets {
                let places = match self.places(scenes, trigger, event, &flags, &clear, &mut job.scans) {
                    Ok(places) => places,
                    Err(f) => {
                        // A story flag that puts nothing on stage says less
                        // than the event's own flags did.
                        if failure.is_none() || flags == set {
                            failure = Some(f);
                        }
                        continue;
                    }
                };
                placed = true;
                for (scene, entrance) in places {
                    let mut set = flags.clone();
                    if entrance.required_flag >= 0 && !set.contains(&(entrance.required_flag as u16)) {
                        set.push(entrance.required_flag as u16);
                        set.sort_unstable();
                    }
                    let setup = Setup { scene, entrance: entrance.number as u16, set, clear: clear.clone() };
                    match self.play_at(&setup, None, trigger, event, record, strict) {
                        Ok(run) => return Ok(Played { setup, run }),
                        Err(e) => failure = Some(Failure::new(e)),
                    }
                }
            }
            let entry = (trigger, set.clone(), clear.clone());
            if placed { &mut retry } else { &mut unplaced }.push(entry);
        }
        let mut failure = failure.unwrap_or_else(|| Failure::new("the row has no trigger".to_string()));
        // The event does not play as the scene is entered: its actor only
        // comes on stage during another event, or what it needs (a log
        // pushed into place, a flag) another event leaves. Play each of the
        // scene's other events and try from where it leaves the player.
        retry.extend(unplaced);
        retry.dedup_by_key(|r| r.0);
        retry.truncate(MAX_RETRIED);
        if within.is_none() && !retry.is_empty() {
            let mut seen: Vec<&str> = Vec::new();
            let others = self.events.iter().filter(|e| {
                let fresh = !seen.contains(&e.name.as_str());
                seen.push(e.name.as_str());
                fresh
                    && e.scenes.contains(&first.number)
                    && e.name != event.name
                    && !is_message_row(&e.name)
                    && e.trigger().is_ok()
                    // What another event can change for this one: who is on
                    // stage, a flag, or what a touch, a cast or a search moves
                    // (a floodgate's switch lowering the river).
                    && (failure.absent || !e.sets.is_empty() || matches!(e.trigger(), Ok(Trigger::Touch(_) | Trigger::Psynergy(..) | Trigger::Search(_))))
            });
            let others: Vec<&Event> = others.collect();
            let mut tried_after = 0;
            for other in others {
                for (trigger, set, clear) in &retry {
                    let key = (other.name.clone(), set.clone(), clear.clone());
                    if !job.ends.contains_key(&key) {
                        let end = self.play_event(other, scenes, job, None, Some((set, clear))).ok().and_then(|p| p.run.end.map(|end| (p.setup, end)));
                        job.ends.insert(key.clone(), end);
                    }
                    let Some((setup, end)) = &job.ends[&key] else {
                        continue;
                    };
                    tried_after += 1;
                    match self.play_at(setup, Some(end), *trigger, event, record, strict) {
                        Ok(run) => return Ok(Played { setup: setup.clone(), run }),
                        Err(e) if failure.absent && (e.contains("off stage") || e.contains("no actor") || e.contains("no sprite")) => {}
                        Err(e) if failure.absent => failure = Failure::new(format!("after {}: {e}", other.name)),
                        Err(_) => {}
                    }
                }
            }
            failure.reason = if failure.absent {
                format!("{}, when the scene is entered and after each of its other events", failure.reason)
            } else if tried_after > 0 {
                format!("{}; nor after any of the scene's other events", failure.reason)
            } else {
                failure.reason
            };
        }
        Err(failure)
    }

    /// Plays an event at one place, from each spot around its trigger until
    /// one counts: it began there, ran a script, and showed itself (one of
    /// its messages when it has messages; a touch or a Move with none moved
    /// something, left, fought or changed the map). `after` is a moment of
    /// the scene to start from instead of its entrance. Unless `strict`, any
    /// run that began counts.
    fn play_at(&self, setup: &Setup, after: Option<&Saved>, trigger: Trigger, event: &Event, record: Option<usize>, strict: bool) -> Result<EventRun, String> {
        let messages = &event.messages;
        let own = self.own(event, setup.scene);
        let own_function = own.iter().copied().find(|&r| r >= 0x10000);
        let message_row = !own.is_empty() && own_function.is_none();
        let mut field = Field::enter(self.rom, self.symbols, setup)?;
        field.relaxed = self.relaxed.get();
        let entered: Saved = field.save();
        match after {
            Some(saved) => field.load(saved),
            None => {
                field.until_control()?;
                field.frames(SETTLE)?;
            }
        }
        let spots = field.spots(trigger)?;
        let mut error = String::new();
        let silent = matches!(trigger, Trigger::Touch(_) | Trigger::Psynergy(MOVE, _));
        for spot in spots.iter().take(MAX_SPOTS) {
            let start = match after {
                Some(saved) => Start::InControl(saved),
                None => Start::Entered(&entered),
            };
            match field.event(start, trigger, spot, &own, messages, MAX_PAGES, record) {
                Ok(run) if !strict => return Ok(run),
                // Another row for the trigger ran its function: the actor
                // with this id in another room, or a later part of the story.
                Ok(run) if own_function.is_some() && !run.own_ran => {
                    error = match run.ran {
                        Some(r) => format!("{} ran instead of its own function", self.name_of(setup.scene, r)),
                        None => "its own function never ran".to_string(),
                    };
                }
                Ok(run) if message_row && !run.pages.iter().any(|page| page.message.is_some_and(|m| own.contains(&m))) => {
                    let shown = run.seen.map_or(String::new(), |m| format!(": message {m:05}{} showed", self.shows(m, setup.scene)));
                    error = format!("its message never waited for the button{shown}");
                }
                // A message only another event lists showed, and none of
                // this event's own: that event's row ran.
                Ok(run) if own_function.is_none() && !run.pages.iter().any(|page| page.message.is_some()) && run.seen.is_some_and(|m| self.listed_elsewhere(m, setup.scene, event)) => {
                    let m = run.seen.unwrap_or_default();
                    error = format!("message {m:05}{} showed instead of its own", self.shows(m, setup.scene));
                }
                Ok(run) if !messages.is_empty() && run.pages.is_empty() => {
                    error = match run.seen {
                        Some(m) => format!("message {m:05}, not one of its own, showed and none of its own waited for the button"),
                        None => "it began, but no message waited for the button".to_string(),
                    }
                }
                // A row of the same trigger whose condition held played
                // instead: none of this event's messages showed.
                Ok(run) if run.rows > 1 && !messages.is_empty() && !run.pages.iter().any(|page| page.message.is_some()) => {
                    let shown = run.seen.map_or(String::new(), |m| format!(" (message {m:05}{})", self.shows(m, setup.scene)));
                    error = format!("another row for its trigger played instead{shown}");
                }
                Ok(run) if run.instant && !run.shown => error = "its dispatch returned at once from every spot: it ran no script".to_string(),
                Ok(run) if silent && messages.is_empty() && !run.shown => {
                    error = "it ran, but nothing moved, showed or changed from any spot".to_string();
                }
                Ok(run) => return Ok(run),
                Err(e) => error = self.name_other(e, setup.scene),
            }
        }
        Err(error)
    }

    /// The results of an event's own row in scene `scene`, as the table
    /// holds them (field::result_key): the function it is named by, from
    /// the overlay's symbols, or for a message row the message it shows.
    /// None when neither is known.
    fn own(&self, event: &Event, scene: u16) -> Vec<u32> {
        if let Some(&address) = self.functions.get(&scene).and_then(|f| f.get(&event.name)) {
            return vec![result_key(address)];
        }
        if is_message_row(&event.name) && event.messages.len() == 1 {
            return event.messages.clone();
        }
        Vec::new()
    }

    /// The function at `address` in scene `scene`, by name.
    fn name_of(&self, scene: u16, address: u32) -> String {
        self.functions
            .get(&scene)
            .and_then(|f| f.iter().find(|(_, &a)| result_key(a) == address))
            .map_or_else(|| format!("the function at {address:#x}"), |(n, _)| n.clone())
    }

    /// Whether message `m` is one only other events of the scene list.
    fn listed_elsewhere(&self, m: u32, scene: u16, event: &Event) -> bool {
        !event.messages.contains(&m) && self.events.iter().any(|e| e.scenes.contains(&scene) && e.name != event.name && e.messages.contains(&m))
    }

    /// `, which <event> shows` for message `m` of a scene, when an event
    /// lists it.
    fn shows(&self, m: u32, scene: u16) -> String {
        self.events
            .iter()
            .find(|e| e.scenes.contains(&scene) && e.messages.contains(&m))
            .map_or(String::new(), |e| format!(", which {} shows", e.name))
    }

    /// An error naming another event's trigger (`another event began (talk
    /// 18)`) with that event's name, when the scene has one for it.
    fn name_other(&self, e: String, scene: u16) -> String {
        let Some(rest) = e.strip_prefix("another event began (") else {
            return e;
        };
        let inner = rest.trim_end_matches(')');
        let trigger = parse_trigger(inner).ok();
        let other = trigger.and_then(|t| {
            self.events.iter().find(|ev| ev.scenes.contains(&scene) && ev.alternatives.iter().any(|a| a.trigger.as_ref().ok() == Some(&t)))
        });
        match other {
            Some(ev) => format!("{} began first ({inner})", ev.name),
            None => e,
        }
    }

    /// Removes an event's GIF and pages from a scene's folder.
    fn remove_event_files(&self, scene: &Scene, name: &str) {
        let base = file_name(name);
        let folder = self.out.join(scene.slug()).join("events");
        let Ok(entries) = std::fs::read_dir(&folder) else {
            return;
        };
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            let Some(stem) = file.strip_suffix(".gif").or_else(|| file.strip_suffix(".png")) else {
                continue;
            };
            let page = stem.strip_prefix(base.as_str()).and_then(|r| r.strip_prefix('-')).is_some_and(|n| n.parse::<u32>().is_ok());
            if stem == base || page {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    /// An event's pages and its GIF.
    fn write_event(&mut self, scene: &Scene, entrance: i16, event: &Event, run: &EventRun) -> Result<(), String> {
        let base = file_name(&event.name);
        let messages = event.messages.iter().map(|m| format!("{m:05}")).collect::<Vec<_>>().join(", ");
        for (k, page) in run.pages.iter().enumerate() {
            let file = if k == 0 {
                format!("{}/events/{base}.png", scene.slug())
            } else {
                format!("{}/events/{base}-{}.png", scene.slug(), k + 1)
            };
            self.write(&page.image, &file)?;
            self.rows.push(Row {
                scene: scene.slug(),
                number: scene.number,
                area: scene.area.clone(),
                kind: "event",
                entrance,
                event: event.name.clone(),
                page: k + 1,
                message: page.message.map(|m| format!("{m:05}")).unwrap_or_default(),
                messages: messages.clone(),
                file,
                ..Row::default()
            });
        }
        if run.frames.is_empty() {
            return Err("no frame was recorded".to_string());
        }
        let file = format!("{}/events/{base}.gif", scene.slug());
        let written = self.gif(&file, &run.frames)?;
        let leads_to = run.leads_to.map(|n| match self.scenes.iter().find(|s| s.number == n).and_then(|s| s.symbol.as_deref()) {
            Some(symbol) => format!("SceneId_{symbol}"),
            None => format!("#{n}"),
        });
        self.rows.push(Row {
            scene: scene.slug(),
            number: scene.number,
            area: scene.area.clone(),
            kind: "event-gif",
            entrance,
            event: event.name.clone(),
            frames: written,
            leads_to: leads_to.unwrap_or_default(),
            note: run.note.clone().unwrap_or_default(),
            messages,
            file,
            ..Row::default()
        });
        Ok(())
    }
}

/// An event's name as a file name: letters, digits and underscores kept,
/// anything else a hyphen (`gXianInteriors_Events-65`).
fn file_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// The scenes whose initialization tests story flags, with those flags in
/// the order it tests them.
pub fn variant_candidates(events: &[Event], flags: &Flags) -> BTreeMap<u16, Vec<String>> {
    let mut out: BTreeMap<u16, Vec<String>> = BTreeMap::new();
    for e in events.iter().filter(|e| e.trigger_text.starts_with("entry")) {
        for t in e.tests.iter().filter(|t| is_story_flag(t, flags) && !VARIANTS.iter().any(|v| v.flag == *t)) {
            for &s in &e.scenes {
                let list = out.entry(s).or_default();
                if !list.contains(t) {
                    list.push(t.clone());
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_index_keeps_its_columns_by_name() {
        let old = "scene\tnumber\tarea\tvariant\tkind\tentrance\tevent\tpage\tsegment\tmessage\tafter\tbefore\tframes\tmessages\tfile";
        let line = same_columns(old, "vale\t4\tVale\t\tevent-gif\t0\tEvent_X\t\t\t\t\t\t90\t00001\tvale/events/Event_X.gif");
        assert_eq!(line, "vale\t4\tVale\t\tevent-gif\t0\tEvent_X\t\t\t\t\t\t90\t\t\t00001\tvale/events/Event_X.gif");
        assert_eq!(same_columns(INDEX_HEADER, "a\tb"), "a\tb");
    }

    #[test]
    fn event_names_become_file_names() {
        assert_eq!(file_name("Event_InspectWell"), "Event_InspectWell");
        assert_eq!(file_name("gXianInteriors_Events[65]"), "gXianInteriors_Events-65");
    }

    #[test]
    fn rows_of_one_name_are_one_event() {
        let row = |name: &str, scene: u16, trigger: Trigger, message: u32| Event {
            scenes: vec![scene],
            name: name.to_string(),
            trigger_text: String::new(),
            alternatives: vec![Alternative { trigger: Ok(trigger), conditions: Vec::new() }],
            tests: Vec::new(),
            sets: Vec::new(),
            clears: Vec::new(),
            messages: vec![message],
        };
        let rocks = [row("MsgValeRockCannotBeMoved", 3, Trigger::Talk(11), 4385), row("MsgValeRockCannotBeMoved", 3, Trigger::Talk(12), 4385)];
        let hops = [row("KolimaForest_HopEast", 40, Trigger::Touch(11), 1), row("KolimaForest_HopEast", 42, Trigger::Touch(11), 2)];
        let all: Vec<&Event> = rocks.iter().chain(hops.iter()).collect();
        let merged = merge_rows(&all);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].alternatives.len(), 2);
        assert_eq!(merged[0].messages, vec![4385]);
        assert_eq!(merged[1].scenes, vec![40, 42]);
        assert_eq!(merged[1].messages, vec![1, 2]);
    }

    #[test]
    fn moments_are_named_by_their_flags() {
        assert_eq!(moment_label("FLAG_VALE_STORM_SAW_RIVER_RESCUE"), "vale-storm-saw-river-rescue");
        assert_eq!(moment_label("FLAG_KOLIMA_CURSE_LIFTED"), "cured");
    }

    /// An event that sets `sets` and clears `clears` when `conditions` hold.
    fn setter(sets: &[&str], conditions: &[(&str, bool)], clears: &[&str]) -> Event {
        let names = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<String>>();
        Event {
            scenes: vec![3],
            name: String::new(),
            trigger_text: String::new(),
            alternatives: vec![Alternative {
                trigger: Ok(Trigger::Touch(1)),
                conditions: conditions.iter().map(|(n, on)| (n.to_string(), *on)).collect(),
            }],
            tests: Vec::new(),
            sets: names(sets),
            clears: names(clears),
            messages: Vec::new(),
        }
    }

    #[test]
    fn moments_follow_the_story_and_end_the_night() {
        // The stormy night, numbered as flags.h numbers it, tested by the
        // scene in its own order (the later riverbank state first).
        let numbers: HashMap<&str, u16> = [
            ("STORM", 0x834),
            ("ROCK", 0x830),
            ("JOINED", 0x837),
            ("GATHERING", 0x841),
            ("LEFT", 0x842),
            ("RESCUE", 0x83a),
            ("AID", 0x840),
            ("PARTY_LEFT", 0x815),
        ]
        .into_iter()
        .collect();
        let events = vec![
            setter(&["STORM"], &[], &[]),
            setter(&[], &[], &["STORM"]),
            setter(&["ROCK"], &[], &[]),
            setter(&["JOINED"], &[("GATHERING", false), ("JOINED", false)], &[]),
            setter(&["RESCUE"], &[("RESCUE", false)], &[]),
            setter(&["AID"], &[("STORM", true), ("PARTY_LEFT", false)], &[]),
            setter(&["GATHERING"], &[("AID", true)], &[]),
            setter(&["LEFT"], &[("GATHERING", true), ("LEFT", false)], &[]),
            setter(&["PARTY_LEFT"], &[], &[]),
            // A villager who speaks only while the storm is on, before the
            // party leaves.
            setter(&[], &[("STORM", true), ("PARTY_LEFT", false)], &[]),
        ];
        let names = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<String>>();
        let candidates = names(&["STORM", "ROCK", "JOINED", "GATHERING", "LEFT", "RESCUE", "PARTY_LEFT"]);
        let number = |n: &str| numbers.get(n).copied();
        let moments = story_moments(&candidates, &[], &events, &number);
        let order: Vec<&str> = moments.iter().map(|m| m.flag.as_str()).collect();
        assert_eq!(order, ["ROCK", "STORM", "JOINED", "RESCUE", "GATHERING", "LEFT", "PARTY_LEFT"]);
        assert_eq!(moments[5].set, names(&["ROCK", "STORM", "JOINED", "RESCUE", "GATHERING", "LEFT"]));
        assert!(moments[5].clear.is_empty());
        // Leaving Vale ends the night: no condition sees the storm with it.
        assert_eq!(moments[6].set, names(&["ROCK", "JOINED", "RESCUE", "GATHERING", "LEFT", "PARTY_LEFT"]));
        assert_eq!(moments[6].clear, names(&["STORM"]));
        // The scene's own arrival comes first.
        let moments = story_moments(&candidates, &names(&["STORM"]), &events, &number);
        assert_eq!(moments[0].flag, "STORM");
    }

    #[test]
    fn message_rows_are_told_from_functions() {
        assert!(is_message_row("MsgValeRockCannotBeMoved"));
        assert!(is_message_row("gValeStorm_Events[26]"));
        assert!(!is_message_row("Event_PushPillar"));
    }
}

impl Context<'_> {
    fn gif(&mut self, file: &str, frames: &[Vec<u16>]) -> Result<usize, String> {
        let path = self.out.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let pictures: Vec<&[u16]> = frames.iter().map(Vec::as_slice).collect();
        let written = super::animation::write_bgr555(&path, &pictures, super::mgba::WIDTH)?;
        self.bytes += written.bytes;
        self.images += 1;
        self.gifs += 1;
        self.gif_bytes += written.bytes;
        Ok(written.frames)
    }
}
