//! `render place` and `render event`: the photo of a scene's main entrance,
//! a map of each of its rooms, story-state variants, and the message pages
//! of its events, as PNG files at the GBA's own size, with an index.

use super::field::{box_holds, Entrance, EventFrame, EventRun, Field, Image, Saved, Setup, Symbols, SETTLE};
use super::story::{Event, Flags, Scene, Trigger};
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
    /// Message numbers to their symbols (messages.tsv), for the segments.
    pub messages: &'a HashMap<u32, String>,
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
/// holds the trigger, in the event's scene order.
const MAX_PLACES: usize = 3;

/// An event to play, and how far its attempts have got.
struct Pending<'e> {
    event: &'e Event,
    trigger: Trigger,
    set: Vec<u16>,
    clear: Vec<u16>,
    /// Scene and entrance pairs that hold the trigger.
    places: Vec<(u16, Entrance)>,
    /// The place and the leader's placement tried next.
    place: usize,
    placement: usize,
    error: String,
    done: bool,
}

/// What a scene shows with given flags, entered by an entrance: its
/// entrances, the header's bounds and where its triggers are.
struct Scan {
    entrances: Vec<Entrance>,
    header: [i32; 4],
    actors: HashMap<u16, (i32, i32)>,
    cells: HashMap<u16, (i32, i32)>,
}

type ScanKey = (u16, u16, Vec<u16>, Vec<u16>);

