//! The folder guides' other pictures: what a mechanism looks like as it
//! plays (a window opening, a shop's counter, the credits' crossfade, the
//! sleep mode), and the machine's memory drawn as pictures (video memory and
//! its palettes, the sprite list, a map's cells, the overlay area).

use super::animation;
use super::field::{box_holds, Field, Image, Spot};
use super::guide::{Entered, Guide};
use super::mgba::{HEIGHT, WIDTH};
use super::places::write_png;
use super::story::{Trigger, MOVE};

const KEY_A: u32 = 0x1;
const KEY_B: u32 = 0x2;
const KEY_RIGHT: u32 = 0x10;
const KEY_LEFT: u32 = 0x20;
const KEY_UP: u32 = 0x40;
const KEY_DOWN: u32 = 0x80;
const KEY_R: u32 = 0x100;
const KEY_L: u32 = 0x200;

/// The places the pictures are made in (scene, entrance).
const VALE: (u16, u16) = (4, 1);
const WORLD_MAP: (u16, u16) = (2, 1);
/// Goma Cave's pillar room, entered from the pond to its south (its default
/// entrance stands in for it with the header's wider bounds).
const GOMA_CAVE: (u16, u16) = (27, 5);
/// Bilibin's item shop and inn, rooms of BilibinInteriors (scene_links.h).
const BILIBIN_ITEM_SHOP: (u16, u16) = (31, 5);
const BILIBIN_INN: (u16, u16) = (31, 6);
/// Facing north, 1/65536 turns: across a counter.
const NORTH: u32 = 0xc000;
/// The Djinn test room (debug/rooms.c).
const DJINN_ROOM: (u16, u16) = (191, 0);
/// The title overlay's entrance that runs the credits (ScrollFar_Entry00).
const CREDITS: (u16, u16) = (0, 9);
/// Vale's actors: Garet's sister, the stone pillar Move pushes.
const GARETS_SISTER: u16 = 18;
const VALE_PILLAR: u16 = 20;
/// Bilibin's item shopkeeper and innkeeper (bilibin_interiors.c).
const BILIBIN_ITEM_SHOPKEEPER: u16 = 18;
const BILIBIN_INNKEEPER: u16 = 19;
/// The Djinn room's Flint.
const FLINT: u16 = 8;
/// The scene number of a battle (SCENE_BATTLE).
const SCENE_BATTLE: u16 = 0x1fe;
/// Frames from a battle's start until its command menu is up and the
/// battle is still.
const BATTLE_SETTLE_FRAMES: u32 = 300;
/// Frames walked south on the world map before its picture is taken.
const WORLD_MAP_SETTLE_FRAMES: u32 = 50;

/// Memory: video RAM (backgrounds, then sprites from OBJ_VRAM), palette RAM,
/// OAM, the display registers, the overlay area and the map's cells and
/// blocks (ram_buffer.h).
const VRAM: u32 = 0x0600_0000;
const OBJ_VRAM: u32 = 0x0601_0000;
const PALETTE: u32 = 0x0500_0000;
const OAM: u32 = 0x0700_0000;
const DISPCNT: u32 = 0x0400_0000;
const LAYER_BITS: u16 = 0x1f00;
const BGCNT: u32 = 0x0400_0008;
const OVERLAY_AREA: u32 = 0x0200_8000;
const OVERLAY_SIZE: u32 = 0x8000;
const MAP_CELLS: u32 = 0x0201_0000;
const MAP_BLOCKS: u32 = 0x0202_0000;
/// MapState.layers' cells pointers (field.rs).
const MAP_LAYER_CELLS: u32 = 0x130;
const MAP_LAYER_SIZE: u32 = 0x30;
/// The map's cells: 128 by 128.
const GRID: i32 = 128;

fn rgb(c: u16) -> u32 {
    animation::rgb(c)
}

/// An image of `w` by `h` pixels in `fill`.
fn canvas(w: usize, h: usize, fill: u32) -> Image {
    Image { width: w, height: h, pixels: vec![fill; w * h] }
}

/// Draws `from` into `to` with its top left at (x, y), `scale` times its
/// size.
fn paste(to: &mut Image, from: &Image, x: usize, y: usize, scale: usize) {
    for fy in 0..from.height * scale {
        for fx in 0..from.width * scale {
            let (tx, ty) = (x + fx, y + fy);
            if tx < to.width && ty < to.height {
                to.pixels[ty * to.width + tx] = from.pixels[(fy / scale) * from.width + fx / scale];
            }
        }
    }
}

fn screen(picture: &[u16]) -> Image {
    Image { width: WIDTH, height: HEIGHT, pixels: picture.iter().map(|&c| rgb(c)).collect() }
}

fn mix(a: u32, b: u32, share: f64) -> u32 {
    let channel = |s: u32| {
        let (x, y) = (f64::from((a >> s) & 0xff), f64::from((b >> s) & 0xff));
        ((x + (y - x) * share).round() as u32) << s
    };
    channel(16) | channel(8) | channel(0)
}

/// Outlines the rectangle (x, y, w, h) in `colour`, clipped.
fn outline(image: &mut Image, x: i32, y: i32, w: i32, h: i32, colour: u32) {
    let mut put = |px: i32, py: i32| {
        if px >= 0 && py >= 0 && (px as usize) < image.width && (py as usize) < image.height {
            image.pixels[py as usize * image.width + px as usize] = colour;
        }
    };
    for i in 0..w {
        put(x + i, y);
        put(x + i, y + h - 1);
    }
    for i in 0..h {
        put(x, y + i);
        put(x + w - 1, y + i);
    }
}

