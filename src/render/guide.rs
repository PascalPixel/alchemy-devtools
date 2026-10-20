//! `render guide`: pictures for the folder guides, each showing one idea the
//! guide explains, played from a built edition with the pad held as a player
//! would: the leader walking and then running, a map built up layer by
//! layer, footprints in the snow, and the world map's perspective view.

use super::animation;
use super::field::{Field, Setup, Symbols};
use super::mgba::WIDTH;
use super::story::Flags;
use std::path::Path;

/// KEYINPUT bits.
const KEY_B: u32 = 0x2;
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
}
