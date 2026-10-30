//! Whole pictures: a map drawn entire on every frame, with the sky above it,
//! rather than the 240 by 160 the camera shows. Each frame is stitched from
//! views of one moment: the machine's state is kept at the end of every
//! frame (States), and for each view the machine goes back to a state a few
//! frames before, the view is pinned (Field::pin_view) and the game runs on
//! to the moment with the keys the run held on each of those frames, objects
//! and all, so the actors, their effects and particles stand where they do
//! in the game on that frame, in every view. The message window (BG0) and
//! the sprites drawn over everything (OAM priority 0: the portrait and the
//! letters, which the game draws as sprites) are left out. The camera's
//! shake moves the view and not the world, so the map seen whole holds
//! still. Venus Lighthouse's beam (guide_beam.rs) and any event `render
//! event --whole` plays (places.rs) are drawn so. This is the Doll House
//! recorder; the Lighthouse Aerie's story (guide_aerie.rs) draws several
//! runs of the game as one recording (stitch_runs).

use super::animation;
use super::field::{Field, Image, Saved, Track};
use super::mgba::{HEIGHT, WIDTH};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

/// The state each view is pinned from, at least so many frames before the
/// frame drawn (stitch), and the frames more it may take the game to build
/// that picture at the pinned view.
pub const LEAD: usize = 2;
pub const LATE: usize = 4;
/// Pixels each view reaches past the canvas and overlaps the next: more than
/// twice the shake (MapGrid_SetShake, at most 6 pixels in the game's
/// scripts), which narrows the scroll bounds by its amplitude and then moves
/// the view by as much. A strong shake moves a view by up to 12 pixels, past
/// the EDGE it leaves out: WIDE_MARGIN covers that too, for the frames
/// MARGIN leaves a gap in (Run::wide: the Lighthouse Aerie's quakes). Views
/// placed elsewhere catch other sprites at their edges and can reach things
/// the game only moves while they are in view (its random draws then
/// differ), so every other frame keeps MARGIN.
const MARGIN: i32 = 16;
const WIDE_MARGIN: i32 = 20;
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
/// A backdrop (BGR555 magenta) drawn behind the sprites alone where a view
/// looks past the map's top; a sprite of that colour there would be lost.
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
/// Threads a whole recording's frames are shared out among (0 for one a
/// CPU), and the fewest frames a thread is given: each first enters the
/// scene on its own machine.
pub static THREADS: AtomicUsize = AtomicUsize::new(0);
const FRAMES_A_THREAD: usize = 12;
/// Every so many states one is kept as it is; those between are kept as
/// their difference from it.
const KEYFRAME: usize = 60;

/// The machine's state at the end of each frame of a run, kept small: one
/// in KEYFRAME as it is, the rest as the bytes that differ from the last of
/// those (runs of equal bytes skipped). A minute of the field keeps in a few
/// tens of megabytes where its states alone would take over a gigabyte.
#[derive(Default)]
pub struct States {
    keyframes: Vec<Vec<u8>>,
    /// Each state: its keyframe, and its difference from it (empty for the
    /// keyframe itself).
    kept: Vec<(usize, Vec<u8>)>,
    tracks: Vec<Track>,
}

impl States {
    pub fn push(&mut self, saved: Saved) {
        if self.kept.len().is_multiple_of(KEYFRAME) {
            self.keyframes.push(saved.state);
            self.kept.push((self.keyframes.len() - 1, Vec::new()));
        } else {
            let k = self.keyframes.len() - 1;
            self.kept.push((k, delta(&self.keyframes[k], &saved.state)));
        }
        self.tracks.push(saved.track);
    }

