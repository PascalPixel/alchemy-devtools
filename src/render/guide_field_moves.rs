//! Recordings for the field folder guide and the Field page: how the
//! leader and the actors move, a stairs exit, what the desert's sand does to
//! the party, the palette fade, and a cast that keeps its state. Each plays a
//! built edition with the pad held as a player would; guide_docs_world
//! dispatches them.

use super::field::{Field, Spot};
use super::guide::{Entered, Guide};
use super::source;

/// KEYINPUT bits.
const KEY_A: u32 = 0x1;
const KEY_RIGHT: u32 = 0x10;
const KEY_LEFT: u32 = 0x20;
const KEY_UP: u32 = 0x40;
const KEY_DOWN: u32 = 0x80;

/// Facings, 1/65536 turns (enum Facing).
const EAST: u32 = 0;
const WEST: u32 = 0x8000;
const NORTH: u32 = 0xc000;

/// struct FieldWork.command: a field Psynergy request, as the Psynergy menu
/// raises it.
const FIELD_COMMAND: u32 = 0x17e;
/// struct MainRoutineWork.hazardSteps.
const HAZARD_STEPS: u32 = 0x232;
/// struct Unit.hp.
const UNIT_HP: u32 = 0x38;

impl<'a> Guide<'a> {
    pub(super) fn run_field_moves(&mut self, name: &str) -> Result<(), String> {
        match name {
            "field-push-pillar" => self.push_pillar(),
            "field-climb" => self.climb(),
            "field-villager" => self.villager(),
            "field-stairs" => self.stairs(),
            "layout-sand-damage" => self.sand_damage(),
            "field-palette-fade" => self.field_palette_fade(),
            "field-cloak" => self.cloak(),
            _ => Err(format!("no mode {name}")),
        }
    }

