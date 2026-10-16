# Psynergy

Psynergy is Alchemy's optional Rust reconstruction toolkit. The target Alchemy checkout's `AGENTS.md` owns its reconstruction rules, compiler routes and progress definitions; follow those when changing behavior here. Keep Alchemy's ordinary build independent of Rust.

Use only our own cartridge dumps, Alchemy's current source and public documentation as Golden Sun evidence. Never read another project's Golden Sun work or leaked Nintendo or Camelot sources. Never commit or upload ROMs, original game files or private inputs.

Keep commands small and direct. `src/` owns matching and progress; `ags/` owns asset readers and reference encoders. Resolve the target Alchemy checkout at runtime, from the working directory or `ALCHEMY_ROOT`; never bake a developer's checkout path into an executable.

Use `cargo test --locked --workspace` with `ALCHEMY_ROOT` pointing at a bootstrapped Alchemy checkout. Changes affecting asset encoding must also pass Alchemy's twelve-edition comparison. Any change to progress definitions or compiler routing follows Alchemy's approval and separate-commit rules.
