//! Cells labelled in words. A picture that marks map cells says what each
//! does, never the collision code the engine looks up: a code every map
//! shares (enum MetatileBehavior) by the name the code gives it, and one of the
//! scene's own (1 to 239) by the rows of the scene's event table that answer
//! it, read as Scene_FindEvent reads them, each row's function or exit put
//! in words the picture supplies. A key beside the picture says what each
//! colour stands for.

use super::field::{Field, Image, Setup, Start};
use super::guide::Guide;
use super::guide_more::{canvas, mix, outline, paste};
use super::guide_take::{crop, room_map, text};
use super::mgba::{HEIGHT, WIDTH};
use super::story::Trigger;
use std::collections::HashMap;

pub(super) const WHITE: u32 = 0xff_ff_ff;
/// The key's text, on the white margin.
pub(super) const INK: u32 = 0x20_20_20;

/// FieldWork.events: the scene's event table, 12-byte rows of control,
/// trigger, condition and value, ended by a control of -1.
const FIELD_EVENTS: u32 = 0x10;
const EVENT_ROW_SIZE: u32 = 12;
/// enum MapEventKind's kinds that answer a cell.
const MAP_EVENT_WARP: u32 = 1;
const MAP_EVENT_COORD: u32 = 2;
const MAP_EVENT_BG: u32 = 3;

/// What answers a cell, which picks its colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(super) enum Kind {
    /// An exit row: stepping on the cell leaves by an exit.
    Exit,
    /// A touch row: stepping on or walking into the cell runs an event.
    Touch,
    /// A search row: the A button facing the cell.
    Search,
    /// One of the scene's own codes that no row answers.
    NoEvent,
    /// MB_WALL.
    Wall,
    /// MB_ENCOUNTER_GROUND_1 and _2.
    Encounters,
    /// MB_JAR to MB_WELL: a search spot every map shares.
    SearchSpot,
    /// MB_LEDGE, MB_CLIMB, MB_JUMP_FORWARD and
    /// MB_JUMP_FORWARD_UNREACHED: cells the leader pushes against.
    Push,
    /// MB_DESERT_SAND.
    Hazard,
}

impl Kind {
    pub(super) fn colour(self) -> u32 {
        match self {
            Kind::Exit => 0x30_80_f0,
            Kind::Touch => 0xe0_40_e0,
            Kind::Search => 0x30_d0_d0,
            Kind::NoEvent => 0x90_90_90,
            Kind::Wall => 0xe0_20_20,
            Kind::Encounters => 0x40_b0_40,
            Kind::SearchSpot => 0xc0_a0_60,
            Kind::Push => 0xff_90_30,
            Kind::Hazard => 0xa0_60_20,
        }
    }

    /// The key's line for the colour.
    pub(super) fn key(self) -> &'static str {
        match self {
            Kind::Exit => "Exit row: stepping on it leaves by an exit",
            Kind::Touch => "Touch row: stepping on or walking into it runs an event",
            Kind::Search => "Search row: the A button facing it",
            Kind::NoEvent => "One of the scene's own codes that no row answers",
            Kind::Wall => "Wall: blocks whatever the heights",
            Kind::Encounters => "Encounter ground: battles of its own",
            Kind::SearchSpot => "Search spot every map shares",
            Kind::Push => "Pushed against: a ledge, a climb or a jump",
            Kind::Hazard => "Hazard: hurts the party over time",
        }
    }
}

/// What a cell's code makes happen: words for its label, and what answers
/// it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct Meaning {
    pub words: String,
    pub kind: Kind,
}

/// A code every map shares, by the name enum MetatileBehavior gives it.
fn shared(code: i32) -> Option<Meaning> {
    let (words, kind) = match code {
        240 => ("Encounter ground 1", Kind::Encounters),
        241 => ("Encounter ground 2", Kind::Encounters),
        242 => ("Jar", Kind::SearchSpot),
        243 => ("Barrel", Kind::SearchSpot),
        244 => ("Wooden box", Kind::SearchSpot),
        245 => ("Grave", Kind::SearchSpot),
        246 => ("Books", Kind::SearchSpot),
        247 => ("Well", Kind::SearchSpot),
        249 => ("Jump across (never reached)", Kind::Push),
        250 => ("Hazard", Kind::Hazard),
        252 => ("Ledge: jump down", Kind::Push),
        253 => ("Climb", Kind::Push),
        254 => ("Jump across", Kind::Push),
        255 => ("Wall", Kind::Wall),
        _ => return None,
    };
    Some(Meaning { words: words.to_string(), kind })
}

