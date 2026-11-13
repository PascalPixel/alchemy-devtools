//! sweep: fake-match devices the build no longer needs, cleared, as the
//! method's last stretch asks once DONE is reached. Each device sweep can
//! take out by itself, an empty asm a `barrier` tag names or a pair of
//! updates that cancel which a `dead` tag names, goes with its tag; the file
//! is compiled in every edition that builds it, by that edition's own
//! recipe, and the removal is kept only when every edition's assembly is the
//! build's own. A device sweep cannot take out alone is left and named.

use crate::alchemy::built;
use crate::alchemy::layout::Layout;
use crate::alchemy::targets::{target_for, DecompTargetId};
use crate::compile::root;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const USAGE: &str = "usage: alchemy-devtools sweep FILE.c... [--dry-run]
For each FAKEMATCH(barrier) tag over empty asm statements and each
FAKEMATCH(dead) tag over a pair of updates that cancel (x++; x--; or
x += n; x -= n;) in each C file under games/, from the last to the first:
the device and its tag are taken out, the file compiled in every edition
whose object list builds it, by that edition's recipe (as diff compiles a
draft), and its assembly compared with the build's own
(out/EDITION/obj/FILE.s, '@' comments and .file aside). The removal stays
when every edition's is the same, so the next device is tried without it.
A file whose unchanged source does not give the build's assembly is left:
build every edition first (make compare-editions). Other devices and tags
are left, each named. A tag taken out whose measurement is a row of this
toolkit's evidence/GAME-fakematch.tsv (H8) names that row, to be deleted
there. --dry-run writes nothing; otherwise each file is written once, with
what stayed out. make compare-editions judges the result, as it judges
every change.";

/// A device sweep can take out with its tag: its kind, the line its tag
/// opens on (1-based), and the lines tag and device take (0-based, end
/// exclusive).
#[derive(Debug, PartialEq, Eq)]
struct Device {
    kind: &'static str,
    line: usize,
    lines: std::ops::Range<usize>,
}

fn patterns() -> &'static [Regex; 4] {
    static PATTERNS: OnceLock<[Regex; 4]> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            Regex::new(r#"^(?:__asm__|asm)(?:\s+(?:volatile|__volatile__))?\s*\(\s*"""#).unwrap(),
            Regex::new(r"^([A-Za-z_][\w.\[\]>-]*)\s*(\+\+|--)\s*;$").unwrap(),
            Regex::new(r"^(\+\+|--)\s*([A-Za-z_][\w.\[\]>-]*)\s*;$").unwrap(),
            Regex::new(r"^([A-Za-z_][\w.\[\]>-]*)\s*([+-])=\s*(.+?)\s*;$").unwrap(),
        ]
    })
}

/// The update a statement makes: what it changes, and by how much with
/// which sign, as text.
fn update(statement: &str) -> Option<(String, char, String)> {
    let [_, post, pre, compound] = patterns();
    if let Some(c) = post.captures(statement) {
        return Some((c[1].to_string(), if &c[2] == "++" { '+' } else { '-' }, "1".into()));
    }
    if let Some(c) = pre.captures(statement) {
        return Some((c[2].to_string(), if &c[1] == "++" { '+' } else { '-' }, "1".into()));
    }
    compound.captures(statement).map(|c| (c[1].to_string(), c[2].chars().next().unwrap_or('+'), c[3].to_string()))
}

/// The devices of a C file sweep can take out alone, and the tags it leaves
/// (their line and why).
fn devices(text: &str) -> (Vec<Device>, Vec<(usize, String)>) {
    let lines: Vec<&str> = text.lines().collect();
    let (mut found, mut left) = (Vec::new(), Vec::new());
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at].trim();
        let Some(kind) = line.strip_prefix("/* FAKEMATCH(").and_then(|rest| rest.split_once(')')).map(|(kind, _)| kind.to_string()) else {
            at += 1;
            continue;
        };
        let Some(end) = (at..lines.len()).find(|&index| lines[index].contains("*/")) else { break };
        let after: Vec<&str> = lines[end + 1..].iter().map(|line| line.trim()).collect();
        let device = match kind.as_str() {
            "barrier" => {
                let count = after.iter().take_while(|line| patterns()[0].is_match(line) && line.ends_with(");")).count();
                (count > 0).then_some(count)
            }
            "dead" => match (after.first().and_then(|first| update(first)), after.get(1).and_then(|second| update(second))) {
                (Some((target, sign, amount)), Some((other, other_sign, other_amount)))
                    if target == other && amount == other_amount && sign != other_sign =>
                {
                    Some(2)
                }
                _ => None,
            },
            _ => None,
        };
        match device {
            Some(count) => found.push(Device { kind: if kind == "barrier" { "barrier" } else { "dead" }, line: at + 1, lines: at..end + 1 + count }),
            None if kind == "barrier" || kind == "dead" => left.push((at + 1, format!("{kind}: not empty asm statements or updates that cancel, right below the tag"))),
            None => left.push((at + 1, format!("{kind}: sweep takes out only barrier and dead devices"))),
        }
        at = end + 1;
    }
    (found, left)
}

