//! A field scene, run from a built edition: where Game_ResetForNewGame has
//! set a new game's defaults (GameState_InitDefaults) and makes its next call
//! (PaletteGlow_Update), the run writes the scene, entrance and story flags
//! into gGameState, so the main loop loads that scene instead of the title.
//! The location-name notice (UiTimedNotice_Create) returns at once. The
//! scene is the player's once Field_RunScene's field loop takes its first
//! step (Field_ProcessStep).
//!
//! A map is stitched from one moment of the game: for each 240 by 160 view
//! of the box the machine is loaded again, the map's followed point
//! (MapScrollWork.origin) is pinned to gGameState.x, y and z holding the
//! view's focus at each entry of its two readers, and the next scheduler
//! call of Map_UpdateLayerScroll runs Map_CenterOnFollowedPoint instead,
//! which redraws each layer around the point as the event camera does. The
//! actors' task (Object_UpdateAllThumb) is skipped meanwhile, so no actor
//! moves or draws on the shared random numbers; the third frame is the view.
//!
//! An event starts with the leader where the player would stand to start
//! it: beside the actor it talks to, on its map layer and level with it, or
//! a step from the cell it touches or searches, on the ground there
//! (Map_GetTerrainHeight, run in the machine between two steps of the field
//! loop). Then A (talk, search), a field Psynergy request (FieldWork.command)
//! or walking onto the cell (touch) starts it, and the field loop's own
//! dispatch (Scene_TalkToActor, Scene_RunTouchOrExit, Field_SearchTile,
//! Field_UsePsynergy) shows which event began. Each message page is kept once
//! it is typed and waits for the button (UiText_CheckTextDismissed), and each
//! yes or no choice (Menu_RunConfirmSelection) once shown; then A moves on.
//! The event is over when the field loop steps again with nothing to read.

use super::mgba::{Gba, HEIGHT, KEY_A, KEY_DOWN, KEY_LEFT, KEY_RIGHT, KEY_UP, LR, SP, WIDTH};
use super::story::Trigger;
use crate::ld_map::Map;

/// struct GameState (game_state.h), struct MapScrollWork (map_scroll.h),
/// struct FieldWork (field_runtime.h), struct SceneEntries and the object
/// slots' offsets.
const GS_SCENE: u32 = 0x1c0;
const GS_ENTRANCE: u32 = 0x1c2;
/// x, y and z: three s32 in a row, 16.16 pixels.
const GS_X: u32 = 0x1dc;
const GS_Y: u32 = 0x1e0;
const GS_Z: u32 = 0x1e4;
const GS_HEADING: u32 = 0x1e8;
/// The leader's terrainId, which the field gives the leader it creates.
const GS_TURN: u32 = 0x1ec;
const GS_SELECTED_ACTOR: u32 = 0x1f4;
const GS_ACTIVE_OWNERS: u32 = 0x1f8;
const MAP_ORIGIN: u32 = 0x00;
const MAP_VIEW_X: u32 = 0xe4;
const MAP_VIEW_Y: u32 = 0xe8;
/// minX, minY, maxX and maxY, s32 16.16 pixels.
const MAP_BOUNDS: u32 = 0xec;
/// MapState.layers: each layer's window onto the cell grid, 0x30 bytes
/// apart, its cells pointer first.
const MAP_LAYER_CELLS: u32 = 0x130;
const MAP_LAYER_SIZE: u32 = 0x30;
const FIELD_EVENTS: u32 = 0x10;
const FIELD_ACTORS: u32 = 0x14;
const FIELD_ACTOR_COUNT: u32 = 66;
/// The first actor a scene places (ACTOR_FIRST_PLACED); 0 to 7 are the party.
const FIRST_PLACED: u16 = 8;
const FIELD_COMMAND: u32 = 0x17e;
/// struct ObjectRuntime.
const OBJ_ANGLE: u32 = 0x06;
const OBJ_X: u32 = 0x08;
const OBJ_Y: u32 = 0x0c;
const OBJ_Z: u32 = 0x10;
const OBJ_TERRAIN_HEIGHT: u32 = 0x14;
const OBJ_TERRAIN_ID: u32 = 0x22;
const OBJ_VELOCITY: u32 = 0x24;
/// 1: one sprite; 2: a group; 0: no sprite, which the camera skips.
const OBJ_KIND: u32 = 0x54;
const OVERLAY_ENTRANCES: u32 = 0x0c;
const ENTRANCE_SIZE: u32 = 24;
/// The map cells' grid: 128 by 128 words, the collision code in byte 2.
const GRID: i32 = 128;
/// Collision codes (enum MapCellCode): the scene's own events, the cells the
/// leader pushes against, and the wall.
const CELL_EVENT_LAST: i32 = 239;
const CELL_LEDGE: i32 = 252;
const CELL_WALL: i32 = 255;
/// Map_CheckObjectStep's limits: ground up to 8 pixels higher or 12 lower
/// is a step the leader can take.
const STEP_UP: i32 = 8 << 16;
const STEP_DOWN: i32 = 12 << 16;
/// struct SceneEvent: its control word (kind in the low four bits, a facing
/// in bits 15..12 with EVENT_TOUCH_FACING), its target and its result.
const EVENT_ROW_SIZE: u32 = 12;
const EVENT_TOUCH_WALKING: u32 = 0x200;
const EVENT_TOUCH_FACING: u32 = 0x400;
/// The SceneEventKind of each dispatch.
pub const EVENT_TALK: u8 = 0;
pub const EVENT_EXIT: u8 = 1;
pub const EVENT_TOUCH: u8 = 2;
pub const EVENT_SEARCH: u8 = 3;
pub const EVENT_PSYNERGY: u8 = 5;
/// The scene number of a battle (SCENE_BATTLE).
const SCENE_BATTLE: u16 = 0x1fe;

/// Frames from the player's control to the entrance photo.
pub const SETTLE: u32 = 30;
/// Frames the camera gets to follow the leader moved in the middle of a
/// scene.
const SETTLE_MOVED: u32 = 90;
/// Frames each map view runs after the camera moves: the BG maps, then the
/// scroll and sprites reach the screen.
const HOLD: u32 = 3;
/// A run past this frame has gone wrong.
const MAX_FRAMES: u32 = 30000;
/// Frames of the leader standing ready that open an event's recording.
const LEAD_IN: u32 = 12;
/// Frames the field loop must step, with nothing to read, before the
/// player has control again.
const CONTROL_FRAMES: u32 = 30;
/// Frames a trigger may take to start its event: the leader walking onto a
/// cell and pushing against what is beyond it.
const START_FRAMES: u32 = 180;

fn fx(px: i32) -> i32 {
    px << 16
}

/// The addresses the run stops at, from the edition's map.
pub struct Symbols {
    init: u32,
    glow: u32,
    load_layered: u32,
    apply_entrance: u32,
    notice: u32,
    step: u32,
    update_layer: u32,
    center: u32,
    dismiss: u32,
    objects: u32,
    typing: u32,
    choice: u32,
    lookup: u32,
    terrain_height: u32,
    talk: u32,
    touch: u32,
    search: u32,
    psynergy: u32,
    game_state: u32,
    flag_bytes: u32,
    map_work: u32,
    field_work: u32,
    overlay: u32,
}

