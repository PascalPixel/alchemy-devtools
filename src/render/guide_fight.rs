//! Recordings for the battle pages, each the game alone as the player sees
//! it: a battle's opening, the command menu, aiming, a round, a first
//! strike, running away, the Gryphons' choices and Twin Beaks, a weapon's
//! unleash, a miss and a critical hit, a sleeper woken, a battle won and the
//! field coming back, the Fusion Dragon's storm, a critical hit with the
//! whole party on the field, an item used and broken, an enemy running, a
//! summon, an action's end and a turn's end, a level gained in the spoils,
//! and a battle lost. Each plays a battle from Camelot's battle test menu
//! (guide_battle.rs's drive, or `render psynergy`'s record_watching and
//! record) or a random encounter in Goma Cave; a roll is forced only by
//! answering the random call that makes it, and weapons never unleash unless
//! a recording is about the unleash, so an Attack reads as one.

use super::guide::Guide;
use super::guide_battle::{Act, Drive};
use super::animation::is_black_bgr555;
use super::field::CLEARED;
use super::mgba::{Gba, LR, WIDTH};
use super::plan::{self, Tables};

/// KEYINPUT bits.
const KEY_A: u32 = 0x1;
const KEY_B: u32 = 0x2;
const KEY_RIGHT: u32 = 0x10;
const KEY_LEFT: u32 = 0x20;
const KEY_DOWN: u32 = 0x80;

/// gFormations rows: two Gryphons (random), and the Lizard King, a Harridan
/// and two Stone Soldiers (fixed).
const GRYPHONS: u32 = 294;
const FOUR_ENEMIES: u32 = 296;
/// gMainRoutineWork.battleType: BATTLE_TYPE_NORMAL rolls a first strike; 1, which
/// no battle start names, opens the battle normally.
const BATTLE_TYPE_NORMAL: u8 = 0;
const BATTLE_START_SET: u8 = 1;
/// Debug party preset 6 (gDebug_ParameterSets): the four at level 20, whose
/// chance to run from two Gryphons (level 23) is 35 percent.
const PRESET_LEVEL_20: u8 = 6;
/// The first enemy's unit id.
const FIRST_ENEMY_UNIT: u16 = 128;
/// struct BattleAction (battle_session.h): its unit.
const RECORD_UNIT: u32 = 0x00;
/// enum BattleCommand.
const COMMAND_DEFEND: u16 = 3;
const PLACE_PARTY: u16 = 0x100;
/// struct Unit's sleep counter, and a sleep a round old.
const UNIT_SLEEP: u32 = 0x13c;
const SLEPT_A_ROUND: u8 = 6;
/// The Swift Sword, whose unleash is Sonic Smash.
const SWIFT_SWORD: u16 = 21;
/// Rolls for BattleAI_ChooseAction's first draw that land in the Gryphon's
/// spans (ACTION_STRATEGY_FALLING): Wing Flutter 0 to 52, Tornado 53 to 99,
/// Twin Beaks 100 to 140.
const ROLL_WING_FLUTTER: u32 = 20;
const ROLL_TORNADO: u32 = 70;
const ROLL_TWIN_BEAKS: u32 = 120;
/// gFormations rows 35 and 47: one Skeleton, one Fighter Bee. Both pick
/// their actions evenly (ACTION_STRATEGY_EVEN, 32 rolls each): the
/// Skeleton's Bone Charge is its sixth and seventh, rolls 160 to 223, and
/// the Fighter Bee's Ally Search its third and fourth, rolls 64 to 127.
const ONE_SKELETON: u32 = 35;
const ONE_FIGHTER_BEE: u32 = 47;
const ROLL_BONE_CHARGE: u32 = 170;
const ROLL_ALLY_SEARCH: u32 = 80;
/// Frames a recording keeps once the screen has stopped changing, and the
/// pixels that may still change (the prompt arrow) in a still screen.
const STILL_KEPT: usize = 45;
/// Frames between presses of A while enemies act.
const SLOW_A: usize = 40;
const STILL_PIXELS: usize = 120;

/// Keys held for a number of frames, in turn, then nothing.
struct Script {
    steps: Vec<(u32, u32)>,
    start: Option<usize>,
}

impl Script {
    /// Taps: each `keys` held four frames, then `wait` frames of nothing.
    fn taps(taps: &[(u32, u32)]) -> Script {
        Script { steps: taps.iter().flat_map(|&(k, w)| [(k, 4), (0, w)]).collect(), start: None }
    }

    /// The keys at frame `f`, the script started at its first call; None
    /// once it is over.
    fn keys(&mut self, f: usize) -> Option<u32> {
        let mut at = *self.start.get_or_insert(f);
        for &(keys, n) in &self.steps {
            at += n as usize;
            if f < at {
                return Some(keys);
            }
        }
        None
    }

    fn frames(&self) -> usize {
        self.steps.iter().map(|s| s.1 as usize).sum()
    }
}

/// `frames` without the black ones it opens with, and cut STILL_KEPT frames
/// after the screen last changed by more than STILL_PIXELS.
fn trimmed(frames: &[Vec<u16>]) -> Vec<Vec<u16>> {
    let lit = |f: &Vec<u16>| f.iter().filter(|&&c| c & 0x7fff != 0).count() > 2000;
    let first = frames.iter().position(lit).unwrap_or(0);
    let last = frames.len().saturating_sub(1);
    let differs = |a: &Vec<u16>, b: &Vec<u16>| a.iter().zip(b).filter(|(x, y)| x != y).count() > STILL_PIXELS;
    let moved = (first..last).rev().find(|&i| differs(&frames[i], &frames[last])).unwrap_or(last);
    frames[first..(moved + STILL_KEPT).min(frames.len())].to_vec()
}

impl<'a> Guide<'a> {
    fn fight_gif(&mut self, file: &str, frames: &[Vec<u16>], what: &str) -> Result<(), String> {
        let path = self.out.join(file);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        let shown: Vec<&[u16]> = frames.iter().map(|f| f.as_slice()).collect();
        let written = super::animation::write_bgr555(&path, &shown, WIDTH)?;
        self.written.push((file.to_string(), written.bytes, format!("{what}; {} frames at 30 a second", written.frames)));
        Ok(())
    }

    fn party_drive(&self, party: &[u8], encounter: u32, battle_start: u8, hooks: Vec<u32>) -> Result<Drive, String> {
        let mut drive = self.quake_drive(encounter, battle_start, hooks)?;
        drive.party = party.to_vec();
        Ok(drive)
    }

    /// At an entry of Battle_Random: answers a weapon's unleash roll so the
    /// weapon stays quiet; true when it did.
    fn quiet_weapon(&self, gba: &mut Gba) -> bool {
        if self.function_at(gba.register(LR) & !1) == "RollWeaponUnleash" {
            gba.return_now(0xffff);
            return true;
        }
        false
    }

    /// A battle against the Lizard King's four from its first frame: the
    /// enemies are announced as the camera swings in, and it settles behind
    /// the party as the first menu opens, as `battle/opening.gif`.
    pub fn fight_opening(&mut self) -> Result<(), String> {
        let menu = self.sym("Battle_BuildUnitActions")?;
        let message = self.sym("BattleMessage_ShowAndWait")?;
        let drive = self.party_drive(&[plan::ISAAC, plan::GARET, plan::IVAN, plan::MIA], FOUR_ENEMIES, BATTLE_START_SET, vec![menu, message])?;
        let mut frames = Vec::new();
        let mut opened: Option<usize> = None;
        let mut messages = Vec::new();
        self.drive(&drive, &mut |gba, hook, f| {
            match hook {
                Some(h) if h == menu => {
                    opened.get_or_insert(f);
                    return Act::Keys(0);
                }
                Some(_) => {
                    messages.push(f);
                    return Act::Keys(0);
                }
                None => {}
            }
            frames.push(gba.picture());
            match (opened, messages.last()) {
                (Some(o), _) if f > o + 50 => Act::Stop,
                (None, Some(&m)) if f > m + 50 => Act::Mash,
                _ => Act::Keys(0),
            }
        })?;
        eprintln!("fight-opening: {} frames, messages at {messages:?}, menu at {opened:?}", frames.len());
        self.fight_gif("battle/opening.gif", &trimmed(&frames), "a battle opening")
    }

