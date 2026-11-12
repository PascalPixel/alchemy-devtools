//! land: what a landing on Alchemy's main runs before it is pushed (C3), from
//! the checkout: all twelve editions, the current-GCC build, the build tools'
//! tests and this toolkit's, then the measurement the pre-push hook
//! (`hooks/pre-push`) sends for CI.

use crate::compile::root;
use std::path::Path;
use std::process::Command;

const USAGE: &str = r#"usage: alchemy-devtools land
Run in an Alchemy checkout, on main, with every source change staged, before
a landing is pushed: make compare-editions (all twelve editions against
rom.sha1), make modern (both games' C through a current GCC), make test (the
build tools' tests, the format and device checks), then this toolkit's own
tests against the checkout (cargo test from alchemy-devtools/, and again
with the render feature where libmgba's headers are: MGBA_PREFIX, else
Homebrew's or the system's prefix), and last the measurement (progress),
which writes the receipt the pre-push hook sends. It stops at the first
step that fails. Install the hook once with
  git config core.hooksPath alchemy-devtools/hooks"#;

/// Runs `program` with `arguments` from `folder`, its output the terminal's.
fn step(program: &str, arguments: &[&str], folder: &Path, environment: &[(&str, &Path)]) -> Result<(), String> {
    let shown = std::iter::once(program).chain(arguments.iter().copied()).collect::<Vec<_>>().join(" ");
    println!("land: {shown}");
    let mut command = Command::new(program);
    command.args(arguments).current_dir(folder);
    for (name, value) in environment {
        command.env(name, value);
    }
    let status = command.status().map_err(|error| format!("{shown}: {error}"))?;
    status.success().then_some(()).ok_or_else(|| format!("{shown} failed ({status}): the landing stops here"))
}

/// The prefix whose headers hold libmgba's, as the render feature's build
/// looks for them.
fn mgba_prefix() -> Option<std::path::PathBuf> {
    let named = std::env::var_os("MGBA_PREFIX").map(std::path::PathBuf::from);
    named.into_iter().chain(["/opt/homebrew", "/usr/local", "/usr"].map(std::path::PathBuf::from)).find(|prefix| prefix.join("include/mgba/core/core.h").is_file())
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    if arguments.iter().any(|argument| matches!(argument.as_str(), "-h" | "--help")) {
        println!("{USAGE}");
        return Ok(());
    }
    if let Some(extra) = arguments.first() {
        return Err(format!("land takes no arguments: {extra}\n{USAGE}"));
    }
    let root = root();
    let manifest = root.join("alchemy-devtools/Cargo.toml");
    if !manifest.is_file() {
        return Err(format!("{} is missing: clone alchemy-devtools into the checkout's alchemy-devtools/ folder", manifest.display()));
    }
    let manifest = manifest.to_string_lossy().into_owned();
    let jobs = std::thread::available_parallelism().map_or(4, usize::from).to_string();
    step("make", &["-j", &jobs, "compare-editions"], root, &[])?;
    step("make", &["-j", &jobs, "modern"], root, &[])?;
    step("make", &["test"], root, &[])?;
    let checkout = [("ALCHEMY_ROOT", root)];
    let tests = ["test", "--locked", "--release", "--workspace", "--manifest-path", &manifest];
    step("cargo", &tests, root, &checkout)?;
    match mgba_prefix() {
        Some(prefix) => {
            let with_render = [&tests[..], &["--features", "render"]].concat();
            step("cargo", &with_render, root, &[("ALCHEMY_ROOT", root), ("MGBA_PREFIX", &prefix)])?;
        }
        None => println!("land: render tests skipped: no libmgba headers (MGBA_PREFIX)"),
    }
    println!("land: progress");
    crate::progress::run(&[])
}
