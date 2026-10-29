//! Recordings or the window, shop and menu guides' sections that had none:
//! each plays the moment its section explains from a built edition, with
//! the pad held as a player would, and writes it as `<folder>/<name>.gif`
//! under the output folder (window/, shop/, menu/, graphics/).

use std::collections::VecDeque;

use super::animation::{self, is_black_bgr555};
use super::field::{Field, CLEARED};
use super::guide::Guide;
use super::mgba::{HEIGHT, WIDTH};
use super::source;
use super::story::Trigger;

/// KEYINPUT bits.
const KEY_A: u32 = 0x1;
const KEY_B: u32 = 0x2;
const KEY_SELECT: u32 = 0x4;
const KEY_START: u32 = 0x8;
const KEY_RIGHT: u32 = 0x10;
const KEY_LEFT: u32 = 0x20;
const KEY_UP: u32 = 0x40;
const KEY_DOWN: u32 = 0x80;
const KEY_R: u32 = 0x100;
const KEY_L: u32 = 0x200;

/// Xian (SceneId_Xian), entered from the world map: all four party members
/// have joined, with the Djinn, Psynergy and equipment of their debug
/// preset (party.rs).
const XIAN: (u16, u16) = (72, 1);
/// The Vale Sanctum from Vale (SceneId_Sanctum, entrance 1), and its healer.
const VALE_SANCTUM: (u16, u16) = (9, 1);
const SANCTUM_HEALER: u16 = 8;
/// Facing north, 1/65536 turns: across a counter.
const NORTH: u32 = 0xc000;

/// struct Unit's fields read and set here (battle_unit.h).
const UNIT_INVENTORY: u32 = 0xd8;
const UNIT_POISON: u32 = 0x131;
const INVENTORY_SLOTS: u32 = 15;
/// The characters by number.
const ISAAC: u8 = 0;
const GARET: u8 = 1;
const IVAN: u8 = 2;

/// Vale by day after the party has left (SceneId_Vale, entrance 1).
const VALE: (u16, u16) = (4, 1);
/// press_settled: the frames always let pass before a key, the frames the
/// picture must hold still (no more than SETTLE_PIXELS changing from one to
/// the next, as the hand and the figures keep moving), and the most frames
/// waited for that.
const SETTLE_PIXELS: usize = 300;
const SETTLE_MIN: u32 = 20;
const SETTLED: u32 = 20;
const SETTLE_LIMIT: u32 = 300;
/// Frames the Item screen is held after the drop.
const DROPPED_HOLD: u32 = 60;
/// The most entrances talk_into tries.
const MAX_ENTRANCE: u16 = 16;
/// Each picture of a close-up's zoom shown this many times over, and the
/// close-up's scale.
const SLOWED: usize = 6;
const CLOSE_UP_SCALE: usize = 3;
/// The frames a window takes to zoom open and shut (WINDOW_OPEN_FRAMES,
/// WINDOW_CLOSE_FRAMES), and the changed pixels that mark it at full size.
const WINDOW_OPEN_FRAMES: usize = 8;
const WINDOW_CLOSE_FRAMES: usize = 4;
const WINDOW_PIXELS: usize = 3000;
/// Pixels kept round a close-up's window.
const CLOSE_UP_MARGIN: usize = 8;
/// Frames the battle runs after its opening before the recording starts,
/// and the most frames a battle message recording keeps.
const BATTLE_SETTLE: u32 = 300;
const BATTLE_LOG_FRAMES: u32 = 60;
/// The title intro: frames of a lit screen that mark the prompt waiting, and
/// the frames kept of the intro after the cut.
const TITLE_LIT_WAIT: usize = 1500;
const TITLE_AFTER: usize = CLEARED + 60;

/// The modes this file plays, after guide_lucky::MODES.
pub const MODES: &[&str] = &["window-zoom", "pauses-pages", "battle-log", "label-page", "text-style", "buy-weapon", "sanctum-cure", "djinn-give", "status-row", "item-give", "title-timeout"];

/// The field menu opened with A and its icon `icon` (0 Psynergy to 3
/// Status, with the Djinn icon there) chosen.
fn open_screen(icon: u32) -> Vec<(u32, u32)> {
    let mut path = vec![(0, 20), (KEY_A, 4), (0, 50)];
    for _ in 0..icon {
        path.extend([(KEY_RIGHT, 4), (0, 25)]);
    }
    path.extend([(KEY_A, 4), (0, 70)]);
    path
}

