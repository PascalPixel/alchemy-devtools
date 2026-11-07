# 🌋 Alchemy Devtools

Optional Rust tools for working on [Alchemy](https://github.com/PascalPixel/alchemy)'s decompilation: compare a C draft with the game's machine code, search for C that compiles to it (a Rust take on [decomp-permuter](https://github.com/simonlindholm/decomp-permuter)), render the game for the documentation, and measure progress. Building or modding the games doesn't need them.

## Build

Install [Rust](https://rustup.rs), then from an Alchemy checkout:

```sh
make devtools
alchemy-devtools/target/release/alchemy-devtools --help
```

The first `make devtools` clones this repository into `alchemy-devtools/`.
