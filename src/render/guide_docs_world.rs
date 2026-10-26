//! More recordings for the world layer's guides (the layout and map folder
//! guides): the sand haze a layout's header drifts over Suhalla Desert, a
//! Psynergy Stone picked up from its treasure spot, and the start of a
//! Colosso final's race. Each plays a built edition with the pad held as a
//! player would; guide_world_play dispatches them.

use super::field::{Field, Setup};
use super::guide::Guide;

/// KEYINPUT bits.
const KEY_UP: u32 = 0x40;
const KEY_DOWN: u32 = 0x80;

/// Facings, 1/65536 turns (enum Facing).
const NORTH: u32 = 0xc000;

/// Suhalla Desert's north (SceneId_SuhallaDesertNorth, map_147) entered from
/// the world map (SUHALLA_DESERT_NORTH_FROM_WORLD_MAP).
const SUHALLA_DESERT_NORTH: (u16, u16) = (164, 1);
/// FLAG_SUHALLA_DESERT_SANDSTORMS_RISEN, FLAG_AREA_TEMP_40 in
/// map/suhalla_desert.c: a storm of the desert has risen.
const FLAG_SUHALLA_DESERT_SANDSTORMS_RISEN: u16 = 0x340;
/// The Psynergy Stone room of Sol Sanctum (SceneId_SolSanctumPsynergyStone)
/// entered from the statue room (SOL_SANCTUM_PSYNERGY_STONE_FROM_SOL_SANCTUM_STATUE),
/// whose treasure spot 100 (map_011's cell 4, 5) holds the stone.
const SOL_SANCTUM_PSYNERGY_STONE: (u16, u16) = (13, 1);
/// Colosseum Final 1 (SceneId_ColosseumAzartMatch) at the race
/// (COLOSSEUM_AZART_MATCH_RACE_FROM_LEND_A_HAND).
const COLOSSEUM_AZART_RACE: (u16, u16) = (143, 1);
/// Venus Lighthouse's aerie (SceneId_VenusLighthouseAerie) back from the first
/// battle (VENUS_LIGHTHOUSE_AERIE_AFTER_FIRST_BATTLE), where
/// VenusLighthouseAerie_CombineSaturosAndMenardi plays, and the Venus Star's
/// shadow (ACTOR_VENUS_STAR_SHADOW), kept at the map's origin until the
/// star is thrown.
const VENUS_LIGHTHOUSE_AERIE_AFTER_FIRST_BATTLE: (u16, u16) = (187, 2);
const ACTOR_VENUS_STAR_SHADOW: u16 = 22;
/// Frames kept before the shadow leaves the origin (Saturos jumps and the
/// star shows above him), and the last frame held at the end so the loop
/// rests before it starts again.
const STAR_LEAD: usize = 24;
const STAR_HOLD: usize = 30;

impl<'a> Guide<'a> {
    pub(super) fn run_docs_world(&mut self, name: &str) -> Result<(), String> {
        match name {
            "layout-haze" => self.haze(),
            "layout-psynergy-stone" => self.psynergy_stone(),
            "map-colosso-race" => self.colosso_race(),
            "map-venus-star-thrown" => self.venus_star_thrown(),
            "venus-lighthouse-aerie-beam-whole" => self.venus_beam_whole(),
            "venus-lighthouse-aerie-eruption-whole" => self.venus_eruption_whole(),
            "venus-lighthouse-aerie-story-dollhouse" => self.venus_story_dollhouse(),
            _ => Err(format!("no mode {name}")),
        }
    }

    /// Suhalla Desert's north as the party comes back to it once a storm has
    /// risen (FLAG_SUHALLA_DESERT_SANDSTORMS_RISEN set, so the map's setup
    /// shows the haze at full weight, SUHALLA_DESERT_HAZE_BLEND): the sand
    /// haze, BG1 as map_147's header scrolls and drifts it, streams across
    /// the desert while the party stands, then walks south.
    fn haze(&mut self) -> Result<(), String> {
        let mut entered = self.enter_first_visit(SUHALLA_DESERT_NORTH, SUHALLA_DESERT_NORTH.0)?;
        entered.field.load(&entered.placed);
        entered.field.set_flag(FLAG_SUHALLA_DESERT_SANDSTORMS_RISEN, true);
        entered.field.until_control()?;
        entered.field.frames(20)?;
        let mut frames = vec![entered.field.picture()];
        for (keys, n) in [(0, 120), (KEY_DOWN, 70), (0, 60)] {
            Self::hold(&mut entered.field, keys, n, &mut frames)?;
        }
        self.world_gif("layout/haze.gif", &frames, "the sand haze over Suhalla Desert's north")
    }

