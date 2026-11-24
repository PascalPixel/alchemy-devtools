//! What `render event` is told to stage around an event, beyond what it
//! finds itself: story flags set or cleared (`--set`, `--clear`), items
//! given (`--give`), the way in (`--entrance`), the cell the leader starts
//! on and the way he faces (`--at`, `--face`), and keys held before the
//! trigger and once the player has control again (`--hold`, `--then`). And
//! the flags an event's own function needs before it does anything (its
//! guard), read from its source.

use std::collections::HashMap;

/// The KEYINPUT bits a pad script names.
const KEY_NAMES: [(&str, u32); 10] = [
    ("a", 1),
    ("b", 2),
    ("select", 4),
    ("start", 8),
    ("right", 0x10),
    ("left", 0x20),
    ("up", 0x40),
    ("down", 0x80),
    ("r", 0x100),
    ("l", 0x200),
];

/// One step of a pad script: keys held (KEYINPUT bits, 0 for none) for a
/// number of frames, or a Psynergy cast (`cast:hover`, `cast:move:east`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub keys: u32,
    pub frames: u32,
    /// The Psynergy cast at this step, where the leader stands and faces,
    /// as the field's shortcut keys cast it (FieldState.psynergy); `keys`
    /// are then the way held after it for `frames` frames (a Move
    /// steered), none for none.
    pub cast: Option<Cast>,
}

/// A Psynergy a pad script casts: its name as given and its number in the
/// game's ability_ids.h, 0 until Stage::resolve has read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cast {
    pub name: String,
    pub id: u16,
}

impl Held {
    /// Keys held for frames, no cast.
    pub const fn keys(keys: u32, frames: u32) -> Held {
        Held { keys, frames, cast: None }
    }
}

/// The bit of a frame of keys (script_frames) that marks a cast: the rest
/// is the ability's number.
pub const CAST_FRAME: u32 = 1 << 31;

/// The bit of a frame of keys that marks the way held after a cast: it is
/// held only once the cast's hand grips (Move's, Carry's: the game reads
/// the d-pad while the cast runs), so the script waits for that first.
pub const CAST_WAY: u32 = 1 << 30;

/// Frames a cast's way is held after it when `cast:ABILITY:WAY` gives
/// none: the hand of Move waits for the d-pad.
pub const CAST_WAY_FRAMES: u32 = 60;

/// What `render event`'s staging options ask for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stage {
    /// Story flags set and cleared for every try, by name or 0x number.
    pub set: Vec<String>,
    pub clear: Vec<String>,
    /// Items put in the leader's bag, by name (ITEM_ or not) or number.
    pub give: Vec<String>,
    /// The only entrance the scene is entered by.
    pub entrance: Option<u16>,
    /// The cell the leader starts on, and its map layer when given.
    pub at: Option<(i32, i32, Option<u8>)>,
    /// The way he faces there (a facing angle, 0 east, 0x4000 south).
    pub face: Option<u32>,
    /// Keys held before the trigger is given: for a touch or a raised
    /// trigger they are what gives it.
    pub hold: Vec<Held>,
    /// Keys held once the player has control again.
    pub then: Vec<Held>,
    /// The keys that answer each prompt in turn (a yes or no choice, a
    /// menu of icons): A for yes, B for no.
    pub answer: Vec<u32>,
    /// The character who leads the party, and the party itself, in order,
    /// by name (character_ids.h, FELIX or felix).
    pub leader: Option<String>,
    pub party: Vec<String>,
    /// An event played first, unrecorded, each event then recorded from
    /// where it leaves the player (--first).
    pub first: Option<String>,
    /// The start, the way faced and the keys of the event played first
    /// (--first-at, --first-face, --first-hold, --first-then): --at and
    /// the rest stage the recorded event alone.
    pub first_at: Option<(i32, i32, Option<u8>)>,
    pub first_face: Option<u32>,
    pub first_hold: Vec<Held>,
    pub first_then: Vec<Held>,
    /// The pixel the leader starts on, and its layer (--at-px): finer than
    /// --at's cell.
    pub at_px: Option<(i32, i32, Option<u8>)>,
    /// The height a staged walk puts the leader at (--at-height, pixels):
    /// where a room's cells change with the height he stands at (Air's
    /// Rock's levels), the ground they give before he is there is not his.
    pub at_height: Option<i32>,
    /// A staged walk arrives as after a battle fought where it starts
    /// (--arrive-as after-battle).
    pub after_battle: bool,
    /// A staged walk plays on, the player still, until the game reaches
    /// this scene function itself (--until FUNCTION): a fireball a statue
    /// throws, a timer the scene keeps.
    pub until: Option<String>,
    /// An event's recording follows up to this many scene changes, each as
    /// long as --limit lets it (--follow-scenes N).
    pub follow_scenes: u32,
    /// Keys held from the moment the event begins, while it runs and
    /// before the player has control again (--during).
    pub during: Vec<Held>,
    /// Functions of the scene made to return at once (--skip).
    pub skip: Vec<String>,
    /// The most seconds a recording runs before it is given up
    /// (--limit; EVENT_LIMIT's 240 when none).
    pub limit: Option<u32>,
    /// The window of the recording written, in frames: from this one, for
    /// this many (--from, --for, given in seconds).
    pub from: Option<u32>,
    pub length: Option<u32>,
    /// A recording with under ten seconds between two screen changes is
    /// written anyway, its note saying so (--keep-short).
    pub keep_short: bool,
    /// The cell a staged walk first walks the leader to, the way planned
    /// as the player would walk it (--walk-to X,Y[,LAYER]).
    pub walk_to: Option<(i32, i32, Option<u8>)>,
    /// The part of the screen written (--crop X,Y,W,H, screen pixels), the
    /// camera held still from the keys' start, each pixel drawn `zoom`
    /// times as wide and high (--zoom; 2 with a crop when none).
    pub crop: Option<[i32; 4]>,
    pub zoom: Option<u32>,
}

impl Stage {
    /// Whether the leader's start is staged by hand: he starts at `--at`,
    /// or the trigger comes from `--hold`, and the walk-up is not planned.
    pub fn staged(&self) -> bool {
        self.start().is_some() || !self.hold.is_empty()
    }

    /// The cell the leader starts on, and its layer: --at's, or the cell
    /// --at-px's pixel lies in.
    pub fn start(&self) -> Option<(i32, i32, Option<u8>)> {
        self.at.or(self.at_px.map(|(x, z, layer)| (x.div_euclid(16), z.div_euclid(16), layer)))
    }

