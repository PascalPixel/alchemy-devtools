//! Where the structures the run reads keep what it reads: the main routine's
//! work, the map's, the field's, an actor's, a scene's tables and a unit's,
//! for each game. The Broken Seal's are written here, as they have been read
//! from its headers since the renderer began; The Lost Age's are read from
//! its own headers at run time (Layout::read): each value a C expression,
//! `offsetof` or a header's constant, compiled by the edition's own recipe
//! (compile::steps), so a moved field moves the run with it and a renamed
//! one stops it with the name it wants. A test compiles The Broken Seal's
//! expressions the same way and checks them against its written values.



/// The two games the renderer plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Game {
    /// The Broken Seal, `tbs`.
    Tbs,
    /// The Lost Age, `tla`.
    Tla,
}

impl Game {
    /// The game an edition (`tla-en`) belongs to.
    pub fn of(edition: &str) -> Result<Game, String> {
        match edition.split('-').next() {
            Some("tbs") => Ok(Game::Tbs),
            Some("tla") => Ok(Game::Tla),
            _ => Err(format!("{edition}: no game of the renderer's")),
        }
    }

    /// Its folder under games/: `tbs` or `tla`.
    pub fn dir(self) -> &'static str {
        match self {
            Game::Tbs => "tbs",
            Game::Tla => "tla",
        }
    }
}

/// Byte offsets into the game's structures, sizes, counts and the few
/// constants of its headers the run needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub game: Game,
    /// struct MainRoutineWork: coins, the scene and entrance to run, the
    /// party's position (x, y and z, s32 16.16 pixels, in a row), heading
    /// and layer, the leader, the active party.
    pub gs_coins: u32,
    pub gs_scene: u32,
    pub gs_entrance: u32,
    pub gs_x: u32,
    pub gs_y: u32,
    pub gs_z: u32,
    pub gs_heading: u32,
    pub gs_turn: u32,
    pub gs_selected_actor: u32,
    pub gs_active_owners: u32,
    /// The map's work: the followed point, the view's top left (x, then y),
    /// the scroll bounds (minX, minY, maxX, maxY), the scrolling layers and
    /// each one's size, and a layer's cells pointer.
    pub map_origin: u32,
    pub map_view_x: u32,
    pub map_view_y: u32,
    pub map_bounds: u32,
    pub map_layers: u32,
    pub map_layer_size: u32,
    pub map_layer_cells: u32,
    /// A layer's scroll offset, parallax scale, drift speed and phase (x then
    /// y, words) and wrap mask (x then y, halfwords).
    pub layer_offset: u32,
    pub layer_scale: u32,
    pub layer_speed: u32,
    pub layer_phase: u32,
    pub layer_mask: u32,
    /// struct FieldWork: the trigger table, the actors and how many there
    /// are, the field Psynergy request, the raised trigger and the mode.
    pub field_events: u32,
    pub field_actors: u32,
    pub field_actor_count: u32,
    pub field_command: u32,
    pub field_raised: u32,
    pub field_mode: u32,
    /// struct Actor: facing, x, y, z, ground height, layer, velocity, sprite
    /// and sprite kind.
    pub obj_angle: u32,
    pub obj_x: u32,
    pub obj_y: u32,
    pub obj_z: u32,
    pub obj_terrain_height: u32,
    pub obj_terrain_id: u32,
    pub obj_velocity: u32,
    pub obj_sprite: u32,
    pub obj_kind: u32,
    /// The size of an entrance table row (struct SceneEntrance) and of a
    /// trigger table row (struct Trigger).
    pub entrance_size: u32,
    pub event_row_size: u32,
    /// A unit's Psynergy slots, four bytes each.
    pub unit_action_slots: u32,
    /// The flag that stops random encounters.
    pub flag_no_encounters: u16,
    /// The heap slots that point to the map's work and the field's, as
    /// offsets into gHeapSlots.
    pub heap_map: u32,
    pub heap_field: u32,
}

