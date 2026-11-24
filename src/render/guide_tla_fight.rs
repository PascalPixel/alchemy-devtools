//! The Lost Age's battle recordings, the mirrors of The Broken Seal's
//! (guide_fight.rs): a battle's opening and a round played from Camelot's
//! battle test menu, a monster's pictures as the camera swings, a weapon's
//! unleash, and a random battle in the Kandorean Temple's caves won, pulled
//! apart, and left for the cave again.
//!
//! The battle test menu is The Lost Age's own (DebugBattle_RunMenu): it
//! reads Down as it opens, waits for A, then loads the debug parameter set
//! A picked (DebugParameterSet_Load) and runs the formation
//! (Battle_Run) over the Colosso's backdrop. The run answers the
//! set and the formation as the calls are made, and shows the background of
//! the scene the formation is met in, as the field would choose it.

use super::guide::Guide;
use super::guide_battle::{mash, Act};
use super::guide_tla::{KEY_A, KEY_DOWN};
use super::mgba::{Gba, LR};
use super::plan;

/// gBattleFormation_Records rows (formations.c): a Wild Wolf, a Punch
/// Ant and a Giant Bat, met in the Kandorean Temple; and one Wild Wolf.
const WOLF_ANT_BAT: u32 = 248;
const ONE_WILD_WOLF: u32 = 129;
/// Debug parameter set 7: Felix, Jenna and Sheba at level 7, as they reach
/// the Kandorean Temple.
const PARTY_TEMPLE: u8 = 7;
/// gMainRoutineState.battleType: 1, which no battle start names, opens the
/// battle with no first strike or surprise.
const BATTLE_START_SET: u8 = 1;
/// The first enemy's unit id, and struct BattleAction's unit.
const FIRST_ENEMY_UNIT: u16 = 128;
const RECORD_UNIT: u32 = 0x00;
/// enum BattleCommand: Defend; the party's places.
const COMMAND_DEFEND: u16 = 3;
const PLACE_PARTY: u16 = 0x100;
/// Frames between presses of A while enemies act.
const SLOW_A: usize = 40;
/// Frames the chooser holds Down as it opens.
const CHOOSER_DOWN_FRAMES: u32 = 8;
/// A run that is not over by this frame has gone wrong.
const MAX_FRAMES: u32 = 20000;

/// A battle from the test menu: the debug set and party, the formation,
/// the battle's start, the scene whose background it shows, and the
/// function entries the watcher is called at.
struct Drive {
    preset: u8,
    party: Vec<u8>,
    formation: u32,
    battle_start: u8,
    background: u16,
    hooks: Vec<u32>,
}

/// The modes this file plays.
pub const MODES: &[&str] = &["fight-opening", "fight-turn-order", "graphics-monster", "fight-unleash"];

impl<'a> Guide<'a> {
    pub(super) fn run_tla_fight(&mut self, name: &str) -> Result<(), String> {
        match name {
            "fight-opening" => self.tla_fight_opening(),
            "fight-turn-order" => self.tla_turn_order(),
            "graphics-monster" => self.tla_monster(),
            "fight-unleash" => self.tla_unleash(),
            _ => Err(format!("no mode {name}")),
        }
    }

    /// The background a battle in the Kandorean Temple's first room shows:
    /// the resource BattleBackground_SelectForMap chooses for the scene, the
    /// last marked row of gScene_BattleBackgroundRules before the scene's.
    fn temple_background(&self) -> Result<u16, String> {
        let scene = Self::tla_scene("KandoreanTempleLifts")?;
        let table = self.sym("gScene_BattleBackgroundRules")?;
        let word = |i: u32| {
            let at = (table + 4 * i - 0x0800_0000) as usize;
            u32::from_le_bytes([self.rom[at], self.rom[at + 1], self.rom[at + 2], self.rom[at + 3]])
        };
        let (mut i, mut chosen) = (0, None);
        loop {
            let entry = word(i);
            if entry == 0 || entry == u32::from(scene) {
                break;
            }
            if entry & 0x8000_0000 != 0 {
                chosen = Some((entry & 0xffff) as u16);
            }
            i += 1;
        }
        chosen.ok_or_else(|| "no background is marked before the Kandorean Temple".to_string())
    }

