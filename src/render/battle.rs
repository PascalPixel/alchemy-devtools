//! One cast, recorded from a built edition: the run boots the ROM, sends
//! control to Camelot's battle test menu (DebugBattle_RunMenu) instead of the
//! title, and steers the battle it starts through RAM and registers at a few
//! function entries named by the linker map: the party, characters copied
//! onto the enemy side as a link battle does, the action the command menu
//! returns, and no enemy action. It keeps every frame from the action's
//! dispatch to the next round's command phase or the battle's end.
//!
//! The battle background's picture is cleared, so BG1 and BG2 show the
//! backdrop, and each line is drawn over a backdrop colour of the caller's;
//! the GBA blends effects with it as it would with its own. The windows are
//! drawn in greys of their colours' lightness. Both are changes to the
//! picture alone: the game's palette RAM keeps its own colours.

use super::mgba::{Gba, HEIGHT, KEY_A, KEY_DOWN, LR};
use super::plan::{self, Aim, Ally, Cast};
use crate::ld_map::Map;

/// formatn.tsv row 1: one Vermin, a fixed formation (the first enemy in the
/// middle).
const ENCOUNTER: u32 = 1;
/// The command, enum BattleCommand's CMD_PSYNERGY.
const CMD_PSYNERGY: u16 = 1;
/// HP of each foe, so every cast leaves it standing.
const FOE_HP: u16 = 999;
/// Luck of a foe facing instant death: half of it comes off the effect's
/// chance in BattleFx_RollSuccess, which leaves none.
const FOE_LUCK: u8 = 99;
/// PP a caster is given when it has too few for the cast.
const CASTER_PP: u16 = 99;
/// Frames a prompt waits before the player presses A.
const ADVANCE: u32 = 45;
/// A run that is not over by this frame has gone wrong.
const MAX_FRAMES: u32 = 6000;

/// struct GameState (game_state.h), struct BattleUnit (battle/battle_unit.h),
/// struct BattleWork (battle_work.h) and struct BattleObj
/// (battle/battle_session.h) offsets.
const STATE_ACTIVE_OWNERS: u32 = 0x1f8;
const STATE_BATTLE_START: u32 = 0x22b;
const STATE_OWNERS: u32 = 0x2c0;
const UNIT_SIZE: u32 = 0x14c;
const UNIT_BASE_HP: u32 = 0x10;
const UNIT_BASE_PP: u32 = 0x12;
const UNIT_HP_GAUGE: u32 = 0x14;
const UNIT_BASE_LUCK: u32 = 0x1e;
const UNIT_MAX_HP: u32 = 0x34;
const UNIT_MAX_PP: u32 = 0x36;
const UNIT_HP: u32 = 0x38;
const UNIT_PP: u32 = 0x3a;
const UNIT_AGILITY: u32 = 0x40;
const UNIT_LUCK: u32 = 0x42;
const UNIT_CLASS_ID: u32 = 0x128;
const UNIT_CLASS_INDEX: u32 = 0x129;
const UNIT_SIDE: u32 = 0x12a;
const UNIT_POISON: u32 = 0x131;
const UNIT_DELUSION: u32 = 0x138;
/// A status's turns as it lands.
const STATUS_TURNS: u8 = 7;
/// A gauge's full share, BattleUnit.hpGauge.
const GAUGE_FULL: u32 = 0x4000;
const SIDE_ABSENT: u8 = 0;
const SIDE_ENEMY: u8 = 1;
const FIRST_ENEMY_UNIT: u32 = 128;
/// Enemy units 128 to 133 (BATTLE_ENEMY_CAPACITY).
const ENEMY_UNITS: u32 = 6;
const WORK_PARTY_UNITS: u32 = 0x58;
const WORK_ENEMY_IDS: u32 = 0x66;
const WORK_BACKGROUND: u32 = 0x648;
/// gBattleWork->events.phase; PLAY_ADVANCE (5) shows the playback arrow.
const WORK_EVENT_PHASE: u32 = 0x800;
const PLAY_ADVANCE: u32 = 5;
const OBJ_RESOURCE: u32 = 0x04;
const OBJ_OVERLAY: u32 = 0x06;
const OBJ_PALETTE: u32 = 0x14;
const PLACE_PARTY: u16 = 0x100;
const PLACE_ENEMY: u16 = 0x180;
/// The battle background's picture: character blocks 2 and 3, below BG1's
/// map in screen block 31.
const PICTURE: std::ops::Range<u32> = 0x0600_8000..0x0600_f800;
/// The window colours, as BG palette entries: palette 15's colours 4 to 10,
/// which PaletteGlow_Update sets from the player's window hue and brightness;
/// palette 14's, palette 15 brightened for a chosen window
/// (Palette_UpdatePulseBrightness); palette 5's colour 14, which the battle
/// copies from colour 4 for the status windows' HP and PP (background.c), and
/// palette 4's, which holds palette 5 brightened. No hue on the game's wheel
/// is grey.
const WINDOW_ENTRIES: [(u16, std::ops::RangeInclusive<u16>); 4] =
    [(15, 4..=10), (14, 4..=10), (5, 14..=14), (4, 14..=14)];

