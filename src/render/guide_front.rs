//! Recordings for The Broken Seal's front page, its engine page and the
//! engine's folder guides (runtime, graphics, sound, debug): each one
//! moment the player sees, played from a built edition with the pad held as
//! a player would, written under the guide folder it belongs to
//! (`front/sanctum-door.gif`).

use super::field::{Field, Setup, Spot};
use super::guide::Guide;
use super::story::Trigger;

const KEY_A: u32 = 0x1;
const KEY_B: u32 = 0x2;
const KEY_SELECT: u32 = 0x4;
const KEY_START: u32 = 0x8;
const KEY_RIGHT: u32 = 0x10;
const KEY_LEFT: u32 = 0x20;
const KEY_UP: u32 = 0x40;
const KEY_DOWN: u32 = 0x80;

/// Vale by the Sanctum's door (VALE_FROM_SANCTUM).
const VALE: (u16, u16) = (4, 1);
/// The world map as the party first leaves Vale (WORLD_MAP_FROM_VALE_PLAZA).
const WORLD_MAP: (u16, u16) = (2, 1);
/// Vale's gate, SceneId_ValePlaza, arrived at from the world map
/// (VALE_PLAZA_FROM_WORLD_MAP).
const VALE_PLAZA: (u16, u16) = (5, 10);
/// Sol Sanctum's entrance, arrived at from Vale at the foot of its path
/// (SOL_ENTRANCE_FROM_VALE).
const SOL_ENTRANCE: (u16, u16) = (19, 1);
/// Sol Sanctum's chest room (SOL_SANCTUM_RELIEFS_CHEST_ROOM_FROM_FIRST_RELIEF_ROOM)
/// and its chest's metatile behavior (MB_SOL_SANCTUM_RELIEFS_CHEST).
const SOL_CHEST_ROOM: (u16, u16) = (16, 2);
const MB_SOL_SANCTUM_RELIEFS_CHEST: u16 = 100;
/// Vale's strongman by the river (vale.c, ACTOR_STRONGMAN_BY_THE_RIVER).
const STRONGMAN_BY_THE_RIVER: u16 = 12;
/// Vale's inn and its innkeeper (cottage_rooms_b.c, ACTOR_INNKEEPER).
const VALE_INN: (u16, u16) = (8, 0);
const VALE_INNKEEPER: u16 = 13;
/// The title overlay's entrance that plays the intro after the prologue.
const TITLE_AFTER_PROLOGUE: (u16, u16) = (0, 3);
/// The title overlay's entrance that runs the credits (Import_CreditsRun).
const CREDITS: (u16, u16) = (0, 9);
/// The level room and the menu test room (scene_headers.s).
const LEVEL_ROOM: u16 = 199;
const MENU_TEST_ROOM: u16 = 200;
/// The level room's level editor and the menu test room's first Felix
/// (debug/level.c, debug/test.c).
const LEVEL_EDITOR: u16 = 11;
const FELIX: u16 = 16;
/// Facings in 1/65536 turns from east, as a spot holds them.
const WEST: u32 = 0x8000;
const NORTH: u32 = 0xc000;
const SOUTH: u32 = 0x4000;
/// The credits' timing (credits.c): the lead before the first picture, and
/// each picture's crossfade and hold, in frames.
const CREDITS_LEAD: usize = 300;
const CREDITS_FADE: usize = 64;
const CREDITS_HOLD: usize = 267;

/// The modes this file plays, after guide_engine::MODES.
pub const MODES: &[&str] = &[
    "front-door",
    "front-area-name",
    "front-warp-rows",
    "front-story",
    "engine-waiting",
    "front-chest",
    "engine-debug-select",
    "runtime-soft-reset",
    "runtime-save",
    "graphics-title-fade",
    "graphics-credits-names",
    "sound-inn-fanfare",
    "debug-map-select",
    "debug-level-editor",
    "debug-felix-facing",
];

fn lit(picture: &[u16]) -> bool {
    picture.iter().any(|&c| c & 0x7fff != 0)
}

impl<'a> Guide<'a> {
    pub(super) fn run_front(&mut self, name: &str) -> Result<(), String> {
        match name {
            "front-door" => self.front_door(),
            "front-area-name" => self.front_area_name(),
            "front-warp-rows" => self.front_warp_rows(),
            "front-story" => self.front_story(),
            "engine-waiting" => self.engine_waiting(),
            "front-chest" => self.front_chest(),
            "engine-debug-select" => self.engine_debug_select(),
            "runtime-soft-reset" => self.runtime_soft_reset(),
            "runtime-save" => self.runtime_save(),
            "graphics-title-fade" => self.graphics_title_fade(),
            "graphics-credits-names" => self.graphics_credits_names(),
            "sound-inn-fanfare" => self.sound_inn_fanfare(),
            "debug-map-select" => self.debug_map_select(),
            "debug-level-editor" => self.debug_level_editor(),
            "debug-felix-facing" => self.debug_felix_facing(),
            _ => Err(format!("no mode {name}")),
        }
    }