/// Each value of a Layout, by name, as a C expression of each game's
/// headers: The Broken Seal's, then The Lost Age's.
const EXPRESSIONS: &[(&str, &str, &str)] = &[
    ("gs_coins", "offsetof(struct MainRoutineWork, coins)", "offsetof(struct MainRoutineWork, coins)"),
    ("gs_scene", "offsetof(struct MainRoutineWork, next)", "offsetof(struct MainRoutineWork, scene)"),
    ("gs_entrance", "offsetof(struct MainRoutineWork, entrance)", "offsetof(struct MainRoutineWork, entrance)"),
    ("gs_x", "offsetof(struct MainRoutineWork, x)", "offsetof(struct MainRoutineWork, x)"),
    ("gs_y", "offsetof(struct MainRoutineWork, y)", "offsetof(struct MainRoutineWork, y)"),
    ("gs_z", "offsetof(struct MainRoutineWork, z)", "offsetof(struct MainRoutineWork, z)"),
    ("gs_heading", "offsetof(struct MainRoutineWork, heading)", "offsetof(struct MainRoutineWork, heading)"),
    ("gs_turn", "offsetof(struct MainRoutineWork, layer)", "offsetof(struct MainRoutineWork, turn)"),
    ("gs_selected_actor", "offsetof(struct MainRoutineWork, leaderActorId)", "offsetof(struct MainRoutineWork, currentOwner)"),
    ("gs_active_owners", "offsetof(struct MainRoutineWork, party)", "offsetof(struct MainRoutineWork, activeOwners)"),
    ("map_origin", "offsetof(struct MapGridScrollWork, origin)", "offsetof(struct MapWork, origin)"),
    ("map_view_x", "offsetof(struct MapGridScrollWork, view[0])", "offsetof(struct MapWork, viewX)"),
    ("map_view_y", "offsetof(struct MapGridScrollWork, view[1])", "offsetof(struct MapWork, viewZ)"),
    ("map_bounds", "offsetof(struct MapGridScrollWork, minX)", "offsetof(struct MapWork, minX)"),
    ("map_layers", "offsetof(struct MapGridScrollWork, layers)", "offsetof(struct MapWork, layers)"),
    ("map_layer_size", "sizeof(struct MapGridLayerScroll)", "sizeof(struct MapLayer)"),
    ("map_layer_cells", "offsetof(struct MapGridLayerScroll, cells)", "offsetof(struct MapLayer, cells)"),
    // The Lost Age's layer leaves these unnamed: offsetX, scaleX, speedX,
    // phaseX and maskX are the names wanted.
    ("layer_offset", "offsetof(struct MapGridLayerScroll, offsetX)", "offsetof(struct MapLayer, unk08)"),
    ("layer_scale", "offsetof(struct MapGridLayerScroll, scaleX)", "offsetof(struct MapLayer, unk10)"),
    ("layer_speed", "offsetof(struct MapGridLayerScroll, speedX)", "offsetof(struct MapLayer, unk18)"),
    ("layer_phase", "offsetof(struct MapGridLayerScroll, phaseX)", "offsetof(struct MapLayer, unk20)"),
    ("layer_mask", "offsetof(struct MapGridLayerScroll, maskX)", "offsetof(struct MapLayer, unk28)"),
    ("field_events", "offsetof(struct FieldWork, triggers)", "offsetof(struct FieldWork, triggers)"),
    ("field_actors", "offsetof(struct FieldWork, actors)", "offsetof(struct FieldWork, actors)"),
    ("field_actor_count", "ACTOR_ID_COUNT", "FIELD_ACTOR_COUNT"),
    ("field_command", "offsetof(struct FieldWork, psynergy)", "offsetof(struct FieldWork, psynergy)"),
    ("field_raised", "offsetof(struct FieldWork, raisedTrigger)", "offsetof(struct FieldWork, raisedTrigger)"),
    ("field_mode", "offsetof(struct FieldWork, mode)", "offsetof(struct FieldWork, mode)"),
    ("obj_angle", "offsetof(struct Actor, facing)", "offsetof(struct Actor, facing)"),
    ("obj_x", "offsetof(struct Actor, x)", "offsetof(struct Actor, x)"),
    ("obj_y", "offsetof(struct Actor, y)", "offsetof(struct Actor, y)"),
    ("obj_z", "offsetof(struct Actor, z)", "offsetof(struct Actor, z)"),
    ("obj_terrain_height", "offsetof(struct Actor, groundHeight)", "offsetof(struct Actor, terrainHeight)"),
    ("obj_terrain_id", "offsetof(struct Actor, layer)", "offsetof(struct Actor, terrainId)"),
    ("obj_velocity", "offsetof(struct Actor, velocityX)", "offsetof(struct Actor, velocityX)"),
    ("obj_sprite", "offsetof(struct Actor, sprite)", "offsetof(struct Actor, sprite)"),
    ("obj_kind", "offsetof(struct Actor, spriteStorageKind)", "offsetof(struct Actor, animationKind)"),
    ("entrance_size", "sizeof(struct SceneEntrance)", "sizeof(struct SceneEntrance)"),
    ("event_row_size", "sizeof(struct Trigger)", "sizeof(struct Trigger)"),
    ("unit_action_slots", "offsetof(struct Unit, psynergy)", "offsetof(struct BattleUnit, actionSlots)"),
    // The Lost Age's flags.h leaves flag 0x160 unnamed; its encounter roll
    // (Encounter_CountStepsAndSelectFormation) tests it where The Broken
    // Seal's tests FLAG_NO_ENCOUNTERS, the name wanted.
    ("flag_no_encounters", "FLAG_NO_ENCOUNTERS", "0x160"),
    ("heap_map", "HEAP_SLOT_MAP_GRID * 4", "offsetof(struct HeapSlots, mapWork)"),
    ("heap_field", "HEAP_SLOT_FIELD * 4", "offsetof(struct HeapSlots, fieldWork)"),
];