impl Symbols {
    pub fn find(map: &Map) -> Result<Symbols, String> {
        let at = |name: &str| map.symbol(name).map(|(_, a)| a & !1).ok_or_else(|| format!("the map has no {name}"));
        Ok(Symbols {
            init: at("GameState_InitDefaults")?,
            glow: at("PaletteGlow_Update")?,
            load_layered: at("Map_LoadLayeredScene")?,
            apply_entrance: at("Scene_ApplyEntrance")?,
            notice: at("UiTimedNotice_Create")?,
            step: at("Field_ProcessStep")?,
            update_layer: at("Map_UpdateLayerScroll")?,
            center: at("Map_CenterOnFollowedPoint")?,
            dismiss: at("UiText_CheckTextDismissed")?,
            objects: at("Object_UpdateAllThumb")?,
            typing: at("UiWork_StepChannelScript")?,
            choice: at("Menu_RunConfirmSelection")?,
            lookup: at("UiText_LookupMessage")?,
            terrain_height: at("Map_GetTerrainHeight")?,
            talk: at("Scene_TalkToActor")?,
            touch: at("Scene_RunTouchOrExit")?,
            search: at("Field_SearchTile")?,
            psynergy: at("Field_UsePsynergy")?,
            game_state: at("gGameState")?,
            flag_bytes: at("gGameFlagBytes")?,
            map_work: at("gMapWork")?,
            field_work: at("gEventWork")?,
            overlay: at("gOverlayArea")?,
        })
    }
}

/// Where the run enters: a scene, its entrance, and the story flags to set
/// and clear.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Setup {
    pub scene: u16,
    pub entrance: u16,
    pub set: Vec<u16>,
    pub clear: Vec<u16>,
}

/// A row of the scene's entrance table (struct SceneEntrance).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entrance {
    pub number: i16,
    pub required_flag: i16,
    pub x: i16,
    pub z: i16,
    /// cameraLeft, cameraTop, cameraRight and cameraBottom; -1 for none.
    pub camera: [i16; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Phase {
    Boot,
    Defaults,
    Loading,
    Entered,
}

/// The run's state, saved and loaded with the machine's.
#[derive(Debug, Clone, Copy)]
struct Track {
    phase: Phase,
    frame: u32,
    init_return: u32,
    load_return: u32,
    apply_return: u32,
    entrances_fn: u32,
    entrances_return: u32,
    entrances: u32,
    /// The map header's scroll bounds, 16.16 pixels.
    header: [i32; 4],
    applied: bool,
    steps: u32,
    arm_center: bool,
    pin_focus: bool,
    freeze: bool,
    focus: [i32; 3],
    last_dismiss: u32,
    last_typing: u32,
    choice_return: u32,
    choice_frame: u32,
    last_message: u32,
    keys: u32,
    /// A function to call at the next Field_ProcessStep entry (0 for none),
    /// with x and z written to the followed point first; and r0 to r3 and
    /// lr kept while it runs.
    call_target: u32,
    call_origin: [i32; 2],
    calling: bool,
    call_saved: [u32; 5],
    /// The instruction the CPU runs next.
    pc: u32,
    /// The last frame the field loop stepped, and the first of the frames
    /// it has stepped in a row up to it.
    step_frame: u32,
    stepping_since: u32,
    /// The last event the field loop dispatched: its kind (EVENT_TALK and
    /// on) and target (an actor or a collision code), and the frame.
    dispatched: Option<(u8, u32, u32)>,
    /// Where that dispatch returns to while it runs (0 once it has), and
    /// its result: 0 when an event ran, -1 when there was none.
    dispatch_return: u32,
    dispatch_result: Option<u32>,
    /// The frame that dispatch returned.
    dispatch_end: u32,
    /// How many times a layered map has started loading.
    loads: u32,
}

/// A saved moment: the machine and the run's state.
pub struct Saved {
    state: Vec<u8>,
    track: Track,
}

/// An image, 0xRRGGBB.
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

/// One page of an event.
pub struct Page {
    pub image: Image,
    /// The message last looked up before the page waited, when the event
    /// lists it.
    pub message: Option<u32>,
}

pub struct Field<'rom> {
    gba: Gba<'rom>,
    s: &'rom Symbols,
    t: Track,
    setup: Setup,
    /// Frames kept while an event is recorded.
    recorded: Option<Vec<Vec<u16>>>,
}

