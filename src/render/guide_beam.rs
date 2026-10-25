//! Venus Lighthouse's beam seen whole: every frame drawn as the aerie's whole
//! map with the sky above it, rather than the 240 by 160 the camera shows.
//! Each frame is stitched from views of one moment: the game is saved a
//! few frames before it, and for each view the machine goes back there, the
//! view is pinned (Field::pin_view) and the game runs on to the moment,
//! objects and all, so the actors, the beam's segments and its particles
//! stand where they do in the game on that frame, in every view. The
//! message window (BG0) and the sprites drawn over everything (OAM priority
//! 0: the portrait and the letters, which the game draws as sprites) are
//! left out. The camera's shake moves the view and not the world, so the
//! map seen whole holds still.

use super::animation;
use super::field::{Field, Image, Saved, Setup};
use super::guide::Guide;
use super::mgba::{HEIGHT, WIDTH};

/// The Lighthouse Aerie (MapId_VenusLighthouseAerie) back from the first
/// battle (VENUS_LIGHTHOUSE_AERIE_AFTER_FIRST_BATTLE), where
/// VenusLighthouseAerie_CombineSaturosAndMenardi throws the Venus Star into
/// the beacon, and after the second (VENUS_LIGHTHOUSE_AERIE_AFTER_SECOND_BATTLE),
/// where VenusLighthouseAerie_FinishSaturosAndMenardiBattle erupts it.
const AFTER_FIRST_BATTLE: (u16, u16) = (187, 2);
const AFTER_SECOND_BATTLE: (u16, u16) = (187, 3);
/// The beam's lowest segment, which CombineSaturosAndMenardi places over the
/// beacon (x 304) as the star goes in.
const LOCALID_BEAM_BOTTOM: u16 = 24;
const BEAM_X: i32 = 304;
const KEY_A: u32 = 0x1;

/// The state each view is pinned from, at least so many frames before the
/// frame drawn (Guide::whole), and the frames more it may take the game to
/// build that picture at the pinned view.
const LEAD: usize = 2;
const LATE: usize = 4;
/// Pixels each view reaches past the canvas and overlaps the next: more than
/// twice the shake (MapGrid_SetShake, at most 6 pixels here), which narrows
/// the scroll bounds by its amplitude and then moves the view by as much.
const MARGIN: i32 = 16;
/// The lines at the top of each view and the columns at its left left out
/// of the whole picture: a tile's.
const EDGE: i32 = 8;
/// The last line of vertical blank (Field::until_line), where the registers
/// and sprites of the frame that follows are changed.
const LAST_BLANK_LINE: u16 = 227;
const DISPCNT: u32 = 0x0400_0000;
const DISPCNT_BG0: u16 = 0x0100;
const DISPCNT_BGS: u16 = 0x0f00;
const BLDCNT: u32 = 0x0400_0050;
/// BLDCNT's second targets and the backdrop as a first target: cleared where
/// the sprites are drawn over KEY, so that neither a blend nor a fade to white
/// changes KEY.
const BLEND_KEY_TARGETS: u16 = 0x3f20;
const OAM: u32 = 0x0700_0000;
const PALETTE_RAM: u32 = 0x0500_0000;
/// A backdrop no sprite of the aerie uses (BGR555 magenta), drawn behind the
/// sprites alone where a view looks past the map's top.
const KEY: u16 = 0x7c1f;
/// Rows of sky the canvas may reach above the map's top, before it is cut
/// to the highest sprite drawn there, SKY_MARGIN rows higher; and the map's
/// top rows, whose commonest colour fills it.
const ABOVE: i32 = 160;
const SKY_MARGIN: i32 = 8;
const SKY_ROWS: i32 = 8;
/// The GIF's frames a second (every second frame of the GBA's 60), and the
/// colours a palette holds besides the one left transparent.
const GIF_FPS: usize = 30;
const GIF_COLOURS: usize = 255;

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
/// animation is compared (LOCALID_BEAM_TOP), with the offsets read for it:
/// ObjectEvent.sprite, Sprite.entries and SpriteAnim.frame.
const STAR_SEARCH: usize = 3000;
const LAST_ACTOR: u16 = 28;
const OBJ_SPRITE: u32 = 0x50;
const SPRITE_ENTRIES: u32 = 0x28;
const SPRITE_ANIM_FRAME: u32 = 0x16;