    /// The same staging for the event --first plays: its own start, way
    /// and keys, the flags, items and party shared.
    pub fn for_first(&self) -> Stage {
        Stage {
            at: self.first_at,
            at_px: None,
            face: self.first_face,
            hold: self.first_hold.clone(),
            then: self.first_then.clone(),
            during: Vec::new(),
            first: None,
            first_at: None,
            first_face: None,
            first_hold: Vec::new(),
            first_then: Vec::new(),
            from: None,
            length: None,
            ..self.clone()
        }
    }

    /// The frames the recording written keeps of `total` (--from, --for):
    /// the first and one past the last.
    pub fn window(&self, total: usize) -> (usize, usize) {
        let start = self.from.map_or(0, |f| f as usize).min(total);
        let end = self.length.map_or(total, |l| start + l as usize).min(total);
        (start, end.max(start))
    }

    /// Whether only a window of the recording is written (--from,
    /// --for): one that runs past the limit is written all the same.
    pub fn windowed(&self) -> bool {
        self.from.is_some() || self.length.is_some()
    }

    /// The most frames a recording runs (--limit), `default` when none.
    pub fn limit_frames(&self, default: u32) -> u32 {
        self.limit.map_or(default, |s| s.saturating_mul(60))
    }

    /// Reads each cast's ability number from the game's ability_ids.h
    /// (`header`'s text): `hover`, `HOVER` or `ABILITY_HOVER`.
    pub fn resolve(&mut self, header: &str) -> Result<(), String> {
        for script in [&mut self.hold, &mut self.then, &mut self.during, &mut self.first_hold, &mut self.first_then] {
            for step in script.iter_mut() {
                if let Some(cast) = step.cast.as_mut() {
                    cast.id = ability_id(header, &cast.name)?;
                }
            }
        }
        Ok(())
    }

    /// Whether, with no event named, the run is a walk: the keys played
    /// from the start given, recorded, whatever they set off.
    pub fn walks(&self, events: &[String]) -> bool {
        events.is_empty() && (self.start().is_some() || !self.hold.is_empty() || !self.then.is_empty() || self.walk_to.is_some() || self.until.is_some())
    }