/// What the rows of a picture's scene do, in words: each row's function or
/// exit by name, what it does as the player sees it, and for a door the
/// scene it leads to, which the picture checks by playing it.
pub(super) type Words = &'static [(&'static str, &'static str, Option<u16>)];

/// The names a scene's rows are read through: its overlay's functions by
/// address, and its exits by number (map_warps.h).
pub(super) struct Names {
    functions: HashMap<u32, String>,
    exits: HashMap<u32, String>,
}

/// One row of the scene's event table that answers a cell.
struct Row {
    control: u32,
    condition: i16,
    value: u32,
}

/// The rows of the scene's event table, as it is now, that answer cell
/// code `code` (exit, touch and search), in table order.
fn rows(field: &Field, code: i32) -> Vec<Row> {
    let mut out = Vec::new();
    let work = field.field_work();
    let mut a = if work == 0 { 0 } else { field.read32(work + FIELD_EVENTS) };
    for _ in 0..1024 {
        if a == 0 {
            break;
        }
        let control = field.read32(a);
        if control == u32::MAX {
            break;
        }
        let kind = control & 15;
        if (MAP_EVENT_WARP..=MAP_EVENT_BG).contains(&kind) && i32::from(field.read8(a + 4)) == code {
            out.push(Row { control, condition: field.read16(a + 6) as i16, value: field.read32(a + 8) });
        }
        a += EVENT_ROW_SIZE;
    }
    out
}

/// Whether a row's condition holds (GameFlag_IsConditionActive): -1
/// always, one with bit 0x1000 while its flag is set, any other while its
/// flag is clear.
fn holds(field: &Field, condition: i16) -> bool {
    if condition == -1 {
        return true;
    }
    let set = field.flag_is_set(condition as u16 & 0xfff);
    if condition as u16 & 0x1000 != 0 { set } else { !set }
}

impl Names {
    /// The name a row's value is read by: the exit's for an exit row, the
    /// function's for a script, else what a search finds.
    fn of(&self, row: &Row) -> String {
        let value = row.value;
        if row.control & 15 == MAP_EVENT_WARP {
            return self.exits.get(&value).cloned().unwrap_or_else(|| format!("exit {value}"));
        }
        if value >= 0x0200_0000 {
            return self.functions.get(&(value & !1)).cloned().unwrap_or_else(|| format!("the function at {value:#x}"));
        }
        match value >> 20 {
            1 => "FIND_ITEM".to_string(),
            2 => "FIND_COINS".to_string(),
            3 => "FIND_PSYNERGY_STONE".to_string(),
            4 => "FIND_MESSAGE".to_string(),
            5 => "FIND_MIMIC".to_string(),
            _ => format!("value {value:#x}"),
        }
    }
}

/// What cell code `code` makes happen in `field`'s scene now, its rows'
/// names put in `words`; None for open ground. The rows taken are those
/// Scene_FindEvent could take: each whose condition holds, and a search on
/// a named target whatever its condition (Field_SearchCell tests it
/// afterwards). Stepping on a cell runs a touch row before an exit row, so
/// a touch row gives the colour, then an exit row, then a search row.
pub(super) fn meaning(field: &Field, names: &Names, words: Words, code: i32) -> Result<Option<Meaning>, String> {
    if code == 0 {
        return Ok(None);
    }
    if let Some(m) = shared(code) {
        return Ok(Some(m));
    }
    let mut parts: Vec<String> = Vec::new();
    let mut kinds = Vec::new();
    for row in rows(field, code) {
        let kind = row.control & 15;
        let named_search = kind == MAP_EVENT_BG && (row.control >> 4) & 31 != 0;
        if !named_search && !holds(field, row.condition) {
            continue;
        }
        let name = names.of(&row);
        let said = words
            .iter()
            .find(|(n, _, _)| *n == name)
            .map(|(_, w, _)| w.to_string())
            .ok_or_else(|| format!("no words for {name}, which answers cell code {code}"))?;
        if !parts.contains(&said) {
            parts.push(said);
        }
        kinds.push(kind);
    }
    let kind = if kinds.contains(&MAP_EVENT_COORD) {
        Kind::Touch
    } else if kinds.contains(&MAP_EVENT_WARP) {
        Kind::Exit
    } else if kinds.contains(&MAP_EVENT_BG) {
        Kind::Search
    } else {
        Kind::NoEvent
    };
    let words = if parts.is_empty() { "No event".to_string() } else { parts.join("; ") };
    Ok(Some(Meaning { words, kind }))
}