/// The eruption: frames kept from before the beam stacks its five segments
/// (FLAG_VENUS_AERIE_BEACON_ERUPTED), the white flash already full, and
/// after, past the three tints and the colours back (360 frames) to the
/// beam closed (gVenusLighthouseAerie_BeamCloseAction, 30) and a moment
/// more.
const ERUPTION_BEFORE: usize = 12;
const ERUPTION_AFTER: usize = 420;
/// FLAG_VENUS_AERIE_BEACON_ERUPTED, FLAG_MAP_TEMP_36 in
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
            if beam.is_none() && field.actor(LOCALID_BEAM_BOTTOM).is_some_and(|a| a.x == BEAM_X) {
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
        let mut saved: Vec<Saved> = Vec::new();
        let mut first_seed = 0;
        for i in 0..=start + looped {
            if i == start - PARTICLE_LIFE {
                first_seed = field.read32(seed);
            }
            if i == reseed {
                field.write32(seed, first_seed);
            }
            saved.push(field.save());
            field.frames(1)?;
        }
        let frames: Vec<usize> = (start..=start + looped).step_by(2).collect();
        let mut pictures = self.whole(&mut field, &saved, &frames, Some(reseed))?;
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
        let mut kept: std::collections::VecDeque<Saved> = std::collections::VecDeque::new();
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
        let mut saved: Vec<Saved> = kept.into_iter().collect();
        for _ in 0..ERUPTION_AFTER {
            saved.push(field.save());
            field.frames(1)?;
            if field.waiting() {
                return Err("a page came up while the beam was open".to_string());
            }
        }
        let frames: Vec<usize> = (LEAD + LATE..saved.len()).step_by(2).collect();
        let pictures = self.whole(&mut field, &saved, &frames, None)?;
        self.whole_gif("places/venus-lighthouse-aerie/eruption-whole.gif", &pictures, "the beacon erupting, the whole aerie and the sky")
    }

    /// Enters the aerie at `place` after the party has left Vale.
    fn aerie(&self, place: (u16, u16)) -> Result<Field<'a>, String> {
        let set = vec![self.flag("FLAG_VALE_PARTY_LEFT")?];
        let clear = vec![self.flag("FLAG_VALE_STORM")?, self.flag("FLAG_CHILDHOOD")?];
        Field::enter(self.rom, self.symbols, &Setup { scene: place.0, entrance: place.1, set, clear })
    }

    /// The frames `frames` (indexes into `saved`, the game's state at each
    /// frame, each at least LEAD) drawn whole: the map's scroll bounds and
    /// the sky over them, cut to the highest sprite drawn there in any frame
    /// (with none, no sky above the map).
    ///
    /// The picture the GBA draws on frame t is the one the game finished
    /// building in the frame before (its frame counter, gFrameCount, then as
    /// at t - 1; the VBlank handler copies it once gFrameRenderPending is
    /// set). Each view is pinned from a state before the game began that
    /// frame, LEAD frames before t or earlier, so the game builds the whole
    /// picture at the pinned view, and is run until the game has built it.
    /// Seen from somewhere else, the game can take longer over a frame than
    /// the GBA gives it (with the beam off screen it often does), which only
    /// delays the picture: the game's frames are the same ones, and the random
    /// seed (gRandomSeed) must then be as the game left it, or the pinned view
    /// has changed what happened. No view is pinned before the state
    /// `unpinned`, where the game was given a change, and drawn after it.
    fn whole(&self, field: &mut Field, saved: &[Saved], frames: &[usize], unpinned: Option<usize>) -> Result<Vec<Image>, String> {
        let seed = self.sym("gRandomSeed")?;
        let frame_count = self.sym("gFrameCount")?;
        let pending = self.sym("gFrameRenderPending")?;
        let bg_scroll = self.sym("gBgScroll")?;
        let last = saved.len() - 1;
        field.load(&saved[last]);
        let b = field.bounds().map(|v| v >> 16);
        let (left, map_top, right, bottom) = (b[0], b[1], b[2], b[3]);
        let top = map_top - ABOVE;
        let (w, h) = ((right - left) as usize, (bottom - top) as usize);
        let bounds = [(left - MARGIN) << 16, (top - MARGIN) << 16, (right + MARGIN) << 16, (bottom + MARGIN) << 16];
        // Views over the sky (their top above the map's) draw only the
        // sprites, over KEY; the rest draw the map. They overlap and reach
        // MARGIN past the edges, so that the shake leaves no gap.
        let stride = |size: usize| size as i32 - 2 * MARGIN;
        let sky_rows = steps(top - MARGIN, map_top + MARGIN, HEIGHT as i32, stride(HEIGHT)).into_iter().map(|y| (y, true));
        let map_rows = steps(map_top - MARGIN, bottom + MARGIN, HEIGHT as i32, stride(HEIGHT)).into_iter().map(|y| (y, false));
        let mut views: Vec<((i32, i32), bool)> = Vec::new();
        for (y, over_sky) in sky_rows.chain(map_rows) {
            for x in steps(left - MARGIN, right + MARGIN, WIDTH as i32, stride(WIDTH)) {
                views.push(((x, y), over_sky));
            }
        }
        // Each state's frame counter, whether the game had finished its frame
        // then, and its seed.
        let marks: Vec<(u32, bool, u32)> = saved
            .iter()
            .map(|s| {
                field.load(s);
                (field.read32(frame_count), field.read8(pending) != 0, field.read32(seed))
            })
            .collect();
        let mut pictures = Vec::new();
        let mut highest = map_top;
        for &t in frames {
            // The picture's frame; the seed as the game finishes it, at t - 1
            // or later when the game itself took longer over it; and the last
            // state, LEAD frames before t or earlier, at which the game had
            // finished the frame before and not begun this one.
            let built = marks[t - 1].0;
            let built_seed = (t - 1..saved.len()).take_while(|&k| marks[k].0 == built).find(|&k| marks[k].1).map(|k| marks[k].2);
            let from = (0..=t - LEAD).rev().find(|&k| marks[k].0 < built && marks[k].1).ok_or_else(|| format!("no state before frame {t} began"))?;
            if unpinned.is_some_and(|r| from < r && r < t) {
                return Err(format!("frame {t}'s views would be pinned across a change the game was given"));
            }
            let mut canvas = vec![u32::MAX; w * h];
            for &(view, over_sky) in &views {
                field.pin_view(&saved[from], (view.0 << 16, view.1 << 16), bounds, bg_scroll)?;
                let mut waited = 0;
                loop {
                    field.frames(1)?;
                    if field.read32(frame_count) == built && field.read8(pending) != 0 {
                        break;
                    }
                    waited += 1;
                    if waited > t - from + LATE {
                        return Err(format!("the view at {view:?} never finished frame {t}'s picture"));
                    }
                }
                if built_seed.is_some_and(|s| s != field.read32(seed)) {
                    return Err(format!("the view at {view:?} changed the game's random draws before frame {t}"));
                }
                // The view the picture was built at (with the shake), before the
                // game moves on to its next frame while the picture is drawn.
                let at = field.unpin_view();
                field.until_line(LAST_BLANK_LINE)?;
                hide_front_sprites(field);
                let hidden = if over_sky { DISPCNT_BGS } else { DISPCNT_BG0 };
                field.io_write16(DISPCNT, field.read16(DISPCNT) & !hidden);
                if over_sky {
                    field.io_write16(BLDCNT, field.read16(BLDCNT) & !BLEND_KEY_TARGETS);
                    field.set_backdrop(KEY);
                }
                field.frames(1)?;
                field.clear_backdrop();
                if field.read16(DISPCNT) & hidden != 0 {
                    return Err(format!("the game showed the layers again while the view at {view:?} drew frame {t}"));
                }
                let picture = field.picture();
                // A picture's first lines and columns can show what the map
                // does not there (a black line, the sky of another height or
                // the backdrop): they are left out, and the views overlap
                // enough to leave no gap.
                for y in EDGE..HEIGHT as i32 {
                    let my = at.1 + y;
                    // A view over the sky gives only the rows above the map,
                    // a view of the map only the map's.
                    if my < top || my >= bottom || over_sky != (my < map_top) {
                        continue;
                    }
                    for x in EDGE..WIDTH as i32 {
                        let mx = at.0 + x;
                        if mx < left || mx >= right {
                            continue;
                        }
                        let c = picture[(y * WIDTH as i32 + x) as usize];
                        let i = (my - top) as usize * w + (mx - left) as usize;
                        if !over_sky {
                            canvas[i] = animation::rgb(c);
                        } else if c != KEY {
                            canvas[i] = animation::rgb(c);
                            highest = highest.min(my);
                        }
                    }
                }
            }
            // The sky above the map is the colour its top SKY_ROWS rows show
            // most: its first row alone can show another while the colours
            // fade.
            let mut sky: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
            for &c in &canvas[ABOVE as usize * w..(ABOVE + SKY_ROWS) as usize * w] {
                *sky.entry(c).or_default() += 1;
            }
            let fill = sky.into_iter().filter(|&(c, _)| c != u32::MAX).max_by_key(|&(_, count)| count).map(|(c, _)| c).ok_or("the map's top rows were in no view")?;
            for c in canvas.iter_mut().take(ABOVE as usize * w) {
                if *c == u32::MAX {
                    *c = fill;
                }
            }
            if canvas.contains(&u32::MAX) {
                return Err(format!("some of the map was in no view on frame {t}"));
            }
            pictures.push(Image { width: w, height: h, pixels: canvas });
        }
        field.load(&saved[last]);
        let cut = if highest < map_top { (highest - SKY_MARGIN).max(top) - top } else { ABOVE } as usize;
        for p in &mut pictures {
            p.pixels.drain(..cut * w);
            p.height -= cut;
        }
        Ok(pictures)
    }

    /// Writes whole pictures, each one frame of every two the GBA draws, as
    /// a GIF at 30 frames a second that loops forever, every pixel in its own
    /// colour: with one palette for the file (animation::write) when the
    /// pictures have no more than 255 colours, else with a palette for each
    /// frame of the colours in the rectangle that changed (the fades give the
    /// eruption thousands of colours, but no frame more than 255).
    fn whole_gif(&mut self, file: &str, pictures: &[Image], what: &str) -> Result<(), String> {
        let path = self.out.join(file);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        let first = pictures.first().ok_or("no frames")?;
        let (w, h) = (first.width, first.height);
        let all: std::collections::HashSet<u32> = pictures.iter().flat_map(|p| p.pixels.iter().copied()).collect();
        if all.len() <= GIF_COLOURS {
            // animation::write shows every second frame it is given: each
            // picture, then one it passes over.
            let frames: Vec<Vec<u32>> = pictures.iter().flat_map(|p| [p.pixels.clone(), Vec::new()]).collect();
            let written = animation::write(&path, &frames, w)?;
            self.written.push((file.to_string(), written.bytes, format!("{what}; {} frames, {} colours", written.frames, written.colours)));
            return Ok(());
        }
        let error = |e: gif::EncodingError| format!("{}: {e}", path.display());
        let out = std::fs::File::create(&path).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        let mut encoder = gif::Encoder::new(std::io::BufWriter::new(out), w as u16, h as u16, &[]).map_err(error)?;
        encoder.set_repeat(gif::Repeat::Infinite).map_err(error)?;
        // Frame i starts at i * 100 / 30 hundredths of a second, rounded, as
        // animation.rs times its frames; a frame the same as the last
        // lengthens it.
        let start = |i: usize| (i * 100 + GIF_FPS / 2) / GIF_FPS;
        let mut pending: Option<(usize, gif::Frame)> = None;
        let mut shown: Option<&Image> = None;
        let mut stored = 0;
        for (i, picture) in pictures.iter().enumerate() {
            let rect = match shown {
                None => Some((0, 0, w, h)),
                Some(last) => changed(&last.pixels, &picture.pixels, w),
            };
            let Some((x, y, rw, rh)) = rect else { continue };
            let keep = |k: usize| shown.is_none_or(|last| last.pixels[k] != picture.pixels[k]);
            let pixels = || (y..y + rh).flat_map(|row| row * w + x..row * w + x + rw);
            let mut colours: Vec<u32> = pixels().filter(|&k| keep(k)).map(|k| picture.pixels[k]).collect();
            colours.sort_unstable();
            colours.dedup();
            if colours.len() > GIF_COLOURS {
                return Err(format!("frame {i} has more than {GIF_COLOURS} colours"));
            }
            // The index after the colours is the transparent one; the
            // palette is padded to a power of two.
            let transparent = colours.len() as u8;
            let buffer: Vec<u8> = pixels().map(|k| if keep(k) { colours.binary_search(&picture.pixels[k]).unwrap_or(0) as u8 } else { transparent }).collect();
            let mut palette: Vec<u8> = colours.iter().flat_map(|&c| [(c >> 16) as u8, (c >> 8) as u8, c as u8]).collect();
            palette.resize(3 * (colours.len() + 1).next_power_of_two().max(2), 0);
            let frame = gif::Frame {
                left: x as u16,
                top: y as u16,
                width: rw as u16,
                height: rh as u16,
                dispose: gif::DisposalMethod::Keep,
                transparent: Some(transparent),
                palette: Some(palette),
                buffer: buffer.into(),
                ..gif::Frame::default()
            };
            if let Some((at, mut f)) = pending.replace((i, frame)) {
                f.delay = (start(i) - start(at)) as u16;
                encoder.write_frame(&f).map_err(error)?;
                stored += 1;
            }
            shown = Some(picture);
        }
        if let Some((at, mut f)) = pending.take() {
            f.delay = (start(pictures.len()) - start(at)) as u16;
            encoder.write_frame(&f).map_err(error)?;
            stored += 1;
        }
        encoder.into_inner().map_err(error)?;
        let bytes = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
        self.written.push((file.to_string(), bytes, format!("{what}; {} frames, {stored} stored", pictures.len())));
        Ok(())
    }
}

