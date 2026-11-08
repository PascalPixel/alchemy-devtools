//! Candidate filtering, deduplication and saved source diffs.
use std::borrow::Cow;
use std::collections::HashSet;
use std::path::Path;
use super::SourceHash;
use super::score::Score;

/// Which candidates are saved as they are found: none worse than the draft,
/// none whose instructions a saved one already has (exact matches aside),
/// and with `better_only` none merely as good as the draft.
pub struct State {
    base: (u64, usize),
    hashes: HashSet<SourceHash>,
    better_only: bool,
}

impl State {
    pub fn new(base: &Score, hash: SourceHash, better_only: bool) -> Self {
        Self { base: base.distance(), hashes: HashSet::from([hash]), better_only }
    }

    pub fn accepts(&self, score: &Score, hash: SourceHash) -> bool {
        let score = score.distance();
        score <= self.base && !self.hashes.contains(&hash) && (score < self.base || !self.better_only)
    }

    pub fn record(&mut self, score: &Score, hash: SourceHash) {
        if !score.is_zero() { self.hashes.insert(hash); }
    }
}

/// Save the complete patch, showing large preprocessed contexts by path only.
pub fn write_diff<'a>(path: &Path, diff: &'a str) -> Result<Cow<'a, str>, String> {
    std::fs::write(path, diff).map_err(|error| format!("{}: {error}", path.display()))?;
    let lines = diff.lines().count();
    if lines <= 80 && diff.len() <= 8 * 1024 {
        Ok(Cow::Borrowed(diff))
    } else {
        Ok(Cow::Owned(format!("diff: {} ({lines} lines, {} bytes; full patch saved)\n", path.display(), diff.len())))
    }
}

