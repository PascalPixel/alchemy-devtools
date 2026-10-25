//! `render guide`: pictures for the folder guides, each showing one idea the
//! guide explains, played from a built edition with the pad held as a player
//! would: the leader walking and then running, a map built up layer by
//! layer, footprints in the snow, and the world map's perspective view.

use super::animation;
use super::field::{box_holds, Field, Saved, Setup, Spot, Start, Symbols};
use super::mgba::{HEIGHT, WIDTH};
use super::source;
use super::story::{Flags, Trigger};
use std::path::Path;

/// KEYINPUT bits.
const KEY_A: u32 = 0x1;
const KEY_B: u32 = 0x2;
const KEY_START: u32 = 0x8;
const KEY_RIGHT: u32 = 0x10;
const KEY_LEFT: u32 = 0x20;
const KEY_UP: u32 = 0x40;
const KEY_DOWN: u32 = 0x80;

/// DISPCNT and its layer bits: 8 to 11 turn BG0 to BG3 on, 12 the sprites.
const DISPCNT: u32 = 0x0400_0000;
const LAYER_BITS: u16 = 0x1f00;

/// Vale's part with the Vale Sanctum, entered by day (scene 4, entrance 1),
/// and Imil (scene 50, entrance 0).
const VALE: (u16, u16) = (4, 1);
const IMIL: (u16, u16) = (50, 0);
/// The world map as the party first leaves Vale (scene 2, entrance 1).
const WORLD_MAP: (u16, u16) = (2, 1);
/// The title overlay as the game starts it at power-on (scene 0, entrance 2).
const POWER_ON: (u16, u16) = (0, 2);
/// Frames from the first one not black to the Golden Sun logo held.
const BOOT_FRAMES: usize = 420;

pub struct Guide<'a> {
    pub rom: &'a [u8],
    pub symbols: &'a Symbols,
    pub flags: &'a Flags,
    pub out: &'a Path,
    /// The edition's linker map, for the addresses a picture reads.
    pub map: &'a crate::ld_map::Map,
    pub written: Vec<(String, u64, String)>,
    /// Rows of the abilities' index.tsv for the field casts recorded.
    pub casts: Vec<String>,
}

impl<'a> Guide<'a> {
    pub(super) fn flag(&self, name: &str) -> Result<u16, String> {
        self.flags.resolve(name, None).ok_or_else(|| format!("flags.tsv names no {name}"))
    }

    fn gif(&mut self, file: &str, frames: &[Vec<u16>], what: &str) -> Result<(), String> {
        let rgb: Vec<Vec<u32>> = frames.iter().map(|f| f.iter().map(|&c| animation::rgb(c)).collect()).collect();
        let written = animation::write(&self.out.join(file), &rgb, WIDTH)?;
        self.written.push((file.to_string(), written.bytes, format!("{what}; {} frames at 30 a second", written.frames)));
        Ok(())
    }