    pub fn len(&self) -> usize {
        self.kept.len()
    }

    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }

    pub fn get(&self, i: usize) -> Saved {
        let (k, ref d) = self.kept[i];
        let state = if d.is_empty() { self.keyframes[k].clone() } else { undelta(&self.keyframes[k], d) };
        Saved { state, track: self.tracks[i] }
    }

    /// The keys held through the frame after state `i` (Saved::keys).
    pub fn keys(&self, i: usize) -> u32 {
        self.tracks[i].keys()
    }

    /// The changes from outside the game made by state `i` (Saved::cuts).
    pub fn cuts(&self, i: usize) -> u32 {
        self.tracks[i].cuts()
    }
}

/// `new` as its difference from `base` (the same length): a run of bytes
/// equal to base's, a run that differs, its bytes, and so on, each length a
/// little-endian u32.
fn delta(base: &[u8], new: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < new.len() {
        let same = new[i..].iter().zip(&base[i..]).take_while(|(a, b)| a == b).count();
        i += same;
        // A run that differs ends at 8 equal bytes in a row, so that a lone
        // equal byte does not cost a pair of lengths.
        let mut end = i;
        let mut equal = 0;
        while end < new.len() && equal < 8 {
            if new[end] == base[end] {
                equal += 1;
            } else {
                equal = 0;
            }
            end += 1;
        }
        let end = end - equal;
        out.extend((same as u32).to_le_bytes());
        out.extend(((end - i) as u32).to_le_bytes());
        out.extend(&new[i..end]);
        i = end;
    }
    out
}

fn undelta(base: &[u8], d: &[u8]) -> Vec<u8> {
    let mut out = base.to_vec();
    let (mut i, mut at) = (0, 0);
    let word = |k: usize| u32::from_le_bytes([d[k], d[k + 1], d[k + 2], d[k + 3]]) as usize;
    while at < d.len() {
        i += word(at);
        let n = word(at + 4);
        out[i..i + n].copy_from_slice(&d[at + 8..at + 8 + n]);
        i += n;
        at += 8 + n;
    }
    out
}

/// The frames `frames` (indexes into `states`, each at least LEAD) drawn
/// whole: the area `area` (left, top, right, bottom, pixels: the map's
/// scroll bounds or a room's box) and the sky over it, cut to the highest
/// sprite drawn there in any frame (with none, no sky above it).
///
/// The picture the GBA draws on frame t is the one the game finished
/// building in the frame before (its frame counter, gFrameCount, then as at
/// t - 1; the VBlank handler copies it once gFrameRenderPending is set).
/// Each view is pinned from a state before the game began that frame, LEAD
/// frames before t or earlier, so the game builds the whole picture at the
/// pinned view, and is run until the game has built it, holding the keys
/// the run held on each of those frames (the game reads them every VBlank).
/// Seen from somewhere else, the game can take longer over a frame than the
/// GBA gives it, which only delays the picture: the game's frames are the
/// same ones, and the random seed (gRandomSeed) must then be as the game
/// left it, or the pinned view has changed what happened. No view is pinned
/// from one side of a change made from outside the game (Field::cut) and
/// drawn on the other.
pub fn stitch(field: &mut Field, states: &States, frames: &[usize], area: [i32; 4]) -> Result<Vec<Image>, String> {
    stitch_runs(&mut [Run { field, states, frames, wide: false }], area)
}

/// One run of the game drawn whole (stitch_runs): its field, the states it
/// kept, the frames drawn from them, and whether a frame its views leave a
/// gap in (a strong shake) is drawn again from views reaching WIDE_MARGIN.
pub struct Run<'f, 'rom> {
    pub field: &'f mut Field<'rom>,
    pub states: &'f States,
    pub frames: &'f [usize],
    pub wide: bool,
}

