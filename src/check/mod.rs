//! check: the build judged against the reference ROMs, and the fake-match
//! tags against S2. Every judgement only reads a ROM; nothing here writes one
//! or feeds it to the build (O1).

mod rom;
mod tags;

const USAGE: &str = "usage: alchemy-devtools check overlays [ID...] [--edition EDITION|GAME]...
       alchemy-devtools check rom [EDITION|GAME...]
       alchemy-devtools check tags [PATH...]
overlays: each built overlay's packed stream (out/<edition>/overlays/
resource_<id>.lz) looked for in its edition's reference ROM; an ID is the
overlay's hex id (64b) or any path naming resource_<id>. No ID checks every
overlay built, no edition every edition built.
rom: each built ROM against its reference, byte for byte; every range that
differs is named by the symbol and object that placed it. No edition checks
every edition built.
tags: every FAKEMATCH tag in the C sources and headers under PATH (games/ by
default) against S2's form, /* FAKEMATCH(<kind>): the form kept; as <plain
form>, N of M instructions differ in <edition>. */, with kind pin,
signature, barrier, wrapper, temporary or dead, in the spellings The Broken
Seal's finished tags use (the plain form named by what it lacks, 'without
them'; a nested function's instructions; a note after the edition); and
every function or macro with more asm statements than tags.
Each prints what it finds and fails when anything differs or is malformed;
overlays and rom also fail when an edition named is not built, or when none
is.";

pub fn run(arguments: &[String]) -> Result<(), String> {
    let Some((what, rest)) = arguments.split_first() else {
        return Err(format!("check needs overlays, rom or tags\n{USAGE}"));
    };
    if [what].into_iter().chain(rest).any(|argument| argument == "-h" || argument == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    match what.as_str() {
        "overlays" => rom::overlays(rest),
        "rom" => rom::roms(rest),
        "tags" => tags::run(rest),
        other => Err(format!("unknown check {other}\n{USAGE}")),
    }
}
