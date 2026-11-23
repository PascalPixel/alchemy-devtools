//! Progress: DONE code and data from the twelve editions' linker maps
//! (calcrom), and the humanization report. The count and its policy are
//! `alchemy::done`, the report `alchemy::humanity`; this is the command line
//! that drives them. Both games reached 100%; the chart of how is README's
//! record, no longer drawn or published from here.

use crate::alchemy::done::{self, Game, GAMES};
use crate::alchemy::humanity;
use std::path::Path;

const USAGE: &str = "usage: alchemy-devtools progress [--humanity [--top N] [--list METRIC]]
With no option, counts DONE code and data in each edition built and matching
rom.sha1 (make compare-editions) and prints a line for each; a game without
all six editions is pending. Nothing is written, sent or published.
--humanity counts what is left to humanize in each game's tracked C and
headers, from the source and, for where names come from, this toolkit's
evidence/GAME-names.tsv (a name's rows, by name and declaring file; for a
game not yet converted, the ★ Named from lines in its source), prints a
table and writes out/reports/humanity.tsv; --top N also lists the N files leading each count,
and --list METRIC prints each counted thing of one of the table's metrics
instead (--list provenance is the name provenance worklist, --list
unused_word the words the game's text never uses, --list door_word the
words of exit names nothing resolves yet, and --list synonym,
synonym_referent, abbreviation, family_shape or prefix_twin the names a
reader could not guess from their siblings). FAKEMATCH devices and
Camelot's leftovers (the LEFTOVER tags and the // DEAD: marks) are counted
on rows of their own and never added together, a tag of a kind S2 does
not name counted among its word's; --list fakematch and --list leftover take
every kind of each. Every number in code counts in magic_code_all; the ones
its tokens, the call it stands in or the parameter it fills show to be
honest under S3 (composed forms, tenths and percentages, decimal digits'
radix and place values, random ranges, also one held in a local first,
decimal positions and durations, read out through a group or a
toFIXED/INTtoFIXED/FIXEDtoINT they fill, the frames a loop that waits a
frame each pass counts, its counter started in the statement before it
too, a local array's size, places on the map grid, beside its own cell
macros too, and in OAM words, data through a pointer call, a cast one
too, dampings, a sheet's own animation numbers, fade, shake, shade,
colour and REG_BLDY levels, masks of whole fields beside &, the sign bit's
>> 31, the shift amounts and byte indices of a word packed into bytes and
the two shifts that cut a bit field, the indices of an array filled in
order from 0 and the weights an array named for them holds, the steps
and indices a .c file's own macros take and the steps of the switch they
write, the cases of a counter moved by one each pass, a product of
numbers, degrees for DEGtoANG and a fraction of a named constant, the
percentages a routine named for a chance returns, amounts in a debug
room's code, hardware addresses from 0x02000000 to 0x0fffffff, a
function-like macro's own definition, a place or a slant's start, a half
cell beside the grid's own macro, the period of a cue on the counter of a
loop whose bound is frames, a cap of decimal digits (> 99), a byte cut by
% 256 or from a cast value by / 256, the shift scaling an Iwram_MulFX
product or a sine, a canvas, palette or fade call's level and a colour
channel's offset, and an index or stride going on from the ones before it
in its routine) are honest_KIND, a number with an honest row of this
toolkit's evidence/GAME-numbers.tsv (by file, routine and number) is
honest_judged, and magic_code is what is left to name, listed with the
callee and the parameter each fills, its numbers with an unsettled row
counted again in magic_unsettled; a lone plain speed a routine turns into 16.16
itself counts as data. A parameter is named as its definition names it
(a far call's as its routine's), or, where that name says nothing of
what it holds, as another declaration does. Every hex number counts in hex: flag
numbers (flags.h, a FLAG_ name, the whole argument of a call or macro named
for a flag or filling a parameter named for one), addresses and
values from 0x02000000 up, masks and bits (beside a bitwise operator or a
shift, io_reg.h's and the flash chip's flash.h's register values, a define
or enumerator in a family of single bits or within its prefix's _MASK,
named a mask or bit, an all-ones value with a name, or written beside a
bitwise operator anywhere and, unless a single bit, never reckoned with), a
picture's pixel words (in a table named for tiles or pixels), the sound
driver's, a function-like macro's own (hex_definition), an instruction's
encoding or a CRC's polynomial by its name (hex_encoding) and one with an
honest row of evidence/GAME-numbers.tsv (hex_judged) are the kinds S3
keeps, and hex_other is
the rest, to judge (a packed field stays; an offset, a count or a 16.16 value
is written otherwise; hex_unsettled counts again those with an unsettled
row). A numbered name with an
its-number evidence row counts as numbered_honest; a name whose rows are of
kinds no check reads counts as provenance_unchecked, and apart as
provenance_unseen where its its-code row says the thing shows nothing on
screen. Files and folders under games/GAME count too (--list
loose_headers, part_folders, numbered_files; a numbered file whose symbol
the C and assembly write only where it is defined, a numbered sample only
sounds nothing plays select by their MIDI program changes, and a numbered
map no row of the scene table names are numbered_files_unused), and a placeholder only its own definition writes
is placeholder_unused. A duplicate group with a row of this toolkit's
evidence/GAME-duplicates.tsv is duplicate_judged. A word the game never
uses that a seen copy of a routine nothing runs or a seen lettered sibling
of an actor placed out of sight lends is lent_word, one of things nothing
shows (its-code rows) unseen_word. --list unit_open names the unit each
question keeps open, with the question's whole text. A macro defined right
before an #include of a file that uses its name, used nowhere else in its
file or #undef'd right after, is a parameter of the shared file, and so is
what that file declares under such a name: undocumented_shared where the
shared file's comments name it, else still counted, listed with the shared
file as where to say it.";

/// Both games' DONE, a line for each edition and one for the games.
fn measure(root: &Path) -> String {
    let games: [Result<Game, String>; 2] = std::thread::scope(|scope| {
        GAMES
            .map(|(game, _, _)| scope.spawn(move || done::measure_game(root, game)))
            .map(|thread| thread.join().expect("measurement thread"))
    });
    let combined = |game: &Result<Game, String>| game.as_ref().ok().map(Game::combined);
    format!("{}measured {}", done::lines_of(&games, &[0, 1]), done::status_line(combined(&games[0]), combined(&games[1])))
}

/// `--humanity`'s options: `--top N` and `--list METRIC`.
fn humanity_options(root: &Path, options: &[String]) -> Result<String, String> {
    let (mut top, mut metric) = (0, None);
    let mut options = options.iter();
    while let Some(option) = options.next() {
        let value = options.next().ok_or_else(|| format!("{option} takes a value\n{USAGE}"))?;
        match option.as_str() {
            "--top" => top = value.parse().map_err(|_| format!("--top takes a number, not {value}"))?,
            "--list" => metric = Some(value.as_str()),
            _ => return Err(format!("unrecognized --humanity option: {option}\n{USAGE}")),
        }
    }
    humanity::run(root, top, metric)
}

pub(crate) fn run(argv: &[String]) -> Result<(), String> {
    let root = crate::compile::root();
    let line = match argv {
        [flag] if flag == "-h" || flag == "--help" => USAGE.to_string(),
        [] => measure(root),
        [flag, options @ ..] if flag == "--humanity" => humanity_options(root, options)?,
        _ => return Err(format!("unrecognized arguments: {}\n{USAGE}", argv.join(" "))),
    };
    println!("{line}");
    Ok(())
}
