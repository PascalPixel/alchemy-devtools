//! How `render event` has the leader walk up to what he does by walking (a
//! step trigger, an exit, a cell he touches), as a player would come to it,
//! and climb what a vine or a ladder lets him: the moves between the cells he
//! can reach, the way up to the trigger along them, the climb an event leads
//! to, the keys held for it, and the check that refuses a recording whose
//! leader only stands there before it begins.

/// A cell of the map grid, (x, z) in cells.
pub type Cell = (i32, i32);

/// How the leader makes a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// Walking into the next cell, the direction key held.
    Walk,
    /// Up or down a vine, a ladder or a climbable wall: the key held on a
    /// climb cell (Field_RunPushCell's CELL_BEHAVIOR_CLIMB) until he stands
    /// on the climb cell at the other end.
    Climb,
    /// Down a ledge with Down held, or over a jump.
    Drop,
}

/// One move of the leader from a cell to another, with the keys held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move {
    pub from: Cell,
    pub to: Cell,
    pub keys: u32,
    pub how: How,
}

/// The fewest cells a walk up to a trigger takes: fewer is no walk, and the
/// recorder falls back to standing the leader beside the trigger.
pub const APPROACH_LEAST: usize = 2;
/// The fewest the walk is made of where the lead-in asks for less: a few
/// cells back, as a player comes to it.
pub const APPROACH_CELLS: usize = 4;
/// Frames the leader takes to walk a cell, at his walking pace of about a
/// pixel a frame (measured in Tret Tree: 86 pixels in 96 frames from a stand,
/// slower up a slope); a walk of lead_in / FRAMES_PER_CELL cells fills the
/// lead-in.
pub const FRAMES_PER_CELL: u32 = 16;

/// The way up to `spot` along `moves`: the moves from a start cell to the
/// spot, the start as near as it can be to `wanted` cells away and no
/// nearer than `least`, where the walk is shortest past `wanted` and turns
/// least. The last move comes along `last` (the keys that then push on into
/// the trigger) where a cell lies that way, so the leader walks straight in.
/// None when no start lies `least` cells away.
pub fn approach(moves: &[Move], spot: Cell, last: u32, wanted: usize, least: usize) -> Option<Vec<Move>> {
    use std::collections::{HashMap, VecDeque};
    // The moves into each cell, the straight one first, walks before climbs.
    let mut into: HashMap<Cell, Vec<&Move>> = HashMap::new();
    for m in moves {
        into.entry(m.to).or_default().push(m);
    }
    for list in into.values_mut() {
        list.sort_by_key(|m| (m.keys != last, m.how != How::Walk, m.from));
    }
    let mut via: HashMap<Cell, (usize, Move)> = HashMap::new();
    let mut queue = VecDeque::from([(spot, 0usize)]);
    while let Some((cell, distance)) = queue.pop_front() {
        for m in into.get(&cell).map(Vec::as_slice).unwrap_or(&[]) {
            if m.from != spot && !via.contains_key(&m.from) {
                via.insert(m.from, (distance + 1, **m));
                queue.push_back((m.from, distance + 1));
            }
        }
    }
    let path = |start: Cell| -> Vec<Move> {
        let mut out = Vec::new();
        let mut at = start;
        while let Some(&(_, m)) = via.get(&at) {
            out.push(m);
            at = m.to;
        }
        out
    };
    let turns = |p: &[Move]| p.windows(2).filter(|w| w[0].keys != w[1].keys).count();
    let far = via.values().map(|v| v.0).max().filter(|&d| d >= least)?;
    let length = if far >= wanted { via.values().map(|v| v.0).filter(|&d| d >= wanted).min()? } else { far };
    let mut starts: Vec<Cell> = via.iter().filter(|(_, v)| v.0 == length).map(|(c, _)| *c).collect();
    starts.sort_unstable();
    starts.into_iter().map(path).min_by_key(|p| turns(p))
}

