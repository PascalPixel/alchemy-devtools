//! The evidence kept here rather than in Alchemy's source (H8): what each
//! name was taken from, `evidence/<game>-names.tsv` (name, kind of evidence,
//! the evidence, the file the name is declared in), and each fake match's and
//! leftover's measurement, `evidence/<game>-fakematch.tsv` (file, routine,
//! the tag's word and kind, the tag's text after them), and the numbers in
//! code someone judged by reading it, `evidence/<game>-numbers.tsv` (file,
//! routine, the number as written, `honest` or `unsettled`, what it is in a
//! few words), and the duplicate groups someone judged not to be one text,
//! `evidence/<game>-duplicates.tsv` (the members' names sorted and joined
//! with a space, `different things` or `kept apart`, why in a few words).
//! All are compiled in
//! from this repository's `evidence/` folder by `build.rs`, so a program
//! reads the evidence of its own revision; a changed row needs a rebuild.
//! `ALCHEMY_EVIDENCE` names a folder whose tables are read in their place
//! as the program runs, so rows being written in a working copy of
//! `evidence/` are judged without rebuilding; a table the folder lacks is
//! still the compiled one.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

mod tables {
    include!(concat!(env!("OUT_DIR"), "/evidence.rs"));
}

/// The environment variable naming a folder of tables read at run time.
pub const OVERRIDE: &str = "ALCHEMY_EVIDENCE";

/// A table of `evidence/` by its file name (`tbs-names.tsv`), or `None`
/// for a game not yet converted: the one in `ALCHEMY_EVIDENCE`'s folder
/// when it is set and holds the file, else the one compiled in.
pub fn table(name: &str) -> Option<&'static str> {
    if let Some(folder) = std::env::var_os(OVERRIDE) {
        if let Some(text) = read_once(&std::path::Path::new(&folder).join(name)) {
            return Some(text);
        }
    }
    tables::TABLES
        .iter()
        .find(|(file, _)| *file == name)
        .map(|(_, text)| *text)
}

/// A table read from a file once per run and kept for the rest of it.
fn read_once(path: &std::path::Path) -> Option<&'static str> {
    static READ: OnceLock<Mutex<HashMap<std::path::PathBuf, Option<&'static str>>>> =
        OnceLock::new();
    let mut read = READ
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *read.entry(path.to_path_buf()).or_insert_with(|| {
        std::fs::read_to_string(path)
            .ok()
            .map(|text| &*Box::leak(text.into_boxed_str()))
    })
}

/// A table's rows, its header line left out, each split at its tabs into
/// `columns` cells; a row with another count of cells is an error naming
/// its line.
fn rows(text: &str, columns: usize) -> Result<Vec<Vec<&str>>, String> {
    let mut out = Vec::new();
    for (at, line) in text.lines().enumerate().skip(1) {
        if line.is_empty() {
            continue;
        }
        let cells: Vec<&str> = line.split('\t').collect();
        if cells.len() != columns {
            return Err(format!(
                "line {}: {} cells, not {columns}",
                at + 1,
                cells.len()
            ));
        }
        out.push(cells);
    }
    Ok(out)
}

/// One row of a game's names table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    pub name: String,
    /// `the game`, `its area name` or `what the player sees`.
    pub kind: String,
    pub evidence: String,
    pub file: String,
}

impl Name {
    /// The row as the `★ Named from ...` line the source once held, which the
    /// provenance checks judge the same way.
    pub fn line(&self) -> String {
        format!("★ Named from {}: {}", self.kind, self.evidence)
    }
}

/// The rows of a names table's text.
pub fn parse_names(text: &str) -> Result<Vec<Name>, String> {
    Ok(rows(text, 4)?
        .into_iter()
        .map(|cells| Name {
            name: cells[0].to_string(),
            kind: cells[1].to_string(),
            evidence: cells[2].to_string(),
            file: cells[3].to_string(),
        })
        .collect())
}

/// A game's names table (`tbs`), empty for a game that has none yet.
pub fn names(game: &str) -> Result<Vec<Name>, String> {
    table(&format!("{game}-names.tsv")).map_or(Ok(Vec::new()), |text| {
        parse_names(text).map_err(|error| format!("evidence/{game}-names.tsv: {error}"))
    })
}

/// One row of a game's fake-match table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Measurement {
    pub file: String,
    /// The function or macro the tag is in.
    pub routine: String,
    /// The tag's word and kind, `FAKEMATCH(dead)`.
    pub kind: String,
    /// The tag's text after its kind: the form kept, and what the plain form
    /// costs in which editions.
    pub text: String,
}

impl Measurement {
    /// The tag as it read in full, to be judged by S2's form.
    pub fn tag(&self) -> String {
        format!("{}: {}", self.kind, self.text)
    }

    /// The row for a tag in `routine`: its own, or one that names it first
    /// (`MapGrid_CopyCellTilesIndicesRect and its siblings`).
    pub fn is_for(&self, routine: &str) -> bool {
        self.routine == routine
            || self
                .routine
                .strip_prefix(routine)
                .is_some_and(|rest| rest.starts_with(' '))
    }
}

