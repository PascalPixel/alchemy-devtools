//! Recordings for the scene, map, debug, game and graphics folder guides:
//! each one moment the player sees, played from a built edition with the
//! pad held as a player would, written under the guide folder it belongs
//! to (`scene/exit-region.gif`).

use super::animation::is_black_bgr555;
use super::field::{Field, CLEARED};
use super::guide::{Entered, Guide};
use super::guide_more::screen;
use super::mgba::{HEIGHT, WIDTH};
use super::source;
use super::story::Trigger;

const KEY_A: u32 = 0x1;
const KEY_B: u32 = 0x2;
const KEY_RIGHT: u32 = 0x10;
const KEY_LEFT: u32 = 0x20;
const KEY_UP: u32 = 0x40;
const KEY_DOWN: u32 = 0x80;

/// The modes this file plays, after guide_lucky::MODES.
pub const MODES: &[&str] = &["scene-exit-region", "game-djinni-flag", "map-recolour", "game-pack-full", "scene-log-state", "debug-menu-test-twice"];

/// Kolima's tunnel (SceneId_KolimaTunnel), entered from Kolima at the foot
/// of the ladder up (KOLIMA_TUNNEL_FROM_KOLIMA).
const KOLIMA_TUNNEL: (&str, u16) = ("KolimaTunnel", 2);
/// Kolima as the party comes up out of its tunnel (KOLIMA_FROM_KOLIMA_TUNNEL).
const KOLIMA_FROM_TUNNEL: (&str, u16) = ("Kolima", 8);
/// Kolima Forest's first scene, entered from the next one, the glade
/// (KOLIMA_FOREST_ENTRANCE_FROM_KOLIMA_FOREST_GLADE), and where its river
/// log lies (gKolimaForestEntranceActorPlacements), in pixels.
const KOLIMA_FOREST_ENTRANCE: (&str, u16) = ("KolimaForestEntrance", 2);
const RIVER_LOG: (i32, i32) = (360, 304);
/// Where the party arrives from the World Map (KOLIMA_FOREST_ENTRANCE_FROM_WORLD_MAP),
/// beside the way out to it at the scene's west edge, and the river bank
/// west of the log, from where the party crosses it.
const KOLIMA_FOREST_WAY_OUT: (i32, i32) = (172, 602);
const LOG_CROSSED: (i32, i32) = (280, 303);
/// Facing west, in 1/65536 turns.
const WEST: u32 = 0x8000;
/// The scene number while a battle runs (MAP_BATTLE).
const MAP_BATTLE: u16 = 0x1fe;
/// The menu test room (SceneId_MenuTest) and its armor merchant, who opens
/// the debug shop (debug/test.c, ACTOR_ARMOR_MERCHANT).
const MENU_TEST_ROOM: u16 = 200;
const MENU_TEST_ARMOR_MERCHANT: u16 = 12;
/// The first actor a scene places (ACTOR_FIRST_PLACED).
const FIRST_PLACED: u16 = 8;
/// struct MainRoutineWork's next scene and entrance, and struct FieldWork's
/// exit request, which with 999 leaves for them (Event_SetDestination).
const GS_SCENE: u32 = 0x1c0;
const GS_ENTRANCE: u32 = 0x1c2;
const FW_EXIT: u32 = 0x170;
const EXIT_TO_DESTINATION: u16 = 999;
/// The direction key for each quarter turn of a facing, from east.
const FACING_KEYS: [u32; 4] = [KEY_RIGHT, KEY_DOWN, KEY_LEFT, KEY_UP];
/// MsgBuyAnythingElse, `Anything else?`, as the shop offers its list again.
const MSG_BUY_ANYTHING_ELSE: u32 = 3240;
/// Sol Sanctum's chest room (SOL_SANCTUM_RELIEFS_CHEST_ROOM_FROM_FIRST_RELIEF_ROOM)
/// and its chest's cell behavior (CELL_BEHAVIOR_SOL_SANCTUM_RELIEFS_CHEST).
const SOL_CHEST_ROOM: (u16, u16) = (16, 2);
const CELL_BEHAVIOR_SOL_SANCTUM_RELIEFS_CHEST: u16 = 100;
/// Isaac's owner number, and struct Unit's bag: 15 slots from 0xd8
/// (battle_unit.h).
const ISAAC: u8 = 0;
const UNIT_INVENTORY: u32 = 0xd8;
const INVENTORY_SLOTS: u32 = 15;

