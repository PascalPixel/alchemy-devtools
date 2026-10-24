//! Take-apart pictures for the menu guide: the field menu's screens played
//! from a built edition with the pad held as a player would, in Xian with
//! the four members of the story there, each screen beside what its work
//! (gMenuWork) and the party's units hold.

use super::field::Field;
use super::guide::Guide;
use super::guide_more::{canvas, outline, paste, screen};
use super::guide_take::{crop, text};
use super::mgba::{HEIGHT, WIDTH};
use super::source;
use std::cell::RefCell;
use std::rc::Rc;

/// Xian (MapId_Xian), entered from the world map: all four members have
/// joined by then, with the Djinn, Psynergy and equipment of their debug
/// preset (party.rs).
const XIAN: (u16, u16) = (72, 1);

/// KEYINPUT bits.
const KEY_A: u32 = 0x1;
const KEY_RIGHT: u32 = 0x10;
const KEY_UP: u32 = 0x40;
const KEY_DOWN: u32 = 0x80;
const KEY_R: u32 = 0x100;
const KEY_L: u32 = 0x200;

/// The field menu opened with A and its icon `icon` (0 Psynergy to 3
/// Status) chosen.
fn open_screen(icon: u32) -> Vec<(u32, u32)> {
    let mut path = vec![(0, 20), (KEY_A, 4), (0, 50)];
    for _ in 0..icon {
        path.extend([(KEY_RIGHT, 4), (0, 25)]);
    }
    path.extend([(KEY_A, 4), (0, 70)]);
    path
}

/// The characters by number.
const ISAAC: u8 = 0;
const GARET: u8 = 1;

/// struct Unit (battle/battle_unit.h).
const UNIT_MAX_HP: u32 = 0x34;
const UNIT_MAX_PP: u32 = 0x36;
const UNIT_HP: u32 = 0x38;
const UNIT_PP: u32 = 0x3a;
const UNIT_ATTACK: u32 = 0x3c;
const UNIT_DEFENSE: u32 = 0x3e;
const UNIT_AGILITY: u32 = 0x40;
const UNIT_LUCK: u32 = 0x42;
const UNIT_HP_GAUGE: u32 = 0x14;
const UNIT_ACTION_SLOTS: u32 = 0x58;
const UNIT_INVENTORY: u32 = 0xd8;
const UNIT_DJINN_ACTIVE: u32 = 0x108;
const UNIT_CLASS_INDEX: u32 = 0x129;
const INVENTORY_SLOTS: u32 = 15;
/// Inventory entry bits: 15..11 quantity less one | 9 equipped | 8..0 id.
const INVENTORY_EQUIPPED: u16 = 0x200;
const INVENTORY_QUANTITY_SHIFT: u16 = 11;
const ITEM_ID_MASK: u16 = 0x1ff;
const PSYNERGY_ABILITY_MASK: u16 = 0x3fff;
/// A gauge's full share, Unit.hpGauge.
const GAUGE_FULL: u32 = 0x4000;

/// struct PsynergyMenuWork (psynergy_menu.h).
const PSY_PANE_ACTION: u32 = 0x178;
const PSY_OWNER_IDS: u32 = 0x21a;

/// struct ItemMenuFiguresView (four_object_motion.h).
const FIGURE_OBJECTS: u32 = 0x224;
const FIGURE_X: u32 = 0x234;
const FIGURE_Y: u32 = 0x23c;

/// The colours the panels are drawn in.
const BACK: u32 = 0x20_22_28;
const TEXT: u32 = 0xe8_e8_e8;
const DIM: u32 = 0x80_84_8c;
const MARK: u32 = 0xff_d8_40;
const HEAD: u32 = 0x9c_c8_ff;
const ELEMENTS: [(&str, u32); 4] = [("VENUS", 0xf0_c0_40), ("MERCURY", 0x60_b0_ff), ("MARS", 0xff_60_40), ("JUPITER", 0xc0_80_ff)];

/// A hooked function's entry: the frame it came in (counted from the first
/// frame of the path, whose picture is frames[frame] once that frame ends)
/// and r0 to r3.
#[derive(Clone, Copy, Debug)]
struct Hit {
    frame: usize,
    pc: u32,
    r: [u32; 4],
}

