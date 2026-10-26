//! Lighthouse Aerie's whole story in the Doll House recorder (whole.rs): one
//! GIF of the aerie's map and the sky in every frame, from the platform
//! carrying Isaac up onto the aerie to the party taken to Lalivero after
//! Sheba's fall and Felix's leap. The game is played as the player plays it,
//! pages answered with A and the leader walked across the floating platforms
//! to the beacon; the two battles are not the aerie, so each run ends the
//! frame before its battle and the next starts where the game sends the party
//! back (a new run entered there, carrying the flags the run before set), and
//! the trips to the world map are played through and left out. The message
//! window and portraits are left out, as the recorder leaves them, and a
//! frame of the aerie's quakes whose views leave a gap is drawn again from
//! views reaching further (whole::Run::wide). Frames the game draws line by
//! line (its screen transitions) are left out too, as no whole picture shows
//! them.

use super::field::{Field, Setup};
use super::guide::Guide;
use super::whole::{self, Run, States, LATE, LEAD};

/// The Lighthouse Aerie (SceneId_VenusLighthouseAerie), the world map
/// (SceneId_WorldMap) and a battle (MAP_BATTLE).
const AERIE: u16 = 187;
const WORLD_MAP: u16 = 2;
const MAP_BATTLE: u16 = 0x1fe;
/// Where each run starts: up from the fourth floor
/// (VENUS_LIGHTHOUSE_AERIE_FROM_VENUS_LIGHTHOUSE_FOURTH_FLOOR), back from the
/// first battle (_AFTER_FIRST_BATTLE) and back from the second
/// (_AFTER_SECOND_BATTLE), which plays on through both trips to the world map
/// (_AFTER_BEACON_LIT, _FROM_WORLD_MAP) to the exit to Lalivero.
const FROM_FOURTH_FLOOR: u16 = 1;
const AFTER_FIRST_BATTLE: u16 = 2;
const AFTER_SECOND_BATTLE: u16 = 3;
/// The runs: where each starts, and the scene that ends it (MAP_BATTLE, or 0
/// for a scene that is neither the aerie nor the world map).
const RUNS: [(u16, u16); 3] = [(FROM_FOURTH_FLOOR, MAP_BATTLE), (AFTER_FIRST_BATTLE, MAP_BATTLE), (AFTER_SECOND_BATTLE, 0)];
/// The leader's walk from where the arrival leaves him (491, 164) to the
/// beacon, in pixels, each point reached by holding one direction: south onto
/// the floor by the platforms (MB_VENUS_LIGHTHOUSE_AERIE_FLOOR_BY_PLATFORMS),
/// which jumps him onto the first platform, west from platform to platform
/// onto the lighthouse's floor by them, up its stair to the north, west and
/// south round the raised beacon, and west onto the cells before it
/// (MB_VENUS_LIGHTHOUSE_AERIE_BEFORE_BEACON), where the confrontation starts.
const WALK: [(i32, i32); 8] = [(504, 200), (472, 200), (440, 200), (408, 200), (408, 136), (376, 136), (376, 232), (344, 232)];
/// Pixels from a point at which it counts as reached.
const NEAR: i32 = 2;
/// KEYINPUT bits.
const KEY_A: u32 = 0x1;
const KEY_RIGHT: u32 = 0x10;
const KEY_LEFT: u32 = 0x20;
const KEY_UP: u32 = 0x40;
const KEY_DOWN: u32 = 0x80;
/// Frames a page or a choice waits before A answers it (the Doll House
/// recorder leaves the window out, so a page waits no longer than a
/// glance), and the frames A is held.
const PAGE_WAIT: u32 = 10;
const PRESS: u32 = 3;
/// Frames the leader may stand still while walked, and a run's frames at
/// most.
const STUCK: u32 = 180;
const RUN_LIMIT: usize = 30000;
/// I/O registers and bits line_effect reads.
const DISPCNT: u32 = 0x0400_0000;
const DISPCNT_WINDOWS: u16 = 0xe000;
const REG_IE: u32 = 0x0400_0200;
const IRQ_HBLANK: u16 = 0x0002;
const REG_DMA0CNT_H: u32 = 0x0400_00ba;
const DMA_ENABLE: u16 = 0x8000;
const DMA_TIMING: u16 = 0x3000;
const DMA_TIMING_HBLANK: u16 = 0x2000;
/// The flags a run carries into the next: the story flags (0x000 to 0x1ff)
/// and the map section flags (0x300 to 0x3ff), which a battle leaves as they
/// were (FLAG_SHAMANS_ROD_HOLDER, FLAG_AERIE_FIRST_BATTLE_BEGUN), but not the
/// scene flags (0x200 to 0x2ff), which entering the scene clears.
const CARRIED_FLAGS: [std::ops::Range<u16>; 2] = [0x000..0x200, 0x300..0x400];

