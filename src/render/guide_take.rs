//! Pictures that show a layer or a memory beside the screen it makes, from
//! the game's own memory: the title intro and the Venus Lighthouse eruption
//! pulled apart into their layers, and Vale's tile animation beside the
//! tiles it rewrites.

use super::animation;
use super::field::{Field, Image};
use super::guide::Guide;
use super::guide_more::{canvas, outline, paste, screen};
use super::mgba::WIDTH;

/// DISPCNT and its layer bits: 8 to 11 turn BG0 to BG3 on, 12 the sprites.
const DISPCNT: u32 = 0x0400_0000;
const LAYER_BITS: u16 = 0x1f00;
/// What a layer shown alone is drawn over where it is empty.
const EMPTY: u16 = 0x0c63;

/// The colour the marks are drawn in.
const RED: u32 = 0xff_30_30;

/// Vale entered by day (scene 4, entrance 1) and Garet's sister there.
const VALE_SCENE: (u16, u16) = (4, 1);
const GARETS_SISTER_ACTOR: u16 = 18;
/// The Lighthouse Aerie (SceneId_VenusLighthouseAerie) after the second
/// battle (VENUS_LIGHTHOUSE_AERIE_AFTER_SECOND_BATTLE).
const VENUS_LIGHTHOUSE_AERIE: (u16, u16) = (187, 3);

impl Guide<'_> {
    /// The address of `name` in the edition's linker map.
    pub(super) fn sym(&self, name: &str) -> Result<u32, String> {
        self.map.symbol(name).map(|(_, a)| a & !1).ok_or_else(|| format!("the map has no {name}"))
    }
}

/// The moment `field` is at with only the layers in `bits` (DISPCNT bits 8
/// to 12) shown, over EMPTY; the field is put back as it was.
pub(super) fn alone(field: &mut Field, bits: u16) -> Result<Image, String> {
    let saved = field.save();
    let shown = field.read16(DISPCNT);
    field.io_write16(DISPCNT, (shown & !LAYER_BITS) | bits);
    field.set_backdrop(EMPTY);
    field.frames(1)?;
    let image = screen(&field.picture());
    field.clear_backdrop();
    field.resume(&saved);
    Ok(image)
}

/// A part of `image`: the rectangle (x, y, w, h), clipped.
pub(super) fn crop(image: &Image, x: i32, y: i32, w: usize, h: usize) -> Image {
    let mut out = canvas(w, h, 0);
    for dy in 0..h as i32 {
        for dx in 0..w as i32 {
            let (sx, sy) = (x + dx, y + dy);
            if sx >= 0 && sy >= 0 && (sx as usize) < image.width && (sy as usize) < image.height {
                out.pixels[dy as usize * w + dx as usize] = image.pixels[sy as usize * image.width + sx as usize];
            }
        }
    }
    out
}

