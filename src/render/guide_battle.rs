//! Battles played from Camelot's battle test menu, as `render psynergy`
//! plays them, for the battle guides' recordings (guide_fight.rs): a run
//! with its party, formation and the function entries it is watched at, a
//! roll forced only by answering Battle_Random at the call that makes it,
//! and the battle's units and calls read from memory and the map.

use super::guide::Guide;
use super::guide_more::screen;
use super::mgba::{Gba, LR};
use super::plan::{self, Tables};

/// KEYINPUT bits.
const KEY_A: u32 = 0x1;
const KEY_DOWN: u32 = 0x80;

/// struct MainRoutineWork offsets (main_routine_work.h), as `battle.rs` uses them.
const STATE_ACTIVE_OWNERS: u32 = 0x1f8;
const STATE_BATTLE_START: u32 = 0x22b;
const STATE_OWNERS: u32 = 0x2c0;
/// A run that is not over by this frame has gone wrong.
const MAX_FRAMES: u32 = 20000;


/// What a battle run is: the debug party preset and party, the formation
/// (a row of gFormations), gMainRoutineWork.battleType and the function entries
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
            start: at("MainRoutine_RunFar").or_else(|_| at("SequenceParent_RunFar")).or_else(|_| at("ParentSequence_RunFar")).or_else(|_| at("Monitor_RunFar")).or_else(|_| at("SequenceManager_RunFar")).or_else(|_| at("Game_RunFar"))?,
            menu: at("DebugBattle_RunMenu")?,
            preset: at("DebugParameterSet_Load").or_else(|_| at("DebugParty_LoadPreset"))?,
            encounter: at("Battle_Run")?,
            game_state: at("gMainRoutineWork").or_else(|_| at("gSequenceParentWork")).or_else(|_| at("gAdventure")).or_else(|_| at("gCommon")).or_else(|_| at("gPersistentState")).or_else(|_| at("gSaveBlock"))?,
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

/// The enemies' table and their names, by row (enemy id less ENEMY_ID_FIRST).
fn enemy_table() -> Result<(super::tables::Table, Vec<String>), String> {
    let game = super::tables::Game::here()?;
    Ok((game.table("gEnemy_Definitions", "EnemyDefinition")?, game.row_names("gEnemy_Definitions", "EnemyDefinition", "MsgEnemyName")?))
}

/// The first enemy's unit number: the party's are below it.
const FIRST_ENEMY_UNIT: u32 = 128;
/// struct Unit's id, an enemy's row of gEnemy_Definitions plus ENEMY_ID_FIRST.
const ENEMY_ID_FIRST: usize = 8;
const UNIT_CLASS_ID: u32 = 0x128;
const UNIT_SIZE: u32 = 0x14c;
const UNIT_AGILITY: u32 = 0x40;

impl<'a> Guide<'a> {
    /// The name of the function holding `address`, from the map.
    pub(super) fn function_at(&self, address: u32) -> String {
        self.map
            .at_address(address)
            .and_then(|p| p.symbols.iter().filter(|(a, _)| *a & !1 <= address).max_by_key(|(a, _)| *a).map(|(_, n)| n.clone()))
            .unwrap_or_default()
    }

    /// A unit's name: a party member's, or an enemy's name, from its
    /// class.
    pub(super) fn unit_name(&self, gba: &Gba, unit: u16) -> String {
        if u32::from(unit) < FIRST_ENEMY_UNIT {
            return plan::character_name(unit as u8).to_string();
        }
        let enemies = self.sym("gEnemyUnits").map(|s| gba.read32(s)).unwrap_or(0);
        let class = gba.read8(enemies + (u32::from(unit) - FIRST_ENEMY_UNIT) * UNIT_SIZE + UNIT_CLASS_ID);
        enemy_table().ok().and_then(|(_, names)| names.get(usize::from(class).wrapping_sub(ENEMY_ID_FIRST)).filter(|n| !n.is_empty()).cloned()).unwrap_or(format!("UNIT {unit}"))
    }
}