impl<'rom> Field<'rom> {
    /// Boots the ROM and runs until Scene_ApplyEntrance has placed the party.
    pub fn enter(rom: &'rom [u8], s: &'rom Symbols, setup: &Setup) -> Result<Field<'rom>, String> {
        let t = Track {
            phase: Phase::Boot,
            frame: 0,
            init_return: 0,
            load_return: 0,
            apply_return: 0,
            entrances_fn: 0,
            entrances_return: 0,
            entrances: 0,
            header: [0; 4],
            applied: false,
            steps: 0,
            arm_center: false,
            pin_focus: false,
            freeze: false,
            focus: [0; 3],
            last_dismiss: 0,
            last_typing: 0,
            choice_return: 0,
            choice_frame: 0,
            last_message: 0,
            keys: 0,
            call_target: 0,
            call_origin: [0; 2],
            calling: false,
            call_saved: [0; 5],
            pc: 0,
            step_frame: 0,
            stepping_since: 0,
            dispatched: None,
            dispatch_return: 0,
            dispatch_result: None,
            dispatch_end: 0,
            loads: 0,
        };
        let mut field = Field { gba: Gba::new(rom)?, s, t, setup: setup.clone(), recorded: None };
        while !field.t.applied {
            field.tick()?;
        }
        Ok(field)
    }

    pub fn set_flag(&mut self, flag: u16, on: bool) {
        let a = self.s.flag_bytes + u32::from((flag >> 3) & 0x1ff);
        let m = 1u8 << (flag & 7);
        let v = self.gba.read8(a);
        self.gba.write8(a, if on { v | m } else { v & !m });
    }

    /// Makes `party` the active party, in order: party flags 0 to 7 and
    /// gGameState.activeOwners.
    pub fn set_party(&mut self, party: &[u8]) {
        let bits = party.iter().fold(0u8, |b, &o| b | 1 << o);
        self.gba.write8(self.s.flag_bytes, bits);
        for (i, &o) in party.iter().enumerate() {
            self.gba.write8(self.s.game_state + GS_ACTIVE_OWNERS + i as u32, o);
        }
    }

    pub fn read8(&self, address: u32) -> u8 {
        self.gba.read8(address)
    }

    pub fn read16(&self, address: u32) -> u16 {
        self.gba.read16(address)
    }


    /// Draws every line from now on over `colour` (BGR555) where nothing else
    /// is drawn; the game's palette keeps its own colour 0.
    pub fn set_backdrop(&mut self, colour: u16) {
        self.gba.set_backdrop(&[colour; HEIGHT]);
    }

    /// Stores a halfword into an I/O register, as the processor would.
    pub fn io_write16(&mut self, address: u32, value: u16) {
        self.gba.io_write16(address, value);
    }

    /// Calls the game function at `target` from the next Field_ProcessStep
    /// entry, with the followed point at (x, z) (16.16 pixels), and runs
    /// until it has returned.
    pub fn call(&mut self, target: u32, origin: [i32; 2]) -> Result<(), String> {
        self.t.call_target = target & !1;
        self.t.call_origin = origin;
        while self.t.call_target != 0 || self.t.calling {
            self.tick()?;
        }
        Ok(())
    }

    /// Moves the party's position in the game state (pixels).
    pub fn set_position(&mut self, x: i32, z: i32) {
        self.gba.write32(self.s.game_state + GS_X, fx(x) as u32);
        self.gba.write32(self.s.game_state + GS_Z, fx(z) as u32);
    }

    /// Stops or starts the actors' task (Object_UpdateAllThumb).
    pub fn freeze(&mut self, on: bool) {
        self.t.freeze = on;
    }

    pub fn map_work(&self) -> u32 {
        self.gba.read32(self.s.map_work)
    }

    /// The scene the game state names now.
    pub fn scene(&self) -> u16 {
        self.gba.read16(self.s.game_state + GS_SCENE)
    }

    /// Whether a message page waits for the button or a choice is open.
    pub fn waiting(&self) -> bool {
        (self.t.last_dismiss > 0 && self.t.last_dismiss + 2 >= self.t.frame) || self.t.choice_return != 0
    }

    /// The message last looked up.
    pub fn last_message(&self) -> u32 {
        self.t.last_message
    }

    /// The last picture, BGR555.
    pub fn picture(&self) -> Vec<u16> {
        self.gba.picture()
    }

    fn apply_flags(&mut self) {
        for f in self.setup.set.clone() {
            self.set_flag(f, true);
        }
        for f in self.setup.clear.clone() {
            self.set_flag(f, false);
        }
    }

    /// One instruction and the hooks at the address it reaches; true at a
    /// frame's end.
    fn tick(&mut self) -> Result<bool, String> {
        let step = self.gba.step();
        let frame_done = step.frame != self.t.frame;
        self.t.frame = step.frame;
        let pc = step.pc;
        self.t.pc = pc;
        let s = self.s;
        let lr = |gba: &Gba| gba.register(LR) & !1;

        if self.t.phase == Phase::Boot && pc == s.init && self.t.init_return == 0 {
            self.t.init_return = lr(&self.gba);
        } else if self.t.init_return != 0 && pc == self.t.init_return {
            // Game_ResetForNewGame now stores the title and entrance 2, then
            // calls PaletteGlow_UpdateFar.
            self.t.init_return = 0;
            self.t.phase = Phase::Defaults;
        } else if self.t.phase == Phase::Defaults && pc == s.glow {
            self.gba.write16(s.game_state + GS_SCENE, self.setup.scene);
            self.gba.write16(s.game_state + GS_ENTRANCE, self.setup.entrance);
            self.apply_flags();
            self.t.phase = Phase::Loading;
        }
        if pc == s.load_layered {
            self.t.loads += 1;
        }
        if pc == s.load_layered && self.t.load_return == 0 {
            self.t.load_return = lr(&self.gba);
        } else if self.t.load_return != 0 && pc == self.t.load_return {
            self.t.load_return = 0;
            let work = self.gba.read32(s.map_work);
            for i in 0..4 {
                self.t.header[i] = self.gba.read32(work + MAP_BOUNDS + 4 * i as u32) as i32;
            }
        }
        if self.t.phase == Phase::Loading && pc == s.apply_entrance && self.t.apply_return == 0 {
            self.t.apply_return = lr(&self.gba);
            self.t.entrances_fn = self.gba.read32(s.overlay + OVERLAY_ENTRANCES) & !1;
        } else if self.t.entrances_fn != 0 && pc == self.t.entrances_fn && self.t.entrances_return == 0 {
            self.t.entrances_return = lr(&self.gba);
        } else if self.t.entrances_return != 0 && pc == self.t.entrances_return {
            self.t.entrances = self.gba.register(0);
            self.t.entrances_return = 0;
            self.t.entrances_fn = 0;
        } else if self.t.apply_return != 0 && pc == self.t.apply_return {
            // Scene_ApplyEntrance has placed the party; the scene's
            // initialization, which reads the story flags, comes next. Scene
            // loading may have cleared some of them, so they are set again.
            self.t.apply_return = 0;
            self.apply_flags();
            self.t.applied = true;
            self.t.phase = Phase::Entered;
        }
        if pc == s.notice {
            self.gba.return_now(0);
        }
        if pc == s.step && self.t.calling {
            for (i, &v) in self.t.call_saved.iter().enumerate().take(4) {
                self.gba.set_register(i as u32, v);
            }
            self.gba.set_register(LR, self.t.call_saved[4]);
            self.t.calling = false;
        } else if pc == s.step && self.t.call_target != 0 {
            for i in 0..4 {
                self.t.call_saved[i] = self.gba.register(i as u32);
            }
            self.t.call_saved[4] = self.gba.register(LR);
            let origin = self.gba.read32(self.gba.read32(s.map_work));
            if origin != 0 {
                self.gba.write32(origin, self.t.call_origin[0] as u32);
                self.gba.write32(origin + 8, self.t.call_origin[1] as u32);
            }
            self.gba.set_register(LR, s.step | 1);
            let target = self.t.call_target;
            self.t.call_target = 0;
            self.t.calling = true;
            self.gba.branch(target);
            return Ok(frame_done);
        }
        if pc == s.step {
            self.t.steps += 1;
            if self.t.step_frame + 1 < self.t.frame {
                self.t.stepping_since = self.t.frame;
            }
            self.t.step_frame = self.t.frame;
        }
        let dispatch = [(s.talk, EVENT_TALK), (s.touch, EVENT_TOUCH), (s.search, EVENT_SEARCH), (s.psynergy, EVENT_PSYNERGY)];
        if let Some(&(_, kind)) = dispatch.iter().find(|(a, _)| *a == pc) {
            self.t.dispatched = Some((kind, self.gba.register(0), self.t.frame));
            self.t.dispatch_return = lr(&self.gba);
            self.t.dispatch_result = None;
        } else if self.t.dispatch_return != 0 && pc == self.t.dispatch_return {
            self.t.dispatch_return = 0;
            self.t.dispatch_result = Some(self.gba.register(0));
            self.t.dispatch_end = self.t.frame;
        }
        if self.t.pin_focus && (pc == s.update_layer || pc == s.center) {
            let work = self.gba.read32(s.map_work);
            self.gba.write32(work + MAP_ORIGIN, s.game_state + GS_X);
            for i in 0..3 {
                self.gba.write32(s.game_state + GS_X + 4 * i as u32, self.t.focus[i] as u32);
            }
        }
        if self.t.arm_center && pc == s.update_layer {
            self.t.arm_center = false;
            self.gba.branch(s.center);
        }
        if pc == s.choice && self.t.choice_return == 0 {
            self.t.choice_return = lr(&self.gba);
            self.t.choice_frame = self.t.frame;
        } else if self.t.choice_return != 0 && pc == self.t.choice_return {
            self.t.choice_return = 0;
        }
        if pc == s.typing {
            self.t.last_typing = self.t.frame;
        }
        if pc == s.lookup {
            self.t.last_message = self.gba.register(1);
        }
        if self.t.freeze && pc == s.objects {
            self.gba.return_now(0);
        }
        if pc == s.dismiss {
            self.t.last_dismiss = self.t.frame;
        }
        if frame_done && self.recorded.is_some() {
            let picture = self.gba.picture();
            if let Some(frames) = self.recorded.as_mut() {
                frames.push(picture);
            }
        }
        if frame_done {
            self.gba.set_keys(self.t.keys);
            if self.t.frame > MAX_FRAMES {
                let scene = self.gba.read16(s.game_state + GS_SCENE);
                return Err(format!("gave up at frame {} in scene {scene}", self.t.frame));
            }
        }
        Ok(frame_done)
    }

    pub fn frames(&mut self, mut n: u32) -> Result<(), String> {
        while n > 0 {
            if self.tick()? {
                n -= 1;
            }
        }
        Ok(())
    }

    /// Holds `keys` (KEYINPUT bits) from the next frame until changed.
    pub fn hold(&mut self, keys: u32) {
        self.t.keys = keys;
    }

    pub fn press(&mut self, frames: u32) -> Result<(), String> {
        self.t.keys = KEY_A;
        self.frames(frames)?;
        self.t.keys = 0;
        Ok(())
    }

    /// Runs until the player has control, pressing A through any page that
    /// waits on the way (an arrival's lines).
    pub fn until_control(&mut self) -> Result<(), String> {
        let (mut waited, mut idle) = (0, 0);
        while self.t.steps == 0 {
            if self.tick()? {
                let waiting = self.t.last_dismiss + 2 >= self.t.frame && self.t.last_dismiss > 0;
                waited = if waiting || self.t.choice_return != 0 { waited + 1 } else { 0 };
                idle += 1;
                // A page or a choice waiting 10 frames, or 120 frames of
                // nothing to read (an arrival that waits for the button).
                if waited >= 10 || idle >= 120 {
                    self.press(3)?;
                    waited = 0;
                    idle = 0;
                }
            }
        }
        let scene = self.gba.read16(self.s.game_state + GS_SCENE);
        if scene != self.setup.scene {
            return Err(format!("the scene went on to scene {scene} before the player had control"));
        }
        Ok(())
    }

    pub fn save(&mut self) -> Saved {
        Saved { state: self.gba.save(), track: self.t }
    }

    pub fn load(&mut self, saved: &Saved) {
        self.gba.load(&saved.state);
        self.t = saved.track;
        self.gba.set_keys(0);
    }

    pub fn screen(&self) -> Image {
        Image { width: WIDTH, height: HEIGHT, pixels: self.gba.picture_rgb() }
    }

    /// The live scroll bounds, 16.16 pixels.
    pub fn bounds(&self) -> [i32; 4] {
        let work = self.map_work();
        [0, 1, 2, 3].map(|i| self.gba.read32(work + MAP_BOUNDS + 4 * i) as i32)
    }

    /// The map header's scroll bounds, 16.16 pixels.
    pub fn header(&self) -> [i32; 4] {
        self.t.header
    }

    /// The scene's entrance table, up to its end (entrance -1).
    pub fn entrances(&self) -> Vec<Entrance> {
        let mut out = Vec::new();
        let mut a = self.t.entrances;
        if a == 0 {
            return out;
        }
        for _ in 0..256 {
            let h = |o: u32| self.gba.read16(a + o) as i16;
            if h(0) == -1 {
                break;
            }
            out.push(Entrance { number: h(0), required_flag: h(2), x: h(4), z: h(8), camera: [h(14), h(16), h(18), h(20)] });
            a += ENTRANCE_SIZE;
        }
        out
    }

    fn actor_object(&self, id: u16) -> u32 {
        let work = self.gba.read32(self.s.field_work);
        if u32::from(id) >= FIELD_ACTOR_COUNT || work == 0 {
            return 0;
        }
        self.gba.read32(work + FIELD_ACTORS + 4 * u32::from(id))
    }

    /// Actor `id`'s object as it is now, when the scene has one.
    pub fn actor(&self, id: u16) -> Option<Actor> {
        let a = self.actor_object(id);
        (a != 0).then(|| {
            let word = |o: u32| self.gba.read32(a + o) as i32;
            Actor {
                x: word(OBJ_X) >> 16,
                z: word(OBJ_Z) >> 16,
                y: word(OBJ_Y),
                layer: self.gba.read8(a + OBJ_TERRAIN_ID),
                drawn: self.gba.read8(a + OBJ_KIND) & 15 != 0,
            }
        })
    }

    /// Actor `id` when it is on stage: placed away from the map's origin,
    /// where scenes keep the actors they do not show, within the map's
    /// bounds, and drawn; else why not.
    pub fn present(&self, id: u16) -> Result<Actor, String> {
        let Some(actor) = self.actor(id) else {
            return Err(format!("the scene has no actor {id}"));
        };
        let h = self.t.header;
        if actor.x == 0 && actor.z == 0 {
            return Err(format!("actor {id} is off stage, at the map's origin"));
        }
        if actor.x < h[0] >> 16 || actor.x >= h[2] >> 16 || actor.z < h[1] >> 16 || actor.z >= h[3] >> 16 {
            return Err(format!("actor {id} is off stage, at ({}, {}) outside the map", actor.x, actor.z));
        }
        if !actor.drawn {
            return Err(format!("actor {id} has no sprite"));
        }
        Ok(actor)
    }

    /// The collision code of cell (cx, cz) of map layer `layer` (its window
    /// onto the cell grid), or -1 off the grid.
    pub fn cell(&self, layer: u8, cx: i32, cz: i32) -> i32 {
        if !(0..GRID).contains(&cx) || !(0..GRID).contains(&cz) || layer > 2 {
            return -1;
        }
        let cells = self.gba.read32(self.map_work() + MAP_LAYER_CELLS + MAP_LAYER_SIZE * u32::from(layer));
        i32::from(self.gba.read8(cells + (((cx + (cz << 7)) << 2) + 2) as u32))
    }

    /// The cells (within the header's bounds, in rows) holding collision
    /// code `code` in any of the three layers, with their layer.
    pub fn cells_with_code(&self, code: u16) -> Vec<(u8, i32, i32)> {
        let h = self.t.header;
        let (x0, z0, x1, z1) = (h[0] >> 20, h[1] >> 20, (h[2] + 0xfffff) >> 20, (h[3] + 0xfffff) >> 20);
        let mut out = Vec::new();
        for layer in 0..3u8 {
            for cz in z0..z1 {
                for cx in x0..x1 {
                    if self.cell(layer, cx, cz) == i32::from(code) {
                        out.push((layer, cx, cz));
                    }
                }
            }
        }
        out
    }

    /// The first cell holding collision code `code`, as its centre in
    /// pixels.
    pub fn cell_with_code(&self, code: u16) -> Option<(i32, i32)> {
        self.cells_with_code(code).first().map(|&(_, cx, cz)| (cx * 16 + 8, cz * 16 + 8))
    }

    /// The scene's event rows for `kind` and `target`, in table order.
    fn event_rows(&self, kind: u8, target: u16) -> Vec<EventRow> {
        let work = self.gba.read32(self.s.field_work);
        let mut a = if work == 0 { 0 } else { self.gba.read32(work + FIELD_EVENTS) };
        let mut out = Vec::new();
        for _ in 0..1024 {
            if a == 0 {
                break;
            }
            let flags = self.gba.read32(a);
            if flags == u32::MAX {
                break;
            }
            if flags & 15 == u32::from(kind) && u16::from(self.gba.read8(a + 4)) == target {
                out.push(EventRow { flags, condition: self.gba.read16(a + 6) as i16, result: self.gba.read32(a + 8) });
            }
            a += EVENT_ROW_SIZE;
        }
        out
    }

    /// Whether an event condition holds (GameFlag_IsConditionActive): -1
    /// always, one with bit 0x1000 while its flag is set, any other while
    /// its flag is clear.
    fn condition_holds(&self, condition: i16) -> bool {
        if condition == -1 {
            return true;
        }
        let flag = condition as u16 & 0xfff;
        let set = self.gba.read8(self.s.flag_bytes + u32::from(flag >> 3)) & (1 << (flag & 7)) != 0;
        if condition as u16 & 0x1000 != 0 { set } else { !set }
    }

    /// Makes the row showing one of `messages` the one the trigger finds:
    /// each row before it for the same trigger whose condition holds is
    /// turned off by its flag, as it is once its own event has been seen
    /// (Garet's family's scene before the line Mind Read hears after it).
    fn unshadow(&mut self, kind: u8, target: u16, messages: &[u32]) {
        let rows = self.event_rows(kind, target);
        let Some(ours) = rows.iter().position(|r| r.result < 0x10000 && messages.contains(&r.result)) else {
            return;
        };
        for row in &rows[..ours] {
            if row.condition != -1 && self.condition_holds(row.condition) {
                self.set_flag(row.condition as u16 & 0xfff, row.condition as u16 & 0x1000 == 0);
            }
        }
    }

    /// Runs the field loop to its next step (Field_ProcessStep's entry),
    /// where a game function can be called between two of its calls.
    fn to_step(&mut self) -> Result<(), String> {
        for _ in 0..4_000_000 {
            if self.t.pc == self.s.step && !self.t.calling && self.t.call_target == 0 {
                return Ok(());
            }
            self.tick()?;
        }
        Err("the field loop did not step".to_string())
    }

    /// Calls the Thumb function at `target` with `args` at the field loop's
    /// step and returns its r0; the loop goes on as if nothing ran. The
    /// function must only read (Map_GetTerrainHeight).
    fn eval(&mut self, target: u32, args: [u32; 4]) -> Result<u32, String> {
        self.to_step()?;
        let saved: Vec<u32> = [0, 1, 2, 3, LR, SP].iter().map(|&r| self.gba.register(r)).collect();
        for (i, &v) in args.iter().enumerate() {
            self.gba.set_register(i as u32, v);
        }
        self.gba.set_register(LR, self.s.step | 1);
        self.gba.branch(target & !1);
        let mut returned = false;
        for _ in 0..200_000 {
            if self.gba.step().pc == self.s.step {
                returned = true;
                break;
            }
        }
        let r0 = self.gba.register(0);
        for (&r, &v) in [0, 1, 2, 3, LR, SP].iter().zip(&saved) {
            self.gba.set_register(r, v);
        }
        if !returned {
            return Err(format!("the call to {target:#x} did not return"));
        }
        Ok(r0)
    }

    /// The ground's height (16.16 pixels) at (x, z) pixels on map layer
    /// `layer`, as Map_GetTerrainHeight gives it.
    pub fn ground(&mut self, layer: u8, x: i32, z: i32) -> Result<i32, String> {
        Ok(self.eval(self.s.terrain_height, [u32::from(layer), fx(x) as u32, fx(z) as u32, 0])? as i32)
    }

    /// The leader's object.
    fn leader_object(&self) -> u32 {
        let selected = self.gba.read32(self.s.game_state + GS_SELECTED_ACTOR);
        self.actor_object(selected as u16)
    }

    /// Puts the leader at `spot`: on its layer, standing on its ground and
    /// facing its way.
    fn put_leader(&mut self, spot: &Spot) {
        let a = self.leader_object();
        if a == 0 {
            return;
        }
        self.gba.write32(a + OBJ_X, fx(spot.x) as u32);
        self.gba.write32(a + OBJ_Z, fx(spot.z) as u32);
        self.gba.write32(a + OBJ_Y, spot.height as u32);
        self.gba.write32(a + OBJ_TERRAIN_HEIGHT, spot.height as u32);
        self.gba.write8(a + OBJ_TERRAIN_ID, spot.layer);
        self.gba.write16(a + OBJ_ANGLE, spot.facing as u16);
        for i in 0..3 {
            self.gba.write32(a + OBJ_VELOCITY + 4 * i, 0);
        }
    }

    /// Why the leader is not standing at `spot`, on its ground, if he is not.
    fn check_leader(&mut self, spot: &Spot) -> Result<(), String> {
        let selected = self.gba.read32(self.s.game_state + GS_SELECTED_ACTOR) as u16;
        let Some(leader) = self.actor(selected) else {
            return Err("the scene has no leader".to_string());
        };
        if (leader.x - spot.x).abs() > 2 || (leader.z - spot.z).abs() > 2 {
            return Err(format!("the leader was moved from ({}, {}) to ({}, {})", spot.x, spot.z, leader.x, leader.z));
        }
        if leader.layer != spot.layer {
            return Err(format!("the leader went from layer {} to layer {}", spot.layer, leader.layer));
        }
        let ground = self.ground(leader.layer, leader.x, leader.z)?;
        if (leader.y - ground).abs() > 1 << 16 {
            return Err(format!("the leader stands {} pixels off the ground", (leader.y - ground) >> 16));
        }
        Ok(())
    }

    /// Whether the leader can stand on a cell with collision code `code`
    /// without starting anything: open ground or encounter ground.
    fn standable(code: i32) -> bool {
        code >= 0 && code != CELL_WALL && !(1..=CELL_EVENT_LAST).contains(&code) && code < CELL_LEDGE
    }

    /// Whether standing on a cell with collision code `code` starts nothing:
    /// open ground, or a code of the scene's events that no touch or exit
    /// row answers.
    fn quiet(&self, code: i32) -> bool {
        Self::standable(code)
            || ((1..=CELL_EVENT_LAST).contains(&code)
                && self.event_rows(EVENT_TOUCH, code as u16).is_empty()
                && self.event_rows(EVENT_EXIT, code as u16).is_empty())
    }

    /// Whether (x, z) pixels lie within the map's bounds, `margin` pixels in.
    fn inside(&self, x: i32, z: i32, margin: i32) -> bool {
        let h = self.t.header;
        x >= (h[0] >> 16) + margin && x < (h[2] >> 16) - margin && z >= (h[1] >> 16) + margin && z < (h[3] >> 16) - margin
    }

    /// The map of `box` (16.16 pixels) from the moment `settled`, the leader
    /// hidden unless `leader`.
    pub fn stitch(&mut self, settled: &Saved, b: [i32; 4], leader: bool) -> Result<Image, String> {
        let (min_x, min_y, max_x, max_y) = (b[0], b[1], b[2], b[3]);
        let (mw, mh) = (((max_x - min_x) >> 16) as usize, ((max_y - min_y) >> 16) as usize);
        let mut canvas = vec![0u32; mw * mh];
        let last_x = (max_x - fx(WIDTH as i32)).max(min_x);
        let last_y = (max_y - fx(HEIGHT as i32)).max(min_y);
        // BG0 holds the windows and gauges (the map's layers are BG1 to BG3),
        // which would show in every view.
        self.gba.show_layer(0, false);
        let mut vy = min_y;
        loop {
            vy = vy.min(last_y);
            let mut vx = min_x;
            loop {
                vx = vx.min(last_x);
                self.load(settled);
                let work = self.map_work();
                // The followed point is the view's top left plus the focus
                // (120, 96).
                self.t.focus = [vx + fx(120), 0, vy + fx(96)];
                self.t.pin_focus = true;
                self.t.freeze = true;
                for (i, &v) in b.iter().enumerate() {
                    self.gba.write32(work + MAP_BOUNDS + 4 * i as u32, v as u32);
                }
                if !leader {
                    let selected = self.gba.read32(self.s.game_state + GS_SELECTED_ACTOR);
                    let object = self.actor_object(selected as u16);
                    if object != 0 {
                        self.gba.write8(object + OBJ_KIND, 0);
                    }
                }
                self.t.arm_center = true;
                self.frames(HOLD)?;
                if self.t.arm_center {
                    return Err("Map_UpdateLayerScroll never ran".to_string());
                }
                let view_x = self.gba.read32(work + MAP_VIEW_X) as i32;
                let view_y = self.gba.read32(work + MAP_VIEW_Y) as i32;
                let screen = self.gba.picture_rgb();
                let (ox, oy) = ((view_x - min_x) >> 16, (view_y - min_y) >> 16);
                for y in 0..HEIGHT as i32 {
                    let my = oy + y;
                    if my < 0 || my >= mh as i32 {
                        continue;
                    }
                    for x in 0..WIDTH as i32 {
                        let mx = ox + x;
                        if mx >= 0 && mx < mw as i32 {
                            canvas[my as usize * mw + mx as usize] = screen[(y * WIDTH as i32 + x) as usize];
                        }
                    }
                }
                if vx == last_x {
                    break;
                }
                vx += fx(WIDTH as i32);
            }
            if vy == last_y {
                break;
            }
            vy += fx(HEIGHT as i32);
        }
        self.gba.show_layer(0, true);
        self.t.pin_focus = false;
        self.t.freeze = false;
        Ok(Image { width: mw, height: mh, pixels: canvas })
    }

    /// Where the leader can stand to start `trigger`, best first, or why
    /// there is nowhere: beside the actor (20 pixels south, west, east or
    /// north of it, then 34, across a counter) on its layer and level with
    /// it, facing it; a cell from a search cell, facing it; a cell from a
    /// touch cell, to walk onto it, the way its row faces first. Every spot
    /// is open ground within the map, a step the leader could take from the
    /// target's ground, with the ground's height there.
    pub fn spots(&mut self, trigger: Trigger) -> Result<Vec<Spot>, String> {
        // North, east, south and west.
        const FACING: [u32; 4] = [0xc000, 0x0000, 0x4000, 0x8000];
        const STEP: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
        const KEYS: [u32; 4] = [KEY_UP, KEY_RIGHT, KEY_DOWN, KEY_LEFT];
        const SIDES: [usize; 4] = [2, 3, 1, 0];
        let step = |rise: i32| (-STEP_DOWN..=STEP_UP).contains(&rise);
        let mut out = Vec::new();
        match trigger {
            Trigger::Psynergy(ability, id) => {
                // Field_FindNearbyPsynergyTarget: the actor within the
                // Psynergy's range and height of the leader, ahead of him
                // with his facing rounded to a quarter turn.
                let (range, height) = psynergy_reach(ability);
                let actor = self.present(id)?;
                let mut later = Vec::new();
                let distances: Vec<i32> = [20, range - 4, range / 2 + 4].into_iter().filter(|&d| d > 8 && d < range).collect();
                for &distance in &distances {
                    for side in SIDES {
                        let (x, z) = (actor.x + STEP[side].0 * distance, actor.z + STEP[side].1 * distance);
                        let code = self.cell(actor.layer, x >> 4, z >> 4);
                        if !self.inside(x, z, 8) || code < 0 || code == CELL_WALL || code >= CELL_LEDGE {
                            continue;
                        }
                        let ground = self.ground(actor.layer, x, z)?;
                        if (actor.y - ground).abs() > height {
                            continue;
                        }
                        let spot = Spot { x, z, layer: actor.layer, height: ground, facing: FACING[(side + 2) & 3], walk: 0, cell: None };
                        if self.quiet(code) { &mut out } else { &mut later }.push(spot);
                    }
                }
                out.extend(later);
                if out.is_empty() {
                    return Err(format!("no ground within reach of actor {id} at ({}, {})", actor.x, actor.z));
                }
            }
            Trigger::Talk(id) => {
                // Field_FindNearbyTalkTarget reaches 32 pixels, 41 for an
                // actor with collision flag 4; the leader faces the actor.
                // Open ground comes first; ground with a code of the scene's
                // events, which would start its own event first, last.
                let actor = self.present(id)?;
                let mut later = Vec::new();
                for distance in [20, 28, 34] {
                    for (dx, dz) in AROUND {
                        let (x, z) = if dx != 0 && dz != 0 {
                            (actor.x + dx * distance * 707 / 1000, actor.z + dz * distance * 707 / 1000)
                        } else {
                            (actor.x + dx * distance, actor.z + dz * distance)
                        };
                        let code = self.cell(actor.layer, x >> 4, z >> 4);
                        if !self.inside(x, z, 8) || code < 0 || code == CELL_WALL || code >= CELL_LEDGE {
                            continue;
                        }
                        let ground = self.ground(actor.layer, x, z)?;
                        if !step(actor.y - ground) {
                            continue;
                        }
                        let facing = facing_toward(actor.x - x, actor.z - z);
                        let spot = Spot { x, z, layer: actor.layer, height: ground, facing, walk: 0, cell: None };
                        if self.quiet(code) { &mut out } else { &mut later }.push(spot);
                    }
                }
                out.extend(later);
                if out.is_empty() {
                    return Err(format!("no ground beside actor {id} at ({}, {}) is level with it", actor.x, actor.z));
                }
            }
            Trigger::Search(code) => {
                // The leader searches the ground layer's cell 16 pixels ahead
                // (Object_GetTriggerTileAheadOfCurrent); a jar or the like
                // counts up to 64 pixels above his ground.
                let cells: Vec<_> = self.cells_with_code(code).into_iter().filter(|c| c.0 == 0).take(8).collect();
                if cells.is_empty() {
                    return Err(format!("no ground-layer cell holds code {code}"));
                }
                for (layer, cx, cz) in cells {
                    let target = self.ground(layer, cx * 16 + 8, cz * 16 + 8)?;
                    for side in SIDES {
                        let (nx, nz) = (cx + STEP[side].0, cz + STEP[side].1);
                        let (x, z) = (nx * 16 + 8, nz * 16 + 8);
                        if !self.inside(x, z, 0) || !self.quiet(self.cell(layer, nx, nz)) {
                            continue;
                        }
                        let ground = self.ground(layer, x, z)?;
                        if (-STEP_DOWN..=64 << 16).contains(&(target - ground)) {
                            let facing = FACING[(side + 2) & 3];
                            out.push(Spot { x, z, layer, height: ground, facing, walk: 0, cell: Some((cx, cz)) });
                        }
                    }
                }
                if out.is_empty() {
                    return Err(format!("no open ground beside a cell with code {code}"));
                }
            }
            Trigger::Touch(code) => {
                // A row that needs pushing (EVENT_TOUCH_WALKING) is pushed
                // from the cell itself, the way the row faces when it names
                // one (EVENT_TOUCH_FACING); any other is walked onto, that
                // way first.
                let rows = self.event_rows(EVENT_TOUCH, code);
                let pushing = rows.iter().any(|r| r.flags & EVENT_TOUCH_WALKING != 0);
                let facing = rows.iter().find(|r| r.flags & EVENT_TOUCH_FACING != 0).map(|r| r.flags & 0xf000);
                let mut ways = vec![0, 2, 3, 1];
                if let Some(f) = facing {
                    ways.sort_by_key(|&d| (FACING[d] as u16).wrapping_sub(f as u16).wrapping_add(0x1fff) > 0x3ffe);
                }
                let cells: Vec<_> = self.cells_with_code(code).into_iter().take(32).collect();
                if cells.is_empty() {
                    return Err(format!("no cell holds code {code}"));
                }
                // Pushes go toward what lies in the next cell first: an
                // actor (a statue, a crate), then a wall.
                let held: Vec<(i32, i32)> =
                    (FIRST_PLACED..FIELD_ACTOR_COUNT as u16).filter_map(|id| self.present(id).ok()).map(|a| (a.x >> 4, a.z >> 4)).collect();
                let mut pushed: Vec<(u8, Spot)> = Vec::new();
                let mut walked = Vec::new();
                for (layer, cx, cz) in cells {
                    let (tx, tz) = (cx * 16 + 8, cz * 16 + 8);
                    let target = self.ground(layer, tx, tz)?;
                    if pushing && self.inside(tx, tz, 0) {
                        let pushes = if facing.is_some() { &ways[..1] } else { &ways[..] };
                        for &d in pushes {
                            let next = (cx + STEP[d].0, cz + STEP[d].1);
                            let rank = if held.contains(&next) {
                                0
                            } else if self.cell(layer, next.0, next.1) == CELL_WALL {
                                1
                            } else {
                                2
                            };
                            pushed.push((rank, Spot { x: tx, z: tz, layer, height: target, facing: FACING[d], walk: KEYS[d], cell: Some((cx, cz)) }));
                        }
                    }
                    for &d in &ways {
                        let (nx, nz) = (cx - STEP[d].0, cz - STEP[d].1);
                        let (x, z) = (nx * 16 + 8, nz * 16 + 8);
                        if !self.inside(x, z, 0) || !self.quiet(self.cell(layer, nx, nz)) {
                            continue;
                        }
                        let ground = self.ground(layer, x, z)?;
                        if step(target - ground) {
                            walked.push(Spot { x, z, layer, height: ground, facing: FACING[d], walk: KEYS[d], cell: Some((cx, cz)) });
                        }
                    }
                }
                pushed.sort_by_key(|(rank, _)| *rank);
                out.extend(pushed.into_iter().map(|(_, spot)| spot));
                out.extend(walked);
                if out.is_empty() {
                    return Err(format!("no open ground a step from a cell with code {code}"));
                }
            }
        }
        Ok(out)
    }

    /// Plays an event from `start` with the leader at `spot`, until the
    /// player has control again. Keeps up to `max_pages` pages as stills;
    /// with `record`, also every frame from just before the trigger, up to
    /// `record` frames, holding each page PAGE_HOLD frames so a GIF shows it.
    /// Fails when the leader does not stay at the spot or another event
    /// begins.
    pub fn event(
        &mut self,
        start: Start,
        trigger: Trigger,
        spot: &Spot,
        messages: &[u32],
        max_pages: usize,
        record: Option<usize>,
    ) -> Result<EventRun, String> {
        match start {
            Start::Entered(saved) => {
                self.load(saved);
                let gs = self.s.game_state;
                self.gba.write32(gs + GS_X, fx(spot.x) as u32);
                self.gba.write32(gs + GS_Y, spot.height as u32);
                self.gba.write32(gs + GS_Z, fx(spot.z) as u32);
                self.gba.write32(gs + GS_HEADING, spot.facing);
                self.gba.write16(gs + GS_TURN, u16::from(spot.layer));
                self.until_control()?;
                self.put_leader(spot);
                self.frames(SETTLE)?;
            }
            Start::InControl(saved) => {
                self.load(saved);
                self.put_leader(spot);
                self.frames(SETTLE_MOVED)?;
            }
        }
        self.check_leader(spot)?;
        if record.is_some() {
            self.recorded = Some(Vec::new());
        }
        self.frames(LEAD_IN)?;
        let cap = record.unwrap_or(EVENT_LIMIT as usize);
        self.t.dispatched = None;
        let trigger_frame = self.t.frame;
        let loads = self.t.loads;
        let mut left = false;
        let (kind, target) = match trigger {
            Trigger::Talk(id) => (EVENT_TALK, id),
            Trigger::Search(code) => (EVENT_SEARCH, code),
            Trigger::Touch(code) => (EVENT_TOUCH, code),
            Trigger::Psynergy(_, id) => (EVENT_PSYNERGY, id),
        };
        // The dispatch it starts: Field_UsePsynergy takes the Psynergy, not
        // the actor.
        let want = (kind, (kind != EVENT_PSYNERGY).then_some(u32::from(target)));
        self.unshadow(kind, target, messages);
        match trigger {
            Trigger::Talk(_) | Trigger::Search(_) => self.press(2)?,
            Trigger::Psynergy(ability, _) => {
                let work = self.gba.read32(self.s.field_work);
                self.gba.write16(work + FIELD_COMMAND, ability);
            }
            Trigger::Touch(_) => self.hold(spot.walk),
        }
        let hold = if record.is_some() { PAGE_HOLD } else { 3 };
        let mut began: Option<u32> = None;
        // The frame the dispatch that began the event returned.
        let mut returned: Option<u32> = None;
        // Whether the field loop stopped stepping for a while once the event
        // began (a script, a battle, a scene change).
        let mut paused = false;
        let mut pages = Vec::new();
        let (mut wait_start, mut press_frame, mut quiet_start) = (0, 0, self.t.frame);
        let mut captured = false;
        let mut pressed = false;
        let mut message = None;
        let mut seen_message = 0;
        let mut capped = false;
        loop {
            if record.is_none() && pages.len() >= max_pages {
                break;
            }
            let length = self.t.frame - trigger_frame;
            if self.recorded.as_ref().map_or(length as usize, |f| f.len()) >= cap {
                capped = true;
                break;
            }
            self.frames(1)?;
            // An exit: the event is over once the scene it leads to starts
            // loading its map.
            if self.t.loads > loads && self.scene() != self.setup.scene && self.scene() != SCENE_BATTLE {
                left = true;
                break;
            }
            if self.t.last_message != seen_message {
                seen_message = self.t.last_message;
                if messages.contains(&seen_message) {
                    message = Some(seen_message);
                }
            }
            let Some(b) = began else {
                // Before the event begins, nothing is pressed for it but
                // the trigger. A dispatch that found nothing to run (a touch
                // the leader has not pushed against long enough) returns -1
                // at once.
                if self.t.dispatch_return == 0 && self.t.dispatch_result == Some(u32::MAX) {
                    self.t.dispatched = None;
                    self.t.dispatch_result = None;
                }
                if let Some((kind, value, frame)) = self.t.dispatched {
                    if kind != want.0 || want.1.is_some_and(|v| v != value) {
                        self.hold(0);
                        return Err(format!("another event began ({} {value})", kind_name(kind)));
                    }
                    began = Some(frame);
                    returned = (self.t.dispatch_return == 0).then_some(self.t.dispatch_end);
                    self.hold(0);
                    quiet_start = self.t.frame;
                } else if self.t.frame - trigger_frame > START_FRAMES {
                    self.hold(0);
                    return Err("the event did not start".to_string());
                }
                continue;
            };
            if returned.is_none() && self.t.dispatch_return == 0 {
                returned = Some(self.t.dispatch_end);
            }
            if self.t.step_frame + 2 < self.t.frame {
                paused = true;
            }
            let waiting = self.t.last_dismiss > trigger_frame && self.t.frame - self.t.last_dismiss <= 2;
            if waiting {
                quiet_start = self.t.frame;
                if wait_start == 0 {
                    wait_start = self.t.frame;
                    pressed = false;
                }
                if !captured && self.t.frame - wait_start >= 3 {
                    if pages.len() < max_pages {
                        pages.push(Page { image: self.screen(), message });
                    }
                    captured = true;
                }
                if captured && !pressed && self.t.frame - wait_start >= hold {
                    pressed = true;
                    press_frame = self.t.frame;
                    self.press(3)?;
                } else if pressed && self.t.frame - press_frame >= 30 {
                    press_frame = self.t.frame;
                    self.press(3)?;
                }
                continue;
            }
            // A yes or no choice: once it has shown for 20 frames, keep it
            // and answer with A (the cursor starts on Yes).
            if self.t.choice_return != 0 {
                quiet_start = self.t.frame;
                if self.t.frame - self.t.choice_frame == 20 && pages.len() < max_pages {
                    pages.push(Page { image: self.screen(), message });
                }
                if self.t.frame - self.t.choice_frame == 20.max(hold) {
                    self.press(3)?;
                }
                continue;
            }
            wait_start = 0;
            captured = false;
            // The field loop has stepped every frame since the event, with
            // nothing to read: the player has control again.
            let typing = self.t.frame - self.t.last_typing <= 2;
            let run = self.t.stepping_since.max(b + 1);
            if !typing && self.t.step_frame + 1 >= self.t.frame && self.t.frame >= run + CONTROL_FRAMES {
                break;
            }
            if typing {
                quiet_start = self.t.frame;
            }
            // A battle the event starts is fought by A alone: Fight, then
            // each character's first choice.
            if self.scene() == SCENE_BATTLE {
                if self.t.frame % 20 == 0 {
                    self.press(3)?;
                }
                continue;
            }
            // Nothing typed or waiting for 120 frames (an effect, a walk)
            // while the field loop is stopped: press A.
            if self.t.frame - quiet_start >= 120 && self.t.step_frame + 1 < self.t.frame {
                self.press(3)?;
                quiet_start = self.t.frame;
            }
        }
        self.hold(0);
        let frames = self.recorded.take().unwrap_or_default();
        let end = (!capped && !left && self.scene() == self.setup.scene).then(|| self.save());
        // An event whose dispatch returned in the frame it began, with the
        // field loop stepping on, ran no script: it showed nothing, or
        // nothing it could show from here.
        let instant = began.is_some() && returned == began && !left && !paused;
        Ok(EventRun { pages, frames, capped, end, instant })
    }
}