    /// Writes `frames` as `path` under the output folder, making its folder.
    fn front_gif(&mut self, path: &str, frames: &[Vec<u16>], what: &str) -> Result<(), String> {
        if let Some(dir) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(self.out.join(dir)).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
        }
        self.guide_gif(path, frames, what)
    }

    /// A scene entered with exactly the story flags given (the renderer's
    /// usual entry always sets FLAG_VALE_PARTY_LEFT), the player in control.
    fn enter_exactly(&self, place: (u16, u16), set: &[&str], clear: &[&str]) -> Result<Field<'a>, String> {
        let set = set.iter().map(|n| self.flag(n)).collect::<Result<Vec<u16>, String>>()?;
        let clear = clear.iter().map(|n| self.flag(n)).collect::<Result<Vec<u16>, String>>()?;
        let setup = Setup { scene: place.0, entrance: place.1, set, clear };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        field.until_control()?;
        field.frames(20)?;
        Ok(field)
    }

    /// Up through the Sanctum's door in Vale: Vale fades out and the
    /// Sanctum's hall fades in, the next map of the loop.
    fn front_door(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with(VALE, &[])?;
        let frames = Self::press_path(&mut entered.field, &[(0, 20), (KEY_UP, 40), (0, 100)])?;
        self.front_gif("front/sanctum-door.gif", &frames, "up through the Sanctum's door, Vale giving way to the Sanctum")
    }

    /// From the world map into Vale's gate, a new area, whose area
    /// name shows on arrival (AreaNameWindow_Show).
    fn front_area_name(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with(WORLD_MAP, &[])?;
        // The field leaves the area name out of every other recording.
        entered.field.show_notice = true;
        let frames = Self::press_path(&mut entered.field, &[(0, 30), (KEY_UP, 40), (0, 100), (KEY_UP, 30), (0, 40)])?;
        self.front_gif("front/area-name.gif", &frames, "into Vale from the world map, its area name shown")
    }

    /// Down the path from Sol Sanctum's entrance, twice: before the
    /// eruption the exit leads to Vale, and with FLAG_SOL_SANCTUM_ERUPTED set
    /// and FLAG_VALE_PARTY_LEFT clear the same exit leads to the erupting
    /// village (gSolSanctumEntrance_Exits).
    fn front_warp_rows(&mut self) -> Result<(), String> {
        let mut frames = Vec::new();
        let mut field = self.enter_exactly(SOL_ENTRANCE, &["FLAG_VALE_PARTY_LEFT"], &["FLAG_SOL_SANCTUM_ERUPTED", "FLAG_VALE_STORM", "FLAG_CHILDHOOD"])?;
        frames.extend(Self::press_path(&mut field, &[(0, 10), (KEY_DOWN, 50), (0, 80)])?);
        let mut field = self.enter_exactly(SOL_ENTRANCE, &["FLAG_SOL_SANCTUM_ERUPTED"], &["FLAG_VALE_PARTY_LEFT", "FLAG_VALE_STORM", "FLAG_CHILDHOOD"])?;
        frames.extend(Self::press_path(&mut field, &[(0, 10), (KEY_DOWN, 50), (0, 140)])?);
        self.front_gif("front/sol-entrance-warp.gif", &frames, "down Sol Sanctum's path to Vale, then to the erupting village")
    }

    /// Vale's gate three times in the story, the leader stepping in from the
    /// world map: the stormy night (FLAG_VALE_STORM, villagers gathered),
    /// after the party has left (FLAG_VALE_PARTY_LEFT) and after it reached
    /// Kalay (FLAG_KALAY_REACHED).
    fn front_story(&mut self) -> Result<(), String> {
        let mut frames = Vec::new();
        let stages: [(&[&str], &[&str]); 3] = [
            (&["FLAG_VALE_STORM", "FLAG_CHILDHOOD"], &["FLAG_VALE_PARTY_LEFT", "FLAG_KALAY_REACHED"]),
            (&["FLAG_VALE_PARTY_LEFT"], &["FLAG_VALE_STORM", "FLAG_KALAY_REACHED", "FLAG_CHILDHOOD"]),
            (&["FLAG_VALE_PARTY_LEFT", "FLAG_KALAY_REACHED"], &["FLAG_VALE_STORM", "FLAG_CHILDHOOD"]),
        ];
        for (set, clear) in stages {
            let mut field = self.enter_exactly(VALE_PLAZA, set, clear)?;
            frames.extend(Self::press_path(&mut field, &[(0, 20), (KEY_UP, 24), (0, 56)])?);
        }
        self.front_gif("front/vale-plaza-story.gif", &frames, "Vale's gate on the stormy night, after the party left and after Kalay")
    }

    /// Talking to the strongman by Vale's river: the line types out and
    /// waits for A while the river keeps flowing.
    fn engine_waiting(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.stand_for(VALE, Trigger::Talk(STRONGMAN_BY_THE_RIVER), STRONGMAN_BY_THE_RIVER)?;
        let frames = Self::press_path(&mut entered.field, &[(0, 20), (KEY_A, 3), (0, 150), (KEY_A, 3), (0, 120), (KEY_A, 3), (0, 30)])?;
        self.front_gif("engine/waiting-talk.gif", &frames, "talking to the strongman by Vale's river")
    }

    /// Opening the chest in Sol Sanctum: the Small Jewel rises and the
    /// treasure fanfare plays; the chest's row lasts until
    /// FLAG_SOL_SANCTUM_FIRST_SMALL_JEWEL_FOUND is set.
    fn front_chest(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with(SOL_CHEST_ROOM, &[])?;
        let frames = Self::record_trigger(&mut entered, Trigger::Search(MB_SOL_SANCTUM_RELIEFS_CHEST), None, 20)?;
        self.front_gif("front/chest.gif", &frames, "a chest opened in Sol Sanctum")
    }

    /// The debug map selector in Vale with gDebugMode set: Start pressed
    /// with B held (SceneManager_Run), the map stepped down `steps` times from Vale's
    /// number with Left, then A, which takes the party there.
    fn selector(&mut self, steps: u32, tail: u32) -> Result<Vec<Vec<u16>>, String> {
        let mode = self.sym("gDebugMode")?;
        let mut entered = self.enter_with(VALE, &[])?;
        let field = &mut entered.field;
        field.write8(mode, 1);
        let mut path = vec![(0, 20), (KEY_B, 4), (KEY_B | KEY_START, 4), (0, 40)];
        for _ in 0..steps {
            path.extend([(KEY_LEFT, 3), (0, 20)]);
        }
        path.extend([(0, 20), (KEY_A, 3), (0, tail)]);
        Self::press_path(field, &path)
    }

    /// Vale's number stepped down past map 0 to SceneId_MenuTest, and A.
    fn engine_debug_select(&mut self) -> Result<(), String> {
        let frames = self.selector(5, 110)?;
        self.front_gif("engine/debug-select.gif", &frames, "the debug map selector opened in Vale, warping to the menu test room")
    }

    /// One step further, to SceneId_IncreaseItemAndLevel, and A.
    fn debug_map_select(&mut self) -> Result<(), String> {
        let frames = self.selector(6, 110)?;
        self.front_gif("debug/map-select.gif", &frames, "the debug map selector opened in Vale, warping to the level room")
    }

    /// A, B, Select and Start pressed together in Vale: the game starts
    /// again, shown from Camelot's logo (the Nintendo logo before it is left
    /// out).
    fn runtime_soft_reset(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with(VALE, &[])?;
        let field = &mut entered.field;
        let mut frames = Self::press_path(field, &[(0, 40), (KEY_A | KEY_B | KEY_SELECT | KEY_START, 6), (0, 1)])?;
        let mut after = Vec::new();
        Self::hold(field, 0, 700, &mut after)?;
        // After the reset: black, the Nintendo logo, black, Camelot's logo.
        let first = after.iter().position(|f| lit(f)).ok_or("nothing after the reset")?;
        let black = first + after[first..].iter().position(|f| !lit(f)).ok_or("the Nintendo logo never ends")?;
        let camelot = black + after[black..].iter().position(|f| lit(f)).ok_or("no Camelot logo")?;
        frames.extend_from_slice(&after[..first.min(30)]);
        frames.extend_from_slice(&after[camelot..(camelot + 120).min(after.len())]);
        self.front_gif("runtime/soft-reset.gif", &frames, "a soft reset in Vale and the game starting again at Camelot's logo")
    }

    /// Start in Vale, Save your quest, the first file, and the save written.
    fn runtime_save(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with(VALE, &[])?;
        let path = [(0, 20), (KEY_START, 4), (0, 50), (KEY_A, 3), (0, 60), (KEY_A, 3), (0, 120), (KEY_A, 3), (0, 30)];
        let frames = Self::press_path(&mut entered.field, &path)?;
        self.front_gif("runtime/save.gif", &frames, "saving from the pause menu in Vale")
    }

    /// The title intro's first seconds after the prologue: the palette
    /// fade from near-black to the picture's colours.
    fn graphics_title_fade(&mut self) -> Result<(), String> {
        let setup = Setup { scene: TITLE_AFTER_PROLOGUE.0, entrance: TITLE_AFTER_PROLOGUE.1, set: Vec::new(), clear: Vec::new() };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        let mut frames = Vec::new();
        Self::hold(&mut field, 0, 400, &mut frames)?;
        let first = frames.iter().position(|f| lit(f)).ok_or("the title stayed black")?;
        let start = first.saturating_sub(10);
        self.front_gif("graphics/title-fade.gif", &frames[start..(first + 110).min(frames.len())], "the title intro fading in from near-black")
    }

    /// The credits' names scrolling over the second picture, Sol Sanctum's
    /// hall, after its crossfade and before the next.
    fn graphics_credits_names(&mut self) -> Result<(), String> {
        let setup = Setup { scene: CREDITS.0, entrance: CREDITS.1, set: Vec::new(), clear: Vec::new() };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        let mut frames = Vec::new();
        Self::hold(&mut field, 0, 2000, &mut frames)?;
        let first = frames.iter().position(|f| lit(f)).ok_or("the credits stayed black")?;
        let start = first + CREDITS_LEAD + 2 * (CREDITS_FADE + CREDITS_HOLD) - 280;
        if start + 200 > frames.len() {
            return Err("the credits ended early".to_string());
        }
        self.front_gif("graphics/credits-names.gif", &frames[start..start + 200], "the credits' names scrolling up over Sol Sanctum's hall")
    }

    /// A night at Vale's inn: the screen closes to black and opens again
    /// while the inn's fanfare plays.
    fn sound_inn_fanfare(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.stand_where(VALE_INN, Trigger::Talk(VALE_INNKEEPER), VALE_INNKEEPER, |s| s.facing == NORTH)?;
        entered.field.set_coins(500);
        let path = [(0, 10), (KEY_A, 3), (0, 60), (KEY_A, 3), (0, 80), (KEY_A, 3), (0, 70), (KEY_A, 3), (0, 640)];
        let frames = Self::press_path(&mut entered.field, &path)?;
        self.front_gif("sound/inn-fanfare.gif", &frames, "a night at Vale's inn")
    }

    /// Talking to the level room's level editor: A raises the party a
    /// level, Start five.
    fn debug_level_editor(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with((LEVEL_ROOM, 0), &[])?;
        let at = entered.field.actor(LEVEL_EDITOR).ok_or("the level editor is not placed")?;
        let spot = Spot { x: at.x + 16, z: at.z, layer: at.layer, height: at.y, facing: WEST, walk: 0, cell: None };
        entered.field.stand(&entered.placed, &spot)?;
        let frames = Self::press_path(&mut entered.field, &[(0, 20), (KEY_A, 3), (0, 90), (KEY_A, 3), (0, 50), (KEY_A, 3), (0, 50), (KEY_START, 3), (0, 60), (KEY_B, 3), (0, 30)])?;
        self.front_gif("debug/level-editor.gif", &frames, "the level room's level editor raising the party")
    }

    /// The menu test room's first Felix talked to from his south, facing
    /// north (the weapon shop's lines, paged with Right), then from his
    /// north, facing south (the Sanctum's).
    fn debug_felix_facing(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with((MENU_TEST_ROOM, 0), &[])?;
        let at = entered.field.actor(FELIX).ok_or("Felix is not placed")?;
        let spot = Spot { x: at.x, z: at.z + 16, layer: at.layer, height: at.y, facing: NORTH, walk: 0, cell: None };
        entered.field.stand(&entered.placed, &spot)?;
        let mut frames = Self::press_path(&mut entered.field, &[(0, 20), (KEY_A, 3), (0, 90), (KEY_RIGHT, 3), (0, 50), (KEY_B, 3), (0, 30)])?;
        let spot = Spot { x: at.x, z: at.z - 16, layer: at.layer, height: at.y, facing: SOUTH, walk: 0, cell: None };
        entered.field.stand(&entered.placed, &spot)?;
        frames.extend(Self::press_path(&mut entered.field, &[(0, 20), (KEY_A, 3), (0, 90), (KEY_B, 3), (0, 30)])?);
        self.front_gif("debug/felix-facing.gif", &frames, "the menu test room's Felix talked to from two sides")
    }
}
