//! The inventory of hand-written assembly against the tree: every assembly
//! file under a game's `src/` or the shared `games/common` that holds code,
//! and every `ASM_` macro of its `include/system/asm.h`, has a row of
//! `evidence/<game>-assembly.tsv`, and every row names a piece that is still
//! there. A row says which family of the System the piece belongs to, the
//! form it takes (where it runs), its instruction set, how it treats r4, and
//! what SGL and GBATEK say of it.

use crate::alchemy::evidence;
use std::collections::BTreeSet;
use std::path::Path;

/// The families a piece may belong to.
const FAMILIES: [&str; 16] = [
    "start-up",
    "bios",
    "maths",
    "matrix",
    "dma",
    "decode",
    "draw",
    "world map",
    "sound",
    "far call",
    "memory",
    "field",
    "copy",
    "register",
    "empty",
    "modern",
];
/// The forms a piece may take: where its instructions run from, or how they
/// reach the C.
const FORMS: [&str; 7] = [
    "rom", "resident", "copied", "built", "table", "macro", "empty",
];
/// How a piece treats r4, which the library's routines use as scratch.
const RULES: [&str; 4] = ["scratch", "saved", "untouched", "lists r5"];
/// The columns of a table.
const COLUMNS: usize = 9;

/// A row of the table: the piece's file and name, with its family, form
/// and rule.
#[derive(Debug, PartialEq, Eq)]
struct Row {
    file: String,
    name: String,
    family: String,
    form: String,
    rule: String,
}

/// A table's rows, its header left out, or what is wrong with a line.
fn parse(text: &str) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    for (at, line) in text.lines().enumerate().skip(1) {
        let cells: Vec<&str> = line.split('\t').collect();
        if cells.len() != COLUMNS {
            return Err(format!(
                "line {}: {} cells, not {COLUMNS}",
                at + 1,
                cells.len()
            ));
        }
        rows.push(Row {
            file: cells[0].to_string(),
            name: cells[1].to_string(),
            family: cells[2].to_string(),
            form: cells[3].to_string(),
            rule: cells[5].to_string(),
        });
    }
    Ok(rows)
}

/// Whether an assembly file holds code: it names an instruction set.
fn has_code(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim_start();
        [
            ".arm",
            ".thumb",
            ".code\t16",
            ".code\t32",
            ".code 16",
            ".code 32",
        ]
        .iter()
        .any(|directive| line.starts_with(directive))
    })
}

/// The `ASM_` macros a header defines.
fn macros(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.strip_prefix("#define ASM_"))
        .filter_map(|rest| rest.split('(').next())
        .map(|name| format!("ASM_{name}"))
        .collect()
}

/// Every `.s` file under `folder` that holds code, as paths from `root`.
fn assembly_files(root: &Path, folder: &Path, found: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for path in entries.filter_map(|entry| entry.ok().map(|entry| entry.path())) {
        if path.is_dir() {
            assembly_files(root, &path, found);
        } else if path.extension().is_some_and(|extension| extension == "s")
            && std::fs::read_to_string(&path).is_ok_and(|text| has_code(&text))
        {
            if let Ok(relative) = path.strip_prefix(root) {
                found.insert(relative.to_string_lossy().into_owned());
            }
        }
    }
}

