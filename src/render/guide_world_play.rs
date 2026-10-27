//! Recordings for the world layer's guides (world.md and the field,
//! overworld, layout and map folder guides): each one thing the player
//! sees, played from a built edition with the pad held as a player would,
//! from the moment it starts to the moment it is over.

use super::field::Field;
use super::guide::{Entered, Guide};
use std::path::Path;

/// KEYINPUT bits.
const KEY_A: u32 = 0x1;
const KEY_RIGHT: u32 = 0x10;
const KEY_LEFT: u32 = 0x20;
const KEY_UP: u32 = 0x40;
const KEY_DOWN: u32 = 0x80;
const KEY_L: u32 = 0x200;

/// The modes this file plays, after guide::MODES and guide_engine::MODES.
pub const MODES: &[&str] = &[
    "world-door",
    "world-wall-slide",
    "world-wooden-box",
    "world-behind-roof",
    "world-map-battle",
    "field-talk-walk",
    "field-well-route",
    "field-step-height",
    "field-world-map-zoom",
    "overworld-battle-return",
    "overworld-move-nothing",
    "layout-ledge",
    "layout-waterfall",
    "map-arrive",
    "map-talk-twice",
    "layout-haze",
    "layout-psynergy-stone",
    "map-colosso-race",
    "map-venus-star-thrown",
    "venus-lighthouse-aerie-beam-whole",
    "venus-lighthouse-aerie-eruption-whole",
    "venus-lighthouse-aerie-story-dollhouse",
];

impl<'a> Guide<'a> {
    pub(super) fn run_world_play(&mut self, name: &str) -> Result<(), String> {
        match name {
            "world-door" => self.play(&DOOR),
            "world-wall-slide" => self.play(&WALL_SLIDE),
            "world-wooden-box" => self.wooden_box(),
            "world-behind-roof" => self.play(&BEHIND_ROOF),
            "world-map-battle" => self.world_map_battle(),
            "field-talk-walk" => self.talk_walk(),
            "field-well-route" => self.play(&WELL_ROUTE),
            "field-step-height" => self.play(&STEP_HEIGHT),
            "field-world-map-zoom" => self.play(&WORLD_MAP_ZOOM),
            "overworld-battle-return" => self.battle_return(),
            "overworld-move-nothing" => self.move_nothing(),
            "layout-ledge" => self.play(&LEDGE),
            "layout-waterfall" => self.play(&WATERFALL),
            "map-arrive" => self.play(&ARRIVE),
            "map-talk-twice" => self.talk_twice(),
            _ => self.run_docs_world(name),
        }
    }

    /// Writes `frames` as `file` under the output folder, making its folder.
    pub(super) fn world_gif(&mut self, file: &str, frames: &[Vec<u16>], what: &str) -> Result<(), String> {
        if let Some(parent) = Path::new(file).parent() {
            std::fs::create_dir_all(self.out.join(parent)).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        self.guide_gif(file, frames, what)
    }

    /// Enters `place` with the story as the party first reaches scene `of`
    /// (0: only the flags `set`), with `set` set as well.
    fn world_enter(&self, place: (u16, u16), of: u16, set: &[&str]) -> Result<Entered<'a>, String> {
        if of == 0 {
            return self.enter_with(place, set);
        }
        let mut entered = self.enter_first_visit(place, of)?;
        if !set.is_empty() {
            let flags = set.iter().map(|n| self.flag(n)).collect::<Result<Vec<u16>, String>>()?;
            entered.field.load(&entered.placed);
            for f in flags {
                entered.field.set_flag(f, true);
            }
            entered.field.until_control()?;
            entered.field.frames(20)?;
        }
        Ok(entered)
    }
}

/// Facings, 1/65536 turns (enum Facing).
const EAST: u32 = 0;
const SOUTH: u32 = 0x4000;
const WEST: u32 = 0x8000;
const NORTH: u32 = 0xc000;

