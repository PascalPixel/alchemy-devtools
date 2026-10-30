//! Story set pieces in the Doll House recorder (whole.rs), as the Venus
//! aerie's (guide_aerie.rs): one GIF of a scene's whole map in every frame,
//! from the party's arrival to the end of its story there. The game is
//! played as the player plays it, from the story as it stands on the first
//! visit: pages answered with A, the leader walked along the set piece's
//! path. A battle is not the scene, so a leg ends the frame before it, and
//! the next starts where the game sends the party back, carrying the flags
//! the leg before set. The message window and portraits are left out, as
//! the recorder leaves them, and so are frames the game draws line by line
//! (its screen transitions).

use super::field::{Field, Setup};
use super::guide::Guide;
use super::guide_aerie::{carried_flags_set, line_effect, lit, shown_frames};
use super::story::Trigger;
use super::whole::{self, Run, States};

/// A battle (MAP_BATTLE).
const MAP_BATTLE: u16 = 0x1fe;
/// Frames a page or a choice waits before A answers it (the window is left
/// out, so a page waits no longer than a glance), and the frames A is held.
const PAGE_WAIT: u32 = 10;
const PRESS: u32 = 3;
/// Frames the leader may stand still while walked, a leg's frames at most,
/// and the frames the player must hold control for a leg that ends settled.
const STUCK: u32 = 180;
const LEG_LIMIT: usize = 30000;
const SETTLED: u32 = 90;
/// Frames the leader stands where he was placed before the leg is recorded.
const PLACED: u32 = 20;
/// Pixels from a point at which it counts as reached.
const NEAR: i32 = 2;
/// KEYINPUT bits.
const KEY_A: u32 = 0x1;
const KEY_RIGHT: u32 = 0x10;
const KEY_LEFT: u32 = 0x20;
const KEY_UP: u32 = 0x40;
const KEY_DOWN: u32 = 0x80;

/// How a leg ends.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum End {
    /// A battle begins.
    Battle,
    /// The walk is over and the player has held control for SETTLED frames.
    Settled,
}

/// One leg of a set piece: where it enters; the trigger that starts its
/// story, which the leader is placed beside as `render event` places him
/// and then gives (none for a leg the arrival starts); the leader's walk
/// (each point reached by holding one direction, in pixels); and how it
/// ends.
pub(super) struct Leg {
    pub entrance: u16,
    pub trigger: Option<Trigger>,
    /// Story flags the leg needs set beyond the first visit's (the row's own
    /// condition, for a story told on a later visit).
    pub set: &'static [&'static str],
    pub walk: &'static [(i32, i32)],
    pub end: End,
}

/// A story set piece: the scene it plays in, its legs in order, the file it
/// is written to under places/ and what it shows.
pub(super) struct SetPiece {
    pub scene: u16,
    pub legs: &'static [Leg],
    pub file: &'static str,
    pub what: &'static str,
}

/// Mercury Lighthouse's aerie (SceneId_MercuryLighthouseAerie): from the
/// party arriving after the beacon is lit
/// (MERCURY_LIGHTHOUSE_AERIE_AFTER_BEACON_LIT), the leader stepping onto
/// cell behavior 5 before the beacon (MercuryLighthouseAerie_ConfrontSaturos)
/// up to the battle; then back from it
/// (MERCURY_LIGHTHOUSE_AERIE_AFTER_SATUROS_BATTLE), where Alex appears, until
/// the player has control again.
pub(super) const MERCURY_LIGHTHOUSE_AERIE: SetPiece = SetPiece {
    scene: 58,
    legs: &[
        Leg { entrance: 1, trigger: Some(Trigger::Touch(5)), set: &[], walk: &[], end: End::Battle },
        Leg { entrance: 2, trigger: None, set: &[], walk: &[], end: End::Settled },
    ],
    file: "mercury-lighthouse-aerie/story-dollhouse.gif",
    what: "Mercury Lighthouse's aerie story in the Doll House recorder, the whole aerie and the sky",
};

