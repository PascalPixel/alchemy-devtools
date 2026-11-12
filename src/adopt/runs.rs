//! `adopt merge --keep-views`: each item's sources joined with views before
//! the merge writes them, and `--game GAME`, every run of a game's linker
//! scripts that could become one object.
//!
//! A run is C objects of the game's own route whose statements stand next to
//! each other in every section they place bytes in, compiled by one command
//! in every edition: never across a bank's start or an overlay (any other
//! line between ends a run), never a file the Makefile routes by name. It
//! joins into its scene's own name where it is a scene's code and that name
//! is free, else into its first object.

use super::views::{self, Part, Reconciled};
use super::{item_parts, joined, parent, rewrite_includes, statement, top_level_includes, Files};
use crate::alchemy::layout::Layout;
use std::collections::BTreeMap;
use std::path::Path;

/// Every run of `game`'s linker scripts, as items `INTO.c=OBJECT.c,...`.
pub(super) fn items(root: &Path, game: &str) -> Result<Vec<String>, String> {
    let layout = Layout::new(game);
    let makefile = crate::alchemy::built::read_text(&root.join("Makefile"))?;
    let mut groups: Vec<Vec<(String, String)>> = Vec::new();
    for script in layout.linker_scripts() {
        let Ok(text) = std::fs::read_to_string(root.join(&script)) else { continue };
        let mut group: Vec<(String, String)> = Vec::new();
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match statement(index, line) {
                Some(found) => group.push((found.stem, found.sections)),
                None => groups.extend((!group.is_empty()).then(|| std::mem::take(&mut group))),
            }
        }
        groups.extend((!group.is_empty()).then_some(group));
    }
    let ours = |stem: &str| {
        let name = format!("{stem}.c");
        let file = name.rsplit('/').next().unwrap_or(&name);
        stem.starts_with(&layout.sources()) && root.join(&name).is_file() && !makefile.contains(file)
    };
    // Each candidate: our objects next to each other placing the same sections.
    let mut candidates: Vec<Vec<String>> = Vec::new();
    for group in &groups {
        let mut run: Vec<String> = Vec::new();
        let mut sections = "";
        for (stem, placed) in group {
            if !ours(stem) || placed != sections || run.contains(stem) {
                if run.len() >= 2 {
                    candidates.push(std::mem::take(&mut run));
                }
                run.clear();
            }
            sections = placed;
            if ours(stem) {
                run.push(stem.clone());
            }
        }
        if run.len() >= 2 {
            candidates.push(run);
        }
    }
    // Cut wherever two of a run's objects stand apart, or out of order, in
    // another section they both place.
    let mut runs: Vec<Vec<String>> = Vec::new();
    for candidate in candidates {
        let mut cuts: Vec<usize> = Vec::new();
        for group in &groups {
            let mut last: Option<(usize, &str)> = None;
            for (at, stem) in candidate.iter().enumerate() {
                let Some(position) = group.iter().position(|(s, _)| s == stem) else { continue };
                if let Some((before, sections)) = last {
                    if position != before + 1 || group[position].1 != sections {
                        cuts.push(at);
                    }
                }
                last = Some((position, &group[position].1));
            }
        }
        runs.extend(cut(&candidate, cuts));
    }
    // Cut where the commands that compile them differ in some edition.
    let commands = recipes(root, game, &runs)?;
    let mut items = Vec::new();
    let mut named: Vec<String> = Vec::new();
    for run in runs {
        let cuts: Vec<usize> = (1..run.len()).filter(|&at| commands.get(&run[at]) != commands.get(&run[at - 1])).collect();
        for run in cut(&run, cuts) {
            let into = into(root, &layout, &run, &mut named);
            let objects: Vec<String> = run.iter().map(|stem| format!("{stem}.c")).collect();
            items.push(format!("{into}.c={}", objects.join(",")));
        }
    }
    Ok(items)
}

/// A run cut before each index of `cuts`, keeping the pieces of two or more.
fn cut(run: &[String], mut cuts: Vec<usize>) -> Vec<Vec<String>> {
    cuts.sort_unstable();
    cuts.dedup();
    let mut pieces = Vec::new();
    let mut start = 0;
    for at in cuts.into_iter().chain([run.len()]) {
        if at - start >= 2 {
            pieces.push(run[start..at].to_vec());
        }
        start = at;
    }
    pieces
}

