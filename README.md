# 🌋 Alchemy Devtools

Reconstruction tools for [Alchemy](https://github.com/PascalPixel/alchemy): compare C with the game's machine code, search for matching forms, inspect assets, and measure code and data progress.

Alchemy builds and mods use its own C tools. Alchemy Devtools is an optional, pinned submodule for people working on the decompilation.

## Build and use

Install Rust and Cargo. From an Alchemy checkout:

```sh
git submodule update --init alchemy-devtools
make devtools
alchemy-devtools/target/release/alchemy-devtools --help
```

Run commands inside that Alchemy checkout. Its matching compilers must be bootstrapped for compilation and permutation, and progress measurement needs the built editions. See Alchemy's `AGENTS.md` for its source constraints and reconstruction rules.

```sh
alchemy-devtools/target/release/alchemy-devtools diff DRAFT.c --show
alchemy-devtools/target/release/alchemy-devtools permute DRAFT.c --minutes 10
make progress
make test-devtools
```

For a standalone checkout:

```sh
cargo build --locked --release
ALCHEMY_ROOT=/path/to/alchemy target/release/alchemy-devtools --help
ALCHEMY_ROOT=/path/to/alchemy cargo test --locked --workspace
```

The tests use Alchemy's bootstrapped compilers and ARM binutils. Current binutils are needed: older releases print some ARM instructions differently. CI uses Rust 1.98.1.

## Pictures

`render psynergy` plays battle Psynergy in a built ☀️ edition and writes each cast as a looping GIF of the whole screen at 30 frames a second, with an `index.tsv` of ability, name, caster, target and frames. It runs the ROM in [libmgba](https://mgba.io), linked through a small C shim, so it is behind the `render` feature: install mGBA's library and headers (Homebrew: `brew install mgba`; `MGBA_PREFIX` names another prefix), then

```sh
cargo build --locked --release --features render --manifest-path alchemy-devtools/Cargo.toml
make tbs-en
alchemy-devtools/target/release/alchemy-devtools render psynergy --all --list
alchemy-devtools/target/release/alchemy-devtools render psynergy --all --out DIR
alchemy-devtools/target/release/alchemy-devtools render psynergy --ability Flare --backdrop backdrop.png --verbose
```

The run boots the build, enters Camelot's battle test menu and steers the battle at function entries named by the linker map. `--backdrop` picks the battlefield: a grey gradient drawn as the GBA's backdrop colour (the default), the battle's own background (`game`), or a 256 by 120 PNG, a battle backdrop's size, put in the background's place in its own format, which the game then draws, scrolls, darkens and blends itself. Windows are drawn grey. Recording starts at the cast's message, after the camera's turn from the command view. `render psynergy --help` gives the casters, targets and setup. The ROM is only read; never publish it or anything copied from it.

`render place` and `render event` start a new game straight in each field scene (the scene, entrance and story flags are written where the new game's defaults are set) and write PNG files at the GBA's size: each scene's main entrance photo, a map of each room (one camera box of its entrances, stitched from one moment with the leader hidden), the same again at each moment of the scene's story that changes who or what a room shows, and the first message pages of each talk, search, touch and Psynergy-on-actor event with its required flags, the leader placed beside the trigger. They read the story tools' `places.tsv`, `events.tsv` and `flags.tsv`, passed by path:

```sh
alchemy-devtools/target/release/alchemy-devtools render place --all --places places.tsv --events events.tsv --flags flags.tsv --out DIR
alchemy-devtools/target/release/alchemy-devtools render event --scene Bilibin1 --places places.tsv --events events.tsv --flags flags.tsv --out DIR
```

An event that moves also gets one GIF of how it starts, played where and at the age the story has it. `render story --flags flags.tsv` makes the front page's pictures: the title, Vale's farewell, the world map cut where Venus Lighthouse lights, and the world map laid flat. `render psynergy --djinn --summons` casts the Djinn and summons.

A door or any other event that leaves its scene is recorded on into the scene it leads to, so its GIF doesn't stop on a black screen: the fade out, the load, the new scene's arrival with its lines answered and its fade in, until the player has control there, tested the same way as at a scene's start, and then half a second more. A blank screen (one colour all over, as during the fade's black and the load) is cut to its first 8 frames each time it appears. The arrival is followed for at most 10 seconds from the load, so a long arrival cutscene (Kraden showing the party into Sol Sanctum) ends at that point, and so does one that begins a battle; `index.tsv`'s `note` column says which. Its `leads_to` column names the scene the event placed the party in next by its SceneId symbol (`SceneId_WorldMap`), the event's own scene for a staircase to another of its entrances, so a caption can say where a door leads.

### Which party plays each scene?

A new game's defaults are Isaac alone at level 1, so every scene would otherwise show him, a child's sprite and 40 HP, wherever he is. `render place`, `render event` and `render guide` instead give each scene the party the player has there, set before the scene loads (`src/render/party.rs`). The rule comes from Camelot's own evidence: the debug party presets (`DebugParty_LoadPreset`, `games/tbs/src/battle/presets.tsv`), whose rows 10 to 70 follow the story's party. They run from Isaac, Garet and Jenna at levels 1 and 2 (Sol Sanctum), Isaac and Garet at 3, Ivan joining at 4 and 5, three members from 6 to 9, Mia joining at 10, four from 11 to 17, Isaac alone at 18 and 19 (the Colosso) and four again from 20 to 33, each with the Djinn, Psynergy and equipment the preset gives. Each stretch of scenes, in story order by scene number, names its members as they joined and the preset whose level fits where it falls between those joins:

| Scenes | Members | Preset (level) |
| --- | --- | --- |
| the stormy night | Isaac, Garet as children | a new game's level 1 |
| Vale | Isaac, Garet | 10 (1) |
| Sol Sanctum, Mt. Aleph | Isaac, Garet, Jenna | 11 (2) |
| the world map, Vault | Isaac, Garet | 12 (3) |
| Goma Cave, Vault revisited | Isaac, Garet, Ivan | 14 (5) |
| Bilibin and its barricade | Isaac, Garet, Ivan | 20 (6) |
| Kolima, its forest, Tret, the caves by Vale and Vault | Isaac, Garet, Ivan | 21 (7) |
| Bilibin Cave, Imil | Isaac, Garet, Ivan | 23 (9) |
| Mercury Lighthouse | the four | 25 (10) |
| Fuchin Temple | the four | 30 (11) |
| Mogall Forest, Xian, Alpine Crossing | the four | 32 (12) |
| Altin, Altin Peak | the four | 34 (14) |
| Lama Temple, Lamakan Desert | the four | 40 (15) |
| Kalay, its docks, Hammet's palace | the four | 42 (16) |
| the Tolbi-bound Ship, Tolbi Docks | the four | 43 (17) |
| Tolbi, Babi's palace | the four | 51 (18) |
| the Colosso | Isaac | 53 (19) |
| Altmiller Cave, Gondowan Passage | the four | 60 (20) |
| Kalay Tunnel, Gondowan Cave | the four | 61 (20) |
| Lunpa, its fortress, West Lunpa Cave | the four | 63 (22) |
| the Suhalla Desert to the Tunnel Ruins | the four | 64 (23) |
| Babi Lighthouse, Lalivero, the Lemurian Ship | the four | 65 (24) |
| Crossbone Isle | the four | 67 (26) |
| Venus Lighthouse | the four | 69 (29) |

A preset's member the stretch doesn't name is left out, every member's HP and PP are restored (`Party_RestoreHpPp`), and then the run's own flags decide the party flags an event needs (`FLAG_IN_PARTY`: Ivan before or after he joins in Vault, Garet before he joins on the stormy night), so the party is listed again from those flags, the stretch's members first. Child sprites come only from FLAG_CHILDHOOD, which only the stormy night sets. The table is an estimate placed on Camelot's presets; an area's encounter levels (`encounter.tsv`, `enemies.tsv`) are the next evidence to check it against.

A battle an event starts is fought by A (Fight, then each member's first command) with no random battle breaking in (FLAG_NO_ENCOUNTERS). If the party loses it, which no scene of the game scripts, or the battle runs past the recording's three minutes (a boss fought by A alone), the GIF ends as the battle begins, its first five seconds kept, and `index.tsv`'s `note` says so: no recording shows the party wiped out.

A scene's story moments come from the named story flags its initialization tests. Each moment sets one more of them, in the order the events' own conditions give (a flag an event needs set comes first; one it needs clear comes after, unless some event clears it again, like the stormy night's FLAG_VALE_STORM), so each is a state the player could see: Vale on the stormy night goes from the fallen rocks to Garet joining, the Boulder falling, Felix in the river with his parents at the bank, the gathering at the river and Garet leaving. A period some event ends is cleared once a later flag shows it over (the storm, once the party has left Vale), and the prologue's FLAG_CHILDHOOD follows FLAG_VALE_STORM. A moment draws only the rooms whose placed actors (place, map layer and sprites) or map cells differ from every state drawn before, and a moment that changes nothing is dropped. It goes in `DIR/<scene>_<moment>/`, named by its flag without `FLAG_`, lowercase with hyphens (`vale-storm_vale-storm-saw-river-rescue/map-0.png`); a scene that tests FLAG_KOLIMA_CURSE_LIFTED is drawn cursed at the new game's state (`<scene>_cursed/`) and cured at that moment (`<scene>_cured/`). A scene draws at most 12 moments, and skipped.tsv names those left past the cap.

`DIR/index.tsv` lists every image, a moment's with its name and flags in the variant column (`vale-storm-garet-left: FLAG_..., FLAG_...; clear ...`); `DIR/skipped.tsv` what was left out and why, and `render place` writes `DIR/variant-flags.tsv`, the story flags each scene's initialization tests.

`render psynergy`, `render place`, `render event` and `render guide` take `--jobs N` (by default the number of CPUs) and run N processes of the same program, each with its own libmgba. The command keeps a queue of the work's pieces, the costliest first, and hands the next one to whichever process finishes: a cast, a scene, one event, or one of the guide's modes (`field-psynergy`'s casts one by one). A static split doesn't work for events, because a few overlays (Crossbone Isle's, Altin Peak's, the Colosseum's) hold far more than the rest, so each event is its own piece. It's still played against all of its overlay's scenes, as a single process would, since an event plays at every scene its row lists and a scene's overlay brings its sibling scenes along; a process keeps each overlay's scans for the next of its events it's handed. An event's cost is estimated from its triggers, the scenes it may play at and the frames its pictures took in the `index.tsv` an earlier run left, and a scene's from its story moments. The command clears the scenes' old event pictures first, then joins what the processes recorded into one `index.tsv`, `skipped.tsv` and `variant-flags.tsv`, which come out the same as `--jobs 1` writes them in one process; only the timing lines on standard error differ.

### What the pictures work around

Some things in the pictures are the game's own doing, and some had to be worked around. Each fix lives in the code named here; this list says why it's there.

- **A child's battle sprite comes from the Machete, not from a flag.** `BattleObjMake` draws Isaac and Garet with sprites 480 and 482 (`SPRITE_ISAAC_MACHETE`, `SPRITE_GARET_MACHETE`) instead of 300 and 301 while they hold the Machete, and `Party_EndChildhood` swaps it for the Short Sword when the prologue ends. In the field the child sprites come from `FLAG_CHILDHOOD`. So `render psynergy` casts with debug preset 0 (`PRESET_FOUR` in `plan.rs`), the four at level 35, and a run that starts on the stormy night sets `FLAG_CHILDHOOD` with `FLAG_VALE_STORM` (`places.rs`), or Isaac stays grown while the scene places Garet as a child. XXX: preset 10, the only preset that equips the Machete (its level-1 rows for Isaac and Garet), is the one the party table gives Vale after the prologue, so a battle there would show the child sprites; no recording yet shows one.
- **A new game's party is Isaac alone at level 1 with 40 HP**, who loses Altin Peak's first water beast fight. That's why each scene gets the story's party (above).
- **The battle windows are grey and the backdrop is ours.** No hue on the game's window-colour wheel is grey, so the shim greys the window palette entries per scanline (`WINDOW_ENTRIES` and `greys` in `battle.rs`, `alchemy_mgba_recolour` in `mgba.c`) and leaves palette RAM alone. A white backdrop made additive fire vanish and a key colour bled into Flare; the backdrop is now a grey gradient or a 256 by 120 PNG put where `BattleBackground_Load` writes (`backdrop.rs`), which the game draws, darkens and blends itself. Fed `suhalla_gate.png`, it gives the game's own picture byte for byte; `alchemy-docs/render/backdrop.png` is the one the docs use.
- **Effects stop 16 lines below the top.** During a cast the game's `WIN0` and `WIN1` cover lines 16 to 135 only; the recordings keep that.
- **An enemy-side character keeps the party's colours.** In a link battle (Down held in the battle test menu) `BattleObjMake` gives the enemy side palette 1, the opponent's colours, so `battle.rs` sets `BattleObj.palette` back to 0.
- **Hiding a background in libmgba crashes it in a battle.** `enableVideoLayer` for `BG1` crashed in a Goma Cave battle; the take-apart pictures (`guide::layer_sheets`) choose each layer through `DISPCNT`, as `layers-3d` does, and fail if the game rewrites it. In mode 2 (the world map) only `BG2` and `BG3` exist, and `guide::stacking` counts only the mode's backgrounds.
- **Maps start from one saved state.** Wandering actors made seams, so their task (`Object_UpdateAllThumb`) is frozen and every view starts from the same state; each view is the third frame after the scroll, since the first two still show the old screen; and the followed point is pinned, or it drifts back to the leader (`field.rs`). A default entrance gives no camera box, so the header's wider bounds would show the cell grid past the room; no room is drawn or played from one (`field::stands_in`).
- **The location name and the heat gauge are kept out of maps**: `UiTimedNotice_Create` returns at once, `BG0` is hidden while a map is stitched, and `FLAG_FIELD_HANDLING_REQUESTS` (`FLAG_CONTACT_BLOCKED` until 21 October) slides the gauge away.
- **File names:** the Djinni Mist is `mist-djinni.gif`, so it doesn't overwrite the Psynergy Mist's GIF.
- **Viewing the GIFs:** some image viewers draw these GIFs with a magenta mask; browsers show them as written.

The story tables `render place`, `render event`, `render story` and `render guide` read (`places.tsv`, `events.tsv`, `flags.tsv`, `messages.tsv`) are written by `alchemy-docs`' `bun scripts/story.ts` into its `build/story/`, from the same Alchemy checkout; the renderer reads that `places.tsv` as it is (one area name a scene, `WorldMap` the only perspective map), or a richer one with `sceneNumber`, `sceneSymbol`, `overlayFile`, `areaNames` and `map` columns; make them again after renames in the source, since their columns follow its names. Two runs that build this crate at once should use separate `CARGO_TARGET_DIR`s.

Still open: Lamakan Desert's maps show faint seams from its heat shimmer; BabiPalace2 and TolbiSpring2 fail because their map scroll update never runs; rooms whose state starts a cutscene that leaves before the player has control (the world map's entrance 69, the Colosseum, Vale Gate once Garet has left) are skipped; and where a scene's events give few conditions, the order of its story moments is a guess.

## Progress publication

Stage source changes before `make land` or `make progress`. Measurement checks that the working source matches the index, records its tree and both report digests, and preserves the original measurement time when the same tree is measured again. Alchemy's pre-push hook uploads the reports and receipt for the pushed main revision.

After the public build passes, CI uses `progress --publish ARCHIVE COMMIT DIRECTORY` to verify the receipt and reconstruct history from its immutable baseline and accepted ancestor measurements. It writes the two figures, history, source revision and accepted receipt. Dates come from the measurement, in UTC, so retrying publication does not move a measurement to today. Alchemy's `.github/README.md` describes the release archive and separate docs deployment.

## Humanization report

Matching made the C byte-identical; humanizing makes it readable (Alchemy's S3), and `progress --humanity` gives that work numbers that should fall the way DONE rose. It reads only the source, so it needs no built edition:

```sh
alchemy-devtools/target/release/alchemy-devtools progress --humanity
alchemy-devtools/target/release/alchemy-devtools progress --humanity --top 10
alchemy-devtools/target/release/alchemy-devtools progress --humanity --list undocumented_function
```

It counts the C and headers git tracks under each game's `src` and `include`, so nothing the build generates, and a header written in assembly (`ed_asm.h`) is skipped. The Broken Seal and The Lost Age are counted separately. The table goes to the terminal; `out/reports/humanity.tsv` has the columns `game`, `metric`, `scope` and `count`, with a `total` row for every metric and then the ten files leading each count (for `long_file` the count is the file's lines; for the duplicate groups the scope lists every copy and the count is the body's length in tokens). `--top N` prints those lists too, and `--list METRIC` prints every counted thing of one metric with its file and line, which is the worklist for a humanizing pass.

The counts come from a small lexer, never the preprocessor, so the rules are deliberately simple:

- **Magic numbers** (`magic_code`) are hex literals from `0x10` and decimal literals from 10 inside function bodies and function-like macros. Comments and strings never count. Numbers that are data by nature don't count as code: initializer lists (`= { ... }`, counted apart as `magic_table`), enum values, object-like `#define` bodies (which name their number), struct and union bodies (field sizes and bit widths), array sizes and anything else outside functions, and sizes written as `(w * h)` from two literals. `magic_argument` is the part of `magic_code` that stands alone as an argument or list element and fills a parameter that may want a name (a speed, a facing, a mode, a value).
- **Numbers passed as data** (`data_argument`) are lone arguments that fill a parameter whose name says it holds data, which S3 lets stay a number and Alchemy's H5 forbids naming: `data_argument_coordinate` for a word `x`, `y`, `z`, `dx`, `dy`, `dz`, `row` or `column` (`srcX`, `fixed_z`, `xOffset`, `sourceRow`), `data_argument_frames` for `frames`, `duration`, `delay` or `ticks`, `data_argument_size` for `width`, `height`, `w`, `h`, `size`, `length`, `radius` or `distance`, and `data_argument_count` for `count`; a name with `velocity`, `speed`, `step`, `freq`, `frequency` or `acceleration` in it is a rate and stays in `magic_argument`. The parameter is read from the callee's declarations anywhere in the game (prototypes, definitions and `static inline` helpers, whichever names the most parameters). A function-like macro's parameter takes its own name's kind, else that of the parameter its body passes it to (`WAIT(n)` is `Engine_EventWait(s32 frames)`), and a macro that only converts its value (`PIXELS(value)`, `((value) << 16)`) takes the kind of the parameter its result fills, so `Actor_SetPosition(ACTOR_GUARD, PIXELS(312), PIXELS(416))` is two coordinates and `Actor_SetSpeed(ACTOR_GUARD, PIXELS(2), 0x8000)` stays two magic numbers. A call through a pointer (`draw[0](canvas, 64, 40)`) or to a function declared without parameter names can't be read and stays in `magic_argument`; so a parameter's name decides its arguments, and a wrong one is fixed in its declaration, never here. `--list magic_argument` and `--list data_argument` give each number followed by its callee and parameter (`WAIT n`; `Old #2` for an unnamed one, `-` for no callee).
- **Placeholder names** (`placeholder`, with the distinct names as `placeholder_names`) start with `unk`, `Unk`, `gUnk`, `sUnk` or `unknown`, contain `Unk` as a word (`Battle_UnkFlags`), `_unk` or `_UNK`, are an address name (`sub_`, `func_`, `data_`, `loc_`, `off_`, `byte_`, `word_`, `dword_`, `D_` or `nullsub` with hex) or `field_` with hex, carry a ROM or RAM address (`_08012345`), or are an unproven sound (`SE_138`, `MUS_017`). Every use outside comments and strings counts, field names included.
- **Numbered names** (`numbered`, `numbered_names`) end in a run of digits that stands for a meaning not yet found: `ColosseumFinal1`, `Event_LeaveByExit10`, `ACTOR_8`, `arg0`. A name doesn't count when it is a placeholder; a register or convention (`REG_` names, `FLAG_SCENE_TEMP_xx` and `FLAG_PLACE_TEMP_xx`, names starting `__`); a one-letter stem (`x1`, `r0`); a width after a word with no underscore (`Random16`, `Dma_Fill32`, `vu16`); or when the word before the digits is hardware or convention: `BG`, `DMA`, `TM`, `Timer`, `Win`, `OBJ`, `OAM`, `TGT`, `MODE`, `FIFO`, `Sound`, `SND`, `CH`, `Key`, `SIO`, `Joy`, `IRQ`, `attr`, `PATCH`, `Tan`, `Vec`, `Vector`, `LZ`, `UTF`, `CRC`, `SHA`, `Base`, `RGB` and `VRAM`.
- **Devices** are the `FAKEMATCH(<kind>)` tags of S2's seven kinds, counted from comments (`fakematch_wrapper` and so on; `fakematch` sums them), and `goto` counts every `goto` statement. In The Broken Seal both may only fall.
- **Edition tests** count each `#if`, `#elif`, `#ifdef` and `#ifndef` that tests `LANGUAGE` (`if_language`), a patch level such as `TBS_PATCH_1` (`if_patch`), or an edition name such as `TBS_EDITION_EN` (`if_edition_name`, which S3 keeps out of C; `edition.h` and `version.h`, where edition names become those values, are left out). `if_in_function` counts those tests written inside a function body, where S3 wants only code whose shape differs.
- **Far-call entries without `///`** (`undocumented_entry`) are the functions a far-call header (`include/farcall/`) declares, the entry points other banks reach, with no documentation at any declaration: D9 gives every entry point its one-sentence summary. The other undocumented declarations are a list to review, not work to fill, and the table prints them under their own heading: D9 deletes a comment a reader could rebuild from the name and gives a declaration that needs none no comment, which no count can tell apart.
- **Undocumented declarations** have no `///` (or `/**`) comment right before them and no `///<` note after them on the same line. A function or global counts once, at its definition if it has one, and is documented when any of its declarations is, so a prototype documented in its header covers the definition. Structs and enums count at each definition; an anonymous enum is documented by a note on its first constant. Macros count once per file and name, leaving out header guards (names ending `_H`).
- **Long objects** (`long_file`) are C objects over 3,000 lines that define a function, S3's size for writing an object holding more than one family as parts. A header, a scene overlay (`src/scene/`, laid out by kind and kept one file), a part (a `// Part of` line, one family already) and a data object (no function) are never counted.
- **Units** (`units`, `units_done`, `unit_open`) are what Alchemy's H4 judges: each place's overlay (`src/scene/<place>.c`) and each folder guide (`src/<folder>/README.md`). A unit's open questions are the `- XXX:` and `- XXX (picture):` items of its overlay's `/// Open questions:` list or its guide's `## Evidence and open questions` section; it is done when it has the list and none of those items, `- XXX (no evidence):` ones aside. `--list unit_open` is the worklist.
- **Duplicates** group function bodies of at least 24 tokens that are identical once every identifier and string is replaced by a placeholder, and table initializers of at least 16 tokens that are identical token for token (names kept, so tables only group when they hold the same data). `duplicate_function` and `duplicate_table` count the groups, and the `_copies` rows the copies past the first in each.

`#if` branches are each read, and the brace depth is taken from the first branch at `#endif`, so a declaration that opens differently by edition doesn't upset the rest of the file. None of these figures counts toward DONE.

No figure here says a unit is humanized (H4): that is a judgement about one system or place, and the source doesn't yet record it in a form a count can read. The folder READMEs' closing sections hold the open questions, but some write them as `XXX:` items and some in prose, and nothing marks a question no evidence can settle apart from one that evidence could.

### Where names come from

S3 wants every name from evidence: a place by its area name, a person or item as the game's text names them, and an unnamed room by what the player sees there. Earlier writers also made names up, and nothing in the source told the two apart, so the report checks each name of a game thing against the game's English text, `games/<game>/text/en.po` (as git tracks it; a game without one isn't checked).

- **Game things** are the definitions whose prefix says what they name: scene ids (`SceneId_`), `ACTOR_`, `FLAG_` (not the temporary flags), `ITEM_`, `ABILITY_`, `PSYNERGY_`, `DJINNI_`, `ENEMY_` and `MONSTER_`, `SUMMON_`, `CHARACTER_`, `CLASS_`, `PERSON_` and `AREA_` (a scene's places), as enumerators, macros or declarations, and every enumerator of `scene_links.h`. Each counts once per file where it is defined (a scene id once for the game). Placeholders are left to their own count. The functions and globals defined under `src/scene` and `src/object` are checked for their words too, but they name what the code does, so they need no evidence.
- **Words the game never uses** (`unused_words`, with `unused_word_name` for the names using them): a name is split into words at underscores, at a capital after a small letter, before the last capital of a run followed by a small letter, and between letters and digits (`SceneId_SolSanctumLunaSolRooms` is Scene, Id, Sol, Sanctum, Luna, Sol, Rooms); digits, the hex letters right after them (`Ov6aa`) and single letters are dropped, and two neighbours the text writes as one word are joined (`McCoy`). A word is accounted for when some message uses it, compared lowercase after a crude stemming both sides share (`'s`, plurals, `-ing`, `-ed` and a final `e` come off, so `moving` meets `move`), or when it is on the short programming vocabulary in `src/progress/provenance.rs` (`Get`, `Update`, `Table`, `Handler`, the kind prefixes, ordinals and compass points, and the engine's drawing and motion terms such as `Transition` and `Blend`). Everything else is flagged; the vocabulary errs short, so a flagged word is a question for a reviewer, not a verdict.
- **Evidence for a name** is read from its own `///` comment and `///<` notes. A name is verified (`provenance_verified`) by a line `★ Named from the game: "quote" (MsgSymbol)` whose quote that message holds (or one of the unnamed messages after it, for a table such as `MsgItemName`), `★ Named from its area name: Area` whose area is one of `gScene_AreaNameRules`' names (the messages from `MsgDebugEntryName`, as many as the table's highest index in `object/scene_rules.s` plus one), or `★ Named from what the player sees: description (its render)`. Quotes are compared with the text's `{...}` commands removed and its lines joined. The same line without its ★ counts as `provenance_unstarred`, and a line that doesn't check out (a quote the message doesn't hold, an unknown symbol, a name that isn't an area name) as `provenance_failed`. Short of a line, `provenance_prose` counts a name whose comment, its enum's comment, or a paragraph of a scene overlay's opening comment that mentions it (a scene id by its bare name) quotes the game (`"..."` or `` `...` `` the text holds), cites a message symbol the text defines, cites the area names, or speaks of its render, a recording or what the player sees. The rest are `provenance_none`.
- **Names a table gives** need no ★ line. An enumerator of an enum whose values index one of the game's name tables is checked against the name the English text gives its index: `ItemId` against `MsgItemName` + id, `AbilityId` against `MsgAbilityName` + id, `DjinniId` against `MsgDjinnName` + id, and `EnemyClassId` against `MsgEnemyName` + its row in `enemies.tsv`, which is its class id less `ENEMY_CLASS_FIRST` (`TABLES` in `src/progress/provenance.rs`; the messages are counted by their `msgid`). The enum is found by its tag (`enum ItemId {`), each enumerator's value is read from its `=` (a number, an earlier enumerator or a numeric macro, joined by `+`, `-`, `|`, `*` and `<<`) or is one past the one before, and an enumerator whose value can't be read is left to the other checks. The enumerator less its prefix and the game's name are compared as lowercase words, with apostrophes dropped and anything else that isn't a letter or digit a space, so `ITEM_BANDITS_SWORD` is `Bandit's Sword` and `ITEM_HYPER_BOOTS` would be `Hyper-Boots`. A match is `provenance_table`; a later row sharing a name may add its number (`ENEMY_MIMIC_8` is a `Mimic`), and the worklist says `numbered`. A name that differs is `provenance_table_mismatch`, listed with the game's name, since the enumerator should probably be renamed to it. The table check comes before any ★ line, because it's the stronger evidence.
- **Door names** of `scene_links.h` need no ★ line when their words check out. A door's name is split into words as above and read from the left, taking at each place the longest run of words that spells a scene id (`TOLBI_SHIP` is `SceneId_TolbiShip`); each scene it names must be verified by its own ★ line. Every other word must be a door word (`TO`, `FROM`, the eight compass points, `UNUSED`, `DEBUG`, `BATTLE`, `AFTER` and `BEFORE`), a room word of one of the scenes it names, or, once `AFTER` or `BEFORE` has come, a word the game's text uses or a word of a ★-verified name that isn't a scene or a door (a flag, say). A scene's room words are the words of its ★ line and the leftover words of the ★-verified doors that name it, so once `TOLBI_TO_INN` quotes the game, `TOLBI_FROM_INN` follows from it; words are compared after the stemming above. A door that passes is `provenance_rooms`. One that doesn't keeps the evidence its comments give, and its unresolved words (an unverified scene by its id) are listed with it: `door_words` counts the distinct ones and `door_word_link` the doors using them. A door whose ★ line fails stays a failure.

`--list provenance` is the worklist: one line for each game thing and each mechanism with a flagged word, with the game, the name, its kind, `file:line`, its flagged words (`-` for none) and its evidence (`verified quote`, `table MsgItemName+5`, `mismatch: MsgAbilityName+300 is "Flint"`, `rooms`, `unstarred area`, `failed: ...`, `prose quote+message`, `none`, or `-` for a mechanism), followed for a door by `; door:` and its unresolved words. `--list unused_word` gives each flagged word with how many names use it and where, most used first, `--list door_word` does the same for the doors' unresolved words, and `--list provenance_none` (or any other of these metrics) lists one kind of result; `--list provenance_table_mismatch` gives each mismatch with the game's name after it. The TSV adds the ten most used flagged words as `unused_word` rows and the ten most used door words as `door_word` rows, and `--top N` prints both with the first name using each.

## Source

`src/` holds the reconstruction commands. `ags/` holds the asset readers and the earlier Rust encoders used for inspection and comparisons with Alchemy's C encoders. These are outside Alchemy's ROM build.

This repository starts with the tooling from Alchemy revision `2c2a8be6b258221ab067e3c7e22c29ff882ef8bc`. The permuter reimplements behavior from [Simon Lindholm's decomp-permuter](https://github.com/simonlindholm/decomp-permuter).
