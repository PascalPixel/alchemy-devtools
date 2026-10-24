//! Immutable release records replace the generated progress branch. Only a
//! successful CI build admits a commit as <commit>.accepted.tsv; report uploads
//! alone never enter the chart. The original history remains an immutable seed.
//!
//! CI measures The Broken Seal itself from the six editions it built. A commit
//! with a receipt (make land) is admitted as that receipt, once its Broken Seal
//! row equals CI's. A commit without one is admitted as a CI record: CI's Broken
//! Seal row, and The Lost Age's figures from the nearest ancestor with a
//! receipt, carried as that measurement only where every path The Lost Age's
//! measurement reads has the same git tree entry, else named as its last
//! measurement and never recorded as this commit's.

use super::calcrom::GameDone;
use super::{figure, history, letters, parse_measurement, publish, read, status_line, write};
use std::path::Path;
use std::process::Command;

const BASELINE: &str = "progress-history-v1.tsv";
const BASELINE_DIGEST: &str =
    "sha256:7bafc24521086fb2d892dc7c40e656ab169956f60bae6f0abcbb7111b435100c";

/// A CI record's first line, then the measurement it admits.
const CI_HEADER: &str = "version\ttree\tmeasured_at\ttla_commit\ttla_source";
const CI_VERSION: &str = "ci1";
/// Every path The Lost Age's measurement reads: its source, object list and
/// linker scripts, its scaffolds, the Makefile and the tools that build it,
/// the pinned compilers, rom.sha1, which admits its ROMs, and the devtools
/// that count its maps. Nothing outside these reaches its build or its count.
const TLA_INPUTS: [&str; 7] = [
    "Makefile",
    "rom.sha1",
    "games/tla",
    "recon/tla",
    "tools",
    "alchemy-gcc",
    "alchemy-devtools",
];

fn reports(directory: &Path, commit: &str) -> Result<Vec<Vec<u8>>, String> {
    ["pb", "tsv"]
        .iter()
        .map(|extension| {
            let path = directory.join(format!("{commit}.{extension}"));
            std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))
        })
        .collect()
}

fn tree(root: &Path, commit: &str) -> Result<String, String> {
    publish::command(root, "git", &["rev-parse", &format!("{commit}^{{tree}}")])
}

/// The git tree entries of everything The Lost Age's measurement reads.
fn tla_inputs(root: &Path, commit: &str) -> Result<String, String> {
    let mut arguments = vec!["ls-tree", "--full-tree", commit, "--"];
    arguments.extend(TLA_INPUTS);
    publish::command(root, "git", &arguments)
}

/// A CI record is dated by its commit, so a rerun admits the same record.
fn committed_at(root: &Path, commit: &str) -> Result<u64, String> {
    publish::command(root, "git", &["show", "-s", "--format=%ct", commit])?
        .parse()
        .map_err(|_| format!("cannot read the commit time of {commit}"))
}

fn ancestor(root: &Path, commit: &str, tip: &str) -> Result<bool, String> {
    let status = Command::new("git")
        .args(["merge-base", "--is-ancestor", commit, tip])
        .current_dir(root)
        .status()
        .map_err(|error| error.to_string())?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(format!("cannot check ancestry of {commit}")),
    }
}

fn measurement(bytes: &[u8]) -> Result<[Option<GameDone>; 2], String> {
    parse_measurement(std::str::from_utf8(bytes).map_err(|e| e.to_string())?)
}

/// A receipt, verified against its commit's tree and both reports: its text,
/// time and both games' figures. A receipt that exists and fails is an error.
fn receipt(
    root: &Path,
    archive: &Path,
    commit: &str,
) -> Result<Option<(String, u64, [Option<GameDone>; 2])>, String> {
    let path = archive.join(format!("{commit}.receipt.tsv"));
    if !path.exists() {
        return Ok(None);
    }
    let saved = read(&path)?;
    let files = reports(archive, commit)?;
    let measured_at = publish::verify(&saved, &tree(root, commit)?, &files)
        .map_err(|error| format!("receipt for {commit}: {error}"))?;
    let values = measurement(&files[1])?;
    if values.iter().any(Option::is_none) {
        return Err(format!("receipt for {commit} lacks a game's six-edition measurement"));
    }
    Ok(Some((saved, measured_at, values)))
}

