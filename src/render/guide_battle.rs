//! Pictures for the battle guide's rules: an enemy's choice of action, a
//! first strike and a surprise, the element multiplier, the rolls a plan
//! makes in advance, an action's staging, the added effects and what follows
//! each target. Each plays a battle from Camelot's battle test menu, as
//! `render psynergy` does, and reads the battle's memory while it plays; a
//! roll is forced only by answering BattleRandom at the call that makes it.

use super::field::Image;
use super::guide::Guide;
use super::guide_more::{canvas, paste, screen};
use super::guide_take::text;
use super::mgba::{Gba, HEIGHT, LR, WIDTH};
use super::plan::{self, Tables};

/// KEYINPUT bits.
const KEY_A: u32 = 0x1;
const KEY_DOWN: u32 = 0x80;

/// struct GameState offsets (game_state.h), as `battle.rs` uses them.
const STATE_ACTIVE_OWNERS: u32 = 0x1f8;
const STATE_BATTLE_START: u32 = 0x22b;
const STATE_OWNERS: u32 = 0x2c0;
/// gGameState.battleStart: BATTLE_START_NORMAL rolls a first strike; 1, which
/// no battle start names, leaves the battle to open normally, as `render
/// psynergy` sets it.
const BATTLE_START_SET: u8 = 1;
/// A run that is not over by this frame has gone wrong.
const MAX_FRAMES: u32 = 20000;

pub(super) const PANEL_BACK: u32 = 0x20_22_28;
pub(super) const PANEL_TEXT: u32 = 0xe8_e8_e8;
pub(super) const PANEL_DIM: u32 = 0x80_84_8c;
pub(super) const PANEL_MARK: u32 = 0xff_d8_40;
pub(super) const PANEL_HEAD: u32 = 0x9c_c8_ff;

/// What a battle run is: the debug party preset and party, the formation
/// (a row of formatn.tsv), gGameState.battleStart and the function entries
/// the watcher is called at.
pub(super) struct Drive {
    pub preset: u8,
    pub party: Vec<u8>,
    pub encounter: u32,
    pub battle_start: u8,
    pub hooks: Vec<u32>,
}

/// What the watcher wants after a frame.
pub(super) enum Act {
    /// The keys the player holds: A pressed every other four frames.
    Mash,
    Keys(u32),
    Stop,
}

/// The addresses a run needs, from the edition's map.
struct Boot {
    start: u32,
    menu: u32,
    preset: u32,
    encounter: u32,
    game_state: u32,
    game_flags: u32,
}

impl<'a> Guide<'a> {
    /// Boots the ROM into Camelot's battle test menu, starts `drive`'s
    /// battle and plays it: `watch` is called with the address at each of
    /// `drive.hooks` (and may step, save and load the machine there), and
    /// with None after each frame from the battle's start, with the frame
    /// counted from there; what it returns decides the keys.
    pub(super) fn drive(&self, drive: &Drive, watch: &mut dyn FnMut(&mut Gba, Option<u32>, usize) -> Act) -> Result<(), String> {
        let at = |name: &str| self.sym(name);
        let s = Boot {
            start: at("Game_RunFar")?,
            menu: at("DebugBattle_RunMenu")?,
            preset: at("DebugParty_LoadPreset")?,
            encounter: at("Battle_RunEncounter")?,
            game_state: at("gGameState")?,
            game_flags: at("gGameFlagBytes")?,
        };
        let mut gba = Gba::new(self.rom)?;
        // 0: booting, 1: the chooser, 2: the battle.
        let mut phase = 0;
        let mut frame = 0;
        let mut battle_frames = 0usize;
        // The menu opens its chooser while Down is held, until a preset loads.
        let mut preset_loaded = false;
        loop {
            let step = gba.step();
            let pc = step.pc;
            if phase == 2 && drive.hooks.contains(&pc) {
                watch(&mut gba, Some(pc), battle_frames);
            }
            let frame_done = step.frame != frame;
            frame = step.frame;
            if phase == 0 && pc == s.start {
                gba.branch(s.menu);
                phase = 1;
            } else if phase == 1 && pc == s.preset {
                gba.set_register(0, drive.preset.into());
                preset_loaded = true;
            } else if phase == 1 && pc == s.encounter {
                gba.set_register(0, drive.encounter);
                let flags = drive.party.iter().fold(0u8, |f, &o| f | 1 << o);
                gba.write8(s.game_flags, flags);
                for (i, &o) in drive.party.iter().enumerate() {
                    gba.write8(s.game_state + STATE_ACTIVE_OWNERS + i as u32, o);
                }
                gba.write8(s.game_state + STATE_BATTLE_START, drive.battle_start);
                phase = 2;
            }
            if !frame_done {
                continue;
            }
            if std::env::var_os("ALCHEMY_DRIVE_DEBUG").is_some() && frame % 300 == 0 {
                let _ = super::places::write_png(&self.out.join(format!("drive-{frame:05}.png")), &screen(&gba.picture()));
            }
            let keys = if phase == 2 {
                let act = watch(&mut gba, None, battle_frames);
                battle_frames += 1;
                match act {
                    Act::Mash => mash(frame),
                    Act::Keys(k) => k,
                    Act::Stop => break,
                }
            } else if phase == 0 || !preset_loaded {
                KEY_DOWN
            } else {
                mash(frame)
            };
            gba.set_keys(keys);
            if frame > MAX_FRAMES {
                return Err(format!("gave up at frame {frame}"));
            }
        }
        Ok(())
    }

