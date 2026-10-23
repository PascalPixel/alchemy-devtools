//! Pictures for the battle guide's rules: an enemy's choice of action, a
//! first strike and a surprise, the element multiplier, the rolls a plan
//! makes in advance, an action's staging, the added effects and what follows
//! each target. Each plays a battle from Camelot's battle test menu, as
//! `render psynergy` does, and reads the battle's memory while it plays; a
//! roll is forced only by answering Battle_Random at the call that makes it.

use super::field::Image;
use super::guide::Guide;
use super::guide_more::{canvas, paste, screen};
use super::guide_take::text;
use super::mgba::{Gba, HEIGHT, LR, WIDTH};
use super::plan::{self, Tables};

/// KEYINPUT bits.
const KEY_A: u32 = 0x1;
const KEY_DOWN: u32 = 0x80;

/// struct SaveBlock offsets (save_block.h), as `battle.rs` uses them.
const STATE_ACTIVE_OWNERS: u32 = 0x1f8;
const STATE_BATTLE_START: u32 = 0x22b;
const STATE_OWNERS: u32 = 0x2c0;
/// gSaveBlock.battleType: BATTLE_TYPE_NORMAL rolls a first strike; 1, which
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
/// (a row of formations.tsv), gSaveBlock.battleType and the function entries
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
            encounter: at("Battle_Run")?,
            game_state: at("gSaveBlock")?,
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

/// Action names by ability id, from abilities.tsv.
pub(super) fn ability_names() -> Result<Vec<String>, String> {
    let path = crate::compile::root().join("games/tbs/src/game/abilities.tsv");
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

/// formations.tsv row 294: two Gryphons, a random formation. enemies.tsv row
/// 90, the Gryphon: ACTION_STRATEGY_FALLING and eight different actions.
const GRYPHONS: u32 = 294;
const GRYPHON: usize = 90;
/// struct BattleAction (battle_session.h).
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
    /// turn, BattleAI_ChooseAction (retry 1) is run 256 times from
    /// the same saved moment, its first Battle_Random answered with each
    /// roll from 0 to 255 in turn, and the action it settles on read from
    /// the request it fills; beside the screen of the turn as it then plays,
    /// with the Gryphon's eight places, the weights of its strategy's row of
    /// gBattleAI_ActionWeights read from the ROM, and the rolls each
    /// place took, as `enemy-actions.png`.
    pub fn enemy_actions(&mut self) -> Result<(), String> {
        let names = ability_names()?;
        let row = table_row("game/enemies.tsv", GRYPHON)?;
        let strategy: u32 = row["action_strategy"].parse().map_err(|_| "no action_strategy")?;
        let actions: Vec<usize> = (0..8).map(|i| row[&format!("action{i}")].parse().unwrap_or(0)).collect();
        let select = self.sym("BattleAI_ChooseAction")?;
        let random = self.sym("Battle_Random")?;
        let weights_at = self.sym("gBattleAI_ActionWeights")?;
        let drive = self.quake_drive(GRYPHONS, BATTLE_START_SET, vec![select])?;
        // Each roll's (command, ability) and the Battle_Random calls made.
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
        // The screen as the Gryphon's announcement is written out, beside its
        // eight places, each a colour, its chance and the rolls it took;
        // below, every roll in the place's colour.
        let shot = &frames[(turn + 60).min(frames.len() - 1)];
        const W: usize = 2 * WIDTH + 496;
        let mut image = canvas(W, 2 * HEIGHT + 150, PANEL_BACK);
        paste(&mut image, &screen(shot), 0, 0, 2);
        let x = (2 * WIDTH + 16) as i32;
        text(&mut image, x, 2, "THE GRYPHON'S EIGHT ACTIONS", PANEL_HEAD, None, 2);
        text(&mut image, x, 18, "ACTION_STRATEGY_FALLING: EACH ONE'S", PANEL_DIM, None, 2);
        text(&mut image, x, 34, "CHANCE IN 256 AND THE ROLLS IT TAKES", PANEL_DIM, None, 2);
        let used = names[usize::from(played.1)].clone();
        for (place, &ability) in actions.iter().enumerate() {
            let y = 56 + 33 * place as i32;
            let (a, b, _) = spans.iter().find(|s| s.2 == place).copied().unwrap_or((0, 0, place));
            let colour = if names[ability] == used { PANEL_MARK } else { PANEL_TEXT };
            fill(&mut image, x as usize, y as usize + 2, 12, 12, PLACE_COLOURS[place]);
            let note = if names[ability] == used { "  (PLAYED)" } else { "" };
            text(&mut image, x + 18, y, &format!("{}{note}", names[ability].to_uppercase()), colour, None, 2);
            let weight = usize::from(weights[place]);
            fill(&mut image, x as usize + 18, y as usize + 18, 2 * weight, 8, PLACE_COLOURS[place]);
            text(&mut image, x + 24 + 2 * weight as i32, y + 14, &format!("{weight} IN 256: ROLLS {a} TO {b}"), PANEL_DIM, None, 2);
        }
        // The 256 rolls, three pixels each.
        let sy = 2 * HEIGHT + 14;
        text(&mut image, 8, sy as i32, "THE ACTION TAKEN FOR EACH ROLL FROM 0 TO 255 (THE LOW BYTE OF BATTLE_RANDOM),", PANEL_TEXT, None, 2);
        text(&mut image, 8, sy as i32 + 16, "IN ITS ACTION'S COLOUR ABOVE; EACH NUMBER IS THE FIRST ROLL OF A SPAN", PANEL_TEXT, None, 2);
        for (roll, p) in picks.iter().enumerate() {
            fill(&mut image, 16 + 3 * roll, sy + 40, 3, 40, PLACE_COLOURS[place_of(p).unwrap_or(0)]);
        }
        for &(a, _, place) in &spans {
            text(&mut image, (16 + 3 * a) as i32, (sy + 84) as i32, &format!("{a}"), PLACE_COLOURS[place], None, 2);
        }
        text(&mut image, 16 + 3 * 256 + 6, (sy + 52) as i32, "ROLL 255", PANEL_DIM, None, 2);
        self.guide_png("enemy-actions.png", &image, "a Gryphon's eight actions, their weights and the place each roll takes")
    }
}