/// Lalivero (SceneId_Lalivero) once the party is back from Venus
/// Lighthouse (FLAG_VENUS_LIGHTHOUSE_PARTY_RETURNED): the leader stepping onto
/// cell behavior 50 (Lalivero_IodemGivesBlackOrb), until the player has
/// control again.
pub(super) const LALIVERO_BLACK_ORB: SetPiece = SetPiece {
    scene: 178,
    legs: &[Leg { entrance: 0, trigger: Some(Trigger::Touch(50)), set: &["FLAG_VENUS_LIGHTHOUSE_PARTY_RETURNED"], walk: &[], end: End::Settled }],
    file: "lalivero/story-dollhouse.gif",
    what: "Iodem giving the Black Orb in Lalivero in the Doll House recorder, the whole town",
};

/// Babi Lighthouse's stone door (SceneId_BabiLighthouseStoneDoor): the leader
/// stepping onto cell behavior 22 (BabiLighthouse_OpenTruthDoor), until the
/// player has control again.
pub(super) const BABI_LIGHTHOUSE_TRUTH_DOOR: SetPiece = SetPiece {
    scene: 174,
    legs: &[Leg { entrance: 1, trigger: Some(Trigger::Touch(22)), set: &[], walk: &[], end: End::Settled }],
    file: "babi-lighthouse-stone-door/story-dollhouse.gif",
    what: "the Truth Door opening in Babi Lighthouse in the Doll House recorder, the whole room",
};

/// One leg as played: its field, the state at the end of each frame, and
/// for each frame whether it shows the scene.
struct Played<'a> {
    field: Field<'a>,
    states: States,
    shown: Vec<bool>,
}

impl<'a> Guide<'a> {
    /// A set piece in the Doll House recorder: every second frame the scene
    /// shows, its map whole, cut straight from the frame before each battle
    /// to the first the scene shows again.
    pub(super) fn set_piece(&mut self, piece: &SetPiece) -> Result<(), String> {
        let mut carried = self.flags.before_first_visit(piece.scene)?;
        let mut legs = Vec::new();
        for leg in piece.legs {
            let played = self.play_leg(piece.scene, leg, &carried)?;
            carried = carried_flags_set(&played.field, &carried);
            legs.push(played);
        }
        let frames: Vec<Vec<usize>> = legs.iter().map(|p| shown_frames(&p.shown)).collect();
        if std::env::var_os("ALCHEMY_RENDER_DEBUG").is_some() {
            for (p, f) in legs.iter().zip(&frames) {
                eprintln!("leg: {} frames played, {} drawn", p.shown.len(), f.len());
            }
        }
        // The scene's scroll bounds as its last frame drawn has them.
        let last = legs.last_mut().zip(frames.last().and_then(|f| f.last())).ok_or("the last leg showed nothing")?;
        last.0.field.load(&last.0.states.get(*last.1));
        let area = last.0.field.bounds().map(|v| v >> 16);
        let mut parts: Vec<Run> = legs.iter_mut().zip(&frames).map(|(p, f)| Run { field: &mut p.field, states: &p.states, frames: f, wide: true }).collect();
        let mut pictures = whole::stitch_runs(&mut parts, area)?;
        // A scene that fades out at the end ends on its last picture that is
        // not all one colour.
        while pictures.last().is_some_and(|p| p.pixels.iter().all(|&c| c == p.pixels[0])) {
            pictures.pop();
        }
        self.whole_gif(&format!("places/{}", piece.file), &pictures, piece.what)
    }

