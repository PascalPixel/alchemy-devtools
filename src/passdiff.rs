//! passdiff: asks the compiler why a form is kept. An object is compiled
//! twice by its edition's own recipe with the compiler's dump after every
//! pass: once as it stands, once with one of its sources replaced by a plain
//! variant. Pass by pass, one function's RTL is compared, numbers aside, so
//! the first pass where the two part ways, and what reaches the assembly
//! from there, can be read.

use crate::alchemy::targets::{self, decomp_target};
use crate::compile::root;
use regex::Regex;
use std::path::{Path, PathBuf};

const USAGE: &str = r#"usage: alchemy-devtools passdiff SOURCE.c PLAIN.c FUNCTION [--edition EDITION] [--object OBJECT.c]
Compiles the object that builds SOURCE.c twice, by --edition's own recipe
(default: the game's English edition) with the compiler's dump after every
pass (-da): as the tree stands (a), and with SOURCE.c replaced by PLAIN.c,
its plain variant (b). SOURCE.c is the object's own file, or a part another
file includes; the object is the one the edition links that includes it,
followed up through every file that includes it, or --object. Each variant
compiles in a copy of the game's source folders (links to the tree, the
replaced file aside), so a part's relative includes still reach their files.
Prints a line per pass: FUNCTION's instructions in a and in b, and how many
lines of its RTL differ once insn and label numbers, addresses and links
are set aside. Then the RTL that differs at the first pass that differs, and
the lines of FUNCTION's assembly that differ (.L labels renumbered): - is a,
+ is b. The first pass names where the plain form parts ways; read on until
the difference stops being the form itself and becomes what reaches the
assembly. The dumps and the normalized RTL, a.PASS.txt and b.PASS.txt, stay
in out/EDITION/obj/drafts/passdiff-FUNCTION, in place of the last run's.
Analysis only: it says nothing a full make compare-editions does not judge."#;

/// What a run is asked for.
struct Options {
    source: String,
    plain: PathBuf,
    function: String,
    edition: Option<String>,
    object: Option<String>,
}

fn parse(arguments: &[String]) -> Result<Options, String> {
    let mut words = Vec::new();
    let mut edition = None;
    let mut object = None;
    let mut i = 0;
    while i < arguments.len() {
        match arguments[i].as_str() {
            "--edition" | "--object" => {
                let value = arguments.get(i + 1).ok_or_else(|| USAGE.to_string())?.clone();
                if arguments[i] == "--edition" { edition = Some(value) } else { object = Some(value) }
                i += 1;
            }
            value if value.starts_with('-') => return Err(format!("unknown passdiff option: {value}\n{USAGE}")),
            value => words.push(value.to_string()),
        }
        i += 1;
    }
    let [source, plain, function] = <[String; 3]>::try_from(words).map_err(|_| USAGE.to_string())?;
    Ok(Options { source, plain: PathBuf::from(plain), function, edition, object })
}

/// A path as the checkout names it: relative to `root`.
fn relative_to(root: &Path, path: &str) -> Result<String, String> {
    let found = std::fs::canonicalize(path).or_else(|_| std::fs::canonicalize(root.join(path))).map_err(|error| format!("{path}: {error}"))?;
    found.strip_prefix(root).map(|inside| inside.to_string_lossy().into_owned()).map_err(|_| format!("{path} is not inside the checkout {}", root.display()))
}

