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

## Source

`src/` holds the reconstruction commands. `ags/` holds the asset readers and the earlier Rust encoders used for inspection and comparisons with Alchemy's C encoders. These are outside Alchemy's ROM build.

This repository starts with the tooling from Alchemy revision `2c2a8be6b258221ab067e3c7e22c29ff882ef8bc`. The permuter reimplements behavior from [Simon Lindholm's decomp-permuter](https://github.com/simonlindholm/decomp-permuter).