/// Where The Lost Age's figures come from in a CI record.
#[derive(Clone, Debug, PartialEq)]
enum Lost {
    /// No ancestor has a receipt.
    Unmeasured,
    /// Measured at this ancestor, whose Lost Age inputs are identical.
    Carried(String, GameDone),
    /// Last measured at this ancestor; its inputs have changed since.
    Last(String, GameDone),
}

/// The nearest ancestor of `commit` with a receipt, and whether everything
/// The Lost Age's measurement reads is the same there.
fn nearest_receipt(root: &Path, archive: &Path, commit: &str) -> Result<Lost, String> {
    let ancestors = publish::command(root, "git", &["rev-list", "--topo-order", commit])?;
    let inputs = tla_inputs(root, commit)?;
    for id in ancestors.lines().filter(|id| *id != commit) {
        let Some((_, _, [_, Some(anchor)])) = receipt(root, archive, id)? else {
            continue;
        };
        return Ok(if tla_inputs(root, id)? == inputs {
            Lost::Carried(id.to_string(), anchor)
        } else {
            Lost::Last(id.to_string(), anchor)
        });
    }
    Ok(Lost::Unmeasured)
}

fn ci_text(tree: &str, measured_at: u64, sun: GameDone, lost: &Lost) -> String {
    let (from, source, anchor) = match lost {
        Lost::Unmeasured => ("-", "-", None),
        Lost::Carried(id, anchor) => (id.as_str(), "identical", Some(*anchor)),
        Lost::Last(id, _) => (id.as_str(), "changed", None),
    };
    format!(
        "{CI_HEADER}\n{CI_VERSION}\t{tree}\t{measured_at}\t{from}\t{source}\n{}",
        super::measurement_text(Some(sun), anchor)
    )
}

/// Checks an accepted CI record against the history it names: its commit's
/// tree and time, and The Lost Age's figures against the receipt they claim,
/// carried only where its inputs are identical. Only CI writes the Broken
/// Seal row, from its own build.
fn verify_ci(root: &Path, archive: &Path, commit: &str, saved: &str) -> Result<(u64, Lost, GameDone), String> {
    let wrong = |why: &str| format!("accepted CI record for {commit}: {why}");
    let mut lines = saved.splitn(3, '\n');
    let (Some(CI_HEADER), Some(fields), Some(rest)) = (lines.next(), lines.next(), lines.next()) else {
        return Err(wrong("unexpected form"));
    };
    let fields = fields.split('\t').collect::<Vec<_>>();
    let [CI_VERSION, tree_id, time, from, source] = fields.as_slice() else {
        return Err(wrong("unexpected fields"));
    };
    if *tree_id != tree(root, commit)? {
        return Err(wrong("its tree is not its commit's"));
    }
    let measured_at = time.parse::<u64>().map_err(|_| wrong("invalid time"))?;
    if measured_at != committed_at(root, commit)? {
        return Err(wrong("its time is not its commit's"));
    }
    let [Some(sun), _] = parse_measurement(rest)? else {
        return Err(wrong("it holds no Broken Seal row"));
    };
    let lost = match (*from, *source) {
        ("-", "-") => Lost::Unmeasured,
        (id, "identical" | "changed") if publish::valid_id(id) && id != commit => {
            if !ancestor(root, id, commit)? {
                return Err(wrong("The Lost Age's figures come from no ancestor"));
            }
            let Some((_, _, [_, Some(measured)])) = receipt(root, archive, id)? else {
                return Err(wrong("The Lost Age's figures name a commit with no receipt"));
            };
            if (tla_inputs(root, id)? == tla_inputs(root, commit)?) != (*source == "identical") {
                return Err(wrong("The Lost Age's inputs are not as it says"));
            }
            if *source == "identical" {
                Lost::Carried(id.to_string(), measured)
            } else {
                Lost::Last(id.to_string(), measured)
            }
        }
        _ => return Err(wrong("unexpected Lost Age source")),
    };
    if ci_text(tree_id, measured_at, sun, &lost) != saved {
        return Err(wrong("its figures are not the ones it names"));
    }
    Ok((measured_at, lost, sun))
}

/// What publication admits for the selected commit.
struct Admitted {
    text: String,
    measured_at: u64,
    values: [Option<GameDone>; 2],
    lost: Option<Lost>,
}