    /// The party and preset `render psynergy` gives Quake, with `encounter`.
    pub(super) fn quake_drive(&self, encounter: u32, battle_start: u8, hooks: Vec<u32>) -> Result<Drive, String> {
        let root = crate::compile::root();
        let plan = Tables::read(root)?.plan()?;
        let cast = plan.casts.into_iter().find(|c| c.ability.name == "Quake").ok_or("no Quake cast")?;
        Ok(Drive { preset: cast.preset, party: vec![plan::ISAAC, plan::GARET, plan::IVAN, plan::MIA], encounter, battle_start, hooks })
    }
}

/// A pressed every other four frames.
pub(super) fn mash(frame: u32) -> u32 {
    if (frame / 4) % 2 == 1 { KEY_A } else { 0 }
}

/// Steps until the function whose entry the machine is at returns to `back`,
/// calling `at` with each address reached on the way (it may answer a call
/// there); gives up after `limit` instructions.
pub(super) fn run_to(gba: &mut Gba, back: u32, limit: u32, at: &mut dyn FnMut(&mut Gba, u32)) -> bool {
    for _ in 0..limit {
        let pc = gba.step().pc;
        if pc == back {
            return true;
        }
        at(gba, pc);
    }
    false
}

/// Action names by ability id, from actions.tsv.
pub(super) fn ability_names() -> Result<Vec<String>, String> {
    let path = crate::compile::root().join("games/tbs/src/game/actions.tsv");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(plan::abilities(&text)?.into_iter().map(|a| a.name).collect())
}

/// A row of a table under games/tbs/src: its columns by name.
pub(super) fn table_row(file: &str, row: usize) -> Result<std::collections::HashMap<String, String>, String> {
    let path = crate::compile::root().join("games/tbs/src").join(file);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut lines = text.lines();
    let head: Vec<String> = lines.next().unwrap_or("").split('\t').map(|c| c.split(':').next().unwrap_or(c).to_string()).collect();
    let line = lines.nth(row).ok_or_else(|| format!("{file} has no row {row}"))?;
    Ok(head.into_iter().zip(line.split('\t').map(str::to_string)).collect())
}

pub(super) fn fill(image: &mut Image, x: usize, y: usize, w: usize, h: usize, colour: u32) {
    for py in y..(y + h).min(image.height) {
        for px in x..(x + w).min(image.width) {
            image.pixels[py * image.width + px] = colour;
        }
    }
}

/// Eight colours for an enemy's eight places.
const PLACE_COLOURS: [u32; 8] = [0xe0_60_50, 0xe0_a0_40, 0xd0_d0_50, 0x70_c0_60, 0x50_b0_c0, 0x60_80_e0, 0xa0_70_e0, 0xd0_70_b0];

/// formatn.tsv row 294: two Gryphons, a random formation. enemies.tsv row
/// 90, the Gryphon: ACTION_STRATEGY_FALLING and eight different actions.
const GRYPHONS: u32 = 294;
const GRYPHON: usize = 90;
/// struct BattleActionRecord (battle_session.h).
const RECORD_UNIT: u32 = 0x00;
const RECORD_COMMAND: u32 = 0x06;
const RECORD_PARAMETER: u32 = 0x08;
/// enum BattleCommand's names.
fn command_name(c: u16) -> &'static str {
    match c {
        0 => "ATTACK",
        1 => "PSYNERGY",
        2 => "ITEM",
        3 => "DEFEND",
        4 => "ABILITY",
        _ => "?",
    }
}
const FIRST_ENEMY_UNIT: u32 = 128;

