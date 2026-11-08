//! adopt: moves matched drafts into the build and keeps each only when every
//! edition's linked bytes agree with its ROM.

const USAGE: &str = "usage: alchemy-devtools adopt main ADDRESS[,ADDRESS...] | overlay DRAFT.c... | edition OBJECT | replace FILE DRAFT.c [--editions EDITIONS] [--dry-run]";

pub fn run(_arguments: &[String]) -> Result<(), String> {
    Err(format!("adopt: not yet\n{USAGE}"))
}