    /// The four against the Lizard King's four, the commands chosen through
    /// the menu with `keys` from its first frame (A pressed after the script
    /// ends); kept from the menu until `after` frames past the first action,
    /// or with `after` None until the script ends.
    fn menu_run(&self, mut keys: Script, after: Option<usize>) -> Result<Vec<Vec<u16>>, String> {
        let menu = self.sym("Battle_BuildUnitActions")?;
        let dispatch = self.sym("BattleStaging_DispatchAction")?;
        let random = self.sym("Battle_Random")?;
        let drive = self.party_drive(&[plan::ISAAC, plan::GARET, plan::IVAN, plan::MIA], FOUR_ENEMIES, BATTLE_START_SET, vec![menu, dispatch, random])?;
        let mut frames = Vec::new();
        let mut opened: Option<usize> = None;
        let mut acted: Option<usize> = None;
        let length = keys.frames();
        self.drive(&drive, &mut |gba, hook, f| {
            match hook {
                Some(h) if h == random => {
                    self.quiet_weapon(gba);
                    return Act::Keys(0);
                }
                Some(h) if h == menu => {
                    opened.get_or_insert(f);
                    return Act::Keys(0);
                }
                Some(_) => {
                    acted.get_or_insert(f);
                    return Act::Keys(0);
                }
                None => {}
            }
            frames.push(gba.picture());
            let over = match (after, opened, acted) {
                (Some(a), _, Some(at)) => f > at + a,
                (None, Some(o), _) => f > o + length,
                _ => false,
            };
            if over {
                return Act::Stop;
            }
            match opened {
                None => Act::Mash,
                Some(_) => keys.keys(f).map_or(Act::Mash, Act::Keys),
            }
        })?;
        let o = opened.ok_or("the menu never opened")?;
        eprintln!("menu run: {} frames, menu at {o}, first action at {acted:?}", frames.len());
        Ok(frames[o..].to_vec())
    }

    /// The command menu: Fight; Isaac opens his Psynergy, picks Quake and
    /// aims it; Garet looks at his Djinn, backs out and attacks; Ivan
    /// defends; Mia attacks; the round begins, as `battle/commands.gif`.
    pub fn fight_commands(&mut self) -> Result<(), String> {
        let keys = Script::taps(&[
            (0, 26),
            (KEY_A, 36),
            (KEY_RIGHT, 30),
            (KEY_A, 46),
            (KEY_DOWN, 30),
            (KEY_A, 40),
            (KEY_A, 46),
            (KEY_RIGHT, 16),
            (KEY_RIGHT, 30),
            (KEY_A, 70),
            (KEY_B, 30),
            (KEY_LEFT, 16),
            (KEY_LEFT, 30),
            (KEY_A, 36),
            (KEY_A, 46),
            (KEY_LEFT, 30),
            (KEY_A, 46),
            (KEY_A, 36),
            (KEY_A, 36),
        ]);
        let frames = self.menu_run(keys, Some(70))?;
        self.fight_gif("battle/commands.gif", &frames, "the command menu")
    }

    /// Aiming Quake: Fight, Psynergy, Quake, then the cursor moved right
    /// along the four enemies and round from the last to the first, as
    /// `battle/aim.gif`.
    pub fn fight_aim(&mut self) -> Result<(), String> {
        let keys = Script::taps(&[(0, 26), (KEY_A, 36), (KEY_RIGHT, 30), (KEY_A, 46), (KEY_DOWN, 30), (KEY_A, 46), (KEY_RIGHT, 40), (KEY_RIGHT, 40), (KEY_RIGHT, 40), (KEY_RIGHT, 40), (KEY_RIGHT, 50)]);
        let frames = self.menu_run(keys, None)?;
        self.fight_gif("battle/aim.gif", &frames, "aiming Quake")
    }
}

impl<'a> Guide<'a> {
    /// One round from the menu's first frame, every command Attack (A
    /// pressed throughout), until the next round's menu.
    fn one_round(&mut self, party: &[u8], encounter: u32, file: &str, what: &str) -> Result<(), String> {
        let menu = self.sym("Battle_BuildUnitActions")?;
        let dispatch = self.sym("BattleStaging_DispatchAction")?;
        let random = self.sym("Battle_Random")?;
        let drive = self.party_drive(party, encounter, BATTLE_START_SET, vec![menu, dispatch, random])?;
        let mut frames = Vec::new();
        let mut menus: Vec<usize> = Vec::new();
        let mut acts: Vec<(usize, String)> = Vec::new();
        self.drive(&drive, &mut |gba, hook, f| {
            match hook {
                Some(h) if h == random => {
                    self.quiet_weapon(gba);
                    return Act::Mash;
                }
                Some(h) if h == menu => {
                    menus.push(f);
                    return Act::Mash;
                }
                Some(_) => {
                    let unit = gba.read16(gba.register(0) + RECORD_UNIT);
                    acts.push((f, self.unit_name(gba, unit)));
                    return Act::Mash;
                }
                None => {}
            }
            frames.push(gba.picture());
            match menus.get(1) {
                Some(&m) if f > m + 30 => Act::Stop,
                // The next round's menu shows, untouched.
                Some(_) => Act::Keys(0),
                None => Act::Mash,
            }
        })?;
        eprintln!("{file}: menus at {menus:?}, actions {acts:?}");
        let start = menus.first().copied().ok_or("no menu")?;
        self.fight_gif(file, &frames[start..], what)
    }

    /// Isaac and Garet against two Gryphons, as `battle/round.gif`.
    pub fn fight_round(&mut self) -> Result<(), String> {
        self.one_round(&[plan::ISAAC, plan::GARET], GRYPHONS, "battle/round.gif", "a round against two Gryphons")
    }

    /// The four against the Lizard King's four, as `battle/turn-order.gif`.
    pub fn fight_turn_order(&mut self) -> Result<(), String> {
        self.one_round(&[plan::ISAAC, plan::GARET, plan::IVAN, plan::MIA], FOUR_ENEMIES, "battle/turn-order.gif", "a round against the Lizard King's four")
    }

    /// Isaac and Garet in a random encounter with two Gryphons, Battle_Run's
    /// first-strike roll answered 0 (`& 15` is 0): `Isaac's party attacks
    /// first!`, held a moment; the two attack while the Gryphons stand
    /// still, and the next round's menu opens, as `battle/first-strike.gif`.
    pub fn fight_first_strike(&mut self) -> Result<(), String> {
        let random = self.sym("Battle_Random")?;
        let menu = self.sym("Battle_BuildUnitActions")?;
        let message = self.sym("BattleMessage_ShowAndWait")?;
        let drive = self.party_drive(&[plan::ISAAC, plan::GARET], GRYPHONS, BATTLE_TYPE_NORMAL, vec![random, menu, message])?;
        let mut frames = Vec::new();
        let mut menus: Vec<usize> = Vec::new();
        let mut answered = false;
        let mut shown: Option<usize> = None;
        self.drive(&drive, &mut |gba, hook, f| {
            match hook {
                Some(h) if h == random => {
                    if !self.quiet_weapon(gba) && !answered && self.function_at(gba.register(LR) & !1) == "Battle_Run" {
                        gba.return_now(0);
                        answered = true;
                    }
                    return Act::Mash;
                }
                Some(h) if h == message => {
                    // MsgPartyStrikesFirst.
                    if gba.register(0) == 2066 {
                        shown = Some(f);
                    }
                    return Act::Mash;
                }
                Some(_) => {
                    menus.push(f);
                    return Act::Mash;
                }
                None => {}
            }
            frames.push(gba.picture());
            if menus.len() >= 2 && f > menus[1] + 40 {
                return Act::Stop;
            }
            match shown {
                Some(s) if f < s + 80 => Act::Keys(0),
                _ => Act::Mash,
            }
        })?;
        eprintln!("fight-first-strike: menus at {menus:?}, answered {answered}, message at {shown:?}");
        if !answered {
            return Err("Battle_Run made no first-strike roll".into());
        }
        self.fight_gif("battle/first-strike.gif", &trimmed(&frames), "a first strike")
    }