/// Vale's part with the Sanctum (SceneId_Vale, entered from the world map),
/// the world map as the party leaves Vale, and Vale's houses
/// (SceneId_CottageRoomsA) entered by the north door (COTTAGE_ROOMS_A_FROM_VALE_NORTH).
const VALE: (u16, u16) = (4, 1);
const WORLD_MAP: (u16, u16) = (2, 1);
const COTTAGE_ROOMS_A_NORTH: (u16, u16) = (7, 2);
/// Vault (SceneId_Vault) from the world map's south road, and its houses on
/// the party's return (SceneId_LogRoomsB), entered by the mayor's
/// door (LOG_ROOMS_B_FROM_VAULT_REVISIT_NORTHWEST).
const VAULT: (u16, u16) = (20, 0);
const VAULT_HOUSES_REVISIT: (u16, u16) = (24, 10);
/// Goma Cave's pond room (SceneId_GomaCavePond), set as the party first reaches
/// the pillar room (SceneId_GomaCavePillar), as random-battle.gif has it.
const GOMA_CAVE_POND: (u16, u16) = (26, 0);
const GOMA_CAVE_PILLAR: u16 = 27;
/// Bilibin Cave's entrance (SceneId_BilibinCaveEntrance).
const BILIBIN_CAVE_ENTRANCE: (u16, u16) = (49, 0);
/// The scene number of a battle (MAP_BATTLE).
const MAP_BATTLE: u16 = 0x1fe;

/// One recording: where it starts, the pad held from there, and what it
/// shows.
struct Play {
    file: &'static str,
    place: (u16, u16),
    /// The scene whose first visit sets the story flags (0: none).
    of: u16,
    set: &'static [&'static str],
    /// Where the leader is stood first (x, z in pixels, facing), or None to
    /// start where the party arrives.
    stand: Option<(i32, i32, u32)>,
    path: &'static [(u32, u32)],
    what: &'static str,
}

/// Walking north into the door of Jenna's grandparents' house in Vale
/// (Vale_EnterJennasGrandparentsHouse, metatile behavior 9): the door
/// opens, the leader walks in and the exit VALE_TO_COTTAGE_ROOMS_A_NORTH ends the
/// map, and the room inside fades in.
const DOOR: Play = Play {
    file: "world/door.gif",
    place: VALE,
    of: 0,
    set: &[],
    stand: Some((328, 1190, NORTH)),
    path: &[(0, 15), (KEY_UP, 110), (0, 70)],
    what: "walking into a house's door in Vale and the room inside",
};

/// Up and right held below the Sanctum's front in Vale: the leader slides
/// right along the front wall, then up along the cliff
/// (Actor_UpdatePlayerControl's headings off the pad's).
const WALL_SLIDE: Play = Play {
    file: "world/wall-slide.gif",
    place: VALE,
    of: 0,
    set: &[],
    stand: None,
    path: &[(KEY_DOWN, 25), (0, 10), (KEY_UP | KEY_RIGHT, 130), (0, 15)],
    what: "sliding along the Sanctum's wall and the cliff in Vale",
};

/// Walking east behind the roof of the house below the leader's path in
/// Vale and back: the roof is drawn over him (the overhead map layer).
const BEHIND_ROOF: Play = Play {
    file: "world/behind-roof.gif",
    place: VALE,
    of: 0,
    set: &[],
    stand: Some((336, 1166, EAST)),
    path: &[(0, 10), (KEY_RIGHT, 90), (0, 15), (KEY_LEFT, 90), (0, 10)],
    what: "walking behind a house's roof in Vale",
};

/// Vault's two children running round the well, each on its eight-point
/// route (gVault_BoyRoundTheWellRoute, gVault_GirlRoundTheWellRoute), about
/// one lap.
const WELL_ROUTE: Play = Play {
    file: "field/well-route.gif",
    place: VAULT,
    of: VAULT.0,
    set: &[],
    stand: Some((180, 440, WEST)),
    path: &[(0, 200)],
    what: "Vault's children running round the well",
};

