//! The Lost Age's other recordings, the mirrors of The Broken Seal's front
//! page, graphics, map, game and debug ones: the title and its intro,
//! Daila's tiles as the tile animation repaints them, the Djinni Spark
//! joining in Mikasalla and gone on the next visit, the debug scene
//! selector, and the splashes the leader leaves wading in the Kandorean
//! Temple.

use super::animation::is_black_bgr555;
use super::field::{Field, Setup, CLEARED};
use super::guide::Guide;
use super::guide_tla::{
    Walk, KEY_A, KEY_B, KEY_DOWN, KEY_LEFT, KEY_RIGHT, KEY_START, KEY_UP, PARTY_DAILA,
};
use super::story::Trigger;

/// The title overlay's entrances (title.c): 2 plays the intro before the
/// prologue, 3 the title after it, which leads on to Idejima.
const TITLE_INTRO: (u16, u16) = (0, 2);
const TITLE_AFTER_PROLOGUE: (u16, u16) = (0, 3);
/// DISPCNT, whose bits 8 to 11 show BG0 to BG3.
const DISPCNT: u32 = 0x0400_0000;
/// The intro's frame, from its start, at which the bird crosses the valley
/// below the tower.
const INTRO_VALLEY: u32 = 1180;
/// Debug set 9: Felix, Jenna and Sheba at level 9, as they reach
/// Mikasalla.
const PARTY_PLATEAU: u32 = 9;
/// Mikasalla (SceneIdMikasalla1) by the north end of its passage (exit 7
/// leads there), Spark there (mikasalla.c: ACTOR_SPARK), and the passage's
/// north mouth, which leads to its south mouth (exit 9) and back (exit 7).
const MIKASALLA: &str = "Mikasalla";
const MIKASALLA_NORTH: u16 = 9;
const SPARK: u16 = 18;
const PASSAGE_NORTH: (i32, i32) = (312, 262);
/// The Kandorean Temple's room with a stream (SceneIdKandoreanTempleStream),
/// a map whose ground effects (MapState.groundEffects) are wading.
const TEMPLE_STREAM: &str = "KandoreanTempleStream";

/// The modes this file plays.
pub const MODES: &[&str] = &[
    "title",
    "graphics-title-fade",
    "title-intro-layers-3d",
    "tile-animation",
    "game-djinni-flag",
    "debug-map-select",
    "field-wading",
];

impl<'a> Guide<'a> {
    pub(super) fn run_tla_more(&mut self, name: &str) -> Result<(), String> {
        match name {
            "title" => self.tla_title(),
            "graphics-title-fade" => self.tla_title_fade(),
            "title-intro-layers-3d" => self.tla_title_intro_exploded(),
            "tile-animation" => {
                let mut entered = self.tla_enter("DailaFlooded", 0, PARTY_DAILA, &[])?;
                let bgcnt = Self::animated_background(&mut entered.field)?;
                self.tile_animation_of(
                    &mut entered.field,
                    bgcnt,
                    "map/tile-animation.gif",
                    "Daila beside the tiles of its layer the tile animation rewrites",
                )
            }
            "game-djinni-flag" => self.tla_djinni_flag(),
            "debug-map-select" => self.tla_map_select(),
            "field-wading" => self.tla_walk(&WADING),
            _ => Err(format!("no mode {name}")),
        }
    }

    /// The control register of the background shown whose first 512 tiles
    /// change most over two seconds: the layer the tile animation repaints.
    /// The field is put back as it was.
    fn animated_background(field: &mut Field) -> Result<u32, String> {
        let saved = field.save();
        let shown = field.read16(DISPCNT);
        let tiles = |field: &Field, bg: u32| -> Vec<u8> {
            let base = 0x0600_0000
                + u32::from((field.read16(super::guide_take::BG0CNT + 2 * bg) >> 2) & 3) * 0x4000;
            (0..512 * 32).map(|k| field.read8(base + k)).collect()
        };
        let before: Vec<Vec<u8>> = (0..4).map(|bg| tiles(field, bg)).collect();
        let mut changed = [0usize; 4];
        for _ in 0..120 {
            field.frames(1)?;
            for bg in 0..4u32 {
                let now = tiles(field, bg);
                changed[bg as usize] += now
                    .chunks(32)
                    .zip(before[bg as usize].chunks(32))
                    .filter(|(a, b)| a != b)
                    .count();
            }
        }
        field.load(&saved);
        let bg = (0..4u32)
            .filter(|&bg| shown & (0x100 << bg) != 0)
            .max_by_key(|&bg| changed[bg as usize])
            .ok_or("no background is shown")?;
        Ok(super::guide_take::BG0CNT + 2 * bg)
    }