impl<'a> Guide<'a> {
    /// Holds each step of `path`, keeping every frame, with each entry of
    /// one of `hooks` and `each` called after every frame.
    fn run_hooked(field: &mut Field<'a>, hooks: &[u32], path: &[(u32, u32)], each: &mut dyn FnMut(&Field<'a>)) -> Result<(Vec<Vec<u16>>, Vec<Hit>), String> {
        let shared: Rc<RefCell<(usize, Vec<Hit>)>> = Rc::new(RefCell::new((0, Vec::new())));
        let inner = shared.clone();
        field.probe_pcs = hooks.to_vec();
        field.probe = Some(Box::new(move |gba, pc, _| {
            let mut s = inner.borrow_mut();
            if pc == 0 {
                s.0 += 1;
            } else {
                let frame = s.0;
                s.1.push(Hit { frame, pc, r: [gba.register(0), gba.register(1), gba.register(2), gba.register(3)] });
            }
        }));
        // Count from the path's first frame.
        shared.borrow_mut().0 = 0;
        let mut frames = Vec::new();
        for &(keys, n) in path {
            field.hold(keys);
            for _ in 0..n {
                field.frames(1)?;
                frames.push(field.picture());
                each(field);
            }
        }
        field.hold(0);
        field.probe = None;
        field.probe_pcs.clear();
        let hits = shared.borrow().1.clone();
        Ok((frames, hits))
    }

    /// The message texts by number, from the story tools' messages.tsv.
    fn message_texts(&self) -> Vec<(String, String)> {
        let text = std::fs::read_to_string(self.flags.dir.join("messages.tsv")).unwrap_or_default();
        let mut out = Vec::new();
        for line in text.lines().skip(1) {
            let f: Vec<&str> = line.split('\t').collect();
            if let (Some(n), Some(s), Some(t)) = (f.first().and_then(|n| n.parse::<usize>().ok()), f.get(1), f.get(2)) {
                if out.len() <= n {
                    out.resize(n + 1, (String::new(), String::new()));
                }
                out[n] = (s.to_string(), t.to_string());
            }
        }
        out
    }

    /// The class names by class index (MsgClassName on).
    fn class_names(&self) -> Vec<String> {
        let messages = self.message_texts();
        let first = messages.iter().position(|(s, _)| s == "MsgClassName").unwrap_or(0);
        messages[first..].iter().map(|(_, t)| t.to_uppercase()).collect()
    }

    /// The Psynergy screen in Xian: Isaac's Cure chosen and used on Garet,
    /// hurt first, one panel for each step of PsynergyMenu_RunSteps,
    /// found where the step's function is entered
    /// (PsynergyMenu_SelectPartySlot, PsynergyMenu_RunList,
    /// PsynergyMenu_ClassifySelectedPsynergy, PsynergyMenu_SelectTarget,
    /// Ability_ApplyToTargets), each at the last frame before the next
    /// step, with what the screen's work and the units hold then, as
    /// `psynergy-steps.png`.
    pub fn menu_psynergy_steps(&mut self) -> Result<(), String> {
        let names = source::character_names()?;
        let menu = self.sym("gMenuWork")?;
        let steps = [
            ("PSYNERGY_STEP_MEMBER", self.sym("PsynergyMenu_SelectPartySlot")?, "PsynergyMenu_SelectPartySlot"),
            ("PSYNERGY_STEP_LIST", self.sym("PsynergyMenu_RunList")?, "PsynergyMenu_RunList"),
            ("PSYNERGY_STEP_CLASSIFY", self.sym("PsynergyMenu_ClassifySelectedPsynergy")?, "PsynergyMenu_ClassifySelectedPsynergy"),
            ("PSYNERGY_STEP_TARGET", self.sym("PsynergyMenu_SelectTarget")?, "PsynergyMenu_SelectTarget"),
            ("PSYNERGY_STEP_APPLY", self.sym("Ability_ApplyToTargets")?, "Ability_ApplyToTargets"),
        ];
        let abilities = super::tables::Game::here()?.ability_names()?;
        let mut field = self.enter(XIAN)?;
        let isaac = field.unit(ISAAC)?;
        let garet = field.unit(GARET)?;
        // Garet hurt, so Cure has something to do.
        let max = u32::from(field.read16(garet + UNIT_MAX_HP));
        let hurt = 60u32;
        field.write16(garet + UNIT_HP, hurt as u16);
        field.write16(garet + UNIT_HP_GAUGE, (hurt * GAUGE_FULL / max) as u16);
        // Psynergy; Isaac; Cure, his first; Garet, one to the right; then
        // the use plays and its message waits.
        let mut path = open_screen(0);
        path.extend([(KEY_A, 4), (0, 60), (KEY_A, 4), (0, 50), (KEY_RIGHT, 4), (0, 40), (KEY_A, 4), (0, 170)]);
        // Each frame: ownerIds, paneAction[0], Garet's HP, Isaac's PP.
        let mut reads: Vec<(u8, u8, u16, u16, u16)> = Vec::new();
        let hooks: Vec<u32> = steps.iter().map(|s| s.1).collect();
        let (frames, hits) = Self::run_hooked(&mut field, &hooks, &path, &mut |f| {
            let work = f.read32(menu);
            reads.push((f.read8(work + PSY_OWNER_IDS), f.read8(work + PSY_OWNER_IDS + 1), f.read16(work + PSY_PANE_ACTION) & PSYNERGY_ABILITY_MASK, f.read16(garet + UNIT_HP), f.read16(isaac + UNIT_PP)))
        })?;
        for h in &hits {
            eprintln!("  frame {}: {} r0..r3 {:x?}", h.frame, steps.iter().find(|s| s.1 == h.pc).map_or("?", |s| s.2), h.r);
        }
        // The first entry of each step, in the order they came.
        let mut order: Vec<(usize, Hit)> = Vec::new();
        for h in &hits {
            let k = steps.iter().position(|s| s.1 == h.pc).unwrap();
            if !order.iter().any(|&(j, _)| j == k) {
                order.push((k, *h));
            }
        }
        if order.iter().map(|o| o.0).collect::<Vec<_>>() != vec![0, 1, 2, 3, 4] {
            return Err(format!("the steps came in the order {:?}", order.iter().map(|o| o.0).collect::<Vec<_>>()));
        }
        let last = frames.len() - 1;
        let at: Vec<usize> = (0..5)
            .map(|i| match i {
                2 => order[2].1.frame,
                4 => last,
                // Before the A that leaves the step, which is held 4 frames.
                _ => order[i + 1].1.frame.saturating_sub(7),
            })
            .collect();
        let name = |a: u16| abilities.get(usize::from(a)).cloned().unwrap_or(format!("{a}")).to_uppercase();
        let (_, _, _, hp_before, pp_before) = reads[order[4].1.frame.saturating_sub(1)];
        let (_, target, action, hp_after, pp_after) = reads[last];
        let user = reads[at[0]].0;
        let lines: [Vec<String>; 5] = [
            vec![format!("THE USER, OWNERIDS[0]: {}", names[usize::from(user)])],
            vec![format!("THE PSYNERGY, PANEACTION[0]: {}", name(reads[at[1]].2))],
            vec!["IT WORKS ON ONE MEMBER,".to_string(), "SO THE NEXT STEP IS _TARGET".to_string()],
            vec![format!("THE TARGET, OWNERIDS[1]: {}", names[usize::from(target)])],
            vec![format!("{}'S HP: {hp_before} TO {hp_after}", names[usize::from(target)]), format!("{}'S PP: {pp_before} TO {pp_after}", names[usize::from(user)])],
        ];
        eprintln!("psynergy-steps: at {at:?}; {} {action}; {lines:?}", name(action));
        let (cw, ch) = (2 * WIDTH + 16, 2 * HEIGHT + 90);
        let mut image = canvas(2 * cw + 8, 3 * ch + 8, BACK);
        for i in 0..5 {
            let (x, y) = (8 + (i % 2) * cw, 8 + (i / 2) * ch);
            text(&mut image, x as i32, y as i32, &format!("{} {}", i + 1, steps[i].0), MARK, None, 2);
            text(&mut image, x as i32, y as i32 + 18, &steps[i].2.to_uppercase(), HEAD, None, 2);
            for (n, l) in lines[i].iter().enumerate() {
                text(&mut image, x as i32, (y + 40 + 16 * n) as i32, l, TEXT, None, 2);
            }
            paste(&mut image, &screen(&frames[at[i]]), x, y + 78, 2);
        }
        let (x, y) = (8 + cw, 8 + 2 * ch + 40);
        let notes = [
            "PSYNERGYMENU_RUNSTEPS'S STEPS, EACH",
            "FOUND WHERE ITS FUNCTION IS ENTERED, AND",
            "THE SCREEN JUST BEFORE THE A THAT ENDS IT.",
            "",
            "_CLASSIFY TAKES NO FRAME: _TARGET STARTS",
            "IN THE SAME FRAME. PSYNERGY THAT WORKS ON",
            "THE FIELD ENDS THERE INSTEAD: THE MENU",
            "CLOSES AND THE FIELD CASTS IT.",
        ];
        for (n, l) in notes.iter().enumerate() {
            text(&mut image, x as i32, (y + 16 * n) as i32, l, DIM, None, 2);
        }
        self.guide_png("psynergy-steps.png", &image, "the Psynergy screen's steps, from Isaac to Cure on Garet")
    }

    /// A unit's stats as the Djinn preview lists them, with its class name
    /// and the Psynergy its action list holds.
    fn stats(field: &Field, unit: u32) -> ([u16; 7], u8, Vec<u16>) {
        let s = [UNIT_MAX_HP, UNIT_MAX_PP, UNIT_ATTACK, UNIT_DEFENSE, UNIT_AGILITY, UNIT_LUCK, UNIT_HP].map(|o| if o == UNIT_LUCK { u16::from(field.read8(unit + o)) } else { field.read16(unit + o) });
        let psynergy = (0..32).map(|i| field.read16(unit + UNIT_ACTION_SLOTS + 4 * i) & PSYNERGY_ABILITY_MASK).filter(|&a| a != 0).collect();
        (s, field.read8(unit + UNIT_CLASS_INDEX), psynergy)
    }

    /// The Djinn screen in Xian: Isaac's Flint put on standby. The screen's
    /// preview (PsynergyList_RunCompareLoop) at twice its size, beside
    /// Isaac's unit as it stands before and as the game leaves it once the
    /// change is made: his Djinn set, class, stats and Psynergy, read from
    /// the unit, as `djinn-preview.png`.
    pub fn djinn_preview(&mut self) -> Result<(), String> {
        let compare = self.sym("PsynergyList_RunCompareLoop")?;
        let abilities = super::tables::Game::here()?.ability_names()?;
        let classes = self.class_names();
        let mut field = self.enter(XIAN)?;
        let isaac = field.unit(ISAAC)?;
        let before = Self::stats(&field, isaac);
        let djinn_before = field.read32(isaac + UNIT_DJINN_ACTIVE);
        // Djinn; Flint, the first; Standby, the first action; the preview.
        let mut path = open_screen(1);
        path.extend([(0, 30), (KEY_A, 4), (0, 60), (KEY_A, 4), (0, 90)]);
        let (frames, hits) = Self::run_hooked(&mut field, &[compare], &path, &mut |_| {})?;
        let during = Self::stats(&field, isaac);
        let hit = hits.first().ok_or("the preview never ran")?;
        let preview = frames.last().cloned().ok_or("no frames")?;
        // A: the change is made.
        let (_, _) = Self::run_hooked(&mut field, &[], &[(KEY_A, 4), (0, 90)], &mut |_| {})?;
        let after = Self::stats(&field, isaac);
        let djinn_after = field.read32(isaac + UNIT_DJINN_ACTIVE);
        eprintln!("djinn-preview: compare mode {:x?} at frame {}; before {before:?} {djinn_before:#x}; during {during:?}; after {after:?} {djinn_after:#x}", hit.r, hit.frame);
        if during.0 != before.0 {
            return Err("the preview left the unit changed".to_string());
        }
        let (w, h) = (2 * WIDTH + 470, 2 * HEIGHT + 64);
        let mut image = canvas(w, h, BACK);
        paste(&mut image, &screen(&preview), 8, 32, 2);
        text(&mut image, 8, 8, "THE PREVIEW: FLINT TO STANDBY, IS THIS OK?", MARK, None, 2);
        let x = (2 * WIDTH + 24) as i32;
        text(&mut image, x, 8, "ISAAC'S UNIT IN MEMORY", HEAD, None, 2);
        text(&mut image, 8, (2 * HEIGHT + 42) as i32, "WHILE THE PREVIEW IS UP, THE UNIT IS AS BEFORE: THE CHANGE IS MADE ONLY ONCE A CONFIRMS IT", DIM, None, 2);
        let (c1, c2) = (x + 150, x + 290);
        text(&mut image, c1, 32, "BEFORE", DIM, None, 2);
        text(&mut image, c2, 32, "ONCE MADE", DIM, None, 2);
        let set = |bits: u32| format!("{} SET", bits.count_ones());
        let class = |c: u8| classes.get(usize::from(c)).cloned().unwrap_or(format!("{c}"));
        let mut rows: Vec<(String, String, String)> = vec![
            ("VENUS DJINN".into(), set(djinn_before), set(djinn_after)),
            ("CLASS".into(), class(before.1), class(after.1)),
        ];
        for (i, label) in ["MAX HP", "MAX PP", "ATTACK", "DEFENSE", "AGILITY", "LUCK"].iter().enumerate() {
            rows.push((label.to_string(), before.0[i].to_string(), after.0[i].to_string()));
        }
        let mut y = 56;
        for (label, b, a) in &rows {
            let colour = if a == b { TEXT } else { MARK };
            text(&mut image, x, y, label, DIM, None, 2);
            text(&mut image, c1, y, b, TEXT, None, 2);
            text(&mut image, c2, y, a, colour, None, 2);
            y += 20;
        }
        y += 8;
        text(&mut image, x, y, "PSYNERGY", DIM, None, 2);
        let name = |a: &u16| abilities.get(usize::from(*a)).cloned().unwrap_or_default().to_uppercase();
        for (i, a) in before.2.iter().enumerate() {
            text(&mut image, c1, y + 20 * i as i32, &name(a), TEXT, None, 2);
        }
        for (i, a) in after.2.iter().enumerate() {
            text(&mut image, c2, y + 20 * i as i32, &name(a), if before.2.contains(a) { TEXT } else { MARK }, None, 2);
        }
        self.guide_png("djinn-preview.png", &image, "the Djinn screen's preview of Flint put on standby, beside Isaac's unit before and after")
    }

    /// The Status screen in Xian: the member row, then A through Isaac's
    /// status, Psynergy and equipment pages, each found where
    /// SummaryScreen_RunPages's step function is entered
    /// (MemberChooser_RunRearrange, SummaryScreen_SelectCommand,
    /// PsynergyMenu_SelectAction, ItemMenu_SelectItem), at twice its size;
    /// each Djinni figure of the row of four outlined where the menu work
    /// places it (ItemMenuFiguresView positionsX and positionsY, the
    /// image standing on that point), as `status-pages.png`.
    pub fn status_pages(&mut self) -> Result<(), String> {
        let menu = self.sym("gMenuWork")?;
        let steps = [
            ("STATUS_STEP_MEMBER", self.sym("MemberChooser_RunRearrange")?, "the member row"),
            ("STATUS_STEP_STATUS", self.sym("SummaryScreen_SelectCommand")?, "the status page"),
            ("STATUS_STEP_PSYNERGY", self.sym("PsynergyMenu_SelectAction")?, "the Psynergy page"),
            ("STATUS_STEP_EQUIPMENT", self.sym("ItemMenu_SelectItem")?, "the equipment page"),
        ];
        let mut field = self.enter(XIAN)?;
        let mut path = open_screen(3);
        path.extend([(KEY_A, 4), (0, 70), (KEY_A, 4), (0, 70), (KEY_A, 4), (0, 70), (KEY_A, 4), (0, 40)]);
        let mut figures: Vec<[(u32, i16, i16); 4]> = Vec::new();
        let hooks: Vec<u32> = steps.iter().map(|s| s.1).collect();
        let (frames, hits) = Self::run_hooked(&mut field, &hooks, &path, &mut |f| {
            let work = f.read32(menu);
            figures.push([0, 1, 2, 3].map(|i| (f.read32(work + FIGURE_OBJECTS + 4 * i), f.read16(work + FIGURE_X + 2 * i) as i16, f.read16(work + FIGURE_Y + 2 * i) as i16)));
        })?;
        for h in &hits {
            eprintln!("  frame {}: {}", h.frame, steps.iter().find(|s| s.1 == h.pc).map_or("?", |s| s.0));
        }
        // Each step's first entry, shown at the last frame before the next.
        let mut starts: Vec<usize> = Vec::new();
        for (k, s) in steps.iter().enumerate() {
            let from = starts.last().copied().unwrap_or(0);
            let f = hits.iter().find(|h| h.pc == s.1 && h.frame >= from).ok_or(format!("{} never came", s.0))?.frame;
            if k > 0 && f < from {
                return Err("the steps came out of order".to_string());
            }
            starts.push(f);
        }
        for (f, w) in figures.windows(2).enumerate() {
            if w[0] != w[1] {
                eprintln!("  figures change at frame {}: {:?}", f + 1, w[1]);
            }
        }
        let back = hits.iter().find(|h| h.pc == steps[1].1 && h.frame > starts[3]).map_or(frames.len(), |h| h.frame);
        let ends = [starts[1], starts[2], starts[3], back];
        let (cw, ch) = (2 * WIDTH + 16, 2 * HEIGHT + 60);
        let mut image = canvas(2 * cw + 8, 2 * ch + 36, BACK);
        // The key: each element's outline colour.
        let mut kx = 8;
        kx += text(&mut image, kx, (2 * ch + 14) as i32, "OUTLINED DJINN FIGURES: ", DIM, None, 2);
        for (name, colour) in ELEMENTS {
            kx += text(&mut image, kx, (2 * ch + 14) as i32, name, colour, None, 2) + 12;
        }
        for k in 0..4 {
            // Before the A that leaves the page, which is held 4 frames.
            let at = ends[k].saturating_sub(7).min(frames.len() - 1);
            let (x, y) = (8 + (k % 2) * cw, 8 + (k / 2) * ch);
            text(&mut image, x as i32, y as i32, &format!("{} {}", k + 1, steps[k].0), MARK, None, 2);
            text(&mut image, x as i32, y as i32 + 18, &steps[k].2.to_uppercase(), HEAD, None, 2);
            let mut shot = screen(&frames[at]);
            let mut shown = Vec::new();
            for (e, &(object, fx, fy)) in figures[at].iter().enumerate() {
                if object == 0 || !(0..WIDTH as i16).contains(&fx) || !(0..HEIGHT as i16).contains(&fy) {
                    continue;
                }
                shown.push((e, fx, fy));
                // The figure stands on its point, about 24 pixels tall.
                outline(&mut shot, i32::from(fx) - 12, i32::from(fy) - 26, 24, 28, ELEMENTS[e].1);
            }
            paste(&mut image, &shot, x, y + 54, 2);
            let place = match shown.first() {
                Some(&(_, _, fy)) => format!("FIGURES STAND AT SCREEN X {}, Y {fy} PIXELS", shown.iter().map(|s| s.1.to_string()).collect::<Vec<_>>().join(" ")),
                None => format!("FIGURES AT SCREEN Y {} PIXELS: ABOVE THE SCREEN", figures[at][0].2),
            };
            text(&mut image, x as i32, y as i32 + 36, &place, TEXT, None, 2);
            eprintln!("  {}: frame {at}, figures {:?}", steps[k].0, shown);
        }
        self.guide_png("status-pages.png", &image, "the Status screen's member row and three pages, the Djinn figures outlined")
    }
}