/// Walking north out of Goma Cave's pond onto the rim at the shelf's edge,
/// 32 pixels higher than the ground on either side: the leader stops.
const STEP_HEIGHT: Play = Play {
    file: "field/step-height.gif",
    place: GOMA_CAVE_POND,
    of: GOMA_CAVE_PILLAR,
    set: &[],
    stand: None,
    path: &[(0, 10), (KEY_UP, 70), (0, 20)],
    what: "stopped at the rim of the pond in Goma Cave",
};

/// The world map as the party leaves Vale: walking south, then L held to
/// pull the view back (FieldEffect_RunVisibilityTransition) and let go.
const WORLD_MAP_ZOOM: Play = Play {
    file: "field/world-map-zoom.gif",
    place: WORLD_MAP,
    of: 0,
    set: &[],
    stand: None,
    path: &[(KEY_DOWN, 50), (0, 15), (KEY_L, 100), (0, 50)],
    what: "L held on the world map",
};

/// Jumping down a ledge (MB_LEDGE) in Bilibin Cave's entrance with Down
/// held (Field_RunPushCell).
const LEDGE: Play = Play {
    file: "layout/ledge.gif",
    place: BILIBIN_CAVE_ENTRANCE,
    of: BILIBIN_CAVE_ENTRANCE.0,
    set: &[],
    stand: Some((408, 176, SOUTH)),
    path: &[(0, 10), (KEY_DOWN, 60), (0, 25)],
    what: "jumping down a ledge in Bilibin Cave's entrance",
};

/// Vale's waterfall and the river below it, their tiles swapped on a timer
/// by the layout's tileset animation (TilesetAnim_Update).
const WATERFALL: Play = Play {
    file: "layout/waterfall.gif",
    place: VALE,
    of: 0,
    set: &[],
    stand: Some((280, 1080, WEST)),
    path: &[(0, 120)],
    what: "Vale's waterfall and river",
};

/// Walking out of the north door of Vale's houses: the map ends and Vale
/// loads with the party at the door (VALE_FROM_COTTAGE_ROOMS_A_NORTH).
const ARRIVE: Play = Play {
    file: "map/arrive.gif",
    place: COTTAGE_ROOMS_A_NORTH,
    of: 0,
    set: &[],
    stand: None,
    path: &[(0, 10), (KEY_DOWN, 60), (0, 110)],
    what: "leaving a house in Vale and arriving outside its door",
};

impl<'a> Guide<'a> {
    /// Enters a recording's place, stands the leader, and keeps every frame
    /// of its path.
    fn start(&self, play: &Play) -> Result<(Entered<'a>, Vec<Vec<u16>>), String> {
        let mut entered = self.world_enter(play.place, play.of, play.set)?;
        if let Some((x, z, facing)) = play.stand {
            let spot = Self::spot_at(&mut entered.field, (x, z, facing))?;
            entered.field.stand(&entered.placed, &spot)?;
        }
        let mut frames = vec![entered.field.picture()];
        for &(keys, n) in play.path {
            Self::hold(&mut entered.field, keys, n, &mut frames)?;
        }
        Ok((entered, frames))
    }

    fn play(&mut self, play: &Play) -> Result<(), String> {
        let (_, frames) = self.start(play)?;
        self.world_gif(play.file, &frames, play.what)
    }