    /// Enters scene `scene` by `entrance` as the story has it when the party
    /// first reaches it, with the leader stood at (x, z) pixels, facing.
    fn stood(&self, scene: &str, entrance: u16, at: (i32, i32, u32)) -> Result<Entered<'a>, String> {
        let id = source::map_id(scene)?;
        let mut entered = self.enter_first_visit((id, entrance), id)?;
        let spot = Self::spot_at(&mut entered.field, at)?;
        entered.field.stand(&entered.placed, &spot)?;
        Ok(entered)
    }

    /// Holds each step's keys in turn, keeping every picture.
    fn steps(field: &mut Field, path: &[(u32, u32)], frames: &mut Vec<Vec<u16>>) -> Result<(), String> {
        for &(keys, n) in path {
            Self::hold(field, keys, n, frames)?;
        }
        Ok(())
    }

    /// Casts `ability` as the Psynergy menu would, through the field's
    /// request, and keeps `n` frames.
    fn cast(field: &mut Field, ability: &str, n: u32, frames: &mut Vec<Vec<u16>>) -> Result<(), String> {
        let id = source::ability_id(ability)?;
        let work = field.field_work();
        field.write16(work + FIELD_COMMAND, id);
        Self::hold(field, 0, n, frames)
    }

    /// Walking east into Vale's stone pillar below the cliff (ACTOR_PILLAR,
    /// cell behavior 30, Vale_PushPillar): the leader leans on it a quarter
    /// second, then he and the pillar slide a cell together
    /// (PushedActor_AdvancePair).
    fn push_pillar(&mut self) -> Result<(), String> {
        let mut entered = self.stood("Vale", 1, (356, 588, EAST))?;
        let field = &mut entered.field;
        let mut frames = vec![field.picture()];
        Self::steps(field, &[(0, 20), (KEY_RIGHT, 150), (0, 40)], &mut frames)?;
        self.world_gif("field/push-pillar.gif", &frames, "Vale's stone pillar pushed a cell east")
    }

    /// Up held at the foot of the climbing wall in Goma Cave's entrance
    /// (CELL_BEHAVIOR_CLIMB, Field_RunPushCell): after 12 frames' push the
    /// leader grabs the wall (Field_ClimbUp), climbs it in 8-pixel steps
    /// (SC_KEYS, Actor_MoveByKeys) and over the top.
    fn climb(&mut self) -> Result<(), String> {
        let mut entered = self.stood("GomaCaveEntrance", 1, (264, 312, NORTH))?;
        let field = &mut entered.field;
        let mut frames = vec![field.picture()];
        Self::steps(field, &[(0, 20), (KEY_UP, 125), (0, 30)], &mut frames)?;
        self.world_gif("field/climb.gif", &frames, "climbing the wall in Goma Cave's entrance")
    }

    /// The woman on the cliff above Vale's waterfall
    /// (ACTOR_WOMAN_ON_ALEPHS_PURITY, MOVEMENT_TYPE_WANDER_NEAR), watched while
    /// the leader stands: she starts, stops and turns as her actor script
    /// runs.
    fn villager(&mut self) -> Result<(), String> {
        let id = source::map_id("Vale")?;
        let mut entered = self.enter_first_visit((id, 1), id)?;
        let field = &mut entered.field;
        let woman = source::actor_id("Vale", "ACTOR_WOMAN_ON_ALEPHS_PURITY")?;
        // The leader beside her, on her ground, so the camera holds her.
        let her = field.actor(woman).ok_or("no woman in Vale")?;
        let layer = her.layer.min(2);
        // The nearest place 32 pixels or more from her on her own ground.
        let mut near = None;
        for dz in (-48..=48).step_by(8) {
            for dx in (-48..=48).step_by(8) {
                let d: i32 = dx * dx + dz * dz;
                if d >= 32 * 32 && field.ground(layer, her.x + dx, her.z + dz)? == her.y && near.is_none_or(|(n, _, _)| d < n) {
                    near = Some((d, her.x + dx, her.z + dz));
                }
            }
        }
        let (_, x, z) = near.ok_or("no ground beside the woman")?;
        let height = her.y;
        let facing = if x < her.x { EAST } else { WEST };
        let spot = Spot { x, z, layer, height, facing, walk: 0, cell: None };
        field.stand(&entered.placed, &spot)?;
        let mut frames = vec![field.picture()];
        Self::hold(field, 0, 480, &mut frames)?;
        self.world_gif("field/villager.gif", &frames, "a villager wandering in Vale")
    }

    /// Goma Cave's pillar room left by the stairs up to the room of the
    /// stream (GOMA_CAVE_PILLAR_TO_GOMA_CAVE_STREAM_WEST, EXIT_STYLE_STAIRS_UP):
    /// the leader walks about below them for ten seconds, climbs into them
    /// (Exit_StartStairsUp, ANIM_LEADER_LEAVE), and arrives in the room of the
    /// stream with its arrival (Entrance_PlayArrival, ANIM_LEADER_ARRIVE).
    fn stairs(&mut self) -> Result<(), String> {
        let mut entered = self.stood("GomaCavePillar", 1, (88, 200, NORTH))?;
        let field = &mut entered.field;
        let from = field.scene();
        let mut frames = vec![field.picture()];
        Self::steps(field, &[(0, 30), (KEY_DOWN, 170), (KEY_RIGHT, 100), (KEY_LEFT, 100), (KEY_UP, 170), (0, 20)], &mut frames)?;
        // Back under the stairs' cell, whatever a wall slid him off by.
        let x = field.leader().ok_or("no leader")?.x;
        let (keys, n) = if x < 88 { (KEY_RIGHT, 88 - x) } else { (KEY_LEFT, x - 88) };
        Self::steps(field, &[(keys, n as u32), (0, 10)], &mut frames)?;
        let mut left = None;
        field.hold(KEY_UP);
        for _ in 0..400 {
            field.frames(1)?;
            frames.push(field.picture());
            if left.is_none() && field.scene() != from {
                left = Some(frames.len());
                field.hold(0);
            }
        }
        field.hold(0);
        left.ok_or("the stairs never took the party out")?;
        Self::hold(field, 0, 300, &mut frames)?;
        self.world_gif("field/stairs.gif", &frames, "the stairs up out of Goma Cave's pillar room")
    }

    /// Walking on the Lamakan Desert's sand until the heat hurts: the count
    /// (gMainRoutineWork.hazardSteps) set three quarters full and the
    /// quarters' complaints marked heard (FLAG_HEAT_QUARTER to
    /// FLAG_HEAT_THREE_QUARTERS), so the thermometer fills and at 600 the
    /// screen flashes, the leader flinches and the party loses a quarter of
    /// its maximum HP (Field_UpdateWalking); then the field menu shows the
    /// HP left.
    fn sand_damage(&mut self) -> Result<(), String> {
        let mut entered = self.stood("LamakanDesertEast", 2, (400, 60, NORTH))?;
        let field = &mut entered.field;
        for flag in ["FLAG_HEAT_QUARTER", "FLAG_HEAT_HALF", "FLAG_HEAT_THREE_QUARTERS", "FLAG_GARET_COMPLAINED", "FLAG_IVAN_COMPLAINED", "FLAG_MIA_COMPLAINED"] {
            let f = self.flag(flag)?;
            field.set_flag(f, true);
        }
        let gs = field.main_routine_work();
        field.write16(gs + HAZARD_STEPS, 450);
        let hp = |field: &mut Field| -> Result<u16, String> {
            let unit = field.unit(0)?;
            Ok(field.read16(unit + UNIT_HP))
        };
        // The thermometer climbs to the count before the recording starts.
        field.frames(30)?;
        let before = hp(field)?;
        let mut frames = vec![field.picture()];
        Self::steps(field, &[(0, 10), (KEY_DOWN, 140), (KEY_RIGHT, 60), (0, 30)], &mut frames)?;
        let after = hp(field)?;
        eprintln!("sand-damage: Isaac's HP {before} to {after}, the count at {}", field.read16(gs + HAZARD_STEPS));
        Self::steps(field, &[(KEY_A, 3), (0, 80)], &mut frames)?;
        self.world_gif("layout/sand-damage.gif", &frames, "the Lamakan Desert's heat hurting the party")
    }

    /// The palette fade in Vale: Halt cast with nobody near tints the screen
    /// toward the cast's colour over 8 frames (FieldPsynergy_FadeToCastColor)
    /// and fades back once its orb has scattered. Reveal's grey is not
    /// recorded: its cast blacks the screen out for 4 frames as it starts and
    /// for 1 as it ends, at most 10 seconds apart, which a recording could
    /// clear only by running half a minute.
    fn field_palette_fade(&mut self) -> Result<(), String> {
        let mut entered = self.stood("Vale", 1, (300, 470, EAST))?;
        let field = &mut entered.field;
        let mut frames = vec![field.picture()];
        Self::hold(field, 0, 20, &mut frames)?;
        Self::cast(field, "ABILITY_HALT", 170, &mut frames)?;
        Self::hold(field, 0, 20, &mut frames)?;
        self.world_gif("field/palette-fade.gif", &frames, "a cast's tint and its fade back in Vale")
    }

    /// Cloak cast in Lunpa Fortress's gates: the leader blinks and hides as
    /// an outline, stays hidden walking east along the dark ground below the
    /// gate (cell behavior CLOAK_TILE, FieldPsynergy_CheckCloakTile), and
    /// blinks back into sight where it ends (FieldPsynergy_EndCloak).
    fn cloak(&mut self) -> Result<(), String> {
        let mut entered = self.stood("LunpaFortressGates", 0, (120, 104, EAST))?;
        let field = &mut entered.field;
        let mut frames = vec![field.picture()];
        Self::hold(field, 0, 20, &mut frames)?;
        Self::cast(field, "ABILITY_CLOAK", 150, &mut frames)?;
        Self::steps(field, &[(KEY_RIGHT, 160), (0, 60)], &mut frames)?;
        self.world_gif("field/cloak.gif", &frames, "Cloak in Lunpa Fortress's gates")
    }
}
