//! A field scene, run from a built edition: where Game_ResetForNewGame has
//! set a new game's defaults (MainRoutineWork_InitializeDefaults) and makes its next call
//! (WindowColor_Apply), the run writes the scene, entrance and story flags
//! into gMainRoutineWork, so the main routine runs that scene instead of the title.
//! The location-name notice (AreaNameWindow_Show) returns at once unless
//! show_notice is set. The
//! scene is the player's once FieldSequence_Run's loop takes its first
//! step (Field_UpdateWalking).
//!
//! A map is stitched from one moment of the game: for each 240 by 160 view
//! of the box the machine is loaded again, the map's followed point
//! (MapGridScrollWork.origin) is pinned to gMainRoutineWork.x, y and z holding the
//! view's focus at each entry of its two readers, and the next scheduler
//! call of MapGrid_UpdateLayerScroll runs MapGrid_CenterOnFollowedPoint instead,
//! which redraws each layer around the point as the event camera does. The
//! actors' task (Actor_UpdateAllThumb) is skipped meanwhile, so no actor
//! moves or draws on the shared random numbers; the third frame is the view.
//!
//! An event starts with the leader where the player would stand to start
//! it: beside the actor it talks to, level with it on the layer he walks
//! (the actor's own only after it), or a step from the cell it touches or
//! searches, on the ground there (MapGrid_GetTerrainHeight, run in the machine
//! between two steps of the field sequence), and always on ground he could walk
//! to from where he entered, in the room the camera shows (Field::reach).
//! Then A (talk, search), a field Psynergy request (FieldWork.command) or
//! walking onto the cell (touch) starts it, and the field sequence's own
//! dispatch (Scene_TalkToActor, Scene_RunStepTriggerOrExit, Field_ExamineCell,
//! Field_UsePsynergy) shows which event began; the event function the CPU
//! then enters (Field::watch) shows whose row it was, since two rooms of a
//! scene can give the same actor id or cell code to different events. Each
//! message page is kept once it is typed and waits for the button
//! (Text_CheckTextDismissed), and each yes or no choice
//! (YesNoMenu_Run) once shown; then A moves on. The event is over
//! when the field sequence steps again with nothing to read and nothing it set
//! moving (a pushed pillar) still moves.

use super::mgba::{Gba, HEIGHT, KEY_A, KEY_DOWN, KEY_LEFT, KEY_RIGHT, KEY_UP, LR, SP, WIDTH};
use super::party::{chapter, order, Party};
use super::story::{Trigger, CARRY, MOVE};
use crate::ld_map::Map;
use std::collections::HashMap;

/// struct MainRoutineWork (main_routine_work.h), struct MapGridScrollWork (map_grid_scroll.h),
/// struct FieldWork (field_work.h), struct SceneEntryPoints and the object
/// slots' offsets.
const GS_COINS: u32 = 0x10;
const GS_SCENE: u32 = 0x1c0;
const GS_ENTRANCE: u32 = 0x1c2;
/// x, y and z: three s32 in a row, 16.16 pixels.
const GS_X: u32 = 0x1dc;
const GS_Y: u32 = 0x1e0;
const GS_Z: u32 = 0x1e4;
const GS_HEADING: u32 = 0x1e8;
/// The leader's layer, which the field gives the leader it creates.
const GS_TURN: u32 = 0x1ec;
const GS_SELECTED_ACTOR: u32 = 0x1f4;
const GS_ACTIVE_OWNERS: u32 = 0x1f8;
const MAP_ORIGIN: u32 = 0x00;
const MAP_VIEW_X: u32 = 0xe4;
const MAP_VIEW_Y: u32 = 0xe8;
/// minX, minY, maxX and maxY, s32 16.16 pixels.
const MAP_BOUNDS: u32 = 0xec;
/// MapGridScrollAnimView.layers: each layer's window onto the cell grid, 0x30 bytes
/// apart, its cells pointer first.
const MAP_LAYER_CELLS: u32 = 0x130;
const MAP_LAYER_SIZE: u32 = 0x30;
/// MapGridScrollWork.layers: BG3, BG2 and BG1, each x then y first.
const MAP_LAYERS: u32 = 0x104;
/// MapGridLayerScroll's offset, parallax scale, drift speed and phase (x
/// then y, words) and wrap mask (x then y, halfwords).
const LAYER_OFFSET: u32 = 0x08;
const LAYER_SCALE: u32 = 0x10;
const LAYER_SPEED: u32 = 0x18;
const LAYER_PHASE: u32 = 0x20;
const LAYER_MASK: u32 = 0x28;
/// The layers' screen maps in VRAM, BG_SCREEN_BLOCK(5) to (7), which
/// MapGrid_SetCameraCenter draws; and the frames Field::pin_view waits for
/// the field's layer task to run.
const PIN_SCREEN_MAPS: u32 = 0x0600_2800;
const PIN_SCREEN_MAP_BYTES: u32 = 3 * 0x800;
const PIN_DRAW_FRAMES: u32 = 8;
const FIELD_EVENTS: u32 = 0x10;
const FIELD_ACTORS: u32 = 0x14;
const FIELD_ACTOR_COUNT: u32 = 66;
/// The first actor a scene places (ACTOR_FIRST_PLACED); 0 to 7 are the party.
const FIRST_PLACED: u16 = 8;
const FIELD_COMMAND: u32 = 0x17e;
/// struct Object.
const OBJ_ANGLE: u32 = 0x06;
const OBJ_X: u32 = 0x08;
const OBJ_Y: u32 = 0x0c;
const OBJ_Z: u32 = 0x10;
const OBJ_TERRAIN_HEIGHT: u32 = 0x14;
const OBJ_TERRAIN_ID: u32 = 0x22;
const OBJ_VELOCITY: u32 = 0x24;
/// 1: one sprite; 2: a group; 0: no sprite, which the camera skips.
const OBJ_KIND: u32 = 0x54;
/// The struct Sprite (kind 1) or the four of a group (kind 2).
const OBJ_SPRITE: u32 = 0x50;
/// struct Sprite's entries, its four layers, and the colour mode
/// (param) of struct SpriteAnimation, whose first halfword is its sprite id.
const ANIM_ENTRIES: u32 = 0x28;
const ANIM_ENTRY_PARAM: u32 = 0x05;
/// FieldWork.mode (an enum ActorSystemMode) and its world map mode.
const FIELD_MODE: u32 = 0x19e;
const ACTOR_SYSTEM_WORLD_MAP: u16 = 3;
/// The world map (struct WorldMapView, which gMapGridWork points to there):
/// 16 by 16 tiles of 16 by 16 cells, each cell 32 pixels square, so 8192
/// pixels each way. tileIds names the tile at each place, and Ram_CellPictures
/// (gCellPictures) holds the 2 by 2 tiles around the party, 32 cells to a row,
/// the collision code in byte 2 as on a layered map (Field_UpdateWalking's
/// step test reads it there).
const PERSPECTIVE_TILE_IDS: u32 = 0x138;
const PERSPECTIVE_QUARTER_TILES: u32 = 0x338;
const WORLD_TILES: i32 = 16;
const WORLD_TILE_CELLS: i32 = 16;
const WORLD_CELL: i32 = 32;
const WORLD_CELLS: i32 = WORLD_TILES * WORLD_TILE_CELLS;
/// WorldMap_GetCollision's value where neither layer has one, and the
/// values the party walks on (WorldMap_IsWalkable).
const WORLD_COLLISION_DEFAULT: u32 = 7;
/// The world map's touch for walking into one of its actors, a place:
/// this plus the actor (COORD_EVENT_PLACE_BASE).
const WORLD_CONTACT_BASE: u16 = 100;
const WORLD_WALKABLE: std::ops::RangeInclusive<u32> = 5..=12;
/// The second layer's tiles follow the first's 320 (WorldMap_UpdateCurrentTileBlock).
const WORLD_LAYER_TILES: u32 = 320;
const OVERLAY_ENTRANCES: u32 = 0x0c;
const ENTRANCE_SIZE: u32 = 24;
/// The map cells' grid: 128 by 128 words, the collision code in byte 2.
const GRID: i32 = 128;
/// Collision codes (enum CellBehavior): the scene's own events, the cells the
/// leader pushes against, and the wall.
const CELL_EVENT_LAST: i32 = 239;
const CELL_LEDGE: i32 = 252;
const CELL_CLIMB: i32 = 253;
const CELL_JUMP: i32 = 254;
pub(super) const CELL_WALL: i32 = 255;
/// North, east, south and west: a cell's step, the heading that faces that
/// way and the direction key that walks it.
const STEP: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
const FACING: [u32; 4] = [0xc000, 0x0000, 0x4000, 0x8000];
const KEYS: [u32; 4] = [KEY_UP, KEY_RIGHT, KEY_DOWN, KEY_LEFT];
/// MapGrid_CheckCollision's limits: ground up to 8 pixels higher or 12 lower
/// is a step the leader can take.
pub(super) const STEP_UP: i32 = 8 << 16;
pub(super) const STEP_DOWN: i32 = 12 << 16;
/// The furthest a ledge, a climb or a jump is taken to drop the leader.
const JUMP_DROP: i32 = 96 << 16;
/// struct MapEvent: its control word (kind in the low four bits, a facing
/// in bits 15..12 with TRIGGER_FLAG_FACING), its target and its result.
const EVENT_ROW_SIZE: u32 = 12;
const STEP_TRIGGER_FLAG_WALKING: u32 = 0x200;
const TRIGGER_FLAG_FACING: u32 = 0x400;
/// The TriggerKind of each dispatch.
pub const TRIGGER_TALK: u8 = 0;
pub const TRIGGER_EXIT: u8 = 1;
pub const TRIGGER_STEP: u8 = 2;
pub const TRIGGER_EXAMINE: u8 = 3;
pub const TRIGGER_PSYNERGY: u8 = 5;
/// No random encounters while set (flags.h).
const FLAG_NO_ENCOUNTERS: u16 = 0x160;
/// struct Unit's psynergy, four bytes each.
const UNIT_ACTION_SLOTS: u32 = 0x58;
/// Palette RAM, 1 KB.
const PALETTE_RAM: u32 = 0x0500_0000;
/// The scene number of a battle (MAP_BATTLE).
const MAP_BATTLE: u16 = 0x1fe;
/// The first line of vertical blank.
const VBLANK_FIRST_LINE: u16 = 160;

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
/// Speakers below this are the party (FieldMessage_ShowAtActor has them
/// speak from the leader); from it on, a speaker is an actor of the scene.
const PARTY_SPEAKERS: u16 = 8;
/// Frames the field sequence must step, with nothing to read, before the
/// player has control again.
const CONTROL_FRAMES: u32 = 30;
/// Frames with nothing on stage moving before an event that has given back
/// control is over, and the most frames it is followed for that.
const STILL_FRAMES: u32 = 10;
const SLIDE_FRAMES: u32 = 240;
/// Frames a trigger may take to start its event: the leader walking onto a
/// cell and pushing against what is beyond it.
const START_FRAMES: u32 = 180;

use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

/// For `render probe heap`: the heap's work (gHeapSlots, 0 while off), the
/// functions at whose entry it is read besides each frame's end (Heap_Release,
/// HeapSlot_Release, Heap_Initialize: just before a pool's
/// next address falls), and the highest next addresses read.
pub static HEAP_WORK: AtomicU32 = AtomicU32::new(0);
pub static HEAP_PCS: [AtomicU32; 3] = [AtomicU32::new(1), AtomicU32::new(1), AtomicU32::new(1)];
pub static HEAP_IWRAM_HIGH: AtomicU32 = AtomicU32::new(0);
pub static HEAP_EWRAM_HIGH: AtomicU32 = AtomicU32::new(0);

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
    /// FieldMessage_ShowAtActor, which opens a message beside its speaker.
    speak: u32,
    objects: u32,
    typing: u32,
    choice: u32,
    lookup: u32,
    terrain_height: u32,
    write_cell_tile: u32,
    map_cell_pictures: u32,
    map_collision: u32,
    map_cell_buffer: u32,
    preset: u32,
    restore: u32,
    unit_get: u32,
    /// The messages a battle the party loses ends with (MsgGoesDownInDefeat,
    /// MsgPartyDefeated), from the edition's ELF.
    defeat: Vec<u32>,
    talk: u32,
    touch: u32,
    search: u32,
    psynergy: u32,
    game_state: u32,
    flag_bytes: u32,
    map_work: u32,
    field_work: u32,
    overlay: u32,
    /// gRandomSeed, gFrameCount, gFrameRenderPending and gBgScroll, which a
    /// whole picture (whole.rs) reads to tell the moment a view shows.
    pub random_seed: u32,
    pub frame_count: u32,
    pub render_pending: u32,
    pub bg_scroll: u32,
}

impl Symbols {
    /// Reads the defeat messages' numbers, absolute symbols the linker map
    /// leaves out, from the edition's ELF at `elf`.
    pub fn read_defeat(&mut self, elf: &std::path::Path) -> Result<(), String> {
        let data = std::fs::read(elf).map_err(|e| format!("cannot read {}: {e}", elf.display()))?;
        let elf = crate::elf::Elf::parse(&data)?;
        self.defeat = elf.symbols.iter().filter(|y| y.name == "MsgGoesDownInDefeat" || y.name == "MsgPartyDefeated").map(|y| y.value).collect();
        Ok(())
    }