impl Context<'_> {
    fn scan(&self, scene: u16, entrance: u16, set: &[u16], clear: &[u16]) -> Result<Scan, String> {
        let setup = Setup { scene, entrance, set: set.to_vec(), clear: clear.to_vec() };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        let entrances = field.entrances();
        let header = field.header();
        field.until_control()?;
        field.frames(SETTLE)?;
        let actors = (0..66u16).filter_map(|id| field.actor(id).map(|p| (id, p))).collect();
        let cells = (1..256u16).filter_map(|code| field.cell_with_code(code).map(|p| (code, p))).collect();
        Ok(Scan { entrances, header, actors, cells })
    }

    /// The scene and entrance pairs where `trigger` stands, scanning the
    /// scene by its first entrance and then by the first entrance of each of
    /// its other rooms.
    fn places(
        &self,
        scenes: &[&Scene],
        trigger: Trigger,
        set: &[u16],
        clear: &[u16],
        scans: &mut HashMap<ScanKey, Result<Scan, String>>,
    ) -> Result<Vec<(u16, Entrance)>, String> {
        let mut out = Vec::new();
        let mut error = None;
        for scene in scenes.iter().filter(|s| is_place(s).is_ok()) {
            let mut entrance = 0u16;
            let mut tried: Vec<[i16; 4]> = Vec::new();
            loop {
                let key = (scene.number, entrance, set.to_vec(), clear.to_vec());
                if !scans.contains_key(&key) {
                    let scan = self.scan(scene.number, entrance, set, clear);
                    scans.insert(key.clone(), scan);
                }
                let scan = match &scans[&key] {
                    Ok(scan) => scan,
                    Err(e) => {
                        error.get_or_insert_with(|| format!("{}: {e}", scene.name()));
                        break;
                    }
                };
                let at = match trigger {
                    Trigger::Talk(a) | Trigger::Psynergy(_, a) => scan.actors.get(&a),
                    Trigger::Search(c) | Trigger::Touch(c) => scan.cells.get(&c),
                };
                if let Some(&(x, z)) = at {
                    let holder = scan
                        .entrances
                        .iter()
                        .find(|e| box_holds(e.camera, scan.header, x, z))
                        .or_else(|| scan.entrances.first());
                    if let Some(&e) = holder {
                        out.push((scene.number, e));
                    }
                    break;
                }
                // Not there by this entrance: try the next room's.
                tried.push(scan.entrances.iter().find(|e| e.number as u16 == entrance).map_or([-1; 4], |e| e.camera));
                let next = scan.entrances.iter().find(|e| !tried.contains(&e.camera) && e.number >= 0);
                match next {
                    Some(e) => {
                        tried.push(e.camera);
                        entrance = e.number as u16;
                    }
                    None => break,
                }
            }
            if out.len() >= MAX_PLACES {
                break;
            }
        }
        if out.is_empty() {
            return Err(error.unwrap_or_else(|| "the trigger is in none of its scenes".to_string()));
        }
        Ok(out)
    }

    /// The message pages of every playable event of `scenes`. Each event is
    /// tried at each of its places with each placement of the leader until a
    /// page waits; every round plays the next attempt of each event still
    /// waiting, grouped by where they start.
    pub fn events(&mut self, scenes: &[&Scene]) {
        let by_number: HashMap<u16, &Scene> = scenes.iter().map(|s| (s.number, *s)).collect();
        let mut scans: HashMap<ScanKey, Result<Scan, String>> = HashMap::new();
        let mut pending: Vec<Pending> = Vec::new();
        let events = self.events;
        for event in events {
            let candidates: Vec<&Scene> = event.scenes.iter().filter_map(|n| by_number.get(n).copied()).collect();
            let Some(&first) = candidates.first() else {
                continue;
            };
            let label = format!("event {}", event.name);
            let trigger = match (&event.trigger, event.messages.is_empty()) {
                (_, true) => {
                    self.skip(first, &label, "no message".to_string());
                    continue;
                }
                (Err(reason), _) => {
                    self.skip(first, &label, reason.clone());
                    continue;
                }
                (Ok(t), _) => *t,
            };
            let (mut set, mut clear) = (Vec::new(), Vec::new());
            let mut unknown = None;
            for (name, on) in &event.conditions {
                match self.flags.resolve(name, first.symbol.as_deref()) {
                    Some(flag) => {
                        set.retain(|&f| f != flag);
                        clear.retain(|&f| f != flag);
                        if *on { &mut set } else { &mut clear }.push(flag);
                    }
                    None => unknown = Some(name.clone()),
                }
            }
            if let Some(name) = unknown {
                self.skip(first, &label, format!("unknown flag {name}"));
                continue;
            }
            set.sort_unstable();
            clear.sort_unstable();
            match self.places(&candidates, trigger, &set, &clear, &mut scans) {
                Ok(places) => pending.push(Pending {
                    event,
                    trigger,
                    set,
                    clear,
                    places,
                    place: 0,
                    placement: 0,
                    error: String::new(),
                    done: false,
                }),
                Err(e) => self.skip(first, &label, e),
            }
        }
        drop(scans);
        let mut names: HashMap<(u16, String), usize> = HashMap::new();
        loop {
            let mut groups: BTreeMap<Setup, Vec<usize>> = BTreeMap::new();
            for (i, p) in pending.iter().enumerate().filter(|(_, p)| !p.done && p.place < p.places.len()) {
                let (scene, entrance) = p.places[p.place];
                let mut set = p.set.clone();
                if entrance.required_flag >= 0 && !set.contains(&(entrance.required_flag as u16)) {
                    set.push(entrance.required_flag as u16);
                    set.sort_unstable();
                }
                let setup = Setup { scene, entrance: entrance.number as u16, set, clear: p.clear.clone() };
                groups.entry(setup).or_default().push(i);
            }
            if groups.is_empty() {
                break;
            }
            for (setup, members) in groups {
                let scene = by_number[&setup.scene];
                if let Err(e) = self.play_group(scene, &setup, &members, &mut pending, &mut names) {
                    for &i in &members {
                        let p = &mut pending[i];
                        p.error = e.clone();
                        p.place += 1;
                        p.placement = 0;
                    }
                }
            }
        }
        for p in pending.iter().filter(|p| !p.done) {
            let scene = by_number[&p.places[0].0];
            self.skip(scene, &format!("event {}", p.event.name), p.error.clone());
        }
    }

    fn play_group(
        &mut self,
        scene: &Scene,
        setup: &Setup,
        members: &[usize],
        pending: &mut [Pending],
        names: &mut HashMap<(u16, String), usize>,
    ) -> Result<(), String> {
        let mut field = Field::enter(self.rom, self.symbols, setup)?;
        let entered: Saved = field.save();
        field.until_control()?;
        field.frames(SETTLE)?;
        let starts: Vec<Option<Result<(i32, i32, u32), String>>> =
            members.iter().map(|&i| field.plan(pending[i].trigger, pending[i].placement)).collect();
        for (&i, start) in members.iter().zip(starts) {
            let p = &mut pending[i];
            let start = match start {
                Some(Ok(s)) => s,
                Some(Err(e)) => {
                    p.error = e;
                    p.place += 1;
                    p.placement = 0;
                    continue;
                }
                None => {
                    p.place += 1;
                    p.placement = 0;
                    continue;
                }
            };
            p.placement += 1;
            let run = match field.event(&entered, p.trigger, start, &p.event.messages, MAX_PAGES, Some(EVENT_CAP)) {
                Ok(run) if run.pages.is_empty() => {
                    p.error = "no page waited for the button".to_string();
                    continue;
                }
                Ok(run) => run,
                Err(e) => {
                    p.error = e;
                    continue;
                }
            };
            let pages = &run.pages;
            let event = p.event;
            let base = file_name(&event.name);
            let count = names.entry((scene.number, base.clone())).or_default();
            *count += 1;
            let base = if *count > 1 { format!("{base}~{count}") } else { base };
            let messages = event.messages.iter().map(|m| format!("{m:05}")).collect::<Vec<_>>().join(", ");
            for (k, page) in pages.iter().enumerate() {
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
                    variant: String::new(),
                    kind: "event",
                    entrance: setup.entrance as i16,
                    event: event.name.clone(),
                    page: k + 1,
                    message: page.message.map(|m| format!("{m:05}")).unwrap_or_default(),
                    messages: messages.clone(),
                    file,
                    ..Row::default()
                });
            }
            p.done = true;
            self.event_gifs(scene, setup, &event.name, &base, &messages, &run)?;
        }
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
}