    /// Enters a scene after the party has left Vale, with the party the
    /// story has there (party.rs), and runs until the player has control.
    pub(super) fn enter(&self, (scene, entrance): (u16, u16)) -> Result<Field<'a>, String> {
        let set = vec![self.flag("FLAG_VALE_PARTY_LEFT")?];
        let clear = vec![self.flag("FLAG_VALE_STORM")?, self.flag("FLAG_CHILDHOOD")?];
        let setup = Setup { scene, entrance, set, clear };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        field.until_control()?;
        field.frames(20)?;
        Ok(field)
    }

    /// Holds `keys` for `frames` frames, keeping each picture.
    pub(super) fn hold(field: &mut Field, keys: u32, frames: u32, out: &mut Vec<Vec<u16>>) -> Result<(), String> {
        field.hold(keys);
        for _ in 0..frames {
            field.frames(1)?;
            out.push(field.picture());
        }
        field.hold(0);
        Ok(())
    }

    /// Holds each step's keys in turn from where `place` starts, as `file`.
    fn walk_path(&mut self, place: (u16, u16), path: &[(u32, u32)], file: &str, what: &str) -> Result<(), String> {
        let mut field = self.enter(place)?;
        let mut frames = vec![field.picture()];
        for &(keys, n) in path {
            Self::hold(&mut field, keys, n, &mut frames)?;
        }
        self.gif(file, &frames, what)
    }

    /// In Vale, walking right at 1 pixel a frame, then running back with B
    /// held at 1.5 (Object_UpdateLeaderControl).
    pub fn walk(&mut self) -> Result<(), String> {
        let path = [(KEY_RIGHT, 60), (0, 15), (KEY_LEFT | KEY_B, 60), (0, 20)];
        self.walk_path(VALE, &path, "walk-run.gif", "walking right, then running back")
    }

    /// Imil's snow, where the leader's footprints stay a moment, each a
    /// field object of its own: west across the open snow below the houses
    /// and then south, clear of the trees, so the trail stays in view.
    pub fn snow(&mut self) -> Result<(), String> {
        let path = [(KEY_LEFT, 45), (KEY_DOWN, 45), (0, 70)];
        self.walk_path(IMIL, &path, "snow.gif", "walking through Imil's snow")
    }

    /// The field menu: opened with A in Vale, moved along its icons and
    /// into the Status screen and back out, as `menus.gif`.
    pub fn menus(&mut self) -> Result<(), String> {
        let path = [
            (0, 20),
            (KEY_A, 4),
            (0, 50),
            (KEY_RIGHT, 4),
            (0, 30),
            (KEY_RIGHT, 4),
            (0, 30),
            (KEY_A, 4),
            (0, 40),
            (KEY_A, 4),
            (0, 150),
            (KEY_B, 4),
            (0, 40),
            (KEY_B, 4),
            (0, 40),
        ];
        self.walk_path(VALE, &path, "menus.gif", "the field menu, its icons and the Status screen")
    }

    /// The Item screen: Isaac's items, one chosen, and the status panel
    /// showing his stats with it equipped, as `items.gif`.
    pub fn items(&mut self) -> Result<(), String> {
        let path = [(0, 20), (KEY_A, 4), (0, 40), (KEY_RIGHT, 4), (0, 30), (KEY_A, 4), (0, 40), (KEY_A, 4), (0, 40), (KEY_RIGHT, 4), (0, 30), (KEY_A, 4), (0, 60), (KEY_DOWN, 4), (0, 60)];
        self.walk_path(VALE, &path, "items.gif", "the Item screen and an item's stats")
    }

    /// The game from power-on with no key pressed and no save file, from the
    /// Camelot logo to the Golden Sun logo, which a new game follows with
    /// name entry, as `boot.gif`. BOOT_FRAMES ends it on the logo.
    pub fn boot(&mut self) -> Result<(), String> {
        let setup = Setup { scene: POWER_ON.0, entrance: POWER_ON.1, set: Vec::new(), clear: Vec::new() };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        let mut frames = Vec::new();
        Self::hold(&mut field, 0, 900, &mut frames)?;
        let first = frames.iter().position(|f| f.iter().any(|&c| c & 0x7fff != 0)).unwrap_or(0);
        let end = (first + BOOT_FRAMES).min(frames.len());
        // The Camelot logo is the first picture after the black that follows
        // the Nintendo logo.
        let lit = |f: &Vec<u16>| f.iter().any(|&c| c & 0x7fff != 0);
        let black = first + frames[first..].iter().position(|f| !lit(f)).ok_or("power-on never turns black")?;
        let camelot = black + frames[black..].iter().position(lit).ok_or("no picture after the black")?;
        self.gif("boot.gif", &frames[camelot..end], "the game at power-on: the Camelot and Golden Sun logos")
    }

    /// Start pressed two screens deep: every screen closes at once.
    pub fn menu_start(&mut self) -> Result<(), String> {
        let path = [(0, 20), (KEY_A, 4), (0, 50), (KEY_A, 4), (0, 50), (KEY_A, 4), (0, 60), (KEY_START, 4), (0, 60)];
        self.walk_path(VALE, &path, "menu-start.gif", "Start closing the Psynergy screen and the field menu at once")
    }

    /// The world map's perspective view as the party walks and runs.
    pub fn world_map(&mut self) -> Result<(), String> {
        let path = [(KEY_DOWN, 50), (KEY_RIGHT | KEY_B, 60), (KEY_UP, 50)];
        self.walk_path(WORLD_MAP, &path, "world-map-walk.gif", "walking and running on the world map")
    }

    /// Vale built up layer by layer: BG3, then BG2 over it, then the
    /// sprites, then BG1 over them. The field writes DISPCNT only as a scene
    /// fades in, so a store into it holds until it is put back.
    pub fn layers(&mut self) -> Result<(), String> {
        let mut field = self.enter(VALE)?;
        let shown = field.read16(DISPCNT);
        let mut frames = Vec::new();
        for (bits, n) in [(0x800u16, 40u32), (0xc00, 40), (0x1c00, 40), (0x1e00, 70)] {
            field.io_write16(DISPCNT, (shown & !LAYER_BITS) | bits);
            for _ in 0..n {
                field.frames(1)?;
                frames.push(field.picture());
            }
        }
        field.io_write16(DISPCNT, shown);
        self.gif("layers.gif", &frames, "BG3, then BG2, the sprites and BG1 added over it")
    }

    /// Vale's layers pulled apart in 3D: the scene as the game shows it,
    /// then BG3, BG2, the sprites and BG1 drawn as sheets, turned and spread
    /// one behind the other, then flat again. Each sheet is captured alone
    /// over KEY, the colour that marks its empty pixels.
    pub fn exploded(&mut self) -> Result<(), String> {
        let mut field = self.enter(VALE)?;
        self.planes_now(&mut field, "planes/field", "Vale")?;
        let shown = field.read16(DISPCNT);
        let saved = field.save();
        let mut sheets = Vec::new();
        for bits in [0x800u16, 0x400, 0x1000, 0x200] {
            field.load(&saved);
            field.io_write16(DISPCNT, (shown & !LAYER_BITS) | bits);
            field.set_backdrop(KEY);
            field.frames(2)?;
            sheets.push(field.picture().iter().map(|&c| (c != KEY).then(|| animation::rgb(c))).collect::<Vec<_>>());
        }
        // Frames at 60 a second: flat, apart, held, together, flat.
        let mut spread = Vec::new();
        spread.extend(std::iter::repeat_n(0.0, 50));
        spread.extend((0..60).map(|i| ease(i as f64 / 59.0)));
        spread.extend(std::iter::repeat_n(1.0, 120));
        spread.extend((0..60).map(|i| ease(1.0 - i as f64 / 59.0)));
        spread.extend(std::iter::repeat_n(0.0, 20));
        let frames: Vec<Vec<u32>> = spread.iter().map(|&e| draw_sheets(&sheets, e)).collect();
        let written = animation::write(&self.out.join("layers-3d.gif"), &frames, CANVAS.0)?;
        self.written.push(("layers-3d.gif".to_string(), written.bytes, format!("Vale's layers pulled apart; {} frames", written.frames)));
        Ok(())
    }
}