/// The headers each game's expressions read.
fn headers(game: Game) -> &'static [&'static str] {
    match game {
        Game::Tbs => &[
            "types.h",
            "system/main_routine_work.h",
            "field/map_grid_scroll.h",
            "field/field_work.h",
            "field/actor.h",
            "field/scene_tables.h",
            "battle/battle_unit.h",
            "system/heap.h",
            "flags.h",
        ],
        Game::Tla => &[
            "types.h",
            "stddef.h",
            "system/main_routine_work.h",
            "map.h",
            "field/field_work.h",
            "actor.h",
            "scene_tables.h",
            "battle/battle_unit.h",
            "system/ram_buffer.h",
            "flags.h",
        ],
    }
}

impl Layout {
    /// The Broken Seal's, as its headers have them (the test
    /// `the_broken_seal_layout_is_its_headers` reads them again).
    pub fn tbs() -> Layout {
        Layout {
            game: Game::Tbs,
            gs_coins: 0x10,
            gs_scene: 0x1c0,
            gs_entrance: 0x1c2,
            gs_x: 0x1dc,
            gs_y: 0x1e0,
            gs_z: 0x1e4,
            gs_heading: 0x1e8,
            gs_turn: 0x1ec,
            gs_selected_actor: 0x1f4,
            gs_active_owners: 0x1f8,
            map_origin: 0x00,
            map_view_x: 0xe4,
            map_view_y: 0xe8,
            map_bounds: 0xec,
            map_layers: 0x104,
            map_layer_size: 0x30,
            map_layer_cells: 0x2c,
            layer_offset: 0x08,
            layer_scale: 0x10,
            layer_speed: 0x18,
            layer_phase: 0x20,
            layer_mask: 0x28,
            field_events: 0x10,
            field_actors: 0x14,
            field_actor_count: 66,
            field_command: 0x17e,
            field_raised: 0x182,
            field_mode: 0x19e,
            obj_angle: 0x06,
            obj_x: 0x08,
            obj_y: 0x0c,
            obj_z: 0x10,
            obj_terrain_height: 0x14,
            obj_terrain_id: 0x22,
            obj_velocity: 0x24,
            obj_sprite: 0x50,
            obj_kind: 0x54,
            entrance_size: 24,
            event_row_size: 12,
            unit_action_slots: 0x58,
            flag_no_encounters: 0x160,
            heap_map: 0x20,
            heap_field: 0x6c,
        }
    }

    /// The game's layout: The Broken Seal's as written, The Lost Age's read
    /// from its headers in the checkout by edition `edition`'s recipe.
    pub fn for_edition(edition: &str) -> Result<Layout, String> {
        match Game::of(edition)? {
            Game::Tbs => Ok(Layout::tbs()),
            Game::Tla => Layout::read(edition),
        }
    }

