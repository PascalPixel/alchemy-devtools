//! A source's compile commands, as the project's own Makefile gives them.
//! `make -n` prints the recipe of the object a source builds; a draft takes
//! that recipe with itself in place of the source and a private folder in
//! place of the build's outputs, so it compiles with the build's compiler,
//! options and route without any of them copied here.

use std::path::{Path, PathBuf};

/// The recipe make prints for `object` (relative to `root`, as the Makefile
/// names it), built from `source`: one argument list per command line,
/// without the lines that only make folders. `variables` are make's
/// command-line variables. Every makefile in `build` is passed as `-o`, since
/// make remakes the makefiles it includes even under `-n`; `-W` makes the
/// object out of date whether or not it was built.
pub(crate) fn recipe(root: &Path, build: &Path, variables: &[String], source: &str, object: &str) -> Result<Vec<Vec<String>>, String> {
    let mut arguments = vec!["make".to_string(), "-n".into()];
    let mut included: Vec<PathBuf> = std::fs::read_dir(root.join(build))
        .map_err(|error| format!("{}: {error}", build.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "mk"))
        .collect();
    included.sort();
    for makefile in included {
        arguments.extend(["-o".into(), makefile.strip_prefix(root).unwrap_or(&makefile).to_string_lossy().into_owned()]);
    }
    arguments.extend(variables.iter().cloned());
    arguments.extend(["-W".into(), source.to_string(), object.to_string()]);
    let printed = crate::gba::process::run(&arguments, root)?;
    let base = object.strip_suffix(".o").unwrap_or(object);
    let outputs = [".o", ".s", ".i"].map(|extension| format!("{base}{extension}"));
    let mut steps = Vec::new();
    for line in printed.lines() {
        let words = words(line).map_err(|error| format!("make's recipe for {object}: {error}: {line}"))?;
        let ours = words.iter().any(|word| word == source || outputs.iter().any(|output| word.contains(output.as_str())));
        if ours && !words.is_empty() {
            steps.push(words);
        }
    }
    if steps.is_empty() {
        return Err(format!("make printed no recipe for {object}"));
    }
    Ok(steps)
}

/// A recipe line split as the shell splits it. Quotes and backslashes are
/// read; a line that needs a shell's operators or expansions is refused.
fn words(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => words.extend(word.take()),
            '\'' => {
                let text = word.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => text.push(c),
                        None => return Err("an unclosed quote".into()),
                    }
                }
            }
            '"' => {
                let text = word.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\' | '$' | '`')) => text.push(c),
                            Some(c) => text.extend(['\\', c]),
                            None => return Err("an unclosed quote".into()),
                        },
                        Some('$' | '`') => return Err("a shell expansion".into()),
                        Some(c) => text.push(c),
                        None => return Err("an unclosed quote".into()),
                    }
                }
            }
            '\\' => word.get_or_insert_with(String::new).extend(chars.next()),
            '|' | '&' | ';' | '<' | '>' | '(' | ')' | '$' | '`' | '*' | '?' => {
                return Err(format!("a shell operator or expansion ({c})"));
            }
            c => word.get_or_insert_with(String::new).push(c),
        }
    }
    words.extend(word);
    Ok(words)
}

/// `steps`, the recipe of `object` from `source`, aimed at `draft` and at
/// `output` (with its `.s` and `.i` beside it): the draft stands for the
/// source, also as the `-dumpbase` name, and every relative path the recipe
/// names under `root` becomes absolute, so the steps run from any folder.
pub(crate) fn retarget(steps: &[Vec<String>], root: &Path, source: &str, object: &str, draft: &Path, output: &Path) -> Vec<Vec<String>> {
    let base = object.strip_suffix(".o").unwrap_or(object);
    let outputs = ["o", "s", "i"].map(|extension| (format!("{base}.{extension}"), output.with_extension(extension).to_string_lossy().into_owned()));
    let draft_name = draft.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    // A search folder is a path whether or not it exists; another word is
    // one when it names something under the root.
    let absolute = |word: &str| -> String {
        for prefix in ["-I", "-B", "-L", ""] {
            let Some(rest) = word.strip_prefix(prefix) else { continue };
            if rest.is_empty() || rest.starts_with(['-', '/']) || (prefix.is_empty() && !root.join(rest).exists()) {
                continue;
            }
            return format!("{prefix}{}", root.join(rest).display());
        }
        word.to_string()
    };
    steps
        .iter()
        .map(|step| {
            let mut retargeted: Vec<String> = Vec::with_capacity(step.len());
            for word in step {
                let after_dumpbase = retargeted.last().is_some_and(|last| last == "-dumpbase");
                let written = outputs.iter().find(|(from, _)| word.contains(from.as_str()));
                retargeted.push(if word == source {
                    draft.to_string_lossy().into_owned()
                } else if let Some((from, to)) = written {
                    word.replace(from.as_str(), to)
                } else if after_dumpbase {
                    draft_name.clone()
                } else {
                    absolute(word)
                });
            }
            retargeted
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipe_lines_split_as_the_shell_splits_them() {
        assert_eq!(words("cpp0 -lang-c '-Acpu(arm)' \"-DNAME=\\\"x y\\\"\" a\\ b").unwrap(),
            ["cpp0", "-lang-c", "-Acpu(arm)", "-DNAME=\"x y\"", "a b"]);
        assert_eq!(words("  as  -o x.o\tx.s ").unwrap(), ["as", "-o", "x.o", "x.s"]);
        for line in ["cpp x.c | cc1", "as x.s && ld", "cc1 -o $@", "echo `date`", "cc1 > x.s", "cc1 'open"] {
            assert!(words(line).is_err(), "{line}");
        }
    }

    #[test]
    fn a_draft_takes_the_sources_place_and_paths_become_absolute() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("include/sub")).unwrap();
        std::fs::create_dir_all(root.path().join("tools")).unwrap();
        let steps = vec![
            "tools/cc -Btools/ -O2 -Iinclude -Iinclude/sub -Imissing -S -o build/obj/src/a.s src/a.c".split(' ').map(String::from).collect(),
            "tools/cc1 build/obj/src/a.i -dumpbase a.c -o build/obj/src/a.s".split(' ').map(String::from).collect::<Vec<_>>(),
            "as -mthumb -o build/obj/src/a.o build/obj/src/a.s".split(' ').map(String::from).collect(),
        ];
        let output = Path::new("/work/run-1/draft.o");
        let r = root.path().display();
        let aimed = retarget(&steps, root.path(), "src/a.c", "build/obj/src/a.o", Path::new("/drafts/draft.c"), output);
        assert_eq!(aimed[0].join(" "), format!("tools/cc -B{r}/tools/ -O2 -I{r}/include -I{r}/include/sub -I{r}/missing -S -o /work/run-1/draft.s /drafts/draft.c"));
        assert_eq!(aimed[1].join(" "), "tools/cc1 /work/run-1/draft.i -dumpbase draft.c -o /work/run-1/draft.s");
        assert_eq!(aimed[2].join(" "), "as -mthumb -o /work/run-1/draft.o /work/run-1/draft.s");
    }
}