impl Guide<'_> {
    /// Venus Lighthouse's beacon erupting, played from the aerie's entrance
    /// after the second battle with A pressed through the lines: from a
    /// second before the beam stacks its five segments (FLAG_SCENE_TEMP_36)
    /// for eight seconds, the game alone with nothing drawn over it, as
    /// `venus-lighthouse-aerie-eruption.gif`;
    /// and the moment five and a half seconds in, the split open and the
    /// colours back, pulled apart into its layers, as
    /// `venus-lighthouse-aerie-layers-3d.gif`.
    pub fn venus_lighthouse_aerie_eruption(&mut self) -> Result<(), String> {
        let stacked = self.flag("FLAG_SCENE_TEMP_36")?;
        let set = vec![self.flag("FLAG_VALE_PARTY_LEFT")?];
        let clear = vec![self.flag("FLAG_VALE_STORM")?, self.flag("FLAG_CHILDHOOD")?];
        let mut field = Field::enter(self.rom, self.symbols, &super::field::Setup { scene: VENUS_LIGHTHOUSE_AERIE.0, entrance: VENUS_LIGHTHOUSE_AERIE.1, set, clear })?;
        let scene = field.scene();
        let mut kept: std::collections::VecDeque<Vec<u32>> = std::collections::VecDeque::new();
        let mut frames = Vec::new();
        let mut since: Option<u32> = None;
        for _ in 0..12000 {
            field.frames(1)?;
            if field.waiting() {
                field.press(3)?;
            }
            if field.scene() != scene {
                break;
            }
            let image = screen(&field.picture());
            match since {
                None if field.flag_is_set(stacked) => {
                    since = Some(0);
                    frames.extend(kept.drain(..));
                    frames.push(image.pixels);
                }
                None => {
                    kept.push_back(image.pixels);
                    if kept.len() > 60 {
                        kept.pop_front();
                    }
                }
                Some(n) => {
                    frames.push(image.pixels);
                    since = Some(n + 1);
                    if n + 1 == 330 {
                        self.explode_now(&mut field, "venus-lighthouse-aerie-layers-3d.gif", "Venus Lighthouse's beacon erupting")?;
                    }
                    if n > 480 {
                        break;
                    }
                }
            }
        }
        if since.is_none() {
            return Err("the beacon never erupted".to_string());
        }
        let written = animation::write(&self.out.join("venus-lighthouse-aerie-eruption.gif"), &frames, WIDTH)?;
        self.written.push(("venus-lighthouse-aerie-eruption.gif".to_string(), written.bytes, format!("Venus Lighthouse's beacon erupting; {} frames", written.frames)));
        Ok(())
    }

    /// The title intro as it plays after the prologue (the title scene's
    /// entrance 0, Title_ShowIntro without its prompt), 140 frames after the
    /// screen first shows anything, while the backgrounds scroll down and the
    /// sprites rise: pulled apart into its layers, as
    /// `title-intro-layers-3d.gif`.
    pub fn title_intro_exploded(&mut self) -> Result<(), String> {
        let setup = super::field::Setup { scene: 0, entrance: 0, set: Vec::new(), clear: Vec::new() };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        let mut n = 0;
        while !field.picture().iter().any(|&c| c & 0x7fff != 0) {
            field.frames(1)?;
            n += 1;
            if n > 2000 {
                return Err("the intro never showed".to_string());
            }
        }
        field.frames(140)?;
        self.explode_now(&mut field, "title-intro-layers-3d.gif", "the title intro")
    }

    /// Vale where Garet's sister stands, beside the ground layer's character
    /// block (the 512 tiles BG3CNT points at, 32 to a row, each tile at its
    /// own 8 by 8 pixels in grey by colour number), every tile the tile
    /// animation rewrites outlined in red for the 8 frames after it changes,
    /// over four seconds, as `tile-animation.gif`.
    pub fn tile_animation(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.stand_for(VALE_SCENE, super::story::Trigger::Talk(GARETS_SISTER_ACTOR), GARETS_SISTER_ACTOR)?;
        let field = &mut entered.field;
        let control = field.read16(0x0400_000e);
        let base = 0x0600_0000 + u32::from((control >> 2) & 3) * 0x4000;
        let grey: Vec<u32> = (0..16).map(|i| { let v = i * 17; v << 16 | v << 8 | v }).collect();
        let (w, h) = (WIDTH + 8 + 256, 160usize.max(128));
        let mut last: Vec<Vec<u8>> = Vec::new();
        let mut marked = vec![0u32; 512];
        let mut frames = Vec::new();
        for _ in 0..240 {
            field.frames(1)?;
            let mut c = canvas(w, h, 0x20_20_28);
            paste(&mut c, &screen(&field.picture()), 0, 0, 1);
            let mut tiles = Vec::new();
            for t in 0..512u32 {
                let bytes: Vec<u8> = (0..32).map(|k| field.read8(base + t * 32 + k)).collect();
                if last.get(t as usize).is_some_and(|b| *b != bytes) {
                    marked[t as usize] = 8;
                }
                tiles.push(bytes);
                let pixels = super::guide_more::tile(field, base + t * 32, false, &grey, 0);
                super::guide_more::put_tile(&mut c, WIDTH + 8 + (t as usize % 32) * 8, 16 + (t as usize / 32) * 8, &pixels, false, false);
            }
            for (t, m) in marked.iter_mut().enumerate() {
                if *m > 0 {
                    outline(&mut c, (WIDTH + 8 + (t % 32) * 8) as i32, (16 + (t / 32) * 8) as i32, 8, 8, RED);
                    *m -= 1;
                }
            }
            last = tiles;
            frames.push(c.pixels);
        }
        let written = animation::write(&self.out.join("tile-animation.gif"), &frames, w)?;
        self.written.push(("tile-animation.gif".to_string(), written.bytes, format!("Vale beside its ground layer's tiles as the tile animation rewrites them; {} frames", written.frames)));
        Ok(())
    }
}
