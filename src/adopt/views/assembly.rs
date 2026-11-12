//! A compiler's assembly cut into what each symbol emits, so a joined file's
//! code can be held against the files it was joined from, function by
//! function: `.L` labels are numbered afresh inside each piece, the file's
//! name and the compiler's own marks dropped. The joined assembly is never
//! rewritten: where a view gave a part's static a private name, the part's
//! own assembly is written with that name (`renamed`), so a use the join
//! bound to another part's static of the same name shows as a difference.

use regex::Regex;
use std::collections::BTreeMap;
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

/// The pieces of a file's assembly, each section's in order, every name as
/// written, so a view whose label did not bind its symbol shows.
pub(crate) fn pieces(assembly: &str) -> Vec<Piece> {
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

/// One file's assembly with each symbol `from` written `to`: a part's own
/// assembly as its object would read had its statics carried their views'
/// private names (`sTable` as `sTable__s0b84`), or a joined object's as it
/// reads once a view's private name is the plain one again.
pub(crate) fn renamed(assembly: &str, renames: &[(String, String)]) -> String {
    if renames.is_empty() {
        return assembly.to_string();
    }
    let map: std::collections::BTreeMap<&str, &str> = renames.iter().map(|(from, to)| (from.as_str(), to.as_str())).collect();
    let alternatives: Vec<String> = map.keys().map(|name| regex::escape(name)).collect();
    // A symbol is the name with nothing of a name either side: `sTable`
    // in `sTable.0` (a local static's), never in `sTable__b` or `.LsTable`.
    let pattern = Regex::new(&format!(r"(^|[^A-Za-z0-9_$.])({})($|[^A-Za-z0-9_$])", alternatives.join("|"))).unwrap();
    let mut text = assembly.to_string();
    // Twice, as two uses can share the one character between them.
    for _ in 0..2 {
        text = pattern.replace_all(&text, |found: &regex::Captures| format!("{}{}{}", &found[1], map[&found[2]], &found[3])).into_owned();
    }
    text
}

/// A common symbol's line read: its size and alignment.
fn common_size(line: &str) -> (u64, u64) {
    let mut fields = line.split(',').skip(1).map(|field| field.trim().parse::<u64>().unwrap_or(0));
    (fields.next().unwrap_or(0), fields.next().unwrap_or(0))
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
            // Common symbols merge in the link, one name one symbol of the
            // largest size and alignment any file gave it: the join must give
            // each name that size and alignment, no more and no less.
            let mut wanted: BTreeMap<&str, (usize, (u64, u64))> = BTreeMap::new();
            for (part, piece) in &theirs {
                let (size, align) = common_size(&piece.lines[0]);
                let entry = wanted.entry(piece.name.as_str()).or_insert((*part, (0, 0)));
                if size > entry.1 .0 {
                    entry.0 = *part;
                }
                entry.1 = (entry.1 .0.max(size), entry.1 .1.max(align));
            }
            let mut joined: BTreeMap<&str, (u64, u64)> = BTreeMap::new();
            for piece in &ours {
                let (size, align) = common_size(&piece.lines[0]);
                let entry = joined.entry(piece.name.as_str()).or_insert((0, 0));
                *entry = (entry.0.max(size), entry.1.max(align));
            }
            for (name, (part, size)) in &wanted {
                match joined.get(name) {
                    None => found.push((*part, name.to_string())),
                    Some(now) if now != size => found.push((*part, format!("{name} (common {},{} where its files had {},{})", now.0, now.1, size.0, size.1))),
                    Some(_) => {}
                }
            }
            for name in joined.keys().filter(|name| !wanted.contains_key(*name)) {
                found.push((theirs.last().map_or(0, |(part, _)| *part), format!("{name} (new)")));
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

    fn suffixed(assembly: &str, names: &[(&str, &str)]) -> String {
        renamed(assembly, &names.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>())
    }

    #[test]
    fn assembly_cuts_into_symbols_with_their_labels_renumbered() {
        let one = pieces(ONE);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].name, "One");
        assert!(one[0].lines.contains(&".L#0:".to_string()) && one[0].lines.iter().any(|line| line.contains(".L#1-One")));
        let two = pieces(TWO);
        assert_eq!(two.iter().map(|p| (p.section.as_str(), p.name.as_str())).collect::<Vec<_>>(), [(".text", "sTwo"), (".comm", "gShared")]);
    }

    #[test]
    fn a_join_differs_where_a_symbol_moved() {
        // b's static took the private name sTwo__b: b's own assembly is read with it.
        let parts = vec![pieces(ONE), pieces(&suffixed(TWO, &[("sTwo", "sTwo__b")]))];
        let joined = format!("{ONE}{}", TWO.replace("sTwo", "sTwo__b").replace(".L7", ".L9").replace(".Lfe1", ".Lfe2"));
        assert!(moved(&parts, &pieces(&joined)).is_empty());
        let changed = joined.replace("bl\tHelper\n.L9", "bl\tOther\n.L9");
        assert_eq!(moved(&parts, &pieces(&changed)), [(1, "sTwo__b".to_string())]);
        // An alias's private name is never read as the symbol: one its label
        // failed to bind shows.
        let unbound = joined.replace("bl\tHelper\n.L9", "bl\tHelper__b\n.L9");
        assert_eq!(moved(&parts, &pieces(&unbound)), [(1, "sTwo__b".to_string())]);
        let lost = ONE.to_string();
        assert_eq!(moved(&parts, &pieces(&lost)), [(1, "sTwo__b".to_string()), (1, "gShared".to_string())]);
    }

    /// Two parts each with a static sFoo; b's became sFoo__b. A use in b that
    /// still spells sFoo binds to a's: the joined assembly is not rewritten,
    /// so the wrong binding shows (before, both read sFoo and passed).
    #[test]
    fn a_use_bound_to_another_part_s_static_shows() {
        let a = ".text\n\t.align\t2\n\t.thumb_func\nsFoo:\n\tbx\tlr\n\t.thumb_func\nA:\n\tbl\tsFoo\n";
        let b = ".text\n\t.align\t2\n\t.thumb_func\nsFoo:\n\tmov\tr0, #1\n\tbx\tlr\n\t.thumb_func\nB:\n\tbl\tsFoo\n";
        let parts = vec![pieces(a), pieces(&suffixed(b, &[("sFoo", "sFoo__b")]))];
        let right = format!("{a}{}", b.replace("sFoo", "sFoo__b"));
        assert!(moved(&parts, &pieces(&right)).is_empty());
        let wrong = format!("{a}{}", b.replacen("sFoo:", "sFoo__b:", 1));
        assert_eq!(moved(&parts, &pieces(&wrong)), [(1, "B".to_string())]);
    }

    #[test]
    fn a_rename_reaches_only_whole_symbols() {
        let text = "\tbl\tsFoo\n\t.word\tsFoo+4\nsFoo.0:\n\t.word\tsFoo__c\n\t.word\t.LsFoo\n\tldr\tr0, =sFoo\nsFoo:\n";
        assert_eq!(suffixed(text, &[("sFoo", "sFoo__b")]), "\tbl\tsFoo__b\n\t.word\tsFoo__b+4\nsFoo__b.0:\n\t.word\tsFoo__c\n\t.word\t.LsFoo\n\tldr\tr0, =sFoo__b\nsFoo__b:\n");
        assert_eq!(suffixed("\t.word\tsA,sA\n", &[("sA", "sA__b")]), "\t.word\tsA__b,sA__b\n");
    }

    /// int gY[2] in one part, int gY in another: the link took 8 bytes. A join
    /// that leaves only the 4-byte one differs, though the name is there.
    #[test]
    fn a_common_symbol_keeps_its_largest_size_and_alignment() {
        let parts = vec![pieces("\t.comm\tgY,8,4\n"), pieces("\t.comm\tgY,4,4\n")];
        assert!(moved(&parts, &pieces("\t.comm\tgY,8,4\n")).is_empty());
        assert_eq!(moved(&parts, &pieces("\t.comm\tgY,4,4\n")), [(0, "gY (common 4,4 where its files had 8,4)".to_string())]);
        assert_eq!(moved(&parts, &pieces("\t.comm\tgY,8,8\n")).len(), 1);
        assert_eq!(moved(&parts, &pieces("\t.comm\tgY,8,4\n\t.comm\tgZ,4,4\n")), [(1, "gZ (new)".to_string())]);
        assert_eq!(moved(&parts, &pieces("")), [(0, "gY".to_string())]);
    }
}