/// Isaac's items for the Item screen pictures, beside the four his preset
/// equips: (item, quantity), mixed among them so each order moves them.
const GIVEN: [(&str, u16); 7] = [("Herb", 3), ("Long Sword", 1), ("Antidote", 1), ("Orb of Force", 1), ("Leather Cap", 1), ("Nut", 2), ("Elven Shirt", 1)];

impl<'a> Guide<'a> {
    /// Fills Isaac's inventory: his equipped items with GIVEN between them.
    fn give_items(field: &mut Field, unit: u32, items: &[String]) -> Result<(), String> {
        let held: Vec<u16> = (0..INVENTORY_SLOTS).map(|i| field.read16(unit + UNIT_INVENTORY + 2 * i)).filter(|&e| e != 0).collect();
        let id = |n: &str| items.iter().position(|i| i == n).map(|p| p as u16).ok_or(format!("no item is named {n}"));
        let mut list = Vec::new();
        for (k, &(name, count)) in GIVEN.iter().enumerate() {
            list.push(id(name)? | (count - 1) << INVENTORY_QUANTITY_SHIFT);
            if k % 2 == 0 {
                if let Some(&e) = held.get(k / 2) {
                    list.push(e);
                }
            }
        }
        for &e in held.iter().skip(4) {
            list.push(e);
        }
        for i in 0..INVENTORY_SLOTS {
            field.write16(unit + UNIT_INVENTORY + 2 * i, list.get(i as usize).copied().unwrap_or(0));
        }
        Ok(())
    }