/// A colour for the n-th of many things, around the colour wheel.
fn hue(n: usize) -> u32 {
    let h = (n as f64 * 0.618_034).fract() * 6.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    let c = |v: f64| (55.0 + v * 200.0) as u32;
    c(r) << 16 | c(g) << 8 | c(b)
}

/// One 8 by 8 tile at `address`, 4 or 8 bits a pixel, with colours from
/// `palette` (the 16 or 256 entries it reads); colour 0 is `empty`.
fn tile(field: &Field, address: u32, wide: bool, palette: &[u32], empty: u32) -> [u32; 64] {
    let mut out = [empty; 64];
    for (i, px) in out.iter_mut().enumerate() {
        let index = if wide {
            usize::from(field.read8(address + i as u32))
        } else {
            let b = field.read8(address + (i / 2) as u32);
            usize::from(if i % 2 == 0 { b & 15 } else { b >> 4 })
        };
        if index != 0 {
            *px = palette.get(index).copied().unwrap_or(empty);
        }
    }
    out
}

fn put_tile(image: &mut Image, x: usize, y: usize, pixels: &[u32; 64], hflip: bool, vflip: bool) {
    for ty in 0..8 {
        for tx in 0..8 {
            let (sx, sy) = (if hflip { 7 - tx } else { tx }, if vflip { 7 - ty } else { ty });
            let (px, py) = (x + tx, y + ty);
            if px < image.width && py < image.height {
                image.pixels[py * image.width + px] = pixels[sy * 8 + sx];
            }
        }
    }
}

/// The 512 colours of palette RAM: 256 for the backgrounds, then 256 for
/// the sprites.
fn palettes(field: &Field) -> Vec<u32> {
    (0..512).map(|i| rgb(field.read16(PALETTE + 2 * i))).collect()
}

impl<'a> Guide<'a> {
    /// Writes `frames` (BGR555 at 60 a second) as `file`.
    fn guide_gif(&mut self, file: &str, frames: &[Vec<u16>], what: &str) -> Result<(), String> {
        let rgb: Vec<Vec<u32>> = frames.iter().map(|f| f.iter().map(|&c| rgb(c)).collect()).collect();
        let written = animation::write(&self.out.join(file), &rgb, WIDTH)?;
        self.written.push((file.to_string(), written.bytes, format!("{what}; {} frames at 30 a second", written.frames)));
        Ok(())
    }

    fn guide_png(&mut self, file: &str, image: &Image, what: &str) -> Result<(), String> {
        let bytes = write_png(&self.out.join(file), image)?;
        self.written.push((file.to_string(), bytes, what.to_string()));
        Ok(())
    }

    /// The scene entered with the leader where `trigger` would start, from
    /// `place` or else from an entrance whose room holds the actor: the
    /// entered scene and the spot, the player in control there.
    fn stand_for(&self, place: (u16, u16), trigger: Trigger, actor: u16) -> Result<(Entered<'a>, Spot), String> {
        self.stand_where(place, trigger, actor, |_| true)
    }

    /// As `stand_for`, at the first spot `pick` accepts.
    fn stand_where(&self, place: (u16, u16), trigger: Trigger, actor: u16, pick: impl Fn(&Spot) -> bool) -> Result<(Entered<'a>, Spot), String> {
        let mut entered = self.enter_with(place, &[])?;
        let first = match entered.field.spots(trigger) {
            Ok(spots) if spots.iter().any(&pick) => {
                let spot = *spots.iter().find(|s| pick(s)).unwrap();
                entered.field.stand(&entered.placed, &spot)?;
                return Ok((entered, spot));
            }
            Ok(_) => "no spot fits".to_string(),
            Err(e) => e,
        };
        let at = entered.field.actor(actor).ok_or(first.clone())?;
        let header = entered.field.header();
        let others: Vec<u16> = entered.field.entrances().iter().filter(|e| e.number as u16 != place.1 && box_holds(e.camera, header, at.x, at.z)).map(|e| e.number as u16).collect();
        for entrance in others {
            let Ok(mut entered) = self.enter_with((place.0, entrance), &[]) else {
                continue;
            };
            if let Some(spot) = entered.field.spots(trigger).ok().and_then(|s| s.into_iter().find(|s| pick(s))) {
                entered.field.stand(&entered.placed, &spot)?;
                return Ok((entered, spot));
            }
        }
        Err(first)
    }

    /// Holds each step's keys in turn from the moment the leader stands
    /// ready, keeping every frame.
    fn press_path(field: &mut Field, path: &[(u32, u32)]) -> Result<Vec<Vec<u16>>, String> {
        let mut frames = vec![field.picture()];
        for &(keys, n) in path {
            Self::hold(field, keys, n, &mut frames)?;
        }
        Ok(frames)
    }

    /// Talking to Garet's sister in Vale: the window zooms open, the line is
    /// typed out and the arrow bobs, then the window closes, as `talk.gif`;
    /// and the window's first frames side by side, as `window-open.png`.
    /// Garet's sister talking in Vale, her first page typed out, pulled
    /// apart, as `talk-layers-3d.gif`: the message window and portrait on
    /// their own layer over the map and the actors.
    pub fn talk_exploded(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.stand_for(VALE, Trigger::Talk(GARETS_SISTER), GARETS_SISTER)?;
        let field = &mut entered.field;
        Self::press_path(field, &[(0, 20), (KEY_A, 3), (0, 150)])?;
        self.explode_now(field, "talk-layers-3d.gif", "a message in Vale")
    }