/// Runs of black pictures in `frames`, as (first, past the last).
fn black_runs(frames: &[Vec<u16>]) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for (i, f) in frames.iter().enumerate() {
        if !is_black_bgr555(f) {
            continue;
        }
        match runs.last_mut() {
            Some(r) if r.1 == i => r.1 = i + 1,
            _ => runs.push((i, i + 1)),
        }
    }
    runs
}

/// `frames` with what lies between its first black screen and the next cut
/// out, so that the two black screens join into one: a scene left and
/// come back to, without the place between.
fn cut_between_blacks(frames: &[Vec<u16>], first: usize) -> Result<Vec<Vec<u16>>, String> {
    let runs = black_runs(frames);
    let (a, b) = (runs.get(first).ok_or("no black screen to cut from")?, runs.get(first + 1).ok_or("no second black screen to cut to")?);
    let mut kept = frames[..a.1].to_vec();
    kept.extend_from_slice(&frames[b.0..]);
    Ok(kept)
}

impl<'a> Guide<'a> {
    pub(super) fn run_scene_map(&mut self, name: &str) -> Result<(), String> {
        match name {
            "scene-exit-region" => self.scene_exit_region(),
            "game-djinni-flag" => self.game_djinni_flag(),
            "map-recolour" => self.map_recolour(),
            "game-pack-full" => self.game_pack_full(),
            "scene-log-state" => self.scene_log_state(),
            "debug-menu-test-twice" => self.debug_menu_test_twice(),
            _ => Err(format!("no mode {name}")),
        }
    }

