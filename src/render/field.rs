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
//!
//! What the leader does by walking (a step trigger, an exit) is recorded as
//! he walks up to it (Field::walk_up): he starts some cells back, on the way
//! a player would come along the ground he can walk and the vines he can
//! climb (approach.rs), and the pad walks him there and on into the trigger,
//! so the lead-in is the walk. A recording in which he mostly stands before
//! it begins is refused. An event the screen does not show (the game moving
//! him and the camera together, as Tret Tree's vines do) is kept with a note
//! saying so, and goes on to the climb it lets him make. A touch that leaves
//! him on a ladder is held on down it (Field::event), a push is gone on with
//! (Field::push_on), and a change to the map under him is uncovered by a
//! step back (Field::step_back).

use super::animation::{is_black_bgr555, BLACK_CLEARANCE};
use super::approach::{self, How, Move, Pad};
use super::mgba::{Gba, HEIGHT, KEY_A, KEY_DOWN, KEY_LEFT, KEY_RIGHT, KEY_UP, LR, SP, WIDTH};
use super::party::{chapter, order, Party};
use super::story::{Trigger, CARRY, MOVE};
use crate::gba::ld_map::Map;
use std::collections::HashMap;

/// The offsets into the game's structures the run reads are its Layout
/// (layout.rs), in Symbols::l.
///
/// The layers' screen maps in VRAM, BG_SCREEN_BLOCK(5) to (7), which
/// MapGrid_SetCameraCenter draws; and the frames Field::pin_view waits for
/// the field's layer task to run.
const PIN_SCREEN_MAPS: u32 = 0x0600_2800;
const PIN_SCREEN_MAP_BYTES: u32 = 3 * 0x800;
const PIN_DRAW_FRAMES: u32 = 8;
/// The first actor a scene places (ACTOR_FIRST_PLACED); 0 to 7 are the party.
const FIRST_PLACED: u16 = 8;
/// A raised trigger no scene answers (theirs are small numbers; the
/// field's own are negative), which a direct start raises.
const RAISED_DIRECT: u32 = 0x7fff;
/// struct Sprite's entries, its four layers, and the colour mode
/// (param) of struct SpriteAnimation, whose first halfword is its sprite id.
const ANIM_ENTRIES: u32 = 0x28;
const ANIM_ENTRY_PARAM: u32 = 0x05;
/// FieldWork.mode's world map mode (an enum ActorSystemMode).
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
const STEP_TRIGGER_FLAG_WALKING: u32 = 0x200;
const TRIGGER_FLAG_FACING: u32 = 0x400;
/// The TriggerKind of each dispatch.
pub const TRIGGER_TALK: u8 = 0;
pub const TRIGGER_EXIT: u8 = 1;
pub const TRIGGER_STEP: u8 = 2;
pub const TRIGGER_EXAMINE: u8 = 3;
pub const TRIGGER_PSYNERGY: u8 = 5;
/// No kind of the game's: an event function started directly
/// (Trigger::Direct).
pub const TRIGGER_DIRECT: u8 = 0xff;
/// No random encounters while set (flags.h).
/// struct Unit's psynergy, four bytes each.
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
pub const LEAD_IN: u32 = 12;
/// How many frames a script may keep the leader in a wall, in black or off
/// the map: longer than a hop over a ledge, shorter than anyone could miss.
const ASTRAY_FRAMES: u32 = 40;
/// How far a script may move the leader in a direct start, in pixels: a turn
/// or a flinch, not a walk.
const DIRECT_WALK: i32 = 16;
/// A step this long between two looks at the leader is no walk: a door or a
/// script has put him somewhere else, and where he was says nothing of it.
const TELEPORT: i32 = 24;
/// Below this many pixels of a placed leader on the screen, what stands in
/// front of him hides him: a pillar, a bush, a bridge. Measured over the
/// recordings looked at: a leader in the open shows 280 to 480.
const HIDDEN_BELOW: i32 = 170;
/// Below this many, the sprite is not what shows him, and nothing can be said.
const UNMEASURED_BELOW: i32 = 20;
/// With this share of the ground at his feet one flat colour, in percent, the
/// map draws nothing there: ground has a texture, the void around a map has none.
const VOID_AT_FEET: i32 = 75;
/// How many connected cells of open ground make somewhere to stand: fewer is a
/// pocket shut in by a cliff or a wall, which the player never reaches.
const OPEN_REGION: usize = 64;
/// How many frames he may stand with black all around him.
const BLACK_FRAMES: u32 = 24;
/// How many frames a script may have him where a placed start could not walk.
/// How many frames he may hang over ground that is there: a fall, a long jump.
const ALOFT_FRAMES: u32 = 90;
/// How near an actor's centre he counts as riding it: a log is two cells long.
const RIDE_REACH: i32 = 20;
/// How many spots beside its speaker a message row tries.
const BESIDE_SPOTS: usize = 4;
/// How many spots of open ground a direct start tries.
const NEAR_SPOTS: usize = 6;
/// Frames a recording keeps of the game on each side of a black screen:
/// BLACK_CLEARANCE and a margin, so the pictures a GIF shows, one of every
/// two, still clear it.
pub const CLEARED: usize = BLACK_CLEARANCE + 4;
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
/// Frames the leader may make no headway on a walk before it has failed (a
/// wanderer in the way, a corner the cells did not show).
const STUCK_FRAMES: u32 = 40;
/// Frames a climb or a drop may take before it has failed: a vine of two
/// cells takes about 150.
const CLIMB_FRAMES: u32 = 600;
/// Rounds of STILL_FRAMES the camera may take to come to rest on the leader
/// put at the start of a walk.
const SETTLE_VIEW_FRAMES: u32 = 30;
/// How far the leader may stray from the cells of the move he is making, in
/// pixels, before the walk has gone wrong (a door, a stair onto another
/// layer, a slide the cells did not show).
const OFF_THE_WAY: i32 = 12;
/// Frames the leader stands where a climb has left him before it is over.
const LANDED_FRAMES: u32 = 8;
/// Walks an event that moved the leader without showing it looks through for
/// the climb it lets him make.
const ONWARD_WALKS: usize = 3;
/// Frames a recording runs on once the climb an unseen event leads to is over.
const ONWARD_TAIL: u32 = 30;
/// Frames a push may be gone on with, the leader walking after what he
/// pushed and pushing it again.
const PUSH_ON_FRAMES: u32 = 1200;
/// Frames what was pushed may lie still before pushing on has ended.
const PUSH_REST_FRAMES: u32 = 60;

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
    field_menu: u32,
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
    /// Scene_RunRaisedTrigger, where a direct start begins.
    run_raised: u32,
    /// What a trigger row's dispatch calls to show a message row
    /// (Field_RunPsynergyEvent, Scene_TalkToActor): Event_Start,
    /// Event_SetMessageId, Event_ShowMessageAndWait and Event_End.
    show_row: [u32; 4],
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
    /// The Lost Age's map entrance list and camera boxes, as loaded with
    /// its map (Field_PlaceAtEntrance; gMapScene, the name wanted is
    /// one that says so); 0 for The Broken Seal.
    pub map_entrances: u32,
    /// Where the game's structures keep what the run reads.
    pub l: super::layout::Layout,
}

impl Symbols {
    /// Reads the defeat messages' numbers, absolute symbols the linker map
    /// leaves out, from the edition's ELF at `elf`.
    pub fn read_defeat(&mut self, elf: &std::path::Path) -> Result<(), String> {
        let data = std::fs::read(elf).map_err(|e| format!("cannot read {}: {e}", elf.display()))?;
        let elf = crate::gba::elf::Elf::parse(&data)?;
        self.defeat = elf.symbols.iter().filter(|y| y.name == "MsgGoesDownInDefeat" || y.name == "MsgPartyDefeated").map(|y| y.value).collect();
        Ok(())
    }

    /// The Broken Seal's, from its map.
    pub fn find(map: &Map) -> Result<Symbols, String> {
        Symbols::find_for(map, super::layout::Layout::tbs())
    }