/// struct BattleWork's firstStrike (battle_work.h) and enum
/// BattleEncounterMode's names.
const WORK_ENCOUNTER_MODE: u32 = 0x45;
fn encounter_name(mode: u8) -> &'static str {
    match mode {
        0 => "FIRST_STRIKE_NONE",
        1 => "FIRST_STRIKE_PARTY",
        2 => "FIRST_STRIKE_ENEMIES",
        _ => "?",
    }
}
/// formations.tsv row 296: a fixed formation of four, the Lizard King, a
/// Harridan and two Stone Soldiers.
const FOUR_ENEMIES: u32 = 296;
/// struct Unit's id, an enemy's row of enemies.tsv plus ENEMY_ID_FIRST.
const ENEMY_ID_FIRST: usize = 8;
const UNIT_CLASS_ID: u32 = 0x128;
const UNIT_SIZE: u32 = 0x14c;
const BATTLE_TYPE_NORMAL: u8 = 0;
/// The numbers of MsgPartyStrikesFirst and MsgPartySurprised (text/en.po).
const MSG_PARTY_STRIKES_FIRST: u32 = 2066;
const MSG_PARTY_SURPRISED: u32 = 2067;
/// Frames from the message's start to its text written out, and from an
/// action's dispatch to its announcement.
const MESSAGE_SHOWN: usize = 70;
const ANNOUNCED: usize = 40;
const MESSAGE_HELD: usize = 90;