    /// The four at level 20 against two Gryphons choose Run twice: the first
    /// try's roll (Random16 in BattleFlee_CheckSuccess) answered 0xffff, so
    /// it fails and the Gryphons act; the second answered 0, so the party
    /// gets away, as `battle/flee.gif`.
    pub fn fight_flee(&mut self) -> Result<(), String> {
        let random = self.sym("Random16")?;
        let menu = self.sym("Battle_BuildUnitActions")?;
        let work = self.sym("gBattleWork")?;
        let mut drive = self.party_drive(&[plan::ISAAC, plan::GARET, plan::IVAN, plan::MIA], GRYPHONS, BATTLE_START_SET, vec![random, menu])?;
        drive.preset = PRESET_LEVEL_20;
        let mut frames = Vec::new();
        let mut menus: Vec<usize> = Vec::new();
        let mut tries = 0;
        let mut keys: Option<Script> = None;
        let mut gone: Option<usize> = None;
        self.drive(&drive, &mut |gba, hook, f| {
            match hook {
                Some(h) if h == random => {
                    if self.function_at(gba.register(LR) & !1) == "BattleFlee_CheckSuccess" {
                        gba.return_now(if tries == 0 { 0xffff } else { 0 });
                        tries += 1;
                    }
                    return Act::Keys(0);
                }
                Some(_) => {
                    menus.push(f);
                    keys = Some(Script::taps(&[(0, 30), (KEY_RIGHT, 40), (KEY_A, 30)]));
                    return Act::Keys(0);
                }
                None => {}
            }
            frames.push(gba.picture());
            if tries >= 2 && gone.is_none() && gba.read32(work) == 0 {
                gone = Some(f);
            }
            if gone.is_some_and(|g| f > g) || f > 4000 {
                return Act::Stop;
            }
            match keys.as_mut().and_then(|k| k.keys(f)) {
                Some(k) => Act::Keys(k),
                None => Act::Mash,
            }
        })?;
        eprintln!("fight-flee: menus at {menus:?}, tries {tries}, gone {gone:?}, {} frames", frames.len());
        if tries != 2 {
            return Err(format!("{tries} tries to run"));
        }
        let start = menus.first().copied().unwrap_or(0);
        self.fight_gif("battle/flee.gif", &frames[start..], "running away")
    }
}

impl<'a> Guide<'a> {
    /// The four defend while the enemies of `encounter` act, for one round:
    /// the first Battle_Random after each enemy's turn enters
    /// BattleAI_ChooseAction with retry 1, as the turn plays (its draw of an
    /// action), answered with `rolls` in turn; with `sure`, every
    /// Battle_RandomPercent after that answered 0, so an added effect's roll
    /// lands. Returns the frames and the frames each enemy's action was
    /// dispatched.
    fn enemy_round(&self, encounter: u32, rolls: &[u32], sure: bool) -> Result<(Vec<Vec<u16>>, Vec<usize>), String> {
        let commands = self.sym("Battle_CollectPartyCommands")?;
        let dispatch = self.sym("BattleStaging_DispatchAction")?;
        let random = self.sym("Battle_Random")?;
        let choose = self.sym("BattleAI_ChooseAction")?;
        let percent = self.sym("Battle_RandomPercent")?;
        let party = [plan::ISAAC, plan::GARET, plan::IVAN, plan::MIA];
        let drive = self.party_drive(&party, encounter, BATTLE_START_SET, vec![commands, dispatch, random, choose, percent])?;
        let mut frames = Vec::new();
        let mut starts = 0;
        let mut turns: Vec<usize> = Vec::new();
        // Each turn's first draw is answered, once.
        let mut pending = false;
        let mut answered = 0;
        let mut failed = None;
        let mut round_over: Option<usize> = None;
        self.drive(&drive, &mut |gba, hook, f| {
            match hook {
                Some(h) if h == commands => {
                    starts += 1;
                    if starts > 1 {
                        // No commands given: the menu opens and waits.
                        return Act::Keys(0);
                    }
                    let order: Vec<(u16, u16, u16, u16, u16)> = party.iter().enumerate().map(|(i, &o)| (u16::from(o), COMMAND_DEFEND, 0, PLACE_PARTY + i as u16, 1)).collect();
                    if let Err(e) = self.give_commands(gba, &order) {
                        failed = Some(e);
                    }
                    return Act::Mash;
                }
                Some(h) if h == choose => {
                    pending = gba.register(1) == 1;
                    return Act::Mash;
                }
                Some(h) if h == percent => {
                    if sure && answered > 0 {
                        gba.return_now(0);
                    }
                    return Act::Mash;
                }
                Some(h) if h == random => {
                    if pending {
                        if let Some(&r) = rolls.get(answered) {
                            gba.return_now(r);
                        }
                        answered += 1;
                        pending = false;
                    }
                    return Act::Mash;
                }
                Some(_) => {
                    if gba.read16(gba.register(0) + RECORD_UNIT) >= FIRST_ENEMY_UNIT {
                        turns.push(f);
                    }
                    return Act::Mash;
                }
                None => {}
            }
            frames.push(gba.picture());
            if starts > 1 {
                // The next round's command view, a moment.
                let end = *round_over.get_or_insert(f + 30);
                return if f >= end { Act::Stop } else { Act::Keys(0) };
            }
            // Once the enemies act, A only every SLOW_A frames, so each
            // message can be read before it goes.
            if turns.is_empty() {
                Act::Mash
            } else {
                Act::Keys(if f % SLOW_A < 4 { KEY_A } else { 0 })
            }
        })?;
        if let Some(e) = failed {
            return Err(e);
        }
        eprintln!("enemy round: {} frames, enemy turns at {turns:?}, {answered} draws answered", frames.len());
        Ok((frames, turns))
    }

    /// The Gryphons' turns while the party defends: the first uses Wing
    /// Flutter and the second Tornado, as `battle/gryphons.gif`.
    pub fn fight_gryphons(&mut self) -> Result<(), String> {
        let (frames, turns) = self.enemy_round(GRYPHONS, &[ROLL_WING_FLUTTER, ROLL_TORNADO], false)?;
        let first = *turns.first().ok_or("no Gryphon acted")?;
        self.fight_gif("battle/gryphons.gif", &trimmed(&frames[first.saturating_sub(10)..]), "two Gryphons' turns")
    }

    /// The first Gryphon's Twin Beaks, from its announcement to the second
    /// Gryphon's action, as `battle_anim/twin-beaks.gif`.
    pub fn fight_twin_beaks(&mut self) -> Result<(), String> {
        let (frames, turns) = self.enemy_round(GRYPHONS, &[ROLL_TWIN_BEAKS, ROLL_WING_FLUTTER], false)?;
        let first = *turns.first().ok_or("no Gryphon acted")?;
        let end = turns.get(1).copied().unwrap_or(frames.len()).min(frames.len());
        self.fight_gif("battle_anim/twin-beaks.gif", &trimmed(&frames[first.saturating_sub(10)..end]), "a Gryphon's Twin Beaks")
    }

    /// A Skeleton's Bone Charge, a rushing strike, as
    /// `battle_anim/bone-charge.gif`.
    pub fn fight_bone_charge(&mut self) -> Result<(), String> {
        let (frames, turns) = self.enemy_round(ONE_SKELETON, &[ROLL_BONE_CHARGE], false)?;
        let first = *turns.first().ok_or("the Skeleton never acted")?;
        self.fight_gif("battle_anim/bone-charge.gif", &trimmed(&frames[first.saturating_sub(10)..]), "a Skeleton's Bone Charge")
    }

