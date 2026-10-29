//! Recordings of Tolbi's two minigames for the overworld guide, each
//! reached as the player reaches it: the attendant in the game rooms by
//! Tolbi's spring (SceneId_StuccoRoomsC, tolbi_spring.c) talked to, the
//! offer accepted, and the game played from the sequence that runs
//! it (SEQUENCE_LUCKY_DICE, SEQUENCE_LUCKY_WHEELS).

use super::field::{Field, Saved};
use super::guide::Guide;
use super::source;
use super::story::Trigger;

/// KEYINPUT bits.
const KEY_A: u32 = 0x1;
const KEY_UP: u32 = 0x40;

/// The game rooms' map and the entrances into their two rooms
/// (STUCCO_ROOMS_C_LUCKY_DICE_FROM_TOLBI, STUCCO_ROOMS_C_LUCKY_WHEELS_FROM_TOLBI).
const GAME_ROOMS: &str = "StuccoRoomsC";
const DICE_ROOM: u16 = 10;
const WHEELS_ROOM: u16 = 11;
/// The sequences that run the games (scene_tables.h), stored in
/// SaveBlock.mapId in place of a map.
const SEQUENCE_LUCKY_WHEELS: u16 = 0x1fc;
const SEQUENCE_LUCKY_DICE: u16 = 0x1fd;
/// struct MainRoutineWork's mapId and the prize categories Lucky Wheels writes
/// (LuckyWheelsWonPrizesView, lucky_wheels.c), s8 each, -1 after the last.
const SAVE_MAP_ID: u32 = 0x1c0;
const SAVE_WON_PRIZES: u32 = 0x12c;
/// struct Unit's bag (battle_unit.h): 15 entries, bits 15..11 the
/// quantity less one.
const UNIT_INVENTORY: u32 = 0xd8;
const INVENTORY_SLOTS: u32 = 15;
const INVENTORY_QUANTITY_SHIFT: u16 = 11;
/// SaveBlock.coins.
const SAVE_COINS: u32 = 0x10;
/// A prize category no line gives, written before a spin so that a win
/// shows as the first entry turning non-negative.
const NO_PRIZE: i8 = -3;

/// Frames from the run starting to the player's first turn, past the fade
/// in; frames kept before A; frames a throw or spin may take to give its
/// result; and the most throws or delays tried for a result worth showing.
const SETTLE: u32 = 60;
const LEAD: u32 = 40;
const LIMIT: u32 = 480;
const TRIES: u32 = 60;
/// Lucky Dice: the coins given, and the frames kept after the winnings
/// join them.
const DICE_COINS: u32 = 1000;
const DICE_TAIL: u32 = 120;
/// Frames from A on a result to the dice on the line again.
const DICE_NEXT: u32 = 60;
/// Lucky Wheels: the Game Tickets given, and the frames kept after the
/// last wheel stops on a win.
const WHEELS_TICKETS: u16 = 9;
const WHEELS_TAIL: u32 = 200;

/// The modes this file plays, after guide_menus::MODES.
pub const MODES: &[&str] = &["overworld-lucky-dice", "overworld-lucky-wheels"];

impl<'a> Guide<'a> {
    /// Plays one of this file's modes.
    pub(super) fn run_lucky(&mut self, name: &str) -> Result<(), String> {
        match name {
            "overworld-lucky-dice" => self.lucky_dice(),
            "overworld-lucky-wheels" => self.lucky_wheels(),
            _ if super::guide_scene_map::MODES.contains(&name) => self.run_scene_map(name),
            _ => self.run_interface(name),
        }
    }