    /// The title overlay entered by `place`, every picture of `frames`.
    fn title_frames(&self, place: (u16, u16), frames: u32) -> Result<Vec<Vec<u16>>, String> {
        let setup = Setup {
            scene: place.0,
            entrance: place.1,
            set: Vec::new(),
            clear: Vec::new(),
        };
        let mut field = Field::enter_with_party(self.rom, self.symbols, &setup, None)?;
        let mut out = Vec::new();
        Self::hold(&mut field, 0, frames, &mut out)?;
        Ok(out)
    }

    /// The title after the prologue: the Golden Sun logo fades in from
    /// white over the valley and its lighthouse and is held, until it
    /// starts to fade towards Idejima, as `story/title.gif`.
    fn tla_title(&mut self) -> Result<(), String> {
        let frames = self.title_frames(TITLE_AFTER_PROLOGUE, 1200)?;
        let first = frames
            .iter()
            .position(|f| !is_black_bgr555(f))
            .ok_or("the title stayed black")?;
        // Held once two seconds pass unchanged; it ends where the picture
        // first changes after that.
        let held = first + 240;
        let differs = |a: &[u16], b: &[u16]| a.iter().zip(b).filter(|(x, y)| x != y).count() > 200;
        let end = (held..frames.len())
            .find(|&i| differs(&frames[i], &frames[held]))
            .ok_or("the title never faded")?;
        self.world_gif(
            "story/title.gif",
            &frames[first..end],
            "the title after the prologue: the Golden Sun logo over the valley and its lighthouse",
        )
    }

    /// The intro's clouds after `Camelot presents`, every colour on screen
    /// brightening from dark red towards its own, as
    /// `graphics/title-fade.gif`: from ten frames before the first frame
    /// after the black that is no longer black, for 110 frames.
    fn tla_title_fade(&mut self) -> Result<(), String> {
        let frames = self.title_frames(TITLE_INTRO, 700)?;
        // Each picture's brightness, the mean of its channels (0 to 93).
        let light = |f: &[u16]| {
            f.iter()
                .map(|&c| u32::from(c & 31) + u32::from((c >> 5) & 31) + u32::from((c >> 10) & 31))
                .sum::<u32>()
                / f.len() as u32
        };
        let brightest = frames.iter().map(|f| light(f)).max().unwrap_or(0);
        // The clouds back after `Camelot presents`: the first picture past
        // half the brightest's light, once the clouds the intro opens on have
        // dimmed to near black.
        let first = frames
            .iter()
            .position(|f| light(f) * 2 > brightest)
            .ok_or("the intro never lit")?;
        let dimmed = first
            + frames[first..]
                .iter()
                .position(|f| light(f) < 3)
                .ok_or("the intro never darkened")?;
        let back = dimmed
            + frames[dimmed..]
                .iter()
                .position(|f| light(f) * 2 > brightest)
                .ok_or("the clouds never came back")?;
        // The fade from its darkest picture, in the second and a half before
        // the clouds are back.
        let from = back.saturating_sub(90).max(dimmed);
        let fade = (from..back)
            .min_by_key(|&i| (light(&frames[i]), std::cmp::Reverse(i)))
            .unwrap_or(back);
        self.world_gif(
            "graphics/title-fade.gif",
            &frames[fade..(fade + 110).min(frames.len())],
            "the intro's clouds fading in from dark red",
        )
    }

    /// The intro's valley below the tower as the bird crosses it, pulled
    /// apart, as `graphics/title-intro-layers-3d.gif`.
    fn tla_title_intro_exploded(&mut self) -> Result<(), String> {
        let setup = Setup {
            scene: TITLE_INTRO.0,
            entrance: TITLE_INTRO.1,
            set: Vec::new(),
            clear: Vec::new(),
        };
        let mut field = Field::enter_with_party(self.rom, self.symbols, &setup, None)?;
        field.frames(INTRO_VALLEY)?;
        self.explode_now(
            &mut field,
            "graphics/title-intro-layers-3d.gif",
            "the intro's valley below the tower",
        )
    }