/// The rows of a fake-match table's text.
pub fn parse_measurements(text: &str) -> Result<Vec<Measurement>, String> {
    Ok(rows(text, 4)?
        .into_iter()
        .map(|cells| Measurement {
            file: cells[0].to_string(),
            routine: cells[1].to_string(),
            kind: cells[2].to_string(),
            text: cells[3].to_string(),
        })
        .collect())
}

/// A game's fake-match table, empty for a game that has none yet.
pub fn measurements(game: &str) -> Result<Vec<Measurement>, String> {
    table(&format!("{game}-fakematch.tsv")).map_or(Ok(Vec::new()), |text| {
        parse_measurements(text).map_err(|error| format!("evidence/{game}-fakematch.tsv: {error}"))
    })
}

/// The kinds a judged number's row may be: a number S3 keeps as it is,
/// which only reading its code shows (`honest`), or one whose meaning
/// nothing in reach settles yet (`unsettled`, its row saying what would).
pub const JUDGED_KINDS: &[&str] = &["honest", "unsettled"];

/// One row of a game's judged numbers: a number in code the count cannot
/// tell from a private id by its form, judged by someone who read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Judged {
    pub file: String,
    /// The function, macro or definition the number is in: a function's or
    /// a function-like macro's name, or the name a `#define`, an enumerator
    /// or a global's initializer gives.
    pub routine: String,
    /// The number as written (`0x169`, `16`, `249U`), without its sign.
    pub number: String,
    /// One of `JUDGED_KINDS`.
    pub kind: String,
    /// What it is, in a few words; for an unsettled one, what would settle it.
    pub what: String,
}

/// The rows of a judged numbers table's text; a row of a kind not in
/// `JUDGED_KINDS`, or with an empty cell, is an error naming its line.
pub fn parse_judged(text: &str) -> Result<Vec<Judged>, String> {
    let mut out = Vec::new();
    for (at, cells) in rows(text, 5)?.into_iter().enumerate() {
        let row = Judged {
            file: cells[0].to_string(),
            routine: cells[1].to_string(),
            number: cells[2].to_string(),
            kind: cells[3].to_string(),
            what: cells[4].to_string(),
        };
        if !JUDGED_KINDS.contains(&row.kind.as_str()) {
            return Err(format!(
                "row {}: kind {}, not one of {}",
                at + 1,
                row.kind,
                JUDGED_KINDS.join(", ")
            ));
        }
        if cells.iter().any(|cell| cell.trim().is_empty()) {
            return Err(format!("row {}: an empty cell", at + 1));
        }
        out.push(row);
    }
    Ok(out)
}

/// A game's judged numbers, empty for a game that has none yet.
pub fn judged(game: &str) -> Result<Vec<Judged>, String> {
    table(&format!("{game}-numbers.tsv")).map_or(Ok(Vec::new()), |text| {
        parse_judged(text).map_err(|error| format!("evidence/{game}-numbers.tsv: {error}"))
    })
}

/// The kinds a judged duplicate group's row may be: copies that are
/// different things the same by chance (each creature's gravity, each
/// overlay's lone entrance, one motion made by different things), or one
/// thing kept as several copies the ROM holds that no shared text could give
/// (two main-image objects with no precedent for sharing, a library's own
/// copy, two return types).
pub const DUPLICATE_KINDS: &[&str] = &["different things", "kept apart"];

/// One row of a game's judged duplicates: a group of copies the duplicate
/// count finds, judged by someone who read them not to be one text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JudgedGroup {
    /// The group's member names, sorted and joined with a space.
    pub members: String,
    /// One of `DUPLICATE_KINDS`.
    pub kind: String,
    /// Why, in a few words.
    pub why: String,
}

/// The rows of a judged duplicates table's text; a row of a kind not in
/// `DUPLICATE_KINDS`, with an empty cell or with members out of order, is an
/// error naming its line.
pub fn parse_duplicates(text: &str) -> Result<Vec<JudgedGroup>, String> {
    let mut out = Vec::new();
    for (at, cells) in rows(text, 3)?.into_iter().enumerate() {
        let row = JudgedGroup {
            members: cells[0].to_string(),
            kind: cells[1].to_string(),
            why: cells[2].to_string(),
        };
        if !DUPLICATE_KINDS.contains(&row.kind.as_str()) {
            return Err(format!(
                "row {}: kind {}, not one of {}",
                at + 1,
                row.kind,
                DUPLICATE_KINDS.join(", ")
            ));
        }
        if cells.iter().any(|cell| cell.trim().is_empty()) {
            return Err(format!("row {}: an empty cell", at + 1));
        }
        let names: Vec<&str> = row.members.split(' ').collect();
        if names.len() < 2 || names.windows(2).any(|pair| pair[0] > pair[1]) {
            return Err(format!(
                "row {}: members not two or more names in order",
                at + 1
            ));
        }
        out.push(row);
    }
    Ok(out)
}