/// Builds `into` as a copy of `top` (relative to `root`) that links to the
/// tree: every folder on the way to one of `files` is a folder of its own,
/// each other entry a link to the tree's, and each of `files` a copy.
fn mirror(root: &Path, top: &str, files: &[&str], into: &Path) -> Result<(), String> {
    let failed = |path: &Path, error: std::io::Error| format!("{}: {error}", path.display());
    let mut folders: Vec<String> = Vec::new();
    for file in files {
        let rest = file.strip_prefix(top).and_then(|rest| rest.strip_prefix('/')).ok_or_else(|| format!("{file} is not under {top}"))?;
        let mut folder = top.to_string();
        folders.push(folder.clone());
        for part in rest.split('/').collect::<Vec<_>>().split_last().map_or(&[][..], |(_, folders)| folders) {
            folder = format!("{folder}/{part}");
            folders.push(folder.clone());
        }
    }
    for folder in &folders {
        let made = into.join(folder);
        if made.is_dir() {
            continue;
        }
        std::fs::create_dir_all(&made).map_err(|error| failed(&made, error))?;
        let entries = std::fs::read_dir(root.join(folder)).map_err(|error| failed(&root.join(folder), error))?;
        for entry in entries.flatten() {
            let named = format!("{folder}/{}", entry.file_name().to_string_lossy());
            let written = into.join(&named);
            if folders.contains(&named) {
                continue;
            } else if files.contains(&named.as_str()) {
                std::fs::copy(entry.path(), &written).map_err(|error| failed(&written, error))?;
            } else {
                std::os::unix::fs::symlink(entry.path(), &written).map_err(|error| failed(&written, error))?;
            }
        }
    }
    Ok(())
}

/// The object `source` is built in: its own when `linked` says so, else
/// the file that includes it, followed up until one is linked.
fn object_of(source: &str, linked: impl Fn(&str) -> Result<bool, String>, includers: &std::collections::BTreeMap<String, String>) -> Result<Option<String>, String> {
    let mut current = source.to_string();
    for _ in 0..16 {
        if linked(&current)? {
            return Ok(Some(current));
        }
        match includers.get(&current) {
            Some(includer) => current = includer.clone(),
            None => return Ok(None),
        }
    }
    Ok(None)
}

/// `function`'s part of a pass dump: from its `;; Function` line to the next.
fn section<'a>(dump: &'a str, function: &str) -> &'a str {
    let Some(start) = dump.find(&format!(";; Function {function}\n")) else { return "" };
    let end = dump[start + 1..].find("\n;; Function ").map_or(dump.len(), |at| start + 1 + at);
    &dump[start..end]
}

/// RTL with what differs between any two compiles set aside: insn and label
/// numbers, addresses, block and line-number notes and lists of links.
fn normalize(rtl: &str) -> Vec<String> {
    static PATTERNS: std::sync::OnceLock<[Regex; 4]> = std::sync::OnceLock::new();
    let [head, pointer, list, label] = PATTERNS.get_or_init(|| {
        [
            Regex::new(r"^\((insn|jump_insn|call_insn|note|code_label|barrier)\S* \d+ \d+ \d+").unwrap(),
            Regex::new(r"0x[0-9a-f]{6,}").unwrap(),
            Regex::new(r"\((insn_list|expr_list)[^)]*\)+").unwrap(),
            Regex::new(r"(code_label|label_ref) \d+").unwrap(),
        ]
    });
    rtl.lines()
        .filter(|line| !line.trim().is_empty() && !line.contains("NOTE_INSN_BLOCK") && !line.starts_with(";; Processing block"))
        .map(|line| head.replace(line, "($1").into_owned())
        // A line-number note names the copy compiled and the line it stood on.
        .filter(|line| !line.starts_with("(note (\""))
        .map(|line| {
            let line = pointer.replace_all(&line, "PTR");
            let line = label.replace_all(&line, "$1 L");
            list.replace_all(&line, "").replace("(nil)", "").trim_end().to_string()
        })
        .collect()
}

fn instructions(rtl: &[String]) -> usize {
    rtl.iter().filter(|line| ["(insn ", "(jump_insn ", "(call_insn "].iter().any(|kind| line.starts_with(kind))).count()
}

/// `function`'s lines of an assembly file, `.L` labels renumbered.
fn assembly(text: &str, function: &str) -> Vec<String> {
    let label = Regex::new(r"\.L(fe)?\d+").unwrap();
    let mut lines = text.lines().skip_while(|line| *line != format!("{function}:"));
    let mut found = Vec::new();
    for line in lines.by_ref() {
        if line.contains(".size") && line.contains(function) {
            break;
        }
        found.push(label.replace_all(line, ".L").into_owned());
    }
    found
}