    /// The game rooms entered by `room`, the leader across from attendant
    /// `attendant`, then, after `prepare`, A every 40 frames through the offer
    /// until the next sequence is `run`. Returns the field there.
    fn lucky_reach(&self, room: u16, attendant: &str, prepare: impl Fn(&mut Field) -> Result<(), String>, run: u16) -> Result<Field<'a>, String> {
        let map = source::map_id(GAME_ROOMS)?;
        let id = source::actor_id(GAME_ROOMS, attendant)?;
        let (entered, _) = self.stand_for((map, room), Trigger::Talk(id), id)?;
        let mut field = entered.field;
        prepare(&mut field)?;
        let save = self.sym("gMainRoutineWork").or_else(|_| self.sym("gSequenceParentWork")).or_else(|_| self.sym("gAdventure")).or_else(|_| self.sym("gCommon")).or_else(|_| self.sym("gPersistentState")).or_else(|_| self.sym("gSaveBlock"))?;
        let mut n = 0;
        while field.read16(save + SAVE_MAP_ID) != run {
            if n > 3000 {
                return Err(format!("the attendant never started the game ({attendant})"));
            }
            if n % 40 == 0 {
                field.press(3)?;
            } else {
                field.frames(1)?;
            }
            n += 1;
        }
        Ok(field)
    }

    /// From `ready`, the player's turn waiting, holds nothing for `delay`
    /// frames and then LEAD more, which are kept, presses A and plays on,
    /// keeping each frame, until `settled` gives the result (or LIMIT frames
    /// pass), then `tail` frames more. Returns the frames and the result.
    fn lucky_try(field: &mut Field, ready: &Saved, delay: u32, tail: u32, mut settled: impl FnMut(&Field) -> Option<u32>) -> Result<(Vec<Vec<u16>>, Option<u32>), String> {
        field.load(ready);
        field.frames(delay)?;
        let mut frames = Vec::new();
        Self::hold(field, 0, LEAD, &mut frames)?;
        Self::hold(field, KEY_A, 3, &mut frames)?;
        for _ in 0..LIMIT {
            Self::hold(field, 0, 1, &mut frames)?;
            if let Some(result) = settled(field) {
                Self::hold(field, 0, tail, &mut frames)?;
                return Ok((frames, Some(result)));
            }
        }
        Ok((frames, None))
    }

    /// Lucky Dice with 1,000 coins: the dice on the throwing line a moment,
    /// thrown with A, tumbling down the board and landing, then the four
    /// numbers they give, the result's lettering and the winnings, held, as
    /// `lucky-dice.gif`. LuckyDice_Run draws on Random16 only for a throw
    /// and the next lucky numbers, so the player throws again (A past the
    /// result, back to the line) until a throw pays more than its bet back
    /// (two pairs or better), and that throw is kept.
    fn lucky_dice(&mut self) -> Result<(), String> {
        let save = self.sym("gMainRoutineWork").or_else(|_| self.sym("gSequenceParentWork")).or_else(|_| self.sym("gAdventure")).or_else(|_| self.sym("gCommon")).or_else(|_| self.sym("gPersistentState")).or_else(|_| self.sym("gSaveBlock"))?;
        let mut field = self.lucky_reach(DICE_ROOM, "ACTOR_TOLBI_SPRING_DICE_ATTENDANT|LOCALID_TOLBI_SPRING_DICE_ATTENDANT", |f| {
            f.set_coins(DICE_COINS);
            Ok(())
        }, SEQUENCE_LUCKY_DICE)?;
        // Past the fade in, the dice on the throwing line.
        field.frames(SETTLE)?;
        let coins = |f: &Field| f.read32(save + SAVE_COINS);
        let mut first = None;
        for _ in 0..TRIES {
            let before = coins(&field);
            let ready = field.save();
            // The bet leaves the coins at the throw; the winnings join them
            // twenty frames into the result, as the winnings window opens.
            let mut low = before;
            let (frames, won) = Self::lucky_try(&mut field, &ready, 0, DICE_TAIL, |f| {
                low = low.min(coins(f));
                (coins(f) > low).then(|| coins(f))
            })?;
            match won {
                Some(after) if after > before => {
                    first = Some(frames);
                    break;
                }
                Some(_) if first.is_none() => first = Some(frames),
                _ => {}
            }
            // A on the result, to the payout or straight back to the line.
            field.press(3)?;
            field.frames(DICE_NEXT)?;
        }
        let frames = first.ok_or_else(|| format!("no throw paid in {TRIES}"))?;
        self.world_gif("overworld/lucky-dice.gif", &frames, "Lucky Dice: two dice thrown, landing, and the result")
    }

    /// Lucky Wheels with nine Game Tickets: four bet, which plays all seven
    /// lines; A spins the five wheels, they stop one by one, and the
    /// winning line's prize is shown, as `lucky-wheels.gif`. LuckyWheels_Update
    /// draws on Random16 every frame, and the wheels stop a random 4 to 7
    /// frames after their turn comes, so the player waiting a frame longer
    /// before A is another spin: the first that wins is kept.
    fn lucky_wheels(&mut self) -> Result<(), String> {
        let save = self.sym("gMainRoutineWork").or_else(|_| self.sym("gSequenceParentWork")).or_else(|_| self.sym("gAdventure")).or_else(|_| self.sym("gCommon")).or_else(|_| self.sym("gPersistentState")).or_else(|_| self.sym("gSaveBlock"))?;
        let ticket = super::tables::Game::here()?.item_names()?.iter().position(|i| i == "Game Ticket").ok_or("no item is named Game Ticket")? as u16;
        let mut field = self.lucky_reach(WHEELS_ROOM, "ACTOR_TOLBI_SPRING_WHEELS_ATTENDANT|LOCALID_TOLBI_SPRING_WHEELS_ATTENDANT", |f| {
            let unit = f.unit(0)?;
            for i in 0..INVENTORY_SLOTS {
                if f.read16(unit + UNIT_INVENTORY + 2 * i) == 0 {
                    f.write16(unit + UNIT_INVENTORY + 2 * i, ticket | (WHEELS_TICKETS - 1) << INVENTORY_QUANTITY_SHIFT);
                    return Ok(());
                }
            }
            Err("Isaac's bag is full".to_string())
        }, SEQUENCE_LUCKY_WHEELS)?;
        field.frames(SETTLE)?;
        // Up three times: the bet from one ticket to four.
        for _ in 0..3 {
            field.hold(KEY_UP);
            field.frames(3)?;
            field.hold(0);
            field.frames(10)?;
        }
        // No prize written yet: LuckyWheels_Update writes the first
        // line's prize when the last wheel stops on a win.
        field.write8(save + SAVE_WON_PRIZES, NO_PRIZE as u8);
        let ready = field.save();
        let won = |f: &Field| ((f.read8(save + SAVE_WON_PRIZES) as i8) >= 0).then_some(1);
        let mut frames = None;
        for delay in 0..TRIES {
            if let (kept, Some(_)) = Self::lucky_try(&mut field, &ready, delay, WHEELS_TAIL, won)? {
                frames = Some(kept);
                break;
            }
        }
        let frames = frames.ok_or_else(|| format!("no spin won in {TRIES}"))?;
        self.world_gif("overworld/lucky-wheels.gif", &frames, "Lucky Wheels: the wheels spun and stopped one by one, and the result")
    }
}
