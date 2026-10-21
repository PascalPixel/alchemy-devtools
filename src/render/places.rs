//! `render place` and `render event`: the photo of a scene's main entrance,
//! a map of each of its rooms, story-state variants, and the message pages
//! of its events, as PNG files at the GBA's own size, with an index.

use super::field::{
    box_holds, Entrance, EventRun, Field, Image, Saved, Setup, Start, Symbols, EVENT_EXIT, EVENT_LIMIT, EVENT_PSYNERGY, EVENT_SEARCH, EVENT_TALK,
    EVENT_TOUCH, SETTLE,
};
use super::story::{parse_trigger, Alternative, Event, Flags, Scene, Trigger, MOVE};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Pages kept of each event.
const MAX_PAGES: usize = 4;
/// Frames a gauge takes to slide off the screen.
const HUD_FRAMES: u32 = 30;

/// A story state shown as its own set of pictures: scenes whose
/// initialization tests `flag` are shown with it clear (`clear`, the new
/// game's) and set (`set`).
struct Variant {
    flag: &'static str,
    clear: &'static str,
    set: &'static str,
}

const VARIANTS: [Variant; 1] = [Variant { flag: "FLAG_KOLIMA_CURSE_LIFTED", clear: "cursed", set: "cured" }];

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
}

const INDEX_HEADER: &str =
    "scene\tnumber\tarea\tvariant\tkind\tentrance\tevent\tpage\tsegment\tmessage\tafter\tbefore\tframes\tmessages\tfile";

impl Row {
    fn line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
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
            self.messages,
            self.file
        )
    }
}