    pub fn talk(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.stand_for(VALE, Trigger::Talk(GARETS_SISTER), GARETS_SISTER)?;
        let field = &mut entered.field;
        let mut frames = Self::press_path(field, &[(0, 20), (KEY_A, 3), (0, 150), (KEY_A, 3), (0, 120), (KEY_A, 3), (0, 40)])?;
        // The window's opening: from the frame before the top 48 lines,
        // where it opens, first change, 10 frames at 60 a second.
        let still = frames[0].clone();
        let top = WIDTH * 48;
        let opens = frames.iter().position(|f| f[..top].iter().zip(&still[..top]).filter(|(a, b)| a != b).count() > 20).unwrap_or(1);
        let mut strip = canvas(5 * (WIDTH + 4) + 4, 2 * (HEIGHT + 4) + 4, 0xffffff);
        for (i, frame) in frames.iter().skip(opens - 1).take(10).enumerate() {
            paste(&mut strip, &screen(frame), 4 + (i % 5) * (WIDTH + 4), 4 + (i / 5) * (HEIGHT + 4), 1);
        }
        self.guide_png("window-open.png", &strip, "the message window's first 10 frames, left to right and down")?;
        let last = frames.last().cloned().unwrap_or_default();
        frames.extend(std::iter::repeat_n(last, 20));
        self.guide_gif("talk.gif", &frames, "talking to Garet's sister in Vale")
    }

    /// Talking to Flint in the Djinn test room: the capture effect and the
    /// Djinni joining, as `djinn-room.gif`.
    pub fn djinn_room(&mut self) -> Result<(), String> {
        // The test rooms draw no map: the actors stand on black.
        let mut entered = self.enter_with(DJINN_ROOM, &[])?;
        let frames = Self::record_trigger(&mut entered, Trigger::Talk(FLINT), None, 30)?;
        self.guide_gif("djinn-room.gif", &frames, "Flint given out in the Djinn test room")
    }


    /// Bilibin's item shop, the party given 500 coins: talked to across the
    /// counter, Buy, a Herb for Isaac, and B out, as `shop.gif`.
    pub fn shop(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.stand_where(BILIBIN_ITEM_SHOP, Trigger::Talk(BILIBIN_ITEM_SHOPKEEPER), BILIBIN_ITEM_SHOPKEEPER, |s| s.facing == NORTH)?;
        entered.field.set_coins(500);
        let path = [
            (0, 20),
            (KEY_A, 3),
            (0, 100),
            (KEY_A, 3),
            (0, 60),
            (KEY_A, 3),
            (0, 60),
            (KEY_A, 3),
            (0, 60),
            (KEY_A, 3),
            (0, 60),
            (KEY_A, 3),
            (0, 60),
            (KEY_B, 3),
            (0, 70),
            (KEY_B, 3),
            (0, 70),
            (KEY_A, 3),
            (0, 90),
        ];
        let frames = Self::press_path(&mut entered.field, &path)?;
        self.guide_gif("shop.gif", &frames, "buying at Bilibin's item shop")
    }

    /// Bilibin's inn, the party given 500 coins: a night's stay, the fade
    /// to black and back, as `inn.gif`.
    pub fn inn(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.stand_where(BILIBIN_INN, Trigger::Talk(BILIBIN_INNKEEPER), BILIBIN_INNKEEPER, |s| s.facing == NORTH)?;
        entered.field.set_coins(500);
        let path = [(0, 20), (KEY_A, 3), (0, 70), (KEY_A, 3), (0, 90), (KEY_A, 3), (0, 90), (KEY_A, 3), (0, 600), (KEY_A, 3), (0, 90)];
        let frames = Self::press_path(&mut entered.field, &path)?;
        self.guide_gif("inn.gif", &frames, "a night at Bilibin's inn")
    }

    /// Move chosen from the field menu in front of Vale's pillar: the menu
    /// closes and the field casts it, as `menu-psynergy.gif`.
    pub fn menu_psynergy(&mut self) -> Result<(), String> {
        let (mut entered, spot) = self.stand_where(VALE, Trigger::Psynergy(MOVE, VALE_PILLAR), VALE_PILLAR, |s| s.walk == KEY_RIGHT)?;
        // The menu, Psynergy, Isaac, then Move where it is in his list.
        let at = entered.field.abilities(0)?.iter().position(|&a| a == MOVE).ok_or("Isaac has no Move here")?;
        let mut path = vec![(0, 20), (KEY_A, 3), (0, 50), (KEY_A, 3), (0, 50), (KEY_A, 3), (0, 50)];
        for _ in 0..at {
            path.extend([(KEY_DOWN, 3), (0, 15)]);
        }
        path.extend([(0, 20), (KEY_A, 3), (0, 100), (spot.walk, 60), (0, 200)]);
        let frames = Self::press_path(&mut entered.field, &path)?;
        self.guide_gif("menu-psynergy.gif", &frames, "Move chosen from the menu and cast on Vale's pillar")
    }

