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

/// stitch, keeping at least `keep_sky` rows of sky above the map's top.
fn stitch_keeping(field: &mut Field, states: &States, frames: &[usize], area: [i32; 4], keep_sky: i32) -> Result<Vec<Image>, String> {
    stitch_runs_keeping(&mut [Run { field, states, frames, wide: false }], area, keep_sky)
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
    stitch_runs_keeping(runs, area, 0)
}

/// stitch_runs, keeping at least `keep_sky` rows of sky (at most ABOVE)
/// above the map's top whether or not a sprite reaches them.
fn stitch_runs_keeping(runs: &mut [Run], area: [i32; 4], keep_sky: i32) -> Result<Vec<Image>, String> {
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
    let cut = if highest < canvas.map_top { (highest - SKY_MARGIN).max(canvas.top) - canvas.top } else { ABOVE };
    let cut = cut.min(ABOVE - keep_sky.clamp(0, ABOVE)) as usize;
    for p in &mut pictures {
        p.pixels.drain(..cut * w);
        p.height -= cut;
    }
    Ok(pictures)
}

/// stitch's frames with the whole sky over the map left on, and the highest
/// row a sprite showed in there.
fn stitch_uncut(field: &mut Field, states: &States, frames: &[usize], canvases: &[Canvas]) -> Result<(Vec<Image>, i32), String> {
    let marks = marks(field, states);
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

/// Each state's mark: its frame counter, whether the game had finished its
/// frame then, and its seed.
fn marks(field: &mut Field, states: &States) -> Vec<Mark> {
    let s = field.symbols();
    let (seed, frame_count, pending) = (s.random_seed, s.frame_count, s.render_pending);
    (0..states.len())
        .map(|i| {
            field.load(&states.get(i));
            (field.read32(frame_count), field.read8(pending) != 0, field.read32(seed))
        })
        .collect()
}

/// The last state before frame `t`'s picture began to be built, LEAD frames
/// before t or earlier, at which the game had finished the frame before
/// (draw_frame pins its views from it).
fn state_before(marks: &[Mark], t: usize) -> Result<usize, String> {
    let built = marks[t - 1].0;
    (0..=t - LEAD).rev().find(|&k| marks[k].0 < built && marks[k].1).ok_or_else(|| format!("no state before frame {t} began"))
}

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
    let from = state_before(marks, t)?;
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

/// How a recorded event's whole frames are cut and dressed (render event
/// --whole-frame, --whole-window).
#[derive(Clone, Copy, Default)]
pub struct Cut {
    /// The frame each whole picture is cut to: left, top, width and height
    /// in the room's pixels from its top left corner (the map pictures'
    /// pixels; a negative top reaches into the sky), or the whole picture.
    pub frame: Option<[i32; 4]>,
    /// Where the message window, its portrait and its letters, which the
    /// whole picture leaves out, are drawn on each frame as the screen
    /// shows them; none leaves them out.
    pub window: Option<Place>,
}

/// A place on a picture's edge for the message window: against its top or
/// bottom edge, centred or in a corner, as far from the edges as the window
/// stands from the screen's nearest edge.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Place {
    pub bottom: bool,
    pub across: Across,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Across {
    Left,
    Centre,
    Right,
}

impl Cut {
    /// A --whole-window value: top, bottom, top-left, top-right,
    /// bottom-left or bottom-right.
    pub fn parse_place(value: &str) -> Result<Place, String> {
        let (edge, across) = value.split_once('-').unwrap_or((value, "centre"));
        let bottom = match edge {
            "top" => false,
            "bottom" => true,
            _ => return Err(format!("--whole-window {value:?}: top, bottom, top-left, top-right, bottom-left or bottom-right")),
        };
        let across = match across {
            "left" => Across::Left,
            "centre" | "center" => Across::Centre,
            "right" => Across::Right,
            _ => return Err(format!("--whole-window {value:?}: top, bottom, top-left, top-right, bottom-left or bottom-right")),
        };
        Ok(Place { bottom, across })
    }

    /// A --whole-frame value: L,T,W,H.
    pub fn parse_frame(value: &str) -> Result<[i32; 4], String> {
        let parts: Vec<i32> = value.split(',').map(|p| p.trim().parse::<i32>().map_err(|e| format!("--whole-frame {value:?}: {e}"))).collect::<Result<_, _>>()?;
        let [l, t, w, h] = parts[..] else {
            return Err(format!("--whole-frame {value:?}: four numbers are needed, left,top,width,height"));
        };
        if w <= 0 || h <= 0 {
            return Err(format!("--whole-frame {value:?}: the width and height must be positive"));
        }
        Ok([l, t, w, h])
    }
}

/// A recorded event drawn whole (render event --whole): every second frame
/// from the trigger, once the run's own changes to the game are made
/// (Field::cut), until the event leaves the scene or a battle begins, over
/// the room's camera box (Field::room), cut and dressed as `cut` says, from
/// the screens `screen` the run recorded (one a state).
pub fn event(field: &mut Field, states: &States, screen: &[Vec<u16>], cut: Cut) -> Result<Vec<Image>, String> {
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
    let mut pictures = match cut.frame {
        None => stitch(field, states, &frames, area)?,
        Some([l, t, w, h]) => {
            let [left, top, right, bottom] = area;
            if l < 0 || l + w > right - left || t + h > bottom - top || -t > ABOVE {
                return Err(format!("the frame {l},{t},{w},{h} reaches outside the room, which spans {} across and {} down with {ABOVE} rows of sky above it", right - left, bottom - top));
            }
            // Only the frame's part of the room is stitched: from the
            // map's top, with the sky rows it reaches into kept, where
            // the frame starts above the map, else from its own top.
            let keep_sky = (-t).max(0);
            let part = [left + l, top + t.max(0), left + l + w, top + t + h];
            let pictures = stitch_keeping(field, states, &frames, part, keep_sky)?;
            let sky = pictures[0].height as i32 - (part[3] - part[1]);
            pictures.into_iter().map(|p| cut_frame(p, [0, -keep_sky, w, h], sky)).collect::<Result<_, _>>()?
        }
    };
    if let Some(place) = cut.window {
        let marks = marks(field, states);
        for (picture, &t) in pictures.iter_mut().zip(&frames) {
            let shown = screen.get(t).ok_or_else(|| format!("no screen was recorded for frame {t}"))?;
            let bare = bare_screen(field, states, t, &marks)?;
            dress(picture, shown, &bare, place);
        }
        field.load(&states.get(states.len() - 1));
    }
    Ok(pictures)
}

/// `picture` cut to `frame` (Cut::frame), whose top is `sky` rows below the
/// picture's.
fn cut_frame(picture: Image, frame: [i32; 4], sky: i32) -> Result<Image, String> {
    let [l, t, w, h] = frame;
    let (top, right, bottom) = (t + sky, l + w, t + sky + h);
    if l < 0 || top < 0 || right > picture.width as i32 || bottom > picture.height as i32 {
        return Err(format!("the frame {l},{t},{w},{h} reaches outside the whole picture, which spans 0 to {} across and {} to {} down", picture.width, -sky, picture.height as i32 - sky));
    }
    let pixels = (top..bottom).flat_map(|y| picture.pixels[y as usize * picture.width + l as usize..y as usize * picture.width + right as usize].iter().copied()).collect();
    Ok(Image { width: w as usize, height: h as usize, pixels })
}

/// The screen the GBA draws on frame `t` with the message window (BG0) and
/// the sprites in front of everything left out: the game run again from
/// the last state before the frame began, at its own camera, so it draws
/// what the run recorded for the frame but for those.
fn bare_screen(field: &mut Field, states: &States, t: usize, marks: &[Mark]) -> Result<Vec<u16>, String> {
    let s = field.symbols();
    let (frame_count, pending) = (s.frame_count, s.render_pending);
    let built = marks[t - 1].0;
    let from = state_before(marks, t)?;
    let start = states.get(from);
    field.load(&start);
    field.hold_now(start.keys());
    let mut ran = 0;
    loop {
        field.hold(states.keys((from + ran + 1).min(states.len() - 1)));
        field.frames(1)?;
        ran += 1;
        if field.read32(frame_count) == built && field.read8(pending) != 0 {
            break;
        }
        if ran > t - from + LATE {
            return Err(format!("the game never finished frame {t}'s picture again"));
        }
    }
    field.until_line(LAST_BLANK_LINE).map_err(|e| format!("frame {t}: {e}"))?;
    hide_front_sprites(field);
    field.io_write16(DISPCNT, field.read16(DISPCNT) & !DISPCNT_BG0);
    field.frames(1)?;
    Ok(field.picture())
}

/// Draws on `picture` what the screen `shown` has that `bare` has not (the
/// message window, its portrait and letters): the screen's pixels where
/// they differ, their box placed as `place` says (Place).
fn dress(picture: &mut Image, shown: &[u16], bare: &[u16], place: Place) {
    let differs = |x: usize, y: usize| shown[y * WIDTH + x] != bare[y * WIDTH + x];
    let rows: Vec<i32> = (0..HEIGHT).filter(|&y| (0..WIDTH).any(|x| differs(x, y))).map(|y| y as i32).collect();
    let columns: Vec<i32> = (0..WIDTH).filter(|&x| (0..HEIGHT).any(|y| differs(x, y))).map(|x| x as i32).collect();
    let (Some(&y0), Some(&y1), Some(&x0), Some(&x1)) = (rows.first(), rows.last(), columns.first(), columns.last()) else { return };
    let (w, h) = (picture.width as i32, picture.height as i32);
    // The box stands as far from the picture's edges as it stands from the
    // screen's nearest edge, above or below it.
    let gap = y0.min(HEIGHT as i32 - 1 - y1);
    let ox = match place.across {
        Across::Left => gap - x0,
        Across::Centre => (w - (x1 - x0 + 1)) / 2 - x0,
        Across::Right => w - gap - 1 - x1,
    };
    let oy = if place.bottom { h - gap - 1 - y1 } else { gap - y0 };
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let k = y * WIDTH + x;
            if shown[k] == bare[k] {
                continue;
            }
            let (px, py) = (ox + x as i32, oy + y as i32);
            if px < 0 || py < 0 || px >= picture.width as i32 || py >= picture.height as i32 {
                continue;
            }
            picture.pixels[py as usize * picture.width + px as usize] = animation::rgb(shown[k]);
        }
    }
}

