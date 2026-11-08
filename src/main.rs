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
mod permute;
mod progress;
mod rank;
mod regress;
mod render;
mod twins;
mod variants;
mod r#where;

const USAGE: &str = r#"usage: alchemy-devtools <command> [args]
Run any command with --help for its options.

  adopt     move matched drafts into the build in every edition, with a journal to undo it
  check     each built ROM and overlay against its reference, and the fake-match tags
  diff      a C draft against the listing it would replace
  editions  what an edition still takes from its scaffold while English links C
  permute   search rewrites and PERM macro choices of a draft toward differ 0
  progress  DONE code and data in each edition, from the linker maps, and what is left to humanize
  rank      every unfinished piece of a game, furthest first, each with its measure command
  regress   the functions whose assembly differs from a saved build, each classed
  render    the docs site's pictures and recordings of a built edition (feature render)
  twins     the reference game's functions closest in shape to each unmatched one
  variants  named rewrites of a draft, each kept only when it narrows the gap
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
        "permute" => result(permute::run(rest)),
        "progress" => result(progress::run(rest)),
        "rank" => result(rank::run(rest)),
        "regress" => result(regress::run(rest)),
        "render" => result(render::run(rest)),
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