    fn inventory(field: &Field, unit: u32, items: &[String]) -> Vec<String> {
        (0..INVENTORY_SLOTS)
            .map(|i| field.read16(unit + UNIT_INVENTORY + 2 * i))
            .filter(|&e| e != 0)
            .map(|e| {
                let name = items.get(usize::from(e & ITEM_ID_MASK)).cloned().unwrap_or_default().to_uppercase();
                let n = (e >> INVENTORY_QUANTITY_SHIFT) + 1;
                let name = if n > 1 { format!("{name}, {n} HELD") } else { name };
                if e & INVENTORY_EQUIPPED != 0 { format!("{name} (E)") } else { name }
            })
            .collect()
    }

    /// The Item screen's member row in Xian, Isaac holding his four
    /// equipped items with seven others between them, and L held with A
    /// pressed four times: his list as given and after each press, the
    /// screen's item window beside the order the inventory then holds,
    /// read from his unit, each press labelled with the sort mode
    /// ItemMenu_Sort was entered with, as
    /// `item-sorting.png`.
    pub fn item_sorting(&mut self) -> Result<(), String> {
        let sort = self.sym("ItemMenu_Sort")?;
        let items = super::tables::Game::here()?.item_names()?;
        let mut field = self.enter(XIAN)?;
        let isaac = field.unit(ISAAC)?;
        Self::give_items(&mut field, isaac, &items)?;
        let mut lists = vec![Self::inventory(&field, isaac, &items)];
        let mut shots = Vec::new();
        let (frames, _) = Self::run_hooked(&mut field, &[], &open_screen(2), &mut |_| {})?;
        shots.push(frames.last().cloned().ok_or("no frames")?);
        let mut modes = Vec::new();
        for _ in 0..4 {
            let (frames, hits) = Self::run_hooked(&mut field, &[sort], &[(KEY_L, 10), (KEY_L | KEY_A, 4), (KEY_L, 10), (0, 30)], &mut |_| {})?;
            let hit = hits.first().ok_or("L and A sorted nothing")?;
            modes.push(hit.r[1]);
            lists.push(Self::inventory(&field, isaac, &items));
            shots.push(frames.last().cloned().ok_or("no frames")?);
        }
        eprintln!("item-sorting: modes {modes:?}");
        for l in &lists {
            eprintln!("  {l:?}");
        }
        const SORTS: [&str; 4] = ["LIST_SORT_EQUIPPED_THEN_ITEMS", "LIST_SORT_EQUIPPED_THEN_EQUIPMENT", "LIST_SORT_EQUIPMENT_FIRST", "LIST_SORT_ITEMS_FIRST"];
        // The item window: tiles 13 to 30 across, 3 to 13 down.
        let (wx, wy, ww, wh): (i32, i32, usize, usize) = (13 * 8, 3 * 8, 17 * 8, 10 * 8);
        // Two to a row: the window at twice its size, the list beside it.
        let (cw, ch) = (2 * ww + 210, 2 * wh + 70);
        let mut image = canvas(2 * cw + 8, 3 * ch + 8, BACK);
        for k in 0..5 {
            let (x, y) = (8 + (k % 2) * cw, 8 + (k / 2) * ch);
            let (head, sub) = if k == 0 { ("AS GIVEN".to_string(), String::new()) } else { (format!("PRESS {k} OF A WITH L HELD:"), SORTS.get(modes[k - 1] as usize).copied().unwrap_or("?").to_string()) };
            text(&mut image, x as i32, y as i32, &head, MARK, None, 2);
            text(&mut image, x as i32, y as i32 + 18, &sub, HEAD, None, 2);
            paste(&mut image, &crop(&screen(&shots[k]), wx, wy, ww, wh), x, y + 40, 2);
            for (n, line) in lists[k].iter().enumerate() {
                text(&mut image, (x + 2 * ww + 10) as i32, (y + 40 + 16 * n) as i32, line, if line.ends_with("(E)") { MARK } else { TEXT }, None, 2);
            }
        }
        let (x, y) = (8 + cw, 8 + 2 * ch + 40);
        let key = ["EACH LIST IS ISAAC'S INVENTORY", "AS HIS UNIT HOLDS IT, IN ORDER.", "", "IN YELLOW, MARKED (E): EQUIPPED.", "THE SORT EACH PRESS USED IS THE", "ONE ITEMMENU_SORT", "WAS ENTERED WITH."];
        for (n, l) in key.iter().enumerate() {
            text(&mut image, x as i32, (y + 16 * n) as i32, l, if n == 3 { MARK } else { DIM }, None, 2);
        }
        self.guide_png("item-sorting.png", &image, "Isaac's items in each of the four sort orders")
    }

