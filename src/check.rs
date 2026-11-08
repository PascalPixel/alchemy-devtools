//! check: each edition's built bytes against its ROM, by function and overlay,
//! and the fake-match tags against the project's format.

const USAGE: &str = "usage: alchemy-devtools check [--edition EDITION] [--tags]";

pub fn run(_arguments: &[String]) -> Result<(), String> {
    Err(format!("check: not yet\n{USAGE}"))
}