/// One run of `first-strikes`: its frames, each frame's firstStrike, and
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
        let enemies = self.sym("gEnemyUnits").map(|s| gba.read32(s)).unwrap_or(0);
        let class = gba.read8(enemies + (u32::from(unit) - FIRST_ENEMY_UNIT) * UNIT_SIZE + UNIT_CLASS_ID);
        table_row("game/enemies.tsv", usize::from(class).wrapping_sub(ENEMY_ID_FIRST)).ok().and_then(|r| r.get("enemy").cloned()).unwrap_or(format!("UNIT {unit}"))
    }

    /// The four against FOUR_ENEMIES in a random encounter
    /// (BATTLE_TYPE_NORMAL), Battle_Run's first-strike rolls
    /// answered with `answers` in turn, played to the second round's
    /// commands.
    fn opening(&self, answers: &[u32]) -> Result<Opening, String> {
        let random = self.sym("Battle_Random")?;
        let build = self.sym("Battle_GatherActions")?;
        let dispatch = self.sym("BattleStaging_DispatchAction")?;
        let message = self.sym("BattleMessage_ShowAndWait")?;
        let work_slot = self.sym("gBattleWork")?;
        let drive = self.quake_drive(FOUR_ENEMIES, BATTLE_TYPE_NORMAL, vec![random, build, dispatch, message])?;
        let mut out = Opening { frames: Vec::new(), modes: Vec::new(), acts: Vec::new(), rolls: Vec::new(), message: None };
        let mut rounds = 0;
        let mut end: Option<usize> = None;
        self.drive(&drive, &mut |gba, hook, f| {
            match hook {
                Some(h) if h == random => {
                    let lr = gba.register(LR) & !1;
                    if self.function_at(lr) == "Battle_Run" {
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
    /// party striking first (Battle_Run's first Battle_Random
    /// answered 0, so `& 15` is 0) and once caught by surprise (answered 1,
    /// then 0 for the second roll's `& 31`); each played through its first
    /// round to the second round's commands, side by side, with
    /// BattleWork.firstStrike as each frame leaves it and the actions the
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
        // Each run a column of moments at twice the size: its message, the
        // round's first and last actions as they are announced, and the
        // next round's commands.
        let numbers = ["NONE", "ONE", "TWO", "THREE", "FOUR", "FIVE", "SIX"];
        let count = |n: usize| numbers.get(n).copied().unwrap_or("MANY");
        let runs = [
            (&first, ["THE FIRST-STRIKE ROLL (1 IN 16) CAME UP:", "THE PARTY STRIKES FIRST"]),
            (&surprised, ["THAT ROLL FAILED, THE SURPRISE ROLL (1 IN 32)", "CAME UP: THE ENEMIES STRIKE FIRST"]),
        ];
        const COLUMN: usize = 2 * WIDTH + 16;
        const SHOT: usize = 2 * HEIGHT + 26;
        let (w, h) = (2 * COLUMN, 44 + 4 * SHOT + 40);
        let mut image = canvas(w, h, PANEL_BACK);
        for (c, (run, head)) in runs.iter().enumerate() {
            let x = 8 + c * COLUMN;
            text(&mut image, x as i32, 4, head[0], PANEL_HEAD, None, 2);
            text(&mut image, x as i32, 20, head[1], PANEL_HEAD, None, 2);
            let m = run.message.ok_or("no first-strike message")?;
            let acts = &run.acts;
            let (first_act, last_act) = (acts.first().ok_or("no action")?, acts.last().ok_or("no action")?);
            let who = |a: &(usize, String, u16)| a.1.to_uppercase();
            let shots = [
                (m + MESSAGE_SHOWN, format!("FIRSTSTRIKE {}", encounter_name(run.modes[m]))),
                (first_act.0 + ANNOUNCED, format!("{} ACTS: THE ROUND'S FIRST ACTION", who(first_act))),
                (last_act.0 + ANNOUNCED, format!("{} ACTS: THE {} AND LAST ACTION", who(last_act), ["", "FIRST", "SECOND", "THIRD", "FOURTH", "FIFTH"].get(acts.len()).copied().unwrap_or("LAST"))),
                (run.frames.len() - 1, format!("ROUND 2: FIRSTSTRIKE {}", encounter_name(*run.modes.last().unwrap_or(&0)))),
            ];
            for (r, (f, caption)) in shots.iter().enumerate() {
                let y = 44 + r * SHOT;
                paste(&mut image, &screen(&run.frames[(*f).min(run.frames.len() - 1)]), x, y, 2);
                text(&mut image, x as i32, (y + 2 * HEIGHT + 4) as i32, caption, if r == 0 { PANEL_MARK } else { PANEL_TEXT }, None, 2);
            }
            let party = acts.iter().filter(|a| ["Isaac", "Garet", "Ivan", "Mia", "Jenna"].contains(&a.1.as_str())).count();
            let summary = if party == acts.len() {
                format!("ROUND 1: {} PARTY MEMBERS ACT, NO ENEMY DOES", count(party))
            } else {
                format!("ROUND 1: {} OF THE FOUR ENEMIES ACT, NO COMMANDS", count(acts.len()))
            };
            text(&mut image, x as i32, (44 + 4 * SHOT + 14) as i32, &summary, PANEL_MARK, None, 2);
        }
        self.guide_png("first-strikes.png", &image, "a first strike beside a surprise")
    }
}

/// struct BattleTurn (battle_type.h) at BattleWork.turn.
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
    /// BattleAbility_ApplyToTarget is dealing with; returns those and the frames.
    fn element_calls(&self, foes: &[u8], menu: Option<&[(u32, u32)]>) -> Result<(Vec<Multiplied>, Vec<Vec<u16>>), String> {
        let root = crate::compile::root();
        let plan = Tables::read(root)?.plan()?;
        let mut cast = plan.casts.into_iter().find(|c| c.ability.name == "Quake").ok_or("no Quake cast")?;
        cast.foes = foes.to_vec();
        let symbols = super::battle::Symbols::find(self.map)?;
        let multiplier = self.sym("Battle_GetElementMultiplier")?;
        let exec = self.sym("BattleAbility_ApplyToTarget")?;
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

        // The chart on top, the two battles below it at twice their size.
        const W: usize = 4 * WIDTH + 16;
        const CHART_H: usize = 470;
        let h = CHART_H + 24 + 2 * HEIGHT + 30;
        let mut image = canvas(W, h, PANEL_BACK);
        const CAST_LINE: u32 = 0x9c_c8_ff;
        const BLOW_LINE: u32 = 0x60_b0_80;
        const BLOW_MARK: u32 = 0xff_90_30;
        text(&mut image, 8, 4, "GELEMENT_POWERRESISTCURVE: THE DAMAGE MULTIPLIER BY THE ACTOR'S POWER", PANEL_HEAD, None, 2);
        text(&mut image, 8, 20, "IN THE ELEMENT LESS THE TARGET'S RESIST IN IT", PANEL_HEAD, None, 2);
        // The key.
        fill(&mut image, 8, 42, 24, 4, CAST_LINE);
        text(&mut image, 38, 36, "A CAST'S LINE", CAST_LINE, None, 2);
        fill(&mut image, 200, 42, 24, 4, BLOW_LINE);
        text(&mut image, 230, 36, "A BLOW'S LINE, HALVED", BLOW_LINE, None, 2);
        fill(&mut image, 430, 38, 10, 10, PANEL_MARK);
        text(&mut image, 446, 36, "QUAKE'S TARGETS", PANEL_MARK, None, 2);
        fill(&mut image, 600, 38, 10, 10, BLOW_MARK);
        text(&mut image, 616, 36, "AN ATTACK'S TARGET", BLOW_MARK, None, 2);
        // Differences -250 to 250 across, multipliers 0 to 2.5 times up.
        let (cx, cy, cw, ch) = (80usize, 64usize, W - 110, CHART_H - 120);
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
            text(&mut image, px(d) - 12, (cy + ch + 6) as i32, &format!("{d:+}").replace("+0", "0"), PANEL_DIM, None, 2);
        }
        for m in [0, 128, 256, 384, 512] {
            fill(&mut image, cx, py(m) as usize, cw, 1, if m == 256 { 0x50_55_60 } else { 0x30_33_3a });
            text(&mut image, 8, py(m) - 6, &format!("{:.1} X", f64::from(m) / 256.0), PANEL_DIM, None, 2);
        }
        text(&mut image, cx as i32 + 120, (cy + ch + 26) as i32, "POWER LESS RESIST, IN POINTS", PANEL_DIM, None, 2);
        text(&mut image, 8, (cy + ch + 26) as i32, "TIMES", PANEL_DIM, None, 2);
        // The line through the ROM's points, held flat past the ends, whole
        // and halved; its value is 256 plus the curve's.
        for d in -250..=250 {
            let c = d.clamp(points[4].0, points[0].0);
            let i = points.iter().position(|p| c > p.0).unwrap_or(5);
            let v = if i == 0 { points[0].1 } else if i == 5 { points[4].1 } else { (c - points[i].0) * (points[i - 1].1 - points[i].1) / (points[i - 1].0 - points[i].0) + points[i].1 };
            dot(&mut image, px(d), py(256 + v), 1, CAST_LINE);
            dot(&mut image, px(d), py(256 + v / 2), 1, BLOW_LINE);
        }
        for p in &points {
            dot(&mut image, px(p.0), py(256 + p.1), 4, CAST_LINE);
        }
        // The calls the game made, each multiplier in 256ths shown as times.
        let times = |r: i32| format!("{:.2} X", f64::from(r) / 256.0);
        for (k, c) in quake.iter().enumerate() {
            dot(&mut image, px(c.input), py(c.result), 6, PANEL_MARK);
            let label = format!("{} {:+}: {}", name(c.unit), c.input, times(c.result));
            // Ivan above to the left, Isaac to the left, Mia below to the right.
            let (dx, dy) = [(-170, -28), (-190, -22), (14, 12)][k.min(2)];
            text(&mut image, px(c.input) + dx, py(c.result) + dy, &label, PANEL_MARK, Some(PANEL_BACK), 2);
        }
        let a = &attack[0];
        dot(&mut image, px(a.input), py(a.result), 6, BLOW_MARK);
        text(&mut image, px(a.input) + 14, py(a.result) + 30, &format!("IVAN, AN ATTACK {:+}: {}", a.input, times(a.result)), BLOW_MARK, Some(PANEL_BACK), 2);
        // The two battles.
        let y = CHART_H + 20;
        let last = quake.last().map_or(0, |c| c.frame);
        paste(&mut image, &screen(&frames[(last + 70).min(frames.len() - 1)]), 8, y, 2);
        text(&mut image, 8, (y + 2 * HEIGHT + 6) as i32, "ISAAC'S QUAKE AT IVAN, ISAAC AND MIA", PANEL_TEXT, None, 2);
        let attack_shot = &attack_frames[(attack[0].frame + 70).min(attack_frames.len() - 1)];
        paste(&mut image, &screen(attack_shot), 2 * WIDTH + 8, y, 2);
        text(&mut image, (2 * WIDTH + 8) as i32, (y + 2 * HEIGHT + 6) as i32, "ISAAC ATTACKS IVAN, UNLEASHING BLIZZARD", PANEL_TEXT, None, 2);
        self.guide_png("element-multiplier.png", &image, "the element multiplier's line with Quake's and an attack's targets on it")
    }
}

/// struct BattleTurn's per-target fields, the first target's.
const PLAN_MULTIPLIER: u32 = 0x1e;
const PLAN_CRITICAL: u32 = 0x2c;
const PLAN_ABILITY: u32 = 0x4c;
const PLAN_FLAGS: u32 = 0x58;
/// struct Unit's inventory (battle_unit.h) and an entry's parts
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
    /// The plan as BattleAbility_ApplyToTarget first reads it: ability, multiplier,
    /// critical, presentation flags.
    plan: (u32, i8, i8, u32),
    /// Frames each message started.
    messages: Vec<usize>,
}

impl<'a> Guide<'a> {
    /// Isaac attacks a copy of Ivan through the command menu, the rolls
    /// BattleTurn_Build and RollWeaponUnleash ask Battle_Random for
    /// answered in turn from `plan_answers` and `unleash`, and, with
    /// `weapon`, that item in place of his equipped weapon as the battle
    /// opens.
    fn rolled_attack(&self, plan_answers: &[u32], unleash: u32, weapon: Option<u16>) -> Result<Rolled, String> {
        let root = crate::compile::root();
        let plan = Tables::read(root)?.plan()?;
        let mut cast = plan.casts.into_iter().find(|c| c.ability.name == "Quake").ok_or("no Quake cast")?;
        cast.foes = vec![plan::IVAN];
        let symbols = super::battle::Symbols::find(self.map)?;
        let random = self.sym("Battle_Random")?;
        let work_slot = self.sym("gBattleWork")?;
        let exec = self.sym("BattleAbility_ApplyToTarget")?;
        let encounter = self.sym("Battle_Run")?;
        let message = self.sym("BattleMessage_ShowAndWait")?;
        let owner = self.sym("gSaveBlock")? + STATE_OWNERS;
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
                    "BattleTurn_Build" => {
                        asked += 1;
                        plan_answers.get(asked - 1).copied()
                    }
                    "RollWeaponUnleash" => Some(unleash),
                    _ => None,
                };
                if let Some(a) = answer {
                    gba.return_now(a);
                    out.rolls.push(Roll { by, answer: a, forced: true });
                } else if by == "BattleTurn_Build" || by == "RollWeaponUnleash" {
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
    /// at Battle_Random: a miss (the miss roll answered 0, so `& 31` is 0), a
    /// critical hit (the miss roll 1, the critical roll 0), and, wearing the
    /// Swift Sword, its unleash Sonic Smash (the unleash roll 0) struck
    /// three times (the blow roll 0, under 153); each beside the plan's
    /// rolls and the fields BattleAbility_ApplyToTarget then reads, as
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
        // Each run a row: four moments of the attack, and below them what
        // each roll decided and what the plan then holds, in words.
        // The frames each row shows, from the battle's start: the swing and
        // its result, chosen by looking at the runs.
        const SHOTS: [[usize; 4]; 3] = [[115, 130, 145, 160], [130, 160, 205, 250], [145, 205, 235, 265]];
        const LINES: usize = 5;
        const ROW: usize = 24 + HEIGHT + 10 + 18 * LINES + 18;
        let (w, h) = (4 * (WIDTH + 8) + 8, runs.len() * ROW + 8);
        let mut image = canvas(w, h, PANEL_BACK);
        let blows = ["NONE LANDS", "ONE", "TWO", "THREE"];
        for (r, (label, run)) in runs.iter().enumerate() {
            let y = 8 + r * ROW;
            text(&mut image, 8, y as i32, label, PANEL_MARK, None, 2);
            for (k, &f) in SHOTS[r].iter().enumerate() {
                let at = f.min(run.frames.len() - 1);
                paste(&mut image, &screen(&run.frames[at]), 8 + k * (WIDTH + 8), y + 22, 1);
            }
            let (ability, multiplier, critical, flags) = run.plan;
            let action = names.get(ability as usize).map_or("?", String::as_str).to_uppercase();
            let attack = action == "ATTACK";
            // The rolls in the order they were asked for.
            let mut decided: Vec<String> = Vec::new();
            let mut plan_rolls = 0;
            for roll in &run.rolls {
                let line = if roll.by == "RollWeaponUnleash" {
                    if attack { "UNLEASH ROLL: NO UNLEASH".to_string() } else { format!("UNLEASH ROLL: UNLEASHES {action}") }
                } else {
                    plan_rolls += 1;
                    match (attack, plan_rolls) {
                        (true, 1) => format!("HIT ROLL (1 IN 32 MISSES): {}", if multiplier == 0 { "MISSES" } else { "LANDS" }),
                        (true, 2) => format!("CRITICAL ROLL (1 IN 32): {}", if critical != 0 { "CRITICAL" } else { "NOT CRITICAL" }),
                        (true, _) => "EQUIPMENT'S CRITICAL BONUS ROLL: NOT CRITICAL".to_string(),
                        (false, _) => format!("EXTRA BLOWS ROLL (153 IN 256): {}", if multiplier > 1 { "YES" } else { "NO" }),
                    }
                };
                decided.push(line);
            }
            let plan_lines = [
                format!("THE ACTION: {action}"),
                format!("BLOWS: {} (TARGETMULTIPLIER)", blows.get(multiplier as usize).copied().unwrap_or("?")),
                format!("CRITICAL: {}", if critical != 0 { "YES, CRITICAL_HIT" } else { "NO" }),
                format!("THE EFFECT SHOWS {} (ITS VARIANT)", ["ONE BLOW", "TWO BLOWS", "THREE BLOWS", "FOUR BLOWS"][((flags >> 12) & 3) as usize]),
            ];
            let ty = (y + 22 + HEIGHT + 8) as i32;
            text(&mut image, 8, ty, "WHAT EACH ROLL DECIDED:", PANEL_HEAD, None, 2);
            for (i, line) in decided.iter().enumerate() {
                text(&mut image, 16, ty + 18 * (i as i32 + 1), line, PANEL_TEXT, None, 2);
            }
            let px = (8 + 2 * (WIDTH + 8)) as i32;
            text(&mut image, px, ty, "THE PLAN FOR THE TARGET:", PANEL_HEAD, None, 2);
            for (i, line) in plan_lines.iter().enumerate() {
                text(&mut image, px + 8, ty + 18 * (i as i32 + 1), line, PANEL_TEXT, None, 2);
            }
        }
        self.guide_png("plan-rolls.png", &image, "a miss, a critical hit and a triple blow beside the plan's rolls")
    }
}

/// BattleTurn.staging (enum BattleStaging).
const PLAN_OUTCOME: u32 = 0x54;
const STAGING_FLEE: u32 = 7;
fn outcome_name(o: u32) -> &'static str {
    match o {
        1 => "STAGING_MESSAGES",
        2 => "STAGING_ATTACK",
        3 => "STAGING_CAST",
        4 => "STAGING_ITEM",
        5 => "STAGING_SUMMON",
        6 => "STAGING_DJINNI",
        7 => "STAGING_FLEE",
        8 => "STAGING_ENEMY_CAST",
        9 => "STAGING_FULL_SCREEN",
        _ => "?",
    }
}

