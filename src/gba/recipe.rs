//! A source's compile commands, as the project's own Makefile gives them.
//! `make -n` prints the recipe of the object a source builds; a draft takes
//! that recipe with itself in place of the source and a private folder in
//! place of the build's outputs, so it compiles with the build's compiler,
//! options and route without any of them copied here. A line make would
//! hand its shell (a pipeline, a redirection, pret's `<(...)`) runs through
//! the Makefile's own SHELL, from the checkout; every other line runs as its
//! words, from any folder. A makefile's parse-time `$(shell ...)` still runs,
//! as it does under make itself.

use std::path::{Path, PathBuf};

/// The recipe make prints for `object` (relative to `root`, as the Makefile
/// names it), built from `source`: one argument list per command line,
/// without the lines that only make folders. `variables` are make's
/// command-line variables. The makefiles `-W` would make out of date are
/// passed as `-o`, since make remakes the makefiles it includes even under
/// `-n`; `-W` makes the object out of date whether or not it was built.
pub(crate) fn recipe(root: &Path, build: &Path, variables: &[String], source: &str, object: &str) -> Result<Vec<Vec<String>>, String> {
    recipes(root, build, variables, &[(source.to_string(), object.to_string())])?.remove(0)
}

/// The recipe of each (source, object) in `wanted`, as `recipe` gives one,
/// from one `make -n` for all of them: make reads the project's makefiles
/// once rather than once an object, the most of each compile's cost to ask
/// for. A line belongs to each object whose source or outputs it names;
/// make's own messages ("make: `X' is up to date.") belong to none. An
/// object asked for twice is asked for once. When make fails on one of
/// them, each is asked for on its own, so one error stays its object's.
pub(crate) fn recipes(root: &Path, build: &Path, variables: &[String], wanted: &[(String, String)]) -> Result<Vec<Result<Vec<Vec<String>>, String>>, String> {
    let mut distinct: Vec<&(String, String)> = Vec::new();
    for pair in wanted {
        if !distinct.contains(&pair) {
            distinct.push(pair);
        }
    }
    if distinct.len() < wanted.len() {
        let once: Vec<(String, String)> = distinct.iter().map(|&pair| pair.clone()).collect();
        let found = recipes(root, build, variables, &once)?;
        return Ok(wanted.iter().map(|pair| found[once.iter().position(|other| other == pair).unwrap_or(0)].clone()).collect());
    }
    let mut arguments = vec!["make".to_string(), "-n".into()];
    let mut held_all: Vec<String> = Vec::new();
    for (_, object) in wanted {
        for makefile in held(root, build, object)? {
            if !held_all.contains(&makefile) {
                held_all.push(makefile);
            }
        }
    }
    for makefile in held_all {
        arguments.extend(["-o".into(), makefile]);
    }
    arguments.extend(variables.iter().cloned());
    for (source, _) in wanted {
        arguments.extend(["-W".into(), source.clone()]);
    }
    arguments.extend(wanted.iter().map(|(_, object)| object.clone()));
    let printed = match crate::gba::process::run(&arguments, root) {
        Ok(printed) => printed,
        Err(error) if wanted.len() == 1 => return Err(error),
        Err(_) => return Ok(wanted.iter().map(|(source, object)| recipe(root, build, variables, source, object)).collect()),
    };
    let mut shell: Option<Vec<String>> = None;
    let mut found: Vec<Vec<Vec<String>>> = vec![Vec::new(); wanted.len()];
    // A recipe line make prints continued with a backslash is one command.
    let printed = printed.replace("\\\n", "");
    for line in printed.lines().filter(|line| !message(line)) {
        let words = words(line);
        for (index, (source, object)) in wanted.iter().enumerate() {
            let base = object.strip_suffix(".o").unwrap_or(object);
            let outputs = [".o", ".s", ".i"].map(|extension| format!("{base}{extension}"));
            match &words {
                Ok(words) => {
                    let ours = words.iter().any(|word| word == source || outputs.iter().any(|output| word.contains(output.as_str())));
                    if ours && !words.is_empty() {
                        found[index].push(words.clone());
                    }
                }
                Err(_) if mentions(line, source) || outputs.iter().any(|output| line.contains(output.as_str())) => {
                    if shell.is_none() {
                        shell = Some(make_shell(root, variables).map_err(|error| format!("make's recipe for {object} needs its shell: {error}"))?);
                    }
                    let mut step = shell.clone().unwrap_or_default();
                    step.push(format!("cd {} && {line}", quote(&root.to_string_lossy())));
                    found[index].push(step);
                }
                Err(_) => {}
            }
        }
    }
    Ok(found
        .into_iter()
        .zip(wanted)
        .map(|(steps, (_, object))| if steps.is_empty() { Err(format!("make printed no recipe for {object}")) } else { Ok(steps) })
        .collect())
}

