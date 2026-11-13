//! An object's files read as one text. A large object keeps its source as
//! parts, `<object>/<part>.c` included in ROM order from `<object>.c`, and
//! a scene includes shared files as text; a view in such a file is settled
//! in the objects that include it. The files a settle may change (those
//! holding a view, using a view's private name or undefining a view's name)
//! and the files on the way to them from each object are joined, each after
//! a line naming it, so a view's edit and the renames it makes reach every
//! file that sees the name; the trial is compiled from a scratch copy of
//! each, its quoted includes pointing at the copies.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The line that opens each file of a joined text.
pub(crate) const FILE: &str = "// settle file: ";

/// A quoted `#include` line's file name.
fn included_name(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix('#')?.trim_start().strip_prefix("include")?.trim_start();
    rest.strip_prefix('"')?.split('"').next()
}

/// Where a quoted include in the file at `path` (from the root) reaches,
/// read from that file's own folder: a path from the root, or None.
fn reached(root: &Path, path: &str, named: &str) -> Option<String> {
    if Path::new(named).is_absolute() {
        return None;
    }
    let folder = path.rsplit_once('/').map_or("", |(folder, _)| folder);
    let full = super::super::normalize(&format!("{folder}/{named}"))?;
    root.join(&full).is_file().then_some(full)
}

/// The files a text includes by a quoted name its own folder reaches.
pub(crate) fn includes(root: &Path, path: &str, text: &str) -> Vec<String> {
    text.lines().filter_map(included_name).filter_map(|named| reached(root, path, named)).collect()
}

/// What an object reaches through quoted includes from its folders: each
/// file once, with the file that first included it.
pub(crate) fn tree(root: &Path, object: &str, read: &dyn Fn(&str) -> Option<String>) -> BTreeMap<String, Option<String>> {
    let mut parents: BTreeMap<String, Option<String>> = BTreeMap::new();
    parents.insert(object.to_string(), None);
    let mut pending = vec![object.to_string()];
    while let Some(path) = pending.pop() {
        let Some(text) = read(&path) else { continue };
        for next in includes(root, &path, &text) {
            if !parents.contains_key(&next) {
                parents.insert(next.clone(), Some(path.clone()));
                pending.push(next);
            }
        }
    }
    parents
}

/// The files of an object's tree a settle needs: the object, each file
/// `wanted` names, and every file on the way to one.
pub(crate) fn needed(object: &str, parents: &BTreeMap<String, Option<String>>, wanted: &dyn Fn(&str) -> bool) -> BTreeSet<String> {
    let mut needed: BTreeSet<String> = BTreeSet::new();
    needed.insert(object.to_string());
    for path in parents.keys().filter(|path| wanted(path)) {
        let mut at = Some(path.clone());
        while let Some(path) = at {
            if !needed.insert(path.clone()) {
                break;
            }
            at = parents.get(&path).cloned().flatten();
        }
    }
    needed
}

/// Files joined into one text, each after a line naming it.
pub(crate) fn join(files: &[(String, String)]) -> String {
    let mut text = String::new();
    for (path, source) in files {
        text.push_str(FILE);
        text.push_str(path);
        text.push('\n');
        text.push_str(source);
        if !source.is_empty() && !source.ends_with('\n') {
            text.push('\n');
        }
    }
    text
}

/// A joined text's files again, each as its text now reads (with a final
/// newline; `like` gives back a file's own lack of one).
pub(crate) fn split(text: &str) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = Vec::new();
    for line in text.split_inclusive('\n') {
        match line.strip_prefix(FILE) {
            Some(path) => files.push((path.trim_end().to_string(), String::new())),
            None => {
                if let Some((_, source)) = files.last_mut() {
                    source.push_str(line);
                }
            }
        }
    }
    files
}

/// A file's new text ending as its old one did.
pub(crate) fn like(old: &str, new: String) -> String {
    match (old.ends_with('\n'), new.strip_suffix('\n')) {
        (false, Some(trimmed)) if !old.is_empty() => trimmed.to_string(),
        _ => new,
    }
}

/// Scratch copies of one object's files, in a private folder that goes when
/// it is dropped: the object as `<folder>/<its name>`, every other file at
/// its path under `<folder>/tree/`. A quoted include that reaches a copied
/// file reaches its copy; one that reaches any other file is written with
/// that file's whole path, so it reaches what it did.
pub(crate) struct Copies {
    pub object: PathBuf,
    /// Each copy's path and the file it stands for.
    pub names: Vec<(String, String)>,
    _directory: tempfile::TempDir,
}