    /// Holds `keys` until the screen turns black, then lets go and runs on
    /// until the game has shown 30 frames since, keeping every picture: a
    /// way out of the map walked into and the map beyond.
    fn through_black(
        field: &mut Field,
        keys: u32,
        frames: &mut Vec<Vec<u16>>,
    ) -> Result<(), String> {
        field.hold(keys);
        let mut seen = false;
        let mut since = 0;
        for _ in 0..1200 {
            field.frames(1)?;
            let picture = field.picture();
            if is_black_bgr555(&picture) {
                seen = true;
                field.hold(0);
                since = 0;
            } else if seen {
                since += 1;
            }
            frames.push(picture);
            if seen && since >= 30 {
                return Ok(());
            }
        }
        field.hold(0);
        Err("the screen never went black and came back".to_string())
    }

    /// Spark, a Mars Djinni, joining in Mikasalla (Mikasalla_MeetSpark),
    /// then the party through the passage below and back: Spark's placement
    /// holds only while its flag is clear, so the second visit finds no one
    /// there. The passage between is cut, joining its two black screens.
    fn tla_djinni_flag(&mut self) -> Result<(), String> {
        let mut entered = self.tla_enter(MIKASALLA, MIKASALLA_NORTH, PARTY_PLATEAU, &[])?;
        let mut frames = Self::record_trigger(&mut entered, Trigger::Talk(SPARK), None, 40)?;
        let field = &mut entered.field;
        let after = field.save();
        let kept = frames.len();
        let mut idle = 0;
        loop {
            Self::hold(field, 0, idle, &mut frames)?;
            Self::tla_walk_to(field, PASSAGE_NORTH, &mut frames)?;
            Self::through_black(field, KEY_DOWN, &mut frames)?;
            let first = frames
                .iter()
                .position(|f| is_black_bgr555(f))
                .ok_or("no black screen on leaving")?;
            if first >= CLEARED {
                break;
            }
            idle = (CLEARED - first) as u32;
            field.load(&after);
            frames.truncate(kept);
        }
        // Back up through the passage, and Spark's spot again.
        Self::hold(field, 0, 20, &mut frames)?;
        Self::through_black(field, KEY_UP, &mut frames)?;
        Self::hold(field, KEY_UP, 30, &mut frames)?;
        Self::run_past_black(field, &mut frames)?;
        // The passage between cut out, from the middle of the black screen
        // on leaving to the middle of the one on coming back, so the two
        // halves join into one black screen.
        let runs = super::guide_scene_map::black_runs(&frames);
        let (a, b) = (
            runs.first().ok_or("no black screen on leaving")?,
            runs.get(1).ok_or("no black screen on coming back")?,
        );
        let mut kept = frames[..(a.0 + a.1) / 2].to_vec();
        kept.extend_from_slice(&frames[(b.0 + b.1) / 2..]);
        let frames = kept;
        self.world_gif(
            "game/djinni-flag.gif",
            &frames,
            "Spark joining in Mikasalla, and the spot empty on the next visit",
        )
    }

    /// The debug scene selector in Daila with DebugMode set: Start pressed
    /// with B held (FieldSequence_Run), the scene stepped down past the
    /// first to the developers' rooms with Left, then A, which takes the
    /// party there, as `debug/map-select.gif`.
    fn tla_map_select(&mut self) -> Result<(), String> {
        let mode = self.sym("DebugMode")?;
        let mut entered = self.tla_enter("DailaFlooded", 0, PARTY_DAILA, &[])?;
        let field = &mut entered.field;
        field.write8(mode, 1);
        let mut path = vec![(0, 20), (KEY_B, 4), (KEY_B | KEY_START, 4), (0, 40)];
        for _ in 0..DEBUG_STEPS {
            path.extend([(KEY_LEFT, 3), (0, 20)]);
        }
        path.extend([(0, 20), (KEY_A, 3), (0, 110)]);
        let mut frames = Self::press_path_cleared(field, &path)?;
        // The first picture of a path played again is the one before it.
        frames.remove(0);
        self.world_gif(
            "debug/map-select.gif",
            &frames,
            "the debug scene selector opened in Daila, warping to a test room",
        )
    }
}

/// How many times the selector steps down from Daila's number with Left.
const DEBUG_STEPS: u32 = 12;

/// Walking down into the Kandorean Temple's stream and along it: the map's
/// ground effect is wading (MapState.groundEffects), and each step in
/// the water starts a splash (Actor_SpawnWaterSplash).
const WADING: Walk = Walk {
    file: "field/wading.gif",
    scene: TEMPLE_STREAM,
    entrance: 1,
    preset: PARTY_DAILA,
    stand: None,
    path: &[
        (0, 20),
        (KEY_DOWN, 60),
        (KEY_RIGHT, 30),
        (KEY_DOWN, 50),
        (0, 60),
    ],
    what: "wading in the Kandorean Temple's stream",
};