    /// Compiles every expression of the edition's game with its headers, by
    /// the recipe its build gives any C file, and reads the values back
    /// from the assembly, one `.word` each, in order.
    pub fn read(edition: &str) -> Result<Layout, String> {
        let game = Game::of(edition)?;
        let target = crate::alchemy::targets::decomp_target(Some(edition))?;
        let work = tempfile::tempdir().map_err(|e| format!("cannot make a folder for the layout probe: {e}"))?;
        let source = work.path().join("layout_probe.c");
        let object = work.path().join("layout_probe.o");
        let mut text = String::from("/* What alchemy-devtools render reads, from the headers. */\n");
        for header in headers(game) {
            text.push_str(&format!("#include \"{header}\"\n"));
        }
        text.push_str("#ifndef offsetof\n#define offsetof(type, member) ((u32) & ((type *)0)->member)\n#endif\nconst int gRenderLayout[] = {\n");
        for (_, tbs, tla) in EXPRESSIONS {
            text.push_str(&format!("  (int)({}),\n", if game == Game::Tbs { tbs } else { tla }));
        }
        text.push_str("};\n");
        std::fs::write(&source, text).map_err(|e| format!("cannot write {}: {e}", source.display()))?;
        for step in crate::compile::steps(target, &source, &object, None, false)? {
            crate::gba::process::run(&step, work.path()).map_err(|e| format!("the layout probe of {}'s headers: {e}", game.dir()))?;
        }
        let assembly = std::fs::read_to_string(object.with_extension("s")).map_err(|e| format!("the layout probe wrote no assembly: {e}"))?;
        let values: Vec<i64> = assembly
            .lines()
            .filter_map(|l| l.trim().strip_prefix(".word"))
            .filter_map(|v| {
                let v = v.trim();
                v.strip_prefix("0x").map_or_else(|| v.parse().ok(), |h| i64::from_str_radix(h, 16).ok())
            })
            .collect();
        if values.len() != EXPRESSIONS.len() {
            return Err(format!("the layout probe gave {} values for {} expressions", values.len(), EXPRESSIONS.len()));
        }
        let get = |name: &str| -> u32 {
            let i = EXPRESSIONS.iter().position(|(n, ..)| *n == name).expect("a layout value's name");
            values[i] as u32
        };
        Ok(Layout {
            game,
            gs_coins: get("gs_coins"),
            gs_scene: get("gs_scene"),
            gs_entrance: get("gs_entrance"),
            gs_x: get("gs_x"),
            gs_y: get("gs_y"),
            gs_z: get("gs_z"),
            gs_heading: get("gs_heading"),
            gs_turn: get("gs_turn"),
            gs_selected_actor: get("gs_selected_actor"),
            gs_active_owners: get("gs_active_owners"),
            map_origin: get("map_origin"),
            map_view_x: get("map_view_x"),
            map_view_y: get("map_view_y"),
            map_bounds: get("map_bounds"),
            map_layers: get("map_layers"),
            map_layer_size: get("map_layer_size"),
            map_layer_cells: get("map_layer_cells"),
            layer_offset: get("layer_offset"),
            layer_scale: get("layer_scale"),
            layer_speed: get("layer_speed"),
            layer_phase: get("layer_phase"),
            layer_mask: get("layer_mask"),
            field_events: get("field_events"),
            field_actors: get("field_actors"),
            field_actor_count: get("field_actor_count"),
            field_command: get("field_command"),
            field_raised: get("field_raised"),
            field_mode: get("field_mode"),
            obj_angle: get("obj_angle"),
            obj_x: get("obj_x"),
            obj_y: get("obj_y"),
            obj_z: get("obj_z"),
            obj_terrain_height: get("obj_terrain_height"),
            obj_terrain_id: get("obj_terrain_id"),
            obj_velocity: get("obj_velocity"),
            obj_sprite: get("obj_sprite"),
            obj_kind: get("obj_kind"),
            entrance_size: get("entrance_size"),
            event_row_size: get("event_row_size"),
            unit_action_slots: get("unit_action_slots"),
            flag_no_encounters: get("flag_no_encounters") as u16,
            heap_map: get("heap_map"),
            heap_field: get("heap_field"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn games_come_from_editions() {
        assert_eq!(Game::of("tbs-en"), Ok(Game::Tbs));
        assert_eq!(Game::of("tla-de"), Ok(Game::Tla));
        assert!(Game::of("emerald").is_err());
    }

    #[test]
    fn the_broken_seal_layout_is_its_headers() {
        let root = crate::compile::root();
        if !root.join("out/tbs-en/tbs-en.gba").exists() {
            return;
        }
        assert_eq!(Layout::read("tbs-en").unwrap(), Layout::tbs());
    }

    #[test]
    fn the_lost_age_layout_reads_its_headers() {
        let root = crate::compile::root();
        if !root.join("out/tla-en/tla-en.gba").exists() {
            return;
        }
        let layout = Layout::read("tla-en").unwrap();
        assert_eq!(layout.game, Game::Tla);
        assert_eq!(layout.entrance_size, 24);
        assert_eq!(layout.event_row_size, 12);
        assert!(layout.gs_x > layout.gs_scene);
    }
}