    /// A Fighter Bee's Ally Search, its roll landing: a second Fighter Bee
    /// joins, as `battle/ally-search.gif`; the screen hardly changes while
    /// the bee looks, so the turn is kept whole rather than trimmed.
    pub fn fight_ally_search(&mut self) -> Result<(), String> {
        let (frames, turns) = self.enemy_round(ONE_FIGHTER_BEE, &[ROLL_ALLY_SEARCH], true)?;
        let first = *turns.first().ok_or("the Fighter Bee never acted")?;
        self.fight_gif("battle/ally-search.gif", &frames[first.saturating_sub(10)..], "a Fighter Bee's Ally Search")
    }
}

impl<'a> Guide<'a> {
    /// Isaac attacks a copy of Ivan wearing the Swift Sword, its unleash roll
    /// answered 0 and its blow roll 0: `Isaac's Swift Sword lets out a
    /// howl!`, then Sonic Smash strikes three times, as `battle/unleash.gif`.
    pub fn fight_unleash(&mut self) -> Result<(), String> {
        let frames = self.rolled_attack(&[0], 0, Some(SWIFT_SWORD))?.frames;
        self.fight_gif("battle/unleash.gif", &trimmed(&frames), "a weapon's unleash")
    }

    /// Isaac attacks a copy of Ivan twice: a miss (the miss roll answered 0)
    /// and a critical hit (the miss roll 1, the critical roll 0), one after
    /// the other, as `battle/rolls.gif`.
    pub fn fight_rolls(&mut self) -> Result<(), String> {
        let mut frames = trimmed(&self.rolled_attack(&[0, 1, 0xffff], 0xffff, None)?.frames);
        frames.extend(trimmed(&self.rolled_attack(&[1, 0], 0xffff, None)?.frames));
        self.fight_gif("battle/rolls.gif", &frames, "a miss and a critical hit")
    }

    /// Isaac casts Quake at copies of Garet, Ivan and Mia, the copy of Ivan
    /// asleep a round already; BattleAbility_ApplyToTarget's wake roll
    /// answered 0, so `Ivan wakes from slumber!`, as `battle/wake.gif`.
    pub fn fight_wake(&mut self) -> Result<(), String> {
        let root = crate::compile::root();
        let plan = Tables::read(root)?.plan()?;
        let mut cast = plan.casts.into_iter().find(|c| c.ability.name == "Quake").ok_or("no Quake cast")?;
        cast.foes = vec![plan::GARET, plan::IVAN, plan::MIA];
        let symbols = super::battle::Symbols::find(self.map)?;
        let random = self.sym("Battle_Random")?;
        let exec = self.sym("BattleAbility_ApplyToTarget")?;
        let commands = self.sym("Battle_CollectPartyCommands")?;
        let sites = self.calls_in(exec, self.sym("Battle_RandomFar")?);
        let [.., wake, _haunt] = sites[..] else {
            return Err("BattleAbility_ApplyToTarget makes fewer than two rolls".into());
        };
        let mut set = false;
        let mut woke = 0;
        let recording = super::battle::record_watching(self.rom, &symbols, &cast, &super::battle::Scene::Game, false, None, &[random, commands], &mut |gba, hook| match hook {
            Some(h) if h == commands && !set => {
                if let Ok(ivan) = self.unit_address(gba, FIRST_ENEMY_UNIT + 1) {
                    gba.write8(ivan + UNIT_SLEEP, SLEPT_A_ROUND);
                    set = true;
                }
            }
            Some(h) if h == random => {
                if gba.register(LR) & !1 == wake {
                    gba.return_now(0);
                    woke += 1;
                }
            }
            _ => {}
        })?;
        eprintln!("fight-wake: {} frames, wake rolls answered {woke}", recording.frames.len());
        if woke == 0 {
            return Err("no wake roll was made".into());
        }
        self.fight_gif("battle/wake.gif", &recording.frames, "a sleeper woken by Quake")
    }
}

impl<'a> Guide<'a> {
    /// A random battle in Goma Cave played to its end with A pressed, the
    /// party attacking, until the cave comes back: kept from BACK_FRAMES
    /// before the field returns, as `battle/won.gif`.
    pub fn fight_won(&mut self) -> Result<(), String> {
        const BACK_FRAMES: usize = 480;
        let (place, stretch) = self.goma_cave_stretch()?;
        let mut entered = self.enter_goma_cave(place)?;
        let field = &mut entered.field;
        let mut frames = Vec::new();
        Self::walk_stretch(field, &stretch, 80, &mut frames, &mut 0, &mut |_| {})?.ok_or("no battle came")?;
        let battle = field.scene();
        frames.clear();
        let mut back: Option<usize> = None;
        for f in 0..20000usize {
            // A until the cave is back, which would open the field menu.
            field.hold(if back.is_none() && (f / 4) % 2 == 1 { KEY_A } else { 0 });
            field.frames(1)?;
            frames.push(field.picture());
            if back.is_none() && field.scene() != battle {
                back = Some(frames.len());
            }
            if back.is_some_and(|b| frames.len() > b + 120) {
                break;
            }
        }
        field.hold(0);
        let b = back.ok_or("the battle never ended")?;
        eprintln!("fight-won: {} frames, field back at {b}", frames.len());
        self.fight_gif("battle/won.gif", &frames[b.saturating_sub(BACK_FRAMES)..], "a battle won and the cave back")
    }
}

/// gFormations rows: one or two Rat Soldiers, with up to two Rats and a
/// Bat as Battle_Run rolls them, and the Fusion Dragon on the Venus
/// Lighthouse aerie, whose battle opens in a storm (transition.c's
/// TRANSITION_STORM_FORMATION).
const RAT_SOLDIERS: u32 = 55;
const FUSION_DRAGON: u32 = 347;
/// enum BattleCommand: COMMAND_ATTACK, and COMMAND_ITEM, whose parameter is
/// an inventory slot; the first enemy's place.
const COMMAND_ATTACK: u16 = 0;
const COMMAND_ITEM: u16 = 2;
const PLACE_ENEMY: u16 = 0x180;
/// The Healing Ring, which casts Cure and may break (ITEM_USE_MAY_BREAK),
/// put in Mia's last inventory slot, which the debug parameter set leaves
/// empty.
const HEALING_RING: u16 = 0x109;
const RING_SLOT: u16 = 14;
/// struct Unit: its inventory and its HP.
const UNIT_INVENTORY: u32 = 0xd8;
const UNIT_HP: u32 = 0x38;
/// The Rat Soldier's first two actions are Flee: rolls 0 to 63 of its even
/// draw (ACTION_STRATEGY_EVEN).
const ROLL_FLEE: u32 = 10;
/// Frames between presses of A in `commanded_round`, long enough to read
/// two lines of messages and to watch a unit leave.
const READ_A: usize = 80;

/// What `commanded_round` asks to answer: a random call, by the function
/// that made it and the unit acting (empty before the first action), or an
/// enemy's draw of its action, by the enemy's name.
enum Asked<'n> {
    Call { by: &'n str, actor: &'n str },
    Choice(&'n str),
}