fn differing(script: &[String]) -> Vec<&String> {
    script.iter().filter(|line| line.starts_with("+ ") || line.starts_with("- ")).collect()
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    if arguments.iter().any(|argument| matches!(argument.as_str(), "-h" | "--help")) {
        println!("{USAGE}");
        return Ok(());
    }
    let options = parse(arguments)?;
    let root = root();
    let source = relative_to(root, &options.source)?;
    let plain = std::fs::read_to_string(&options.plain).map_err(|error| format!("{}: {error}", options.plain.display()))?;
    let edition = options.edition.as_deref().map(|named| decomp_target(Some(named))).transpose()?;
    let (game, target) = targets::draft_edition(None, edition, &source)?;
    let top = format!("games/{game}/src");
    let object = match &options.object {
        Some(named) => relative_to(root, named)?,
        None => object_of(&source, |file| crate::compile::listed(target, file), &crate::adopt::included_sources(root, game))?
            .ok_or_else(|| format!("{source}: no object {} links includes it (build {} first, or name --object)", target.id, target.id))?,
    };
    let stem = object.strip_suffix(".c").ok_or_else(|| format!("{object}: passdiff compiles C"))?;
    let built = format!("{}/obj/{stem}.o", target.output_dir);
    let recipe = crate::gba::recipe::recipe(root, Path::new(target.output_dir), &[format!("EDITION={}", target.id)], &object, &built)
        .map_err(|error| format!("{error}; build {} first", target.id))?;

    let mut work = crate::diff::Work::new(target)?;
    let kept = work.keep_as(&format!("passdiff-{}", options.function));
    let name = Path::new(&object).file_name().unwrap_or_default().to_string_lossy().into_owned();
    for (variant, replaced) in [("a", None), ("b", Some(&plain))] {
        let folder = work.folder.join(variant);
        mirror(root, &top, &[&object, &source], &folder)?;
        if let Some(text) = replaced {
            std::fs::write(folder.join(&source), text).map_err(|error| format!("{source}: {error}"))?;
        }
        let output = folder.join(format!("{}.o", name.trim_end_matches(".c")));
        let mut steps = crate::gba::recipe::retarget(&recipe, root, &object, &built, &folder.join(&object), &output);
        for step in &mut steps {
            if step.iter().any(|argument| argument == "-S" || argument == "-dumpbase") {
                step.push("-da".into());
            }
        }
        for step in &steps {
            crate::gba::process::run(step, &folder).map_err(|error| {
                let said = if variant == "a" { "as it stands" } else { "with the plain variant" };
                format!("{object} does not compile {said}: {error}")
            })?;
        }
    }

    let dump = Regex::new(&format!(r"^{}\.\d\d\.", regex::escape(&name))).unwrap();
    let mut passes: Vec<String> = std::fs::read_dir(work.folder.join("a"))
        .map_err(|error| error.to_string())?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|file| dump.is_match(file))
        .collect();
    passes.sort();
    let read = |variant: &str, file: &str| std::fs::read_to_string(work.folder.join(variant).join(file)).unwrap_or_default();
    let mut first: Option<(String, Vec<String>)> = None;
    let mut seen = false;
    let mut rows = Vec::new();
    for pass in &passes {
        let a = normalize(section(&read("a", pass), &options.function));
        let b = normalize(section(&read("b", pass), &options.function));
        seen |= !a.is_empty() || !b.is_empty();
        let short = &pass[name.len() + 1..];
        for (variant, rtl) in [("a", &a), ("b", &b)] {
            let written = work.folder.join(format!("{variant}.{short}.txt"));
            std::fs::write(&written, rtl.join("\n") + "\n").map_err(|error| format!("{}: {error}", written.display()))?;
        }
        let (_, script) = crate::diff::compare(&a, &b);
        let changed = differing(&script).len();
        rows.push(format!("{short}\tinsns a={} b={}\tdiffer {changed}", instructions(&a), instructions(&b)));
        if changed > 0 && first.is_none() {
            first = Some((short.to_string(), script));
        }
    }
    if !seen {
        return Err(format!("{} is in no pass dump of {object}: check its name", options.function));
    }
    for row in rows {
        println!("{row}");
    }
    match &first {
        Some((pass, script)) => {
            println!("\n== first pass that differs: {pass}, - a (as it stands), + b (plain) ==");
            for line in differing(script).into_iter().take(60) {
                println!("{line}");
            }
        }
        None => println!("\nno pass differs: the plain variant is the same RTL"),
    }
    let asm = |variant: &str| read(variant, &format!("{}.s", name.trim_end_matches(".c")));
    let (a, b) = (assembly(&asm("a"), &options.function), assembly(&asm("b"), &options.function));
    let (_, script) = crate::diff::compare(&a, &b);
    let changed = differing(&script);
    println!("\n== {}'s assembly: {} of {} lines differ in {} ==", options.function, changed.len(), a.len(), target.id);
    for line in changed.into_iter().take(60) {
        println!("{line}");
    }
    println!("\nthe dumps and normalized RTL (a.PASS.txt, b.PASS.txt): {}", kept.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_part_is_replaced_in_a_linked_copy_and_its_function_s_rtl_compared_without_numbers() {
        let tree = tempfile::tempdir().unwrap();
        let root = tree.path();
        for (file, text) in [("games/g/src/scene/a.c", "#include \"a/part.c\"\n"), ("games/g/src/scene/a/part.c", "int x;\n"), ("games/g/src/scene/a/other.c", ""), ("games/g/src/field/f.c", "")] {
            std::fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
            std::fs::write(root.join(file), text).unwrap();
        }
        let into = root.join("work");
        mirror(root, "games/g/src", &["games/g/src/scene/a.c", "games/g/src/scene/a/part.c"], &into).unwrap();
        assert!(into.join("games/g/src/field").is_symlink());
        assert!(into.join("games/g/src/scene/a/other.c").is_symlink());
        let part = into.join("games/g/src/scene/a/part.c");
        assert!(!part.is_symlink());
        std::fs::write(&part, "int y;\n").unwrap();
        assert_eq!(std::fs::read_to_string(root.join("games/g/src/scene/a/part.c")).unwrap(), "int x;\n");

        let includers = [("games/g/src/scene/a/part.c".to_string(), "games/g/src/scene/a.c".to_string())].into_iter().collect();
        let linked = |file: &str| Ok(file == "games/g/src/scene/a.c");
        assert_eq!(object_of("games/g/src/scene/a/part.c", linked, &includers).unwrap().as_deref(), Some("games/g/src/scene/a.c"));

        let dump = ";; Function Other\n(insn 9 8 10 (nil))\n;; Function Wanted\n\n(note 11 10 12 (\"a/x.c\") 12 0)\n(insn/f 12 11 13 (set (reg:SI 30) (const_int 0)) -1 (nil)\n    (expr_list:REG_EQUAL (const_int 0) (nil)))\n(note 14 13 15 \"\" NOTE_INSN_BLOCK_BEG)\n(jump_insn 15 14 16 (set (pc) (label_ref 0x10a2b3c4)))\n";
        let rtl = normalize(section(dump, "Wanted"));
        assert_eq!(rtl, [";; Function Wanted", "(insn (set (reg:SI 30) (const_int 0)) -1", "     ))", "(jump_insn (set (pc) (label_ref PTR)))"]);
        assert_eq!(instructions(&rtl), 2);

        let asm = "Wanted:\n\tpush\t{lr}\n\tb\t.L12\n.L12:\n.Lfe1:\n\t.size\t Wanted,.Lfe1-Wanted\nNext:\n";
        assert_eq!(assembly(asm, "Wanted"), ["Wanted:", "\tpush\t{lr}", "\tb\t.L", ".L:", ".L:"]);
    }
}
