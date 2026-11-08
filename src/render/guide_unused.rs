//! Code the game never reaches, staged by hand: each row names the moment its
//! code needs (the room, the flags, where the leader stands), so that what the
//! function does happens on screen. A row is added after its frames have been
//! looked at against the function's name and comment; a start with nothing
//! staged (Trigger::Direct from the scene's arrival) proves only that pixels
//! changed, often off camera or in another room of the overlay.

use super::animation::{is_black_bgr555, BLACK_CLEARANCE};
use super::field::{Field, Setup, Spot, Start, LEAD_IN};
use super::guide::Guide;
use super::source;
use super::story::Trigger;

const SOUTH: u32 = 0x4000;
const NORTH: u32 = 0xc000;
/// Frames kept after the event: past the black-screen rule, for one that
/// leaves its room (animation::BLACK_CLEARANCE).
const AFTER_LEAVING: u32 = 620;
const AFTER: u32 = 60;

/// Where the leader stands for a row: always ground he can walk to from his
/// arrival (Field::reach), never a coordinate taken on trust.
enum Stand {
    /// The cell he can walk to nearest (x, z) pixels.
    Near(i32, i32, u32),
    /// Not on stage at all: he is hidden and his position only places the camera,
    /// for code whose actors stand where the party does not.
    Camera(i32, i32),
    /// Beside a step trigger's cell, as Field::spots finds it.
    OnTouch(u16, u32),
}

struct Stage {
    function: &'static str,
    scene: &'static str,
    /// A real arrival: the number of an entrance another map's exit leads to.
    /// Entrance 0 is often the debug default, which can stand in the sky.
    entrance: u16,
    set: &'static [&'static str],
    stand: Stand,
    /// An actor the function needs on stage that only another event places:
    /// its id and position.
    place: Option<(u16, i32, i32)>,
    tail: u32,
}

const STAGES: &[Stage] = &[
    // At the foot of the Sanctum's door on the stormy night; it leads into a
    // house, as the function says.
    Stage { function: "ValePrologue_UnusedOpenSanctumDoor", scene: "ValePrologue", entrance: 1, set: &["FLAG_VALE_STORM", "FLAG_CHILDHOOD"], stand: Stand::Near(257, 440, NORTH), place: None, tail: AFTER_LEAVING },
    // In the eastern hall, where the statue it moves stands.
    Stage { function: "SolSanctumStatue_UnusedMoveNorthwestStatue", scene: "SolSanctumStatue", entrance: 1, set: &[], stand: Stand::Camera(560, 170), place: None, tail: AFTER },
    // Three seals open, the state its only caller asks for.
    Stage {
        function: "SolSanctumStatue_UnusedPutOutSeals",
        scene: "SolSanctumStatue",
        entrance: 1,
        set: &["FLAG_SOL_SANCTUM_CENTER_STATUE_MOVED", "FLAG_SOL_SANCTUM_NORTHWEST_SEAL_OPEN", "FLAG_SOL_SANCTUM_SOUTHWEST_SEAL_OPEN", "FLAG_SOL_SANCTUM_SOUTHEAST_SEAL_OPEN"],
        stand: Stand::Near(575, 132, SOUTH),
        place: None,
        tail: AFTER,
    },
    // Alex (actor 24) where MercuryLighthouseAerie_AlexAppears walks him, in
    // one view with the lift's statue.
    Stage { function: "MercuryLighthouseAerie_UnusedAlexCastsPly", scene: "MercuryLighthouseAerie", entrance: 5, set: &[], stand: Stand::Camera(207, 149), place: Some((24, 312, 176)), tail: AFTER },
    // In the Keys room, on the cell below the third blue door, facing south as its
    // rows ask.
    Stage { function: "CrossboneIsleKeys_UnusedJumpOntoPost", scene: "CrossboneIsleKeys", entrance: 0, set: &[], stand: Stand::OnTouch(53, SOUTH), place: None, tail: AFTER },
];