/// An event's recording stops at 20 seconds.
const EVENT_CAP: usize = 1200;
/// A whole-event GIF is kept for an event under 10 seconds.
const WHOLE_EVENT: usize = 600;
/// A segment stops at 15 seconds.
const SEGMENT_CAP: usize = 900;
/// A frame moves when this many pixels change from the one before.
const MOVED_PIXELS: usize = 64;
/// Frames of motion that make an event or a segment worth a GIF.
const MOTION: usize = 20;
/// A pause this long splits a segment.
const PAUSE: usize = 90;
/// Frames of the next message kept at a segment's end: its typing and part
/// of its hold.
const MESSAGE_TAIL: usize = 90;

fn moved(a: &EventFrame, b: &EventFrame) -> bool {
    a.picture.iter().zip(&b.picture).filter(|(x, y)| x != y).count() >= MOVED_PIXELS
}

/// Frames a message window takes to open or close, around its text.
const WINDOW: usize = 12;

/// The frames that move, leaving out those within WINDOW of text, where
/// only a message window opens or closes.
fn motion_frames(frames: &[EventFrame]) -> Vec<bool> {
    let mut near = vec![false; frames.len()];
    for (i, f) in frames.iter().enumerate().filter(|(_, f)| f.text) {
        let _ = f;
        for n in near.iter_mut().take((i + WINDOW + 1).min(frames.len())).skip(i.saturating_sub(WINDOW)) {
            *n = true;
        }
    }
    (0..frames.len()).map(|i| i > 0 && !near[i] && moved(&frames[i - 1], &frames[i])).collect()
}

/// A stretch of an event between two of its messages.
struct Segment {
    /// The messages shown before it.
    index: usize,
    range: std::ops::Range<usize>,
    after: Option<u32>,
    before: Option<u32>,
}

