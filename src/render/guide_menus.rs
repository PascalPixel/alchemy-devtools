//! Recordings for the menu, window, shop and game guides: one GIF of the
//! screen for each thing a section explains, played from a built edition
//! with the pad held as a player would. Each shows only what the game draws.

use super::field::{Field, Setup};
use super::guide::Guide;
use super::source;
use super::story::Trigger;

/// KEYINPUT bits.
const KEY_A: u32 = 0x1;
const KEY_B: u32 = 0x2;
const KEY_START: u32 = 0x8;
const KEY_RIGHT: u32 = 0x10;
const KEY_LEFT: u32 = 0x20;
const KEY_UP: u32 = 0x40;
const KEY_DOWN: u32 = 0x80;
const KEY_R: u32 = 0x100;
const KEY_L: u32 = 0x200;

/// Vale's part with the Sanctum, by day (SceneId_Vale, entrance 1), and
/// Garet's sister there.
const VALE: (u16, u16) = (4, 1);
const GARETS_SISTER: u16 = 18;
/// Xian (SceneId_Xian), entered from the world map: all four members have
/// joined by then, with the Djinn, Psynergy and equipment of their debug
/// preset (party.rs).
const XIAN: (u16, u16) = (72, 1);
/// Bilibin from the world map's road (SceneId_Bilibin, entrance 1): a new
/// area, so its area name shows on arrival.
const BILIBIN: (u16, u16) = (30, 1);
/// The Vale Sanctum from Vale (SceneId_Sanctum, entrance 1).
const VALE_SANCTUM: (u16, u16) = (9, 1);
/// Facing north, 1/65536 turns: across a counter.
const NORTH: u32 = 0xc000;

/// struct Unit's fields read and set here (battle_unit.h).
const UNIT_HP_GAUGE: u32 = 0x14;
const UNIT_MAX_HP: u32 = 0x34;
const UNIT_HP: u32 = 0x38;
const UNIT_INVENTORY: u32 = 0xd8;
const UNIT_POISON: u32 = 0x131;
const INVENTORY_SLOTS: u32 = 15;
/// Inventory entry bits: 15..11 quantity less one | 10 broken | 9 equipped.
const INVENTORY_BROKEN: u16 = 0x400;
const INVENTORY_QUANTITY_SHIFT: u16 = 11;
/// A gauge's full share, Unit.hpGauge.
const GAUGE_FULL: u32 = 0x4000;
/// struct Common's fields (common.h).
const SAVE_SHOP_CREDIT: u32 = 0x118;
const SAVE_MESSAGE_SPEED: u32 = 0x20c;
/// The characters by number.
const ISAAC: u8 = 0;
const GARET: u8 = 1;