impl Copies {
    pub fn write(root: &Path, object: &str, files: &[(&str, &str)]) -> Result<Copies, String> {
        let directory = tempfile::Builder::new().prefix("alchemy-settle-").tempdir().map_err(|error| format!("cannot make a scratch folder: {error}"))?;
        let name = Path::new(object).file_name().map_or_else(|| "object.c".into(), |name| name.to_string_lossy().into_owned());
        let place = |path: &str| if path == object { directory.path().join(&name) } else { directory.path().join("tree").join(path) };
        let placed: BTreeMap<&str, PathBuf> = files.iter().map(|(path, _)| (*path, place(path))).collect();
        for (path, text) in files {
            let mut out = String::with_capacity(text.len());
            for line in text.split_inclusive('\n') {
                let rewritten = included_name(line).and_then(|named| {
                    let full = reached(root, path, named)?;
                    let to = placed.get(full.as_str()).cloned().unwrap_or_else(|| root.join(&full));
                    Some(line.replacen(&format!("\"{named}\""), &format!("\"{}\"", to.display()), 1))
                });
                out.push_str(rewritten.as_deref().unwrap_or(line));
            }
            let target = &placed[path];
            if let Some(folder) = target.parent() {
                std::fs::create_dir_all(folder).map_err(|error| format!("cannot make {}: {error}", folder.display()))?;
            }
            std::fs::write(target, out).map_err(|error| format!("cannot write {}: {error}", target.display()))?;
        }
        let names = placed.iter().map(|(path, copy)| (copy.to_string_lossy().into_owned(), path.to_string())).collect();
        Ok(Copies { object: placed.get(object).cloned().ok_or("the object is not among its files")?, names, _directory: directory })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_joined_split_again() {
        let files = vec![("a.c".to_string(), "int a;\n".to_string()), ("b/c.c".to_string(), "int c;".to_string())];
        let text = join(&files);
        assert_eq!(text, "// settle file: a.c\nint a;\n// settle file: b/c.c\nint c;\n");
        let back = split(&text);
        assert_eq!(back[0], files[0]);
        assert_eq!(like(&files[1].1, back[1].1.clone()), "int c;");
    }

    #[test]
    fn an_object_reaches_its_parts_and_needs_the_way_to_them() {
        let root = tempfile::tempdir().unwrap();
        let write = |path: &str, text: &str| {
            let full = root.path().join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, text).unwrap();
        };
        write("src/big.c", "#include \"big_internal.h\"\n#include \"big/one.c\"\n#include \"module/far.h\"\n");
        write("src/big_internal.h", "int x;\n");
        write("src/big/one.c", "#include \"../big_internal.h\"\n#include \"two.c\"\n");
        write("src/big/two.c", "/* the view */\n");
        let read = |path: &str| std::fs::read_to_string(root.path().join(path)).ok();
        let parents = tree(root.path(), "src/big.c", &read);
        assert_eq!(parents.keys().cloned().collect::<Vec<_>>(), ["src/big.c", "src/big/one.c", "src/big/two.c", "src/big_internal.h"]);
        assert_eq!(parents["src/big/two.c"].as_deref(), Some("src/big/one.c"));
        let needed = needed("src/big.c", &parents, &|path| path == "src/big/two.c");
        assert_eq!(needed.into_iter().collect::<Vec<_>>(), ["src/big.c", "src/big/one.c", "src/big/two.c"]);
        // The copies include each other, and the files not copied as they are.
        let copies = Copies::write(root.path(), "src/big.c", &[("src/big.c", "#include \"big_internal.h\"\n#include \"big/one.c\"\n"), ("src/big/one.c", "#include \"two.c\"\n"), ("src/big/two.c", "int y;\n")]).unwrap();
        let object = std::fs::read_to_string(&copies.object).unwrap();
        let one = copies.names.iter().find(|(_, path)| path == "src/big/one.c").unwrap().0.clone();
        assert_eq!(object, format!("#include \"{}\"\n#include \"{one}\"\n", root.path().join("src/big_internal.h").display()));
        assert!(std::fs::read_to_string(&one).unwrap().contains("/tree/src/big/two.c\""));
    }
}
