//! The Lost Age's recordings for its folder guides: for each recording of
//! The Broken Seal's guides, the nearest moment of The Lost Age where it has
//! the same mechanism (Felix's party, early in the story), and recordings of
//! its own for what only The Lost Age has. Each is written under the same
//! folder and name as The Broken Seal's (`battle/opening.gif`), so the two
//! sets can be published side by side.
//!
//! The Lost Age has no story tables: a scene is entered from a new game's
//! defaults with one of Camelot's debug parameter sets for the party
//! (party::tla), and only the flags a recording names set.

use super::field::{Field, Setup};
use super::guide::{Entered, Guide};
use super::party;

/// KEYINPUT bits.
pub(super) const KEY_A: u32 = 0x1;
pub(super) const KEY_B: u32 = 0x2;
pub(super) const KEY_START: u32 = 0x8;
pub(super) const KEY_RIGHT: u32 = 0x10;
pub(super) const KEY_LEFT: u32 = 0x20;
pub(super) const KEY_UP: u32 = 0x40;
pub(super) const KEY_DOWN: u32 = 0x80;
pub(super) const KEY_R: u32 = 0x100;
pub(super) const KEY_L: u32 = 0x200;

/// Facings, 1/65536 turns.
pub(super) const EAST: u32 = 0;
pub(super) const SOUTH: u32 = 0x4000;
pub(super) const WEST: u32 = 0x8000;
pub(super) const NORTH: u32 = 0xc000;

/// Debug parameter set 7 (gDebug_ParameterSets): Felix, Jenna and Sheba at
/// level 7, Felix with Echo set, the party between Daila and Madra.
pub(super) const PARTY_DAILA: u32 = 7;

/// The modes The Lost Age plays, in order: the moment mode, then each
/// recording file's.
pub fn modes() -> impl Iterator<Item = &'static str> {
    ["moment", "survey"].into_iter().chain(super::guide_tla_town::MODES.iter().copied()).chain(super::guide_tla_town::MORE_MODES.iter().copied()).chain(super::guide_tla_fight::MODES.iter().copied()).chain(super::guide_tla_fight::CAVE_MODES.iter().copied()).chain(super::guide_tla_more::MODES.iter().copied())
}

impl<'a> Guide<'a> {
    /// Plays one of The Lost Age's modes.
    pub(super) fn run_tla(&mut self, name: &str) -> Result<(), String> {
        match name {
            "moment" => self.play_moment(),
            "survey" => {
                for scene in 2..313u16 {
                    let setup = Setup { scene, entrance: 0, set: Vec::new(), clear: Vec::new() };
                    let Ok(mut field) = Field::enter_with_party(self.rom, self.symbols, &setup, Some(party::tla(PARTY_DAILA)?)) else { continue };
                    if field.until_control().is_err() { continue; }
                    let work = field.map_work();
                    let effects = field.read8(work + 0x17);
                    if effects != 0 {
                        eprintln!("scene {scene}: ground effects {effects:#x}");
                    }
                }
                Ok(())
            }
            _ if super::guide_tla_town::MODES.contains(&name) => self.run_tla_town(name),
            _ if super::guide_tla_town::MORE_MODES.contains(&name) => self.run_tla_town_more(name),
            _ if super::guide_tla_fight::MODES.contains(&name) => self.run_tla_fight(name),
            _ if super::guide_tla_fight::CAVE_MODES.contains(&name) => self.run_tla_cave(name),
            _ if super::guide_tla_more::MODES.contains(&name) => self.run_tla_more(name),
            _ => Err(format!("The Lost Age has no recording {name}")),
        }
    }

    /// The flags a run clears after the defaults: The Broken Seal's stormy
    /// night and childhood, none of The Lost Age's.
    pub(super) fn story_clears(&self) -> Result<Vec<u16>, String> {
        if self.symbols.l.game == super::layout::Game::Tla {
            return Ok(Vec::new());
        }
        Ok(vec![self.flag("FLAG_VALE_STORM")?, self.flag("FLAG_CHILDHOOD")?])
    }

    /// The moment mode's field: The Broken Seal's chapter party, or The Lost
    /// Age's debug set MOMENT_PRESET (default PARTY_DAILA).
    pub(super) fn moment_enter(&self, setup: &Setup) -> Result<Field<'a>, String> {
        if self.symbols.l.game != super::layout::Game::Tla {
            return Field::enter(self.rom, self.symbols, setup);
        }
        let preset = std::env::var("MOMENT_PRESET").ok().and_then(|p| p.parse().ok()).unwrap_or(PARTY_DAILA);
        Field::enter_with_party(self.rom, self.symbols, setup, Some(party::tla(preset)?))
    }

    /// tla-en's ELF and The Lost Age's headers, read once.
    pub(super) fn tla_game() -> Result<&'static super::tables::Game, String> {
        static GAME: std::sync::OnceLock<Result<super::tables::Game, String>> = std::sync::OnceLock::new();
        GAME.get_or_init(|| super::tables::Game::read_edition(crate::compile::root(), "tla-en")).as_ref().map_err(Clone::clone)
    }

    /// The value The Lost Age's headers give `name` (ABILITY_FROST).
    pub(super) fn tla_constant(name: &str) -> Result<u16, String> {
        Ok(Self::tla_game()?.constant(name)? as u16)
    }

    /// The Lost Age's scene named `symbol` in gScenes (scenes.s), by number.
    pub(super) fn tla_scene(symbol: &str) -> Result<u16, String> {
        let root = crate::compile::root();
        let scenes = super::story::scenes_from_tree(root, "tla", "tla-en")?;
        scenes.iter().find(|s| s.symbol.as_deref() == Some(symbol)).map(|s| s.number).ok_or_else(|| format!("The Lost Age has no scene {symbol}"))
    }

    /// Enters The Lost Age's scene `symbol` by `entrance` with debug set
    /// `preset` for the party and `set` set, and runs until the player has
    /// control, keeping the moment the party was placed.
    pub(super) fn tla_enter(&self, symbol: &str, entrance: u16, preset: u32, set: &[u16]) -> Result<Entered<'a>, String> {
        let setup = Setup { scene: Self::tla_scene(symbol)?, entrance, set: set.to_vec(), clear: Vec::new() };
        let mut field = Field::enter_with_party(self.rom, self.symbols, &setup, Some(party::tla(preset)?))?;
        let placed = field.save();
        field.until_control()?;
        field.frames(20)?;
        Ok(Entered { field, placed })
    }
}