impl<'a> Guide<'a> {
    /// Isaac's Quake at copies of Garet, Ivan and Mia as `render psynergy`
    /// casts it, with BattleWork.turn.staging rewritten to `outcome` (when
    /// given) where BattleTurn_Build returns to
    /// BattleStaging_DispatchAction, before the dispatch switches on
    /// it; returns the frames, the outcome the plan held, and the return
    /// address found.
    fn staged_quake(&self, outcome: Option<u32>, back: u32) -> Result<(Vec<Vec<u16>>, u32, u32), String> {
        let root = crate::compile::root();
        let plan = Tables::read(root)?.plan()?;
        let cast = plan.casts.into_iter().find(|c| c.ability.name == "Quake").ok_or("no Quake cast")?;
        let symbols = super::battle::Symbols::find(self.map)?;
        let build = self.sym("BattleTurn_Build")?;
        let work_slot = self.sym("gBattleWork")?;
        let mut found = 0;
        let mut own = 0;
        let hooks: Vec<u32> = if back == 0 { vec![build] } else { vec![build, back] };
        let recording = super::battle::record_watching(self.rom, &symbols, &cast, &super::battle::Scene::Game, false, None, &hooks, &mut |gba, hook| match hook {
            Some(h) if h == build => found = gba.register(LR) & !1,
            Some(_) => {
                let p = gba.read32(work_slot) + WORK_PLAN;
                own = gba.read32(p + PLAN_OUTCOME);
                if let Some(o) = outcome {
                    gba.write32(p + PLAN_OUTCOME, o);
                }
            }
            None => {}
        })?;
        Ok((recording.frames, own, found))
    }