impl<'a> Guide<'a> {
    /// Two Gryphons against the four at level 35: at the first Gryphon's
    /// turn, BattleCommand_SelectAutomatic (retry 1) is run 256 times from
    /// the same saved moment, its first BattleRandom answered with each
    /// roll from 0 to 255 in turn, and the action it settles on read from
    /// the request it fills; beside the screen of the turn as it then plays,
    /// with the Gryphon's eight places, the weights of its strategy's row of
    /// gBattleCommand_SelectionWeights read from the ROM, and the rolls each
    /// place took, as `enemy-actions.png`.
    pub fn enemy_actions(&mut self) -> Result<(), String> {
        let names = ability_names()?;
        let row = table_row("game/enemies.tsv", GRYPHON)?;
        let strategy: u32 = row["action_strategy"].parse().map_err(|_| "no action_strategy")?;
        let actions: Vec<usize> = (0..8).map(|i| row[&format!("action{i}")].parse().unwrap_or(0)).collect();
        let select = self.sym("BattleCommand_SelectAutomatic")?;
        let random = self.sym("BattleRandom")?;
        let weights_at = self.sym("gBattleCommand_SelectionWeights")?;
        let drive = self.quake_drive(GRYPHONS, BATTLE_START_SET, vec![select])?;
        // Each roll's (command, ability) and the BattleRandom calls made.
        let mut picks: Vec<(u16, u16, u32)> = Vec::new();
        let mut weights: Vec<u8> = Vec::new();
        let mut turn: Option<(usize, u32)> = None;
        let mut played = (0u16, 0u16);
        let mut frames: Vec<Vec<u16>> = Vec::new();
        self.drive(&drive, &mut |gba, hook, f| {
            if hook.is_some() {
                // The first Gryphon's turn: SelectAutomatic with retry 1.
                let request = gba.register(0);
                if gba.register(1) != 1 || u32::from(gba.read16(request + RECORD_UNIT)) != FIRST_ENEMY_UNIT || turn.is_some() {
                    return Act::Mash;
                }
                weights = (0..8).map(|i| gba.read8(weights_at + 8 * strategy + i)).collect();
                let back = gba.register(LR) & !1;
                let saved = gba.save();
                for roll in 0..256u32 {
                    gba.load(&saved);
                    let mut calls = 0;
                    run_to(gba, back, 2_000_000, &mut |gba, pc| {
                        if pc == random {
                            if calls == 0 {
                                gba.return_now(roll);
                            }
                            calls += 1;
                        }
                    });
                    picks.push((gba.read16(request + RECORD_COMMAND), gba.read16(request + RECORD_PARAMETER), calls));
                }
                gba.load(&saved);
                turn = Some((f, request));
                return Act::Mash;
            }
            frames.push(gba.picture());
            match turn {
                Some((t, request)) if f == t + 1 => {
                    played = (gba.read16(request + RECORD_COMMAND), gba.read16(request + RECORD_PARAMETER));
                    Act::Mash
                }
                Some((t, _)) if f > t + 120 => Act::Stop,
                _ => Act::Mash,
            }
        })?;
        let (turn, _) = turn.ok_or("the Gryphon never chose an action")?;
        // The place each roll took: the place whose ability the request names.
        let place_of = |(_, ability, _): &(u16, u16, u32)| actions.iter().position(|&a| a == usize::from(*ability));
        let mut spans: Vec<(usize, usize, usize)> = Vec::new();
        for (roll, p) in picks.iter().enumerate() {
            let place = place_of(p).ok_or_else(|| format!("roll {roll} took ability {}, not one of the eight", p.1))?;
            match spans.last_mut() {
                Some(s) if s.2 == place => s.1 = roll,
                _ => spans.push((roll, roll, place)),
            }
        }
        eprintln!("enemy-actions: strategy {strategy}, weights {weights:?}, turn at frame {turn}, played {} {}", command_name(played.0), names[usize::from(played.1)]);
        for &(a, b, place) in &spans {
            eprintln!("  rolls {a}..={b}: place {place}, {}", names[actions[place]]);
        }
        if picks.iter().any(|p| p.2 != 1) {
            return Err("a roll was refused and drawn again".into());
        }
        // The frame the Gryphon's announcement is written out.
        let shot = &frames[(turn + 60).min(frames.len() - 1)];
        const SIDE: usize = 620;
        let (w, h) = (2 * WIDTH + SIDE, 2 * HEIGHT + 150);
        let mut image = canvas(w, h, PANEL_BACK);
        paste(&mut image, &screen(shot), 0, 0, 2);
        let x = (2 * WIDTH + 14) as i32;
        text(&mut image, x, 6, "GRYPHON (ENEMIES.TSV ROW 90)", PANEL_HEAD, None, 2);
        text(&mut image, x, 24, "ACTION_STRATEGY_FALLING: WEIGHTS IN 256", PANEL_DIM, None, 2);
        text(&mut image, x, 48, "PLACE ABILITY       WEIGHT", PANEL_DIM, None, 2);
        let used = names[usize::from(played.1)].clone();
        for (place, &ability) in actions.iter().enumerate() {
            let y = 68 + 26 * place as i32;
            let (a, b, _) = spans.iter().find(|s| s.2 == place).copied().unwrap_or((0, 0, place));
            let name = names[ability].to_uppercase();
            let colour = if names[ability] == used { PANEL_MARK } else { PANEL_TEXT };
            text(&mut image, x, y, &format!("{place}     {name}"), colour, None, 2);
            let bx = x as usize + 236;
            let weight = usize::from(weights[place]);
            fill(&mut image, bx, y as usize + 2, 2 * weight, 12, PLACE_COLOURS[place]);
            text(&mut image, (bx + 2 * weight + 6) as i32, y, &format!("{weight}  ROLLS {a}-{b}"), PANEL_DIM, None, 2);
        }
        // The 256 rolls, four pixels each, coloured by the place taken.
        let sy = 2 * HEIGHT + 20;
        text(&mut image, 8, sy as i32, "EACH ROLL, THE LOW BYTE OF BATTLERANDOM FROM 0 TO 255, BY THE PLACE THE GAME TOOK", PANEL_TEXT, None, 2);
        for (roll, p) in picks.iter().enumerate() {
            fill(&mut image, 8 + 4 * roll, sy + 22, 4, 40, PLACE_COLOURS[place_of(p).unwrap_or(0)]);
        }
        for &(a, _, place) in &spans {
            text(&mut image, (8 + 4 * a) as i32, (sy + 66) as i32, &format!("{a}"), PLACE_COLOURS[place], None, 2);
        }
        text(&mut image, 8, (sy + 96) as i32, &format!("THE GRYPHON'S TURN AS IT PLAYED: {}", used.to_uppercase()), PANEL_MARK, None, 2);
        self.guide_png("enemy-actions.png", &image, "a Gryphon's eight actions, their weights and the place each roll takes")
    }
}

