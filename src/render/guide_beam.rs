//! Venus Lighthouse's beam seen whole: every frame drawn as the aerie's whole
//! map with the sky above it, rather than the 240 by 160 the camera shows,
//! stitched from views of one moment as whole.rs draws them.

use super::field::{Field, Image, Setup};
use super::guide::Guide;
use super::whole::{self, States, LATE, LEAD};

/// The Lighthouse Aerie (SceneId_VenusLighthouseAerie) back from the first
/// battle (VENUS_LIGHTHOUSE_AERIE_AFTER_FIRST_BATTLE), where
/// VenusLighthouseAerie_CombineSaturosAndMenardi throws the Venus Star into
/// the beacon, and after the second (VENUS_LIGHTHOUSE_AERIE_AFTER_SECOND_BATTLE),
/// where VenusLighthouseAerie_FinishSaturosAndMenardiBattle erupts it.
const AFTER_FIRST_BATTLE: (u16, u16) = (187, 2);
const AFTER_SECOND_BATTLE: (u16, u16) = (187, 3);
/// The beam's lowest segment, which CombineSaturosAndMenardi places over the
/// beacon (x 304) as the star goes in.
const ACTOR_BEAM_BOTTOM: u16 = 24;
const BEAM_X: i32 = 304;
const KEY_A: u32 = 0x1;
const PALETTE_RAM: u32 = 0x0500_0000;

/// The beam after the star. The loop is a whole number of the floating
/// platforms' bobs (64 frames, measured from their heights) and of the third
/// frames the beam's particles spawn on
/// (VenusLighthouseAerie_SpawnUpperOrbitParticles): so many times
/// STAR_LOOP_UNIT, at most STAR_LOOP_UNITS. The frames the colours take to
/// come back after the beam appears, at the least; the frames the loop
/// starts after the game first waits on a page, at the least; and the frames
/// a particle lives (gVenusLighthouseAerie_OrbitParticleScript: 16 risen,
/// then freed), with some to spare.
const STAR_LOOP_UNIT: usize = 192;
const STAR_LOOP_UNITS: usize = 4;
const STAR_COLOURS: u32 = 200;
const STAR_SETTLE: usize = 60;
const PARTICLE_LIFE: usize = 30;
/// Frames looked through for the start, and the last actor whose idle
/// animation is compared (ACTOR_BEAM_TOP), with the offsets read for it:
/// Actor.sprite, Sprite.entries and SpriteAnimation.picture.
const STAR_SEARCH: usize = 3000;
const LAST_ACTOR: u16 = 28;
const OBJ_SPRITE: u32 = 0x50;
const SPRITE_ENTRIES: u32 = 0x28;
const SPRITE_ANIMATION_PICTURE: u32 = 0x16;

/// The eruption: frames kept from before the beam stacks its five segments
/// (FLAG_VENUS_AERIE_BEACON_ERUPTED), the white flash already full, and
/// after, past the three tints and the colours back (360 frames) to the
/// beam closed (gVenusLighthouseAerie_BeamCloseAction, 30) and a moment
/// more.
const ERUPTION_BEFORE: usize = 12;
const ERUPTION_AFTER: usize = 420;
/// FLAG_VENUS_AERIE_BEACON_ERUPTED, FLAG_SCENE_TEMP_36 in
/// map/venus_lighthouse_aerie.c: the beacon has erupted.
const FLAG_VENUS_AERIE_BEACON_ERUPTED: u16 = 0x236;

