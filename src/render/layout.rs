//! Where the structures the run reads keep what it reads: the main routine's
//! state, the map's, the field's, an actor's, a scene's tables and a unit's,
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
    /// struct MainRoutineState: coins, the scene and entrance to run, the
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
    /// Whether the leader walks or climbs (movementMode: 1 while he climbs).
    pub gs_movement_mode: u32,
    /// The map's state: the followed point, the view's top left (x, then y),
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
    /// struct FieldState: the trigger table, the actors and how many there
    /// are, the field Psynergy request, the raised trigger and the mode.
    pub field_events: u32,
    pub field_actors: u32,
    pub field_actor_count: u32,
    pub field_command: u32,
    pub field_raised: u32,
    /// FieldState.exit: the exit taken, which ends the scene.
    pub field_exit: u32,
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
    /// The heap slots that point to the map's state and the field's, as
    /// offsets into gHeapSlots.
    pub heap_map: u32,
    pub heap_field: u32,
}

/// Each value of a Layout, by name, as a C expression of each game's
/// headers: The Broken Seal's, then The Lost Age's.
const EXPRESSIONS: &[(&str, &str, &str)] = &[
    (
        "gs_coins",
        "offsetof(struct MainRoutineState, holdings.coins)",
        "offsetof(struct MainRoutineState, coins)",
    ),
    (
        "gs_scene",
        "offsetof(struct MainRoutineState, next)",
        "offsetof(struct MainRoutineState, next)",
    ),
    (
        "gs_entrance",
        "offsetof(struct MainRoutineState, entrance)",
        "offsetof(struct MainRoutineState, entrance)",
    ),
    (
        "gs_x",
        "offsetof(struct MainRoutineState, x)",
        "offsetof(struct MainRoutineState, x)",
    ),
    (
        "gs_y",
        "offsetof(struct MainRoutineState, y)",
        "offsetof(struct MainRoutineState, y)",
    ),
    (
        "gs_z",
        "offsetof(struct MainRoutineState, z)",
        "offsetof(struct MainRoutineState, z)",
    ),
    (
        "gs_heading",
        "offsetof(struct MainRoutineState, heading)",
        "offsetof(struct MainRoutineState, heading)",
    ),
    (
        "gs_turn",
        "offsetof(struct MainRoutineState, layer)",
        "offsetof(struct MainRoutineState, layer)",
    ),
    (
        "gs_selected_actor",
        "offsetof(struct MainRoutineState, leaderActorId)",
        "offsetof(struct MainRoutineState, leaderActorId)",
    ),
    (
        "gs_active_owners",
        "offsetof(struct MainRoutineState, party)",
        "offsetof(struct MainRoutineState, party)",
    ),
    (
        "gs_movement_mode",
        "offsetof(struct MainRoutineState, movementMode)",
        "offsetof(struct MainRoutineState, movementMode)",
    ),
    (
        "map_origin",
        "offsetof(struct MapGridScrollState, origin)",
        "offsetof(struct MapState, origin)",
    ),
    (
        "map_view_x",
        "offsetof(struct MapGridScrollState, view[0])",
        "offsetof(struct MapState, viewX)",
    ),
    (
        "map_view_y",
        "offsetof(struct MapGridScrollState, view[1])",
        "offsetof(struct MapState, viewZ)",
    ),
    (
        "map_bounds",
        "offsetof(struct MapGridScrollState, minX)",
        "offsetof(struct MapState, minX)",
    ),
    (
        "map_layers",
        "offsetof(struct MapGridScrollState, layers)",
        "offsetof(struct MapState, layers)",
    ),
    (
        "map_layer_size",
        "sizeof(struct MapGridLayerScroll)",
        "sizeof(struct MapLayer)",
    ),
    (
        "map_layer_cells",
        "offsetof(struct MapGridLayerScroll, cells)",
        "offsetof(struct MapLayer, cells)",
    ),
    // The Lost Age's layer leaves these unnamed: offsetX, scaleX, speedX,
    // phaseX and maskX are the names wanted.
    (
        "layer_offset",
        "offsetof(struct MapGridLayerScroll, offsetX)",
        "offsetof(struct MapLayer, offsetX)",
    ),
    (
        "layer_scale",
        "offsetof(struct MapGridLayerScroll, scaleX)",
        "offsetof(struct MapLayer, scaleX)",
    ),
    (
        "layer_speed",
        "offsetof(struct MapGridLayerScroll, speedX)",
        "offsetof(struct MapLayer, speedX)",
    ),
    (
        "layer_phase",
        "offsetof(struct MapGridLayerScroll, phaseX)",
        "offsetof(struct MapLayer, phaseX)",
    ),
    (
        "layer_mask",
        "offsetof(struct MapGridLayerScroll, maskX)",
        "offsetof(struct MapLayer, maskX)",
    ),
    (
        "field_events",
        "offsetof(struct FieldState, triggers)",
        "offsetof(struct FieldState, triggers)",
    ),
    (
        "field_actors",
        "offsetof(struct FieldState, actors)",
        "offsetof(struct FieldState, actors)",
    ),
    ("field_actor_count", "ACTOR_ID_COUNT", "FIELD_ACTOR_COUNT"),
    (
        "field_command",
        "offsetof(struct FieldState, psynergy)",
        "offsetof(struct FieldState, psynergy)",
    ),
    (
        "field_raised",
        "offsetof(struct FieldState, raisedTrigger)",
        "offsetof(struct FieldState, raisedTrigger)",
    ),
    (
        "field_exit",
        "offsetof(struct FieldState, exit)",
        "offsetof(struct FieldState, exit)",
    ),
    (
        "field_mode",
        "offsetof(struct FieldState, mode)",
        "offsetof(struct FieldState, mode)",
    ),
    (
        "obj_angle",
        "offsetof(struct Actor, facing)",
        "offsetof(struct Actor, facing)",
    ),
    (
        "obj_x",
        "offsetof(struct Actor, x)",
        "offsetof(struct Actor, x)",
    ),
    (
        "obj_y",
        "offsetof(struct Actor, y)",
        "offsetof(struct Actor, y)",
    ),
    (
        "obj_z",
        "offsetof(struct Actor, z)",
        "offsetof(struct Actor, z)",
    ),
    (
        "obj_terrain_height",
        "offsetof(struct Actor, groundHeight)",
        "offsetof(struct Actor, groundHeight)",
    ),
    (
        "obj_terrain_id",
        "offsetof(struct Actor, layer)",
        "offsetof(struct Actor, layer)",
    ),
    (
        "obj_velocity",
        "offsetof(struct Actor, velocityX)",
        "offsetof(struct Actor, velocityX)",
    ),
    (
        "obj_sprite",
        "offsetof(struct Actor, sprite)",
        "offsetof(struct Actor, sprite)",
    ),
    (
        "obj_kind",
        "offsetof(struct Actor, spriteStorageKind)",
        "offsetof(struct Actor, spriteStorageKind)",
    ),
    (
        "entrance_size",
        "sizeof(struct SceneEntrance)",
        "sizeof(struct SceneEntrance)",
    ),
    (
        "event_row_size",
        "sizeof(struct Trigger)",
        "sizeof(struct Trigger)",
    ),
    (
        "unit_action_slots",
        "offsetof(struct Unit, psynergy)",
        "offsetof(struct Unit, psynergy)",
    ),
    // The Lost Age's flags.h leaves flag 0x160 unnamed; its encounter roll
    // (Encounter_CountStepsAndSelectFormation) tests it where The Broken
    // Seal's tests FLAG_NO_ENCOUNTERS, the name wanted.
    ("flag_no_encounters", "FLAG_NO_ENCOUNTERS", "0x160"),
    (
        "heap_map",
        "HEAP_SLOT_MAP_GRID * 4",
        "offsetof(struct HeapSlots, mapGrid)",
    ),
    (
        "heap_field",
        "HEAP_SLOT_FIELD * 4",
        "offsetof(struct HeapSlots, fieldState)",
    ),
];