fn admit(root: &Path, archive: &Path, commit: &str, sun: GameDone) -> Result<Admitted, String> {
    if let Some((text, measured_at, values)) = receipt(root, archive, commit)? {
        if values[0] != Some(sun) {
            return Err(format!(
                "the receipt for {commit} claims Broken Seal figures CI did not measure: receipt {:?} · CI {sun:?}",
                values[0].unwrap_or_default()
            ));
        }
        return Ok(Admitted { text, measured_at, values, lost: None });
    }
    let lost = nearest_receipt(root, archive, commit)?;
    let measured_at = committed_at(root, commit)?;
    let anchor = match &lost {
        Lost::Carried(_, anchor) => Some(*anchor),
        _ => None,
    };
    Ok(Admitted {
        text: ci_text(&tree(root, commit)?, measured_at, sun, &lost),
        measured_at,
        values: [Some(sun), anchor],
        lost: Some(lost),
    })
}

/// An accepted ancestor's time and figures, from its receipt or CI record.
fn accepted(
    root: &Path,
    archive: &Path,
    id: &str,
    saved: &str,
) -> Result<(u64, [Option<GameDone>; 2]), String> {
    if saved.starts_with(CI_HEADER) {
        let (time, lost, sun) = verify_ci(root, archive, id, saved)?;
        let anchor = match lost {
            Lost::Carried(_, anchor) => Some(anchor),
            _ => None,
        };
        return Ok((time, [Some(sun), anchor]));
    }
    let files = reports(archive, id)?;
    let time = publish::verify(saved, &tree(root, id)?, &files)?;
    let values = measurement(&files[1])?;
    if values.iter().any(Option::is_none) {
        return Err(format!("incomplete accepted measurement: {id}"));
    }
    Ok((time, values))
}

/// How the published Lost Age figures were measured, for source.tsv and the
/// closing line.
fn lost_columns(commit: &str, lost: &Option<Lost>) -> (String, &'static str) {
    match lost {
        None => (commit.to_string(), "measured"),
        Some(Lost::Carried(id, _)) => (id.clone(), "identical"),
        Some(Lost::Last(id, _)) => (id.clone(), "changed"),
        Some(Lost::Unmeasured) => ("-".to_string(), "-"),
    }
}

