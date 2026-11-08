//! where: the bytes each edition still places from `recon/`, by object, as code
//! or data.

const USAGE: &str = "usage: alchemy-devtools where [--edition EDITION] [--top N]";

pub fn run(_arguments: &[String]) -> Result<(), String> {
    Err(format!("where: not yet\n{USAGE}"))
}