/// What compiles each object, edition by edition, with its file names put
/// aside: its recipe, and whether the edition's main image links it.
fn recipes(root: &Path, game: &str, runs: &[Vec<String>]) -> Result<BTreeMap<String, Vec<String>>, String> {
    let stems: Vec<&String> = runs.iter().flatten().collect();
    let mut commands: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for id in crate::alchemy::built::editions(&[game.to_string()])? {
        let target = crate::alchemy::targets::target_for(id);
        let linked = crate::alchemy::link::linked(root, id)?;
        let wanted: Vec<(String, String)> = stems.iter().map(|stem| (format!("{stem}.c"), format!("{}/obj/{stem}.o", target.output_dir))).collect();
        let found = crate::gba::recipe::recipes(root, Path::new(target.output_dir), &[format!("EDITION={id}")], &wanted)?;
        for ((source, object), recipe) in wanted.iter().zip(found) {
            let stem = source.strip_suffix(".c").unwrap_or(source);
            let words = match recipe {
                Ok(recipe) => {
                    let steps = crate::gba::recipe::retarget(&recipe, root, source, object, Path::new("/merge/object.c"), Path::new("/merge/object.o"));
                    format!("{} {}", linked.main.iter().any(|s| s == stem), steps.concat().join(" "))
                }
                Err(error) => format!("unbuilt {stem}: {error}"),
            };
            commands.entry(stem.to_string()).or_default().push(words);
        }
    }
    Ok(commands)
}

/// The object a run joins into: its scene's own name where every object is
/// that scene's code and no other run took the name, else its first object.
fn into(root: &Path, layout: &Layout, run: &[String], named: &mut Vec<String>) -> String {
    let scenes = format!("{}scene/", layout.sources());
    let scene_of = |stem: &str| stem.strip_prefix(&scenes).map(|rest| rest.split('/').next().unwrap_or(rest).to_string());
    let scene = scene_of(&run[0]).filter(|scene| run.iter().all(|stem| scene_of(stem).as_ref() == Some(scene)));
    if let Some(scene) = scene {
        let own = format!("{scenes}{scene}");
        let free = !named.contains(&own) && (run.contains(&own) || !root.join(format!("{own}.c")).exists());
        if free {
            named.push(own.clone());
            return own;
        }
    }
    named.push(run[0].clone());
    run[0].clone()
}

/// A job: an item and its sources, read before the work is shared out.
struct Job {
    item: String,
    into: String,
    objects: Vec<String>,
    sources: Vec<(String, String)>,
}

/// Settles each item with views: the runs it joined, each with its text,
/// and items views cannot take passed on as they are for the merge to
/// judge. Items are settled in parallel; what each became is printed.
pub(super) fn settle(root: &Path, files: &mut Files, items: &[String]) -> Result<Vec<(String, Option<String>)>, String> {
    let mut jobs: Vec<Result<Job, String>> = Vec::new();
    for item in items {
        let read = (|| -> Result<Job, String> {
            let (into, objects) = item_parts(item)?;
            let mut sources = Vec::new();
            for object in &objects {
                sources.push((object.clone(), files.read(&format!("{object}.c"))?));
            }
            Ok(Job { item: item.clone(), into, objects, sources })
        })();
        jobs.push(read.map_err(|why| format!("{item}\u{0}{why}")));
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(jobs.len().max(1));
    let mut results: Vec<Option<Result<Reconciled, String>>> = (0..jobs.len()).map(|_| None).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let at = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(job) = jobs.get(at) else { break };
                        let Ok(job) = job else { continue };
                        done.push((at, reconcile(root, job)));
                    }
                    done
                })
            })
            .collect();
        for handle in handles {
            for (at, result) in handle.join().unwrap_or_default() {
                results[at] = Some(result);
            }
        }
    });
    let mut planned = Vec::new();
    let mut totals: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut runs, mut out) = (0, 0);
    for (job, result) in jobs.iter().zip(results) {
        let job = match job {
            Ok(job) => job,
            Err(error) => {
                let (item, why) = error.split_once('\u{0}').unwrap_or((error, ""));
                println!("views: {item} is merged as it stands: {why}");
                planned.push((item.to_string(), None));
                continue;
            }
        };
        match result {
            Some(Ok(reconciled)) => {
                for settled in &reconciled.joined {
                    let into = group_into(job, &settled.stems);
                    let objects: Vec<String> = settled.stems.iter().map(|stem| format!("{stem}.c")).collect();
                    let kinds: Vec<String> = settled.views.iter().map(|(kind, n)| format!("{n} {kind}")).collect();
                    let kept = if kinds.is_empty() { "no views".to_string() } else { format!("views kept: {}", kinds.join(", ")) };
                    println!("views: {} parts join into {into}.c, {kept}", settled.stems.len());
                    for (kind, n) in &settled.views {
                        *totals.entry(kind).or_default() += n;
                    }
                    runs += 1;
                    planned.push((format!("{into}.c={}", objects.join(",")), Some(settled.text.clone())));
                }
                for (stem, why) in &reconciled.left_out {
                    out += 1;
                    println!("views: {stem}.c left out of {}: {why}", job.item.split('=').next().unwrap_or(""));
                }
            }
            Some(Err(why)) => println!("views: {} not joined: {why}", job.item),
            None => println!("views: {} not settled", job.item),
        }
    }
    let kinds: Vec<String> = totals.iter().map(|(kind, n)| format!("{n} {kind}")).collect();
    println!("views: {runs} runs settled, {out} parts left out; views kept: {}", if kinds.is_empty() { "none".into() } else { kinds.join(", ") });
    Ok(planned)
}