    /// The credits from their start, kept from just before the first
    /// backdrop fades into the second until the second is held, as
    /// `credits.gif`; and at the crossfade's middle the two backdrops, the
    /// names and the picture they make, as `credits-layers.png`.
    pub fn credits(&mut self) -> Result<(), String> {
        let setup = super::field::Setup { scene: CREDITS.0, entrance: CREDITS.1, set: Vec::new(), clear: Vec::new() };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        let mut frames = Vec::new();
        Self::hold(&mut field, 0, 1300, &mut frames)?;
        // The first frame not black, then the lead and the first slide's
        // fade and hold: the second slide's fade starts 300 + 64 + 267 frames
        // in.
        let first = frames.iter().position(|f| f.iter().any(|&c| c & 0x7fff != 0)).unwrap_or(0);
        // (measured: the blend steps run in the 40 frames before it).
        let fade = first + 631 - 40;
        if fade + 200 > frames.len() {
            return Err("the credits ended early".to_string());
        }
        self.guide_gif("credits.gif", &frames[fade - 60..fade + 160], "the credits' first crossfade, the names scrolling")?;
        // Back to the crossfade's middle, each layer alone.
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        field.frames((fade + 20) as u32)?;
        let saved = field.save();
        let shown = field.read16(DISPCNT);
        let mut shots = Vec::new();
        // DISPCNT bits 8 to 12: BG0 to BG3 and the sprites; a store holds
        // for the frame drawn next.
        for bits in [0x800u16, 0x400, 0x1000, shown & LAYER_BITS] {
            field.load(&saved);
            field.io_write16(DISPCNT, (shown & !LAYER_BITS) | bits);
            field.frames(1)?;
            shots.push(screen(&field.picture()));
        }
        let mut out = canvas(2 * WIDTH + 12, 2 * HEIGHT + 12, 0xffffff);
        for (i, shot) in shots.iter().enumerate() {
            paste(&mut out, shot, 4 + (i % 2) * (WIDTH + 4), 4 + (i / 2) * (HEIGHT + 4), 1);
        }
        self.guide_png("credits-layers.png", &out, "the crossfade's middle: BG3, BG2, the sprites and the screen")
    }

    /// Holding only L and R in Vale until the game sleeps: three seconds, then
    /// the white screen that comes before the console stops, as `sleep.gif`.
    pub fn sleep(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with(VALE, &[])?;
        let mut frames = Self::press_path(&mut entered.field, &[(0, 20), (KEY_L | KEY_R, 250)])?;
        // The stopped console draws nothing new: end on the white screen.
        if let Some(last) = frames.iter().rposition(|f| f.iter().all(|&c| c & 0x7fff == 0x7fff)) {
            frames.truncate(last + 1);
        }
        self.guide_gif("sleep.gif", &frames, "L and R held in Vale until the screen turns white for sleep")
    }

    /// One frame of Vale with each entry of the sprite list (OAM) outlined
    /// in its own colour, beside the sprites alone, as `oam.png`.
    /// Where Garet's sister stands in Vale at twice the GBA's size, each
    /// actor on screen marked at its feet and labelled with its actor
    /// number (the scene's own placement rows start at ACTOR_FIRST_PLACED),
    /// as `scene-actors.png`.
    pub fn scene_actors(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.stand_for(VALE, Trigger::Talk(GARETS_SISTER), GARETS_SISTER)?;
        let field = &mut entered.field;
        field.frames(1)?;
        let mut image = canvas(2 * WIDTH, 2 * HEIGHT, 0);
        paste(&mut image, &screen(&field.picture()), 0, 0, 2);
        let (vx, vy) = field.view();
        for id in 0..ACTOR_SLOTS {
            let Some(a) = field.actor(id) else { continue };
            // A raised actor is drawn its height higher up the screen.
            let (sx, sy) = (2 * (a.x - vx), 2 * (a.z - (a.y >> 16) - vy));
            if !(0..2 * WIDTH as i32).contains(&sx) || !(0..2 * HEIGHT as i32).contains(&sy) {
                continue;
            }
            let colour = hue(usize::from(id));
            outline(&mut image, sx - 3, sy - 3, 6, 6, colour);
            label(&mut image, sx - 6, sy - 60, id, colour);
        }
        self.guide_png("scene-actors.png", &image, "Vale's actors labelled by number")
    }

    pub fn oam(&mut self) -> Result<(), String> {
        // Where Garet's sister and her neighbours stand.
        let (mut entered, _) = self.stand_for(VALE, Trigger::Talk(GARETS_SISTER), GARETS_SISTER)?;
        let field = &mut entered.field;
        let saved = field.save();
        field.frames(1)?;
        let full = screen(&field.picture());
        field.load(&saved);
        let shown = field.read16(DISPCNT);
        field.io_write16(DISPCNT, (shown & !LAYER_BITS) | 0x1000);
        field.set_backdrop(0x4210);
        field.frames(1)?;
        let mut alone = screen(&field.picture());
        field.io_write16(DISPCNT, shown);
        let mut boxed = full;
        let sizes = [[(8, 8), (16, 16), (32, 32), (64, 64)], [(16, 8), (32, 8), (32, 16), (64, 32)], [(8, 16), (8, 32), (16, 32), (32, 64)]];
        let mut n = 0;
        for i in 0..128 {
            let a0 = field.read16(OAM + 8 * i);
            let a1 = field.read16(OAM + 8 * i + 2);
            let mode = (a0 >> 8) & 3;
            if mode == 2 {
                continue;
            }
            let shape = usize::from(a0 >> 14);
            if shape == 3 {
                continue;
            }
            let (mut w, mut h) = sizes[shape][usize::from(a1 >> 14)];
            if mode == 3 {
                w *= 2;
                h *= 2;
            }
            let mut y = i32::from(a0 & 0xff);
            if y >= 160 {
                y -= 256;
            }
            let mut x = i32::from(a1 & 0x1ff);
            if x >= 240 {
                x -= 512;
            }
            if x + w <= 0 || y + h <= 0 || x >= WIDTH as i32 || y >= HEIGHT as i32 {
                continue;
            }
            let colour = hue(n);
            n += 1;
            outline(&mut boxed, x, y, w, h, colour);
            outline(&mut alone, x, y, w, h, colour);
        }
        let mut out = canvas(2 * WIDTH * 2 + 12, HEIGHT * 2 + 8, 0xffffff);
        paste(&mut out, &boxed, 4, 4, 2);
        paste(&mut out, &alone, 8 + 2 * WIDTH, 4, 2);
        self.guide_png("oam.png", &out, &format!("Vale with its {n} sprite list entries on screen outlined"))
    }