    /// Felix, Jenna and Sheba from debug set 7 against `formation` in the
    /// temple.
    fn temple_drive(&self, formation: u32, hooks: Vec<u32>) -> Result<Drive, String> {
        Ok(Drive { preset: PARTY_TEMPLE, party: vec![plan::FELIX, plan::JENNA, plan::SHEBA], formation, battle_start: BATTLE_START_SET, background: self.temple_background()?, hooks })
    }

    /// Boots the ROM into The Lost Age's battle test menu, starts `drive`'s
    /// battle and plays it, as Guide::drive does The Broken Seal's: `watch`
    /// is called at each of the hooks with the address and after each
    /// battle frame with None, and its answer decides the keys.
    fn tla_drive(&self, drive: &Drive, watch: &mut dyn FnMut(&mut Gba, Option<u32>, usize) -> Act) -> Result<(), String> {
        let b = super::layout::BattleLayout::for_edition("tla-en")?;
        let start = self.sym("MainRoutine_RunFar")?;
        let menu = self.sym("DebugBattle_RunMenu")?;
        let preset = self.sym("DebugParameterSet_Load")?;
        let encounter = self.sym("Battle_Run")?;
        let game_state = self.sym("gMainRoutineState")?;
        let game_flags = self.sym("gGameFlagBytes")?;
        let mut gba = Gba::new(self.rom)?;
        // 0: booting, 1: the chooser, 2: the battle.
        let (mut phase, mut frame, mut chooser_from, mut battle_frames) = (0, 0, 0, 0usize);
        loop {
            let step = gba.step();
            let pc = step.pc;
            if phase == 2 && drive.hooks.contains(&pc) {
                watch(&mut gba, Some(pc), battle_frames);
            }
            let frame_done = step.frame != frame;
            frame = step.frame;
            if phase == 0 && pc == start {
                gba.branch(menu);
                phase = 1;
                chooser_from = frame;
            } else if phase == 1 && pc == preset {
                gba.set_register(0, drive.preset.into());
            } else if phase == 1 && pc == encounter {
                gba.set_register(0, drive.formation);
                let flags = drive.party.iter().fold(0u8, |f, &o| f | 1 << o);
                gba.write8(game_flags, flags);
                for (i, &o) in drive.party.iter().enumerate() {
                    gba.write8(game_state + b.state_active_owners + i as u32, o);
                }
                gba.write8(game_state + b.state_battle_type, drive.battle_start);
                gba.write16(game_state + b.state_battle_background, drive.background);
                phase = 2;
            }
            if !frame_done {
                continue;
            }
            let keys = match phase {
                2 => {
                    let act = watch(&mut gba, None, battle_frames);
                    battle_frames += 1;
                    match act {
                        Act::Mash => mash(frame),
                        Act::Keys(k) => k,
                        Act::Stop => break,
                    }
                }
                // Down from power-on and as the chooser opens, then A to
                // pick.
                0 => KEY_DOWN,
                _ if frame <= chooser_from + CHOOSER_DOWN_FRAMES => KEY_DOWN,
                _ => mash(frame),
            };
            gba.set_keys(keys);
            if frame > MAX_FRAMES {
                return Err(format!("gave up at frame {frame}"));
            }
        }
        Ok(())
    }