impl<'a> Guide<'a> {
    /// One round of `encounter` with the four's commands given, (unit,
    /// command, parameter, target, range) each: `prepare` is called as the
    /// commands are given, and `answer` at each Battle_Random and Random16,
    /// returning the number to answer with or None to let the call run.
    /// Returns the frames and the frame and unit of each action dispatched.
    #[allow(clippy::type_complexity)]
    fn commanded_round(&self, encounter: u32, commands: &[(u16, u16, u16, u16, u16)], prepare: &mut dyn FnMut(&mut Gba), answer: &mut dyn FnMut(Asked) -> Option<u32>) -> Result<(Vec<Vec<u16>>, Vec<(usize, String)>), String> {
        let given = self.sym("Battle_CollectPartyCommands")?;
        let dispatch = self.sym("BattleStaging_DispatchAction")?;
        let random = self.sym("Battle_Random")?;
        let random16 = self.sym("Random16")?;
        let choose = self.sym("BattleAI_ChooseAction")?;
        let party = [plan::ISAAC, plan::GARET, plan::IVAN, plan::MIA];
        let drive = self.party_drive(&party, encounter, BATTLE_START_SET, vec![given, dispatch, random, random16, choose])?;
        let mut frames = Vec::new();
        let mut starts = 0;
        let mut acts: Vec<(usize, String)> = Vec::new();
        // The enemy whose draw of an action the next Battle_Random is.
        let mut choosing: Option<String> = None;
        let mut failed = None;
        let mut round_over: Option<usize> = None;
        self.drive(&drive, &mut |gba, hook, f| {
            match hook {
                Some(h) if h == given => {
                    starts += 1;
                    if starts > 1 {
                        // No commands given: the menu opens and waits.
                        return Act::Keys(0);
                    }
                    prepare(gba);
                    if let Err(e) = self.give_commands(gba, commands) {
                        failed = Some(e);
                    }
                    return Act::Mash;
                }
                Some(h) if h == choose => {
                    // Retry 1: the draw as the enemy acts.
                    if gba.register(1) == 1 {
                        let unit = gba.read16(gba.register(0) + RECORD_UNIT);
                        choosing = Some(self.unit_name(gba, unit));
                    }
                    return Act::Mash;
                }
                Some(h) if h == random || h == random16 => {
                    if h == random && self.quiet_weapon(gba) {
                        return Act::Mash;
                    }
                    let by = self.function_at(gba.register(LR) & !1);
                    let asked = match (h == random, choosing.take()) {
                        (true, Some(name)) => answer(Asked::Choice(&name)),
                        _ => answer(Asked::Call { by: &by, actor: acts.last().map_or("", |a| a.1.as_str()) }),
                    };
                    if let Some(a) = asked {
                        gba.return_now(a);
                    }
                    return Act::Mash;
                }
                Some(_) => {
                    let unit = gba.read16(gba.register(0) + RECORD_UNIT);
                    acts.push((f, self.unit_name(gba, unit)));
                    return Act::Mash;
                }
                None => {}
            }
            frames.push(gba.picture());
            if starts > 1 {
                let end = *round_over.get_or_insert(f + 30);
                return if f >= end { Act::Stop } else { Act::Keys(0) };
            }
            // Once the round plays, A only every READ_A frames, so each
            // message can be read before it goes.
            if acts.is_empty() {
                Act::Mash
            } else {
                Act::Keys(if f % READ_A < 4 { KEY_A } else { 0 })
            }
        })?;
        if let Some(e) = failed {
            return Err(e);
        }
        eprintln!("commanded round: {} frames, actions {acts:?}", frames.len());
        Ok((frames, acts))
    }

    /// The frames from `before` frames before the first action `who` matches
    /// to ten after the next action starts.
    fn one_action(frames: &[Vec<u16>], acts: &[(usize, String)], before: usize, who: &dyn Fn(&str) -> bool) -> Result<Vec<Vec<u16>>, String> {
        let at = acts.iter().position(|(_, name)| who(name)).ok_or("the unit never acted")?;
        let start = acts[at].0.saturating_sub(before);
        let end = acts.get(at + 1).map_or(frames.len(), |a| a.0 + 10).min(frames.len());
        Ok(trimmed(&frames[start..end]))
    }

    /// Isaac attacks a Gryphon while the others defend, and his plan's rolls
    /// (Battle_Random in BattleActionRun_Build) are answered 1 for the miss
    /// and 0 for the critical hit: the field clears to the two, the camera
    /// zooms in over the run up and the blow lands as the ring and spark
    /// effect, as `battle/critical.gif`. Kept from his action to the next.
    pub fn fight_critical(&mut self) -> Result<(), String> {
        let commands = [
            (u16::from(plan::ISAAC), COMMAND_ATTACK, 0, PLACE_ENEMY, 1),
            (u16::from(plan::GARET), COMMAND_DEFEND, 0, PLACE_PARTY + 1, 1),
            (u16::from(plan::IVAN), COMMAND_DEFEND, 0, PLACE_PARTY + 2, 1),
            (u16::from(plan::MIA), COMMAND_DEFEND, 0, PLACE_PARTY + 3, 1),
        ];
        let answers = [1, 0];
        let mut asked = 0;
        let (frames, acts) = self.commanded_round(GRYPHONS, &commands, &mut |_| {}, &mut |call| match call {
            Asked::Call { by: "BattleActionRun_Build", actor: "Isaac" } => {
                asked += 1;
                answers.get(asked - 1).copied()
            }
            _ => None,
        })?;
        if asked < answers.len() {
            return Err(format!("Isaac's attack made {asked} rolls"));
        }
        let frames = Self::one_action(&frames, &acts, 10, &|name| name == "Isaac")?;
        self.fight_gif("battle/critical.gif", &frames, "a critical hit")
    }

    /// Mia uses a Healing Ring on Garet, hurt to a third of his HP, while
    /// the others defend against two Gryphons: the background dims while
    /// Cure plays, and the ring's break roll (Battle_Random in
    /// BattleStaging_RunItem, `& 7`) is answered 0, so it breaks, as
    /// `battle/item.gif`. Kept from Mia's action to the next.
    pub fn fight_item(&mut self) -> Result<(), String> {
        let commands = [
            (u16::from(plan::ISAAC), COMMAND_DEFEND, 0, PLACE_PARTY, 1),
            (u16::from(plan::GARET), COMMAND_DEFEND, 0, PLACE_PARTY + 1, 1),
            (u16::from(plan::IVAN), COMMAND_DEFEND, 0, PLACE_PARTY + 2, 1),
            (u16::from(plan::MIA), COMMAND_ITEM, RING_SLOT, PLACE_PARTY + 1, 1),
        ];
        let mut broke = 0;
        let (frames, acts) = self.commanded_round(
            GRYPHONS,
            &commands,
            &mut |gba| {
                if let (Ok(mia), Ok(garet)) = (self.unit_address(gba, plan::MIA.into()), self.unit_address(gba, plan::GARET.into())) {
                    gba.write16(mia + UNIT_INVENTORY + 2 * u32::from(RING_SLOT), HEALING_RING);
                    let hp = gba.read16(garet + UNIT_HP);
                    gba.write16(garet + UNIT_HP, hp / 3);
                }
            },
            &mut |asked| match asked {
                Asked::Call { by: "BattleStaging_RunItem", .. } => {
                    broke += 1;
                    Some(0)
                }
                _ => None,
            },
        )?;
        if broke == 0 {
            return Err("the Healing Ring made no break roll".into());
        }
        let frames = Self::one_action(&frames, &acts, 10, &|name| name == "Mia")?;
        self.fight_gif("battle/item.gif", &frames, "an item used and broken")
    }

    /// A Rat Soldier runs: the four defend, its draw of an action answered
    /// with Flee and BattleStaging_RunFlee's roll (Random16, 7 in 10 away)
    /// answered 0, so it gets away, as `battle/enemy-flees.gif`. Kept from a
    /// moment before its action to the next.
    pub fn fight_enemy_flees(&mut self) -> Result<(), String> {
        let commands: Vec<(u16, u16, u16, u16, u16)> = [plan::ISAAC, plan::GARET, plan::IVAN, plan::MIA].iter().enumerate().map(|(i, &o)| (u16::from(o), COMMAND_DEFEND, 0, PLACE_PARTY + i as u16, 1)).collect();
        let mut fled = 0;
        let mut chosen = false;
        let (frames, acts) = self.commanded_round(RAT_SOLDIERS, &commands, &mut |_| {}, &mut |asked| match asked {
            Asked::Choice(name) if name.starts_with("Rat Soldier") && !chosen => {
                chosen = true;
                Some(ROLL_FLEE)
            }
            Asked::Call { by: "BattleStaging_RunFlee", .. } => {
                fled += 1;
                Some(0)
            }
            _ => None,
        })?;
        if fled == 0 {
            return Err("no Rat Soldier ran".into());
        }
        let frames = Self::one_action(&frames, &acts, 70, &|name| name.starts_with("Rat Soldier"))?;
        self.fight_gif("battle/enemy-flees.gif", &frames, "an enemy running away")
    }