/// Writes whole pictures, each one frame of every two the GBA draws, as a
/// GIF at 30 frames a second that loops forever, every pixel in its own
/// colour: with one palette for the file (animation::write) when the
/// pictures have no more than 255 colours, else with a palette for each
/// frame of the colours in the rectangle that changed (a fade gives
/// thousands of colours, but no frame more than 255). Where one frame
/// alone has more than 255 colours (a whole map at night, with its
/// message window), the pictures are cut to 255 colours shared by the
/// file (quantize), and the note says so. Returns the file's bytes and a
/// note of its frames.
pub fn write_gif(path: &Path, pictures: &[Image]) -> Result<(u64, String), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let first = pictures.first().ok_or("no frames")?;
    let w = first.width;
    animation::check_black_clearance(&pictures.iter().map(|p| animation::is_black(&p.pixels)).collect::<Vec<_>>(), 2)?;
    // animation::write shows every second frame it is given: each
    // picture, then one it passes over.
    let one_palette = |pictures: &[Image]| {
        let frames: Vec<Vec<u32>> = pictures.iter().flat_map(|p| [p.pixels.clone(), Vec::new()]).collect();
        animation::write(path, &frames, w)
    };
    let all: HashSet<u32> = pictures.iter().flat_map(|p| p.pixels.iter().copied()).collect();
    if all.len() <= GIF_COLOURS {
        let written = one_palette(pictures)?;
        return Ok((written.bytes, format!("{} frames, {} colours", written.frames, written.colours)));
    }
    if let Some(written) = write_exact(path, pictures)? {
        return Ok(written);
    }
    let cut = quantize(pictures, GIF_COLOURS);
    let written = one_palette(&cut)?;
    Ok((written.bytes, format!("{} frames, cut to {} colours", written.frames, written.colours)))
}

