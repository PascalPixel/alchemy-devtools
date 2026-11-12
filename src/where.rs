//! where: the bytes each edition still places from `recon/`, by object and
//! section, as code or data; `--done` gives DONE from the same maps for a
//! tree in progress.

use crate::alchemy::built::{self, Piece};
use crate::alchemy::done::{self, commas, Game};
use crate::alchemy::targets::DecompTargetId;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const USAGE: &str = "usage: alchemy-devtools where [EDITION|GAME...] [--top N]
       alchemy-devtools where --done [EDITION|GAME...]
Reads each edition's main and overlay linker maps under out/ (make
compare-editions builds them) and sums the bytes still placed from recon/, by
folder and by piece: an object, with its section unless that is .text. Code
and data are as C2 and --done count them: a .text section is code, every
other byte data; of the data, a scaffold's .rom.* sections are marked
scaffold, code and data not yet told apart.
--top N lists the N largest pieces of each edition (25 by default). No
edition names all twelve; a game names its six.
--done counts DONE code and data as calcrom does (C2), from the maps alone
and whatever each ROM is: analysis of a tree in progress, never a
measurement. alchemy-devtools land measures the tree and writes the receipt.";

/// What a piece's bytes count as under C2, with a scaffold's sections, data
/// to C2, marked apart.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Kind {
    Code,
    Data,
    Scaffold,
}

impl Kind {
    fn of(piece: &Piece) -> Kind {
        match (piece.is_code(), piece.is_scaffold()) {
            (true, _) => Kind::Code,
            (false, true) => Kind::Scaffold,
            (false, false) => Kind::Data,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Kind::Code => "code",
            Kind::Data => "data",
            Kind::Scaffold => "scaffold",
        }
    }
}

/// The bytes still placed from `recon/`, by piece and by folder.
#[derive(Default)]
struct Left {
    /// (kind, label) → bytes
    pieces: BTreeMap<(Kind, String), u64>,
    /// (kind, folder) → bytes, and the objects that place them
    folders: BTreeMap<(Kind, String), (u64, BTreeSet<String>)>,
}

impl Left {
    fn of(pieces: &[Piece]) -> Left {
        let mut left = Left::default();
        for piece in pieces.iter().filter(|piece| piece.stem.starts_with("recon/")) {
            let size = u64::from(piece.placed.size);
            let kind = Kind::of(piece);
            *left.pieces.entry((kind, piece.label())).or_default() += size;
            let folder = piece.stem.rsplit_once('/').map_or("", |(folder, _)| folder).to_string();
            let entry = left.folders.entry((kind, folder)).or_default();
            entry.0 += size;
            entry.1.insert(piece.stem.clone());
        }
        left
    }

    fn total(&self, kind: Kind) -> u64 {
        self.pieces.iter().filter(|((k, _), _)| *k == kind).map(|(_, size)| size).sum()
    }

    fn report(&self, edition: DecompTargetId, top: usize) -> String {
        let (code, scaffold) = (self.total(Kind::Code), self.total(Kind::Scaffold));
        let data = self.total(Kind::Data) + scaffold;
        if code + data == 0 {
            return format!("{edition}: nothing placed from recon/\n");
        }
        let width = commas((code + data) as i64).len();
        let mut text = format!(
            "{edition}: {} code and {} data bytes still placed from recon/, in {} pieces",
            commas(code as i64),
            commas(data as i64),
            self.pieces.len()
        );
        if scaffold > 0 {
            text += &format!("; {} of the data in scaffold sections, code and data not yet told apart", commas(scaffold as i64));
        }
        text.push('\n');
        let mut folders: Vec<_> = self.folders.iter().collect();
        folders.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(b.0)));
        for ((kind, folder), (size, objects)) in folders {
            let label = match objects.len() {
                1 => objects.first().unwrap().clone(),
                n => format!("{folder}/* ({n} objects)"),
            };
            text += &format!("  {:>width$}  {:<8}  {label}\n", commas(*size as i64), kind.name());
        }
        if top > 0 {
            let mut pieces: Vec<_> = self.pieces.iter().collect();
            pieces.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            text += &format!("  largest {}:\n", top.min(pieces.len()));
            for ((kind, label), size) in pieces.into_iter().take(top) {
                text += &format!("  {:>width$}  {:<8}  {label}\n", commas(*size as i64), kind.name());
            }
        }
        text
    }
}

fn number(value: Option<&String>, flag: &str) -> Result<usize, String> {
    value.and_then(|value| value.parse().ok()).ok_or_else(|| format!("{flag} takes a number\n{USAGE}"))
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let (mut words, mut top, mut done) = (Vec::new(), 25, false);
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            "--top" => top = number(arguments.next(), "--top")?,
            "--done" => done = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}\n{USAGE}")),
            word => words.push(word.to_string()),
        }
    }
    let root = crate::compile::root();
    let editions = built::editions(&words)?;
    if done {
        print!("{}", done_report(root, &editions));
        return Ok(());
    }
    for edition in editions {
        match built::pieces(root, edition) {
            Ok(pieces) => print!("{}", Left::of(&pieces).report(edition, top)),
            Err(error) => println!("{edition}: not built ({error}); make compare-{edition}"),
        }
    }
    Ok(())
}