/// Several runs drawn whole one after another over the same area, as one
/// picture sequence: each run's frames as stitch draws them, the sky over
/// the area cut once for all, to the highest sprite drawn there in any run.
pub fn stitch_runs(runs: &mut [Run], area: [i32; 4]) -> Result<Vec<Image>, String> {
    let canvas = Canvas::new(area, MARGIN);
    let mut pictures = Vec::new();
    let mut highest = canvas.map_top;
    for run in runs.iter_mut() {
        let mut canvases = vec![Canvas::new(area, MARGIN)];
        if run.wide {
            canvases.push(Canvas::new(area, WIDE_MARGIN));
        }
        let (mut p, h) = stitch_uncut(run.field, run.states, run.frames, &canvases)?;
        pictures.append(&mut p);
        highest = highest.min(h);
    }
    let w = canvas.w;
    let cut = if highest < canvas.map_top { (highest - SKY_MARGIN).max(canvas.top) - canvas.top } else { ABOVE } as usize;
    for p in &mut pictures {
        p.pixels.drain(..cut * w);
        p.height -= cut;
    }
    Ok(pictures)
}

/// stitch's frames with the whole sky over the map left on, and the highest
/// row a sprite showed in there.
fn stitch_uncut(field: &mut Field, states: &States, frames: &[usize], canvases: &[Canvas]) -> Result<(Vec<Image>, i32), String> {
    let s = field.symbols();
    let (seed, frame_count, pending) = (s.random_seed, s.frame_count, s.render_pending);
    // Each state's frame counter, whether the game had finished its frame
    // then, and its seed.
    let marks: Vec<Mark> = (0..states.len())
        .map(|i| {
            field.load(&states.get(i));
            (field.read32(frame_count), field.read8(pending) != 0, field.read32(seed))
        })
        .collect();
    // The frames are shared out among threads, each drawing a run of them
    // on its own field (Field::recipe) from the same states: every view is
    // drawn from a state loaded whole, so which machine draws it does not
    // matter.
    let wanted = match THREADS.load(Relaxed) {
        0 => std::thread::available_parallelism().map_or(1, |n| n.get()),
        n => n,
    };
    let threads = wanted.min(frames.len() / FRAMES_A_THREAD).max(1);
    let (pictures, highest) = match field.recipe() {
        Some(recipe) if threads > 1 => {
            let part = frames.len().div_ceil(threads);
            let drawn: Vec<Result<(Vec<Image>, i32), String>> = std::thread::scope(|scope| {
                let workers: Vec<_> = frames
                    .chunks(part)
                    .map(|run| {
                        let (recipe, marks) = (&recipe, &marks);
                        scope.spawn(move || draw(&mut recipe.enter()?, states, run, canvases, marks))
                    })
                    .collect();
                workers.into_iter().map(|w| w.join().unwrap_or_else(|_| Err("a thread drawing frames whole failed".to_string()))).collect()
            });
            let mut pictures = Vec::with_capacity(frames.len());
            let mut highest = canvases[0].map_top;
            for run in drawn {
                let (mut p, h) = run?;
                pictures.append(&mut p);
                highest = highest.min(h);
            }
            (pictures, highest)
        }
        _ => draw(field, states, frames, canvases, &marks)?,
    };
    field.load(&states.get(states.len() - 1));
    Ok((pictures, highest))
}

/// Whether the game finished a frame between two states, the second a blank
/// or more after the first: its picture is waiting at the second
/// (gFrameRenderPending), and was not at the first or the frame counter has
/// moved on since.
fn completes(before: Mark, after: Mark) -> bool {
    after.1 && (!before.1 || after.0 != before.0)
}

/// A state's frame counter (gFrameCount), whether the game had finished
/// its frame (gFrameRenderPending set) and its random seed (gRandomSeed).
type Mark = (u32, bool, u32);

/// The whole picture's area and the views it is drawn from.
struct Canvas {
    left: i32,
    top: i32,
    map_top: i32,
    right: i32,
    bottom: i32,
    w: usize,
    h: usize,
    /// The scroll bounds the views are pinned within (16.16 pixels).
    bounds: [i32; 4],
    /// Each view's top left, and whether it looks over the sky.
    views: Vec<((i32, i32), bool)>,
}