/// Each key of `keys` pressed for 4 frames, then `wait` frames.
fn presses(keys: &[u32], wait: u32) -> Vec<(u32, u32)> {
    keys.iter().flat_map(|&k| [(k, 4), (0, wait)]).collect()
}

impl<'a> Guide<'a> {
    /// Plays one of this file's modes.
    pub(super) fn run_interface(&mut self, name: &str) -> Result<(), String> {
        match name {
            "window-zoom" => self.window_zoom(),
            "pauses-pages" => self.pauses_pages(),
            "battle-log" => self.battle_log(),
            "label-page" => self.label_page(),
            "text-style" => self.text_style(),
            "title-timeout" => self.title_timeout(),
            "buy-weapon" => self.buy_weapon(),
            "sanctum-cure" => self.sanctum_cure(),
            "djinn-give" => self.djinn_give(),
            "status-row" => self.status_row(),
            "item-give" => self.item_give(),
            _ => Err(format!("no mode {name}")),
        }
    }

    /// Plays `path` on `field` from now and writes every frame as `file`.
    fn record_into(&mut self, field: &mut Field, path: &[(u32, u32)], file: &str, what: &str) -> Result<(), String> {
        let frames = Self::press_path(field, path)?;
        self.world_gif(file, &frames, what)
    }

    /// Item ids by name, from the built edition's item table.
    fn item_number(name: &str) -> Result<u16, String> {
        let items = super::tables::Game::here()?.item_names()?;
        items.iter().position(|i| i == name).map(|p| p as u16).ok_or(format!("no item is named {name}"))
    }

    /// Fills `owner`'s pack to its 15 slots with one of each of `fillers`.
    fn fill_pack(field: &mut Field, owner: u8, fillers: &[&str]) -> Result<(), String> {
        let unit = field.unit(owner)?;
        let mut fillers = fillers.iter();
        for s in 0..INVENTORY_SLOTS {
            if field.read16(unit + UNIT_INVENTORY + 2 * s) == 0 {
                let name = fillers.next().ok_or("too few fillers")?;
                field.write16(unit + UNIT_INVENTORY + 2 * s, Self::item_number(name)?);
            }
        }
        Ok(())
    }

