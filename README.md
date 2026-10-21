# Alchemy Devtools

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

Moving events also get GIFs: the whole event when it is short, and a segment for each stretch between two of its messages, which the index ties to the messages before and after it. `render story --flags flags.tsv` makes the front page's pictures: the title, Vale's farewell, the world map cut where Venus Lighthouse lights, and the world map laid flat. `render psynergy --djinn --summons` casts the Djinn and summons.

A scene's story moments come from the named story flags its initialization tests. Each moment sets one more of them, in the order the events' own conditions give (a flag an event needs set comes first; one it needs clear comes after, unless some event clears it again, like the stormy night's FLAG_VALE_STORM), so each is a state the player could see: Vale on the stormy night goes from the fallen rocks to Garet joining, the Boulder falling, Felix in the river with his parents at the bank, the gathering at the river and Garet leaving. A period some event ends is cleared once a later flag shows it over (the storm, once the party has left Vale), and the prologue's FLAG_CHILDHOOD follows FLAG_VALE_STORM. A moment draws only the rooms whose placed actors (place, map layer and sprites) or map cells differ from every state drawn before, and a moment that changes nothing is dropped. It goes in `DIR/<scene>_<moment>/`, named by its flag without `FLAG_`, lowercase with hyphens (`vale-storm_vale-storm-saw-river-rescue/map-0.png`); a scene that tests FLAG_KOLIMA_CURSE_LIFTED is drawn cursed at the new game's state (`<scene>_cursed/`) and cured at that moment (`<scene>_cured/`). A scene draws at most 12 moments, and skipped.tsv names those left past the cap.

`DIR/index.tsv` lists every image, a moment's with its name and flags in the variant column (`vale-storm-garet-left: FLAG_..., FLAG_...; clear ...`); `DIR/skipped.tsv` what was left out and why, and `render place` writes `DIR/variant-flags.tsv`, the story flags each scene's initialization tests.

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

- **Magic numbers** (`magic_code`) are hex literals from `0x10` and decimal literals from 10 inside function bodies and function-like macros. Comments and strings never count. Numbers that are data by nature don't count as code: initializer lists (`= { ... }`, counted apart as `magic_table`), enum values, object-like `#define` bodies (which name their number), struct and union bodies (field sizes and bit widths), array sizes and anything else outside functions, and sizes written as `(w * h)` from two literals. `magic_argument` is the part of `magic_code` that stands alone as an argument or list element (`Actor_WalkTo(ACTOR_PARTY_LEADER, 257, 420)`), mostly script coordinates and waits.
- **Placeholder names** (`placeholder`, with the distinct names as `placeholder_names`) start with `unk`, `Unk`, `gUnk`, `sUnk` or `unknown`, contain `Unk` as a word (`Battle_UnkFlags`), `_unk` or `_UNK`, are an address name (`sub_`, `func_`, `data_`, `loc_`, `off_`, `byte_`, `word_`, `dword_`, `D_` or `nullsub` with hex) or `field_` with hex, carry a ROM or RAM address (`_08012345`), or are an unproven sound (`SE_138`, `MUS_017`). Every use outside comments and strings counts, field names included.
- **Numbered names** (`numbered`, `numbered_names`) end in a run of digits that stands for a meaning not yet found: `ColosseumFinal1`, `Event_LeaveByExit10`, `ACTOR_8`, `arg0`. A name doesn't count when it is a placeholder; a register or convention (`REG_` names, `FLAG_SCENE_TEMP_xx` and `FLAG_PLACE_TEMP_xx`, names starting `__`); a one-letter stem (`x1`, `r0`); a width after a word with no underscore (`Random16`, `Dma_Fill32`, `vu16`); or when the word before the digits is hardware or convention: `BG`, `DMA`, `TM`, `Timer`, `Win`, `OBJ`, `OAM`, `TGT`, `MODE`, `FIFO`, `Sound`, `SND`, `CH`, `Key`, `SIO`, `Joy`, `IRQ`, `attr`, `PATCH`, `Tan`, `Vec`, `Vector`, `LZ`, `UTF`, `CRC`, `SHA`, `Base`, `RGB` and `VRAM`.
- **Devices** are the `FAKEMATCH(<kind>)` tags of S2's seven kinds, counted from comments (`fakematch_wrapper` and so on; `fakematch` sums them), and `goto` counts every `goto` statement. In The Broken Seal both may only fall.
- **Edition tests** count each `#if`, `#elif`, `#ifdef` and `#ifndef` that tests `LANGUAGE` (`if_language`), a patch level such as `TBS_PATCH_1` (`if_patch`), or an edition name such as `TBS_EDITION_EN` (`if_edition_name`, which S3 keeps out of C; `edition.h` and `version.h`, where edition names become those values, are left out). `if_in_function` counts those tests written inside a function body, where S3 wants only code whose shape differs.
- **Undocumented declarations** have no `///` (or `/**`) comment right before them and no `///<` note after them on the same line. A function or global counts once, at its definition if it has one, and is documented when any of its declarations is, so a prototype documented in its header covers the definition. Structs and enums count at each definition; an anonymous enum is documented by a note on its first constant. Macros count once per file and name, leaving out header guards (names ending `_H`).
- **Long files** are those over 3,000 lines, S3's size for writing an object as parts.
- **Duplicates** group function bodies of at least 24 tokens that are identical once every identifier and string is replaced by a placeholder, and table initializers of at least 16 tokens that are identical token for token (names kept, so tables only group when they hold the same data). `duplicate_function` and `duplicate_table` count the groups, and the `_copies` rows the copies past the first in each.

`#if` branches are each read, and the brace depth is taken from the first branch at `#endif`, so a declaration that opens differently by edition doesn't upset the rest of the file. None of these figures counts toward DONE.

## Source

`src/` holds the reconstruction commands. `ags/` holds the asset readers and the earlier Rust encoders used for inspection and comparisons with Alchemy's C encoders. These are outside Alchemy's ROM build.

This repository starts with the tooling from Alchemy revision `2c2a8be6b258221ab067e3c7e22c29ff882ef8bc`. The permuter reimplements behavior from [Simon Lindholm's decomp-permuter](https://github.com/simonlindholm/decomp-permuter).