    /// An edition's, from its map, with its game's layout `l`. Where The
    /// Lost Age's source still names a thing otherwise than The Broken
    /// Seal's, The Broken Seal's name is looked for first and then The Lost
    /// Age's (the second of each pair), the name its source should change; a
    /// work pointer The Lost Age reaches only through its heap slots
    /// (gFieldWork, gMapGridWork) is its slot in gHeapSlots.
    pub fn find_for(map: &Map, l: super::layout::Layout) -> Result<Symbols, String> {
        let find = |name: &str| map.symbol(name).map(|(_, a)| a & !1);
        let at = |name: &str| find(name).ok_or_else(|| format!("the map has no {name}"));
        let either = |name: &str, other: &str| find(name).or_else(|| find(other)).ok_or_else(|| format!("the map has no {name} (nor {other})"));
        let slot = |name: &str, offset: u32| {
            find(name).or_else(|| find("gHeapSlots").map(|slots| slots + offset)).ok_or_else(|| format!("the map has no {name} (nor gHeapSlots)"))
        };
        Ok(Symbols {
            init: at("MainRoutineWork_InitializeDefaults")?,
            glow: at("WindowColor_Apply")?,
            load_layered: at("MapGrid_LoadMap")?,
            // The Lost Age places the party in three tries, its scene's table
            // by number, its map's entrances, the table's first row
            // (Field_ApplyEntrance, BattleMap_ApplySceneEntranceView before 22 November: it
            // is The Broken Seal's Scene_ApplyEntrance grown); the run takes
            // the scene as entered once all three are done.
            apply_entrance: if l.game == super::layout::Game::Tla { either("Field_ApplyEntrance", "BattleMap_ApplySceneEntranceView")? } else { at("Scene_ApplyEntrance")? },
            notice: at("AreaNameWindow_Show")?,
            field_menu: at("FieldMenu_Run")?,
            step: at("Field_UpdateWalking")?,
            update_layer: at("MapGrid_UpdateLayerScroll")?,
            center: either("MapGrid_CenterOnFollowedPoint", "Map_CenterOnFollowedPoint")?,
            dismiss: at("Text_CheckTextDismissed")?,
            speak: at("FieldMessage_ShowAtActor")?,
            objects: at("Actor_UpdateAllThumb")?,
            typing: at("Message_Type")?,
            choice: at("YesNoMenu_Run")?,
            lookup: at("Message_Find")?,
            terrain_height: at("MapGrid_GetTerrainHeight")?,
            write_cell_tile: at("WorldMap_WriteLayerCellTile")?,
            map_cell_pictures: either("gCellPictures", "gMapBlocks")?,
            map_collision: either("gMapGridShapes", "gMapCollision")?,
            map_cell_buffer: either("gMapGridCellBuffer", "gMapCellBuffer")?,
            // The Broken Seal's story chapters load a debug preset, and The
            // Lost Age's guide recordings (party.rs); DebugParty_LoadPreset
            // is The Lost Age's name for it.
            preset: find("DebugParameterSet_Load").or_else(|| find("DebugParty_LoadPreset")).unwrap_or(0),
            restore: at("Party_RestoreHpPp")?,
            unit_get: at("Unit_Get")?,
            defeat: Vec::new(),
            talk: at("Scene_TalkToActor")?,
            touch: at("Scene_RunStepTriggerOrExit")?,
            search: at("Field_ExamineCell")?,
            psynergy: at("Field_UsePsynergy")?,
            run_raised: either("Scene_RunRaisedTrigger", "Scene_RunRaisedEvent")?,
            show_row: [at("Event_Start")?, at("Event_SetMessageId")?, at("Event_ShowMessageAndWait")?, at("Event_End")?],
            game_state: at("gMainRoutineWork")?,
            flag_bytes: at("gGameFlagBytes")?,
            map_work: slot("gMapGridWork", l.heap_map)?,
            field_work: slot("gFieldWork", l.heap_field)?,
            overlay: at("gOverlayArea")?,
            random_seed: either("gRandomSeed", "gRandomState")?,
            frame_count: at("gFrameCount")?,
            render_pending: at("gFrameRenderPending")?,
            bg_scroll: at("gBgScroll")?,
            map_entrances: if l.game == super::layout::Game::Tla { at("gMapScene")? } else { 0 },
            l,
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
    /// While an event plays: the field menu a press of A would open is
    /// not opened, and the frame it would have opened is kept (the player
    /// had control again; A with nothing ahead opens it, FieldSequence_Run).
    block_menu: bool,
    menu_frame: u32,
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
    /// An event's direct start: the calls the field sequence makes in place
    /// of Scene_RunRaisedTrigger for the trigger it raised (RAISED_DIRECT),
    /// in order, each with r0 and r1; how many there are; how many are made;
    /// and where the first was to return to in the field sequence.
    raise: [(u32, [u32; 2]); 4],
    raise_len: u8,
    raise_next: u8,
    raise_lr: u32,
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
    show_notice: bool,
}

impl<'rom> Recipe<'rom> {
    pub fn enter(&self) -> Result<Field<'rom>, String> {
        let mut field = Field::enter(self.rom, self.s, &self.setup)?;
        field.returns.clone_from(&self.returns);
        field.pokes.clone_from(&self.pokes);
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

/// One page of an event, a moment it waited for the button.
pub struct Page {
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
    /// Whether a direct start moved the leader off where the arrival left him.
    resettled: bool,
    /// The event has a trigger of its own (a touch, a search), so a direct
    /// start is a stand-in that staged nothing; a raised or entry event has no
    /// other start, and its script may walk the leader as it means to.
    pub has_trigger: bool,
    /// Frames of the leader standing ready before a recorded event's
    /// trigger: LEAD_IN, or more where the event reaches a black screen
    /// sooner than BLACK_CLEARANCE (Context::lead_in).
    pub lead_in: u32,
    /// Whether an event is recorded with the leader hidden from its start.
    pub hide_leader: bool,
    /// The addresses whose entry calls `probe` (with the machine, the
    /// address and the frame), which also runs at each frame's end (address
    /// 0), for a recording that watches the game; and the functions made to
    /// return at once, with r0.
    pub probe_pcs: Vec<u32>,
    pub probe: Option<Box<dyn FnMut(&Gba<'rom>, u32, u32) + 'rom>>,
    pub returns: Vec<(u32, u32)>,
    /// Words written as the CPU reaches an address: (address, where, value).
    pub pokes: Vec<(u32, u32, u32)>,
    /// Lets AreaNameWindow_Show run, for a picture of the notice.
    pub show_notice: bool,
    /// Whether a recorded touch has the leader walk up to it from some cells
    /// back (render event's recordings), and is refused when he mostly
    /// stands before it; a guide's staged recordings keep him where they
    /// put him.
    pub walk_up: bool,
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
            show_notice: self.show_notice,
        })
    }

    /// Actor `id`'s object's address, 0 for none.
    pub fn actor_address(&self, id: u16) -> u32 {
        self.actor_object(id)
    }

    /// Boots the ROM and runs until Scene_ApplyEntrance has placed the party.
    pub fn enter(rom: &'rom [u8], s: &'rom Symbols, setup: &Setup) -> Result<Field<'rom>, String> {
        let party = if s.l.game == super::layout::Game::Tbs { chapter(setup.scene)? } else { None };
        Field::enter_with_party(rom, s, setup, party)
    }

    /// As `enter`, with `party` in place of the chapter the scene falls in:
    /// The Lost Age's guide recordings name a debug preset of their own
    /// (party::tla).
    pub fn enter_with_party(rom: &'rom [u8], s: &'rom Symbols, setup: &Setup, party: Option<&'static Party>) -> Result<Field<'rom>, String> {
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
            block_menu: false,
            menu_frame: 0,
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
            raise: [(0, [0; 2]); 4],
            raise_len: 0,
            raise_next: 0,
            raise_lr: 0,
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
        let mut field = Field { gba: Gba::new(rom)?, rom, s, t, setup: setup.clone(), recorded: None, hide_leader: false, states: None, whole: false, watch: Vec::new(), own: Vec::new(), world_cells: None, relaxed: false, unwalked: false, resettled: false, has_trigger: false, lead_in: LEAD_IN, probe_pcs: Vec::new(), probe: None, returns: Vec::new(), pokes: Vec::new(), show_notice: false, walk_up: false, party };
        while !field.t.applied {
            field.run_instruction()?;
        }
        Ok(field)
    }

    /// The story flags' bytes, all 4096 flags.
    fn flag_bytes(&self) -> Vec<u8> {
        (0..0x200).map(|i| self.gba.read8(self.s.flag_bytes + i)).collect()
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
            self.gba.write8(self.s.game_state + self.s.l.gs_active_owners + i as u32, o);
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
        (self.gba.read32(work + self.s.l.map_view_x) as i32 >> 16, self.gba.read32(work + self.s.l.map_view_y) as i32 >> 16)
    }

    /// The leader as he is now.
    pub fn leader(&self) -> Option<Actor> {
        self.actor(self.gba.read32(self.s.game_state + self.s.l.gs_selected_actor) as u16)
    }

    /// Enters again from `placed` (as Scene_ApplyEntrance placed the party)
    /// with the leader at `spot`, and runs until the player has control and
    /// the camera has settled on him, as an event's start does.
    pub fn stand(&mut self, placed: &Saved, spot: &Spot) -> Result<(), String> {
        self.load(placed);
        let gs = self.s.game_state;
        self.gba.write32(gs + self.s.l.gs_x, fx(spot.x) as u32);
        self.gba.write32(gs + self.s.l.gs_y, spot.height as u32);
        self.gba.write32(gs + self.s.l.gs_z, fx(spot.z) as u32);
        self.gba.write32(gs + self.s.l.gs_heading, spot.facing);
        self.gba.write16(gs + self.s.l.gs_turn, u16::from(spot.layer));
        self.until_control()?;
        self.put_leader(spot);
        self.frames(SETTLE)
    }

    /// The abilities character `owner` has, in the order of their slots
    /// (struct Unit's psynergy, an action id in each low 14 bits),
    /// as the field menu lists them.
    pub fn abilities(&mut self, owner: u8) -> Result<Vec<u16>, String> {
        let unit = self.eval(self.s.unit_get, [u32::from(owner), 0, 0, 0])?;
        Ok((0..32).map(|i| self.gba.read16(unit + self.s.l.unit_action_slots + 4 * i) & 0x3fff).filter(|&a| a != 0).collect())
    }

    /// The address of character `owner`'s struct Unit (Unit_Get).
    pub fn unit(&mut self, owner: u8) -> Result<u32, String> {
        self.eval(self.s.unit_get, [u32::from(owner), 0, 0, 0])
    }

    /// The address of gMainRoutineWork, struct MainRoutineWork.
    pub fn main_routine_work(&self) -> u32 {
        self.s.game_state
    }

    /// The scene's struct FieldWork (gFieldWork).
    pub fn field_work(&self) -> u32 {
        self.gba.read32(self.s.field_work)
    }

    /// Gives the party `coins` (SaveBlock.coins).
    pub fn set_coins(&mut self, coins: u32) {
        self.gba.write32(self.s.game_state + self.s.l.gs_coins, coins);
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
        self.t.call_origin = [self.gba.read32(self.s.game_state + self.s.l.gs_x) as i32, self.gba.read32(self.s.game_state + self.s.l.gs_z) as i32];
        self.t.call_args = Some(args);
    }

    /// Moves the party's position in the game state (pixels).
    pub fn set_position(&mut self, x: i32, z: i32) {
        self.gba.write32(self.s.game_state + self.s.l.gs_x, fx(x) as u32);
        self.gba.write32(self.s.game_state + self.s.l.gs_z, fx(z) as u32);
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
        self.gba.read16(self.s.game_state + self.s.l.gs_scene)
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
            self.gba.write8(self.s.game_state + self.s.l.gs_active_owners + i as u32, owner);
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
            self.gba.write16(s.game_state + self.s.l.gs_scene, self.setup.scene);
            self.gba.write16(s.game_state + self.s.l.gs_entrance, self.setup.entrance);
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
                self.t.header[i] = self.gba.read32(work + self.s.l.map_bounds + 4 * i as u32) as i32;
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
        if pc == s.field_menu && self.t.block_menu {
            self.gba.return_now(0);
            self.t.menu_frame = self.t.frame;
        }
        if (frame_done || self.probe_pcs.contains(&pc)) && self.probe.is_some() {
            let mut probe = self.probe.take().unwrap();
            probe(&self.gba, if self.probe_pcs.contains(&pc) { pc } else { 0 }, self.t.frame);
            if frame_done && self.probe_pcs.contains(&pc) {
                probe(&self.gba, 0, self.t.frame);
            }
            self.probe = Some(probe);
        }
        for i in 0..self.pokes.len() {
            let (at, address, value) = self.pokes[i];
            if at == pc {
                self.gba.write32(address, value);
            }
        }
        if let Some(&(_, r0)) = self.returns.iter().find(|(a, _)| *a == pc) {
            self.gba.return_now(r0);
            return Ok(frame_done);
        }
        // A direct start: the field sequence answers the trigger it raised by
        // calling Scene_RunRaisedTrigger, from its request loop with the
        // field busy, as it starts every event. In its place the event's own
        // calls are made, each returning here for the next and the last to
        // the loop, so the event begins and ends as a dispatched one does.
        if pc == s.run_raised && self.t.raise_next < self.t.raise_len {
            let i = usize::from(self.t.raise_next);
            let (target, args) = self.t.raise[i];
            if i == 0 {
                self.t.raise_lr = self.gba.register(LR);
                self.t.dispatched = Some((TRIGGER_DIRECT, 0, self.t.frame));
                self.t.dispatch_return = self.t.raise_lr & !1;
                self.t.dispatch_result = None;
                // The branch lands on the function: the next step reports
                // the instruction after its first, so it runs from here.
                if self.watch.contains(&target) {
                    self.t.ran = target;
                    self.t.own_ran |= self.own.contains(&target);
                }
            }
            self.t.raise_next += 1;
            let last = self.t.raise_next == self.t.raise_len;
            self.gba.set_register(0, args[0]);
            self.gba.set_register(1, args[1]);
            self.gba.set_register(LR, if last { self.t.raise_lr } else { s.run_raised | 1 });
            self.gba.branch(target);
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
            self.gba.write32(work + self.s.l.map_origin, s.game_state + self.s.l.gs_x);
            for i in 0..3 {
                self.gba.write32(s.game_state + self.s.l.gs_x + 4 * i as u32, self.t.focus[i] as u32);
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
            if (PARTY_SPEAKERS..self.s.l.field_actor_count as u16).contains(&id) && self.actor(id).is_none() {
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
                let scene = self.gba.read16(s.game_state + self.s.l.gs_scene);
                return Err(format!("gave up at frame {} in scene {scene}", self.t.frame));
            }
        }
        Ok(frame_done)
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
        let scene = self.gba.read16(self.s.game_state + self.s.l.gs_scene);
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
            if self.t.last_typing != typed || self.gba.read16(self.s.game_state + self.s.l.gs_scene) != self.setup.scene {
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
        [0, 1, 2, 3].map(|i| self.gba.read32(work + self.s.l.map_bounds + 4 * i) as i32)
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
        work != 0 && u16::from(self.gba.read8(work + self.s.l.field_mode)) == ACTOR_SYSTEM_WORLD_MAP
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
            a += self.s.l.entrance_size;
        }
        for e in self.map_entrances() {
            if !out.iter().any(|o| o.number == e.number) {
                out.push(e);
            }
        }
        out
    }

    /// The Lost Age's other entrances, which its map rather than its scene
    /// keeps (Field_PlaceAtEntrance, where no row of the scene's table has
    /// the number): the map's entrance list, each row its number (s8, -1
    /// ends), x and z in pixels (s16) and a byte of flags, and its camera
    /// boxes, each four s8 in cells (left, top, right, bottom), the first
    /// that holds an entrance's spot its box. The heights the game takes off
    /// z to test a box are not read here: an entrance's box is the first
    /// that holds it at its own height or up to 128 pixels above. Empty for
    /// The Broken Seal, whose map has none.
    fn map_entrances(&self) -> Vec<Entrance> {
        let base = self.s.map_entrances;
        if base == 0 || self.world() || self.gba.read32(self.s.map_work) == 0 {
            return Vec::new();
        }
        let list = base + u32::from(self.gba.read16(base));
        let boxes = base + u32::from(self.gba.read16(base + 4));
        let mut cameras: Vec<[i16; 4]> = Vec::new();
        for i in 0..64 {
            let b = |o: u32| i16::from(self.gba.read8(boxes + 4 * i + o) as i8);
            if b(0) == -1 {
                break;
            }
            cameras.push([b(0) << 4, b(1) << 4, b(2) << 4, b(3) << 4]);
        }
        let mut out = Vec::new();
        for i in 0..128 {
            let at = list + 6 * i;
            let number = i16::from(self.gba.read8(at) as i8);
            if number == -1 {
                break;
            }
            let word = |o: u32| (u16::from(self.gba.read8(at + o)) | u16::from(self.gba.read8(at + o + 1)) << 8) as i16;
            let (x, z) = (word(1), word(3));
            let holds = |c: &[i16; 4], h: i16| x >= c[0] && x <= c[2] && z - h >= c[1] && z - h <= c[3];
            let camera = (0..=16).find_map(|k| cameras.iter().find(|c| holds(c, 8 * k))).map_or([-1; 4], |c| *c);
            out.push(Entrance { number, required_flag: -1, x, z, camera });
        }
        out
    }

    /// Puts an actor at (x, z) pixels, as Actor_SetPosition does.
    /// Names the event about to be played by its function's result key
    /// (result_key), so that `spots` picks its own rows where a cell's code
    /// has others' too.
    pub fn play_as(&mut self, own: &[u32]) {
        self.own = own.iter().copied().filter(|&r| r >= 0x10000).collect();
    }

    pub fn put_actor(&mut self, id: u16, x: i32, z: i32) {
        let a = self.actor_object(id);
        if a != 0 {
            self.gba.write32(a + self.s.l.obj_x, fx(x) as u32);
            self.gba.write32(a + self.s.l.obj_z, fx(z) as u32);
        }
    }

    fn actor_object(&self, id: u16) -> u32 {
        let work = self.gba.read32(self.s.field_work);
        if u32::from(id) >= self.s.l.field_actor_count || work == 0 {
            return 0;
        }
        self.gba.read32(work + self.s.l.field_actors + 4 * u32::from(id))
    }

    /// Stops drawing actor `id` (its object kind 0, which the camera still
    /// follows when it is the leader).
    pub fn hide_actor(&mut self, id: u16) {
        let a = self.actor_object(id);
        if a != 0 {
            self.gba.write8(a + self.s.l.obj_kind, 0);
        }
    }

    /// Actor `id`'s object as it is now, when the scene has one.
    pub fn actor(&self, id: u16) -> Option<Actor> {
        let a = self.actor_object(id);
        (a != 0).then(|| {
            let word = |o: u32| self.gba.read32(a + o) as i32;
            Actor {
                x: word(self.s.l.obj_x) >> 16,
                z: word(self.s.l.obj_z) >> 16,
                y: word(self.s.l.obj_y),
                layer: self.gba.read8(a + self.s.l.obj_terrain_id),
                drawn: self.gba.read8(a + self.s.l.obj_kind) & 15 != 0,
            }
        })
    }

    /// The actors the scene keeps parked in the map's corner cell, at its
    /// origin, until it needs them (Alex on Mercury Lighthouse's aerie,
    /// before he appears): the game's camera never shows that corner.
    pub fn parked(&self) -> Vec<Actor> {
        (0..self.s.l.field_actor_count as u16).filter_map(|id| self.actor(id)).filter(|a| (0..16).contains(&a.x) && (0..16).contains(&a.z)).collect()
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
        (FIRST_PLACED..self.s.l.field_actor_count as u16).filter_map(|id| self.present(id).ok().map(|a| (id, a.x, a.z))).collect()
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
        let cells = self.gba.read32(self.map_work() + self.s.l.map_layers + self.s.l.map_layer_cells + self.s.l.map_layer_size * u32::from(layer));
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
        let cells = self.gba.read32(self.map_work() + self.s.l.map_layers + self.s.l.map_layer_cells + self.s.l.map_layer_size * u32::from(layer));
        self.gba.read32(cells + ((cx + (cz << 7)) << 2) as u32) as i32
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
        let kind = self.gba.read8(object + self.s.l.obj_kind) & 15;
        let first = self.gba.read32(object + self.s.l.obj_sprite);
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
        for id in FIRST_PLACED..self.s.l.field_actor_count as u16 {
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
        let mut a = if work == 0 { 0 } else { self.gba.read32(work + self.s.l.field_events) };
        let mut out = Vec::new();
        for _ in 0..1024 {
            if a == 0 || self.gba.read32(a) == u32::MAX {
                break;
            }
            out.push(((self.gba.read32(a) & 15) as u8, u16::from(self.gba.read8(a + 4)), result_key(self.gba.read32(a + 8))));
            a += self.s.l.event_row_size;
        }
        out
    }

    /// The rows of the trigger table the field holds now, whole: each
    /// control word, target (its halfword), condition and result.
    pub fn trigger_rows(&self) -> Vec<super::triggers::Row> {
        let work = self.gba.read32(self.s.field_work);
        let mut a = if work == 0 { 0 } else { self.gba.read32(work + self.s.l.field_events) };
        let mut out = Vec::new();
        for _ in 0..1024 {
            if a == 0 || self.gba.read32(a) == u32::MAX {
                break;
            }
            out.push(super::triggers::Row { control: self.gba.read32(a), target: self.gba.read16(a + 4), condition: self.gba.read16(a + 6) as i16, result: self.gba.read32(a + 8) });
            a += self.s.l.event_row_size;
        }
        out
    }

    /// The scene's event rows for `kind` and `target`, in table order.
    fn event_rows(&self, kind: u8, target: u16) -> Vec<EventRow> {
        let work = self.gba.read32(self.s.field_work);
        let mut a = if work == 0 { 0 } else { self.gba.read32(work + self.s.l.field_events) };
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
            a += self.s.l.event_row_size;
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
        let selected = self.gba.read32(self.s.game_state + self.s.l.gs_selected_actor);
        self.actor_object(selected as u16)
    }

    /// Puts the leader at `spot`: on its layer, standing on its ground and
    /// facing its way.
    pub fn put_leader(&mut self, spot: &Spot) {
        let a = self.leader_object();
        if a == 0 {
            return;
        }
        self.gba.write32(a + self.s.l.obj_x, fx(spot.x) as u32);
        self.gba.write32(a + self.s.l.obj_z, fx(spot.z) as u32);
        self.gba.write32(a + self.s.l.obj_y, spot.height as u32);
        self.gba.write32(a + self.s.l.obj_terrain_height, spot.height as u32);
        self.gba.write8(a + self.s.l.obj_terrain_id, spot.layer);
        self.gba.write16(a + self.s.l.obj_angle, spot.facing as u16);
        for i in 0..3 {
            self.gba.write32(a + self.s.l.obj_velocity + 4 * i, 0);
        }
    }

    /// What of `misplaced` counts against a start that placed the leader:
    /// off the screen shows nothing wrong, and the camera may yet come to him.
    fn wrong_for_a_placed_start(mut wrong: Vec<String>) -> Vec<String> {
        wrong.retain(|why| !why.starts_with("he is off the screen"));
        wrong
    }

    /// Spots on open ground in the leader's room, nearest him first, 32 pixels
    /// apart: every cell around each is ground he can stand on, nothing
    /// starts there, and no actor stands on it.
    fn open_ground_near(&mut self, leader: &Actor, facing: u32) -> Result<Vec<Spot>, String> {
        let layer = leader.layer.min(2);
        let room = self.room();
        let stage = self.stage();
        let mut cells = Vec::new();
        for cz in (room[1] >> 4)..=(room[3] >> 4) {
            for cx in (room[0] >> 4)..=(room[2] >> 4) {
                let (x, z) = (cx * 16 + 8, cz * 16 + 8);
                if x < room[0] + 16 || x >= room[2] - 16 || z < room[1] + 16 || z >= room[3] - 16 || !self.inside(x, z, 16) {
                    continue;
                }
                let open = (-1..=1).all(|dz| (-1..=1).all(|dx| self.quiet(self.cell(layer, cx + dx, cz + dz)) && Self::standable(self.cell(layer, cx + dx, cz + dz))));
                if open && self.in_open_region(layer, cx, cz) && !stage.iter().any(|&(_, ax, az)| (ax - x).abs() < 24 && (az - z).abs() < 24) {
                    let (dx, dz) = (x - leader.x, z - leader.z);
                    cells.push((dx * dx + dz * dz, x, z));
                }
            }
        }
        cells.sort_unstable();
        let mut chosen: Vec<(i32, i32)> = Vec::new();
        let mut out = Vec::new();
        for (_, x, z) in cells {
            if out.len() == NEAR_SPOTS {
                break;
            }
            if chosen.iter().any(|&(a, b)| (a - x).abs() < 32 && (b - z).abs() < 32) {
                continue;
            }
            chosen.push((x, z));
            let height = self.ground(layer, x, z)?;
            out.push(Spot { x, z, layer, height, facing, walk: 0, cell: None });
        }
        Ok(out)
    }

    /// Why the frame just played shows the leader where the party cannot be,
    /// and for how many frames that may last before the recording is false: a
    /// hop passes over a wall or a gap in moments, while a lift or a fall may
    /// hold him off the ground for longer. Nothing while the game has him
    /// undrawn, off the screen, or riding an actor (a log, a platform). The
    /// cell grid beyond a map reads as open ground. `off_ground` is how far off
    /// the kept height he stood when he was placed, which `misplaced` passed.
    fn astray(&self, off_ground: i32) -> Option<(String, u32)> {
        let leader = self.leader()?;
        if !leader.drawn || self.world() {
            return None;
        }
        let (x, z) = (leader.x, leader.z);
        if self.stage().iter().any(|&(_, ax, az)| (ax - x).abs() <= RIDE_REACH && (az - z).abs() <= RIDE_REACH) {
            return None;
        }
        let layer = leader.layer.min(2);
        let (vx, vy) = self.view();
        let (sx, sy) = (x - vx, z - (leader.y >> 16) - vy);
        if !(0..WIDTH as i32).contains(&sx) || !(0..HEIGHT as i32).contains(&sy) {
            return None;
        }
        match self.code_at(layer, x, z) {
            -1 => return Some((format!("({x}, {z}) is off the cell grid"), ASTRAY_FRAMES)),
            CELL_WALL => return Some((format!("({x}, {z}) is a wall cell of layer {layer}"), ASTRAY_FRAMES)),
            _ => {}
        }
        let room = self.room();
        if x < room[0] || x >= room[2] || z < room[1] || z >= room[3] {
            return Some((format!("({x}, {z}) is outside his room's camera box {room:?}"), ASTRAY_FRAMES));
        }
        let black = self.black_around(sx, sy);
        if black > 75 {
            return Some((format!("he stands in black, {black}% of the screen around him"), BLACK_FRAMES));
        }
        // The height the game keeps for him: `ground` would run the game on to
        // a step of the field sequence, which a script has stopped.
        let ground = self.gba.read32(self.leader_object() + self.s.l.obj_terrain_height) as i32;
        if (leader.y - ground - off_ground).abs() > 2 << 16 {
            return Some((format!("he stands {} pixels off the ground", (leader.y - ground - off_ground) >> 16), ALOFT_FRAMES));
        }
        None
    }

    /// What the screen says against where a start placed the leader, which the
    /// cell grid cannot: that what stands in front hides him, or that the map
    /// draws nothing at his feet. Nothing while he is off the screen.
    fn unseen(&mut self) -> Result<Option<String>, String> {
        let saved = self.save();
        let result = self.look_at_leader();
        self.load(&saved);
        result
    }

    fn look_at_leader(&mut self) -> Result<Option<String>, String> {
        self.frames(2)?;
        let Some(leader) = self.leader() else {
            return Ok(None);
        };
        let (vx, vy) = self.view();
        let (sx, sy) = (leader.x - vx, leader.z - (leader.y >> 16) - vy);
        if !(0..WIDTH as i32).contains(&sx) || !(0..HEIGHT as i32).contains(&sy) {
            return Ok(None);
        }
        let with_him = self.screen();
        let mut colours = std::collections::HashMap::new();
        let mut feet = 0;
        for py in (sy + 1).max(0)..(sy + 10).min(HEIGHT as i32) {
            for px in (sx - 14).max(0)..(sx + 14).min(WIDTH as i32) {
                feet += 1;
                *colours.entry(with_him.pixels[py as usize * WIDTH + px as usize]).or_insert(0) += 1;
            }
        }
        let flat = if feet > 0 { colours.values().max().copied().unwrap_or(0) * 100 / feet } else { 0 };
        if flat >= VOID_AT_FEET {
            return Ok(Some(format!("the map draws nothing at his feet, {flat}% of them one flat colour")));
        }
        let selected = self.gba.read32(self.s.game_state + self.s.l.gs_selected_actor) as u16;
        self.hide_actor(selected);
        self.frames(2)?;
        let without = self.screen();
        let mut seen = 0;
        for py in (sy - 32).max(0)..(sy + 6).min(HEIGHT as i32) {
            for px in (sx - 14).max(0)..(sx + 14).min(WIDTH as i32) {
                let i = py as usize * WIDTH + px as usize;
                if with_him.pixels[i] != without.pixels[i] {
                    seen += 1;
                }
            }
        }
        Ok(((UNMEASURED_BELOW..HIDDEN_BELOW).contains(&seen)).then(|| format!("something stands in front of him: {seen} pixels of him show")))
    }

    /// Whether (cx, cz) lies in a connected stretch of open ground of at least
    /// OPEN_REGION cells on `layer`.
    fn in_open_region(&self, layer: u8, cx: i32, cz: i32) -> bool {
        let mut seen = std::collections::HashSet::from([(cx, cz)]);
        let mut next = vec![(cx, cz)];
        while let Some((x, z)) = next.pop() {
            if seen.len() >= OPEN_REGION {
                return true;
            }
            for (nx, nz) in [(x + 1, z), (x - 1, z), (x, z + 1), (x, z - 1)] {
                if self.inside(nx * 16 + 8, nz * 16 + 8, 0) && Self::standable(self.cell(layer, nx, nz)) && seen.insert((nx, nz)) {
                    next.push((nx, nz));
                }
            }
        }
        seen.len() >= OPEN_REGION
    }

    /// How much of the screen within 24 pixels of (sx, sy), the leader's own
    /// sprite left out, is black where the map draws nothing, in percent.
    fn black_around(&self, sx: i32, sy: i32) -> i32 {
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
        if all > 0 {
            black * 100 / all
        } else {
            0
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
            let black = self.black_around(sx, sy);
            if black > 80 {
                out.push(format!("he stands in black, {black}% of the screen around him"));
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
        let selected = self.gba.read32(self.s.game_state + self.s.l.gs_selected_actor) as u16;
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

    /// The facings in which walking on a cell with collision code `code`
    /// starts its rows, when every row that holds now fires only as he
    /// walks into it facing one way (STEP_TRIGGER_WALKING_INTO); None when
    /// one fires however he steps on it, or leaves the scene.
    fn facings(&self, code: i32) -> Option<Vec<u32>> {
        if !(1..=CELL_EVENT_LAST).contains(&code) || self.event_rows(TRIGGER_EXIT, code as u16).iter().any(|r| self.condition_holds(r.condition)) {
            return None;
        }
        let mut out = Vec::new();
        for r in self.event_rows(TRIGGER_STEP, code as u16).iter().filter(|r| self.condition_holds(r.condition)) {
            if r.flags & STEP_TRIGGER_FLAG_WALKING == 0 || r.flags & TRIGGER_FLAG_FACING == 0 {
                return None;
            }
            out.push(r.flags & 0xf000);
        }
        Some(out)
    }

    /// Whether a climb cell whose north holds code `code` is the top of its
    /// ladder, which Up climbs him over onto that cell: ground, or a cell
    /// that starts something (the mast's way back down).
    fn over_the_top(code: i32) -> bool {
        code >= 0 && code != CELL_WALL && code != CELL_CLIMB && code != CELL_LEDGE && code != CELL_JUMP
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
        let selected = self.gba.read32(self.s.game_state + self.s.l.gs_selected_actor) as u16;
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
                // than JUMP_DROP below where he took off. A climb takes him
                // only to the climb cell next to his, north or south
                // (Field_ClimbUp and Field_ClimbDown need one there), where
                // he stands again once he has climbed over its top or let go
                // at its foot (Tret Tree's vines).
                // At the top of a ladder (the climb cell's north is
                // ground), Up climbs him over it onto that ground
                // (Field_ClimbUp while he climbs): the mast's ladder up to
                // Tolbi's crow's nest.
                if here == CELL_CLIMB {
                    let over_the_top = d == 0 && Self::over_the_top(code);
                    if ((d == 0 || d == 2) && code == CELL_CLIMB || over_the_top) && !cells[index(n)] {
                        cells[index(n)] = true;
                        queue.push_back(n);
                    }
                    continue;
                }
                let way = match here {
                    CELL_LEDGE => d == 2,
                    CELL_JUMP => true,
                    _ => false,
                };
                if !way {
                    continue;
                }
                let from = self.ground(layer, c.0 * 16 + 8, c.1 * 16 + 8)?;
                let reach_cells = if here == CELL_JUMP { 2..=2 } else { 1..=4 };
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
                    if (-JUMP_DROP..=STEP_UP).contains(&rise) {
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
                field.gba.write32(work + self.s.l.map_bounds + 4 * i as u32, v as u32);
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
        let layers: Vec<u32> = (0..3).flat_map(|l| [0, 4].map(|o| self.gba.read32(work + self.s.l.map_layers + self.s.l.map_layer_size * l + o))).collect();
        pin(self, false);
        for (i, &word) in maps.iter().enumerate() {
            self.gba.write32(PIN_SCREEN_MAPS + 4 * i as u32, word);
        }
        for (i, &word) in layers.iter().enumerate() {
            self.gba.write32(work + self.s.l.map_layers + self.s.l.map_layer_size * (i as u32 / 2) + 4 * (i as u32 % 2), word);
        }
        self.gba.write32(work + self.s.l.map_view_x, view.0 as u32);
        self.gba.write32(work + self.s.l.map_view_y, view.1 as u32);
        for l in 0..3u32 {
            let at = work + self.s.l.map_layers + self.s.l.map_layer_size * l;
            let word = |o: u32| self.gba.read32(at + o) as i32;
            // Each axis: the view times the layer's parallax, wrapped with its
            // drift when it drifts, plus its offset; BG3 is the first layer.
            let axis = |v: i32, k: u32| {
                let mut base = ((i64::from(v) * i64::from(word(self.s.l.layer_scale + 4 * k))) >> 16) as i32;
                if word(self.s.l.layer_speed + 4 * k) != 0 {
                    let mask = i32::from(self.gba.read16(at + self.s.l.layer_mask + 2 * k));
                    base = (base + word(self.s.l.layer_phase + 4 * k)) & ((mask << 19) | 0x7ffff);
                }
                base + word(self.s.l.layer_offset + 4 * k)
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
        (self.gba.read32(work + self.s.l.map_view_x) as i32 >> 16, self.gba.read32(work + self.s.l.map_view_y) as i32 >> 16)
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
                    self.gba.write32(work + self.s.l.map_bounds + 4 * i as u32, v as u32);
                }
                if !leader {
                    let selected = self.gba.read32(self.s.game_state + self.s.l.gs_selected_actor);
                    let object = self.actor_object(selected as u16);
                    if object != 0 {
                        self.gba.write8(object + self.s.l.obj_kind, 0);
                    }
                }
                self.t.arm_center = true;
                self.frames(HOLD)?;
                if self.t.arm_center {
                    return Err("MapGrid_UpdateLayerScroll never ran".to_string());
                }
                let view_x = self.gba.read32(work + self.s.l.map_view_x) as i32;
                let view_y = self.gba.read32(work + self.s.l.map_view_y) as i32;
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

    /// The moves the leader can make between the cells of `reach`
    /// (approach.rs): walks between cells that start nothing as he stands on
    /// them (or the cells of `keep`, where a walk starts or ends), climbs
    /// between climb cells one above another, and drops down ledges; never
    /// onto a cell an actor stands on.
    fn moves(&mut self, reach: &Reach, keep: &[Option<approach::Cell>]) -> Result<Vec<Move>, String> {
        let layer = reach.layer;
        let stage = self.stage();
        let mut ground: HashMap<(i32, i32), i32> = HashMap::new();
        let mut height = |field: &mut Self, x: i32, z: i32| -> Result<i32, String> {
            if let Some(&h) = ground.get(&(x, z)) {
                return Ok(h);
            }
            let h = field.ground(layer, x, z)?;
            ground.insert((x, z), h);
            Ok(h)
        };
        // Each cell's ways: None where he may not walk, else the facings
        // in which walking on it starts something. A cell whose every row
        // fires only as he walks into it facing one way (a hole's rim in
        // Osenia Cavern, walked into south, west or east) is crossed the
        // other ways, as the player crosses it.
        let mut open: HashMap<approach::Cell, Option<Vec<u32>>> = HashMap::new();
        let mut ways = |field: &Self, c: approach::Cell| -> Option<Vec<u32>> {
            open.entry(c)
                .or_insert_with(|| {
                    let (x, z) = (c.0 * 16 + 8, c.1 * 16 + 8);
                    let code = field.cell(layer, c.0, c.1);
                    let free = !stage.iter().any(|&(_, ax, az)| (ax - x).abs() < 16 && (az - z).abs() < 16);
                    if !reach.holds(layer, x, z) || !field.inside(x, z, 8) || !free {
                        return None;
                    }
                    if keep.contains(&Some(c)) || code == CELL_CLIMB || (Self::standable(code) && field.quiet(code)) {
                        return Some(Vec::new());
                    }
                    field.facings(code)
                })
                .clone()
        };
        let allows = |ways: Option<Vec<u32>>, d: usize| ways.is_some_and(|facings| facings.iter().all(|&f| !facing_within(FACING[d], f)));
        let (x0, z0, x1, z1) = (reach.room[0] >> 4, reach.room[1] >> 4, (reach.room[2] + 15) >> 4, (reach.room[3] + 15) >> 4);
        let mut out = Vec::new();
        for cz in z0..z1 {
            for cx in x0..x1 {
                let c = (cx, cz);
                if ways(self, c).is_none() {
                    continue;
                }
                let here = self.cell(layer, cx, cz);
                for d in 0..4 {
                    let n = (cx + STEP[d].0, cz + STEP[d].1);
                    if !allows(ways(self, c), d) || !allows(ways(self, n), d) {
                        continue;
                    }
                    let there = self.cell(layer, n.0, n.1);
                    if here == CELL_CLIMB && there == CELL_CLIMB && STEP[d].1 != 0 {
                        out.push(Move { from: c, to: n, keys: KEYS[d], how: How::Climb });
                        continue;
                    }
                    // As Field::passable, from heights kept once read.
                    let mut heights = [0; 5];
                    for (i, h) in heights.iter_mut().enumerate() {
                        let along = 4 * i as i32;
                        *h = height(self, cx * 16 + 8 + STEP[d].0 * along, cz * 16 + 8 + STEP[d].1 * along)?;
                    }
                    if (0..3).all(|i| (-STEP_DOWN..=STEP_UP).contains(&(heights[i + 2] - heights[i]))) {
                        out.push(Move { from: c, to: n, keys: KEYS[d], how: How::Walk });
                    }
                }
                // Down a ledge: the first cell beyond on the ground below.
                if here == CELL_LEDGE {
                    let from = height(self, cx * 16 + 8, cz * 16 + 8)?;
                    for k in 1..=4 {
                        let m = (cx, cz + k);
                        let beyond = self.cell(layer, m.0, m.1);
                        if beyond == CELL_WALL || beyond == CELL_LEDGE {
                            continue;
                        }
                        let rise = height(self, m.0 * 16 + 8, m.1 * 16 + 8)? - from;
                        if (-JUMP_DROP..=STEP_UP).contains(&rise) {
                            if allows(ways(self, m), 2) {
                                out.push(Move { from: c, to: m, keys: KEY_DOWN, how: How::Drop });
                            }
                            break;
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    /// The way the leader walks up to `spot` from some cells back
    /// (approach::approach): enough of them to fill the lead-in, at least
    /// approach::APPROACH_CELLS. Or why he has none.
    fn plan_approach(&mut self, spot: &Spot, origin: Option<approach::Cell>) -> Result<Result<Vec<Move>, String>, String> {
        // Reached past the trigger's other cells (a hole three cells wide,
        // entered from any side): the moves never step on a cell that
        // starts something but the spot.
        let reach = self.reach(None)?;
        let cell = (spot.x >> 4, spot.z >> 4);
        let moves = self.moves(&reach, &[Some(cell), origin])?;
        let wanted = (self.lead_in.div_ceil(approach::FRAMES_PER_CELL) as usize).max(approach::APPROACH_CELLS);
        // The end of the way from where the party was, as the player comes
        // (up the vine to a branch only the vine reaches); else the way up
        // from any side.
        if let Some(way) = origin.and_then(|o| approach::route(&moves, o, cell)).filter(|way| way.len() >= wanted) {
            return Ok(Ok(way[way.len() - wanted..].to_vec()));
        }
        Ok(approach::approach(&moves, cell, spot.walk, wanted, approach::APPROACH_LEAST)
            .ok_or_else(|| format!("no walk of {} cells or more leads up to it", approach::APPROACH_LEAST)))
    }

    /// The leader's place as it changes from frame to frame: pixels, and his
    /// height in pixels.
    fn leader_place(&self) -> Option<(i32, i32, i32)> {
        self.leader().map(|a| (a.x, a.z, a.y >> 16))
    }

    /// Runs a frame with the keys held now, noting whether the leader moved
    /// in it (`moving`) and what was held (`pad`).
    fn step_watching(&mut self, moving: &mut Vec<bool>, pad: &mut Pad) -> Result<bool, String> {
        let before = self.leader_place();
        pad.push(self.t.keys);
        self.frames(1)?;
        let moved = self.leader_place() != before;
        moving.push(moved);
        Ok(moved)
    }

    /// After a push, the leader goes on pushing the same way (`keys`) with
    /// the pad, as long as one of `ids` keeps moving, until it has lain
    /// still for PUSH_REST_FRAMES or a flag not set in `before` (flag_bytes,
    /// as the event began) is; then until all is still. How many more times
    /// it moved, and whether such a flag is set; None, with the state and
    /// the recording as they were, where pushing on stopped on a message or
    /// left the scene.
    fn push_on(&mut self, keys: u32, ids: &[u16], before: &[u8], pad: &mut Pad) -> Result<Option<(u32, bool)>, String> {
        let goal = |f: &Self| f.flag_bytes().iter().zip(before).any(|(now, was)| now & !was != 0);
        if goal(self) {
            return Ok(Some((0, true)));
        }
        let saved = self.save();
        let kept = self.recorded.as_ref().map_or(0, Vec::len);
        let places = |f: &Self| -> Vec<(i32, i32)> { ids.iter().map(|&id| f.actor(id).map_or((0, 0), |a| (a.x, a.z))).collect() };
        let mut last = places(self);
        let mut rest = places(self);
        let (mut still, mut pushes, mut reached) = (0, 0, false);
        let dismissed = self.t.last_dismiss;
        let mut moving = Vec::new();
        for _ in 0..PUSH_ON_FRAMES {
            self.hold(if reached { 0 } else { keys });
            self.step_watching(&mut moving, pad)?;
            let gone = self.scene() != self.setup.scene || self.t.last_dismiss != dismissed || self.recorded.as_ref().and_then(|f| f.last()).is_some_and(|p| is_black_bgr555(p));
            if gone {
                self.load(&saved);
                if let Some(frames) = self.recorded.as_mut() {
                    frames.truncate(kept);
                }
                return Ok(None);
            }
            let now = places(self);
            if now != last {
                last = now.clone();
                still = 0;
            } else {
                still += 1;
                // A move that came to rest is one more push.
                if still == STILL_FRAMES && now.iter().zip(&rest).any(|(a, b)| (a.0 - b.0).abs().max((a.1 - b.1).abs()) >= 8) {
                    pushes += 1;
                    rest = now;
                }
            }
            reached |= goal(self);
            if still >= PUSH_REST_FRAMES {
                break;
            }
        }
        self.hold(0);
        self.frames(ONWARD_TAIL)?;
        Ok(Some((pushes, reached)))
    }

    /// The cells (x, z) whose word in any layer differs from `before`
    /// (cell_codes, taken earlier in the same scene): what an event drew or
    /// opened in the map.
    fn changed_cells(&self, before: &[i32]) -> Vec<(i32, i32)> {
        let now = self.cell_codes();
        if now.len() != before.len() || now.is_empty() {
            return Vec::new();
        }
        let h = self.t.header;
        let (x0, z0, x1, z1) = (h[0] >> 20, h[1] >> 20, (h[2] + 0xfffff) >> 20, (h[3] + 0xfffff) >> 20);
        let (w, rows) = ((x1 - x0).max(0) as usize, (z1 - z0).max(0) as usize);
        let mut out: Vec<(i32, i32)> = Vec::new();
        for (i, _) in now.iter().zip(before).enumerate().filter(|(_, (a, b))| a != b) {
            let (cz, cx) = ((i % (w * rows)) / w, i % w);
            let cell = (x0 + cx as i32, z0 + cz as i32);
            if !out.contains(&cell) {
                out.push(cell);
            }
        }
        out
    }

    /// Whether cell (cx, cz) is on the screen now.
    fn cell_on_screen(&mut self, layer: u8, cx: i32, cz: i32) -> Result<bool, String> {
        let (x, z) = (cx * 16 + 8, cz * 16 + 8);
        let height = self.ground(layer, x, z)? >> 16;
        let (vx, vy) = self.view();
        Ok((0..WIDTH as i32).contains(&(x - vx)) && (0..HEIGHT as i32).contains(&(z - height - vy)))
    }

    /// After an event that changed the map where the leader's sprite covers
    /// it (the hole Scoop digs in the cell he faces), he steps back from it,
    /// two cells where he can and else one, or aside, with the pad, so what
    /// it drew shows instead of his back. Whether he did.
    fn step_back(&mut self, changed: &[(i32, i32)], pad: &mut Pad) -> Result<bool, String> {
        let Some(a) = self.leader() else { return Ok(false) };
        let (lx, lz) = (a.x >> 4, a.z >> 4);
        // His sprite stands about two cells tall over his own.
        if !changed.iter().any(|&(cx, cz)| (cx - lx).abs() <= 1 && (-1..=2).contains(&(lz - cz))) {
            return Ok(false);
        }
        let layer = a.layer.min(2);
        let angle = u32::from(self.gba.read16(self.leader_object() + self.s.l.obj_angle));
        // 0x0000 faces east, 0x4000 south, 0x8000 west, 0xc000 north.
        let ahead = (((angle + 0x2000) & 0xffff) / 0x4000 + 1) as usize % 4;
        for d in [(ahead + 2) % 4, (ahead + 1) % 4, (ahead + 3) % 4] {
            // A cell or two that way he can walk to, drawn anew or not.
            let mut cells = 0;
            for k in 0..2 {
                let (cx, cz) = (lx + STEP[d].0 * k, lz + STEP[d].1 * k);
                let code = self.cell(layer, cx + STEP[d].0, cz + STEP[d].1);
                if !Self::standable(code) || !self.quiet(code) || !self.passable(layer, (cx, cz), d)? {
                    break;
                }
                cells += 1;
            }
            if cells == 0 {
                continue;
            }
            let (x0, z0) = (a.x, a.z);
            let mut moving = Vec::new();
            self.hold(KEYS[d]);
            for _ in 0..2 * STUCK_FRAMES {
                self.step_watching(&mut moving, pad)?;
                let now = self.leader().ok_or("the leader has no actor")?;
                if (now.x - x0).abs().max((now.z - z0).abs()) >= 16 * cells - 2 {
                    break;
                }
            }
            self.hold(0);
            self.frames(ONWARD_TAIL)?;
            return Ok(true);
        }
        Ok(false)
    }

    /// `cells` split into those in a room some entrance's camera box shows,
    /// the room the run entered first, and how many lie in none: the copy a
    /// scene keeps beside its map of cells it draws in later (Osenia's hole
    /// in Caverns, the log's crossing in Mogall Forest), which no player
    /// reaches and no trigger fires from.
    fn in_rooms(&self, cells: Vec<(u8, i32, i32)>) -> (Vec<(u8, i32, i32)>, usize) {
        let cameras: Vec<[i16; 4]> = self.entrances().iter().map(|e| e.camera).collect();
        in_rooms(cells, &cameras, self.header(), self.room(), self.cell_size())
    }

    /// Draws every layer of the map again around the point the camera
    /// follows (MapGrid_CenterOnFollowedPoint), as the game does once it has
    /// placed the party.
    fn redraw_view(&mut self) -> Result<(), String> {
        let origin = self.gba.read32(self.gba.read32(self.s.map_work));
        if origin == 0 || self.world() {
            return Ok(());
        }
        let (x, z) = (self.gba.read32(origin) as i32, self.gba.read32(origin + 8) as i32);
        self.call(self.s.center, [x, z])?;
        self.frames(2)
    }

    /// Whether the leader is climbing (MainRoutineWork.movementMode).
    fn climbing(&self) -> bool {
        self.gba.read8(self.s.game_state + self.s.l.gs_movement_mode) != 0
    }

    /// Whether the leader is still on the way of `m`: on its layer, within
    /// OFF_THE_WAY pixels of the cells it goes between. Else the game has
    /// taken him somewhere the cells did not show, and the walk has failed.
    fn on_the_way(&self, m: &Move, layer: u8) -> Result<(), String> {
        let a = self.leader().ok_or("the leader has no actor")?;
        let (lo_x, hi_x) = (m.from.0.min(m.to.0) * 16 + 8 - OFF_THE_WAY, m.from.0.max(m.to.0) * 16 + 8 + OFF_THE_WAY);
        let (lo_z, hi_z) = (m.from.1.min(m.to.1) * 16 + 8 - OFF_THE_WAY, m.from.1.max(m.to.1) * 16 + 8 + OFF_THE_WAY);
        if a.layer != layer || !(lo_x..=hi_x).contains(&a.x) || !(lo_z..=hi_z).contains(&a.z) {
            return Err(format!("he left the way from cell ({}, {}) to ({}, {}), at ({}, {}) on layer {}", m.from.0, m.from.1, m.to.0, m.to.1, a.x, a.z, a.layer));
        }
        self.in_sight()
    }

    /// Whether the leader the recording follows is on the screen: a walk
    /// the camera does not show (a room's box it stops at, a camera that
    /// has not caught up) shows nothing coming to the trigger.
    fn in_sight(&self) -> Result<(), String> {
        let a = self.leader().ok_or("the leader has no actor")?;
        let (vx, vy) = self.view();
        let (sx, sy) = (a.x - vx, a.z - (a.y >> 16) - vy);
        if !a.drawn || !(0..WIDTH as i32).contains(&sx) || !(8..HEIGHT as i32 + 8).contains(&sy) {
            return Err(format!("he is off the screen at ({sx}, {sy}) on it"));
        }
        Ok(())
    }

    /// Walks the leader along `path` with the pad, as a player would: each
    /// run of walks one way with its key held (and the key across, while he
    /// is off its line), each climb or drop with its key held until he stands
    /// on its far cell. The last walk hands over two cells early when it goes
    /// the way `then` pushes on, so he walks on into the trigger without a stop.
    fn follow(&mut self, path: &[Move], then: u32, moving: &mut Vec<bool>, pad: &mut Pad) -> Result<(), String> {
        let layer = self.leader().ok_or("the leader has no actor")?.layer;
        let mut i = 0;
        while i < path.len() {
            let m = path[i];
            if m.how != How::Walk {
                let mut frames = 0;
                let mut landed = 0;
                loop {
                    self.hold(m.keys);
                    let moved = self.step_watching(moving, pad)?;
                    self.on_the_way(&m, layer)?;
                    frames += 1;
                    let there = self.leader().is_some_and(|a| (a.x >> 4, a.z >> 4) == m.to);
                    landed = if there && !moved && !self.climbing() { landed + 1 } else { 0 };
                    if landed >= LANDED_FRAMES {
                        break;
                    }
                    if frames > CLIMB_FRAMES {
                        self.hold(0);
                        return Err(format!("holding {:#x} on cell ({}, {}) did not take him to cell ({}, {})", m.keys, m.from.0, m.from.1, m.to.0, m.to.1));
                    }
                }
                i += 1;
                continue;
            }
            let mut j = i;
            while j + 1 < path.len() && path[j + 1].how == How::Walk && path[j + 1].keys == m.keys {
                j += 1;
            }
            let end = path[j].to;
            let hand_over = if j + 1 == path.len() && m.keys == then { 32 } else { 2 };
            let d = KEYS.iter().position(|&k| k == m.keys).ok_or("a walk with no direction key")?;
            let (dx, dz) = STEP[d];
            let (tx, tz) = (end.0 * 16 + 8, end.1 * 16 + 8);
            let mut stuck = 0;
            loop {
                let a = self.leader().ok_or("the leader has no actor")?;
                if (tx - a.x) * dx + (tz - a.z) * dz <= hand_over {
                    break;
                }
                // Back onto the line of cells when he has drifted off it.
                let side = if dx != 0 { tz - a.z } else { tx - a.x };
                let across = match (dx != 0, side) {
                    (_, -2..=2) => 0,
                    (true, s) if s > 0 => KEY_DOWN,
                    (true, _) => KEY_UP,
                    (false, s) if s > 0 => KEY_RIGHT,
                    (false, _) => KEY_LEFT,
                };
                self.hold(m.keys | across);
                stuck = if self.step_watching(moving, pad)? { 0 } else { stuck + 1 };
                self.on_the_way(&Move { from: m.from, to: end, ..m }, layer)?;
                if stuck > STUCK_FRAMES {
                    self.hold(0);
                    return Err(format!("he made no headway from ({}, {}) toward cell ({}, {})", a.x, a.z, end.0, end.1));
                }
            }
            i = j + 1;
        }
        Ok(())
    }

    /// After an event that moved the leader without showing it and leads to
    /// no climb, he walks on as he was for a few steps (while the ground ahead
    /// starts nothing), so the recording shows that nothing came of it.
    fn walk_on(&mut self, keys: u32, pad: &mut Pad) -> Result<(), String> {
        if let Some(d) = KEYS.iter().position(|&k| k == keys) {
            let mut moving = Vec::new();
            for _ in 0..ONWARD_TAIL {
                let Some(a) = self.leader() else { break };
                let ahead = self.code_at(a.layer.min(2), a.x + STEP[d].0 * 12, a.z + STEP[d].1 * 12);
                if !self.quiet(ahead) {
                    break;
                }
                self.hold(keys);
                if !self.step_watching(&mut moving, pad)? {
                    break;
                }
            }
        }
        self.hold(0);
        self.frames(ONWARD_TAIL)
    }

    /// After an event that moved the leader without showing it, the climb
    /// it lets him make from where it put him (approach::onward), walked and
    /// climbed with the pad: what the move was for. Whether there was one.
    fn climb_onward(&mut self, moving: &mut Vec<bool>, pad: &mut Pad) -> Result<Option<u32>, String> {
        let reach = self.reach(None)?;
        let leader = self.leader().ok_or("the leader has no actor")?;
        let moves = self.moves(&reach, &[])?;
        let Some(path) = approach::onward(&moves, (leader.x >> 4, leader.z >> 4), ONWARD_WALKS) else {
            return Ok(None);
        };
        let keys = path.last().map_or(0, |m| m.keys);
        self.follow(&path, 0, moving, pad)?;
        self.hold(0);
        self.frames(ONWARD_TAIL)?;
        Ok(Some(keys))
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
        self.resettled = false;
        // A direct start needs no place: the leader stays where the scene's
        // arrival left him, facing as it left him, unless the party cannot be
        // there; then the nearest open ground has him. A message row spoken
        // by an actor is heard beside that actor.
        if let Trigger::Direct(_) | Trigger::Cast(_) | Trigger::Message(..) = trigger {
            let leader = self.leader().ok_or("the leader has no actor")?;
            let facing = self.gba.read32(self.s.game_state + self.s.l.gs_heading);
            let here = Spot { x: leader.x, z: leader.z, layer: leader.layer, height: leader.y, facing, walk: 0, cell: None };
            let mut out = Vec::new();
            if let Trigger::Message(_, actor) = trigger {
                if actor != u16::MAX {
                    let reach = self.reach(None)?;
                    let beside = self.spots_in(Trigger::Talk(actor), &reach).or_else(|_| self.spots_in(Trigger::Talk(actor), &reach.anywhere()));
                    out.extend(beside.unwrap_or_default().into_iter().take(BESIDE_SPOTS));
                }
            }
            if !out.is_empty() {
                return Ok(out);
            }
            let wrong = Self::wrong_for_a_placed_start(self.misplaced()?);
            if wrong.is_empty() {
                return Ok(vec![here]);
            }
            self.resettled = true;
            let near = self.open_ground_near(&leader, facing)?;
            return if near.is_empty() { Err(format!("the arrival leaves the leader where the party cannot be ({}) and no open ground is near", wrong.join(", "))) } else { Ok(near) };
        }
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
            (FIRST_PLACED..self.s.l.field_actor_count as u16).filter_map(|id| self.present(id).ok().map(|a| (id, a))).collect();
        let mut out = Vec::new();
        let mut later = Vec::new();
        // Spots dropped because the leader cannot walk there.
        let mut unreached = 0;
        match trigger {
            // `spots` places a direct start itself.
            Trigger::Direct(_) | Trigger::Cast(_) | Trigger::Message(..) => return Err("a direct start or a cast anywhere is placed where the leader stands".to_string()),
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
            Trigger::Search(code) | Trigger::PsynergyCell(_, code) => {
                // The leader searches, or casts on, the ground layer's cell 16 pixels ahead
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
                // The rows of the event being played, where the cell's code
                // has rows of other events too (a hole walked into from three
                // sides, each its own event).
                let all = self.event_rows(TRIGGER_STEP, code);
                let mine: Vec<EventRow> = all.iter().filter(|r| self.own.contains(&result_key(r.result))).cloned().collect();
                let rows = if mine.is_empty() { all } else { mine };
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
                let (cells, templates) = self.in_rooms(cells);
                if cells.is_empty() {
                    return Err(format!(
                        "the {templates} cells with code {code} lie outside every room an entrance shows: a copy the scene keeps beside the map, drawn in where something opens the way (MapGrid_CopyCellAttributes), and nothing here has opened it"
                    ));
                }
                // Pushes go toward what lies in the next cell first: an
                // actor (a statue, a crate, a log whose footprint covers
                // it), then a wall.
                let mut pushed: Vec<(i32, usize, Spot)> = Vec::new();
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
                            // A row that faces one way is walked into what the
                            // code's cells surround (Osenia Cavern's pit, with a
                            // cell of the code north, west and east of it): the
                            // push toward the cell most of them border first.
                            let ahead = (cx + STEP[d].0, cz + STEP[d].1);
                            let around = if facing.is_some() { STEP.iter().filter(|s| (ahead.0 + s.0, ahead.1 + s.1) != (cx, cz) && self.cell(layer, ahead.0 + s.0, ahead.1 + s.1) == i32::from(code)).count() } else { 0 };
                            pushed.push((rank, around, Spot { x: tx, z: tz, layer, height: target, facing: FACING[d], walk: KEYS[d], cell: Some((cx, cz)) }));
                        }
                    }
                    for (rank, &d) in ways.iter().enumerate() {
                        let rank = if facing.is_some() { rank } else { 0 };
                        let (x, z) = (tx - STEP[d].0 * size, tz - STEP[d].1 * size);
                        // The top or foot of a vine is ground he stands on
                        // and walks off sideways: Left and Right start no
                        // climb.
                        let here = self.code_at(layer, x, z);
                        let off_a_climb = here == CELL_CLIMB && STEP[d].1 == 0;
                        if !self.inside(x, z, 0) || !(self.quiet(here) || off_a_climb) || !reach.holds(layer, x, z) {
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
                pushed.sort_by_key(|(rank, around, s)| (s.layer != reach.layer, std::cmp::Reverse(*around), *rank));
                // The way the row faces first, then spots in the open.
                walked.sort_by_key(|(rank, s)| (s.layer != reach.layer, *rank, std::cmp::Reverse(reach.open(s.layer, s.x, s.z))));
                out.extend(pushed.into_iter().map(|(_, _, spot)| spot));
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
        // Where the party was before the leader was put at the spot: where
        // the entrance placed it, or where the event before left him. A walk
        // up to the spot comes the way from there.
        let origin;
        let early = match start {
            Start::Entered(saved) => {
                self.load(saved);
                let gs = self.s.game_state;
                origin = Some((self.gba.read32(gs + self.s.l.gs_x) as i32 >> 20, self.gba.read32(gs + self.s.l.gs_z) as i32 >> 20));
                self.gba.write32(gs + self.s.l.gs_x, fx(spot.x) as u32);
                self.gba.write32(gs + self.s.l.gs_y, spot.height as u32);
                self.gba.write32(gs + self.s.l.gs_z, fx(spot.z) as u32);
                self.gba.write32(gs + self.s.l.gs_heading, spot.facing);
                self.gba.write16(gs + self.s.l.gs_turn, u16::from(spot.layer));
                self.until_control()?;
                self.put_leader(spot);
                let early = self.stage();
                self.frames(SETTLE)?;
                early
            }
            Start::InControl(saved) => {
                self.load(saved);
                origin = self.leader().map(|a| (a.x >> 4, a.z >> 4));
                self.put_leader(spot);
                let early = self.stage();
                self.frames(SETTLE_MOVED)?;
                early
            }
        };
        self.check_leader(spot)?;
        // A recording never shows the leader where the party cannot be (in the
        // sky, in a wall, on water), and he is never left out of what he does:
        // a spot he cannot stand on is refused, and the next spot or entrance
        // is tried. Only a stage that places the camera by him hides him.
        // Beside an actor or in a tight spot is the game's own place for him
        // when he walked there (peering into a well); only a start that placed
        // him can be wrong.
        let placed_him = self.unwalked || matches!(trigger, Trigger::Direct(_) | Trigger::Cast(_) | Trigger::Message(..));
        if self.hide_leader {
            let selected = self.gba.read32(self.s.game_state + self.s.l.gs_selected_actor) as u16;
            self.hide_actor(selected);
        } else if record.is_some() {
            let mut wrong = Self::wrong_for_a_placed_start(self.misplaced()?);
            wrong.retain(|why| placed_him || !(why.starts_with("he stands on actor") || why.starts_with("he can walk to") || why.starts_with("he has no sprite")));
            if !wrong.is_empty() {
                return Err(format!("the leader is misplaced at ({}, {}): {}", spot.x, spot.z, wrong.join(", ")));
            }
            if placed_him {
                if let Some(why) = self.unseen()? {
                    return Err(format!("the leader is misplaced at ({}, {}): {why}", spot.x, spot.z));
                }
            }
        }
        let off_ground = self.leader().map_or(0, |l| l.y - self.gba.read32(self.leader_object() + self.s.l.obj_terrain_height) as i32);
        // What the leader does by walking he is recorded walking up to and
        // doing (Field::walk_up): from the start of a way up to the spot
        // some cells back, which fills the lead-in, so the trigger fires
        // from the walk. Where no way up is found or the walk fails, he
        // stands at the spot as before, and the recording's note says so.
        let walks = self.walk_up && record.is_some() && matches!(trigger, Trigger::Touch(_));
        let mut way_up = None;
        let mut walk_note = None;
        if walks && spot.walk != 0 && !self.hide_leader && !self.world() {
            match self.plan_approach(spot, origin)? {
                Ok(path) => way_up = Some(path),
                Err(why) => walk_note = Some(format!("{why}, so he starts beside it")),
            }
        }
        // Whether the leader moved in each frame before the event began, and
        // the keys held meanwhile.
        let mut moving: Vec<bool> = Vec::new();
        let mut pad = Pad::default();
        let beside = self.save();
        if let Some(full) = way_up.take() {
            // No random battle breaks into the walk.
            self.set_flag(self.s.l.flag_no_encounters, true);
            // From the start of the way, else from nearer along it where
            // the map does not show the ground there (a dark corridor
            // behind the room the camera keeps to).
            let mut tries = vec![full.clone()];
            let mut shorter = full.len() / 2;
            while shorter >= approach::APPROACH_LEAST {
                tries.push(full[full.len() - shorter..].to_vec());
                shorter /= 2;
            }
            let mut wrongs = Vec::new();
            for path in tries {
                let first = path[0].from;
                let (x, z) = (first.0 * 16 + 8, first.1 * 16 + 8);
                let height = self.ground(spot.layer, x, z)?;
                let d = KEYS.iter().position(|&k| k == path[0].keys).unwrap_or(2);
                let start = Spot { x, z, layer: spot.layer, height, facing: FACING[d], walk: 0, cell: None };
                self.put_leader(&start);
                self.frames(SETTLE_MOVED)?;
                // The camera has come to rest on him before the walk is recorded.
                let mut view = self.view();
                for _ in 0..SETTLE_VIEW_FRAMES {
                    self.frames(STILL_FRAMES)?;
                    if std::mem::replace(&mut view, self.view()) == view {
                        break;
                    }
                }
                // The scroll draws one row or column of cells a frame as the
                // view crosses it, so a camera that came a long way fast has
                // left rows undrawn (the lower branch missing under him in Tret
                // Tree): every layer is drawn again around him, as the game does
                // when it places the party.
                self.redraw_view()?;
                let mut wrong = match self.check_leader(&start) {
                    Ok(()) => self.misplaced()?,
                    Err(why) => vec![why],
                };
                // The map must show the ground he starts on.
                if wrong.is_empty() {
                    wrong.extend(self.unseen()?.filter(|why| why.starts_with("the map draws nothing")));
                }
                if wrong.is_empty() {
                    way_up = Some(path);
                    break;
                }
                wrongs.push(format!("from ({}, {}): {}", first.0, first.1, wrong.join(", ")));
                self.load(&beside);
            }
            if way_up.is_none() {
                walk_note = Some(format!("the walk up to it could not start ({}), so he starts beside it", wrongs.join("; ")));
            }
        }
        if record.is_some() {
            self.recorded = Some(Vec::new());
            self.keep_states(self.whole);
        }
        match way_up {
            Some(path) => {
                // The walk is the lead-in; where the way up is shorter than
                // the lead-in asks (a black screen to clear), he stands at
                // its start for the rest.
                let walk = path.iter().map(|m| if m.how == How::Walk { approach::FRAMES_PER_CELL } else { CLIMB_FRAMES / 3 }).sum::<u32>();
                let mut walked = Ok(());
                let padding = self.lead_in.saturating_sub(walk);
                for _ in 0..padding {
                    self.step_watching(&mut moving, &mut pad)?;
                    walked = walked.and_then(|()| self.in_sight());
                }
                // Standing there first is the room shown before he sets
                // off, not the walk: whether he walks up is judged from his
                // first step.
                moving.clear();
                walk_note = approach::describe(&path, padding);
                if let Err(why) = walked.and_then(|()| self.follow(&path, spot.walk, &mut moving, &mut pad)) {
                    walk_note = Some(format!("the walk up to it stopped ({why}), so he starts beside it"));
                    self.load(&beside);
                    self.recorded = Some(Vec::new());
                    self.keep_states(false);
                    self.keep_states(self.whole);
                    moving.clear();
                    pad = Pad::default();
                    for _ in 0..self.lead_in {
                        self.step_watching(&mut moving, &mut pad)?;
                    }
                }
            }
            None if record.is_some() => {
                for _ in 0..self.lead_in {
                    self.step_watching(&mut moving, &mut pad)?;
                }
            }
            None => self.frames(self.lead_in)?,
        }
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
            Trigger::PsynergyCell(_, code) => (TRIGGER_PSYNERGY, code),
            Trigger::Cast(_) => (TRIGGER_PSYNERGY, 0),
            Trigger::Direct(_) | Trigger::Message(..) => (TRIGGER_DIRECT, 0),
        };
        let direct = kind == TRIGGER_DIRECT;
        let anywhere = matches!(trigger, Trigger::Cast(_));
        // The dispatch it starts: Field_UsePsynergy takes the Psynergy, not
        // the actor.
        let want = (kind, (kind != TRIGGER_PSYNERGY).then_some(u32::from(target)));
        // A direct start reads no table.
        let table = if direct { Vec::new() } else { self.event_rows(kind, target) };
        let rows = table.len();
        if !direct && !anywhere && !own.is_empty() && !self.has_row(kind, target, own) {
            return Err(format!("the event table here has no row of its own for {} {target}", kind_name(kind)));
        }
        // What the event needs holds as it starts: the scene's own setup may
        // have changed a flag the run set (Mercury Lighthouse marks its floor
        // switches pressed as it is entered).
        self.apply_flags();
        // No random battle breaks into the event: a battle shown is one the
        // event starts (self.s.l.flag_no_encounters).
        self.set_flag(self.s.l.flag_no_encounters, true);
        if !direct && !anywhere {
            self.unshadow(kind, target, own, messages)?;
        }
        // The flags set and the actors moved for the event are written from
        // outside the game.
        self.cut();
        self.own = own.iter().copied().filter(|&r| r >= 0x10000).collect();
        self.watch = if direct { self.own.clone() } else { table.iter().map(|r| result_key(r.result)).filter(|&r| r >= 0x10000).collect() };
        self.t.ran = 0;
        self.t.own_ran = false;
        // A direct start is judged against the same moment left alone (see
        // `shown` below).
        let control = (direct || anywhere).then(|| self.save());
        match trigger {
            Trigger::Talk(_) | Trigger::Search(_) => self.press(2)?,
            Trigger::Psynergy(ability, _) | Trigger::PsynergyCell(ability, _) | Trigger::Cast(ability) => {
                let work = self.gba.read32(self.s.field_work);
                self.gba.write16(work + self.s.l.field_command, ability);
                self.cut();
            }
            Trigger::Touch(_) => self.hold(spot.walk),
            // Raised for the field sequence to start (see run_instruction):
            // the function, called as a row's dispatch calls it with the
            // value it would pass, here the raised one; or, for a message
            // row, the four calls a row's dispatch makes for it, the actor -1
            // when it has none.
            Trigger::Direct(_) | Trigger::Message(..) => {
                self.t.raise_len = match trigger {
                    Trigger::Direct(address) => {
                        self.t.raise[0] = (address, [RAISED_DIRECT, 0]);
                        1
                    }
                    Trigger::Message(message, actor) => {
                        let [start, set, show, end] = self.s.show_row;
                        let actor = if actor == u16::MAX { u32::MAX } else { u32::from(actor) };
                        self.t.raise = [(start, [0, 0]), (set, [u32::from(message), 0]), (show, [actor, 0]), (end, [0, 0])];
                        4
                    }
                    _ => 0,
                };
                self.t.raise_next = 0;
                let work = self.gba.read32(self.s.field_work);
                self.gba.write16(work + self.s.l.field_raised, RAISED_DIRECT as u16);
                self.cut();
            }
        }
        let hold = if record.is_some() { PAGE_HOLD } else { 3 };
        // What is on stage as the trigger comes, to tell whether the event
        // moved anything: the actors' places and the map's cells.
        let watched: Vec<(u16, i32, i32)> = self.stage().into_iter().filter(|a| early.contains(a)).collect();
        let cells_before = self.cell_codes();
        let palette_before = self.palette();
        let flags_before = self.flag_bytes();
        let mut moved = false;
        let mut fought = false;
        // Where in the recording a battle the event starts began, and whether
        // the party lost it.
        let mut battle_start = 0;
        // Where in the recording the party left that battle, once it has.
        let mut battle_end: Option<usize> = None;
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
        mobile.push(self.gba.read32(self.s.game_state + self.s.l.gs_selected_actor) as u16);
        let place = |f: &Self| -> Vec<(i32, i32, i32)> { mobile.iter().map(|&id| f.actor(id).map_or((0, 0, 0), |a| (a.x, a.z, a.y >> 16))).collect() };
        let mut places = place(self);
        let mut last_motion = 0;
        let mut astray_since: Option<u32> = None;
        let mut leader_last: Option<(i32, i32)> = None;
        let mut teleported = false;
        // Where the leader stood on the map and on the screen the frame
        // before, until the event begins: an event that moves him and the
        // camera together (Tret Tree's vines) changes the one and not the
        // other, and shows nothing.
        let screen_place = |f: &Self| f.leader_place().map(|(x, z, h)| ((x, z), (x - f.view().0, z - h - f.view().1)));
        let mut last_place = screen_place(self);
        let mut unseen_move: Option<(i32, i32)> = None;
        // The recorded frames before the event began.
        let mut lead = 0;
        // The frame the climb a touch left him on was first held on.
        let mut climb_held: Option<u32> = None;
        self.t.block_menu = true;
        self.t.menu_frame = 0;
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
                let place = screen_place(self);
                moving.push(place.map(|p| p.0) != last_place.map(|p| p.0));
                pad.push(self.t.keys);
                let before = std::mem::replace(&mut last_place, place);
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
                    lead = self.recorded.as_ref().map_or(0, Vec::len);
                    returned = (self.t.dispatch_return == 0).then_some(self.t.dispatch_end);
                    self.hold(if steer.is_some() { spot.walk } else { 0 });
                    leader_from = self.actor(self.gba.read32(self.s.game_state + self.s.l.gs_selected_actor) as u16);
                    quiet_start = self.t.frame;
                    // What he does by walking shows him walking to it.
                    if walks {
                        if std::env::var_os("ALCHEMY_RENDER_DEBUG").is_some() {
                            eprintln!("    walk up from ({}, {}): {} keys held (MOMENT_HOLD): {pad}", spot.x, spot.z, walk_note.as_deref().unwrap_or("walked"));
                        }
                        if let Some(why) = approach::stands_still(&moving) {
                            self.hold(0);
                            return Err(why);
                        }
                    }
                    // Moved on the map and not on the screen, by a dispatch
                    // that ran no script: the event is over, and nothing
                    // showed.
                    if let (Some((map0, screen0)), Some((map1, screen1))) = (before, place) {
                        let (dx, dz) = (map1.0 - map0.0, map1.1 - map0.1);
                        let on_screen = (screen1.0 - screen0.0).abs().max((screen1.1 - screen0.1).abs());
                        if returned == Some(frame) && dx.abs().max(dz.abs()) >= TELEPORT && on_screen <= 4 {
                            unseen_move = Some((dx, dz));
                            break;
                        }
                    }
                } else if self.t.frame - pushed_from > START_FRAMES {
                    self.hold(0);
                    let how = match trigger {
                        Trigger::Talk(_) | Trigger::Search(_) => "pressing A",
                        Trigger::Touch(_) => "walking onto the cell",
                        Trigger::Psynergy(..) | Trigger::PsynergyCell(..) | Trigger::Cast(_) => "the cast",
                        Trigger::Direct(_) | Trigger::Message(..) => "the direct start",
                    };
                    return Err(format!("{how} from ({}, {}) started nothing in {} frames", spot.x, spot.z, START_FRAMES));
                }
                continue;
            };
            if returned.is_none() && self.t.dispatch_return == 0 {
                returned = Some(self.t.dispatch_end);
            }
            // A press of A opened the field menu: the player had control again.
            if self.t.menu_frame > b {
                break;
            }
            if !moved {
                let shifted = |a: Option<Actor>, x: i32, z: i32, by: i32| a.is_some_and(|a| (a.x - x).abs().max((a.z - z).abs()) >= by);
                moved = watched.iter().any(|&(id, x, z)| shifted(self.actor(id), x, z, 4));
                if let Some(from) = leader_from {
                    let now = self.actor(self.gba.read32(self.s.game_state + self.s.l.gs_selected_actor) as u16);
                    moved |= shifted(now, from.x, from.z, 20) || now.is_some_and(|a| (a.y - from.y).abs() >= 6 << 16);
                }
            }
            if let Some(now) = self.leader() {
                teleported |= leader_last.is_some_and(|(x, z)| (now.x - x).abs().max((now.z - z).abs()) >= TELEPORT);
                leader_last = Some((now.x, now.z));
            }
            // The script has carried the leader where the party cannot be:
            // what plays is not the game (a log ride with no log under him).
            // A start he walked to is the game's own, and all it does after is too.
            let shows_him = placed_him && !teleported && record.is_some() && !self.hide_leader && arrival.is_none() && self.scene() == self.setup.scene && !self.recorded.as_ref().and_then(|f| f.last()).is_some_and(|p| is_black_bgr555(p));
            // A direct start staged nothing: if the script walks the leader, the
            // event is about him, and he is walking from the wrong place.
            if shows_him && self.has_trigger && matches!(trigger, Trigger::Direct(_)) {
                if let (Some(from), Some(now)) = (leader_from, self.leader()) {
                    if (now.x - from.x).abs().max((now.z - from.z).abs()) >= DIRECT_WALK {
                        self.hold(0);
                        return Err(format!("the script walks the leader from ({}, {}) to ({}, {}), and a direct start staged nothing for that", from.x, from.z, now.x, now.z));
                    }
                }
            }
            if shows_him && self.t.frame % 4 == 0 {
                match self.astray(off_ground) {
                    Some((why, limit)) => {
                        let since = astray_since.get_or_insert(self.t.frame);
                        if self.t.frame - *since >= limit {
                            self.hold(0);
                            return Err(format!("the script keeps the leader where the party cannot be: {why}"));
                        }
                    }
                    None => astray_since = None,
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
            if fought && battle_end.is_none() && self.scene() != MAP_BATTLE {
                battle_end = Some(self.recorded.as_ref().map_or(0, Vec::len));
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
                        pages.push(Page { message });
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
                    pages.push(Page { message });
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
                // A touch that leaves him on a ladder (Tolbi's mast, by
                // Field_ClimbDown) goes on as the player goes on: the way
                // he came held, Up or Down, until he is off it at its other
                // end or the scene below has been entered.
                if walks && (spot.walk == KEY_UP || spot.walk == KEY_DOWN) {
                    let since = *climb_held.get_or_insert(self.t.frame);
                    if self.climbing() && self.t.frame - since < CLIMB_FRAMES {
                        self.hold(spot.walk);
                        continue;
                    }
                    self.hold(0);
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
        // A push goes on the same way, as a player pushes a log on until it
        // stops: each push the game's own, until what was pushed lies still
        // or the event's flags are set (the log rolled into the river).
        let pushing = self.event_rows(TRIGGER_STEP, target).iter().any(|r| r.flags & STEP_TRIGGER_FLAG_WALKING != 0);
        let pushed = watched.iter().any(|&(id, x, z)| self.actor(id).is_some_and(|a| (a.x - x).abs().max((a.z - z).abs()) >= 8));
        let mut goal = None;
        let mut push_note = None;
        if walks && pushing && pushed && spot.walk != 0 && unseen_move.is_none() && !left && !fought && !capped && !defeated && self.scene() == self.setup.scene {
            let ids: Vec<u16> = watched.iter().map(|a| a.0).collect();
            match self.push_on(spot.walk, &ids, &flags_before, &mut pad)? {
                Some((pushes, reached)) => {
                    goal = Some(reached);
                    if pushes > 0 {
                        push_note = Some(format!(
                            "he pushes it on the same way {pushes} more time{}{}",
                            if pushes == 1 { "" } else { "s" },
                            if goal == Some(true) { ", to where it does what its event is for" } else { ", until it stops" }
                        ));
                    }
                }
                None => goal = Some(false),
            }
        }
        // An event the screen does not show: said so, and followed by the
        // climb the move lets him make, which is what it is for.
        let unseen = match unseen_move {
            Some((dx, dz)) => {
                let way = match (dx.abs() >= dz.abs(), dx > 0, dz > 0) {
                    (true, true, _) => "east",
                    (true, false, _) => "west",
                    (false, _, true) => "south",
                    (false, _, false) => "north",
                };
                let mut why = format!("nothing visible happens: it moves the leader and the camera {} pixels {way} at once, onto ground drawn where he stood, so the picture stays the same", dx.abs().max(dz.abs()));
                if record.is_some() && self.walk_up {
                    let (mut onward, kept) = (Vec::new(), self.recorded.as_ref().map_or(0, Vec::len));
                    let set_down = self.save();
                    match self.climb_onward(&mut onward, &mut pad) {
                        Ok(Some(keys)) => why.push_str(&format!("; the recording goes on to the climb it leads to, {} held", if keys == KEY_UP { "Up" } else { "Down" })),
                        Ok(None) => self.walk_on(spot.walk, &mut pad)?,
                        Err(e) => {
                            // A climb that went wrong is not shown.
                            self.load(&set_down);
                            if let Some(frames) = self.recorded.as_mut() {
                                frames.truncate(kept);
                            }
                            why.push_str(&format!("; the climb it leads to could not be shown ({e})"));
                        }
                    }
                }
                Some(why)
            }
            None => None,
        };
        // What an event changed in the map is shown: where he stands over
        // it, he steps back from it; where it lies off the screen and the
        // event showed nothing else, the row says nothing visible happens.
        let mut hidden_note = None;
        let here = !left && !fought && !capped && !defeated && self.scene() == self.setup.scene && !self.world();
        if record.is_some() && self.walk_up && unseen.is_none() && here && !self.hide_leader && !moved && pages.is_empty() {
            let changed = self.changed_cells(&cells_before);
            if !changed.is_empty() {
                if self.step_back(&changed, &mut pad)? {
                    hidden_note = Some("he steps back afterwards, so that what it changed in the map where he stood shows".to_string());
                }
                let layer = self.leader().map_or(0, |a| a.layer.min(2));
                let mut seen = false;
                for &(cx, cz) in &changed {
                    seen |= self.cell_on_screen(layer, cx, cz)?;
                }
                if !seen && pages.is_empty() && !moved && self.palette() == palette_before {
                    let (cx, cz) = changed[0];
                    hidden_note = Some(format!("nothing visible happens: the {} cells of the map it changes, from cell ({cx}, {cz}), lie off the screen", changed.len()));
                }
            }
        }
        self.t.block_menu = false;
        // Where the event leaves him is where the player takes over: the party
        // must be able to be there.
        if placed_him && !teleported && record.is_some() && !self.hide_leader && !left && !fought && !capped && self.scene() == self.setup.scene {
            if let Some((why, limit)) = self.astray(off_ground) {
                if limit != ALOFT_FRAMES {
                    return Err(format!("the event leaves the leader where the party cannot be: {why}"));
                }
            }
        }
        // A recording through a black screen goes on, the player in control
        // and still, until it has shown BLACK_CLEARANCE frames of the game
        // after the last one (animation::check_black_clearance); the state
        // is put back after, so what plays next starts as it would have.
        let unfinished = capped && fought && self.scene() == MAP_BATTLE;
        if record.is_some() && !capped && !defeated {
            let since = self.recorded.as_ref().and_then(|f| f.iter().rposition(|p| is_black_bgr555(p)).map(|i| f.len() - 1 - i));
            if let Some(since) = since.filter(|&s| s < CLEARED) {
                let resume = self.save();
                self.frames((CLEARED - since) as u32)?;
                self.load(&resume);
            }
        }
        let mut frames = self.recorded.take().unwrap_or_default();
        let states = self.keep_states(false);
        // A battle the recording cannot hold to its end (a boss fought by A
        // alone) is shown to its opening, as is one the party lost: past its
        // last black screen by BLACK_CLEARANCE where the run has the frames.
        if defeated || unfinished {
            let opening = battle_start + BATTLE_OPENING;
            let cleared = frames.iter().take(opening).rposition(|p| is_black_bgr555(p)).map_or(0, |i| i + 1 + CLEARED);
            frames.truncate(opening.max(cleared));
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
        let mut shown = !pages.is_empty() || left || fought || moved || (!left && (self.cell_codes() != cells_before || self.palette() != palette_before));
        // A direct start that spoke, moved no one, fought and left nowhere
        // shows itself only by what it drew: its last picture against the
        // same moment played as long with nothing started, where water and
        // cycling colours run alike.
        if let Some(control) = control.filter(|_| shown && pages.is_empty() && !left && !fought && !moved) {
            let span = self.t.frame.saturating_sub(trigger_frame);
            let drawn = self.picture();
            let after = self.save();
            self.load(&control);
            self.frames(span)?;
            let idle = self.picture();
            self.load(&after);
            let changed = drawn.iter().zip(&idle).filter(|(a, b)| a != b).count();
            shown = changed * 100 >= drawn.len();
        }
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
        if self.resettled {
            let placed = "the arrival leaves the leader where the party cannot be, so he starts on the nearest open ground".to_string();
            note = Some(note.map_or(placed.clone(), |n| format!("{placed}; {n}")));
        }
        if matches!(trigger, Trigger::Direct(_)) {
            let started = "started directly: nothing staged the moment its code needs, so what it does may be off camera or missing".to_string();
            note = Some(note.map_or(started.clone(), |n| format!("{started}; {n}")));
        }
        if self.unwalked {
            let placed = "the leader starts where the walk from the entrance does not reach, since what opens the way there is not played first".to_string();
            note = Some(note.map_or(placed.clone(), |n| format!("{placed}; {n}")));
        }
        if let Some(walk) = walk_note {
            note = Some(note.map_or(walk.clone(), |n| format!("{walk}; {n}")));
        }
        if let Some(why) = &unseen {
            note = Some(note.map_or(why.clone(), |n| format!("{why}; {n}")));
        }
        if let Some(why) = push_note {
            note = Some(note.map_or(why.clone(), |n| format!("{n}; {why}")));
        }
        if let Some(why) = hidden_note {
            note = Some(note.map_or(why.clone(), |n| format!("{n}; {why}")));
        }
        if std::env::var_os("ALCHEMY_RENDER_DEBUG").is_some() && walks {
            eprintln!("    keys held (MOMENT_HOLD): {pad}");
        }
        let frames_len = frames.len();
        Ok(EventRun { pages, frames, states, whole: None, capped, end, instant, shown, seen: foreign, rows, ran, own_ran, leads_to, note, battle: fought.then(|| (battle_start, battle_end.unwrap_or(frames_len))), absent_speaker: (self.t.absent_speaker != 0).then_some(self.t.absent_speaker), lead, unseen, goal })
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
/// one (dx, dz) pixels away, as Trig_Atan gives it.
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
        TRIGGER_DIRECT => "direct start",
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
    /// The recorded frames a battle the event started ran over: its first,
    /// and the first after it, or the recording's end.
    pub battle: Option<(usize, usize)>,
    /// A speaker one of the event's messages was opened beside that the
    /// scene has no actor for: played here, its cast is not on stage (Mercury
    /// Lighthouse Aerie's hostages after the battle), and its windows open
    /// beside nobody.
    pub absent_speaker: Option<u16>,
    /// The recorded frames before the event began: the lead-in, the walk up
    /// to it included.
    pub lead: usize,
    /// Why nothing the event does shows on the screen, when it ran and moved
    /// the leader without the picture changing (Tret Tree's vines); the
    /// recording shows him walking across and the climb that follows.
    pub unseen: Option<String>,
    /// For a push the leader went on with (Field::push_on): whether it set
    /// a flag, what a pushed log or pillar is for (the log rolled into the
    /// river). None for anything else.
    pub goal: Option<bool>,
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

/// Whether facing `a` lies within an eighth of a turn of `b` (16-bit
/// angles): the same quarter turn.
fn facing_within(a: u32, b: u32) -> bool {
    (a as u16).wrapping_sub(b as u16).wrapping_add(0x1fff) <= 0x3ffe
}

/// `cells` split into those in a room one of `cameras` (the entrances'
/// camera boxes) shows, those in `room` first, and how many lie in none.
/// Where any entrance gives a whole box, an entrance that gives none (a
/// scene's default, which no exit leads to) shows no room of its own.
pub fn in_rooms(cells: Vec<(u8, i32, i32)>, cameras: &[[i16; 4]], header: [i32; 4], room: [i32; 4], size: i32) -> (Vec<(u8, i32, i32)>, usize) {
    let boxed: Vec<[i16; 4]> = cameras.iter().copied().filter(|c| c.iter().all(|&s| s != -1)).collect();
    let boxes = if boxed.is_empty() { cameras.to_vec() } else { boxed };
    let centre = |&(_, cx, cz): &(u8, i32, i32)| (cx * size + size / 2, cz * size + size / 2);
    let (mut kept, left): (Vec<_>, Vec<_>) = cells.into_iter().partition(|c| {
        let (x, z) = centre(c);
        boxes.is_empty() || boxes.iter().any(|&b| box_holds(b, header, x, z))
    });
    kept.sort_by_key(|c| {
        let (x, z) = centre(c);
        !(x >= room[0] && x < room[2] && z >= room[1] && z < room[3])
    });
    (kept, left.len())
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
    fn a_copy_of_cells_beside_the_map_is_no_trigger() {
        // Osenia Cavern: the west hole at (8, 12) in the room an entrance
        // shows, the spare one Scoop copies from at (35, 12) beside it, and
        // the default entrance with no box of its own.
        let header = [0, 0, 640 << 16, 480 << 16];
        let cameras = [[-1, -1, -1, -1], [32, 48, 416, 464], [80, 608, 368, 768]];
        let (kept, beside) = in_rooms(vec![(0, 35, 12), (0, 8, 12), (0, 9, 12)], &cameras, header, [32, 48, 416, 464], 16);
        assert_eq!(kept, vec![(0, 8, 12), (0, 9, 12)]);
        assert_eq!(beside, 1);
        // Only the spare: no trigger is left.
        assert_eq!(in_rooms(vec![(0, 35, 12)], &cameras, header, [32, 48, 416, 464], 16), (vec![], 1));
        // A scene whose entrances give no box: the whole map is the room.
        let (kept, beside) = in_rooms(vec![(0, 35, 12)], &[[-1, -1, -1, -1]], header, [0, 0, 640, 480], 16);
        assert_eq!((kept.len(), beside), (1, 0));
        // The room entered comes first.
        let (kept, _) = in_rooms(vec![(0, 10, 45), (0, 8, 12)], &cameras, header, [32, 48, 416, 464], 16);
        assert_eq!(kept[0], (0, 8, 12));
    }

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
