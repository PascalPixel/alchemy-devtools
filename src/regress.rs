//! regress: the functions whose assembly differs between a saved build of the
//! tree and the current one, each with its class and the instructions that
//! differ: the evidence a compiler, option or routing change needs (K3).

use crate::alchemy::built;
use crate::alchemy::targets::DecompTargetId;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: alchemy-devtools regress REFERENCE [SOURCE...] [--edition EDITION|GAME]... [--show]
Compares the assembly the compiler wrote for each C source under games/
(out/<edition>/obj/<source>.s) in REFERENCE, a saved copy of out/, with the
current out/, edition by edition and function by function. Build first: the
current tree's .s are the build's own (make compare-editions). Each function
that differs is classed, worst first: instruction (an instruction added,
dropped or changed), directive (the same instructions, other directives:
alignment, sections), order+register, register-only, order-only. The count
is the instructions that differ by longest common subsequence. No SOURCE
compares every source REFERENCE holds; no edition, every edition both trees
hold. --show prints each differing function's lines from the first edition
it differs in.";

/// The classes, mildest first.
const CLASSES: [&str; 5] = ["order-only", "register-only", "order+register", "directive", "instruction"];

/// One function's assembly: its lines with local labels numbered within it,
/// and the instructions and data words among them.
#[derive(Debug, Default, PartialEq, Eq)]
struct Function {
    lines: Vec<String>,
    instructions: Vec<String>,
}

/// A compiled file: its functions by name, and every line outside them.
#[derive(Debug, Default, PartialEq, Eq)]
struct Assembly {
    functions: BTreeMap<String, Function>,
    rest: Vec<String>,
}

fn is_label_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '$'
}

/// `.L13`, `.LCB7`, `.LC2` and `.Lfe1` renumbered in order of first use, so two compilations
/// compare by shape and not by the compiler's counter.
fn renumber(line: &str, names: &mut BTreeMap<String, usize>) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find(".L") {
        let (before, after) = rest.split_at(at);
        let start_ok = !before.chars().next_back().is_some_and(is_label_char);
        let tail = &after[2..];
        let digits = ["fe", "CB", "C"].iter().find_map(|prefix| tail.strip_prefix(prefix)).unwrap_or(tail);
        let numbered = digits.chars().take_while(char::is_ascii_digit).count();
        out += before;
        if start_ok && numbered > 0 {
            let label = &after[..2 + (tail.len() - digits.len()) + numbered];
            let count = names.len();
            let number = *names.entry(label.to_string()).or_insert(count);
            out += &format!(".L#{number}");
            rest = &after[label.len()..];
        } else {
            out += ".L";
            rest = tail;
        }
    }
    out + rest
}

fn parse(text: &str) -> Assembly {
    let lines: Vec<&str> = text.lines().filter(|line| !line.starts_with('@') && !line.contains(".file\t")).collect();
    let mut assembly = Assembly::default();
    let mut current: Option<String> = None;
    let mut names = BTreeMap::new();
    for (at, line) in lines.iter().enumerate() {
        let label = line.split_once(':').map(|(label, _)| label).filter(|label| {
            !label.is_empty() && !label.starts_with(['\t', ' ']) && label.chars().all(is_label_char)
        });
        let declared = label.is_some_and(|label| {
            lines[at.saturating_sub(3)..at].iter().any(|before| {
                before.contains(".type") && before.contains("function") && before.split([' ', '\t', ',']).any(|word| word == label)
            })
        });
        if let (Some(label), true) = (label, declared) {
            current = Some(label.to_string());
            names.clear();
            assembly.functions.insert(label.to_string(), Function::default());
            continue;
        }
        let Some(name) = current.clone() else {
            assembly.rest.push(renumber(line, &mut names));
            continue;
        };
        if line.contains(".size") {
            current = None;
            continue;
        }
        if line.starts_with(".Lfe") {
            continue;
        }
        let line = renumber(line, &mut names);
        let function = assembly.functions.get_mut(&name).unwrap();
        let directive = line.strip_prefix('\t').is_some_and(|body| body.starts_with('.'));
        let data = ["\t.word", "\t.short", "\t.hword", "\t.byte"].iter().any(|prefix| line.starts_with(prefix));
        if line.starts_with('\t') && (!directive || data) {
            function.instructions.push(line.clone());
        }
        function.lines.push(line);
    }
    assembly
}

