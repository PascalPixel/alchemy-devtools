//! The Lost Age's town recordings, the mirrors of The Broken Seal's Vale,
//! Vault and Bilibin ones: Daila after the wave, where Felix, Jenna and
//! Sheba first come ashore (walkthroughs/: chapter 1), its people, its
//! shops, inn and sanctum, its doors and roofs, and the menus opened there.

use super::field::Spot;
use super::guide::Guide;
use super::guide_tla::{Walk, EAST, KEY_A, KEY_B, KEY_DOWN, KEY_LEFT, KEY_RIGHT, KEY_START, KEY_UP, NORTH, PARTY_DAILA, WEST};
use super::story::Trigger;

/// Daila as the party first comes (SceneId_Daila1), from the World Map, and
/// its houses (SceneId_DailaInteriors1) and sanctum (SceneId_DailaInteriors2).
const DAILA: &str = "Daila1";
const DAILA_HOUSES: &str = "DailaInteriors1";
const DAILA_SANCTUM: &str = "DailaInteriors2";
/// Daila's people (daila.c's trigger table, as `render event` names them):
/// the man by the well who says the wave left a lot behind, and the
/// keeper of the stall.
const DAILA_WAVE_MAN: u16 = 18;
const DAILA_STALL_KEEPER: u16 = 21;
/// The innkeeper (daila_interiors.c) and the healer of the sanctum.
const DAILA_INNKEEPER: u16 = 20;
const DAILA_HEALER: u16 = 8;
/// A guest of the inn (DailaInteriors1), who says he hears the seafood in
/// Madra is good.
const DAILA_INN_GUEST: u16 = 23;
/// The lines of the screen a window at its top covers.
const WINDOW_ROWS: usize = 52;

/// The field menu opened with A and its icon `icon` (0 Psynergy to 3
/// Status) chosen.
fn open_screen(icon: u32) -> Vec<(u32, u32)> {
    let mut path = vec![(0, 20), (KEY_A, 4), (0, 50)];
    for _ in 0..icon {
        path.extend([(KEY_RIGHT, 4), (0, 25)]);
    }
    path.extend([(KEY_A, 4), (0, 70)]);
    path
}

/// Each key of `keys` pressed for 4 frames, then `wait` frames.
fn presses(keys: &[u32], wait: u32) -> Vec<(u32, u32)> {
    keys.iter().flat_map(|&k| [(k, 4), (0, wait)]).collect()
}

/// The modes this file plays.
pub const MODES: &[&str] = &[
    "field-talk-walk",
    "talk",
    "menus",
    "menu-start",
    "summary-screen",
    "djinn-standby",
    "save-game",
    "runtime-save",
    "message-blanks",
    "sound-inn-fanfare",
    "shop",
    "sanctum-revive",
    "map-talk-twice",
];

impl<'a> Guide<'a> {
    pub(super) fn run_tla_town(&mut self, name: &str) -> Result<(), String> {
        match name {
            "field-talk-walk" => self.tla_talk_walk(),
            "talk" => self.tla_window_talk(),
            "menus" => self.tla_menu_path("menu/menus.gif", &MENUS, "the field menu in Daila, its icons and the Status screen"),
            "menu-start" => self.tla_menu_path("menu/menu-start.gif", &MENU_START, "Start closing the Psynergy screen and the field menu at once in Daila"),
            "summary-screen" => self.tla_summary_screen(),
            "djinn-standby" => self.tla_djinn_standby(),
            "save-game" => self.tla_menu_path("window/save.gif", &SAVE, "saving the game from the Pause menu in Daila"),
            "runtime-save" => self.tla_menu_path("runtime/save.gif", &SAVE, "saving from the Pause menu in Daila"),
            "message-blanks" => self.tla_message_blanks(),
            "sound-inn-fanfare" => self.tla_inn_fanfare(),
            "shop" => self.tla_shop(),
            "sanctum-revive" => self.tla_sanctum(),
            "map-talk-twice" => self.tla_talk_twice(),
            _ => Err(format!("no mode {name}")),
        }
    }