    /// Quake played seven times, its plan's outcome rewritten each time to
    /// one of STAGING_CAST to STAGING_FULL_SCREEN as the plan returns,
    /// so the dispatch stages the same action each way; side by side, frame
    /// for frame, as `staging.gif`.
    pub fn staging(&mut self) -> Result<(), String> {
        let (_, _, back) = self.staged_quake(None, 0)?;
        eprintln!("staging: BattleTurn_Build returns to {back:#x} ({})", self.function_at(back));
        let mut runs = Vec::new();
        for o in 3..=9u32 {
            let (frames, own, _) = self.staged_quake(Some(o), back)?;
            eprintln!("  {}: {} frames (Quake's own {})", outcome_name(o), frames.len(), outcome_name(own));
            runs.push((o, own, frames));
        }
        // Fleeing ends the battle; the others end with the round.
        let n = runs.iter().filter(|r| r.0 != STAGING_FLEE).map(|r| r.2.len()).max().unwrap_or(0);
        const CELL_W: usize = WIDTH + 8;
        const CELL_H: usize = HEIGHT + 22;
        let (w, h) = (4 * CELL_W + 8, 2 * CELL_H + 8);
        let mut out = Vec::new();
        // Fifteen pictures a second, each shown for two of the GIF's frames.
        for i in (0..n).step_by(4) {
            let mut image = canvas(w, h, PANEL_BACK);
            for (k, (o, own, frames)) in runs.iter().enumerate() {
                let (x, y) = (8 + (k % 4) * CELL_W, 4 + (k / 4) * CELL_H);
                let label = if o == own { format!("{} (QUAKE'S OWN)", outcome_name(*o)) } else { outcome_name(*o).to_string() };
                text(&mut image, x as i32 - 2, y as i32, &label, if o == own { PANEL_MARK } else { PANEL_TEXT }, None, 2);
                paste(&mut image, &screen(&frames[i.min(frames.len() - 1)]), x, y + 18, 1);
            }
            out.push(image.pixels.clone());
            out.push(image.pixels.clone());
            out.push(image.pixels.clone());
            out.push(image.pixels);
        }
        let written = super::animation::write(&self.out.join("staging.gif"), &out, w)?;
        self.written.push(("staging.gif".to_string(), written.bytes, format!("Quake under each outcome; {} frames", written.frames)));
        Ok(())
    }
}

/// struct Unit's counters (battle_unit.h): the stat changes' turns
/// and stages, then the statuses.
const UNIT_ATTACK_TURNS: u32 = 0x132;
const UNIT_SLEEP: u32 = 0x13c;
const UNIT_HAUNT: u32 = 0x140;
const UNIT_HP: u32 = 0x38;
/// The counters `added-effects` reads, from attackTurns to sleep.
const COUNTERS: [&str; 11] = ["ATTACKTURNS", "ATTACKSTAGE", "DEFENSETURNS", "DEFENSESTAGE", "RESISTANCETURNS", "RESISTANCESTAGE", "DELUSION", "CONFUSION", "CHARM", "STUN", "SLEEP"];
/// abilities.tsv: Impair, Delude, Guard.
const IMPAIR: u16 = 108;
const DELUDE: u16 = 116;
const GUARD: u16 = 106;
/// enum BattleCommand and the places a command aims at
/// (BattleAction.target): a party place from 0x100, an enemy's from
/// 0x180.
const COMMAND_PSYNERGY: u16 = 1;
const COMMAND_DEFEND: u16 = 3;
const PLACE_PARTY: u16 = 0x100;
const PLACE_ENEMY: u16 = 0x180;
const UNIT_AGILITY: u32 = 0x40;
/// formations.tsv row 1: one Vermin.
const ONE_VERMIN: u32 = 1;
/// Frames from a change to its message written out.
const SHOWN: usize = 24;

/// A unit's counters at one moment, attackTurns to sleep.
fn counters(gba: &Gba, unit: u32) -> Vec<i8> {
    (0..COUNTERS.len() as u32).map(|i| gba.read8(unit + UNIT_ATTACK_TURNS + i) as i8).collect()
}

impl<'a> Guide<'a> {
    /// The address of unit `id`'s struct Unit.
    pub(super) fn unit_address(&self, gba: &Gba, id: u16) -> Result<u32, String> {
        if u32::from(id) >= FIRST_ENEMY_UNIT {
            Ok(gba.read32(self.sym("gEnemyUnits")?) + (u32::from(id) - FIRST_ENEMY_UNIT) * UNIT_SIZE)
        } else {
            Ok(self.sym("gSaveBlock")? + STATE_OWNERS + u32::from(id) * UNIT_SIZE)
        }
    }