    /// A battle against a Wild Wolf, a Punch Ant and a Giant Bat from its
    /// first frame: the enemies are announced as the camera swings in, and
    /// it settles behind the party as the first menu opens, as
    /// `battle/opening.gif`.
    fn tla_fight_opening(&mut self) -> Result<(), String> {
        let menu = self.sym("Battle_CollectPartyCommands")?;
        let message = self.sym("BattleMessage_ShowAndWait")?;
        let drive = self.temple_drive(WOLF_ANT_BAT, vec![menu, message])?;
        let mut frames = Vec::new();
        let mut opened: Option<usize> = None;
        let mut messages = Vec::new();
        self.tla_drive(&drive, &mut |gba, hook, f| {
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
        self.fight_gif("battle/opening.gif", &super::guide_fight::trimmed(&frames), "a battle opening in the Kandorean Temple")
    }

    /// One round from the menu's first frame against the Wild Wolf, the
    /// Punch Ant and the Giant Bat, every command Attack (A pressed
    /// throughout), until the next round's menu, as `battle/turn-order.gif`.
    fn tla_turn_order(&mut self) -> Result<(), String> {
        let menu = self.sym("Battle_CollectPartyCommands")?;
        let dispatch = self.sym("BattleStaging_DispatchAction")?;
        let drive = self.temple_drive(WOLF_ANT_BAT, vec![menu, dispatch])?;
        let mut frames = Vec::new();
        let mut menus: Vec<usize> = Vec::new();
        let mut acts: Vec<(usize, u16)> = Vec::new();
        self.tla_drive(&drive, &mut |gba, hook, f| {
            match hook {
                Some(h) if h == menu => {
                    menus.push(f);
                    return Act::Mash;
                }
                Some(_) => {
                    acts.push((f, gba.read16(gba.register(0) + RECORD_UNIT)));
                    return Act::Mash;
                }
                None => {}
            }
            frames.push(gba.picture());
            match menus.get(1) {
                Some(&m) if f > m + 30 => Act::Stop,
                Some(_) => Act::Keys(0),
                // Once the enemies act, A only every SLOW_A frames, so each
                // message can be read before it goes.
                None if acts.iter().any(|a| a.1 >= FIRST_ENEMY_UNIT) => Act::Keys(if f % SLOW_A < 4 { KEY_A } else { 0 }),
                None => Act::Mash,
            }
        })?;
        eprintln!("fight-turn-order: menus at {menus:?}, actions {acts:?}");
        let start = menus.first().copied().ok_or("no menu")?;
        self.fight_gif("battle/turn-order.gif", &frames[start..], "a round against a Wild Wolf, a Punch Ant and a Giant Bat")
    }

    /// One Wild Wolf from the battle's first picture to the end of its
    /// first action, the party defending, as `graphics/monster.gif`: the
    /// battle shows the wolf's picture for the camera's angle as the camera
    /// swings in and round, and a striking one as it attacks.
    fn tla_monster(&mut self) -> Result<(), String> {
        let commands = self.sym("Battle_CollectPartyCommands")?;
        let dispatch = self.sym("BattleStaging_DispatchAction")?;
        let drive = self.temple_drive(ONE_WILD_WOLF, vec![commands, dispatch])?;
        let b = super::layout::BattleLayout::for_edition("tla-en")?;
        let owners = self.sym("gCharacterUnits")?;
        let mut frames = Vec::new();
        let (mut starts, mut turns, mut over) = (0, Vec::new(), None::<usize>);
        let party = drive.party.clone();
        self.tla_drive(&drive, &mut |gba, hook, f| {
            match hook {
                Some(h) if h == commands => {
                    starts += 1;
                    if starts == 1 {
                        // Each member defends: the command records filled
                        // in, as the menu would, and their count returned.
                        let record = gba.register(0);
                        for (i, &o) in party.iter().enumerate() {
                            let agility = gba.read16(owners + u32::from(o) * b.unit_size + b.unit_agility);
                            let fields = [u16::from(o), 0, agility, COMMAND_DEFEND, 0, PLACE_PARTY + i as u16, 1, 0];
                            for (k, v) in fields.into_iter().enumerate() {
                                gba.write16(record + 16 * i as u32 + 2 * k as u32, v);
                            }
                        }
                        gba.return_now(party.len() as u32);
                    }
                    return Act::Keys(0);
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
                let end = *over.get_or_insert(f + 30);
                return if f >= end { Act::Stop } else { Act::Keys(0) };
            }
            if turns.is_empty() {
                Act::Mash
            } else {
                Act::Keys(if f % SLOW_A < 4 { KEY_A } else { 0 })
            }
        })?;
        eprintln!("graphics-monster: {} frames, the wolf acted at {turns:?}", frames.len());
        turns.first().ok_or("the Wild Wolf never acted")?;
        self.fight_gif("graphics/monster.gif", &super::guide_fight::trimmed(&frames), "a Wild Wolf's pictures as the camera swings and it attacks")
    }

    /// Felix attacks a copy of Sheba with the Gaia Blade of his debug set,
    /// its unleash roll answered 0: `Felix's Gaia Blade lets out a howl!`,
    /// and the unleash strikes, as `battle/unleash.gif`.
    fn tla_unleash(&mut self) -> Result<(), String> {
        let root = crate::compile::root();
        let plan = super::plan::Tables::read_edition(root, "tla-en")?.plan()?;
        let mut cast = plan.casts.into_iter().find(|c| c.caster() == plan::FELIX && c.ability.name == "Quake").ok_or("no Quake cast of Felix's")?;
        cast.foes = vec![plan::SHEBA];
        let symbols = super::battle::Symbols::find_for(self.map, super::layout::BattleLayout::for_edition("tla-en")?)?;
        let random = self.sym("Battle_Random")?;
        let mut unleashed = 0;
        let recording = super::battle::record_watching(self.rom, &symbols, &cast, &super::battle::Scene::Game, false, Some(&super::guide_battle::ATTACK_MENU), &[random], &mut |gba, hook| {
            if hook == Some(random) && self.function_at(gba.register(LR) & !1) == "RollWeaponUnleash" {
                gba.return_now(0);
                unleashed += 1;
            }
        })?;
        eprintln!("fight-unleash: {} frames, unleash rolls answered {unleashed}", recording.frames.len());
        if unleashed == 0 {
            return Err("no unleash roll was made".into());
        }
        self.fight_gif("battle/unleash.gif", &super::guide_fight::trimmed(&recording.frames), "a weapon's unleash")
    }
}

/// The Kandorean Temple's rooms walked for a random battle, gScenes' rows
/// (SceneId_KandoreanTemple2 on), as the walkthrough has the party meet
/// its Wild Wolves, Punch Ants and Giant Bats there.
const TEMPLE_ROOMS: &[&str] = &["KandoreanTempleMimic", "KandoreanTempleChestbeaters", "KandoreanTemplePillar", "KandoreanTempleStream", "KandoreanTempleSteam", "KandoreanTempleFog"];

/// The modes of the Kandorean Temple's random battles.
pub const CAVE_MODES: &[&str] = &["fight-won", "overworld-battle-return", "battle-layers-3d"];

impl<'a> Guide<'a> {
    pub(super) fn run_tla_cave(&mut self, name: &str) -> Result<(), String> {
        match name {
            "fight-won" => self.tla_fight_won(),
            "overworld-battle-return" => self.tla_battle_return(),
            "battle-layers-3d" => self.tla_battle_exploded(),
            _ => Err(format!("no mode {name}")),
        }
    }

    /// The longest stretch of open ground the leader can walk to and fro in
    /// the temple's rooms, by any of their entrances, with little dark
    /// around him: the place and the stretch (Guide::goma_cave_stretch for
    /// The Lost Age).
    fn temple_stretch(&self) -> Result<((&'static str, u16), super::guide_more::Stretch), String> {
        let mut best: Option<(i32, (&'static str, u16), super::guide_more::Stretch)> = None;
        for &room in TEMPLE_ROOMS {
            let Ok(first) = self.tla_enter(room, 0, super::guide_tla::PARTY_DAILA, &[]) else {
                continue;
            };
            for entrance in first.field.entrances().iter().map(|e| e.number as u16) {
                for across in [true, false] {
                    let Ok(mut entered) = self.tla_enter(room, entrance, super::guide_tla::PARTY_DAILA, &[]) else {
                        continue;
                    };
                    let Ok(stretch) = Self::measure_stretch(&mut entered.field, across) else {
                        continue;
                    };
                    let length = stretch.ends[1] - stretch.ends[0];
                    if length < 64 || best.as_ref().is_some_and(|b| b.0 >= length) {
                        continue;
                    }
                    let mut dark = 0;
                    if Self::walk_stretch(&mut entered.field, &stretch, 2, &mut Vec::new(), &mut dark, &mut |_| {}).is_err() || dark > super::guide_more::DARK_LIMIT {
                        continue;
                    }
                    best = Some((length, (room, entrance), stretch));
                }
            }
        }
        let (_, place, stretch) = best.ok_or("no room of the Kandorean Temple has open ground to walk")?;
        eprintln!("temple stretch: {place:?}, {} pixels", stretch.ends[1] - stretch.ends[0]);
        Ok((place, stretch))
    }

    /// Walks the temple's stretch until a random battle starts: the field
    /// and the frames walked, the last the battle's first.
    fn temple_battle(&self) -> Result<(super::guide::Entered<'a>, super::guide_more::Stretch, Vec<Vec<u16>>, usize, u16), String> {
        let ((room, entrance), stretch) = self.temple_stretch()?;
        let cave = Self::tla_scene(room)?;
        let mut entered = self.tla_enter(room, entrance, super::guide_tla::PARTY_DAILA, &[])?;
        // Stood a while first, so a battle that comes soon still has the
        // cave before it.
        let mut walk = Vec::new();
        Self::hold(&mut entered.field, 0, super::field::CLEARED as u32, &mut walk)?;
        let at = Self::walk_stretch(&mut entered.field, &stretch, 80, &mut walk, &mut 0, &mut |_| {})?.ok_or("no battle came in the temple")?;
        Ok((entered, stretch, walk, at, cave))
    }

    /// A random battle in the temple fought to its end, A pressed every
    /// SLOW_A frames so each message can be read, and the cave again until
    /// the leader has walked a little: the frames of the walk before it
    /// (from CLEARED frames before the battle's first black screen), of the
    /// battle, and after, and where the battle starts and the cave comes
    /// back in them.
    fn temple_fight(&self) -> Result<(Vec<Vec<u16>>, usize, usize), String> {
        let (mut entered, stretch, walk, at, cave) = self.temple_battle()?;
        let field = &mut entered.field;
        let mut frames: Vec<Vec<u16>> = walk[at.saturating_sub(super::field::CLEARED + 60)..].to_vec();
        let opens = frames.len();
        let mut back = None;
        for n in 0..20000usize {
            field.hold(if n % SLOW_A < 4 { KEY_A } else { 0 });
            field.frames(1)?;
            frames.push(field.picture());
            if field.scene() == cave {
                back = Some(frames.len());
                break;
            }
        }
        field.hold(0);
        let back = back.ok_or("the battle never ended")?;
        eprintln!("temple fight: the battle took {} frames", back - opens);
        // The cave until the player walks again, then a few steps.
        let steps = field.steps();
        for _ in 0..600 {
            field.frames(1)?;
            frames.push(field.picture());
            if field.steps() > steps + 30 {
                break;
            }
        }
        Self::hold(field, stretch.keys[0], 40, &mut frames)?;
        Self::run_past_black(field, &mut frames)?;
        Ok((frames, opens, back))
    }

    /// A random battle in the temple won and the cave back: from CLEARED
    /// frames before the battle's end, as `battle/won.gif`.
    fn tla_fight_won(&mut self) -> Result<(), String> {
        let (frames, opens, back) = self.temple_fight()?;
        // The last black screen of the battle: its end, which fades out.
        let last_black = frames[opens..back].iter().rposition(|f| super::animation::is_black_bgr555(f)).map_or(back, |b| opens + b);
        let first_black = frames[opens..last_black].iter().rposition(|f| !super::animation::is_black_bgr555(f)).map_or(opens, |b| opens + b);
        let from = first_black.saturating_sub(super::field::CLEARED + 30).max(opens);
        self.fight_gif("battle/won.gif", &frames[from..], "a battle won in the Kandorean Temple and the cave back")
    }

    /// A random battle in the temple walked into, fought to its end, and
    /// the cave loaded again with the party where it stood, as
    /// `overworld/battle-return.gif`.
    fn tla_battle_return(&mut self) -> Result<(), String> {
        let (frames, _, _) = self.temple_fight()?;
        self.world_gif("overworld/battle-return.gif", &frames, "a Kandorean Temple battle walked into, won, and the cave again")
    }

    /// A random battle in the temple pulled apart once it has settled, as
    /// `battle/layers-3d.gif`.
    fn tla_battle_exploded(&mut self) -> Result<(), String> {
        let (mut entered, _, _, _, _) = self.temple_battle()?;
        let field = &mut entered.field;
        field.frames(300)?;
        self.explode_now(field, "battle/layers-3d.gif", "a Kandorean Temple battle")
    }
}