/// Every register name made `R`, as a class that ignores allocation reads it.
fn without_registers(line: &str) -> String {
    fn flush(word: &mut String, out: &mut String) {
        let register = matches!(word.as_str(), "ip" | "lr" | "sl" | "fp")
            || (word.len() > 1 && word.starts_with('r') && word[1..].chars().all(|c| c.is_ascii_digit()));
        out.push_str(if register { "R" } else { word });
        word.clear();
    }
    let (mut out, mut word) = (String::with_capacity(line.len()), String::new());
    for c in line.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

fn without_labels(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(at) = rest.find(".L#") {
        out += &rest[..at + 2];
        rest = rest[at + 3..].trim_start_matches(|c: char| c.is_ascii_digit());
    }
    out + rest
}

fn sorted(lines: impl Iterator<Item = String>) -> Vec<String> {
    let mut lines: Vec<String> = lines.collect();
    lines.sort();
    lines
}

/// The mildest class that explains how `current` differs from `reference`,
/// or none when they are the same.
fn classify(reference: &Function, current: &Function) -> Option<&'static str> {
    if reference.lines == current.lines {
        return None;
    }
    if reference.instructions == current.instructions {
        return Some("directive");
    }
    let plain = |f: &Function| f.lines.iter().map(|line| without_registers(line)).collect::<Vec<_>>();
    if plain(reference) == plain(current) {
        return Some("register-only");
    }
    let bag = |f: &Function| sorted(f.instructions.iter().map(|line| without_labels(line)));
    if bag(reference) == bag(current) {
        return Some("order-only");
    }
    let loose = |f: &Function| sorted(f.instructions.iter().map(|line| without_registers(&without_labels(line))));
    if loose(reference) == loose(current) {
        return Some("order+register");
    }
    Some("instruction")
}

fn lcs(a: &[String], b: &[String]) -> usize {
    let mut previous = vec![0usize; b.len() + 1];
    for x in a {
        let mut row = vec![0usize; b.len() + 1];
        for (j, y) in b.iter().enumerate() {
            row[j + 1] = if x == y { previous[j] + 1 } else { previous[j + 1].max(row[j]) };
        }
        previous = row;
    }
    previous[b.len()]
}

/// The instructions that differ: the longer side's count less what both keep in order.
fn differing(reference: &[String], current: &[String]) -> usize {
    reference.len().max(current.len()) - lcs(reference, current)
}

/// A line diff of two functions, `-` for the reference and `+` for the
/// current tree, with two lines of context around each change.
fn show(reference: &[String], current: &[String]) -> String {
    let (n, m) = (reference.len(), current.len());
    let mut table = vec![0u32; (n + 1) * (m + 1)];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i * (m + 1) + j] = if reference[i] == current[j] {
                table[(i + 1) * (m + 1) + j + 1] + 1
            } else {
                table[(i + 1) * (m + 1) + j].max(table[i * (m + 1) + j + 1])
            };
        }
    }
    let mut steps: Vec<(char, &str)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && reference[i] == current[j] {
            steps.push((' ', &reference[i]));
            i += 1;
            j += 1;
        } else if i < n && (j == m || table[(i + 1) * (m + 1) + j] >= table[i * (m + 1) + j + 1]) {
            steps.push(('-', &reference[i]));
            i += 1;
        } else {
            steps.push(('+', &current[j]));
            j += 1;
        }
    }
    let near = |at: usize| steps[at.saturating_sub(2)..(at + 3).min(steps.len())].iter().any(|(mark, _)| *mark != ' ');
    let mut text = String::new();
    let mut skipped = false;
    for (at, (mark, line)) in steps.iter().enumerate() {
        if near(at) {
            if skipped {
                text += "    ...\n";
                skipped = false;
            }
            text += &format!("    {mark}{}\n", line.trim_start_matches('\t').replace('\t', " "));
        } else {
            skipped = true;
        }
    }
    text
}