/// struct BattleWork's encounterMode (battle_work.h) and enum
/// BattleEncounterMode's names.
const WORK_ENCOUNTER_MODE: u32 = 0x45;
fn encounter_name(mode: u8) -> &'static str {
    match mode {
        0 => "BATTLE_ENCOUNTER_NORMAL",
        1 => "BATTLE_ENCOUNTER_PARTY_FIRST",
        2 => "BATTLE_ENCOUNTER_ENEMIES_FIRST",
        _ => "?",
    }
}
/// formatn.tsv row 296: a fixed formation of four, the Lizard King, a
/// Harridan and two Stone Soldiers.
const FOUR_ENEMIES: u32 = 296;
/// struct BattleUnit's classId, an enemy's row of enemies.tsv plus ENEMY_CLASS_FIRST.
const ENEMY_CLASS_FIRST: usize = 8;
const UNIT_CLASS_ID: u32 = 0x128;
const UNIT_SIZE: u32 = 0x14c;
const BATTLE_START_NORMAL: u8 = 0;
/// The numbers of MsgPartyStrikesFirst and MsgPartySurprised (text/en.po).
const MSG_PARTY_STRIKES_FIRST: u32 = 2066;
const MSG_PARTY_SURPRISED: u32 = 2067;
/// Frames from the message's start to its text written out, and from an
/// action's dispatch to its announcement.
const MESSAGE_SHOWN: usize = 70;
const ANNOUNCED: usize = 40;
const MESSAGE_HELD: usize = 90;

/// One run of `first-strikes`: its frames, each frame's encounterMode, and
/// each action dispatched in the first round (frame, unit, command).
struct Opening {
    frames: Vec<Vec<u16>>,
    modes: Vec<u8>,
    acts: Vec<(usize, String, u16)>,
    rolls: Vec<u32>,
    /// The frame the first-strike or surprise message starts.
    message: Option<usize>,
}

impl<'a> Guide<'a> {
    /// The name of the function holding `address`, from the map.
    pub(super) fn function_at(&self, address: u32) -> String {
        self.map
            .at_address(address)
            .and_then(|p| p.symbols.iter().filter(|(a, _)| *a & !1 <= address).max_by_key(|(a, _)| *a).map(|(_, n)| n.clone()))
            .unwrap_or_default()
    }

    /// A unit's name: a party member's, or an enemy's class from
    /// enemies.tsv.
    pub(super) fn unit_name(&self, gba: &Gba, unit: u16) -> String {
        if u32::from(unit) < FIRST_ENEMY_UNIT {
            return plan::character_name(unit as u8).to_string();
        }
        let enemies = self.sym("gBattleOwnerStates").map(|s| gba.read32(s)).unwrap_or(0);
        let class = gba.read8(enemies + (u32::from(unit) - FIRST_ENEMY_UNIT) * UNIT_SIZE + UNIT_CLASS_ID);
        table_row("game/enemies.tsv", usize::from(class).wrapping_sub(ENEMY_CLASS_FIRST)).ok().and_then(|r| r.get("enemy").cloned()).unwrap_or(format!("UNIT {unit}"))
    }

    /// The four against FOUR_ENEMIES in a random encounter
    /// (BATTLE_START_NORMAL), Battle_RunEncounter's first-strike rolls
    /// answered with `answers` in turn, played to the second round's
    /// commands.
    fn opening(&self, answers: &[u32]) -> Result<Opening, String> {
        let random = self.sym("BattleRandom")?;
        let build = self.sym("BattlePresentation_BuildActions")?;
        let dispatch = self.sym("BattlePresentation_DispatchAction")?;
        let message = self.sym("UiText_ShowMessageAndWaitCore")?;
        let work_slot = self.sym("gBattleWork")?;
        let drive = self.quake_drive(FOUR_ENEMIES, BATTLE_START_NORMAL, vec![random, build, dispatch, message])?;
        let mut out = Opening { frames: Vec::new(), modes: Vec::new(), acts: Vec::new(), rolls: Vec::new(), message: None };
        let mut rounds = 0;
        let mut end: Option<usize> = None;
        self.drive(&drive, &mut |gba, hook, f| {
            match hook {
                Some(h) if h == random => {
                    let lr = gba.register(LR) & !1;
                    if self.function_at(lr) == "Battle_RunEncounter" {
                        let n = out.rolls.len();
                        if let Some(&a) = answers.get(n) {
                            gba.return_now(a);
                        }
                        out.rolls.push(answers.get(n).copied().unwrap_or(u32::MAX));
                    }
                    return Act::Mash;
                }
                Some(h) if h == message => {
                    if matches!(gba.register(0), MSG_PARTY_STRIKES_FIRST | MSG_PARTY_SURPRISED) {
                        out.message = Some(f);
                    }
                    return Act::Mash;
                }
                Some(h) if h == build => {
                    rounds += 1;
                    if rounds == 2 {
                        end = Some(f + 40);
                    }
                    return Act::Mash;
                }
                Some(_) => {
                    let record = gba.register(0);
                    let (unit, command) = (gba.read16(record + RECORD_UNIT), gba.read16(record + RECORD_COMMAND));
                    if rounds == 1 {
                        out.acts.push((f, self.unit_name(gba, unit), command));
                    }
                    return Act::Mash;
                }
                None => {}
            }
            out.frames.push(gba.picture());
            let work = gba.read32(work_slot);
            out.modes.push(if work == 0 { 0 } else { gba.read8(work + WORK_ENCOUNTER_MODE) });
            match (end, out.message) {
                (Some(e), _) if f >= e => Act::Stop,
                // The message held a moment before A dismisses it.
                (_, Some(m)) if f < m + MESSAGE_HELD => Act::Keys(0),
                _ => Act::Mash,
            }
        })?;
        Ok(out)
    }

