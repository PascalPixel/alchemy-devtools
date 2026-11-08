//! editions: the objects some edition does not link, and the scaffold sections
//! that hold a function another edition already links as C.

const USAGE: &str = "usage: alchemy-devtools editions [--game GAME]";

pub fn run(_arguments: &[String]) -> Result<(), String> {
    Err(format!("editions: not yet\n{USAGE}"))
}
