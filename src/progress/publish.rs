use super::decomp::REPORT;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const MEASUREMENT: &str = "out/reports/decomp/measurement.tsv";

const REPO: &str = "PascalPixel/alchemy";
/// The archive is a run of releases, `decomp-reports` then
/// `decomp-reports-2` and on, since GitHub holds at most 1,000 assets in a
/// release. Readers search every volume; writers add to the newest, opening
/// the next while the newest still has room for a landing (three assets from
/// the push, two from CI).
const RELEASE: &str = "decomp-reports";
const VOLUME_ASSETS: usize = 1000;
const LANDING_ASSETS: usize = 5;
const RECEIPT: &str = "out/reports/decomp/publication.tsv";
const STALE: &str =
    "progress measurement is stale or missing: stage all source changes, then run make land";
const ASSETS: [(&str, &str); 2] = [(REPORT, "pb"), (MEASUREMENT, "tsv")];
const HEADER: &str = "version\ttree\tmeasured_at\tpb_sha256\ttsv_sha256";

pub(super) fn command(root: &Path, program: &str, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(arguments)
        .current_dir(root)
        .output()
        .map_err(|error| format!("cannot run {program}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn receipt(tree: &str, measured_at: u64, files: &[Vec<u8>]) -> String {
    let digests = files.iter().map(|bytes| digest(bytes)).collect::<Vec<_>>();
    format!(
        "{HEADER}\n1\t{tree}\t{measured_at}\t{}\n",
        digests.join("\t")
    )
}

pub(super) fn valid_id(id: &str) -> bool {
    id.len() == 40
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A receipt names the staged source tree and both report digests. Its time
/// survives remeasurement of identical inputs, uploads and CI retries.
pub(super) fn verify(saved: &str, tree: &str, files: &[Vec<u8>]) -> Result<u64, String> {
    let fields: Vec<_> = saved
        .lines()
        .nth(1)
        .unwrap_or_default()
        .split('\t')
        .collect();
    let measured_at = fields
        .get(2)
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (1..=253_402_300_799).contains(value))
        .ok_or(STALE)?;
    if !valid_id(tree) || files.len() != 2 || saved != receipt(tree, measured_at, files) {
        return Err(STALE.into());
    }
    Ok(measured_at)
}

/// Builds read the working tree, so an index-only fingerprint is insufficient.
/// Staged changes are allowed; unstaged and untracked source must be resolved.
pub(super) fn source_tree(root: &Path) -> Result<String, String> {
    command(
        root,
        "git",
        &["diff", "--exit-code", "--ignore-submodules=none"],
    )?;
    if !command(root, "git", &["ls-files", "--others", "--exclude-standard"])?.is_empty() {
        return Err("untracked source: stage or ignore it before measuring progress".into());
    }
    command(root, "git", &["write-tree"])
}

fn canonical_remote(remote: &str) -> bool {
    [
        "https://github.com/PascalPixel/alchemy",
        "https://github.com/PascalPixel/alchemy.git",
        "git@github.com:PascalPixel/alchemy.git",
        "ssh://git@github.com/PascalPixel/alchemy.git",
    ]
    .iter()
    .any(|url| remote.eq_ignore_ascii_case(url))
}

fn read_assets(root: &Path) -> Result<Vec<Vec<u8>>, String> {
    ASSETS
        .iter()
        .map(|(path, _)| std::fs::read(root.join(path)).map_err(|_| STALE.to_string()))
        .collect()
}

pub(crate) fn prepare(root: &Path, expected_tree: &str) -> Result<(), String> {
    let tree = source_tree(root)?;
    if tree != expected_tree {
        return Err("source changed while progress was being measured".into());
    }
    let files = read_assets(root)?;
    let measured_at = std::fs::read_to_string(root.join(RECEIPT))
        .ok()
        .and_then(|saved| verify(&saved, &tree, &files).ok())
        .unwrap_or(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_secs(),
        );
    std::fs::write(root.join(RECEIPT), receipt(&tree, measured_at, &files))
        .map_err(|error| error.to_string())?;
    println!("progress measurement prepared for source tree {tree}");
    Ok(())
}

fn prepared(root: &Path, tree: &str) -> Result<Vec<Vec<u8>>, String> {
    let mut files = read_assets(root)?;
    let saved = std::fs::read_to_string(root.join(RECEIPT)).map_err(|_| STALE.to_string())?;
    verify(&saved, tree, &files)?;
    files.push(saved.into_bytes());
    Ok(files)
}

fn needs_upload(remote_digest: &str, bytes: &[u8]) -> Result<bool, String> {
    match remote_digest {
        "" => Ok(true),
        value if value == digest(bytes) => Ok(false),
        _ => Err(
            "existing measurement for this commit has a different digest; refusing to replace it"
                .into(),
        ),
    }
}

/// A volume's number: 1 for `decomp-reports`, N for `decomp-reports-N`.
fn volume_number(tag: &str) -> Option<usize> {
    match tag.strip_prefix(RELEASE)? {
        "" => Some(1),
        rest => rest
            .strip_prefix('-')
            .filter(|n| !n.starts_with('0'))
            .and_then(|n| n.parse().ok())
            .filter(|&n| n > 1),
    }
}

fn volume_tag(number: usize) -> String {
    if number == 1 {
        RELEASE.to_string()
    } else {
        format!("{RELEASE}-{number}")
    }
}

/// The archive's volumes, in order.
pub(super) fn volumes(root: &Path) -> Result<Vec<String>, String> {
    let tags = command(
        root,
        "gh",
        &["api", "--paginate", &format!("repos/{REPO}/releases"), "--jq", ".[].tag_name"],
    )?;
    let mut numbered: Vec<usize> = tags.lines().filter_map(volume_number).collect();
    numbered.sort_unstable();
    numbered.dedup();
    if numbered.first() != Some(&1) {
        return Err(format!("the {RELEASE} release is missing"));
    }
    Ok(numbered.into_iter().map(volume_tag).collect())
}

fn release_query(root: &Path, tag: &str, query: &str) -> Result<String, String> {
    command(
        root,
        "gh",
        &["api", &format!("repos/{REPO}/releases/tags/{tag}"), "--jq", query],
    )
}

/// An asset's digest in whichever volume holds it, empty when none does.
fn remote_digest(root: &Path, volumes: &[String], name: &str) -> Result<String, String> {
    let query = format!(".assets[] | select(.name == \"{name}\") | .digest");
    for tag in volumes {
        let found = release_query(root, tag, &query)?;
        if !found.is_empty() {
            return Ok(found);
        }
    }
    Ok(String::new())
}

/// The volume to add a landing's assets to: the newest, or a new one when
/// the newest has no room left for a whole landing.
fn writable_volume(root: &Path, volumes: &mut Vec<String>) -> Result<String, String> {
    let newest = volumes.last().cloned().ok_or("no archive volume")?;
    let count: usize = release_query(root, &newest, ".assets | length")?
        .parse()
        .map_err(|_| format!("cannot count the assets of {newest}"))?;
    if count + LANDING_ASSETS <= VOLUME_ASSETS {
        return Ok(newest);
    }
    let next = volume_tag(volume_number(&newest).unwrap_or(1) + 1);
    let title = format!("decomp reports {}", volume_number(&next).unwrap_or(2));
    let notes = format!("Progress measurements, continued from {newest}: GitHub holds 1,000 assets in a release.");
    // A concurrent writer may have opened it already.
    if release_query(root, &next, ".tag_name").is_err() {
        command(
            root,
            "gh",
            &["release", "create", &next, "--repo", REPO, "--prerelease", "--title", &title, "--notes", &notes],
        )
        .or_else(|why| release_query(root, &next, ".tag_name").map(|_| String::new()).map_err(|_| why))?;
    }
    volumes.push(next.clone());
    Ok(next)
}

/// Adds one file to the archive under its own name, unless a volume already
/// holds it with the same digest; refuses one that holds it changed.
fn archive_file(root: &Path, volumes: &mut Vec<String>, path: &Path, bytes: &[u8]) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("invalid report path")?;
    if !needs_upload(&remote_digest(root, volumes, name)?, bytes)? {
        return Ok(());
    }
    let tag = writable_volume(root, volumes)?;
    command(
        root,
        "gh",
        &["release", "upload", &tag, path.to_str().ok_or("invalid report path")?, "--repo", REPO],
    )?;
    if needs_upload(&remote_digest(root, volumes, name)?, bytes)? {
        return Err(format!("uploaded {name} is absent from {tag}"));
    }
    Ok(())
}

/// CI's upload of a file it made (an accepted record, a site archive).
pub(crate) fn archive(root: &Path, path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    archive_file(root, &mut volumes(root)?, path, &bytes)
}

pub(crate) fn upload(root: &Path, commit: &str, remote: &str) -> Result<(), String> {
    if !canonical_remote(remote) {
        return Ok(());
    }
    if !valid_id(commit) {
        return Err("invalid source commit for report publication".into());
    }
    let tree = command(root, "git", &["rev-parse", &format!("{commit}^{{tree}}")])?;
    let files = prepared(root, &tree)?;
    let directory = root.join("out/reports/decomp/upload");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let mut volumes = volumes(root)?;
    for (extension, bytes) in ["pb", "tsv", "receipt.tsv"].iter().zip(&files) {
        let path = directory.join(format!("{commit}.{extension}"));
        std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
        archive_file(root, &mut volumes, &path, bytes)?;
    }
    println!("progress measurement uploaded for {commit}; CI publishes it after the push");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_binds_both_reports_tree_and_time() {
        let tree = "0123456789012345678901234567890123456789";
        let files = vec![b"protobuf".to_vec(), b"measurement".to_vec()];
        let saved = receipt(tree, 1_792_497_600, &files);
        assert_eq!(verify(&saved, tree, &files).unwrap(), 1_792_497_600);
        assert!(verify(&saved, "1123456789012345678901234567890123456789", &files).is_err());
        assert!(verify(&saved, tree, &[b"changed".to_vec(), files[1].clone()]).is_err());
        assert!(verify(&saved, tree, &[files[0].clone(), b"changed".to_vec()]).is_err());
        assert!(verify(&(saved.clone() + "extra\n"), tree, &files).is_err());
        assert!(verify(&saved.replace("1792497600", "0"), tree, &files).is_err());
        assert!(!needs_upload(&digest(&files[0]), &files[0]).unwrap());
        assert!(needs_upload(&digest(&files[0]), &files[1]).is_err());
    }

    #[test]
    fn archive_volumes_are_numbered_after_the_first() {
        assert_eq!(volume_number("decomp-reports"), Some(1));
        assert_eq!(volume_number("decomp-reports-2"), Some(2));
        assert_eq!(volume_number("decomp-reports-12"), Some(12));
        for other in ["decomp-reports-1", "decomp-reports-02", "decomp-reports-x", "decomp-reportsX", "v1"] {
            assert_eq!(volume_number(other), None, "{other}");
        }
        assert_eq!(volume_tag(1), "decomp-reports");
        assert_eq!(volume_tag(3), "decomp-reports-3");
    }

    #[test]
    fn measurement_requires_working_source_to_equal_the_index() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        command(root, "git", &["init", "--quiet"]).unwrap();
        std::fs::write(root.join("source.c"), "staged\n").unwrap();
        command(root, "git", &["add", "source.c"]).unwrap();
        assert!(source_tree(root).is_ok());
        std::fs::write(root.join("source.c"), "unstaged\n").unwrap();
        assert!(source_tree(root).is_err());
        command(root, "git", &["add", "source.c"]).unwrap();
        assert!(source_tree(root).is_ok());
        std::fs::write(root.join("extra.c"), "untracked\n").unwrap();
        assert!(source_tree(root).is_err());
    }

    #[test]
    fn identical_remeasurement_keeps_its_first_timestamp() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        command(root, "git", &["init", "--quiet"]).unwrap();
        std::fs::write(root.join(".gitignore"), "out/\n").unwrap();
        command(root, "git", &["add", ".gitignore"]).unwrap();
        let tree = source_tree(root).unwrap();
        for (path, _) in ASSETS {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"report").unwrap();
        }
        let first = receipt(&tree, 1_792_497_600, &read_assets(root).unwrap());
        std::fs::write(root.join(RECEIPT), &first).unwrap();
        prepare(root, &tree).unwrap();
        assert_eq!(std::fs::read_to_string(root.join(RECEIPT)).unwrap(), first);
        assert_eq!(prepared(root, &tree).unwrap().len(), 3);
        std::fs::write(root.join(MEASUREMENT), b"changed").unwrap();
        assert!(prepared(root, &tree).is_err());
        assert!(prepare(root, "0123456789012345678901234567890123456789").is_err());
    }
}