    /// Vale's video memory drawn as tiles, each in the palette the screen's
    /// maps or sprites give it: the four background character blocks, then
    /// the sprites' 32 KB; under them the 256 background colours and the 256
    /// sprite colours, as `vram.png`.
    pub fn vram(&mut self) -> Result<(), String> {
        let entered = self.enter_with(VALE, &[])?;
        let field = &entered.field;
        let colours = palettes(field);
        let empty = 0x20_20_28;
        // Each background tile's palette, from the maps of the layers shown.
        let dispcnt = field.read16(DISPCNT);
        let mut bg_palette = vec![None::<(usize, bool)>; 2048];
        for bg in 0..4u32 {
            if dispcnt & (0x100 << bg) == 0 {
                continue;
            }
            let control = field.read16(BGCNT + 2 * bg);
            let (char_base, screen_base, wide) = (u32::from((control >> 2) & 3), u32::from((control >> 8) & 31), control & 0x80 != 0);
            let blocks = [1, 2, 2, 4][usize::from(control >> 14)];
            for e in 0..(1024 * blocks) {
                let entry = field.read16(VRAM + screen_base * 0x800 + 2 * e);
                let size = if wide { 64 } else { 32 };
                let address = char_base * 0x4000 + u32::from(entry & 0x3ff) * size;
                let slot = (address / 32) as usize;
                if slot < 2048 && bg_palette[slot].is_none() {
                    bg_palette[slot] = Some((usize::from(entry >> 12), wide));
                }
            }
        }
        // The map's blocks name the rest of each layer's tiles, those off
        // screen too: layer 0 on BG3, 1 on BG2, 2 on BG1.
        for layer in 0..3u32 {
            let control = field.read16(BGCNT + 2 * (3 - layer));
            if dispcnt & (0x100 << (3 - layer)) == 0 || control & 0x80 != 0 {
                continue;
            }
            let char_base = u32::from((control >> 2) & 3);
            for e in 0..4096 * 4 {
                let entry = field.read16(MAP_BLOCKS + 2 * e);
                let slot = (char_base * 512 + u32::from(entry & 0x3ff)) as usize;
                if entry != 0 && slot < 2048 && bg_palette[slot].is_none() {
                    bg_palette[slot] = Some((usize::from(entry >> 12), false));
                }
            }
        }
        // Each sprite tile's palette, from the sprite list.
        let mut obj_palette = vec![None::<usize>; 1024];
        let sizes = [[(1, 1), (2, 2), (4, 4), (8, 8)], [(2, 1), (4, 1), (4, 2), (8, 4)], [(1, 2), (1, 4), (2, 4), (4, 8)]];
        for i in 0..128 {
            let (a0, a1, a2) = (field.read16(OAM + 8 * i), field.read16(OAM + 8 * i + 2), field.read16(OAM + 8 * i + 4));
            if (a0 >> 8) & 3 == 2 || a0 >> 14 == 3 {
                continue;
            }
            let (w, h) = sizes[usize::from(a0 >> 14)][usize::from(a1 >> 14)];
            let wide = a0 & 0x2000 != 0;
            for t in 0..(w * h) * if wide { 2 } else { 1 } {
                let slot = (usize::from(a2 & 0x3ff) + t) & 1023;
                obj_palette[slot].get_or_insert(if wide { 256 } else { usize::from(a2 >> 12) });
            }
        }
        let mut out = canvas(768 + 12, 256 + 8 + 2 * 24 + 8, 0xffffff);
        let grey: Vec<u32> = (0..256).map(|i| { let v = (i * 255 / 15).min(255) as u32; v << 16 | v << 8 | v }).collect();
        for block in 0..4u32 {
            let (bx, by) = (4 + (block as usize % 2) * 256, 4 + (block as usize / 2) * 128);
            for t in 0..512u32 {
                let slot = (block * 512 + t) as usize;
                let (pixels, _) = match bg_palette[slot] {
                    Some((p, wide)) => {
                        let pal: Vec<u32> = if wide { colours[..256].to_vec() } else { colours[p * 16..p * 16 + 16].to_vec() };
                        (tile(field, VRAM + slot as u32 * 32, false, &pal, empty), true)
                    }
                    None => (tile(field, VRAM + slot as u32 * 32, false, &grey[..16], empty), false),
                };
                put_tile(&mut out, bx + (t as usize % 32) * 8, by + (t as usize / 32) * 8, &pixels, false, false);
            }
        }
        for t in 0..1024usize {
            let pal: Vec<u32> = match obj_palette[t] {
                Some(p) if p < 16 => colours[256 + p * 16..256 + p * 16 + 16].to_vec(),
                Some(_) => colours[256..].to_vec(),
                None => grey[..16].to_vec(),
            };
            let pixels = tile(field, OBJ_VRAM + t as u32 * 32, false, &pal, empty);
            put_tile(&mut out, 8 + 512 + (t % 32) * 8, 4 + (t / 32) * 8, &pixels, false, false);
        }
        for (row, start) in [0usize, 256].into_iter().enumerate() {
            for i in 0..256 {
                let (x, y) = (4 + i * 3, 4 + 256 + 8 + row * 24);
                for dy in 0..20 {
                    for dx in 0..3 {
                        out.pixels[(y + dy) * out.width + x + dx] = colours[start + i];
                    }
                }
            }
        }
        self.guide_png("vram.png", &out, "Vale's video memory as tiles, and its palettes")
    }