    /// Enters `scene` at the leg's entrance with the story flags `set` and
    /// plays it, keeping every frame's state, until it ends.
    fn play_leg(&self, scene: u16, leg: &Leg, set: &[u16]) -> Result<Played<'a>, String> {
        let mut flags = vec![self.flag("FLAG_VALE_PARTY_LEFT")?];
        flags.extend(set.iter().filter(|f| !flags.contains(f)).copied().collect::<Vec<_>>());
        for name in leg.set {
            let f = self.flag(name)?;
            if !flags.contains(&f) {
                flags.push(f);
            }
        }
        let clear = vec![self.flag("FLAG_VALE_STORM")?, self.flag("FLAG_CHILDHOOD")?];
        let mut field = Field::enter(self.rom, self.symbols, &Setup { scene, entrance: leg.entrance, set: flags, clear })?;
        // A leg started by a trigger: the leader placed where `render event`
        // would place him for it, out of the walk's reach if need be, and
        // the trigger given once the leg is recorded: the direction held
        // onto a touched cell until the event takes control, or A pressed.
        let mut giving = 0;
        if let Some(trigger) = leg.trigger {
            field.until_control()?;
            field.relaxed = true;
            let spot = field.spots(trigger)?.into_iter().next().ok_or("no spot to give its trigger from")?;
            field.put_leader(&spot);
            field.frames(PLACED)?;
            giving = match trigger {
                Trigger::Touch(_) => spot.walk,
                Trigger::Talk(_) | Trigger::Search(_) => KEY_A,
                _ => return Err(format!("a set piece cannot give {trigger:?}")),
            };
        }
        field.keep_states(true);
        let mut walk = leg.walk;
        let mut shown = Vec::new();
        let (mut waited, mut pressing, mut still, mut settled) = (0, 0, 0, 0);
        let mut steps = field.steps();
        let mut place = None;
        loop {
            field.frames(1)?;
            let now = field.scene();
            shown.push(now == scene && lit(&field) && !line_effect(&field));
            if now == MAP_BATTLE {
                if leg.end == End::Battle {
                    break;
                }
                return Err(format!("a battle began where none was expected, entering at {}", leg.entrance));
            }
            if shown.len() > LEG_LIMIT {
                return Err(format!("the leg from entrance {} went on past {LEG_LIMIT} frames", leg.entrance));
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
                settled = 0;
                if waited >= PAGE_WAIT {
                    field.hold(KEY_A);
                    pressing = PRESS;
                    waited = 0;
                }
                continue;
            }
            waited = 0;
            let Some(leader) = field.leader().filter(|_| in_control) else {
                // The event has taken control: the trigger is given.
                giving = 0;
                field.hold(0);
                settled = 0;
                continue;
            };
            if giving == KEY_A {
                field.hold(KEY_A);
                pressing = PRESS;
                giving = 0;
                continue;
            }
            if giving != 0 {
                field.hold(giving);
                continue;
            }
            let Some(&(x, z)) = walk.first() else {
                field.hold(0);
                settled += 1;
                if settled == 1 && std::env::var_os("ALCHEMY_RENDER_DEBUG").is_some() {
                    eprintln!("leg from entrance {}: the walk is over with the leader at ({}, {})", leg.entrance, leader.x, leader.z);
                }
                if leg.end == End::Settled && settled >= SETTLED {
                    break;
                }
                continue;
            };
            settled = 0;
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
                if std::env::var_os("ALCHEMY_RENDER_DEBUG").is_some() {
                    print_reach(&mut field, leader.x, leader.z);
                    print_codes(&field, 0);
                    print_codes(&field, 1);
                }
                return Err(format!("the leader stood at ({}, {}) walking to ({x}, {z})", leader.x, leader.z));
            }
        }
        field.hold(0);
        let states = field.keep_states(false).ok_or("no states were kept")?;
        Ok(Played { field, states, shown })
    }
}

/// ALCHEMY_RENDER_DEBUG: the cells the leader can walk to from where he
/// stands (`#`, `@` for his own), 16 pixels each, with each row's z, to
/// plan a set piece's walk.
fn print_reach(field: &mut Field, x: i32, z: i32) {
    let Ok(reach) = field.reach(None) else {
        eprintln!("no reach from ({x}, {z})");
        return;
    };
    let [x0, z0, x1, z1] = field.bounds().map(|v| v >> 16);
    for cz in (z0..z1).step_by(16) {
        let row: String = (x0..x1)
            .step_by(16)
            .map(|cx| if (cx..cx + 16).contains(&x) && (cz..cz + 16).contains(&z) { '@' } else if reach.holds(reach.layer, cx + 8, cz + 8) { '#' } else { '.' })
            .collect();
        eprintln!("{cz:4} {row}");
    }
}

/// ALCHEMY_RENDER_DEBUG: each 16-pixel cell's code on `layer` (base 36, `.`
/// for 0, `+` past 35), with each row's z.
fn print_codes(field: &Field, layer: u8) {
    let [x0, z0, x1, z1] = field.bounds().map(|v| v >> 16);
    for cz in (z0..z1).step_by(16) {
        let row: String = (x0..x1)
            .step_by(16)
            .map(|cx| match field.code_at(layer, cx + 8, cz + 8) {
                0 => '.',
                c @ 1..=35 => char::from_digit(c as u32, 36).unwrap_or('?'),
                c if c < 0 => ' ',
                _ => '+',
            })
            .collect();
        eprintln!("{layer} {cz:4} {row}");
    }
}
