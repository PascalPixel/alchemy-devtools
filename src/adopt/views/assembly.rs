//! A compiler's assembly cut into what each symbol emits, so a joined file's
//! code can be held against the files it was joined from, function by
//! function: `.L` labels are numbered afresh inside each piece, the file's
//! name and the compiler's own marks dropped, and a view's suffix taken off.

use regex::Regex;
use std::sync::OnceLock;

/// What one symbol emits in one section: its directives, label and lines.
/// A piece with no name is what a section holds before any symbol (string
/// literals, for one).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Piece {
    pub section: String,
    pub name: String,
    pub lines: Vec<String>,
}

/// Directives that belong to the symbol after them.
fn leads(line: &str) -> bool {
    let word = line.split_whitespace().next().unwrap_or("");
    matches!(word, ".globl" | ".global" | ".type" | ".thumb_func" | ".align" | ".balign" | ".p2align" | ".code" | ".thumb" | ".arm" | ".weak")
}

fn section_of(line: &str) -> Option<String> {
    let mut words = line.split(|c: char| c.is_whitespace() || c == ',');
    let first = words.next()?;
    match first {
        ".text" | ".data" | ".bss" => Some(first.to_string()),
        ".section" => words.find(|word| !word.is_empty()).map(str::to_string),
        _ => None,
    }
}

/// The pieces of a file's assembly, each section's in order. `statics` are
/// the private names views gave statics, read as their own (`sTable__s0b84`
/// is `sTable`); every other name stays as written, so a view whose label
/// did not bind its symbol shows.
pub(crate) fn pieces(assembly: &str, statics: &[String]) -> Vec<Piece> {
    let assembly = unsuffixed(assembly, statics);
    let mut pieces: Vec<Piece> = Vec::new();
    let mut section = ".text".to_string();
    let mut open: Option<usize> = None;
    let mut pending: Vec<String> = Vec::new();
    for raw in assembly.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(".file") || line.starts_with(".ident") || line.starts_with('@') || line == "gcc2_compiled.:" || line == ".gcc2_compiled.:" || line.split_whitespace().eq([".code", "16"]) {
            continue;
        }
        if let Some(name) = section_of(line) {
            section = name;
            open = None;
            continue;
        }
        if let Some(name) = line.strip_suffix(':').filter(|name| !name.starts_with(".L") && !name.contains(char::is_whitespace)) {
            pieces.push(Piece { section: section.clone(), name: name.to_string(), lines: std::mem::take(&mut pending) });
            open = Some(pieces.len() - 1);
            continue;
        }
        let common = line.split_whitespace().next().filter(|word| *word == ".comm" || *word == ".lcomm");
        if let Some(word) = common {
            let name = line[word.len()..].trim().split(',').next().unwrap_or("").trim().to_string();
            pieces.push(Piece { section: word.to_string(), name, lines: vec![line.to_string()] });
            continue;
        }
        if leads(line) {
            pending.push(line.to_string());
            continue;
        }
        let index = match open {
            Some(index) => index,
            None => {
                pieces.push(Piece { section: section.clone(), name: String::new(), lines: Vec::new() });
                pieces.len() - 1
            }
        };
        open = Some(index);
        pieces[index].lines.append(&mut pending);
        pieces[index].lines.push(line.to_string());
    }
    if let (Some(index), false) = (open, pending.is_empty()) {
        pieces[index].lines.append(&mut pending);
    }
    for piece in &mut pieces {
        renumber(piece);
    }
    pieces
}

fn unsuffixed(assembly: &str, statics: &[String]) -> String {
    if statics.is_empty() {
        return assembly.to_string();
    }
    let alternatives: Vec<String> = statics.iter().map(|name| regex::escape(name)).collect();
    let pattern = Regex::new(&format!(r"\b(?:{})\b", alternatives.join("|"))).unwrap();
    pattern.replace_all(assembly, |found: &regex::Captures| found[0].rsplit_once("__").map_or(found[0].to_string(), |(name, _)| name.to_string())).into_owned()
}

/// Its `.L` labels numbered in the order they first appear.
fn renumber(piece: &mut Piece) {
    static LABEL: OnceLock<Regex> = OnceLock::new();
    let label = LABEL.get_or_init(|| Regex::new(r"\.L[A-Za-z]*[0-9]+").unwrap());
    let mut seen: Vec<String> = Vec::new();
    for line in &mut piece.lines {
        *line = label
            .replace_all(line, |found: &regex::Captures| {
                let text = found[0].to_string();
                let index = seen.iter().position(|known| *known == text).unwrap_or_else(|| {
                    seen.push(text.clone());
                    seen.len() - 1
                });
                format!(".L#{index}")
            })
            .into_owned();
    }
}

