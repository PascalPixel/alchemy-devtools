//! Take-apart pictures for the place overlays of Vale, Sol Sanctum and the
//! Sanctum: each plays the moment a place's comment describes and draws the
//! screen beside the layer, the palettes or the actors that make it, from
//! the game's own memory.

use super::animation;
use super::field::{Field, Image, Setup, Spot};
use super::guide::Guide;
use super::guide_more::{canvas, hue, outline, paste, screen};
use super::guide_take::{alone, mark_cells, palette_grid, room_map, text};
use super::mgba::{HEIGHT, WIDTH};

const KEY_A: u32 = 0x1;
const KEY_RIGHT: u32 = 0x10;
const KEY_LEFT: u32 = 0x20;
const KEY_UP: u32 = 0x40;
const KEY_DOWN: u32 = 0x80;

const RED: u32 = 0xff_30_30;
const YELLOW: u32 = 0xff_e0_30;
const CYAN: u32 = 0x30_d0_f0;
const WHITE: u32 = 0xff_ff_ff;
const GREY: u32 = 0x40_40_40;

/// DISPCNT bits 8 to 12: BG0 to BG3, then the sprites.
const SPRITES: u16 = 0x1000;
const BACKGROUNDS: u16 = 0x0f00;
/// struct Actor: its position (16.16 pixels) and scale.
const OBJ_X: u32 = 0x08;
const OBJ_Z: u32 = 0x10;
/// Its object kind: 0 draws nothing.
const OBJ_KIND: u32 = 0x54;
const OBJ_SCALE_X: u32 = 0x18;

/// One panel of a take-apart frame.
#[derive(Clone)]
pub(super) enum Panel {
    /// The screen as the player sees it, the actors in `marks` boxed.
    Screen,
    /// The sprites alone.
    Sprites,
    /// The sprites alone with these actors taken away.
    SpritesWithout(Vec<u16>),
    /// The backgrounds alone.
    Backgrounds,
    /// The 16 background palettes, or the 16 sprite palettes.
    Palettes(bool),
}

