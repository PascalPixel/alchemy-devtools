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
//! An event starts from the moment Scene_ApplyEntrance has placed the party,
//! with the party moved next to the trigger: then A (talk, search), a field
//! Psynergy request (FieldWork.command) or the cell itself (touch) starts it.
//! Each message page is kept once it is typed and waits for the button
//! (UiText_CheckTextDismissed), and each yes or no choice
//! (Menu_RunConfirmSelection) once shown; then A moves on.

use super::mgba::{Gba, HEIGHT, KEY_A, LR, WIDTH};
use super::story::Trigger;
use crate::ld_map::Map;

/// struct GameState (game_state.h), struct MapScrollWork (map_scroll.h),
/// struct FieldWork (field_runtime.h), struct SceneEntries and the object
/// slots' offsets.
const GS_SCENE: u32 = 0x1c0;
const GS_ENTRANCE: u32 = 0x1c2;
/// x, y and z: three s32 in a row, 16.16 pixels.
const GS_X: u32 = 0x1dc;
const GS_Z: u32 = 0x1e4;
const GS_HEADING: u32 = 0x1e8;
const GS_SELECTED_ACTOR: u32 = 0x1f4;
const GS_ACTIVE_OWNERS: u32 = 0x1f8;
const MAP_ORIGIN: u32 = 0x00;
const MAP_VIEW_X: u32 = 0xe4;
const MAP_VIEW_Y: u32 = 0xe8;
/// minX, minY, maxX and maxY, s32 16.16 pixels.
const MAP_BOUNDS: u32 = 0xec;
const MAP_LAYERS: u32 = 0x104;
const LAYER_CELLS: u32 = 0x2c;
const FIELD_ACTORS: u32 = 0x14;
const FIELD_ACTOR_COUNT: u32 = 66;
const FIELD_COMMAND: u32 = 0x17e;
const OBJ_X: u32 = 0x08;
const OBJ_Z: u32 = 0x10;
/// 1: one sprite; 0: the camera skips the object.
const OBJ_KIND: u32 = 0x54;
const OVERLAY_ENTRANCES: u32 = 0x0c;
const ENTRANCE_SIZE: u32 = 24;
/// The map cells' grid: 128 by 128 words, the collision code in byte 2.
const GRID: i32 = 128;