/// What differs between a table's rows and the pieces the tree holds.
fn judge(rows: &[Row], files: &BTreeSet<String>, macros: &[String], header: &str) -> Vec<String> {
    let mut found = Vec::new();
    for row in rows {
        for (cell, allowed, what) in [
            (&row.family, &FAMILIES[..], "family"),
            (&row.form, &FORMS[..], "form"),
            (&row.rule, &RULES[..], "rule"),
        ] {
            if !allowed.contains(&cell.as_str()) {
                found.push(format!("{} {}: unknown {what} {cell}", row.file, row.name));
            }
        }
    }
    let listed: BTreeSet<&str> = rows
        .iter()
        .filter(|row| row.file.ends_with(".s"))
        .map(|row| row.file.as_str())
        .collect();
    for file in files.iter().filter(|file| !listed.contains(file.as_str())) {
        found.push(format!("{file}: assembly with no row"));
    }
    for file in listed.iter().filter(|file| !files.contains(**file)) {
        found.push(format!("{file}: a row for a file that holds no assembly"));
    }
    let named: BTreeSet<&str> = rows
        .iter()
        .filter(|row| row.file == header)
        .map(|row| row.name.as_str())
        .collect();
    for name in macros.iter().filter(|name| !named.contains(name.as_str())) {
        found.push(format!("{header} {name}: a macro with no row"));
    }
    for name in named
        .iter()
        .filter(|name| !macros.iter().any(|defined| defined == *name))
    {
        found.push(format!("{header} {name}: a row for a macro that is gone"));
    }
    found
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    if let Some(argument) = arguments.first() {
        return Err(format!("check assembly takes no argument, not {argument}"));
    }
    let root = crate::compile::root();
    let mut wrong = 0;
    for game in ["tbs", "tla"] {
        let table = format!("{game}-assembly.tsv");
        let text = evidence::table(&table).ok_or_else(|| format!("no evidence/{table}"))?;
        let rows = parse(text).map_err(|message| format!("evidence/{table}: {message}"))?;
        // The files both games share, under games/common, are each game's.
        let mut files = BTreeSet::new();
        assembly_files(root, &root.join("games").join(game).join("src"), &mut files);
        assembly_files(root, &root.join("games").join("common"), &mut files);
        let header = format!("games/{game}/include/system/asm.h");
        let macros = std::fs::read_to_string(root.join(&header))
            .map(|text| macros(&text))
            .unwrap_or_default();
        let found = judge(&rows, &files, &macros, &header);
        for finding in &found {
            println!("{finding}");
        }
        wrong += found.len();
        let forms: Vec<String> = FORMS
            .iter()
            .map(|form| (form, rows.iter().filter(|row| row.form == *form).count()))
            .filter(|(_, count)| *count > 0)
            .map(|(form, count)| format!("{form} {count}"))
            .collect();
        let saved = rows.iter().filter(|row| row.rule == "saved").count();
        println!(
            "games/{game}: {} assembly files, {} macros, {} rows ({}); {saved} keep r4 for their caller",
            files.len(),
            macros.len(),
            rows.len(),
            forms.join(", ")
        );
    }
    if wrong > 0 {
        return Err(format!("{wrong} pieces of assembly and rows do not agree"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_is_wanted_for_each_file_and_macro() {
        let table = "file\tname\tfamily\tform\tmode\trule\tsgl\tgbatek\tproof\n\
games/tla/src/system/trig.s\ttrig\tmaths\trom\tthumb\tuntouched\tslSin\tnone\tstated\n\
games/tla/src/system/gone.s\tgone\tmaths\tram\tarm\tscratch\tnone\tnone\tstated\n\
games/tla/include/system/asm.h\tASM_DMA_STORE\tdma\tmacro\teither\tuntouched\tslDMAXCopy\tnone\tmarked\n";
        let rows = parse(table).unwrap();
        let files: BTreeSet<String> = ["games/tla/src/system/trig.s", "games/tla/src/system/irq.s"]
            .into_iter()
            .map(String::from)
            .collect();
        let macros = macros(
            "#define ASM_DMA_STORE(a, b) \\\n  x\n#define ASM_NOTHING() \\\n  y\n#define OTHER 1\n",
        );
        assert_eq!(macros, ["ASM_DMA_STORE", "ASM_NOTHING"]);
        assert_eq!(
            judge(&rows, &files, &macros, "games/tla/include/system/asm.h"),
            [
                "games/tla/src/system/gone.s gone: unknown form ram",
                "games/tla/src/system/irq.s: assembly with no row",
                "games/tla/src/system/gone.s: a row for a file that holds no assembly",
                "games/tla/include/system/asm.h ASM_NOTHING: a macro with no row",
            ]
        );
        assert!(parse("file\nonly one cell\n").is_err());
        assert!(has_code("\t.arm\nmov r0, r1\n") && !has_code("\t.4byte 1\n"));
    }
}
