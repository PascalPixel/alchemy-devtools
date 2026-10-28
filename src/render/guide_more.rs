//! The folder guides' other pictures: what a mechanism looks like as it
//! plays (a window opening, a shop's counter, the credits' crossfade, the
//! sleep mode), and the machine's memory drawn as pictures (video memory and
//! its palettes, the sprite list, a map's cells, the overlay area).

use super::animation;
use super::animation::is_black_bgr555;
use super::field::{box_holds, Actor, Field, Image, Spot, CELL_WALL, CLEARED, STEP_DOWN, STEP_UP};
use super::guide::{Entered, Guide};
use super::mgba::{HEIGHT, WIDTH};
use super::places::write_png;
use super::source;
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
/// Bilibin's item shop and inn, rooms of CottageRoomsC (scene_exits.h).
const BILIBIN_ITEM_SHOP: (u16, u16) = (31, 5);
const BILIBIN_INN: (u16, u16) = (31, 6);
/// Facing north, 1/65536 turns: across a counter.
const NORTH: u32 = 0xc000;
/// The test scenes (debug/test_scenes.c).
const DJINN_ROOM: (u16, u16) = (191, 0);
/// The title overlay's entrance that runs the credits (Import_CreditsRun).
const CREDITS: (u16, u16) = (0, 9);
/// Vale's actors: Garet's sister, the stone pillar Move pushes.
const GARETS_SISTER: u16 = 18;
const VALE_PILLAR: u16 = 20;
/// Bilibin's item shopkeeper and innkeeper (cottage_rooms_c.c).
const BILIBIN_ITEM_SHOPKEEPER: u16 = 18;
const BILIBIN_INNKEEPER: u16 = 19;
/// The Djinn room's Flint.
const FLINT: u16 = 8;
/// The scene number of a battle (MAP_BATTLE).
const MAP_BATTLE: u16 = 0x1fe;
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
const MAPGRID_CELLS: u32 = 0x0201_0000;
const MAP_BLOCKS: u32 = 0x0202_0000;
/// MapGridScrollAnimView.layers' cells pointers (field.rs).
const MAP_LAYER_CELLS: u32 = 0x130;
const MAP_LAYER_SIZE: u32 = 0x30;
/// The map's cells: 128 by 128.
const GRID: i32 = 128;

fn rgb(c: u16) -> u32 {
    animation::rgb(c)
}

/// An image of `w` by `h` pixels in `fill`.
pub(super) fn canvas(w: usize, h: usize, fill: u32) -> Image {
    Image { width: w, height: h, pixels: vec![fill; w * h] }
}

/// Draws `from` into `to` with its top left at (x, y), `scale` times its
/// size.
pub(super) fn paste(to: &mut Image, from: &Image, x: usize, y: usize, scale: usize) {
    for fy in 0..from.height * scale {
        for fx in 0..from.width * scale {
            let (tx, ty) = (x + fx, y + fy);
            if tx < to.width && ty < to.height {
                to.pixels[ty * to.width + tx] = from.pixels[(fy / scale) * from.width + fx / scale];
            }
        }
    }
}

pub(super) fn screen(picture: &[u16]) -> Image {
    Image { width: WIDTH, height: HEIGHT, pixels: picture.iter().map(|&c| rgb(c)).collect() }
}

pub(super) fn mix(a: u32, b: u32, share: f64) -> u32 {
    let channel = |s: u32| {
        let (x, y) = (f64::from((a >> s) & 0xff), f64::from((b >> s) & 0xff));
        ((x + (y - x) * share).round() as u32) << s
    };
    channel(16) | channel(8) | channel(0)
}