/// The rectangle (x, y, width, height) where two pictures `w` wide differ.
fn changed(old: &[u32], new: &[u32], w: usize) -> Option<(usize, usize, usize, usize)> {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for (i, _) in old.iter().zip(new).enumerate().filter(|(_, (a, b))| a != b) {
        let (x, y) = (i % w, i / w);
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    (x0 != usize::MAX).then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

/// Hides the sprites drawn in front of everything (OAM priority 0: the
/// message portrait and letters) for the frame about to be drawn; the
/// field's sprites, the beam's particles among them, are priority 1.
fn hide_front_sprites(field: &mut Field) {
    for i in 0..128u32 {
        let attr0 = field.read16(OAM + 8 * i);
        let priority = field.read16(OAM + 8 * i + 4) >> 10 & 3;
        if attr0 & 0x300 != 0x200 && priority == 0 {
            field.write16(OAM + 8 * i, (attr0 & !0x300) | 0x200);
        }
    }
}

/// What each actor shows: its place and height (pixels) and the frame each
/// layer of its sprite shows (ObjectEvent.sprite, Sprite.entries,
/// SpriteAnim.frame), for the actors 0 (the party) to LAST_ACTOR.
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
            out.push(if is_ram(anim) { i32::from(field.read8(anim + SPRITE_ANIM_FRAME)) } else { -1 });
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

/// The starts of views `size` long, `stride` apart, that cover `from` to
/// `to`, the last one ending at `to` (or starting at `from` when the span is
/// shorter).
fn steps(from: i32, to: i32, size: i32, stride: i32) -> Vec<i32> {
    let mut out = Vec::new();
    let mut at = from;
    loop {
        let start = at.min(to - size).max(from);
        out.push(start);
        if start + size >= to {
            break;
        }
        at += stride;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::steps;

    /// Views cover the span without a gap, the last ending at its end.
    #[test]
    fn views_cover_the_span() {
        assert_eq!(steps(0, 420, 160, 160), vec![0, 160, 260]);
        assert_eq!(steps(-8, 428, 160, 144), vec![-8, 136, 268]);
        assert_eq!(steps(-168, 8, 160, 144), vec![-168, -152]);
        assert_eq!(steps(0, 100, 160, 144), vec![0]);
        assert_eq!(steps(-8, 608, 240, 224), vec![-8, 216, 368]);
    }
}