    /// Fills the command menu's records (Battle_CollectPartyCommands, at
    /// its entry) with `commands`, (unit, command, ability, place, range)
    /// each, and returns from it with their count.
    pub(super) fn give_commands(&self, gba: &mut Gba, commands: &[(u16, u16, u16, u16, u16)]) -> Result<(), String> {
        let record = gba.register(0);
        for (i, &(unit, command, ability, place, range)) in commands.iter().enumerate() {
            let agility = gba.read16(self.unit_address(gba, unit)? + UNIT_AGILITY);
            let fields = [unit, 0, agility, command, ability, place, range, 0];
            for (k, v) in fields.into_iter().enumerate() {
                gba.write16(record + 16 * i as u32 + 2 * k as u32, v);
            }
        }
        gba.return_now(commands.len() as u32);
        Ok(())
    }
}

/// The frames of `added-effects`, the frames its rounds start and those its
/// messages start.
type Rounds = (Vec<Counted>, Vec<usize>, Vec<usize>);

/// A counter changed other than by counting down: the frame, the units, the
/// counter (an index of COUNTERS), its turns before and after, and the
/// stage after.
type Change<'a> = (usize, Vec<&'a str>, usize, i8, i8, i8);

/// One frame of `added-effects`: the screen, and the Vermin's and Isaac's
/// counters.
struct Counted {
    screen: Vec<u16>,
    vermin: Vec<i8>,
    isaac: Vec<i8>,
}

impl<'a> Guide<'a> {
    /// The four at level 35 against one Vermin: in the first round Isaac
    /// casts Impair and Ivan Delude at the Vermin and Mia casts Guard on
    /// Isaac; in each later round the four defend. Each frame keeps the
    /// screen and both units' counters; the rounds' starts and the messages'
    /// starts are kept too.
    fn added_effect_rounds(&self, rounds: usize) -> Result<Rounds, String> {
        let commands = self.sym("Battle_CollectPartyCommands")?;
        let message = self.sym("BattleMessage_ShowAndWait")?;
        let drive = self.quake_drive(ONE_VERMIN, BATTLE_START_SET, vec![commands, message])?;
        let mut frames: Vec<Counted> = Vec::new();
        let mut starts: Vec<usize> = Vec::new();
        let mut messages: Vec<usize> = Vec::new();
        let mut failed: Option<String> = None;
        self.drive(&drive, &mut |gba, hook, f| {
            match hook {
                Some(h) if h == commands => {
                    starts.push(f);
                    let order: Vec<(u16, u16, u16, u16, u16)> = if starts.len() == 1 {
                        vec![
                            (u16::from(plan::ISAAC), COMMAND_PSYNERGY, IMPAIR, PLACE_ENEMY, 1),
                            (u16::from(plan::IVAN), COMMAND_PSYNERGY, DELUDE, PLACE_ENEMY, 2),
                            (u16::from(plan::MIA), COMMAND_PSYNERGY, GUARD, PLACE_PARTY, 1),
                        ]
                    } else {
                        [plan::ISAAC, plan::GARET, plan::IVAN, plan::MIA].iter().enumerate().map(|(i, &o)| (u16::from(o), COMMAND_DEFEND, 0, PLACE_PARTY + i as u16, 1)).collect()
                    };
                    if let Err(e) = self.give_commands(gba, &order) {
                        failed = Some(e);
                    }
                    return Act::Mash;
                }
                Some(_) => {
                    messages.push(f);
                    return Act::Mash;
                }
                None => {}
            }
            let (Ok(v), Ok(i)) = (self.unit_address(gba, FIRST_ENEMY_UNIT as u16), self.unit_address(gba, u16::from(plan::ISAAC))) else {
                return Act::Mash;
            };
            frames.push(Counted { screen: gba.picture(), vermin: counters(gba, v), isaac: counters(gba, i) });
            if starts.len() > rounds {
                Act::Stop
            } else {
                Act::Mash
            }
        })?;
        if let Some(e) = failed {
            return Err(e);
        }
        Ok((frames, starts, messages))
    }