/// A field Psynergy's reach by its ability id: its range
/// (gBattleFx_TargetRangeByMode, by field kind) and the height between the
/// leader and the actor it allows, in pixels and 16.16 pixels.
fn psynergy_reach(ability: u16) -> (i32, i32) {
    match ability {
        // Mind Read: 32 pixels, 16 high.
        0x8d => (32, 16 << 16),
        // Force: 48.
        0x8e => (48, 8 << 16),
        // Catch: 48, 48 high.
        0x94 => (48, 48 << 16),
        // Douse: 32, 64 high.
        0x21 => (32, 64 << 16),
        // Whirlwind and Halt: 32.
        0x4e | 0x91 => (32, 8 << 16),
        // Frost, Growth, Lift, Carry and Cloak: 16.
        _ => (16, 8 << 16),
    }
}

/// Around an actor: south, west, east and north, then the diagonals.
const AROUND: [(i32, i32); 8] = [(0, 1), (-1, 0), (1, 0), (0, -1), (-1, 1), (1, 1), (-1, -1), (1, -1)];

/// The heading (1/65536 turns, 0 east, 0x4000 south) from a point toward
/// one (dx, dz) pixels away, as ArcTan2 gives it.
fn facing_toward(dx: i32, dz: i32) -> u32 {
    let turns = f64::from(dz).atan2(f64::from(dx)) / std::f64::consts::TAU;
    ((turns * 65536.0).round() as i64).rem_euclid(65536) as u32
}

