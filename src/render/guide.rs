//! `render guide`: pictures for the folder guides, each showing one idea the
//! guide explains, played from a built edition with the pad held as a player
//! would: the leader walking and then running, a map built up layer by
//! layer, footprints in the snow, and the world map's perspective view.

use super::animation;
use super::field::{Field, Setup, Symbols};
use super::mgba::{HEIGHT, WIDTH};
use super::story::Flags;
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
    pub written: Vec<(String, u64, String)>,
}

impl<'a> Guide<'a> {
    fn flag(&self, name: &str) -> Result<u16, String> {
        self.flags.resolve(name, None).ok_or_else(|| format!("flags.tsv names no {name}"))
    }

    fn gif(&mut self, file: &str, frames: &[Vec<u16>], what: &str) -> Result<(), String> {
        let rgb: Vec<Vec<u32>> = frames.iter().map(|f| f.iter().map(|&c| animation::rgb(c)).collect()).collect();
        let written = animation::write(&self.out.join(file), &rgb, WIDTH)?;
        self.written.push((file.to_string(), written.bytes, format!("{what}; {} frames at 30 a second", written.frames)));
        Ok(())
    }

    /// Enters a scene after the party has left Vale, with Isaac, Garet, Ivan
    /// and Mia, and runs until the player has control.
    fn enter(&self, (scene, entrance): (u16, u16)) -> Result<Field<'a>, String> {
        let set = vec![self.flag("FLAG_PARTY_LEFT_VALE")?];
        let clear = vec![self.flag("FLAG_VALE_STORM")?, self.flag("FLAG_CHILDHOOD")?];
        let setup = Setup { scene, entrance, set, clear };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        field.set_party(&[0, 1, 2, 3]);
        field.until_control()?;
        field.frames(20)?;
        Ok(field)
    }

    /// Holds `keys` for `frames` frames, keeping each picture.
    fn hold(field: &mut Field, keys: u32, frames: u32, out: &mut Vec<Vec<u16>>) -> Result<(), String> {
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
    /// held at 1.5 (FieldObject_UpdatePlayerControl).
    pub fn walk(&mut self) -> Result<(), String> {
        let path = [(KEY_RIGHT, 60), (0, 15), (KEY_LEFT | KEY_B, 60), (0, 20)];
        self.walk_path(VALE, &path, "walk-run.gif", "walking right, then running back")
    }

    /// Imil's snow, where the leader's footprints stay a moment, each a
    /// field object of its own.
    pub fn snow(&mut self) -> Result<(), String> {
        let path = [(KEY_DOWN, 40), (KEY_RIGHT, 50), (KEY_UP, 40), (0, 60)];
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

    /// The game from power-on with no key pressed and no save file: the
    /// Nintendo and Camelot logos, then the Golden Sun logo, which a new
    /// game follows with name entry, as `boot.gif`. BOOT_FRAMES ends it on
    /// the logo.
    pub fn boot(&mut self) -> Result<(), String> {
        let setup = Setup { scene: POWER_ON.0, entrance: POWER_ON.1, set: Vec::new(), clear: Vec::new() };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        let mut frames = Vec::new();
        Self::hold(&mut field, 0, 900, &mut frames)?;
        let first = frames.iter().position(|f| f.iter().any(|&c| c & 0x7fff != 0)).unwrap_or(0);
        let end = (first + BOOT_FRAMES).min(frames.len());
        self.gif("boot.gif", &frames[first..end], "the game from power-on: the Nintendo, Camelot and Golden Sun logos")
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
const KEY: u16 = 0x7c1f;
/// The exploded view's picture, its background and the sheets' tint and edge.
const CANVAS: (usize, usize) = (520, 400);
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

fn ease(t: f64) -> f64 {
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
fn draw_sheets(sheets: &[Vec<Option<u32>>], e: f64) -> Vec<u32> {
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