    /// The battle against the Fusion Dragon from its first frame: the storm,
    /// the enemies' shattering opening and the party fading in, kept until
    /// the first menu has opened, as `battle/storm.gif`.
    pub fn fight_storm(&mut self) -> Result<(), String> {
        let menu = self.sym("Battle_BuildUnitActions")?;
        let message = self.sym("BattleMessage_ShowAndWait")?;
        let drive = self.party_drive(&[plan::ISAAC, plan::GARET, plan::IVAN, plan::MIA], FUSION_DRAGON, BATTLE_START_SET, vec![menu, message])?;
        let mut frames = Vec::new();
        let mut opened: Option<usize> = None;
        let mut messages = Vec::new();
        self.drive(&drive, &mut |gba, hook, f| {
            match hook {
                Some(h) if h == menu => {
                    opened.get_or_insert(f);
                    return Act::Keys(0);
                }
                Some(_) => {
                    messages.push(f);
                    return Act::Keys(0);
                }
                None => {}
            }
            frames.push(gba.picture());
            // `Fusion Dragon appeared!` is left a moment before A.
            match (opened, messages.last()) {
                (Some(o), _) if f > o + 50 => Act::Stop,
                (None, Some(&m)) if f > m + 60 => Act::Mash,
                (None, None) => Act::Mash,
                _ => Act::Keys(0),
            }
        })?;
        eprintln!("fight-storm: {} frames, messages at {messages:?}, menu at {opened:?}", frames.len());
        self.fight_gif("battle/storm.gif", &trimmed(&frames), "the Fusion Dragon's storm")
    }

    /// Isaac summons Judgment at copies of Garet, Ivan and Mia through
    /// BattleEffect_RunSummon: the side left alone fades, and the battle
    /// wipes away to the summon's sky and back, recorded as `render
    /// psynergy` records a cast, with ten seconds of the game on each side
    /// of its black screens, as `battle_anim/summon.gif`.
    pub fn fight_summon(&mut self) -> Result<(), String> {
        let root = crate::compile::root();
        let plan = Tables::read(root)?.plan()?;
        let cast = plan.casts.into_iter().find(|c| c.ability.name == "Judgment").ok_or("no Judgment cast")?;
        let symbols = super::battle::Symbols::find(self.map)?;
        let recording = super::battle::record(self.rom, &symbols, &cast, &super::battle::Scene::Game, false)?;
        eprintln!("fight-summon: {} frames", recording.frames.len());
        self.fight_gif("battle_anim/summon.gif", &recording.frames, "a summon")
    }
}

/// struct Unit (battle/battle_unit.h): its size and the fields the ends of
/// an action, a turn and a battle read.
const UNIT_SIZE: u32 = 0x14c;
const UNIT_LEVEL: u32 = 0x0f;
const UNIT_BASE_HP: u32 = 0x10;
const UNIT_HP_GAUGE: u32 = 0x14;
const UNIT_MAX_HP: u32 = 0x34;
const UNIT_DJINN_SET: u32 = 0x108;
const UNIT_DJINN_SET_COUNTS: u32 = 0x11c;
const UNIT_EXPERIENCE: u32 = 0x124;
const UNIT_SIDE: u32 = 0x12a;
const UNIT_POISON: u32 = 0x131;
const UNIT_DELUSION: u32 = 0x138;
const UNIT_GRIM_REAPER: u32 = 0x141;
/// Unit.hpGauge full (UNIT_GAUGE_FULL), and BATTLE_SIDE_ENEMY.
const GAUGE_FULL: u16 = 0x4000;
const SIDE_ENEMY: u8 = 1;
/// The enemy units a battle has.
const ENEMY_UNITS: u16 = 6;
/// HP that no blow of these recordings takes, so the enemies outlast them.
const LASTING_HP: u16 = 999;
/// POISON_NORMAL, and a counter with one turn left.
const POISON_NORMAL: u8 = 1;
const LAST_TURN: u8 = 1;
/// enum BattleCommand, and the first enemy's place.
const COMMAND_SUMMON: u16 = 6;
/// gCharacter_LevelExpTable: 99 levels a character.
const LEVELS: u32 = 99;
/// The levels at which the Squire learns Psynergy (gClass_DefinitionTable).
const SQUIRE_LEARNS: [u8; 9] = [2, 4, 6, 10, 13, 14, 20, 26, 42];
/// Frames between presses of A while the spoils are handed out, so each
/// message can be read.
const SPOILS_A: usize = 60;

/// What a battle of `summon_turns` showed: every frame, the frames each
/// action ended (with its unit), each turn ended and each action was
/// dispatched, and the frame the menu came back after the last turn.
struct Ends {
    frames: Vec<Vec<u16>>,
    action_ends: Vec<(usize, u8)>,
    turn_ends: Vec<usize>,
    dispatches: Vec<usize>,
    over: usize,
}

impl<'a> Guide<'a> {
    /// A message's number, from the edition's ELF (messages are absolute
    /// symbols the linker map leaves out).
    fn message_number(&self, name: &str) -> Result<u32, String> {
        let path = crate::compile::root().join("out/tbs-en/tbs-en.elf");
        let data = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let elf = crate::elf::Elf::parse(&data)?;
        elf.symbols.iter().find(|s| s.name == name).map(|s| s.value).ok_or_else(|| format!("the ELF has no {name}"))
    }

