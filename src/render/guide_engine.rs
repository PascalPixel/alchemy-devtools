//! Take-apart pictures for the engine's guides (runtime, system, map, scene,
//! field, game, graphics and debug): the machine's tables and memory read
//! from the running game and drawn beside the screen they make.

use super::animation;
use super::field::{Field, Image};
use super::guide::Guide;
use super::guide_more::{canvas, hue, mix, outline, paste, screen};
use super::guide_take::text;
use super::mgba::{HEIGHT, WIDTH};
use super::source;
use std::cell::RefCell;
use std::rc::Rc;

const KEY_A: u32 = 0x1;
const KEY_B: u32 = 0x2;
const KEY_RIGHT: u32 = 0x10;
const KEY_LEFT: u32 = 0x20;
const KEY_UP: u32 = 0x40;
const KEY_DOWN: u32 = 0x80;

/// Vale's part with the Vale Sanctum, entered by day.
const VALE: (u16, u16) = (4, 1);
/// SceneId_MenuTest, the debug menu test room (scenes.s).
const MENU_TEST_ROOM: u16 = 200;

/// struct Adventure (gAdventure): the coins, and the scene and entrance the
/// field loads next.
const GS_COINS: u32 = 0x10;
const GS_SCENE: u32 = 0x1c0;
const GS_ENTRANCE: u32 = 0x1c2;
/// struct FieldWork: the exit request, which ends the scene; 999 leaves for
/// the game state's scene and entrance (Event_SetDestination).
const FW_MARKERS: u32 = 0x11c;
const FW_EXIT: u32 = 0x170;
const EXIT_TO_DESTINATION: u16 = 999;
/// struct Unit's level.
const UNIT_LEVEL: u32 = 0x0f;

/// The panels' colours.
const BACK: u32 = 0x20_22_28;
const INK: u32 = 0xe8_e8_e8;
const DIM: u32 = 0x80_84_8c;
const MARK: u32 = 0xff_d8_40;
const HEAD: u32 = 0x9c_c8_ff;
const RED: u32 = 0xff_50_50;
const GREEN: u32 = 0x60_e0_70;

/// The rare items whose counters MenuTest_Initialize raises,
/// by item id (item_ids.h), with how many times it raises each.
const MENU_TEST_COUNTERS: [(u32, &str, u32); 11] = [
    (0x05, "SHAMSHIR", 3),
    (0x06, "SILVER BLADE", 2),
    (0x07, "MASAMUNE", 1),
    (0x6a, "CHINA DRESS", 1),
    (0x6c, "COCKTAIL DRESS", 1),
    (0x6d, "BLESSED ROBE", 1),
    (0x71, "ORACLE'S ROBE", 1),
    (0x7b, "DRAGON SHIELD", 1),
    (0x82, "VAMBRACE", 1),
    (0x8c, "SPIRIT ARMLET", 1),
    (0x97, "WARRIOR'S HELM", 1),
];

/// What a visit to the menu test room is read for: who is in the party and
/// at what level, the coins, and each raised counter.
struct PartyState {
    members: Vec<(usize, u8)>,
    coins: u32,
    counters: Vec<u8>,
}

impl<'a> Guide<'a> {
    /// The party, coins and rare-item counters as the game holds them now.
    fn party_state(&self, field: &mut Field) -> Result<PartyState, String> {
        let flags = self.sym("gGameFlagBytes")?;
        let state = self.sym("gAdventure").or_else(|_| self.sym("gCommon")).or_else(|_| self.sym("gPersistentState")).or_else(|_| self.sym("gSaveBlock"))?;
        let counters = self.sym("gRareItemStock")?;
        let slots = self.sym("gItem_ArtifactSlotTable")?;
        let mut members = Vec::new();
        for owner in 0..8u32 {
            // FLAG_IN_PARTY(owner) is flag `owner`.
            if field.read8(flags + owner / 8) & (1 << (owner % 8)) != 0 {
                let unit = field.unit(owner as u8)?;
                members.push((owner as usize, field.read8(unit + UNIT_LEVEL)));
            }
        }
        let mut values = Vec::new();
        for &(item, _, _) in MENU_TEST_COUNTERS.iter().filter(|c| c.2 != 0) {
            let slot = u32::from(field.read8(slots + item));
            if slot == 0 {
                return Err(format!("item {item:#x} has no counter"));
            }
            values.push(field.read8(counters + slot - 1));
        }
        Ok(PartyState { members, coins: field.read32(state + GS_COINS), counters: values })
    }

    /// Sends the party from the scene it is in to `scene`'s `entrance`, as
    /// Event_SetDestination does, and runs `frames` frames.
    fn travel(&self, field: &mut Field, (scene, entrance): (u16, u16), frames: u32) -> Result<(), String> {
        let state = self.sym("gAdventure").or_else(|_| self.sym("gCommon")).or_else(|_| self.sym("gPersistentState")).or_else(|_| self.sym("gSaveBlock"))?;
        field.write16(state + GS_SCENE, scene);
        field.write16(state + GS_ENTRANCE, entrance);
        let work = field.field_work();
        field.write16(work + FW_EXIT, EXIT_TO_DESTINATION);
        field.frames(frames)
    }

    /// The menu test room's setup entry, MenuTest_Initialize,
    /// seen through the party it changes: the party in Vale before the
    /// visit, as the room's first entry leaves it, and after it has run a
    /// second time as the next visit runs it, as `menu-test-setup.png`. The
    /// party is sent into the room as Event_SetDestination sends it, and the
    /// entry's runs are counted by probing it.
    pub fn menu_test_setup(&mut self) -> Result<(), String> {
        let characters = source::character_names()?;
        let setup_fn = self.overlay_sym("MenuTest_GetTriggers|MenuTest_GetEvents", "MenuTest_Initialize")?;
        let state = self.sym("gAdventure").or_else(|_| self.sym("gCommon")).or_else(|_| self.sym("gPersistentState")).or_else(|_| self.sym("gSaveBlock"))?;
        let mut entered = self.enter_with(VALE, &[])?;
        let field = &mut entered.field;
        let before = self.party_state(field)?;
        let vale = screen(&field.picture());
        let runs = Rc::new(RefCell::new(0u32));
        let counted = runs.clone();
        field.probe_pcs = vec![setup_fn];
        field.probe = Some(Box::new(move |gba, pc, _| {
            if pc != 0 && gba.read16(state + GS_SCENE) == MENU_TEST_ROOM {
                *counted.borrow_mut() += 1;
            }
        }));
        self.travel(field, (MENU_TEST_ROOM, 0), 240)?;
        if *runs.borrow() != 1 || field.scene() != MENU_TEST_ROOM {
            return Err(format!("the setup ran {} times on the way into scene {}", runs.borrow(), field.scene()));
        }
        let first = self.party_state(field)?;
        let room = screen(&field.picture());
        field.call(setup_fn, [0, 0])?;
        field.probe = None;
        let second = self.party_state(field)?;
        for (i, row) in MENU_TEST_COUNTERS.iter().filter(|c| c.2 != 0).enumerate() {
            let (a, b, c) = (u32::from(before.counters[i]), u32::from(first.counters[i]), u32::from(second.counters[i]));
            if b != (a + row.2).min(99) || c != (b + row.2).min(99) {
                return Err(format!("{}'s counter went {a}, {b}, {c}", row.1));
            }
        }
        // Three columns: before, the first visit, the second.
        let states = [("IN VALE, BEFORE", &before), ("AFTER THE FIRST VISIT", &first), ("AFTER A SECOND VISIT", &second)];
        let col = WIDTH + 16;
        let mut image = canvas(3 * col + 8, HEIGHT + 16 + 330, BACK);
        paste(&mut image, &vale, 8, 8, 1);
        paste(&mut image, &room, 8 + col, 8, 1);
        paste(&mut image, &room, 8 + 2 * col, 8, 1);
        for (n, (title, s)) in states.iter().enumerate() {
            let x = (8 + n * col) as i32;
            let mut y = HEIGHT as i32 + 16;
            text(&mut image, x, y, title, HEAD, None, 2);
            y += 20;
            text(&mut image, x, y, "PARTY        LEVEL", DIM, None, 2);
            y += 16;
            for owner in 0..8usize {
                let now = s.members.iter().find(|m| m.0 == owner);
                let then = if n == 0 { now } else { states[n - 1].1.members.iter().find(|m| m.0 == owner) };
                let Some(&(_, level)) = now else {
                    if then.is_some() {
                        text(&mut image, x, y, &format!("{:<13}LEFT", characters[owner]), RED, None, 2);
                        y += 16;
                    }
                    continue;
                };
                let changed = n > 0 && then.map(|t| t.1) != Some(level);
                let colour = if then.is_none() { GREEN } else if changed { MARK } else { INK };
                text(&mut image, x, y, &format!("{:<13}{level:>3}", characters[owner]), colour, None, 2);
                y += 16;
            }
            let y = HEIGHT as i32 + 16 + 20 + 16 + 16 * 5 + 8;
            let changed = n > 0 && states[n - 1].1.coins != s.coins;
            text(&mut image, x, y, &format!("COINS {}", s.coins), if changed { MARK } else { INK }, None, 2);
            let mut y = y + 24;
            text(&mut image, x, y, "RARE ITEM    COUNTER", DIM, None, 2);
            y += 16;
            for (i, row) in MENU_TEST_COUNTERS.iter().filter(|c| c.2 != 0).enumerate() {
                let changed = n > 0 && states[n - 1].1.counters[i] != s.counters[i];
                text(&mut image, x, y, &format!("{:<15}{:>2}", row.1, s.counters[i]), if changed { MARK } else { INK }, None, 2);
                y += 14;
            }
        }
        self.guide_png("menu-test-setup.png", &image, "the menu test room's setup: the party and rare-item counters before a visit, after it and after a second")
    }

    /// The Long Sword's row of gItem_DefinitionTable beside the screens that show its
    /// columns, as `item-row.png`: in Bilibin's weapon shop (merchant 16)
    /// the Buy list with its price; the member chooser on Isaac, his Attack
    /// with his Bandit's Sword and with the Long Sword, and on Ivan, whose
    /// bit of equipMask is clear; and, once it is bought and the shop
    /// left, the Item screen with it chosen among Isaac's items. The row is
    /// read from the build; the screens are played with the pad.
    pub fn item_row(&mut self) -> Result<(), String> {
        const LONG_SWORD: &str = "Long Sword";
        let game = super::tables::Game::here()?;
        let items = game.table("gItem_DefinitionTable", "ItemDefinition")?;
        let row = game.item_names()?.iter().position(|n| n == LONG_SWORD).ok_or("no item is the Long Sword")?;
        let col = |name: &str| -> String {
            let value = items.values(row, name).ok().and_then(|v| v.first().copied()).unwrap_or_default();
            if name == "equipMask" { format!("0X{value:X}") } else { value.to_string() }
        };
        let (mut entered, _) = self.stand_where((31, 4), super::story::Trigger::Talk(16), 16, |s| s.facing == 0xc000)?;
        let field = &mut entered.field;
        field.set_coins(2000);
        let mut shots = Vec::new();
        let steps: [&[(u32, u32)]; 4] = [
            // Talk, and Buy: the list, the Long Sword first.
            &[(0, 20), (KEY_A, 3), (0, 100), (KEY_A, 3), (0, 60)],
            // Chosen: who gets it, the hand on Isaac.
            &[(KEY_A, 3), (0, 60)],
            // The hand on Ivan, two along.
            &[(KEY_RIGHT, 3), (0, 20), (KEY_RIGHT, 3), (0, 40)],
            // Back to Isaac and bought for him, equipped, his Bandit's
            // Sword kept; out of the shop; the field menu's Item screen,
            // Isaac's items, the hand moved up onto the last, the sword.
            &[(KEY_LEFT, 3), (0, 20), (KEY_LEFT, 3), (0, 20), (KEY_A, 3), (0, 60), (KEY_A, 3), (0, 60), (KEY_A, 3), (0, 60), (KEY_A, 3), (0, 60), (KEY_B, 3), (0, 60), (KEY_B, 3), (0, 60), (KEY_B, 3), (0, 60), (KEY_B, 3), (0, 60), (KEY_A, 3), (0, 90), (KEY_DOWN, 20), (0, 20), (KEY_A, 4), (0, 40), (KEY_RIGHT, 4), (0, 30), (KEY_RIGHT, 4), (0, 30), (KEY_A, 4), (0, 60), (KEY_A, 4), (0, 60), (KEY_UP, 4), (0, 40)],
        ];
        for path in steps {
            let frames = Self::press_path(field, path)?;
            shots.push(screen(frames.last().ok_or("no frames")?));
        }
        let captions = [
            format!("PRICE {}: THE BUY LIST", col("price")),
            format!("PRIMARYBONUS {}: ISAAC'S ATTACK 56 TO 58", col("primaryBonus")),
            format!("EQUIPMASK {}: IVAN CAN'T EQUIP IT", col("equipMask")),
            format!("ICON {} AND ITS NAME: THE ITEM SCREEN", col("icon")),
        ];
        let left = 330usize;
        let (w, h) = (left + 2 * (2 * WIDTH + 8) + 8, 8 + 2 * (2 * HEIGHT + 30));
        let mut image = canvas(w, h, BACK);
        for (i, shot) in shots.iter().enumerate() {
            let (x, y) = (left + (i % 2) * (2 * WIDTH + 8), 8 + (i / 2) * (2 * HEIGHT + 30));
            paste(&mut image, shot, x, y, 2);
            text(&mut image, x as i32, (y + 2 * HEIGHT + 6) as i32, &captions[i], MARK, None, 2);
        }
        let mut y = 8;
        text(&mut image, 8, y, "GITEM_DEFINITIONTABLE, THE LONG SWORD", HEAD, None, 2);
        y += 22;
        let shown = ["price", "type", "flags", "equipMask", "icon", "primaryBonus", "secondaryBonus", "unleashAbility", "unleashChance", "useType", "element", "effects.kind", "abilityId"];
        for name in shown {
            let on_screen = ["price", "equipMask", "icon", "primaryBonus"].contains(&name);
            text(&mut image, 8, y, &format!("{:<16}{}", name.replace("effects.", "effects[0].").to_uppercase(), col(name)), if on_screen { MARK } else { INK }, None, 2);
            y += 16;
        }
        y += 8;
        for line in ["TYPE 1: A WEAPON", "EQUIPMASK 0X13:", "ISAAC, GARET, FELIX", "ELEMENT 4: NONE", "YELLOW: ON THE SCREENS"] {
            text(&mut image, 8, y, line, DIM, None, 2);
            y += 16;
        }
        self.guide_png("item-row.png", &image, "the Long Sword's row of gItem_DefinitionTable beside the shop and Item screens that show its columns")
    }