    /// A random encounter with four enemies opened twice: once with the
    /// party striking first (Battle_RunEncounter's first BattleRandom
    /// answered 0, so `& 15` is 0) and once caught by surprise (answered 1,
    /// then 0 for the second roll's `& 31`); each played through its first
    /// round to the second round's commands, side by side, with
    /// BattleWork.encounterMode as each frame leaves it and the actions the
    /// round dispatched, as `first-strikes.gif`.
    pub fn first_strikes(&mut self) -> Result<(), String> {
        let first = self.opening(&[0])?;
        let surprised = self.opening(&[1, 0])?;
        for (label, run) in [("first", &first), ("surprised", &surprised)] {
            eprintln!("first-strikes {label}: {} frames, rolls {:?}, modes {:?}", run.frames.len(), run.rolls, run.modes.iter().copied().collect::<std::collections::BTreeSet<_>>());
            for a in &run.acts {
                eprintln!("  frame {}: {} {}", a.0, a.1, command_name(a.2));
            }
        }
        // Each run as a row of moments: its message, each action of the
        // first round as it is announced, and the next round's commands.
        let runs = [(&first, "THE PARTY STRIKES FIRST: BATTLERANDOM ANSWERED 0, AND 0 AND 15 IS 0"), (&surprised, "CAUGHT BY SURPRISE: ANSWERED 1 (1 AND 15 IS NOT 0), THEN 0 (0 AND 31 IS 0)")];
        let mut rows: Vec<(String, Vec<(usize, String)>)> = Vec::new();
        for (run, label) in runs {
            let mut shots = Vec::new();
            let m = run.message.ok_or("no first-strike message")?;
            shots.push((m + MESSAGE_SHOWN, encounter_name(run.modes[m]).to_string()));
            for (f, unit, _) in &run.acts {
                shots.push((f + ANNOUNCED, format!("{} ACTS", unit.to_uppercase())));
            }
            shots.push((run.frames.len() - 1, "ROUND 2: COMMANDS".to_string()));
            rows.push((label.to_string(), shots));
        }
        let columns = rows.iter().map(|r| r.1.len()).max().unwrap_or(1);
        const CELL: usize = WIDTH + 12;
        const ROW: usize = 26 + HEIGHT + 26;
        let (w, h) = (columns * CELL + 12, rows.len() * ROW + 8);
        let mut image = canvas(w, h, PANEL_BACK);
        for (r, ((label, shots), (run, _))) in rows.iter().zip(runs).enumerate() {
            let y = r * ROW + 6;
            text(&mut image, 8, y as i32, label, PANEL_HEAD, None, 2);
            for (c, (f, caption)) in shots.iter().enumerate() {
                let at = (*f).min(run.frames.len() - 1);
                let x = 8 + c * CELL;
                paste(&mut image, &screen(&run.frames[at]), x, y + 22, 1);
                let colour = if c == 0 { PANEL_MARK } else { PANEL_TEXT };
                text(&mut image, x as i32, (y + 26 + HEIGHT) as i32, caption, colour, None, 2);
            }
        }
        self.guide_png("first-strikes.png", &image, "a first strike beside a surprise")
    }
}

/// struct BattlePlan (battle_type.h) at BattleWork.plan.
const WORK_PLAN: u32 = 0x654;
const PLAN_TARGET_IDS: u32 = 0x02;

/// One call of Battle_GetElementMultiplier: the target being dealt with,
/// the difference passed, whether halved, and the multiplier returned.
#[derive(Debug, Clone)]
struct Multiplied {
    unit: u16,
    input: i32,
    halve: u32,
    result: i32,
    frame: usize,
}

/// The keys that make Isaac attack from the battle's first menu: Fight,
/// Attack, the aimed enemy.
const ATTACK_MENU: [(u32, u32); 7] = [(0, 20), (KEY_A, 4), (0, 30), (KEY_A, 4), (0, 30), (KEY_A, 4), (0, 320)];

impl<'a> Guide<'a> {
    /// Isaac's Quake (or, with `menu`, the action those keys choose) at
    /// copies of `foes`, as `render psynergy` casts it, with every call of
    /// Battle_GetElementMultiplier read at its entry (the difference passed,
    /// and whether it is halved) and at its return, and the target
    /// BattleTargetExec is dealing with; returns those and the frames.
    fn element_calls(&self, foes: &[u8], menu: Option<&[(u32, u32)]>) -> Result<(Vec<Multiplied>, Vec<Vec<u16>>), String> {
        let root = crate::compile::root();
        let plan = Tables::read(root)?.plan()?;
        let mut cast = plan.casts.into_iter().find(|c| c.ability.name == "Quake").ok_or("no Quake cast")?;
        cast.foes = foes.to_vec();
        let symbols = super::battle::Symbols::find(self.map)?;
        let multiplier = self.sym("Battle_GetElementMultiplier")?;
        let exec = self.sym("BattleTargetExec")?;
        let mut calls = Vec::new();
        let mut unit = 0u16;
        let mut frame = 0usize;
        let recording = super::battle::record_watching(self.rom, &symbols, &cast, &super::battle::Scene::Game, false, menu, &[multiplier, exec], &mut |gba, hook| match hook {
            Some(h) if h == exec => {
                let plan = gba.register(0);
                unit = u16::from(gba.read8(plan + PLAN_TARGET_IDS + gba.register(1)));
            }
            Some(_) => {
                let (input, halve) = (gba.register(0) as i32, gba.register(1));
                let back = gba.register(LR) & !1;
                let saved = gba.save();
                run_to(gba, back, 100_000, &mut |_, _| {});
                calls.push(Multiplied { unit, input, halve, result: gba.register(0) as i32, frame });
                gba.load(&saved);
            }
            None => frame += 1,
        })?;
        Ok((calls, recording.frames))
    }

