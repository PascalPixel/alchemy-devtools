//! What any GBA decompilation can use: the Thumb reader, the ELF and GNU ld
//! map readers, a function's bounds in a linked image, a source's recipe from
//! its Makefile, calcrom's count of what a map places, decomp.dev's report, a C
//! lexer, the runner that starts a build's tools at fixed addresses, and the
//! shapes that find a function's twin in another build.
//! Nothing here knows which game or project it serves, and the test below keeps
//! it so: no path out of `crate::gba`, no game's or project's name in its code.

pub(crate) mod bounds;
pub(crate) mod clex;
pub(crate) mod count;
pub(crate) mod elf;
pub(crate) mod ld_map;
pub(crate) mod process;
pub(crate) mod recipe;
pub(crate) mod report;
pub(crate) mod shape;
pub(crate) mod thumb;

#[cfg(test)]
mod boundary {
    use std::path::Path;

    /// The names of one game or one project, refused outside comments; matched
    /// whole and in any case, so `TBS_EDITION` is one and `atlas` is not.
    const NAMES: [&str; 5] = ["tbs", "tla", "golden sun", "camelot", "alchemy"];

    fn identifier(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    /// The source with its comments blanked: strings and character literals
    /// stay code, so a `//` inside a string is read, and every line keeps its
    /// number.
    fn code(source: &str) -> String {
        let chars: Vec<char> = source.chars().collect();
        let at = |k: usize| chars.get(k).copied().unwrap_or('\0');
        let before = |k: usize| k.checked_sub(1).map_or('\0', at);
        let mut out = String::with_capacity(source.len());
        let mut i = 0;
        while i < chars.len() {
            let start = i;
            let mut comment = false;
            let raw = at(i) == 'r'
                && (!identifier(before(i)) || (before(i) == 'b' && !identifier(before(i - 1))));
            let hashes = if raw { (i + 1..).take_while(|&k| at(k) == '#').count() } else { 0 };
            if at(i) == '/' && at(i + 1) == '/' {
                comment = true;
                while i < chars.len() && at(i) != '\n' {
                    i += 1;
                }
            } else if at(i) == '/' && at(i + 1) == '*' {
                comment = true;
                let mut depth = 0;
                while i < chars.len() {
                    if at(i) == '/' && at(i + 1) == '*' {
                        depth += 1;
                        i += 2;
                    } else if at(i) == '*' && at(i + 1) == '/' {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
            } else if raw && at(i + 1 + hashes) == '"' {
                i += 2 + hashes;
                while i < chars.len() && !(at(i) == '"' && (1..=hashes).all(|k| at(i + k) == '#')) {
                    i += 1;
                }
                i += 1 + hashes;
            } else if at(i) == '"' {
                i += 1;
                while i < chars.len() && at(i) != '"' {
                    i += if at(i) == '\\' { 2 } else { 1 };
                }
                i += 1;
            } else if at(i) == '\'' && at(i + 1) == '\\' {
                i += 3;
                while i < chars.len() && at(i) != '\'' {
                    i += 1;
                }
                i += 1;
            } else if at(i) == '\'' && at(i + 2) == '\'' {
                i += 3;
            } else {
                i += 1;
            }
            let end = i.min(chars.len());
            for &c in &chars[start..end] {
                out.push(if comment && c != '\n' { ' ' } else { c });
            }
        }
        out
    }

    fn whole(text: &str, word: &str) -> bool {
        text.match_indices(word).any(|(k, _)| {
            let before = text[..k].chars().next_back();
            let after = text[k + word.len()..].chars().next();
            !before.is_some_and(|c| c.is_ascii_alphanumeric())
                && !after.is_some_and(|c| c.is_ascii_alphanumeric())
        })
    }

    /// One project's layout, refused outside comments: its folders as a path
    /// starts them (`build/obj/` is one, `layout/` is not) and its file names.
    const FOLDERS: [&str; 5] = ["out/", "obj/", "games/", "roms/", "recon/"];
    const FILES: [&str; 6] = ["rom.sha1", "ld_script", "objects.mk", "deps.mk", "resource_", "_overlay"];

    fn folder(text: &str, word: &str) -> bool {
        text.match_indices(word).any(|(k, _)| !text[..k].chars().next_back().is_some_and(identifier))
    }

    /// Each line of code that reaches outside `crate::gba`, through `crate::`
    /// or a `super` above it, or names a game, a project or its layout, by its
    /// number. `top` is whether the source is `gba` itself, whose `super` is
    /// the crate.
    fn findings(source: &str, top: bool) -> Vec<(usize, String)> {
        let mut found = Vec::new();
        for (number, line) in code(source).lines().enumerate() {
            let outside = line.match_indices("crate::").any(|(k, _)| {
                let path = &line[k + "crate::".len()..];
                !path.starts_with("gba") || path[3..].starts_with(identifier)
            }) || line.contains("super::super") || (top && folder(line, "super::"));
            let lower = line.to_ascii_lowercase();
            let named = NAMES.iter().any(|name| whole(&lower, name))
                || FOLDERS.iter().any(|name| folder(&lower, name))
                || FILES.iter().any(|name| lower.contains(name));
            if outside || named {
                found.push((number + 1, line.trim().to_string()));
            }
        }
        found
    }

    #[test]
    fn generic_code_reaches_no_further_than_gba_and_names_no_game() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/gba");
        let mut read = 0;
        let mut found = Vec::new();
        for entry in walkdir::WalkDir::new(&directory).sort_by_file_name() {
            let path = entry.unwrap().into_path();
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            let mut source = std::fs::read_to_string(&path).unwrap();
            // This module names what it refuses: only its code above the tests is read.
            if path == directory.join("mod.rs") {
                source.truncate(source.find("#[cfg(test)]").unwrap_or(source.len()));
            }
            read += 1;
            for (line, text) in findings(&source, path == directory.join("mod.rs")) {
                let name = path.strip_prefix(&directory).unwrap().display();
                found.push(format!("src/gba/{name}:{line}: {text}"));
            }
        }
        assert!(read > 1, "no Rust read under {}", directory.display());
        assert!(found.is_empty(), "src/gba must stay generic:\n{}", found.join("\n"));
    }

    #[test]
    fn comments_are_skipped_and_strings_and_characters_are_read() {
        let source = "use crate::gba::elf; // tla\n\
                      /* Alchemy */ let path = \"recon/x.s // \";\n\
                      use crate::diff;\n\
                      let quote = '\"'; let tbs = 1;\n\
                      fn f<'a>(x: &'a str) {} // TBS\n\
                      let s = r#\"a \" crate::gbax\"#; /* /* Camelot */ */\n\
                      let atlas = crate::gba::thumb::bl_displacement;\n";
        let lines: Vec<usize> = findings(source, false).iter().map(|(line, _)| *line).collect();
        assert_eq!(lines, [2, 3, 4, 6]);
    }

    #[test]
    fn a_projects_layout_and_a_path_above_gba_are_refused() {
        let source = "let map = \"build/obj/src/a.o\";\n\
                      let layout = \"layout/x\"; let place = \"build/src/a.o\";\n\
                      let sums = root.join(\"rom.sha1\");\n\
                      use super::super::diff;\n\
                      use super::elf;\n\
                      let list = \"games/x/ld_script.ld\";\n";
        let lines = |top| findings(source, top).iter().map(|(line, _)| *line).collect::<Vec<usize>>();
        assert_eq!(lines(false), [1, 3, 4, 6]);
        assert_eq!(lines(true), [1, 3, 4, 5, 6]);
    }
}
