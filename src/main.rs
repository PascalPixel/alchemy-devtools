//! alchemy-devtools: the matching aids and the progress count. The ROMs build from
//! the Makefile alone; nothing here takes part in a build. `gba` holds what any GBA
//! decompilation could use, `alchemy` holds Alchemy's own layout, and each command
//! has its front-end beside them.

use std::process::ExitCode;

mod adopt;
mod alchemy;
mod check;
mod compile;
mod diff;
mod editions;
mod gba;
mod land;
// The colour math is read only by the PNG recolouring the render feature builds.
#[cfg_attr(not(feature = "render"), allow(dead_code))]
mod lit;
mod passdiff;
mod permute;
mod progress;
mod rank;
mod regress;
mod render;
mod sweep;
mod twins;
mod variants;
mod r#where;

const USAGE: &str = r#"usage: alchemy-devtools <command> [args]
Run any command with --help for its options.

  adopt     gather a game's objects into one in its linker script, with a journal to undo it
  check     each built ROM, overlay and PACKER row against the reference ROMs, and the fake-match tags
  diff      a C draft against the listing it would replace
  editions  what an edition still takes from its scaffold while English links C
  land      before a landing on main: all twelve editions, make modern, the tests, then the measurement
  lit       recolour a picture in alchemy-lit, the colour correction the docs site's filter is fitted to
  passdiff  why a form is kept: an object compiled as it stands and with a plain variant, compared pass by pass
  permute   search rewrites and PERM macro choices of a draft toward differ 0
  progress  DONE code and data in each edition, from the linker maps, and what is left to humanize
  rank      every unfinished piece of a game, furthest first, each with its measure command
  regress   the functions whose assembly differs from a saved build, each classed
  render    the docs site's pictures and recordings of a built edition (feature render)
  sweep     take out the fake-match devices every edition compiles the same without
  twins     for each unfinished function (with --all, each matched one), the other game's matched function most like it in shape
  variants  named rewrites of a draft, measured as diff measures it, best first; --keep writes the best
  where     the bytes each edition still places from recon/, and DONE from the maps"#;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = arguments.first().map(String::as_str) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let rest = &arguments[1..];
    match command {
        "adopt" => result(adopt::run(rest)),
        "check" => result(check::run(rest)),
        "diff" => result(diff::run(rest)),
        "editions" => result(editions::run(rest)),
        "land" => result(land::run(rest)),
        "lit" => result(lit::run(rest)),
        "passdiff" => result(passdiff::run(rest)),
        "permute" => result(permute::run(rest)),
        "progress" => result(progress::run(rest)),
        "rank" => result(rank::run(rest)),
        "regress" => result(regress::run(rest)),
        "render" => result(render::run(rest)),
        "sweep" => result(sweep::run(rest)),
        "twins" => result(twins::run(rest)),
        "variants" => result(variants::run(rest)),
        "where" => result(r#where::run(rest)),
        "-h" | "--help" => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("unknown alchemy-devtools command: {command}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn result(value: Result<(), String>) -> ExitCode {
    match value {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