/// One source's differences over every edition compared.
#[derive(Default)]
struct Source {
    /// function → (class, instructions differing, instructions in the reference)
    functions: BTreeMap<String, (&'static str, usize, usize)>,
    editions: Vec<DecompTargetId>,
    rest: bool,
    /// (function, its lines shown) from the first edition each differs in
    shown: Vec<(String, String)>,
}

impl Source {
    fn worst(&self) -> &'static str {
        let rank = |class: &str| CLASSES.iter().position(|c| *c == class).unwrap_or(0);
        self.functions.values().map(|(class, _, _)| *class).max_by_key(|class| rank(class)).unwrap_or("data")
    }

    fn differing(&self) -> usize {
        self.functions.values().map(|(_, count, _)| count).sum()
    }
}

/// Compares one source's two compilations in one edition into `source`.
fn compare(reference: &str, current: &str, edition: DecompTargetId, show_lines: bool, source: &mut Source) {
    if reference == current {
        return;
    }
    let (a, b) = (parse(reference), parse(current));
    let rank = |class: &str| CLASSES.iter().position(|c| *c == class).unwrap_or(0);
    let mut differs = false;
    let empty = Function::default();
    let names: Vec<&String> = a.functions.keys().chain(b.functions.keys().filter(|name| !a.functions.contains_key(*name))).collect();
    for name in names {
        let (fa, fb) = (a.functions.get(name), b.functions.get(name));
        let class = match (fa, fb) {
            (Some(fa), Some(fb)) => classify(fa, fb),
            _ => Some("instruction"),
        };
        let Some(class) = class else { continue };
        differs = true;
        let (fa, fb) = (fa.unwrap_or(&empty), fb.unwrap_or(&empty));
        let count = differing(&fa.instructions, &fb.instructions);
        let entry = source.functions.entry(name.clone()).or_insert((class, 0, fa.instructions.len()));
        if rank(class) > rank(entry.0) {
            entry.0 = class;
        }
        entry.1 = entry.1.max(count);
        if show_lines && !source.shown.iter().any(|(shown, _)| shown == name) {
            source.shown.push((name.clone(), format!("  {name} in {edition}, {class}:\n{}", show(&fa.lines, &fb.lines))));
        }
    }
    if a.rest != b.rest {
        source.rest = true;
        differs = true;
    }
    if differs && !source.editions.contains(&edition) {
        source.editions.push(edition);
    }
}

/// Every `.c` source under games/ whose assembly the reference holds for an edition.
fn sources_held(root: &Path, reference: &Path, editions: &[DecompTargetId]) -> Vec<String> {
    let mut found = std::collections::BTreeSet::new();
    for &id in editions {
        let games = reference.join(id.as_str()).join("obj/games");
        for entry in walkdir::WalkDir::new(&games).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "s") {
                continue;
            }
            let Ok(relative) = path.strip_prefix(reference.join(id.as_str()).join("obj")) else { continue };
            let source = relative.with_extension("c");
            if root.join(&source).is_file() {
                found.insert(source.display().to_string());
            }
        }
    }
    found.into_iter().collect()
}

