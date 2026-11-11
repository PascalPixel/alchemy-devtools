This file is not for humans.

# Alchemy Devtools

The toolkit and the method for matching a GBA game's machine code as C: measure a draft against the game, rank what is left, adopt what matches, count what is done, and run that work as agents. The project's own `AGENTS.md` owns what its source may contain (for Alchemy: S1 to S3, C3, O1 to O5, K1 to K3); this file owns how to get there and, for Alchemy, what counts and how unfinished work is kept (C1, C2 and S4, under "Rules moved from Alchemy"). Where they meet, the project's rules win, and any change to what counts or to compiler routing follows the project's approval and separate-commit rules.

Evidence is our own cartridge dumps, the project's source and public documentation. Never another project's work on the same game, never leaked SDKs or sources, never a ROM or game file committed or uploaded.

The project builds without Rust; nothing here takes part in a build. Resolve the target checkout at runtime, from the working directory or `ALCHEMY_ROOT`; never bake a path into an executable. `render` runs a built edition in libmgba behind the optional `render` feature and only reads the ROM. Test with `cargo test --locked --workspace` and `ALCHEMY_ROOT` pointing at a bootstrapped checkout, once without the feature and once with it; neither may warn.

## Where code goes

- `src/gba/` is what any GBA decompilation could use: ELF and GNU ld map readers, a function's bounds, a source's recipe from its own Makefile (shell lines through the Makefile's SHELL), calcrom's count over a map, decomp.dev's report, the Thumb reader, a C lexer, the process runner, the twins' shapes. It reaches nothing outside `crate::gba` and names no game, project or layout; the boundary test in `src/gba/mod.rs` refuses `crate::` paths out, `super::super`, the games' and project's names, and a project's folders and files (`out/`, `obj/`, `games/`, `recon/`, `roms/`, `rom.sha1`, `objects.tsv`, `resource_`). A rule that is one project's (which folder is done, an overlay's sections, a placeholder's spelling it alone uses) is passed in by its caller.
- `src/alchemy/` is Alchemy's: rom.sha1's editions, the object list and scaffolds, the build's object folder (`built::object_stem` is its one reading), Camelot's overlays, DONE and its publication, humanity.
- Each command's front-end is `src/<command>.rs`, or `src/<command>/` when it has parts; one command reaches another only through what it makes `pub(crate)` for that.

## Running the work

The main session ranks, briefs, verifies, adopts and lands; agents match. A workflow is one phase: read its results before starting the next. What took Alchemy's The Lost Age from 47% to 88% in a week:

1. **Rank before choosing.** The worklist comes from the tree by a command, never by hand: `rank` (with `--twins` for each piece's nearest matched function in the other game) lists every unfinished function in every edition, furthest first by skeleton gap (the instructions that differ once registers, stack offsets, pool offsets and branch distances are ignored), a function with no draft furthest of all, identical shapes grouped so one agent solves a family and its copies follow, and what does not measure last. Re-rank between rounds; a round takes the ranking's head and drops what the last round could not move, saying so. `where` and `editions` say where the bytes still to match are. With nothing left to match, `twins GAME --all` (`--min N` for the shortest function read, 12 by default) pairs each function a game places from `games/` with the reference game's most like it, so a mechanism one game has humanized lends its names to the other's (Alchemy H6).
2. **One agent per function or family, each editing only its own draft.** Nothing shared is edited in parallel: the object list, scaffolds, linker scripts, headers, the Makefile. A type an agent needs goes in its draft. Agents never run make, never commit, never change git state, and keep scratch files in a folder of their own outside the repository; compiler dumps appearing in the repository root mean a brief forgot to say so.
3. **The brief is short and exact.** Name the rules that bite instead of pasting them; agents load the project's `AGENTS.md`. Give the function, its draft and current figure, the exact measure command (rank's `measure` column: `diff` with nested listings, `--symbol`, `--edition`, `--arm`), the tools for the work (`diff --show`, `--hunks`, `--heat`, `--rtl`, `--alloc`, `--flag` for analysis; `variants` for named rewrites; `permute` for a search), the order of work (control flow and types, then allocation and scheduling, then tagged devices mildest first, only after reshaping fails), the stop rule (differ 0, or about twenty attempts in a row with no gain), keep-best (copy the draft whenever it improves, restore when an attempt goes backwards), the header to leave behind (`// Matches: differ 0 (edition, date)` or `// Draft, differ N of M (...)`, then what was tried), and the one file it may edit.
4. **Structured results.** Every agent returns a schema: status, differ in each edition, devices added and removed, what is left, and one new fact that reaches other drafts. The main session sorts and acts on data, not prose.
5. **Facts carry between rounds.** A final agent groups the round's new facts by reach and names the contradictions; the next round's briefs point at the result. Without it every round rediscovers the same compiler behaviour. A fact that holds across rounds belongs in "What matching has measured".
6. **Verify before adopting.** An independent, skeptical agent re-measures every claimed match in every edition (`diff --edition`) and reads it against the project's source rules: every device tagged with a measured N of M (`check tags`), no disguised device, no assembly body, edition tests by the project's rule, no unevidenced name. Only confirmed matches go forward.
7. **Adopt serially, with the build as judge.** One owner adopts in batches with `adopt`, which refuses a draft that does not measure differ 0 and holds a lock so no two adoptions meet, then builds every edition and traces a failure back to its function, which returns to its draft while the rest of the batch stays: `adopt --verify` does all of it (it blames the object that placed the first differing byte), or by hand `make compare-editions`, `check rom` (the symbol and object that placed each differing byte, and the origin where a size change moved the layout), `check overlays`, `check packer`, and `adopt --undo JOURNAL ITEM`. A measured differ 0 is not a match: literal pool placement and padding, the next object's alignment, labels and nested functions, an edition's own bytes, and names the tables lack all show only in the linked ROM.
8. **Review before landing.** An independent review of the landing range looks for what the build cannot see (untagged or disguised devices, edition names in C, unevidenced names, stale comments); `regress` against a saved `out/` classes every function a compiler, option or routing change moved. Fix, then land with the measurement (`progress`, through the project's `make land`).
9. **Share the checkout by owner.** Concurrent workflows divide by what they edit: drafts, adopted source files, the toolkit. A binary in use is never rebuilt under a running workflow: change the toolkit in a worktree of its own.
10. **Tools live in this repository.** A script that ranks, adopts or measures becomes a command here the day it proves useful; scratch folders get wiped, and Alchemy lost its ranking and adoption scripts that way.
11. **Scale to the work.** Matching is wide: one agent per item over the whole ranking. Design questions are narrow: a few readers, then the main session decides. Never cap silently; log what a round left out (rank names the listings it leaves out on stderr). Check that an item list is not empty before launching it.

Near misses stopped by allocation or scheduling are not chased while far-off work remains; the far-off work often settles them. The last stretch works them with tagged devices where the project allows them, so DONE is reached and the devices are cleared afterwards: `sweep` takes out each device every edition compiles the same without.

Cost decides the shape. One agent per draft with a second agent verifying each claim cost about 64M tokens for 0.9 points of The Lost Age: most claims failed on rules the matching agent never checked. A few agents, each working a batch in turn and checking its own drafts against the rules (`check tags`, the source rules, `diff` in every edition) before calling one clean, took 2.75 points for 1.1M. Make the agent that writes a match prove it; keep independent verification for samples and landings.

## Rules moved from Alchemy

Alchemy's matching is finished, and its `AGENTS.md` keeps what its source may contain. These rules are about matching and counting; they keep their Alchemy ids, and changing what they count needs Pascal and lands as its own commit (Alchemy W6).

- **C1** A function counts only when its whole extent, literal pool included, compiles exactly and every edition stays byte-identical; in an edition, only when that edition links its C.
- **C2** DONE is pret's calcrom over the linker maps, by folder: bytes placed from `games/` are done, from `recon/` not yet. Code is `.text`; data is everything else. A game's figure sums its six editions.
- **S4** Never throw work away: commit each adoption and attempt; a near miss stays a draft with its remaining difference in its header; an edition whose bytes differ stays on its scaffold and never blocks the others.

How Alchemy's matching ran, in its own words before `recon/` went:

- Match furthest first: drafts in `recon/<game>/en/main/<address>.c`; a near miss stays a draft with its remaining difference in its header (S4); a match is adopted into the module of its linker neighbours in every edition whose bytes agree, and committed (C1). A mismatch is fixed in source, or the code stays a draft (Alchemy O4). Anything that is not proven assembly (Alchemy S1) stays in `recon/` until it matches as C.
- ROM bytes reach the build only through pret's `.incbin "baserom.gba"` scaffold in `recon/`, which never counts (Alchemy O1).
- "Not yet matched in this edition" is decided by the object list or the scaffold, never by a `LANGUAGE` test (Pascal, 7 November 2026; Alchemy S3).
- The Lost Age may add tagged devices to reach a match (Alchemy S2).

## What matching has measured

Facts about Alchemy's compiler, alchemy-gcc, not rules.

- `p[n]` and `*(p + n)` can compile differently; each site keeps the spelling it matches with.
- A struct declared before its type is complete keeps a smaller alignment: a header includes its structs' headers first. alchemy-gcc pads every struct to 4 bytes, so a table read by columns is a `[][N]` array.
- Locals declared in an inner block take their pseudos later and change allocation. Allocation order is r3, r2, r1, r0, ip, lr, r4 to r7, r8, r10, r9, r11; global.c ranks pseudos by refs over live length; a constant `REG_EQUAL` note doubles a pseudo's live length.
- A loop built from labels and `goto` has no loop notes, so loop.c and jump.c leave it alone; an empty C loop is deleted.
- An empty asm counts 4 bytes when literal pools are placed and emits none; a one-pass block's loop-end note ends CSE's block and is a scheduling barrier; sched2 breaks ties by RTL order. CSE reuses a register holding 0 or 1.
- A file that tests `LANGUAGE` without including `edition.h` silently gets the Japanese branch.
- Within an object, code and data follow definition order; only the object list orders objects.
- Japanese is often an older layout of a function. A function that saves r9 and never uses it is a nested function storing its static chain.
- `-mtune=gba`'s multiplies `3 3` (Alchemy K2), against `5 5`, bring 33 drafts closer and 1 further while every matched function still matches (8 November 2026).
- To measure a rewrite in seconds, compile its one object to assembly for one edition and diff it against the build's, `.L` numbers ignored; then `make compare-editions`.