    /// `gElement_PowerResistCurve` read from the ROM and drawn as the line
    /// Battle_GetElementMultiplier reads, and halved as a blow reads it;
    /// beside it Isaac's Quake at copies of Ivan, Isaac and Mia, and his
    /// Attack at a copy of Ivan, each target marked where the game's own call
    /// put it (the difference it was passed and the multiplier it returned),
    /// with the screen as Quake's last damage is announced, as
    /// `element-multiplier.png`.
    pub fn element_multiplier(&mut self) -> Result<(), String> {
        let foes = [plan::IVAN, plan::ISAAC, plan::MIA];
        let (quake, frames) = self.element_calls(&foes, None)?;
        let (attack, attack_frames) = self.element_calls(&[plan::IVAN], Some(&ATTACK_MENU))?;
        let name = |u: u16| plan::character_name(foes[usize::from(u).saturating_sub(FIRST_ENEMY_UNIT as usize).min(2)]).to_uppercase();
        for c in quake.iter().chain(&attack) {
            eprintln!("  frame {}: unit {} input {} halve {} -> {}", c.frame, c.unit, c.input, c.halve, c.result);
        }
        if quake.len() != 3 || attack.len() != 1 {
            return Err(format!("{} calls for Quake and {} for the attack", quake.len(), attack.len()));
        }
        // The curve as the ROM holds it: five (difference, value) pairs.
        let curve_at = self.sym("gElement_PowerResistCurve")? - 0x0800_0000;
        let half = |i: u32| i16::from_le_bytes([self.rom[(curve_at + 2 * i) as usize], self.rom[(curve_at + 2 * i + 1) as usize]]) as i32;
        let points: Vec<(i32, i32)> = (0..5).map(|p| (half(2 * p), half(2 * p + 1))).collect();
        eprintln!("element-multiplier: curve {points:?}");

        const CHART_W: usize = 640;
        let (w, h) = (2 * WIDTH + CHART_W + 16, 2 * HEIGHT + 2 * HEIGHT / 2 + 40);
        let mut image = canvas(w, h, PANEL_BACK);
        let last = quake.last().map_or(0, |c| c.frame);
        paste(&mut image, &screen(&frames[(last + 70).min(frames.len() - 1)]), 0, 0, 2);
        let attack_shot = &attack_frames[(attack[0].frame + 70).min(attack_frames.len() - 1)];
        let small = screen(attack_shot);
        paste(&mut image, &small, WIDTH / 2, 2 * HEIGHT + 30, 1);
        text(&mut image, 8, (2 * HEIGHT + 8) as i32, "ISAAC ATTACKS IVAN: A BLOW, HALVED", PANEL_DIM, None, 2);

        // The chart: differences -250 to 250 across, multipliers 0 to 2.5
        // (0 to 640 in 256ths) up.
        let (cx, cy, cw, ch) = (2 * WIDTH + 70, 40usize, CHART_W - 90, h - 110);
        let px = |d: i32| cx as i32 + (d + 250) * cw as i32 / 500;
        let py = |m: i32| (cy + ch) as i32 - m * ch as i32 / 640;
        let dot = |image: &mut Image, x: i32, y: i32, r: i32, colour: u32| {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx * dx + dy * dy <= r * r && x + dx >= 0 && y + dy >= 0 {
                        fill(image, (x + dx) as usize, (y + dy) as usize, 1, 1, colour);
                    }
                }
            }
        };
        for d in (-200..=200).step_by(50) {
            fill(&mut image, px(d) as usize, cy, 1, ch, if d == 0 { 0x50_55_60 } else { 0x30_33_3a });
            text(&mut image, px(d) - 12, (cy + ch + 6) as i32, &format!("{d}"), PANEL_DIM, None, 2);
        }
        for m in [0, 128, 256, 384, 512] {
            fill(&mut image, cx, py(m) as usize, cw, 1, if m == 256 { 0x50_55_60 } else { 0x30_33_3a });
            text(&mut image, cx as i32 - 62, py(m) - 6, &format!("{:.1}X", f64::from(m) / 256.0), PANEL_DIM, None, 2);
        }
        text(&mut image, cx as i32, 6, "GELEMENT_POWERRESISTCURVE: THE MULTIPLIER BY POWER LESS RESIST", PANEL_HEAD, None, 2);
        text(&mut image, cx as i32 + 120, (cy + ch + 26) as i32, "THE ACTOR'S POWER LESS THE TARGET'S RESIST", PANEL_DIM, None, 2);
        // The line through the ROM's points, held flat past the ends, whole
        // and halved; its value is 256 plus the curve's.
        for d in -250..=250 {
            let c = d.clamp(points[4].0, points[0].0);
            let i = points.iter().position(|p| c > p.0).unwrap_or(5);
            let v = if i == 0 { points[0].1 } else if i == 5 { points[4].1 } else { (c - points[i].0) * (points[i - 1].1 - points[i].1) / (points[i - 1].0 - points[i].0) + points[i].1 };
            dot(&mut image, px(d), py(256 + v), 1, 0x9c_c8_ff);
            dot(&mut image, px(d), py(256 + v / 2), 1, 0x60_b0_80);
        }
        for p in &points {
            dot(&mut image, px(p.0), py(256 + p.1), 4, 0x9c_c8_ff);
        }
        text(&mut image, px(110), py(256 + 220), "A CAST", 0x9c_c8_ff, None, 2);
        text(&mut image, px(-245), py(105), "A BLOW, HALVED", 0x60_b0_80, None, 2);
        // The calls the game made.
        for (k, c) in quake.iter().enumerate() {
            dot(&mut image, px(c.input), py(c.result), 6, PANEL_MARK);
            let label = format!("{} {:+} = {}/256", name(c.unit), c.input, c.result);
            // Ivan above to the left, Isaac to the left, Mia below to the right.
            let (dx, dy) = [(-200, -28), (-230, -22), (14, 12)][k.min(2)];
            text(&mut image, px(c.input) + dx, py(c.result) + dy, &label, PANEL_MARK, Some(PANEL_BACK), 2);
        }
        let a = &attack[0];
        dot(&mut image, px(a.input), py(a.result), 6, 0xff_90_30);
        text(&mut image, px(a.input) - 250, py(a.result) + 44, &format!("ATTACK UNLEASHING BLIZZARD, IVAN {:+} = {}/256", a.input, a.result), 0xff_90_30, Some(PANEL_BACK), 2);
        self.guide_png("element-multiplier.png", &image, "the element multiplier's line with Quake's and an attack's targets on it")
    }
}