/// The colour an empty pixel of a sheet is drawn in: full magenta, which no
/// map tile here uses.
pub(super) const KEY: u16 = 0x7c1f;
/// The exploded view's picture, its background and the sheets' tint and edge.
pub(super) const CANVAS: (usize, usize) = (520, 400);
const BACKGROUND: u32 = 0x10_10_14;
const EDGE: u32 = 0xb0_b8_c8;
const SHEET: u32 = 0x30_36_44;
/// How far apart the sheets end (pixels of a sheet), and the turn and tilt
/// they end at.
const GAP: f64 = 70.0;
const YAW: f64 = -38.0;
const PITCH: f64 = 14.0;
/// The camera's distance and the picture's scale at the sheets' plane.
const DISTANCE: f64 = 700.0;
const SCALE: f64 = 1.5;

pub(super) fn ease(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

fn mix(a: u32, b: u32, share: f64) -> u32 {
    let channel = |s: u32| {
        let (x, y) = (f64::from((a >> s) & 0xff), f64::from((b >> s) & 0xff));
        ((x + (y - x) * share).round() as u32) << s
    };
    channel(16) | channel(8) | channel(0)
}

/// Rotates `p` by the turn about the vertical axis, then the tilt about the
/// horizontal one.
fn rotate(p: [f64; 3], yaw: f64, pitch: f64) -> [f64; 3] {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    let x = p[0] * cy + p[2] * sy;
    let z = -p[0] * sy + p[2] * cy;
    [x, p[1] * cp - z * sp, p[1] * sp + z * cp]
}

fn det(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    a[0] * (b[1] * c[2] - b[2] * c[1]) - b[0] * (a[1] * c[2] - a[2] * c[1]) + c[0] * (a[1] * b[2] - a[2] * b[1])
}

/// One frame of the exploded view with the sheets `e` of the way apart
/// (0 flat, 1 apart), the first sheet at the back, drawn back to front.
pub(super) fn draw_sheets(sheets: &[Vec<Option<u32>>], e: f64) -> Vec<u32> {
    let (w, h) = CANVAS;
    let mut canvas = vec![BACKGROUND; w * h];
    let (yaw, pitch) = ((YAW * e).to_radians(), (PITCH * e).to_radians());
    let focal = DISTANCE * SCALE;
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    let n = sheets.len() as f64;
    let eu = rotate([1.0, 0.0, 0.0], yaw, pitch);
    let ev = rotate([0.0, 1.0, 0.0], yaw, pitch);
    for (i, sheet) in sheets.iter().enumerate() {
        let depth = ((n - 1.0) / 2.0 - i as f64) * GAP * e;
        let p0 = rotate([-(WIDTH as f64) / 2.0, -(HEIGHT as f64) / 2.0, depth], yaw, pitch);
        // Camera at (0, 0, -DISTANCE): solve camera + t ray = p0 + a eu + b ev.
        let rhs = [-p0[0], -p0[1], -DISTANCE - p0[2]];
        for y in 0..h {
            for x in 0..w {
                let ray = [(x as f64 + 0.5 - cx) / focal, (y as f64 + 0.5 - cy) / focal, 1.0];
                let back = [-ray[0], -ray[1], -ray[2]];
                let d = det(eu, ev, back);
                if d.abs() < 1e-9 {
                    continue;
                }
                let a = det(rhs, ev, back) / d;
                let b = det(eu, rhs, back) / d;
                if !(0.0..WIDTH as f64).contains(&a) || !(0.0..HEIGHT as f64).contains(&b) {
                    continue;
                }
                let at = &mut canvas[y * w + x];
                let near_edge = a < 1.0 || b < 1.0 || a > WIDTH as f64 - 1.0 || b > HEIGHT as f64 - 1.0;
                if near_edge && e > 0.02 {
                    *at = mix(*at, EDGE, e);
                    continue;
                }
                match sheet[b as usize * WIDTH + a as usize] {
                    Some(c) => *at = c,
                    None => *at = mix(*at, SHEET, 0.45 * e),
                }
            }
        }
    }
    canvas
}

/// A field Psynergy recorded for the abilities' index: the ability
/// (ability_ids.h), the map and warp destination it is cast in, what it is
/// cast at (an object event's local id, or none for one cast on the area)
/// and in this renderer's words, where the leader stands for an area cast
/// (pixels, facing), and the frames kept after the player has control
/// again.
struct FieldCast {
    ability: &'static str,
    map: &'static str,
    warp_dest: u16,
    actor: Option<&'static str>,
    target: &'static str,
    stand: Option<(i32, i32, u32)>,
    tail: u32,
}

/// Facings, 1/65536 turns.
const FACE_NORTH: u32 = 0xc000;
const FACE_EAST: u32 = 0x0000;

/// Where each field Psynergy that had no recording is cast: on the thing
/// its map's event answers, or in the room whose event it changes.
/// A Psynergy may be listed more than once, at places tried in turn.
const FIELD_CASTS: &[FieldCast] = &[
    FieldCast { ability: "ABILITY_REVEAL", map: "ValeCaveKite", warp_dest: 1, actor: None, target: "the hidden floor", stand: Some((248, 280, FACE_NORTH)), tail: 600 },
    FieldCast { ability: "ABILITY_MOVE", map: "Vale", warp_dest: 1, actor: Some("LOCALID_PILLAR"), target: "a stone pillar", stand: None, tail: 40 },
    FieldCast { ability: "ABILITY_AVOID", map: "GomaCavePillar", warp_dest: 5, actor: None, target: "the party", stand: None, tail: 30 },
    FieldCast { ability: "ABILITY_MIND_READ", map: "Vale", warp_dest: 1, actor: Some("LOCALID_GARETS_SISTER"), target: "Garet's sister", stand: None, tail: 20 },
    FieldCast { ability: "ABILITY_CATCH", map: "Vale", warp_dest: 1, actor: Some("LOCALID_NUT"), target: "a Nut", stand: None, tail: 30 },
    FieldCast { ability: "ABILITY_RETREAT", map: "GomaCavePillar", warp_dest: 5, actor: None, target: "the party", stand: None, tail: 240 },
    FieldCast { ability: "ABILITY_FORCE", map: "Xian", warp_dest: 0, actor: Some("LOCALID_FORCE_TARGET"), target: "the training tree", stand: None, tail: 30 },
    FieldCast { ability: "ABILITY_LIFT", map: "GondowanCave", warp_dest: 0, actor: Some("LOCALID_BOULDER"), target: "a boulder", stand: None, tail: 30 },
    FieldCast { ability: "ABILITY_CARRY", map: "TunnelRuinsLadyStatues", warp_dest: 0, actor: Some("LOCALID_MIDDLE_CARRY_STONE"), target: "a stone block", stand: None, tail: 60 },
    FieldCast { ability: "ABILITY_CLOAK", map: "LunpaFortressGates", warp_dest: 0, actor: None, target: "the party", stand: None, tail: 120 },
    FieldCast { ability: "ABILITY_HALT", map: "ValeCaveKite", warp_dest: 5, actor: Some("LOCALID_KITE"), target: "Kite", stand: Some((792, 256, FACE_NORTH)), tail: 60 },
];

/// A field cast read through the source: the ability's number and name
/// (gAbilityDefinitions), the map's number, its area name there (scene), and the
/// object event's local id.
struct Cast {
    id: u16,
    name: String,
    place: (u16, u16),
    scene: String,
    actor: Option<u16>,
    target: &'static str,
    stand: Option<(i32, i32, u32)>,
    tail: u32,
}

fn field_casts() -> Result<Vec<Cast>, String> {
    FIELD_CASTS
        .iter()
        .map(|cast| {
            let id = source::ability_id(cast.ability)?;
            let map = source::map_id(cast.map)?;
            Ok(Cast {
                id,
                name: source::ability_name(id)?,
                place: (map, cast.warp_dest),
                scene: source::area_name(map, cast.warp_dest)?,
                actor: cast.actor.map(|name| source::local_id(cast.map, name)).transpose()?,
                target: cast.target,
                stand: cast.stand,
                tail: cast.tail,
            })
        })
        .collect()
}

/// The longest a field cast is recorded, at 60 frames a second.
const CAST_FRAMES: usize = 1800;

/// A field scene entered for a recording: the field with the player in
/// control, and the moment Map_ApplyWarpDest placed the party, from which
/// an event starts with the leader already at its spot and the camera on
/// him.
pub struct Entered<'a> {
    pub field: Field<'a>,
    pub placed: Saved,
}