pub(super) fn publish(
    root: &Path,
    archive: &Path,
    commit: &str,
    directory: &Path,
    measured: &Path,
) -> Result<String, String> {
    if !publish::valid_id(commit)
        || publish::command(root, "git", &["rev-parse", "HEAD"])? != commit
    {
        return Err("publication must check out the measured commit exactly".into());
    }
    let baseline = archive.join(BASELINE);
    if publish::digest(&std::fs::read(&baseline).map_err(|e| e.to_string())?) != BASELINE_DIGEST {
        return Err("the archived progress baseline is missing or changed".into());
    }
    let [Some(sun), None] = parse_measurement(&read(measured)?)? else {
        return Err("CI's measurement must hold The Broken Seal's row alone".into());
    };
    let current = admit(root, archive, commit, sun)?;

    let mut records = Vec::new();
    for entry in std::fs::read_dir(archive).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".accepted.tsv") else {
            continue;
        };
        if !publish::valid_id(id) {
            return Err(format!("invalid accepted revision: {name}"));
        }
        let saved = read(&entry.path())?;
        if id == commit {
            if saved != current.text {
                return Err("this commit already has a different accepted record".into());
            }
            continue;
        }
        // An old rerun cannot incorporate newer commits or another branch.
        if !ancestor(root, id, commit)? {
            continue;
        }
        let (time, values) = accepted(root, archive, id, &saved)?;
        if time > current.measured_at {
            return Err(format!("measurement clock precedes accepted ancestor {id}"));
        }
        records.push((time, id.to_string(), values));
    }
    records.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    // The selected revision wins a same-hour tie with its ancestors.
    records.push((current.measured_at, commit.to_string(), current.values));
    let mut history = history::load_file(&baseline)?;
    for (time, _, [sun, anchor]) in &records {
        let hour = history::utc_hour(*time);
        history::record(&mut history, &hour[..10], *sun, *anchor);
        history::record_hour(&mut history, &hour, *sun, *anchor);
    }
    if let Some(Lost::Last(id, _)) = &current.lost {
        history.mark_last_measured(id);
    }
    let hour = history::utc_hour(current.measured_at);
    let today = &hour[..10];
    history::mark_drawn(&mut history, today);
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    write(&directory.join("history.tsv"), &history::text(&history))?;
    let (tla_commit, tla_source) = lost_columns(commit, &current.lost);
    write(
        &directory.join("source.tsv"),
        &format!(
            "commit\tmeasured_at\ttla_commit\ttla_source\n{commit}\t{}\t{tla_commit}\t{tla_source}\n",
            current.measured_at
        ),
    )?;
    write(&directory.join(format!("{commit}.accepted.tsv")), &current.text)?;
    // decomp.dev reads all twelve editions, which only a receipt holds.
    if current.lost.is_none() {
        std::fs::copy(archive.join(format!("{commit}.pb")), directory.join("report.pb"))
            .map_err(|error| format!("report.pb: {error}"))?;
    }
    let letters = letters::Letters::face();
    let chart = figure::chart(&letters, &history::as_drawn(&history));
    let map = figure::map(&letters, root);
    for (name, canvas) in [(figure::CHART, chart), (figure::MAP, map)] {
        std::fs::write(
            directory.join(name),
            canvas.png(letters::FIGURE_SCALE, today)?,
        )
        .map_err(|error| format!("{name}: {error}"))?;
    }
    let [sun, anchor] = current.values;
    let shown = |done: GameDone| format!("code {:.2}% data {:.2}%", done.percent(), done.data_percent());
    let ci = format!("☀️ {} measured by CI at {commit}", shown(sun.expect("CI measures The Broken Seal")));
    Ok(match &current.lost {
        None => format!(
            "published {} from {commit}'s receipt; its Broken Seal row equals CI's",
            status_line(sun, anchor)
        ),
        Some(Lost::Carried(id, anchor)) => format!(
            "published {ci} · ⚓️ {} measured at {id}, whose Lost Age inputs are identical",
            shown(*anchor)
        ),
        Some(Lost::Last(id, anchor)) => format!(
            "published {ci} · ⚓️ last measured at {id}: {}; its Lost Age inputs have changed since",
            shown(*anchor)
        ),
        Some(Lost::Unmeasured) => format!("published {ci} · ⚓️ never measured: no ancestor has a receipt"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(root: &Path, arguments: &[&str]) -> String {
        publish::command(root, "git", arguments).unwrap()
    }

    fn commit_file(root: &Path, path: &str, text: &str) -> String {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        git(root, &["add", "-A"]);
        git(
            root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--quiet",
                "-m",
                text,
            ],
        );
        git(root, &["rev-parse", "HEAD"])
    }

    #[test]
    fn historical_publication_excludes_newer_or_divergent_commits() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        git(root, &["init", "--quiet"]);
        let first = commit_file(root, "source.c", "first");
        let later = commit_file(root, "source.c", "later");
        assert!(ancestor(root, &first, &later).unwrap());
        assert!(!ancestor(root, &later, &first).unwrap());
        git(root, &["checkout", "--quiet", "--detach", &first]);
        let other = commit_file(root, "source.c", "other branch");
        assert!(!ancestor(root, &other, &later).unwrap());
        assert!(ancestor(root, &later, &later).unwrap());
        assert!(ancestor(root, "0000000000000000000000000000000000000000", &later).is_err());
    }

    fn done(game_c: i64) -> GameDone {
        GameDone {
            game_c,
            executable: 1000,
            data: 5,
            data_total: 10,
            ..GameDone::default()
        }
    }

    /// Writes a receipt and both reports for `commit`, as make land and the
    /// pre-push hook would.
    fn upload(root: &Path, archive: &Path, commit: &str, sun: GameDone, anchor: GameDone) {
        let files = vec![b"report".to_vec(), super::super::measurement_text(Some(sun), Some(anchor)).into_bytes()];
        let measured_at = committed_at(root, commit).unwrap() - 60;
        let digests = files.iter().map(|bytes| publish::digest(bytes)).collect::<Vec<_>>();
        let saved = format!(
            "version\ttree\tmeasured_at\tpb_sha256\ttsv_sha256\n1\t{}\t{measured_at}\t{}\n",
            tree(root, commit).unwrap(),
            digests.join("\t")
        );
        assert!(publish::verify(&saved, &tree(root, commit).unwrap(), &files).is_ok());
        std::fs::write(archive.join(format!("{commit}.receipt.tsv")), saved).unwrap();
        std::fs::write(archive.join(format!("{commit}.pb")), &files[0]).unwrap();
        std::fs::write(archive.join(format!("{commit}.tsv")), &files[1]).unwrap();
    }

    #[test]
    fn the_lost_age_is_carried_only_while_its_inputs_are_identical() {
        let directory = tempfile::tempdir().unwrap();
        let (root, archive) = (directory.path().join("repo"), directory.path().join("archive"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&archive).unwrap();
        let root = root.as_path();
        git(root, &["init", "--quiet"]);
        commit_file(root, "games/tla/src/a.c", "tla");
        let measured = commit_file(root, "games/tbs/src/a.c", "tbs");
        assert_eq!(nearest_receipt(root, &archive, &measured).unwrap(), Lost::Unmeasured);
        upload(root, &archive, &measured, done(10), done(20));
        let readme = commit_file(root, "README.md", "readme");
        let tbs = commit_file(root, "games/tbs/src/a.c", "tbs changed");
        assert_eq!(
            nearest_receipt(root, &archive, &tbs).unwrap(),
            Lost::Carried(measured.clone(), done(20))
        );
        let carried = admit(root, &archive, &readme, done(11)).unwrap();
        assert_eq!(carried.values, [Some(done(11)), Some(done(20))]);
        assert_eq!(
            verify_ci(root, &archive, &readme, &carried.text).unwrap(),
            (carried.measured_at, Lost::Carried(measured.clone(), done(20)), done(11))
        );
        // A record may not claim The Lost Age's figures for other inputs.
        let tla = commit_file(root, "games/tla/src/a.c", "tla changed");
        let last = admit(root, &archive, &tla, done(11)).unwrap();
        assert_eq!(last.values, [Some(done(11)), None]);
        assert_eq!(last.lost, Some(Lost::Last(measured.clone(), done(20))));
        let forged = last.text.replace("\tchanged\n", "\tidentical\n")
            + "tla\t0\t0\t0\t20\t1000\t0\t5\t10\n";
        assert!(verify_ci(root, &archive, &tla, &forged).is_err());
        assert!(verify_ci(root, &archive, &readme, &carried.text.replace("\t11\t", "\t12\t")).is_ok());
        assert!(verify_ci(root, &archive, &tla, &carried.text).is_err());
        let wrong_tla = carried.text.replace("tla\t0\t0\t0\t20", "tla\t0\t0\t0\t21");
        assert!(verify_ci(root, &archive, &readme, &wrong_tla).is_err());
        // A rebuilt tool or a new devtools pin changes what counts it.
        commit_file(root, "games/tla/src/a.c", "tla");
        let tool = commit_file(root, "tools/ags-text/a.c", "tool");
        assert!(matches!(nearest_receipt(root, &archive, &tool).unwrap(), Lost::Last(..)));
    }

    #[test]
    fn a_receipt_must_agree_with_ci_and_its_own_tree() {
        let directory = tempfile::tempdir().unwrap();
        let (root, archive) = (directory.path().join("repo"), directory.path().join("archive"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&archive).unwrap();
        let root = root.as_path();
        git(root, &["init", "--quiet"]);
        let measured = commit_file(root, "games/tbs/src/a.c", "tbs");
        upload(root, &archive, &measured, done(10), done(20));
        let admitted = admit(root, &archive, &measured, done(10)).unwrap();
        assert_eq!(admitted.lost, None);
        assert_eq!(admitted.values, [Some(done(10)), Some(done(20))]);
        assert!(admit(root, &archive, &measured, done(11)).is_err());
        // A receipt for another tree is refused, not skipped.
        let next = commit_file(root, "games/tbs/src/a.c", "tbs changed");
        let saved = std::fs::read_to_string(archive.join(format!("{measured}.receipt.tsv"))).unwrap();
        std::fs::write(archive.join(format!("{next}.receipt.tsv")), saved).unwrap();
        for extension in ["pb", "tsv"] {
            std::fs::copy(
                archive.join(format!("{measured}.{extension}")),
                archive.join(format!("{next}.{extension}")),
            )
            .unwrap();
        }
        assert!(admit(root, &archive, &next, done(10)).is_err());
    }
}