/// Outlines the rectangle (x, y, w, h) in `colour`, clipped.
pub(super) fn outline(image: &mut Image, x: i32, y: i32, w: i32, h: i32, colour: u32) {
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
pub(super) fn hue(n: usize) -> u32 {
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
pub(super) fn tile(field: &Field, address: u32, wide: bool, palette: &[u32], empty: u32) -> [u32; 64] {
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

pub(super) fn put_tile(image: &mut Image, x: usize, y: usize, pixels: &[u32; 64], hflip: bool, vflip: bool) {
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
pub(super) fn palettes(field: &Field) -> Vec<u32> {
    (0..512).map(|i| rgb(field.read16(PALETTE + 2 * i))).collect()
}

impl<'a> Guide<'a> {
    /// Writes `frames` (BGR555 at 60 a second) as `file`.
    pub(super) fn guide_gif(&mut self, file: &str, frames: &[Vec<u16>], what: &str) -> Result<(), String> {
        let rgb: Vec<Vec<u32>> = frames.iter().map(|f| f.iter().map(|&c| rgb(c)).collect()).collect();
        let written = animation::write(&self.out.join(file), &rgb, WIDTH)?;
        self.written.push((file.to_string(), written.bytes, format!("{what}; {} frames at 30 a second", written.frames)));
        Ok(())
    }

    pub(super) fn guide_png(&mut self, file: &str, image: &Image, what: &str) -> Result<(), String> {
        let bytes = write_png(&self.out.join(file), image)?;
        self.written.push((file.to_string(), bytes, what.to_string()));
        Ok(())
    }

    /// The scene entered with the leader where `trigger` would start, from
    /// `place` or else from an entrance whose room holds the actor: the
    /// entered scene and the spot, the player in control there.
    pub(super) fn stand_for(&self, place: (u16, u16), trigger: Trigger, actor: u16) -> Result<(Entered<'a>, Spot), String> {
        self.stand_where(place, trigger, actor, |_| true)
    }

    /// As `stand_for`, at the first spot `pick` accepts.
    pub(super) fn stand_where(&self, place: (u16, u16), trigger: Trigger, actor: u16, pick: impl Fn(&Spot) -> bool) -> Result<(Entered<'a>, Spot), String> {
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
    pub(super) fn press_path(field: &mut Field, path: &[(u32, u32)]) -> Result<Vec<Vec<u16>>, String> {
        let mut frames = vec![field.picture()];
        for &(keys, n) in path {
            Self::hold(field, keys, n, &mut frames)?;
        }
        Ok(frames)
    }

    /// As press_path, for a path through a black screen: played again with
    /// the leader standing ready for longer first where it reaches black
    /// sooner than CLEARED, then run on with the pad left alone until it
    /// has shown CLEARED frames past its last black screen
    /// (animation::check_black_clearance).
    pub(super) fn press_path_cleared(field: &mut Field, path: &[(u32, u32)]) -> Result<Vec<Vec<u16>>, String> {
        let start = field.save();
        let mut frames = Self::press_path(field, path)?;
        if let Some(first) = frames.iter().position(|f| is_black_bgr555(f)).filter(|&b| b < CLEARED) {
            field.load(&start);
            let mut longer = vec![(0, (CLEARED - first) as u32)];
            longer.extend_from_slice(path);
            frames = Self::press_path(field, &longer)?;
        }
        Self::run_past_black(field, &mut frames)?;
        Ok(frames)
    }

    /// Runs `field` on, the pad left alone and every picture kept, until
    /// `frames` shows CLEARED frames past its last black screen.
    pub(super) fn run_past_black(field: &mut Field, frames: &mut Vec<Vec<u16>>) -> Result<(), String> {
        // A black screen the run on reaches is cleared in turn, four at most.
        for _ in 0..4 {
            let Some(last) = frames.iter().rposition(|f| is_black_bgr555(f)) else { break };
            let since = frames.len() - 1 - last;
            if since >= CLEARED {
                break;
            }
            Self::hold(field, 0, (CLEARED - since) as u32, frames)?;
        }
        Ok(())
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

    /// Bilibin's item shop as the shopkeeper asks `What would you like?`,
    /// pulled apart, as `shop-layers-3d.gif`: Buy chosen, the price, coins,
    /// list and description windows open on BG0 with their text drawn into
    /// them, and the message's letters, the portrait and the item icons as
    /// sprites over them. The path is `shop.gif`'s up to that question.
    pub fn shop_exploded(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.stand_where(BILIBIN_ITEM_SHOP, Trigger::Talk(BILIBIN_ITEM_SHOPKEEPER), BILIBIN_ITEM_SHOPKEEPER, |s| s.facing == NORTH)?;
        entered.field.set_coins(500);
        Self::press_path(&mut entered.field, &[(0, 20), (KEY_A, 3), (0, 100), (KEY_A, 3), (0, 50)])?;
        self.explode_now(&mut entered.field, "shop-layers-3d.gif", "Bilibin's item shop")
    }

    /// Bilibin's inn, the party given 500 coins: a night's stay, the fade
    /// to black and back, as `inn.gif`.
    pub fn inn(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.stand_where(BILIBIN_INN, Trigger::Talk(BILIBIN_INNKEEPER), BILIBIN_INNKEEPER, |s| s.facing == NORTH)?;
        entered.field.set_coins(500);
        let path = [(0, 20), (KEY_A, 3), (0, 70), (KEY_A, 3), (0, 90), (KEY_A, 3), (0, 90), (KEY_A, 3), (0, 600), (KEY_A, 3), (0, 90)];
        let frames = Self::press_path_cleared(&mut entered.field, &path)?;
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
        let image = Self::labelled_actors(&mut entered.field)?;
        self.guide_png("scene-actors.png", &image, "Vale's actors labelled by number")
    }

    /// The debug level room and menu test room, which draw no map, each with
    /// its actors labelled by number, as `debug-level-room.png` and
    /// `debug-menu-room.png`.
    pub fn debug_rooms(&mut self) -> Result<(), String> {
        for (scene, file, what) in [(LEVEL_ROOM, "debug-level-room.png", "the level room's actors labelled"), (MENU_TEST_ROOM, "debug-menu-room.png", "the menu test room's actors labelled")] {
            let mut entered = self.enter_with((scene, 0), &[])?;
            let image = Self::labelled_actors(&mut entered.field)?;
            self.guide_png(file, &image, what)?;
        }
        Ok(())
    }

    /// Each placed actor of the debug level room talked to (A pressed where
    /// a talk with it starts) and the screen two seconds later, as
    /// `debug-level-room-<actor>.png`, the talk itself as
    /// `debug-level-room-<actor>.gif`.
    pub fn debug_room_talks(&mut self) -> Result<(), String> {
        for actor in LEVEL_ROOM_ACTORS {
            let mut entered = self.enter_with((LEVEL_ROOM, 0), &[])?;
            // The room draws no map, so the trigger's spots are all off
            // stage: the leader stands 16 pixels east of the actor, facing
            // it, on its ground (the icon and portrait browsers stand one
            // above the other, 24 pixels apart).
            let at = entered.field.actor(actor).ok_or(format!("actor {actor} is not placed"))?;
            let spot = Spot { x: at.x + 16, z: at.z, layer: at.layer, height: at.y, facing: WEST, walk: 0, cell: None };
            entered.field.stand(&entered.placed, &spot)?;
            let frames = Self::press_path(&mut entered.field, &[(0, 20), (KEY_A, 3), (0, 120)])?;
            let last = frames.last().cloned().unwrap_or_default();
            self.guide_png(&format!("debug-level-room-{actor}.png"), &screen(&last), &format!("actor {actor} of the debug level room, two seconds after A"))?;
            self.guide_gif(&format!("debug-level-room-{actor}.gif"), &frames, &format!("talking to actor {actor} of the debug level room"))?;
        }
        Ok(())
    }

    /// A scene entered and left to play on its own, for a moment no event
    /// recording reaches: MOMENT_PLACE (`scene,entrance`), MOMENT_SET (flag
    /// names or `0x` numbers, comma separated), MOMENT_FRAMES (default 600)
    /// and MOMENT_EVERY (default 30). Writes `moment.gif` and, from frame
    /// MOMENT_FROM on, every MOMENT_EVERY-th frame with its actors labelled as
    /// `moment-<frame>.png`; for each of MOMENT_ACTORS it prints where the
    /// actor is and what it adds to the screen, the next frames with it and
    /// with it moved to the map's origin compared. MOMENT_A presses A that
    /// often to page through messages, and MOMENT_OAM prints the sprite
    /// list's blended entries. MOMENT_PLANES also writes each of those
    /// frames as its planes, into `planes/moment-<frame>/`. MOMENT_CELLS
    /// prints where the map holds each of its collision codes.
    pub fn play_moment(&mut self) -> Result<(), String> {
        let var = |name: &str| std::env::var(name).unwrap_or_default();
        let place: Vec<u16> = var("MOMENT_PLACE").split(',').filter_map(|v| v.trim().parse().ok()).collect();
        // A full run of the guide plays no moment unless one is asked for.
        let [scene, entrance] = place[..] else {
            eprintln!("moment: skipped, MOMENT_PLACE (scene,entrance) is not set");
            return Ok(());
        };
        let names = var("MOMENT_SET");
        let set: Vec<&str> = names.split(',').map(str::trim).filter(|n| !n.is_empty()).collect();
        let frames: u32 = var("MOMENT_FRAMES").parse().unwrap_or(600);
        let every: u32 = var("MOMENT_EVERY").parse().unwrap_or(30);
        let watch: Vec<u16> = var("MOMENT_ACTORS").split(',').filter_map(|v| v.trim().parse().ok()).collect();
        // From the moment the party was placed, before the scene's opening
        // script has run, which may never hand the player control.
        let resolve = |n: &&str| match n.strip_prefix("0x") {
            Some(hex) => u16::from_str_radix(hex, 16).map_err(|e| e.to_string()),
            None => self.flag(n),
        };
        let set = set.iter().map(resolve).collect::<Result<Vec<u16>, String>>()?;
        let clear = vec![self.flag("FLAG_VALE_STORM")?, self.flag("FLAG_CHILDHOOD")?];
        let setup = super::field::Setup { scene, entrance, set, clear };
        let mut entered = Field::enter(self.rom, self.symbols, &setup)?;
        let field = &mut entered;
        // MOMENT_CELLS: where the map holds each of these collision codes
        // (the event rows' cell codes): each cell's top-left corner in
        // pixels on its map layer and its ground's height, which lifts it
        // that far up the picture. A map picture (render place's map-N.png)
        // starts at the scroll bounds' top left, also printed.
        let codes: Vec<u16> = var("MOMENT_CELLS").split(',').filter_map(|v| v.trim().parse().ok()).collect();
        if !codes.is_empty() {
            let b = field.bounds();
            println!("scroll bounds: x {}..{} z {}..{}", b[0] >> 16, b[2] >> 16, b[1] >> 16, b[3] >> 16);
        }
        for code in codes {
            let size = field.cell_size();
            for (layer, cx, cz) in field.cells_with_code(code)? {
                let (x, z) = (cx * size, cz * size);
                let height = field.ground(layer, x + size / 2, z + size / 2)? >> 16;
                println!("cell code {code}: layer {layer}, x {x} z {z}, height {height}");
            }
        }
        // MOMENT_STAND (`x,z` or `x,z,facing` in pixels and 1/65536 turns):
        // the leader stood there on the ground once the player has control.
        // MOMENT_HOLD (`keys,frames,keys,frames...`): those keys then held
        // for that many frames, one pair after another, as the player would
        // walk him. The leader's place and the camera's are printed after.
        let stand: Vec<i64> = var("MOMENT_STAND").split(',').filter_map(|v| v.trim().parse().ok()).collect();
        let hold: Vec<u32> = var("MOMENT_HOLD").split(',').filter_map(|v| v.trim().parse().ok()).collect();
        if !stand.is_empty() || !hold.is_empty() {
            let placed = field.save();
            field.until_control()?;
            field.frames(20)?;
            if let [x, z, ref rest @ ..] = stand[..] {
                let facing = rest.first().map_or(0x4000, |&f| f as u32);
                let spot = Self::spot_at(field, (x as i32, z as i32, facing))?;
                field.stand(&placed, &spot)?;
            }
        }
        for pair in hold.chunks(2) {
            let [keys, count] = pair[..] else { break };
            field.hold(keys);
            field.frames(count)?;
            field.hold(0);
            field.frames(4)?;
        }
        if let Some(a) = field.leader() {
            let (vx, vz) = field.view();
            println!("leader: x {} z {} layer {} height {}; view x {vx} z {vz}", a.x, a.z, a.layer, a.y >> 16);
        }
        let mut shots = vec![field.picture()];
        // MOMENT_A: A pressed for 3 frames every so many, to page through
        // the scene's messages.
        let press: u32 = var("MOMENT_A").parse().unwrap_or(0);
        // MOMENT_FROM: the first frame written or measured.
        let from: u32 = var("MOMENT_FROM").parse().unwrap_or(0);
        for n in 1..=frames {
            if press != 0 {
                field.hold(if n % press < 3 { KEY_A } else { 0 });
            }
            field.frames(1)?;
            shots.push(field.picture());
            if n % every == 0 && n >= from {
                // MOMENT_PLANES: the same moment taken apart, each plane
                // alone in planes/moment-<frame>/, for an illustration to
                // crop. planes_now comes back to the saved moment, so the
                // recording goes on unchanged.
                if !var("MOMENT_PLANES").is_empty() {
                    self.planes_now(field, &format!("planes/moment-{n}"), "a moment")?;
                }
                let image = Self::labelled_actors(field)?;
                self.guide_png(&format!("moment-{n}.png"), &image, "a moment, its actors labelled")?;
                if !var("MOMENT_OAM").is_empty() {
                    // The sprite list's entries drawn in a mode other than
                    // normal: semi-transparent (1) or an object window (2).
                    for i in 0..128 {
                        let a0 = field.read16(OAM + 8 * i);
                        let a1 = field.read16(OAM + 8 * i + 2);
                        let a2 = field.read16(OAM + 8 * i + 4);
                        let mode = (a0 >> 10) & 3;
                        if (a0 >> 8) & 3 != 2 && mode != 0 {
                            println!("frame {n}: OAM {i} mode {mode} y {} x {} tile {} priority {}", a0 & 0xff, a1 & 0x1ff, a2 & 0x3ff, (a2 >> 10) & 3);
                        }
                    }
                    println!("frame {n}: DISPCNT {:#06x} WINOUT {:#06x} BLDCNT {:#06x}", field.read16(DISPCNT), field.read16(0x0400_004a), field.read16(0x0400_0050));
                }
                for &id in &watch {
                    println!("frame {n}: {}", super::probe::describe(field, id));
                    // What the actor adds to the screen: the next frame with
                    // it and with it moved off to the map's origin.
                    let saved = field.save();
                    field.frames(3)?;
                    let with = field.picture();
                    field.load(&saved);
                    let a = field.actor_address(id);
                    if a != 0 {
                        field.write32(a + 8, 0);
                        field.write32(a + 0x10, 0);
                    }
                    field.frames(3)?;
                    let without = field.picture();
                    field.load(&saved);
                    let changed: Vec<usize> = (0..with.len()).filter(|&i| with[i] != without[i]).collect();
                    if let (Some(lo), Some(hi)) = (changed.iter().map(|i| i % WIDTH).min(), changed.iter().map(|i| i % WIDTH).max()) {
                        let (top, bottom) = (changed[0] / WIDTH, changed[changed.len() - 1] / WIDTH);
                        println!("frame {n}: actor {id} changes {} pixels, x {lo}..={hi}, y {top}..={bottom}", changed.len());
                        let mut marked = screen(&with);
                        outline(&mut marked, lo as i32 - 1, top as i32 - 1, (hi - lo + 3) as i32, (bottom - top + 3) as i32, 0xff00ff);
                        self.guide_png(&format!("moment-{n}-actor-{id}.png"), &marked, "what the actor adds, outlined")?;
                        // The changed box with the actor and without it, side
                        // by side at eight times the size.
                        let (bw, bh) = (hi - lo + 9, bottom - top + 9);
                        let mut pair = canvas(2 * 8 * bw + 8, 8 * bh, 0xff00ff);
                        for (k, source) in [&with, &without].into_iter().enumerate() {
                            for y in 0..bh {
                                for x in 0..bw {
                                    let (sx, sy) = ((lo + x).saturating_sub(4).min(WIDTH - 1), (top + y).saturating_sub(4).min(HEIGHT - 1));
                                    let colour = rgb(source[sy * WIDTH + sx]);
                                    for dy in 0..8 {
                                        for dx in 0..8 {
                                            pair.pixels[(8 * y + dy) * pair.width + k * (8 * bw + 8) + 8 * x + dx] = colour;
                                        }
                                    }
                                }
                            }
                        }
                        self.guide_png(&format!("moment-{n}-actor-{id}-pair.png"), &pair, "with and without the actor")?;
                    } else {
                        println!("frame {n}: actor {id} changes nothing on screen");
                    }
                }
            }
        }
        self.guide_gif("moment.gif", &shots, "a moment played on its own")
    }

    /// The screen at twice the GBA's size, each actor on it marked at its
    /// feet and labelled with its actor number.
    pub(super) fn labelled_actors(field: &mut Field) -> Result<Image, String> {
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
        Ok(image)
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

    /// The ground under one screen of Goma Cave, as `cells.png`: the screen,
    /// and the same screen with the ground of the layer the leader follows
    /// drawn over it pixel by pixel at its place on the map, raised by the
    /// height of the leader's ground as his sprite is (0 here). The leader's
    /// layer shows on a background that scrolls with the view, and a layer's
    /// cells pointer already starts at its window into the grid, so map and
    /// screen pixels match. A cell with a collision code is tinted by what
    /// answers it (guide_cells::meaning: a wall red, an exit row blue, a
    /// touch row magenta), and each of the scene's own is joined to words
    /// saying what it does; open ground more than a step above the leader's
    /// (MapGrid_CheckCollision's 8 pixels) is orange, and more than 12 below it
    /// purple; a white line runs wherever the ground's height changes; a key
    /// under it says what each colour stands for. Goma Cave walls its floor
    /// with rims of ground 32 pixels high, often cut on a cell's diagonal,
    /// rather than with wall codes, so heights are read for every pixel
    /// (MapGrid_GetTerrainHeight) where a cell isn't flat. The screen is above,
    /// its ground below.
    pub fn cells(&mut self) -> Result<(), String> {
        let names = self.cell_names("gGomaCavePillar_Triggers|gGomaCavePillar_Events", "GomaCavePillar")?;
        let mut entered = self.enter_goma_cave(GOMA_CAVE)?;
        let field = &mut entered.field;
        Self::press_path(field, &[(0, 10)])?;
        let plain = screen(&field.picture());
        let (drawn, coded, shades) = Self::ground_overlay(field, &plain, &names, super::guide_cells::GOMA_CAVE_PILLAR_WORDS)?;
        let mut picture = canvas(2 * WIDTH, 4 * HEIGHT + 8, 0xffffff);
        paste(&mut picture, &plain, 0, 0, 2);
        let below = 2 * HEIGHT as i32 + 8;
        paste(&mut picture, &drawn, 0, below as usize, 2);
        // A label for each of the scene's own meanings, from its rightmost
        // cell.
        let mut marks: Vec<super::guide_cells::Mark> = Vec::new();
        let mut legend: Vec<(u32, String)> = shades;
        for (m, sx, sy) in &coded {
            let colour = m.kind.colour();
            if !legend.iter().any(|(c, _)| *c == colour) {
                legend.push((colour, m.kind.key().to_string()));
            }
            let at = (2 * sx + 31, below + 2 * sy + 16);
            if matches!(m.kind, super::guide_cells::Kind::Exit | super::guide_cells::Kind::Touch | super::guide_cells::Kind::Search | super::guide_cells::Kind::NoEvent) {
                match marks.iter_mut().find(|k| k.1 == m.words) {
                    Some(k) if k.2 .0 >= at.0 => {}
                    Some(k) => k.2 = at,
                    None => marks.push((colour, m.words.clone(), at)),
                }
            }
        }
        let out = super::guide_cells::labelled(&picture, &marks, &legend, 216);
        self.guide_png("cells.png", &out, "one screen of Goma Cave and its ground, each cell's meaning in words")
    }

    /// `plain`, the screen as it is now, with the ground of the leader's
    /// layer drawn over it as `cells` describes; and each cell on screen
    /// with a collision code, by its meaning and its top left on the
    /// screen, and the ground's shades the picture uses with their words.
    #[allow(clippy::type_complexity)]
    fn ground_overlay(field: &mut Field, plain: &Image, names: &super::guide_cells::Names, words: super::guide_cells::Words) -> Result<(Image, Vec<(super::guide_cells::Meaning, i32, i32)>, Vec<(u32, String)>), String> {
        let mut drawn = Image { width: WIDTH, height: HEIGHT, pixels: plain.pixels.clone() };
        let (vx, vy) = field.view();
        let leader = field.leader().ok_or("no leader")?;
        let layer = leader.layer.min(2);
        let floor = field.ground(layer, leader.x, leader.z)? >> 16;
        let (w, h) = (WIDTH as i32, HEIGHT as i32);
        // Each screen pixel's ground height and collision code.
        let mut height = vec![0i32; WIDTH * HEIGHT];
        let mut code = vec![0i32; WIDTH * HEIGHT];
        // Each code's meaning, read once.
        let mut meanings: std::collections::HashMap<i32, Option<super::guide_cells::Meaning>> = std::collections::HashMap::new();
        let mut coded = Vec::new();
        // The map pixel under each screen pixel, at the leader's height as
        // his sprite is drawn.
        let (mx, mz) = (vx, vy + floor);
        for cz in mz.div_euclid(16)..=(mz + h - 1).div_euclid(16) {
            for cx in mx.div_euclid(16)..=(mx + w - 1).div_euclid(16) {
                let (kind, flat) = field.shape(layer, cx, cz);
                let c = field.cell(layer, cx, cz);
                if !meanings.contains_key(&c) {
                    meanings.insert(c, super::guide_cells::meaning(field, names, words, c)?);
                }
                if let Some(m) = &meanings[&c] {
                    coded.push((m.clone(), cx * 16 - mx, cz * 16 - mz));
                }
                for py in 0..16 {
                    for px in 0..16 {
                        let (sx, sy) = (cx * 16 + px - mx, cz * 16 + py - mz);
                        if !(0..w).contains(&sx) || !(0..h).contains(&sy) {
                            continue;
                        }
                        let ground = if kind == 0 { flat } else { field.ground(layer, cx * 16 + px, cz * 16 + py)? >> 16 };
                        let at = (sy * w + sx) as usize;
                        height[at] = ground;
                        code[at] = c;
                    }
                }
            }
        }
        let colour_of = |c: i32| meanings.get(&c).and_then(|m| m.as_ref()).map(|m| m.kind.colour());
        let (high, low) = (0xff_80_20, 0x80_40_e0);
        let (mut any_high, mut any_low, mut any_line) = (false, false, false);
        for sy in 0..h {
            for sx in 0..w {
                let at = (sy * w + sx) as usize;
                let tint = match code[at] {
                    0 if height[at] - floor > 8 => {
                        any_high = true;
                        Some(high)
                    }
                    0 if height[at] - floor < -12 => {
                        any_low = true;
                        Some(low)
                    }
                    c => colour_of(c),
                };
                let pixel = &mut drawn.pixels[at];
                *pixel = match tint {
                    Some(t) => mix(*pixel, t, 0.55),
                    None => mix(*pixel, 0x30_30_30, 0.15),
                };
                // The cells' grid, faintly.
                if (sx + mx).rem_euclid(16) == 0 || (sy + mz).rem_euclid(16) == 0 {
                    *pixel = mix(*pixel, 0x50_50_50, 0.6);
                }
            }
        }
        // A cell with a collision code outlined in its colour, so it shows
        // under a sprite too.
        for (m, sx, sy) in &coded {
            outline(&mut drawn, *sx, *sy, 16, 16, m.kind.colour());
        }
        for sy in 0..h {
            for sx in 0..w {
                let at = (sy * w + sx) as usize;
                let right = sx + 1 < w && height[at + 1] != height[at];
                let below = sy + 1 < h && height[at + WIDTH] != height[at];
                if right || below {
                    drawn.pixels[at] = 0xff_ff_ff;
                    any_line = true;
                }
            }
        }
        let mut shades = Vec::new();
        if any_high {
            shades.push((high, "Ground more than 8 pixels above the leader's: too high to step up".to_string()));
        }
        if any_low {
            shades.push((low, "Ground more than 12 pixels below the leader's: too far to step down".to_string()));
        }
        if any_line {
            shades.push((0xff_ff_ff, "White line: the ground's height changes".to_string()));
        }
        Ok((drawn, coded, shades))
    }

    /// The whole cell grid of the map Goma Cave stands on, each cell drawn
    /// from its block, and each layer's window onto it outlined: the ground
    /// layer white, the middle green, the overhead orange, as `grid.png`.
    pub fn grid(&mut self) -> Result<(), String> {
        let entered = self.enter_goma_cave(GOMA_CAVE)?;
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
            if cells < MAPGRID_CELLS {
                continue;
            }
            let index = ((cells - MAPGRID_CELLS) / 4) as i32;
            let control = field.read16(BGCNT + 2 * (3 - layer));
            windows.push((index % GRID, index / GRID, u32::from((control >> 2) & 3), control & 0x80 != 0));
        }
        let mut image = canvas((GRID * 16) as usize, (GRID * 16) as usize, 0x10_10_14);
        for cz in 0..GRID {
            for cx in 0..GRID {
                let Some(&(_, _, char_base, wide)) = windows.iter().rev().find(|w| cx >= w.0 && cz >= w.1 && cx < w.0 + span_x.max(1) && cz < w.1 + span_z.max(1)) else {
                    continue;
                };
                let word = field.read32(MAPGRID_CELLS + 4 * (cx + cz * GRID) as u32);
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

    /// Walking back and forth in Goma Cave, as the party first reaches it,
    /// until a random battle starts, and the battle's opening, as
    /// `random-battle.gif`.
    pub fn random_battle(&mut self) -> Result<(), String> {
        let (place, stretch) = self.goma_cave_stretch()?;
        let mut entered = self.enter_goma_cave(place)?;
        let field = &mut entered.field;
        let mut frames = Vec::new();
        let at = Self::walk_stretch(field, &stretch, 80, &mut frames, &mut 0, &mut |_| {})?.ok_or("no battle came")?;
        Self::hold(field, 0, 240, &mut frames)?;
        Self::run_past_black(field, &mut frames)?;
        // From CLEARED frames of walking before the battle's first black
        // screen, where the walk is that long.
        let black = at + frames[at..].iter().position(|f| is_black_bgr555(f)).unwrap_or(0);
        let start = black.saturating_sub(CLEARED);
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
        let (place, stretch) = self.goma_cave_stretch()?;
        let mut entered = self.enter_goma_cave(place)?;
        let field = &mut entered.field;
        Self::walk_stretch(field, &stretch, 80, &mut Vec::new(), &mut 0, &mut |_| {})?.ok_or("no battle came")?;
        field.frames(BATTLE_SETTLE_FRAMES)?;
        self.explode_now(field, "battle-layers-3d.gif", "a Goma Cave battle")
    }

    /// Enters a room of Goma Cave as the story has it when the party first
    /// reaches the pillar room, the first of its rooms the story plays in
    /// (Guide::enter_first_visit).
    pub(super) fn enter_goma_cave(&self, place: (u16, u16)) -> Result<Entered<'a>, String> {
        self.enter_first_visit(place, GOMA_CAVE.0)
    }

    /// The room and entrance of Goma Cave and the stretch of ground from it
    /// that a random battle is walked into, the cave entered as the party
    /// first reaches it (Guide::enter_first_visit): each entrance its room's
    /// table lists (another number puts the party at the default place, in
    /// the dark outside the room) and each way from it, the straight run the
    /// game lets the leader walk there (Guide::measure_stretch), walked there
    /// and back once; of those the camera shows him on, never with more than
    /// DARK_LIMIT of the pixels around him black (a cave draws nothing beyond
    /// its rooms), the longest.
    pub(super) fn goma_cave_stretch(&self) -> Result<((u16, u16), Stretch), String> {
        let mut best: Option<(i32, (u16, u16), Stretch)> = None;
        for scene in source::map_id(GOMA_CAVE_ROOMS.0)?..=source::map_id(GOMA_CAVE_ROOMS.1)? {
            let table = self.enter_goma_cave((scene, 1))?.field.entrances();
            for entrance in table.iter().map(|e| e.number as u16) {
                for across in [true, false] {
                    let Ok(mut entered) = self.enter_goma_cave((scene, entrance)) else {
                        continue;
                    };
                    let Ok(stretch) = Self::measure_stretch(&mut entered.field, across) else {
                        continue;
                    };
                    let length = stretch.ends[1] - stretch.ends[0];
                    if length < 64 || best.as_ref().is_some_and(|b| b.0 >= length) {
                        continue;
                    }
                    let mut dark = 0;
                    if Self::walk_stretch(&mut entered.field, &stretch, 2, &mut Vec::new(), &mut dark, &mut |_| {}).is_err() || dark > DARK_LIMIT {
                        continue;
                    }
                    best = Some((length, (scene, entrance), stretch));
                }
            }
        }
        let (_, place, stretch) = best.ok_or("no entrance of Goma Cave has open ground to walk")?;
        Ok((place, stretch))
    }

    /// Walks the leader `legs` times from one end of `stretch` to the other
    /// with the d-pad (B held), each leg until he reaches its end, keeping
    /// every frame; `dark` keeps the most of the 32 by 32 pixels around him
    /// that were black in any frame. Returns the frame a battle began, if
    /// one did. The game alone moves him, and every frame his footing is
    /// checked against what the game allows (Guide::check_footing): the walk
    /// fails rather than show him anywhere he could not stand. `each` sees
    /// the field after every frame.
    pub(super) fn walk_stretch(field: &mut Field, stretch: &Stretch, legs: u32, frames: &mut Vec<Vec<u16>>, dark: &mut u32, each: &mut dyn FnMut(&Field)) -> Result<Option<usize>, String> {
        let mut way = 1;
        let mut last = field.leader().ok_or("the scene has no leader")?;
        for _ in 0..legs {
            field.hold(stretch.keys[way] | KEY_B);
            let mut still = 0;
            for _ in 0..400 {
                field.frames(1)?;
                let picture = field.picture();
                each(field);
                if field.scene() == MAP_BATTLE {
                    frames.push(picture);
                    field.hold(0);
                    return Ok(Some(frames.len()));
                }
                let now = field.leader().ok_or("the leader is gone")?;
                Self::check_footing(field, stretch, &last, &now)?;
                let (vx, vy) = field.view();
                let (sx, sy) = (now.x - vx, now.z - vy - 16);
                let black = (sy - 16..sy + 16)
                    .flat_map(|y| (sx - 16..sx + 16).map(move |x| (x, y)))
                    .filter(|&(x, y)| (0..WIDTH as i32).contains(&x) && (0..HEIGHT as i32).contains(&y))
                    .filter(|&(x, y)| { let c = picture[y as usize * WIDTH + x as usize]; (0..3).all(|i| (c >> (5 * i)) & 31 < 4) })
                    .count() as u32;
                *dark = (*dark).max(black);
                frames.push(picture);
                still = if (now.x, now.z) == (last.x, last.z) { still + 1 } else { 0 };
                last = now;
                let along = if stretch.across { last.x } else { last.z };
                if (way == 1 && along >= stretch.ends[1]) || (way == 0 && along <= stretch.ends[0]) {
                    break;
                }
                // Turning round takes the game a few frames; standing longer
                // means something stopped him short of the end.
                if still >= 12 {
                    return Err(format!("the leader stopped at ({}, {}) short of the stretch's end", last.x, last.z));
                }
            }
            way = 1 - way;
        }
        field.hold(0);
        Ok(None)
    }

    /// Whether the leader's step from `last` to `now` is one the game lets
    /// him take along `stretch`, else why not: he stays on its line (the
    /// game turns him aside only when something blocks his way), moves no
    /// more than a running step, stands on a cell of his layer that is no
    /// wall (CELL_BEHAVIOR_WALL), and climbs or drops no more than
    /// MapGrid_CheckCollision's 8 and 12 pixels.
    fn check_footing(field: &mut Field, stretch: &Stretch, last: &Actor, now: &Actor) -> Result<(), String> {
        let at = format!("at ({}, {}) on layer {}", now.x, now.z, now.layer);
        let (line, was) = if stretch.across { (now.z, last.z) } else { (now.x, last.x) };
        if line != was {
            return Err(format!("the leader was turned aside {at}"));
        }
        if (now.x - last.x).abs() + (now.z - last.z).abs() > 4 {
            return Err(format!("the leader jumped from ({}, {}) {at}", last.x, last.z));
        }
        let code = field.cell(now.layer, now.x >> 4, now.z >> 4);
        if code < 0 || code == CELL_WALL {
            return Err(format!("the leader stands in a wall {at}"));
        }
        if !(-STEP_DOWN..=STEP_UP).contains(&(now.y - last.y)) {
            return Err(format!("the leader's height went from {} to {} {at}", last.y >> 16, now.y >> 16));
        }
        let ground = field.ground(now.layer, now.x, now.z)?;
        if (now.y - ground).abs() > 1 << 16 {
            return Err(format!("the leader is {} pixels off the ground {at}", (now.y - ground) >> 16));
        }
        Ok(())
    }

    /// The straight run of ground through the leader's place that the game
    /// lets him walk, across (x) or down (z): each way held with the d-pad
    /// (B held) from where he stands until the game stops him or turns him
    /// off the line, every frame checked as Guide::walk_stretch checks it,
    /// and the ends STRETCH_MARGIN short of where he got to, so the walk
    /// never runs into what stopped him. The field is put back as it was.
    fn measure_stretch(field: &mut Field, across: bool) -> Result<Stretch, String> {
        let saved = field.save();
        let scene = field.scene();
        let start = field.leader().ok_or("the scene has no leader")?;
        let keys = if across { [KEY_LEFT, KEY_RIGHT] } else { [KEY_UP, KEY_DOWN] };
        let along = |a: &Actor| if across { a.x } else { a.z };
        // Open both ways while measuring: the check is of the line and the
        // footing, not of where the stretch ends.
        let open = Stretch { across, ends: [i32::MIN, i32::MAX], keys };
        let mut ends = [along(&start); 2];
        for way in 0..2 {
            field.load(&saved);
            field.hold(keys[way] | KEY_B);
            let mut last = start;
            let mut still = 0;
            for _ in 0..400 {
                field.frames(1)?;
                if field.scene() != scene {
                    break;
                }
                let Some(now) = field.leader() else { break };
                if Self::check_footing(field, &open, &last, &now).is_err() {
                    break;
                }
                still = if (now.x, now.z) == (last.x, last.z) { still + 1 } else { 0 };
                last = now;
                ends[way] = along(&last);
                if still >= 6 {
                    break;
                }
            }
        }
        field.hold(0);
        field.load(&saved);
        Ok(Stretch { across, ends: [ends[0] + STRETCH_MARGIN, ends[1] - STRETCH_MARGIN], keys })
    }

    /// The world map's still picture: R, the cursor moved over the map, L
    /// held to bring it back to the party, and A closing it, as `world-map-picture.gif`.
    pub fn world_map_picture(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with(WORLD_MAP, &[])?;
        let path = [(0, 20), (KEY_R, 3), (0, 90), (KEY_RIGHT, 50), (0, 40), (KEY_UP, 40), (0, 40), (KEY_L, 40), (0, 40), (KEY_A, 3), (0, 60)];
        let frames = Self::press_path_cleared(&mut entered.field, &path)?;
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

/// A straight run of walkable ground (Guide::measure_stretch): along x or z,
/// its two ends in pixels, and the keys that walk toward each.
#[derive(Clone, Copy)]
pub(super) struct Stretch {
    pub(super) across: bool,
    pub(super) ends: [i32; 2],
    pub(super) keys: [u32; 2],
}

/// Goma Cave's rooms: the pond, the pillar room, the stream and the bridge,
/// gScenes' rows from SceneId_GomaCavePond to SceneId_GomaCaveBridge.
const GOMA_CAVE_ROOMS: (&str, &str) = ("GomaCavePond", "GomaCaveBridge");
/// The most of the 32 by 32 pixels around the leader that may be black as a
/// random battle is walked into: an eighth.
const DARK_LIMIT: u32 = 128;
/// How far short of where the game stops the leader a stretch ends
/// (Guide::measure_stretch), pixels.
const STRETCH_MARGIN: i32 = 8;

/// The field's actor slots (FIELD_ACTOR_COUNT in field.rs).
pub(super) const ACTOR_SLOTS: u16 = 66;
/// SceneId_IncreaseItemAndLevel and SceneId_MenuTest, the debug level room
/// and menu test room (scenes.s).
const LEVEL_ROOM: u16 = 199;
const MENU_TEST_ROOM: u16 = 200;
/// The level room's placed actors, ACTOR_FIRST_PLACED on (enum
/// IncreaseItemAndLevelActorId, debug/level.c); outside Japanese all seven are placed.
const LEVEL_ROOM_ACTORS: std::ops::Range<u16> = 8..15;
/// Facing west, in 1/65536 turns (NORTH is a quarter turn short of a whole).
const WEST: u32 = 0x8000;

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
pub(super) fn label(image: &mut Image, x: i32, y: i32, n: u16, colour: u32) {
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