/// The stretches of an event with motion between its messages: each run of
/// frames with no text, split at a scene change or a pause, ending with the
/// next message typed.
fn segments(frames: &[EventFrame]) -> Vec<Segment> {
    let moving = motion_frames(frames);
    let mut out = Vec::new();
    let mut texts = 0;
    let mut i = 0;
    while i < frames.len() {
        if frames[i].text {
            // A run of text is one message shown (or more, back to back).
            while i < frames.len() && frames[i].text {
                i += 1;
            }
            texts += 1;
            continue;
        }
        let start = i;
        while i < frames.len() && !frames[i].text {
            i += 1;
        }
        // Split the run at scene changes and long pauses.
        let mut pieces = Vec::new();
        let mut piece_start = start;
        let mut still = 0;
        for j in start + 1..i {
            let change = frames[j].scene != frames[j - 1].scene;
            still = if moved(&frames[j - 1], &frames[j]) { 0 } else { still + 1 };
            if change || still == PAUSE {
                pieces.push(piece_start..j);
                piece_start = j;
                still = 0;
            }
        }
        pieces.push(piece_start..i);
        let last = pieces.len() - 1;
        for (n, piece) in pieces.into_iter().enumerate() {
            let motion = (piece.start..piece.end).filter(|&j| moving[j]).count();
            // The stretch after the last message, until the field loop runs
            // again, needs more: the actors there turn back and wander.
            let tail = i >= frames.len();
            if motion < if tail { 3 * MOTION } else { MOTION } {
                continue;
            }
            let mut end = piece.end;
            if n == last {
                // The next message, typed and held a moment.
                while end < frames.len() && end < piece.end + MESSAGE_TAIL && frames[end].text {
                    end += 1;
                }
            }
            let end = end.min(piece.start + SEGMENT_CAP);
            let after = (start > 0).then(|| frames[start - 1].message);
            let before = (i < frames.len()).then(|| frames[(i + 5).min(frames.len() - 1)].message);
            out.push(Segment { index: texts, range: piece.start..end, after, before });
        }
    }
    out
}

impl Context<'_> {
    /// An event's GIFs, when it moves: the whole event when it is short, and
    /// a GIF of each stretch with motion between its messages.
    fn event_gifs(
        &mut self,
        scene: &Scene,
        setup: &Setup,
        name: &str,
        base: &str,
        messages: &str,
        run: &EventRun,
    ) -> Result<(), String> {
        let frames = &run.frames;
        let motion = motion_frames(frames).iter().filter(|&&m| m).count();
        if motion < MOTION {
            return Ok(());
        }
        let label = |m: Option<u32>| match m {
            Some(m) if m > 0 => match self.messages.get(&m) {
                Some(symbol) => format!("{m:05} {symbol}"),
                None => format!("{m:05}"),
            },
            _ => String::new(),
        };
        let row = |kind, segment, after: String, before: String, count: usize, file: String| Row {
            scene: scene.slug(),
            number: scene.number,
            area: scene.area.clone(),
            kind,
            entrance: setup.entrance as i16,
            event: name.to_string(),
            segment,
            after,
            before,
            frames: count,
            messages: messages.to_string(),
            file,
            ..Row::default()
        };
        let mut rows = Vec::new();
        if frames.len() < WHOLE_EVENT && !run.capped {
            let file = format!("{}/events/{base}.gif", scene.slug());
            let written = self.gif(&file, frames)?;
            rows.push(row("event-gif", None, String::new(), String::new(), written, file));
        }
        if run.capped {
            self.skip(scene, &format!("event {name}"), format!("its GIFs stop at {} seconds", EVENT_CAP / 60));
        }
        for s in segments(frames) {
            let file = format!("{}/events/{base}_{}.gif", scene.slug(), s.index);
            let written = self.gif(&file, &frames[s.range.clone()])?;
            rows.push(row("event-segment", Some(s.index), label(s.after), label(s.before), written, file));
        }
        self.rows.extend(rows);
        Ok(())
    }

    fn gif(&mut self, file: &str, frames: &[EventFrame]) -> Result<usize, String> {
        let rgb: Vec<Vec<u32>> = frames.iter().map(|f| f.picture.iter().map(|&c| super::animation::rgb(c)).collect()).collect();
        let path = self.out.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let written = super::animation::write(&path, &rgb, super::mgba::WIDTH)?;
        self.bytes += written.bytes;
        self.images += 1;
        self.gifs += 1;
        self.gif_bytes += written.bytes;
        Ok(written.frames)
    }
}