/// A field Psynergy The Lost Age adds, cast where an early scene answers
/// it: the file, the ability, the scene, what it is cast on (an actor, or a
/// cell's collision code for one cast at the cell ahead), and in this
/// renderer's words.
struct FieldCast {
    file: &'static str,
    ability: &'static str,
    scene: &'static str,
    on: On,
    what: &'static str,
}

/// What a field cast is aimed at.
enum On {
    Actor(u16),
    Cell(u16),
}

/// The new field Psynergy, each where the walkthrough first meets it: Lash
/// ties the rope at the Shrine of the Sea God (ShrineOfTheSeaGod_LashRope),
/// Pound drives down the Dehkan Plateau's first pillar
/// (DehkanPlateau_PoundRoom1Pillar), Cyclone blows away the Kandorean
/// Temple's first patch of grass (KandoreanTemple_CycloneFirstPatch), and
/// Scoop digs up the Psy Crystal under Daila's puddle (Daila_DigUpPsyCrystal).
const FIELD_CASTS: &[FieldCast] = &[
    FieldCast {
        file: "field/lash.gif",
        ability: "ABILITY_LASH",
        scene: "ShrineOfTheSeaGodRope",
        on: On::Actor(10),
        what: "Lash cast at the rope at the Shrine of the Sea God",
    },
    FieldCast {
        file: "field/pound.gif",
        ability: "ABILITY_POUND",
        scene: "DehkanPlateauFullMetalVestChest",
        on: On::Actor(11),
        what: "Pound cast on a pillar on the Dehkan Plateau",
    },
    FieldCast {
        file: "field/cyclone.gif",
        ability: "ABILITY_CYCLONE",
        scene: "KandoreanTempleGate",
        on: On::Cell(30),
        what: "Cyclone cast on a patch of grass at the Kandorean Temple",
    },
    FieldCast {
        file: "field/scoop.gif",
        ability: "ABILITY_SCOOP",
        scene: "Daila",
        on: On::Cell(100),
        what: "Scoop cast on the spot under a puddle in Daila",
    },
];
/// The entrances a field cast's scene is entered by, in turn, until one
/// leads to where the cast is answered.
const CAST_ENTRANCES: u16 = 12;

/// The modes of the new field Psynergy.
pub const CAST_MODES: &[&str] = &["field-lash", "field-pound", "field-cyclone", "field-scoop"];

impl<'a> Guide<'a> {
    pub(super) fn run_tla_cast(&mut self, name: &str) -> Result<(), String> {
        let cast = FIELD_CASTS
            .iter()
            .find(|c| {
                c.file.trim_start_matches("field/").trim_end_matches(".gif")
                    == name.trim_start_matches("field-")
            })
            .ok_or_else(|| format!("no mode {name}"))?;
        let ability = Self::tla_constant(cast.ability)?;
        let trigger = match cast.on {
            On::Actor(actor) => Trigger::Psynergy(ability, actor),
            On::Cell(code) => Trigger::PsynergyCell(ability, code),
        };
        let mut last = String::from("no entrance");
        for entrance in 0..CAST_ENTRANCES {
            let Ok(mut entered) = self.tla_enter(cast.scene, entrance, PARTY_PLATEAU, &[]) else {
                continue;
            };
            match Self::record_trigger(&mut entered, trigger, None, 40) {
                Ok(frames) => return self.world_gif(cast.file, &frames, cast.what),
                Err(e) => last = format!("{} by entrance {entrance}: {e}", cast.scene),
            }
        }
        Err(last)
    }
}

/// Sunshine the blacksmith in Yallam's houses (yallam_rooms.c, actor
/// 14), and the Tear Stone, a material he forges (item 429:
/// YallamRooms_PartyHasForgeMaterial).
const YALLAM_HOUSES: &str = "YallamRoomsInn";
const SUNSHINE: u16 = 14;
const TEAR_STONE: u16 = 429;
/// struct BattleUnit's inventory, the same in both games.
const UNIT_INVENTORY: u32 = 0xd8;
const INVENTORY_SLOTS: u32 = 15;

/// The modes of The Lost Age's forge.
pub const FORGE_MODES: &[&str] = &["shop-forge"];