    /// A Psynergy Stone searched in Sol Sanctum (treasure spot 100, a
    /// search row with FIND_PSYNERGY_STONE): the stone the treasure spot
    /// placed rises over the leader, the party's PP is filled, and the stone
    /// disappears.
    fn psynergy_stone(&mut self) -> Result<(), String> {
        let mut entered = self.enter_with(SOL_SANCTUM_PSYNERGY_STONE, &[])?;
        let spot = Self::spot_at(&mut entered.field, (72, 110, NORTH))?;
        entered.field.stand(&entered.placed, &spot)?;
        let mut frames = vec![entered.field.picture()];
        Self::hold(&mut entered.field, 0, 15, &mut frames)?;
        Self::hold(&mut entered.field, KEY_UP, 4, &mut frames)?;
        Self::hold(&mut entered.field, 0, 10, &mut frames)?;
        Self::talk_through(&mut entered.field, &mut frames)?;
        Self::hold(&mut entered.field, 0, 30, &mut frames)?;
        self.world_gif("layout/psynergy-stone.gif", &frames, "a Psynergy Stone taken in Sol Sanctum")
    }

    /// The start of Colosso's first final as the race begins: from the party
    /// placed at the start, through the lettering and the count, to the race
    /// gauge filling for Azart, who follows his recorded path
    /// (Colosso_StartOpponent, colosso/opponent_path_a.tsv) while Isaac
    /// stands.
    fn colosso_race(&mut self) -> Result<(), String> {
        let mut set = vec![self.flag("FLAG_VALE_PARTY_LEFT")?];
        for f in self.flags.before_first_visit(COLOSSEUM_AZART_RACE.0)? {
            if !set.contains(&f) {
                set.push(f);
            }
        }
        let clear = vec![self.flag("FLAG_VALE_STORM")?, self.flag("FLAG_CHILDHOOD")?];
        let setup = Setup { scene: COLOSSEUM_AZART_RACE.0, entrance: COLOSSEUM_AZART_RACE.1, set, clear };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        let mut frames = Vec::new();
        Self::hold(&mut field, 0, 650, &mut frames)?;
        Self::hold(&mut field, KEY_UP, 30, &mut frames)?;
        self.world_gif("map/colosso-race.gif", &frames, "the start of Colosso's first final")
    }
}

impl Guide<'_> {
    /// Saturos throwing the Venus Star into Venus Lighthouse's beacon, as
    /// `map/venus-star-thrown.gif`: the aerie back from the first battle,
    /// its pages answered with A, from a moment before the star shows above
    /// him (its shadow leaves the map's origin) through its three bounces
    /// until it drops into the beacon as the group's bubbles come up (its
    /// first emote bubble, probed), that frame held. The GIF loops forever.
    fn venus_star_thrown(&mut self) -> Result<(), String> {
        let set = vec![self.flag("FLAG_VALE_PARTY_LEFT")?];
        let clear = vec![self.flag("FLAG_VALE_STORM")?, self.flag("FLAG_CHILDHOOD")?];
        let place = VENUS_LIGHTHOUSE_AERIE_AFTER_FIRST_BATTLE;
        let setup = Setup { scene: place.0, entrance: place.1, set, clear };
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        // VenusLighthouseAerie_ShowGroupEmote pops each one's bubble through
        // Actor_ShowEmote, FieldEffect_SpawnLinked.
        let emote = self.sym("FieldEffect_SpawnLinked")?;
        let reacted = std::rc::Rc::new(std::cell::Cell::new(false));
        let seen = reacted.clone();
        field.probe_pcs = vec![emote];
        field.probe = Some(Box::new(move |_, pc, _| {
            if pc == emote {
                seen.set(true);
            }
        }));
        let thrown = |f: &Field| f.actor(ACTOR_VENUS_STAR_SHADOW).is_some_and(|a| a.x != 0 || a.z != 0);
        let mut star = false;
        let mut kept: std::collections::VecDeque<Vec<u16>> = std::collections::VecDeque::new();
        let mut frames: Vec<Vec<u16>> = Vec::new();
        for _ in 0..12000 {
            field.frames(1)?;
            if field.scene() != place.0 {
                break;
            }
            let picture = field.picture();
            if !star {
                if thrown(&field) {
                    star = true;
                    reacted.set(false);
                    frames.extend(kept.drain(..));
                    frames.push(picture);
                } else {
                    if field.waiting() {
                        field.press(3)?;
                    }
                    kept.push_back(picture);
                    if kept.len() > STAR_LEAD {
                        kept.pop_front();
                    }
                }
                continue;
            }
            frames.push(picture);
            if reacted.get() || frames.len() > 900 {
                break;
            }
        }
        // The star leaves the screen in the frame the bubbles come up: that
        // frame ends it, held, the star gone and the group startled.
        field.frames(1)?;
        frames.push(field.picture());
        field.probe = None;
        if !star {
            return Err("the Venus Star was never thrown".to_string());
        }
        let last = frames.last().cloned().ok_or("no frames")?;
        frames.extend(std::iter::repeat(last).take(STAR_HOLD));
        self.world_gif("map/venus-star-thrown.gif", &frames, "Saturos throwing the Venus Star into Venus Lighthouse's beacon")
    }
}