/// Whether a printed line is make's own message, `make: ...` or
/// `make[1]: ...`, not a recipe line.
fn message(line: &str) -> bool {
    let (program, _) = line.split_once(": ").unwrap_or(("", ""));
    let program = program.split_once('[').map_or(program, |(name, _)| name);
    !program.contains(' ') && program.rsplit('/').next().is_some_and(|name| name.ends_with("make"))
}

/// The makefiles make would remake under `-n` once `-W` makes the source
/// new, relative to `root`: every `.mk` in `build` (a generated object list
/// or dependency list), and the object's own dependency file, `NAME.d`
/// beside `NAME.o`, which a Makefile that includes it remakes from the source.
fn held(root: &Path, build: &Path, object: &str) -> Result<Vec<String>, String> {
    let mut included: Vec<PathBuf> = std::fs::read_dir(root.join(build))
        .map_err(|error| format!("{}: {error}", build.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "mk"))
        .collect();
    included.sort();
    let mut held: Vec<String> = included.iter().map(|path| path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned()).collect();
    let dependencies = format!("{}.d", object.strip_suffix(".o").unwrap_or(object));
    if root.join(&dependencies).is_file() {
        held.push(dependencies);
    }
    Ok(held)
}

/// The shell make runs recipe lines with, and its flags: the Makefile's
/// SHELL, and .SHELLFLAGS where make has it (-c before GNU make 3.82). Read
/// from a rule of our own read after the project's makefile, as make 3.81
/// has no --eval.
fn make_shell(root: &Path, variables: &[String]) -> Result<Vec<String>, String> {
    let makefile = ["GNUmakefile", "makefile", "Makefile"].into_iter().find(|name| root.join(name).is_file()).ok_or("no makefile")?;
    let mut probe = tempfile::Builder::new().prefix("devtools-shell").suffix(".mk").tempfile().map_err(|error| error.to_string())?;
    use std::io::Write;
    probe
        .write_all(b".PHONY: devtools-shell\ndevtools-shell:\n\t@echo devtools-shell $(SHELL) devtools-flags $(or $(.SHELLFLAGS),-c)\n")
        .map_err(|error| error.to_string())?;
    let mut arguments = vec!["make".to_string(), "-n".into(), "-f".into(), makefile.into(), "-f".into(), probe.path().to_string_lossy().into_owned()];
    arguments.extend(variables.iter().cloned());
    arguments.push("devtools-shell".into());
    let printed = crate::gba::process::run(&arguments, root)?;
    let line = printed.lines().find(|line| line.starts_with("echo devtools-shell ")).ok_or("make printed no SHELL")?;
    let mut words = words(line)?.into_iter().skip(2).filter(|word| word != "devtools-flags").collect::<Vec<_>>();
    if words.len() < 2 {
        words = vec!["/bin/sh".into(), "-c".into()];
    }
    Ok(words)
}

/// Whether `text` names `path` as a whole path, not as the tail or head of
/// a longer one.
fn mentions(text: &str, path: &str) -> bool {
    !path.is_empty() && text.match_indices(path).any(|(at, _)| bounded(text, at, path.len()))
}

fn bounded(text: &str, at: usize, length: usize) -> bool {
    let path_char = |c: char| c.is_alphanumeric() || matches!(c, '_' | '.' | '/' | '-' | '+');
    !text[..at].chars().next_back().is_some_and(path_char) && !text[at + length..].chars().next().is_some_and(path_char)
}

/// `text` with every whole-path mention of `from` made `to`.
fn replace_path(text: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (at, _) in text.match_indices(from).filter(|&(at, _)| bounded(text, at, from.len())) {
        out += &text[last..at];
        out += to;
        last = at + from.len();
    }
    out + &text[last..]
}

