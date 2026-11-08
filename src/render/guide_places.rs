//! The picture for Sol Sanctum's statue room: the screen beside the layer
//! that makes its drifting sky, from the game's own memory.

use super::animation;
use super::field::Spot;
use super::guide::Guide;
use super::guide_more::{canvas, paste, screen};
use super::guide_take::alone;
use super::mgba::WIDTH;

const WHITE: u32 = 0xff_ff_ff;

/// DISPCNT bits 8 to 12: BG0 to BG3, then the sprites.
const SPRITES: u16 = 0x1000;
const BACKGROUNDS: u16 = 0x0f00;

impl Guide<'_> {
    /// The statue room's three rows of sky sprites drifting behind the
    /// colonnade: the screen, the sprites alone and the backgrounds alone,
    /// for eight seconds, as `statue-sky-rows.gif`.
    pub fn statue_sky_rows(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with((12, 0), &[])?;
        let field = &mut entered.field;
        let placed = field.save();
        let height = field.ground(0, 288, 120)?;
        field.stand(&placed, &Spot { x: 288, z: 120, layer: 0, height, facing: 0xc000, walk: 0, cell: None })?;
        // The top of the screen, where the rows are drawn, twice its size:
        // the screen, the sprites alone and the backgrounds alone, one above
        // another.
        const ROWS: usize = 72;
        let mut frames = Vec::new();
        for i in 0..480 {
            field.frames(1)?;
            if i % 2 == 1 {
                let last = frames.last().cloned().unwrap_or_default();
                frames.push(last);
                continue;
            }
            // The screen a frame on, the moment the layers alone show.
            let now = field.save();
            field.frames(1)?;
            let whole = screen(&field.picture());
            field.resume(&now);
            let shots = [whole, alone(field, SPRITES)?, alone(field, BACKGROUNDS)?];
            let mut out = canvas(2 * WIDTH + 8, 3 * (2 * ROWS + 4) + 4, WHITE);
            for (k, shot) in shots.iter().enumerate() {
                let top = super::guide_take::crop(shot, 0, 0, WIDTH, ROWS);
                paste(&mut out, &top, 4, 4 + k * (2 * ROWS + 4), 2);
            }
            frames.push(out.pixels);
        }
        let written = animation::write(&self.out.join("statue-sky-rows.gif"), &frames, 2 * WIDTH + 8)?;
        self.written.push(("statue-sky-rows.gif".to_string(), written.bytes, format!("the statue room's drifting sky rows: the screen, the sprites, the backgrounds; {} frames", written.frames)));
        Ok(())
    }
}