/// An actor to box on the screen panel: its id, colour and label.
pub(super) type Mark = (u16, u32, &'static str);

/// Draws a box round where actor `id` stands on the screen, at its height,
/// as wide as its scale (16 pixels at full size), and its label above.
fn mark_actor(image: &mut Image, field: &Field, (id, colour, label): Mark, scale: i32) {
    let Some(a) = field.actor(id) else { return };
    if a.x == 0 && a.z == 0 {
        return;
    }
    let (vx, vy) = field.view();
    let address = field.actor_address(id);
    let size = (field.read32(address + OBJ_SCALE_X) as i32 >> 12).clamp(2, 32);
    let (sx, sy) = (a.x - vx, a.z - (a.y >> 16) - vy);
    outline(image, scale * (sx - size / 2), scale * (sy - size), scale * size, scale * size, colour);
    text(image, scale * (sx - size / 2), scale * (sy - size) - 7, label, WHITE, Some(colour), 1);
}

/// One take-apart frame: `panels` side by side under a white margin, each at
/// the GBA's size (palettes 6 pixels a colour).
pub(super) fn compose(field: &mut Field, panels: &[Panel], marks: &[Mark]) -> Result<Image, String> {
    // Every panel shows the same moment, two frames on from `field`'s: a
    // layer shown alone is drawn a frame after DISPCNT is written, and an
    // actor taken away leaves the sprite list a frame after that.
    let start = field.save();
    let ahead = |field: &mut Field, n: u32| -> Result<(), String> {
        field.resume(&start);
        field.frames(n)
    };
    let mut images = Vec::new();
    for panel in panels {
        let mut image = match panel {
            Panel::Screen => {
                ahead(field, 2)?;
                screen(&field.picture())
            }
            Panel::Sprites => {
                ahead(field, 1)?;
                alone(field, SPRITES)?
            }
            Panel::Backgrounds => {
                ahead(field, 1)?;
                alone(field, BACKGROUNDS)?
            }
            Panel::SpritesWithout(ids) => {
                field.resume(&start);
                let saved = field.save();
                for &id in ids {
                    let a = field.actor_address(id);
                    // Off the map and drawing nothing: an actor whose
                    // update puts it back still draws no sprite, and a
                    // shadow stays with the position.
                    if a != 0 {
                        field.write32(a + OBJ_X, 0);
                        field.write32(a + OBJ_Z, 0);
                        field.write8(a + OBJ_KIND, 0);
                    }
                }
                // The sprite list is built a frame ahead of the screen.
                field.frames(1)?;
                let image = alone(field, SPRITES)?;
                field.resume(&saved);
                image
            }
            Panel::Palettes(sprites) => {
                ahead(field, 2)?;
                palette_grid(field, if *sprites { 256 } else { 0 }, 16, 6)
            }
        };
        if matches!(panel, Panel::Screen) {
            for &m in marks {
                mark_actor(&mut image, field, m, 1);
            }
        }
        images.push(image);
    }
    field.resume(&start);
    let w: usize = images.iter().map(|i| i.width + 4).sum::<usize>() + 4;
    let mut out = canvas(w, HEIGHT + 8, WHITE);
    let mut x = 4;
    for image in &images {
        paste(&mut out, image, x, 4, 1);
        x += image.width + 4;
    }
    Ok(out)
}

impl<'a> Guide<'a> {
    /// Enters `place` from a new game with these flags set and cleared and
    /// stops where the scene's setup is about to run.
    pub(super) fn arrive(&self, place: (u16, u16), set: &[&str], clear: &[&str]) -> Result<Field<'a>, String> {
        let resolve = |names: &[&str]| -> Result<Vec<u16>, String> { names.iter().map(|n| self.flag(n)).collect() };
        let setup = Setup { scene: place.0, entrance: place.1, set: resolve(set)?, clear: resolve(clear)? };
        Field::enter(self.rom, self.symbols, &setup)
    }

    /// Plays on from where `field` is for at most `limit` frames, pressing A
    /// through each page that waits, until the player has had control for
    /// half a second. Each frame `keep` says whether to draw it; drawn frames
    /// are composed of `panels` with `marks`, the frames between kept as they
    /// were so the GIF runs at the game's speed.
    pub(super) fn play_apart(field: &mut Field, limit: u32, panels: &[Panel], marks: &[Mark], keep: &mut dyn FnMut(&Field) -> bool) -> Result<Vec<Vec<u32>>, String> {
        let mut frames: Vec<Vec<u32>> = Vec::new();
        let (mut waited, mut idle, mut stepping) = (0, 0, 0);
        let mut last_steps = field.steps();
        for i in 0..limit {
            field.frames(1)?;
            if keep(field) {
                if i % 2 == 0 || frames.is_empty() {
                    frames.push(compose(field, panels, marks)?.pixels);
                } else {
                    let last = frames.last().cloned().unwrap_or_default();
                    frames.push(last);
                }
            }
            let steps = field.steps();
            stepping = if steps != last_steps { stepping + 1 } else { 0 };
            last_steps = steps;
            if stepping >= 30 {
                break;
            }
            waited = if field.waiting() { waited + 1 } else { 0 };
            idle += 1;
            if waited >= 20 || (idle >= 240 && steps == last_steps && field.waiting()) {
                field.hold(KEY_A);
                field.frames(2)?;
                field.hold(0);
                waited = 0;
                idle = 0;
            }
        }
        Ok(frames)
    }

    fn place_gif(&mut self, file: &str, frames: &[Vec<u32>], what: &str) -> Result<(), String> {
        let width = frames.first().map(|f| f.len() / (HEIGHT + 8)).ok_or_else(|| format!("{file}: no frames"))?;
        let written = animation::write(&self.out.join(file), frames, width)?;
        self.written.push((file.to_string(), written.bytes, format!("{what}; {} frames", written.frames)));
        Ok(())
    }

    /// The Sanctum after the eruption (SANCTUM_FROM_VALE_ERUPTION): the
    /// account of Sol Sanctum in sepia, the screen beside the background
    /// palettes the sepia transform rewrites, as `sanctum-account-sepia.gif`;
    /// and each time the Great Healer's Psynergy shows, the screen beside the
    /// sprites and the sprites without his afterimage (ACTOR_HEALER_AFTERIMAGE,
    /// 14), the healer and the afterimage boxed, as
    /// `sanctum-healer-afterimages.gif`.
    pub fn sanctum_account(&mut self) -> Result<(), String> {
        let reenact = self.overlay_sym("Sanctum_GetActorTemplates|Sanctum_GetObjectEventTemplates", "Sanctum_ReenactSolSanctum")?;
        let request = self.overlay_sym("Sanctum_GetActorTemplates|Sanctum_GetObjectEventTemplates", "Sanctum_RequestQuestAcceptance")?;
        let mut field = self.arrive((9, 20), &["FLAG_SOL_SANCTUM_ERUPTED", "FLAG_SOL_SANCTUM_STAR_ROOM_COLLAPSED"], &["FLAG_VALE_PARTY_LEFT", "FLAG_VALE_STORM", "FLAG_CHILDHOOD"])?;
        let phase = std::rc::Rc::new(std::cell::Cell::new(0u8));
        let set = phase.clone();
        field.probe_pcs = vec![reenact, request];
        field.probe = Some(Box::new(move |_gba, pc, _frame| {
            if pc == reenact {
                set.set(1);
            } else if pc == request {
                set.set(2);
            }
        }));
        let saved = field.save();
        // The account: from the reenactment's start to the quest's request.
        let sepia = [Panel::Screen, Panel::Palettes(false), Panel::Palettes(true)];
        let account = Self::play_apart(&mut field, 6000, &sepia, &[], &mut |_f| phase.get() == 1)?;
        // Its first ten seconds and the last five, the fade back to the
        // plain colours.
        let account: Vec<Vec<u32>> = if account.len() > 900 { account[..600].iter().chain(&account[account.len() - 300..]).cloned().collect() } else { account };
        self.place_gif("sanctum-account-sepia.gif", &account, "the Sanctum's account of Sol Sanctum in sepia, beside the background and sprite palettes")?;
        // The healer's Psynergy, wherever his afterimage stands.
        field.load(&saved);
        phase.set(0);
        let panels = [Panel::Screen, Panel::Sprites, Panel::SpritesWithout(vec![14])];
        let marks: [Mark; 2] = [(8, YELLOW, "HEALER"), (14, RED, "AFTERIMAGE")];
        let healer = Self::play_apart(&mut field, 30000, &panels, &marks, &mut |f| match (f.actor(14), f.actor(8)) {
            (Some(a), Some(h)) => (a.x - h.x).abs() <= 4 && (a.z - h.z).abs() <= 3 && !f.waiting(),
            _ => false,
        })?;
        field.probe = None;
        self.place_gif("sanctum-healer-afterimages.gif", &healer, "the Great Healer's Psynergy: the screen, the sprites, the sprites without his afterimage")
    }

    /// A rising effect: actor `effect` (a blank actor, ACTOR_RISING_EFFECT)
    /// on the platform while it stands there, the screen beside the sprites
    /// and the sprites without it, the riser and the effect boxed.
    fn rising(&mut self, file: &str, place: (u16, u16), set: &[&str], clear: &[&str], effect: u16, limit: u32) -> Result<(), String> {
        let mut field = self.arrive(place, set, clear)?;
        let panels = [Panel::Screen, Panel::Sprites, Panel::SpritesWithout(vec![effect])];
        let marks: [Mark; 1] = [(effect, RED, "EFFECT")];
        let frames = Self::play_apart(&mut field, limit, &panels, &marks, &mut |f| f.actor(effect).is_some_and(|a| a.x == 471 && a.z == 290))?;
        self.place_gif(file, &frames, "an actor rising with ACTOR_RISING_EFFECT: the screen, the sprites, the sprites without the effect")
    }

    /// Isaac rising into the Elemental Star Room on the first arrival, as
    /// `star-room-rising.gif`.
    pub fn star_room_rising(&mut self) -> Result<(), String> {
        self.rising("star-room-rising.gif", (17, 10), &[], &["FLAG_VALE_PARTY_LEFT", "FLAG_SOL_SANCTUM_STAR_ROOM_EXPLAINED", "FLAG_VALE_STORM", "FLAG_CHILDHOOD"], 8, 3000)
    }

    /// Saturos's group leaving the collapsing chamber one by one, as
    /// `eruption-departures.gif`.
    pub fn eruption_departures(&mut self) -> Result<(), String> {
        self.rising("eruption-departures.gif", (18, 10), &[], &["FLAG_VALE_PARTY_LEFT", "FLAG_VALE_STORM", "FLAG_CHILDHOOD"], 8, 40000)
    }

    /// The statue room's three rows of sky sprites drifting behind the
    /// colonnade: the screen, the sprites alone and the backgrounds alone,
    /// for eight seconds, as `statue-sky-rows.gif`.
    pub fn statue_sky_rows(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with((12, 0), &[])?;
        let field = &mut entered.field;
        let placed = field.save();
        let height = field.ground(0, 288, 120)?;
        field.stand(&placed, &Spot { x: 288, z: 120, layer: 0, height, facing: 0xc000, walk: 0, cell: None })?;
        // The top of the screen, where the rows are drawn, twice its size:
        // the screen, the sprites alone and the backgrounds alone, one above
        // another.
        const ROWS: usize = 72;
        let mut frames = Vec::new();
        for i in 0..480 {
            field.frames(1)?;
            if i % 2 == 1 {
                let last = frames.last().cloned().unwrap_or_default();
                frames.push(last);
                continue;
            }
            // The screen a frame on, the moment the layers alone show.
            let now = field.save();
            field.frames(1)?;
            let whole = screen(&field.picture());
            field.resume(&now);
            let shots = [whole, alone(field, SPRITES)?, alone(field, BACKGROUNDS)?];
            let mut out = canvas(2 * WIDTH + 8, 3 * (2 * ROWS + 4) + 4, WHITE);
            for (k, shot) in shots.iter().enumerate() {
                let top = super::guide_take::crop(shot, 0, 0, WIDTH, ROWS);
                paste(&mut out, &top, 4, 4 + k * (2 * ROWS + 4), 2);
            }
            frames.push(out.pixels);
        }
        let written = animation::write(&self.out.join("statue-sky-rows.gif"), &frames, 2 * WIDTH + 8)?;
        self.written.push(("statue-sky-rows.gif".to_string(), written.bytes, format!("the statue room's drifting sky rows: the screen, the sprites, the backgrounds; {} frames", written.frames)));
        Ok(())
    }

    /// The game's opening in Vale (VALE_FROM_TITLE): Garet's Psynergy, from
    /// the first flash of his sprite until the hand of Move vanishes, the
    /// screen beside the sprites and the sprite palettes, Garet, the pillar
    /// and the hand boxed, as `opening-move.gif`.
    pub fn vale_opening(&mut self) -> Result<(), String> {
        let begin = self.overlay_sym("Vale_GetActorTemplates|Vale_GetObjectEventTemplates", "Scene_StartEventPsynergy")?;
        let end = self.overlay_sym("Vale_GetActorTemplates|Vale_GetObjectEventTemplates", "Scene_EndEventPsynergy")?;
        let mut field = self.arrive((4, 16), &[], &["FLAG_VALE_PARTY_LEFT", "FLAG_VALE_STORM", "FLAG_CHILDHOOD"])?;
        let on = std::rc::Rc::new(std::cell::Cell::new(false));
        let set = on.clone();
        field.probe_pcs = vec![begin, end];
        field.probe = Some(Box::new(move |_gba, pc, _frame| set.set(pc == begin || (pc != end && set.get()))));
        let panels = [Panel::Screen, Panel::Sprites, Panel::Palettes(true)];
        let marks: [Mark; 3] = [(1, YELLOW, "GARET"), (9, CYAN, "PILLAR"), (14, RED, "HAND")];
        let mut tail = 0;
        let frames = Self::play_apart(&mut field, 20000, &panels, &marks, &mut |_f| {
            if on.get() {
                tail = 30;
                true
            } else if tail > 0 {
                tail -= 1;
                true
            } else {
                false
            }
        })?;
        field.probe = None;
        self.place_gif("opening-move.gif", &frames, "the opening's Psynergy: the screen, the sprites, the sprite palettes")
    }

    /// A villager of Vale Gate showing his Psynergy by day
    /// (ValePlaza_DemonstratePsynergy): talked to, then the screen beside the
    /// sprites and the sprite palettes while his display lasts, as
    /// `psynergy-display.gif`.
    pub fn vale_gate_display(&mut self) -> Result<(), String> {
        let begin = self.overlay_sym("ValePlaza_GetActorTemplates|ValePlaza_GetObjectEventTemplates", "Scene_StartEventPsynergy")?;
        let end = self.overlay_sym("ValePlaza_GetActorTemplates|ValePlaza_GetObjectEventTemplates", "Scene_EndEventPsynergy")?;
        let mut entered = self.enter_story((5, 0), &[], &["FLAG_VALE_PARTY_LEFT", "FLAG_VALE_STORM", "FLAG_CHILDHOOD"])?;
        let field = &mut entered.field;
        // ACTOR_STRONGMAN_SHOWING_PSYNERGY (ACTOR_FIRST_PLACED + 9).
        let id = 17u16;
        let spot = *field.spots(super::story::Trigger::Talk(id))?.first().ok_or("nowhere to talk to him from")?;
        field.load(&entered.placed);
        field.until_control()?;
        field.put_leader(&spot);
        super::guide_take::settle_view(field)?;
        let on = std::rc::Rc::new(std::cell::Cell::new(false));
        let set = on.clone();
        field.probe_pcs = vec![begin, end];
        field.probe = Some(Box::new(move |_gba, pc, _frame| set.set(pc == begin || (pc != end && set.get()))));
        field.hold(KEY_A);
        field.frames(2)?;
        field.hold(0);
        let panels = [Panel::Screen, Panel::Sprites, Panel::Palettes(true)];
        let marks: [Mark; 1] = [(id, YELLOW, "VILLAGER")];
        let mut tail = 0;
        let frames = Self::play_apart(field, 4000, &panels, &marks, &mut |_f| {
            if on.get() {
                tail = 20;
                true
            } else if tail > 0 {
                tail -= 1;
                true
            } else {
                false
            }
        })?;
        field.probe = None;
        self.place_gif("psynergy-display.gif", &frames, "a villager's Psynergy display: the screen, the sprites, the sprite palettes")
    }

    /// Garet's sleeping grandfather as the party comes home after Kalay
    /// (CottageRoomsA_GaretComesHome): the screen beside the sprites, the three
    /// letters Z (actors 23 to 25) boxed and the frame of their 180-frame
    /// cycle written, as `sleep-z.gif`.
    pub fn cottage_rooms_a_sleep(&mut self) -> Result<(), String> {
        let count = self.sym("gFrameCount")?;
        let mut field = self.arrive((7, 6), &["FLAG_VALE_PARTY_LEFT", "FLAG_KALAY_REACHED"], &["FLAG_VALE_STORM", "FLAG_CHILDHOOD"])?;
        let panels = [Panel::Screen, Panel::Sprites];
        let marks: [Mark; 4] = [(8, YELLOW, "GRANDFATHER"), (23, RED, "Z1"), (24, CYAN, "Z2"), (25, 0x60_f0_60, "Z3")];
        let mut frames = Vec::new();
        let mut waited = 0;
        for i in 0..3000u32 {
            field.frames(1)?;
            // Read on through the family's pages; the letters drift on.
            waited = if field.waiting() { waited + 1 } else { 0 };
            if waited >= 20 {
                field.hold(KEY_A);
                field.frames(2)?;
                field.hold(0);
                waited = 0;
            }
            // From the first letter on, two whole cycles of 180 frames.
            if frames.is_empty() && !field.actor(23).is_some_and(|a| a.x != 0) {
                continue;
            }
            if i % 2 == 0 || frames.is_empty() {
                let mut image = compose(&mut field, &panels, &marks)?;
                let cycle = field.read32(count) % 180;
                text(&mut image, 8, HEIGHT as i32 - 6, &format!("FRAME {cycle:3} OF 180"), WHITE, Some(GREY), 1);
                frames.push(image.pixels);
            } else {
                let last = frames.last().cloned().unwrap_or_default();
                frames.push(last);
            }
            if frames.len() >= 360 {
                break;
            }
        }
        self.place_gif("sleep-z.gif", &frames, "Garet's grandfather asleep: the screen and the sprites, the letters Z boxed")
    }

    /// Vale on the stormy night by Kraden's cottage (ValeKradensCottage): the room's
    /// map with the two rocks boxed and the six cells the rockslide rewrites
    /// outlined with their collision codes, beside the same cells by day, as
    /// `rockslide-cells.png`.
    pub fn vale_kraden_rockslide(&mut self) -> Result<(), String> {
        const CELLS: [(i32, i32); 6] = [(14, 21), (15, 21), (23, 19), (24, 19), (23, 20), (24, 20)];
        let mut maps = Vec::new();
        for (storm, set, clear) in [(true, vec!["FLAG_VALE_STORM", "FLAG_CHILDHOOD"], vec!["FLAG_VALE_PARTY_LEFT"]), (false, vec![], vec!["FLAG_VALE_PARTY_LEFT", "FLAG_VALE_STORM", "FLAG_CHILDHOOD"])] {
            let mut e = self.enter_story((6, 1), &set, &clear)?;
            let settled = e.field.save();
            let rocks: Vec<(i32, i32)> = [9u16, 10].iter().filter_map(|&id| e.field.actor(id)).filter(|a| a.x != 0).map(|a| (a.x, a.z - (a.y >> 16))).collect();
            let codes: Vec<i32> = CELLS.iter().map(|&(cx, cz)| e.field.cell(0, cx, cz)).collect();
            let (mut map, (ox, oz)) = room_map(&mut e.field, &settled)?;
            let marks: Vec<(i32, i32, i32)> = CELLS.iter().map(|&(cx, cz)| (cx, cz, 0)).collect();
            mark_cells(&mut map, (ox, oz), &marks, if storm { RED } else { YELLOW }, 0.35);
            for (k, &(cx, cz)) in CELLS.iter().enumerate() {
                text(&mut map, cx * 16 - ox + 2, cz * 16 - oz + 2, &codes[k].to_string(), WHITE, Some(if storm { RED } else { YELLOW }), 1);
            }
            for (x, z) in rocks {
                outline(&mut map, x - ox - 16, z - oz - 24, 32, 28, CYAN);
            }
            maps.push(map);
        }
        let (w, h) = (maps[0].width + maps[1].width + 12, maps[0].height.max(maps[1].height) + 8);
        let mut out = canvas(w, h, WHITE);
        paste(&mut out, &maps[0], 4, 4, 1);
        paste(&mut out, &maps[1], maps[0].width + 8, 4, 1);
        self.guide_png("rockslide-cells.png", &out, "Kraden's cottage on the stormy night and by day, the rockslide's cells outlined with their codes")
    }

    /// Kite's part of Vale Cave's second room with Kite behind its ledge
    /// (FLAG_VALE_CAVE_KITE_BEHIND_LEDGE): the leader walks round the ledge
    /// from trigger cell to trigger cell, each frame twice the size with the
    /// twelve trigger cells (codes 45 to 56) outlined and numbered, the
    /// matching points of gValeCave_WalkTargets drawn and numbered, and Kite
    /// boxed, as `kite-ledge.gif`.
    pub fn vale_cave_kite(&mut self) -> Result<(), String> {
        const TARGETS: [(i32, i32); 12] = [(760, 296), (760, 280), (776, 280), (792, 280), (808, 280), (824, 280), (824, 296), (824, 312), (808, 312), (792, 312), (776, 312), (760, 312)];
        let mut e = self.enter_story((94, 5), &["FLAG_VALE_PARTY_LEFT", "FLAG_VALE_CAVE_KITE_BEHIND_LEDGE"], &["FLAG_VALE_STORM", "FLAG_CHILDHOOD"])?;
        let mut triggers: Vec<(u16, i32, i32)> = Vec::new();
        for code in 45..57u16 {
            for (_, cx, cz) in e.field.cells_with_code(code)? {
                triggers.push((code, cx, cz));
            }
        }
        // Walk to each trigger's cell in turn, round the ring and back.
        let mut order: Vec<(i32, i32)> = (45..57u16).filter_map(|c| triggers.iter().find(|t| t.0 == c).map(|t| (t.1 * 16 + 8, t.2 * 16 + 8))).collect();
        if let Some(&first) = order.first() {
            order.push(first);
        }
        let field = &mut e.field;
        let mut frames = Vec::new();
        let draw = |field: &mut Field, frames: &mut Vec<Vec<u32>>| -> Result<(), String> {
            let (vx, vy) = field.view();
            let mut image = canvas(2 * WIDTH, 2 * HEIGHT, 0);
            paste(&mut image, &screen(&field.picture()), 0, 0, 2);
            for &(code, cx, cz) in &triggers {
                let (sx, sy) = (2 * (cx * 16 - vx), 2 * (cz * 16 - vy));
                let colour = hue(usize::from(code - 45));
                outline(&mut image, sx, sy, 32, 32, colour);
                text(&mut image, sx + 2, sy + 2, &(code - 45).to_string(), WHITE, Some(colour), 1);
            }
            for (k, &(x, z)) in TARGETS.iter().enumerate() {
                let (sx, sy) = (2 * (x - vx), 2 * (z - vy));
                let colour = hue(k);
                outline(&mut image, sx - 3, sy - 3, 6, 6, colour);
                text(&mut image, sx + 4, sy - 3, &k.to_string(), WHITE, Some(colour), 1);
            }
            mark_actor(&mut image, field, (8, RED, "KITE"), 2);
            frames.push(image.pixels.clone());
            frames.push(image.pixels);
            Ok(())
        };
        for &(tx, tz) in &order {
            for _ in 0..240 {
                let Some(l) = field.leader() else { break };
                let (dx, dz) = (tx - l.x, tz - l.z);
                if dx.abs() <= 2 && dz.abs() <= 2 {
                    break;
                }
                let mut keys = 0;
                if dx > 2 {
                    keys |= KEY_RIGHT;
                } else if dx < -2 {
                    keys |= KEY_LEFT;
                }
                if dz > 2 {
                    keys |= KEY_DOWN;
                } else if dz < -2 {
                    keys |= KEY_UP;
                }
                field.hold(keys);
                field.frames(2)?;
                draw(field, &mut frames)?;
            }
        }
        field.hold(0);
        for _ in 0..30 {
            field.frames(2)?;
            draw(field, &mut frames)?;
        }
        let written = animation::write(&self.out.join("kite-ledge.gif"), &frames, 2 * WIDTH)?;
        self.written.push(("kite-ledge.gif".to_string(), written.bytes, format!("Kite keeping to the far side of its ledge; {} frames", written.frames)));
        Ok(())
    }
}