/// The keys that make Isaac attack from the battle's first menu: Fight,
/// Attack, the aimed enemy.
const ATTACK_MENU: [(u32, u32); 7] = [(0, 20), (KEY_A, 4), (0, 30), (KEY_A, 4), (0, 30), (KEY_A, 4), (0, 320)];

/// struct Unit's inventory (battle_unit.h) and an entry's parts
/// (enum InventoryEncoding).
const UNIT_INVENTORY: u32 = 0xd8;
const INVENTORY_SLOTS: u32 = 15;
const ITEM_ID_MASK: u16 = 0x1ff;
const INVENTORY_EQUIPPED: u16 = 0x200;

impl<'a> Guide<'a> {
    /// Isaac attacks a copy of Ivan through the command menu, the rolls
    /// BattleActionRun_Build and RollWeaponUnleash ask Battle_Random for
    /// answered in turn from `plan_answers` and `unleash`, and, with
    /// `weapon`, that item in place of his equipped weapon as the battle
    /// opens; the frames it plays.
    pub(super) fn rolled_attack(&self, plan_answers: &[u32], unleash: u32, weapon: Option<u16>) -> Result<Vec<Vec<u16>>, String> {
        let root = crate::compile::root();
        let plan = Tables::read(root)?.plan()?;
        let mut cast = plan.casts.into_iter().find(|c| c.ability.name == "Quake").ok_or("no Quake cast")?;
        cast.foes = vec![plan::IVAN];
        let symbols = super::battle::Symbols::find(self.map)?;
        let random = self.sym("Battle_Random")?;
        let exec = self.sym("BattleAbility_ApplyToTarget")?;
        let encounter = self.sym("Battle_Run")?;
        let owner = self.sym("gMainRoutineWork")? + STATE_OWNERS;
        let game = super::tables::Game::here()?;
        let types = game.table("gItem_DefinitionTable", "ItemDefinition")?.column("type")?;
        let weapon_type = game.constant("ITEM_TYPE_WEAPON")?;
        let mut planned = false;
        let mut asked = 0;
        let mut swapped = weapon.is_none();
        let recording = super::battle::record_watching(self.rom, &symbols, &cast, &super::battle::Scene::Game, false, Some(&ATTACK_MENU), &[random, exec, encounter], &mut |gba, hook| match hook {
            Some(h) if h == encounter && !swapped => {
                // Isaac's equipped weapon becomes `weapon`, as if he wore it.
                for slot in 0..INVENTORY_SLOTS {
                    let at = owner + UNIT_INVENTORY + 2 * slot;
                    let entry = gba.read16(at);
                    if entry & INVENTORY_EQUIPPED != 0 && types.get(usize::from(entry & ITEM_ID_MASK)) == Some(&weapon_type) {
                        gba.write16(at, (entry & !ITEM_ID_MASK) | weapon.unwrap_or(0));
                        swapped = true;
                    }
                }
            }
            Some(h) if h == random && !planned => {
                let answer = match self.function_at(gba.register(LR) & !1).as_str() {
                    "BattleActionRun_Build" => {
                        asked += 1;
                        plan_answers.get(asked - 1).copied()
                    }
                    "RollWeaponUnleash" => Some(unleash),
                    _ => None,
                };
                if let Some(a) = answer {
                    gba.return_now(a);
                }
            }
            Some(h) if h == exec => planned = true,
            _ => {}
        })?;
        if !swapped {
            return Err("Isaac wears no weapon".into());
        }
        Ok(recording.frames)
    }

    /// The address of unit `id`'s struct Unit.
    pub(super) fn unit_address(&self, gba: &Gba, id: u16) -> Result<u32, String> {
        if u32::from(id) >= FIRST_ENEMY_UNIT {
            Ok(gba.read32(self.sym("gEnemyUnits")?) + (u32::from(id) - FIRST_ENEMY_UNIT) * UNIT_SIZE)
        } else {
            Ok(self.sym("gMainRoutineWork")? + STATE_OWNERS + u32::from(id) * UNIT_SIZE)
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
}