/// One run of the game: its field, the state at the end of each frame, and
/// for each frame whether it shows the aerie lit.
struct Played<'a> {
    field: Field<'a>,
    states: States,
    shown: Vec<bool>,
}

impl<'a> Guide<'a> {
    /// The aerie's story in the Doll House recorder, as
    /// `places/venus-lighthouse-aerie/story-dollhouse.gif`: every second
    /// frame the aerie shows, its map whole with the sky, from the arrival to
    /// the exit to Lalivero, cut straight from the frame before each battle,
    /// and from the last frame before each trip to the world map, to the
    /// first the aerie shows again. The message window and portraits are left
    /// out, as the recorder leaves them.
    pub(super) fn venus_story_dollhouse(&mut self) -> Result<(), String> {
        let mut carried = Vec::new();
        let mut runs = Vec::new();
        for (entrance, ends) in RUNS {
            let played = self.play_aerie(entrance, &carried, ends)?;
            carried = carried_flags_set(&played.field, &carried);
            runs.push(played);
        }
        let frames: Vec<Vec<usize>> = runs.iter().map(|p| shown_frames(&p.shown)).collect();
        if std::env::var_os("ALCHEMY_RENDER_DEBUG").is_some() {
            for (p, f) in runs.iter().zip(&frames) {
                eprintln!("run: {} frames played, {} drawn, {:?} to {:?}", p.shown.len(), f.len(), f.first(), f.last());
            }
        }
        // The aerie's scroll bounds as its last frame drawn has them.
        let last = runs.last_mut().zip(frames.last().and_then(|f| f.last())).ok_or("the last run showed nothing")?;
        last.0.field.load(&last.0.states.get(*last.1));
        let area = last.0.field.bounds().map(|v| v >> 16);
        let mut parts: Vec<Run> = runs.iter_mut().zip(&frames).map(|(p, f)| Run { field: &mut p.field, states: &p.states, frames: f, wide: true }).collect();
        let mut pictures = whole::stitch_runs(&mut parts, area)?;
        // The exit fades the aerie to black before the scene changes: the
        // GIF ends on its last picture that is not all one colour.
        while pictures.last().is_some_and(|p| p.pixels.iter().all(|&c| c == p.pixels[0])) {
            pictures.pop();
        }
        self.whole_gif("places/venus-lighthouse-aerie/story-dollhouse.gif", &pictures, "the Lighthouse Aerie's story in the Doll House recorder, the whole aerie and the sky")
    }

