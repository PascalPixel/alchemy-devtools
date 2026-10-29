//! Recordings for the battle pages, each the game alone as the player sees
//! it: a battle's opening, the command menu, aiming, a round, a first
//! strike, running away, the Gryphons' choices and Twin Beaks, a weapon's
//! unleash, a miss and a critical hit, a sleeper woken, and a battle won
//! and the field coming back. Each plays a battle from Camelot's battle test
//! menu (guide_battle.rs's drive, or `render psynergy`'s record_watching)
//! or a random encounter in Goma Cave; a roll is forced only by answering
//! the random call that makes it, and weapons never unleash unless a
//! recording is about the unleash, so an Attack reads as one.

use super::guide::Guide;
use super::guide_battle::{Act, Drive};
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
/// The Skeleton's first five actions are Attack, rolls 0 to 159.
const ROLL_SKELETON_ATTACK: u32 = 10;
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

    /// One Skeleton from the battle's first picture to the end of its
    /// Attack, the party defending, as `graphics/monster.gif`: its
    /// pictures in `monster/skeleton.png` stand it at six angles and strike
    /// at six, and the battle shows the one for the camera's angle as the
    /// camera swings in and round, and a striking one as it attacks.
    pub fn graphics_monster(&mut self) -> Result<(), String> {
        let (frames, turns) = self.enemy_round(ONE_SKELETON, &[ROLL_SKELETON_ATTACK], false)?;
        turns.first().ok_or("the Skeleton never acted")?;
        self.fight_gif("graphics/monster.gif", &trimmed(&frames), "a Skeleton's pictures as the camera swings and it attacks")
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
    "graphics-monster",
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
            "graphics-monster" => self.graphics_monster(),
            _ => return None,
        })
    }
}