/// A trigger's kind by name.
fn kind_name(kind: u8) -> &'static str {
    match kind {
        EVENT_TALK => "talk",
        EVENT_TOUCH => "touch",
        EVENT_SEARCH => "search",
        EVENT_PSYNERGY => "Psynergy",
        _ => "event",
    }
}

/// A row of the scene's event table (struct SceneEvent).
#[derive(Debug, Clone, Copy)]
struct EventRow {
    /// The control word: the kind in the low four bits, then its needs.
    flags: u32,
    /// As GameFlag_IsConditionActive reads it.
    condition: i16,
    /// A message below 0x10000, else a function's address.
    result: u32,
}

/// An actor's object as an event sees it.
#[derive(Debug, Clone, Copy)]
pub struct Actor {
    /// Pixels.
    pub x: i32,
    pub z: i32,
    /// Its height, 16.16 pixels.
    pub y: i32,
    /// The map layer its ground is read from (terrainId).
    pub layer: u8,
    /// Whether it has a sprite.
    pub drawn: bool,
}

/// Where the leader stands to start an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spot {
    /// Pixels.
    pub x: i32,
    pub z: i32,
    pub layer: u8,
    /// The ground's height there, 16.16 pixels.
    pub height: i32,
    /// 1/65536 turns.
    pub facing: u32,
    /// The direction keys held to walk onto the touched cell; 0 for none.
    pub walk: u32,
    /// The cell searched or touched.
    pub cell: Option<(i32, i32)>,
}

