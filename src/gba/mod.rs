//! What any GBA decompilation can use: the Thumb reader, the ELF and GNU ld
//! map readers, a function's bounds in a linked image, a source's recipe from
//! its Makefile, calcrom's count of what a map places, decomp.dev's report, a C
//! lexer, and the runner that starts a build's tools at fixed addresses.
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

    /// Each line of code that reaches outside `crate::gba` or names a game or
    /// project, by its number.
    fn findings(source: &str) -> Vec<(usize, String)> {
        let mut found = Vec::new();
        for (number, line) in code(source).lines().enumerate() {
            let outside = line.match_indices("crate::").any(|(k, _)| {
                let path = &line[k + "crate::".len()..];
                !path.starts_with("gba") || path[3..].starts_with(identifier)
            });
            let lower = line.to_ascii_lowercase();
            let named = NAMES.iter().any(|name| whole(&lower, name)) || lower.contains("recon/");
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
            for (line, text) in findings(&source) {
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
        let lines: Vec<usize> = findings(source).iter().map(|(line, _)| *line).collect();
        assert_eq!(lines, [2, 3, 4, 6]);
    }
}