/// DONE for each game the editions belong to, every edition counted from its
/// maps by calcrom's own rule, and each ROM's standing against rom.sha1.
fn done_report(root: &Path, editions: &[DecompTargetId]) -> String {
    let mut games: Vec<&str> = Vec::new();
    for &id in editions {
        if !games.contains(&id.game()) {
            games.push(id.game());
        }
    }
    let mut text = String::from("DONE from the maps, analysis only: alchemy-devtools land measures the tree and writes the receipt.\n");
    for game in games {
        match done::count_game(root, game, done::count_edition) {
            Ok(counted) => text += &done_lines(root, game, &counted),
            Err(error) => text += &format!("{game}: pending ({error})\n"),
        }
    }
    text
}

fn done_lines(root: &Path, game: &str, counted: &Game) -> String {
    let all = counted.combined();
    let mut text = format!(
        "{game}: code {:.2}% ({} / {} bytes) · data {:.2}% ({} / {} bytes), six editions\n",
        all.percent(),
        commas(all.bytes()),
        commas(all.executable),
        all.data_percent(),
        commas(all.data),
        commas(all.data_total),
    );
    for (language, edition) in &counted.editions {
        let d = edition.done;
        let rom = match done::verified(root, &format!("{game}-{language}")) {
            Ok(()) => "ROM matches rom.sha1".to_string(),
            Err(error) => error,
        };
        text += &format!(
            "  {language} code {:.2}% ({} / {}) · data {:.2}% ({} / {}) · {rom}\n",
            d.percent(),
            commas(d.bytes()),
            commas(d.executable),
            d.data_percent(),
            commas(d.data),
            commas(d.data_total),
        );
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gba::ld_map::Placed;

    fn id(name: &str) -> DecompTargetId {
        crate::alchemy::targets::parse_decomp_target(name).unwrap()
    }

    fn piece(stem: &str, name: &str, size: u32) -> Piece {
        Piece {
            stem: stem.to_string(),
            placed: Placed {
                name: name.to_string(),
                path: String::new(),
                address: 0x0800_0000,
                size,
                load: 0x0800_0000,
                loaded: true,
                symbols: Vec::new(),
                output: None,
            },
        }
    }

    #[test]
    fn pieces_from_recon_sum_by_folder_and_kind() {
        let pieces = [
            piece("recon/tla/raw/08000100", ".text", 0x100),
            piece("recon/tla/raw/08000200", ".text", 0x40),
            piece("recon/tla/ja/rom", ".rom.00000300", 0x20),
            piece("recon/tla/ja/rom", ".rom.00000400", 0x10),
            piece("recon/tla/unidentified", ".unidentified.08000500", 6),
            piece("games/tla/src/main", ".text", 0x1000),
        ];
        let left = Left::of(&pieces);
        assert_eq!((left.total(Kind::Code), left.total(Kind::Scaffold), left.total(Kind::Data)), (0x140, 0x30, 6));
        let report = left.report(id("tla-ja"), 2);
        let head = "tla-ja: 320 code and 54 data bytes still placed from recon/, in 5 pieces; 48 of the data in scaffold sections, code and data not yet told apart\n";
        assert!(report.starts_with(head), "{report}");
        assert!(report.contains("  320  code      recon/tla/raw/* (2 objects)\n"), "{report}");
        assert!(report.contains("   48  scaffold  recon/tla/ja/rom\n"), "{report}");
        assert!(report.contains("    6  data      recon/tla/unidentified\n"), "{report}");
        assert!(report.contains("  largest 2:\n  256  code      recon/tla/raw/08000100\n   64  code      recon/tla/raw/08000200\n"), "{report}");
        assert_eq!(Left::of(&pieces[5..]).report(id("tbs-en"), 25), "tbs-en: nothing placed from recon/\n");
    }

    /// In every edition built, the bytes `where` finds left are those
    /// calcrom counts not done: its data with the scaffold sections, and in an
    /// edition whose code total is its own, its code.
    #[test]
    fn what_is_left_is_what_calcrom_counts_not_done() {
        let root = crate::compile::root();
        for edition in built::editions(&[]).unwrap() {
            let Ok(pieces) = built::pieces(root, edition) else { continue };
            let left = Left::of(&pieces);
            let done = done::count_edition(root, edition.as_str()).unwrap().done;
            let not_done = (done.executable - done.bytes()) + (done.data_total - done.data);
            assert_eq!((left.total(Kind::Code) + left.total(Kind::Scaffold) + left.total(Kind::Data)) as i64, not_done, "{edition}");
            assert_eq!((left.total(Kind::Scaffold) + left.total(Kind::Data)) as i64, done.data_total - done.data, "{edition}");
        }
    }
}