    /// Stat stages and status counters as they change and count down, as
    /// `added-effects.png`.
    pub fn added_effects(&mut self) -> Result<(), String> {
        let (frames, starts, messages) = self.added_effect_rounds(9)?;
        eprintln!("added-effects: {} frames, rounds at {starts:?}", frames.len());
        let mut last: Option<(&Vec<i8>, &Vec<i8>)> = None;
        for (f, c) in frames.iter().enumerate() {
            if last != Some((&c.vermin, &c.isaac)) {
                eprintln!("  frame {f}: vermin {:?} isaac {:?}", c.vermin, c.isaac);
                last = Some((&c.vermin, &c.isaac));
            }
        }
        eprintln!("  messages at {messages:?}");
        // The moments a counter changes other than by counting down, each
        // told in words and shown once its message is written out:
        // (frame, units, counter, turns before, turns after, stage after).
        let mut changes: Vec<Change> = Vec::new();
        for f in 1..frames.len() {
            let (a, b) = (&frames[f - 1], &frames[f]);
            for (unit, before, after) in [("VERMIN", &a.vermin, &b.vermin), ("ISAAC", &a.isaac, &b.isaac)] {
                // Defense turns (its stage changes with it) and delusion.
                for i in [2, 6] {
                    let (x, y) = (before[i], after[i]);
                    if x != y && !(y == x - 1 && y > 0) {
                        match changes.last_mut() {
                            // Two units' changes shown by the same message.
                            Some(c) if f - c.0 < 30 && c.2 == i && (c.3, c.4) == (x, y) => c.1.push(unit),
                            _ => changes.push((f, vec![unit], i, x, y, after[3])),
                        }
                    }
                }
            }
        }
        eprintln!("  changes {changes:?}");
        let possessive = |units: &[&str]| -> String {
            let names: Vec<String> = units.iter().map(|u| if *u == "VERMIN" { "THE VERMIN'S".to_string() } else { format!("{u}'S") }).collect();
            names.join(" AND ")
        };
        let words = |(_, units, i, x, y, stage): &Change| -> [String; 2] {
            let whose = possessive(units);
            match (*i, *x, *y) {
                (6, 0, n) => [format!("{} DELUDED", units.join(" AND ")), format!("FOR {n} TURNS")],
                (6, left, 0) if left > 1 => [format!("{whose} DELUSION ENDS"), format!("EARLY, {left} TURNS LEFT")],
                (6, _, _) => [format!("{whose} DELUSION ENDS"), "ITS TURNS RUN OUT".to_string()],
                (_, 0, n) => [format!("{whose} DEFENSE"), format!("{stage:+} STAGES FOR {n} TURNS")],
                (_, _, 0) => [format!("{whose} DEFENSE"), "BACK TO NORMAL, TURNS RUN OUT".to_string()],
                _ => [format!("{whose} DEFENSE"), format!("{y} TURNS LEFT")],
            }
        };
        const CELL: usize = WIDTH + 8;
        const PER_ROW: usize = 3;
        const SHOT_H: usize = HEIGHT + 44;
        let shot_rows = changes.len().div_ceil(PER_ROW);
        let ty = 46 + shot_rows * SHOT_H + 10;
        let w = PER_ROW * CELL + 16;
        let h = ty + 40 + 20 * starts.len() + 16;
        let mut image = canvas(w, h, PANEL_BACK);
        text(&mut image, 8, 4, "ROUND 1: IVAN CASTS DELUDE AND ISAAC IMPAIR AT THE VERMIN,", PANEL_HEAD, None, 2);
        text(&mut image, 8, 20, "MIA CASTS GUARD ON ISAAC. LATER ROUNDS: THE FOUR DEFEND.", PANEL_HEAD, None, 2);
        for (k, change) in changes.iter().enumerate() {
            let f = change.0;
            // The message this change wrote, once written out.
            let at = messages.iter().find(|&&m| m + 2 >= f).map_or(f, |&m| m).max(f) + SHOWN;
            let at = at.min(frames.len() - 1);
            let (x, y) = (8 + (k % PER_ROW) * CELL, 46 + (k / PER_ROW) * SHOT_H);
            paste(&mut image, &screen(&frames[at].screen), x, y, 1);
            let [one, two] = words(change);
            text(&mut image, x as i32, (y + HEIGHT + 4) as i32, &one, PANEL_MARK, None, 2);
            text(&mut image, x as i32, (y + HEIGHT + 20) as i32, &two, PANEL_MARK, None, 2);
        }
        // The counters as each round's commands are given: after the round
        // before it has ended.
        text(&mut image, 8, ty as i32, "AS EACH ROUND STARTS (A STAGE IS AN EIGHTH OF THE STAT):", PANEL_HEAD, None, 2);
        let columns: [(&str, bool, usize); 3] = [("VERMIN'S DEFENSE", true, 2), ("VERMIN'S DELUSION", true, 6), ("ISAAC'S DEFENSE", false, 2)];
        let cx = |c: usize| (110 + 214 * c) as i32;
        for (c, (label, _, _)) in columns.iter().enumerate() {
            text(&mut image, cx(c), (ty + 22) as i32, label, PANEL_DIM, None, 2);
        }
        for (r, &s) in starts.iter().enumerate() {
            let y = (ty + 42 + 20 * r) as i32;
            text(&mut image, 8, y, &format!("ROUND {}", r + 1), PANEL_DIM, None, 2);
            let counted = &frames[s.min(frames.len() - 1)];
            for (c, &(_, vermin, i)) in columns.iter().enumerate() {
                let v = if vermin { &counted.vermin } else { &counted.isaac };
                let cell = match (i, v[i]) {
                    (2, 0) => "NORMAL".to_string(),
                    (2, turns) => format!("{:+} STAGES, {turns} TURNS", v[3]),
                    (_, 0) => "NONE".to_string(),
                    (_, turns) => format!("{turns} TURNS LEFT"),
                };
                text(&mut image, cx(c), y, &cell, if v[i] == 0 { PANEL_DIM } else { PANEL_MARK }, None, 2);
            }
        }
        self.guide_png("added-effects.png", &image, "stat stages and status counters as they change and count down")
    }
}

/// struct BattleTurn.hauntDamage.
const PLAN_HAUNT: u32 = 0x60;
/// A sleep counter already counted down once: the target fell asleep in an
/// earlier round (STATUS_TURNS less one).
const SLEPT_A_ROUND: u8 = 6;