/// The headers each game's expressions read.
fn headers(game: Game) -> &'static [&'static str] {
    match game {
        Game::Tbs => &[
            "types.h",
            "system/main_routine_state.h",
            "field/map_grid_scroll.h",
            "field/field_state.h",
            "field/actor.h",
            "field/scene_tables.h",
            "battle/battle_unit.h",
            "system/heap.h",
            "flags.h",
        ],
        Game::Tla => &[
            "types.h",
            "stddef.h",
            "system/main_routine_state.h",
            "field/map_grid.h",
            "field/field_state.h",
            "field/actor.h",
            "field/scene_tables.h",
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
            gs_movement_mode: 0x1f2,
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
            field_exit: 0x170,
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
    /// from the assembly, in order.
    pub fn read(edition: &str) -> Result<Layout, String> {
        let game = Game::of(edition)?;
        let expressions: Vec<&str> = EXPRESSIONS
            .iter()
            .map(|(_, tbs, tla)| if game == Game::Tbs { *tbs } else { *tla })
            .collect();
        let values = probe(edition, headers(game), &expressions)?;
        let get = |name: &str| -> u32 {
            let i = EXPRESSIONS
                .iter()
                .position(|(n, ..)| *n == name)
                .expect("a layout value's name");
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
            gs_movement_mode: get("gs_movement_mode"),
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
            field_exit: get("field_exit"),
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

/// Compiles `expressions` with `headers` by edition `edition`'s recipe for
/// any C file (compile::steps), as one array of ints, and reads each value
/// back from its assembly: a `.word`, or a run of zeros as `.space`.
fn probe(edition: &str, headers: &[&str], expressions: &[&str]) -> Result<Vec<i64>, String> {
    let target = crate::alchemy::targets::decomp_target(Some(edition))?;
    let work = tempfile::tempdir()
        .map_err(|e| format!("cannot make a folder for the layout probe: {e}"))?;
    let source = work.path().join("layout_probe.c");
    let object = work.path().join("layout_probe.o");
    let mut text = String::from("/* What alchemy-devtools render reads, from the headers. */\n");
    for header in headers {
        text.push_str(&format!("#include \"{header}\"\n"));
    }
    text.push_str("#ifndef offsetof\n#define offsetof(type, member) ((u32) & ((type *)0)->member)\n#endif\nconst int gRenderLayout[] = {\n");
    for expression in expressions {
        text.push_str(&format!("  (int)({expression}),\n"));
    }
    text.push_str("};\n");
    std::fs::write(&source, text).map_err(|e| format!("cannot write {}: {e}", source.display()))?;
    for step in crate::compile::steps(target, &source, &object, None, false)? {
        crate::gba::process::run(&step, work.path())
            .map_err(|e| format!("the layout probe of {edition}'s headers: {e}"))?;
    }
    let assembly = std::fs::read_to_string(object.with_extension("s"))
        .map_err(|e| format!("the layout probe wrote no assembly: {e}"))?;
    let number = |v: &str| {
        let v = v.trim();
        v.strip_prefix("0x").map_or_else(
            || v.parse::<i64>().ok(),
            |h| i64::from_str_radix(h, 16).ok(),
        )
    };
    let mut values: Vec<i64> = Vec::new();
    let mut started = false;
    for line in assembly.lines().map(str::trim) {
        if line.starts_with("gRenderLayout:") {
            started = true;
        } else if !started {
            continue;
        } else if let Some(v) = line.strip_prefix(".word") {
            values.push(number(v).ok_or_else(|| format!("the layout probe wrote {line:?}"))?);
        } else if let Some(n) = line.strip_prefix(".space").and_then(number) {
            values.extend(std::iter::repeat_n(0, (n / 4) as usize));
        } else if line.starts_with(".size") {
            break;
        }
    }
    if values.len() != expressions.len() {
        return Err(format!(
            "the layout probe gave {} values for {} expressions",
            values.len(),
            expressions.len()
        ));
    }
    Ok(values)
}

/// What a battle's run reads (battle.rs): the main routine's state, a
/// unit's record, the battle's state and a battle object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleLayout {
    /// MainRoutineState's active party, in order.
    pub state_active_owners: u32,
    /// MainRoutineState.battleType: 1, a set battle, rolls no first strike.
    pub state_battle_type: u32,
    /// The battle background the next battle draws (the scene whose
    /// background it is).
    pub state_battle_background: u32,
    /// The party's Djinn recovery list (its four-byte entries), from
    /// gMainRoutineState, and where its count is from the list.
    pub recovery_list: u32,
    pub recovery_count: u32,
    /// struct Unit (The Lost Age's BattleUnit): its size and fields.
    pub unit_size: u32,
    pub unit_base_hp: u32,
    pub unit_base_pp: u32,
    pub unit_hp_gauge: u32,
    pub unit_base_luck: u32,
    pub unit_max_hp: u32,
    pub unit_max_pp: u32,
    pub unit_hp: u32,
    pub unit_pp: u32,
    pub unit_agility: u32,
    pub unit_luck: u32,
    pub unit_class_id: u32,
    pub unit_class_index: u32,
    pub unit_side: u32,
    pub unit_poison: u32,
    pub unit_delusion: u32,
    pub unit_djinn_held: u32,
    pub unit_djinn_held_counts: u32,
    pub unit_djinn_active: u32,
    pub unit_djinn_active_counts: u32,
    /// struct BattleState: the party's and the enemies' unit lists, the
    /// background's colours and id, and the playback's phase.
    pub battle_party_units: u32,
    pub battle_enemy_ids: u32,
    pub battle_palette: u32,
    pub battle_background: u32,
    pub battle_event_phase: u32,
    /// struct BattleObject: its sprite, the sprite over it, its palette.
    pub obj_resource: u32,
    pub obj_overlay: u32,
    pub obj_palette: u32,
    /// The heap slots of the battle's state and of the enemies' units, as
    /// offsets into gHeapSlots.
    pub heap_battle: u32,
    pub heap_enemy_units: u32,
    /// Whether the debug chooser loads a preset before it waits for A (The
    /// Broken Seal's), which lets Down go; The Lost Age's loads the one A
    /// picks. A property of the game, not of what its source calls the
    /// routine: The Lost Age's took The Broken Seal's name.
    pub preset_first: bool,
}

/// Each BattleLayout value as each game's C expression.
const BATTLE_EXPRESSIONS: &[(&str, &str, &str)] = &[
    ("state_active_owners", "offsetof(struct MainRoutineState, party)", "offsetof(struct MainRoutineState, party)"),
    ("state_battle_type", "offsetof(struct MainRoutineState, battleType)", "offsetof(struct MainRoutineState, battleType)"),
    ("state_battle_background", "offsetof(struct MainRoutineState, battleBackground)", "offsetof(struct MainRoutineState, battleBackground)"),
    // Djinn_GetRecoveryTable returns the main routine's state from summonMask
    // on; The Lost Age's list is its entries after 8 bytes.
    ("recovery_list", "0x0c", "0x0c + offsetof(struct DjinnRecoveryTable, list.entries)"),
    ("recovery_count", "0x100", "offsetof(struct DjinnRecoveryTable, list.count) - offsetof(struct DjinnRecoveryTable, list.entries)"),
    ("unit_size", "sizeof(struct Unit)", "sizeof(struct Unit)"),
    ("unit_base_hp", "offsetof(struct Unit, baseHp)", "offsetof(struct Unit, baseHp)"),
    ("unit_base_pp", "offsetof(struct Unit, basePp)", "offsetof(struct Unit, basePp)"),
    ("unit_hp_gauge", "offsetof(struct Unit, hpGauge)", "offsetof(struct Unit, hpGauge)"),
    ("unit_base_luck", "offsetof(struct Unit, baseLuck)", "offsetof(struct Unit, baseLuck)"),
    ("unit_max_hp", "offsetof(struct Unit, hpMax)", "offsetof(struct Unit, hpMax)"),
    ("unit_max_pp", "offsetof(struct Unit, ppMax)", "offsetof(struct Unit, ppMax)"),
    ("unit_hp", "offsetof(struct Unit, hp)", "offsetof(struct Unit, hp)"),
    ("unit_pp", "offsetof(struct Unit, pp)", "offsetof(struct Unit, pp)"),
    ("unit_agility", "offsetof(struct Unit, agility)", "offsetof(struct Unit, agility)"),
    ("unit_luck", "offsetof(struct Unit, luck)", "offsetof(struct Unit, luck)"),
    ("unit_class_id", "offsetof(struct Unit, id)", "offsetof(struct Unit, id)"),
    ("unit_class_index", "offsetof(struct Unit, classId)", "offsetof(struct Unit, classId)"),
    ("unit_side", "offsetof(struct Unit, side)", "offsetof(struct Unit, side)"),
    ("unit_poison", "offsetof(struct Unit, poison)", "offsetof(struct Unit, poison)"),
    ("unit_delusion", "offsetof(struct Unit, delusion)", "offsetof(struct Unit, delusion)"),
    ("unit_djinn_held", "offsetof(struct Unit, djinnHeld)", "offsetof(struct Unit, djinnHeld)"),
    ("unit_djinn_held_counts", "offsetof(struct Unit, djinnHeldCounts)", "offsetof(struct Unit, djinnHeldCounts)"),
    ("unit_djinn_active", "offsetof(struct Unit, djinnSet)", "offsetof(struct Unit, djinnSet)"),
    ("unit_djinn_active_counts", "offsetof(struct Unit, djinnSetCounts)", "offsetof(struct Unit, djinnSetCounts)"),
    ("battle_party_units", "offsetof(struct BattleState, partyUnits)", "offsetof(struct BattleState, partyUnits)"),
    ("battle_enemy_ids", "offsetof(struct BattleState, enemyIds)", "offsetof(struct BattleState, enemyIds)"),
    ("battle_palette", "offsetof(struct BattleState, palette)", "offsetof(struct BattleState, palette)"),
    // The Lost Age's leaves the background's id unnamed, the first halfword
    // after the brightness as The Broken Seal's: background is the name
    // wanted.
    ("battle_background", "offsetof(struct BattleState, background)", "offsetof(struct BattleState, background)"),
    ("battle_event_phase", "offsetof(struct BattleState, playback.phase)", "offsetof(struct BattleState, events.phase)"),
    ("obj_resource", "offsetof(struct BattleObject, resource)", "offsetof(struct BattleObject, resource)"),
    ("obj_overlay", "offsetof(struct BattleObject, overlay)", "offsetof(struct BattleObject, overlay)"),
    ("obj_palette", "offsetof(struct BattleObject, palette)", "offsetof(struct BattleObject, palette)"),
    ("heap_battle", "HEAP_SLOT_BATTLE * 4", "offsetof(struct HeapSlots, battleState)"),
    ("heap_enemy_units", "HEAP_SLOT_ENEMY_UNITS * 4", "offsetof(struct HeapSlots, enemyUnits)"),
];

fn battle_headers(game: Game) -> &'static [&'static str] {
    match game {
        Game::Tbs => &[
            "types.h",
            "system/main_routine_state.h",
            "battle/battle_unit.h",
            "battle/battle_state.h",
            "battle/battle_action.h",
            "battle/battle_side.h",
            "system/heap.h",
        ],
        Game::Tla => &[
            "types.h",
            "stddef.h",
            "system/main_routine_state.h",
            "battle/battle_unit.h",
            "battle/battle_state.h",
            "battle/battle_action.h",
            "battle/battle_side.h",
            "system/ram_buffer.h",
        ],
    }
}

impl BattleLayout {
    /// The Broken Seal's, as written since the renderer began.
    pub fn tbs() -> BattleLayout {
        BattleLayout {
            state_active_owners: 0x1f8,
            state_battle_type: 0x22b,
            state_battle_background: 0x1d6,
            recovery_list: 0x0c,
            recovery_count: 0x100,
            unit_size: 0x14c,
            unit_base_hp: 0x10,
            unit_base_pp: 0x12,
            unit_hp_gauge: 0x14,
            unit_base_luck: 0x1e,
            unit_max_hp: 0x34,
            unit_max_pp: 0x36,
            unit_hp: 0x38,
            unit_pp: 0x3a,
            unit_agility: 0x40,
            unit_luck: 0x42,
            unit_class_id: 0x128,
            unit_class_index: 0x129,
            unit_side: 0x12a,
            unit_poison: 0x131,
            unit_delusion: 0x138,
            unit_djinn_held: 0xf8,
            unit_djinn_held_counts: 0x118,
            unit_djinn_active: 0x108,
            unit_djinn_active_counts: 0x11c,
            battle_party_units: 0x58,
            battle_enemy_ids: 0x66,
            battle_palette: 0x544,
            battle_background: 0x648,
            battle_event_phase: 0x800,
            obj_resource: 0x04,
            obj_overlay: 0x06,
            obj_palette: 0x14,
            heap_battle: 0x24,
            heap_enemy_units: 0xd8,
            preset_first: true,
        }
    }

    /// The edition's game's: The Broken Seal's as written, The Lost Age's
    /// read from its headers.
    pub fn for_edition(edition: &str) -> Result<BattleLayout, String> {
        match Game::of(edition)? {
            Game::Tbs => Ok(BattleLayout::tbs()),
            Game::Tla => BattleLayout::read(edition),
        }
    }

    pub fn read(edition: &str) -> Result<BattleLayout, String> {
        let game = Game::of(edition)?;
        let expressions: Vec<&str> = BATTLE_EXPRESSIONS
            .iter()
            .map(|(_, tbs, tla)| if game == Game::Tbs { *tbs } else { *tla })
            .collect();
        let values = probe(edition, battle_headers(game), &expressions)?;
        let get = |name: &str| -> u32 {
            let i = BATTLE_EXPRESSIONS
                .iter()
                .position(|(n, ..)| *n == name)
                .expect("a battle layout value's name");
            values[i] as u32
        };
        Ok(BattleLayout {
            state_active_owners: get("state_active_owners"),
            state_battle_type: get("state_battle_type"),
            state_battle_background: get("state_battle_background"),
            recovery_list: get("recovery_list"),
            recovery_count: get("recovery_count"),
            unit_size: get("unit_size"),
            unit_base_hp: get("unit_base_hp"),
            unit_base_pp: get("unit_base_pp"),
            unit_hp_gauge: get("unit_hp_gauge"),
            unit_base_luck: get("unit_base_luck"),
            unit_max_hp: get("unit_max_hp"),
            unit_max_pp: get("unit_max_pp"),
            unit_hp: get("unit_hp"),
            unit_pp: get("unit_pp"),
            unit_agility: get("unit_agility"),
            unit_luck: get("unit_luck"),
            unit_class_id: get("unit_class_id"),
            unit_class_index: get("unit_class_index"),
            unit_side: get("unit_side"),
            unit_poison: get("unit_poison"),
            unit_delusion: get("unit_delusion"),
            unit_djinn_held: get("unit_djinn_held"),
            unit_djinn_held_counts: get("unit_djinn_held_counts"),
            unit_djinn_active: get("unit_djinn_active"),
            unit_djinn_active_counts: get("unit_djinn_active_counts"),
            battle_party_units: get("battle_party_units"),
            battle_enemy_ids: get("battle_enemy_ids"),
            battle_palette: get("battle_palette"),
            battle_background: get("battle_background"),
            battle_event_phase: get("battle_event_phase"),
            obj_resource: get("obj_resource"),
            obj_overlay: get("obj_overlay"),
            obj_palette: get("obj_palette"),
            heap_battle: get("heap_battle"),
            heap_enemy_units: get("heap_enemy_units"),
            preset_first: game == Game::Tbs,
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
        assert_eq!(BattleLayout::read("tbs-en").unwrap(), BattleLayout::tbs());
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
        BattleLayout::read("tla-en").unwrap();
    }
}