impl<'a> Guide<'a> {
    /// The beam the Venus Star lights, seen whole, as
    /// `places/venus-lighthouse-aerie/beam-whole.gif`: the aerie back from
    /// the first battle, its pages answered with A until the star has gone
    /// in, the beam has narrowed and the colours are back, and the next page
    /// (Menardi's) left waiting, so the actors stand still but for their idle
    /// animations while the beam's particles circle it and the floating
    /// platforms bob; the window and its letters left out. The loop is a multiple of STAR_LOOP_UNIT frames,
    /// and so that its last frame leads into its first, the random seed
    /// (gRandomSeed) is set, PARTICLE_LIFE frames before the end, to what it
    /// was as long before the start: the particles spawned then are the ones
    /// the first frame shows.
    pub(super) fn venus_beam_whole(&mut self) -> Result<(), String> {
        let seed = self.sym("gRandomSeed")?;
        let mut field = self.aerie(AFTER_FIRST_BATTLE)?;
        let mut beam: Option<(u32, Vec<u32>)> = None;
        let mut n = 0;
        loop {
            field.frames(1)?;
            n += 1;
            if n > 12000 || field.scene() != AFTER_FIRST_BATTLE.0 {
                return Err("the beam never settled with a page waiting".to_string());
            }
            if beam.is_none() && field.actor(ACTOR_BEAM_BOTTOM).is_some_and(|a| a.x == BEAM_X) {
                beam = Some((n, bg_palette(&field)));
            }
            if field.waiting() {
                let back = beam.as_ref().is_some_and(|(at, colours)| n > at + STAR_COLOURS && bg_palette(&field) == *colours);
                if back {
                    break;
                }
                field.press(3)?;
            }
        }
        // Where the loop starts, at least STAR_SETTLE frames on: where every
        // actor shows the frame of its idle animation it shows a loop later,
        // in the shortest loop that has such a start. The idle animations
        // repeat every 177 to 614 frames, each to its own count, so the loop
        // starts where they line up (idle_frames).
        let waiting = field.save();
        let mut idle: Vec<Vec<i32>> = vec![idle_frames(&field)];
        for _ in 0..STAR_SEARCH {
            field.frames(1)?;
            idle.push(idle_frames(&field));
        }
        let lined_up = (1..=STAR_LOOP_UNITS).map(|units| units * STAR_LOOP_UNIT).find_map(|n| (STAR_SETTLE..STAR_SEARCH - n).find(|&s| (s - 2..=s).all(|k| idle[k] == idle[k + n])).map(|s| (s, n)));
        let (begin, looped) = lined_up.ok_or("the actors' idle animations never line up a loop apart")?;
        // States are kept from PARTICLE_LIFE and LEAD frames before the loop.
        // The seed is set between two of the game's frames, and no view may be
        // pinned before it and drawn after (Guide::whole).
        let start = PARTICLE_LIFE + LEAD;
        let reseed = start + looped - PARTICLE_LIFE;
        field.load(&waiting);
        field.frames((begin - start) as u32)?;
        let mut states = States::default();
        let mut first_seed = 0;
        for i in 0..=start + looped {
            if i == start - PARTICLE_LIFE {
                first_seed = field.read32(seed);
            }
            if i == reseed {
                field.write32(seed, first_seed);
                field.cut();
            }
            states.push(field.save());
            field.frames(1)?;
        }
        let frames: Vec<usize> = (start..=start + looped).step_by(2).collect();
        let mut pictures = self.whole(&mut field, &states, &frames)?;
        // The frame after the last is the first again.
        let after = pictures.pop().ok_or("no frames")?;
        let differ = after.pixels.iter().zip(&pictures[0].pixels).filter(|(a, b)| a != b).count();
        if differ != 0 {
            return Err(format!("the frame after the loop's last differs from its first in {differ} pixels"));
        }
        self.whole_gif("places/venus-lighthouse-aerie/beam-whole.gif", &pictures, "the beam the Venus Star lights, the whole aerie and the sky")
    }