/// The modes this file plays, after guide_engine::MODES.
pub const MODES: &[&str] = &[
    "windows-zoom",
    "message-speed",
    "message-blanks",
    "pause-menu",
    "save-game",
    "option-menu",
    "naming-screen",
    "area-name-arrival",
    "field-menu-icons",
    "psynergy-cure",
    "item-use",
    "djinn-standby",
    "summary-screen",
    "status-panel",
    "item-sort",
    "shop-counter",
    "shop-long-sword",
    "game-ticket",
    "shop-sell",
    "shop-repair",
    "shop-artifacts",
    "sanctum-revive",
    "item-picker-full",
    "inventory-slots",
];

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
    pub(super) fn run_menus(&mut self, name: &str) -> Result<(), String> {
        match name {
            "windows-zoom" => self.windows_zoom(),
            "message-speed" => self.message_speed(),
            "message-blanks" => self.message_blanks(),
            "pause-menu" => self.pause_menu(),
            "save-game" => self.save_game(),
            "option-menu" => self.option_menu(),
            "naming-screen" => self.naming_screen(),
            "area-name-arrival" => self.area_name_arrival(),
            "field-menu-icons" => self.field_menu_icons(),
            "psynergy-cure" => self.psynergy_cure(),
            "item-use" => self.item_use(),
            "djinn-standby" => self.djinn_standby(),
            "summary-screen" => self.summary_screen(),
            "status-panel" => self.status_panel(),
            "item-sort" => self.item_sort(),
            "shop-counter" => self.shop_counter(),
            "shop-long-sword" => self.shop_long_sword(),
            "game-ticket" => self.game_ticket(),
            "shop-sell" => self.shop_sell(),
            "shop-repair" => self.shop_repair(),
            "shop-artifacts" => self.shop_artifacts(),
            "sanctum-revive" => self.sanctum_revive(),
            "item-picker-full" => self.item_picker_full(),
            "inventory-slots" => self.inventory_slots(),
            _ => self.run_lucky(name),
        }
    }

    /// Plays `path` on `field` from now and writes every frame as `file`.
    fn record(&mut self, field: &mut Field, path: &[(u32, u32)], file: &str, what: &str) -> Result<(), String> {
        let frames = Self::press_path(field, path)?;
        self.guide_gif(file, &frames, what)
    }

    /// Item ids by name, from the built edition's item table.
    fn item_id(name: &str) -> Result<u16, String> {
        let items = super::tables::Game::here()?.item_names()?;
        items.iter().position(|i| i == name).map(|p| p as u16).ok_or(format!("no item is named {name}"))
    }

    /// Puts `entries` (item ids with their state and count bits) at the
    /// front of `owner`'s bag, the items held before after them.
    fn give_front(field: &mut Field, owner: u8, entries: &[u16]) -> Result<(), String> {
        let unit = field.unit(owner)?;
        let held: Vec<u16> = (0..INVENTORY_SLOTS).map(|i| field.read16(unit + UNIT_INVENTORY + 2 * i)).filter(|&e| e != 0).collect();
        let list: Vec<u16> = entries.iter().chain(held.iter()).copied().collect();
        for i in 0..INVENTORY_SLOTS {
            field.write16(unit + UNIT_INVENTORY + 2 * i, list.get(i as usize).copied().unwrap_or(0));
        }
        Ok(())
    }

    /// `owner` down to `hp` HP, the gauge to match.
    fn hurt(field: &mut Field, owner: u8, hp: u32) -> Result<(), String> {
        let unit = field.unit(owner)?;
        let max = u32::from(field.read16(unit + UNIT_MAX_HP)).max(1);
        field.write16(unit + UNIT_HP, hp as u16);
        field.write16(unit + UNIT_HP_GAUGE, (hp * GAUGE_FULL / max) as u16);
        Ok(())
    }

    /// The Item screen in Xian opening, its six windows zooming open, and B:
    /// they zoom shut and the field menu's icons come back, as
    /// `windows-zoom.gif`. (B on the icons straight after a screen closes
    /// is not taken, so the recordings that back out of a screen end on the
    /// icons.)
    fn windows_zoom(&mut self) -> Result<(), String> {
        let mut field = self.enter(XIAN)?;
        let mut path = open_screen(2);
        path.extend(presses(&[KEY_B], 50));
        self.record(&mut field, &path, "windows-zoom.gif", "the Item screen's windows zooming open and shut")
    }

    /// Garet's sister's first line in Vale typed at the slow message speed,
    /// then, talked to again from the same moment, at the fast speed, as
    /// `message-speed.gif`.
    fn message_speed(&mut self) -> Result<(), String> {
        let save = self.sym("gCommon").or_else(|_| self.sym("gPersistentState")).or_else(|_| self.sym("gSaveBlock"))?;
        let (mut entered, _) = self.stand_for(VALE, Trigger::Talk(GARETS_SISTER), GARETS_SISTER)?;
        let field = &mut entered.field;
        let ready = field.save();
        field.write8(save + SAVE_MESSAGE_SPEED, 0);
        let mut frames = Self::press_path(field, &[(0, 10), (KEY_A, 3), (0, 120)])?;
        field.load(&ready);
        field.write8(save + SAVE_MESSAGE_SPEED, 2);
        // The second run from its first frame played: press_path's first
        // picture is the screen as it was before the state was loaded.
        frames.extend(Self::press_path(field, &[(0, 10), (KEY_A, 3), (0, 60), (KEY_A, 3), (0, 60), (KEY_A, 3), (0, 40)])?.into_iter().skip(1));
        self.guide_gif("message-speed.gif", &frames, "a line typed at the slow and then the fast message speed")
    }

    /// Bilibin's innkeeper asked for a room: the price, a number filled
    /// into the line, and No, as `message-blanks.gif`.
    fn message_blanks(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.bilibin_keeper(6, "ACTOR_BILIBIN_INNKEEPER|LOCALID_BILIBIN_INNKEEPER")?;
        entered.field.set_coins(500);
        let path = [(0, 20), (KEY_A, 3), (0, 70), (KEY_A, 3), (0, 90), (KEY_DOWN, 3), (0, 20), (KEY_A, 3), (0, 90), (KEY_A, 3), (0, 40)];
        self.record(&mut entered.field, &path, "message-blanks.gif", "the innkeeper's price, a number filled into the line")
    }

    /// The pause menu in Vale: Start, the hand down its three entries and
    /// B, as `pause-menu.gif`.
    fn pause_menu(&mut self) -> Result<(), String> {
        let mut field = self.enter(VALE)?;
        let mut path = vec![(0, 20), (KEY_START, 4), (0, 50)];
        path.extend(presses(&[KEY_DOWN, KEY_DOWN, KEY_B], 35));
        self.record(&mut field, &path, "pause-menu.gif", "the pause menu's three entries")
    }

    /// Saving in Vale: Start, Save your quest, the first file, and the
    /// messages to `Your game was saved.`, as `save.gif`.
    fn save_game(&mut self) -> Result<(), String> {
        let mut field = self.enter(VALE)?;
        let path = [(0, 20), (KEY_START, 4), (0, 50), (KEY_A, 4), (0, 100), (KEY_A, 4), (0, 200), (KEY_A, 4), (0, 40)];
        self.record(&mut field, &path, "save.gif", "saving the game from the pause menu")
    }

    /// The option menu in Vale: the window hue turned and its brightness
    /// lowered, the windows' colours changing as it goes, then B, which
    /// puts the saved colours back, as `options.gif`.
    fn option_menu(&mut self) -> Result<(), String> {
        let mut field = self.enter(VALE)?;
        let mut path = vec![(0, 20), (KEY_START, 4), (0, 50)];
        path.extend(presses(&[KEY_DOWN, KEY_DOWN, KEY_A], 35));
        path.extend(presses(&[KEY_RIGHT; 6], 12));
        path.extend(presses(&[KEY_DOWN], 25));
        path.extend(presses(&[KEY_LEFT; 4], 12));
        path.extend(presses(&[KEY_B, KEY_B], 45));
        self.record(&mut field, &path, "options.gif", "the option menu's window colours changed and cancelled")
    }

    /// A new game with no save file: the name entry screen, the hand across the
    /// keyboard, a letter taken off and two put on, and End, as
    /// `naming-screen.gif`.
    fn naming_screen(&mut self) -> Result<(), String> {
        let menu = source::map_id("MainMenu")?;
        let setup = Setup { scene: menu, entrance: 1, set: Vec::new(), clear: Vec::new() };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        let mut lead = Vec::new();
        Self::hold(&mut field, 0, 240, &mut lead)?;
        // From the first frame not black.
        let first = lead.iter().position(|f| f.iter().any(|&c| c & 0x7fff != 0)).unwrap_or(0);
        let mut frames: Vec<Vec<u16>> = lead[first..].to_vec();
        // The hand starts on End: B twice takes "ac" off "Isaac"; right wraps
        // to the first column of the bottom row, up three times reaches "a", and two to the right "c";
        // Start jumps to End and A ends the name.
        let mut path = presses(&[KEY_B, KEY_B], 25);
        path.extend(presses(&[KEY_RIGHT, KEY_UP, KEY_UP, KEY_UP, KEY_A, KEY_RIGHT, KEY_RIGHT, KEY_A], 18));
        path.extend(presses(&[KEY_START], 30));
        path.extend(presses(&[KEY_A], 60));
        for &(keys, n) in &path {
            Self::hold(&mut field, keys, n, &mut frames)?;
        }
        self.guide_gif("naming-screen.gif", &frames, "the name entry screen of a new game")
    }

    /// Arriving in Bilibin from the world map: its area name shows near
    /// the top of the screen and goes, as `area-name.gif`.
    fn area_name_arrival(&mut self) -> Result<(), String> {
        let mut field = self.arrive(BILIBIN, &[], &["FLAG_VALE_STORM", "FLAG_CHILDHOOD"])?;
        field.show_notice = true;
        let mut frames = Vec::new();
        Self::hold(&mut field, 0, 260, &mut frames)?;
        let first = frames.iter().position(|f| f.iter().any(|&c| c & 0x7fff != 0)).unwrap_or(0);
        self.guide_gif("area-name.gif", &frames[first..], "Bilibin's area name on arrival")
    }

    /// The field menu in Xian, where the party has Djinn: its four icons
    /// one after another and back, then B, as `field-menu-icons.gif`.
    fn field_menu_icons(&mut self) -> Result<(), String> {
        let mut field = self.enter(XIAN)?;
        let mut path = vec![(0, 20), (KEY_A, 4), (0, 40)];
        path.extend(presses(&[KEY_RIGHT, KEY_RIGHT, KEY_RIGHT, KEY_RIGHT, KEY_B], 30));
        self.record(&mut field, &path, "field-menu-icons.gif", "the field menu's four icons")
    }

    /// The Psynergy screen in Xian: Isaac's Cure used on Garet, hurt first,
    /// his HP rising and Isaac's PP falling, then B out, as
    /// `psynergy-cure.gif`.
    fn psynergy_cure(&mut self) -> Result<(), String> {
        let mut field = self.enter(XIAN)?;
        Self::hurt(&mut field, GARET, 40)?;
        let mut path = open_screen(0);
        path.extend([(KEY_A, 4), (0, 60), (KEY_A, 4), (0, 50), (KEY_RIGHT, 4), (0, 40), (KEY_A, 4), (0, 120)]);
        path.extend(presses(&[KEY_B, KEY_B, KEY_B], 30));
        self.record(&mut field, &path, "psynergy-cure.gif", "Cure used on Garet from the Psynergy screen")
    }

    /// The Item screen in Xian: Isaac's Herb used on Garet, hurt first, as
    /// `item-use.gif`.
    fn item_use(&mut self) -> Result<(), String> {
        let mut field = self.enter(XIAN)?;
        Self::hurt(&mut field, GARET, 40)?;
        let herb = Self::item_id("Herb")?;
        Self::give_front(&mut field, ISAAC, &[herb | 2 << INVENTORY_QUANTITY_SHIFT])?;
        let mut path = open_screen(2);
        path.extend(presses(&[KEY_A, KEY_A, KEY_A], 45));
        path.extend(presses(&[KEY_RIGHT, KEY_A], 40));
        path.extend([(0, 100)]);
        path.extend(presses(&[KEY_B, KEY_B, KEY_B], 30));
        self.record(&mut field, &path, "item-use.gif", "a Herb used on Garet from the Item screen")
    }

    /// The Djinn screen in Xian: Isaac's Flint put on standby, the preview
    /// of what changes, and A, as `djinn-standby.gif`.
    fn djinn_standby(&mut self) -> Result<(), String> {
        let mut field = self.enter(XIAN)?;
        let mut path = open_screen(1);
        path.extend([(0, 30), (KEY_A, 4), (0, 60), (KEY_A, 4), (0, 120), (KEY_A, 4), (0, 120)]);
        path.extend(presses(&[KEY_B, KEY_B], 35));
        self.record(&mut field, &path, "djinn-standby.gif", "Flint put on standby from the Djinn screen")
    }

    /// The Status screen in Xian: Isaac's three pages, R to Garet, and B
    /// out, as `summary-screen.gif`.
    fn summary_screen(&mut self) -> Result<(), String> {
        let mut field = self.enter(XIAN)?;
        let mut path = open_screen(3);
        path.extend(presses(&[KEY_A], 60));
        path.extend(presses(&[KEY_A, KEY_A, KEY_A, KEY_R], 60));
        path.extend(presses(&[KEY_B, KEY_B, KEY_B], 30));
        self.record(&mut field, &path, "summary-screen.gif", "the Status screen's pages")
    }

    /// The Item screen in Xian, Isaac holding a Long Sword he does not wear
    /// and the Orb of Force: the status panel beside his list as the hand
    /// moves to each, as `status-panel.gif`.
    fn status_panel(&mut self) -> Result<(), String> {
        let mut field = self.enter(XIAN)?;
        let entries = [Self::item_id("Herb")?, Self::item_id("Long Sword")?, Self::item_id("Orb of Force")?];
        Self::give_front(&mut field, ISAAC, &entries)?;
        let mut path = open_screen(2);
        path.extend(presses(&[KEY_A], 50));
        path.extend(presses(&[KEY_DOWN, KEY_DOWN, KEY_DOWN], 50));
        path.extend(presses(&[KEY_B, KEY_B, KEY_B], 30));
        self.record(&mut field, &path, "status-panel.gif", "the status panel as the hand moves over a sword and the Orb of Force")
    }

    /// The Item screen's member row in Xian, Isaac's bag shuffled, and L
    /// held with A pressed four times, as `item-sort.gif`.
    fn item_sort(&mut self) -> Result<(), String> {
        let mut field = self.enter(XIAN)?;
        let isaac = field.unit(ISAAC)?;
        let held: Vec<u16> = (0..INVENTORY_SLOTS).map(|i| field.read16(isaac + UNIT_INVENTORY + 2 * i)).filter(|&e| e != 0).collect();
        let given = [("Herb", 3u16), ("Long Sword", 1), ("Antidote", 1), ("Orb of Force", 1), ("Leather Cap", 1), ("Nut", 2), ("Elven Shirt", 1)];
        let mut list = Vec::new();
        for (k, &(name, count)) in given.iter().enumerate() {
            list.push(Self::item_id(name)? | (count - 1) << INVENTORY_QUANTITY_SHIFT);
            if k % 2 == 0 {
                if let Some(&e) = held.get(k / 2) {
                    list.push(e);
                }
            }
        }
        list.extend(held.iter().skip(4));
        for i in 0..INVENTORY_SLOTS {
            field.write16(isaac + UNIT_INVENTORY + 2 * i, list.get(i as usize).copied().unwrap_or(0));
        }
        let mut path = open_screen(2);
        path.extend([(0, 20)]);
        for _ in 0..4 {
            path.extend([(KEY_L, 10), (KEY_L | KEY_A, 4), (KEY_L, 10), (0, 45)]);
        }
        path.extend(presses(&[KEY_B, KEY_B], 30));
        self.record(&mut field, &path, "item-sort.gif", "Isaac's bag sorted in each of the four orders")
    }

    /// A room of CottageRoomsC entered by entrance `room`, the
    /// leader across the counter from `keeper` (cottage_rooms_c.c).
    fn bilibin_keeper(&self, room: u16, keeper: &str) -> Result<(super::guide::Entered<'a>, super::field::Spot), String> {
        let map = source::map_id("CottageRoomsC")?;
        let id = source::actor_id("CottageRoomsC", keeper)?;
        self.stand_where((map, room), Trigger::Talk(id), id, |s| s.facing == NORTH)
    }

    /// Bilibin's weapon shop: the counter's four icons, Buy, Sell,
    /// Artifacts and Repair, and B to leave, as `shop-counter.gif`.
    fn shop_counter(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.bilibin_keeper(4, "ACTOR_BILIBIN_WEAPON_MERCHANT|LOCALID_BILIBIN_WEAPON_MERCHANT")?;
        entered.field.set_coins(1000);
        let mut path = vec![(0, 20), (KEY_A, 3), (0, 100)];
        path.extend(presses(&[KEY_RIGHT, KEY_RIGHT, KEY_RIGHT, KEY_RIGHT, KEY_B], 35));
        path.extend([(0, 60), (KEY_A, 3), (0, 40)]);
        self.record(&mut entered.field, &path, "shop-counter.gif", "the weapon shop's four icons")
    }

    /// Bilibin's weapon shop: Buy, the Long Sword, and the hand over each
    /// member, the shop showing what it would do to their Attack, as
    /// `long-sword.gif`.
    fn shop_long_sword(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.bilibin_keeper(4, "ACTOR_BILIBIN_WEAPON_MERCHANT|LOCALID_BILIBIN_WEAPON_MERCHANT")?;
        entered.field.set_coins(1000);
        let mut path = vec![(0, 20), (KEY_A, 3), (0, 100), (KEY_A, 3), (0, 70), (KEY_A, 3), (0, 60)];
        path.extend(presses(&[KEY_RIGHT, KEY_RIGHT, KEY_RIGHT, KEY_B, KEY_B, KEY_B], 45));
        path.extend([(0, 60), (KEY_A, 3), (0, 40)]);
        self.record(&mut entered.field, &path, "long-sword.gif", "the Long Sword offered to each member")
    }

    /// Bilibin's item shop with the shop credit 5 short of the first Game
    /// Ticket: a Herb bought, and the ticket given, as `game-ticket.gif`.
    fn game_ticket(&mut self) -> Result<(), String> {
        let save = self.sym("gCommon").or_else(|_| self.sym("gPersistentState")).or_else(|_| self.sym("gSaveBlock"))?;
        let (mut entered, _) = self.bilibin_keeper(5, "ACTOR_BILIBIN_ITEM_MERCHANT|LOCALID_BILIBIN_ITEM_MERCHANT")?;
        entered.field.set_coins(500);
        entered.field.write32(save + SAVE_SHOP_CREDIT, 295);
        let mut path = vec![(0, 20), (KEY_A, 3), (0, 100)];
        // Buy, the Herb, Isaac, one; then the thanks, the ticket's offer, Isaac
        // to carry it, and the thanks again; B back out of the list and the
        // counter.
        path.extend(presses(&[KEY_A, KEY_A, KEY_A, KEY_A], 60));
        path.extend(presses(&[KEY_A, KEY_A, KEY_A, KEY_A], 70));
        path.extend(presses(&[KEY_B, KEY_B], 60));
        path.extend([(0, 60), (KEY_A, 3), (0, 40)]);
        self.record(&mut entered.field, &path, "game-ticket.gif", "a Game Ticket given after a purchase")
    }

    /// Bilibin's item shop: Sell, Isaac, his Herb, and the coins it brings,
    /// as `sell.gif`.
    fn shop_sell(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.bilibin_keeper(5, "ACTOR_BILIBIN_ITEM_MERCHANT|LOCALID_BILIBIN_ITEM_MERCHANT")?;
        entered.field.set_coins(500);
        let herb = Self::item_id("Herb")?;
        Self::give_front(&mut entered.field, ISAAC, &[herb])?;
        let mut path = vec![(0, 20), (KEY_A, 3), (0, 100)];
        path.extend(presses(&[KEY_RIGHT, KEY_A, KEY_A, KEY_A, KEY_A, KEY_A], 60));
        path.extend(presses(&[KEY_B, KEY_B, KEY_B], 50));
        self.record(&mut entered.field, &path, "sell.gif", "a Herb sold at the item shop")
    }

    /// Bilibin's weapon shop: Repair, Isaac's broken Healing Ring (a ring
    /// breaks one use in eight; a sword never does), and the smith's hammer,
    /// as `repair.gif`.
    fn shop_repair(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.bilibin_keeper(4, "ACTOR_BILIBIN_WEAPON_MERCHANT|LOCALID_BILIBIN_WEAPON_MERCHANT")?;
        entered.field.set_coins(1000);
        // The ring goes last: while the smith has it, its slot is left empty
        // and the list shows no more than the items before it.
        let ring = Self::item_id("Healing Ring")?;
        let isaac = entered.field.unit(ISAAC)?;
        let held = (0..INVENTORY_SLOTS).filter(|&i| entered.field.read16(isaac + UNIT_INVENTORY + 2 * i) != 0).count() as u32;
        entered.field.write16(isaac + UNIT_INVENTORY + 2 * held, ring | INVENTORY_BROKEN);
        let mut path = vec![(0, 20), (KEY_A, 3), (0, 100)];
        path.extend(presses(&[KEY_RIGHT, KEY_RIGHT, KEY_RIGHT, KEY_A], 40));
        // The smith's advice waits for A; then Isaac.
        path.extend([(0, 80)]);
        path.extend(presses(&[KEY_A, KEY_A], 60));
        path.extend(presses(&vec![KEY_RIGHT; held as usize], 15));
        path.extend(presses(&[KEY_A, KEY_A, KEY_A, KEY_A, KEY_A, KEY_A], 80));
        // B: not equipped now; it ends as the smith asks what else needs
        // fixing.
        path.extend(presses(&[KEY_B], 70));
        self.record(&mut entered.field, &path, "repair.gif", "a broken Healing Ring repaired")
    }

    /// Bilibin's weapon shop: Artifacts and its list, as `artifacts.gif`.
    fn shop_artifacts(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.bilibin_keeper(4, "ACTOR_BILIBIN_WEAPON_MERCHANT|LOCALID_BILIBIN_WEAPON_MERCHANT")?;
        entered.field.set_coins(5000);
        let mut path = vec![(0, 20), (KEY_A, 3), (0, 100)];
        path.extend(presses(&[KEY_RIGHT, KEY_RIGHT, KEY_A], 40));
        path.extend([(0, 80)]);
        path.extend(presses(&[KEY_B, KEY_B, KEY_B], 50));
        self.record(&mut entered.field, &path, "artifacts.gif", "the weapon shop's Artifacts list")
    }

    /// The Vale Sanctum's Revive with the four in the party: Garet and Mia
    /// down, Ivan poisoned: the hand on Ivan is refused, Garet is revived,
    /// and B leaves, as `sanctum.gif`.
    fn sanctum_revive(&mut self) -> Result<(), String> {
        let map = VALE_SANCTUM;
        let healer = 8;
        let (mut entered, _) = self.stand_where(map, Trigger::Talk(healer), healer, |s| s.facing == NORTH)?;
        let field = &mut entered.field;
        field.set_party(&[0, 1, 2, 3]);
        field.set_coins(2000);
        for (owner, down, poison) in [(1u8, true, false), (2, false, true), (3, true, false)] {
            let unit = field.unit(owner)?;
            if down {
                field.write16(unit + UNIT_HP, 0);
                field.write16(unit + UNIT_HP_GAUGE, 0);
            }
            if poison {
                field.write8(unit + UNIT_POISON, 1);
            }
        }
        let mut path = vec![(0, 20), (KEY_A, 3), (0, 100)];
        for _ in 0..2 {
            path.extend([(KEY_A, 3), (0, 60)]);
        }
        path.extend([(0, 60), (KEY_RIGHT, 3), (0, 45), (KEY_A, 3), (0, 45), (KEY_LEFT, 3), (0, 45), (KEY_A, 3), (0, 60)]);
        for _ in 0..3 {
            path.extend([(KEY_A, 3), (0, 60)]);
        }
        path.extend([(0, 200), (KEY_A, 3), (0, 60), (KEY_B, 3), (0, 60), (KEY_B, 3), (0, 60), (KEY_A, 3), (0, 40)]);
        self.record(field, &path, "sanctum.gif", "the Vale Sanctum's Revive")
    }

    /// Isaac and Garet in Vale with every bag slot full, given a Nut: the
    /// item picker asks which item to drop, as `item-picker.gif`.
    fn item_picker_full(&mut self) -> Result<(), String> {
        const FILLERS: [&str; 26] = [
            "Herb", "Vial", "Potion", "Psy Crystal", "Antidote", "Elixir", "Mist Potion", "Power Bread", "Cookie", "Apple", "Hard Nut", "Mint", "Lucky Pepper", "Smoke Bomb", "Sleep Bomb", "Bramble Seed", "Crystal Powder", "Oil Drop", "Weasel's Claw", "Bone", "Corn", "Sacred Feather", "Lucky Medal", "Mythril Bag", "Small Jewel", "Dragon Skin",
        ];
        let give = self.sym("PartyInventory_GiveItem")?;
        let mut entered = self.enter_with(VALE, &[])?;
        let field = &mut entered.field;
        field.set_party(&[0, 1]);
        let mut fillers = FILLERS.iter().filter_map(|n| Self::item_id(n).ok()).map(Ok::<u16, String>);
        for owner in [0u8, 1] {
            let unit = field.unit(owner)?;
            for s in 0..INVENTORY_SLOTS {
                if field.read16(unit + UNIT_INVENTORY + 2 * s) == 0 {
                    let item = fillers.next().ok_or("too few fillers")??;
                    field.write16(unit + UNIT_INVENTORY + 2 * s, item);
                }
            }
        }
        field.start_call(give, [u32::from(Self::item_id("Nut")?), 0]);
        let path = [
            (0, 60), (KEY_A, 3), (0, 60), (KEY_A, 3), (0, 60), (KEY_A, 3), (0, 60), (KEY_A, 3), (0, 60),
            (KEY_RIGHT, 3), (0, 50), (KEY_LEFT, 3), (0, 50), (KEY_A, 3), (0, 50), (KEY_DOWN, 3), (0, 40), (KEY_A, 3), (0, 120),
        ];
        self.record(field, &path, "item-picker.gif", "a Nut given with every bag full")
    }

    /// The Item screen in Xian, Isaac holding three Herbs and a broken
    /// Long Sword beside what he wears: the hand along his list, as
    /// `inventory.gif`.
    fn inventory_slots(&mut self) -> Result<(), String> {
        let mut field = self.enter(XIAN)?;
        let entries = [Self::item_id("Herb")? | 2 << INVENTORY_QUANTITY_SHIFT, Self::item_id("Long Sword")? | INVENTORY_BROKEN];
        Self::give_front(&mut field, ISAAC, &entries)?;
        let mut path = open_screen(2);
        path.extend(presses(&[KEY_A], 50));
        path.extend(presses(&[KEY_DOWN, KEY_DOWN, KEY_DOWN, KEY_DOWN], 45));
        path.extend(presses(&[KEY_B, KEY_B, KEY_B], 30));
        self.record(&mut field, &path, "inventory.gif", "Isaac's bag on the Item screen")
    }
}