    /// Daila entered from the World Map, the leader where the party arrives.
    fn daila(&self) -> Result<super::guide::Entered<'a>, String> {
        self.tla_enter(DAILA, 0, PARTY_DAILA, &[])
    }

    /// Plays `path` in Daila from where the party arrives.
    fn tla_menu_path(&mut self, file: &str, path: &[(u32, u32)], what: &str) -> Result<(), String> {
        let mut entered = self.daila()?;
        let frames = Self::press_path(&mut entered.field, path)?;
        self.world_gif(file, &frames, what)
    }

    /// Felix walks up to the man by Daila's well and talks to him, then
    /// walks on: the walk stops for the talk and starts again after it.
    fn tla_talk_walk(&mut self) -> Result<(), String> {
        let (mut entered, spot) = self.tla_stand(DAILA, 0, PARTY_DAILA, Trigger::Talk(DAILA_WAVE_MAN), |_| true)?;
        // A step and a half back from where he talks, facing the man.
        let (dx, dz, toward) = match spot.facing {
            EAST => (-24, 0, KEY_RIGHT),
            WEST => (24, 0, KEY_LEFT),
            NORTH => (0, 24, KEY_UP),
            _ => (0, -24, KEY_DOWN),
        };
        let back = Spot { x: spot.x + dx, z: spot.z + dz, ..spot };
        entered.field.stand(&entered.placed, &back)?;
        let field = &mut entered.field;
        let mut frames = vec![field.picture()];
        Self::hold(field, 0, 15, &mut frames)?;
        Self::hold(field, toward, 26, &mut frames)?;
        Self::hold(field, 0, 10, &mut frames)?;
        // His two pages, each typed out and paged with A.
        for (keys, n) in TWO_PAGES {
            Self::hold(field, keys, n, &mut frames)?;
        }
        let away = if toward == KEY_UP || toward == KEY_DOWN { KEY_LEFT } else { KEY_DOWN };
        Self::hold(field, away, 60, &mut frames)?;
        Self::hold(field, 0, 10, &mut frames)?;
        self.world_gif("field/talk-walk.gif", &frames, "Felix talking to the man by Daila's well and walking on")
    }

    /// Talking to the man by Daila's well: the window opens, the line is
    /// typed out and the arrow bobs, the next page, and the window closes.
    fn tla_window_talk(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.tla_stand(DAILA, 0, PARTY_DAILA, Trigger::Talk(DAILA_WAVE_MAN), |_| true)?;
        let field = &mut entered.field;
        let mut frames = Self::press_path(field, &[(0, 20), (KEY_A, 3), (0, 150), (KEY_A, 3), (0, 120), (KEY_A, 3), (0, 40)])?;
        let last = frames.last().cloned().unwrap_or_default();
        frames.extend(std::iter::repeat_n(last, 20));
        self.world_gif("window/talk.gif", &frames, "talking to the man by Daila's well")
    }

    /// The Status screen in Daila: Felix's pages, R to Jenna, and B out.
    fn tla_summary_screen(&mut self) -> Result<(), String> {
        let mut path = open_screen(3);
        path.extend(presses(&[KEY_A], 60));
        path.extend(presses(&[KEY_A, KEY_A, KEY_A, super::guide_tla::KEY_R], 60));
        path.extend(presses(&[KEY_B, KEY_B, KEY_B], 30));
        self.tla_menu_path("menu/summary-screen.gif", &path, "the Status screen's pages in Daila")
    }

    /// The Djinn screen in Daila: Felix's Echo put on standby, the preview
    /// of what changes, and A.
    fn tla_djinn_standby(&mut self) -> Result<(), String> {
        let mut path = open_screen(1);
        path.extend([(0, 30), (KEY_A, 4), (0, 60), (KEY_A, 4), (0, 120), (KEY_A, 4), (0, 120)]);
        path.extend(presses(&[KEY_B, KEY_B], 35));
        self.tla_menu_path("menu/djinn-standby.gif", &path, "Echo put on standby from the Djinn screen in Daila")
    }

    /// Daila's innkeeper asked for a room across the counter: the price, a
    /// number filled into the line, and No.
    fn tla_message_blanks(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.tla_stand(DAILA_HOUSES, 0, PARTY_DAILA, Trigger::Talk(DAILA_INNKEEPER), |s| s.facing == WEST)?;
        entered.field.set_coins(500);
        let path = [(0, 20), (KEY_A, 3), (0, 70), (KEY_A, 3), (0, 90), (KEY_DOWN, 3), (0, 20), (KEY_A, 3), (0, 90), (KEY_A, 3), (0, 40)];
        let frames = Self::press_path(&mut entered.field, &path)?;
        self.world_gif("window/message-blanks.gif", &frames, "Daila's innkeeper's price, a number filled into the line")
    }

    /// A night at Daila's inn: the screen closes to black and opens again
    /// while the inn's jingle plays.
    fn tla_inn_fanfare(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.tla_stand(DAILA_HOUSES, 0, PARTY_DAILA, Trigger::Talk(DAILA_INNKEEPER), |s| s.facing == WEST)?;
        entered.field.set_coins(500);
        let path = [(0, 10), (KEY_A, 3), (0, 60), (KEY_A, 3), (0, 80), (KEY_A, 3), (0, 70), (KEY_A, 3), (0, 640)];
        let frames = Self::press_path_cleared(&mut entered.field, &path)?;
        self.world_gif("sound/inn-fanfare.gif", &frames, "a night at Daila's inn")
    }

    /// Daila's stall, the party given 500 coins: talked to across the
    /// counter, Buy, a Herb for Felix, and B out.
    fn tla_shop(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.tla_stand(DAILA, 0, PARTY_DAILA, Trigger::Talk(DAILA_STALL_KEEPER), |s| s.facing == NORTH)?;
        entered.field.set_coins(500);
        let path = [
            (0, 20),
            (KEY_A, 3),
            (0, 100),
            (KEY_A, 3),
            (0, 60),
            (KEY_A, 3),
            (0, 60),
            (KEY_A, 3),
            (0, 60),
            (KEY_A, 3),
            (0, 60),
            (KEY_A, 3),
            (0, 60),
            (KEY_B, 3),
            (0, 70),
            (KEY_B, 3),
            (0, 70),
            (KEY_A, 3),
            (0, 90),
        ];
        let frames = Self::press_path(&mut entered.field, &path)?;
        self.world_gif("shop/shop.gif", &frames, "buying a Herb at Daila's stall")
    }

    /// Daila's sanctum, Jenna downed and the party given 500 coins: the
    /// healer talked to across the counter, Revive, the hand over the party
    /// and Jenna chosen.
    fn tla_sanctum(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.tla_stand(DAILA_SANCTUM, 0, PARTY_DAILA, Trigger::Talk(DAILA_HEALER), |s| s.facing == NORTH)?;
        let flag = self.flag("FLAG_DAILA_HEALER_SENT_TO_POI")?;
        entered.field.set_flag(flag, true);
        entered.field.set_coins(500);
        Self::down(&mut entered.field, super::plan::JENNA)?;
        // The hand starts on the member who is down: it is moved to Sheba and
        // Felix, who need none, and back to Jenna, and A.
        let path = [(0, 20), (KEY_A, 3), (0, 100), (KEY_A, 3), (0, 80), (KEY_A, 3), (0, 60), (KEY_RIGHT, 3), (0, 50), (KEY_LEFT, 3), (0, 40), (KEY_LEFT, 3), (0, 50), (KEY_RIGHT, 3), (0, 40), (KEY_A, 3), (0, 60), (KEY_A, 3), (0, 200), (KEY_A, 3), (0, 80), (KEY_A, 3), (0, 60)];
        let frames = Self::press_path(&mut entered.field, &path)?;
        self.world_gif("shop/sanctum.gif", &frames, "Jenna revived at Daila's sanctum")
    }

    /// Daila's healer talked to twice from across the counter: the first
    /// time she asks the party whether they have time and nowhere to go
    /// (FLAG_DAILA_HEALER_SENT_TO_POI clear), the second she runs the
    /// sanctum.
    fn tla_talk_twice(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.tla_stand(DAILA_SANCTUM, 0, PARTY_DAILA, Trigger::Talk(DAILA_HEALER), |s| s.facing == NORTH)?;
        let field = &mut entered.field;
        let mut frames = vec![field.picture()];
        Self::hold(field, 0, 15, &mut frames)?;
        // The first talk: her question, Yes, and her two pages on Master Poi.
        // The second: the sanctum's welcome and its choices, cancelled.
        let path = [(KEY_A, 3), (0, 140), (KEY_A, 3), (0, 150), (KEY_A, 3), (0, 190), (KEY_A, 3), (0, 100), (KEY_A, 3), (0, 140), (KEY_B, 3), (0, 80), (KEY_A, 3), (0, 60)];
        for (keys, n) in path {
            Self::hold(field, keys, n, &mut frames)?;
        }
        self.world_gif("map/talk-twice.gif", &frames, "Daila's healer talked to twice")
    }

    /// `owner` downed in the field: no HP, the gauge empty.
    pub(super) fn down(field: &mut super::field::Field, owner: u8) -> Result<(), String> {
        let b = super::layout::BattleLayout::for_edition("tla-en")?;
        let unit = field.unit(owner)?;
        field.write16(unit + b.unit_hp, 0);
        field.write16(unit + b.unit_hp_gauge, 0);
        Ok(())
    }
}