/// `text` without the lines `lines` takes.
fn without(text: &str, lines: &std::ops::Range<usize>) -> String {
    let mut out: Vec<&str> = text.lines().enumerate().filter(|(index, _)| !lines.contains(index)).map(|(_, line)| line).collect();
    if text.ends_with('\n') {
        out.push("");
    }
    out.join("\n")
}

/// Assembly as sweep compares it: without `@` comments and `.file`, which
/// name the compiler and the file.
fn assembly(text: &str) -> Vec<&str> {
    text.lines().filter(|line| !line.starts_with('@') && !line.contains(".file\t")).collect()
}

/// The file on disk while it compiles, put back as it was unless kept.
struct Source {
    path: PathBuf,
    original: String,
}

impl Drop for Source {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.path, &self.original);
    }
}

/// Whether `text`, compiled in place of the file in each of `editions`,
/// gives the build's own assembly in every one; the first that differs.
fn same_as_built(root: &Path, source: &Source, stem: &str, text: &str, editions: &[DecompTargetId]) -> Result<Option<DecompTargetId>, String> {
    std::fs::write(&source.path, text).map_err(|error| format!("{}: {error}", source.path.display()))?;
    for &id in editions {
        let target = target_for(id);
        let work = crate::diff::Work::new(target)?;
        let object = work.folder.join(format!("{}.o", stem.rsplit('/').next().unwrap_or(stem)));
        for step in crate::compile::steps(target, &source.path, &object, None, false)? {
            crate::gba::process::run(&step, &work.folder)?;
        }
        let compiled = std::fs::read_to_string(object.with_extension("s")).map_err(|error| format!("{id}: no assembly written: {error}"))?;
        let built_path = built::output(root, id).join("obj").join(format!("{stem}.s"));
        let built = std::fs::read_to_string(&built_path).map_err(|error| format!("{}: {error}", built_path.display()))?;
        if assembly(&compiled) != assembly(&built) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// The rows of the game's fake-match table (H8) that `kept` leaves without
/// their tag and `original` did not: the measurements of the tags taken out.
fn rows_taken_out(path: &str, original: &str, kept: &str) -> Result<Vec<String>, String> {
    if original == kept {
        return Ok(Vec::new());
    }
    let before = crate::check::tags::untaken_rows(path, original)?;
    Ok(crate::check::tags::untaken_rows(path, kept)?.into_iter().filter(|row| !before.contains(row)).collect())
}

fn sweep_file(root: &Path, file: &str, dry_run: bool) -> Result<(usize, usize), String> {
    let relative = file.trim_start_matches("./");
    let stem = relative.strip_suffix(".c").ok_or_else(|| format!("{file}: sweep reads C files"))?;
    let layout = Layout::of_path(relative).filter(|layout| relative.starts_with(&layout.sources())).ok_or_else(|| format!("{file} is not under games/GAME/src"))?;
    let path = root.join(relative);
    let original = std::fs::read_to_string(&path).map_err(|error| format!("{file}: {error}"))?;
    let mut editions: Vec<DecompTargetId> = Vec::new();
    for id in built::editions(std::slice::from_ref(&layout.game))? {
        if crate::compile::listed(target_for(id), relative)? {
            editions.push(id);
        }
    }
    if editions.is_empty() {
        return Err(format!("{file}: no edition's object list builds it"));
    }
    let (found, left) = devices(&original);
    for (line, why) in &left {
        println!("{relative}:{line}: left, {why}");
    }
    if found.is_empty() {
        return Ok((0, 0));
    }
    let source = Source { path: path.clone(), original: original.clone() };
    if let Some(id) = same_as_built(root, &source, stem, &original, &editions)? {
        return Err(format!("{file}: as it stands it does not give {id}'s built assembly; build every edition first"));
    }
    let (mut kept, mut cleared) = (original.clone(), 0);
    for device in found.iter().rev() {
        let candidate = without(&kept, &device.lines);
        match same_as_built(root, &source, stem, &candidate, &editions)? {
            None => {
                println!("{relative}:{}: {} device taken out; every edition's assembly is the build's", device.line, device.kind);
                kept = candidate;
                cleared += 1;
            }
            Some(id) => println!("{relative}:{}: {} device kept; without it {id} compiles otherwise", device.line, device.kind),
        }
    }
    drop(source);
    for row in rows_taken_out(relative, &original, &kept)? {
        println!("{relative}: its row for {row} goes from evidence/{}-fakematch.tsv in alchemy-devtools", layout.game);
    }
    if cleared > 0 && !dry_run {
        std::fs::write(&path, &kept).map_err(|error| format!("{file}: {error}"))?;
    }
    Ok((cleared, found.len() - cleared))
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    if arguments.is_empty() || arguments.iter().any(|argument| matches!(argument.as_str(), "-h" | "--help")) {
        println!("{USAGE}");
        return Ok(());
    }
    let dry_run = arguments.iter().any(|argument| argument == "--dry-run");
    if let Some(flag) = arguments.iter().find(|argument| argument.starts_with('-') && *argument != "--dry-run") {
        return Err(format!("unknown sweep option {flag}\n{USAGE}"));
    }
    let root = root();
    let (mut cleared, mut kept, mut failed) = (0, 0, Vec::new());
    for file in arguments.iter().filter(|argument| !argument.starts_with('-')) {
        match sweep_file(root, file, dry_run) {
            Ok((out, stayed)) => {
                cleared += out;
                kept += stayed;
            }
            Err(error) => {
                println!("{error}");
                failed.push(file.as_str());
            }
        }
    }
    let verb = if dry_run { "would be taken out" } else { "taken out" };
    println!("{cleared} devices {verb}, {kept} kept; judge with make compare-editions");
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("not swept: {}", failed.join(" ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn barrier_and_dead_devices_are_found_with_their_tags() {
        let text = "void f(void) {\n  /* FAKEMATCH(barrier): x held; as no asm, 2 of 9\n   * instructions differ in tla-en. */\n  asm(\"\" : \"+r\"(x));\n  g();\n  /* FAKEMATCH(dead): updates that cancel; as none, 1 of 9 instructions differ in tla-en. */\n  p++;\n  p--;\n  /* FAKEMATCH(dead): a store; as none, 1 of 9 instructions differ in tla-en. */\n  q = 0;\n  /* FAKEMATCH(temporary): a copy; as none, 1 of 9 instructions differ in tla-en. */\n  r = s;\n  /* FAKEMATCH(dead): n += 4; as none, 3 of 9 instructions differ in tla-en. */\n  n += 4;\n  n -= 4;\n}\n";
        let (found, left) = devices(text);
        assert_eq!(
            found,
            [
                Device { kind: "barrier", line: 2, lines: 1..4 },
                Device { kind: "dead", line: 6, lines: 5..8 },
                Device { kind: "dead", line: 13, lines: 12..15 },
            ]
        );
        assert_eq!(left.iter().map(|(line, _)| *line).collect::<Vec<_>>(), [9, 11]);
        let swept = without(text, &found[1].lines);
        assert!(swept.contains("  g();\n  /* FAKEMATCH(dead): a store") && swept.ends_with("}\n"));
        assert_eq!(assembly("@ Generated by gcc\n\t.file\t\"x.c\"\n\tbx lr\n"), ["\tbx lr"]);
        assert_eq!(update("x->count -= n + 1;"), Some(("x->count".into(), '-', "n + 1".into())));
    }

    #[test]
    fn a_tag_taken_out_names_its_row() {
        // The Broken Seal's rows for this routine: one temporary, two dead.
        let path = "games/tbs/include/system/serial.h";
        let original = "void Serial_ResetTransferState(void) {\n  /* FAKEMATCH(temporary): zero held. */\n  /* FAKEMATCH(dead): state set again. */\n  /* FAKEMATCH(dead): a loop. */\n  do {\n  } while (0);\n}\n";
        let kept = without(original, &(3..6));
        assert_eq!(rows_taken_out(path, original, &kept).unwrap(), ["Serial_ResetTransferState (FAKEMATCH(dead))"]);
        assert!(rows_taken_out(path, original, original).unwrap().is_empty());
        assert!(rows_taken_out("games/xyz/src/x.c", original, &kept).unwrap().is_empty(), "a game with no table");
    }
}
