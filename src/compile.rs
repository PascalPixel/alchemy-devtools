//! A C draft compiled as the Makefile compiles the file it would replace:
//! the recipe `make -n` prints for that file's object in the draft's
//! edition, with the draft in its place (`gba::recipe`).

use crate::alchemy::targets::DecompTarget;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Locate the checkout at runtime, including when Alchemy Devtools is a submodule.
fn checkout_root(folder: &Path) -> Option<PathBuf> {
    folder.ancestors().find(|folder| {
        folder.join("rom.sha1").is_file()
            && folder.join("games").is_dir()
            && folder.join("Makefile").is_file()
    }).map(Path::to_path_buf)
}

/// A standalone Alchemy Devtools checkout can target Alchemy through ALCHEMY_ROOT.
pub fn root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let folder = std::env::var_os("ALCHEMY_ROOT").map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok());
        if let Some(found) = folder.as_deref().and_then(checkout_root) {
            return found.canonicalize().unwrap_or(found);
        }
        eprintln!("alchemy-devtools: run inside an Alchemy checkout or set ALCHEMY_ROOT to its path");
        std::process::exit(1)
    })
}

/// The commands that compile `draft` into `object` (its `.s` and `.i` beside
/// it) as `target`'s build compiles the file the draft replaces:
/// - a file the edition's object list names takes its own recipe;
/// - an overlay draft takes the recipe of C its overlay alone links, which
///   for The Lost Age builds for size;
/// - with `arm`, the draft takes the ARM C route: make is told it is one of
///   the Makefile's `ARM_C`;
/// - any other draft takes the recipe the Makefile gives every other C file.
pub(crate) fn steps(target: DecompTarget, draft: &Path, object: &Path, overlay: Option<&str>, arm: bool) -> Result<Vec<Vec<String>>, String> {
    steps_many(target, &[(draft, object, overlay, arm)]).remove(0)
}

/// What `steps` gives each of `drafts` (draft, object, overlay, arm), asking
/// make once for every draft that shares make's variables.
pub(crate) fn steps_many(target: DecompTarget, drafts: &[(&Path, &Path, Option<&str>, bool)]) -> Vec<Result<Vec<Vec<String>>, String>> {
    let root = root();
    let object_of = |source: &str| format!("{}/obj/{}.o", target.output_dir, source.strip_suffix(".c").unwrap_or(source));
    // Each draft's make variables and the source and object whose recipe it takes.
    let routed: Vec<Result<(Vec<String>, String, String), String>> = drafts
        .iter()
        .map(|&(draft, _, overlay, arm)| {
            // The draft as make names it: relative to the checkout when inside it.
            let named = if draft.is_absolute() { draft.strip_prefix(root).unwrap_or(draft) } else { draft };
            let named = named.to_string_lossy().into_owned();
            let mut variables = vec![format!("EDITION={}", target.id)];
            let source = if arm {
                variables.push(format!("ARM_C={named}"));
                named
            } else if let Some(overlay) = overlay.filter(|_| !listed(target, &named)) {
                crate::alchemy::overlay::linked_source(root, Path::new(target.output_dir), overlay)?
            } else {
                named
            };
            let made = object_of(&source);
            Ok((variables, source, made))
        })
        .collect();
    let mut recipes: Vec<Option<Result<Vec<Vec<String>>, String>>> = vec![None; drafts.len()];
    let mut groups: Vec<(Vec<String>, Vec<usize>)> = Vec::new();
    for (index, routed) in routed.iter().enumerate() {
        match routed {
            Ok((variables, ..)) => match groups.iter_mut().find(|(group, _)| group == variables) {
                Some((_, members)) => members.push(index),
                None => groups.push((variables.clone(), vec![index])),
            },
            Err(error) => recipes[index] = Some(Err(error.clone())),
        }
    }
    for (variables, members) in groups {
        let wanted: Vec<(String, String)> = members
            .iter()
            .filter_map(|&index| routed[index].as_ref().ok().map(|(_, source, made)| (source.clone(), made.clone())))
            .collect();
        match crate::gba::recipe::recipes(root, Path::new(target.output_dir), &variables, &wanted) {
            Ok(found) => {
                for (index, recipe) in members.into_iter().zip(found) {
                    recipes[index] = Some(recipe);
                }
            }
            Err(error) => {
                for index in members {
                    recipes[index] = Some(Err(error.clone()));
                }
            }
        }
    }
    recipes
        .into_iter()
        .zip(drafts.iter().zip(&routed))
        .map(|(recipe, (&(draft, object, _, _), routed))| {
            let recipe = recipe.unwrap_or_else(|| Err("no recipe asked for".into())).map_err(|error| format!("{error}; build {} first", target.id))?;
            let (_, source, made) = routed.as_ref().map_err(Clone::clone)?;
            Ok(crate::gba::recipe::retarget(&recipe, root, source, made, draft, object))
        })
        .collect()
}

/// Whether the edition's object list (ags-objects' `objects.mk`) names the
/// object `source` builds.
fn listed(target: DecompTarget, source: &str) -> bool {
    let object = format!("$(BUILD)/obj/{}.o", source.strip_suffix(".c").unwrap_or(source));
    std::fs::read_to_string(root().join(target.output_dir).join("objects.mk"))
        .is_ok_and(|list| list.split_whitespace().any(|word| word == object))
}

#[cfg(test)]
mod checkout_tests {
    use super::*;

    #[test]
    fn finds_the_game_checkout_above_a_submodule() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir(root.join("games")).unwrap();
        std::fs::write(root.join("rom.sha1"), "").unwrap();
        std::fs::write(root.join("Makefile"), "").unwrap();
        let nested = root.join("alchemy-devtools/src");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(checkout_root(&nested).as_deref(), Some(root));
        std::fs::remove_file(root.join("rom.sha1")).unwrap();
        assert!(checkout_root(&nested).is_none());
    }
}