    /// Bilibin's weapon shop (CottageRoomsC, entrance 4), the leader across
    /// the counter from the merchant, the four party members there.
    fn weapon_counter(&self) -> Result<Field<'a>, String> {
        let map = source::map_id("CottageRoomsC")?;
        let id = source::actor_id("CottageRoomsC", "ACTOR_BILIBIN_WEAPON_MERCHANT|LOCALID_BILIBIN_WEAPON_MERCHANT")?;
        let (mut entered, _) = self.stand_where((map, 4), Trigger::Talk(id), id, |s| s.facing == NORTH)?;
        entered.field.set_party(&[0, 1, 2, 3]);
        entered.field.set_coins(3000);
        Ok(entered.field)
    }

    /// Buying the Long Sword at Bilibin's weapon shop: the hand passes the
    /// four party members, the comparison beside each; Ivan, who cannot
    /// equip it, is chosen and `You can't equip this, Ivan.` asks whether he
    /// still wants it; No, then Isaac, who takes it, equips it and is offered
    /// what his old sword fetches, as `shop/buy-weapon.gif`.
    fn buy_weapon(&mut self) -> Result<(), String> {
        let mut field = self.weapon_counter()?;
        let mut path = vec![(0, 20), (KEY_A, 3), (0, 100), (KEY_A, 3), (0, 70), (KEY_A, 3), (0, 70)];
        path.extend(presses(&[KEY_RIGHT, KEY_RIGHT, KEY_RIGHT, KEY_LEFT], 45));
        path.extend(presses(&[KEY_A], 90));
        path.extend(presses(&[KEY_B], 60));
        path.extend(presses(&[KEY_LEFT, KEY_LEFT], 30));
        path.extend(presses(&[KEY_A, KEY_A, KEY_A, KEY_A, KEY_A, KEY_A], 80));
        path.extend(presses(&[KEY_B, KEY_B], 60));
        self.record_into(&mut field, &path, "shop/buy-weapon.gif", "the Long Sword bought, refused for Ivan and equipped on Isaac")
    }

    /// The Vale Sanctum's Cure poison, Ivan poisoned: the service chosen,
    /// the hand passing a party member who needs nothing, Ivan cured, as
    /// `shop/sanctum-cure.gif`.
    fn sanctum_cure(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.stand_where(VALE_SANCTUM, Trigger::Talk(SANCTUM_HEALER), SANCTUM_HEALER, |s| s.facing == NORTH)?;
        let field = &mut entered.field;
        field.set_party(&[0, 1, 2, 3]);
        field.set_coins(2000);
        let ivan = field.unit(IVAN)?;
        field.write8(ivan + UNIT_POISON, 1);
        // The welcome, then Cure poison, the second icon.
        let mut path = vec![(0, 20), (KEY_A, 3), (0, 100)];
        path.extend(presses(&[KEY_RIGHT], 40));
        // The priest's question about poison, which waits for A.
        path.extend(presses(&[KEY_A], 80));
        path.extend(presses(&[KEY_A], 60));
        // The hand starts on Ivan, who needs the service: left to Garet,
        // who does not, and back.
        path.extend(presses(&[KEY_LEFT, KEY_RIGHT], 60));
        // Ivan, Yes to the donation, and A past `I shall apply the cure.`:
        // the burst and its tune, then the coins.
        path.extend(presses(&[KEY_A, KEY_A], 70));
        path.extend(presses(&[KEY_A], 320));
        // More aid? No, and the farewell.
        path.extend(presses(&[KEY_A, KEY_B, KEY_B], 60));
        self.record_into(field, &path, "shop/sanctum-cure.gif", "the Vale Sanctum's Cure poison")
    }

    /// The Djinn screen in Xian: Isaac's first Djinni traded for one of
    /// Garet's, the preview of both party members, R through its pages, and
    /// A, as `menu/djinn-give.gif`.
    fn djinn_give(&mut self) -> Result<(), String> {
        let mut field = self.enter(XIAN)?;
        let mut path = open_screen(1);
        path.extend([(0, 30), (KEY_A, 4), (0, 60)]);
        path.extend(presses(&[KEY_RIGHT, KEY_DOWN], 40));
        path.extend(presses(&[KEY_A], 120));
        path.extend(presses(&[KEY_R, KEY_R, KEY_R], 80));
        path.extend(presses(&[KEY_A], 120));
        path.extend(presses(&[KEY_B, KEY_B], 35));
        self.record_into(&mut field, &path, "menu/djinn-give.gif", "a Djinni traded from the Djinn screen")
    }

    /// The Status screen's party row in Xian: R moves Isaac one place along
    /// the party and L back, then Select lists the Djinn found, as
    /// `menu/status-row.gif`.
    fn status_row(&mut self) -> Result<(), String> {
        let mut field = self.enter(XIAN)?;
        // The found flags of the eight Djinn the preset gives the party, the
        // first two of each element (djinn.c: FLAG_DJINN_FIRST + 20 x
        // element + index); the first also lets Select open the list.
        let first = self.flag("FLAG_DJINN_FIRST")?;
        for element in 0..4 {
            for index in 0..2 {
                field.set_flag(first + 20 * element + index, true);
            }
        }
        let mut path = open_screen(3);
        path.extend(presses(&[KEY_R, KEY_R, KEY_L, KEY_L], 45));
        path.extend(presses(&[KEY_SELECT], 150));
        path.extend(presses(&[KEY_B], 50));
        path.extend(presses(&[KEY_B, KEY_B], 35));
        self.record_into(&mut field, &path, "menu/status-row.gif", "the Status screen's party row reordered, and the Djinn found")
    }

    /// The Item screen in Xian, Garet's pack full: Isaac's Herb given to
    /// him and traded for his Vial, then Isaac's Nut dropped, as
    /// `menu/item-give.gif`.
    fn item_give(&mut self) -> Result<(), String> {
        const FILLERS: [&str; 15] = ["Vial", "Potion", "Antidote", "Elixir", "Power Bread", "Cookie", "Apple", "Hard Nut", "Mint", "Lucky Pepper", "Smoke Bomb", "Sleep Bomb", "Bramble Seed", "Crystal Powder", "Oil Drop"];
        let mut field = self.enter(XIAN)?;
        let herb = Self::item_number("Herb")?;
        let isaac = field.unit(ISAAC)?;
        let held: Vec<u16> = (0..INVENTORY_SLOTS).map(|i| field.read16(isaac + UNIT_INVENTORY + 2 * i)).filter(|&e| e != 0).collect();
        // One Herb, so that the whole entry changes hands, and a Nut.
        let front = [herb, Self::item_number("Nut")?];
        let list: Vec<u16> = front.iter().chain(held.iter()).copied().collect();
        for i in 0..INVENTORY_SLOTS {
            field.write16(isaac + UNIT_INVENTORY + 2 * i, list.get(i as usize).copied().unwrap_or(0));
        }
        Self::fill_pack(&mut field, GARET, &FILLERS)?;
        // Each key once the screen has settled: the field menu and its Item
        // icon; Isaac, his Herb, Give (below Use), Garet; Garet's pack is
        // full, so his list, the four things he wears first, down to the
        // Vial, which Isaac takes for the Herb. Then Isaac's list again, its
        // first item, the Nut, and Drop (the grid's bottom right, from Use)
        // and Yes (the answer starts on No). The list takes no A at once
        // after a trade, so it is pressed twice.
        let keys = [
            KEY_A, KEY_RIGHT, KEY_RIGHT, KEY_A,
            KEY_A, KEY_A, KEY_DOWN, KEY_A, KEY_RIGHT, KEY_A,
            KEY_DOWN, KEY_DOWN, KEY_DOWN, KEY_DOWN, KEY_A,
            KEY_A, KEY_A, KEY_A, KEY_DOWN, KEY_RIGHT, KEY_RIGHT, KEY_A, KEY_UP, KEY_A,
        ];
        let mut frames = Self::press_path(&mut field, &[(0, 20)])?;
        Self::press_settled(&mut field, &keys, &mut frames)?;
        Self::hold(&mut field, 0, DROPPED_HOLD, &mut frames)?;
        Self::press_settled(&mut field, &[KEY_B, KEY_B, KEY_B], &mut frames)?;
        Self::hold(&mut field, 0, 30, &mut frames)?;
        self.world_gif("menu/item-give.gif", &frames, "an item given into a full pack and traded, then another dropped")
    }

    /// Presses each of `keys` for 4 frames once the picture has held still
    /// for SETTLED frames (or SETTLE_LIMIT have passed), keeping every frame:
    /// a key pressed while a window is still opening or its text is still
    /// typing can be lost.
    fn press_settled(field: &mut Field, keys: &[u32], frames: &mut Vec<Vec<u16>>) -> Result<(), String> {
        for &key in keys {
            Self::hold(field, 0, SETTLE_MIN, frames)?;
            let mut still = 0;
            for _ in 0..SETTLE_LIMIT {
                let before = field.picture();
                Self::hold(field, 0, 1, frames)?;
                let moved = frames.last().map_or(0, |f| f.iter().zip(&before).filter(|(a, b)| a != b).count());
                still = if moved <= SETTLE_PIXELS { still + 1 } else { 0 };
                if still >= SETTLED {
                    break;
                }
            }
            Self::hold(field, key, 4, frames)?;
        }
        Ok(())
    }

    /// Talking to `actor` of scene `map`, pressing A along `path`, as
    /// `file`. The scene is entered by the first of its entrances, from 1,
    /// that places the actor: a sheet of rooms places only the actors the
    /// camera shows (CONDITION_IN_VIEW).
    fn talk_into(&mut self, map: &str, actor: &str, path: &[(u32, u32)], file: &str, what: &str) -> Result<(), String> {
        let scene = source::map_id(map)?;
        let id = source::actor_id(map, actor)?;
        let mut first = None;
        for entrance in 1..=MAX_ENTRANCE {
            match self.stand_for((scene, entrance), Trigger::Talk(id), id) {
                Ok((mut entered, _)) => return self.record_into(&mut entered.field, path, file, what),
                Err(e) => {
                    first.get_or_insert(e);
                }
            }
        }
        Err(first.unwrap_or_default())
    }

    /// Garet's brother in Vale talked to, his one-page line typed and closed
    /// with A, as a close-up of his window at three times the GBA's size:
    /// the eight frames of its zoom open and the four of its zoom shut each
    /// shown SLOWED times over, the typing between at the game's speed, as
    /// `window/window-zoom.gif`. The close-up is the rectangle the window
    /// and its speaker change, with a margin.
    fn window_zoom(&mut self) -> Result<(), String> {
        let brother = source::actor_id("Vale", "ACTOR_GARETS_BROTHER")?;
        let (mut entered, _) = self.stand_for(VALE, Trigger::Talk(brother), brother)?;
        let frames = Self::press_path(&mut entered.field, &[(0, 20), (KEY_A, 3), (0, 150), (KEY_A, 3), (0, 50)])?;
        // The speaker turns to the party as A is pressed, a change of a few
        // hundred pixels; the window at full size changes thousands. Its
        // zoom open is the eight frames up to the first full-size picture,
        // its zoom shut the four after the last.
        let differ = |a: &[u16], b: &[u16]| a.iter().zip(b).filter(|(x, y)| x != y).count();
        let (still, last) = (&frames[0], &frames[frames.len() - 1]);
        let full = frames.iter().position(|f| differ(f, still) > WINDOW_PIXELS).ok_or("no window opened")?;
        let last_full = frames.iter().rposition(|f| differ(f, still) > WINDOW_PIXELS).unwrap_or(full);
        let opens = full.saturating_sub(WINDOW_OPEN_FRAMES);
        let closed = frames.iter().rposition(|f| differ(f, last) > 20).ok_or("the window never closed")? + 1;
        // The rectangle every frame between differs from the still in.
        let (mut x0, mut y0, mut x1, mut y1) = (WIDTH, HEIGHT, 0, 0);
        for f in &frames[opens..closed] {
            for (i, (a, b)) in f.iter().zip(still).enumerate() {
                if a != b {
                    let (x, y) = (i % WIDTH, i / WIDTH);
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                }
            }
        }
        let (x0, y0) = (x0.saturating_sub(CLOSE_UP_MARGIN), y0.saturating_sub(CLOSE_UP_MARGIN));
        let (x1, y1) = ((x1 + CLOSE_UP_MARGIN).min(WIDTH - 1), (y1 + CLOSE_UP_MARGIN).min(HEIGHT - 1));
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        let crop = |f: &[u16]| -> Vec<u32> {
            let mut out = Vec::with_capacity(w * h * CLOSE_UP_SCALE * CLOSE_UP_SCALE);
            for y in 0..h * CLOSE_UP_SCALE {
                for x in 0..w * CLOSE_UP_SCALE {
                    out.push(animation::rgb(f[(y0 + y / CLOSE_UP_SCALE) * WIDTH + x0 + x / CLOSE_UP_SCALE]));
                }
            }
            out
        };
        // The zooms slowed, with a frame either side.
        let open_end = (full + 2).min(frames.len());
        let close_start = last_full.max(open_end);
        let close_end = (last_full + WINDOW_CLOSE_FRAMES + 2).min(frames.len());
        let mut shown = Vec::new();
        for (i, f) in frames.iter().enumerate() {
            let times = if (opens.saturating_sub(1)..open_end).contains(&i) || (close_start..close_end).contains(&i) { SLOWED } else { 1 };
            let picture = crop(f);
            shown.extend(std::iter::repeat_n(picture, times));
        }
        let file = "window/window-zoom.gif";
        std::fs::create_dir_all(self.out.join("window")).map_err(|e| format!("cannot create window/: {e}"))?;
        let written = animation::write(&self.out.join(file), &shown, w * CLOSE_UP_SCALE)?;
        self.written.push((file.to_string(), written.bytes, format!("a message window zooming open and shut, close up and slowed; {} frames at 30 a second", written.frames)));
        Ok(())
    }

    /// The gem owner in Vault (SceneId_LogRoomsB): `Waah!`, a skippable
    /// pause, the page arrow, the window cleared and measured again for the
    /// shorter second page, and A closing it, as `window/pauses-pages.gif`.
    fn pauses_pages(&mut self) -> Result<(), String> {
        let path = [(0, 20), (KEY_A, 3), (0, 150), (KEY_A, 3), (0, 120), (KEY_A, 3), (0, 50)];
        self.talk_into("LogRoomsB", "ACTOR_GEM_OWNER", &path, "window/pauses-pages.gif", "a line with a pause and a page break")
    }

    /// The sick old man in Imil's houses (SceneId_IceRooms) while the
    /// epidemic lasts: each `Cough` typed in the shaking style ({text_effect:4},
    /// the decoder taking one off: TEXT_STYLE_SHAKE), its letters jittering a
    /// pixel either way, as `window/text-style.gif`.
    fn text_style(&mut self) -> Result<(), String> {
        let path = [(0, 20), (KEY_A, 3), (0, 160), (KEY_A, 3), (0, 100), (KEY_A, 3), (0, 140), (KEY_A, 3), (0, 50)];
        self.talk_into("IceRooms", "ACTOR_SICK_OLD_MAN", &path, "window/text-style.gif", "a line typed with shaking letters")
    }

    /// A random battle in Goma Cave, recorded from once it has opened: A
    /// through the commands, every party member attacking, and the turn's
    /// messages running on the text canvas at the bottom of the screen,
    /// as `window/battle-log.gif`. It ends before the screen goes black.
    fn battle_log(&mut self) -> Result<(), String> {
        let (place, stretch) = self.goma_cave_stretch()?;
        let mut entered = self.enter_goma_cave(place)?;
        let field = &mut entered.field;
        Self::walk_stretch(field, &stretch, 80, &mut Vec::new(), &mut 0, &mut |_| {})?.ok_or("no battle came")?;
        field.frames(BATTLE_SETTLE)?;
        // A through the opening's messages, Fight, and each party member's
        // Attack and target; spare presses fall on the turn's messages.
        let mut path = presses(&[KEY_A; 16], 40);
        path.push((0, BATTLE_LOG_FRAMES));
        let mut frames = Self::press_path(field, &path)?;
        if let Some(black) = frames.iter().position(|f| is_black_bgr555(f)) {
            frames.truncate(black.saturating_sub(30));
        }
        self.world_gif("window/battle-log.gif", &frames, "a battle turn's messages on the text canvas")
    }

    /// The Item screen in Xian, Isaac's pack filled to its 15 items: his
    /// list, five to a page, turned with Right through its three pages and
    /// round to the first, each page's names drawn again as window tiles,
    /// as `window/label-page.gif`.
    fn label_page(&mut self) -> Result<(), String> {
        const FILLERS: [&str; 15] = ["Herb", "Nut", "Vial", "Potion", "Antidote", "Elixir", "Power Bread", "Cookie", "Apple", "Hard Nut", "Mint", "Lucky Pepper", "Smoke Bomb", "Sleep Bomb", "Bramble Seed"];
        let mut field = self.enter(XIAN)?;
        Self::fill_pack(&mut field, ISAAC, &FILLERS)?;
        let mut path = open_screen(2);
        path.extend(presses(&[KEY_A], 60));
        path.extend(presses(&[KEY_RIGHT, KEY_RIGHT, KEY_RIGHT], 70));
        path.extend(presses(&[KEY_B, KEY_B, KEY_B], 30));
        self.record_into(&mut field, &path, "window/label-page.gif", "the Item screen's list of items turned a page at a time")
    }

    /// The title intro's prompt left alone until a minute passes: a game
    /// saved in Vale, so that the cartridge holds a file, then a soft reset
    /// (A, B, Select and Start) to the logos and the intro. Kept: the last
    /// seconds of `Press Start` pulsing, the fade to black and a moment of
    /// it, then, cut from there past the logos, the intro starting again,
    /// as `graphics/title-timeout.gif`.
    fn title_timeout(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with(VALE, &[])?;
        let field = &mut entered.field;
        Self::press_path(field, &[(0, 20), (KEY_START, 4), (0, 50), (KEY_A, 4), (0, 100), (KEY_A, 4), (0, 200), (KEY_A, 4), (0, 40)])?;
        Self::press_path(field, &[(KEY_A | KEY_B | KEY_SELECT | KEY_START, 6), (0, 1)])?;
        // Played on until the first black screen after a long lit one: the
        // prompt has waited its minute and the screen has faded out.
        let mut recent: VecDeque<Vec<u16>> = VecDeque::new();
        let mut lit = 0;
        for n in 0.. {
            if n > 12000 {
                return Err("the title intro never timed out".to_string());
            }
            field.frames(1)?;
            let picture = field.picture();
            let black = is_black_bgr555(&picture);
            if black && lit >= TITLE_LIT_WAIT {
                break;
            }
            lit = if black { 0 } else { lit + 1 };
            recent.push_back(picture);
            if recent.len() > CLEARED + 120 {
                recent.pop_front();
            }
        }
        let mut frames: Vec<Vec<u16>> = recent.into_iter().collect();
        Self::hold(field, 0, 30, &mut frames)?;
        // The logos come and go; the intro is the first lit stretch that
        // lasts.
        let mut run: Vec<Vec<u16>> = Vec::new();
        for n in 0.. {
            if n > 3000 {
                return Err("the intro never came back".to_string());
            }
            field.frames(1)?;
            let picture = field.picture();
            if is_black_bgr555(&picture) {
                run.clear();
                continue;
            }
            run.push(picture);
            if run.len() >= TITLE_AFTER {
                break;
            }
        }
        frames.extend(run);
        self.world_gif("graphics/title-timeout.gif", &frames, "the title prompt timing out and the intro starting again")
    }
}