/// The pictures with their colours cut to `n` by median cut: the colours
/// are boxed, the box of the widest range split at its median by weight
/// until there are `n`, and each colour takes its box's weighted mean.
fn quantize(pictures: &[Image], n: usize) -> Vec<Image> {
    let mut weights: HashMap<u32, usize> = HashMap::new();
    for c in pictures.iter().flat_map(|p| p.pixels.iter()) {
        *weights.entry(*c).or_default() += 1;
    }
    let channel = |c: u32, k: usize| (c >> (16 - 8 * k)) as usize & 0xff;
    let mut boxes: Vec<Vec<u32>> = vec![weights.keys().copied().collect()];
    while boxes.len() < n {
        // The box and channel of the widest range, among boxes that can split.
        let widest = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .flat_map(|(i, b)| (0..3).map(move |k| (i, k, b.iter().map(|&c| channel(c, k)).max().unwrap_or(0) - b.iter().map(|&c| channel(c, k)).min().unwrap_or(0))))
            .max_by_key(|&(_, _, range)| range);
        let Some((i, k, _)) = widest else { break };
        let mut b = std::mem::take(&mut boxes[i]);
        b.sort_unstable_by_key(|&c| channel(c, k));
        let total: usize = b.iter().map(|c| weights[c]).sum();
        let mut seen = 0;
        let mut at = b.len() - 1;
        for (j, c) in b.iter().enumerate() {
            seen += weights[c];
            if seen * 2 >= total {
                at = j.max(1).min(b.len() - 1);
                break;
            }
        }
        let rest = b.split_off(at);
        boxes[i] = b;
        boxes.push(rest);
    }
    let mut palette: HashMap<u32, u32> = HashMap::new();
    for b in &boxes {
        let total: usize = b.iter().map(|c| weights[c]).sum::<usize>().max(1);
        let mean = |k: usize| (b.iter().map(|&c| channel(c, k) * weights[&c]).sum::<usize>() + total / 2) / total;
        let colour = (mean(0) << 16 | mean(1) << 8 | mean(2)) as u32;
        for &c in b {
            palette.insert(c, colour);
        }
    }
    pictures.iter().map(|p| Image { width: p.width, height: p.height, pixels: p.pixels.iter().map(|c| palette[c]).collect() }).collect()
}