/// The field menu opened with A in Daila, moved along its icons and into
/// the Status screen and back out.
const MENUS: [(u32, u32); 17] = [(0, 20), (KEY_A, 4), (0, 50), (KEY_RIGHT, 4), (0, 30), (KEY_RIGHT, 4), (0, 30), (KEY_RIGHT, 4), (0, 30), (KEY_A, 4), (0, 40), (KEY_A, 4), (0, 150), (KEY_B, 4), (0, 40), (KEY_B, 4), (0, 40)];
/// Start pressed two screens deep: every screen closes at once.
const MENU_START: [(u32, u32); 9] = [(0, 20), (KEY_A, 4), (0, 50), (KEY_A, 4), (0, 50), (KEY_A, 4), (0, 60), (KEY_START, 4), (0, 60)];
/// Start, Save your quest, the first file, and the messages to `Your game
/// was saved.`
const SAVE: [(u32, u32); 9] = [(0, 20), (KEY_START, 4), (0, 50), (KEY_A, 4), (0, 100), (KEY_A, 4), (0, 200), (KEY_A, 4), (0, 40)];
/// A talk of two pages from its first press: each page typed out and paged
/// with A, and the window closing.
const TWO_PAGES: [(u32, u32); 6] = [(KEY_A, 3), (0, 150), (KEY_A, 3), (0, 120), (KEY_A, 3), (0, 40)];