    pub fn find(map: &Map) -> Result<Symbols, String> {
        let at = |name: &str| map.symbol(name).map(|(_, a)| a & !1).ok_or_else(|| format!("the map has no {name}"));
        Ok(Symbols {
            init: at("MainRoutineWork_InitializeDefaults").or_else(|_| at("SequenceParentWork_InitializeDefaults")).or_else(|_| at("Adventure_InitializeDefaults")).or_else(|_| at("Common_InitializeDefaults")).or_else(|_| at("SaveBlock_InitializeDefaults"))?,
            glow: at("WindowColor_Apply")?,
            load_layered: at("MapGrid_LoadMap")?,
            // Each second spelling is Alchemy's before The Broken Seal's rename.
            apply_entrance: at("Scene_ApplyEntrance").or_else(|_| at("Scene_ApplyWarpDest"))?,
            notice: at("AreaNameWindow_Show").or_else(|_| at("MapNamePopup_Show"))?,
            step: at("Field_UpdateWalking")?,
            update_layer: at("MapGrid_UpdateLayerScroll")?,
            center: at("MapGrid_CenterOnFollowedPoint")?,
            dismiss: at("Text_CheckTextDismissed")?,
            speak: at("FieldMessage_ShowAtActor")?,
            objects: at("Actor_UpdateAllThumb")?,
            typing: at("Message_Type").or_else(|_| at("TextPrinter_Step"))?,
            choice: at("YesNoMenu_Run")?,
            lookup: at("Message_Find")?,
            terrain_height: at("MapGrid_GetTerrainHeight")?,
            write_cell_tile: at("WorldMap_WriteLayerCellTile")?,
            map_cell_pictures: at("gCellPictures").or_else(|_| at("gMetatiles"))?,
            map_collision: at("gMapGridShapes")?,
            map_cell_buffer: at("gMapGridCellBuffer")?,
            preset: at("DebugParameterSet_Load").or_else(|_| at("DebugParty_LoadPreset"))?,
            restore: at("Party_RestoreHpPp")?,
            unit_get: at("Unit_Get")?,
            defeat: Vec::new(),
            talk: at("Scene_TalkToActor").or_else(|_| at("Scene_TalkToObjectEvent"))?,
            touch: at("Scene_RunStepTriggerOrExit").or_else(|_| at("Scene_RunCoordEventOrWarp"))?,
            search: at("Field_ExamineCell")?,
            psynergy: at("Field_UsePsynergy")?,
            game_state: at("gMainRoutineWork").or_else(|_| at("gSequenceParentWork")).or_else(|_| at("gAdventure")).or_else(|_| at("gCommon")).or_else(|_| at("gPersistentState")).or_else(|_| at("gSaveBlock"))?,
            flag_bytes: at("gGameFlagBytes")?,
            map_work: at("gMapGridWork")?,
            field_work: at("gFieldWork")?,
            overlay: at("gOverlayArea")?,
            random_seed: at("gRandomSeed")?,
            frame_count: at("gFrameCount")?,
            render_pending: at("gFrameRenderPending")?,
            bg_scroll: at("gBgScroll")?,
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
pub(super) struct Track {
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
    /// The first speaker an event's message was opened beside that the
    /// scene has no actor for (0 for none): the event is playing where its
    /// cast is not on stage.
    absent_speaker: u16,
    last_dismiss: u32,
    last_typing: u32,
    choice_return: u32,
    choice_frame: u32,
    last_message: u32,
    keys: u32,
    /// A function to call at the next Field_UpdateWalking entry (0 for none),
    /// with x and z written to the followed point first; and r0 to r3 and
    /// lr kept while it runs.
    call_target: u32,
    call_origin: [i32; 2],
    calling: bool,
    call_saved: [u32; 5],
    /// r0 and r1 for the call, when start_call gives them.
    call_args: Option<[u32; 2]>,
    /// The instruction the CPU runs next.
    pc: u32,
    /// Whether the CPU sleeps until the next interrupt.
    halted: bool,
    /// The last frame the field sequence stepped, and the first of the frames
    /// it has stepped in a row up to it.
    step_frame: u32,
    stepping_since: u32,
    /// The last event the field sequence dispatched: its kind (TRIGGER_TALK and
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
    /// How many times Scene_ApplyEntrance has placed the party: once each
    /// scene the field runs, the world map included.
    entries: u32,
    /// The first of the watched event functions (Field::watch) the CPU
    /// entered since the last dispatch, 0 for none.
    ran: u32,
    /// Whether one of the event's own functions (Field::own) has run since.
    own_ran: bool,
    /// The calls that set up the scene's party (Field::party_calls) made
    /// so far at WindowColor_Apply's entry, and r0 to r3 and lr kept
    /// meanwhile.
    party_calls: u8,
    party_saved: [u32; 5],
    /// How many times the run has been changed from outside the game since
    /// it began (Field::cut): a whole picture never pins a view from one
    /// side of a change and draws it on the other.
    cuts: u32,
}

/// What makes a field (Field::recipe): its ROM, symbols and setup, and the
/// changes it makes as the CPU runs.
pub struct Recipe<'rom> {
    rom: &'rom [u8],
    s: &'rom Symbols,
    setup: Setup,
    returns: Vec<(u32, u32)>,
    pokes: Vec<(u32, u32, u32)>,
    register_pokes: Vec<(u32, u32, u32)>,
    show_notice: bool,
}

impl<'rom> Recipe<'rom> {
    pub fn enter(&self) -> Result<Field<'rom>, String> {
        let mut field = Field::enter(self.rom, self.s, &self.setup)?;
        field.returns.clone_from(&self.returns);
        field.pokes.clone_from(&self.pokes);
        field.register_pokes.clone_from(&self.register_pokes);
        field.show_notice = self.show_notice;
        Ok(field)
    }
}

/// A saved moment: the machine and the run's state.
pub struct Saved {
    pub(super) state: Vec<u8>,
    pub(super) track: Track,
}

impl Track {
    pub(super) fn keys(&self) -> u32 {
        self.keys
    }

    pub(super) fn cuts(&self) -> u32 {
        self.cuts
    }
}

impl Saved {
    /// The keys the machine holds from this moment (KEYINPUT bits), as the
    /// last frame left them.
    pub fn keys(&self) -> u32 {
        self.track.keys
    }
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
    rom: &'rom [u8],
    s: &'rom Symbols,
    t: Track,
    setup: Setup,
    /// Frames kept while an event is recorded.
    recorded: Option<Vec<Vec<u16>>>,
    /// The machine's state at the end of each frame, kept while an event is
    /// recorded whole (Field::whole), to draw it whole from.
    states: Option<super::whole::States>,
    /// Whether a recorded event also keeps its states, for whole.rs.
    pub whole: bool,
    /// The event functions of the rows for the trigger being played, to see
    /// which one the dispatch runs.
    watch: Vec<u32>,
    /// The event's own functions among them.
    own: Vec<u32>,
    /// The world map's cell words, WORLD_CELLS by WORLD_CELLS in rows, once
    /// read (Field::read_world).
    world_cells: Option<Vec<u32>>,
    /// Whether, when the leader can walk to no spot for a trigger, a spot he
    /// cannot walk to will do (Field::spots), and whether the last spots
    /// were such.
    pub relaxed: bool,
    unwalked: bool,
    /// For `render probe`: the addresses whose entry calls `probe` (with the
    /// machine, the address and the frame), which also runs at each frame's
    /// end (address 0); and the functions made to return at once, with r0.
    pub probe_pcs: Vec<u32>,
    pub probe: Option<Box<dyn FnMut(&Gba<'rom>, u32, u32) + 'rom>>,
    pub returns: Vec<(u32, u32)>,
    /// Words written as the CPU reaches an address: (address, where, value).
    pub pokes: Vec<(u32, u32, u32)>,
    /// Lets AreaNameWindow_Show run, for a picture of the notice.
    pub show_notice: bool,
    /// Registers set as the CPU reaches an address: (address, register,
    /// value), as a caller would have passed the value.
    pub register_pokes: Vec<(u32, u32, u32)>,
    /// The story chapter the scene falls in (party.rs), if any.
    party: Option<&'static Party>,
}

impl<'rom> Field<'rom> {
    pub fn write32(&mut self, address: u32, value: u32) {
        self.gba.write32(address, value);
    }

    /// The addresses the run reads, from the edition's map.
    pub fn symbols(&self) -> &'rom Symbols {
        self.s
    }

    /// How to make another field like this one, on another thread, to load
    /// its states into; none while a probe watches it, which cannot be
    /// copied.
    pub fn recipe(&self) -> Option<Recipe<'rom>> {
        (self.probe.is_none() && self.probe_pcs.is_empty()).then(|| Recipe {
            rom: self.rom,
            s: self.s,
            setup: self.setup.clone(),
            returns: self.returns.clone(),
            pokes: self.pokes.clone(),
            register_pokes: self.register_pokes.clone(),
            show_notice: self.show_notice,
        })
    }

    /// Actor `id`'s object's address, 0 for none.
    pub fn actor_address(&self, id: u16) -> u32 {
        self.actor_object(id)
    }

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
            absent_speaker: 0,
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
            call_args: None,
            pc: 0,
            halted: false,
            step_frame: 0,
            stepping_since: 0,
            dispatched: None,
            dispatch_return: 0,
            dispatch_result: None,
            dispatch_end: 0,
            loads: 0,
            entries: 0,
            ran: 0,
            own_ran: false,
            party_calls: 0,
            party_saved: [0; 5],
            cuts: 0,
        };
        let mut field = Field { gba: Gba::new(rom)?, rom, s, t, setup: setup.clone(), recorded: None, states: None, whole: false, watch: Vec::new(), own: Vec::new(), world_cells: None, relaxed: false, unwalked: false, probe_pcs: Vec::new(), probe: None, returns: Vec::new(), pokes: Vec::new(), register_pokes: Vec::new(), show_notice: false, party: chapter(setup.scene)? };
        while !field.t.applied {
            field.run_instruction()?;
        }
        Ok(field)
    }

    pub fn flag_is_set(&self, flag: u16) -> bool {
        self.gba.read8(self.s.flag_bytes + u32::from((flag >> 3) & 0x1ff)) & (1 << (flag & 7)) != 0
    }

    pub fn set_flag(&mut self, flag: u16, on: bool) {
        let a = self.s.flag_bytes + u32::from((flag >> 3) & 0x1ff);
        let m = 1u8 << (flag & 7);
        let v = self.gba.read8(a);
        self.gba.write8(a, if on { v | m } else { v & !m });
    }

    /// Makes `party` the active party, in order: party flags 0 to 7 and
    /// gMainRoutineWork.party.
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

    pub fn read32(&self, address: u32) -> u32 {
        self.gba.read32(address)
    }

    pub fn write8(&mut self, address: u32, value: u8) {
        self.gba.write8(address, value);
    }

    pub fn write16(&mut self, address: u32, value: u16) {
        self.gba.write16(address, value);
    }

    /// The map's view: the screen's top left on the map, in pixels.
    pub fn view(&self) -> (i32, i32) {
        let work = self.map_work();
        (self.gba.read32(work + MAP_VIEW_X) as i32 >> 16, self.gba.read32(work + MAP_VIEW_Y) as i32 >> 16)
    }

    /// The leader as he is now.
    pub fn leader(&self) -> Option<Actor> {
        self.actor(self.gba.read32(self.s.game_state + GS_SELECTED_ACTOR) as u16)
    }

    /// Enters again from `placed` (as Scene_ApplyEntrance placed the party)
    /// with the leader at `spot`, and runs until the player has control and
    /// the camera has settled on him, as an event's start does.
    pub fn stand(&mut self, placed: &Saved, spot: &Spot) -> Result<(), String> {
        self.load(placed);
        let gs = self.s.game_state;
        self.gba.write32(gs + GS_X, fx(spot.x) as u32);
        self.gba.write32(gs + GS_Y, spot.height as u32);
        self.gba.write32(gs + GS_Z, fx(spot.z) as u32);
        self.gba.write32(gs + GS_HEADING, spot.facing);
        self.gba.write16(gs + GS_TURN, u16::from(spot.layer));
        self.until_control()?;
        self.put_leader(spot);
        self.frames(SETTLE)
    }

    /// The abilities character `owner` has, in the order of their slots
    /// (struct Unit's psynergy, an action id in each low 14 bits),
    /// as the field menu lists them.
    pub fn abilities(&mut self, owner: u8) -> Result<Vec<u16>, String> {
        let unit = self.eval(self.s.unit_get, [u32::from(owner), 0, 0, 0])?;
        Ok((0..32).map(|i| self.gba.read16(unit + UNIT_ACTION_SLOTS + 4 * i) & 0x3fff).filter(|&a| a != 0).collect())
    }

    /// The address of character `owner`'s struct Unit (Unit_Get).
    pub fn unit(&mut self, owner: u8) -> Result<u32, String> {
        self.eval(self.s.unit_get, [u32::from(owner), 0, 0, 0])
    }

    /// The scene's struct FieldWork (gFieldWork).
    pub fn field_work(&self) -> u32 {
        self.gba.read32(self.s.field_work)
    }

    /// Gives the party `coins` (SaveBlock.coins).
    pub fn set_coins(&mut self, coins: u32) {
        self.gba.write32(self.s.game_state + GS_COINS, coins);
    }



    /// Draws every line from now on over `colour` (BGR555) where nothing else
    /// is drawn; the game's palette keeps its own colour 0.
    pub fn set_backdrop(&mut self, colour: u16) {
        self.gba.set_backdrop(&[colour; HEIGHT]);
    }

    /// Draws every line with the game's own colour 0 again.
    pub fn clear_backdrop(&mut self) {
        self.gba.clear_backdrop();
    }

    /// Stores a halfword into an I/O register, as the processor would.
    pub fn io_write16(&mut self, address: u32, value: u16) {
        self.gba.io_write16(address, value);
    }

    /// Calls the game function at `target` from the next Field_UpdateWalking
    /// entry, with the followed point at (x, z) (16.16 pixels), and runs
    /// until it has returned.
    pub fn call(&mut self, target: u32, origin: [i32; 2]) -> Result<(), String> {
        self.t.call_target = target & !1;
        self.t.call_origin = origin;
        while self.t.call_target != 0 || self.t.calling {
            self.run_instruction()?;
        }
        Ok(())
    }

    /// As `call`, with r0 and r1, without waiting: the frames that follow
    /// run it, so a function that waits on the pad can be played.
    pub fn start_call(&mut self, target: u32, args: [u32; 2]) {
        self.t.call_target = target & !1;
        self.t.call_origin = [self.gba.read32(self.s.game_state + GS_X) as i32, self.gba.read32(self.s.game_state + GS_Z) as i32];
        self.t.call_args = Some(args);
    }

    /// Moves the party's position in the game state (pixels).
    pub fn set_position(&mut self, x: i32, z: i32) {
        self.gba.write32(self.s.game_state + GS_X, fx(x) as u32);
        self.gba.write32(self.s.game_state + GS_Z, fx(z) as u32);
    }

    /// Stops or starts the actors' task (Actor_UpdateAllThumb).
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

    /// How many times the field sequence has stepped, which it does only while
    /// the player has control.
    pub fn steps(&self) -> u32 {
        self.t.steps
    }

    /// The message last looked up.
    pub fn last_message(&self) -> u32 {
        self.t.last_message
    }

    /// The colours of palette RAM, backgrounds' and sprites', and the
    /// blending registers (REG_BLDCNT, REG_BLDALPHA, REG_BLDY): what the
    /// screen's colours come from besides its pictures.
    fn palette(&self) -> Vec<u32> {
        let mut out: Vec<u32> = (0..256).map(|i| self.gba.read32(PALETTE_RAM + 4 * i)).collect();
        out.extend([0x50, 0x52, 0x54].map(|r| u32::from(self.gba.read16(0x0400_0000 + r))));
        out
    }

    /// The last picture, BGR555.
    pub fn picture(&self) -> Vec<u16> {
        self.gba.picture()
    }

    /// The calls that give the scene's chapter its party (party.rs) before
    /// the scene loads: Camelot's debug preset, then every member's HP and PP
    /// restored (Party_RestoreHpPp).
    fn party_calls(&self) -> Vec<(u32, u32)> {
        match self.party.and_then(|c| c.preset) {
            Some(preset) => vec![(self.s.preset, preset), (self.s.restore, 1)],
            None => Vec::new(),
        }
    }

    /// At WindowColor_Apply's entry, while a new game is set up: makes the
    /// next of the party's calls, returning to that entry, or, once all are
    /// made, puts back the registers the first found. Whether it called.
    fn party_call(&mut self) -> bool {
        let calls = self.party_calls();
        let done = usize::from(self.t.party_calls);
        if done == 0 && !calls.is_empty() {
            for i in 0..4 {
                self.t.party_saved[i] = self.gba.register(i as u32);
            }
            self.t.party_saved[4] = self.gba.register(LR);
        }
        if done < calls.len() {
            let (target, r0) = calls[done];
            self.t.party_calls += 1;
            self.gba.set_register(0, r0);
            self.gba.set_register(LR, self.s.glow | 1);
            self.gba.branch(target);
            return true;
        }
        if done > 0 && done == calls.len() {
            for (i, &v) in self.t.party_saved.iter().enumerate().take(4) {
                self.gba.set_register(i as u32, v);
            }
            self.gba.set_register(LR, self.t.party_saved[4]);
            self.t.party_calls += 1;
        }
        false
    }

    /// Lists the party (gMainRoutineWork.party) as the party flags have it
    /// now, the chapter's members first (party::order); before the run's
    /// flags decide, the chapter's members are the party flags.
    fn list_party(&mut self) {
        let Some(chapter) = self.party else {
            return;
        };
        if self.t.phase == Phase::Defaults {
            let bits = chapter.members.iter().fold(0u8, |b, &o| b | 1 << o);
            self.gba.write8(self.s.flag_bytes, bits);
            self.apply_flags();
        }
        let flags = self.gba.read8(self.s.flag_bytes);
        let party = order(&chapter.members, flags);
        for (i, &owner) in party.iter().enumerate() {
            self.gba.write8(self.s.game_state + GS_ACTIVE_OWNERS + i as u32, owner);
        }
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
    fn run_instruction(&mut self) -> Result<bool, String> {
        let step = self.gba.step();
        let frame_done = step.frame != self.t.frame;
        self.t.frame = step.frame;
        let pc = step.pc;
        self.t.pc = pc;
        self.t.halted = step.halted != 0;
        let s = self.s;
        let lr = |gba: &Gba| gba.register(LR) & !1;

        if self.t.phase == Phase::Boot && pc == s.init && self.t.init_return == 0 {
            self.t.init_return = lr(&self.gba);
        } else if self.t.init_return != 0 && pc == self.t.init_return {
            // Game_ResetForNewGame now stores the title and entrance 2, then
            // calls WindowColor_ApplyFar.
            self.t.init_return = 0;
            self.t.phase = Phase::Defaults;
        } else if self.t.phase == Phase::Defaults && pc == s.glow && self.party_call() {
            return Ok(frame_done);
        } else if self.t.phase == Phase::Defaults && pc == s.glow {
            self.gba.write16(s.game_state + GS_SCENE, self.setup.scene);
            self.gba.write16(s.game_state + GS_ENTRANCE, self.setup.entrance);
            self.apply_flags();
            self.list_party();
            self.t.phase = Phase::Loading;
        }
        if pc == s.load_layered {
            self.t.loads += 1;
        }
        if pc == s.apply_entrance {
            self.t.entries += 1;
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
            self.list_party();
            self.t.applied = true;
            self.t.phase = Phase::Entered;
        }
        if pc == s.notice && !self.show_notice {
            self.gba.return_now(0);
        }
        if (frame_done || self.probe_pcs.contains(&pc)) && self.probe.is_some() {
            let mut probe = self.probe.take().unwrap();
            probe(&self.gba, if self.probe_pcs.contains(&pc) { pc } else { 0 }, self.t.frame);
            if frame_done && self.probe_pcs.contains(&pc) {
                probe(&self.gba, 0, self.t.frame);
            }
            self.probe = Some(probe);
        }
        for i in 0..self.register_pokes.len() {
            let (at, register, value) = self.register_pokes[i];
            if at == pc {
                self.gba.set_register(register, value);
            }
        }
        for i in 0..self.pokes.len() {
            let (at, address, value) = self.pokes[i];
            if at == pc {
                self.gba.write32(address, value);
            }
        }
        let heap = HEAP_WORK.load(Relaxed);
        if heap != 0 && (frame_done || HEAP_PCS.iter().any(|p| p.load(Relaxed) == pc)) {
            let ew = self.gba.read32(heap);
            let iw = self.gba.read32(heap + 4);
            if (0x0300_0000..0x0300_8000).contains(&iw) && iw > HEAP_IWRAM_HIGH.load(Relaxed) {
                HEAP_IWRAM_HIGH.store(iw, Relaxed);
            }
            if (0x0200_0000..0x0204_0000).contains(&ew) && ew > HEAP_EWRAM_HIGH.load(Relaxed) {
                HEAP_EWRAM_HIGH.store(ew, Relaxed);
            }
        }
        if let Some(&(_, r0)) = self.returns.iter().find(|(a, _)| *a == pc) {
            self.gba.return_now(r0);
            return Ok(frame_done);
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
            if let Some(args) = self.t.call_args.take() {
                self.gba.set_register(0, args[0]);
                self.gba.set_register(1, args[1]);
            }
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
        let dispatch = [(s.talk, TRIGGER_TALK), (s.touch, TRIGGER_STEP), (s.search, TRIGGER_EXAMINE), (s.psynergy, TRIGGER_PSYNERGY)];
        if let Some(&(_, kind)) = dispatch.iter().find(|(a, _)| *a == pc) {
            self.t.dispatched = Some((kind, self.gba.register(0), self.t.frame));
            self.t.dispatch_return = lr(&self.gba);
            self.t.dispatch_result = None;
        } else if self.t.dispatch_return != 0 && pc == self.t.dispatch_return {
            self.t.dispatch_return = 0;
            self.t.dispatch_result = Some(self.gba.register(0));
            self.t.dispatch_end = self.t.frame;
        }
        if self.t.dispatched.is_some() && !self.watch.is_empty() && self.watch.contains(&pc) {
            if self.t.ran == 0 {
                self.t.ran = pc;
            }
            self.t.own_ran |= self.own.contains(&pc);
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
        if pc == s.speak && self.t.dispatched.is_some() && self.t.absent_speaker == 0 {
            let id = (self.gba.register(0) & 0xfff) as u16;
            // A speaker past the field's objects (0xfff) is none: the window
            // opens in its usual place, beside nobody by design.
            if (PARTY_SPEAKERS..FIELD_ACTOR_COUNT as u16).contains(&id) && self.actor(id).is_none() {
                self.t.absent_speaker = id;
            }
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
            if let Some(mut states) = self.states.take() {
                states.push(self.save());
                self.states = Some(states);
            }
            if self.t.frame > MAX_FRAMES {
                let scene = self.gba.read16(s.game_state + GS_SCENE);
                return Err(format!("gave up at frame {} in scene {scene}", self.t.frame));
            }
        }
        Ok(frame_done)
    }

    /// Runs `n` frames an instruction at a time, and returns the halfwords
    /// at `addresses` each time any of them changes: the frame it changed in
    /// (counted from 0) and their values then, with the picture of the last
    /// whole frame. It stops early when the scene's field work is freed or
    /// replaced (gFieldWork changes), since the addresses are then another
    /// scene's.
    pub fn watch_halfwords(&mut self, addresses: &[u32], n: u32) -> Result<Vec<(u32, Vec<u16>, Vec<u16>)>, String> {
        let read = |field: &Field| addresses.iter().map(|&a| field.gba.read16(a)).collect::<Vec<u16>>();
        let work = self.gba.read32(self.s.field_work);
        let mut last = read(self);
        let mut out = vec![(0, last.clone(), self.picture())];
        let mut frame = 0;
        while frame < n && self.gba.read32(self.s.field_work) == work {
            if self.run_instruction()? {
                frame += 1;
            }
            let now = read(self);
            if now != last {
                out.push((frame, now.clone(), self.picture()));
                last = now;
            }
        }
        Ok(out)
    }

    /// Runs an instruction at a time until VCOUNT reads `line`: 227, the
    /// last line of vertical blank, is after the game's VBlank work has
    /// copied its OAM and registers and before the next picture is drawn.
    pub fn until_line(&mut self, line: u16) -> Result<(), String> {
        // A halted processor passes several lines in one step, and may pass
        // this one; it is met within a few frames, or this fails. Asleep in
        // vertical blank short of the line, the game has done its frame's
        // work: the step is tried, and taken back when it sleeps past the
        // line, so the run stops there instead (a quiet moment, a page
        // waiting, sleeps through the whole blank).
        let vcount = |f: &Self| f.gba.read16(0x0400_0006) & 0xff;
        let mut frames = 0;
        while vcount(self) != line {
            let before = vcount(self);
            if self.t.halted && (VBLANK_FIRST_LINE..line).contains(&before) {
                let asleep = self.save();
                self.run_instruction()?;
                let after = vcount(self);
                if !(before..=line).contains(&after) {
                    self.resume(&asleep);
                    return Ok(());
                }
                continue;
            }
            if self.run_instruction()? {
                frames += 1;
                if frames > 8 {
                    return Err(format!("line {line} was never reached in 8 frames"));
                }
            }
        }
        Ok(())
    }

    pub fn frames(&mut self, mut n: u32) -> Result<(), String> {
        while n > 0 {
            if self.run_instruction()? {
                n -= 1;
            }
        }
        Ok(())
    }

    /// Marks a change made to the run from outside the game (a flag set, a
    /// value written): the states kept after it are on its other side.
    pub fn cut(&mut self) {
        self.t.cuts += 1;
    }

    /// Holds `keys` from now: the machine's keys at once, as a state saved
    /// holding them left them, and from each frame on.
    pub fn hold_now(&mut self, keys: u32) {
        self.t.keys = keys;
        self.gba.set_keys(keys);
    }

    /// Starts keeping the machine's state at the end of every frame, or
    /// stops and returns what was kept.
    pub fn keep_states(&mut self, on: bool) -> Option<super::whole::States> {
        let kept = self.states.take();
        if on {
            self.states = Some(super::whole::States::default());
        }
        kept
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
            if self.run_instruction()? {
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

    /// A photo of an arrival that plays a scene and goes on to another map
    /// before the player has control (the eruption's return to Vale): the
    /// screen half a second before its first message starts typing, or
    /// before it leaves the map, or after `max` frames, whichever comes
    /// first; of those, the last that is not mostly black, as a fade leaves
    /// it.
    pub fn arrival_scene(&mut self, max: u32) -> Result<Image, String> {
        const BEFORE: usize = 30;
        let mut kept: std::collections::VecDeque<Image> = std::collections::VecDeque::new();
        let typed = self.t.last_typing;
        for _ in 0..max {
            self.frames(1)?;
            if self.t.last_typing != typed || self.gba.read16(self.s.game_state + GS_SCENE) != self.setup.scene {
                break;
            }
            kept.push_back(self.screen());
            if kept.len() > 4 * BEFORE {
                kept.pop_front();
            }
        }
        let lit = |image: &Image| image.pixels.iter().filter(|&&p| p & 0xf0_f0_f0 != 0).count() * 2 > image.pixels.len();
        let early = kept.len().saturating_sub(BEFORE);
        kept.iter().take(early.max(1)).rev().find(|i| lit(i)).or_else(|| kept.iter().rev().find(|i| lit(i))).map(|i| Image { width: i.width, height: i.height, pixels: i.pixels.clone() }).ok_or_else(|| "the arrival showed nothing but black".to_string())
    }

    pub fn save(&mut self) -> Saved {
        Saved { state: self.gba.save(), track: self.t }
    }

    pub fn load(&mut self, saved: &Saved) {
        self.gba.load(&saved.state);
        self.t = saved.track;
        self.gba.set_keys(0);
    }

    /// Loads `saved` with the keys it was saved holding still held, so a
    /// moment can be looked at and played on as though it had not been.
    pub fn resume(&mut self, saved: &Saved) {
        self.load(saved);
        self.gba.set_keys(self.t.keys);
    }

    pub fn screen(&self) -> Image {
        Image { width: WIDTH, height: HEIGHT, pixels: self.gba.picture_rgb() }
    }

    /// The live scroll bounds, 16.16 pixels.
    pub fn bounds(&self) -> [i32; 4] {
        let work = self.map_work();
        [0, 1, 2, 3].map(|i| self.gba.read32(work + MAP_BOUNDS + 4 * i) as i32)
    }

    /// The map header's scroll bounds, 16.16 pixels; the whole world on the
    /// world map, which loads no layered map.
    pub fn header(&self) -> [i32; 4] {
        if self.world() {
            return [0, 0, fx(WORLD_CELLS * WORLD_CELL), fx(WORLD_CELLS * WORLD_CELL)];
        }
        self.t.header
    }

    /// Whether the field runs the world map (FieldWork.mode).
    pub fn world(&self) -> bool {
        let work = self.gba.read32(self.s.field_work);
        work != 0 && self.gba.read16(work + FIELD_MODE) == ACTOR_SYSTEM_WORLD_MAP
    }

    /// The side of a map cell in pixels: 16, or the world map's 32.
    pub fn cell_size(&self) -> i32 {
        if self.world() { WORLD_CELL } else { 16 }
    }

    /// The collision code of the cell under (x, z) pixels on map layer
    /// `layer`.
    pub fn code_at(&self, layer: u8, x: i32, z: i32) -> i32 {
        let size = self.cell_size();
        self.cell(layer, x.div_euclid(size), z.div_euclid(size))
    }

    /// Reads every cell of the world map's two layers once: each of its
    /// tiles is unpacked into the party's block buffer as
    /// WorldMap_UpdateCurrentTileBlock does it (WorldMap_WriteLayerCellTile, the second
    /// layer's tiles WORLD_LAYER_TILES on) and read back, and the machine is
    /// then put back as it was.
    fn read_world(&mut self) -> Result<(), String> {
        if self.world_cells.is_some() || !self.world() {
            return Ok(());
        }
        let saved = self.save();
        let work = self.map_work();
        let mut cells = vec![0u32; (2 * WORLD_CELLS * WORLD_CELLS) as usize];
        for layer in 0..2 {
            for ty in 0..WORLD_TILES {
                for tx in 0..WORLD_TILES {
                    let id = u32::from(self.gba.read16(work + PERSPECTIVE_TILE_IDS + 2 * (ty * WORLD_TILES + tx) as u32));
                    let tile = id + layer as u32 * WORLD_LAYER_TILES;
                    // The quarter forgets its tile, so it is unpacked again.
                    let quarter = ((layer * 2 + (ty & 1)) * 2 + (tx & 1)) as u32;
                    self.gba.write16(work + PERSPECTIVE_QUARTER_TILES + 2 * quarter, 0xffff);
                    self.eval(self.s.write_cell_tile, [layer as u32, tx as u32, ty as u32, tile])?;
                    let base = self.s.map_cell_pictures + ((((layer * 2 + (ty & 1)) * 32 + (tx & 1)) << 6) as u32);
                    for r in 0..WORLD_TILE_CELLS {
                        for c in 0..WORLD_TILE_CELLS {
                            let word = self.gba.read32(base + (r * 128 + c * 4) as u32);
                            let at = (layer * WORLD_CELLS + ty * WORLD_TILE_CELLS + r) * WORLD_CELLS + tx * WORLD_TILE_CELLS + c;
                            cells[at as usize] = word;
                        }
                    }
                }
            }
        }
        self.load(&saved);
        self.world_cells = Some(cells);
        Ok(())
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

    /// Stops drawing actor `id` (its object kind 0, which the camera still
    /// follows when it is the leader).
    pub fn hide_actor(&mut self, id: u16) {
        let a = self.actor_object(id);
        if a != 0 {
            self.gba.write8(a + OBJ_KIND, 0);
        }
    }

    /// Moves actor `id` to (x, z) pixels on the map, where it stays until
    /// something moves it.
    pub fn set_actor_position(&mut self, id: u16, x: i32, z: i32) {
        let a = self.actor_object(id);
        if a != 0 {
            self.gba.write32(a + OBJ_X, fx(x) as u32);
            self.gba.write32(a + OBJ_Z, fx(z) as u32);
        }
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
    /// bounds, and drawn (on the world map, wherever the party is); else why
    /// not.
    pub fn present(&self, id: u16) -> Result<Actor, String> {
        let Some(actor) = self.actor(id) else {
            return Err(format!("the scene has no actor {id}"));
        };
        let h = self.header();
        if actor.x == 0 && actor.z == 0 {
            return Err(format!("actor {id} is off stage, at the map's origin"));
        }
        if actor.x < h[0] >> 16 || actor.x >= h[2] >> 16 || actor.z < h[1] >> 16 || actor.z >= h[3] >> 16 {
            return Err(format!("actor {id} is off stage, at ({}, {}) outside the map", actor.x, actor.z));
        }
        // The world map draws only the actors near the party
        // (WorldMap_UpdateAreaContacts).
        if !actor.drawn && !self.world() {
            return Err(format!("actor {id} has no sprite"));
        }
        Ok(actor)
    }

    /// The actors on stage other than the party, with their places (pixels).
    fn stage(&self) -> Vec<(u16, i32, i32)> {
        (FIRST_PLACED..FIELD_ACTOR_COUNT as u16).filter_map(|id| self.present(id).ok().map(|a| (id, a.x, a.z))).collect()
    }

    /// The collision code of cell (cx, cz) of map layer `layer` (its window
    /// onto the cell grid), or -1 off the grid. On the world map the cell is
    /// one of its 32-pixel cells, and only its ground layer has codes.
    pub fn cell(&self, layer: u8, cx: i32, cz: i32) -> i32 {
        if self.world() {
            return if layer == 0 { (self.world_word(0, cx, cz) >> 16) as i32 & 0xff } else { -1 };
        }
        if !(0..GRID).contains(&cx) || !(0..GRID).contains(&cz) || layer > 2 {
            return -1;
        }
        let cells = self.gba.read32(self.map_work() + MAP_LAYER_CELLS + MAP_LAYER_SIZE * u32::from(layer));
        i32::from(self.gba.read8(cells + (((cx + (cz << 7)) << 2) + 2) as u32))
    }

    /// The word of cell (cx, cz) of the world map's layer `layer` (0 or 1):
    /// from the whole map once it has been read, else from the blocks around
    /// the party.
    fn world_word(&self, layer: i32, cx: i32, cz: i32) -> u32 {
        let (cx, cz) = (cx.rem_euclid(WORLD_CELLS), cz.rem_euclid(WORLD_CELLS));
        match &self.world_cells {
            Some(cells) => cells[((layer * WORLD_CELLS + cz) * WORLD_CELLS + cx) as usize],
            None => self.gba.read32(self.s.map_cell_pictures + ((layer << 12) + (((cx & 31) + ((cz & 31) << 5)) << 2)) as u32),
        }
    }

    /// The whole word of cell (cx, cz) of map layer `layer`: its picture
    /// (cell picture), its collision code and its ground; 0 off the grid.
    pub fn cell_word(&self, layer: u8, cx: i32, cz: i32) -> i32 {
        if !(0..GRID).contains(&cx) || !(0..GRID).contains(&cz) || layer > 2 {
            return 0;
        }
        let cells = self.gba.read32(self.map_work() + MAP_LAYER_CELLS + MAP_LAYER_SIZE * u32::from(layer));
        self.gba.read32(cells + ((cx + (cz << 7)) << 2) as u32) as i32
    }

    /// The terrain kind of cell (cx, cz) of map layer `layer` (enum
    /// TerrainKind, the low four bits of its shape's first byte) and its
    /// first height sample in pixels, which is the height everywhere in a
    /// TERRAIN_FLAT cell.
    pub fn shape(&self, layer: u8, cx: i32, cz: i32) -> (u8, i32) {
        let shape = (self.cell_word(layer, cx, cz) as u32) >> 24;
        let at = self.s.map_collision + 4 * shape;
        (self.gba.read8(at) & 15, i32::from(self.gba.read8(at + 1) as i8) * 8)
    }

    /// The cells within the header's bounds, by layer and row, as (layer,
    /// cx, cz); the world map's ground cells, read whole first.
    fn all_cells(&mut self) -> Result<Vec<(u8, i32, i32)>, String> {
        if self.world() {
            self.read_world()?;
            return Ok((0..WORLD_CELLS).flat_map(|cz| (0..WORLD_CELLS).map(move |cx| (0, cx, cz))).collect());
        }
        let h = self.t.header;
        let (x0, z0, x1, z1) = (h[0] >> 20, h[1] >> 20, (h[2] + 0xfffff) >> 20, (h[3] + 0xfffff) >> 20);
        Ok((0..3u8).flat_map(|layer| (z0..z1).flat_map(move |cz| (x0..x1).map(move |cx| (layer, cx, cz)))).collect())
    }

    /// The cells (within the header's bounds, in rows) holding collision
    /// code `code` in any of the three layers, with their layer.
    pub fn cells_with_code(&mut self, code: u16) -> Result<Vec<(u8, i32, i32)>, String> {
        Ok(self.all_cells()?.into_iter().filter(|&(layer, cx, cz)| self.cell(layer, cx, cz) == i32::from(code)).collect())
    }

    /// The cells within the header's bounds holding each of the scene's
    /// event codes, with their layer, in layers and rows.
    pub fn cells_by_code(&mut self) -> Result<HashMap<u16, Vec<(u8, i32, i32)>>, String> {
        let mut out: HashMap<u16, Vec<(u8, i32, i32)>> = HashMap::new();
        for (layer, cx, cz) in self.all_cells()? {
            let code = self.cell(layer, cx, cz);
            if (1..=CELL_EVENT_LAST).contains(&code) {
                out.entry(code as u16).or_default().push((layer, cx, cz));
            }
        }
        Ok(out)
    }

    /// Every cell's word within the header's bounds, layer by layer and row
    /// by row: what an event changes in the map, its pictures (a floor
    /// switch sinking) or its collision (a gate opening). None on the world
    /// map, whose blocks change as the party walks.
    fn cell_codes(&self) -> Vec<i32> {
        if self.world() {
            return Vec::new();
        }
        let h = self.t.header;
        let (x0, z0, x1, z1) = (h[0] >> 20, h[1] >> 20, (h[2] + 0xfffff) >> 20, (h[3] + 0xfffff) >> 20);
        let mut out = Vec::new();
        for layer in 0..3u8 {
            for cz in z0..z1 {
                for cx in x0..x1 {
                    out.push(self.cell_word(layer, cx, cz));
                }
            }
        }
        out
    }

    /// The sides of box `b` (16.16 pixels) whose outermost row or column of
    /// cells holds one cell over and over in all three layers: what the cell
    /// grid beyond a map's own extent shows (top, bottom, left, right).
    pub fn uniform_edges(&self, b: [i32; 4]) -> Vec<&'static str> {
        let (x0, z0, x1, z1) = (b[0] >> 20, b[1] >> 20, ((b[2] + 0xfffff) >> 20) - 1, ((b[3] + 0xfffff) >> 20) - 1);
        if x1 <= x0 || z1 <= z0 {
            return Vec::new();
        }
        let word = |x: i32, z: i32| [0u8, 1, 2].map(|l| self.cell_word(l, x, z));
        let same = |cells: Vec<(i32, i32)>| cells.windows(2).all(|w| word(w[0].0, w[0].1) == word(w[1].0, w[1].1));
        let mut out = Vec::new();
        for (name, cells) in [
            ("top", (x0..=x1).map(|x| (x, z0)).collect::<Vec<_>>()),
            ("bottom", (x0..=x1).map(|x| (x, z1)).collect()),
            ("left", (z0..=z1).map(|z| (x0, z)).collect()),
            ("right", (z0..=z1).map(|z| (x1, z)).collect()),
        ] {
            if same(cells) {
                out.push(name);
            }
        }
        out
    }

    /// The sprites actor object `object` draws, as each animation layer's
    /// sprite id and colour mode: a villager turned into a tree keeps its
    /// place and changes these.
    pub fn sprite_ids(&self, object: u32) -> Vec<i32> {
        let kind = self.gba.read8(object + OBJ_KIND) & 15;
        let first = self.gba.read32(object + OBJ_SPRITE);
        let sprites: Vec<u32> = match kind {
            1 => vec![first],
            2 if first != 0 => (0..4).map(|i| self.gba.read32(first + 4 * i)).collect(),
            _ => Vec::new(),
        };
        let mut out = Vec::new();
        for sprite in sprites.into_iter().filter(|&s| s != 0) {
            for i in 0..4 {
                let entry = self.gba.read32(sprite + ANIM_ENTRIES + 4 * i);
                if entry != 0 {
                    out.push(i32::from(self.gba.read16(entry) as i16));
                    out.push(i32::from(self.gba.read8(entry + ANIM_ENTRY_PARAM)));
                }
            }
        }
        out
    }

    /// What the scene shows now that story flags can change: each placed
    /// actor on stage (its place, map layer and sprites) and every cell's
    /// word (pictures and collision). Two story states that give a room the
    /// same signature look the same in its map.
    pub fn signature(&self) -> Vec<i32> {
        let mut out = Vec::new();
        for id in FIRST_PLACED..FIELD_ACTOR_COUNT as u16 {
            if let Ok(a) = self.present(id) {
                let sprites = self.sprite_ids(self.actor_object(id));
                out.extend([i32::from(id), a.x, a.z, i32::from(a.layer), sprites.len() as i32]);
                out.extend(sprites);
            }
        }
        out.push(-1);
        out.extend(self.cell_codes());
        out
    }

    /// The rows of the scene's event table as it is now, as (kind, target,
    /// result): the table the scene gives for the entrance it was entered by
    /// (the loft's own in Vault's houses). A result is as `result_key` gives
    /// it.
    pub fn triggers(&self) -> Vec<(u8, u16, u32)> {
        let work = self.gba.read32(self.s.field_work);
        let mut a = if work == 0 { 0 } else { self.gba.read32(work + FIELD_EVENTS) };
        let mut out = Vec::new();
        for _ in 0..1024 {
            if a == 0 || self.gba.read32(a) == u32::MAX {
                break;
            }
            out.push(((self.gba.read32(a) & 15) as u8, u16::from(self.gba.read8(a + 4)), result_key(self.gba.read32(a + 8))));
            a += EVENT_ROW_SIZE;
        }
        out
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

    /// Makes the event's row the one the trigger finds: each row before it
    /// for the same trigger whose condition holds is turned off by its flag,
    /// as it is once its own event has been seen (Garet's family's scene
    /// before the line Mind Read hears after it; the loose rock before the
    /// roofer's rescue in Vault), and its own condition is made to hold. The
    /// event's row is the one whose result is one of `own` (its function, or
    /// the message a message row shows), else the one that shows one of
    /// `messages`, else the first whose condition the run's own flags meet,
    /// else the first with none. A flag the run sets or clears itself is left
    /// alone; when that keeps the row from being found, says why.
    fn unshadow(&mut self, kind: u8, target: u16, own: &[u32], messages: &[u32]) -> Result<(), String> {
        let rows = self.event_rows(kind, target);
        let flag = |c: i16| c as u16 & 0xfff;
        let wanted = |c: i16| c as u16 & 0x1000 != 0;
        let met = |c: i16| c != -1 && if wanted(c) { self.setup.set.contains(&flag(c)) } else { self.setup.clear.contains(&flag(c)) };
        let ours = rows
            .iter()
            .position(|r| own.contains(&result_key(r.result)))
            .or_else(|| rows.iter().position(|r| result_key(r.result) < 0x10000 && messages.contains(&result_key(r.result))))
            .or_else(|| rows.iter().position(|r| met(r.condition)))
            .or_else(|| rows.iter().position(|r| r.condition == -1));
        let Some(ours) = ours else {
            return Ok(());
        };
        let (set, clear) = (self.setup.set.clone(), self.setup.clear.clone());
        let mine = |c: i16| set.contains(&flag(c)) || clear.contains(&flag(c));
        let own_condition = rows[ours].condition;
        if own_condition != -1 && !self.condition_holds(own_condition) {
            if mine(own_condition) {
                return Err(format!("its row needs flag {:#05x} {}, which the run's flags rule out", flag(own_condition), if wanted(own_condition) { "set" } else { "clear" }));
            }
            self.set_flag(flag(own_condition), wanted(own_condition));
        }
        for r in &rows[..ours] {
            let c = r.condition;
            if c == -1 || flag(c) == flag(own_condition) {
                continue;
            }
            if self.condition_holds(c) {
                if mine(c) {
                    return Err(format!("an earlier row for its trigger, whose flag {:#05x} the run's flags hold, comes first", flag(c)));
                }
                self.set_flag(flag(c), !wanted(c));
            }
        }
        if rows[..ours].iter().any(|r| r.condition == -1 && r.flags == rows[ours].flags && result_key(r.result) != result_key(rows[ours].result)) {
            return Err("an earlier row for its trigger always runs first".to_string());
        }
        Ok(())
    }

    /// Whether the scene's event table, as it is now, has a row for `kind`
    /// and `target` whose result is one of `own`.
    pub fn has_row(&self, kind: u8, target: u16, own: &[u32]) -> bool {
        self.event_rows(kind, target).iter().any(|r| own.contains(&result_key(r.result)))
    }

    /// Runs the field sequence to its next step (Field_UpdateWalking's entry),
    /// where a game function can be called between two of its calls.
    fn to_step(&mut self) -> Result<(), String> {
        for _ in 0..4_000_000 {
            if self.t.pc == self.s.step && !self.t.calling && self.t.call_target == 0 {
                return Ok(());
            }
            self.run_instruction()?;
        }
        Err("the field sequence did not step".to_string())
    }

    /// Calls the Thumb function at `target` with `args` at the field sequence's
    /// step and returns its r0; the loop goes on as if nothing ran. The
    /// function must only read (MapGrid_GetTerrainHeight).
    fn eval(&mut self, target: u32, args: [u32; 4]) -> Result<u32, String> {
        self.to_step()?;
        let saved: Vec<u32> = [0, 1, 2, 3, LR, SP].iter().map(|&r| self.gba.register(r)).collect();
        for (i, &v) in args.iter().enumerate() {
            self.gba.set_register(i as u32, v);
        }
        self.gba.set_register(LR, self.s.step | 1);
        self.gba.branch(target & !1);
        let mut returned = false;
        for _ in 0..4_000_000 {
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
    /// `layer`, as MapGrid_GetTerrainHeight gives it.
    pub fn ground(&mut self, layer: u8, x: i32, z: i32) -> Result<i32, String> {
        // The world map is flat: the party walks at height 0.
        if self.world() {
            return Ok(0);
        }
        Ok(self.eval(self.s.terrain_height, [u32::from(layer), fx(x) as u32, fx(z) as u32, 0])? as i32)
    }

    /// The leader's object.
    fn leader_object(&self) -> u32 {
        let selected = self.gba.read32(self.s.game_state + GS_SELECTED_ACTOR);
        self.actor_object(selected as u16)
    }

    /// Puts the leader at `spot`: on its layer, standing on its ground and
    /// facing its way.
    pub fn put_leader(&mut self, spot: &Spot) {
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

    /// What is wrong with where the leader stands, for a photo that should
    /// show him where the party can be: without a sprite, off the cell grid, on a wall cell
    /// (a cliff, a wall, water), off his ground, outside his room's camera
    /// box or the screen, on another actor (a stump, a pillar), or on ground
    /// from which he can walk almost nowhere. Empty when nothing is.
    pub fn misplaced(&mut self) -> Result<Vec<String>, String> {
        let leader = self.leader().ok_or("the scene has no leader")?;
        let (x, z) = (leader.x, leader.z);
        let mut out = Vec::new();
        if self.world() {
            self.read_world()?;
            if !self.world_walkable(x, z) {
                out.push(format!("({x}, {z}) is world map ground the party cannot walk on"));
            }
            return Ok(out);
        }
        if !leader.drawn {
            out.push("he has no sprite".to_string());
        }
        let layer = leader.layer.min(2);
        match self.code_at(layer, x, z) {
            -1 => out.push(format!("({x}, {z}) is off the cell grid")),
            CELL_WALL => out.push(format!("({x}, {z}) is a wall cell of layer {layer}")),
            _ => {}
        }
        let ground = self.ground(layer, x, z)?;
        if (leader.y - ground).abs() > 2 << 16 {
            out.push(format!("he stands {} pixels off the ground", (leader.y - ground) >> 16));
        }
        let room = self.room();
        if x < room[0] || x >= room[2] || z < room[1] || z >= room[3] {
            out.push(format!("({x}, {z}) is outside his room's camera box {room:?}"));
        }
        let (vx, vy) = self.view();
        let (sx, sy) = (x - vx, z - (leader.y >> 16) - vy);
        if !(0..WIDTH as i32).contains(&sx) || !(0..HEIGHT as i32).contains(&sy) {
            out.push(format!("he is off the screen, at ({sx}, {sy}) on it"));
        } else {
            // The screen within 24 pixels of his feet, his own sprite
            // left out, is black where the map draws nothing.
            let screen = self.screen();
            let (mut black, mut all) = (0, 0);
            for py in (sy - 24).max(0)..(sy + 24).min(HEIGHT as i32) {
                for px in (sx - 24).max(0)..(sx + 24).min(WIDTH as i32) {
                    if (px - sx).abs() < 10 && py > sy - 28 && py < sy + 4 {
                        continue;
                    }
                    all += 1;
                    if screen.pixels[py as usize * WIDTH + px as usize] & 0xf0_f0_f0 == 0 {
                        black += 1;
                    }
                }
            }
            if all > 0 && black * 10 > all * 8 {
                out.push(format!("he stands in black, {}% of the screen around him", black * 100 / all));
            }
        }
        for (id, ax, az) in self.stage() {
            if (ax - x).abs() < 14 && (az - z).abs() < 14 {
                out.push(format!("he stands on actor {id}, at ({ax}, {az})"));
            }
        }
        let reach = self.reach(None)?;
        let cells = reach.cells.iter().filter(|&&c| c).count();
        if cells < 4 {
            out.push(format!("he can walk to {cells} cells from there"));
        }
        Ok(out)
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
        // A floor switch the leader stands on sinks under him by two
        // pixels (Mercury Lighthouse).
        if (leader.y - ground).abs() > 2 << 16 {
            return Err(format!("the leader stands {} pixels off the ground", (leader.y - ground) >> 16));
        }
        Ok(())
    }

    /// Whether the leader can stand on a cell with collision code `code`
    /// without starting anything: open ground, encounter ground, or a ledge
    /// or jump cell, which does nothing until he pushes on from it.
    fn standable(code: i32) -> bool {
        code >= 0 && code != CELL_WALL && code != CELL_CLIMB && !(1..=CELL_EVENT_LAST).contains(&code)
    }

    /// Whether standing on a cell with collision code `code` starts nothing:
    /// open ground, or a code of the scene's events that no touch or exit
    /// row answers while the flags are as they are (a warning given once).
    fn quiet(&self, code: i32) -> bool {
        let idle = |kind| self.event_rows(kind, code as u16).iter().all(|r| !self.condition_holds(r.condition));
        Self::standable(code) || ((1..=CELL_EVENT_LAST).contains(&code) && idle(TRIGGER_STEP) && idle(TRIGGER_EXIT))
    }

    /// Whether the leader walking on crosses a cell with collision code
    /// `code`: not an exit's cell while an exit row for it holds, nor one
    /// with the code `target` being played.
    fn crossable(&self, code: i32, target: Option<u16>) -> bool {
        if !(1..=CELL_EVENT_LAST).contains(&code) {
            return true;
        }
        target != Some(code as u16) && self.event_rows(TRIGGER_EXIT, code as u16).iter().all(|r| !self.condition_holds(r.condition))
    }

    /// Whether (x, z) pixels lie within the map's bounds, `margin` pixels in.
    fn inside(&self, x: i32, z: i32, margin: i32) -> bool {
        let h = self.header();
        x >= (h[0] >> 16) + margin && x < (h[2] >> 16) - margin && z >= (h[1] >> 16) + margin && z < (h[3] >> 16) - margin
    }

    /// The room the run entered: its entrance's camera box in pixels, the
    /// header's sides where the entrance gives none. The camera never leaves
    /// it, so a leader outside it is off the screen.
    pub fn room(&self) -> [i32; 4] {
        let entrances = self.entrances();
        let entrance = entrances.iter().find(|e| e.number as u16 == self.setup.entrance).or_else(|| entrances.first());
        let h = self.header();
        let side = |i: usize| match entrance {
            Some(e) if e.camera[i] != -1 => i32::from(e.camera[i]),
            _ => h[i] >> 16,
        };
        [side(0), side(1), side(2), side(3)]
    }

    /// Whether the leader can walk from cell `a` into the next cell the way
    /// `d` (an index of STEP) on map layer `layer`, as Actor_UpdatePlayerControl
    /// lets him: MapGrid_CheckCollision passes the ground 8 pixels ahead of him
    /// all the way.
    fn passable(&mut self, layer: u8, a: (i32, i32), d: usize) -> Result<bool, String> {
        if self.world() {
            let (dx, dz) = STEP[d];
            return Ok([8, 16].iter().all(|along| self.world_walkable(a.0 * 16 + 8 + dx * along, a.1 * 16 + 8 + dz * along)));
        }
        // The player control tests the ground 8 pixels ahead of the leader
        // against his own: walking from a's centre to b's, every point 4
        // pixels apart is a step from the one 8 pixels behind it.
        let (dx, dz) = STEP[d];
        let mut heights = [0; 5];
        for (i, h) in heights.iter_mut().enumerate() {
            let along = 4 * i as i32;
            *h = self.ground(layer, a.0 * 16 + 8 + dx * along, a.1 * 16 + 8 + dz * along)?;
        }
        Ok((0..3).all(|i| (-STEP_DOWN..=STEP_UP).contains(&(heights[i + 2] - heights[i]))))
    }

    /// The cells the leader can walk to from where he stands, on his layer,
    /// within his room: across open ground, encounter ground and event
    /// cells, down ledges, along climbs and over jumps, never through a wall
    /// or a break in the ground higher than a step, and onto but not across
    /// an exit or a cell with code `target`, the touch being played.
    pub fn reach(&mut self, target: Option<u16>) -> Result<Reach, String> {
        let selected = self.gba.read32(self.s.game_state + GS_SELECTED_ACTOR) as u16;
        let leader = self.actor(selected).ok_or("the scene has no leader")?;
        if self.world() {
            return self.reach_world(&leader, target);
        }
        let layer = leader.layer.min(2);
        let room = self.room();
        let h = self.t.header;
        let lo = |i: usize| (room[i].max(h[i] >> 16)) >> 4;
        let hi = |i: usize| (room[i].min(h[i] >> 16) + 15) >> 4;
        let (x0, z0, x1, z1) = (lo(0), lo(1), hi(2), hi(3));
        let within = |c: (i32, i32)| c.0 >= x0 && c.0 < x1 && c.1 >= z0 && c.1 < z1 && (0..GRID).contains(&c.0) && (0..GRID).contains(&c.1);
        let mut cells = vec![false; (GRID * GRID) as usize];
        let index = |c: (i32, i32)| (c.1 * GRID + c.0) as usize;
        let start = (leader.x >> 4, leader.z >> 4);
        let mut queue = std::collections::VecDeque::new();
        if within(start) {
            cells[index(start)] = true;
            queue.push_back(start);
        }
        // Whether the walk crosses cells with a code, by code.
        let mut crossed: HashMap<i32, bool> = HashMap::new();
        let mut stops = Vec::new();
        while let Some(c) = queue.pop_front() {
            let here = self.cell(layer, c.0, c.1);
            // An exit's cell, and one with the code of the touch being
            // played, are reached but not crossed: stepping on it would
            // leave or start the event, so what lies only beyond it (the
            // dark behind a door, the way behind the crate in Vault's inn) is
            // not his to stand on. Other events' cells are crossed, as the
            // player crosses them once their story has played.
            if c != start && !*crossed.entry(here).or_insert_with(|| self.crossable(here, target)) {
                if !stops.contains(&here) {
                    stops.push(here);
                }
                continue;
            }
            for d in 0..4 {
                let n = (c.0 + STEP[d].0, c.1 + STEP[d].1);
                if !within(n) {
                    continue;
                }
                let code = self.cell(layer, n.0, n.1);
                if !cells[index(n)] && code >= 0 && code != CELL_WALL && self.passable(layer, c, d)? {
                    cells[index(n)] = true;
                    queue.push_back(n);
                }
                // Standing on a ledge, a climb or a jump cell and pushing on
                // (Field_RunPushCell) takes him over what is ahead: down
                // a ledge with Down held, up or down a climb, across a jump
                // (Field_TryJumpForward: over a wall or a drop, never ground
                // more than a step higher, onto ground within a step). He
                // lands on the first cell beyond that is ground, no more
                // than JUMP_DROP below where he took off; a climb goes up as
                // far.
                let way = match here {
                    CELL_LEDGE => d == 2,
                    CELL_CLIMB => d == 0 || d == 2,
                    CELL_JUMP => true,
                    _ => false,
                };
                if !way {
                    continue;
                }
                let from = self.ground(layer, c.0 * 16 + 8, c.1 * 16 + 8)?;
                let (reach_cells, up) = match here {
                    CELL_JUMP => (2..=2, STEP_UP),
                    CELL_LEDGE => (1..=4, STEP_UP),
                    _ => (1..=8, JUMP_DROP),
                };
                if here == CELL_JUMP {
                    let over = self.ground(layer, n.0 * 16 + 8, n.1 * 16 + 8)?;
                    if code != CELL_WALL && over - from > STEP_UP {
                        continue;
                    }
                }
                for k in reach_cells {
                    let m = (c.0 + STEP[d].0 * k, c.1 + STEP[d].1 * k);
                    let beyond = self.cell(layer, m.0, m.1);
                    if !within(m) || beyond < 0 {
                        break;
                    }
                    if beyond == CELL_WALL || (beyond == here && here != CELL_JUMP) {
                        continue;
                    }
                    let rise = self.ground(layer, m.0 * 16 + 8, m.1 * 16 + 8)? - from;
                    if (-JUMP_DROP..=up).contains(&rise) {
                        if !cells[index(m)] {
                            cells[index(m)] = true;
                            queue.push_back(m);
                        }
                        break;
                    }
                }
            }
        }
        Ok(Reach { layer, cells, room, stops, origin: (0, 0), width: GRID })
    }

    /// The world map's collision value at (x, z) pixels, as
    /// WorldMap_GetCollision reads it: 4-bit values in 16-pixel squares, from
    /// the upper layer where it has one, else the lower, else
    /// WORLD_MAP_COLLISION_DEFAULT. The game reads each square's collision
    /// tile from the BG maps WorldMap_WriteLayerCellTile draws around the party (a
    /// cell's block in gMapGridCellBuffer gives its four squares' tiles, top two
    /// first); here it comes from the cells of the whole map.
    fn world_collision(&self, x: i32, z: i32) -> u32 {
        let (hx, hz) = (x >> 1, z >> 1);
        let offset = ((((hz >> 1) & 3) << 1) + ((hx >> 2) & 1)) as u32;
        let square = (((z >> 4) & 1) * 2 + ((x >> 4) & 1)) as u32;
        for (layer, tiles) in [(1, 0x800), (0, 0)] {
            let block = self.world_word(layer, x.div_euclid(WORLD_CELL), z.div_euclid(WORLD_CELL)) & 0xffff;
            let number = u32::from(self.gba.read8(self.s.map_cell_buffer + block * 4 + square));
            let packed = self.gba.read8(self.s.map_collision + tiles + number * 8 + offset);
            let value = if hx & 2 != 0 { packed >> 4 } else { packed & 15 };
            if value != 0 {
                return u32::from(value);
            }
        }
        WORLD_COLLISION_DEFAULT
    }

    /// Whether the party can walk at (x, z) pixels on the world map
    /// (WorldMap_IsWalkable).
    fn world_walkable(&self, x: i32, z: i32) -> bool {
        WORLD_WALKABLE.contains(&self.world_collision(x, z))
    }

    /// The world map's Field::reach: the 16-pixel squares whose ground the
    /// world map's collision lets the leader walk to from where he stands,
    /// crossing no exit's cell nor one with code `target`.
    fn reach_world(&mut self, leader: &Actor, target: Option<u16>) -> Result<Reach, String> {
        self.read_world()?;
        let width = WORLD_CELLS * WORLD_CELL / 16;
        let origin = (0, 0);
        let mut cells = vec![false; (width * width) as usize];
        let index = |c: (i32, i32)| ((c.1 - origin.1) * width + c.0 - origin.0) as usize;
        let within = |c: (i32, i32)| (0..width).contains(&(c.0 - origin.0)) && (0..width).contains(&(c.1 - origin.1));
        let start = (leader.x >> 4, leader.z >> 4);
        let mut queue = std::collections::VecDeque::from([start]);
        cells[index(start)] = true;
        let mut crossed: HashMap<i32, bool> = HashMap::new();
        let mut stops = Vec::new();
        while let Some(c) = queue.pop_front() {
            let here = self.code_at(0, c.0 * 16 + 8, c.1 * 16 + 8);
            if c != start && !*crossed.entry(here).or_insert_with(|| self.crossable(here, target)) {
                if !stops.contains(&here) {
                    stops.push(here);
                }
                continue;
            }
            for d in 0..4 {
                let n = (c.0 + STEP[d].0, c.1 + STEP[d].1);
                if within(n) && !cells[index(n)] && self.passable(0, c, d)? {
                    cells[index(n)] = true;
                    queue.push_back(n);
                }
            }
        }
        let h = self.header();
        Ok(Reach { layer: 0, cells, room: [h[0] >> 16, h[1] >> 16, h[2] >> 16, h[3] >> 16], stops, origin, width })
    }

    /// Loads `from` with the view's top left pinned at `view` (16.16 pixels)
    /// inside the scroll bounds `b`, which may reach past the header's, and
    /// the layers' screen maps already drawn there, so the game runs on from
    /// `from` as it would have, objects and all, until unpin_view: views
    /// pinned from one saved moment and run the same frames show the same
    /// moment. The maps are drawn whole (MapGrid_CenterOnFollowedPoint) in a
    /// run from `from` that is then put back, because drawing them takes the
    /// game more than a frame and would leave it a frame behind; their
    /// screen maps (BG_SCREEN_BLOCK(5) to (7)) and the layers' positions are
    /// carried over. The view and the layers' BG scroll values (in
    /// `bg_scroll`, gBgScroll) are set as MapGrid_UpdateLayerScroll would set
    /// them there, unshaken, for a frame in which the game does not scroll.
    pub fn pin_view(&mut self, from: &Saved, view: (i32, i32), b: [i32; 4], bg_scroll: u32) -> Result<(), String> {
        // The maps are drawn within the map's own bounds: drawn from above its
        // top, MapGrid_SetCameraCenter would place its rows a cell out
        // (its tile numbers round toward zero). The view pinned past an edge
        // then shows the map's rows and columns where they are, and only
        // what lies past the edge is not the map.
        let pin = |field: &mut Field, within: bool| {
            field.load(from);
            let work = field.map_work();
            field.t.focus = [view.0 + fx(120), 0, view.1 + fx(96)];
            field.t.pin_focus = true;
            let live = field.bounds();
            for (i, &v) in b.iter().enumerate() {
                let v = match (within, i) {
                    (false, _) => v,
                    (true, 0 | 1) => v.max(live[i]),
                    (true, _) => v.min(live[i]),
                };
                field.gba.write32(work + MAP_BOUNDS + 4 * i as u32, v as u32);
            }
            work
        };
        let work = pin(self, true);
        self.t.arm_center = true;
        let mut ran = 0;
        while self.t.arm_center {
            self.frames(1)?;
            ran += 1;
            if ran > PIN_DRAW_FRAMES {
                return Err("MapGrid_UpdateLayerScroll never ran".to_string());
            }
        }
        // The drawing may run past the frame it started in.
        self.frames(1)?;
        let maps: Vec<u32> = (0..PIN_SCREEN_MAP_BYTES / 4).map(|i| self.gba.read32(PIN_SCREEN_MAPS + 4 * i)).collect();
        let layers: Vec<u32> = (0..3).flat_map(|l| [0, 4].map(|o| self.gba.read32(work + MAP_LAYERS + MAP_LAYER_SIZE * l + o))).collect();
        pin(self, false);
        for (i, &word) in maps.iter().enumerate() {
            self.gba.write32(PIN_SCREEN_MAPS + 4 * i as u32, word);
        }
        for (i, &word) in layers.iter().enumerate() {
            self.gba.write32(work + MAP_LAYERS + MAP_LAYER_SIZE * (i as u32 / 2) + 4 * (i as u32 % 2), word);
        }
        self.gba.write32(work + MAP_VIEW_X, view.0 as u32);
        self.gba.write32(work + MAP_VIEW_Y, view.1 as u32);
        for l in 0..3u32 {
            let at = work + MAP_LAYERS + MAP_LAYER_SIZE * l;
            let word = |o: u32| self.gba.read32(at + o) as i32;
            // Each axis: the view times the layer's parallax, wrapped with its
            // drift when it drifts, plus its offset; BG3 is the first layer.
            let axis = |v: i32, k: u32| {
                let mut base = ((i64::from(v) * i64::from(word(LAYER_SCALE + 4 * k))) >> 16) as i32;
                if word(LAYER_SPEED + 4 * k) != 0 {
                    let mask = i32::from(self.gba.read16(at + LAYER_MASK + 2 * k));
                    base = (base + word(LAYER_PHASE + 4 * k)) & ((mask << 19) | 0x7ffff);
                }
                base + word(LAYER_OFFSET + 4 * k)
            };
            let (x, y) = (axis(view.0, 0), axis(view.1, 1));
            self.gba.write16(bg_scroll + 4 * (3 - l), (x >> 16) as u16);
            self.gba.write16(bg_scroll + 4 * (3 - l) + 2, (y >> 16) as u16);
        }
        Ok(())
    }

    /// Lets the view follow the camera again, and returns the view the map
    /// work last drew from (its top left, pixels).
    pub fn unpin_view(&mut self) -> (i32, i32) {
        self.t.pin_focus = false;
        let work = self.map_work();
        (self.gba.read32(work + MAP_VIEW_X) as i32 >> 16, self.gba.read32(work + MAP_VIEW_Y) as i32 >> 16)
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
                    return Err("MapGrid_UpdateLayerScroll never ran".to_string());
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
    /// there is nowhere. Every spot is one he could walk to in his room
    /// (Field::reach), within the map, on ground that starts nothing, with
    /// the ground's height there. Beside an actor he talks to or casts on, he
    /// stands level with it within the reach (20 pixels south, west, east or
    /// north of it, then the diagonals and further, across a counter), facing
    /// it, and spots where another actor is the nearer target come last. A
    /// cell he searches is 16 pixels ahead of him, any of eight ways. A cell
    /// he touches he walks onto from a step away, the way its row faces
    /// first; one he pushes against he pushes from the cell itself, toward
    /// an actor beyond it first (a statue, a log), then a wall. Move's and
    /// Carry's spots each come with the d-pad direction that pushes the actor: away from
    /// the leader first, then either side, then toward him.
    ///
    /// With `relaxed`, when the leader can walk to none of them (what opens
    /// the way is another event's, a log floated or a pillar pushed, that the
    /// run does not play first), the spots are those he could stand on
    /// anywhere in his room, and the event's recording says so.
    pub fn spots(&mut self, trigger: Trigger) -> Result<Vec<Spot>, String> {
        self.unwalked = false;
        let reach = self.reach(match trigger {
            Trigger::Touch(code) => Some(code),
            _ => None,
        })?;
        match self.spots_in(trigger, &reach) {
            Err(e) if self.relaxed && e.contains("cannot walk") => {
                let anywhere = reach.anywhere();
                let spots = self.spots_in(trigger, &anywhere).map_err(|_| e)?;
                self.unwalked = true;
                Ok(spots)
            }
            spots => spots,
        }
    }

    fn spots_in(&mut self, trigger: Trigger, reach: &Reach) -> Result<Vec<Spot>, String> {
        const SIDES: [usize; 4] = [2, 3, 1, 0];
        let step = |rise: i32| (-STEP_DOWN..=STEP_UP).contains(&rise);
        // The actors on stage other than the party, which a talk or a cast
        // may find first and a push may roll.
        let others: Vec<(u16, Actor)> =
            (FIRST_PLACED..FIELD_ACTOR_COUNT as u16).filter_map(|id| self.present(id).ok().map(|a| (id, a))).collect();
        let mut out = Vec::new();
        let mut later = Vec::new();
        // Spots dropped because the leader cannot walk there.
        let mut unreached = 0;
        match trigger {
            Trigger::Psynergy(ability, id) => {
                // Field_FindNearbyPsynergyTarget: the actor within the
                // Psynergy's range and height of the leader, ahead of him
                // with his facing rounded to a quarter turn.
                let (range, height) = psynergy_reach(ability);
                let actor = self.present(id)?;
                let distances: Vec<i32> = [20, range - 4, range / 2 + 4].into_iter().filter(|&d| d > 8 && d < range).collect();
                let mut ranked: Vec<(usize, Spot)> = Vec::new();
                for (li, layer) in beside_layers(reach.layer, actor.layer).into_iter().enumerate() {
                    for &distance in &distances {
                        for side in SIDES {
                            let (x, z) = (actor.x + STEP[side].0 * distance, actor.z + STEP[side].1 * distance);
                            let code = self.code_at(layer, x, z);
                            if !self.inside(x, z, 8) || code < 0 || code == CELL_WALL || code == CELL_CLIMB {
                                continue;
                            }
                            if !reach.holds(layer, x, z) {
                                unreached += 1;
                                continue;
                            }
                            let ground = self.ground(layer, x, z)?;
                            if (actor.y - ground).abs() > height {
                                continue;
                            }
                            let toward = (side + 2) & 3;
                            let pushes: Vec<u32> =
                                if ability == MOVE || ability == CARRY { [toward, (toward + 1) & 3, (toward + 3) & 3, side].map(|d| KEYS[d]).to_vec() } else { vec![0] };
                            let first = self.quiet(code) && !nearer(&others, id, &actor, x, z, FACING[toward], range, 0x1800);
                            for walk in pushes {
                                let spot = Spot { x, z, layer, height: ground, facing: FACING[toward], walk, cell: None };
                                ranked.push((2 * li + usize::from(!first), spot));
                            }
                        }
                    }
                }
                ranked.sort_by_key(|(rank, _)| *rank);
                out.extend(ranked.into_iter().map(|(_, spot)| spot));
                if out.is_empty() {
                    return Err(match unreached {
                        0 => format!("the ground within reach of actor {id} at ({}, {}) is walled, or more than {} pixels above or below it", actor.x, actor.z, height >> 16),
                        _ => format!("the leader cannot walk to any ground within reach of actor {id} at ({}, {}){}", actor.x, actor.z, reach.crossing()),
                    });
                }
            }
            Trigger::Talk(id) => {
                // Field_FindNearbyTalkTarget reaches 32 pixels, 41 for an
                // actor with collision flag 4; the leader faces the actor.
                // Open ground comes first; ground with a code of the scene's
                // events, which would start its own event first, last.
                let actor = self.present(id)?;
                let mut ranked: Vec<(usize, Spot)> = Vec::new();
                for (li, layer) in beside_layers(reach.layer, actor.layer).into_iter().enumerate() {
                    for distance in [20, 28, 34] {
                        for (dx, dz) in AROUND {
                            let (x, z) = if dx != 0 && dz != 0 {
                                (actor.x + dx * distance * 707 / 1000, actor.z + dz * distance * 707 / 1000)
                            } else {
                                (actor.x + dx * distance, actor.z + dz * distance)
                            };
                            let code = self.code_at(layer, x, z);
                            if !self.inside(x, z, 8) || code < 0 || code == CELL_WALL || code == CELL_CLIMB {
                                continue;
                            }
                            if !reach.holds(layer, x, z) {
                                unreached += 1;
                                continue;
                            }
                            let ground = self.ground(layer, x, z)?;
                            if !step(actor.y - ground) {
                                continue;
                            }
                            let facing = facing_toward(actor.x - x, actor.z - z);
                            let spot = Spot { x, z, layer, height: ground, facing, walk: 0, cell: None };
                            let first = self.quiet(code) && !nearer(&others, id, &actor, x, z, facing, 41, 0x2fff);
                            ranked.push((2 * li + usize::from(!first), spot));
                        }
                    }
                }
                ranked.sort_by_key(|(rank, _)| *rank);
                out.extend(ranked.into_iter().map(|(_, spot)| spot));
                if out.is_empty() {
                    return Err(match unreached {
                        0 => format!("no ground beside actor {id} at ({}, {}) is level with it", actor.x, actor.z),
                        _ => format!("the leader cannot walk to the ground beside actor {id} at ({}, {}){}", actor.x, actor.z, reach.crossing()),
                    });
                }
            }
            Trigger::Search(code) => {
                // The leader searches the ground layer's cell 16 pixels ahead
                // of him, whichever way he faces
                // (Actor_GetTriggerCellAheadOfCurrent). He stands on his own
                // layer, level with the cell or up to 64 pixels below it (a
                // jar on a shelf).
                let cells: Vec<_> = self.cells_with_code(code)?.into_iter().filter(|c| c.0 == 0).collect();
                let size = self.cell_size();
                if cells.is_empty() {
                    return Err(format!("no ground-layer cell holds code {code}"));
                }
                let layer = reach.layer;
                // From the cell to the leader: south, west, east and north,
                // then the diagonals; 16 pixels, or 11 each way.
                const BACK: [(i32, i32); 8] = [(0, 16), (-16, 0), (16, 0), (0, -16), (-11, 11), (11, 11), (-11, -11), (11, -11)];
                for (_, cx, cz) in cells {
                    let (tx, tz) = (cx * size + size / 2, cz * size + size / 2);
                    let target = self.ground(layer, tx, tz)?;
                    for (bx, bz) in BACK {
                        let (x, z) = (tx + bx, tz + bz);
                        if !self.inside(x, z, 0) || !self.quiet(self.code_at(layer, x, z)) {
                            continue;
                        }
                        if !reach.holds(layer, x, z) {
                            unreached += 1;
                            continue;
                        }
                        let ground = self.ground(layer, x, z)?;
                        if (-STEP_DOWN..=64 << 16).contains(&(target - ground)) {
                            let facing = facing_toward(-bx, -bz);
                            let spot = Spot { x, z, layer, height: ground, facing, walk: 0, cell: Some((cx, cz)) };
                            if bx == 0 || bz == 0 { &mut out } else { &mut later }.push(spot);
                        }
                    }
                }
                out.extend(later);
                if out.is_empty() {
                    return Err(match unreached {
                        0 => format!("no open ground faces a cell with code {code}"),
                        _ => format!("the leader cannot walk to any ground facing a cell with code {code}{}", reach.crossing()),
                    });
                }
            }
            Trigger::Touch(code) => {
                // A row that needs pushing (STEP_TRIGGER_FLAG_WALKING) is pushed
                // from the cell itself, the way the row faces when it names
                // one (TRIGGER_FLAG_FACING); any other is walked onto, that
                // way first.
                let rows = self.event_rows(TRIGGER_STEP, code);
                let pushing = rows.iter().any(|r| r.flags & STEP_TRIGGER_FLAG_WALKING != 0);
                let facing = rows.iter().find(|r| r.flags & TRIGGER_FLAG_FACING != 0).map(|r| r.flags & 0xf000);
                let mut ways = vec![0, 2, 3, 1];
                if let Some(f) = facing {
                    ways.sort_by_key(|&d| (FACING[d] as u16).wrapping_sub(f as u16).wrapping_add(0x1fff) > 0x3ffe);
                }
                let cells = self.cells_with_code(code)?;
                let size = self.cell_size();
                // The world map's places are actors the leader walks into
                // (WorldMap_UpdateAreaContacts), not cells.
                if cells.is_empty() && self.world() && code >= WORLD_CONTACT_BASE {
                    return self.contact_spots(code - WORLD_CONTACT_BASE, reach);
                }
                if cells.is_empty() {
                    return Err(format!("no cell holds code {code}"));
                }
                // Pushes go toward what lies in the next cell first: an
                // actor (a statue, a crate, a log whose footprint covers
                // it), then a wall.
                let mut pushed: Vec<(i32, Spot)> = Vec::new();
                let mut walked = Vec::new();
                for (layer, cx, cz) in cells {
                    let (tx, tz) = (cx * size + size / 2, cz * size + size / 2);
                    if !reach.holds(layer, tx, tz) && !STEP.iter().any(|s| reach.holds(layer, tx - s.0 * size, tz - s.1 * size)) {
                        unreached += 1;
                        continue;
                    }
                    let target = self.ground(layer, tx, tz)?;
                    if pushing && self.inside(tx, tz, 0) && reach.holds(layer, tx, tz) {
                        let pushes = if facing.is_some() { &ways[..1] } else { &ways[..] };
                        for &d in pushes {
                            let (nx, nz) = (tx + STEP[d].0 * 16, tz + STEP[d].1 * 16);
                            let near = others.iter().map(|(_, a)| (a.x - nx).abs().max((a.z - nz).abs())).min().unwrap_or(i32::MAX);
                            let rank = if near <= 12 {
                                0
                            } else if near <= 24 {
                                1
                            } else if self.code_at(layer, nx, nz) == CELL_WALL {
                                2
                            } else {
                                3
                            };
                            pushed.push((rank, Spot { x: tx, z: tz, layer, height: target, facing: FACING[d], walk: KEYS[d], cell: Some((cx, cz)) }));
                        }
                    }
                    for (rank, &d) in ways.iter().enumerate() {
                        let rank = if facing.is_some() { rank } else { 0 };
                        let (x, z) = (tx - STEP[d].0 * size, tz - STEP[d].1 * size);
                        if !self.inside(x, z, 0) || !self.quiet(self.code_at(layer, x, z)) || !reach.holds(layer, x, z) {
                            continue;
                        }
                        let ground = self.ground(layer, x, z)?;
                        if self.passable(layer, (x >> 4, z >> 4), d)? {
                            walked.push((rank, Spot { x, z, layer, height: ground, facing: FACING[d], walk: KEYS[d], cell: Some((cx, cz)) }));
                        }
                    }
                }
                // Cells on the layer the leader walks come first: he stands on
                // another only where a scene puts him there (a bridge).
                pushed.sort_by_key(|(rank, s)| (s.layer != reach.layer, *rank));
                // The way the row faces first, then spots in the open.
                walked.sort_by_key(|(rank, s)| (s.layer != reach.layer, *rank, std::cmp::Reverse(reach.open(s.layer, s.x, s.z))));
                out.extend(pushed.into_iter().map(|(_, spot)| spot));
                out.extend(walked.into_iter().map(|(_, spot)| spot));
                if out.is_empty() {
                    return Err(match unreached {
                        0 => format!("no open ground a step from a cell with code {code}"),
                        _ => format!("the leader cannot walk to a cell with code {code} or a step from one{}", reach.crossing()),
                    });
                }
            }
        }
        Ok(out)
    }

    /// Where the leader stands to walk into world map actor `id`, a place:
    /// a little way south, west, east or north of it, walking toward it,
    /// nearer first. WorldMap_UpdateAreaContacts raises the place's
    /// touch (WORLD_CONTACT_BASE + its actor) once he is close enough.
    fn contact_spots(&mut self, id: u16, reach: &Reach) -> Result<Vec<Spot>, String> {
        let actor = self.present(id)?;
        let mut out = Vec::new();
        let mut unreached = 0;
        // South, west, east and north of it, then the diagonals, which the
        // world map's d-pad walks too (gWorldMap_KeyHeadings); only where
        // the way to it is ground he walks on.
        for distance in [48, 64, 96] {
            for (dx, dz) in AROUND {
                let along = if dx != 0 && dz != 0 { distance * 707 / 1000 } else { distance };
                let (x, z) = (actor.x + dx * along, actor.z + dz * along);
                if !self.inside(x, z, 8) || !self.quiet(self.code_at(0, x, z)) {
                    continue;
                }
                if !reach.holds(0, x, z) {
                    unreached += 1;
                    continue;
                }
                let open = (0..along / 8).all(|k| self.world_walkable(x - dx * 8 * k, z - dz * 8 * k));
                if !open {
                    continue;
                }
                let key = |d: i32, less: u32, more: u32| match d {
                    1 => less,
                    -1 => more,
                    _ => 0,
                };
                // Toward the actor: against the side it lies on.
                let walk = key(dx, KEY_LEFT, KEY_RIGHT) | key(dz, KEY_UP, KEY_DOWN);
                out.push(Spot { x, z, layer: 0, height: 0, facing: facing_toward(-dx, -dz), walk, cell: None });
            }
        }
        if out.is_empty() {
            return Err(match unreached {
                0 => format!("no open ground lies beside world map actor {id} at ({}, {})", actor.x, actor.z),
                _ => format!("the leader cannot walk to the ground beside world map actor {id} at ({}, {}){}", actor.x, actor.z, reach.crossing()),
            });
        }
        Ok(out)
    }

    /// Plays an event from `start` with the leader at `spot`, until the
    /// player has control again. Keeps up to `max_pages` pages as stills;
    /// with `record`, also every frame from just before the trigger, up to
    /// `record` frames, holding each page PAGE_HOLD frames so a GIF shows it.
    /// `own` are the results of the event's own row (its function, or the
    /// message a message row shows), when they are known: the row must be in
    /// the scene's table, and the run says which row's function ran.
    /// Fails when the leader does not stay at the spot or another event
    /// begins.
    pub fn event(
        &mut self,
        start: Start,
        trigger: Trigger,
        spot: &Spot,
        own: &[u32],
        messages: &[u32],
        max_pages: usize,
        record: Option<usize>,
    ) -> Result<EventRun, String> {
        // The actors on stage as the leader is put at the spot: those still
        // in place when the trigger comes are watched for the event moving
        // them; those that move on their own meanwhile (a sparkle, a
        // wanderer) are not.
        let early = match start {
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
                let early = self.stage();
                self.frames(SETTLE)?;
                early
            }
            Start::InControl(saved) => {
                self.load(saved);
                self.put_leader(spot);
                let early = self.stage();
                self.frames(SETTLE_MOVED)?;
                early
            }
        };
        self.check_leader(spot)?;
        if record.is_some() {
            self.recorded = Some(Vec::new());
            self.keep_states(self.whole);
        }
        self.frames(LEAD_IN)?;
        let cap = record.unwrap_or(EVENT_LIMIT as usize);
        self.t.dispatched = None;
        let trigger_frame = self.t.frame;
        self.t.absent_speaker = 0;
        let loads = self.t.loads;
        let entries = self.t.entries;
        let mut left = false;
        // The scene the party is placed in next, when it is placed again
        // before any battle (a staircase to another entrance of the same
        // scene included), and, while recorded, the frame it began loading
        // another scene: the arrival is followed from there.
        let mut leads_to: Option<u16> = None;
        let mut arrival: Option<u32> = None;
        let mut note = None;
        let (kind, target) = match trigger {
            Trigger::Talk(id) => (TRIGGER_TALK, id),
            Trigger::Search(code) => (TRIGGER_EXAMINE, code),
            Trigger::Touch(code) => (TRIGGER_STEP, code),
            Trigger::Psynergy(_, id) => (TRIGGER_PSYNERGY, id),
        };
        // The dispatch it starts: Field_UsePsynergy takes the Psynergy, not
        // the actor.
        let want = (kind, (kind != TRIGGER_PSYNERGY).then_some(u32::from(target)));
        let table = self.event_rows(kind, target);
        let rows = table.len();
        if !own.is_empty() && !self.has_row(kind, target, own) {
            return Err(format!("the event table here has no row of its own for {} {target}", kind_name(kind)));
        }
        // What the event needs holds as it starts: the scene's own setup may
        // have changed a flag the run set (Mercury Lighthouse marks its floor
        // switches pressed as it is entered).
        self.apply_flags();
        // No random battle breaks into the event: a battle shown is one the
        // event starts (FLAG_NO_ENCOUNTERS).
        self.set_flag(FLAG_NO_ENCOUNTERS, true);
        self.unshadow(kind, target, own, messages)?;
        // The flags set and the actors moved for the event are written from
        // outside the game.
        self.cut();
        self.watch = table.iter().map(|r| result_key(r.result)).filter(|&r| r >= 0x10000).collect();
        self.t.ran = 0;
        self.own = own.iter().copied().filter(|&r| r >= 0x10000).collect();
        self.t.own_ran = false;
        match trigger {
            Trigger::Talk(_) | Trigger::Search(_) => self.press(2)?,
            Trigger::Psynergy(ability, _) => {
                let work = self.gba.read32(self.s.field_work);
                self.gba.write16(work + FIELD_COMMAND, ability);
                self.cut();
            }
            Trigger::Touch(_) => self.hold(spot.walk),
        }
        let hold = if record.is_some() { PAGE_HOLD } else { 3 };
        // What is on stage as the trigger comes, to tell whether the event
        // moved anything: the actors' places and the map's cells.
        let watched: Vec<(u16, i32, i32)> = self.stage().into_iter().filter(|a| early.contains(a)).collect();
        let cells_before = self.cell_codes();
        let palette_before = self.palette();
        let mut moved = false;
        let mut fought = false;
        // Where in the recording a battle the event starts began, and whether
        // the party lost it.
        let mut battle_start = 0;
        let room = self.room();
        let mut beyond = false;
        let mut defeated = false;
        // Where the leader was when the event began, to see him jump or be
        // led away.
        let mut leader_from: Option<Actor> = None;
        // Move and Carry are steered once cast: the spot's direction is held
        // until the actor starts to move (a carried one is then dropped with
        // A, once nothing has happened for a while).
        let mut steer = match trigger {
            Trigger::Psynergy(MOVE | CARRY, id) if spot.walk != 0 => self.actor(id).map(|a| (id, a.x, a.z)),
            _ => None,
        };
        let mut began: Option<u32> = None;
        // The frame the trigger was last given, and whether it was given
        // again after another row for the cell ran first.
        let mut pushed_from = trigger_frame;
        let mut pushed_again = false;
        // The frame the dispatch that began the event returned.
        let mut returned: Option<u32> = None;
        // Whether the field sequence stopped stepping for a while once the event
        // began (a script, a battle, a scene change).
        let mut paused = false;
        let mut pages = Vec::new();
        let (mut wait_start, mut press_frame, mut quiet_start) = (0, 0, self.t.frame);
        let mut captured = false;
        let mut pressed = false;
        let mut message = None;
        let mut seen_message = 0;
        // The first message shown once the event began that is not one of
        // its own.
        let mut foreign = None;
        let mut capped = false;
        // What the event may set moving, the leader included, and the last
        // frame any of it moved: a pushed pillar or log slides on after the
        // script has ended, and the event is shown until it rests.
        let mut mobile: Vec<u16> = watched.iter().map(|a| a.0).collect();
        mobile.push(self.gba.read32(self.s.game_state + GS_SELECTED_ACTOR) as u16);
        let place = |f: &Self| -> Vec<(i32, i32, i32)> { mobile.iter().map(|&id| f.actor(id).map_or((0, 0, 0), |a| (a.x, a.z, a.y >> 16))).collect() };
        let mut places = place(self);
        let mut last_motion = 0;
        loop {
            if record.is_none() && pages.len() >= max_pages {
                break;
            }
            let length = self.t.frame - trigger_frame;
            if arrival.is_none() && self.recorded.as_ref().map_or(length as usize, |f| f.len()) >= cap {
                capped = true;
                break;
            }
            self.frames(1)?;
            // A recorded event that speaks from an actor not on stage is
            // refused (EventRun::absent_speaker): no need to play it out.
            if record.is_some() && self.t.absent_speaker != 0 {
                break;
            }
            if let Some(at) = arrival {
                match self.arrive(at, hold, &mut wait_start, &mut press_frame, &mut quiet_start)? {
                    Arrival::Going => continue,
                    Arrival::InControl => {
                        // The scene it leads to, held a moment once it is
                        // all on screen.
                        self.frames(ARRIVAL_HOLD)?;
                        break;
                    }
                    Arrival::Ended(why) => {
                        note = Some(why);
                        break;
                    }
                }
            }
            if self.t.entries > entries && leads_to.is_none() && !fought && self.scene() != MAP_BATTLE {
                leads_to = Some(self.scene());
            }
            // An exit: once the scene it leads to starts loading its map (a
            // layered map) or placing the party (the world map), the event
            // has left. Unrecorded, it is over; recorded, the arrival is
            // followed until the player has control there.
            if (self.t.loads > loads || self.t.entries > entries) && self.scene() != self.setup.scene && self.scene() != MAP_BATTLE {
                left = true;
                leads_to = Some(self.scene());
                if record.is_none() {
                    break;
                }
                arrival = Some(self.t.frame);
                (wait_start, press_frame, quiet_start) = (0, 0, self.t.frame);
                continue;
            }
            if self.t.last_message != seen_message {
                seen_message = self.t.last_message;
                if messages.contains(&seen_message) {
                    message = Some(seen_message);
                } else if began.is_some() && seen_message < 0x10000 {
                    foreign.get_or_insert(seen_message);
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
                    self.hold(if steer.is_some() { spot.walk } else { 0 });
                    leader_from = self.actor(self.gba.read32(self.s.game_state + GS_SELECTED_ACTOR) as u16);
                    quiet_start = self.t.frame;
                } else if self.t.frame - pushed_from > START_FRAMES {
                    self.hold(0);
                    let how = match trigger {
                        Trigger::Talk(_) | Trigger::Search(_) => "pressing A",
                        Trigger::Touch(_) => "walking onto the cell",
                        Trigger::Psynergy(..) => "the cast",
                    };
                    return Err(format!("{how} from ({}, {}) started nothing in {} frames", spot.x, spot.z, START_FRAMES));
                }
                continue;
            };
            if returned.is_none() && self.t.dispatch_return == 0 {
                returned = Some(self.t.dispatch_end);
            }
            if !moved {
                let shifted = |a: Option<Actor>, x: i32, z: i32, by: i32| a.is_some_and(|a| (a.x - x).abs().max((a.z - z).abs()) >= by);
                moved = watched.iter().any(|&(id, x, z)| shifted(self.actor(id), x, z, 4));
                if let Some(from) = leader_from {
                    let now = self.actor(self.gba.read32(self.s.game_state + GS_SELECTED_ACTOR) as u16);
                    moved |= shifted(now, from.x, from.z, 20) || now.is_some_and(|a| (a.y - from.y).abs() >= 6 << 16);
                }
            }
            // The camera showing past the room's box: what lies there is
            // the cell grid beyond the map, never meant to be seen.
            if !beyond && arrival.is_none() && self.scene() == self.setup.scene && !self.world() {
                let (vx, vy) = self.view();
                let r = room;
                let wide = r[2] - r[0] >= WIDTH as i32;
                let tall = r[3] - r[1] >= HEIGHT as i32;
                beyond = (wide && (vx < r[0] || vx + WIDTH as i32 > r[2])) || (tall && (vy < r[1] || vy + HEIGHT as i32 > r[3]));
            }
            if !fought && self.scene() == MAP_BATTLE {
                battle_start = self.recorded.as_ref().map_or(0, Vec::len);
            }
            fought |= self.scene() == MAP_BATTLE;
            // The party lost: no scene of the game scripts that, so the
            // recording ends as the battle begins.
            if self.scene() == MAP_BATTLE && self.s.defeat.contains(&self.t.last_message) {
                defeated = true;
                break;
            }
            let now = place(self);
            if now != places {
                places = now;
                last_motion = self.t.frame;
            }
            if let Some((id, x, z)) = steer {
                // The hand waits for the d-pad; A would end the cast.
                quiet_start = self.t.frame;
                if self.actor(id).is_some_and(|a| a.x != x || a.z != z) || self.t.frame - b > 300 {
                    self.hold(0);
                    steer = None;
                }
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
            // The field sequence has stepped every frame since the event, with
            // nothing to read: the player has control again.
            let typing = self.t.frame - self.t.last_typing <= 2;
            let run = self.t.stepping_since.max(b + 1);
            let resting = self.t.frame >= last_motion + STILL_FRAMES || self.t.frame >= run + CONTROL_FRAMES + SLIDE_FRAMES;
            if !typing && self.t.step_frame + 1 >= self.t.frame && self.t.frame >= run + CONTROL_FRAMES && resting {
                // Another row for the cell ran first and gave back control (a
                // floor switch under the pillar the leader pushes from it):
                // he goes on pushing, once, for the event's own.
                if !self.own.is_empty() && !self.t.own_ran && spot.walk != 0 && !pushed_again {
                    pushed_again = true;
                    pushed_from = self.t.frame;
                    began = None;
                    returned = None;
                    self.t.dispatched = None;
                    self.hold(spot.walk);
                    continue;
                }
                break;
            }
            if typing {
                quiet_start = self.t.frame;
            }
            // A battle the event starts is fought by A alone: Fight, then
            // each character's first choice.
            if self.scene() == MAP_BATTLE {
                if self.t.frame % 20 == 0 {
                    self.press(3)?;
                }
                continue;
            }
            // Nothing typed or waiting for 120 frames (an effect, a walk)
            // while the field sequence is stopped: press A.
            if self.t.frame - quiet_start >= 120 && self.t.step_frame + 1 < self.t.frame {
                self.press(3)?;
                quiet_start = self.t.frame;
            }
        }
        self.hold(0);
        let mut frames = self.recorded.take().unwrap_or_default();
        let states = self.keep_states(false);
        // A battle the recording cannot hold to its end (a boss fought by A
        // alone) is shown to its opening, as is one the party lost.
        let unfinished = capped && fought && self.scene() == MAP_BATTLE;
        if defeated || unfinished {
            frames.truncate(battle_start + BATTLE_OPENING);
            capped = false;
            let why = if defeated { "the party, as strong as the story has it here, lost it" } else { "fought by A alone, it runs past the recording's length" };
            note = Some(format!("the recording ends as the battle begins: {why}"));
        }
        if leads_to.is_some() {
            frames = shorten_blanks(frames, BLANK_KEEP);
        }
        self.watch.clear();
        let ran = (self.t.ran != 0).then_some(self.t.ran);
        let own_ran = self.t.own_ran;
        let end = (!capped && !left && !defeated && !unfinished && self.scene() == self.setup.scene).then(|| self.save());
        // An event whose dispatch returned in the frame it began, with the
        // field sequence stepping on, ran no script: it showed nothing, or
        // nothing it could show from here.
        let instant = began.is_some() && returned == began && !left && !paused;
        // Colours the event changed and left changed show too: a cave
        // darkening as the party goes deeper (Altmiller Cave's blend).
        let shown = !pages.is_empty() || left || fought || moved || (!left && (self.cell_codes() != cells_before || self.palette() != palette_before));
        // ALCHEMY_RENDER_DEBUG prints what each try did, to see why a spot
        // was refused.
        if std::env::var_os("ALCHEMY_RENDER_DEBUG").is_some() {
            let moved_now: Vec<_> = watched.iter().filter_map(|&(id, x, z)| self.actor(id).map(|a| (id, a.x - x, a.z - z))).filter(|m| m.1 != 0 || m.2 != 0).collect();
            eprintln!("    spot ({}, {}) layer {} walk {:#x}: began {began:?} ran {:#x} pages {} left {left} fought {fought} moved {moved} {moved_now:?} cells {} leader {:?}", spot.x, spot.z, spot.layer, spot.walk, self.t.ran, pages.len(), self.cell_codes() != cells_before, leader_from.map(|f| (f.x, f.z)));
        }
        if beyond {
            let past = "the camera shows past the room's box, where the map has no ground".to_string();
            note = Some(note.map_or(past.clone(), |n| format!("{n}; {past}")));
        }
        if self.unwalked {
            let placed = "the leader starts where the walk from the entrance does not reach, since what opens the way there is not played first".to_string();
            note = Some(note.map_or(placed.clone(), |n| format!("{placed}; {n}")));
        }
        Ok(EventRun { pages, frames, states, whole: None, capped, end, instant, shown, seen: foreign, rows, ran, own_ran, leads_to, note, absent_speaker: (self.t.absent_speaker != 0).then_some(self.t.absent_speaker) })
    }

    /// One frame of an exit's arrival in the scene it leads to, which began
    /// loading at frame `at`: its pages are read and answered as the event's
    /// are (none is kept), until the field sequence has stepped CONTROL_FRAMES
    /// in a row with nothing to read, as at a scene's start.
    fn arrive(&mut self, at: u32, hold: u32, wait_start: &mut u32, press_frame: &mut u32, quiet_start: &mut u32) -> Result<Arrival, String> {
        let frame = self.t.frame;
        if self.scene() == MAP_BATTLE {
            return Ok(Arrival::Ended("ends as a battle begins".to_string()));
        }
        if frame - at >= ARRIVAL_LIMIT {
            return Ok(Arrival::Ended(format!("the arrival runs past {} seconds; the recording ends there", ARRIVAL_LIMIT / 60)));
        }
        if self.t.last_dismiss > at && frame - self.t.last_dismiss <= 2 {
            *quiet_start = frame;
            if *wait_start == 0 {
                *wait_start = frame;
                *press_frame = 0;
            }
            if *press_frame == 0 && frame - *wait_start >= hold {
                *press_frame = frame;
                self.press(3)?;
            } else if *press_frame != 0 && frame - *press_frame >= 30 {
                *press_frame = frame;
                self.press(3)?;
            }
            return Ok(Arrival::Going);
        }
        *wait_start = 0;
        if self.t.choice_return != 0 {
            *quiet_start = frame;
            if frame - self.t.choice_frame == 20.max(hold) {
                self.press(3)?;
            }
            return Ok(Arrival::Going);
        }
        let typing = frame - self.t.last_typing <= 2;
        if typing {
            *quiet_start = frame;
        }
        let run = self.t.stepping_since.max(at + 1);
        if !typing && self.t.step_frame + 1 >= frame && frame >= run + CONTROL_FRAMES {
            return Ok(Arrival::InControl);
        }
        if frame - *quiet_start >= 120 && self.t.step_frame + 1 < frame {
            self.press(3)?;
            *quiet_start = frame;
        }
        Ok(Arrival::Going)
    }
}

/// How an exit's arrival goes on (Field::arrive).
enum Arrival {
    Going,
    InControl,
    /// Stopped before the player had control, and why.
    Ended(String),
}

/// Frames an exit's arrival is followed from the moment the next scene
/// starts loading, ten seconds; an arrival cutscene longer than that ends
/// there.
const ARRIVAL_LIMIT: u32 = 600;
/// Frames the scene an exit leads to is held once the player has control
/// there.
const ARRIVAL_HOLD: u32 = 30;
/// Frames of a blank screen (one colour all over: the fade's black, a
/// load) kept in a row in an exit's recording.
const BLANK_KEEP: usize = 8;

/// `frames` with each run of blank frames, one colour all over, cut to its
/// first `keep`, so an exit's GIF does not sit on black while the next
/// scene loads.
fn shorten_blanks(frames: Vec<Vec<u16>>, keep: usize) -> Vec<Vec<u16>> {
    let blank = |f: &[u16]| f.first().is_some_and(|&c| f.iter().all(|&p| p == c));
    let mut run = 0;
    frames
        .into_iter()
        .filter(|f| {
            run = if blank(f) { run + 1 } else { 0 };
            run <= keep
        })
        .collect()
}

/// A field Psynergy's reach by its ability id: its range
/// (gFieldEffect_TargetRangeByMode, by field kind) and the height between the
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
        // Move, Whirlwind and Halt: 32.
        MOVE | 0x4e | 0x91 => (32, 8 << 16),
        // Frost, Growth, Lift, Carry and Cloak: 16.
        _ => (16, 8 << 16),
    }
}

/// Whether an actor other than `id` would be found before `target` from
/// (x, z) facing `facing` (1/65536 turns): nearer, within `range` pixels,
/// and ahead within `cone` unless it is 11 pixels away or closer.
#[allow(clippy::too_many_arguments)]
fn nearer(others: &[(u16, Actor)], id: u16, target: &Actor, x: i32, z: i32, facing: u32, range: i32, cone: i32) -> bool {
    let distance = |a: &Actor| f64::from((a.x - x).pow(2) + (a.z - z).pow(2)).sqrt() as i32;
    let own = distance(target);
    others.iter().any(|(o, a)| {
        let d = distance(a);
        let ahead = || (facing_toward(a.x - x, a.z - z).wrapping_sub(facing) as u16 as i16 as i32).abs() < cone;
        *o != id && d < own && d < range && (d <= 11 || ahead())
    })
}

/// The map layers the leader may stand on beside an actor: his own, where
/// he walks, then the actor's when it is another (an actor on a bridge or a
/// ledge another layer draws), which he stands on only where a scene puts
/// him.
fn beside_layers(own: u8, actor: u8) -> Vec<u8> {
    if own == actor { vec![own] } else { vec![own, actor.min(2)] }
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
        TRIGGER_TALK => "talk",
        TRIGGER_STEP => "touch",
        TRIGGER_EXAMINE => "search",
        TRIGGER_PSYNERGY => "Psynergy",
        _ => "event",
    }
}

/// A row's result as the renderer compares it: an event function's address
/// without its Thumb bit, else the message it shows (a message number, or a
/// found message, FIND_MESSAGE's 0x00400000 with its number).
pub fn result_key(result: u32) -> u32 {
    if result >> 16 == FIND_MESSAGE >> 16 {
        result & 0xffff
    } else if result >= 0x10000 {
        result & !1
    } else {
        result
    }
}

/// FIND_MESSAGE (scene_tables.h): a row's result that shows a message as
/// something found.
const FIND_MESSAGE: u32 = 0x0040_0000;

/// A row of the scene's event table (struct MapEvent).
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
    /// The map layer its ground is read from (layer).
    pub layer: u8,
    /// Whether it has a sprite.
    pub drawn: bool,
}

/// Where the leader can walk in his room (Field::reach).
pub struct Reach {
    /// The map layer he walks on.
    pub layer: u8,
    /// `width` by `width` 16-pixel cells, row by row, from cell `origin`.
    cells: Vec<bool>,
    origin: (i32, i32),
    width: i32,
    /// The room's camera box, pixels.
    room: [i32; 4],
    /// The codes of the cells the walk reached but did not cross: exits,
    /// and the touch being played.
    stops: Vec<i32>,
}

impl Reach {
    /// Whether the leader can stand at (x, z) pixels on map layer `layer`
    /// and be seen: inside the room, 8 pixels from its sides, on a cell he
    /// can walk to. On another layer than his, which no walk reaches, only
    /// the room is known.
    pub fn holds(&self, layer: u8, x: i32, z: i32) -> bool {
        let r = self.room;
        if x < r[0] + 8 || x >= r[2] - 8 || z < r[1] + 8 || z >= r[3] - 8 {
            return false;
        }
        if layer != self.layer {
            return true;
        }
        let (cx, cz) = ((x >> 4) - self.origin.0, (z >> 4) - self.origin.1);
        (0..self.width).contains(&cx) && (0..self.width).contains(&cz) && self.cells[(cz * self.width + cx) as usize]
    }

    /// ` without crossing an exit or a cell of the touch itself (codes 1, 26)`
    /// when the walk stopped at such cells, else nothing.
    pub fn crossing(&self) -> String {
        match self.stops.len() {
            0 => String::new(),
            _ => format!(" without crossing an exit or a cell of the touch itself (codes {})", self.stops.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")),
        }
    }

    /// The same room with every cell taken as one he can walk to.
    pub fn anywhere(&self) -> Reach {
        Reach { layer: self.layer, cells: vec![true; self.cells.len()], room: self.room, stops: Vec::new(), origin: self.origin, width: self.width }
    }

    /// How many of the eight cells around (x, z) pixels he can walk to: a
    /// spot in the open (8) shows him better than one on the side of a
    /// stair or the edge of a cliff.
    pub fn open(&self, layer: u8, x: i32, z: i32) -> usize {
        if layer != self.layer {
            return 8;
        }
        let (cx, cz) = (x >> 4, z >> 4);
        AROUND.iter().filter(|(dx, dz)| self.holds(layer, (cx + dx) * 16 + 8, (cz + dz) * 16 + 8)).count()
    }
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

/// Frames of a battle kept when the party loses it: its opening, the
/// enemies appearing.
const BATTLE_OPENING: usize = 150;

/// Frames a page is held before A while an event is recorded.
const PAGE_HOLD: u32 = 45;
/// Frames an event may run, four minutes: Mercury Lighthouse Aerie's
/// confrontation with Saturos runs past three to its battle, with every
/// page held; the stormy night at the river and Ivan joining in Vault run
/// under two.
pub const EVENT_LIMIT: u32 = 14400;

/// What an event left: its pages, when recorded its frames, and the moment
/// the player had control again, when that was in the same scene.
pub struct EventRun {
    pub pages: Vec<Page>,
    /// Each frame's picture, BGR555.
    pub frames: Vec<Vec<u16>>,
    /// The machine's state at the end of each of those frames, when the
    /// event was recorded whole (Field::whole).
    pub states: Option<super::whole::States>,
    /// The event drawn whole from those states (whole::event), or why it
    /// could not be.
    pub whole: Option<Result<Vec<Image>, String>>,
    /// Whether the recording stopped at its cap.
    pub capped: bool,
    /// Whether the event ran no script: its dispatch returned in the frame
    /// it began.
    pub instant: bool,
    /// Whether anything showed: a page, an exit, a battle, an actor moved
    /// (the leader jumping or led 20 pixels away included), or the map's
    /// cells changed.
    pub shown: bool,
    /// The first message the event showed that is not one of its own.
    pub seen: Option<u32>,
    /// How many rows of the scene's event table answer the trigger: with
    /// one, a page none of the event's messages is on is still its own (a
    /// branch the story tools did not follow, a found item's line).
    pub rows: usize,
    /// The event function of a row for the trigger that ran first, when one
    /// did.
    pub ran: Option<u32>,
    /// Whether one of the event's own functions ran, after any other row
    /// for the trigger (a floor switch the leader steps on before he
    /// pushes the pillar beside it).
    pub own_ran: bool,
    pub end: Option<Saved>,
    /// The scene the event placed the party in next, before any battle:
    /// the scene an exit leads to (recorded until the player has control
    /// there), or this scene for a staircase to another of its entrances.
    pub leads_to: Option<u16>,
    /// Why a recorded arrival stopped before the player had control: a
    /// battle began, or it ran past ARRIVAL_LIMIT.
    pub note: Option<String>,
    /// A speaker one of the event's messages was opened beside that the
    /// scene has no actor for: played here, its cast is not on stage (Mercury
    /// Lighthouse Aerie's hostages after the battle), and its windows open
    /// beside nobody.
    pub absent_speaker: Option<u16>,
}

/// Whether entrance `e` only stands in for a room another entrance gives
/// its camera box: it gives none itself (the scene's default entrance, which
/// no exit leads to), and its place lies in another's box. Entered by it,
/// the camera takes the header's bounds, wider than the room, and shows the
/// cell grid beyond the map (Alpine Crossing's teal and orange fill).
pub fn stands_in(e: &Entrance, all: &[Entrance]) -> bool {
    let inside = |o: &Entrance| {
        let c = o.camera;
        c.iter().all(|&s| s != -1) && (c[0]..c[2]).contains(&e.x) && (c[1]..c[3]).contains(&e.z)
    };
    e.camera.iter().all(|&s| s == -1) && all.iter().any(|o| o.number != e.number && inside(o))
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
    fn rows_are_told_by_their_function_or_message() {
        assert_eq!(result_key(0x0200_9349), 0x0200_9348);
        assert_eq!(result_key(4804), 4804);
        assert_eq!(result_key(0x0040_29c5), 10693);
    }

    #[test]
    fn the_leader_stands_on_his_own_layer_first() {
        assert_eq!(beside_layers(0, 0), vec![0]);
        assert_eq!(beside_layers(0, 1), vec![0, 1]);
    }

    #[test]
    fn boxes_take_the_header_for_missing_sides() {
        let header = [0, 0, fx(512), fx(256)];
        assert!(box_holds([-1, -1, -1, -1], header, 500, 10));
        assert!(!box_holds([0, 0, 240, 160], header, 300, 10));
        assert!(box_holds([256, -1, -1, -1], header, 300, 10));
    }

    #[test]
    fn an_exit_keeps_only_the_start_of_each_blank_run() {
        let (black, white, picture) = (vec![0u16; 4], vec![0x7fffu16; 4], vec![1u16, 2, 3, 4]);
        let mut frames = vec![picture.clone()];
        frames.extend(std::iter::repeat_n(black.clone(), 5));
        frames.push(picture.clone());
        frames.extend(std::iter::repeat_n(white.clone(), 3));
        let kept = shorten_blanks(frames, 2);
        assert_eq!(kept, vec![picture.clone(), black.clone(), black, picture, white.clone(), white]);
    }
}