    /// A cutscene's script beside the frames each of its commands waits
    /// through, as `scene-script.gif`: Vale_ReactToAnotherTremor, a Vale
    /// villager's talk event once FLAG_KALAY_REACHED is set, played by
    /// talking to her. Its body is read from vale.c a statement to a line;
    /// its calls are the BL instructions of its code in the loaded overlay,
    /// one to a statement, and every halfword of that code is probed, so a
    /// statement is current from the frame its call is reached until the
    /// frame the instruction after it is.
    pub fn scene_script(&mut self) -> Result<(), String> {
        const EVENT: &str = "Vale_ReactToAnotherTremor";
        const WOMAN: u16 = 17;
        let source = std::fs::read_to_string(crate::compile::root().join("games/tbs/src/scene/vale.c")).map_err(|e| format!("vale.c: {e}"))?;
        let head = source.lines().position(|l| l.contains(&format!("void {EVENT}(void)"))).ok_or("vale.c has no such event")?;
        // One statement a line, a statement clang-format wrapped joined again.
        let lines: Vec<&str> = source.lines().skip(head + 1).take_while(|l| !l.starts_with('}')).map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("//")).collect();
        let body: Vec<String> = lines.join(" ").split_inclusive(';').map(|s| s.trim().replace("( ", "(")).filter(|s| !s.is_empty()).collect();
        // As the 3 by 5 font draws them: no semicolons, and X for a times.
        let shown: Vec<String> = body.iter().map(|l| l.trim_end_matches(';').replace('*', "X")).collect();
        let mut entered = self.enter_with(VALE, &["FLAG_KALAY_REACHED"])?;
        let spot = entered.field.spots(super::story::Trigger::Talk(WOMAN))?.into_iter().next().ok_or("nowhere to talk to her")?;
        entered.field.stand(&entered.placed, &spot)?;
        let field = &mut entered.field;
        let (_, elf) = loaded_overlay(field).ok_or("no overlay is loaded")?;
        let sym = elf.symbols.iter().find(|s| s.name == EVENT).ok_or("the overlay has no such event")?;
        let (start, end) = (sym.value & !1, (sym.value & !1) + sym.size);
        // Its BL instructions, in order, and where each returns to.
        let mut calls = Vec::new();
        let mut a = start;
        while a + 4 <= end {
            let (h0, h1) = (field.read16(a), field.read16(a + 2));
            if h0 & 0xf800 == 0xf000 && h1 & 0xf800 == 0xf800 {
                let offset = ((i32::from(h0 & 0x7ff) << 21) >> 9) | (i32::from(h1 & 0x7ff) << 1);
                calls.push((a, a + 4, (a as i32 + 4 + offset) as u32));
                a += 4;
            } else {
                a += 2;
            }
        }
        if calls.len() != body.len() {
            let names: Vec<String> = calls.iter().map(|c| self.name_at(field, c.2)).collect();
            return Err(format!("{} calls ({names:?}) for {} statements", calls.len(), body.len()));
        }
        let hits = Rc::new(RefCell::new(Vec::new()));
        let h = hits.clone();
        field.probe_pcs = (start..end).step_by(2).collect();
        field.probe = Some(Box::new(move |_, pc, _| {
            if pc != 0 {
                h.borrow_mut().push(pc);
            }
        }));
        // Talk to her, and play until the script has run its last call.
        // Wide enough for the longest statement, as `text` measures it.
        let longest = shown.iter().map(|l| text(&mut canvas(1, 1, BACK), -100, -100, l, 0, None, 2)).max().unwrap_or(0).max(544) as usize;
        let (w, hgt) = (WIDTH + 16 + longest + 16, 330);
        let mut frames = Vec::new();
        let mut current: Option<usize> = None;
        let mut waited = vec![0u32; calls.len()];
        let mut done = false;
        for f in 0..900u32 {
            if f == 2 {
                field.hold(KEY_A);
            } else if f == 6 {
                field.hold(0);
            }
            let before = hits.borrow().len();
            field.frames(1)?;
            for &pc in &hits.borrow()[before..] {
                if let Some(i) = calls.iter().position(|c| c.0 == pc) {
                    current = Some(i);
                } else if let Some(i) = calls.iter().position(|c| c.1 == pc) {
                    if current == Some(i) {
                        current = None;
                        if i + 1 == calls.len() {
                            done = true;
                        }
                    }
                }
            }
            if let Some(i) = current {
                waited[i] += 1;
            }
            // A page that waits for the button is answered.
            if field.waiting() {
                field.hold(KEY_A);
            } else if f > 6 {
                field.hold(0);
            }
            let mut c = canvas(w, hgt, BACK);
            paste(&mut c, &screen(&field.picture()), 4, 4, 1);
            let x = (WIDTH + 16) as i32;
            text(&mut c, x, 6, &format!("{EVENT}:"), HEAD, None, 2);
            for (i, line) in shown.iter().enumerate() {
                let y = 26 + 36 * i as i32;
                let on = current == Some(i);
                text(&mut c, x, y, line, if on { 0x10_10_10 } else { INK }, if on { Some(MARK) } else { None }, 2);
                if waited[i] > 0 {
                    text(&mut c, x + 16, y + 16, &format!("{} FRAMES", waited[i]), if on { MARK } else { DIM }, None, 2);
                }
            }
            frames.push(c.pixels);
            if done {
                break;
            }
        }
        field.probe = None;
        if !done {
            return Err("the script never ran its last call".to_string());
        }
        let last = frames.last().cloned().unwrap_or_default();
        frames.extend(std::iter::repeat_n(last, 60));
        let written = animation::write(&self.out.join("scene-script.gif"), &frames, w)?;
        self.written.push(("scene-script.gif".to_string(), written.bytes, format!("{EVENT} beside the frames each command waits; {} frames", written.frames)));
        Ok(())
    }

    /// The scene and place flags across two entrances, as
    /// `temporary-flags.png`: in Vale every one of FLAG_SCENE_TEMP_00 to
    /// FLAG_AREA_TEMP_FF is set by hand; the party is sent to the Sanctum,
    /// a scene of the same place, then to the world map, another place, and
    /// back into Vale (Event_SetDestination's way); each time the 512 flags are drawn as
    /// two grids of 16 by 16, lit where set, beside the screen.
    pub fn temporary_flags(&mut self) -> Result<(), String> {
        const FIRST: u32 = 0x200;
        const CELL: usize = 7;
        let flags = self.sym("gGameFlagBytes")?;
        let mut entered = self.enter_with(VALE, &[])?;
        let field = &mut entered.field;
        for byte in FIRST / 8..0x400 / 8 {
            field.write8(flags + byte, 0xff);
        }
        let mut states = vec![("VALE, ALL SET BY HAND", screen(&field.picture()), self.temp_flags(field, flags))];
        self.travel(field, (9, 1), 300)?;
        states.push(("THE SANCTUM, THE SAME PLACE", screen(&field.picture()), self.temp_flags(field, flags)));
        self.travel(field, (2, 1), 300)?;
        states.push(("THE WORLD MAP, ANOTHER PLACE", screen(&field.picture()), self.temp_flags(field, flags)));
        self.travel(field, VALE, 300)?;
        states.push(("VALE AGAIN", screen(&field.picture()), self.temp_flags(field, flags)));
        let scene_set = |s: &[bool]| s[..256].iter().filter(|&&b| b).count();
        let place_set = |s: &[bool]| s[256..].iter().filter(|&&b| b).count();
        if scene_set(&states[1].2) > 16 || place_set(&states[1].2) < 240 || place_set(&states[2].2) > 16 {
            return Err(format!("set after each entrance: {:?}", states.iter().map(|s| (scene_set(&s.2), place_set(&s.2))).collect::<Vec<_>>()));
        }
        let col = WIDTH + 16;
        let grid = 16 * CELL;
        let mut image = canvas(4 * col + 8, 8 + HEIGHT + 30 + grid + 76, BACK);
        for (n, (label, shot, set)) in states.iter().enumerate() {
            let x = 8 + n * col;
            paste(&mut image, shot, x, 8, 1);
            text(&mut image, x as i32, (HEIGHT + 14) as i32, label, HEAD, None, 2);
            for half in 0..2usize {
                let gx = x + half * (grid + 8);
                let gy = HEIGHT + 34;
                for i in 0..256usize {
                    let on = set[half * 256 + i];
                    let colour = if on { [0x60_b0_ff, 0xff_b0_50][half] } else { 0x30_32_3a };
                    let (cx, cy) = (gx + (i % 16) * CELL, gy + (i / 16) * CELL);
                    for dy in 0..CELL - 1 {
                        for dx in 0..CELL - 1 {
                            image.pixels[(cy + dy) * image.width + cx + dx] = colour;
                        }
                    }
                }
                let count = set[half * 256..half * 256 + 256].iter().filter(|&&b| b).count();
                text(&mut image, gx as i32, (gy + grid + 6) as i32, &format!("{} {count}", ["MAP", "AREA"][half]), [0x60_b0_ff, 0xff_b0_50][half], None, 2);
            }
        }
        let y = (HEIGHT + 34 + grid + 26) as i32;
        text(&mut image, 8, y, "FLAG_SCENE_TEMP_00 TO _FF (BLUE) AND FLAG_AREA_TEMP_00 TO _FF (ORANGE),", DIM, None, 2);
        text(&mut image, 8, y + 16, "A SQUARE A FLAG, LIT WHERE SET, EACH AS THE PARTY HAS CONTROL AGAIN", DIM, None, 2);
        self.guide_png("temporary-flags.png", &image, "the map and map-section flags as the party enters a map of the same section and then another section")
    }

    /// Flags FLAG_SCENE_TEMP_00 to FLAG_AREA_TEMP_FF, whether each is set.
    fn temp_flags(&self, field: &Field, flags: u32) -> Vec<bool> {
        (0x200..0x400u32).map(|f| field.read8(flags + f / 8) & (1 << (f % 8)) != 0).collect()
    }

    /// Two Colosso final matches' overlays, Azart's (resource 0x3ba) and
    /// Navampa's (0x3bc), drawn a byte to a pixel, 128 to a line, as
    /// `overlay-common.png`: each from its unpacked build output, grey by
    /// value, and every function or table both define under one name with
    /// the same bytes, the copies shared/ headers place in each, tinted in a
    /// colour of its own, the same in both. Names and sizes come from each
    /// overlay's ELF.
    pub fn overlay_common(&mut self) -> Result<(), String> {
        const PAIR: [(&str, &str); 2] = [("resource_3ba", "AZART'S MATCH"), ("resource_3bc", "NAVAMPA'S MATCH")];
        const LINE: usize = 128;
        const SCALE: usize = 2;
        let dir = crate::compile::root().join("out/tbs-en/overlays");
        let mut overlays = Vec::new();
        for (name, _) in PAIR {
            let bytes = std::fs::read(dir.join(format!("{name}.bin"))).map_err(|e| format!("{name}.bin: {e}"))?;
            let data = std::fs::read(dir.join(format!("{name}.elf"))).map_err(|e| format!("{name}.elf: {e}"))?;
            let elf = crate::elf::Elf::parse(&data)?;
            // Functions and tables with a size, by name: their offset in the
            // overlay and their bytes.
            let mut things: std::collections::BTreeMap<String, (usize, Vec<u8>)> = std::collections::BTreeMap::new();
            for s in elf.symbols.iter().filter(|s| s.size > 0 && (s.kind == 1 || s.kind == 2) && s.value & !1 >= 0x0200_8000) {
                let at = ((s.value & !1) - 0x0200_8000) as usize;
                if at + s.size as usize <= bytes.len() {
                    things.insert(s.name.clone(), (at, bytes[at..at + s.size as usize].to_vec()));
                }
            }
            overlays.push((bytes, things));
        }
        // The same as `common` with every Thumb BL pair's offset ignored.
        let unbl = |b: &[u8]| -> Vec<u16> {
            let mut h: Vec<u16> = b.chunks(2).map(|c| u16::from(c[0]) | u16::from(*c.get(1).unwrap_or(&0)) << 8).collect();
            for i in 0..h.len().saturating_sub(1) {
                if h[i] & 0xf800 == 0xf000 && h[i + 1] & 0xf800 == 0xf800 {
                    h[i] = 0xf000;
                    h[i + 1] = 0xf800;
                }
            }
            h
        };
        // A copy in both: one name and one size in each, from a shared/
        // header (the compiler library's _call_via_ stubs are linked into
        // every overlay and left out). It differs only where it holds an
        // address when every halfword that differs, BL offsets aside, is
        // in an aligned word that is an address in RAM or the ROM in both.
        let is_address = |w: u32| matches!(w >> 24, 0x02 | 0x03 | 0x08);
        let headers: String = std::fs::read_dir(crate::compile::root().join("games/tbs/src/scene/shared")).or_else(|_| std::fs::read_dir(crate::compile::root().join("games/tbs/src/scene/common"))).map_err(|e| format!("shared/: {e}"))?.flatten().filter_map(|e| std::fs::read_to_string(e.path()).ok()).collect();
        let copies: Vec<String> = overlays[0].1.iter().filter(|(n, (_, b))| headers.contains(n.as_str()) && !n.starts_with("_call_via") && overlays[1].1.get(*n).is_some_and(|(_, c)| c.len() == b.len())).map(|(n, _)| n.clone()).collect();
        let mut identical = 0;
        let mut addresses_only = 0;
        for n in &copies {
            let (a0, a) = &overlays[0].1[n];
            let (b0, b) = &overlays[1].1[n];
            if a == b {
                identical += 1;
                continue;
            }
            let (ha, hb) = (unbl(a), unbl(b));
            let word = |bytes: &Vec<u8>, base: usize, i: usize| {
                let at = (base + 2 * i) & !3;
                let o = at - base.min(at);
                let full = &overlays[if std::ptr::eq(bytes, a) { 0 } else { 1 }].0;
                let _ = o;
                u32::from_le_bytes([full[at], full[at + 1], full[at + 2], full[at + 3]])
            };
            if (0..ha.len()).filter(|&i| ha[i] != hb[i]).all(|i| is_address(word(a, *a0, i)) && is_address(word(b, *b0, i))) {
                addresses_only += 1;
            } else {
                eprintln!("overlay-common: {n} differs beyond its addresses");
            }
        }
        if identical + addresses_only != copies.len() {
            return Err("a copy differs beyond its addresses".to_string());
        }
        let common = copies;
        let shared_bytes: usize = common.iter().map(|n| overlays[0].1[n].1.len()).sum();
        let rows = overlays.iter().map(|o| o.0.len().div_ceil(LINE)).max().unwrap_or(0);
        let panel = LINE * SCALE + 24;
        let (w, h) = (8 + 2 * panel + 8 + 40, 30 + rows * SCALE + 60);
        let mut image = canvas(w, h, BACK);
        for (k, (bytes, things)) in overlays.iter().enumerate() {
            let x0 = 8 + k * panel;
            let mut tint: Vec<Option<u32>> = vec![None; bytes.len()];
            for (i, n) in common.iter().enumerate() {
                let (at, b) = &things[n];
                for t in tint.iter_mut().skip(*at).take(b.len()) {
                    *t = Some(hue(i));
                }
            }
            for (i, &v) in bytes.iter().enumerate() {
                let g = u32::from(v);
                let grey = g << 16 | g << 8 | g;
                let colour = match tint[i] {
                    Some(c) => mix(grey, c, 0.7),
                    None => mix(grey, 0, 0.35),
                };
                let (x, y) = (x0 + (i % LINE) * SCALE, 30 + (i / LINE) * SCALE);
                for d in 0..SCALE * SCALE {
                    image.pixels[(y + d / SCALE) * w + x + d % SCALE] = colour;
                }
            }
            text(&mut image, x0 as i32, 8, &format!("{} ({} BYTES)", PAIR[k].1, bytes.len()), HEAD, None, 2);
        }
        let y = (30 + rows * SCALE + 8) as i32;
        text(&mut image, 8, y, &format!("TINTED: {} FUNCTIONS AND TABLES FROM SHARED/, {shared_bytes} BYTES", common.len()), INK, None, 2);
        text(&mut image, 8, y + 16, &format!("OF EACH: {identical} BYTE FOR BYTE, {addresses_only} DIFFERING ONLY IN ADDRESSES"), INK, None, 2);
        text(&mut image, 8, y + 32, "GREY: EACH OVERLAY'S OWN CODE AND TABLES", DIM, None, 2);
        self.guide_png("overlay-common.png", &image, "two Colosso match overlays a byte to a pixel, the code both carry from shared/ tinted")
    }

    /// The register write queue through the first three frames of the title
    /// intro's palette fade, as `io-write-queue.png`: the queue's 32 slots
    /// down the side and the scanlines along the bottom, three frames of
    /// 228 lines (160 drawn, then 68 of VBlank), each slot a bar from the
    /// line its entry was pushed to the line IoWriteQueue_FlushPending
    /// emptied it, coloured and labelled by what the entry does. The count
    /// (gIoWriteQueue.count) and VCOUNT are watched an instruction at a
    /// time; the entries are read as FlushPending is entered.
    pub fn io_write_queue(&mut self) -> Result<(), String> {
        let queue = self.sym("gIoWriteQueue")?;
        let flush = self.sym("IoWriteQueue_FlushPending")?;
        let start = self.sym("TitlePaletteFade_Start")?;
        let setup = super::field::Setup { scene: 0, entrance: 0, set: Vec::new(), clear: Vec::new() };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        let flushes = Rc::new(RefCell::new(Vec::new()));
        let started = Rc::new(RefCell::new(false));
        let (f, s) = (flushes.clone(), started.clone());
        field.probe_pcs = vec![flush, start];
        field.probe = Some(Box::new(move |gba, pc, _| {
            if pc == start {
                *s.borrow_mut() = true;
            } else if pc == flush && *s.borrow() {
                let n = u32::from(gba.read16(queue)).min(32);
                let entries: Vec<(u32, u32, u32)> = (0..n).map(|i| (gba.read32(queue + 4 + 12 * i), gba.read32(queue + 8 + 12 * i), gba.read32(queue + 12 + 12 * i))).collect();
                f.borrow_mut().push(entries);
            }
        }));
        let mut n = 0;
        while !*started.borrow() {
            field.frames(1)?;
            n += 1;
            if n > 3000 {
                return Err("the intro's fade never started".to_string());
            }
        }
        // From the VBlank before the frame TitlePaletteFade_Start ran in: its
        // pushes so far are kept in `already`.
        let already = field.read16(queue);
        let changes = field.watch_halfwords(&[queue, 0x0400_0006], 3)?;
        field.probe = None;
        let flushes = flushes.borrow();
        // Each slot's bars: (first line, last line, entry), lines counted
        // from the first change watched.
        const LINES: i32 = 228;
        let first_line = i32::from(changes[0].1[1]);
        let mut line = 0i32;
        let mut last_vcount = changes[0].1[1];
        let mut open: Vec<i32> = vec![0; usize::from(already)];
        let mut bars: Vec<(usize, i32, i32, (u32, u32, u32))> = Vec::new();
        let mut empties = Vec::new();
        let mut count = already;
        let mut flushed = 0usize;
        for (_, values, _) in &changes[1..] {
            let (now, vcount) = (values[0], values[1]);
            line += (i32::from(vcount) - i32::from(last_vcount)).rem_euclid(LINES);
            last_vcount = vcount;
            if now > count {
                open.extend(std::iter::repeat_n(line, usize::from(now - count)));
            } else if now < count {
                let entries = flushes.get(flushed).ok_or("a flush the probe did not see")?;
                if entries.len() != open.len() {
                    return Err(format!("{} entries flushed, {} pushed", entries.len(), open.len()));
                }
                for (slot, (&from, &entry)) in open.iter().zip(entries).enumerate() {
                    bars.push((slot, from, line, entry));
                }
                empties.push(line);
                open.clear();
                flushed += 1;
            }
            count = now;
        }
        if flushed < 2 {
            return Err(format!("only {flushed} flushes were seen"));
        }
        // The slots any entry used, and one row more to stand for the rest.
        let used = bars.iter().map(|b| b.0).max().unwrap_or(0) + 1;
        let (left, top, row) = (96i32, 40i32, 30i32);
        let width = left + line + 16;
        let height = top + row * (used as i32 + 1) + 88;
        let mut image = canvas(width as usize, height as usize, BACK);
        // VBlank bands: VCOUNT 160 to 227.
        for x in 0..line {
            let vcount = (first_line + x).rem_euclid(LINES);
            let shade = if vcount >= 160 { 0x3a_34_48 } else { 0x26_28_30 };
            for y in top..top + row * used as i32 {
                image.pixels[(y * width + left + x) as usize] = shade;
            }
            if vcount == 160 {
                text(&mut image, left + x + 3, top - 18, "VBLANK", DIM, None, 2);
            }
        }
        for slot in 0..used {
            text(&mut image, 4, top + row * slot as i32 + 8, &format!("SLOT {slot}"), DIM, None, 2);
        }
        text(&mut image, 4, top + row * used as i32 + 8, &format!("SLOTS {used} TO 31: NEVER USED HERE"), DIM, None, 2);
        for &(slot, from, to, (_, address, control)) in &bars {
            let (colour, label) = match (address, control >> 16 & 3) {
                (0x0500_0000, 0) => (0x60_b0_ff, "DMA: BACKGROUND COLOURS"),
                (0x0500_0200, 0) => (0xff_90_60, "DMA: SPRITE COLOURS"),
                (0x0400_0000, w) if w != 0 => (0x70_e0_80, "WRITE: DISPCNT"),
                _ => (0xa0_a0_a0, "OTHER"),
            };
            let y = top + row * slot as i32;
            for yy in y + 2..y + row - 2 {
                for x in left + from..left + to {
                    image.pixels[(yy * width + x) as usize] = colour;
                }
            }
            // As much of the label as fits in the bar.
            let fits = ((to - from - 4) / 8).max(0) as usize;
            let label: String = label.chars().take(fits).collect();
            text(&mut image, left + from + 2, y + 8, &label, BACK, None, 2);
        }
        for &x in &empties {
            for d in 0..2 {
                outline(&mut image, left + x + d, top - 4, 1, row * used as i32 + 8, MARK);
            }
        }
        let y = top + row * (used as i32 + 1) + 8;
        text(&mut image, 4, y, "THREE FRAMES OF THE TITLE INTRO, A PIXEL A SCANLINE.", INK, None, 2);
        text(&mut image, 4, y + 18, "A BAR STARTS WHERE ITS ENTRY IS PUSHED AND ENDS AT THE", INK, None, 2);
        text(&mut image, 4, y + 36, "YELLOW LINE, THE VBLANK THAT CARRIES IT OUT AND EMPTIES THE QUEUE.", INK, None, 2);
        text(&mut image, 4, y + 54, "BLUE AND ORANGE: THE FADE'S COLOURS, GREEN: A REGISTER WRITE.", INK, None, 2);
        self.guide_png("io-write-queue.png", &image, "the register write queue filling through three frames of the title intro and emptied at each VBlank")
    }

    /// The resource directory's rows drawn against the ROM, as
    /// `resource-directory.png`: the 8 MB cartridge a pixel to 8 bytes,
    /// 8 KB to a line, each row's data coloured by its kind (an overlay, a
    /// map, a map's tiles or palette, the main image's own tables, any other
    /// picture or table) from the address the row holds to the next symbol
    /// after it; what no row points into is grey where the link placed
    /// something and black where it placed nothing. Read from the built ROM
    /// and its link map.
    pub fn resource_directory(&mut self) -> Result<(), String> {
        let directory = self.sym("Resource_DirectoryTable")?;
        let stamp = self.sym("Resource_BuildStamp")?;
        let rows = (stamp - directory) / 4;
        let word = |a: u32| {
            let i = (a - 0x0800_0000) as usize;
            u32::from_le_bytes([self.rom[i], self.rom[i + 1], self.rom[i + 2], self.rom[i + 3]])
        };
        const KINDS: [(&str, u32); 6] = [
            ("OVERLAYS", 0xff_70_50),
            ("MAPS", 0x40_c8_70),
            ("MAP TILES AND PALETTES", 0x30_80_e0),
            ("THE MAIN IMAGE'S OWN TABLES", 0xf0_e0_40),
            ("OTHER PICTURES AND TABLES", 0xc0_70_e0),
            ("UNUSED ROWS", 0),
        ];
        let kind_of = |name: &str| -> usize {
            if name.starts_with("Resource_Overlay") {
                0
            } else if name.starts_with("Field_Map") {
                1
            } else if name.starts_with("Tileset_") {
                2
            } else if name.starts_with("FarCall_") || name == "Rom_Start" || name == "Resource_DirectoryTable" || name == "Resource_BuildStamp" {
                3
            } else {
                4
            }
        };
        const SPAN: usize = 8 << 20;
        const PER: usize = 8;
        let mut owner: Vec<Option<usize>> = vec![None; SPAN / PER];
        let mut counts = [0usize; 6];
        let mut sizes = [0usize; 6];
        for row in 0..rows {
            let place = word(directory + 4 * row);
            if place == 0 {
                counts[5] += 1;
                continue;
            }
            let p = self.map.at_address(place).ok_or_else(|| format!("row {row}'s {place:#x} is in nothing the map placed"))?;
            // The kind any name at the place gives, an overlay or a map
            // before the rest.
            let kind = p.symbols.iter().filter(|s| s.0 == place).map(|s| kind_of(&s.1)).min().unwrap_or(4);
            counts[kind] += 1;
            // A table of the main image is drawn whole, as the link placed its
            // object; anything else up to the next name after it.
            let end = if kind == 3 { p.end() } else { p.symbols.iter().map(|s| s.0).filter(|&a| a > place).min().unwrap_or(p.end()).min(p.end()) };
            sizes[kind] += (end - place) as usize;
            for a in (place..end).step_by(PER) {
                let i = (a - 0x0800_0000) as usize / PER;
                if i < owner.len() && owner[i].is_none() {
                    owner[i] = Some(kind);
                }
            }
        }
        let mut placed = vec![false; SPAN / PER];
        for p in self.map.placed.iter().filter(|p| !p.is_fill() && p.size > 0 && p.address >= 0x0800_0000 && p.address < 0x0880_0000) {
            for a in (p.address..p.end().min(0x0880_0000)).step_by(PER) {
                placed[(a - 0x0800_0000) as usize / PER] = true;
            }
        }
        const LINE: usize = 1024;
        let (left, top) = (120usize, 8usize);
        let lines = SPAN / PER / LINE;
        let mut image = canvas(left + LINE + 16 + 420, top + lines + 8, BACK);
        for i in 0..SPAN / PER {
            let colour = match owner[i] {
                Some(k) => KINDS[k].1,
                None if placed[i] => 0x5a_5c_64,
                None => 0x08_08_0a,
            };
            image.pixels[(top + i / LINE) * image.width + left + i % LINE] = colour;
        }
        for mb in 0..=8usize {
            let y = (top + mb * (1 << 20) / PER / LINE) as i32;
            if mb < 8 {
                text(&mut image, 8, y, &format!("{:#010X}", 0x0800_0000 + (mb << 20)), DIM, None, 2);
            }
            outline(&mut image, left as i32 - 6, y, 5, 1, DIM);
        }
        let x = (left + LINE + 16) as i32;
        let mut y = top as i32;
        text(&mut image, x, y, &format!("{rows} DIRECTORY ROWS", ), HEAD, None, 2);
        y += 24;
        for (k, (name, colour)) in KINDS.iter().enumerate() {
            if k < 5 {
                for dy in 0..12 {
                    for dx in 0..12 {
                        image.pixels[(y as usize + dy) * image.width + x as usize + dx] = *colour;
                    }
                }
            }
            text(&mut image, x + 18, y, name, INK, None, 2);
            let detail = if k == 5 { format!("{} ROWS HOLDING 0", counts[k]) } else if k == 3 { format!("{} ROWS", counts[k]) } else { format!("{} ROWS, {} KB", counts[k], sizes[k].div_ceil(1024)) };
            text(&mut image, x + 18, y + 16, &detail, DIM, None, 2);
            y += 40;
        }
        for (colour, name) in [(0x5a_5c_64, "NO ROW POINTS HERE:"), (0x08_08_0a, "NOTHING PLACED")] {
            for dy in 0..12 {
                for dx in 0..12 {
                    image.pixels[(y as usize + dy) * image.width + x as usize + dx] = colour;
                }
            }
            outline(&mut image, x, y, 12, 12, DIM);
            text(&mut image, x + 18, y, name, INK, None, 2);
            if colour != 0x08_08_0a {
                text(&mut image, x + 18, y + 16, "THE MAIN IMAGE'S CODE AND DATA", DIM, None, 2);
            }
            y += 40;
        }
        text(&mut image, x, y + 8, "ONE PIXEL IS 8 BYTES,", DIM, None, 2);
        text(&mut image, x, y + 24, "ONE LINE 8 KB", DIM, None, 2);
        let total: usize = counts.iter().sum();
        if total != rows as usize {
            return Err(format!("{total} rows counted of {rows}"));
        }
        self.guide_png("resource-directory.png", &image, "the ROM a pixel to 8 bytes, coloured by the kind of resource each directory row points at")
    }

    /// EWRAM and IWRAM drawn a pixel to a byte, 512 bytes to a line, beside
    /// the screen, as `memory-maps.gif`: first as the party walks from Vale
    /// into the Sanctum and the scene loads, then as Isaac walks Goma Cave
    /// until a random battle opens. Each byte is a grey of its value tinted
    /// with its area's colour (the EWRAM table and the IWRAM table of the
    /// System guide, from the link map's names), and each heap's blocks in
    /// use, from its start to the next free address in gHeapSlots's first
    /// two words, are tinted brighter than the free rest, with a white line
    /// at the next free address.
    pub fn memory_maps(&mut self) -> Result<(), String> {
        let work_slot = self.sym("gHeapSlots")?;
        let ewram_heap = self.sym("gEwramHeap")?;
        let iwram_heap = self.sym("gIwramHeap")?;
        // EWRAM's areas, from ewram.s's names.
        let ewram_areas: Vec<(u32, u32, &str, u32)> = vec![
            (0x0200_0000, self.sym("gOverlayArea")?, "MAIN IMAGE'S VARIABLES", 0x90_90_90),
            (self.sym("gOverlayArea")?, self.sym("gMapGridCellBuffer")?, "OVERLAY AREA", 0xff_70_50),
            (self.sym("gMapGridCellBuffer")?, self.sym("gActorSpriteSlots")?, "MAP CELLS", 0x40_c8_70),
            (self.sym("gActorSpriteSlots")?, self.sym("gDecodeBuffer")?, "ACTOR SPRITE SLOTS", 0xe0_a0_e0),
            (self.sym("gDecodeBuffer")?, self.sym("gMetatiles")?, "DECODE BUFFER", 0xe0_e0_60),
            (self.sym("gMetatiles")?, self.sym("gMapGridAnimTiles")?, "MAP BLOCKS", 0x30_b0_a0),
            (self.sym("gMapGridAnimTiles")?, self.sym("gMapGridShapes")?, "MAP ANIM TILES", 0x80_d0_ff),
            (self.sym("gMapGridShapes")?, ewram_heap, "MAP COLLISION", 0x60_e0_40),
            (ewram_heap, self.sym("gBgTileBuffer")?, "EWRAM HEAP", 0x40_80_ff),
            (self.sym("gBgTileBuffer")?, 0x0204_0000, "BG TILE BUFFER", 0xa0_60_ff),
        ];
        let iwram_areas: Vec<(u32, u32, &str, u32)> = vec![
            (0x0300_0000, 0x0300_1400, "RESIDENT RUNTIME", 0xf0_e0_40),
            (0x0300_1400, iwram_heap, "IWRAM VARIABLES", 0x90_90_90),
            (iwram_heap, self.sym("SoundMainRAM")?, "IWRAM HEAP", 0x40_80_ff),
            (self.sym("SoundMainRAM")?, 0x0300_7800, "SOUND MIXER", 0xff_b0_50),
            (0x0300_7800, 0x0300_8000, "STACKS", 0xff_70_50),
        ];
        // Each heap's range: the EWRAM heap runs to the end of EWRAM, over
        // the BG tile buffer, and the IWRAM heap to Ram_IwramHeapEnd, over
        // the mixer.
        let heaps = [(ewram_heap, 0x0204_0000u32, 0u32), (iwram_heap, 0x0300_7800, 4)];
        let (sx, ex) = (8usize, 8 + 2 * WIDTH + 16);
        let (ew_y, iw_y) = (24usize, 24 + 2 * HEIGHT + 64);
        let (w, h) = (ex + 512 + 12 + 280, ew_y + 512 + 12);
        // Each memory: its base, size, areas, place and bytes a line.
        let memories = [(0x0200_0000u32, 0x40000u32, &ewram_areas, (ex, ew_y), 512usize, "EWRAM, 256 KB, 512 BYTES A LINE"), (0x0300_0000, 0x8000, &iwram_areas, (sx, iw_y), 256, "IWRAM, 32 KB, 256 BYTES A LINE")];
        let draw = |field: &Field, label: &str| -> Vec<u32> {
            let mut c = canvas(w, h, BACK);
            paste(&mut c, &screen(&field.picture()), sx, ew_y, 2);
            text(&mut c, sx as i32, (ew_y + 2 * HEIGHT + 8) as i32, label, INK, None, 2);
            for &(base, size, areas, (mx, top), line, title) in &memories {
                let (heap_start, heap_end, slot) = heaps[usize::from(base >= 0x0300_0000)];
                let next = field.read32(work_slot + slot);
                let mut area = 0;
                for i in 0..size / 4 {
                    let word = field.read32(base + 4 * i);
                    for b in 0..4u32 {
                        let a = base + 4 * i + b;
                        while a >= areas[area].1 {
                            area += 1;
                        }
                        let v = (word >> (8 * b)) & 0xff;
                        let grey = v << 16 | v << 8 | v;
                        let free = a >= heap_start && a < heap_end && a >= next;
                        let mut colour = mix(grey, areas[area].3, if free { 0.2 } else { 0.45 });
                        if free {
                            colour = mix(colour, 0, 0.55);
                        }
                        let n = (a - base) as usize;
                        c.pixels[(top + n / line) * w + mx + n % line] = colour;
                    }
                }
                // The next free address: white from it to the end of its
                // line, and a tick above and below it.
                if next >= heap_start && next < heap_end {
                    let n = (next - base) as usize;
                    let y = top + n / line;
                    for x in n % line..line {
                        c.pixels[y * w + mx + x] = 0xff_ff_ff;
                    }
                    outline(&mut c, (mx + n % line) as i32 - 1, y as i32 - 4, 3, 9, 0xff_ff_ff);
                }
                for &(from, to, name, colour) in areas.iter() {
                    let y = top + ((from - base) as usize) / line;
                    let rows = ((to - from) as usize).div_ceil(line);
                    outline(&mut c, (mx + line + 4) as i32, y as i32, 3, rows as i32, colour);
                    let scale = if rows >= 14 { 2 } else { 1 };
                    text(&mut c, (mx + line + 10) as i32, y as i32 + (rows as i32 / 2 - 3 * scale).max(0), name, colour, None, scale);
                }
                text(&mut c, mx as i32, top as i32 - 18, title, HEAD, None, 2);
            }
            c.pixels
        };
        let mut frames: Vec<Vec<u32>> = Vec::new();
        // The scene load: Vale to the Sanctum, a sample every fourth frame,
        // each kept twice so the GIF's 30 a second shows it.
        let mut entered = self.enter_with(VALE, &[])?;
        let field = &mut entered.field;
        for (keys, n) in [(0u32, 20u32), (KEY_UP, 200), (0, 100)] {
            field.hold(keys);
            for i in 0..n {
                field.frames(1)?;
                if i % 4 == 0 {
                    let label = if field.scene() == VALE.0 { "VALE" } else { "LOADING THE SANCTUM" };
                    let f = draw(field, label);
                    frames.push(f.clone());
                    frames.push(f);
                }
            }
        }
        field.hold(0);
        if field.scene() == VALE.0 {
            return Err("the walk north never left Vale".to_string());
        }
        let mut more = self.memory_battle(&draw)?;
        frames.append(&mut more);
        let written = animation::write(&self.out.join("memory-maps.gif"), &frames, w)?;
        self.written.push(("memory-maps.gif".to_string(), written.bytes, format!("EWRAM and IWRAM a pixel to a byte through a scene load and a battle; {} frames", written.frames)));
        Ok(())
    }

    /// Goma Cave, as the party first reaches it, walked back and forth along
    /// the stretch random-battle.gif walks (Guide::goma_cave_stretch) until a
    /// random battle opens, drawn with `draw` a sample every fourth frame
    /// from two seconds before the battle to five after.
    fn memory_battle(&self, draw: &dyn Fn(&Field, &str) -> Vec<u32>) -> Result<Vec<Vec<u32>>, String> {
        let (place, stretch) = self.goma_cave_stretch()?;
        let mut entered = self.enter_goma_cave(place)?;
        let field = &mut entered.field;
        let mut kept: Vec<Vec<u32>> = Vec::new();
        let mut frame = 0u32;
        let mut sample = |field: &Field| {
            frame += 1;
            if frame % 4 == 0 {
                kept.push(draw(field, "GOMA CAVE"));
                if kept.len() > 30 {
                    kept.remove(0);
                }
            }
        };
        Self::walk_stretch(field, &stretch, 80, &mut Vec::new(), &mut 0, &mut sample)?.ok_or("no battle came in Goma Cave")?;
        let mut frames: Vec<Vec<u32>> = kept.into_iter().flat_map(|f| [f.clone(), f]).collect();
        for i in 0..300u32 {
            field.frames(1)?;
            if i % 4 == 0 {
                let f = draw(field, "A BATTLE");
                frames.push(f.clone());
                frames.push(f);
            }
        }
        Ok(frames)
    }

    /// The map's whole cell grid as the three layers' windows show it, each
    /// cell drawn from its block and the tiles video memory holds, at full
    /// size: the window of layer n (0 the ground on BG3, 1 the middle on
    /// BG2, 2 the overhead on BG1) starts at the cell its MapGridLayerScroll's
    /// cells pointer names and spans `spans[n]` cells. Returns the picture
    /// and each window's first cell.
    fn grid_picture(field: &Field, spans: [(i32, i32); 3]) -> (Image, [(i32, i32); 3]) {
        const MAPGRID_CELLS: u32 = 0x0201_0000;
        const MAP_BLOCKS: u32 = 0x0202_0000;
        const GRID: i32 = 128;
        let colours = super::guide_more::palettes(field);
        let work = field.map_work();
        let mut windows = [(0, 0); 3];
        let mut image = canvas((GRID * 16) as usize, (GRID * 16) as usize, 0x10_10_14);
        for layer in 0..3u32 {
            let cells = field.read32(work + 0x104 + 0x30 * layer + 0x2c);
            let index = ((cells - MAPGRID_CELLS) / 4) as i32;
            let (wx, wz) = (index % GRID, index / GRID);
            windows[layer as usize] = (wx, wz);
            let control = field.read16(0x0400_0008 + 2 * (3 - layer));
            let (char_base, wide) = (u32::from((control >> 2) & 3), control & 0x80 != 0);
            let (sw, sh) = spans[layer as usize];
            for cz in wz..(wz + sh).min(GRID) {
                for cx in wx..(wx + sw).min(GRID) {
                    let block = field.read32(MAPGRID_CELLS + 4 * (cx + cz * GRID) as u32) & 0xfff;
                    for q in 0..4u32 {
                        let entry = field.read16(MAP_BLOCKS + block * 8 + 2 * q);
                        let size = if wide { 64 } else { 32 };
                        let address = 0x0600_0000 + char_base * 0x4000 + u32::from(entry & 0x3ff) * size;
                        let pal: Vec<u32> = if wide { colours[..256].to_vec() } else { let p = usize::from(entry >> 12); colours[p * 16..p * 16 + 16].to_vec() };
                        let pixels = super::guide_more::tile(field, address, wide, &pal, 0x10_10_14);
                        super::guide_more::put_tile(&mut image, (cx * 16) as usize + (q as usize % 2) * 8, (cz * 16) as usize + (q as usize / 2) * 8, &pixels, entry & 0x400 != 0, entry & 0x800 != 0);
                    }
                }
            }
        }
        (image, windows)
    }

    /// Suhalla Desert's north, whose header gives the overhead layer a
    /// parallax of 20 sixteenths and a drift, as `map-header.gif`: the
    /// screen as the leader walks, beside the map's cell grid at half size
    /// with each layer's window (its origin from the header, its size the
    /// camera's bounds, or the wrap period of the drifting layer) outlined
    /// in its colour and the part of it on screen this frame
    /// (MapGridLayerScroll's x and y) drawn as a box, wrapped as the layer
    /// wraps. The header's values are read from the ROM through the scene's
    /// row of gMaps and the directory.
    pub fn map_header(&mut self) -> Result<(), String> {
        const SUHALLA_DESERT_NORTH: u16 = 164;
        let scenes = self.sym("gScenes")?;
        let layered = self.sym("gMaps")?;
        let directory = self.sym("Resource_DirectoryTable")?;
        let map_001 = 0x128u32;
        let rom = |a: u32| self.rom[(a - 0x0800_0000) as usize];
        let r16 = |a: u32| u16::from(rom(a)) | u16::from(rom(a + 1)) << 8;
        let r32 = |a: u32| u32::from(r16(a)) | u32::from(r16(a + 2)) << 16;
        let map = u32::from(r16(scenes + 8 * u32::from(SUHALLA_DESERT_NORTH) + 4));
        let header = r32(directory + 4 * (map_001 + u32::from(r16(layered + 12 * map))));
        let bounds: Vec<i32> = (0..4).map(|i| i32::from(rom(header + i))).collect();
        let layers: Vec<[i32; 8]> = (0..3).map(|l| {
            let mut v = [0i32; 8];
            for (i, x) in v.iter_mut().enumerate() {
                let b = rom(header + 12 + 8 * l + i as u32);
                *x = if (2..6).contains(&i) { i32::from(b as i8) } else { i32::from(b) };
            }
            v
        }).collect();
        // With the storms already risen, so the sand haze on BG1 is up.
        let mut entered = self.enter_with((SUHALLA_DESERT_NORTH, 1), &["FLAG_SUHALLA_DESERT_SANDSTORMS_RISEN"])?;
        let field = &mut entered.field;
        // Each window in cells: the bounds, or the wrap period when the
        // layer wraps sooner.
        let (bw, bh) = ((bounds[2] - bounds[0]) / 2, (bounds[3] - bounds[1]) / 2);
        let spans: [(i32, i32); 3] = std::array::from_fn(|l| ((layers[l][6] + 1).min(bw * 2) / 2, (layers[l][7] + 1).min(bh * 2) / 2));
        let (grid, windows) = Self::grid_picture(field, spans);
        // The part of the grid the windows take, at half size.
        let x1 = (0..3).map(|l| windows[l].0 + spans[l].0).max().unwrap_or(0);
        let z1 = (0..3).map(|l| windows[l].1 + spans[l].1).max().unwrap_or(0);
        let (gw, gh) = ((x1 * 8) as usize, (z1 * 8) as usize);
        let mut half = canvas(gw, gh, 0);
        for y in 0..gh {
            for x in 0..gw {
                half.pixels[y * gw + x] = grid.pixels[2 * y * grid.width + 2 * x];
            }
        }
        const COLOURS: [u32; 3] = [0xff_ff_ff, 0x40_e0_60, 0xff_a0_30];
        let names = ["GROUND, BG3", "MIDDLE, BG2", "OVERHEAD, BG1"];
        let (gx, gy) = (8 + WIDTH + 16, 8usize);
        let (w, h) = (gx + gw + 8, gy + gh + 8 + 3 * 52);
        let mut frames = Vec::new();
        let work_at = |field: &Field, l: u32, o: u32| field.read32(field.map_work() + 0x104 + 0x30 * l + o) as i32 >> 16;
        for (keys, n) in [(0u32, 20u32), (KEY_RIGHT, 50), (KEY_DOWN, 50), (KEY_LEFT, 40)] {
            field.hold(keys);
            for _ in 0..n {
                field.frames(1)?;
                let mut c = canvas(w, h, BACK);
                paste(&mut c, &screen(&field.picture()), 8, 8, 1);
                paste(&mut c, &half, gx, gy, 1);
                for l in 0..3usize {
                    let (wx, wz) = windows[l];
                    let (sw, sh) = spans[l];
                    let colour = COLOURS[l];
                    let (ox, oy) = (gx as i32 + wx * 8, gy as i32 + wz * 8);
                    for d in 0..2 {
                        outline(&mut c, ox - d, oy - d, sw * 8 + 2 * d, sh * 8 + 2 * d, colour);
                    }
                    // The screen's part of the layer, wrapped in its window.
                    let (lx, ly) = (work_at(field, l as u32, 0) - wx * 16, work_at(field, l as u32, 4) - wz * 16);
                    let (pw, ph) = (sw * 16, sh * 16);
                    for dy in [0, -ph, ph] {
                        for dx in [0, -pw, pw] {
                            let (rx, ry) = (lx.rem_euclid(pw) + dx, ly.rem_euclid(ph) + dy);
                            for yy in ry.max(0)..(ry + HEIGHT as i32).min(ph) {
                                for xx in rx.max(0)..(rx + WIDTH as i32).min(pw) {
                                    if (xx + yy) % 2 == 0 && (xx % 2 == 0) {
                                        let at = &mut c.pixels[((oy + yy / 2) as usize) * w + (ox + xx / 2) as usize];
                                        *at = mix(*at, colour, 0.5);
                                    }
                                }
                            }
                        }
                    }
                    let y = (gy + gh + 8) as i32 + 52 * l as i32;
                    let lay = &layers[l];
                    text(&mut c, gx as i32, y, &format!("{}: ORIGIN {}, {} TILES, WRAP {}, {} TILES", names[l], lay[0], lay[1], lay[6] + 1, lay[7] + 1), colour, None, 2);
                    text(&mut c, gx as i32, y + 16, &format!("PARALLAX {}/16, {}/16 OF THE VIEW", lay[2], lay[3]), DIM, None, 2);
                    text(&mut c, gx as i32, y + 32, &format!("DRIFT {}/16, {}/16 PIXEL A FRAME", lay[4], lay[5]), DIM, None, 2);
                }
                text(&mut c, 8, (HEIGHT + 16) as i32, "BOUNDS", DIM, None, 2);
                text(&mut c, 8, (HEIGHT + 32) as i32, &format!("{}, {} TO {}, {}", bounds[0], bounds[1], bounds[2], bounds[3]), DIM, None, 2);
                text(&mut c, 8, (HEIGHT + 48) as i32, "TILES", DIM, None, 2);
                frames.push(c.pixels);
            }
        }
        field.hold(0);
        let written = animation::write(&self.out.join("map-header.gif"), &frames, w)?;
        self.written.push(("map-header.gif".to_string(), written.bytes, format!("Suhalla Desert's layer windows, bounds and parallax over its grid; {} frames", written.frames)));
        Ok(())
    }

    /// Goma Cave's grid and its placeholder blocks, as `placeholders.png`:
    /// the party is sent from Vale into Goma Cave, and the block plane is
    /// read as TileMap_ConvertBuffer is entered, before Tile_ConvertMapGrid has
    /// numbered it; the cells' words are read once the cave is loaded. On
    /// the left the three layer windows at half size, every cell dimmed but
    /// the placeholders, each a block's first use, tinted yellow; on the
    /// right the first 16 by 10 cells of the ground layer's window at twice
    /// size with each cell's block number, yellow where the cell was a
    /// placeholder and the number is the one the conversion gave it.
    pub fn placeholders(&mut self) -> Result<(), String> {
        const GOMA_CAVE: (u16, u16) = (27, 5);
        const GRID: i32 = 128;
        const PLACEHOLDER: u16 = 0xfff;
        let convert = self.sym("TileMap_ConvertBuffer")?;
        let state = self.sym("gAdventure").or_else(|_| self.sym("gCommon")).or_else(|_| self.sym("gPersistentState")).or_else(|_| self.sym("gSaveBlock"))?;
        let mut entered = self.enter_with(VALE, &[])?;
        let field = &mut entered.field;
        let plane = Rc::new(RefCell::new(Vec::new()));
        let p = plane.clone();
        field.probe_pcs = vec![convert];
        field.probe = Some(Box::new(move |gba, pc, _| {
            if pc == convert && gba.read16(state + GS_SCENE) == GOMA_CAVE.0 && p.borrow().is_empty() {
                *p.borrow_mut() = (0..(GRID * GRID) as u32).map(|i| gba.read16(0x0201_8000 + 2 * i)).collect::<Vec<u16>>();
            }
        }));
        self.travel(field, GOMA_CAVE, 300)?;
        field.probe = None;
        let raw = plane.borrow().clone();
        if raw.is_empty() || field.scene() != GOMA_CAVE.0 {
            return Err("the block plane was not read on the way into Goma Cave".to_string());
        }
        let blocks: Vec<u16> = (0..(GRID * GRID) as u32).map(|i| (field.read32(0x0201_0000 + 4 * i) & 0xfff) as u16).collect();
        // The numbering: placeholders get 0 up in grid order, every other
        // cell keeps its number.
        let mut next = 0u16;
        let mut placeholders = 0;
        for i in 0..raw.len() {
            if raw[i] & 0xfff == PLACEHOLDER {
                if blocks[i] != next {
                    return Err(format!("placeholder {placeholders} at cell {i} became block {}", blocks[i]));
                }
                next += 1;
                placeholders += 1;
            } else if blocks[i] != raw[i] & 0xfff {
                return Err(format!("cell {i} changed from {} to {}", raw[i] & 0xfff, blocks[i]));
            }
        }
        let spans = {
            let b = field.bounds();
            let s = ((b[2] - b[0]) >> 20, (b[3] - b[1]) >> 20);
            [s, s, s]
        };
        let (grid, windows) = Self::grid_picture(field, spans);
        let (sw, sh) = spans[0];
        let x1 = (0..3).map(|l| windows[l].0 + sw).max().unwrap_or(0);
        let z1 = (0..3).map(|l| windows[l].1 + sh).max().unwrap_or(0);
        // An example: the first placeholder whose block a later cell uses
        // again by number, and that later cell.
        let (first, later) = (0..raw.len())
            .filter(|&i| raw[i] & 0xfff == PLACEHOLDER && blocks[i] > 0 && i as i32 / GRID >= 6)
            .find_map(|i| (i + 3 * GRID as usize..raw.len()).find(|&j| raw[j] & 0xfff == blocks[i]).map(|j| (i, j)))
            .ok_or("no placeholder's block is used again")?;
        let n = blocks[first];
        // Left: a quarter size, cells dimmed but the placeholders.
        const Q: usize = 4;
        let (lw, lh) = (x1 as usize * Q, z1 as usize * Q);
        let right_x = 8 + lw + 16;
        let (w, h) = (right_x + 400, 8 + lh.max(240) + 50);
        let mut image = canvas(w, h, BACK);
        for y in 0..lh {
            for x in 0..lw {
                let (cx, cz) = ((x / Q) as i32, (y / Q) as i32);
                let i = (cx + cz * GRID) as usize;
                let inside = (0..3).any(|l| cx >= windows[l].0 && cx < windows[l].0 + sw && cz >= windows[l].1 && cz < windows[l].1 + sh);
                let mut c = grid.pixels[(16 / Q) * y * grid.width + (16 / Q) * x];
                if !inside {
                    c = BACK;
                } else if raw[i] & 0xfff == PLACEHOLDER {
                    c = mix(c, MARK, 0.7);
                } else {
                    c = mix(c, 0, 0.6);
                }
                image.pixels[(8 + y) * w + 8 + x] = c;
            }
        }
        for l in 0..3 {
            outline(&mut image, 8 + windows[l].0 * Q as i32, 8 + windows[l].1 * Q as i32, sw * Q as i32, sh * Q as i32, DIM);
            text(&mut image, 10 + windows[l].0 * Q as i32, 10 + windows[l].1 * Q as i32, ["GROUND", "MIDDLE", "OVERHEAD"][l], INK, Some(0x20_22_28), 1);
        }
        // The two example cells: ringed on the grid, and six times their size
        // on the right with what the conversion does to them.
        let examples = [(first, 0xff_ff_ff, "THE FIRST USE OF A PICTURE", vec!["STORED AS 4095, A PLACEHOLDER.".to_string(), "TILE_CONVERTMAP GIVES IT THE".to_string(), format!("NEXT BLOCK NUMBER, {n}: COUNTING"), "FROM 0, THE BLOCKS GO IN THE".to_string(), "ORDER THE GRID FIRST USES THEM.".to_string()]),
            (later, 0x50_d0_ff, "A LATER USE OF IT", vec![format!("ALREADY STORED AS BLOCK {n},"), "AND KEPT AS IT IS.".to_string()])];
        let mut y = 8i32;
        for (cell, colour, title, lines) in &examples {
            let (cx, cz) = ((*cell as i32) % GRID, (*cell as i32) / GRID);
            for d in 0..2 {
                outline(&mut image, 8 + cx * Q as i32 - 3 - d, 8 + cz * Q as i32 - 3 - d, Q as i32 + 6 + 2 * d, Q as i32 + 6 + 2 * d, *colour);
            }
            for py in 0..96usize {
                for px in 0..96usize {
                    let c = grid.pixels[(cz as usize * 16 + py / 6) * grid.width + cx as usize * 16 + px / 6];
                    image.pixels[(y as usize + py) * w + right_x + px] = c;
                }
            }
            for d in 0..2 {
                outline(&mut image, right_x as i32 - 1 - d, y - 1 - d, 98 + 2 * d, 98 + 2 * d, *colour);
            }
            text(&mut image, right_x as i32 + 106, y, title, *colour, None, 2);
            for (k, line) in lines.iter().enumerate() {
                text(&mut image, right_x as i32 + 106, y + 20 + 16 * k as i32, line, INK, None, 2);
            }
            y += 120;
        }
        let blocks_used = blocks.iter().copied().max().unwrap_or(0) + 1;
        let y = (8 + lh.max(240) + 8) as i32;
        text(&mut image, 8, y, &format!("GOMA CAVE\x27S THREE LAYER WINDOWS, THE {placeholders} PLACEHOLDERS IN YELLOW."), INK, None, 2);
        text(&mut image, 8, y + 18, &format!("ONE PLACEHOLDER FOR EACH OF THE MAP\x27S {blocks_used} BLOCKS, AT ITS FIRST USE."), INK, None, 2);
        self.guide_png("placeholders.png", &image, "Goma Cave's grid with each block's first use, the placeholder Tile_ConvertMapGrid numbers")
    }

    /// The marker list of the map CrossboneIsleKeys and CrossboneIsleBane
    /// share, drawn over both rooms, as `markers.png`: each scene is
    /// entered, its room stitched from the map as it loads it, and its
    /// placed markers read from FieldWork.markers; every marker of the list
    /// (read in place through MapGridScrollWork.script) is outlined at its cell
    /// with its id, in the colour of the scene that placed it.
    pub fn markers(&mut self) -> Result<(), String> {
        const SCENES: [(u16, &str, u32); 2] = [(117, "CROSSBONEISLEKEYS", 0xff_d8_40), (120, "CROSSBONEISLEBANE", 0x50_d0_ff)];
        let mut rooms = Vec::new();
        let mut list: Vec<(u8, u8, u8)> = Vec::new();
        let mut placed: Vec<(u8, usize, i32)> = Vec::new();
        for (n, &(scene, _, _)) in SCENES.iter().enumerate() {
            let mut entered = self.enter_with((scene, 1), &[])?;
            let field = &mut entered.field;
            let work = field.field_work();
            for m in 0..10u32 {
                let b = work + FW_MARKERS + 8 * m;
                let object = field.read32(b);
                if object != 0 {
                    // Where the placed object stands, and its height, which
                    // draws it that much higher up the map.
                    let at = |o: u32| field.read32(object + o) as i32 >> 16;
                    placed.push((field.read8(b + 4), n, at(0x0c)));
                }
            }
            // The list, read in place from the map's header.
            let mut a = field.read32(field.map_work() + 0x10);
            let mut here = Vec::new();
            while field.read8(a) != 255 {
                here.push((field.read8(a), field.read8(a + 1), field.read8(a + 2)));
                a += 3;
            }
            if n > 0 && here != list {
                return Err("the two scenes read different marker lists".to_string());
            }
            list = here;
            // The room, widened to take in a marker just past its edge.
            let field = &mut entered.field;
            let settled = field.save();
            let mut r = field.room();
            for &(column, row, _) in &list {
                let (x, y) = (i32::from(column) * 16, i32::from(row) * 16);
                if x >= r[0] - 32 && x < r[2] + 32 && y >= r[1] - 32 && y < r[3] + 32 {
                    r = [r[0].min(x - 8), r[1].min(y - 24), r[2].max(x + 24), r[3].max(y + 24)];
                }
            }
            let image = field.stitch(&settled, [r[0] << 16, r[1] << 16, r[2] << 16, r[3] << 16], false)?;
            rooms.push((image, (r[0], r[1])));
        }
        let x0 = rooms.iter().map(|r| r.1 .0).min().unwrap_or(0);
        let y0 = rooms.iter().map(|r| r.1 .1).min().unwrap_or(0);
        let x1 = rooms.iter().map(|r| r.1 .0 + r.0.width as i32).max().unwrap_or(0);
        let y1 = rooms.iter().map(|r| r.1 .1 + r.0.height as i32).max().unwrap_or(0);
        let (w, h) = ((x1 - x0) as usize + 16, (y1 - y0) as usize + 16 + 106);
        let mut image = canvas(w, h, BACK);
        for (room, (ox, oy)) in &rooms {
            paste(&mut image, room, (ox - x0) as usize + 8, (oy - y0) as usize + 8, 1);
        }
        for &(column, row, id) in &list {
            let lift = placed.iter().find(|p| p.0 == id).map_or(0, |p| p.2);
            let (x, y) = (i32::from(column) * 16 - x0 + 8, i32::from(row) * 16 - y0 + 8 - lift);
            let by = placed.iter().find(|p| p.0 == id).map(|p| p.1);
            let colour = by.map_or(0xff_ff_ff, |s| SCENES[s].2);
            for d in 0..2 {
                outline(&mut image, x - d, y - d, 16 + 2 * d, 16 + 2 * d, colour);
            }
            // What each chest holds, from the scenes' event rows (EXAMINE_CHEST
            // with FIND_COINS or FIND_ITEM, crossbone_isle_keys.c), staggered
            // so that neighbours' labels clear each other; the marker's id
            // small under its cell.
            let what = match id { 100 => "333 COINS", 101 => "FAIRY RING", 102 => "COOKIE", 103 => "SMOKE BOMB", 104 => "666 COINS", 105 => "WATER OF LIFE", 106 => "ELIXIR", 107 => "DEMON AXE", _ => "?" };
            let lift = if id % 2 == 1 { 36 } else { 18 };
            let w = text(&mut image, -100, -100, what, 0, None, 2);
            let lx = (x + 8 - w / 2).clamp(2, image.width as i32 - w - 2);
            text(&mut image, lx, y - lift, what, 0x10_10_10, Some(colour), 2);
            text(&mut image, x + 1, y + 18, &id.to_string(), colour, None, 1);
        }
        let mut y = (y1 - y0) as i32 + 20;
        for (n, &(_, name, colour)) in SCENES.iter().enumerate() {
            let ids: Vec<String> = placed.iter().filter(|p| p.1 == n).map(|p| p.0.to_string()).collect();
            text(&mut image, 8, y, &format!("{name} PLACES THE CHESTS OF MARKERS {}", ids.join(", ")), colour, None, 2);
            y += 18;
        }
        text(&mut image, 8, y, &format!("THE MAP'S LIST HOLDS {} MARKERS, EACH LABELLED BY WHAT ITS CHEST HOLDS", list.len()), DIM, None, 2);
        text(&mut image, 8, y + 18, "THE DEMON AXE'S MARKER (107) IS PLACED, THEN MOVED TO 0, 0 BY THE", DIM, None, 2);
        text(&mut image, 8, y + 36, "MAP UNTIL THE STONE PILLAR CARRYING ITS CHEST REACHES THE LADDER", DIM, None, 2);
        self.guide_png("markers.png", &image, "the markers of the map Crossbone Isle's two rooms share, and which map places each")
    }

    /// The world map's squares unpacked around the party, as
    /// `world-map-squares.gif`: the party runs south and east from the world
    /// map's entrance 3, south of Vale, with FLAG_NO_ENCOUNTERS set; beside
    /// the screen, 5 by 4 of the world's 512-pixel squares, each drawn from
    /// the ground layer's block buffer while it is unpacked there
    /// (WorldMap_WriteLayerCellTile's quarters of Ram_Metatiles: a cell word's
    /// low halfword picks a block of four tile numbers in Ram_MapGridCellBuffer,
    /// each drawn as the mean colour of its 256-colour tile in `BG3`'s char
    /// block, where the ground layer's screen map is). The 2 by 2 squares unpacked now are bright and outlined,
    /// those unpacked earlier dim, the rest black; each frame the quarters'
    /// squares (WorldMapView.quarterTiles) are checked against the squares
    /// the followed point's place and the tileIds table give.
    pub fn world_map_squares(&mut self) -> Result<(), String> {
        const WORLD_MAP: (u16, u16) = (2, 3);
        const SQUARES: i32 = 16;
        const CELLS: i32 = 16;
        const PX: i32 = 6;
        const SIDE: i32 = CELLS * PX;
        const VALE_EXIT: (i32, i32) = (5680, 1226);
        let mut entered = self.enter_with(WORLD_MAP, &["FLAG_NO_ENCOUNTERS"])?;
        let field = &mut entered.field;
        let colours = super::guide_more::palettes(field);
        let char_base = 0x0600_0000 + u32::from((field.read16(0x0400_000e) >> 2) & 3) * 0x4000;
        let mut tile_means: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        let place_of = |field: &Field| {
            let origin = field.read32(field.map_work());
            let (px, pz) = (field.read32(origin) as i32, field.read32(origin + 8) as i32);
            ((px - 0x0100_0000) >> 25, (pz - 0x0140_0000) >> 25, px >> 16, pz >> 16)
        };
        let (q0x, q0z, _, _) = place_of(field);
        let (wx0, wz0, ww, wh) = (q0x - 1, q0z - 1, 5, 4);
        // Each square's picture once it has been unpacked.
        let mut drawn: std::collections::HashMap<(i32, i32), Image> = std::collections::HashMap::new();
        let (mx, my) = (8 + WIDTH + 16, 8usize);
        let (w, h) = (mx + (ww * SIDE) as usize + 8, 8 + (wh * SIDE) as usize + 8);
        let mut frames = Vec::new();
        let ways = [KEY_DOWN, KEY_RIGHT, KEY_DOWN, KEY_RIGHT, KEY_UP];
        for (leg, &way) in ways.iter().enumerate() {
            let keys = KEY_B | way;
            field.hold(keys);
            let (mut last, mut still) = (field.leader().map(|a| (a.x, a.z)), 0);
            for f in 0..240u32 {
                field.frames(1)?;
                if field.scene() != WORLD_MAP.0 {
                    return Err(format!("the walk left the world map for scene {}", field.scene()));
                }
                let now = field.leader().map(|a| (a.x, a.z));
                still = if now == last { still + 1 } else { 0 };
                last = now;
                if still >= 8 && leg > 0 {
                    break;
                }
                if f % 6 != 0 {
                    continue;
                }
                let work = field.map_work();
                let (qx, qz, px, pz) = place_of(field);
                let mut now_places = Vec::new();
                for row in 0..2 {
                    for col in 0..2 {
                        let (sx, sz) = ((qx + col).rem_euclid(SQUARES), (qz + row).rem_euclid(SQUARES));
                        let id = field.read16(work + 0x138 + 2 * (sz * SQUARES + sx) as u32);
                        let (bx, by) = ((sx & 1) as u32, (sz & 1) as u32);
                        let shown = field.read16(work + 0x338 + 2 * (by * 2 + bx));
                        if shown != id {
                            return Err(format!("the quarter for {sx}, {sz} shows square {shown}, not {id}"));
                        }
                        // The square as the ground layer's buffer holds it.
                        let mut pic = canvas(SIDE as usize, SIDE as usize, 0);
                        for r in 0..CELLS as u32 {
                            for c in 0..CELLS as u32 {
                                let word = field.read32(0x0202_0000 + by * 2048 + bx * 64 + r * 128 + c * 4);
                                let block = word & 0xffff;
                                for t in 0..4u32 {
                                    let tile = u32::from(field.read8(0x0201_0000 + block * 4 + t));
                                    let mean = *tile_means.entry(tile).or_insert_with(|| {
                                        let mut sum = [0u32; 3];
                                        for p in 0..64 {
                                            let c = colours[usize::from(field.read8(char_base + tile * 64 + p))];
                                            for (k, s) in sum.iter_mut().enumerate() {
                                                *s += (c >> (16 - 8 * k)) & 0xff;
                                            }
                                        }
                                        (sum[0] / 64) << 16 | (sum[1] / 64) << 8 | sum[2] / 64
                                    });
                                    let (tx, ty) = (c as usize * PX as usize + (t as usize % 2) * 3, r as usize * PX as usize + (t as usize / 2) * 3);
                                    for d in 0..9 {
                                        pic.pixels[(ty + d / 3) * SIDE as usize + tx + d % 3] = mean;
                                    }
                                }
                            }
                        }
                        drawn.insert((sx, sz), pic);
                        now_places.push((sx, sz));
                    }
                }
                let mut c = canvas(w, h, BACK);
                paste(&mut c, &screen(&field.picture()), 8, 8, 1);
                for (&(sx, sz), pic) in &drawn {
                    let (gx, gz) = (sx - wx0, sz - wz0);
                    if !(0..ww).contains(&gx) || !(0..wh).contains(&gz) {
                        continue;
                    }
                    let lit = now_places.contains(&(sx, sz));
                    let (ox, oy) = (mx + (gx * SIDE) as usize, my + (gz * SIDE) as usize);
                    for y in 0..SIDE as usize {
                        for x in 0..SIDE as usize {
                            let p = pic.pixels[y * SIDE as usize + x];
                            c.pixels[(oy + y) * w + ox + x] = if lit { p } else { mix(p, 0x20_22_28, 0.6) };
                        }
                    }
                }
                for gz in 0..=wh {
                    outline(&mut c, mx as i32, my as i32 + gz * SIDE, ww * SIDE, 1, 0x40_42_4a);
                }
                for gx in 0..=ww {
                    outline(&mut c, mx as i32 + gx * SIDE, my as i32, 1, wh * SIDE, 0x40_42_4a);
                }
                let (bx0, bz0) = ((qx - wx0) * SIDE, (qz - wz0) * SIDE);
                for d in 0..3 {
                    outline(&mut c, mx as i32 + bx0 - d, my as i32 + bz0 - d, 2 * SIDE + 2 * d, 2 * SIDE + 2 * d, MARK);
                }
                // The party, and Vale's door, at their places.
                let at = |x: i32, z: i32| (mx as i32 + (x - wx0 * 512) * PX / 32, my as i32 + (z - wz0 * 512) * PX / 32);
                let (dx, dz) = at(px, pz);
                outline(&mut c, dx - 3, dz - 3, 7, 7, 0xff_ff_ff);
                let (vx, vz) = at(VALE_EXIT.0, VALE_EXIT.1);
                outline(&mut c, vx - 2, vz - 2, 5, 5, 0xff_60_60);
                text(&mut c, vx + 5, vz - 6, "VALE", 0xff_ff_ff, Some(0x80_30_30), 2);
                let mut y = (8 + HEIGHT + 12) as i32;
                for (line, colour) in [("YELLOW: THE 2 BY 2", MARK), ("SQUARES UNPACKED NOW", MARK), ("AROUND THE PARTY", MARK), ("(WHITE SQUARE)", INK), ("", INK), ("DIM: UNPACKED BEFORE", DIM), ("BLACK: NOT YET", DIM), ("", INK), ("A SQUARE IS 16 BY 16", INK), ("CELLS, 512 PIXELS", INK), ("ACROSS", INK)] {
                    text(&mut c, 8, y, line, colour, None, 2);
                    y += 16;
                }
                frames.push(c.pixels.clone());
                frames.push(c.pixels);
            }
        }
        field.hold(0);
        let written = animation::write(&self.out.join("world-map-squares.gif"), &frames, w)?;
        if drawn.len() < 6 {
            return Err(format!("only {} squares were unpacked", drawn.len()));
        }
        self.written.push(("world-map-squares.gif".to_string(), written.bytes, format!("the world map's squares unpacked around the party; {} frames", written.frames)));
        Ok(())
    }


    /// The Azart match's recorded path over its course, as `colosso-path.png`:
    /// the course stitched from the match's tour of it, `opponent_path_a.tsv`
    /// drawn point by point in yellow with a mark every five seconds, and
    /// where Azart (actor 8) stood each frame of the race in cyan, which is
    /// the path mirrored across the finish (x 1304, Colosso_StartRaceGauge's
    /// finishX) onto the course beside Isaac's.
    pub fn colosso_path(&mut self) -> Result<(), String> {
        const RACE: (u16, u16) = (143, 1);
        const TOUR: u16 = 3;
        const AZART: u16 = 8;
        const FINISH_X: i32 = 1304;
        let tsv = std::fs::read_to_string(crate::compile::root().join("games/tbs/src/map/colosso/opponent_path_a.tsv")).map_err(|e| format!("opponent_path_a.tsv: {e}"))?;
        let path: Vec<(i32, i32)> = tsv.lines().skip(1).filter_map(|l| {
            let mut it = l.split('\t').map(|v| v.trim().parse::<i32>());
            Some((it.next()?.ok()?, it.next()?.ok()?))
        }).collect();
        // The run ends at the first row of two zeros.
        let run: Vec<(i32, i32)> = path.iter().copied().take_while(|&p| p != (0, 0)).collect();
        let mut tour = self.enter_with((RACE.0, TOUR), &[])?;
        let settled = tour.field.save();
        let h = tour.field.header();
        let mut map = tour.field.stitch(&settled, h, false)?;
        drop(tour);
        let (ox, oz) = (h[0] >> 16, h[1] >> 16);
        let mut entered = self.enter_with(RACE, &[])?;
        let field = &mut entered.field;
        let mut played = Vec::new();
        for _ in 0..run.len() + 120 {
            field.frames(1)?;
            let Some(a) = field.actor(AZART) else { break };
            played.push((a.x, a.z));
        }
        // How many frames Azart stood on a point of the path, mirrored.
        let on: usize = played.iter().filter(|&&(x, z)| run.iter().any(|&(px, pz)| (2 * FINISH_X - px - x).abs() <= 1 && (pz - z).abs() <= 1)).count();
        if on * 10 < played.len() * 9 {
            return Err(format!("Azart stood on the mirrored path {on} frames of {}", played.len()));
        }
        let put = |map: &mut Image, x: i32, z: i32, colour: u32| outline(map, x - ox - 1, z - oz - 1, 2, 2, colour);
        for &(x, z) in &run {
            put(&mut map, x, z, MARK);
        }
        for (i, &(x, z)) in run.iter().enumerate().step_by(300) {
            outline(&mut map, x - ox - 3, z - oz - 3, 6, 6, 0xff_ff_ff);
            text(&mut map, x - ox + 4, z - oz - 14, &format!("{}", i / 60), 0x10_10_10, Some(MARK), 2);
        }
        let tall = map.height as i32;
        outline(&mut map, FINISH_X - ox, 0, 1, tall, 0xff_ff_ff);
        text(&mut map, FINISH_X - ox + 3, 2, "FINISH", 0xff_ff_ff, Some(0x40_40_40), 2);
        let mut out = canvas(map.width + 8, map.height + 8 + 44, BACK);
        paste(&mut out, &map, 4, 4, 1);
        let y = map.height as i32 + 10;
        text(&mut out, 4, y, &format!("YELLOW: OPPONENT_PATH_A.TSV, {} POINTS, ONE A FRAME, SECONDS MARKED", run.len()), MARK, None, 2);
        text(&mut out, 4, y + 16, &format!("AZART RUNS THE POINTS MIRRORED ACROSS THE FINISH, PAST THE MAP'S EAST EDGE: {on} OF {} FRAMES ON ONE", played.len()), INK, None, 2);
        self.guide_png("colosso-path.png", &out, "the Azart match's recorded path over its course, and Azart running it mirrored")
    }

    /// The leader's probes for a step as he is pushed into a wall and slides
    /// along it, as `player-probes.gif`: Vale at twice the GBA's size, and
    /// over it every point Actor_UpdatePlayerControl tested that frame
    /// through MapGrid_CheckCollision for the leader, green where the ground
    /// was open (COLLISION_NONE) and red where it was not, with a line from
    /// the leader to the pad's way. The probes are read at the function's
    /// entry (r1, the position) and its results at the return addresses a
    /// first run of the same frames found.
    pub fn player_probes(&mut self, path: &[(u32, u32)], file: &str) -> Result<(), String> {
        let check = self.sym("MapGrid_CheckCollision")?;
        let mut entered = self.enter_with(VALE, &[])?;
        let field = &mut entered.field;
        let start = field.save();
        let leader = field.actor_address(0);
        // First run: the addresses the checks return to.
        let returns = Rc::new(RefCell::new(Vec::<u32>::new()));
        let r = returns.clone();
        field.probe_pcs = vec![check];
        field.probe = Some(Box::new(move |gba, pc, _| {
            if pc == check && gba.register(0) == leader {
                let lr = gba.register(14) & !1;
                if !r.borrow().contains(&lr) {
                    r.borrow_mut().push(lr);
                }
            }
        }));
        for &(keys, n) in path {
            field.hold(keys);
            field.frames(n)?;
        }
        field.hold(0);
        // Second run: each probe and its result, by frame.
        field.load(&start);
        let probes = Rc::new(RefCell::new(Vec::<(u32, i32, i32, i32, Option<u32>)>::new()));
        let p = probes.clone();
        let backs = returns.borrow().clone();
        let mut pcs = backs.clone();
        pcs.push(check);
        field.probe_pcs = pcs;
        let waiting = Rc::new(RefCell::new(false));
        let wait = waiting.clone();
        field.probe = Some(Box::new(move |gba, pc, frame| {
            if pc == check && gba.register(0) == leader {
                let at = gba.register(1);
                let word = |o: u32| gba.read32(at + o) as i32 >> 16;
                p.borrow_mut().push((frame, word(0), word(4), word(8), None));
                *wait.borrow_mut() = true;
            } else if pc != 0 && *wait.borrow() && backs.contains(&pc) {
                if let Some(last) = p.borrow_mut().last_mut() {
                    last.4 = Some(gba.register(0));
                }
                *wait.borrow_mut() = false;
            }
        }));
        let mut frames = Vec::new();
        for &(keys, n) in path {
            field.hold(keys);
            for _ in 0..n {
                let before = probes.borrow().len();
                field.frames(1)?;
                let frame_probes: Vec<(i32, i32, i32, Option<u32>)> = probes.borrow()[before..].iter().map(|e| (e.1, e.2, e.3, e.4)).collect();
                const ZOOM: i32 = 6;
                const SPAN: i32 = 48;
                let inset_x = 2 * WIDTH + 12;
                let (w, h) = (inset_x + (ZOOM * SPAN) as usize + 8, 2 * HEIGHT + 8);
                let mut c = canvas(w, h, BACK);
                let shot = screen(&field.picture());
                paste(&mut c, &shot, 4, 4, 2);
                let (vx, vy) = field.view();
                let lead = field.leader().ok_or("no leader")?;
                let (lx, ly) = (lead.x - vx, lead.z - (lead.y >> 16) - vy);
                // The inset: the 48 pixels around the leader's feet, 6 times.
                let (ix0, iy0) = (lx - SPAN / 2, ly - SPAN / 2);
                for y in 0..ZOOM * SPAN {
                    for x in 0..ZOOM * SPAN {
                        let (px, py) = (ix0 + x / ZOOM, iy0 + y / ZOOM);
                        let colour = if (0..WIDTH as i32).contains(&px) && (0..HEIGHT as i32).contains(&py) { mix(shot.pixels[py as usize * WIDTH + px as usize], 0, 0.3) } else { 0 };
                        c.pixels[(4 + y as usize) * w + inset_x + x as usize] = colour;
                    }
                }
                outline(&mut c, 4 + 2 * ix0, 4 + 2 * iy0, 2 * SPAN, 2 * SPAN, 0xff_ff_ff);
                let held = !frame_probes.is_empty();
                // The heading the step took: the first probe of the last
                // five, when all five were open.
                let tail = frame_probes.len().saturating_sub(5);
                let took = frame_probes.len() >= 5 && frame_probes[tail..].iter().all(|p| p.3 == Some(0));
                let centre = (inset_x as i32 + ZOOM * SPAN / 2, 4 + ZOOM * SPAN / 2);
                for (i, &(x, y, z, result)) in frame_probes.iter().enumerate() {
                    let (sx, sy) = (x - vx, z - y - vy);
                    let colour = match result {
                        Some(0) => 0x40_ff_60,
                        Some(_) => 0xff_40_40,
                        None => 0xff_ff_ff,
                    };
                    let colour = if took && i == tail { MARK } else { colour };
                    let (zx, zy) = (inset_x as i32 + ZOOM * (sx - ix0), 4 + ZOOM * (sy - iy0));
                    let steps = 24;
                    for t in 0..=steps {
                        let (qx, qy) = (centre.0 + (zx - centre.0) * t / steps, centre.1 + (zy - centre.1) * t / steps);
                        if qx >= inset_x as i32 && qy >= 0 && (qx as usize) < w && (qy as usize) < h {
                            let at = &mut c.pixels[qy as usize * w + qx as usize];
                            *at = mix(*at, colour, 0.6);
                        }
                    }
                    for d in 0..3 {
                        outline(&mut c, zx - 4 - d, zy - 4 - d, 8 + 2 * d, 8 + 2 * d, colour);
                    }
                    outline(&mut c, 4 + 2 * sx - 2, 4 + 2 * sy - 2, 4, 4, colour);
                }
                let opens = frame_probes.iter().filter(|p| p.3 == Some(0)).count();
                let label = if held { format!("{} PROBES, {} OPEN", frame_probes.len(), opens) } else { "NO PROBES THIS FRAME".to_string() };
                text(&mut c, 8, 8, &label, 0xff_ff_ff, Some(0x20_20_20), 2);
                if took {
                    text(&mut c, inset_x as i32 + 4, 8, "YELLOW: THE WAY THE STEP TOOK", MARK, Some(0x20_20_20), 2);
                }
                frames.push(c.pixels);
            }
        }
        field.hold(0);
        field.probe = None;
        let w = 2 * WIDTH + 12 + 6 * 48 + 8;
        let written = animation::write(&self.out.join(file), &frames, w)?;
        self.written.push((file.to_string(), written.bytes, format!("the leader's step probes as he slides along a wall; {} frames", written.frames)));
        Ok(())
    }

    /// The task table in Vale through one frame, as `scheduler.png`: Vale
    /// beside the 20 slots of gTasks, each with its order, the
    /// pass its order's high byte picks and its routine, and the place it
    /// ran in that pass. The runs are seen by probing Task_RunPass
    /// (r0 the pass's key) and every task's routine.
    pub fn scheduler(&mut self) -> Result<(), String> {
        let table = self.sym("gTasks")?;
        let run_pass = self.sym("Task_RunPass")?;
        let mut entered = self.enter_with(VALE, &[])?;
        let field = &mut entered.field;
        field.frames(10)?;
        let tasks: Vec<(u32, u16)> = (0..20).map(|i| (field.read32(table + 8 * i) & !1, field.read16(table + 8 * i + 4))).collect();
        let events = Rc::new(RefCell::new(Vec::new()));
        let e = events.clone();
        let mut pcs: Vec<u32> = tasks.iter().filter(|t| t.0 != 0).map(|t| t.0).collect();
        pcs.push(run_pass);
        field.probe_pcs = pcs;
        field.probe = Some(Box::new(move |gba, pc, frame| {
            if pc != 0 {
                e.borrow_mut().push((frame, pc, gba.register(0)));
            }
        }));
        field.frames(2)?;
        field.probe = None;
        let picture = screen(&field.picture());
        // One frame's runs: the first frame number seen whole.
        let events = events.borrow();
        let frame = events.first().ok_or("no pass ran")?.0;
        let mut pass = 0u32;
        let mut runs: Vec<Option<(u32, usize)>> = vec![None; 20];
        let mut counts = [0usize; 2];
        for &(_, pc, r0) in events.iter().filter(|e| e.0 == frame) {
            if pc == run_pass {
                pass = r0;
                continue;
            }
            let slot = tasks.iter().position(|t| t.0 == pc).ok_or("a routine ran from no slot")?;
            let which = usize::from(pass == 0xc80);
            if u32::from(tasks[slot].1 >> 8) != pass >> 8 {
                return Err(format!("slot {slot}, order {:#x}, ran in pass {pass:#x}", tasks[slot].1));
            }
            counts[which] += 1;
            runs[slot] = Some((pass, counts[which]));
        }
        // Every task the passes match ran once, in table order.
        for (slot, t) in tasks.iter().enumerate() {
            let matched = t.1 >> 8 == 0x0c || t.1 >> 8 == 0x04;
            if matched != runs[slot].is_some() {
                return Err(format!("slot {slot} ran {:?}", runs[slot]));
            }
        }
        let row = 16i32;
        let (tx, ty) = (WIDTH as i32 + 24, 8i32);
        let mut image = canvas(WIDTH + 24 + 680, (8 + row * 23 + 8) as usize, BACK);
        paste(&mut image, &picture, 8, 8, 1);
        text(&mut image, 8, HEIGHT as i32 + 16, "VALE, THIS FRAME", DIM, None, 2);
        let columns = [0, 48, 120, 240, 320];
        for (c, head) in ["SLOT", "ORDER", "PASS", "RAN", "ROUTINE"].iter().enumerate() {
            text(&mut image, tx + columns[c], ty, head, HEAD, None, 2);
        }
        for (slot, t) in tasks.iter().enumerate() {
            let y = ty + row * (slot as i32 + 1) + 4;
            let (pass_name, colour) = match t.1 >> 8 {
                0x0c => ("FRAME", 0x8c_c8_ff),
                0x04 => ("VBLANK", 0xff_b0_50),
                0xff if t.1 == 0xffff => ("EMPTY", DIM),
                0x7f if t.1 == 0x7fff => ("REMOVED", DIM),
                _ => ("SUSPENDED", DIM),
            };
            text(&mut image, tx + columns[0], y, &format!("{slot:>2}"), DIM, None, 2);
            text(&mut image, tx + columns[1], y, &format!("{:04X}", t.1), colour, None, 2);
            text(&mut image, tx + columns[2], y, pass_name, colour, None, 2);
            if let Some((pass, n)) = runs[slot] {
                let label = format!("{}{n}", if pass == 0xc80 { "F" } else { "V" });
                text(&mut image, tx + columns[3], y, &label, MARK, None, 2);
            }
            if t.0 != 0 {
                text(&mut image, tx + columns[4], y, &self.name_at(field, t.0), INK, None, 2);
            }
        }
        let y = ty + row * 21 + 8;
        text(&mut image, tx, y, "RAN: V IN THE VBLANK PASS, F IN THE FRAME PASS,", DIM, None, 2);
        text(&mut image, tx, y + 14, "EACH PASS IN TABLE ORDER", DIM, None, 2);
        self.guide_png("scheduler.png", &image, "Vale's task table through one frame: each slot's order, pass and routine, and when it ran")
    }

    /// The name of the code or data at `address`: the symbol at or before it
    /// in the edition's map, or, in the overlay area, in the ELF of the
    /// overlay loaded there now; with the offset past the symbol when there
    /// is one.
    fn name_at(&self, field: &Field, address: u32) -> String {
        let symbols: Vec<(u32, String)> = if (0x0200_8000..0x0201_0000).contains(&address) {
            match loaded_overlay(field) {
                Some((_, elf)) => elf.symbols.iter().filter(|s| !s.name.is_empty() && !s.name.starts_with('$') && s.kind != 4).map(|s| (s.value & !1, s.name.clone())).collect(),
                None => Vec::new(),
            }
        } else {
            match self.map.at_address(address) {
                Some(p) => p.symbols.clone(),
                None => Vec::new(),
            }
        };
        let best = symbols.iter().filter(|(a, _)| *a <= address).max_by_key(|(a, _)| *a);
        match best {
            Some((a, n)) if *a == address => n.clone(),
            Some((a, n)) => format!("{n}+{:#x}", address - a),
            None => format!("{address:#010x}"),
        }
    }

    /// The title intro's palette fade (Title_ShowIntro's TitlePaletteFade_Start
    /// of 60 frames), every frame of it shown twice: the screen beside the
    /// 256 background and 256 sprite colours of palette RAM, a palette of
    /// 16 to a row, as `palette-fade.gif`. The fade's frames are counted
    /// from TitlePaletteFade_Start to the update that finds the fade done
    /// (TitlePaletteFade_Update with the work's duration 0).
    pub fn palette_fade(&mut self) -> Result<(), String> {
        let start = self.sym("TitlePaletteFade_Start")?;
        let setup = super::field::Setup { scene: 0, entrance: 0, set: Vec::new(), clear: Vec::new() };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        let started = Rc::new(RefCell::new(None));
        let seen = started.clone();
        field.probe_pcs = vec![start];
        field.probe = Some(Box::new(move |gba, pc, frame| {
            if pc != 0 && seen.borrow().is_none() {
                *seen.borrow_mut() = Some((frame, gba.register(0)));
            }
        }));
        let mut n = 0;
        while started.borrow().is_none() {
            field.frames(1)?;
            n += 1;
            if n > 3000 {
                return Err("the intro's fade never started".to_string());
            }
        }
        field.probe = None;
        let (_, length) = started.borrow().unwrap();
        if length != 60 {
            return Err(format!("the fade was started for {length} frames"));
        }
        const CELL: usize = 9;
        let (w, h) = (WIDTH + 16 + 2 * 16 * CELL + 16, HEIGHT + 8 + 24);
        let draw = |field: &Field, label: &str| {
            let mut c = canvas(w, h, BACK);
            paste(&mut c, &screen(&field.picture()), 8, 8, 1);
            for half in 0..2usize {
                let x0 = WIDTH + 16 + half * (16 * CELL + 8);
                for i in 0..256usize {
                    let colour = animation::rgb(field.read16(0x0500_0000 + 2 * (half * 256 + i) as u32));
                    let (x, y) = (x0 + (i % 16) * CELL, 8 + (i / 16) * CELL);
                    for dy in 0..CELL - 1 {
                        for dx in 0..CELL - 1 {
                            c.pixels[(y + dy) * w + x + dx] = colour;
                        }
                    }
                }
                text(&mut c, x0 as i32, (HEIGHT + 10) as i32, ["BACKGROUND COLOURS", "SPRITE COLOURS"][half], DIM, None, 1);
            }
            text(&mut c, 8, (HEIGHT + 12) as i32, label, INK, None, 2);
            c.pixels
        };
        let mut frames = Vec::new();
        let mut last: Vec<u16> = (0..512).map(|i| field.read16(0x0500_0000 + 2 * i)).collect();
        let mut changes = 0;
        // Each step reaches palette RAM at the VBlank after the update that
        // queued it, so the first shows on the second frame after
        // TitlePaletteFade_Start; the first frame still has the colours the intro
        // loaded, under a blank screen.
        for f in 1..=61u32 {
            field.frames(1)?;
            let now: Vec<u16> = (0..512).map(|i| field.read16(0x0500_0000 + 2 * i)).collect();
            if now != last {
                changes = f;
            }
            last = now;
            if f == 1 {
                continue;
            }
            let picture = draw(&field, &format!("STEP {} OF 60", f - 1));
            frames.push(picture.clone());
            frames.push(picture);
        }
        // The last colours held a second.
        let held = draw(&field, "STEP 60 OF 60");
        frames.extend(std::iter::repeat_n(held, 90));
        if changes != 61 {
            return Err(format!("the colours changed for {changes} frames"));
        }
        let written = animation::write(&self.out.join("palette-fade.gif"), &frames, w)?;
        self.written.push(("palette-fade.gif".to_string(), written.bytes, format!("the title intro's 60-frame palette fade beside palette RAM, one step a frame; {} frames", written.frames)));
        Ok(())
    }
}

