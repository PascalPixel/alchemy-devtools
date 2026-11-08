//! twins: for each unmatched function, the reference game's functions closest in
//! shape, from the built ELFs.

const USAGE: &str = "usage: alchemy-devtools twins REFERENCE_GAME [--game GAME]";

pub fn run(_arguments: &[String]) -> Result<(), String> {
    Err(format!("twins: not yet\n{USAGE}"))
}