impl<'a> Guide<'a> {
    /// Felix, a Tear Stone at the front of his bag, talks to Sunshine: Yes
    /// to his offer, the two walk to the anvil, and the forge's screen
    /// (Forge_Run), the selling screen aimed at materials, takes the Tear
    /// Stone, as `shop/forge.gif`.
    pub(super) fn tla_forge(&mut self) -> Result<(), String> {
        // The houses' rooms are entered by each door in turn until one is
        // the smithy.
        let mut entered = None;
        let mut last = String::from("no door");
        for door in 0..8 {
            let Ok(mut e) = self.tla_enter(YALLAM_HOUSES, door, PARTY_PIERS_FORGE, &[]) else {
                continue;
            };
            // Below him, facing him: the field's search for a spot finds
            // none across the smithy's floor.
            let Some(at) = e.field.actor(SUNSHINE) else {
                last = format!("door {door} shows no Sunshine");
                continue;
            };
            let spot = Self::spot_at(&mut e.field, (at.x, at.z + 16, super::guide_tla::NORTH))?;
            if e.field.stand(&e.placed, &spot).is_ok()
                && e.field
                    .leader()
                    .is_some_and(|l| (l.x - spot.x).abs() <= 2 && (l.z - spot.z).abs() <= 2)
            {
                entered = Some(e);
                break;
            }
            last = format!("door {door}: the leader could not stand below Sunshine");
        }
        let mut entered = entered.ok_or(last)?;
        let field = &mut entered.field;
        let unit = field.unit(super::plan::FELIX)?;
        let held: Vec<u16> = (0..INVENTORY_SLOTS)
            .map(|i| field.read16(unit + UNIT_INVENTORY + 2 * i))
            .filter(|&e| e != 0)
            .collect();
        for (i, entry) in std::iter::once(TEAR_STONE)
            .chain(held)
            .take(INVENTORY_SLOTS as usize)
            .enumerate()
        {
            field.write16(unit + UNIT_INVENTORY + 2 * i as u32, entry);
        }
        let path = [
            (0, 20),
            (KEY_A, 3),
            (0, 140),
            (KEY_A, 3),
            (0, 150),
            (KEY_A, 3),
            (0, 220),
            (KEY_A, 3),
            (0, 80),
            (KEY_A, 3),
            (0, 80),
            (KEY_A, 3),
            (0, 120),
            (KEY_A, 3),
            (0, 120),
            (KEY_A, 3),
            (0, 120),
            (KEY_A, 3),
            (0, 200),
        ];
        let frames = Self::press_path(field, &path)?;
        self.world_gif(
            "shop/forge.gif",
            &frames,
            "a Tear Stone forged by Sunshine in Yallam",
        )
    }
}

/// Debug set 19, the party once Piers has joined, as it reaches Yallam.
const PARTY_PIERS_FORGE: u32 = 19;

/// Contigo's game room (SceneIdContigo3) and the man who offers Super
/// Lucky Dice there (contigo.c: Contigo_SendToSuperLuckyDice, actor 8).
const CONTIGO_GAMES: &str = "ContigoLuckyDice";
const SUPER_DICE_MAN: u16 = 8;
/// Coins given first: a round costs the party's average level times ten.
const DICE_COINS: u32 = 5000;

/// The modes of Contigo's games.
pub const DICE_MODES: &[&str] = &["overworld-super-lucky-dice"];

impl<'a> Guide<'a> {
    /// The man in Contigo's game room talked to, Yes to a round of Super
    /// Lucky Dice, and the game it runs (SEQUENCE_SUPER_DICE): its four dice
    /// thrown with A, then the Hi-Lo game, A pressed every second, as
    /// `overworld/super-lucky-dice.gif`.
    pub(super) fn tla_super_lucky_dice(&mut self) -> Result<(), String> {
        let (mut entered, _) = self.tla_stand(
            CONTIGO_GAMES,
            0,
            PARTY_PIERS_FORGE,
            Trigger::Talk(SUPER_DICE_MAN),
            |_| true,
        )?;
        let field = &mut entered.field;
        field.set_coins(DICE_COINS);
        let mut path = vec![
            (0, 20),
            (KEY_A, 3),
            (0, 160),
            (KEY_A, 3),
            (0, 120),
            (KEY_A, 3),
            (0, 240),
        ];
        for _ in 0..18 {
            path.extend([(KEY_A, 3), (0, 60)]);
        }
        let mut frames = Self::press_path_cleared(field, &path)?;
        frames.remove(0);
        self.world_gif(
            "overworld/super-lucky-dice.gif",
            &frames,
            "a round of Super Lucky Dice in Contigo",
        )
    }
}