    /// The map cells under one screen of Goma Cave: the screen, and the same
    /// screen with each ground cell's collision drawn over it (walls red,
    /// the scene's event cells yellow, the codes every map shares blue) and
    /// a line wherever the ground's height changes between two cells, as
    /// `cells.png`.
    pub fn cells(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with(GOMA_CAVE, &[])?;
        let field = &mut entered.field;
        Self::press_path(field, &[(0, 10)])?;
        let plain = screen(&field.picture());
        let mut drawn = Image { width: WIDTH, height: HEIGHT, pixels: plain.pixels.clone() };
        let (vx, vy) = field.view();
        let layer = field.leader().map_or(0, |l| l.layer.min(2));
        // A cell's ground is drawn its height above its place on the map, as
        // the actors standing on it are: rows below the screen can show.
        let (cx0, cz0) = (vx.div_euclid(16), vy.div_euclid(16));
        let rows = cz0..=cz0 + 16;
        let mut heights = std::collections::HashMap::new();
        for cz in rows.clone() {
            for cx in cx0..=cx0 + 15 {
                heights.insert((cx, cz), field.ground(layer, cx * 16 + 8, cz * 16 + 8)? >> 16);
            }
        }
        for cz in rows.clone() {
            for cx in cx0..=cx0 + 15 {
                let h = heights[&(cx, cz)];
                let tint = match field.cell(layer, cx, cz) {
                    255 => Some(0xe0_20_20),
                    1..=239 => Some(0xf0_d0_20),
                    240..=254 => Some(0x30_80_f0),
                    _ => None,
                };
                let (sx, sy) = (cx * 16 - vx, cz * 16 - vy - h);
                for y in sy.max(0)..(sy + 16).min(HEIGHT as i32) {
                    for x in sx.max(0)..(sx + 16).min(WIDTH as i32) {
                        let at = &mut drawn.pixels[y as usize * WIDTH + x as usize];
                        *at = match tint {
                            Some(t) => mix(*at, t, 0.5),
                            None => mix(*at, 0x30_30_30, 0.15),
                        };
                    }
                }
                outline(&mut drawn, sx, sy, 16, 16, 0x50_50_50);
                // A white edge where the ground behind is at another height.
                if heights.get(&(cx, cz - 1)).is_some_and(|&b| b != h) {
                    outline(&mut drawn, sx, sy, 16, 2, 0xff_ff_ff);
                }
                if heights.get(&(cx - 1, cz)).is_some_and(|&l| l != h) {
                    outline(&mut drawn, sx, sy, 2, 16, 0xff_ff_ff);
                }
            }
        }
        let mut out = canvas(4 * WIDTH + 12, 2 * HEIGHT + 8, 0xffffff);
        paste(&mut out, &plain, 4, 4, 2);
        paste(&mut out, &drawn, 8 + 2 * WIDTH, 4, 2);
        self.guide_png("cells.png", &out, "one screen of Goma Cave and its ground cells")
    }

    /// The whole cell grid of the map Goma Cave stands on, each cell drawn
    /// from its block, and each layer's window onto it outlined: the ground
    /// layer white, the middle green, the overhead orange, as `grid.png`.
    pub fn grid(&mut self) -> Result<(), String> {
        let entered = self.enter_with(GOMA_CAVE, &[])?;
        let field = &entered.field;
        let colours = palettes(field);
        let work = field.map_work();
        let header = field.header();
        let (span_x, span_z) = ((header[2] - header[0]) >> 20, (header[3] - header[1]) >> 20);
        // Layer 0 on BG3, 1 on BG2 and 2 on BG1: each window's origin in the
        // grid and its tiles' character block.
        let mut windows = Vec::new();
        for layer in 0..3u32 {
            let cells = field.read32(work + MAP_LAYER_CELLS + MAP_LAYER_SIZE * layer);
            if cells < MAP_CELLS {
                continue;
            }
            let index = ((cells - MAP_CELLS) / 4) as i32;
            let control = field.read16(BGCNT + 2 * (3 - layer));
            windows.push((index % GRID, index / GRID, u32::from((control >> 2) & 3), control & 0x80 != 0));
        }
        let mut image = canvas((GRID * 16) as usize, (GRID * 16) as usize, 0x10_10_14);
        for cz in 0..GRID {
            for cx in 0..GRID {
                let Some(&(_, _, char_base, wide)) = windows.iter().rev().find(|w| cx >= w.0 && cz >= w.1 && cx < w.0 + span_x.max(1) && cz < w.1 + span_z.max(1)) else {
                    continue;
                };
                let word = field.read32(MAP_CELLS + 4 * (cx + cz * GRID) as u32);
                let block = word & 0xfff;
                for q in 0..4u32 {
                    let entry = field.read16(MAP_BLOCKS + block * 8 + 2 * q);
                    let size = if wide { 64 } else { 32 };
                    let address = VRAM + char_base * 0x4000 + u32::from(entry & 0x3ff) * size;
                    let pal: Vec<u32> = if wide { colours[..256].to_vec() } else { let p = usize::from(entry >> 12); colours[p * 16..p * 16 + 16].to_vec() };
                    let pixels = tile(field, address, wide, &pal, 0x10_10_14);
                    put_tile(&mut image, (cx * 16) as usize + (q as usize % 2) * 8, (cz * 16) as usize + (q as usize / 2) * 8, &pixels, entry & 0x400 != 0, entry & 0x800 != 0);
                }
            }
        }
        for (i, w) in windows.iter().enumerate() {
            let colour = [0xff_ff_ff, 0x40_e0_60, 0xff_a0_30][i];
            for d in 0..4 {
                outline(&mut image, w.0 * 16 - d, w.1 * 16 - d, span_x * 16 + 2 * d, span_z * 16 + 2 * d, colour);
            }
        }
        // Half size.
        let mut out = canvas(image.width / 2, image.height / 2, 0);
        for y in 0..out.height {
            for x in 0..out.width {
                out.pixels[y * out.width + x] = image.pixels[2 * y * image.width + 2 * x];
            }
        }
        self.guide_png("grid.png", &out, "the cell grid of Goma Cave's map and its three layer windows")
    }