/// The message strip's first line; the game draws the strip black.
pub const STRIP: usize = 136;

/// A BGR555 grey of five-bit level `level`.
pub const fn grey(level: u16) -> u16 {
    level | level << 5 | level << 10
}

/// The backdrop of each line: a gradient from grey `top` to grey `bottom`
/// (five-bit levels) over the battlefield, black under the strip.
pub fn gradient(top: u16, bottom: u16) -> [u16; HEIGHT] {
    let mut lines = [0; HEIGHT];
    let last = STRIP as u16 - 1;
    for (y, line) in lines.iter_mut().enumerate().take(STRIP) {
        let fall = (2 * y as u16 * (top - bottom) + last) / (2 * last);
        *line = grey(top - fall);
    }
    lines
}

/// Each BGR555 colour's grey of the same lightness (Rec. 601 luma).
pub fn greys() -> Vec<u16> {
    (0..0x8000u32)
        .map(|c| {
            let (r, g, b) = (c & 31, (c >> 5) & 31, (c >> 10) & 31);
            grey(((299 * r + 587 * g + 114 * b + 500) / 1000) as u16)
        })
        .collect()
}

/// The addresses the run stops at and writes, from the edition's map.
pub struct Symbols {
    start: u32,
    menu: u32,
    preset: u32,
    encounter: u32,
    collect: u32,
    obj_make: u32,
    background: u32,
    build: u32,
    commands: u32,
    opponents: u32,
    dispatch: u32,
    wait_advance: u32,
    battle_over: u32,
    battle_work: u32,
    game_state: u32,
    game_flags: u32,
    owner_states: u32,
}

impl Symbols {
    pub fn find(map: &Map) -> Result<Symbols, String> {
        let at = |name: &str| {
            map.symbol(name).map(|(_, a)| a & !1).ok_or_else(|| format!("the map has no {name}"))
        };
        Ok(Symbols {
            start: at("Game_ResetForNewGameFar")?,
            menu: at("DebugBattle_RunMenu")?,
            preset: at("DebugParty_LoadPreset")?,
            encounter: at("Battle_RunEncounter")?,
            collect: at("BattleParty_CollectUnitList")?,
            obj_make: at("BattleObjMake")?,
            background: at("BattleBackground_Load")?,
            build: at("BattlePresentation_BuildActions")?,
            commands: at("Battle_CollectPartyCommands")?,
            opponents: at("BattlePres_BuildOpponentEntries")?,
            dispatch: at("BattlePresentation_DispatchAction")?,
            wait_advance: at("BattlePresentation_WaitForAdvance")?,
            battle_over: at("Battle_ClearSerialTransferPayload")?,
            battle_work: at("gBattleWork")?,
            game_state: at("gGameState")?,
            game_flags: at("gGameFlagBytes")?,
            owner_states: at("gBattleOwnerStates")?,
        })
    }
}