/// The enum of `scene`'s warps in map_warps.h: under `SceneId_<scene>'s
/// warps.`, or `warp destinations and warps.` where one enum holds both.
fn warps_block<'t>(links: &'t str, scene: &str) -> Result<&'t str, String> {
    let heading = links
        .find(&format!("/// SceneId_{scene}'s warps."))
        .or_else(|| links.find(&format!("/// SceneId_{scene}'s warp destinations and warps.")))
        .ok_or_else(|| format!("map_warps.h lists no warps of SceneId_{scene}"))?;
    let block = &links[heading..];
    Ok(&block[..block.find("};").unwrap_or(block.len())])
}

impl<'a> Guide<'a> {
    /// The names `scene`'s rows are read through: the functions of the
    /// overlay that defines `anchor`, and its warps (`warps_block`).
    pub(super) fn cell_names(&self, anchor: &str, scene: &str) -> Result<Names, String> {
        let root = crate::compile::root();
        let dir = root.join("out/tbs-en/overlays");
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let mut functions = None;
        for entry in entries.flatten() {
            let path = entry.path();
            let file = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !file.ends_with(".elf") || file.ends_with(".symbols.elf") {
                continue;
            }
            let Ok(data) = std::fs::read(&path) else { continue };
            let Ok(elf) = crate::elf::Elf::parse(&data) else { continue };
            if !elf.symbols.iter().any(|s| s.name == anchor) {
                continue;
            }
            functions = Some(elf.symbols.iter().filter(|s| s.kind == 2 && !s.name.is_empty()).map(|s| (s.value & !1, s.name.clone())).collect());
            break;
        }
        let functions = functions.ok_or_else(|| format!("no overlay defines {anchor}"))?;
        let links_path = root.join("games/tbs/include/map_warps.h");
        let links = std::fs::read_to_string(&links_path).map_err(|e| format!("cannot read {}: {e}", links_path.display()))?;
        let block = warps_block(&links, scene)?;
        // Comments taken out, so a name and its number may stand on two lines.
        let plain: String = block.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join(" ");
        let pattern = regex::Regex::new(r"([A-Z][A-Z0-9_]+)\s*=\s*(\d+)").map_err(|e| e.to_string())?;
        let exits = pattern.captures_iter(&plain).filter_map(|c| Some((c[2].parse().ok()?, c[1].to_string()))).collect();
        Ok(Names { functions, exits })
    }

    /// Checks each door in `words` by playing it: the leader put where
    /// walking onto cell code `code` starts its row, and the scene the
    /// party lands in must be the one `words` names.
    pub(super) fn check_doors(field: &mut Field, placed: &super::field::Saved, names: &Names, words: Words, codes: &[i32]) -> Result<(), String> {
        for &code in codes {
            let found: Vec<(String, u16)> = rows(field, code)
                .iter()
                .filter(|r| holds(field, r.condition))
                .filter_map(|r| {
                    let name = names.of(r);
                    words.iter().find(|(n, _, _)| *n == name).and_then(|(_, _, s)| s.map(|s| (name, s)))
                })
                .collect();
            let Some((name, scene)) = found.first().cloned() else { continue };
            let saved = field.save();
            let trigger = Trigger::Touch(code as u16);
            let spots = field.spots(trigger)?;
            let mut led = None;
            let mut why = format!("{} places to stand", spots.len());
            for spot in spots.iter().take(6) {
                match field.event(Start::Entered(placed), trigger, spot, &[], &[], 4, None) {
                    Ok(run) => {
                        led = run.leads_to;
                        why = format!("pages {}, shown {}", run.pages.len(), run.shown);
                        if led.is_some() {
                            break;
                        }
                    }
                    Err(e) => why = e,
                }
            }
            field.load(&saved);
            if led != Some(scene) {
                return Err(format!("{name} (cell code {code}) led to scene {led:?}, not {scene} ({why})"));
            }
        }
        Ok(())
    }
}

