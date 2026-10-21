//! --jobs: a render's work shared out among child processes of this binary,
//! each with its own libmgba machines. The parent starts N children with
//! `--child` and keeps a queue of the work's pieces, by key, the costliest
//! first; each child is handed the next key whenever it finishes one, so
//! one long piece does not hold the others back. A child sends back what it
//! recorded for each piece instead of writing index.tsv and skipped.tsv,
//! and the parent writes those files as a run in one process would.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::Mutex;

/// A line a child prints to its standard output: one of its records, the
/// failure of a piece, or the end of a piece.
const RECORD: &str = "\x1falchemy-render\trecord\t";
const FAILED: &str = "\x1falchemy-render\tfailed\t";
const DONE: &str = "\x1falchemy-render\tdone";

/// What --jobs asked for.
pub struct Jobs {
    /// Child processes to run; 1 renders in this process.
    pub count: usize,
    /// Whether this process is a child, reading its pieces' keys from its
    /// standard input.
    pub child: bool,
}

impl Jobs {
    pub fn new() -> Jobs {
        Jobs { count: std::thread::available_parallelism().map_or(1, |n| n.get()), child: false }
    }

    /// Reads one of the job options, or says it is not one.
    pub fn option(&mut self, argument: &str, value: &mut dyn FnMut(&str) -> Result<String, String>) -> Result<bool, String> {
        match argument {
            "--jobs" => {
                let text = value("--jobs")?;
                self.count = text.parse().ok().filter(|&n| n > 0).ok_or_else(|| format!("--jobs {text:?}: a number of processes, 1 or more"))?;
            }
            "--child" => self.child = true,
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Whether to share `pieces` pieces of work out among children.
    pub fn split(&self, pieces: usize) -> bool {
        !self.child && self.count > 1 && pieces > 1
    }
}

/// What the children sent back: every record, and a line for each piece
/// that failed or was never played.
pub struct Joined {
    pub records: Vec<String>,
    pub failed: Vec<String>,
}

/// Runs `render <command> <arguments> --jobs 1 --child` up to `count`
/// times at once and hands out `pieces` (each a key and its estimated
/// cost), the costliest first, to whichever child is free.
pub fn run(command: &str, arguments: &[String], pieces: &[(String, u64)], count: usize) -> Result<Joined, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find this program to run its children: {e}"))?;
    let mut order: Vec<&(String, u64)> = pieces.iter().collect();
    order.sort_by(|a, b| b.1.cmp(&a.1));
    let queue = Mutex::new(order.into_iter().map(|(key, _)| key.clone()).collect::<VecDeque<String>>());
    let joined = Mutex::new(Joined { records: Vec::new(), failed: Vec::new() });
    let mut children = Vec::new();
    for _ in 0..count.min(pieces.len()) {
        let child = Command::new(&exe)
            .arg("render")
            .arg(command)
            .args(arguments)
            .args(["--jobs", "1", "--child"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
        children.push(child);
    }
    std::thread::scope(|scope| {
        for mut child in children {
            let (queue, joined) = (&queue, &joined);
            scope.spawn(move || {
                let mut input = child.stdin.take().expect("piped");
                let mut output = BufReader::new(child.stdout.take().expect("piped"));
                let mut records = Vec::new();
                let mut failed = Vec::new();
                'pieces: loop {
                    let Some(key) = queue.lock().unwrap().pop_front() else {
                        break;
                    };
                    if writeln!(input, "{}", escape(&key)).and_then(|()| input.flush()).is_err() {
                        failed.push(format!("{key}: its process had stopped"));
                        break;
                    }
                    loop {
                        let mut line = String::new();
                        if output.read_line(&mut line).unwrap_or(0) == 0 {
                            failed.push(format!("{key}: its process stopped"));
                            break 'pieces;
                        }
                        let line = line.trim_end_matches('\n');
                        if line == DONE {
                            break;
                        } else if let Some(record) = line.strip_prefix(RECORD) {
                            records.push(unescape(record));
                        } else if let Some(failure) = line.strip_prefix(FAILED) {
                            failed.push(unescape(failure));
                        } else {
                            println!("{line}");
                        }
                    }
                }
                drop(input);
                match child.wait() {
                    Ok(status) if !status.success() => failed.push(format!("a child process {status}")),
                    Err(e) => failed.push(format!("a child process: {e}")),
                    Ok(_) => {}
                }
                let mut joined = joined.lock().unwrap();
                joined.records.extend(records);
                joined.failed.extend(failed);
            });
        }
    });
    let mut joined = joined.into_inner().unwrap();
    for key in queue.into_inner().unwrap() {
        joined.failed.push(format!("{key}: not played, every child process had stopped"));
    }
    Ok(joined)
}

/// A child's side: plays each key its standard input gives with `piece`,
/// and prints what it recorded, or why it failed, then that it is done.
pub fn serve(mut piece: impl FnMut(&str) -> Result<Vec<String>, String>) -> Result<(), String> {
    let error = |e: std::io::Error| format!("the parent process is gone: {e}");
    for line in std::io::stdin().lock().lines() {
        let key = unescape(&line.map_err(error)?);
        let result = piece(&key);
        let mut out = std::io::stdout().lock();
        match result {
            Ok(records) => {
                for record in records {
                    writeln!(out, "{RECORD}{}", escape(&record)).map_err(error)?;
                }
            }
            Err(e) => writeln!(out, "{FAILED}{}", escape(&format!("{key}: {e}"))).map_err(error)?,
        }
        writeln!(out, "{DONE}").map_err(error)?;
        out.flush().map_err(error)?;
    }
    Ok(())
}

/// A record on one line: its own line breaks and backslashes escaped.
fn escape(record: &str) -> String {
    record.replace('\\', "\\\\").replace('\n', "\\n")
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
    fn escapes_records() {
        for record in ["plain\ttab", "two\nlines", "back\\slash\\n", "end\\"] {
            assert!(!escape(record).contains('\n'));
            assert_eq!(unescape(&escape(record)), record);
        }
    }
}
