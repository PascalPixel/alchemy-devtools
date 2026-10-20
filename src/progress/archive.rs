//! Immutable release records replace the generated progress branch. Only a
//! successful CI build admits a receipt as <commit>.accepted.tsv; report uploads
//! alone never enter the chart. The original history remains an immutable seed.

use super::{figure, history, letters, parse_measurement, publish, read, status_line, write};
use std::path::Path;
use std::process::Command;

const BASELINE: &str = "progress-history-v1.tsv";
const BASELINE_DIGEST: &str =
    "sha256:7bafc24521086fb2d892dc7c40e656ab169956f60bae6f0abcbb7111b435100c";

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

pub(super) fn publish(
    root: &Path,
    archive: &Path,
    commit: &str,
    directory: &Path,
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
    let current = read(&archive.join(format!("{commit}.receipt.tsv")))?;
    let current_files = reports(archive, commit)?;
    let measured_at = publish::verify(&current, &tree(root, commit)?, &current_files)?;
    let current_measurement =
        parse_measurement(std::str::from_utf8(&current_files[1]).map_err(|e| e.to_string())?)?;
    if current_measurement.iter().any(Option::is_none) {
        return Err("publication requires both games' six-edition measurements".into());
    }

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
            if saved != current {
                return Err("this commit already has a different accepted receipt".into());
            }
            continue;
        }
        // An old rerun cannot incorporate newer commits or another branch.
        if !ancestor(root, id, commit)? {
            continue;
        }
        let files = reports(archive, id)?;
        let time = publish::verify(&saved, &tree(root, id)?, &files)?;
        if time > measured_at {
            return Err(format!("measurement clock precedes accepted ancestor {id}"));
        }
        let values = parse_measurement(std::str::from_utf8(&files[1]).map_err(|e| e.to_string())?)?;
        if values.iter().any(Option::is_none) {
            return Err(format!("incomplete accepted measurement: {id}"));
        }
        records.push((time, id.to_string(), values));
    }
    records.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    // The selected revision wins a same-hour tie with its ancestors.
    records.push((measured_at, commit.to_string(), current_measurement));
    let mut history = history::load_file(&baseline)?;
    for (time, _, [sun, anchor]) in &records {
        let hour = history::utc_hour(*time);
        history::record(&mut history, &hour[..10], *sun, *anchor);
        history::record_hour(&mut history, &hour, *sun, *anchor);
    }
    let hour = history::utc_hour(measured_at);
    let today = &hour[..10];
    history::mark_drawn(&mut history, today);
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    write(&directory.join("history.tsv"), &history::text(&history))?;
    write(
        &directory.join("source.tsv"),
        &format!("commit\tmeasured_at\n{commit}\t{measured_at}\n"),
    )?;
    write(&directory.join(format!("{commit}.accepted.tsv")), &current)?;
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
    let [sun, anchor] = current_measurement;
    Ok(format!(
        "published {} from {commit}",
        status_line(sun, anchor)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historical_publication_excludes_newer_or_divergent_commits() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        publish::command(root, "git", &["init", "--quiet"]).unwrap();
        let commit = |text: &str| {
            std::fs::write(root.join("source.c"), text).unwrap();
            publish::command(root, "git", &["add", "source.c"]).unwrap();
            publish::command(
                root,
                "git",
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
            )
            .unwrap();
            publish::command(root, "git", &["rev-parse", "HEAD"]).unwrap()
        };
        let first = commit("first");
        let later = commit("later");
        assert!(ancestor(root, &first, &later).unwrap());
        assert!(!ancestor(root, &later, &first).unwrap());
        publish::command(root, "git", &["checkout", "--quiet", "--detach", &first]).unwrap();
        let other = commit("other branch");
        assert!(!ancestor(root, &other, &later).unwrap());
        assert!(ancestor(root, &later, &later).unwrap());
        assert!(ancestor(root, "0000000000000000000000000000000000000000", &later).is_err());
    }
}