/// One recording played from a place with the pad: where it starts, the
/// party, where the leader is stood first (pixels, facing; None to start
/// where the party arrives), the keys held in turn, and what it shows.
pub(super) struct Walk {
    pub file: &'static str,
    pub scene: &'static str,
    pub entrance: u16,
    pub preset: u32,
    pub stand: Option<(i32, i32, u32)>,
    pub path: &'static [(u32, u32)],
    pub what: &'static str,
}

impl<'a> Guide<'a> {
    /// Enters a walk's place and stands the leader where it says.
    pub(super) fn tla_start(&self, walk: &Walk) -> Result<Entered<'a>, String> {
        let mut entered = self.tla_enter(walk.scene, walk.entrance, walk.preset, &[])?;
        if let Some(stand) = walk.stand {
            let spot = Self::spot_at(&mut entered.field, stand)?;
            entered.field.stand(&entered.placed, &spot)?;
        }
        Ok(entered)
    }

    /// Plays a walk and writes it; a walk that passes through a black screen
    /// shows CLEARED frames of the game on each side of it.
    pub(super) fn tla_walk(&mut self, walk: &Walk) -> Result<(), String> {
        let mut entered = self.tla_start(walk)?;
        let mut frames = Self::press_path_cleared(&mut entered.field, walk.path)?;
        // A path played again with a longer lead keeps as its first picture
        // the screen from before the state was loaded: the second is the
        // same moment, drawn after.
        frames.remove(0);
        self.world_gif(walk.file, &frames, walk.what)
    }

    /// Walks the leader toward (x, z) with the d-pad as a player would, along
    /// x and then along z, keeping every frame, until he is within a pixel
    /// of each or something stops him.
    pub(super) fn tla_walk_to(field: &mut Field, (x, z): (i32, i32), frames: &mut Vec<Vec<u16>>) -> Result<(), String> {
        for along_x in [true, false] {
            let (mut last, mut still) = (None, 0);
            for _ in 0..600 {
                let at = field.leader().ok_or("the scene has no leader")?;
                let d = if along_x { x - at.x } else { z - at.z };
                if d.abs() <= 1 || still > 20 {
                    break;
                }
                let key = match (along_x, d > 0) {
                    (true, true) => KEY_RIGHT,
                    (true, false) => KEY_LEFT,
                    (false, true) => KEY_DOWN,
                    (false, false) => KEY_UP,
                };
                field.hold(key);
                field.frames(1)?;
                frames.push(field.picture());
                still = if last == Some((at.x, at.z)) { still + 1 } else { 0 };
                last = Some((at.x, at.z));
            }
            field.hold(0);
        }
        Ok(())
    }

    /// Enters The Lost Age's scene and stands the leader where `trigger`
    /// would start, at the first spot `pick` accepts.
    pub(super) fn tla_stand(&self, scene: &str, entrance: u16, preset: u32, trigger: super::story::Trigger, pick: impl Fn(&super::field::Spot) -> bool) -> Result<(Entered<'a>, super::field::Spot), String> {
        let mut entered = self.tla_enter(scene, entrance, preset, &[])?;
        let first = match entered.field.spots(trigger) {
            Ok(spots) => {
                if std::env::var_os("ALCHEMY_TLA_DEBUG").is_some() {
                    eprintln!("{scene} {trigger:?}: spots {:?}", spots.iter().map(|s| (s.x, s.z, s.facing, s.walk)).collect::<Vec<_>>());
                }
                match spots.iter().find(|s| pick(s)) {
                    Some(&spot) => {
                        entered.field.stand(&entered.placed, &spot)?;
                        return Ok((entered, spot));
                    }
                    None => format!("no spot in {scene} fits {trigger:?}"),
                }
            }
            Err(e) => e,
        };
        // A room of a scene of several rooms: from each entrance whose room
        // holds the actor talked to.
        let super::story::Trigger::Talk(actor) = trigger else {
            return Err(first);
        };
        let at = entered.field.actor(actor).ok_or(first.clone())?;
        let header = entered.field.header();
        let others: Vec<u16> = entered.field.entrances().iter().filter(|e| e.number as u16 != entrance && super::field::box_holds(e.camera, header, at.x, at.z)).map(|e| e.number as u16).collect();
        for other in others {
            let Ok(mut entered) = self.tla_enter(scene, other, preset, &[]) else {
                continue;
            };
            if let Some(spot) = entered.field.spots(trigger).ok().and_then(|s| s.into_iter().find(|s| pick(s))) {
                entered.field.stand(&entered.placed, &spot)?;
                return Ok((entered, spot));
            }
        }
        Err(first)
    }
}