    /// Reads one staging option and its value; false when `option` is none.
    pub fn option(&mut self, option: &str, value: &mut dyn FnMut(&str) -> Result<String, String>) -> Result<bool, String> {
        match option {
            "--set" => self.set.push(value(option)?),
            "--clear" => self.clear.push(value(option)?),
            "--give" => self.give.push(value(option)?),
            "--entrance" => {
                let v = value(option)?;
                self.entrance = Some(v.parse().map_err(|_| format!("--entrance takes a number, not {v:?}"))?);
            }
            "--at" => self.at = Some(parse_cell(&value(option)?)?),
            "--face" => self.face = Some(parse_face(&value(option)?)?),
            "--hold" => self.hold.extend(parse_script(&value(option)?)?),
            "--then" => self.then.extend(parse_script(&value(option)?)?),
            "--answer" => self.answer.extend(parse_answers(&value(option)?)?),
            "--leader" => self.leader = Some(value(option)?),
            "--first" => self.first = Some(value(option)?),
            "--party" => self.party.extend(value(option)?.split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty())),
            "--first-at" => self.first_at = Some(parse_cell(&value(option)?)?),
            "--first-face" => self.first_face = Some(parse_face(&value(option)?)?),
            "--first-hold" => self.first_hold.extend(parse_script(&value(option)?)?),
            "--first-then" => self.first_then.extend(parse_script(&value(option)?)?),
            "--until" => self.until = Some(value(option)?.trim().to_string()),
            "--follow-scenes" => {
                let v = value(option)?;
                self.follow_scenes = v.trim().parse().ok().filter(|&n| (1..=8).contains(&n)).ok_or_else(|| format!("--follow-scenes takes a number of scene changes, 1 to 8, not {v:?}"))?;
            }
            "--arrive-as" => {
                let how = value(option)?;
                self.after_battle = match how.trim() {
                    "after-battle" => true,
                    "entrance" => false,
                    other => return Err(format!("--arrive-as takes after-battle or entrance, not {other}")),
                };
            }
            "--at-height" => self.at_height = Some(value(option)?.trim().parse().map_err(|_| "--at-height takes a height in pixels, as 208".to_string())?),
            "--at-px" => self.at_px = Some(parse_cell(&value(option)?).map_err(|e| e.replace("--at takes a cell as X,Y or X,Y,LAYER", "--at-px takes a pixel as X,Z or X,Z,LAYER"))?),
            "--during" => self.during.extend(parse_script(&value(option)?)?),
            "--skip" => self.skip.push(value(option)?),
            "--limit" => {
                let v = value(option)?;
                self.limit = Some(v.parse().ok().filter(|&s: &u32| s > 0).ok_or_else(|| format!("--limit takes a number of seconds, not {v:?}"))?);
            }
            "--from" | "--for" => {
                let v = value(option)?;
                let seconds: f64 = v.parse().ok().filter(|s: &f64| s.is_finite() && *s >= 0.0).ok_or_else(|| format!("{option} takes a number of seconds, not {v:?}"))?;
                let frames = Some((seconds * 60.0).round() as u32);
                if option == "--from" { self.from = frames } else { self.length = frames }
            }
            "--keep-short" => self.keep_short = true,
            "--crop" => self.crop = Some(parse_crop(&value(option)?)?),
            "--zoom" => {
                let v = value(option)?;
                self.zoom = Some(v.parse().ok().filter(|z: &u32| (1..=4).contains(z)).ok_or_else(|| format!("--zoom takes 1, 2, 3 or 4, not {v:?}"))?);
            }
            "--frames" => {
                let v = value(option)?;
                let (a, b) = parse_frames(&v)?;
                self.from = Some(a);
                self.length = Some(b - a + 1);
            }
            "--walk-to" => self.walk_to = Some(parse_cell(&value(option)?).map_err(|e| e.replace("--at takes", "--walk-to takes"))?),
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// The options again, as a child of --jobs is given them.
    pub fn is_option(option: &str) -> bool {
        OPTIONS.contains(&option)
    }
}

/// Every staging option `render event` reads (Stage::option).
pub const OPTIONS: [&str; 31] = [
    "--set", "--clear", "--give", "--entrance", "--at", "--face", "--hold", "--then", "--answer", "--leader", "--party", "--first", "--first-at", "--first-face", "--first-hold", "--first-then", "--at-px", "--during", "--skip", "--limit", "--from", "--for", "--keep-short", "--walk-to", "--at-height", "--arrive-as", "--until", "--follow-scenes", "--crop", "--zoom", "--frames",
];

/// The screen's size, which a crop lies within.
const SCREEN: [i32; 2] = [240, 160];

/// `X,Y,W,H`: a rectangle of the screen, in its pixels, within it.
pub fn parse_crop(text: &str) -> Result<[i32; 4], String> {
    let bad = || format!("--crop takes X,Y,W,H in the screen's pixels (240 by 160), not {text:?}");
    let n: Vec<i32> = text.split(',').map(|p| p.trim().parse().map_err(|_| bad())).collect::<Result<_, _>>()?;
    match n[..] {
        [x, y, w, h] if x >= 0 && y >= 0 && w > 0 && h > 0 && x + w <= SCREEN[0] && y + h <= SCREEN[1] => Ok([x, y, w, h]),
        _ => Err(bad()),
    }
}

/// `A-B`: the first and last frame of a recording kept, 60 a second.
pub fn parse_frames(text: &str) -> Result<(u32, u32), String> {
    let bad = || format!("--frames takes A-B, the first and last frame kept (60 a second), not {text:?}");
    let (a, b) = text.split_once('-').ok_or_else(bad)?;
    let (a, b): (u32, u32) = (a.trim().parse().map_err(|_| bad())?, b.trim().parse().map_err(|_| bad())?);
    if b < a {
        return Err(bad());
    }
    Ok((a, b))
}

/// `frame`, a screen's pixels in rows of `width`, cut to `crop` (X, Y, W,
/// H) and each pixel drawn `zoom` times as wide and high.
pub fn crop_frame(frame: &[u16], width: usize, crop: [i32; 4], zoom: usize) -> Vec<u16> {
    let [x, y, w, h] = crop.map(|v| v as usize);
    let mut out = Vec::with_capacity(w * h * zoom * zoom);
    for row in y..y + h {
        let line: Vec<u16> = frame[row * width + x..row * width + x + w].iter().flat_map(|&p| std::iter::repeat_n(p, zoom)).collect();
        for _ in 0..zoom {
            out.extend_from_slice(&line);
        }
    }
    out
}

/// `X,Y` or `X,Y,LAYER`: a map cell, and its layer.
pub fn parse_cell(text: &str) -> Result<(i32, i32, Option<u8>), String> {
    let parts: Vec<&str> = text.split(',').map(str::trim).collect();
    let bad = || format!("--at takes a cell as X,Y or X,Y,LAYER, not {text:?}");
    match parts[..] {
        [x, z] => Ok((x.parse().map_err(|_| bad())?, z.parse().map_err(|_| bad())?, None)),
        [x, z, layer] => Ok((x.parse().map_err(|_| bad())?, z.parse().map_err(|_| bad())?, Some(layer.parse().map_err(|_| bad())?))),
        _ => Err(bad()),
    }
}

/// A way to face, by its compass name (north, north-east and so on, or
/// n, ne, ...), as a facing angle.
pub fn parse_face(text: &str) -> Result<u32, String> {
    let ways = [
        (["east", "e"], 0x0000),
        (["south-east", "se"], 0x2000),
        (["south", "s"], 0x4000),
        (["south-west", "sw"], 0x6000),
        (["west", "w"], 0x8000),
        (["north-west", "nw"], 0xa000),
        (["north", "n"], 0xc000),
        (["north-east", "ne"], 0xe000),
    ];
    let lower = text.trim().to_ascii_lowercase().replace('_', "-");
    ways.iter()
        .find(|(names, _)| names.contains(&lower.as_str()) || names[0].replace('-', "") == lower)
        .map(|(_, angle)| *angle)
        .ok_or_else(|| format!("--face takes north, north-east, east and so on (or n, ne, e ...), not {text:?}"))
}

/// A pad script: steps `KEYS:FRAMES` apart by commas, KEYS key names joined
/// by `+` (up, down, left, right, a, b, l, r, start, select), or `wait` for
/// none (`up:40,wait:20,a:2`); or `cast:ABILITY`, a Psynergy cast where the
/// leader stands and faces, `cast:ABILITY:WAY` with the way (north, east,
/// south, west or up, right, down, left) held after it for
/// CAST_WAY_FRAMES frames, and `cast:ABILITY:WAY:FRAMES` for as many.
pub fn parse_script(text: &str) -> Result<Vec<Held>, String> {
    let mut out = Vec::new();
    for step in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(rest) = step.strip_prefix("cast:") {
            out.push(parse_cast(rest)?);
            continue;
        }
        let (keys, frames) = step.split_once(':').ok_or_else(|| format!("a pad step is KEYS:FRAMES (up:40) or cast:ABILITY, not {step:?}"))?;
        let frames: u32 = frames.trim().parse().map_err(|_| format!("{step:?} holds its keys for no number of frames"))?;
        out.push(Held::keys(parse_keys(keys)?, frames));
    }
    Ok(out)
}

/// KEYS: key names joined by `+`, or `wait`.
fn parse_keys(keys: &str) -> Result<u32, String> {
    let mut bits = 0;
    for key in keys.split('+').map(|k| k.trim().to_ascii_lowercase()) {
        if key == "wait" || key == "none" {
            continue;
        }
        bits |= KEY_NAMES.iter().find(|(n, _)| *n == key).map(|(_, b)| *b).ok_or_else(|| format!("{key:?} is no key (up, down, left, right, a, b, l, r, start, select, wait)"))?;
    }
    Ok(bits)
}