/// The address of a function in the overlay that defines it.
fn overlay_function(overlays: &std::path::Path, name: &str) -> Result<u32, String> {
    for entry in std::fs::read_dir(overlays).map_err(|e| format!("{}: {e}", overlays.display()))?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("elf") {
            continue;
        }
        let Some(elf) = std::fs::read(&path).ok().and_then(|data| crate::gba::elf::Elf::parse(&data).ok()) else {
            continue;
        };
        if let Some(symbol) = elf.symbols.iter().find(|y| y.kind == 2 && y.name == name) {
            return Ok(symbol.value & !1);
        }
    }
    Err(format!("no overlay defines {name}"))
}

impl<'a> Guide<'a> {
    /// `render guide --only unused`, from an Alchemy checkout with a built tbs-en:
    /// every row, as unused/FUNCTION.gif.
    pub(super) fn unused(&mut self) -> Result<(), String> {
        let overlays = crate::compile::root().join("out/tbs-en/overlays");
        std::fs::create_dir_all(self.out.join("unused")).map_err(|e| e.to_string())?;
        for stage in STAGES {
            let frames = self.stage(stage, &overlays).map_err(|e| format!("{}: {e}", stage.function))?;
            self.gif(&format!("unused/{}.gif", stage.function), &frames, stage.function)?;
        }
        Ok(())
    }

    fn stage(&self, stage: &Stage, overlays: &std::path::Path) -> Result<Vec<Vec<u16>>, String> {
        let set = stage.set.iter().map(|name| self.flag(name)).collect::<Result<Vec<u16>, String>>()?;
        let setup = Setup { scene: source::map_id(stage.scene)?, entrance: stage.entrance, set, clear: Vec::new() };
        let address = overlay_function(overlays, stage.function)?;
        let mut field = Field::enter(self.rom, self.symbols, &setup)?;
        let placed = field.save();
        field.until_control()?;
        field.frames(20)?;
        let spot: Spot = match stage.stand {
            Stand::Near(x, z, facing) => {
                let reach = field.reach(None)?;
                let layer = reach.layer;
                let mut cells: Vec<(i32, i32)> = (0..128).flat_map(|cz| (0..128).map(move |cx| (cx * 16 + 8, cz * 16 + 8))).filter(|&(cx, cz)| reach.holds(layer, cx, cz) && reach.open(layer, cx, cz) >= 3).collect();
                cells.sort_by_key(|&(cx, cz)| (cx - x).pow(2) + (cz - z).pow(2));
                let &(cx, cz) = cells.first().ok_or("no ground he can walk to")?;
                eprintln!("{}: stands at ({cx}, {cz}), for ({x}, {z})", stage.function);
                Self::spot_at(&mut field, (cx, cz, facing))?
            }
            Stand::Camera(x, z) => {
                field.hide_leader = true;
                Self::spot_at(&mut field, (x, z, SOUTH))?
            }
            Stand::OnTouch(code, facing) => {
                let mut spot = field.spots(Trigger::Touch(code))?.into_iter().next().ok_or("no cell to stand on")?;
                spot.facing = facing;
                spot.walk = 0;
                spot
            }
        };
        let start = match stage.place {
            Some((actor, x, z)) => {
                field.put_leader(&spot);
                field.put_actor(actor, x, z);
                field.frames(4)?;
                let ready = field.save();
                ready
            }
            None => placed,
        };
        let begin = |saved| if stage.place.is_some() { Start::InControl(saved) } else { Start::Entered(saved) };
        let mut frames = field.event(begin(&start), Trigger::Direct(address), &spot, &[], &[], 8, Some(4000))?.frames;
        if !field.hide_leader && stage.tail == AFTER {
            let wrong = field.misplaced()?;
            if !wrong.is_empty() {
                return Err(format!("the leader ends misplaced: {}", wrong.join(", ")));
            }
        }
        // One that reaches a black screen too soon is played again with the
        // leader standing ready for longer (animation::BLACK_CLEARANCE).
        if let Some(first) = frames.iter().position(|f| is_black_bgr555(f)).filter(|&b| b < BLACK_CLEARANCE + 30) {
            field.lead_in = LEAD_IN + (BLACK_CLEARANCE + 30 - first) as u32;
            frames = field.event(begin(&start), Trigger::Direct(address), &spot, &[], &[], 8, Some(4000))?.frames;
            field.lead_in = LEAD_IN;
        }
        for _ in 0..stage.tail {
            field.frames(1)?;
            frames.push(field.picture());
        }
        Ok(frames)
    }
}