/// The moment an event is played from.
#[derive(Clone, Copy)]
pub enum Start<'a> {
    /// As Scene_ApplyEntrance has placed the party: the leader is put at the
    /// spot before the scene's actors are made.
    Entered(&'a Saved),
    /// With the player in control (after another event): the leader is
    /// moved to the spot.
    InControl(&'a Saved),
}

/// Frames a page is held before A while an event is recorded.
const PAGE_HOLD: u32 = 45;
/// Frames an event may run, 90 seconds: the longest scripted scenes seen,
/// the stormy night at the river, run 77.
pub const EVENT_LIMIT: u32 = 5400;

/// What an event left: its pages, when recorded its frames, and the moment
/// the player had control again, when that was in the same scene.
pub struct EventRun {
    pub pages: Vec<Page>,
    /// Each frame's picture, BGR555.
    pub frames: Vec<Vec<u16>>,
    /// Whether the recording stopped at its cap.
    pub capped: bool,
    /// Whether the event ran no script: its dispatch returned in the frame
    /// it began.
    pub instant: bool,
    pub end: Option<Saved>,
}

/// Whether box `b` (pixels, -1 sides taking the header's, 16.16) holds the
/// point (x, z).
pub fn box_holds(camera: [i16; 4], header: [i32; 4], x: i32, z: i32) -> bool {
    let side = |i: usize| if camera[i] == -1 { header[i] >> 16 } else { i32::from(camera[i]) };
    x >= side(0) && x < side(2) && z >= side(1) && z < side(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_leader_faces_the_actor() {
        assert_eq!(facing_toward(5, 0), 0x0000);
        assert_eq!(facing_toward(0, 5), 0x4000);
        assert_eq!(facing_toward(-5, 0), 0x8000);
        assert_eq!(facing_toward(0, -5), 0xc000);
        assert_eq!(facing_toward(5, 5), 0x2000);
    }

    #[test]
    fn psynergy_reaches_as_far_as_its_kind() {
        assert_eq!(psynergy_reach(0x94), (48, 48 << 16));
        assert_eq!(psynergy_reach(0x8d), (32, 16 << 16));
        assert_eq!(psynergy_reach(0x8f), (16, 8 << 16));
    }

    #[test]
    fn boxes_take_the_header_for_missing_sides() {
        let header = [0, 0, fx(512), fx(256)];
        assert!(box_holds([-1, -1, -1, -1], header, 500, 10));
        assert!(!box_holds([0, 0, 240, 160], header, 300, 10));
        assert!(box_holds([256, -1, -1, -1], header, 300, 10));
    }
}
