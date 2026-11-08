//! variants: named rewrites of a draft, each measured and kept only when it
//! narrows the skeleton gap.

const USAGE: &str = "usage: alchemy-devtools variants DRAFT.c REWRITES [ADDRESS]";

pub fn run(_arguments: &[String]) -> Result<(), String> {
    Err(format!("variants: not yet\n{USAGE}"))
}
