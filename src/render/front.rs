//! `render story`: pictures for The Broken Seal's front page, each played
//! from a built edition as the game shows it: the title's logo card, Vale's
//! farewell to Isaac and Garet, the world map cut after Venus Lighthouse is
//! lit, and the whole world map laid flat.

use super::animation;
use super::field::{Field, Image, Setup, Symbols};
use super::mgba::{HEIGHT, WIDTH};
use super::places::write_png;
use super::story::Flags;
use std::path::Path;

/// The scenes and entrances the pictures start from (places.tsv): the
/// title's entrance 10 shows the logo card after the ending; ValeHome's 12
/// runs Event_DepartForQuest; the world map's 91 plays
/// WorldMap_PlayLighthouseTransition(1) after Venus Lighthouse's beacon
/// erupts.
const TITLE: (u16, u16) = (0, 3);
const VALE_HOME: (u16, u16) = (5, 12);
const WORLD_MAP: (u16, u16) = (2, 91);

/// Frames a page is held before A, so a GIF shows it.
const PAGE_HOLD: u32 = 45;

pub struct Front<'a> {
    pub rom: &'a [u8],
    pub symbols: &'a Symbols,
    pub flags: &'a Flags,
    pub out: &'a Path,
    pub written: Vec<(String, u64, String)>,
}

/// What a recording keeps of each frame.
struct Frame {
    picture: Vec<u16>,
    message: u32,
    scene: u16,
}

fn is_black(picture: &[u16]) -> bool {
    picture.iter().all(|&c| c & 0x7fff == 0)
}

fn image(picture: &[u16]) -> Image {
    Image { width: WIDTH, height: HEIGHT, pixels: picture.iter().map(|&c| animation::rgb(c)).collect() }
}

impl Front<'_> {
    fn flag(&self, name: &str) -> Result<u16, String> {
        self.flags.resolve(name, None).ok_or_else(|| format!("flags.tsv names no {name}"))
    }

    fn png(&mut self, file: &str, picture: &Image, what: &str) -> Result<(), String> {
        let bytes = write_png(&self.out.join(file), picture)?;
        self.written.push((file.to_string(), bytes, what.to_string()));
        Ok(())
    }

    fn gif(&mut self, file: &str, frames: &[&Frame], what: &str) -> Result<(), String> {
        let rgb: Vec<Vec<u32>> = frames.iter().map(|f| f.picture.iter().map(|&c| animation::rgb(c)).collect()).collect();
        let written = animation::write(&self.out.join(file), &rgb, WIDTH)?;
        self.written.push((file.to_string(), written.bytes, format!("{what}; {} frames at 30 a second", written.frames)));
        Ok(())
    }

    /// Runs `frames` frames, keeping each picture, holding each waiting page
    /// PAGE_HOLD frames before A; stops early when `stop` says so.
    fn record(&self, field: &mut Field, frames: u32, stop: &dyn Fn(&Field) -> bool) -> Result<Vec<Frame>, String> {
        let mut out = Vec::new();
        let mut waited = 0;
        for _ in 0..frames {
            field.frames(1)?;
            out.push(Frame { picture: field.picture(), message: field.last_message(), scene: field.scene() });
            waited = if field.waiting() { waited + 1 } else { 0 };
            if waited >= PAGE_HOLD {
                field.press(3)?;
                for _ in 0..3 {
                    out.push(Frame { picture: field.picture(), message: field.last_message(), scene: field.scene() });
                }
                waited = 0;
            }
            if stop(field) {
                break;
            }
        }
        Ok(out)
    }

    /// The title screen as the game shows it after the prologue (the title
    /// overlay's entrances other than 2, 9 and 10): the sun rising over
    /// Mt. Aleph, then the Golden Sun logo, without the "Press Start" prompt
    /// the power-on title adds; then it fades to Vale.
    pub fn title(&mut self) -> Result<(), String> {
        let setup = Setup { scene: TITLE.0, entrance: TITLE.1, set: Vec::new(), clear: Vec::new() };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        let scene = setup.scene;
        let frames = self.record(&mut field, 3000, &|f: &Field| f.scene() != scene)?;
        let shown: Vec<&Frame> = frames.iter().filter(|f| f.scene == scene).collect();
        let first = shown.iter().position(|f| !is_black(&f.picture)).ok_or("the title stayed black")?;
        // The logo held is the longest steady stretch: the GIF ends with it,
        // before the fade towards Vale, and the still is its middle.
        let held = steadiest(&shown);
        let end = held.end.max(first + 1);
        let still = (held.start + held.end) / 2;
        self.gif("title.gif", &shown[first..end], "the sun rising over Mt. Aleph, then the Golden Sun logo held")?;
        self.png("title.png", &image(&shown[still].picture), "the Golden Sun logo over the sky, no prompt")?;
        Ok(())
    }

    /// Vale's send-off (Event_DepartForQuest, ValeHome's entrance 12, with
    /// FLAG_REACHED_KALAY clear): Isaac and Garet hear everyone is at the
    /// gate, walk to the crowd, and the town calls out "Farewell!". The GIF
    /// runs from message `first` to the hold of message `last`; the still is
    /// the crowd just before message `crowd`.
    pub fn farewell(&mut self, first: u32, crowd: u32, last: u32) -> Result<(), String> {
        let setup = Setup {
            scene: VALE_HOME.0,
            entrance: VALE_HOME.1,
            set: Vec::new(),
            clear: vec![self.flag("FLAG_REACHED_KALAY")?, self.flag("FLAG_VALE_STORM")?, self.flag("FLAG_CHILDHOOD")?],
        };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        field.set_party(&[0, 1]);
        let scene = setup.scene;
        let frames = self.record(&mut field, 20000, &|f: &Field| f.scene() != scene)?;
        let at = |m: u32| frames.iter().position(|f| f.message == m).ok_or(format!("message {m:05} never showed"));
        let start = at(first)?;
        let last_shown = frames.iter().rposition(|f| f.message == last).ok_or(format!("message {last:05} never showed"))?;
        let end = (last_shown + PAGE_HOLD as usize).min(frames.len());
        let clip: Vec<&Frame> = frames[start..end].iter().filter(|f| f.scene == scene).collect();
        self.gif("vale-farewell.gif", &clip, &format!("Event_DepartForQuest from message {first:05} to {last:05}"))?;
        let gathered = at(crowd)?.saturating_sub(2);
        self.png("vale-farewell.png", &image(&frames[gathered].picture), &format!("the town gathered, before message {crowd:05}"))?;
        Ok(())
    }

    /// The world map cut at entrance 91, after Venus Lighthouse's beacon
    /// erupts: WorldMap_PlayLighthouseTransition(1), until the scene goes on
    /// to the aerie's entrance 93.
    pub fn lighthouse(&mut self) -> Result<(), String> {
        let set = ["FLAG_VENUS_LIGHTHOUSE_LIT", "FLAG_MERCURY_LIGHTHOUSE_LIT", "FLAG_PARTY_LEFT_VALE"]
            .iter()
            .map(|n| self.flag(n))
            .collect::<Result<Vec<u16>, String>>()?;
        let setup = Setup { scene: WORLD_MAP.0, entrance: WORLD_MAP.1, set, clear: Vec::new() };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        field.set_party(&[0, 1, 2, 3]);
        let scene = setup.scene;
        let frames = self.record(&mut field, 1500, &|f: &Field| f.scene() != scene)?;
        let start = frames.iter().position(|f| !is_black(&f.picture)).ok_or("the cut stayed black")?;
        let end = frames.iter().rposition(|f| !is_black(&f.picture) && f.scene == scene).ok_or("the cut stayed black")? + 1;
        let clip: Vec<&Frame> = frames[start..end].iter().collect();
        self.gif("venus-lighthouse-lit.gif", &clip, "the world map cut at entrance 91")?;
        // The still: the frame with the most of the beacon's warm light.
        let still = clip.iter().max_by_key(|f| warmth(&f.picture)).ok_or("the cut is empty")?;
        self.png("venus-lighthouse-lit.png", &image(&still.picture), "the beacon's flare held at the lighthouse's top")?;
        Ok(())
    }
}