/// Which object a settled group joins into: INTO where the group holds it,
/// or holds the item's first object and INTO is new; else its first object.
fn group_into(job: &Job, stems: &[String]) -> String {
    let anew = !job.objects.contains(&job.into);
    if stems.contains(&job.into) || (anew && stems.first() == job.objects.first()) {
        job.into.clone()
    } else {
        stems[0].clone()
    }
}

fn reconcile(root: &Path, job: &Job) -> Result<Reconciled, String> {
    let game = Layout::of_path(&job.into).ok_or_else(|| format!("{} names no game", job.into))?.game;
    let editions = crate::alchemy::built::editions(&[game])?;
    let compiler = views::Recipes::new(root, &job.objects[0], &editions)?;
    let prepare = |group: &[(String, String)]| {
        let stems: Vec<String> = group.iter().map(|(stem, _)| stem.clone()).collect();
        let into = group_into(job, &stems);
        let target = format!("{into}.c");
        let mut parts = Vec::new();
        let mut included: Vec<String> = Vec::new();
        for (index, (stem, text)) in group.iter().enumerate() {
            let source = format!("{stem}.c");
            let own = rewrite_includes(text, parent(&source), parent(&target), |path| root.join(path).is_file());
            let own = if index == 0 { own } else { joined(&own, &included).trim_start_matches('\n').to_string() };
            included.extend(top_level_includes(&own));
            parts.push(Part::new(stem, own));
        }
        (target, parts)
    };
    Ok(views::reconcile(&job.sources, &prepare, &compiler))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_cut_where_their_objects_stand_apart() {
        let run: Vec<String> = ["a", "b", "c", "d", "e"].iter().map(|s| s.to_string()).collect();
        assert_eq!(cut(&run, vec![2, 2, 3]), [vec!["a".to_string(), "b".into()], vec!["d".into(), "e".into()]]);
        assert_eq!(cut(&run, vec![]).len(), 1);
        assert!(cut(&run, vec![1, 2, 3, 4]).is_empty());
    }

    #[test]
    fn a_game_s_runs_are_its_neighbouring_c_objects() {
        let root = tempfile::tempdir().unwrap();
        let script = "SECTIONS\n{\n    .text :\n    {\n        \"*/games/tla/src/scene/cave/entry.o\"(.text)\n        \"*/games/tla/src/scene/cave/s0100.o\"(.text)\n        \"*/games/tla/src/scene/cave/s0200.o\"(.text)\n\n        \"*/games/tla/src/scene/cave/s0300.o\"(.text)\n        \"*/games/tla/src/field/move.o\"(.text)\n        \"*/games/tla/src/scene/cave/s0400.o\"(.text)\n        \"*/games/tla/src/scene/cave/s0500.o\"(.text)\n        \"*/games/tla/src/scene/cave/s0200.o\"(.data)\n        \"*/games/tla/src/scene/cave/s0300.o\"(.data)\n    }\n}\n";
        for (path, text) in [
            ("games/tla/ld_script.ld", script),
            ("Makefile", "ARM_C := games/tla/src/scene/cave/s0500.c\n"),
            ("games/tla/src/scene/cave/entry.s", ""),
            ("games/tla/src/field/move.c", ""),
        ] {
            std::fs::create_dir_all(root.path().join(path).parent().unwrap()).unwrap();
            std::fs::write(root.path().join(path), text).unwrap();
        }
        for stem in ["s0100", "s0200", "s0300", "s0400", "s0500"] {
            std::fs::write(root.path().join(format!("games/tla/src/scene/cave/{stem}.c")), "").unwrap();
        }
        // Without the editions' recipes the command cut cannot be read, so
        // this test stops at the runs the scripts allow.
        let mut groups = Vec::new();
        let text = std::fs::read_to_string(root.path().join("games/tla/ld_script.ld")).unwrap();
        for (index, line) in text.lines().enumerate() {
            groups.extend(statement(index, line).map(|s| s.stem));
        }
        assert_eq!(groups.len(), 9);
        let mut named = Vec::new();
        let layout = Layout::new("tla");
        let cave = |stems: &[&str]| stems.iter().map(|s| format!("games/tla/src/scene/cave/{s}")).collect::<Vec<_>>();
        assert_eq!(into(root.path(), &layout, &cave(&["s0100", "s0200"]), &mut named), "games/tla/src/scene/cave");
        assert_eq!(into(root.path(), &layout, &cave(&["s0400", "s0500"]), &mut named), "games/tla/src/scene/cave/s0400");
    }
}
