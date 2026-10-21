//! --jobs: a render's work split across child processes of this binary,
//! each with its own libmgba machines. The parent gives each child its share
//! by key (`--share`, repeated) and a part file (`--part`) to write what it
//! recorded into instead of index.tsv and skipped.tsv; the parent then joins
//! the parts and writes those files as a run in one process would.

use std::path::{Path, PathBuf};
use std::process::Command;

/// What --jobs and the child's own options asked for.
#[derive(Default)]
pub struct Jobs {
    /// Child processes to run; 1 renders in this process.
    pub count: usize,
    /// In a child, the file its records go to.
    pub part: Option<PathBuf>,
    /// In a child, the keys of its share of the work.
    pub share: Vec<String>,
}

impl Jobs {
    pub fn new() -> Jobs {
        Jobs { count: std::thread::available_parallelism().map_or(1, |n| n.get()), ..Jobs::default() }
    }

    /// Reads one of the job options, or says it is not one.
    pub fn option(&mut self, argument: &str, value: &mut dyn FnMut(&str) -> Result<String, String>) -> Result<bool, String> {
        match argument {
            "--jobs" => {
                let text = value("--jobs")?;
                self.count = text.parse().ok().filter(|&n| n > 0).ok_or_else(|| format!("--jobs {text:?}: a number of processes, 1 or more"))?;
            }
            "--part" => self.part = Some(PathBuf::from(value("--part")?)),
            "--share" => self.share.push(value("--share")?),
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Whether this process is a child, given a share of the work.
    pub fn child(&self) -> bool {
        self.part.is_some()
    }

    /// Whether to split `items` things to do across children.
    pub fn split(&self, items: usize) -> bool {
        !self.child() && self.count > 1 && items > 1
    }
}

/// Deals weighted keys to at most `n` groups, the heaviest first, each onto
/// the group with the least work so far; a group keeps its keys in the order
/// they were given.
pub fn deal(items: &[(String, u64)], n: usize) -> Vec<Vec<String>> {
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(|&a, &b| items[b].1.cmp(&items[a].1).then(a.cmp(&b)));
    let mut groups: Vec<(u64, Vec<usize>)> = vec![(0, Vec::new()); n.min(items.len()).max(1)];
    for i in order {
        let lightest = (0..groups.len()).min_by_key(|&g| (groups[g].0, g)).unwrap();
        groups[lightest].0 += items[i].1;
        groups[lightest].1.push(i);
    }
    groups
        .into_iter()
        .filter(|(_, members)| !members.is_empty())
        .map(|(_, mut members)| {
            members.sort_unstable();
            members.into_iter().map(|i| items[i].0.clone()).collect()
        })
        .collect()
}

/// Runs `render <command> <arguments>` once for each group, all at once,
/// each with `--jobs 1`, its keys and its own part file, and returns the
/// parts' records in group order, with a line for each child that failed.
pub fn run(command: &str, arguments: &[String], groups: &[Vec<String>]) -> Result<(Vec<Vec<String>>, Vec<String>), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find this program to run its children: {e}"))?;
    let dir = std::env::temp_dir();
    let mut children = Vec::new();
    for (i, keys) in groups.iter().enumerate() {
        let part = dir.join(format!("alchemy-devtools-render-{}-{i}.part", std::process::id()));
        let _ = std::fs::remove_file(&part);
        let mut child = Command::new(&exe);
        child.arg("render").arg(command).args(arguments).args(["--jobs", "1"]).arg("--part").arg(&part);
        for key in keys {
            child.arg("--share").arg(key);
        }
        let process = child.spawn().map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
        children.push((process, part, keys));
    }
    let mut parts = Vec::new();
    let mut failed = Vec::new();
    for (mut process, part, keys) in children {
        let status = process.wait().map_err(|e| format!("cannot wait for a child: {e}"))?;
        let records = read_part(&part);
        let _ = std::fs::remove_file(&part);
        match records {
            Ok(records) if status.success() => parts.push(records),
            Ok(records) => {
                failed.push(format!("the child for {} {status}", keys.join(", ")));
                parts.push(records);
            }
            Err(e) => failed.push(format!("the child for {} {status}: {e}", keys.join(", "))),
        }
    }
    Ok((parts, failed))
}

/// Writes a child's records, one a line, a record's own line breaks and
/// backslashes escaped.
pub fn write_part(path: &Path, records: &[String]) -> Result<(), String> {
    let mut text = String::new();
    for record in records {
        text.push_str(&record.replace('\\', "\\\\").replace('\n', "\\n"));
        text.push('\n');
    }
    std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn read_part(path: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("no part {}: {e}", path.display()))?;
    Ok(text.lines().map(unescape).collect())
}

fn unescape(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deals_the_heaviest_first_to_the_lightest() {
        let items: Vec<(String, u64)> = [("a", 5), ("b", 1), ("c", 4), ("d", 3), ("e", 1)].iter().map(|&(k, w)| (k.to_string(), w)).collect();
        assert_eq!(deal(&items, 2), vec![vec!["a", "b", "e"], vec!["c", "d"]]);
        assert_eq!(deal(&items, 9).len(), 5);
        assert_eq!(deal(&items, 1), vec![vec!["a", "b", "c", "d", "e"]]);
    }

    #[test]
    fn escapes_records() {
        for record in ["plain\ttab", "two\nlines", "back\\slash\\n", "end\\"] {
            let escaped = record.replace('\\', "\\\\").replace('\n', "\\n");
            assert!(!escaped.contains('\n'));
            assert_eq!(unescape(&escaped), record);
        }
    }
}