/// A word as the shell reads it back unchanged.
fn quote(word: &str) -> String {
    if !word.is_empty() && word.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '/' | '-' | '+' | ',' | '=' | ':' | '@' | '%')) {
        return word.to_string();
    }
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// Whether a step is a recipe line run through make's shell: its last word
/// is the line, run from the checkout.
pub(crate) fn is_shell(step: &[String]) -> bool {
    step.len() > 1 && step.last().is_some_and(|line| line.starts_with("cd ") && line.contains(" && "))
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
/// A line run through the shell has the same paths replaced in its text.
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
            if is_shell(step) {
                let mut step = step.clone();
                let mut line = replace_path(&step.pop().unwrap_or_default(), source, &quote(&draft.to_string_lossy()));
                for (from, to) in &outputs {
                    line = replace_path(&line, from, &quote(to));
                }
                step.push(line);
                return step;
            }
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
            "tools/cc -Btools/ -O2 -Iinclude -Iinclude/sub -Imissing -S -o build/src/a.s src/a.c".split(' ').map(String::from).collect(),
            "tools/cc1 build/src/a.i -dumpbase a.c -o build/src/a.s".split(' ').map(String::from).collect::<Vec<_>>(),
            "as -mthumb -o build/src/a.o build/src/a.s".split(' ').map(String::from).collect(),
        ];
        let output = Path::new("/work/run-1/draft.o");
        let r = root.path().display();
        let aimed = retarget(&steps, root.path(), "src/a.c", "build/src/a.o", Path::new("/drafts/draft.c"), output);
        assert_eq!(aimed[0].join(" "), format!("tools/cc -B{r}/tools/ -O2 -I{r}/include -I{r}/include/sub -I{r}/missing -S -o /work/run-1/draft.s /drafts/draft.c"));
        assert_eq!(aimed[1].join(" "), "tools/cc1 /work/run-1/draft.i -dumpbase draft.c -o /work/run-1/draft.s");
        assert_eq!(aimed[2].join(" "), "as -mthumb -o /work/run-1/draft.o /work/run-1/draft.s");
    }

    /// Two objects from one make: each takes only its own lines, one asked
    /// for twice takes the same, and make's own messages are no one's.
    #[test]
    fn one_make_gives_each_object_its_own_recipe() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("build/src")).unwrap();
        std::fs::write(root.join("Makefile"), "build/src/%.o: src/%.c\n\tcc -c -o $@ $<\n").unwrap();
        for name in ["a", "b"] {
            std::fs::write(root.join(format!("src/{name}.c")), "int x;\n").unwrap();
        }
        let pair = |name: &str| (format!("src/{name}.c"), format!("build/src/{name}.o"));
        let found = recipes(root, Path::new("build"), &[], &[pair("a"), pair("b"), pair("a")]).unwrap();
        let line = |steps: &Result<Vec<Vec<String>>, String>| steps.as_ref().unwrap().iter().map(|step| step.join(" ")).collect::<Vec<_>>();
        assert_eq!(line(&found[0]), ["cc -c -o build/src/a.o src/a.c"]);
        assert_eq!(line(&found[1]), ["cc -c -o build/src/b.o src/b.c"]);
        assert_eq!(line(&found[2]), line(&found[0]));
        assert!(message("make: `build/src/a.o' is up to date.") && message("/usr/bin/gmake[1]: Nothing to be done for `x'."));
        assert!(!message("cc: error: x") && !message("echo make: it"));
    }

    /// pret's shape: a pipeline that needs the Makefile's SHELL, and a
    /// dependency file the Makefile includes and would remake from the source.
    #[test]
    fn a_pipeline_runs_through_the_makefiles_shell_and_its_dependency_file_is_left_alone() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("build/src")).unwrap();
        std::fs::write(
            root.join("Makefile"),
            "SHELL := /bin/sh\n-include build/src/a.d\nbuild/src/a.o: src/a.c\n\t@echo \"cc <flags> -o $@ $<\"\n\tcat $< | tr a-z A-Z > $@\nbuild/src/a.d: src/a.c\n\techo remade > $@\n",
        )
        .unwrap();
        std::fs::write(root.join("src/a.c"), "int a;\n").unwrap();
        std::fs::write(root.join("build/src/a.d"), "# kept\n").unwrap();
        let steps = recipe(root, Path::new("build"), &[], "src/a.c", "build/src/a.o").unwrap();
        assert_eq!(std::fs::read_to_string(root.join("build/src/a.d")).unwrap(), "# kept\n", "-o kept the dependency file");
        let shell = steps.last().unwrap();
        assert!(is_shell(shell) && !is_shell(&steps[0]), "{steps:?}");
        assert_eq!(shell[..2], ["/bin/sh", "-c"]);
        let work = tempfile::tempdir().unwrap();
        let draft = work.path().join("my draft.c");
        std::fs::write(&draft, "int b;\n").unwrap();
        let output = work.path().join("draft.o");
        let aimed = retarget(&steps, root, "src/a.c", "build/src/a.o", &draft, &output);
        assert!(aimed.last().unwrap().last().unwrap().ends_with(&format!("cat '{}' | tr a-z A-Z > {}", draft.display(), quote(&output.to_string_lossy()))));
        for step in &aimed {
            crate::gba::process::run(step, work.path()).unwrap();
        }
        assert_eq!(std::fs::read_to_string(&output).unwrap(), "INT B;\n");
        assert!(mentions("cc src/a.c -o x", "src/a.c") && !mentions("cc build/src/a.c", "src/a.c") && !mentions("src/a.cc", "src/a.c"));
    }
}
