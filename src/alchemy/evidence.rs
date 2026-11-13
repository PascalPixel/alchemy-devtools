//! The evidence kept here rather than in Alchemy's source (H8): what each
//! name was taken from, `evidence/<game>-names.tsv` (name, kind of evidence,
//! the evidence, the file the name is declared in), and each fake match's and
//! leftover's measurement, `evidence/<game>-fakematch.tsv` (file, routine,
//! the tag's word and kind, the tag's text after them). Both are compiled in
//! from this repository's `evidence/` folder by `build.rs`, so a program
//! reads the evidence of its own revision; a changed row needs a rebuild.

mod tables {
    include!(concat!(env!("OUT_DIR"), "/evidence.rs"));
}

/// A table of `evidence/` by its file name (`tbs-names.tsv`), or `None`
/// for a game not yet converted.
pub fn table(name: &str) -> Option<&'static str> {
    tables::TABLES.iter().find(|(file, _)| *file == name).map(|(_, text)| *text)
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
            return Err(format!("line {}: {} cells, not {columns}", at + 1, cells.len()));
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
        .map(|cells| Name { name: cells[0].to_string(), kind: cells[1].to_string(), evidence: cells[2].to_string(), file: cells[3].to_string() })
        .collect())
}

/// A game's names table (`tbs`), empty for a game that has none yet.
pub fn names(game: &str) -> Result<Vec<Name>, String> {
    table(&format!("{game}-names.tsv")).map_or(Ok(Vec::new()), |text| parse_names(text).map_err(|error| format!("evidence/{game}-names.tsv: {error}")))
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
    /// (`MapGrid_CopyCellPictureIndicesRect and its siblings`).
    pub fn is_for(&self, routine: &str) -> bool {
        self.routine == routine || self.routine.strip_prefix(routine).is_some_and(|rest| rest.starts_with(' '))
    }
}

/// The rows of a fake-match table's text.
pub fn parse_measurements(text: &str) -> Result<Vec<Measurement>, String> {
    Ok(rows(text, 4)?
        .into_iter()
        .map(|cells| Measurement { file: cells[0].to_string(), routine: cells[1].to_string(), kind: cells[2].to_string(), text: cells[3].to_string() })
        .collect())
}

/// A game's fake-match table, empty for a game that has none yet.
pub fn measurements(game: &str) -> Result<Vec<Measurement>, String> {
    table(&format!("{game}-fakematch.tsv"))
        .map_or(Ok(Vec::new()), |text| parse_measurements(text).map_err(|error| format!("evidence/{game}-fakematch.tsv: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_are_compiled_in_and_read_whole() {
        let rows = names("tbs").unwrap();
        assert!(rows.len() > 4000, "{}", rows.len());
        assert!(rows.iter().all(|row| row.file.starts_with("games/tbs/") && !row.name.is_empty()));
        let measured = measurements("tbs").unwrap();
        assert!(measured.len() > 200, "{}", measured.len());
        assert!(measured.iter().all(|row| row.file.starts_with("games/tbs/") && (row.kind.starts_with("FAKEMATCH(") || row.kind.starts_with("LEFTOVER("))));
        // A game not yet converted has no table, and so no rows.
        assert!(table("xyz-names.tsv").is_none() && names("xyz").unwrap().is_empty() && measurements("xyz").unwrap().is_empty());
    }

    #[test]
    fn rows_and_their_lines() {
        let text = "name\tkind\tevidence\tfile\nABILITY_ATTACK\tthe game\t\"Attack\" (MsgAbilityName)\tgames/tbs/include/ability_ids.h\n\n";
        let rows = parse_names(text).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].line(), "★ Named from the game: \"Attack\" (MsgAbilityName)");
        assert_eq!(parse_names("header\nA\tB\tC\n").unwrap_err(), "line 2: 3 cells, not 4");
        let rows = parse_measurements("file\troutine\tkind\tmeasurement\na.c\tF and its siblings\tFAKEMATCH(dead)\tkept; as none, 1 of 2 instructions differ in tbs-en.\n").unwrap();
        assert_eq!(rows[0].tag(), "FAKEMATCH(dead): kept; as none, 1 of 2 instructions differ in tbs-en.");
        assert!(rows[0].is_for("F") && !rows[0].is_for("F_2") && !rows[0].is_for("G"));
    }
}