/// `ABILITY[:WAY[:FRAMES]]` of a `cast:` step.
fn parse_cast(text: &str) -> Result<Held, String> {
    let parts: Vec<&str> = text.split(':').map(str::trim).collect();
    let name = parts[0];
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!("cast: names an ability (cast:hover, cast:move:east), not {text:?}"));
    }
    let keys = match parts.get(1) {
        None => 0,
        Some(way) => match way.to_ascii_lowercase().as_str() {
            "north" | "n" | "up" => 0x40,
            "south" | "s" | "down" => 0x80,
            "east" | "e" | "right" => 0x10,
            "west" | "w" | "left" => 0x20,
            other => return Err(format!("cast:{name}: takes a way (north, east, south, west), not {other:?}")),
        },
    };
    let frames = match parts.get(2) {
        None if keys == 0 => 0,
        None => CAST_WAY_FRAMES,
        Some(n) => n.parse().map_err(|_| format!("cast:{text} holds its way for no number of frames"))?,
    };
    if parts.len() > 3 {
        return Err(format!("a cast step is cast:ABILITY[:WAY[:FRAMES]], not cast:{text}"));
    }
    Ok(Held { keys, frames, cast: Some(Cast { name: name.to_string(), id: 0 }) })
}

/// An ability's number from the game's ability_ids.h (`enum AbilityId`),
/// by its name with or without ABILITY_, in either case.
pub fn ability_id(header: &str, name: &str) -> Result<u16, String> {
    let upper = name.trim().to_ascii_uppercase();
    let wanted = if upper.starts_with("ABILITY_") { upper } else { format!("ABILITY_{upper}") };
    // The value each enumerator takes: its own number, else one past the
    // last; one given by an expression (ABILITY_DJINNI_FIRST + ...) leaves
    // those after it unknown until a number comes.
    let mut next = Some(0i64);
    let item = regex::Regex::new(r"^\s*(ABILITY_\w+)\s*(?:=\s*([^,/]+?))?\s*(?:,|$|//)").expect("an enumerator pattern");
    for line in header.lines() {
        let Some(found) = item.captures(line) else {
            continue;
        };
        let value = match found.get(2).map(|v| v.as_str().trim()) {
            Some(v) => v.strip_prefix("0x").map_or_else(|| v.parse().ok(), |h| i64::from_str_radix(h, 16).ok()),
            None => next,
        };
        next = value.map(|v| v + 1);
        if found[1] == wanted {
            return value.map(|v| v as u16).ok_or_else(|| format!("ability_ids.h gives {wanted} no plain number"));
        }
    }
    Err(format!("ability_ids.h has no {wanted}"))
}

/// `--answer`'s list: each prompt's answer in turn, `yes` (A), `no` (B,
/// which a yes or no choice and a menu take as No or backing out) or keys
/// joined by `+` (`right+a`).
pub fn parse_answers(text: &str) -> Result<Vec<u32>, String> {
    let mut out = Vec::new();
    for answer in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let keys = match answer.to_ascii_lowercase().as_str() {
            "yes" | "y" => 1,
            "no" | "n" => 2,
            other => parse_script(&format!("{other}:1"))?.first().map_or(0, |h| h.keys),
        };
        if keys == 0 {
            return Err(format!("--answer takes yes, no or keys (a, b, right+a), not {answer:?}"));
        }
        out.push(keys);
    }
    Ok(out)
}

/// The keys a script holds in each frame, in order; a cast is one frame,
/// CAST_FRAME with the ability's number, before its way's frames, each
/// marked CAST_WAY.
pub fn script_frames(script: &[Held]) -> Vec<u32> {
    script
        .iter()
        .flat_map(|h| {
            let way = if h.cast.is_some() { CAST_WAY } else { 0 };
            h.cast.as_ref().map(|c| CAST_FRAME | u32::from(c.id)).into_iter().chain(std::iter::repeat_n(h.keys | way, h.frames as usize))
        })
        .collect()
}