/// What the run kept.
pub struct Recording {
    /// Every frame of the cast at 60 frames a second, BGR555.
    pub frames: Vec<Vec<u16>>,
    /// Whether the action ended the battle.
    pub battle_over: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Phase {
    Boot,
    /// The menu's chooser, waiting for A.
    Chooser,
    /// Battle_RunEncounter has started.
    Battle,
    /// The units are set; A through the opening message.
    Intro,
    /// The command menu has returned the action.
    Commanded,
    /// Recording the action.
    Playing,
    /// The action is over; recording the round's end.
    Played,
}

/// Plays `cast` on `rom` over the backdrop `lines` and returns its frames;
/// `log` prints each step of the setup.
pub fn record(rom: &[u8], symbols: &Symbols, cast: &Cast, lines: &[u16; HEIGHT], log: bool) -> Result<Recording, String> {
    let mut gba = Gba::new(rom)?;
    let s = symbols;
    let owner = |id: u8| s.game_state + STATE_OWNERS + u32::from(id) * UNIT_SIZE;
    let caster = cast.caster();
    let say = |frame: u32, text: String| {
        if log {
            eprintln!("  [{frame}] {text}");
        }
    };
    let windows: Vec<u16> = WINDOW_ENTRIES.iter().flat_map(|(p, colours)| colours.clone().map(move |c| p * 16 + c)).collect();

    let mut phase = Phase::Boot;
    // From power-on: the menu opens its chooser while Down is held.
    let mut hold_down = true;
    let mut frame = 0;
    let mut background_return = 0;
    let mut background_id = 0;
    let mut obj_return = 0;
    let mut obj_actor = 0;
    let mut obj_unit = 0;
    let mut dispatch_return = 0;
    let mut wait_return = 0;
    let mut tail: Option<u32> = None;
    let mut prompt_frames = 0;
    let mut battle_over = false;
    let mut frames = Vec::new();

    loop {
        let step = gba.step();
        let pc = step.pc;
        let frame_done = step.frame != frame;
        frame = step.frame;

        if phase == Phase::Boot && pc == s.start {
            // System_Initialize is about to start the title: run the battle
            // test menu instead. It reads Down from gKeysHeld at once.
            gba.branch(s.menu);
            phase = Phase::Chooser;
        } else if phase == Phase::Chooser && pc == s.preset {
            // The chooser loads preset 0 first: load ours instead.
            say(frame, format!("DebugParty_LoadPreset({}) -> {}", gba.register(0), cast.preset));
            gba.set_register(0, cast.preset.into());
            hold_down = false;
        } else if phase == Phase::Chooser && pc == s.encounter {
            gba.set_register(0, ENCOUNTER);
            // The party: party flags 0 to 7 and the order.
            let flags = cast.party.iter().fold(0u8, |f, &o| f | 1 << o);
            gba.write8(s.game_flags, flags);
            for (i, &o) in cast.party.iter().enumerate() {
                gba.write8(s.game_state + STATE_ACTIVE_OWNERS + i as u32, o);
            }
            // A set battle: no first strike or surprise roll.
            gba.write8(s.game_state + STATE_BATTLE_START, 1);
            let unit = owner(caster);
            if gba.read16(unit + UNIT_PP) < u16::from(cast.ability.pp_cost) {
                for field in [UNIT_BASE_PP, UNIT_MAX_PP, UNIT_PP] {
                    gba.write16(unit + field, CASTER_PP);
                }
            }
            if let Some(&ally) = cast.party.get(1) {
                let unit = owner(ally);
                let max = u32::from(gba.read16(unit + UNIT_MAX_HP));
                // UnitCalc keeps the share of HP the gauge holds.
                let set_hp = |gba: &mut Gba, hp: u32| {
                    gba.write16(unit + UNIT_HP, hp as u16);
                    gba.write16(unit + UNIT_HP_GAUGE, (hp * GAUGE_FULL / max.max(1)) as u16);
                };
                match cast.ally {
                    Ally::Healthy => {}
                    Ally::Hurt => set_hp(&mut gba, max / 2),
                    Ally::Downed => set_hp(&mut gba, 0),
                    Ally::Poisoned => gba.write8(unit + UNIT_POISON, 1),
                    Ally::Deluded => gba.write8(unit + UNIT_DELUSION, STATUS_TURNS),
                }
            }
            for &o in cast.party.iter().chain(&cast.foes) {
                let unit = owner(o);
                say(
                    frame,
                    format!(
                        "{}: classId {}, classIndex {}, HP {}/{}, PP {}/{}",
                        plan::character_name(o),
                        gba.read8(unit + UNIT_CLASS_ID),
                        gba.read8(unit + UNIT_CLASS_INDEX),
                        gba.read16(unit + UNIT_HP),
                        gba.read16(unit + UNIT_MAX_HP),
                        gba.read16(unit + UNIT_PP),
                        gba.read16(unit + UNIT_MAX_PP)
                    ),
                );
            }
            gba.set_backdrop(lines);
            gba.set_recolour(&windows, &greys());
            phase = Phase::Battle;
        } else if phase >= Phase::Battle && pc == s.background && background_return == 0 {
            background_return = gba.register(LR) & !1;
            background_id = gba.register(1);
        } else if background_return != 0 && pc == background_return {
            // Only the battle's own background is cleared: some effects load
            // a sky of their own.
            let work = gba.read32(s.battle_work);
            if background_id == u32::from(gba.read16(work + WORK_BACKGROUND)) {
                for a in PICTURE.step_by(4) {
                    gba.write32(a, 0);
                }
            }
            background_return = 0;
        } else if phase == Phase::Battle && pc == s.collect {
            // The enemy list is built: enemy units 128 on become copies of
            // the foes' owner records, as a link battle receives the
            // opponent's, and the other enemies leave.
            if !cast.foes.is_empty() {
                let enemies = gba.read32(s.owner_states);
                for unit in 0..ENEMY_UNITS {
                    let to = enemies + unit * UNIT_SIZE;
                    match cast.foes.get(unit as usize) {
                        Some(&foe) => {
                            for i in (0..UNIT_SIZE).step_by(4) {
                                let word = gba.read32(owner(foe) + i);
                                gba.write32(to + i, word);
                            }
                            // UnitCalc rebuilds maxHp from baseHp and luck
                            // from baseLuck each round
                            // (BattleUnit_ResetGuardLevels), so raise both.
                            for field in [UNIT_BASE_HP, UNIT_MAX_HP, UNIT_HP] {
                                gba.write16(to + field, FOE_HP);
                            }
                            if cast.lucky_foes {
                                gba.write8(to + UNIT_BASE_LUCK, FOE_LUCK);
                                gba.write8(to + UNIT_LUCK, FOE_LUCK);
                            }
                            gba.write8(to + UNIT_SIDE, SIDE_ENEMY);
                        }
                        None => gba.write8(to + UNIT_SIDE, SIDE_ABSENT),
                    }
                }
            }
            phase = Phase::Intro;
        } else if pc == s.obj_make && obj_return == 0 {
            obj_actor = gba.register(0);
            obj_unit = gba.register(1);
            obj_return = gba.register(LR) & !1;
        } else if obj_return != 0 && pc == obj_return {
            // BattleObjMake gives a character on the enemy side palette 1,
            // the link opponent's colours: draw it in its own.
            if obj_unit >= FIRST_ENEMY_UNIT && !cast.foes.is_empty() {
                gba.write32(obj_actor + OBJ_PALETTE, 0);
            }
            let unit = if obj_unit >= FIRST_ENEMY_UNIT {
                gba.read32(s.owner_states) + (obj_unit - FIRST_ENEMY_UNIT) * UNIT_SIZE
            } else {
                owner(obj_unit as u8)
            };
            say(
                frame,
                format!(
                    "BattleObjMake unit {obj_unit}: classId {}, classIndex {}, sprite {}, weapon sprite {}, palette {}",
                    gba.read8(unit + UNIT_CLASS_ID),
                    gba.read8(unit + UNIT_CLASS_INDEX),
                    gba.read16(obj_actor + OBJ_RESOURCE),
                    gba.read16(obj_actor + OBJ_OVERLAY),
                    gba.read32(obj_actor + OBJ_PALETTE)
                ),
            );
            obj_return = 0;
        } else if phase >= Phase::Intro && pc == s.build {
            if phase == Phase::Played {
                // The next round's commands: the battle is idle.
                tail = Some(0);
            }
        } else if phase == Phase::Intro && pc == s.commands {
            // The command menu (Battle_CollectPartyCommands, reached through
            // its far stub): fill in the first command record and return one
            // command.
            let record = gba.register(0);
            let work = gba.read32(s.battle_work);
            let place = match cast.aim {
                Aim::Foe => PLACE_ENEMY + place_of(&gba, work + WORK_ENEMY_IDS, FIRST_ENEMY_UNIT as u16)?,
                Aim::Ally => PLACE_PARTY + place_of(&gba, work + WORK_PARTY_UNITS, cast.party[1].into())?,
            };
            let agility = gba.read16(owner(caster) + UNIT_AGILITY);
            let fields = [caster.into(), 0, agility, CMD_PSYNERGY, cast.ability.id, place, cast.ability.range.into(), 0];
            for (i, value) in fields.into_iter().enumerate() {
                gba.write16(record + 2 * i as u32, value);
            }
            say(frame, format!("command: {} casts ability {} at place {place:#x}", plan::character_name(caster), cast.ability.id));
            gba.return_now(1);
            phase = Phase::Commanded;
        } else if phase >= Phase::Intro && pc == s.opponents {
            // The enemies' actions: none.
            gba.return_now(0);
        } else if phase == Phase::Commanded && pc == s.dispatch {
            say(frame, "BattlePresentation_DispatchAction: recording".to_string());
            dispatch_return = gba.register(LR) & !1;
            phase = Phase::Playing;
        } else if phase >= Phase::Intro && pc == s.wait_advance && wait_return == 0 {
            wait_return = gba.register(LR) & !1;
        } else if wait_return != 0 && pc == wait_return {
            wait_return = 0;
        } else if phase >= Phase::Playing && pc == s.battle_over && tail.is_none() {
            say(frame, "the battle is over".to_string());
            battle_over = true;
            tail = Some(0);
        } else if phase == Phase::Playing && pc == dispatch_return {
            say(frame, "the dispatch returned".to_string());
            phase = Phase::Played;
        }

        if !frame_done {
            continue;
        }
        if phase >= Phase::Playing {
            frames.push(gba.picture());
            if let Some(t) = tail.as_mut() {
                *t += 1;
                if *t > 1 {
                    break;
                }
            }
        }
        // The player: Down from power-on; A every other four frames in the
        // chooser and through the opening message; then A once a prompt has
        // waited: BattlePresentation_WaitForAdvance, or the playback task's
        // arrow.
        let work = if phase >= Phase::Battle { gba.read32(s.battle_work) } else { 0 };
        let waiting = wait_return != 0 || (work != 0 && gba.read32(work + WORK_EVENT_PHASE) == PLAY_ADVANCE);
        prompt_frames = if waiting { prompt_frames + 1 } else { 0 };
        let keys = if hold_down {
            KEY_DOWN
        } else if matches!(phase, Phase::Chooser | Phase::Intro) {
            if (frame / 4) % 2 == 1 { KEY_A } else { 0 }
        } else if waiting && (ADVANCE..ADVANCE + 2).contains(&prompt_frames) {
            KEY_A
        } else {
            0
        };
        gba.set_keys(keys);
        if frame > MAX_FRAMES {
            return Err(format!("gave up at frame {frame} in phase {phase:?}"));
        }
    }
    say(frame, format!("{} frames", frames.len()));
    Ok(Recording { frames, battle_over })
}

/// The place of `unit` in a unit list ended by BATTLE_UNIT_LIST_END.
fn place_of(gba: &Gba, list: u32, unit: u16) -> Result<u16, String> {
    (0..8)
        .map(|i| gba.read16(list + 2 * i))
        .take_while(|&id| id != 0xff && id != 0xffff)
        .position(|id| id == unit)
        .map(|p| p as u16)
        .ok_or_else(|| format!("unit {unit} is not in the battle"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gradient_runs_from_top_to_bottom_over_the_battlefield() {
        let lines = gradient(25, 9);
        assert_eq!((lines[0], lines[STRIP - 1], lines[STRIP]), (grey(25), grey(9), 0));
        assert!(lines.windows(2).take(STRIP - 1).all(|w| w[0] >= w[1]));
    }

    #[test]
    fn window_greys_keep_their_lightness() {
        let table = greys();
        assert_eq!(table[0x7fff], 0x7fff);
        assert_eq!(table[0], 0);
        // The default window colour, teal 0x4180, is a dark grey.
        assert_eq!(table[0x4180], grey(9));
    }
}