    /// Isaac and Garet at level 35 against two Gryphons that cannot fall
    /// and do nothing, for `turns` turns: in the first Isaac summons Venus
    /// with a Venus Djinni put on standby before the battle (taken off his
    /// set as DjinnRecovery_Reconcile lists it) while Garet defends, and in
    /// each later turn both defend. `prepare` is called at the start of each
    /// turn, numbered from 1, before the commands are given.
    fn summon_turns(&self, turns: usize, prepare: &dyn Fn(&Self, &mut Gba, usize) -> Result<(), String>) -> Result<Ends, String> {
        let root = crate::compile::root();
        let plan = Tables::read(root)?.plan()?;
        let venus = plan.casts.into_iter().find(|c| c.ability.name == "Venus" && matches!(c.command, plan::Command::Summon { .. })).ok_or("no Venus summon")?;
        let plan::Command::Summon { row, element, count } = venus.command else {
            return Err("Venus is no summon".into());
        };
        let reconcile = self.sym("DjinnRecovery_Reconcile")?;
        let commands = self.sym("Battle_CollectPartyCommands")?;
        let opponents = self.sym("Battle_BuildOpponentActions")?;
        let action_end = self.sym("BattleUnit_ProcessActionEnd")?;
        let turn_end = self.sym("Battle_ProcessTurnEnd")?;
        let dispatch = self.sym("BattleStaging_DispatchAction")?;
        let random = self.sym("Battle_Random")?;
        let drive = self.party_drive(&[plan::ISAAC, plan::GARET], GRYPHONS, BATTLE_START_SET, vec![reconcile, commands, opponents, action_end, turn_end, dispatch, random])?;
        let mut ends = Ends { frames: Vec::new(), action_ends: Vec::new(), turn_ends: Vec::new(), dispatches: Vec::new(), over: 0 };
        let mut turn = 0;
        let mut over: Option<usize> = None;
        let mut failed: Option<String> = None;
        let mut on_standby = 0;
        self.drive(&drive, &mut |gba, hook, f| {
            match hook {
                Some(h) if h == random => {
                    self.quiet_weapon(gba);
                    return Act::Mash;
                }
                Some(h) if h == reconcile => {
                    let Ok(isaac) = self.unit_address(gba, u16::from(plan::ISAAC)) else { return Act::Mash };
                    let set = isaac + UNIT_DJINN_SET + 4 * u32::from(element);
                    for index in 0..32 {
                        let bits = gba.read32(set);
                        if on_standby < count && bits & 1 << index != 0 {
                            gba.write32(set, bits & !(1 << index));
                            let counted = isaac + UNIT_DJINN_SET_COUNTS + u32::from(element);
                            gba.write8(counted, gba.read8(counted).saturating_sub(1));
                            on_standby += 1;
                        }
                    }
                    return Act::Mash;
                }
                Some(h) if h == commands => {
                    turn += 1;
                    if turn > turns {
                        // The menu opens and waits.
                        over.get_or_insert(f);
                        return Act::Keys(0);
                    }
                    let result = (|| {
                        if turn == 1 {
                            for i in 0..ENEMY_UNITS {
                                let enemy = self.unit_address(gba, FIRST_ENEMY_UNIT + i)?;
                                if gba.read8(enemy + UNIT_SIDE) == SIDE_ENEMY {
                                    for field in [UNIT_BASE_HP, UNIT_MAX_HP, UNIT_HP] {
                                        gba.write16(enemy + field, LASTING_HP);
                                    }
                                    gba.write16(enemy + UNIT_HP_GAUGE, GAUGE_FULL);
                                }
                            }
                        }
                        prepare(self, gba, turn)?;
                        let isaac = if turn == 1 {
                            (u16::from(plan::ISAAC), COMMAND_SUMMON, u16::from(row), PLACE_ENEMY, u16::from(venus.ability.range))
                        } else {
                            (u16::from(plan::ISAAC), COMMAND_DEFEND, 0, PLACE_PARTY, 1)
                        };
                        self.give_commands(gba, &[isaac, (u16::from(plan::GARET), COMMAND_DEFEND, 0, PLACE_PARTY + 1, 1)])
                    })();
                    if let Err(e) = result {
                        failed = Some(e);
                    }
                    return Act::Mash;
                }
                Some(h) if h == opponents => {
                    gba.return_now(0);
                    return Act::Mash;
                }
                Some(h) if h == action_end => {
                    ends.action_ends.push((f, gba.read8(gba.register(0))));
                    return Act::Mash;
                }
                Some(h) if h == turn_end => {
                    ends.turn_ends.push(f);
                    return Act::Mash;
                }
                Some(_) => {
                    ends.dispatches.push(f);
                    return Act::Mash;
                }
                None => {}
            }
            ends.frames.push(gba.picture());
            if over.is_some_and(|o| f > o + STILL_KEPT) || failed.is_some() {
                return Act::Stop;
            }
            // The menu after the last turn is left alone; while the actions
            // play, A only every SLOW_A frames, so each message can be read.
            if over.is_some() {
                Act::Keys(0)
            } else if ends.dispatches.is_empty() {
                Act::Mash
            } else {
                Act::Keys(if f % SLOW_A < 4 { KEY_A } else { 0 })
            }
        })?;
        if let Some(e) = failed {
            return Err(e);
        }
        if on_standby < count {
            return Err(format!("Isaac had {on_standby} of the {count} Venus Djinn the summon needs set"));
        }
        ends.over = over.ok_or("the battle never came back to the menu")?;
        eprintln!("summon turns: {} frames, actions ended at {:?}, turns ended at {:?}, dispatches at {:?}, menu back at {}", ends.frames.len(), ends.action_ends, ends.turn_ends, ends.dispatches, ends.over);
        Ok(ends)
    }

    /// Isaac, poisoned and with the Grim Reaper's countdown at its last
    /// turn, summons Venus: as his action ends `Isaac's Earth Power rises
    /// by <n>!` over a pillar of earth, the poison bites, and the Grim Reaper
    /// calls him, as `battle/action-end.gif`.
    pub fn fight_action_end(&mut self) -> Result<(), String> {
        let ends = self.summon_turns(1, &|guide, gba, turn| {
            if turn == 1 {
                let isaac = guide.unit_address(gba, u16::from(plan::ISAAC))?;
                gba.write8(isaac + UNIT_POISON, POISON_NORMAL);
                gba.write8(isaac + UNIT_GRIM_REAPER, LAST_TURN);
            }
            Ok(())
        })?;
        let (end, _) = *ends.action_ends.iter().find(|&&(_, unit)| unit == plan::ISAAC).ok_or("Isaac's action never ended")?;
        let next = ends.dispatches.iter().chain(&ends.turn_ends).copied().filter(|&d| d > end).min().unwrap_or(ends.frames.len());
        let from = end.saturating_sub(ACTION_END_BEFORE);
        let to = (next + 10).min(ends.frames.len());
        self.fight_gif("battle/action-end.gif", &ends.frames[from..to], "an action's end")
    }

    /// Isaac summons Venus and both defend for a turn; in the next Isaac,
    /// deluded with one turn left, defends again: as that turn ends the
    /// Venus Djinni is set to him again and he sees clearly, as
    /// `battle/turn-end.gif`.
    pub fn fight_turn_end(&mut self) -> Result<(), String> {
        let ends = self.summon_turns(2, &|guide, gba, turn| {
            if turn == 2 {
                let isaac = guide.unit_address(gba, u16::from(plan::ISAAC))?;
                gba.write8(isaac + UNIT_DELUSION, LAST_TURN);
            }
            Ok(())
        })?;
        let end = *ends.turn_ends.get(1).ok_or("the second turn never ended")?;
        let from = end.saturating_sub(TURN_END_BEFORE);
        let to = (ends.over + MENU_KEPT).min(ends.frames.len());
        self.fight_gif("battle/turn-end.gif", &ends.frames[from..to], "a turn's end")
    }
}

/// Frames kept before an action's end and a turn's end.
const ACTION_END_BEFORE: usize = 90;
const TURN_END_BEFORE: usize = 60;
/// Frames kept of the command menu that opens after a turn.
const MENU_KEPT: usize = 30;

impl<'a> Guide<'a> {
    /// Goma Cave's random battle with Isaac's experience one point short of
    /// the next level at which his class learns Psynergy: the battle played
    /// to its end with A pressed, kept from just before the experience is
    /// handed out to a moment after the coins, as `game/level-up.gif`.
    pub fn fight_level_up(&mut self) -> Result<(), String> {
        let gained = self.message_number("MsgExpGained")?;
        let coins = self.message_number("MsgCoinsGained")?;
        let table = self.sym("gCharacter_LevelExpTable")?;
        let (place, stretch) = self.goma_cave_stretch()?;
        let mut entered = self.enter_goma_cave(place)?;
        let field = &mut entered.field;
        let isaac = field.unit(plan::ISAAC)?;
        let level = field.read8(isaac + UNIT_LEVEL);
        let next = *SQUIRE_LEARNS.iter().find(|&&l| l > level).ok_or("Isaac learns nothing more")?;
        let at = (table - 0x0800_0000 + 4 * (u32::from(plan::ISAAC) * LEVELS + u32::from(next) - 1)) as usize;
        let needed = u32::from_le_bytes(self.rom[at..at + 4].try_into().map_err(|_| "the table is past the ROM")?);
        eprintln!("fight-level-up: Isaac level {level}, raised to {} with {} experience", next - 1, needed - 1);
        field.write8(isaac + UNIT_LEVEL, next - 1);
        field.write32(isaac + UNIT_EXPERIENCE, needed - 1);
        let mut frames = Vec::new();
        Self::walk_stretch(field, &stretch, 80, &mut frames, &mut 0, &mut |_| {})?.ok_or("no battle came")?;
        frames.clear();
        let mut first: Option<usize> = None;
        let mut last: Option<usize> = None;
        for f in 0..20000usize {
            let keys = match first {
                None => (f / 4) % 2 == 1,
                Some(_) => f % SPOILS_A < 4,
            };
            field.hold(if keys { KEY_A } else { 0 });
            field.frames(1)?;
            frames.push(field.picture());
            if first.is_none() && field.last_message() == gained {
                first = Some(frames.len());
            }
            if last.is_none() && field.last_message() == coins {
                last = Some(frames.len());
            }
            if last.is_some_and(|l| frames.len() > l + SPOILS_A) {
                break;
            }
        }
        field.hold(0);
        let (first, last) = (first.ok_or("no experience was handed out")?, last.ok_or("no coins were handed out")?);
        eprintln!("fight-level-up: experience at {first}, coins at {last}");
        self.fight_gif("game/level-up.gif", &frames[first.saturating_sub(SPOILS_BEFORE)..], "the spoils with a level gained")
    }