/// Debug parameter set 19: Felix, Jenna, Sheba and Piers at level 19, the
/// party once Piers has joined, who knows Frost.
const PARTY_PIERS: u32 = 19;
/// Daila once the puddles stand in it (SceneId_Daila2), the first puddle
/// Frost raises (daila.c: actor 22, by house 1), and the World Map outside
/// Daila's south road (WORLD_MAP from Daila's exit 8).
const DAILA_PUDDLES: &str = "Daila2";
const FIRST_PUDDLE: u16 = 22;
/// Tavi home again (daila_interiors.c names it): Daila's houses lead out to
/// Daila2, the puddles' Daila, and not to Daila1.
const FLAG_TAVI_RESCUED: u16 = 0x873;
const WORLD_MAP: (&str, u16) = ("WorldMap", 2);
/// House 1's door cell in Daila (exit 1), and where to stand below it.
const HOUSE_DOOR_BELOW: (i32, i32) = (120, 344);

/// The modes of the second batch.
pub const MORE_MODES: &[&str] = &["windows-zoom", "world-door", "front-door", "world-behind-roof", "layers-3d", "world-map-layers-3d", "scene-log-state"];

impl<'a> Guide<'a> {
    pub(super) fn run_tla_town_more(&mut self, name: &str) -> Result<(), String> {
        match name {
            "windows-zoom" => self.tla_window_zoom(),
            "world-door" => self.tla_walk(&DOOR),
            "front-door" => self.tla_walk(&SANCTUM_DOOR),
            "world-behind-roof" => self.tla_walk(&BEHIND_ROOF),
            "layers-3d" => {
                let mut entered = self.daila()?;
                self.explode_now(&mut entered.field, "field/layers-3d.gif", "Daila's screen")
            }
            "world-map-layers-3d" => {
                let mut entered = self.tla_enter(WORLD_MAP.0, WORLD_MAP.1, PARTY_DAILA, &[])?;
                let field = &mut entered.field;
                // A short walk first, so the view is the tilted one the
                // player walks in.
                field.hold(KEY_DOWN);
                field.frames(50)?;
                field.hold(0);
                field.frames(2)?;
                self.explode_now(field, "field/world-map-layers-3d.gif", "the World Map outside Daila")
            }
            "scene-log-state" => self.tla_puddle_state(),
            _ => Err(format!("no mode {name}")),
        }
    }