impl Canvas {
    fn new(area: [i32; 4], margin: i32) -> Canvas {
        let [left, map_top, right, bottom] = area;
        let top = map_top - ABOVE;
        let bounds = [(left - margin) << 16, (top - margin) << 16, (right + margin) << 16, (bottom + margin) << 16];
        // Views over the sky (their top above the map's) draw only the
        // sprites, over KEY; the rest draw the map. They overlap and reach
        // margin past the edges, so that the shake leaves no gap.
        let stride = |size: usize| size as i32 - 2 * margin;
        let sky_rows = steps(top - margin, map_top + margin, HEIGHT as i32, stride(HEIGHT)).into_iter().map(|y| (y, true));
        let map_rows = steps(map_top - margin, bottom + margin, HEIGHT as i32, stride(HEIGHT)).into_iter().map(|y| (y, false));
        let mut views = Vec::new();
        for (y, over_sky) in sky_rows.chain(map_rows) {
            for x in steps(left - margin, right + margin, WIDTH as i32, stride(WIDTH)) {
                views.push(((x, y), over_sky));
            }
        }
        Canvas { left, top, map_top, right, bottom, w: (right - left) as usize, h: (bottom - top) as usize, bounds, views }
    }
}

/// Draws the frames `frames` whole on `field` (stitch), the sky above the
/// map uncut, each from the views of the first of `canvases` that leaves no
/// gap in it: the pictures, and the highest row a sprite showed in above the
/// map (the map's top for none).
fn draw(field: &mut Field, states: &States, frames: &[usize], canvases: &[Canvas], marks: &[Mark]) -> Result<(Vec<Image>, i32), String> {
    let mut pictures = Vec::new();
    let mut highest = canvases[0].map_top;
    for &t in frames {
        let mut drawn = None;
        for c in canvases {
            drawn = draw_frame(field, states, t, c, marks, &mut highest)?;
            if drawn.is_some() {
                break;
            }
        }
        pictures.push(drawn.ok_or_else(|| format!("some of the map was in no view on frame {t}"))?);
    }
    Ok((pictures, highest))
}