    /// Goma Cave's random battle with the party at 1 HP and the enemies
    /// lasting: played with A pressed until `Isaac's party has been
    /// defeated!`, then left alone through the fade to where the party
    /// wakes, kept from CLEARED frames before the first black screen to
    /// CLEARED after the last, as `battle/lost.gif`.
    pub fn fight_lost(&mut self) -> Result<(), String> {
        let defeated = self.message_number("MsgPartyDefeated")?;
        let enemies = self.sym("gEnemyUnits")?;
        let (place, stretch) = self.goma_cave_stretch()?;
        let mut entered = self.enter_goma_cave(place)?;
        let field = &mut entered.field;
        // The restart point the story leaves on the way to Goma Cave: Vault
        // sets it to its Sanctum as the party enters (gScene_RestartRules).
        let work = self.sym("gMainRoutineWork")?;
        field.write16(work + STATE_RESTART_SCENE, super::source::map_id("Sanctum")?);
        field.write16(work + STATE_RESTART_ENTRANCE, VAULT_SANCTUM_ENTRANCE);
        for owner in [plan::ISAAC, plan::GARET, plan::IVAN, plan::MIA] {
            let unit = field.unit(owner)?;
            let max = u32::from(field.read16(unit + UNIT_MAX_HP)).max(1);
            field.write16(unit + UNIT_HP, 1);
            // Unit_Calculate keeps the share of HP the gauge holds.
            field.write16(unit + UNIT_HP_GAUGE, (u32::from(GAUGE_FULL) / max).max(1) as u16);
        }
        let mut frames = Vec::new();
        Self::walk_stretch(field, &stretch, 80, &mut frames, &mut 0, &mut |_| {})?.ok_or("no battle came")?;
        let battle = field.scene();
        frames.clear();
        let mut lasting = [false; ENEMY_UNITS as usize];
        let mut lost: Option<usize> = None;
        let mut back: Option<usize> = None;
        for f in 0..30000usize {
            // The enemies last once the battle has set them up.
            if f > ENEMIES_SET && lost.is_none() {
                let base = field.read32(enemies);
                for (i, done) in lasting.iter_mut().enumerate() {
                    let unit = base + i as u32 * UNIT_SIZE;
                    if !*done && field.read8(unit + UNIT_SIDE) == SIDE_ENEMY && field.read16(unit + UNIT_HP) != 0 {
                        for at in [UNIT_BASE_HP, UNIT_MAX_HP, UNIT_HP] {
                            field.write16(unit + at, LASTING_HP);
                        }
                        field.write16(unit + UNIT_HP_GAUGE, GAUGE_FULL);
                        *done = true;
                    }
                }
            }
            // A until the field is back, the defeat message left a moment
            // first so it can be read.
            let reading = lost.is_some_and(|l| frames.len() < l + DEFEAT_SHOWN);
            field.hold(if back.is_none() && !reading && (f / 4) % 2 == 1 { KEY_A } else { 0 });
            field.frames(1)?;
            frames.push(field.picture());
            if lost.is_none() && field.last_message() == defeated {
                lost = Some(frames.len());
            }
            if back.is_none() && lost.is_some() && field.scene() != battle {
                back = Some(frames.len());
            }
            if let (Some(l), Some(b)) = (lost, back) {
                // CLEARED frames past the last black screen since the
                // defeat, once the field has been back long enough to fade in.
                let since = frames[l..].iter().rposition(|p| is_black_bgr555(p)).map(|k| frames.len() - 1 - (k + l));
                if since.is_some_and(|s| s >= CLEARED + 10) || frames.len() > b + FIELD_KEPT {
                    break;
                }
            }
        }
        field.hold(0);
        let lost = lost.ok_or("the party was never defeated")?;
        let back = back.ok_or("the party never left the battle")?;
        let black = frames[lost..].iter().position(|p| is_black_bgr555(p)).map(|b| b + lost).ok_or("the battle never faded")?;
        let from = black.saturating_sub(CLEARED + 10);
        eprintln!("fight-lost: {} frames, defeated at {lost}, first black at {black}, scene {} at {back}", frames.len(), field.scene());
        self.fight_gif("battle/lost.gif", &frames[from..], "a battle lost and where the party wakes")
    }
}

/// Frames kept before the experience message, which show the battle's
/// last moments and the win.
const SPOILS_BEFORE: usize = 150;
/// Frames into a battle by which its enemy units are set up.
const ENEMIES_SET: usize = 60;
/// Frames a defeat is followed past the battle at most.
const FIELD_KEPT: usize = 3000;
/// Frames the defeat message is left on screen before A.
const DEFEAT_SHOWN: usize = 150;
/// struct MainRoutineWork: the restart point (main_routine_work.h).
const STATE_RESTART_SCENE: u32 = 0x1c4;
const STATE_RESTART_ENTRANCE: u32 = 0x1c6;
/// The Sanctum's entrance Vault's restart rules name.
const VAULT_SANCTUM_ENTRANCE: u16 = 12;

/// The modes this file plays.
pub const MODES: &[&str] = &[
    "fight-opening",
    "fight-commands",
    "fight-aim",
    "fight-round",
    "fight-turn-order",
    "fight-first-strike",
    "fight-flee",
    "fight-gryphons",
    "fight-twin-beaks",
    "fight-unleash",
    "fight-rolls",
    "fight-wake",
    "fight-won",
    "fight-bone-charge",
    "fight-ally-search",
    "fight-storm",
    "fight-critical",
    "fight-item",
    "fight-enemy-flees",
    "fight-summon",
    "fight-action-end",
    "fight-turn-end",
    "fight-level-up",
    "fight-lost",
];

impl Guide<'_> {
    /// Plays one of this file's modes; None when `name` is not one.
    pub fn fight(&mut self, name: &str) -> Option<Result<(), String>> {
        Some(match name {
            "fight-opening" => self.fight_opening(),
            "fight-commands" => self.fight_commands(),
            "fight-aim" => self.fight_aim(),
            "fight-round" => self.fight_round(),
            "fight-turn-order" => self.fight_turn_order(),
            "fight-first-strike" => self.fight_first_strike(),
            "fight-flee" => self.fight_flee(),
            "fight-gryphons" => self.fight_gryphons(),
            "fight-twin-beaks" => self.fight_twin_beaks(),
            "fight-unleash" => self.fight_unleash(),
            "fight-rolls" => self.fight_rolls(),
            "fight-wake" => self.fight_wake(),
            "fight-won" => self.fight_won(),
            "fight-bone-charge" => self.fight_bone_charge(),
            "fight-ally-search" => self.fight_ally_search(),
            "fight-storm" => self.fight_storm(),
            "fight-critical" => self.fight_critical(),
            "fight-item" => self.fight_item(),
            "fight-enemy-flees" => self.fight_enemy_flees(),
            "fight-summon" => self.fight_summon(),
            "fight-action-end" => self.fight_action_end(),
            "fight-turn-end" => self.fight_turn_end(),
            "fight-level-up" => self.fight_level_up(),
            "fight-lost" => self.fight_lost(),
            _ => return None,
        })
    }
}