/// struct BattlePlan's per-target fields, the first target's.
const PLAN_MULTIPLIER: u32 = 0x1e;
const PLAN_CRITICAL: u32 = 0x2c;
const PLAN_ABILITY: u32 = 0x4c;
const PLAN_FLAGS: u32 = 0x58;
/// struct BattleUnit's inventory (battle_unit.h) and an entry's parts
/// (enum InventoryEncoding).
const UNIT_INVENTORY: u32 = 0xd8;
const INVENTORY_SLOTS: u32 = 15;
const ITEM_ID_MASK: u16 = 0x1ff;
const INVENTORY_EQUIPPED: u16 = 0x200;
/// items.tsv: the Swift Sword, whose unleash is Sonic Smash, a triple
/// blow; ITEM_TYPE_WEAPON.
const SWIFT_SWORD: u16 = 21;
const ITEM_TYPE_WEAPON: &str = "1";
/// enum BattleCritical.
fn critical_name(c: i8) -> &'static str {
    match c {
        0 => "NONE",
        1 => "CRITICAL_HIT",
        2 => "CRITICAL_STRONG",
        _ => "?",
    }
}

/// One roll answered, or let be, during a plan: the function that asked,
/// the answer and what it was compared with.
struct Roll {
    by: String,
    answer: u32,
    forced: bool,
}

/// What `rolls` read from one attack.
struct Rolled {
    frames: Vec<Vec<u16>>,
    rolls: Vec<Roll>,
    /// The plan as BattleTargetExec first reads it: ability, multiplier,
    /// critical, presentation flags.
    plan: (u32, i8, i8, u32),
    /// Frames each message started.
    messages: Vec<usize>,
}

impl<'a> Guide<'a> {
    /// Isaac attacks a copy of Ivan through the command menu, the rolls
    /// BattleCommand_BuildPlan and RollWeaponUnleash ask BattleRandom for
    /// answered in turn from `plan_answers` and `unleash`, and, with
    /// `weapon`, that item in place of his equipped weapon as the battle
    /// opens.
    fn rolled_attack(&self, plan_answers: &[u32], unleash: u32, weapon: Option<u16>) -> Result<Rolled, String> {
        let root = crate::compile::root();
        let plan = Tables::read(root)?.plan()?;
        let mut cast = plan.casts.into_iter().find(|c| c.ability.name == "Quake").ok_or("no Quake cast")?;
        cast.foes = vec![plan::IVAN];
        let symbols = super::battle::Symbols::find(self.map)?;
        let random = self.sym("BattleRandom")?;
        let work_slot = self.sym("gBattleWork")?;
        let exec = self.sym("BattleTargetExec")?;
        let encounter = self.sym("Battle_RunEncounter")?;
        let message = self.sym("UiText_ShowMessageAndWaitCore")?;
        let owner = self.sym("gGameState")? + STATE_OWNERS;
        let types: Vec<String> = {
            let path = root.join("games/tbs/src/game/items.tsv");
            let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let mut lines = text.lines();
            let column = lines.next().unwrap_or("").split('\t').position(|c| c.starts_with("type:")).ok_or("items.tsv has no type")?;
            lines.map(|l| l.split('\t').nth(column).unwrap_or("").to_string()).collect()
        };
        let mut out = Rolled { frames: Vec::new(), rolls: Vec::new(), plan: (0, 0, 0, 0), messages: Vec::new() };
        let mut planned = false;
        let mut frame = 0usize;
        let mut asked = 0;
        let mut swapped = weapon.is_none();
        let recording = super::battle::record_watching(self.rom, &symbols, &cast, &super::battle::Scene::Game, false, Some(&ATTACK_MENU), &[random, exec, encounter, message], &mut |gba, hook| match hook {
            Some(h) if h == encounter && !swapped => {
                // Isaac's equipped weapon becomes `weapon`, as if he wore it.
                for slot in 0..INVENTORY_SLOTS {
                    let at = owner + UNIT_INVENTORY + 2 * slot;
                    let entry = gba.read16(at);
                    if entry & INVENTORY_EQUIPPED != 0 && types.get(usize::from(entry & ITEM_ID_MASK)).map(String::as_str) == Some(ITEM_TYPE_WEAPON) {
                        gba.write16(at, (entry & !ITEM_ID_MASK) | weapon.unwrap_or(0));
                        swapped = true;
                    }
                }
            }
            Some(h) if h == random && !planned => {
                let by = self.function_at(gba.register(LR) & !1);
                let answer = match by.as_str() {
                    "BattleCommand_BuildPlan" => {
                        asked += 1;
                        plan_answers.get(asked - 1).copied()
                    }
                    "RollWeaponUnleash" => Some(unleash),
                    _ => None,
                };
                if let Some(a) = answer {
                    gba.return_now(a);
                    out.rolls.push(Roll { by, answer: a, forced: true });
                } else if by == "BattleCommand_BuildPlan" || by == "RollWeaponUnleash" {
                    out.rolls.push(Roll { by, answer: 0, forced: false });
                }
            }
            Some(h) if h == exec => planned = true,
            Some(h) if h == message => out.messages.push(frame),
            Some(_) => {}
            None => {
                // The plan stays in BattleWork until the next action.
                let work = gba.read32(work_slot);
                if work != 0 {
                    let p = work + WORK_PLAN;
                    out.plan = (gba.read32(p + PLAN_ABILITY), gba.read8(p + PLAN_MULTIPLIER) as i8, gba.read8(p + PLAN_CRITICAL) as i8, gba.read32(p + PLAN_FLAGS));
                }
                frame += 1;
            }
        })?;
        if !swapped {
            return Err("Isaac wears no weapon".into());
        }
        out.frames = recording.frames;
        Ok(out)
    }