/// The shortest way along `moves` from `from` to `to`, or None.
pub fn route(moves: &[Move], from: Cell, to: Cell) -> Option<Vec<Move>> {
    use std::collections::{HashMap, VecDeque};
    let mut out_of: HashMap<Cell, Vec<&Move>> = HashMap::new();
    for m in moves {
        out_of.entry(m.from).or_default().push(m);
    }
    let mut via: HashMap<Cell, Move> = HashMap::new();
    let mut queue = VecDeque::from([from]);
    while let Some(cell) = queue.pop_front() {
        if cell == to {
            let mut path = Vec::new();
            let mut at = to;
            while let Some(&m) = via.get(&at) {
                path.push(m);
                at = m.from;
            }
            path.reverse();
            return Some(path);
        }
        for m in out_of.get(&cell).map(Vec::as_slice).unwrap_or(&[]) {
            if m.to != from && !via.contains_key(&m.to) {
                via.insert(m.to, **m);
                queue.push_back(m.to);
            }
        }
    }
    None
}

/// A walk along `moves` from `from` on into the room: to a cell `wanted`
/// moves away, or the farthest there is, the way that turns least. None
/// when no cell lies two moves away.
pub fn away(moves: &[Move], from: Cell, wanted: usize) -> Option<Vec<Move>> {
    use std::collections::{HashMap, VecDeque};
    let mut out_of: HashMap<Cell, Vec<&Move>> = HashMap::new();
    for m in moves.iter().filter(|m| m.how == How::Walk) {
        out_of.entry(m.from).or_default().push(m);
    }
    let mut via: HashMap<Cell, (usize, Move)> = HashMap::new();
    let mut queue = VecDeque::from([(from, 0usize)]);
    while let Some((cell, distance)) = queue.pop_front() {
        if distance == wanted {
            continue;
        }
        for m in out_of.get(&cell).map(Vec::as_slice).unwrap_or(&[]) {
            if m.to != from && !via.contains_key(&m.to) {
                via.insert(m.to, (distance + 1, **m));
                queue.push_back((m.to, distance + 1));
            }
        }
    }
    let far = via.values().map(|v| v.0).max().filter(|&d| d >= APPROACH_LEAST)?;
    let path = |end: Cell| -> Vec<Move> {
        let mut out = Vec::new();
        let mut at = end;
        while let Some(&(_, m)) = via.get(&at) {
            out.push(m);
            at = m.from;
        }
        out.reverse();
        out
    };
    let turns = |p: &[Move]| p.windows(2).filter(|w| w[0].keys != w[1].keys).count();
    let mut ends: Vec<Cell> = via.iter().filter(|(_, v)| v.0 == far).map(|(c, _)| *c).collect();
    ends.sort_unstable();
    ends.into_iter().map(path).min_by_key(|p| turns(p))
}

/// `path` begun `cells` moves farther back, where the way up to the trigger
/// is too short to fill the lead-in: from a cell off it, out along a side
/// way where one leads off its start, else back along the path itself (a
/// walk to and fro), so the leader is seen walking rather than standing.
/// The path as it was when nothing lies farther.
pub fn lengthen(moves: &[Move], path: &[Move], cells: usize) -> Vec<Move> {
    let Some(start) = path.first().map(|m| m.from) else {
        return path.to_vec();
    };
    let on_path: Vec<Cell> = path.iter().map(|m| m.to).collect();
    let aside: Vec<Move> = moves.iter().filter(|m| !on_path.contains(&m.to) && !on_path.contains(&m.from)).copied().collect();
    let out = away(&aside, start, cells).filter(|w| w.len() * 2 >= cells).or_else(|| away(moves, start, cells));
    let Some(back) = out.and_then(|w| route(moves, w.last()?.to, start)) else {
        return path.to_vec();
    };
    back.into_iter().chain(path.iter().copied()).collect()
}