/// An affine background plane drawn flat from VRAM: its map (a byte a
/// tile), its 256-colour tiles and the BG palette.
pub fn affine_plane(field: &Field, bgcnt: u16) -> (Image, Vec<u8>) {
    let tiles = 16usize << ((bgcnt >> 14) & 3);
    let char_base = 0x0600_0000 + u32::from((bgcnt >> 2) & 3) * 0x4000;
    let screen_base = 0x0600_0000 + u32::from((bgcnt >> 8) & 31) * 0x800;
    let palette: Vec<u32> = (0..256u32).map(|i| animation::rgb(field.read16(0x0500_0000 + 2 * i))).collect();
    let size = tiles * 8;
    let mut pixels = vec![0u32; size * size];
    let mut indices = vec![0u8; size * size];
    for ty in 0..tiles {
        for tx in 0..tiles {
            let tile = u32::from(field.read8(screen_base + (ty * tiles + tx) as u32));
            for y in 0..8 {
                for x in 0..8 {
                    let index = field.read8(char_base + tile * 64 + (y * 8 + x) as u32);
                    pixels[(ty * 8 + y) * size + tx * 8 + x] = palette[usize::from(index)];
                    indices[(ty * 8 + y) * size + tx * 8 + x] = index;
                }
            }
        }
    }
    (Image { width: size, height: size, pixels }, indices)
}


/// The world map's grid: 16 by 16 squares, each 32 by 32 tiles of its
/// affine planes (256 pixels), two squares across a plane.
const SQUARES: usize = 16;
const SQUARE: usize = 256;
/// PerspectiveWork.tileIds: the square drawn at each grid place.
const TILE_IDS: u32 = 0x138;