impl<'a> Guide<'a> {
    /// Enters a scene as `enter` does, with `set` set as well, keeping the
    /// moment the party was placed.
    pub(super) fn enter_with(&self, place: (u16, u16), set: &[&str]) -> Result<Entered<'a>, String> {
        let set = set.iter().map(|name| self.flag(name)).collect::<Result<Vec<u16>, String>>()?;
        self.enter_set(place, set)
    }

    /// Enters a scene as the story has it when the party first reaches scene
    /// `of` (the place's first room, for any room of it): with every story
    /// flag set before then (Flags::before_first_visit), the chapter's party
    /// (party.rs), and the prologue over.
    pub(super) fn enter_first_visit(&self, place: (u16, u16), of: u16) -> Result<Entered<'a>, String> {
        self.enter_set(place, self.flags.before_first_visit(of)?)
    }

    /// Enters a scene as `enter` does, with the flags `set` set as well.
    fn enter_set(&self, place: (u16, u16), set: Vec<u16>) -> Result<Entered<'a>, String> {
        let mut flags = vec![self.flag("FLAG_VALE_PARTY_LEFT")?];
        for f in set {
            if !flags.contains(&f) {
                flags.push(f);
            }
        }
        // Set first, then cleared: the night's storm and the childhood
        // sprites are over, whatever a beat before left set.
        let clear = vec![self.flag("FLAG_VALE_STORM")?, self.flag("FLAG_CHILDHOOD")?];
        let setup = Setup { scene: place.0, entrance: place.1, set: flags, clear };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        let placed = field.save();
        field.until_control()?;
        field.frames(20)?;
        Ok(Entered { field, placed })
    }

    /// Plays `trigger` with the leader at `spot`, or else at each place
    /// Field::spots gives in turn until one shows something, and keeps every
    /// frame from just before it to the player's control again, then `tail`
    /// frames more.
    pub(super) fn record_trigger(entered: &mut Entered, trigger: Trigger, spot: Option<Spot>, tail: u32) -> Result<Vec<Vec<u16>>, String> {
        let field = &mut entered.field;
        let spots = match spot {
            Some(s) => vec![s],
            None => field.spots(trigger)?,
        };
        let mut last = String::from("nowhere to stand");
        for s in spots.iter().take(12) {
            match field.event(Start::Entered(&entered.placed), trigger, s, &[], &[], 16, Some(CAST_FRAMES)) {
                Ok(run) if run.shown || spot.is_some() => {
                    let mut frames = run.frames;
                    for _ in 0..tail {
                        field.frames(1)?;
                        frames.push(field.picture());
                    }
                    return Ok(frames);
                }
                Ok(_) => last = format!("from ({}, {}) it showed nothing", s.x, s.z),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// A spot at (x, z) pixels on the leader's layer, on the ground there.
    pub(super) fn spot_at(field: &mut Field, (x, z, facing): (i32, i32, u32)) -> Result<Spot, String> {
        let layer = field.leader().ok_or("the scene has no leader")?.layer.min(2);
        let height = field.ground(layer, x, z)?;
        Ok(Spot { x, z, layer, height, facing, walk: 0, cell: None })
    }

    /// Records one field cast: from its entrance, and for a cast on an
    /// actor the leader cannot reach from there, from each entrance whose
    /// room holds the actor.
    fn record_cast(&self, cast: &Cast) -> Result<Vec<Vec<u16>>, String> {
        let mut entered = self.enter_with(cast.place, &[])?;
        let trigger = Trigger::Psynergy(cast.id, cast.actor.unwrap_or(0));
        if let (Some(_), Some(stand)) = (cast.actor, cast.stand) {
            let spot = Self::spot_at(&mut entered.field, stand)?;
            return Self::record_trigger(&mut entered, trigger, Some(spot), cast.tail);
        }
        let Some(id) = cast.actor else {
            let stand = match cast.stand {
                Some(stand) => stand,
                None => {
                    let leader = entered.field.leader().ok_or("the scene has no leader")?;
                    (leader.x, leader.z, FACE_EAST)
                }
            };
            let spot = Self::spot_at(&mut entered.field, stand)?;
            return Self::record_trigger(&mut entered, trigger, Some(spot), cast.tail);
        };
        let first = match Self::record_trigger(&mut entered, trigger, None, cast.tail) {
            Ok(frames) => return Ok(frames),
            Err(e) => e,
        };
        let actor = entered.field.actor(id).ok_or(first.clone())?;
        let header = entered.field.header();
        let others: Vec<u16> = entered
            .field
            .entrances()
            .iter()
            .filter(|e| e.number as u16 != cast.place.1 && box_holds(e.camera, header, actor.x, actor.z))
            .map(|e| e.number as u16)
            .collect();
        for entrance in others {
            let Ok(mut entered) = self.enter_with((cast.place.0, entrance), &[]) else {
                continue;
            };
            if let Ok(frames) = Self::record_trigger(&mut entered, trigger, None, cast.tail) {
                return Ok(frames);
            }
        }
        Err(first)
    }

    /// The field Psynergy that had no recording, each cast where its scene
    /// answers it, as `<name>.gif` with a row of `index.tsv`.
    pub fn field_psynergy(&mut self, only: &[String]) -> Result<(), String> {
        let mut failed = Vec::new();
        let mut done: Vec<String> = Vec::new();
        let casts = field_casts()?;
        for (i, cast) in casts.iter().enumerate() {
            if done.contains(&cast.name) || (!only.is_empty() && !only.iter().any(|o| o.eq_ignore_ascii_case(&cast.name))) {
                continue;
            }
            let last = !casts[i + 1..].iter().any(|c| c.name == cast.name);
            let file = format!("{}.gif", super::plan::slug(&cast.name));
            match self.record_cast(cast) {
                Ok(frames) => {
                    let rgb: Vec<Vec<u32>> = frames.iter().map(|f| f.iter().map(|&c| animation::rgb(c)).collect()).collect();
                    let written = animation::write(&self.out.join(&file), &rgb, WIDTH)?;
                    self.written.push((file.clone(), written.bytes, format!("{} cast at {} in {}", cast.name, cast.target, cast.scene)));
                    self.casts.push(format!("{}\t{}\tIsaac\t{}\t{}\t{file}\t{}", cast.id, cast.name, cast.target, written.frames, cast.scene));
                    done.push(cast.name.clone());
                }
                Err(e) => {
                    eprintln!("{} in {}: {e}", cast.name, cast.scene);
                    if last {
                        failed.push(cast.name.clone());
                    }
                }
            }
        }
        if failed.is_empty() { Ok(()) } else { Err(format!("{} failed", failed.join(", "))) }
    }
}


/// The field Psynergy `field-psynergy` casts, each once, in its order.
pub fn field_cast_names() -> Result<Vec<String>, String> {
    let mut names: Vec<String> = Vec::new();
    for cast in field_casts()? {
        if !names.contains(&cast.name) {
            names.push(cast.name);
        }
    }
    Ok(names)
}

/// Every mode, in the order `render guide` plays them.
pub const MODES: &[&str] = &[
    "walk", "snow", "layers", "layers-3d", "battle-layers-3d", "world-map-layers-3d", "menu-layers-3d", "status-layers-3d", "talk-layers-3d", "shop-layers-3d", "scene-actors", "debug-rooms", "debug-room-talks", "moment", "world-map", "menus", "menu-start", "boot", "items", "field-psynergy", "talk", "djinn-room", "shop",
    "inn", "menu-psynergy", "credits", "sleep", "oam", "vram", "cells", "grid", "random-battle", "world-map-picture", "overlay",
    "tret-tree-branch-split",
    "kolima-forest-tree-healing",
    "altmiller-cave-darkness",
    "bilibin-cave-third-puddle",
    "fuchin-falls-cave-logs",
    "mercury-lighthouse-pool",
    "lunpa-fortress-patrol",
    "tolbi-spring-toss",
    "kolima-forest-entrance-fields",
    "tret-tree-interiors-cocoons",
    "colosseum-azart-race",
    "colosseum-navampa-scales",
    "colosseum-satrage-lifts",
    "tunnel-ruins-balance",
    "venus-lighthouse-aerie-eruption",
    "scene-event-cells",
    "credits-tile-ring",
    "title-intro-layers-3d",
    "item-screen-windows",
    "sound-players",
    "field-requests",
    "grid-planes",
    "tile-animation",
    "world-map-scanlines",
    "world-map-footprint",
    "round-events",
    "battle-places",
    "effect-canvas",
    "effect-transfer",
    "effect-particles",
    "sprite-layers",
    "shop-counters",
    "sanctum-account",
    "star-room-rising",
    "eruption-departures",
    "statue-sky-rows",
    "vale-opening",
    "vale-gate-display",
    "vale-interiors-sleep",
    "vale-kraden-rockslide",
    "vale-cave-kite",
    "work-block",
    "text-ring",
    "render-outputs",
    "area-name",
    "sanctum",
    "item-picker",
    "menu-slot",
    "enemy-actions",
    "first-strikes",
    "element-multiplier",
    "plan-rolls",
    "staging",
    "added-effects",
    "after-target",
    "effect-variants",
    "djinn-attacks",
    "spark-groups",
    "stat-reductions",
    "target-strikes",
    "psynergy-steps",
    "djinn-preview",
    "status-pages",
    "status-panels",
    "item-sorting",
];

impl Guide<'_> {
    /// Plays one mode; `casts` picks field Psynergy by name.
    pub fn run(&mut self, name: &str, casts: &[String]) -> Result<(), String> {
        match name {
            "walk" => self.walk(),
            "snow" => self.snow(),
            "layers" => self.layers(),
            "layers-3d" => self.exploded(),
            "battle-layers-3d" => self.battle_exploded(),
            "world-map-layers-3d" => self.world_map_exploded(),
            "menu-layers-3d" => self.menu_exploded(),
            "status-layers-3d" => self.status_exploded(),
            "talk-layers-3d" => self.talk_exploded(),
            "shop-layers-3d" => self.shop_exploded(),
            "scene-actors" => self.scene_actors(),
            "debug-rooms" => self.debug_rooms(),
            "debug-room-talks" => self.debug_room_talks(),
            "moment" => self.play_moment(),
            "world-map" => self.world_map(),
            "menus" => self.menus(),
            "menu-start" => self.menu_start(),
            "boot" => self.boot(),
            "items" => self.items(),
            "field-psynergy" => self.field_psynergy(casts),
            "talk" => self.talk(),
            "djinn-room" => self.djinn_room(),
            "shop" => self.shop(),
            "inn" => self.inn(),
            "menu-psynergy" => self.menu_psynergy(),
            "credits" => self.credits(),
            "sleep" => self.sleep(),
            "oam" => self.oam(),
            "vram" => self.vram(),
            "cells" => self.cells(),
            "grid" => self.grid(),
            "random-battle" => self.random_battle(),
            "world-map-picture" => self.world_map_picture(),
            "overlay" => self.overlay(),
            "tret-tree-branch-split" => self.tret_tree_branch_split(),
            "kolima-forest-tree-healing" => self.kolima_forest_tree_healing(),
            "altmiller-cave-darkness" => self.altmiller_cave_darkness(),
            "bilibin-cave-third-puddle" => self.bilibin_cave_third_puddle(),
            "fuchin-falls-cave-logs" => self.fuchin_falls_cave_logs(),
            "mercury-lighthouse-pool" => self.mercury_lighthouse_pool(),
            "lunpa-fortress-patrol" => self.lunpa_fortress_patrol(),
            "tolbi-spring-toss" => self.tolbi_spring_toss(),
            "kolima-forest-entrance-fields" => self.kolima_forest_entrance_fields(),
            "tret-tree-interiors-cocoons" => self.tret_tree_interiors_cocoons(),
            "colosseum-azart-race" => self.colosseum_azart_race(),
            "colosseum-navampa-scales" => self.colosseum_navampa_scales(),
            "colosseum-satrage-lifts" => self.colosseum_satrage_lifts(),
            "tunnel-ruins-balance" => self.tunnel_ruins_balance(),
            "venus-lighthouse-aerie-eruption" => self.venus_lighthouse_aerie_eruption(),
            "scene-event-cells" => self.scene_event_cells(),
            "credits-tile-ring" => self.credits_tile_ring(),
            "title-intro-layers-3d" => self.title_intro_exploded(),
            "item-screen-windows" => self.item_screen_windows(),
            "sound-players" => self.sound_players(),
            "field-requests" => self.field_requests(),
            "grid-planes" => self.grid_planes(),
            "tile-animation" => self.tile_animation(),
            "world-map-scanlines" => self.world_map_scanlines(),
            "world-map-footprint" => self.world_map_footprint(),
            "round-events" => self.round_events(),
            "battle-places" => self.battle_places(),
            "effect-canvas" => self.effect_canvas(),
            "effect-transfer" => self.effect_transfer(),
            "effect-particles" => self.effect_particles(),
            "sprite-layers" => self.sprite_layers(),
            "shop-counters" => self.shop_counters(),
            "sanctum-account" => self.sanctum_account(),
            "star-room-rising" => self.star_room_rising(),
            "eruption-departures" => self.eruption_departures(),
            "statue-sky-rows" => self.statue_sky_rows(),
            "vale-opening" => self.vale_opening(),
            "vale-gate-display" => self.vale_gate_display(),
            "vale-interiors-sleep" => self.vale_interiors_sleep(),
            "vale-kraden-rockslide" => self.vale_kraden_rockslide(),
            "vale-cave-kite" => self.vale_cave_kite(),
            "work-block" => self.window_work_block(),
            "text-ring" => self.window_text_ring(),
            "render-outputs" => self.window_render_outputs(),
            "area-name" => self.window_area_name(),
            "sanctum" => self.sanctum_members(),
            "item-picker" => self.item_picker(),
            "menu-slot" => self.shop_menu_slot(),
            "enemy-actions" => self.enemy_actions(),
            "first-strikes" => self.first_strikes(),
            "element-multiplier" => self.element_multiplier(),
            "plan-rolls" => self.plan_rolls(),
            "staging" => self.staging(),
            "added-effects" => self.added_effects(),
            "after-target" => self.after_target(),
            "effect-variants" => self.effect_variants(),
            "djinn-attacks" => self.djinn_attacks(),
            "spark-groups" => self.spark_groups(),
            "stat-reductions" => self.stat_reductions(),
            "target-strikes" => self.target_strikes(),
            "psynergy-steps" => self.menu_psynergy_steps(),
            "djinn-preview" => self.djinn_preview(),
            "status-pages" => self.status_pages(),
            "status-panels" => self.status_panels(),
            "item-sorting" => self.item_sorting(),
            _ => match self.fight(name) {
                Some(result) => result,
                None => self.run_engine(name),
            },
        }
    }
}

/// The layers the picture shows, back to front, as the hardware stacks them:
/// a lower priority (BGxCNT bits 1..0, an OAM entry's attribute 2 bits
/// 11..10) is nearer, sprites are nearer than a background of the same
/// priority, and of two backgrounds the lower-numbered is nearer. The
/// sprites take the priority most of the shown entries use, and only the
/// backgrounds the display mode has count. Layers are
/// numbered by their DISPCNT bit: 0 to 3 the backgrounds, 4 the sprites.
pub(super) fn stacking(field: &Field) -> Vec<u32> {
    let shown = field.read16(DISPCNT);
    let mut layers: Vec<(u16, u8, u32)> = Vec::new();
    // Mode 0 has four text backgrounds, mode 1 BG0 to BG2, mode 2 only the
    // affine BG2 and BG3.
    let modes = match shown & 7 {
        0 => 0..4u32,
        1 => 0..3,
        2 => 2..4,
        _ => 2..3,
    };
    for bg in modes {
        if shown & (0x100 << bg) != 0 {
            layers.push((field.read16(BG0CNT + 2 * bg) & 3, 1, bg));
        }
    }
    if shown & 0x1000 != 0 {
        let mut count = [0usize; 4];
        for entry in 0..128u32 {
            let attr0 = field.read16(OAM + 8 * entry);
            if attr0 & 0x300 == 0x200 {
                continue;
            }
            count[(field.read16(OAM + 8 * entry + 4) >> 10 & 3) as usize] += 1;
        }
        let priority = (0..4).max_by_key(|&p| (count[p], std::cmp::Reverse(p))).unwrap_or(0) as u16;
        layers.push((priority, 0, 4));
    }
    // Nearest first, then turned round so the farthest sheet comes first.
    layers.sort_by_key(|&(priority, kind, n)| (priority, kind, n));
    layers.iter().rev().map(|&(_, _, n)| n).collect()
}

/// Each of `layers` captured alone over KEY from the same saved moment, as a
/// sheet whose empty pixels are None. Each layer is chosen through DISPCNT,
/// as `exploded` does: hiding a background in libmgba itself crashed it in a
/// battle. It fails if the game rewrites DISPCNT during the capture.
pub(super) fn layer_sheets(field: &mut Field, layers: &[u32]) -> Result<Vec<Vec<Option<u32>>>, String> {
    let saved = field.save();
    let shown = field.read16(DISPCNT);
    let mut sheets = Vec::new();
    for &layer in layers {
        field.load(&saved);
        let bit = 0x100u16 << layer;
        field.io_write16(DISPCNT, (shown & !LAYER_BITS) | bit);
        field.set_backdrop(KEY);
        field.frames(2)?;
        if field.read16(DISPCNT) & LAYER_BITS != bit {
            return Err(format!("the game rewrote DISPCNT while layer {layer} was captured"));
        }
        sheets.push(field.picture().iter().map(|&c| (c != KEY).then(|| animation::rgb(c))).collect());
    }
    field.load(&saved);
    Ok(sheets)
}

/// The exploded view's frames at 60 a second: flat, apart, held, together,
/// flat, as `layers-3d.gif` plays them.
pub(super) fn spread_frames(sheets: &[Vec<Option<u32>>]) -> Vec<Vec<u32>> {
    let mut spread = Vec::new();
    spread.extend(std::iter::repeat_n(0.0, 50));
    spread.extend((0..60).map(|i| ease(i as f64 / 59.0)));
    spread.extend(std::iter::repeat_n(1.0, 120));
    spread.extend((0..60).map(|i| ease(1.0 - i as f64 / 59.0)));
    spread.extend(std::iter::repeat_n(0.0, 20));
    spread.iter().map(|&e| draw_sheets(sheets, e)).collect()
}

/// The first background control register; BG1CNT to BG3CNT follow it.
const BG0CNT: u32 = 0x0400_0008;
/// Sprite attribute memory: 128 entries of 8 bytes.
const OAM: u32 = 0x0700_0000;

impl Guide<'_> {
    /// The moment `field` is at, pulled apart like Vale's layers into
    /// `file`: each layer the display mode shows, in the order the hardware
    /// stacks them (stacking), as sheets that spread and close again.
    pub(super) fn explode_now(&mut self, field: &mut Field, file: &str, what: &str) -> Result<(), String> {
        let stem = file.trim_end_matches(".gif").trim_end_matches("-layers-3d");
        self.planes_now(field, &format!("planes/{stem}"), what)?;
        let layers = stacking(field);
        let sheets = layer_sheets(field, &layers)?;
        let frames = spread_frames(&sheets);
        let written = animation::write(&self.out.join(file), &frames, CANVAS.0)?;
        let order: Vec<String> = layers.iter().map(|&n| if n == 4 { "sprites".to_string() } else { format!("BG{n}") }).collect();
        self.written.push((file.to_string(), written.bytes, format!("{what} pulled apart, back to front {}; {} frames", order.join(", "), written.frames)));
        Ok(())
    }

    /// The field menu open in Vale, pulled apart, as `menu-layers-3d.gif`:
    /// the menu's windows apart from the map and the party behind them.
    pub fn menu_exploded(&mut self) -> Result<(), String> {
        let mut field = self.enter(VALE)?;
        let mut frames = Vec::new();
        Self::hold(&mut field, 0, 20, &mut frames)?;
        Self::hold(&mut field, KEY_A, 4, &mut frames)?;
        Self::hold(&mut field, 0, 50, &mut frames)?;
        self.explode_now(&mut field, "menu-layers-3d.gif", "the field menu in Vale")
    }

    /// The Status screen open in Vale, pulled apart, as `status-layers-3d.gif`:
    /// the field menu's third icon chosen (the Djinn icon is left out until
    /// the first Djinni joins, so it is Status), its party row, its windows
    /// and their text on BG0, and the figures, portrait and hand as sprites,
    /// over the map they hide.
    pub fn status_exploded(&mut self) -> Result<(), String> {
        let mut field = self.enter(VALE)?;
        let mut frames = Vec::new();
        for (keys, n) in [(0, 20), (KEY_A, 4), (0, 50), (KEY_RIGHT, 4), (0, 30), (KEY_RIGHT, 4), (0, 30), (KEY_A, 4), (0, 90)] {
            Self::hold(&mut field, keys, n, &mut frames)?;
        }
        self.explode_now(&mut field, "status-layers-3d.gif", "the Status screen in Vale")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_casts_resolve_through_the_source() {
        let casts = field_casts().unwrap();
        let names = field_cast_names().unwrap();
        // Every field Psynergy once: the kind-9 abilities from Move to
        // Avoid, as ability_ids.h numbers them.
        let (first, last) = (source::ability_id("ABILITY_MOVE").unwrap(), source::ability_id("ABILITY_AVOID").unwrap());
        let mut ids: Vec<u16> = casts.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids, (first..=last).collect::<Vec<_>>());
        assert_eq!(names.len(), ids.len());
        assert!(casts.iter().all(|c| !c.scene.is_empty()));
    }
}