    /// Enters `scene`'s `entrance` as the story has it when the party
    /// first reaches that scene.
    fn enter_scene(&self, (scene, entrance): (&str, u16)) -> Result<Entered<'a>, String> {
        let scene = source::map_id(scene)?;
        self.enter_first_visit((scene, entrance), scene)
    }

    /// Walks the leader to (x, z), across and then along, keeping every
    /// picture; at most `limit` frames.
    fn walk_to(field: &mut Field, (x, z): (i32, i32), frames: &mut Vec<Vec<u16>>, limit: u32, run: bool) -> Result<(), String> {
        // An axis the leader stops short on, within NEAR pixels, counts as
        // reached: the ground's edges are seldom where a round number is.
        const NEAR: i32 = 8;
        let (mut n, mut still, mut last) = (0, 0, None);
        let (mut x_done, mut z_done) = (false, false);
        loop {
            let Some(at) = field.leader() else {
                field.frames(1)?;
                frames.push(field.picture());
                n += 1;
                if n >= limit {
                    return Err("the leader never came back".to_string());
                }
                continue;
            };
            still = if last == Some((at.x, at.z)) { still + 1 } else { 0 };
            last = Some((at.x, at.z));
            if still >= 40 {
                let on_x = !x_done && (at.x - x).abs() > 1;
                let off = if on_x { (at.x - x).abs() } else { (at.z - z).abs() };
                if off > NEAR {
                    return Err(format!("the leader stopped at ({}, {}) on the way to ({x}, {z})", at.x, at.z));
                }
                if on_x { x_done = true } else { z_done = true }
                still = 0;
            }
            let keys = if !x_done && (at.x - x).abs() > 1 {
                (if at.x < x { KEY_RIGHT } else { KEY_LEFT }) | if run && (at.x - x).abs() > 8 { KEY_B } else { 0 }
            } else if !z_done && (at.z - z).abs() > 1 {
                (if at.z < z { KEY_DOWN } else { KEY_UP }) | if run && (at.z - z).abs() > 8 { KEY_B } else { 0 }
            } else {
                break;
            };
            field.hold(keys);
            field.frames(1)?;
            frames.push(field.picture());
            n += 1;
            if n >= limit {
                return Err(format!("the leader stopped at ({}, {}) on the way to ({x}, {z})", at.x, at.z));
            }
        }
        field.hold(0);
        Ok(())
    }

    /// Walks the leader to the cell holding (x, z) by the shortest way along
    /// the cells he can walk to (Field::reach), cell centre to cell centre.
    fn path_to(field: &mut Field, (x, z): (i32, i32), frames: &mut Vec<Vec<u16>>, run: bool) -> Result<(), String> {
        let at = field.leader().ok_or("the scene has no leader")?;
        let reach = field.reach(None)?;
        let layer = reach.layer;
        let from = (at.x >> 4, at.z >> 4);
        let to = (x >> 4, z >> 4);
        // The cells the scene's placed actors stand on, which the map's
        // cells do not show.
        let taken: std::collections::HashSet<(i32, i32)> = (FIRST_PLACED..64)
            .filter_map(|id| field.actor(id))
            .filter(|a| a.drawn)
            .flat_map(|a| {
                let near = move |c: &(i32, i32)| (c.0 * 16 + 8 - a.x).abs() < 16 && (c.1 * 16 + 8 - a.z).abs() < 16;
                (-1..=1).flat_map(move |dz| (-1..=1).map(move |dx| ((a.x >> 4) + dx, (a.z >> 4) + dz))).filter(near).collect::<Vec<_>>()
            })
            .collect();
        let mut back: std::collections::HashMap<(i32, i32), (i32, i32)> = std::collections::HashMap::new();
        let mut queue = std::collections::VecDeque::from([from]);
        back.insert(from, from);
        while let Some(c) = queue.pop_front() {
            if c == to {
                break;
            }
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let n = (c.0 + dx, c.1 + dz);
                if !back.contains_key(&n) && (n == to || (reach.holds(layer, n.0 * 16 + 8, n.1 * 16 + 8) && !taken.contains(&n))) {
                    back.insert(n, c);
                    queue.push_back(n);
                }
            }
        }
        // Where the walk cannot reach the cell, the nearest it can.
        let to = *back.keys().min_by_key(|c| (c.0 - to.0).pow(2) + (c.1 - to.1).pow(2)).ok_or("nowhere to walk")?;
        let mut cells = vec![to];
        while let Some(&c) = cells.last() {
            let p = back[&c];
            if p == c {
                break;
            }
            cells.push(p);
        }
        cells.reverse();
        // Only the cells where the way turns.
        let mut turns = Vec::new();
        for i in 1..cells.len() {
            let turn = i + 1 == cells.len() || (cells[i].0 - cells[i - 1].0, cells[i].1 - cells[i - 1].1) != (cells[i + 1].0 - cells[i].0, cells[i + 1].1 - cells[i].1);
            if turn {
                turns.push((cells[i].0 * 16 + 8, cells[i].1 * 16 + 8));
            }
        }
        for t in turns {
            Self::walk_to(field, t, frames, 600, run)?;
        }
        Ok(())
    }

    /// Holds `keys`, keeping every picture, until the field is in a scene
    /// other than `from` and the player has control there again, then
    /// lets go; at most `limit` frames.
    fn hold_until_elsewhere(field: &mut Field, keys: u32, frames: &mut Vec<Vec<u16>>, limit: u32) -> Result<(), String> {
        let from = field.scene();
        field.hold(keys);
        for _ in 0..limit {
            field.frames(1)?;
            frames.push(field.picture());
            if field.scene() != from && !is_black_bgr555(frames.last().unwrap()) {
                field.hold(0);
                return Ok(());
            }
        }
        field.hold(0);
        Err(format!("still in scene {from} after {limit} frames"))
    }

    /// Up the ladder at the back of Kolima's tunnel: a box at its top, an
    /// exit region (gKolimaTunnelRegions), takes the leader out to Kolima
    /// as soon as he climbs into it, though no cell there is a door. The
    /// leader looks round the tunnel first, so that the game shows CLEARED
    /// frames before the screen turns black, and Kolima is shown as long
    /// after.
    fn scene_exit_region(&mut self) -> Result<(), String> {
        let mut entered = self.enter_scene(KOLIMA_TUNNEL)?;
        let field = &mut entered.field;
        let start = field.leader().map(|a| (a.x, a.z)).ok_or("the tunnel has no leader")?;
        let ready = field.save();
        let mut idle = 0;
        let mut frames;
        loop {
            let mut path = vec![(0, 20 + idle), (KEY_DOWN, 21), (KEY_LEFT, 30), (0, 30), (KEY_RIGHT, 30), (0, 10)];
            path.retain(|s| s.1 > 0);
            frames = Self::press_path(field, &path)?;
            Self::walk_to(field, start, &mut frames, 120, false)?;
            Self::hold_until_elsewhere(field, KEY_UP, &mut frames, 600)?;
            let first = frames.iter().position(|f| is_black_bgr555(f)).ok_or("the climb shows no black screen")?;
            if first >= CLEARED {
                break;
            }
            idle += (CLEARED - first) as u32;
            field.load(&ready);
            // The picture is drawn anew only by the next frame.
            field.frames(1)?;
        }
        Self::run_past_black(field, &mut frames)?;
        self.scene_map_gif("scene/exit-region.gif", &frames, "up the ladder of Kolima's tunnel into its exit region, and out into Kolima")
    }

    /// Granite, a Venus Djinni, joining in Kolima beside the way down to
    /// its tunnel (Kolima_GraniteJoins), then the party down into the
    /// tunnel and up again: Granite's placement row holds only while its
    /// found flag, FLAG_DJINNI(DJINNI_GRANITE), is clear, so the second
    /// visit finds no one there. The tunnel between the two visits is cut,
    /// joining the two black screens into one.
    fn game_djinni_flag(&mut self) -> Result<(), String> {
        let granite = source::actor_id("Kolima", "ACTOR_GRANITE")?;
        let mut entered = self.enter_scene(KOLIMA_FROM_TUNNEL)?;
        let hole = entered.field.leader().map(|a| (a.x, a.z)).ok_or("Kolima has no leader")?;
        let mut frames = Self::record_trigger(&mut entered, Trigger::Talk(granite), None, 40)?;
        let field = &mut entered.field;
        let after = field.save();
        let mut idle = 0;
        let kept = frames.len();
        loop {
            Self::hold(field, 0, idle, &mut frames)?;
            Self::walk_to(field, hole, &mut frames, 400, false)?;
            Self::hold_until_elsewhere(field, KEY_UP, &mut frames, 600)?;
            let first = frames.iter().position(|f| is_black_bgr555(f)).ok_or("no black screen on leaving")?;
            if first >= CLEARED {
                break;
            }
            idle = (CLEARED - first) as u32;
            field.load(&after);
            frames.truncate(kept);
        }
        // Up the tunnel's ladder again, back into Kolima.
        Self::hold_until_elsewhere(field, KEY_UP, &mut frames, 600)?;
        Self::run_past_black(field, &mut frames)?;
        Self::hold(field, 0, 30, &mut frames)?;
        let frames = cut_between_blacks(&frames, 0)?;
        self.scene_map_gif("game/djinni-flag.gif", &frames, "Granite joining in Kolima, and the spot empty on the next visit")
    }

    /// Kolima Forest's river log walked into from the east: it rolls to the
    /// bank and tips into the river (KolimaForestEntrance_PushLog), which
    /// sets FLAG_KOLIMA_FOREST_ENTRANCE_LOG_IN_RIVER, a scene temp flag.
    /// The party runs to and fro until a random battle comes and fights it
    /// with A; coming back from a battle clears no temp flag, so the log is
    /// still in the river, and the party crosses it. Then out to the World
    /// Map and straight back: the arrival clears the flag, and the log lies
    /// on the far bank again. The battle between its opening and its end
    /// is cut, a cut no black screen marks, and the World Map is cut,
    /// joining the two black screens on either side of it into one.
    fn scene_log_state(&mut self) -> Result<(), String> {
        let scene = source::map_id(KOLIMA_FOREST_ENTRANCE.0)?;
        let mut entered = self.enter_first_visit((scene, KOLIMA_FOREST_ENTRANCE.1), scene)?;
        let field = &mut entered.field;
        let spot = Self::spot_at(field, (RIVER_LOG.0 + 24, RIVER_LOG.1, WEST))?;
        field.stand(&entered.placed, &spot)?;
        field.frames(1)?;
        let mut frames = Vec::new();
        Self::hold(field, 0, 30, &mut frames)?;
        Self::hold(field, KEY_LEFT, 70, &mut frames)?;
        Self::hold(field, 0, 150, &mut frames)?;
        // Running back and forth beside the river until a battle comes.
        let mut opened = false;
        'walk: for leg in 0..80 {
            field.hold(KEY_B | if leg % 2 == 0 { KEY_RIGHT } else { KEY_LEFT });
            for _ in 0..30 {
                field.frames(1)?;
                frames.push(field.picture());
                if field.scene() == MAP_BATTLE {
                    opened = true;
                    break 'walk;
                }
            }
        }
        if !opened {
            return Err("no battle came".to_string());
        }
        // The battle fought with A until the forest is back.
        let mut ended = false;
        for n in 0..12000u32 {
            field.hold(if n % 20 < 3 { KEY_A } else { 0 });
            field.frames(1)?;
            frames.push(field.picture());
            if field.scene() == scene {
                ended = true;
                break;
            }
        }
        field.hold(0);
        if !ended {
            return Err("the battle never ended".to_string());
        }
        Self::run_past_black(field, &mut frames)?;
        Self::hold(field, 0, 200, &mut frames)?;
        // No second battle on the way out. Across the log, still in the
        // river, and on to the World Map.
        field.set_flag(self.flag("FLAG_NO_ENCOUNTERS")?, true);
        Self::walk_to(field, LOG_CROSSED, &mut frames, 200, false)?;
        Self::path_to(field, KOLIMA_FOREST_WAY_OUT, &mut frames, true)?;
        Self::hold_until_elsewhere(field, KEY_LEFT, &mut frames, 600)?;
        // Straight back in from the World Map, then to the bank across from
        // the log.
        Self::hold_until_elsewhere(field, KEY_RIGHT, &mut frames, 600)?;
        Self::path_to(field, LOG_CROSSED, &mut frames, true)?;
        Self::hold(field, KEY_RIGHT, 8, &mut frames)?;
        // The log on the far bank, until the scene has shown CLEARED frames.
        let since = frames.len() - frames.iter().rposition(|f| is_black_bgr555(f)).unwrap_or(0);
        Self::hold(field, 0, (CLEARED + 60).saturating_sub(since).max(60) as u32, &mut frames)?;
        let runs = black_runs(&frames);
        if runs.len() != 4 {
            return Err(format!("expected four black screens (the battle's two, out and in), not {runs:?}"));
        }
        if runs[0].0 < CLEARED {
            return Err(format!("the battle came after only {} frames", runs[0].0));
        }
        // The battle's opening and its end, CLEARED between them in all.
        let opening = runs[0].1 + CLEARED / 2 + 40;
        let ending = runs[1].0.saturating_sub(CLEARED / 2 + 40);
        if opening >= ending {
            return Err("the battle is too short to cut".to_string());
        }
        let mut kept = frames[..opening].to_vec();
        kept.extend_from_slice(&frames[ending..]);
        let kept = cut_between_blacks(&kept, 2)?;
        self.scene_map_gif("scene/log-state.gif", &kept, "Kolima Forest's log tipped into the river, still there after a battle, and back on the bank after the party leaves and returns")
    }

    /// Menu test entered twice (SceneId_MenuTest). Its setup,
    /// MenuTest_Initialize, runs on each entry and adds one Masamune to the
    /// shops' stock each time. On the first visit the debug shop
    /// (Shop_OpenDebugShop, opened by the armor merchant, a weapon shop
    /// outside the Japanese edition) lists the Masamune among its Artifacts
    /// and it is bought, so it leaves the list. The room is entered again
    /// as Event_SetDestination sends the party, and the same list shows the
    /// Masamune back on sale.
    fn debug_menu_test_twice(&mut self) -> Result<(), String> {
        let place = (MENU_TEST_ROOM, 0);
        let talk = Trigger::Talk(MENU_TEST_ARMOR_MERCHANT);
        let (_, spot) = self.stand_for(place, talk, MENU_TEST_ARMOR_MERCHANT)?;
        let toward = FACING_KEYS[(spot.facing >> 14) as usize & 3];
        let mut entered = self.enter_with(place, &[])?;
        let field = &mut entered.field;
        let mut frames = Vec::new();
        Self::menu_test_visit(field, (spot.x, spot.z), toward, true, &mut frames)?;
        // Into the room again, as Event_SetDestination sends the party.
        let work = self.sym("gMainRoutineWork")?;
        field.write16(work + GS_SCENE, MENU_TEST_ROOM);
        field.write16(work + GS_ENTRANCE, 0);
        let field_work = field.field_work();
        field.write16(field_work + FW_EXIT, EXIT_TO_DESTINATION);
        for _ in 0..300 {
            field.frames(1)?;
            frames.push(field.picture());
            if is_black_bgr555(frames.last().unwrap()) {
                break;
            }
        }
        Self::run_past_black(field, &mut frames)?;
        Self::menu_test_visit(field, (spot.x, spot.z), toward, false, &mut frames)?;
        self.scene_map_gif("debug/menu-test-twice.gif", &frames, "Menu test entered twice: the Masamune bought out, and back on sale on the next visit")
    }

    /// One visit to Menu test from where the party arrives: to the armor
    /// merchant at `stand`, facing him with `toward`, and the debug shop's
    /// Artifacts, the Shamshir, the Silver Blade and the Masamune, the hand
    /// moved onto the Masamune. With `buy` the Masamune is bought for Isaac and the
    /// list offered again without it; then out of the shop.
    fn menu_test_visit(field: &mut Field, stand: (i32, i32), toward: u32, buy: bool, frames: &mut Vec<Vec<u16>>) -> Result<(), String> {
        Self::hold(field, 0, 30, frames)?;
        if let Err(e) = Self::path_to(field, stand, frames, false) {
            // Stopped by the merchant's own footprint, a step short.
            let at = field.leader().ok_or("the room has no leader")?;
            if (at.x - stand.0).abs() + (at.z - stand.1).abs() > 20 {
                return Err(e);
            }
        }
        Self::hold(field, toward, 2, frames)?;
        let list = [(0, 10), (KEY_A, 3), (0, 90), (KEY_RIGHT, 3), (0, 25), (KEY_RIGHT, 3), (0, 25), (KEY_A, 3), (0, 75), (KEY_A, 3), (0, 50), (KEY_RIGHT, 3), (0, 30), (KEY_RIGHT, 3), (0, 75)];
        for &(keys, n) in &list {
            Self::hold(field, keys, n, frames)?;
        }
        if buy {
            // Chosen, and Isaac to have it; then A through what the shop
            // says (a Game Ticket may come with it), each press watched for
            // the list's question.
            for &(keys, n) in &[(KEY_A, 3), (0, 60), (KEY_A, 3), (0, 60)] {
                Self::hold(field, keys, n, frames)?;
            }
            let mut offered = false;
            for _ in 0..12 {
                Self::hold(field, KEY_A, 3, frames)?;
                for _ in 0..60 {
                    field.frames(1)?;
                    frames.push(field.picture());
                    offered |= field.last_message() == MSG_BUY_ANYTHING_ELSE;
                }
                if offered {
                    break;
                }
            }
            if !offered {
                return Err("the shop never offered its list again".to_string());
            }
        }
        Self::hold(field, 0, 90, frames)?;
        // Out of the list and the counter; the farewell waits for a press,
        // and then the field steps again.
        for &(keys, n) in &[(KEY_B, 3), (0, 45), (KEY_B, 3), (0, 50)] {
            Self::hold(field, keys, n, frames)?;
        }
        let steps = field.steps();
        for _ in 0..6 {
            Self::hold(field, KEY_A, 3, frames)?;
            Self::hold(field, 0, 40, frames)?;
            if field.steps() > steps + 10 {
                return Ok(());
            }
        }
        Err("the shop never closed".to_string())
    }

    /// The chest in Sol Sanctum opened with Isaac's 15 slots full: the
    /// party is given its item (PartyInventory_Add), which tries each
    /// party member in turn, so it goes to Garet; then the field menu's
    /// Item screen, Isaac's full list and Garet's with the item at its
    /// end.
    fn game_pack_full(&mut self) -> Result<(), String> {
        const FILLERS: [&str; 20] = [
            "Vial", "Potion", "Antidote", "Elixir", "Power Bread", "Cookie", "Apple", "Hard Nut", "Mint", "Lucky Pepper", "Smoke Bomb", "Sleep Bomb", "Bramble Seed", "Crystal Powder", "Oil Drop", "Weasel's Claw", "Bone", "Corn", "Sacred Feather", "Dragon Skin",
        ];
        let names = super::tables::Game::here()?.item_names()?;
        let mut fillers = FILLERS.iter().filter_map(|n| names.iter().position(|i| i == n).map(|p| p as u16));
        let mut entered = self.enter_with(SOL_CHEST_ROOM, &[])?;
        let field = &mut entered.field;
        field.load(&entered.placed);
        let unit = field.unit(ISAAC)?;
        for slot in 0..INVENTORY_SLOTS {
            if field.read16(unit + UNIT_INVENTORY + 2 * slot) == 0 {
                field.write16(unit + UNIT_INVENTORY + 2 * slot, fillers.next().ok_or("too few fillers")?);
            }
        }
        entered.placed = field.save();
        field.until_control()?;
        field.frames(20)?;
        let mut frames = Self::record_trigger(&mut entered, Trigger::Search(CELL_BEHAVIOR_SOL_SANCTUM_RELIEFS_CHEST), None, 30)?;
        let field = &mut entered.field;
        // The field menu, its Item icon, Isaac's list, then Garet's.
        let path = [(KEY_A, 3), (0, 50), (KEY_RIGHT, 3), (0, 30), (KEY_A, 3), (0, 90), (KEY_RIGHT, 3), (0, 90), (KEY_B, 3), (0, 30), (KEY_B, 3), (0, 40)];
        for (keys, n) in path {
            Self::hold(field, keys, n, &mut frames)?;
        }
        self.scene_map_gif("game/pack-full.gif", &frames, "a chest's item going to Garet while Isaac's bag is full")
    }

    /// Two scenes of Crossbone Isle drawn from one tileset, each with a
    /// palette of its own (the directory's recolor rows 15 and 17 over the
    /// same tiles): the floor of stepping stones and the floor of keys,
    /// side by side, as `map/recolour.png`.
    fn map_recolour(&mut self) -> Result<(), String> {
        let mut shots = Vec::new();
        for place in [("CrossboneIsleSteppingStones", 1), ("CrossboneIsleKeys", 1)] {
            let entered = self.enter_scene(place)?;
            shots.push(screen(&entered.field.picture()));
        }
        let gap = 8;
        let mut image = super::guide_more::canvas(2 * WIDTH + gap, HEIGHT, 0);
        super::guide_more::paste(&mut image, &shots[0], 0, 0, 1);
        super::guide_more::paste(&mut image, &shots[1], WIDTH + gap, 0, 1);
        std::fs::create_dir_all(self.out.join("map")).map_err(|e| format!("cannot make map: {e}"))?;
        self.guide_png("map/recolour.png", &image, "two floors of Crossbone Isle, one tileset in two palettes")
    }

    /// Writes `frames` as `path` under the output folder, making its folder.
    fn scene_map_gif(&mut self, path: &str, frames: &[Vec<u16>], what: &str) -> Result<(), String> {
        if let Some(dir) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(self.out.join(dir)).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
        }
        self.guide_gif(path, frames, what)
    }
}