pub fn write(root: &Path, score: u64, source: &str, base: &str) -> Result<(), String> {
    let mut counter = 1u64;
    let directory = loop {
        let directory = root.join(format!("output-{score}-{counter}"));
        match std::fs::create_dir(&directory) {
            Ok(()) => break directory,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => counter += 1,
            Err(error) => return Err(format!("{}: {error}", directory.display())),
        }
    };
    let diff = diff(base, source);
    for (name, text) in [("source.c", source.to_string()), ("score.txt", format!("{score}\n")), ("diff.txt", diff)] {
        let path = directory.join(name);
        std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    println!("wrote to {}", directory.display());
    Ok(())
}

/// Compare stripped lines while preserving whitespace in the saved diff.
pub fn diff(base: &str, source: &str) -> String {
    let a: Vec<_> = base.split('\n').collect();
    let b: Vec<_> = source.split('\n').collect();
    let left: Vec<_> = a.iter().map(|s| s.trim()).collect();
    let right: Vec<_> = b.iter().map(|s| s.trim()).collect();
    let mut ops = Vec::new();
    let (mut i, mut j) = (0, 0);
    for (ai, bj, size) in super::score::matching_blocks_autojunk(&left, &right, true) {
        if i < ai || j < bj { ops.push((false, i, ai, j, bj)); }
        if size > 0 { ops.push((true, ai, ai + size, bj, bj + size)); }
        (i, j) = (ai + size, bj + size);
    }
    if let Some(op) = ops.first_mut().filter(|op| op.0) { op.1 = op.2.saturating_sub(3).max(op.1); op.3 = op.4.saturating_sub(3).max(op.3); }
    if let Some(op) = ops.last_mut().filter(|op| op.0) { op.2 = (op.1 + 3).min(op.2); op.4 = (op.3 + 3).min(op.4); }
    let mut groups = Vec::new();
    let mut group = Vec::new();
    for op in ops {
        if op.0 && op.2 - op.1 > 6 {
            group.push((true, op.1, op.1 + 3, op.3, op.3 + 3));
            groups.push(std::mem::take(&mut group));
            group.push((true, op.2 - 3, op.2, op.4 - 3, op.4));
        } else { group.push(op); }
    }
    if group.iter().any(|op| !op.0) { groups.push(group); }
    let range = |start: usize, end: usize| {
        let count = end - start;
        if count == 1 { (start + 1).to_string() }
        else { format!("{},{}", start + usize::from(count > 0), count) }
    };
    let mut lines = Vec::new();
    for group in groups {
        if lines.is_empty() { lines.extend(["--- before".into(), "+++ after".into()]); }
        let first = group[0];
        let last = group[group.len() - 1];
        lines.push(format!("@@ -{} +{} @@", range(first.1, last.2), range(first.3, last.4)));
        for (equal, i, end_i, j, end_j) in group {
            if equal { lines.extend(a[i..end_i].iter().map(|s| format!(" {s}"))); }
            else {
                lines.extend(a[i..end_i].iter().map(|s| format!("-{s}")));
                lines.extend(b[j..end_j].iter().map(|s| format!("+{s}")));
            }
        }
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_policy_sequences() {
        let hash = |name: &str| super::super::text_key(name);
        let base = Score { weighted: 100, ..Score::default() };
        let mut policy = State::new(&base, hash("base"), false);
        for row in include_str!("c/fixtures/output-policy.tsv").lines() {
            let c: Vec<_> = row.split('\t').collect();
            if c[1] == "0" {
                policy = State::new(&base, hash("base"), c[0] == "1");
            }
            let score = Score { weighted: c[2].parse().unwrap(), ..Score::default() };
            let accepted = policy.accepts(&score, hash(c[3]));
            assert_eq!(accepted, c[4] == "1", "{row}");
            if accepted { policy.record(&score, hash(c[3])); }
        }
    }

    #[test]
    fn equal_weighted_scores_retain_strict_improvements() {
        let hash = |name: &str| super::super::text_key(name);
        let base = Score { differ: 42, ..Score::default() };
        let mut policy = State::new(&base, hash("base"), true);
        assert!(!policy.accepts(&base, hash("tie")));
        let improved = Score { differ: 2, ..Score::default() };
        assert!(policy.accepts(&improved, hash("improved")));
        policy.record(&improved, hash("improved"));
        assert!(!policy.accepts(&improved, hash("improved")));
        assert!(!policy.accepts(&base, hash("worse")));
        let exact = Score::default();
        assert!(policy.accepts(&exact, hash("exact")));
        policy.record(&exact, hash("exact"));
        assert!(policy.accepts(&exact, hash("exact")), "every spelling of an exact match is kept");
    }

    #[test]
    fn whitespace_ignoring_source_diffs() {
        for row in include_str!("c/fixtures/output-diffs.tsv").lines() {
            let c: Vec<_> = row.split('\t').map(|s| s.replace("\\n", "\n")).collect();
            assert_eq!(diff(&c[0], &c[1]), c[2], "base {:?}, source {:?}", c[0], c[1]);
        }
    }

    #[test]
    fn giant_preprocessed_patch_keeps_console_bounded_and_artifact_complete() {
        let base: String = (0..1546).map(|i| format!("extern int global_{i};\n")).collect();
        let expanded: String = (0..9883).map(|i| format!("typedef unsigned int context_{i};\n")).collect();
        let patch = super::super::unified(&base, &expanded, "recon/tbs/en/main/08000000.c");
        assert!(patch.lines().count() > 10_000 && patch.len() > 8 * 1024);
        let work = tempfile::tempdir().unwrap();
        let path = work.path().join("best-1.diff");
        let shown = write_diff(&path, &patch).unwrap();
        assert_eq!(shown.lines().count(), 1);
        assert!(shown.len() < 512);
        assert!(shown.contains(&path.display().to_string()));
        assert!(shown.contains(&format!("{} lines", patch.lines().count())));
        assert!(!shown.contains("context_9882"));
        assert_eq!(std::fs::read(&path).unwrap(), patch.as_bytes());
    }

    #[test]
    fn console_patch_limits_preserve_small_diffs_and_bound_long_lines() {
        let work = tempfile::tempdir().unwrap();
        let path = work.path().join("best-1.diff");
        for patch in ["+changed\n".repeat(80), "x".repeat(8 * 1024)] {
            assert_eq!(write_diff(&path, &patch).unwrap(), patch);
            assert_eq!(std::fs::read(&path).unwrap(), patch.as_bytes());
        }
        for patch in ["+changed\n".repeat(81), "x".repeat(8 * 1024 + 1)] {
            let shown = write_diff(&path, &patch).unwrap();
            assert_eq!(shown.lines().count(), 1);
            assert!(shown.len() < 512);
            assert_eq!(std::fs::read(&path).unwrap(), patch.as_bytes());
        }
    }

    #[test]
    fn candidate_files_keep_existing_outputs() {
        let root = tempfile::tempdir().unwrap();
        let source = "extern int global;\n\nint target(void)\n{\n  return 2;\n}\n";
        write(root.path(), 0, source, source).unwrap();
        write(root.path(), 0, source, source).unwrap();
        for number in [1, 2] {
            let path = root.path().join(format!("output-0-{number}"));
            assert_eq!(std::fs::read_to_string(path.join("source.c")).unwrap(), source);
            assert_eq!(std::fs::read_to_string(path.join("score.txt")).unwrap(), "0\n");
            assert_eq!(std::fs::read_to_string(path.join("diff.txt")).unwrap(), "\n");
        }
    }
}