/// The climb `moves` let the leader make from `from`: up to `walks` walks
/// and then a climb, fewest walks first. None when no climb is that near.
pub fn onward(moves: &[Move], from: Cell, walks: usize) -> Option<Vec<Move>> {
    use std::collections::{HashMap, VecDeque};
    let mut out_of: HashMap<Cell, Vec<&Move>> = HashMap::new();
    for m in moves {
        out_of.entry(m.from).or_default().push(m);
    }
    let mut via: HashMap<Cell, Move> = HashMap::new();
    let mut queue = VecDeque::from([(from, 0usize)]);
    while let Some((cell, distance)) = queue.pop_front() {
        let here = out_of.get(&cell).map(Vec::as_slice).unwrap_or(&[]);
        if let Some(climb) = here.iter().find(|m| m.how == How::Climb) {
            let mut path = vec![**climb];
            let mut at = cell;
            while let Some(&m) = via.get(&at) {
                path.push(m);
                at = m.from;
            }
            path.reverse();
            return Some(path);
        }
        if distance == walks {
            continue;
        }
        for m in here.iter().filter(|m| m.how == How::Walk) {
            if m.to != from && !via.contains_key(&m.to) {
                via.insert(m.to, **m);
                queue.push_back((m.to, distance + 1));
            }
        }
    }
    None
}