/// The script as `--hold` writes it, for a note.
pub fn describe(script: &[Held]) -> String {
    script
        .iter()
        .map(|h| {
            let keys: Vec<&str> = KEY_NAMES.iter().filter(|(_, b)| h.keys & b != 0).map(|(n, _)| *n).collect();
            match &h.cast {
                Some(c) if h.frames == 0 => format!("cast:{}", c.name.to_ascii_lowercase()),
                Some(c) => format!("cast:{}:{}:{}", c.name.to_ascii_lowercase(), keys.join("+"), h.frames),
                None => format!("{}:{}", if keys.is_empty() { "wait".to_string() } else { keys.join("+") }, h.frames),
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(feature = "render")]
/// An item's number from the game's item_ids.h (`enum ItemId`), by its name
/// with or without ITEM_, or a number.
pub fn item_id(header: &str, name: &str) -> Result<u16, String> {
    let name = name.trim();
    if let Some(n) = name.strip_prefix("0x").map_or_else(|| name.parse().ok(), |h| u16::from_str_radix(h, 16).ok()) {
        return Ok(n);
    }
    let wanted = if name.starts_with("ITEM_") { name.to_string() } else { format!("ITEM_{}", name.to_ascii_uppercase()) };
    super::source::enums(header, &HashMap::new())?
        .into_iter()
        .flat_map(|(_, values)| values)
        .find(|(n, _)| *n == wanted)
        .and_then(|(_, v)| v)
        .map(|v| v as u16)
        .ok_or_else(|| format!("item_ids.h has no {wanted}"))
}

/// The flags function `name` of C source `text` needs before it does
/// anything, as (flag, set) pairs: the condition of an `if` its whole body
/// lies in (after its declarations), or the opposite of the one that
/// returns at once, when the condition is flag tests joined by `&&` (an
/// early return, by `||`). A flag test is `GameFlag_Get(F)`,
/// `Event_GetFlag(F)` or `Flag_IsSet(F)`, with `!` or compared with 0.
/// Nothing for anything else.
pub fn guard(text: &str, name: &str) -> Vec<(String, bool)> {
    let Some(body) = function_body(text, name) else {
        return Vec::new();
    };
    let plain = strip_comments(&body);
    let mut rest = plain.trim();
    // Declarations first (`struct Actor *leader = Actor_Get(...);`).
    let declaration = regex::Regex::new(r"^(?:const\s+)?(?:struct\s+\w+\s*\*?|s8|u8|s16|u16|s32|u32|bool8|bool32|int)\s*\*?\s*\w+(?:\s*=[^;]*)?;").expect("a declaration pattern");
    while let Some(found) = declaration.find(rest) {
        rest = rest[found.end()..].trim_start();
    }
    let Some(after_if) = rest.strip_prefix("if") else {
        return Vec::new();
    };
    let Some((condition, after)) = parenthesised(after_if.trim_start()) else {
        return Vec::new();
    };
    let after = after.trim_start();
    // `if (C) return;`: the body needs C false.
    let returns = |t: &str| t.strip_prefix("return").is_some_and(|r| r.trim_start().starts_with(';'));
    let braced_return = after.strip_prefix('{').map(str::trim_start).is_some_and(|t| returns(t) && t.trim_start_matches("return").trim_start().trim_start_matches(';').trim_start().starts_with('}'));
    if returns(after) || braced_return {
        let terms: Vec<&str> = split_top(&condition, "||");
        return terms.iter().map(|t| flag_test(t).map(|(f, on)| (f, !on))).collect::<Option<Vec<_>>>().unwrap_or_default();
    }
    // `if (C) { ... }` closing the body, with no else.
    let Some(block) = after.strip_prefix('{') else {
        return Vec::new();
    };
    let Some(end) = matching_brace(block) else {
        return Vec::new();
    };
    if !block[end + 1..].trim().is_empty() {
        return Vec::new();
    }
    let terms: Vec<&str> = split_top(&condition, "&&");
    terms.iter().map(|t| flag_test(t)).collect::<Option<Vec<_>>>().unwrap_or_default()
}

/// The map copies whose rectangle's destination comes before its size
/// (`(sourceX, sourceY, destinationX, destinationY, width, height)`); the
/// others give the size first. Those that copy pictures, then those that
/// copy only what the cells do.
const PICTURE_COPIES: [(&str, bool); 7] = [
    ("MapGrid_CopyCellsTo", true),
    ("MapGrid_CopyCellTilesIndicesRect", true),
    ("MapGrid_CopyCells", false),
    ("MapGrid_CopyCellsLayered", false),
    ("MapGrid_CopyCellTilesCellsRect", false),
    ("FieldTileCopies_CopyTouchedCells", false),
    ("TileCopies_CopyTouchedCells", false),
];
const BEHAVIOUR_COPIES: [(&str, bool); 3] = [("MapGrid_CopyCellAttributes", false), ("Map_CopyCellAttributeRect", false), ("Map_CopyCellsRect", false)];

/// The map cells function `name` of C source `text` changes or puts
/// something on, what a Psynergy cast for it must reach to show it: the
/// destinations of its map copies with numbers for arguments (those that
/// copy pictures, else those that copy what cells do), and the cells of the
/// pixels an actor is put at (`Actor_SetPosition(id, 657, 246)`).
pub fn drawn_cells(text: &str, name: &str) -> Vec<(i32, i32)> {
    let Some(body) = function_body(text, name) else {
        return Vec::new();
    };
    let plain = strip_comments(&body);
    let numbers = |args: &str| -> Option<Vec<i32>> { args.split(',').map(|a| a.trim().parse::<i32>().ok()).collect() };
    let calls = |table: &[(&str, bool)]| -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for &(function, to_first) in table {
            let pattern = regex::Regex::new(&format!(r"\b{function}\s*\(([^;]*?)\)\s*;")).expect("a call pattern");
            for found in pattern.captures_iter(&plain) {
                let Some(n) = numbers(&found[1]) else {
                    continue;
                };
                if n.len() < 6 {
                    continue;
                }
                let (x, z, w, h) = if to_first { (n[2], n[3], n[4], n[5]) } else { (n[4], n[5], n[2], n[3]) };
                for dz in 0..h.clamp(1, 8) {
                    for dx in 0..w.clamp(1, 8) {
                        if !out.contains(&(x + dx, z + dz)) {
                            out.push((x + dx, z + dz));
                        }
                    }
                }
            }
        }
        out
    };
    let mut out = calls(&PICTURE_COPIES);
    let placed = regex::Regex::new(r"\bActor_SetPosition(?:AndFacing)?\s*\(\s*\w+\s*,\s*(\d+)\s*,\s*(\d+)\s*[,)]").expect("a placement pattern");
    for found in placed.captures_iter(&plain) {
        if let (Ok(x), Ok(z)) = (found[1].parse::<i32>(), found[2].parse::<i32>()) {
            out.push((x / 16, z / 16));
        }
    }
    if out.is_empty() {
        out = calls(&BEHAVIOUR_COPIES);
    }
    out
}

/// What function `name` of C source `text` assigns to, as its statements
/// write it (`leader->riseCounter`, `gFadeOverlayState->actor`), its own
/// locals left out: for an event that shows nothing, what it changes
/// instead.
pub fn assigned(text: &str, name: &str) -> Vec<String> {
    let Some(body) = function_body(text, name) else {
        return Vec::new();
    };
    let plain = strip_comments(&body);
    let statement = regex::Regex::new(r"(?m)^\s*([A-Za-z_][\w]*(?:\s*(?:->|\.)\s*\w+|\[[^\]]*\])+|g[A-Z]\w*)\s*(?:[-+|&]?=)[^=]").expect("an assignment pattern");
    let mut out: Vec<String> = Vec::new();
    for found in statement.captures_iter(&plain) {
        let target: String = found[1].split_whitespace().collect();
        if !out.contains(&target) {
            out.push(target);
        }
    }
    out
}

/// One flag test: the flag's name and whether it must be set.
fn flag_test(term: &str) -> Option<(String, bool)> {
    let mut t = term.trim();
    while let Some(inner) = t.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        t = inner.trim();
    }
    let mut on = true;
    if let Some(r) = t.strip_prefix('!') {
        if !r.starts_with('=') {
            on = false;
            t = r.trim();
        }
    }
    for (suffix, flips) in [("== 0", true), ("== FALSE", true), ("!= 0", false), ("== 1", false), ("!= FALSE", false)] {
        if let Some(r) = t.strip_suffix(suffix) {
            t = r.trim();
            on ^= flips;
            break;
        }
    }
    for getter in ["GameFlag_Get(", "Event_GetFlag(", "Flag_IsSet("] {
        if let Some(r) = t.strip_prefix(getter).and_then(|r| r.strip_suffix(')')) {
            let flag = r.trim();
            if !flag.is_empty() && flag.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Some((flag.to_string(), on));
            }
        }
    }
    None
}

/// The text of function `name`'s body in `text`, braces left out.
fn function_body(text: &str, name: &str) -> Option<String> {
    let pattern = regex::Regex::new(&format!(r"(?m)^[\w\s\*]*\b{}\s*\([^)]*\)\s*\{{", regex::escape(name))).ok()?;
    let start = pattern.find(text)?.end();
    let end = matching_brace(&text[start..])?;
    Some(text[start..start + end].to_string())
}