    /// Presses A and keeps every frame until the talk or search it starts
    /// is over and the player has control again, pressing A through each
    /// page that waits.
    pub(super) fn talk_through(field: &mut Field, frames: &mut Vec<Vec<u16>>) -> Result<(), String> {
        Self::hold(field, KEY_A, 3, frames)?;
        // The field loop stops stepping while the talk runs.
        let (mut last, mut still, mut talking, mut waited) = (field.steps(), 0, false, 0);
        for _ in 0..3000 {
            field.frames(1)?;
            frames.push(field.picture());
            let steps = field.steps();
            if steps == last {
                still += 1;
                if still >= 3 {
                    talking = true;
                }
            } else {
                if talking {
                    return Ok(());
                }
                still = 0;
            }
            last = steps;
            waited = if field.waiting() { waited + 1 } else { 0 };
            if waited >= 25 {
                Self::hold(field, KEY_A, 3, frames)?;
                waited = 0;
            }
        }
        Err("the talk never handed back control".to_string())
    }

    /// A wooden box searched in Vale (MB_WOODEN_BOX): `Isaac checked the
    /// wooden box...` and `but didn't find anything.`, the lines every map
    /// shares.
    fn wooden_box(&mut self) -> Result<(), String> {
        let play = Play { file: "world/wooden-box.gif", place: VALE, of: 0, set: &[], stand: Some((152, 1226, NORTH)), path: &[(0, 15), (KEY_UP, 8), (0, 15)], what: "" };
        let (mut entered, mut frames) = self.start(&play)?;
        Self::talk_through(&mut entered.field, &mut frames)?;
        Self::hold(&mut entered.field, 0, 20, &mut frames)?;
        self.world_gif(play.file, &frames, "a wooden box searched in Vale")
    }

    /// Talking to the young man who welcomes the party to Vault, then
    /// walking on: the walk stops for the talk and starts again after it.
    fn talk_walk(&mut self) -> Result<(), String> {
        let play = Play { file: "field/talk-walk.gif", place: VAULT, of: VAULT.0, set: &[], stand: Some((381, 380, NORTH)), path: &[(0, 10), (KEY_UP, 20), (0, 10)], what: "" };
        let (mut entered, mut frames) = self.start(&play)?;
        Self::talk_through(&mut entered.field, &mut frames)?;
        Self::hold(&mut entered.field, 0, 10, &mut frames)?;
        Self::hold(&mut entered.field, KEY_LEFT, 60, &mut frames)?;
        Self::hold(&mut entered.field, 0, 10, &mut frames)?;
        self.world_gif(play.file, &frames, "talking to a villager in Vault and walking on")
    }

    /// The mayor of Vault talked to twice in his house on the party's
    /// return: the first time he welcomes Isaac back
    /// (LogRoomsB_WelcomeIsaacBack, while
    /// FLAG_VAULT_REVISIT_ISAAC_WELCOMED_BACK is clear), the second he only
    /// warns of the thieves (LogRoomsB_WarnOfThievesRevenge).
    fn talk_twice(&mut self) -> Result<(), String> {
        let play = Play { file: "map/talk-twice.gif", place: VAULT_HOUSES_REVISIT, of: VAULT_HOUSES_REVISIT.0, set: &[], stand: Some((510, 452, EAST)), path: &[(0, 15)], what: "" };
        let (mut entered, mut frames) = self.start(&play)?;
        Self::talk_through(&mut entered.field, &mut frames)?;
        Self::hold(&mut entered.field, 0, 30, &mut frames)?;
        Self::talk_through(&mut entered.field, &mut frames)?;
        Self::hold(&mut entered.field, 0, 20, &mut frames)?;
        self.world_gif(play.file, &frames, "the mayor of Vault talked to twice")
    }