/// write_gif's frames each with a palette of the colours in the rectangle
/// that changed, every pixel in its own colour; None where a frame has more
/// than GIF_COLOURS of them.
fn write_exact(path: &Path, pictures: &[Image]) -> Result<Option<(u64, String)>, String> {
    let first = pictures.first().ok_or("no frames")?;
    let (w, h) = (first.width, first.height);
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
            return Ok(None);
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
    Ok(Some((bytes, format!("{} frames, {stored} stored", pictures.len()))))
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
    use super::{cut_frame, delta, quantize, steps, undelta, Across, Cut, Image, Place};

    /// The window's place is an edge, alone or with a corner.
    #[test]
    fn a_place_is_parsed() {
        assert_eq!(Cut::parse_place("top"), Ok(Place { bottom: false, across: Across::Centre }));
        assert_eq!(Cut::parse_place("bottom-right"), Ok(Place { bottom: true, across: Across::Right }));
        assert_eq!(Cut::parse_place("top-left"), Ok(Place { bottom: false, across: Across::Left }));
        assert!(Cut::parse_place("left").is_err());
        assert!(Cut::parse_place("top-middle").is_err());
    }

    /// Quantized pictures hold at most the colours asked for, and a
    /// picture within that many keeps every colour as it is.
    #[test]
    fn colours_are_cut_to_the_palette() {
        let pixels: Vec<u32> = (0..1000u32).map(|i| i * 0x010203 % 0x100_0000).collect();
        let cut = quantize(&[Image { width: 100, height: 10, pixels: pixels.clone() }], 16);
        let colours: std::collections::HashSet<u32> = cut[0].pixels.iter().copied().collect();
        assert!(colours.len() <= 16 && colours.len() > 1);
        let kept = quantize(&[Image { width: 2, height: 1, pixels: vec![0x102030, 0xfffefd] }], 16);
        assert_eq!(kept[0].pixels, vec![0x102030, 0xfffefd]);
    }

    /// A frame is four numbers with a positive size.
    #[test]
    fn a_frame_is_parsed() {
        assert_eq!(Cut::parse_frame("0,-16,480,270"), Ok([0, -16, 480, 270]));
        assert!(Cut::parse_frame("0,0,480").is_err());
        assert!(Cut::parse_frame("0,0,480,0").is_err());
        assert!(Cut::parse_frame("a,0,480,270").is_err());
    }

    /// A frame is cut in the room's pixels, the sky above the map counted
    /// from a negative top, and refused where it leaves the picture.
    #[test]
    fn a_frame_is_cut_from_the_room() {
        let picture = || Image { width: 4, height: 5, pixels: (0..20).collect() };
        let cut = cut_frame(picture(), [1, 0, 2, 2], 2).unwrap();
        assert_eq!((cut.width, cut.height, cut.pixels), (2, 2, vec![9, 10, 13, 14]));
        let sky = cut_frame(picture(), [0, -2, 4, 1], 2).unwrap();
        assert_eq!(sky.pixels, vec![0, 1, 2, 3]);
        assert!(cut_frame(picture(), [0, -3, 4, 1], 2).is_err());
        assert!(cut_frame(picture(), [3, 0, 2, 1], 2).is_err());
    }

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