/// Frame `t` drawn whole from the views of `c` (draw), none when they leave
/// a gap in it; `highest` is lowered to the highest row a sprite shows in
/// above the map.
fn draw_frame(field: &mut Field, states: &States, t: usize, c: &Canvas, marks: &[Mark], highest: &mut i32) -> Result<Option<Image>, String> {
    let s = field.symbols();
    let (seed, frame_count, pending, bg_scroll) = (s.random_seed, s.frame_count, s.render_pending, s.bg_scroll);
    let (left, top, map_top, right, bottom, w, h) = (c.left, c.top, c.map_top, c.right, c.bottom, c.w, c.h);
    // The picture's frame; the seed as the game finishes it, at t - 1
    // or later when the game itself took longer over it; and the last
    // state, LEAD frames before t or earlier, at which the game had
    // finished the frame before and not begun this one.
    let built = marks[t - 1].0;
    let finished = (t - 1..states.len()).take_while(|&k| marks[k].0 == built).find(|&k| marks[k].1);
    let built_seed = finished.map(|k| marks[k].2);
    let from = (0..=t - LEAD).rev().find(|&k| marks[k].0 < built && marks[k].1).ok_or_else(|| format!("no state before frame {t} began"))?;
    // The frames the game finished from there to this one's picture: a
    // view that makes the game slower finishes the same frame a blank or
    // more later, when the frame counter has passed `built`.
    let needed = finished.map(|k| (from + 1..=k).filter(|&i| completes(marks[i - 1], marks[i])).count());
    if states.cuts(from) != states.cuts(t - 1) {
        return Err(format!("frame {t}'s views would be pinned across a change made from outside the game"));
    }
    let start = states.get(from);
    let mut canvas = vec![u32::MAX; w * h];
    for &(view, over_sky) in &c.views {
        field.pin_view(&start, (view.0 << 16, view.1 << 16), c.bounds, bg_scroll)?;
        field.hold_now(start.keys());
        let mut ran = 0;
        let mut last = marks[from];
        let mut done = 0;
        loop {
            // The keys the run held as this frame ended, past its last
            // state those it ended with.
            field.hold(states.keys((from + ran + 1).min(states.len() - 1)));
            field.frames(1)?;
            ran += 1;
            let now = (field.read32(frame_count), field.read8(pending) != 0, 0);
            done += usize::from(completes(last, now));
            last = now;
            if now.0 == built && now.1 {
                break;
            }
            if now.0 > built && now.1 && Some(done) == needed {
                break;
            }
            if ran > t - from + LATE {
                return Err(format!("the view at {view:?} never finished frame {t}'s picture"));
            }
        }
        if built_seed.is_some_and(|s| s != field.read32(seed)) {
            return Err(format!("the view at {view:?} changed the game's random draws before frame {t}"));
        }
        // The view the picture was built at (with the shake), before the
        // game moves on to its next frame while the picture is drawn. The
        // view stays pinned until it is drawn: a game slow over its frames
        // still runs its last while the first lines are drawn, and let go,
        // its view would jump back to the camera and draw the row and column
        // that come into view there into the picture.
        let at = field.view();
        field.until_line(LAST_BLANK_LINE).map_err(|e| format!("frame {t}, the view at {view:?}: {e}"))?;
        hide_front_sprites(field);
        hide_parked_sprites(field);
        let hidden = if over_sky { DISPCNT_BGS } else { DISPCNT_BG0 };
        field.io_write16(DISPCNT, field.read16(DISPCNT) & !hidden);
        if over_sky {
            field.io_write16(BLDCNT, field.read16(BLDCNT) & !BLEND_KEY_TARGETS);
            field.set_backdrop(KEY);
        }
        field.frames(1)?;
        field.clear_backdrop();
        field.unpin_view();
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
                    *highest = (*highest).min(my);
                }
            }
        }
    }
    // The sky above the map is the colour its top SKY_ROWS rows show
    // most: its first row alone can show another while the colours
    // fade.
    let mut sky: HashMap<u32, usize> = HashMap::new();
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
        return Ok(None);
    }
    Ok(Some(Image { width: w, height: h, pixels: canvas }))
}

/// A recorded event drawn whole (render event --whole): every second frame
/// from the trigger, once the run's own changes to the game are made
/// (Field::cut), until the event leaves the scene or a battle begins, over
/// the room's camera box (Field::room).
pub fn event(field: &mut Field, states: &States) -> Result<Vec<Image>, String> {
    if states.is_empty() {
        return Err("no state was kept".to_string());
    }
    let last = states.len() - 1;
    let first = (0..=last).find(|&i| states.cuts(i) == states.cuts(last)).unwrap_or(last);
    field.load(&states.get(first));
    let scene = field.scene();
    let area = field.room();
    let mut end = first;
    for i in first + 1..=last {
        field.load(&states.get(i));
        if field.scene() != scene {
            break;
        }
        end = i;
    }
    // A frame's views are pinned from a state at least LEAD frames before
    // it, which the game had finished: the first frames leave room for one
    // after the run's changes.
    let frames: Vec<usize> = (first + LEAD + 2..=end).step_by(2).collect();
    if frames.is_empty() {
        return Err("the event left the scene before a frame could be drawn whole".to_string());
    }
    stitch(field, states, &frames, area)
}