/// The overlay unpacked in the overlay area now: its resource name and its
/// ELF, found as the build's unpacked overlay whose bytes the area holds.
fn loaded_overlay(field: &Field) -> Option<(String, crate::elf::Elf)> {
    let dir = crate::compile::root().join("out/tbs-en/overlays");
    let mut names: Vec<_> = std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "bin")).collect();
    names.sort();
    for path in names {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let n = bytes.len().min(0x8000);
        if n < 64 {
            continue;
        }
        if (0..n as u32).all(|i| field.read8(0x0200_8000 + i) == bytes[i as usize]) {
            let elf_path = path.with_extension("elf");
            let data = std::fs::read(&elf_path).ok()?;
            let elf = crate::elf::Elf::parse(&data).ok()?;
            let name = path.file_stem()?.to_string_lossy().to_string();
            return Some((name, elf));
        }
    }
    None
}

/// The modes this file plays, after guide::MODES.
pub const MODES: &[&str] = &["menu-test-setup", "palette-fade", "scheduler", "io-write-queue", "resource-directory", "memory-maps", "map-header", "placeholders", "markers", "world-map-squares", "colosso-path", "player-probes", "item-row", "scene-script", "temporary-flags", "overlay-common"];

impl Guide<'_> {
    /// Plays one of this file's modes.
    pub(super) fn run_engine(&mut self, name: &str) -> Result<(), String> {
        match name {
            "menu-test-setup" => self.menu_test_setup(),
            "palette-fade" => self.palette_fade(),
            "scheduler" => self.scheduler(),
            "io-write-queue" => self.io_write_queue(),
            "resource-directory" => self.resource_directory(),
            "memory-maps" => self.memory_maps(),
            "map-header" => self.map_header(),
            "placeholders" => self.placeholders(),
            "markers" => self.markers(),
            "world-map-squares" => self.world_map_squares(),
            "colosso-path" => self.colosso_path(),
            "item-row" => self.item_row(),
            "scene-script" => self.scene_script(),
            "temporary-flags" => self.temporary_flags(),
            "overlay-common" => self.overlay_common(),
            "player-probes" => self.player_probes(&[(0, 10), (KEY_RIGHT, 40), (KEY_UP | KEY_RIGHT, 70), (0, 10)], "player-probes.gif"),
            _ if super::guide_world_play::MODES.contains(&name) => self.run_world_play(name),
            _ if super::guide_front::MODES.contains(&name) => self.run_front(name),
            _ => self.run_menus(name),
        }
    }
}
