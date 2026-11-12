//! check: the build judged against the reference ROMs, the PACKER rows
//! against the streams they name, and the fake-match tags against S2. Every judgement only reads a ROM; nothing here writes one
//! or feeds it to the build (O1).

mod packer;
pub(crate) mod rom;
mod tags;

const USAGE: &str = "usage: alchemy-devtools check overlays [ID...] [--edition EDITION|GAME]...
       alchemy-devtools check packer [EDITION|GAME...]
       alchemy-devtools check rom [EDITION|GAME...] [--all | --origin]
       alchemy-devtools check tags [PATH...]
overlays: each built overlay's packed stream (out/<edition>/overlays/
resource_<id>.lz) looked for in its edition's reference ROM; an ID is the
overlay's hex id (64b) or any path naming resource_<id>. No ID checks every
overlay built, no edition every edition built.
packer: each row a .s file under games/ marks `@ PACKER: left in the buffer by
NAME+OFF` against the bytes NAME's stream has at OFF in the edition's
reference ROM, in every edition whose assembler reads the row (its
.ifdef, .ifeq and .else followed with the edition's symbols). No edition
checks every edition built.
rom: each built ROM against its reference, byte for byte; every range that
differs is named by the symbol and object that placed it, the first 40
unless --all. Each edition that differs first names its origin: the first
object named for its place (a scaffold's .rom.OFFSET, an English listing or
fADDRESS module) that the build places elsewhere, the last placed right
before it, and the objects from there on, each marked where its bytes differ;
everything after a size change differs, so that is where to look. --origin
prints only that. No edition checks every edition built.
tags: every FAKEMATCH tag in the C sources and headers under PATH (games/ by
default) against S2's form, /* FAKEMATCH(<kind>): the form kept; as <plain
form>, N of M instructions differ in <edition>. */, with kind signature,
pin, barrier, wrapper, volatile, temporary or dead, in the spellings The
Broken Seal's finished tags use (the plain form named by what it lacks,
'without them'; a nested function's instructions; a note after the
edition); every LEFTOVER tag, Camelot's own idle code, to the same form,
/* LEFTOVER(<kind>): what remains and the evidence; as <plain form>, N of M
instructions differ in <edition>. */, with kind frame, template or form,
its first part naming its evidence (an edition, a language or The Lost Age,
sibling routines, its family's macro or helper, or the frame); and every
function or macro with more asm statements than FAKEMATCH tags.
Each prints what it finds and fails when anything differs or is malformed;
overlays, packer and rom also fail when an edition named is not built, or
when none is.";

pub fn run(arguments: &[String]) -> Result<(), String> {
    let Some((what, rest)) = arguments.split_first() else {
        return Err(format!("check needs overlays, packer, rom or tags\n{USAGE}"));
    };
    if [what].into_iter().chain(rest).any(|argument| argument == "-h" || argument == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    match what.as_str() {
        "overlays" => rom::overlays(rest),
        "packer" => packer::run(rest),
        "rom" => rom::roms(rest),
        "tags" => tags::run(rest),
        other => Err(format!("unknown check {other}\n{USAGE}")),
    }
}
