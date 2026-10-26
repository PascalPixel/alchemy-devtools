# Alchemy Devtools

Alchemy Devtools is Alchemy's optional Rust reconstruction toolkit. The target Alchemy checkout's `AGENTS.md` owns its reconstruction rules, compiler routes and progress definitions; follow those when changing behavior here. Keep Alchemy's ordinary build independent of Rust.

Use only our own cartridge dumps, Alchemy's current source and public documentation as Golden Sun evidence. Never read another project's Golden Sun work or leaked Nintendo or Camelot sources. Never commit or upload ROMs, original game files or private inputs.

Keep commands small and direct. `src/` owns matching, progress and the `render` pictures; the few asset readers it needs (`src/lz.rs` for packed ROM streams, `src/po.rs` for PO catalogs) live there too, and Alchemy's C tools in `tools/` are the only encoders. `render` runs a built edition in libmgba behind the optional `render` cargo feature, so the default build needs no emulator; it only reads the ROM, and what it writes are pictures of what the ROM draws. Resolve the target Alchemy checkout at runtime, from the working directory or `ALCHEMY_ROOT`; never bake a developer's checkout path into an executable.

Use `cargo test --locked --workspace` with `ALCHEMY_ROOT` pointing at a bootstrapped Alchemy checkout. Any change to progress definitions or compiler routing follows Alchemy's approval and separate-commit rules.