    /// The status panel (StatusPanel_Draw's window, tile (0, 5),
    /// 13 by 12 tiles) once for each kind, in Xian with Isaac holding his
    /// equipped items and seven more: on the Item screen's member row
    /// (STATUS_PANEL_EXPERIENCE), with R held there (STATUS_PANEL_STATS),
    /// and with the cursor on a weapon he does not wear (STATUS_PANEL_EQUIP)
    /// and on the Orb of Force (STATUS_PANEL_LEARN), each kind read from the
    /// call's mode; and STATUS_PANEL_CLASS, which no call passes, drawn by
    /// giving the member row's first call that mode instead. As
    /// `status-panels.png`.
    pub fn status_panels(&mut self) -> Result<(), String> {
        let draw = self.sym("StatusPanel_Draw")?;
        let items = super::tables::Game::here()?.item_names()?;
        // (kind's name, mode, screen) for each panel.
        let mut panels: Vec<(String, u32, Vec<u16>, String)> = Vec::new();
        let names = |m: u32| match m & 0xff {
            0 => "STATUS_PANEL_EXPERIENCE",
            2 | 3 => "STATUS_PANEL_EQUIP",
            4 => "STATUS_PANEL_LEARN",
            6 => "STATUS_PANEL_CLASS",
            8 => "STATUS_PANEL_STATS",
            _ => "?",
        };
        {
            let mut field = self.enter(XIAN)?;
            let isaac = field.unit(ISAAC)?;
            Self::give_items(&mut field, isaac, &items)?;
            // The member row; R held there and let go; A for Isaac's list
            // and the cursor along all of it.
            let mut path = open_screen(2);
            path.extend([(KEY_R, 40), (0, 30), (KEY_A, 4), (0, 40)]);
            // The list runs down its columns of five: down to the Long Sword,
            // back up, and right to the Orb of Force.
            for keys in [KEY_DOWN, KEY_DOWN, KEY_UP, KEY_UP, KEY_RIGHT] {
                path.extend([(keys, 4), (0, 36)]);
            }
            let mut inventories: Vec<Vec<u16>> = Vec::new();
            let (frames, hits) = Self::run_hooked(&mut field, &[draw], &path, &mut |f| inventories.push((0..INVENTORY_SLOTS).map(|i| f.read16(isaac + UNIT_INVENTORY + 2 * i)).collect()))?;
            let item_of = |h: &Hit| -> (String, bool) {
                let e = inventories[h.frame][h.r[2] as usize];
                (items.get(usize::from(e & ITEM_ID_MASK)).cloned().unwrap_or_default(), e & INVENTORY_EQUIPPED != 0)
            };
            for h in &hits {
                eprintln!("  frame {}: owner {} slot {} {:?} mode {:#x}", h.frame, h.r[1], h.r[2], item_of(h), h.r[3]);
            }
            // The list opens at the A after R; a hit before it is the
            // member row.
            let list_from = open_screen(2).iter().map(|p| p.1 as usize).sum::<usize>() + 70;
            let wants: [(u32, &dyn Fn(&Hit) -> bool, &str); 4] = [
                (0, &|h| h.frame < list_from, "ON THE ITEM SCREEN'S|MEMBER ROW"),
                (8, &|h| h.frame < list_from, "ON THE MEMBER ROW,|WITH R HELD"),
                (2, &|h| item_of(h) == ("Long Sword".to_string(), false), ""),
                (4, &|h| item_of(h).0 == "Orb of Force", ""),
            ];
            for (want, pick, what) in wants {
                let k = hits.iter().position(|h| h.r[3] & 0xff == want && pick(h)).ok_or(format!("no call drew {}", names(want)))?;
                let hit = hits[k];
                let end = hits.get(k + 1).map_or(frames.len(), |n| n.frame).min(hit.frame + 30);
                let note = if what.is_empty() { format!("THE CURSOR ON|THE {}", item_of(&hit).0.to_uppercase()) } else { what.to_string() };
                eprintln!("  {}: frame {} shown {}", names(want), hit.frame, end - 1);
                panels.push((names(want).to_string(), hit.r[3], frames[end - 1].clone(), note));
            }
        }
        {
            // STATUS_PANEL_CLASS: the member row's first call given mode 6.
            let mut field = self.enter(XIAN)?;
            field.register_pokes.push((draw, 3, 6));
            let (frames, hits) = Self::run_hooked(&mut field, &[draw], &open_screen(2), &mut |_| {})?;
            let hit = hits.first().ok_or("the member row drew no panel")?;
            eprintln!("  forced: frame {} mode {:#x} given 6", hit.frame, hit.r[3]);
            panels.push(("STATUS_PANEL_CLASS".to_string(), 6, frames.last().cloned().unwrap(), "NO CALL PASSES IT: GIVEN|TO THE MEMBER ROW'S CALL".to_string()));
        }
        let (px, py, pw, ph) = (0usize, 5 * 8usize, 13 * 8usize, 12 * 8usize);
        // Three to a row, each at twice its size under its kind and where
        // it was drawn.
        let (cw, ch) = (2 * pw + 112, 2 * ph + 76);
        let mut image = canvas(3 * cw + 8, 2 * ch + 8, BACK);
        for (k, (kind, _, shot, note)) in panels.iter().enumerate() {
            let (x, y) = (8 + (k % 3) * cw, 8 + (k / 3) * ch);
            text(&mut image, x as i32, y as i32, kind, MARK, None, 2);
            let (first, second) = note.split_once('|').unwrap_or((note, ""));
            text(&mut image, x as i32, y as i32 + 18, first, TEXT, None, 2);
            text(&mut image, x as i32, y as i32 + 34, second, TEXT, None, 2);
            paste(&mut image, &crop(&screen(shot), px as i32, py as i32, pw, ph), x, y + 56, 2);
        }
        self.guide_png("status-panels.png", &image, "the status panel once for each kind")
    }
}