    /// Isaac attacks a copy of Ivan three times, the plan's rolls answered
    /// at BattleRandom: a miss (the miss roll answered 0, so `& 31` is 0), a
    /// critical hit (the miss roll 1, the critical roll 0), and, wearing the
    /// Swift Sword, its unleash Sonic Smash (the unleash roll 0) struck
    /// three times (the blow roll 0, under 153); each beside the plan's
    /// rolls and the fields BattleTargetExec then reads, as
    /// `plan-rolls.png`.
    pub fn plan_rolls(&mut self) -> Result<(), String> {
        let names = ability_names()?;
        let runs = [
            ("A MISS", self.rolled_attack(&[0, 1, 0xffff], 0xffff, None)?),
            ("A CRITICAL HIT", self.rolled_attack(&[1, 0], 0xffff, None)?),
            ("A TRIPLE BLOW", self.rolled_attack(&[0], 0, Some(SWIFT_SWORD))?),
        ];
        for (label, r) in &runs {
            eprintln!("plan-rolls {label}: {} frames, messages at {:?}", r.frames.len(), r.messages);
            for roll in &r.rolls {
                eprintln!("  {} {} {}", roll.by, if roll.forced { "answered" } else { "rolled" }, roll.answer);
            }
            eprintln!("  plan: ability {} multiplier {} critical {} flags {:#x}", names.get(r.plan.0 as usize).map_or("?", String::as_str), r.plan.1, critical_name(r.plan.2), r.plan.3);
        }
        // Each run a row: three moments of the attack beside its rolls.
        // The frames each row shows, from the battle's start: the swing and
        // its result, chosen by looking at the runs.
        const SHOTS: [[usize; 4]; 3] = [[115, 130, 145, 160], [130, 160, 205, 250], [145, 205, 235, 265]];
        const PANEL: usize = 430;
        const ROW: usize = HEIGHT + 34;
        let (w, h) = (4 * (WIDTH + 8) + PANEL + 16, runs.len() * ROW + 8);
        let mut image = canvas(w, h, PANEL_BACK);
        for (r, (label, run)) in runs.iter().enumerate() {
            let y = 8 + r * ROW;
            text(&mut image, 8, y as i32, label, PANEL_MARK, None, 2);
            for (k, &f) in SHOTS[r].iter().enumerate() {
                let at = f.min(run.frames.len() - 1);
                paste(&mut image, &screen(&run.frames[at]), 8 + k * (WIDTH + 8), y + 22, 1);
            }
            let x = (8 + 4 * (WIDTH + 8) + 8) as i32;
            let mut ty = y as i32 + 22;
            text(&mut image, x, ty, "BATTLERANDOM ASKED BY:", PANEL_HEAD, None, 2);
            for roll in &run.rolls {
                ty += 18;
                let by = if roll.by == "RollWeaponUnleash" { "ROLLWEAPONUNLEASH" } else { "BATTLECOMMAND_BUILDPLAN" };
                text(&mut image, x + 8, ty, &format!("{by} {}", roll.answer), PANEL_TEXT, None, 2);
            }
            ty += 26;
            text(&mut image, x, ty, "THE PLAN, FIRST TARGET:", PANEL_HEAD, None, 2);
            let (ability, multiplier, critical, flags) = run.plan;
            for line in [
                format!("ABILITYID {}", names.get(ability as usize).map_or("?", String::as_str).to_uppercase()),
                format!("TARGETMULTIPLIER {multiplier}"),
                format!("TARGETCRITICAL {}", critical_name(critical)),
                format!("VARIANT {}", (flags >> 12) & 3),
            ] {
                ty += 18;
                text(&mut image, x + 8, ty, &line, PANEL_TEXT, None, 2);
            }
        }
        self.guide_png("plan-rolls.png", &image, "a miss, a critical hit and a triple blow beside the plan's rolls")
    }
}
