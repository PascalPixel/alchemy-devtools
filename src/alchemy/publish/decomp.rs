//! The report decomp.dev reads, from all twelve editions: each edition is a
//! version of its game (`The Lost Age 🇩🇪`) whose units are the parts of its
//! count, and each part is a category of that version.

use crate::alchemy::done::{Counted, Game, GAMES};
use crate::gba::report::{self, Category, Unit};
use std::path::Path;

pub(crate) const REPORT: &str = "out/reports/decomp/combined_report/report.pb";

const PARTS: [(&str, &str); 6] = [
    ("c", "C"),
    ("assembly", "Assembly (includes library and veneers)"),
    ("library", "Compiler-library assembly"),
    ("veneers", "8-byte veneers"),
    ("remaining", "Not yet C"),
    ("data", "Data (DONE data: from games/ or fill, out of every non-code byte)"),
];

/// Each part of an edition: its name, code bytes, data bytes, the
/// categories it belongs to and whether it is done.
type Bucket = (&'static str, i64, i64, &'static [&'static str], bool);

fn buckets(counted: &Counted) -> Result<[Bucket; 7], String> {
    let d = counted.done;
    let (c, assembly) = (d.common_c + d.game_c, d.common_asm + d.game_asm);
    if [c, assembly, d.veneers, counted.library]
        .iter()
        .any(|&b| b < 0)
        || counted.library + d.veneers > assembly
        || d.bytes() > d.executable
        || d.data < 0
        || d.data > d.data_total
    {
        return Err("progress parts do not partition executable bytes".into());
    }
    Ok([
        ("C", c, 0, &["c"], true),
        (
            "Assembly excluding library and veneers",
            assembly - counted.library - d.veneers,
            0,
            &["assembly"],
            true,
        ),
        (
            "Compiler-library assembly",
            counted.library,
            0,
            &["assembly", "library"],
            true,
        ),
        ("8-byte veneers", d.veneers, 0, &["assembly", "veneers"], true),
        (
            "Not yet credited",
            d.executable - d.bytes(),
            0,
            &["remaining"],
            false,
        ),
        ("Data from source", 0, d.data, &["data"], true),
        (
            "Data not yet from source",
            0,
            d.data_total - d.data,
            &["data"],
            false,
        ),
    ])
}

fn version(game: &str, language: &str) -> String {
    let flag = match language {
        "ja" => "🇯🇵",
        "en" => "🇺🇸",
        "de" => "🇩🇪",
        "es" => "🇪🇸",
        "fr" => "🇫🇷",
        _ => "🇮🇹",
    };
    format!("{game} {flag}")
}

fn report(games: [&Game; 2]) -> Result<Vec<u8>, String> {
    let (mut units, mut categories) = (Vec::new(), Vec::new());
    for ((_, _, title), game) in GAMES.into_iter().zip(games) {
        for (language, counted) in &game.editions {
            let name = version(title, language);
            for (part, bytes, data, tags, complete) in buckets(counted)? {
                if bytes == 0 && data == 0 {
                    continue;
                }
                let (bytes, data) = (bytes as u64, data as u64);
                let matched = |bytes: u64| if complete { bytes } else { 0 };
                let mut listed = vec![name.clone()];
                listed.extend(tags.iter().map(|tag| format!("{name}.{tag}")));
                units.push(Unit {
                    name: format!("{name}/{part}"),
                    code: (bytes, matched(bytes)),
                    data: (data, matched(data)),
                    complete,
                    categories: listed,
                });
            }
            categories.push(Category { id: name.clone(), name: name.clone() });
            for (part, label) in PARTS {
                categories.push(Category { id: format!("{name}.{part}"), name: label.to_string() });
            }
        }
    }
    Ok(report::encode(&units, &categories))
}

/// Writes the report when both games are measured, and removes a stale one
/// otherwise.
pub(crate) fn write(root: &Path, games: &[Result<Game, String>; 2]) -> Result<String, String> {
    let path = root.join(REPORT);
    let [Ok(sun), Ok(anchor)] = games else {
        return match std::fs::remove_file(&path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("{REPORT}: {error}"))
            }
            _ => Ok("decomp-report=pending".into()),
        };
    };
    let bytes = report([sun, anchor])?;
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| format!("{REPORT}: {e}"))?;
    std::fs::write(&path, bytes).map_err(|e| format!("{REPORT}: {e}"))?;
    Ok(format!("decomp-report={REPORT}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_reaches_decomp_dev_beside_code() {
        let mut counted = Counted::default();
        counted.done.game_c = 6;
        counted.done.executable = 10;
        counted.done.data = 7;
        counted.done.data_total = 9;
        let parts = buckets(&counted).unwrap();
        let code: i64 = parts.iter().map(|part| part.1).sum();
        let data: i64 = parts.iter().map(|part| part.2).sum();
        let done: i64 = parts.iter().filter(|part| part.4).map(|part| part.2).sum();
        assert_eq!((code, data, done), (10, 9, 7));
        counted.done.data = 10;
        assert!(buckets(&counted).is_err());
    }
}