/// The share of a square's pixels that are near white: a square drawn with
/// the other graphics set's tiles shows as flat white.
fn whiteness(canvas: &[u32], size: usize, sx: usize, sz: usize) -> usize {
    let mut n = 0;
    for y in 0..SQUARE {
        for x in 0..SQUARE {
            let c = canvas[(sz * SQUARE + y) * size + sx * SQUARE + x];
            if (c >> 16) & 0xff > 0xe0 && (c >> 8) & 0xff > 0xe0 && c & 0xff > 0xe0 {
                n += 1;
            }
        }
    }
    n
}

impl Front<'_> {
    /// The world map drawn flat from its own planes: for every 2 by 2
    /// squares the followed point is moved there and Map_UpdateCurrentTileBlock
    /// draws them into BG3 (the ground) and BG2 (mountains, woods and towns
    /// over it); the planes are read from VRAM and laid on one canvas, BG2 over
    /// BG3. The world map loads one of two graphics sets by where the party
    /// stands (WorldMap_LoadGraphics: the second for terrain kind 21, the snow
    /// by Mercury Lighthouse), so the map is drawn from a run in each, and
    /// each square takes the run that draws it with the less flat white.
    pub fn world(&mut self, update: u32) -> Result<(), String> {
        let size = SQUARES * SQUARE;
        let mut canvases = Vec::new();
        let mut ids: Vec<Vec<u16>> = Vec::new();
        // Entrance 1 (leaving Vale), and the same run moved near Mercury
        // Lighthouse.
        for at in [None, Some((5800, 500))] {
            let mut canvas = vec![0u32; size * size];
            let setup = Setup { scene: WORLD_MAP.0, entrance: 1, set: vec![self.flag("FLAG_PARTY_LEFT_VALE")?], clear: Vec::new() };
            let mut field = Field::enter(self.rom, self.symbols, &setup)?;
            if let Some((x, z)) = at {
                field.set_position(x, z);
            }
            field.until_control()?;
            field.frames(10)?;
            field.freeze(true);
            let work = field.map_work();
            ids.push((0..256).map(|i| field.read16(work + TILE_IDS + 2 * i)).collect());
            for sz in (0..SQUARES).step_by(2) {
                for sx in (0..SQUARES).step_by(2) {
                    // The 2 by 2 squares start 256 pixels left of and 320
                    // above the point, in 512-pixel world steps.
                    let origin = [((sx * 512 + 256 + 8) << 16) as i32, ((sz * 512 + 320 + 8) << 16) as i32];
                    field.call(update, origin)?;
                    field.frames(1)?;
                    let r = |a: u32| field.read16(0x0400_0000 + a);
                    let (ground, _) = affine_plane(&field, r(0xe));
                    let (over, over_index) = affine_plane(&field, r(0xc));
                    for (x, z) in [(sx, sz), (sx + 1, sz), (sx, sz + 1), (sx + 1, sz + 1)] {
                        let (qx, qz) = ((x & 1) * SQUARE, (z & 1) * SQUARE);
                        for y in 0..SQUARE {
                            for px in 0..SQUARE {
                                let i = (qz + y) * ground.width + qx + px;
                                let c = if over_index[i] != 0 { over.pixels[i] } else { ground.pixels[i] };
                                canvas[(z * SQUARE + y) * size + x * SQUARE + px] = c;
                            }
                        }
                    }
                }
            }
            canvases.push(canvas);
        }
        let mut canvas = canvases[0].clone();
        let mut second = 0;
        for sz in 0..SQUARES {
            for sx in 0..SQUARES {
                // The squares the second set swaps in, and those it draws
                // with less flat white.
                let swapped = ids[1][sz * 16 + sx] != ids[0][sz * 16 + sx];
                if swapped || whiteness(&canvases[1], size, sx, sz) < whiteness(&canvases[0], size, sx, sz) {
                    second += 1;
                    for y in 0..SQUARE {
                        let row = (sz * SQUARE + y) * size + sx * SQUARE;
                        canvas[row..row + SQUARE].copy_from_slice(&canvases[1][row..row + SQUARE]);
                    }
                }
            }
        }
        let image = Image { width: size, height: size, pixels: canvas };
        self.png(
            "world.png",
            &image,
            &format!("the world map laid flat from its two planes, 16 by 16 squares, {second} from the snow's graphics set"),
        )?;
        Ok(())
    }
}

/// Pixels of bright warm light: much red, little blue.
fn warmth(picture: &[u16]) -> usize {
    picture.iter().filter(|&&c| c & 31 >= 26 && (c >> 10) & 31 <= 16).count()
}

fn differ(a: &[u16], b: &[u16]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

/// The longest stretch of frames each nearly the same as the one before
/// (under 2 in 100 pixels changed), as a range.
fn steadiest(frames: &[&Frame]) -> std::ops::Range<usize> {
    let (mut best, mut start) = (0..0, 0);
    for i in 1..=frames.len() {
        let steady = i < frames.len()
            && !is_black(&frames[i].picture)
            && differ(&frames[i - 1].picture, &frames[i].picture) * 50 < WIDTH * HEIGHT;
        if !steady {
            if i - start > best.len() {
                best = start..i;
            }
            start = i;
        }
    }
    best
}
