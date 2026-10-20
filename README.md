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

## Battle pictures

`render psynergy` plays battle Psynergy in a built ☀️ edition and writes each cast as a looping GIF of the whole screen at 30 frames a second, with an `index.tsv` of ability, name, caster, target and frames. It runs the ROM in [libmgba](https://mgba.io), linked through a small C shim, so it is behind the `render` feature: install mGBA's library and headers (Homebrew: `brew install mgba`; `MGBA_PREFIX` names another prefix), then

```sh
cargo build --locked --release --features render --manifest-path alchemy-devtools/Cargo.toml
make tbs-en
alchemy-devtools/target/release/alchemy-devtools render psynergy --all --list
alchemy-devtools/target/release/alchemy-devtools render psynergy --all --out DIR
alchemy-devtools/target/release/alchemy-devtools render psynergy --ability Flare --backdrop backdrop.png --verbose
```

The run boots the build, enters Camelot's battle test menu and steers the battle at function entries named by the linker map. `--backdrop` picks the battlefield: a grey gradient drawn as the GBA's backdrop colour (the default), the battle's own background (`game`), or a 256 by 120 PNG, a battle backdrop's size, put in the background's place in its own format, which the game then draws, scrolls, darkens and blends itself. Windows are drawn grey. Recording starts at the cast's message, after the camera's turn from the command view. `render psynergy --help` gives the casters, targets and setup. The ROM is only read; never publish it or anything copied from it.

## Progress publication

Stage source changes before `make land` or `make progress`. Measurement checks that the working source matches the index, records its tree and both report digests, and preserves the original measurement time when the same tree is measured again. Alchemy's pre-push hook uploads the reports and receipt for the pushed main revision.

After the public build passes, CI uses `progress --publish ARCHIVE COMMIT DIRECTORY` to verify the receipt and reconstruct history from its immutable baseline and accepted ancestor measurements. It writes the two figures, history, source revision and accepted receipt. Dates come from the measurement, in UTC, so retrying publication does not move a measurement to today. Alchemy's `.github/README.md` describes the release archive and separate docs deployment.

## Source

`src/` holds the reconstruction commands. `ags/` holds the asset readers and the earlier Rust encoders used for inspection and comparisons with Alchemy's C encoders. These are outside Alchemy's ROM build.

This repository starts with the tooling from Alchemy revision `2c2a8be6b258221ab067e3c7e22c29ff882ef8bc`. The permuter reimplements behavior from [Simon Lindholm's decomp-permuter](https://github.com/simonlindholm/decomp-permuter).
