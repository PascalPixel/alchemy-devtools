//! regress: the functions whose assembly differs between two builds of the tree,
//! each with its class.

const USAGE: &str = "usage: alchemy-devtools regress REFERENCE_DIRECTORY [FILE] [--show]";

pub fn run(_arguments: &[String]) -> Result<(), String> {
    Err(format!("regress: not yet\n{USAGE}"))
}