    /// Walking back and forth in Goma Cave until a random battle starts, and
    /// the battle's opening, as `random-battle.gif`.
    pub fn random_battle(&mut self) -> Result<(), String> {
        let (entrance, stretch) = self.goma_cave_stretch()?;
        let mut entered = self.enter_with((GOMA_CAVE.0, entrance), &[])?;
        let field = &mut entered.field;
        let mut frames = Vec::new();
        let at = Self::walk_stretch(field, &stretch, 80, &mut frames, &mut 0)?.ok_or("no battle came")?;
        for _ in 0..240 {
            field.frames(1)?;
            frames.push(field.picture());
        }
        // From two seconds of walking before the battle began.
        let start = at.saturating_sub(180);
        self.guide_gif("random-battle.gif", &frames[start..], "a random battle in Goma Cave")
    }

    /// The world map pulled apart like Vale's layers, as `world-map-layers-3d.gif`:
    /// the tilted ground, which the HBlank DMA rescales line by line, apart
    /// from the sprites and windows drawn flat over it.
    pub fn world_map_exploded(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with(WORLD_MAP, &[])?;
        let field = &mut entered.field;
        // A short walk south first, as world-map-walk.gif does, so the view
        // is the tilted one the player walks in.
        field.hold(KEY_DOWN);
        field.frames(WORLD_MAP_SETTLE_FRAMES)?;
        field.hold(0);
        field.frames(2)?;
        self.explode_now(field, "world-map-layers-3d.gif", "the world map")
    }

    /// A random battle in Goma Cave pulled apart like Vale's layers, once the
    /// battle has settled: each background it shows and the sprites, drawn as
    /// sheets in the order the hardware stacks them (guide::stacking), as
    /// `battle-layers-3d.gif`.
    pub fn battle_exploded(&mut self) -> Result<(), String> {
        let (entrance, stretch) = self.goma_cave_stretch()?;
        let mut entered = self.enter_with((GOMA_CAVE.0, entrance), &[])?;
        let field = &mut entered.field;
        Self::walk_stretch(field, &stretch, 80, &mut Vec::new(), &mut 0)?.ok_or("no battle came")?;
        field.frames(BATTLE_SETTLE_FRAMES)?;
        self.explode_now(field, "battle-layers-3d.gif", "a Goma Cave battle")
    }

    /// The entrance of Goma Cave and the stretch of ground from it that a
    /// random battle is walked into: each entrance and each way from it, the
    /// straight run of ground the field's collision lets the leader walk
    /// (Field::reach), tried there and back once; the one the camera shows
    /// him on best (the least of the dark around him, where a cave draws
    /// nothing), and then the longest.
    fn goma_cave_stretch(&self) -> Result<(u16, Stretch), String> {
        let mut best: Option<(u32, i32, u16, Stretch)> = None;
        for entrance in 1..=6u16 {
            for across in [true, false] {
                let Ok(mut entered) = self.enter_with((GOMA_CAVE.0, entrance), &[]) else {
                    continue;
                };
                let Ok(stretch) = Self::open_stretch(&mut entered.field, across) else {
                    continue;
                };
                let length = stretch.ends[1] - stretch.ends[0];
                if length < 64 {
                    continue;
                }
                let mut frames = Vec::new();
                let mut dark = 0;
                Self::walk_stretch(&mut entered.field, &stretch, 2, &mut frames, &mut dark)?;
                if best.as_ref().is_none_or(|b| (dark, -length) < (b.0, -b.1)) {
                    best = Some((dark, length, entrance, stretch));
                }
            }
        }
        let (_, _, entrance, stretch) = best.ok_or("no entrance of Goma Cave has open ground to walk")?;
        Ok((entrance, stretch))
    }

    /// Walks the leader `legs` times from one end of `stretch` to the other
    /// with the d-pad (B held), each leg until he reaches its end or
    /// something stops him, keeping every frame; `dark` keeps the most of
    /// the 32 by 32 pixels around him that were black in any frame. Returns
    /// the frame a battle began, if one did.
    fn walk_stretch(field: &mut Field, stretch: &Stretch, legs: u32, frames: &mut Vec<Vec<u16>>, dark: &mut u32) -> Result<Option<usize>, String> {
        let mut way = 1;
        for _ in 0..legs {
            field.hold(stretch.keys[way] | KEY_B);
            let mut still = 0;
            let mut last = field.leader().map(|a| (a.x, a.z));
            for _ in 0..400 {
                field.frames(1)?;
                let picture = field.picture();
                if field.scene() == SCENE_BATTLE {
                    frames.push(picture);
                    field.hold(0);
                    return Ok(Some(frames.len()));
                }
                let now = field.leader();
                if let Some(a) = now {
                    let (vx, vy) = field.view();
                    let (sx, sy) = (a.x - vx, a.z - vy - 16);
                    let black = (sy - 16..sy + 16)
                        .flat_map(|y| (sx - 16..sx + 16).map(move |x| (x, y)))
                        .filter(|&(x, y)| (0..WIDTH as i32).contains(&x) && (0..HEIGHT as i32).contains(&y))
                        .filter(|&(x, y)| { let c = picture[y as usize * WIDTH + x as usize]; (0..3).all(|i| (c >> (5 * i)) & 31 < 4) })
                        .count() as u32;
                    *dark = (*dark).max(black);
                }
                frames.push(picture);
                let now = now.map(|a| (a.x, a.z));
                still = if now == last { still + 1 } else { 0 };
                last = now;
                let along = now.map_or(0, |(x, z)| if stretch.across { x } else { z });
                if still >= 6 || (way == 1 && along >= stretch.ends[1]) || (way == 0 && along <= stretch.ends[0]) {
                    break;
                }
            }
            way = 1 - way;
        }
        field.hold(0);
        Ok(None)
    }