/// Writes whole pictures, each one frame of every two the GBA draws, as a
/// GIF at 30 frames a second that loops forever, every pixel in its own
/// colour: with one palette for the file (animation::write) when the
/// pictures have no more than 255 colours, else with a palette for each
/// frame of the colours in the rectangle that changed (a fade gives
/// thousands of colours, but no frame more than 255). Returns the file's
/// bytes and a note of its frames.
pub fn write_gif(path: &Path, pictures: &[Image]) -> Result<(u64, String), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let first = pictures.first().ok_or("no frames")?;
    let (w, h) = (first.width, first.height);
    animation::check_black_clearance(&pictures.iter().map(|p| animation::is_black(&p.pixels)).collect::<Vec<_>>(), 2)?;
    let all: HashSet<u32> = pictures.iter().flat_map(|p| p.pixels.iter().copied()).collect();
    if all.len() <= GIF_COLOURS {
        // animation::write shows every second frame it is given: each
        // picture, then one it passes over.
        let frames: Vec<Vec<u32>> = pictures.iter().flat_map(|p| [p.pixels.clone(), Vec::new()]).collect();
        let written = animation::write(path, &frames, w)?;
        return Ok((written.bytes, format!("{} frames, {} colours", written.frames, written.colours)));
    }
    let error = |e: gif::EncodingError| format!("{}: {e}", path.display());
    let out = std::fs::File::create(path).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
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
    let bytes = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    Ok((bytes, format!("{} frames, {stored} stored", pictures.len())))
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
/// field's sprites are priority 1.
fn hide_front_sprites(field: &mut Field) {
    for i in 0..128u32 {
        let attr0 = field.read16(OAM + 8 * i);
        let priority = field.read16(OAM + 8 * i + 4) >> 10 & 3;
        if attr0 & 0x300 != 0x200 && priority == 0 {
            field.write16(OAM + 8 * i, (attr0 & !0x300) | 0x200);
        }
    }
}

/// Hides, for the frame about to be drawn, the sprites of the actors parked
/// at the map's origin (Field::parked), which the game never shows but a
/// view of the map's corner would: each sprite within reach of where such
/// an actor stands on the screen, above its feet.
fn hide_parked_sprites(field: &mut Field) {
    let parked = field.parked();
    if parked.is_empty() {
        return;
    }
    let (vx, vy) = field.view();
    for i in 0..128u32 {
        let attr0 = field.read16(OAM + 8 * i);
        if attr0 & 0x300 == 0x200 {
            continue;
        }
        let y = i32::from(attr0 & 0xff);
        let y = if y >= HEIGHT as i32 { y - 256 } else { y };
        let x = i32::from(field.read16(OAM + 8 * i + 2) & 0x1ff);
        let x = if x >= 256 { x - 512 } else { x };
        let near = parked.iter().any(|a| {
            let (sx, sy) = (a.x - vx, a.z - (a.y >> 16) - vy);
            (x - sx).abs() <= PARKED_REACH && (-PARKED_HEIGHT..=PARKED_REACH).contains(&(y - sy))
        });
        if near {
            field.write16(OAM + 8 * i, (attr0 & !0x300) | 0x200);
        }
    }
}

/// Pixels a parked actor's sprites reach beside and below its place, and
/// above it.
const PARKED_REACH: i32 = 32;
const PARKED_HEIGHT: i32 = 64;

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
    use super::{delta, steps, undelta};

    /// Views cover the span without a gap, the last ending at its end.
    #[test]
    fn views_cover_the_span() {
        assert_eq!(steps(0, 420, 160, 160), vec![0, 160, 260]);
        assert_eq!(steps(-8, 428, 160, 144), vec![-8, 136, 268]);
        assert_eq!(steps(-168, 8, 160, 144), vec![-168, -152]);
        assert_eq!(steps(0, 100, 160, 144), vec![0]);
        assert_eq!(steps(-8, 608, 240, 224), vec![-8, 216, 368]);
    }

    /// A state kept as its difference comes back byte for byte, whatever
    /// runs of equal and differing bytes it has.
    #[test]
    fn a_difference_restores_the_state() {
        let base: Vec<u8> = (0..4096u32).map(|i| (i * 7 % 251) as u8).collect();
        let mut new = base.clone();
        for i in [0, 1, 5, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 2000, 4095] {
            new[i] ^= 0x5a;
        }
        new[100..400].iter_mut().for_each(|b| *b = b.wrapping_add(1));
        assert_eq!(undelta(&base, &delta(&base, &new)), new);
        assert_eq!(undelta(&base, &delta(&base, &base)), base);
        assert!(delta(&base, &base).len() <= 8);
    }
}