    /// Walking the world map south of Vale until a random battle starts:
    /// from two and a half seconds before it to the battle's opening.
    fn world_map_battle(&mut self) -> Result<(), String> {
        let mut entered = self.world_enter(WORLD_MAP, 0, &[])?;
        let field = &mut entered.field;
        let mut frames = vec![field.picture()];
        let legs = [KEY_DOWN, KEY_LEFT, KEY_UP, KEY_RIGHT];
        let mut at = None;
        'walk: for leg in 0..40 {
            field.hold(legs[leg % 4]);
            for _ in 0..60 {
                field.frames(1)?;
                frames.push(field.picture());
                if field.scene() == MAP_BATTLE {
                    at = Some(frames.len());
                    break 'walk;
                }
            }
        }
        field.hold(0);
        let at = at.ok_or("no battle came on the world map")?;
        Self::hold(field, 0, 240, &mut frames)?;
        self.world_gif("world/world-map-battle.gif", &frames[at.saturating_sub(150)..], "a random battle on the world map")
    }

    /// A random battle in Goma Cave fought to its end with A, and the cave
    /// loaded again with the party where it stood (Actor_Snapshot,
    /// Actor_Restore): from the battle's last seconds to the walk
    /// again.
    fn battle_return(&mut self) -> Result<(), String> {
        let (place, stretch) = self.goma_cave_stretch()?;
        let mut entered = self.enter_goma_cave(place)?;
        let field = &mut entered.field;
        let mut walk = Vec::new();
        let at = Self::walk_stretch(field, &stretch, 80, &mut walk, &mut 0, &mut |_| {})?.ok_or("no battle came")?;
        let before = walk[at.saturating_sub(60)..].to_vec();
        let mut frames = Vec::new();
        let mut back = None;
        for n in 0..12000u32 {
            field.hold(if n % 20 < 3 { KEY_A } else { 0 });
            field.frames(1)?;
            frames.push(field.picture());
            if field.scene() == place.0 {
                back = Some(frames.len());
                break;
            }
        }
        field.hold(0);
        let back = back.ok_or("the battle never ended")?;
        eprintln!("battle-return: the battle took {back} frames");
        // Until the player walks again.
        let steps = field.steps();
        for _ in 0..600 {
            field.frames(1)?;
            frames.push(field.picture());
            if field.steps() > steps + 30 {
                break;
            }
        }
        Self::hold(field, stretch.keys[0], 40, &mut frames)?;
        // Isaac's and Garet's last swings, for the sprites' guide.
        let swings = back.saturating_sub(300).max(120);
        self.world_gif("field/battle-swing.gif", &frames[swings..(swings + 150).min(back)], "the last swings of a Goma Cave battle")?;
        // The walk and the battle's opening, then, cut, its last ten
        // seconds and the cave again.
        let mut kept = before;
        kept.extend_from_slice(&frames[..120.min(back)]);
        kept.extend_from_slice(&frames[back.saturating_sub(300).max(120)..]);
        self.world_gif("overworld/battle-return.gif", &kept, "a Goma Cave battle's end and the cave again")
    }

    /// Move cast in Vale with nothing ahead: the hand appears and pushes,
    /// and nothing moves, since no trigger answers it there.
    fn move_nothing(&mut self) -> Result<(), String> {
        let play = Play { file: "overworld/move-nothing.gif", place: VALE, of: 0, set: &[], stand: Some((300, 470, EAST)), path: &[(0, 10)], what: "" };
        let (mut entered, mut frames) = self.start(&play)?;
        let field = &mut entered.field;
        let ability = super::source::ability_id("ABILITY_MOVE")?;
        let at = field.abilities(0)?.iter().position(|&a| a == ability).ok_or("Isaac has no Move here")?;
        let mut path = vec![(KEY_A, 3), (0, 50), (KEY_A, 3), (0, 50), (KEY_A, 3), (0, 50)];
        for _ in 0..at {
            path.extend([(KEY_DOWN, 3), (0, 15)]);
        }
        path.push((0, 20));
        for (keys, n) in path {
            Self::hold(field, keys, n, &mut frames)?;
        }
        // From the last moment of the Psynergy list, Move chosen, to the
        // colours' return.
        let from = frames.len().saturating_sub(20);
        Self::hold(field, KEY_A, 3, &mut frames)?;
        Self::hold(field, 0, 170, &mut frames)?;
        self.world_gif(play.file, &frames[from..], "Move cast in Vale with nothing ahead")
    }
}