/// Where a joined file's code differs from its parts': each difference the
/// name of what moved and the index of the part it came from.
pub(crate) fn moved(parts: &[Vec<Piece>], joined: &[Piece]) -> Vec<(usize, String)> {
    let mut sections: Vec<&str> = Vec::new();
    for piece in parts.iter().flatten().chain(joined) {
        if !sections.contains(&piece.section.as_str()) {
            sections.push(&piece.section);
        }
    }
    let mut found = Vec::new();
    for section in sections {
        let theirs: Vec<(usize, &Piece)> = parts.iter().enumerate().flat_map(|(part, pieces)| pieces.iter().map(move |piece| (part, piece))).filter(|(_, piece)| piece.section == section).collect();
        let ours: Vec<&Piece> = joined.iter().filter(|piece| piece.section == section).collect();
        if section == ".comm" {
            // Common symbols merge in the link: one name is one symbol.
            let mut names: Vec<&str> = theirs.iter().map(|(_, piece)| piece.name.as_str()).collect();
            names.dedup();
            names.sort_unstable();
            names.dedup();
            let mut joined_names: Vec<&str> = ours.iter().map(|piece| piece.name.as_str()).collect();
            joined_names.sort_unstable();
            joined_names.dedup();
            if let Some(name) = names.iter().find(|name| !joined_names.contains(name)) {
                let part = theirs.iter().find(|(_, piece)| piece.name == *name).map_or(0, |(part, _)| *part);
                found.push((part, name.to_string()));
            }
            continue;
        }
        let before = found.len();
        for (at, (part, piece)) in theirs.iter().enumerate() {
            match ours.get(at) {
                Some(other) if other.name == piece.name => {
                    if other.lines != piece.lines {
                        found.push((*part, describe(piece)));
                    }
                }
                _ => {
                    // Out of step from here on: the first is what moved.
                    found.push((*part, describe(piece)));
                    break;
                }
            }
        }
        if found.len() == before && ours.len() > theirs.len() {
            let extra = ours[theirs.len()];
            let part = theirs.last().map_or(0, |(part, _)| *part);
            found.push((part, format!("{} (new)", describe(extra))));
        }
    }
    found
}

fn describe(piece: &Piece) -> String {
    match piece.name.is_empty() {
        true => format!("the unnamed {} data", piece.section),
        false => piece.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = "\t.file\t\"a.c\"\n\t.code\t16\n.text\n\t.align\t2\n\t.globl\tOne\n\t.type\t One,function\n\t.thumb_func\nOne:\n\tpush\t{lr}\n\tbl\tHelper\n\tb\t.L3\n.L3:\n\tpop\t{pc}\n.Lfe1:\n\t.size\t One,.Lfe1-One\n";
    const TWO: &str = "\t.file\t\"b.c\"\n.text\n\t.align\t2\n\t.thumb_func\nsTwo:\n\tbl\tHelper\n.L7:\n\tb\t.L7\n.Lfe1:\n\t.size\t sTwo,.Lfe1-sTwo\n\t.comm\tgShared,4,4\n";

    #[test]
    fn assembly_cuts_into_symbols_with_their_labels_renumbered() {
        let one = pieces(ONE, &[]);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].name, "One");
        assert!(one[0].lines.contains(&".L#0:".to_string()) && one[0].lines.iter().any(|line| line.contains(".L#1-One")));
        let two = pieces(TWO, &[]);
        assert_eq!(two.iter().map(|p| (p.section.as_str(), p.name.as_str())).collect::<Vec<_>>(), [(".text", "sTwo"), (".comm", "gShared")]);
    }

    #[test]
    fn a_join_differs_where_a_symbol_moved() {
        let parts = vec![pieces(ONE, &[]), pieces(TWO, &[])];
        let joined = format!("{ONE}{}", TWO.replace("sTwo", "sTwo__b").replace(".L7", ".L9").replace(".Lfe1", ".Lfe2"));
        assert!(moved(&parts, &pieces(&joined, &["sTwo__b".into()])).is_empty());
        let changed = joined.replace("bl\tHelper\n.L9", "bl\tOther\n.L9");
        assert_eq!(moved(&parts, &pieces(&changed, &["sTwo__b".into()])), [(1, "sTwo".to_string())]);
        // An alias's private name is never read as the symbol: one its label
        // failed to bind shows.
        let unbound = joined.replace("bl\tHelper\n.L9", "bl\tHelper__b\n.L9");
        assert_eq!(moved(&parts, &pieces(&unbound, &["sTwo__b".into()])), [(1, "sTwo".to_string())]);
        let lost = ONE.to_string();
        assert_eq!(moved(&parts, &pieces(&lost, &[])), [(1, "sTwo".to_string()), (1, "gShared".to_string())]);
    }
}