/// Where the brace closing an opened block lies in `text` (which starts
/// inside it).
fn matching_brace(text: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' if depth == 0 => return Some(i),
            '}' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// `(inside) rest`: the parenthesised text at the start and what follows.
fn parenthesised(text: &str) -> Option<(String, &str)> {
    let inner = text.strip_prefix('(')?;
    let mut depth = 0i32;
    for (i, c) in inner.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return Some((inner[..i].to_string(), &inner[i + 1..])),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// `text` split at `op` outside parentheses.
fn split_top<'t>(text: &'t str, op: &str) -> Vec<&'t str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ if depth == 0 && text[i..].starts_with(op) => {
                out.push(&text[start..i]);
                i += op.len();
                start = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&text[start..]);
    out
}

fn strip_comments(text: &str) -> String {
    regex::Regex::new(r"(?s)/\*.*?\*/|//[^\n]*").map_or_else(|_| text.to_string(), |r| r.replace_all(text, "").into_owned())
}

/// `#define NAME VALUE` lines of a C source, and its enumerators given a
/// value (`FLAG_LOG_ROLLED = FLAG_AREA_TEMP_10,`), for flags a scene names
/// itself.
pub fn defines(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let given = regex::Regex::new(r"(?m)^\s*(FLAG_\w+)\s*=\s*([\w()]+)\s*,").expect("an enumerator pattern");
    for found in given.captures_iter(text) {
        out.insert(found[1].to_string(), found[2].to_string());
    }
    for line in text.lines() {
        let mut words = line.split_whitespace();
        if let (Some("#define"), Some(name), Some(value)) = (words.next(), words.next(), words.next()) {
            if !name.contains('(') {
                out.insert(name.to_string(), value.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scene_names_flags_by_define_and_enum() {
        let names = defines("#define FLAG_TOLD 0x8b0 ///< told\nenum {\n  FLAG_LOG_ROLLED = FLAG_AREA_TEMP_10,\n  OTHER = 3,\n};\n#define F(x) (x)\n");
        assert_eq!(names.get("FLAG_TOLD").map(String::as_str), Some("0x8b0"));
        assert_eq!(names.get("FLAG_LOG_ROLLED").map(String::as_str), Some("FLAG_AREA_TEMP_10"));
        assert!(!names.contains_key("OTHER") && !names.contains_key("F(x)"));
    }

    #[test]
    fn a_pad_script_is_keys_held_for_frames() {
        let script = parse_script("up:40, wait:20,a+right:2").unwrap();
        assert_eq!(script, vec![Held::keys(0x40, 40), Held::keys(0, 20), Held::keys(0x11, 2)]);
        assert_eq!(script_frames(&script).len(), 62);
        assert_eq!(describe(&script), "up:40,wait:20,a+right:2");
        assert!(parse_script("up").is_err());
        assert!(parse_script("jump:3").is_err());
    }

    #[test]
    fn a_cast_is_a_step_of_a_pad_script() {
        let header = "enum AbilityId {\n  ABILITY_NONE,\n  ABILITY_FROST = 24,\n  ABILITY_DOUSE, // a comment\n  ABILITY_MOVE = 0x8c,\n  ABILITY_HOVER\n  ABILITY_FIZZ = ABILITY_DJINNI_FIRST + 20,\n  ABILITY_SLEET,\n  ABILITY_LAST = 700,\n  ABILITY_AFTER\n};";
        let mut stage = Stage::default();
        assert!(stage.option("--then", &mut |_| Ok("wait:30, cast:hover, cast:Move:east, cast:frost:s:5, up:2".to_string())).unwrap());
        assert_eq!(stage.then[1], Held { keys: 0, frames: 0, cast: Some(Cast { name: "hover".to_string(), id: 0 }) });
        assert_eq!((stage.then[2].keys, stage.then[2].frames), (0x10, CAST_WAY_FRAMES));
        assert_eq!((stage.then[3].keys, stage.then[3].frames), (0x80, 5));
        stage.resolve(header).unwrap();
        let frames = script_frames(&stage.then);
        assert_eq!(frames.len(), 30 + 1 + 1 + 60 + 1 + 5 + 2);
        assert_eq!(frames[30], CAST_FRAME | 0x8d);
        assert_eq!((frames[31], frames[32]), (CAST_FRAME | 0x8c, 0x10 | CAST_WAY));
        assert_eq!(frames[29], 0);
        assert_eq!(frames[98], 0x40);
        assert_eq!(frames[92], CAST_FRAME | 24);
        assert_eq!(describe(&stage.then), "wait:30,cast:hover,cast:move:right:60,cast:frost:down:5,up:2");
        assert_eq!(parse_script(&describe(&stage.then)).unwrap().len(), 5);
        assert!(Stage { then: parse_script("cast:sand").unwrap(), ..Stage::default() }.resolve(header).is_err());
        assert!(parse_script("cast:move:sideways").is_err());
        assert!(parse_script("cast:").is_err());
        assert_eq!(ability_id(header, "ABILITY_DOUSE").unwrap(), 25);
        assert_eq!(ability_id(header, "douse").unwrap(), 25);
        assert!(ability_id(header, "sleet").is_err());
        assert_eq!(ability_id(header, "after").unwrap(), 701);
    }

    #[test]
    fn the_run_s_other_options_are_read() {
        let mut stage = Stage::default();
        let given = [
            ("--during", "up:60"),
            ("--skip", "Settlements_StepOntoWall"),
            ("--limit", "600"),
            ("--from", "12.5"),
            ("--for", "30"),
            ("--at-px", "676,792,0"),
            ("--first-at", "15,23"),
            ("--first-face", "south"),
            ("--first-hold", "down:20"),
            ("--first-then", "wait:10"),
        ];
        for (option, value) in given {
            assert!(Stage::is_option(option));
            assert!(stage.option(option, &mut |_| Ok(value.to_string())).unwrap());
        }
        assert!(stage.option("--keep-short", &mut |_| Err("no value".to_string())).unwrap());
        assert!(stage.keep_short);
        assert_eq!(stage.during, vec![Held::keys(0x40, 60)]);
        assert_eq!(stage.skip, vec!["Settlements_StepOntoWall"]);
        assert_eq!(stage.limit_frames(14400), 36000);
        assert_eq!(Stage::default().limit_frames(14400), 14400);
        assert_eq!((stage.from, stage.length), (Some(750), Some(1800)));
        assert_eq!(stage.window(5000), (750, 2550));
        assert_eq!(stage.window(1000), (750, 1000));
        assert_eq!(stage.window(100), (100, 100));
        assert_eq!(Stage::default().window(90), (0, 90));
        assert!(stage.windowed() && !Stage::default().windowed());
        assert_eq!(stage.at_px, Some((676, 792, Some(0))));
        assert_eq!(stage.start(), Some((42, 49, Some(0))));
        assert!(stage.staged());
        assert!(stage.walks(&[]));
        assert!(stage.option("--limit", &mut |_| Ok("0".to_string())).is_err());
        assert!(stage.option("--from", &mut |_| Ok("-1".to_string())).is_err());
        assert!(stage.option("--at-px", &mut |_| Ok("3".to_string())).unwrap_err().contains("--at-px"));
    }

    #[test]
    fn a_height_to_start_at_is_read() {
        let mut stage = Stage::default();
        assert!(Stage::is_option("--at-height"));
        assert!(stage.option("--at-height", &mut |_| Ok("208".to_string())).unwrap());
        assert_eq!(stage.at_height, Some(208));
        assert!(stage.option("--at-height", &mut |_| Ok("high".to_string())).unwrap_err().contains("--at-height"));
    }

    #[test]
    fn a_function_to_wait_for_and_scene_changes_to_follow_are_read() {
        let mut stage = Stage::default();
        assert!(Stage::is_option("--until") && Stage::is_option("--follow-scenes"));
        assert!(stage.option("--until", &mut |_| Ok("MagmaRock_BurstStatue".to_string())).unwrap());
        assert_eq!(stage.until.as_deref(), Some("MagmaRock_BurstStatue"));
        assert!(stage.option("--follow-scenes", &mut |_| Ok("2".to_string())).unwrap());
        assert_eq!(stage.follow_scenes, 2);
        assert!(stage.option("--follow-scenes", &mut |_| Ok("0".to_string())).is_err());
        assert!(stage.option("--follow-scenes", &mut |_| Ok("many".to_string())).is_err());
    }

    #[test]
    fn an_arrival_as_after_a_battle_is_read() {
        let mut stage = Stage::default();
        assert!(Stage::is_option("--arrive-as"));
        assert!(stage.option("--arrive-as", &mut |_| Ok("after-battle".to_string())).unwrap());
        assert!(stage.after_battle);
        assert!(stage.option("--arrive-as", &mut |_| Ok("entrance".to_string())).unwrap());
        assert!(!stage.after_battle);
        assert!(stage.option("--arrive-as", &mut |_| Ok("battle".to_string())).is_err());
    }

    #[test]
    fn a_walk_to_a_cell_a_crop_and_a_frame_range_are_read() {
        let mut stage = Stage::default();
        for (option, value) in [("--walk-to", "17,13"), ("--crop", "100,40,64,48"), ("--zoom", "3"), ("--frames", "120-299")] {
            assert!(Stage::is_option(option));
            assert!(stage.option(option, &mut |_| Ok(value.to_string())).unwrap());
        }
        assert_eq!(stage.walk_to, Some((17, 13, None)));
        assert!(stage.walks(&[]) && !Stage { walk_to: None, ..stage.clone() }.walks(&[]));
        assert_eq!((stage.crop, stage.zoom), (Some([100, 40, 64, 48]), Some(3)));
        assert_eq!(stage.window(1000), (120, 300));
        assert_eq!(stage.window(200), (120, 200));
        assert!(parse_crop("200,0,64,16").is_err());
        assert!(parse_crop("0,0,0,16").is_err());
        assert!(parse_frames("30-10").is_err() && parse_frames("30").is_err());
        assert!(stage.option("--zoom", &mut |_| Ok("5".to_string())).is_err());
        assert!(stage.option("--walk-to", &mut |_| Ok("3".to_string())).unwrap_err().contains("--walk-to"));
        // A 4 by 2 screen cut to its middle 2 by 1, drawn twice as large.
        let frame = [0, 1, 2, 3, 4, 5, 6, 7];
        assert_eq!(crop_frame(&frame, 4, [1, 1, 2, 1], 2), vec![5, 5, 6, 6, 5, 5, 6, 6]);
        assert_eq!(crop_frame(&frame, 4, [0, 0, 4, 2], 1), frame.to_vec());
    }

    #[test]
    fn the_event_played_first_has_its_own_start() {
        let mut stage = Stage { at: Some((10, 9, None)), hold: parse_script("up:8").unwrap(), then: parse_script("down:8").unwrap(), during: parse_script("a:1").unwrap(), set: vec!["FLAG_X".to_string()], first: Some("E_First".to_string()), ..Stage::default() };
        let first = stage.for_first();
        assert_eq!((first.at, first.hold.len(), first.then.len(), first.during.len()), (None, 0, 0, 0));
        assert_eq!((first.set.clone(), first.first.clone()), (vec!["FLAG_X".to_string()], None));
        assert!(!first.staged());
        stage.first_at = Some((15, 23, Some(1)));
        stage.first_face = Some(0x4000);
        stage.first_hold = parse_script("down:20").unwrap();
        let first = stage.for_first();
        assert_eq!((first.at, first.face, first.hold.clone(), first.first_at), (Some((15, 23, Some(1))), Some(0x4000), vec![Held::keys(0x80, 20)], None));
        assert!(first.staged());
    }

    #[test]
    fn a_prompt_is_answered_yes_no_or_by_keys() {
        assert_eq!(parse_answers("no, yes,right+a").unwrap(), vec![2, 1, 0x11]);
        assert!(parse_answers("maybe").is_err());
        assert!(parse_answers("wait").is_err());
        let mut stage = Stage::default();
        for (option, value) in [("--answer", "no"), ("--leader", "jenna"), ("--party", "JENNA, FELIX")] {
            assert!(stage.option(option, &mut |_| Ok(value.to_string())).unwrap());
        }
        assert_eq!((stage.answer, stage.leader.as_deref(), stage.party), (vec![2], Some("jenna"), vec!["JENNA".to_string(), "FELIX".to_string()]));
        let mut chained = Stage::default();
        assert!(chained.option("--first", &mut |_| Ok("Garoh_PlaceFirstPillar".to_string())).unwrap());
        assert_eq!(chained.first.as_deref(), Some("Garoh_PlaceFirstPillar"));
    }

    #[test]
    fn keys_with_no_event_named_are_a_walk() {
        let mut stage = Stage::default();
        assert!(!stage.walks(&[]));
        stage.then = parse_script("up:30").unwrap();
        assert!(stage.walks(&[]));
        assert!(!stage.walks(&["Garoh_IsNight".to_string()]));
        stage.set.push("FLAG_GAROH_NIGHT".to_string());
        assert!(!Stage { set: stage.set.clone(), ..Stage::default() }.walks(&[]));
    }

    #[test]
    fn a_cell_and_a_way_to_face() {
        assert_eq!(parse_cell("12,28").unwrap(), (12, 28, None));
        assert_eq!(parse_cell("12, 28, 1").unwrap(), (12, 28, Some(1)));
        assert!(parse_cell("12").is_err());
        assert_eq!(parse_face("north").unwrap(), 0xc000);
        assert_eq!(parse_face("SW").unwrap(), 0x6000);
        assert_eq!(parse_face("south-east").unwrap(), 0x2000);
        assert_eq!(parse_face("southeast").unwrap(), 0x2000);
        assert!(parse_face("up").is_err());
    }

    #[test]
    fn staging_options_are_read_and_named() {
        let mut stage = Stage::default();
        let mut given = ["FLAG_A".to_string(), "4".to_string(), "3,4".to_string(), "left:16".to_string()].into_iter();
        for option in ["--set", "--entrance", "--at", "--hold"] {
            assert!(stage.option(option, &mut |_| Ok(given.next().unwrap())).unwrap());
            assert!(Stage::is_option(option));
        }
        assert!(!stage.option("--scene", &mut |_| Ok(String::new())).unwrap());
        assert_eq!(stage.set, vec!["FLAG_A"]);
        assert_eq!(stage.entrance, Some(4));
        assert_eq!(stage.at, Some((3, 4, None)));
        assert_eq!(stage.hold, vec![Held::keys(0x20, 16)]);
        assert!(stage.staged());
        assert!(!Stage::default().staged());
    }

    #[cfg(feature = "render")]
    #[test]
    fn items_are_found_by_name() {
        let header = "enum ItemId {\n  ITEM_NONE,\n  ITEM_HERB,\n  ITEM_RUIN_KEY = 389,\n  ITEM_MAGMA_BALL\n};";
        assert_eq!(item_id(header, "ITEM_MAGMA_BALL").unwrap(), 390);
        assert_eq!(item_id(header, "ruin_key").unwrap(), 389);
        assert_eq!(item_id(header, "0x10").unwrap(), 16);
        assert!(item_id(header, "ITEM_SWORD").is_err());
    }

    #[test]
    fn what_a_cast_must_reach_is_what_its_function_draws() {
        let text = "void C_Reveal(void) {\n  MapGrid_CopyCellsTo(27, 4, 48, 12, 1, 2);\n  MapGrid_CopyCellAttributes(48, 1, 2, 1, 48, 19);\n}\n\
                    void C_Weeds(void) {\n  FieldTileCopies_CopyTouchedCells(3, 50, 1, 1, 8, 54, 0);\n}\n\
                    void C_Idol(void) {\n  Actor_SetPosition(FIELD_TREASURE_ACTOR, 657, 246);\n}\n\
                    void C_Door(void) {\n  Map_CopyCellAttributeRect(2, 4, 1, 1, 7, 14);\n}\n\
                    void C_Unknown(void) {\n  MapGrid_CopyCellsTo(x, 4, 48, 12, 1, 2);\n}\n";
        assert_eq!(drawn_cells(text, "C_Reveal"), vec![(48, 12), (48, 13)]);
        assert_eq!(drawn_cells(text, "C_Weeds"), vec![(8, 54)]);
        assert_eq!(drawn_cells(text, "C_Idol"), vec![(41, 15)]);
        assert_eq!(drawn_cells(text, "C_Door"), vec![(7, 14)]);
        assert!(drawn_cells(text, "C_Unknown").is_empty());
    }

    #[test]
    fn what_a_function_assigns_is_read() {
        let text = "static void S_Start(void) {\n  struct Actor *leader = Actor_Get(0);\n  s32 i;\n  gFadeOverlayState->actor = leader;\n  leader->riseCounter = 1;\n  i = 2;\n  gCount += 1;\n  if (leader->x == 3)\n    gState.flags[i] |= 4;\n}\n";
        assert_eq!(assigned(text, "S_Start"), vec!["gFadeOverlayState->actor", "leader->riseCounter", "gCount", "gState.flags[i]"]);
    }

    #[test]
    fn a_guard_around_the_whole_body_is_what_it_needs() {
        let text = "void A_Stop(void) {\n  if (GameFlag_Get(FLAG_REUNITED) &&\n      !GameFlag_Get(FLAG_SET_OUT)) {\n    Event_Start();\n    if (x) { y(); }\n  }\n}\n\
                    void A_Early(void) {\n  struct Actor *a = Actor_Get(3);\n  // a comment\n  if (Event_GetFlag(FLAG_DONE) != 0 || !Flag_IsSet(FLAG_OPEN))\n    return;\n  Event_Start();\n}\n\
                    void A_Else(void) {\n  if (GameFlag_Get(FLAG_X)) {\n    a();\n  } else {\n    b();\n  }\n}\n\
                    void A_Braced(void) {\n\n  if (!Event_GetFlag(FLAG_WALKED)) {\n    return;\n  }\n  Event_Start();\n}\n\
                    void A_Zero(s32 mode) {\n  if (Event_GetFlag(FLAG_Y) == 0) {\n    a();\n  }\n}\n\
                    void A_Other(void) {\n  if (Actor_Get(3)) {\n    a();\n  }\n}\n";
        assert_eq!(guard(text, "A_Stop"), vec![("FLAG_REUNITED".to_string(), true), ("FLAG_SET_OUT".to_string(), false)]);
        assert_eq!(guard(text, "A_Early"), vec![("FLAG_DONE".to_string(), false), ("FLAG_OPEN".to_string(), true)]);
        assert!(guard(text, "A_Else").is_empty());
        assert_eq!(guard(text, "A_Braced"), vec![("FLAG_WALKED".to_string(), true)]);
        assert_eq!(guard(text, "A_Zero"), vec![("FLAG_Y".to_string(), false)]);
        assert!(guard(text, "A_Other").is_empty());
        assert!(guard(text, "A_Missing").is_empty());
    }
}