/// What a recording's row says of the way up to its trigger, where the page
/// needs it to make sense of the picture: a way that climbs or drops is most
/// of what the recording shows, and `padding` frames stood at its start (a
/// way shorter than a black screen's lead-in) are seconds of nothing
/// happening. None for a plain walk.
pub fn describe(path: &[Move], padding: u32) -> Option<String> {
    let mut parts = Vec::new();
    let climbs: Vec<&Move> = path.iter().filter(|m| m.how == How::Climb).collect();
    if let Some(first) = climbs.first() {
        let way = if first.keys == 0x40 { "up" } else { "down" };
        let key = if first.keys == 0x40 { "Up" } else { "Down" };
        let cells = if climbs.len() == 1 { "a cell".to_string() } else { format!("{} cells", climbs.len()) };
        parts.push(format!("the recording begins with the way up to it, which climbs {way} a vine or ladder ({key} held, {cells} of climbing)"));
    } else if path.iter().any(|m| m.how == How::Drop) {
        parts.push("the recording begins with the way up to it, which drops down a ledge".to_string());
    }
    // Begun farther back than the way up goes (lengthen): he passes a cell
    // twice, walking off from it and back.
    if path.iter().enumerate().any(|(i, m)| path[i + 1..].iter().any(|n| n.to == m.to)) {
        parts.push("the way up is shorter than ten seconds before the black screen, so he walks a way off along it and back first".to_string());
    }
    if padding >= 60 {
        parts.push(format!(
            "he stands at the start of the way for {:.1} seconds before walking {} cells up to it, so that ten seconds of the game come before the black screen",
            f64::from(padding) / 60.0,
            path.len()
        ));
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

/// Frames before a trigger looked at for the leader standing still: shorter
/// lead-ins say nothing either way.
const STILL_LEAST: usize = 8;

/// What the leader did in a frame before his event began.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    /// He stood still.
    Stood,
    /// He walked or climbed.
    Walked,
    /// He held a direction into a cell whose row fires only once he has
    /// pushed against it a while (STEP_TRIGGER_FLAG_WALKING: a ladder's foot,
    /// a pillar, the edge of a gap), without moving: the game's own wait
    /// before what he does, which a player sees him push through.
    Pushed,
}

/// Why a recording of something the leader does by walking shows him
/// standing instead, from what he did in each frame before its event began:
/// he stood still for most of them. Pushing into the cell counts as doing
/// it. None when he walked or pushed.
pub fn stands_still(frames: &[Frame]) -> Option<String> {
    let stood = frames.iter().filter(|&&f| f == Frame::Stood).count();
    (frames.len() >= STILL_LEAST && stood * 2 > frames.len())
        .then(|| format!("the leader stands still for {stood} of the {} frames before it begins, so the recording shows nothing coming to it", frames.len()))
}

/// Pixels of a frame that may differ from another's with the two still
/// showing the same: a stray pixel of a sparkle or a ripple.
const SAME_PIXELS: usize = 24;
/// Frames that may differ (by more than SAME_PIXELS) with a recording still
/// showing the same as another.
const SAME_FRAMES: usize = 2;

/// Whether the frames `played` show what `idle` (the same moment played
/// with the same keys and the event's rows taken out of the scene's table)
/// shows, frame by frame: the event changed nothing on the screen. Water and
/// cycling colours run alike in both.
pub fn looks_the_same(played: &[Vec<u16>], idle: &[Vec<u16>]) -> bool {
    let differ = played.iter().zip(idle).filter(|(a, b)| a.len() != b.len() || a.iter().zip(b.iter()).filter(|(p, q)| p != q).count() > SAME_PIXELS).count();
    differ <= SAME_FRAMES
}

/// A pad script: keys (KEYINPUT bits) held for so many frames, one pair
/// after another, as MOMENT_HOLD takes it (`keys,frames,keys,frames...`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pad(pub Vec<(u32, u32)>);

impl Pad {
    pub fn parse(text: &str) -> Result<Pad, String> {
        let numbers = text.split(',').map(str::trim).filter(|v| !v.is_empty()).map(|v| v.parse::<u32>().map_err(|e| format!("{v}: {e}"))).collect::<Result<Vec<u32>, String>>()?;
        if numbers.len() % 2 != 0 {
            return Err(format!("a pad script is keys and frames in pairs; {text} has an odd number"));
        }
        Ok(Pad(numbers.chunks(2).map(|p| (p[0], p[1])).collect()))
    }

    /// Adds `keys` held for one more frame.
    pub fn push(&mut self, keys: u32) {
        match self.0.last_mut() {
            Some((k, n)) if *k == keys => *n += 1,
            _ => self.0.push((keys, 1)),
        }
    }
}

impl std::fmt::Display for Pad {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let pairs: Vec<String> = self.0.iter().map(|(k, n)| format!("{k},{n}")).collect();
        write!(f, "{}", pairs.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RIGHT: u32 = 0x10;
    const LEFT: u32 = 0x20;
    const UP: u32 = 0x40;
    const DOWN: u32 = 0x80;

    /// Walks both ways between the open cells of `grid` (`.` open, `#` not,
    /// `C` a climb cell), and climbs between climb cells one above another.
    fn moves(grid: &[&str]) -> Vec<Move> {
        let at = |x: i32, z: i32| grid.get(z as usize).and_then(|r| r.as_bytes().get(x as usize)).copied().unwrap_or(b'#');
        let mut out = Vec::new();
        for (z, row) in grid.iter().enumerate() {
            for x in 0..row.len() as i32 {
                let (z, c) = (z as i32, at(x, z as i32));
                if c == b'#' {
                    continue;
                }
                for (dx, dz, keys) in [(0, -1, UP), (1, 0, RIGHT), (0, 1, DOWN), (-1, 0, LEFT)] {
                    let n = at(x + dx, z + dz);
                    if n == b'#' {
                        continue;
                    }
                    let how = if c == b'C' && n == b'C' && dx == 0 { How::Climb } else { How::Walk };
                    out.push(Move { from: (x, z), to: (x + dx, z + dz), keys, how });
                }
            }
        }
        out
    }

    #[test]
    fn the_leader_walks_up_from_a_few_cells_back_straight_in() {
        // A branch running east to a trunk at its end, the trigger's spot at
        // (6, 1), pushed into eastward.
        let grid = ["########", "#......#", "#.######", "#.######"];
        let path = approach(&moves(&grid), (6, 1), RIGHT, 4, APPROACH_LEAST).unwrap();
        assert_eq!(path.len(), 4);
        assert_eq!(path[0].from, (2, 1));
        assert!(path.iter().all(|m| m.keys == RIGHT));
        assert_eq!(path.last().unwrap().to, (6, 1));
        // A longer lead-in walks further, round the corner.
        let path = approach(&moves(&grid), (6, 1), RIGHT, 7, APPROACH_LEAST).unwrap();
        assert_eq!(path[0].from, (1, 3));
        assert_eq!(path.len(), 7);
        // With more asked than the branch holds, all of it.
        assert_eq!(approach(&moves(&grid), (6, 1), RIGHT, 30, APPROACH_LEAST).unwrap().len(), 7);
    }

    #[test]
    fn a_trigger_with_no_walk_up_to_it_has_no_approach() {
        let grid = ["####", "#..#", "####"];
        assert_eq!(approach(&moves(&grid), (2, 1), RIGHT, 4, APPROACH_LEAST), None);
    }

    #[test]
    fn the_way_up_climbs_a_vine() {
        // The upper branch (row 0) reached only up the vine (column 1) from
        // the lower one (row 2).
        let grid = ["#C..", "#C##", "....", "####"];
        let all = moves(&grid);
        let path = approach(&all, (2, 0), RIGHT, 4, APPROACH_LEAST).unwrap();
        assert!(path.iter().any(|m| m.how == How::Climb && m.keys == UP));
        assert_eq!(path.last().unwrap(), &Move { from: (1, 0), to: (2, 0), keys: RIGHT, how: How::Walk });
        // Set down beside the vine's top, the climb down is a step away.
        let on = onward(&all, (2, 0), 3).unwrap();
        assert_eq!(on.last().unwrap().how, How::Climb);
        assert_eq!(on.last().unwrap().keys, DOWN);
        assert_eq!(on[0], Move { from: (2, 0), to: (1, 0), keys: LEFT, how: How::Walk });
        assert_eq!(onward(&all, (3, 2), 0), None);
        // Come from the far end of the lower branch, the way leads up the
        // vine: its last four moves are the walk to it, the climb and the
        // step off its top.
        let way = route(&all, (3, 2), (2, 0)).unwrap();
        assert_eq!(way.len(), 5);
        assert_eq!(way[way.len() - 2].how, How::Climb);
        assert_eq!(route(&all, (2, 0), (0, 0)), None);
    }

    #[test]
    fn after_arriving_he_walks_on_into_the_room() {
        // In at the door of a room four cells across: he walks on, the
        // straightest way, as far as asked or as the room goes.
        let grid = ["######", "#....#", "#....#", "##.###"];
        let all = moves(&grid);
        let walk = away(&all, (2, 3), 2).unwrap();
        assert_eq!(walk.len(), 2);
        assert!(walk.iter().all(|m| m.keys == UP));
        let walk = away(&all, (2, 3), 30).unwrap();
        assert_eq!(walk.len(), 4);
        assert_eq!(walk[0].from, (2, 3));
        // A cell with nowhere to go has no walk.
        assert_eq!(away(&moves(&["###", "#.#", "###"]), (1, 1), 4), None);
    }

    #[test]
    fn a_way_too_short_is_begun_farther_back() {
        // A branch two cells long up to the trigger at (3, 1), with a side
        // way south from its start.
        let grid = ["#####", "#...#", "#.###", "#.###", "#.###"];
        let all = moves(&grid);
        // Two cells asked to be five: begun three cells down the side way,
        // he walks up it and along the branch.
        let short = vec![Move { from: (1, 1), to: (2, 1), keys: RIGHT, how: How::Walk }, Move { from: (2, 1), to: (3, 1), keys: RIGHT, how: How::Walk }];
        let longer = lengthen(&all, &short, 3);
        assert_eq!(longer.len(), 5);
        assert_eq!(longer[0].from, (1, 4));
        assert_eq!(&longer[3..], &short[..]);
        // Nowhere to go: as it was.
        let lone = moves(&["####", "#..#", "####"]);
        let step = vec![Move { from: (1, 1), to: (2, 1), keys: RIGHT, how: How::Walk }];
        assert_eq!(lengthen(&lone, &step, 4), step);
    }

    #[test]
    fn a_leader_who_never_moves_is_refused_and_one_who_walks_up_is_kept() {
        use Frame::{Stood, Walked};
        assert!(stands_still(&[Stood; 40]).is_some());
        // Standing twelve frames, then walking a cell onto the trigger.
        let walked: Vec<Frame> = std::iter::repeat_n(Stood, 12).chain(std::iter::repeat_n(Walked, 16)).collect();
        assert_eq!(stands_still(&walked), None);
        // Ten seconds stood before a short step is standing.
        let stood: Vec<Frame> = std::iter::repeat_n(Stood, 600).chain(std::iter::repeat_n(Walked, 16)).collect();
        assert!(stands_still(&stood).unwrap().contains("600 of the 616"));
        assert_eq!(stands_still(&[Stood; 4]), None);
    }

    #[test]
    fn pushing_into_a_cell_that_waits_for_a_push_is_doing_it() {
        use Frame::{Pushed, Stood, Walked};
        // Started beside Goma Cave's gap: three frames turning, then
        // twenty-seven of West held into the cell before the jump.
        let pushed: Vec<Frame> = std::iter::repeat_n(Stood, 3).chain(std::iter::repeat_n(Pushed, 27)).collect();
        assert_eq!(stands_still(&pushed), None);
        // A walk of two cells to a ladder's foot, then the push up it.
        let walked: Vec<Frame> = std::iter::repeat_n(Walked, 32).chain(std::iter::repeat_n(Pushed, 27)).collect();
        assert_eq!(stands_still(&walked), None);
        // Standing is still standing however it ends.
        let stood: Vec<Frame> = std::iter::repeat_n(Stood, 40).chain(std::iter::repeat_n(Pushed, 10)).collect();
        assert!(stands_still(&stood).is_some());
    }

    #[test]
    fn a_way_that_climbs_says_so_and_a_plain_walk_says_nothing() {
        let grid = ["#C..", "#C##", "....", "####"];
        let all = moves(&grid);
        let up = route(&all, (3, 2), (2, 0)).unwrap();
        let said = describe(&up, 0).unwrap();
        assert!(said.contains("climbs up a vine or ladder (Up held"), "{said}");
        let walk = route(&all, (3, 2), (0, 2)).unwrap();
        assert_eq!(describe(&walk, 12), None);
        // Begun farther back, back along itself.
        let there = route(&all, (0, 2), (3, 2)).unwrap();
        let fro: Vec<Move> = route(&all, (3, 2), (0, 2)).unwrap().into_iter().chain(there).collect();
        assert!(describe(&fro, 0).unwrap().contains("walks a way off along it and back"));
        // Stood at the start for the rest of a black screen's ten seconds.
        let said = describe(&walk, 480).unwrap();
        assert!(said.contains("8.0 seconds before walking 3 cells"), "{said}");
    }

    #[test]
    fn standing_at_the_start_before_walking_up_is_not_standing_at_the_trigger() {
        // As Field::event judges it: the frames stood at the start of a way
        // shorter than the lead-in are left out, the walk is kept.
        let stood = vec![Frame::Stood; 500];
        assert!(stands_still(&stood).is_some());
        let walk: Vec<Frame> = std::iter::repeat_n(Frame::Walked, 48).chain(std::iter::repeat_n(Frame::Stood, 16)).collect();
        assert_eq!(stands_still(&walk), None);
    }

    #[test]
    fn an_event_that_changes_nothing_on_the_screen_looks_the_same() {
        let still = vec![vec![7u16; 64]; 30];
        assert!(looks_the_same(&still, &still));
        // A leader who jumps: thirty pixels of him elsewhere for ten frames.
        let mut jumped = still.clone();
        for frame in &mut jumped[10..20] {
            frame[..30].fill(1);
        }
        assert!(!looks_the_same(&jumped, &still));
        // A sparkle of a few pixels in a frame or two is no change.
        let mut sparkled = still.clone();
        sparkled[3][..30].fill(1);
        sparkled[4][..5].fill(1);
        assert!(looks_the_same(&sparkled, &still));
    }

    #[test]
    fn pad_scripts_read_and_write_as_moment_hold_takes_them() {
        let pad = Pad::parse("32,20, 64,150").unwrap();
        assert_eq!(pad, Pad(vec![(32, 20), (64, 150)]));
        assert_eq!(pad.to_string(), "32,20,64,150");
        assert!(Pad::parse("32").is_err());
        let mut held = Pad::default();
        for keys in [LEFT, LEFT, UP] {
            held.push(keys);
        }
        assert_eq!(held, Pad(vec![(LEFT, 2), (UP, 1)]));
    }
}