fn modified(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|metadata| metadata.modified()).ok()
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let (mut positional, mut words, mut show_lines) = (Vec::new(), Vec::new(), false);
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            "--show" => show_lines = true,
            "--edition" => words.push(arguments.next().ok_or("--edition takes an edition or a game")?.clone()),
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}\n{USAGE}")),
            word => positional.push(word.to_string()),
        }
    }
    let Some((reference, sources)) = positional.split_first() else {
        return Err(format!("regress needs the saved out/ to compare with\n{USAGE}"));
    };
    let root = crate::compile::root();
    let reference = PathBuf::from(reference);
    let reference = if reference.is_absolute() { reference } else { std::env::current_dir().map_err(|e| e.to_string())?.join(reference) };
    let editions: Vec<DecompTargetId> = built::editions(&words)?
        .into_iter()
        .filter(|&id| reference.join(id.as_str()).join("obj").is_dir() && built::output(root, id).join("obj").is_dir())
        .collect();
    if editions.is_empty() {
        return Err(format!("{} and out/ hold no edition's objects in common", reference.display()));
    }
    let sources: Vec<String> = match sources {
        [] => sources_held(root, &reference, &editions),
        named => named.iter().map(|source| source.trim_start_matches("./").to_string()).collect(),
    };
    let (mut compared, mut stale) = (0, 0);
    let mut differing_sources: Vec<(String, Source)> = Vec::new();
    for name in &sources {
        let stem = name.strip_suffix(".c").unwrap_or(name);
        let mut source = Source::default();
        for &id in &editions {
            let (old, new) = (
                reference.join(id.as_str()).join("obj").join(format!("{stem}.s")),
                built::output(root, id).join("obj").join(format!("{stem}.s")),
            );
            let (Ok(old_text), new_text) = (std::fs::read_to_string(&old), std::fs::read_to_string(&new)) else {
                continue;
            };
            let Ok(new_text) = new_text else {
                println!("{name}: not built in {id}'s current tree");
                continue;
            };
            if modified(&new) < modified(&root.join(name)) {
                println!("{name}: {id}'s assembly is older than the source; rebuild before comparing");
                stale += 1;
            }
            compared += 1;
            compare(&old_text, &new_text, id, show_lines, &mut source);
        }
        if !source.editions.is_empty() {
            differing_sources.push((name.clone(), source));
        }
    }
    let mut classes: BTreeMap<&str, usize> = BTreeMap::new();
    for (name, source) in &differing_sources {
        *classes.entry(source.worst()).or_default() += 1;
        let editions: Vec<&str> = source.editions.iter().map(|id| id.as_str()).collect();
        match source.functions.is_empty() {
            true => println!("{name}: only lines outside functions differ (data or directives), in {}", editions.join(" ")),
            false => println!(
                "{name}: {}, {} instructions differ in {}{}",
                source.worst(),
                source.differing(),
                editions.join(" "),
                if source.rest { "; lines outside functions too" } else { "" }
            ),
        }
        for (function, (class, count, total)) in &source.functions {
            println!("  {function} {class} {count}/{total}");
        }
        for (_, shown) in &source.shown {
            print!("{shown}");
        }
    }
    let mut order: Vec<(&&str, &usize)> = classes.iter().collect();
    order.sort_by_key(|(class, _)| std::cmp::Reverse(CLASSES.iter().position(|c| c == *class)));
    let classes: Vec<String> = order.into_iter().map(|(class, count)| format!("{class} {count}")).collect();
    println!(
        "compared {} sources ({compared} compilations) in {} editions: {} differ{}{}",
        sources.len(),
        editions.len(),
        differing_sources.len(),
        if classes.is_empty() { String::new() } else { format!(" ({})", classes.join(", ")) },
        if stale > 0 { format!("; {stale} compilations older than their source") } else { String::new() }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> DecompTargetId {
        crate::alchemy::targets::parse_decomp_target(name).unwrap()
    }

    const REFERENCE: &str = "\
@ Generated by gcc 2.96 20000731 (experimental) for ARM/elf
\t.file\t\"x.c\"
\t.code\t16
.text
\t.align\t2
\t.global\tFirst
\t.thumb_func
\t.type\t First,function
First:
\tpush\t{r4, lr}
\tmov\tr4, r0
\tcmp\tr4, #0
\tbeq\t.L3
\tbl\tOther
.L3:
\tpop\t{r4, pc}
.Lfe1:
\t.size\t First,.Lfe1-First
";

    fn function(source: &str, name: &str) -> Function {
        parse(source).functions.remove(name).unwrap()
    }

    #[test]
    fn functions_read_with_their_labels_renumbered() {
        let assembly = parse(REFERENCE);
        let first = &assembly.functions["First"];
        assert_eq!(first.instructions.len(), 6);
        assert!(first.lines.contains(&"\tbeq\t.L#0".to_string()) && first.lines.contains(&".L#0:".to_string()));
        assert_eq!(assembly.rest, ["\t.code\t16", ".text", "\t.align\t2", "\t.global\tFirst", "\t.thumb_func", "\t.type\t First,function"]);
        assert_eq!(renumber("\tb\t.L12 @ .Lfe3 x.L5", &mut BTreeMap::new()), "\tb\t.L#0 @ .L#1 x.L5");
        assert_eq!(without_registers("\tmov\tr4, r10 @ fp lr3"), "\tmov\tR, R @ R lr3");
    }

    #[test]
    fn differences_are_classed_mildest_first() {
        let first = function(REFERENCE, "First");
        assert_eq!(classify(&first, &function(REFERENCE, "First")), None);
        let relabelled = REFERENCE.replace(".L3", ".L7");
        assert_eq!(classify(&first, &function(&relabelled, "First")), None);
        let aligned = REFERENCE.replace("\tpush\t{r4, lr}\n", "\tpush\t{r4, lr}\n\t.align\t2\n");
        assert_eq!(classify(&first, &function(&aligned, "First")), Some("directive"));
        let registers = REFERENCE.replace("r4", "r5");
        assert_eq!(classify(&first, &function(&registers, "First")), Some("register-only"));
        let order = REFERENCE.replace("\tmov\tr4, r0\n\tcmp\tr4, #0\n", "\tcmp\tr4, #0\n\tmov\tr4, r0\n");
        assert_eq!(classify(&first, &function(&order, "First")), Some("order-only"));
        let both = REFERENCE.replace("\tmov\tr4, r0\n\tcmp\tr4, #0\n", "\tcmp\tr5, #0\n\tmov\tr5, r0\n");
        assert_eq!(classify(&first, &function(&both, "First")), Some("order+register"));
        let changed = REFERENCE.replace("\tcmp\tr4, #0\n", "\tcmp\tr4, #1\n");
        let changed = function(&changed, "First");
        assert_eq!(classify(&first, &changed), Some("instruction"));
        assert_eq!(differing(&first.instructions, &changed.instructions), 1);
        let shown = show(&first.lines, &changed.lines);
        assert!(shown.contains("    -cmp r4, #0\n    +cmp r4, #1\n"), "{shown}");
    }

    #[test]
    fn a_source_sums_its_functions_over_editions() {
        let mut source = Source::default();
        let changed = REFERENCE.replace("\tcmp\tr4, #0\n", "\tcmp\tr4, #1\n\tcmp\tr4, #2\n");
        compare(REFERENCE, &changed, id("tla-en"), true, &mut source);
        compare(REFERENCE, &REFERENCE.replace("r4", "r5"), id("tla-ja"), false, &mut source);
        compare(REFERENCE, REFERENCE, id("tla-de"), false, &mut source);
        assert_eq!(source.functions["First"], ("instruction", 4, 6));
        assert_eq!(source.editions, [id("tla-en"), id("tla-ja")]);
        assert_eq!((source.worst(), source.differing(), source.rest), ("instruction", 4, false));
        assert!(source.shown[0].1.starts_with("  First in tla-en, instruction:\n"));
    }
}