    /// A guest of Daila's inn talked to, his line typed and closed with A,
    /// as a close-up of the window, its zooms slowed (window_close_up).
    fn tla_window_zoom(&mut self) -> Result<(), String> {
        // A room of Daila's houses, where no water moves beside the window,
        // and a guest of the inn: his two pages, the window at the top of the
        // screen, over the lines the close-up keeps.
        let (mut entered, _) = self.tla_stand(DAILA_HOUSES, 0, PARTY_DAILA, Trigger::Talk(DAILA_INN_GUEST), |_| true)?;
        let frames = Self::press_path(&mut entered.field, &[(0, 20), (KEY_A, 3), (0, 150), (KEY_A, 3), (0, 150), (KEY_A, 3), (0, 50)])?;
        self.window_close_up(&frames, WINDOW_ROWS)
    }

    /// Frost cast on the puddle by Daila's first house: it rises as a
    /// pillar of ice. Felix walks into the house and out again, and the
    /// puddle is a puddle once more: its flag is a scene temp flag, wiped
    /// each time the party enters a scene.
    fn tla_puddle_state(&mut self) -> Result<(), String> {
        let frost = Self::tla_constant("ABILITY_FROST")?;
        let mut entered = self.tla_enter(DAILA_PUDDLES, 0, PARTY_PIERS, &[FLAG_TAVI_RESCUED])?;
        let mut frames = Self::record_trigger(&mut entered, Trigger::Psynergy(frost, FIRST_PUDDLE), None, 60)?;
        let field = &mut entered.field;
        // The pillar a moment, so the game shows CLEARED frames before the
        // house's black screen; then down from it and along to the door.
        Self::hold(field, 0, 150, &mut frames)?;
        let at = field.leader().ok_or("the scene has no leader")?;
        Self::tla_walk_to(field, (at.x, HOUSE_DOOR_BELOW.1), &mut frames)?;
        Self::tla_walk_to(field, HOUSE_DOOR_BELOW, &mut frames)?;
        Self::hold(field, KEY_UP, 60, &mut frames)?;
        // Inside: CLEARED frames on this side of each black screen.
        Self::hold(field, 0, super::field::CLEARED as u32 + 60, &mut frames)?;
        Self::hold(field, KEY_DOWN, 50, &mut frames)?;
        Self::hold(field, 0, super::field::CLEARED as u32 + 60, &mut frames)?;
        self.world_gif("scene/log-state.gif", &frames, "a puddle in Daila frozen into a pillar, and a puddle again after a house")
    }
}

/// Walking north into the door of Daila's third house (Daila_EnterDoor,
/// exit 3): the leader walks in and the room inside fades in.
const DOOR: Walk = Walk {
    file: "world/door.gif",
    scene: DAILA,
    entrance: 0,
    preset: PARTY_DAILA,
    stand: Some((344, 372, NORTH)),
    path: &[(0, 15), (KEY_UP, 110), (0, 70)],
    what: "walking into a house's door in Daila and the room inside",
};

/// Up the steps to the door of Daila's sanctum (exit 7): Daila fades out
/// and the sanctum fades in.
const SANCTUM_DOOR: Walk = Walk {
    file: "front/sanctum-door.gif",
    scene: DAILA,
    entrance: 0,
    preset: PARTY_DAILA,
    stand: Some((256, 176, NORTH)),
    path: &[(0, 20), (KEY_UP, 80), (0, 100)],
    what: "up through the sanctum's door, Daila giving way to the sanctum",
};

/// Walking east behind the house up on the rise in Daila and back: its roof
/// is drawn over the leader.
const BEHIND_ROOF: Walk = Walk {
    file: "world/behind-roof.gif",
    scene: DAILA,
    entrance: 0,
    preset: PARTY_DAILA,
    stand: Some((420, 210, EAST)),
    path: &[(0, 10), (KEY_RIGHT, 70), (0, 15), (KEY_LEFT, 70), (0, 10)],
    what: "walking behind a house's roof in Daila",
};