    /// The beacon erupting, seen whole, as
    /// `places/venus-lighthouse-aerie/eruption-whole.gif`: the aerie after
    /// the second battle, its pages answered with A, from the white flash
    /// just before the beam stacks its five segments, through the split and
    /// the three tints to the colours back and the beam closed. It plays
    /// once and starts again at the flash.
    pub(super) fn venus_eruption_whole(&mut self) -> Result<(), String> {
        let erupted = FLAG_VENUS_AERIE_BEACON_ERUPTED;
        let mut field = self.aerie(AFTER_SECOND_BATTLE)?;
        let mut kept: std::collections::VecDeque<super::field::Saved> = std::collections::VecDeque::new();
        let mut n = 0u32;
        while !field.flag_is_set(erupted) {
            kept.push_back(field.save());
            if kept.len() > ERUPTION_BEFORE + LEAD + LATE {
                kept.pop_front();
            }
            // A held for three frames in six while a page waits, a frame at
            // a time so that every state is kept.
            field.hold(if field.waiting() && n % 6 < 3 { KEY_A } else { 0 });
            field.frames(1)?;
            n += 1;
            if n > 12000 || field.scene() != AFTER_SECOND_BATTLE.0 {
                return Err("the beacon never erupted".to_string());
            }
        }
        field.hold(0);
        let mut states = States::default();
        for saved in kept {
            states.push(saved);
        }
        for _ in 0..ERUPTION_AFTER {
            states.push(field.save());
            field.frames(1)?;
            if field.waiting() {
                return Err("a page came up while the beam was open".to_string());
            }
        }
        let frames: Vec<usize> = (LEAD + LATE..states.len()).step_by(2).collect();
        let pictures = self.whole(&mut field, &states, &frames)?;
        self.whole_gif("places/venus-lighthouse-aerie/eruption-whole.gif", &pictures, "the beacon erupting, the whole aerie and the sky")
    }

    /// Enters the aerie at `place` after the party has left Vale.
    fn aerie(&self, place: (u16, u16)) -> Result<Field<'a>, String> {
        let set = vec![self.flag("FLAG_VALE_PARTY_LEFT")?];
        let clear = vec![self.flag("FLAG_VALE_STORM")?, self.flag("FLAG_CHILDHOOD")?];
        Field::enter(self.rom, self.symbols, &Setup { scene: place.0, entrance: place.1, set, clear })
    }

    /// The frames `frames` of `states` drawn whole (whole::stitch): the
    /// aerie's scroll bounds as the last state has them, and the sky.
    fn whole(&self, field: &mut Field, states: &States, frames: &[usize]) -> Result<Vec<Image>, String> {
        field.load(&states.get(states.len() - 1));
        let area = field.bounds().map(|v| v >> 16);
        whole::stitch(field, states, frames, area)
    }

    pub(super) fn whole_gif(&mut self, file: &str, pictures: &[Image], what: &str) -> Result<(), String> {
        let (bytes, frames) = whole::write_gif(&self.out.join(file), pictures)?;
        self.written.push((file.to_string(), bytes, format!("{what}; {frames}")));
        Ok(())
    }
}

/// What each actor shows: its place and height (pixels) and the frame each
/// layer of its sprite shows (Actor.sprite, Sprite.entries,
/// SpriteAnimation.picture), for the actors 0 (the party) to LAST_ACTOR.
fn idle_frames(field: &Field) -> Vec<i32> {
    let mut out = Vec::new();
    for id in 0..=LAST_ACTOR {
        let Some(a) = field.actor(id) else { continue };
        out.extend([a.x, a.z, a.y >> 16]);
        let sprite = field.read32(field.actor_address(id) + OBJ_SPRITE);
        if !is_ram(sprite) {
            continue;
        }
        for k in 0..4 {
            let anim = field.read32(sprite + SPRITE_ENTRIES + 4 * k);
            out.push(if is_ram(anim) { i32::from(field.read8(anim + SPRITE_ANIMATION_PICTURE)) } else { -1 });
        }
    }
    out
}

/// Whether `address` is in EWRAM or IWRAM, where the game keeps its sprites.
fn is_ram(address: u32) -> bool {
    matches!(address >> 24, 2 | 3)
}

/// BG palette RAM (entries 0 to 255), two to a word.
fn bg_palette(field: &Field) -> Vec<u32> {
    (0..128u32).map(|i| field.read32(PALETTE_RAM + 4 * i)).collect()
}