/// Splits `s` into lines of at most `n` characters at spaces.
pub(super) fn wrap(s: &str, n: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in s.split(' ') {
        match lines.last_mut() {
            Some(line) if line.len() + 1 + word.len() <= n => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

/// A filled square of `colour` with a dark rim, the key's swatch.
pub(super) fn swatch(image: &mut Image, x: i32, y: i32, side: i32, colour: u32) {
    for dy in 0..side {
        for dx in 0..side {
            let (px, py) = (x + dx, y + dy);
            if px >= 0 && py >= 0 && (px as usize) < image.width && (py as usize) < image.height {
                image.pixels[py as usize * image.width + px as usize] = colour;
            }
        }
    }
    outline(image, x, y, side, side, INK);
}

/// The height a key entry of `words` takes at `chars` characters a line.
fn entry_height(words: &str, chars: usize) -> i32 {
    wrap(words, chars).len() as i32 * 16 + 6
}

/// A key: each entry's swatch and its words, at twice the font's size, on
/// white, `width` pixels wide, under `title` when there is one.
pub(super) fn key(entries: &[(u32, String)], width: usize, title: Option<&str>) -> Image {
    let chars = (width - 28) / 8;
    let mut h = 8 + entries.iter().map(|(_, w)| entry_height(w, chars)).sum::<i32>();
    if title.is_some() {
        h += 20;
    }
    let mut out = canvas(width, h as usize, WHITE);
    let mut y = 4;
    if let Some(t) = title {
        text(&mut out, 0, y, &t.to_uppercase(), INK, None, 2);
        y += 20;
    }
    for (colour, words) in entries {
        swatch(&mut out, 2, y + 1, 14, *colour);
        for (i, line) in wrap(words, chars).iter().enumerate() {
            text(&mut out, 22, y + 16 * i as i32, &line.to_uppercase(), INK, None, 2);
        }
        y += entry_height(words, chars);
    }
    out
}

/// Tints the cell whose top left is (sx, sy) on a picture drawn `side`
/// pixels to a cell, and outlines it, in `colour`.
pub(super) fn tint_cell(image: &mut Image, sx: i32, sy: i32, side: i32, colour: u32, share: f64) {
    for y in sy.max(0)..(sy + side).min(image.height as i32) {
        for x in sx.max(0)..(sx + side).min(image.width as i32) {
            let at = &mut image.pixels[y as usize * image.width + x as usize];
            *at = mix(*at, colour, share);
        }
    }
    outline(image, sx, sy, side, side, colour);
}

/// A line from (x0, y0) to (x1, y1), three pixels wide: `colour` on a
/// dark rim, so it shows over any picture.
pub(super) fn leader_line(image: &mut Image, (x0, y0): (i32, i32), (x1, y1): (i32, i32), colour: u32) {
    let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
    for pass in 0..2 {
        let (reach, c) = if pass == 0 { (2, INK) } else { (1, colour) };
        for i in 0..=steps {
            let x = x0 + (x1 - x0) * i / steps;
            let y = y0 + (y1 - y0) * i / steps;
            for dy in -reach + 1..reach {
                for dx in -reach + 1..reach {
                    let (px, py) = (x + dx, y + dy);
                    if px >= 0 && py >= 0 && (px as usize) < image.width && (py as usize) < image.height {
                        image.pixels[py as usize * image.width + px as usize] = c;
                    }
                }
            }
        }
    }
}

/// Colours for a picture whose cells are told apart by meaning, not only
/// by kind: a wall red, encounter ground green, a code no row answers
/// grey, and each other meaning its own colour, the first exit blue; the
/// entries, in the order first seen, are the picture's key.
#[derive(Default)]
pub(super) struct Palette {
    pub entries: Vec<(u32, String)>,
}

const PALETTE: [u32; 10] = [0xf0_d0_20, 0xe0_40_e0, 0x30_d0_d0, 0xff_90_30, 0x90_60_f0, 0x80_e0_60, 0xf0_80_b0, 0x80_b0_ff, 0xd0_d0_d0, 0xa0_a0_30];

impl Palette {
    pub(super) fn colour(&mut self, m: &Meaning) -> u32 {
        if let Some((c, _)) = self.entries.iter().find(|e| e.1 == m.words) {
            return *c;
        }
        let used = |c: u32, entries: &[(u32, String)]| entries.iter().any(|e| e.0 == c);
        let c = match m.kind {
            Kind::Wall | Kind::Encounters | Kind::NoEvent => m.kind.colour(),
            Kind::Exit if !used(Kind::Exit.colour(), &self.entries) => Kind::Exit.colour(),
            _ => PALETTE.iter().copied().find(|&c| !used(c, &self.entries)).unwrap_or(0xff_ff_ff),
        };
        self.entries.push((c, m.words.clone()));
        c
    }
}

/// One label of a labelled picture: its colour, its words, and the point on
/// the picture its line starts from (the right edge of its rightmost cell).
pub(super) type Mark = (u32, String, (i32, i32));

/// `picture` with a column of labels beside it, each joined by a line to
/// its mark: the labels run down the picture in the order of their marks,
/// each level with its mark where the ones above leave room, so the lines
/// stay short and do not cross; `legend` says under it all what each
/// colour stands for. `key_width` is the labels' column.
pub(super) fn labelled(picture: &Image, marks: &[Mark], legend: &[(u32, String)], key_width: usize) -> Image {
    let chars = (key_width - 28) / 8;
    let mut order: Vec<usize> = (0..marks.len()).collect();
    order.sort_by_key(|&i| marks[i].2 .1);
    let mut placed = vec![0i32; marks.len()];
    let mut end = 4;
    for &i in &order {
        let y = (marks[i].2 .1 + 4 - 8).max(end);
        placed[i] = y;
        end = y + entry_height(&marks[i].1, chars);
    }
    let width = 4 + picture.width + 12 + key_width + 4;
    let body = (4 + picture.height as i32 + 4).max(end);
    let colours = if legend.is_empty() { canvas(1, 1, WHITE) } else { key(legend, width - 8, Some("Colours")) };
    let mut out = canvas(width, body as usize + 8 + colours.height, WHITE);
    paste(&mut out, picture, 4, 4, 1);
    let kx = 4 + picture.width as i32 + 12;
    for (i, (colour, _, (ax, ay))) in marks.iter().enumerate() {
        leader_line(&mut out, (4 + ax, 4 + ay), (kx + 2, placed[i] + 8), *colour);
    }
    for (i, (colour, words, _)) in marks.iter().enumerate() {
        swatch(&mut out, kx + 2, placed[i] + 1, 14, *colour);
        for (n, line) in wrap(words, chars).iter().enumerate() {
            text(&mut out, kx + 22, placed[i] + 16 * n as i32, &line.to_uppercase(), INK, None, 2);
        }
    }
    if !legend.is_empty() {
        paste(&mut out, &colours, 4, body as usize + 4, 1);
    }
    out
}

/// What the rows of the Goma Cave room of two pillars and a stump
/// (SceneId_GomaCavePillar) do: its exits lead to the rooms scene_ids.h
/// names from their renders.
pub(super) const GOMA_CAVE_PILLAR_WORDS: Words = &[
    ("GOMA_CAVE_PILLAR_TO_GOMA_CAVE_STREAM_WEST", "Way out to the room of the stream", None),
    ("GOMA_CAVE_PILLAR_TO_GOMA_CAVE_STREAM_EAST", "Way out to the room of the stream", None),
    ("GOMA_CAVE_PILLAR_TO_GOMA_CAVE_POND_WEST", "Way down to the room of the pond", None),
    ("GOMA_CAVE_PILLAR_TO_GOMA_CAVE_POND_SOUTH", "Way down to the room of the pond", None),
    ("GOMA_CAVE_PILLAR_TO_GOMA_CAVE_POND_EAST", "Way down to the room of the pond", None),
    ("EventScript_JumpGapWest", "Jump the gap west (walk west into it)", None),
    ("EventScript_PushEastPillar", "Push the east stone pillar a step (walk into it)", None),
    ("FIND_ITEM", "Chest", None),
];

/// What the rows of Fuchin Falls Cave's first room
/// (gImilFuchinFalls_EntranceEvents) do, from their functions' comments:
/// walking into a log's trigger cell pushes it along the water, and
/// stepping on it plays the log's rolling.
pub(super) const FUCHIN_FIRST_ROOM_WORDS: Words = &[
    ("FuchinFallsCave_Entrance_MoveFirstLogEast", "Push the first log east (walk east into it)", None),
    ("FuchinFallsCave_Entrance_MoveFirstLogWest", "Push the first log back west (walk west into it)", None),
    ("FuchinFallsCave_Entrance_MoveSecondLogSouth", "Push the second log back south (walk south into it)", None),
    ("FuchinFallsCave_Entrance_MoveSecondLogNorth", "Push the second log north (walk north into it)", None),
    ("FuchinFallsCave_Entrance_MoveThirdLogSouth", "Push the third log back south (walk south into it)", None),
    ("FuchinFallsCave_Entrance_MoveThirdLogNorth", "Push the third log north (walk north into it)", None),
    ("FuchinFallsCave_AnimateFirstLog", "the log rolls when stepped on", None),
    ("FuchinFallsCave_AnimateSecondLog", "the log rolls when stepped on", None),
    ("FuchinFallsCave_AnimateThirdLog", "the log rolls when stepped on", None),
    ("FUCHIN_FALLS_CAVE_ENTRANCE_TO_FUCHIN_TEMPLE_GROUNDS", "Way out to Fuchin Temple", None),
    ("FUCHIN_FALLS_CAVE_ENTRANCE_TO_FUCHIN_FALLS_CAVE_ZEPHYR", "Way out to Zephyr's room", None),
    ("FUCHIN_FALLS_CAVE_ENTRANCE_TO_FUCHIN_FALLS_CAVE_ARCTIC_BLADE", "Way out to the Arctic Blade's room", None),
    ("FUCHIN_FALLS_CAVE_ENTRANCE_SOUTH_HALF_TO_NORTH_HALF", "Way to the cave's northern part", None),
    ("FUCHIN_FALLS_CAVE_ENTRANCE_NORTH_HALF_TO_SOUTH_HALF", "Way to the cave's southern part", None),
    ("FuchinFallsCave_ReadDragonFlameInscription", "Search: an inscription", None),
    ("FIND_MIMIC", "Chest: a mimic", None),
];

/// What the rows of Mercury Lighthouse's ground floor
/// (SceneId_MercuryLighthouseEntrance) do around the pool, from their
/// functions' comments; the exits' rooms are named in map_warps.h.
pub(super) const MERCURY_LIGHTHOUSE_ENTRANCE_WORDS: Words = &[
    ("MercuryLighthouseEntrance_HopAcrossPool", "Hop two cells (walk into it)", None),
    ("MercuryLighthouseEntrance_LoseLights", "takes the leader's lights", None),
    ("MercuryLighthouseEntrance_RippleAroundLeader", "ripples spread from the leader", None),
    ("MercuryLighthouseEntrance_ChargeLights", "gives the leader lights, up to three", None),
    ("MercuryLighthouseEntrance_ReadHeartInscription", "Search: the inscription", None),
    ("MercuryLighthouseEntrance_ReturnToDoorway", "Back to the doorway", None),
    ("MERCURY_LIGHTHOUSE_ENTRANCE_GODDESS_TO_TWIN_FOUNTAINS_WEST", "Way out to the room of two pools", None),
    ("MERCURY_LIGHTHOUSE_ENTRANCE_GODDESS_TO_TWIN_FOUNTAINS_EAST", "Way out to the room of two pools", None),
    ("MERCURY_LIGHTHOUSE_ENTRANCE_GODDESS_TO_SIX_STATUES", "Way out to the hall of six statues", None),
];

/// What the rows of the shifting floor stage of Colosseum Final 2
/// (SceneId_ColosseumSatrageMatch) do where its lifts' task copies cells.
pub(super) const SATRAGE_WORDS: Words = &[];

/// Vale by day, on the day the story begins (SceneId_Vale, entered by the
/// Sanctum's door).
const VALE: (u16, u16) = (4, 1);
/// The scenes Vale's doors lead into: SceneId_CottageRoomsA and
/// SceneId_CottageRoomsB.
const COTTAGE_ROOMS_A: u16 = 7;
const COTTAGE_ROOMS_B: u16 = 8;

/// What the rows of Vale's event table do, from their functions'
/// comments, the scenes their exits lead to, and the event renders.
const VALE_WORDS: Words = &[
    ("EventScript_EnterSanctum", "Door into the Sanctum", Some(9)),
    ("EventScript_ApproachMtAleph", "Path up Mt. Aleph", None),
    ("EventScript_WarpToValeKradensCottageNorth", "Way out to Kraden's cottage, north", Some(6)),
    ("EventScript_WarpToValeKradensCottageSouth", "Way out to Kraden's cottage, south", Some(6)),
    ("EventScript_WarpToValePlaza", "Way out to the Psynergy Stone", Some(5)),
    ("EventScript_EnterGaretsHouse", "Door into Garet's house (walk north into it)", Some(COTTAGE_ROOMS_A)),
    ("EventScript_EnterCottageRoomsA", "Door into a house (walk north into it)", Some(COTTAGE_ROOMS_A)),
    ("EventScript_EnterIsaacsHouse", "Door into Isaac's house (walk north into it)", Some(COTTAGE_ROOMS_B)),
    ("EventScript_EnterJennasGrandparentsHouse", "Door into the house of Jenna's grandparents (walk north into it)", Some(COTTAGE_ROOMS_A)),
    ("EventScript_EnterCottageRoomsAWest", "Door into a house (walk north into it)", Some(COTTAGE_ROOMS_A)),
    ("EventScript_StepOntoBridge", "Onto the bridge: its cells change", None),
    ("EventScript_StepOffBridge", "Off the bridge: its cells change back", None),
    ("EventScript_WatchThePuppies", "The puppies bark", None),
    ("EventScript_TurnBackFromThePuppies", "The puppies bark", None),
    ("EventScript_ClimbLadderOntoRoof", "Ladder up onto the roof of Isaac's house (walk north into it)", None),
    ("EventScript_ClimbDownFromRoof", "Ladder down from the roof (walk south into it)", None),
    ("EventScript_PushPillar", "Push the stone pillar", None),
    ("EventScript_SearchTheLockedDoor", "Search the locked door", None),
];

impl<'a> Guide<'a> {
    /// The screen's worth of Vale by day with the most of the scene's own
    /// cells, on the day the story begins, drawn from the map with the
    /// leader hidden at twice the GBA's size: every cell whose collision
    /// code is one of the scene's own (1 to 239), drawn its ground's height
    /// up and tinted by what answers it (an exit row blue, a touch row
    /// magenta, a search row cyan, none grey), joined by a line to a key
    /// beside the picture that says in words what each does, each door
    /// checked by walking into it, as `scene-event-cells.png`.
    pub fn scene_event_cells(&mut self) -> Result<(), String> {
        let clear = vec![self.flag("FLAG_VALE_STORM")?, self.flag("FLAG_CHILDHOOD")?, self.flag("FLAG_VALE_PARTY_LEFT")?];
        let setup = Setup { scene: VALE.0, entrance: VALE.1, set: Vec::new(), clear };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        let placed = field.save();
        field.until_control()?;
        field.frames(20)?;
        let names = self.cell_names("Vale_GetEvents", "Vale")?;
        let settled = field.save();
        // The scene's own cells, with their layers, and the screen's worth of
        // the room that holds the most of them.
        let cells: Vec<(u8, i32, i32)> = field.cells_by_code()?.values().flatten().copied().collect();
        let r = field.room();
        let mut best = (0, (r[0] / 16, r[1] / 16));
        for cz in (r[1] / 16..r[3] / 16 - 9).step_by(2) {
            for cx in (r[0] / 16..r[2] / 16 - 14).step_by(2) {
                let n = cells.iter().filter(|&&(_, x, z)| (cx..cx + 15).contains(&x) && (cz..cz + 10).contains(&z)).count();
                if n > best.0 {
                    best = (n, (cx, cz));
                }
            }
        }
        let (map, origin) = room_map(&mut field, &settled)?;
        field.load(&settled);
        let (vx, vy) = (best.1 .0 * 16, best.1 .1 * 16);
        let part = crop(&map, vx - origin.0, vy - origin.1, WIDTH, HEIGHT);
        // Each code on screen: its meaning and its cells' places on the
        // picture.
        let mut groups: Vec<(i32, Meaning, Vec<(i32, i32)>)> = Vec::new();
        for &(layer, cx, cz) in &cells {
            let code = field.cell(layer, cx, cz);
            let lift = field.ground(layer, cx * 16 + 8, cz * 16 + 8)? >> 16;
            let (sx, sy) = (2 * (cx * 16 - vx), 2 * (cz * 16 - vy - lift));
            if sx < 0 || sy < 0 || sx + 32 > 2 * WIDTH as i32 || sy + 32 > 2 * HEIGHT as i32 {
                continue;
            }
            match groups.iter_mut().find(|g| g.0 == code) {
                Some(g) => g.2.push((sx, sy)),
                None => {
                    let m = meaning(&field, &names, VALE_WORDS, code)?.ok_or("an event cell of code 0")?;
                    groups.push((code, m, vec![(sx, sy)]));
                }
            }
        }
        let codes: Vec<i32> = groups.iter().map(|g| g.0).collect();
        Self::check_doors(&mut field, &placed, &names, VALE_WORDS, &codes)?;
        let mut picture = canvas(2 * WIDTH, 2 * HEIGHT, 0);
        paste(&mut picture, &part, 0, 0, 2);
        let mut marks: Vec<Mark> = Vec::new();
        let mut kinds: Vec<Kind> = Vec::new();
        for (_, m, places) in &groups {
            for &(sx, sy) in places {
                tint_cell(&mut picture, sx, sy, 32, m.kind.colour(), 0.55);
                outline(&mut picture, sx + 1, sy + 1, 30, 30, m.kind.colour());
            }
            let right = *places.iter().max_by_key(|p| (p.0, p.1)).ok_or("a group with no cells")?;
            marks.push((m.kind.colour(), m.words.clone(), (right.0 + 31, right.1 + 16)));
            if !kinds.contains(&m.kind) {
                kinds.push(m.kind);
            }
        }
        let legend: Vec<(u32, String)> = kinds.iter().map(|k| (k.colour(), k.key().to_string())).collect();
        let image = labelled(&picture, &marks, &legend, 216);
        self.guide_png("scene-event-cells.png", &image, "Vale's event cells, each joined to words saying what it does, coloured by the kind of row that answers it")
    }
}

#[cfg(test)]
mod tests {
    use super::warps_block;

    #[test]
    fn warps_are_found_under_either_heading() {
        let links = "/// SceneId_Title's warp destinations and warps.\nenum {\n  TITLE_FROM_VALE_PLAZA = 1,\n};\n\n/// SceneId_GomaCavePillar's warp destinations.\nenum {\n  GOMA_CAVE_PILLAR_FROM_GOMA_CAVE_STREAM_WEST = 1,\n};\n\n/// SceneId_GomaCavePillar's warps.\nenum {\n  GOMA_CAVE_PILLAR_TO_GOMA_CAVE_STREAM_WEST = 1,\n};\n";
        assert!(warps_block(links, "GomaCavePillar").unwrap().contains("GOMA_CAVE_PILLAR_TO_GOMA_CAVE_STREAM_WEST = 1"));
        assert!(!warps_block(links, "GomaCavePillar").unwrap().contains("_FROM_"));
        assert!(warps_block(links, "Title").unwrap().contains("TITLE_FROM_VALE_PLAZA"));
        assert!(warps_block(links, "Vale").is_err());
    }
}