impl<'a> Guide<'a> {
    /// The return addresses of the Thumb `bl` calls to `callee` inside the
    /// function at `start` (up to the next symbol), in address order, read
    /// from the ROM.
    pub(super) fn calls_in(&self, start: u32, callee: u32) -> Vec<u32> {
        let end = self
            .map
            .at_address(start)
            .and_then(|p| p.symbols.iter().map(|(a, _)| *a & !1).filter(|&a| a > start).min())
            .unwrap_or(start + 0x1000);
        let half = |a: u32| u16::from_le_bytes([self.rom[(a - 0x0800_0000) as usize], self.rom[(a - 0x0800_0000 + 1) as usize]]);
        let mut out = Vec::new();
        let mut a = start;
        while a + 4 <= end {
            let (hi, lo) = (half(a), half(a + 2));
            if hi & 0xf800 == 0xf000 && lo & 0xf800 == 0xf800 {
                let offset = ((i32::from(hi & 0x7ff) << 21) >> 9) + (i32::from(lo & 0x7ff) << 1);
                if (a as i32 + 4 + offset) as u32 == callee {
                    out.push(a + 4);
                }
                a += 4;
            } else {
                a += 2;
            }
        }
        out
    }
    /// Isaac, haunted (Unit.haunt set as the plan is built), casts
    /// Quake at copies of Garet, Ivan and Mia, the copy of Ivan asleep for a
    /// round already (sleep SLEPT_A_ROUND); BattleAbility_ApplyToTarget's wake roll and
    /// haunt roll, its last two calls of Battle_RandomFar, are answered 0,
    /// so the sleeper wakes and every target feeds the evil spirit. Each
    /// target's HP, sleep and BattleTurn.hauntDamage are read as
    /// BattleAbility_ApplyToTarget starts and as the next starts or the turn ends,
    /// beside the screen as each is announced, as `after-target.png`.
    pub fn after_target(&mut self) -> Result<(), String> {
        let root = crate::compile::root();
        let plan = Tables::read(root)?.plan()?;
        let mut cast = plan.casts.into_iter().find(|c| c.ability.name == "Quake").ok_or("no Quake cast")?;
        cast.foes = vec![plan::GARET, plan::IVAN, plan::MIA];
        let foe_name = |u: u16| plan::character_name(cast.foes[usize::from(u) - FIRST_ENEMY_UNIT as usize]).to_uppercase();
        let symbols = super::battle::Symbols::find(self.map)?;
        let random = self.sym("Battle_Random")?;
        let exec = self.sym("BattleAbility_ApplyToTarget")?;
        let build = self.sym("BattleTurn_Build")?;
        let turn_end = self.sym("BattleUnit_ProcessTurnEnd")?;
        let work_slot = self.sym("gBattleWork")?;
        let sites = self.calls_in(exec, self.sym("Battle_RandomFar")?);
        let [.., wake, haunt] = sites[..] else {
            return Err("BattleAbility_ApplyToTarget makes fewer than two rolls".into());
        };
        // Each target: (unit, HP before, after, sleep before, after,
        // hauntDamage after, rolls answered, the frame it ended).
        struct Dealt {
            unit: u16,
            hp: (i16, i16),
            sleep: (u8, u8),
            haunt: u32,
            answered: Vec<&'static str>,
            end: usize,
        }
        let mut dealt: Vec<Dealt> = Vec::new();
        let mut open = false;
        let mut frame = 0usize;
        let mut set = false;
        let mut isaac_hp = (0i16, 0i16);
        let mut ended = 0usize;
        let unit_hp = |gba: &Gba, u: u16| -> i16 { self.unit_address(gba, u).map_or(0, |a| gba.read16(a + UNIT_HP) as i16) };
        let unit_sleep = |gba: &Gba, u: u16| -> u8 { self.unit_address(gba, u).map_or(0, |a| gba.read8(a + UNIT_SLEEP)) };
        let recording = super::battle::record_watching(self.rom, &symbols, &cast, &super::battle::Scene::Game, false, None, &[random, exec, build, turn_end], &mut |gba, hook| {
            let plan_at = gba.read32(work_slot) + WORK_PLAN;
            let close = |gba: &Gba, dealt: &mut Vec<Dealt>, open: &mut bool| {
                if let Some(d) = dealt.last_mut().filter(|_| *open) {
                    d.hp.1 = unit_hp(gba, d.unit);
                    d.sleep.1 = unit_sleep(gba, d.unit);
                    d.haunt = gba.read32(plan_at + PLAN_HAUNT);
                    d.end = frame;
                }
                *open = false;
            };
            match hook {
                Some(h) if h == build && !set => {
                    let isaac = self.unit_address(gba, u16::from(plan::ISAAC)).unwrap_or(0);
                    gba.write8(isaac + UNIT_HAUNT, 1);
                    let ivan = self.unit_address(gba, FIRST_ENEMY_UNIT as u16 + 1).unwrap_or(0);
                    gba.write8(ivan + UNIT_SLEEP, SLEPT_A_ROUND);
                    set = true;
                }
                Some(h) if h == random => {
                    let back = gba.register(LR) & !1;
                    if back == wake || back == haunt {
                        gba.return_now(0);
                        if let Some(d) = dealt.last_mut() {
                            d.answered.push(if back == wake { "WAKE" } else { "HAUNT" });
                        }
                    }
                }
                Some(h) if h == exec => {
                    close(gba, &mut dealt, &mut open);
                    let p = gba.register(0);
                    let u = u16::from(gba.read8(p + PLAN_TARGET_IDS + gba.register(1)));
                    dealt.push(Dealt { unit: u, hp: (unit_hp(gba, u), 0), sleep: (unit_sleep(gba, u), 0), haunt: 0, answered: Vec::new(), end: 0 });
                    open = true;
                }
                Some(h) if h == turn_end => {
                    close(gba, &mut dealt, &mut open);
                    isaac_hp.0 = unit_hp(gba, u16::from(plan::ISAAC));
                    ended = frame;
                }
                Some(_) => {}
                None => {
                    isaac_hp.1 = unit_hp(gba, u16::from(plan::ISAAC));
                    frame += 1;
                }
            }
        })?;
        let frames = recording.frames;
        for d in &dealt {
            eprintln!("after-target: {} HP {:?} sleep {:?} hauntDamage {} answered {:?} until frame {}", foe_name(d.unit), d.hp, d.sleep, d.haunt, d.answered, d.end);
        }
        eprintln!("  turn end at {ended}: Isaac HP {:?}", isaac_hp);
        if dealt.len() != 3 {
            return Err(format!("{} targets", dealt.len()));
        }
        // A shot of each target's last message, and of the evil spirit's,
        // each with what happened told in words.
        const CELL: usize = WIDTH + 8;
        const TOP: usize = 44;
        const PANEL_H: usize = 104;
        let (w, h) = (4 * CELL + 8, TOP + HEIGHT + PANEL_H);
        let mut image = canvas(w, h, PANEL_BACK);
        text(&mut image, 8, 4, "ISAAC, HAUNTED, CASTS QUAKE. THE COPY OF IVAN HAS SLEPT A ROUND (6 TURNS OF", PANEL_HEAD, None, 2);
        text(&mut image, 8, 20, "SLEEP LEFT). THE WAKE AND HAUNT ROLLS ARE ANSWERED 0, SO BOTH COME UP.", PANEL_HEAD, None, 2);
        for (k, d) in dealt.iter().enumerate() {
            let x = 8 + k * CELL;
            paste(&mut image, &screen(&frames[d.end.saturating_sub(SHOWN / 3).min(frames.len() - 1)]), x, TOP, 1);
            let mut y = (TOP + HEIGHT + 6) as i32;
            let lost = d.hp.0 - d.hp.1;
            let lines = [
                (format!("{} LOSES {lost} HP", foe_name(d.unit)), false),
                (if d.sleep.0 == 0 { "AWAKE: NO WAKE ROLL".to_string() } else if d.sleep.1 == 0 { "ASLEEP: THE ROLL WAKES HIM".to_string() } else { "ASLEEP: STAYS ASLEEP".to_string() }, d.sleep.0 != d.sleep.1),
                (format!("SPIRIT'S SHARE: {} HP ({lost} / 4)", lost / 4), false),
                (format!("HAUNTDAMAGE SO FAR: {} HP", d.haunt), true),
            ];
            for (line, marked) in &lines {
                text(&mut image, x as i32, y, line, if *marked { PANEL_MARK } else { PANEL_TEXT }, None, 2);
                y += 18;
            }
            let _ = &d.answered;
        }
        let x = 8 + 3 * CELL;
        paste(&mut image, &screen(&frames[frames.len().saturating_sub(SHOWN)]), x, TOP, 1);
        let mut y = (TOP + HEIGHT + 6) as i32;
        for line in ["THE TURN'S END:".to_string(), format!("THE SPIRIT TAKES {} HP", isaac_hp.0 - isaac_hp.1), format!("ISAAC: {} HP TO {}", isaac_hp.0, isaac_hp.1)] {
            text(&mut image, x as i32, y, &line, PANEL_MARK, None, 2);
            y += 18;
        }
        self.guide_png("after-target.png", &image, "a sleeping target woken by damage, and the evil spirit's share")
    }
}