    /// Enters the aerie at `entrance` with the story flags `set` besides the
    /// party having left Vale, and plays it, keeping every frame's state,
    /// until a battle begins (`ends` MAP_BATTLE) or, with `ends` 0, until the
    /// party leaves for a scene that is neither the aerie nor the world map.
    fn play_aerie(&self, entrance: u16, set: &[u16], ends: u16) -> Result<Played<'a>, String> {
        let mut flags = vec![self.flag("FLAG_VALE_PARTY_LEFT")?];
        flags.extend(set.iter().filter(|f| !flags.contains(f)).copied().collect::<Vec<_>>());
        let clear = vec![self.flag("FLAG_VALE_STORM")?, self.flag("FLAG_CHILDHOOD")?];
        let mut field = Field::enter(self.rom, self.symbols, &Setup { scene: AERIE, entrance, set: flags, clear })?;
        field.keep_states(true);
        let mut walk: &[(i32, i32)] = if entrance == FROM_FOURTH_FLOOR { &WALK } else { &[] };
        let mut shown = Vec::new();
        let (mut waited, mut pressing, mut still) = (0, 0, 0);
        let mut steps = field.steps();
        let mut place = None;
        loop {
            field.frames(1)?;
            let scene = field.scene();
            shown.push(scene == AERIE && lit(&field) && !line_effect(&field));
            let done = if ends == 0 { scene != AERIE && scene != WORLD_MAP && scene != MAP_BATTLE } else { scene == ends };
            if done {
                break;
            }
            if scene == MAP_BATTLE {
                return Err(format!("a battle began where none was expected, entering at {entrance}"));
            }
            if shown.len() > RUN_LIMIT {
                return Err(format!("the run from entrance {entrance} went on past {RUN_LIMIT} frames"));
            }
            let in_control = field.steps() != steps;
            steps = field.steps();
            if pressing > 0 {
                pressing -= 1;
                if pressing == 0 {
                    field.hold(0);
                }
                continue;
            }
            if field.waiting() {
                waited += 1;
                if waited >= PAGE_WAIT {
                    field.hold(KEY_A);
                    pressing = PRESS;
                    waited = 0;
                }
                continue;
            }
            waited = 0;
            let Some(leader) = field.leader().filter(|_| in_control) else {
                field.hold(0);
                continue;
            };
            let Some(&(x, z)) = walk.first() else {
                field.hold(0);
                continue;
            };
            let (dx, dz) = (x - leader.x, z - leader.z);
            if dx.abs() <= NEAR && dz.abs() <= NEAR {
                walk = &walk[1..];
                continue;
            }
            field.hold(match (dx.abs() > dz.abs(), dx > 0, dz > 0) {
                (true, true, _) => KEY_RIGHT,
                (true, false, _) => KEY_LEFT,
                (false, _, true) => KEY_DOWN,
                (false, _, false) => KEY_UP,
            });
            still = if place == Some((leader.x, leader.z)) { still + 1 } else { 0 };
            place = Some((leader.x, leader.z));
            if still > STUCK {
                return Err(format!("the leader stood at ({}, {}) walking to ({x}, {z})", leader.x, leader.z));
            }
        }
        field.hold(0);
        let states = field.keep_states(false).ok_or("no states were kept")?;
        // The last point lies on the cells before the beacon: the
        // confrontation takes the leader over as he steps onto them.
        if walk.len() > 1 {
            return Err(format!("the walk to the beacon stopped short of {:?}", walk[0]));
        }
        Ok(Played { field, states, shown })
    }
}

/// Whether the screen is drawn line by line as a whole picture cannot be:
/// a screen transition's windows (DISPCNT's window bits) or a scanline
/// effect (an HBlank interrupt, or DMA 0 started each HBlank), such as the
/// dissolve the aerie leaves and comes back from the world map with and the
/// window that cuts the arrival's camera from the beacon to the party.
fn line_effect(field: &Field) -> bool {
    let windows = field.read16(DISPCNT) & DISPCNT_WINDOWS != 0;
    let hblank_irq = field.read16(REG_IE) & IRQ_HBLANK != 0;
    let dma0 = field.read16(REG_DMA0CNT_H);
    windows || hblank_irq || (dma0 & DMA_ENABLE != 0 && dma0 & DMA_TIMING == DMA_TIMING_HBLANK)
}

/// Whether the screen shows more than one colour: not the black or white of
/// a fade at its full.
fn lit(field: &Field) -> bool {
    let picture = field.picture();
    picture.iter().any(|&c| c != picture[0])
}

/// The frames drawn from a run: every second frame of each stretch the
/// aerie shows, from its first frame lit to its last, the first leaving the
/// LEAD and LATE frames each view is pinned from and run through before it on
/// the aerie.
fn shown_frames(shown: &[bool]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < shown.len() {
        if !shown[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < shown.len() && shown[i] {
            i += 1;
        }
        out.extend((start + LEAD + LATE..i).step_by(2));
    }
    out
}

/// The carried flags set now, `before` among them.
fn carried_flags_set(field: &Field, before: &[u16]) -> Vec<u16> {
    let mut out = before.to_vec();
    out.extend(CARRIED_FLAGS.iter().cloned().flatten().filter(|&f| field.flag_is_set(f) && !before.contains(&f)));
    out
}

#[cfg(test)]
mod tests {
    use super::{shown_frames, LATE, LEAD};

    /// Each stretch the aerie shows is drawn every second frame, after the
    /// frames its views need before it.
    #[test]
    fn stretches_are_drawn_every_second_frame() {
        let first = LEAD + LATE;
        let mut shown = vec![false; 3];
        shown.extend(vec![true; first + 5]);
        shown.extend(vec![false; 4]);
        shown.extend(vec![true; first + 2]);
        let second = 3 + first + 5 + 4;
        assert_eq!(shown_frames(&shown), vec![3 + first, 3 + first + 2, 3 + first + 4, second + first]);
    }
}