/// A game's judged duplicates, empty for a game that has none yet.
pub fn duplicates(game: &str) -> Result<Vec<JudgedGroup>, String> {
    table(&format!("{game}-duplicates.tsv")).map_or(Ok(Vec::new()), |text| {
        parse_duplicates(text).map_err(|error| format!("evidence/{game}-duplicates.tsv: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn judged_duplicates_are_read_and_checked() {
        let rows = parse_duplicates(
            "members\tkind\twhy\ngA gB\tdifferent things\teach creature's gravity\n",
        )
        .unwrap();
        assert_eq!(
            rows,
            [JudgedGroup {
                members: "gA gB".into(),
                kind: "different things".into(),
                why: "each creature's gravity".into()
            }]
        );
        assert!(
            parse_duplicates("members\tkind\twhy\ngA gB\tthe same\tno\n")
                .unwrap_err()
                .contains("kind the same")
        );
        assert!(
            parse_duplicates("members\tkind\twhy\ngB gA\tkept apart\ttwo objects\n")
                .unwrap_err()
                .contains("in order")
        );
        assert!(
            parse_duplicates("members\tkind\twhy\ngA\tkept apart\ttwo objects\n")
                .unwrap_err()
                .contains("two or more")
        );
        assert!(duplicates("tla").unwrap().len() > 10);
    }

    #[test]
    fn the_tables_are_compiled_in_and_read_whole() {
        let rows = names("tbs").unwrap();
        assert!(rows.len() > 4000, "{}", rows.len());
        assert!(rows
            .iter()
            .all(|row| row.file.starts_with("games/tbs/") && !row.name.is_empty()));
        let measured = measurements("tbs").unwrap();
        assert!(measured.len() > 100, "{}", measured.len());
        assert!(measured.iter().all(|row| row.file.starts_with("games/tbs/")
            && (row.kind.starts_with("FAKEMATCH(") || row.kind.starts_with("LEFTOVER("))));
        for game in ["tbs", "tla"] {
            let judged = judged(game).unwrap();
            assert!(
                judged.iter().any(|row| row.kind == "honest")
                    && judged.iter().any(|row| row.kind == "unsettled"),
                "{game}"
            );
            let root = crate::compile::root();
            for row in &judged {
                assert!(
                    row.file.starts_with(&format!("games/{game}/"))
                        && root.join(&row.file).is_file(),
                    "{game}: {}",
                    row.file
                );
                assert!(
                    row.number
                        .bytes()
                        .next()
                        .is_some_and(|b| b.is_ascii_digit()),
                    "{game}: {} {}",
                    row.file,
                    row.number
                );
            }
        }
        // A game not yet converted has no table, and so no rows.
        assert!(
            table("xyz-names.tsv").is_none()
                && names("xyz").unwrap().is_empty()
                && measurements("xyz").unwrap().is_empty()
                && judged("xyz").unwrap().is_empty()
        );
    }

    #[test]
    fn a_folder_of_tables_read_as_the_program_runs() {
        let folder = std::env::temp_dir().join(format!("alchemy-evidence-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("xyz-names.tsv"),
            "name\tkind\tevidence\tfile\nA\tthe game\t\"A\" (MsgA)\tgames/xyz/a.c\n",
        )
        .unwrap();
        assert!(read_once(&folder.join("xyz-names.tsv")).is_some_and(|text| text.contains("MsgA")));
        assert!(
            read_once(&folder.join("tbs-names.tsv")).is_none(),
            "a table the folder lacks is not read from it"
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn rows_and_their_lines() {
        let text = "name\tkind\tevidence\tfile\nABILITY_ATTACK\tthe game\t\"Attack\" (MsgAbilityName)\tgames/tbs/include/ability_ids.h\n\n";
        let rows = parse_names(text).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].line(),
            "★ Named from the game: \"Attack\" (MsgAbilityName)"
        );
        assert_eq!(
            parse_names("header\nA\tB\tC\n").unwrap_err(),
            "line 2: 3 cells, not 4"
        );
        let rows = parse_measurements("file\troutine\tkind\tmeasurement\na.c\tF and its siblings\tFAKEMATCH(dead)\tkept; as none, 1 of 2 instructions differ in tbs-en.\n").unwrap();
        assert_eq!(
            rows[0].tag(),
            "FAKEMATCH(dead): kept; as none, 1 of 2 instructions differ in tbs-en."
        );
        assert!(rows[0].is_for("F") && !rows[0].is_for("F_2") && !rows[0].is_for("G"));
        let header = "file\troutine\tnumber\tkind\twhat\n";
        let rows = parse_judged(&format!(
            "{header}a.c\tF\t0x169\tunsettled\ta flag nothing reads; a reader would settle it\n"
        ))
        .unwrap();
        assert_eq!(
            (
                rows[0].routine.as_str(),
                rows[0].number.as_str(),
                rows[0].kind.as_str()
            ),
            ("F", "0x169", "unsettled")
        );
        assert_eq!(
            parse_judged(&format!("{header}a.c\tF\t16\tnamed\tx\n")).unwrap_err(),
            "row 1: kind named, not one of honest, unsettled"
        );
        assert_eq!(
            parse_judged(&format!("{header}a.c\tF\t16\thonest\t \n")).unwrap_err(),
            "row 1: an empty cell"
        );
    }
}