/// Writes `rows` into index.tsv, keeping the lines already there for other
/// files, sorted by file.
pub fn write_index(out: &Path, rows: &[Row]) -> Result<(), String> {
    let path = out.join("index.tsv");
    let mut lines: BTreeMap<String, String> = BTreeMap::new();
    if let Ok(text) = std::fs::read_to_string(&path) {
        for line in text.lines().skip(1) {
            if let Some(file) = line.rsplit('\t').next() {
                lines.insert(file.to_string(), line.to_string());
            }
        }
    }
    for row in rows {
        lines.insert(row.file.clone(), row.line());
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
    /// old lines go.
    pub cleared: Vec<(String, String)>,
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

    /// The variants a scene is shown in: (label, flags set, flags cleared).
    fn variants(&self, scene: &Scene) -> Vec<(String, Vec<u16>, Vec<u16>)> {
        let mut out = Vec::new();
        let name = scene.symbol.as_deref();
        for v in &VARIANTS {
            let tested = self.events.iter().any(|e| {
                e.scenes.contains(&scene.number) && e.trigger_text.starts_with("entry") && e.tests.iter().any(|t| t == v.flag)
            });
            if let (true, Some(flag)) = (tested, self.flags.resolve(v.flag, name)) {
                out.push((v.clear.to_string(), Vec::new(), vec![flag]));
                out.push((v.set.to_string(), vec![flag], Vec::new()));
            }
        }
        if out.is_empty() {
            out.push((String::new(), Vec::new(), Vec::new()));
        }
        out
    }

    /// The entrance photo and a map of each room, for each variant.
    pub fn place(&mut self, scene: &Scene) {
        if let Err(reason) = is_place(scene) {
            self.skip(scene, "place", reason);
            return;
        }
        for (variant, set, clear) in self.variants(scene) {
            if let Err(e) = self.place_variant(scene, &variant, &set, &clear) {
                self.skip(scene, &format!("place {variant}"), e);
            }
        }
    }

    fn place_variant(&mut self, scene: &Scene, variant: &str, set: &[u16], clear: &[u16]) -> Result<(), String> {
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
                let row = |kind, file: String| Row {
                    scene: scene.slug(),
                    number: scene.number,
                    area: scene.area.clone(),
                    variant: variant.to_string(),
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
                Ok(())
            })();
            if let Err(e) = result {
                self.skip(scene, &format!("{folder} room of entrance {}", room.number), e);
            }
        }
        if scene.map == "world" {
            self.skip(scene, "maps", "the perspective world map has no layered map to stitch".to_string());
        }
        Ok(())
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
    /// The triggers its event table answers, as (kind, target).
    triggers: Vec<(u8, u16)>,
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
struct Job {
    scans: HashMap<ScanKey, Result<Scan, String>>,
    ends: HashMap<(String, Vec<u16>, Vec<u16>), Option<(Setup, Saved)>>,
}

/// Whether an event is a row of a scene's event table that only shows a
/// message, named by it (MsgValeRockCannotBeMoved): it changes nothing
/// another event could need.
fn is_message_row(name: &str) -> bool {
    name.starts_with("Msg") || name.contains('[') || name.chars().next().is_some_and(|c| c.is_ascii_digit())
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
        let cells = field.cells_by_code();
        let triggers = field.triggers();
        Ok(Scan { entrances, header, actors, absent, cells, triggers })
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
            let centre = |&(_, cx, cz): &(u8, i32, i32)| (cx * 16 + 8, cz * 16 + 8);
            match trigger {
                Trigger::Talk(a) | Trigger::Psynergy(_, a) => scan.actors.get(&a).copied().into_iter().collect(),
                Trigger::Search(c) => scan.cells.get(&c).map_or_else(Vec::new, |v| v.iter().filter(|c| c.0 == 0).map(centre).collect()),
                Trigger::Touch(c) => scan.cells.get(&c).map_or_else(Vec::new, |v| v.iter().map(centre).collect()),
            }
        };
        // Whether the event table a scan's entrance gives answers the
        // trigger.
        let answers = |scan: &Scan| match trigger {
            Trigger::Talk(a) => scan.triggers.contains(&(EVENT_TALK, a)),
            Trigger::Search(c) => scan.triggers.contains(&(EVENT_SEARCH, c)),
            Trigger::Touch(c) => scan.triggers.contains(&(EVENT_TOUCH, c)) || scan.triggers.contains(&(EVENT_EXIT, c)),
            Trigger::Psynergy(..) => scan.triggers.iter().any(|t| t.0 == EVENT_PSYNERGY),
        };
        for scene in scenes.iter().filter(|s| is_place(s).is_ok()) {
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
                if answers(scan) && here.iter().any(|&(x, z)| box_holds(e.camera, scan.header, x, z)) {
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
        let mut job = Job::default();
        let numbers: Vec<u16> = scenes.iter().map(|s| s.number).collect();
        let rows: Vec<&Event> = self
            .events
            .iter()
            .filter(|e| self.wanted.is_empty() || self.wanted.iter().any(|w| *w == e.name))
            .filter(|e| e.scenes.iter().any(|n| numbers.contains(n)))
            .filter(|e| self.only.is_empty() || e.scenes.iter().any(|n| self.only.contains(n)))
            .collect();
        for event in merge_rows(&rows) {
            let candidates: Vec<&Scene> = scenes.iter().copied().filter(|s| event.scenes.contains(&s.number)).collect();
            let Some(&first) = candidates.first() else {
                continue;
            };
            let label = format!("event {}", event.name);
            match self.play_event(&event, &candidates, &mut job, Some(EVENT_LIMIT as usize), None) {
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
            let flag_sets = std::iter::once(set.clone()).chain(extras.iter().map(|&f| {
                let mut s = set.clone();
                s.push(f);
                s.sort_unstable();
                s
            }));
            let mut placed = false;
            for flags in flag_sets {
                let places = match self.places(scenes, trigger, &flags, &clear, &mut job.scans) {
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
                    // stage, a flag, or what a touch or a cast moves.
                    && (failure.absent || !e.sets.is_empty() || matches!(e.trigger(), Ok(Trigger::Touch(_) | Trigger::Psynergy(..))))
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
        let mut field = Field::enter(self.rom, self.symbols, setup)?;
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
            match field.event(start, trigger, spot, messages, MAX_PAGES, record) {
                Ok(run) if !strict => return Ok(run),
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
                Ok(run) if run.instant => error = "its dispatch returned at once from every spot: it ran no script".to_string(),
                Ok(run) if silent && messages.is_empty() && !run.shown => {
                    error = "it ran, but nothing moved, showed or changed from any spot".to_string();
                }
                Ok(run) => return Ok(run),
                Err(e) => error = self.name_other(e, setup.scene),
            }
        }
        Err(error)
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
        self.rows.push(Row {
            scene: scene.slug(),
            number: scene.number,
            area: scene.area.clone(),
            kind: "event-gif",
            entrance,
            event: event.name.clone(),
            frames: written,
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

/// The scenes whose initialization tests story flags not yet shown as
/// variants, with those flags.
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