/// Frames from the player's control to the entrance photo.
pub const SETTLE: u32 = 30;
/// Frames each map view runs after the camera moves: the BG maps, then the
/// scroll and sprites reach the screen.
const HOLD: u32 = 3;
/// A run past this frame has gone wrong.
const MAX_FRAMES: u32 = 30000;

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
    recorded: Option<Vec<EventFrame>>,
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
            let text = self.t.frame - self.t.last_typing <= 2
                || (self.t.last_dismiss > 0 && self.t.frame - self.t.last_dismiss <= 2)
                || self.t.choice_return != 0;
            let frame = EventFrame {
                picture: self.gba.picture(),
                text,
                message: self.t.last_message,
                scene: self.gba.read16(s.game_state + GS_SCENE),
            };
            if let Some(frames) = self.recorded.as_mut() {
                frames.push(frame);
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

    /// Actor `id`'s position (x, z pixels), when the scene has placed it.
    pub fn actor(&self, id: u16) -> Option<(i32, i32)> {
        let actor = self.actor_object(id);
        (actor != 0).then(|| ((self.gba.read32(actor + OBJ_X) as i32) >> 16, (self.gba.read32(actor + OBJ_Z) as i32) >> 16))
    }

    /// The collision code of layer 0's cell (cx, cz).
    fn cell_code(&self, cx: i32, cz: i32) -> i32 {
        if !(0..GRID).contains(&cx) || !(0..GRID).contains(&cz) {
            return -1;
        }
        let cells = self.gba.read32(self.map_work() + MAP_LAYERS + LAYER_CELLS);
        i32::from(self.gba.read8(cells + (((cx + (cz << 7)) << 2) + 2) as u32))
    }

    /// The first cell (in rows within the header's bounds) with collision
    /// code `code`, as its centre in pixels.
    pub fn cell_with_code(&self, code: u16) -> Option<(i32, i32)> {
        let h = self.t.header;
        let (x0, z0, x1, z1) = (h[0] >> 20, h[1] >> 20, h[2] >> 20, h[3] >> 20);
        for cz in z0..z1 {
            for cx in x0..x1 {
                if self.cell_code(cx, cz) == i32::from(code) {
                    return Some((cx * 16 + 8, cz * 16 + 8));
                }
            }
        }
        None
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

    /// Where the leader stands for `trigger` (pixels) and faces, the
    /// `attempt`th way, or None past the last: beside an actor (20 pixels
    /// south, west, east or north, then 34, across a counter), facing it; on
    /// a cell beside a search cell, free ones first, facing it; on a touch
    /// cell.
    pub fn plan(&self, trigger: Trigger, attempt: usize) -> Option<Result<(i32, i32, u32), String>> {
        // North, east, south and west.
        const FACING: [u32; 4] = [0xc000, 0x0000, 0x4000, 0x8000];
        const STEP: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
        const SIDES: [usize; 4] = [2, 3, 1, 0];
        match trigger {
            Trigger::Talk(actor) | Trigger::Psynergy(_, actor) => {
                let side = *SIDES.get(attempt % 4)?;
                let distance = *[20, 34].get(attempt / 4)?;
                let Some((ax, az)) = self.actor(actor) else {
                    return Some(Err(format!("actor {actor} is not in the scene")));
                };
                let (dx, dz) = STEP[side];
                Some(Ok((ax + dx * distance, az + dz * distance, FACING[(side + 2) & 3])))
            }
            Trigger::Touch(code) => {
                if attempt > 0 {
                    return None;
                }
                Some(self.cell_with_code(code).map(|(x, z)| (x, z, FACING[2])).ok_or_else(|| format!("no cell holds code {code}")))
            }
            Trigger::Search(code) => {
                let Some((x, z)) = self.cell_with_code(code) else {
                    return (attempt == 0).then(|| Err(format!("no cell holds code {code}")));
                };
                let (cx, cz) = (x >> 4, z >> 4);
                let mut sides: Vec<usize> = SIDES.iter().copied().filter(|&d| self.cell_code(cx + STEP[d].0, cz + STEP[d].1) == 0).collect();
                sides.extend(SIDES.iter().copied().filter(|d| !sides.contains(d)).collect::<Vec<_>>());
                let side = *sides.get(attempt)?;
                let (nx, nz) = (cx + STEP[side].0, cz + STEP[side].1);
                Some(Ok((nx * 16 + 8, nz * 16 + 8, FACING[(side + 2) & 3])))
            }
        }
    }

    /// Plays an event from `entered` with the leader at `start`. Keeps up to
    /// `max_pages` pages as stills; with `record`, also every frame up to the
    /// event's end or `cap` frames, holding each page PAGE_HOLD frames so a
    /// GIF shows it.
    pub fn event(
        &mut self,
        entered: &Saved,
        trigger: Trigger,
        start: (i32, i32, u32),
        messages: &[u32],
        max_pages: usize,
        record: Option<usize>,
    ) -> Result<EventRun, String> {
        self.load(entered);
        let gs = self.s.game_state;
        self.gba.write32(gs + GS_X, fx(start.0) as u32);
        self.gba.write32(gs + GS_Z, fx(start.1) as u32);
        self.gba.write32(gs + GS_HEADING, start.2);
        self.until_control()?;
        self.frames(SETTLE)?;
        if record.is_some() {
            self.recorded = Some(Vec::new());
        }
        match trigger {
            Trigger::Talk(_) | Trigger::Search(_) => self.press(2)?,
            Trigger::Psynergy(ability, _) => {
                let work = self.gba.read32(self.s.field_work);
                self.gba.write16(work + FIELD_COMMAND, ability);
            }
            Trigger::Touch(_) => {}
        }
        let hold = if record.is_some() { PAGE_HOLD } else { 3 };
        let trigger_frame = self.t.frame;
        let steps_at_start = self.t.steps;
        let mut pages = Vec::new();
        let (mut wait_start, mut press_frame, mut quiet_start) = (0, 0, self.t.frame);
        let mut captured = false;
        let mut pressed = false;
        let mut since = 0;
        let mut message = None;
        let mut seen_message = 0;
        let mut capped = false;
        loop {
            if record.is_none() && pages.len() >= max_pages {
                break;
            }
            if let (Some(cap), Some(frames)) = (record, &self.recorded) {
                if frames.len() >= cap {
                    capped = true;
                    break;
                }
            }
            self.frames(1)?;
            since += 1;
            if self.t.last_message != seen_message {
                seen_message = self.t.last_message;
                if messages.contains(&seen_message) {
                    message = Some(seen_message);
                }
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
            // Back in the field loop with no page: the event is over.
            if self.t.steps > steps_at_start + 30 && since > 60 {
                break;
            }
            if self.t.frame - self.t.last_typing <= 2 {
                quiet_start = self.t.frame;
            }
            // Nothing typed or waiting for 120 frames (an effect, a walk):
            // press A.
            if self.t.frame - quiet_start >= 120 {
                self.press(3)?;
                quiet_start = self.t.frame;
            }
            if since > 3000 {
                break;
            }
        }
        let frames = self.recorded.take().unwrap_or_default();
        Ok(EventRun { pages, frames, capped })
    }
}

/// Frames a page is held before A while an event is recorded.
const PAGE_HOLD: u32 = 45;

/// One recorded frame of an event and what the game was doing.
pub struct EventFrame {
    pub picture: Vec<u16>,
    /// Text being typed, a page waiting or a choice open.
    pub text: bool,
    /// The message last looked up.
    pub message: u32,
    pub scene: u16,
}

/// What an event left: its pages and, when recorded, its frames.
pub struct EventRun {
    pub pages: Vec<Page>,
    pub frames: Vec<EventFrame>,
    /// Whether the recording stopped at its cap.
    pub capped: bool,
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
    fn boxes_take_the_header_for_missing_sides() {
        let header = [0, 0, fx(512), fx(256)];
        assert!(box_holds([-1, -1, -1, -1], header, 500, 10));
        assert!(!box_holds([0, 0, 240, 160], header, 300, 10));
        assert!(box_holds([256, -1, -1, -1], header, 300, 10));
    }
}