    /// The straight run of ground through the leader's cell that he can
    /// walk (Field::reach), crossing no exit, across (x) or down (z).
    fn open_stretch(field: &mut Field, across: bool) -> Result<Stretch, String> {
        let reach = field.reach(None)?;
        let leader = field.leader().ok_or("the scene has no leader")?;
        let (cx, cz) = (leader.x >> 4, leader.z >> 4);
        let layer = reach.layer;
        let run = |dx: i32, dz: i32| (1..64).take_while(|k| reach.holds(layer, (cx + dx * k) * 16 + 8, (cz + dz * k) * 16 + 8)).count() as i32;
        // The ends' centres, a cell short of each end.
        let ends = |less: i32, more: i32, at: i32| [(at - less + 1) * 16 + 8, (at + more - 1) * 16 + 8];
        Ok(if across {
            Stretch { across: true, ends: ends(run(-1, 0), run(1, 0), cx), keys: [KEY_LEFT, KEY_RIGHT] }
        } else {
            Stretch { across: false, ends: ends(run(0, -1), run(0, 1), cz), keys: [KEY_UP, KEY_DOWN] }
        })
    }

    /// The world map's still picture: R, the cursor moved over the map, L
    /// held to bring it back to the party, and A closing it, as `world-map-picture.gif`.
    pub fn world_map_picture(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with(WORLD_MAP, &[])?;
        let path = [(0, 20), (KEY_R, 3), (0, 90), (KEY_RIGHT, 50), (0, 40), (KEY_UP, 40), (0, 40), (KEY_L, 40), (0, 40), (KEY_A, 3), (0, 60)];
        let frames = Self::press_path(&mut entered.field, &path)?;
        self.guide_gif("world-map-picture.gif", &frames, "the world map's still picture")
    }

    /// Walking from Vale into the Sanctum, the screen beside the 32 KB
    /// overlay area drawn a pixel a byte: the bytes change as the Sanctum's
    /// overlay is unpacked over Vale's, as `overlay.gif`.
    pub fn overlay(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with(VALE, &[])?;
        let field = &mut entered.field;
        let (w, h) = (WIDTH + 8 + 128, 256usize);
        let mut frames = Vec::new();
        let draw = |field: &Field| {
            let mut c = canvas(w, h, 0);
            paste(&mut c, &screen(&field.picture()), 0, (h - HEIGHT) / 2, 1);
            for i in 0..OVERLAY_SIZE / 4 {
                let word = field.read32(OVERLAY_AREA + 4 * i);
                for b in 0..4 {
                    let v = (word >> (8 * b)) & 0xff;
                    let n = (4 * i + b) as usize;
                    c.pixels[(n / 128) * w + WIDTH + 8 + n % 128] = v << 16 | v << 8 | v;
                }
            }
            c.pixels
        };
        frames.push(draw(field));
        for (keys, n) in [(0u32, 20u32), (KEY_UP, 200), (0, 120)] {
            field.hold(keys);
            for _ in 0..n {
                field.frames(1)?;
                frames.push(draw(field));
            }
        }
        field.hold(0);
        let written = animation::write(&self.out.join("overlay.gif"), &frames, w)?;
        self.written.push(("overlay.gif".to_string(), written.bytes, format!("walking into the Sanctum beside the overlay area; {} frames", written.frames)));
        Ok(())
    }

}

/// A straight run of walkable ground (Guide::open_stretch): along x or z,
/// its two ends in pixels, and the keys that walk toward each.
struct Stretch {
    across: bool,
    ends: [i32; 2],
    keys: [u32; 2],
}

/// The field's actor slots (FIELD_ACTOR_COUNT in field.rs).
const ACTOR_SLOTS: u16 = 66;

/// Each digit of a 3 by 5 font, a row of three bits to a line, top first.
const DIGITS: [[u8; 5]; 10] = [
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [7, 1, 7, 4, 7],
    [7, 1, 3, 1, 7],
    [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7],
    [7, 4, 7, 5, 7],
    [7, 1, 1, 1, 1],
    [7, 5, 7, 5, 7],
    [7, 5, 7, 1, 7],
];

/// Writes `n` at (x, y) in the 3 by 5 font at twice its size, white on a box
/// of `colour`, clipped.
fn label(image: &mut Image, x: i32, y: i32, n: u16, colour: u32) {
    let text = n.to_string();
    let (w, h) = (text.len() as i32 * 8 + 2, 14);
    let mut put = |px: i32, py: i32, c: u32| {
        if px >= 0 && py >= 0 && (px as usize) < image.width && (py as usize) < image.height {
            image.pixels[py as usize * image.width + px as usize] = c;
        }
    };
    for by in 0..h {
        for bx in 0..w {
            put(x + bx, y + by, colour);
        }
    }
    for (i, ch) in text.bytes().enumerate() {
        let rows = DIGITS[usize::from(ch - b'0')];
        for (r, bits) in rows.iter().enumerate() {
            for c in 0..3 {
                if bits & (4 >> c) != 0 {
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        put(x + 2 + i as i32 * 8 + 2 * c + dx, y + 2 + 2 * r as i32 + dy, 0xff_ff_ff);
                    }
                }
            }
        }
    }
}
