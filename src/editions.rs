//! editions: the objects from `games/` some edition does not link, with the
//! names each defines, and each edition's scaffold sections that hold a name
//! English links as C: the work one edition has left where another is done.
//! Names come from symbol tables, the object's own and the scaffold's.

use crate::alchemy::built;
use crate::alchemy::targets::DecompTargetId;
use crate::gba::elf::{self, Elf};
use crate::alchemy::done::commas;
use std::collections::HashMap;
use std::path::Path;

const USAGE: &str = "usage: alchemy-devtools editions [EDITION|GAME...] [--json]
Lists each object from games/ that some edition does not link (the rows of
games/<game>/objects.tsv merged per object), with the names its built object
defines; under each name, the scaffold section (recon/<game>/<language>/rom.s)
that still holds it in an edition that does not link the object, with its
size. Then, for each edition with a scaffold, the bytes its scaffold holds:
in sections with a name English links as C, with names English does not, and
with no name. With no edition, every game that still has recon/<game>; a game
names its six editions. --json prints the same as one JSON object.";

/// SHN_LORESERVE: section indices from here on are not sections.
const RESERVED: u16 = 0xff00;
const STB_GLOBAL: u8 = 1;

/// A scaffold section where an edition still places a name.
#[derive(Debug)]
struct Held {
    edition: DecompTargetId,
    section: String,
    size: u64,
}

/// An object some edition does not link, or whose names some scaffold holds.
#[derive(Debug)]
struct Object {
    object: String,
    file: String,
    linked: Vec<DecompTargetId>,
    missing: Vec<DecompTargetId>,
    /// Every global name the object defines, each with where the scaffolds
    /// that still hold it place it.
    names: Vec<(String, Vec<Held>)>,
}

/// One edition's scaffold, summed by what its sections hold.
#[derive(Debug)]
struct Scaffold {
    edition: DecompTargetId,
    source: String,
    unlinked: usize,
    linked_sections: usize,
    linked_bytes: u64,
    other_bytes: u64,
    unnamed_bytes: u64,
}

/// A section of an object, with its size and the global names it defines.
#[derive(Debug, PartialEq, Eq)]
struct Section {
    name: String,
    size: u64,
    names: Vec<String>,
}

/// The allocated sections of an object file, in its order, each with the
/// global names its symbol table places in it.
fn sections(path: &Path) -> Result<Vec<Section>, String> {
    let data = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let headers = elf::headers(&data).map_err(|error| format!("{}: {error}", path.display()))?;
    let symbols = Elf::parse(&data).map_err(|error| format!("{}: {error}", path.display()))?.symbols;
    let mut sections: Vec<Section> = headers
        .iter()
        .map(|header| Section { name: header.name.clone(), size: header.size as u64, names: Vec::new() })
        .collect();
    let mut globals: Vec<_> = symbols
        .iter()
        .filter(|symbol| symbol.bind == STB_GLOBAL && symbol.section != 0 && symbol.section < RESERVED)
        .collect();
    globals.sort_by_key(|symbol| (symbol.section, symbol.value));
    for symbol in globals {
        if let Some(section) = sections.get_mut(usize::from(symbol.section)) {
            section.names.push(symbol.name.clone());
        }
    }
    const SHF_ALLOC: u32 = 2;
    Ok(headers
        .iter()
        .zip(sections)
        .filter(|(header, section)| header.flags & SHF_ALLOC != 0 && section.size > 0)
        .map(|(_, section)| section)
        .collect())
}

/// The global names an object defines, section by section.
fn defined(path: &Path) -> Result<Vec<String>, String> {
    Ok(sections(path)?.into_iter().flat_map(|section| section.names).collect())
}

/// The editions of `game` among `chosen`, in the targets' order.
fn of_game(chosen: &[DecompTargetId], game: &str) -> Vec<DecompTargetId> {
    chosen.iter().copied().filter(|&id| id.game() == game).collect()
}

/// One game's objects and scaffolds.
fn survey(root: &Path, game: &str, chosen: &[DecompTargetId]) -> Result<(Vec<Object>, Vec<Scaffold>), String> {
    let all: Vec<DecompTargetId> = built::editions(&[game.to_string()])?;
    let english = built::edition(game, "en").ok_or_else(|| format!("{game} has no English edition"))?;
    // The object list's rows merged per object, in link order.
    let mut order: Vec<String> = Vec::new();
    let mut languages: HashMap<String, Vec<&str>> = HashMap::new();
    for row in built::object_list(root, game)?.iter().filter(|row| row.object.starts_with("games/")) {
        let linked = languages.entry(row.object.clone()).or_insert_with(|| {
            order.push(row.object.clone());
            Vec::new()
        });
        for &id in &all {
            if row.links(id.language()) && !linked.contains(&id.language()) {
                linked.push(id.language());
            }
        }
    }
    let mut objects: Vec<Object> = Vec::new();
    // Each name English links from games/, by the object that defines it.
    let mut owner: HashMap<String, usize> = HashMap::new();
    for stem in &order {
        let linked: Vec<DecompTargetId> = all.iter().copied().filter(|&id| languages[stem].contains(&id.language())).collect();
        let missing: Vec<DecompTargetId> = all.iter().copied().filter(|id| !linked.contains(id)).collect();
        let english_links = linked.contains(&english);
        if missing.iter().all(|id| !chosen.contains(id)) && !english_links {
            continue;
        }
        // English's names first, then those only another edition's build defines.
        let mut names = if english_links { defined(&built::object(root, english, stem))? } else { Vec::new() };
        for &id in linked.iter().filter(|&&id| id != english && !missing.is_empty()) {
            for name in defined(&built::object(root, id, stem))? {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        let file = built::source(root, stem).unwrap_or_else(|| stem.clone());
        let at = objects.len();
        if english_links {
            for name in &names {
                owner.entry(name.clone()).or_insert(at);
            }
        }
        objects.push(Object {
            object: stem.clone(),
            file,
            linked,
            missing,
            names: names.into_iter().map(|name| (name, Vec::new())).collect(),
        });
    }
    let mut scaffolds = Vec::new();
    for &id in chosen.iter().filter(|&&id| id != english) {
        let Some((source, object)) = built::scaffold(root, id) else { continue };
        let mut scaffold = Scaffold {
            edition: id,
            source,
            unlinked: objects.iter().filter(|object| object.missing.contains(&id)).count(),
            linked_sections: 0,
            linked_bytes: 0,
            other_bytes: 0,
            unnamed_bytes: 0,
        };
        for section in sections(&object)? {
            let mut held = false;
            for name in &section.names {
                let Some(&at) = owner.get(name) else { continue };
                held = true;
                let place = Held { edition: id, section: section.name.clone(), size: section.size };
                if let Some((_, places)) = objects[at].names.iter_mut().find(|(n, _)| n == name) {
                    places.push(place);
                }
            }
            if held {
                scaffold.linked_sections += 1;
                scaffold.linked_bytes += section.size;
            } else if section.names.is_empty() {
                scaffold.unnamed_bytes += section.size;
            } else {
                scaffold.other_bytes += section.size;
            }
        }
        scaffolds.push(scaffold);
    }
    // Keep what some chosen edition lacks, or what a scaffold still holds.
    objects.retain(|object| {
        object.missing.iter().any(|id| chosen.contains(id)) || object.names.iter().any(|(_, places)| !places.is_empty())
    });
    Ok((objects, scaffolds))
}

fn languages(editions: &[DecompTargetId]) -> String {
    editions.iter().map(|&id| id.language()).collect::<Vec<_>>().join(" ")
}

fn text(objects: &[Object], scaffolds: &[Scaffold]) -> String {
    let scanned: Vec<DecompTargetId> = scaffolds.iter().map(|scaffold| scaffold.edition).collect();
    let mut text = String::new();
    for object in objects {
        text += &format!("{}: linked by {}", object.file, languages(&object.linked));
        if !object.missing.is_empty() {
            text += &format!("; not by {}", languages(&object.missing));
        }
        text += "\n";
        for (name, places) in &object.names {
            let mut parts: Vec<String> = places
                .iter()
                .map(|place| format!("{} {} ({})", place.edition.language(), place.section, bytes(place.size)))
                .collect();
            let absent: Vec<_> = object
                .missing
                .iter()
                .copied()
                .filter(|id| scanned.contains(id) && places.iter().all(|place| place.edition != *id))
                .collect();
            if !absent.is_empty() {
                parts.push(format!("not in the scaffold of {}", languages(&absent)));
            }
            text += &match parts.is_empty() {
                true => format!("  {name}\n"),
                false => format!("  {name}: {}\n", parts.join(", ")),
            };
        }
    }
    for scaffold in scaffolds {
        let edition = scaffold.edition;
        text += &format!(
            "{edition}: {} objects not linked; {} holds {} in {} sections with a name English links as C, {} with names English does not, {} with no name\n",
            scaffold.unlinked,
            scaffold.source,
            bytes(scaffold.linked_bytes),
            scaffold.linked_sections,
            bytes(scaffold.other_bytes),
            bytes(scaffold.unnamed_bytes),
        );
    }
    text
}

fn bytes(count: u64) -> String {
    format!("{} bytes", commas(count as i64))
}

fn quote(text: &str) -> String {
    let mut quoted = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => quoted += "\\\"",
            '\\' => quoted += "\\\\",
            c if (c as u32) < 0x20 => quoted += &format!("\\u{:04x}", c as u32),
            c => quoted.push(c),
        }
    }
    quoted + "\""
}

fn list(editions: &[DecompTargetId]) -> String {
    format!("[{}]", editions.iter().map(|id| quote(id.as_str())).collect::<Vec<_>>().join(","))
}

fn json(objects: &[Object], scaffolds: &[Scaffold]) -> String {
    let objects: Vec<String> = objects
        .iter()
        .map(|object| {
            let names: Vec<String> = object
                .names
                .iter()
                .map(|(name, places)| {
                    let places: Vec<String> = places
                        .iter()
                        .map(|place| {
                            format!(
                                "{{\"edition\":{},\"section\":{},\"size\":{}}}",
                                quote(place.edition.as_str()),
                                quote(&place.section),
                                place.size
                            )
                        })
                        .collect();
                    format!("{{\"name\":{},\"scaffold\":[{}]}}", quote(name), places.join(","))
                })
                .collect();
            format!(
                "{{\"file\":{},\"object\":{},\"linked\":{},\"missing\":{},\"names\":[{}]}}",
                quote(&object.file),
                quote(&object.object),
                list(&object.linked),
                list(&object.missing),
                names.join(",")
            )
        })
        .collect();
    let scaffolds: Vec<String> = scaffolds
        .iter()
        .map(|scaffold| {
            format!(
                "{{\"edition\":{},\"source\":{},\"unlinked\":{},\"linkedSections\":{},\"linkedBytes\":{},\"otherBytes\":{},\"unnamedBytes\":{}}}",
                quote(scaffold.edition.as_str()),
                quote(&scaffold.source),
                scaffold.unlinked,
                scaffold.linked_sections,
                scaffold.linked_bytes,
                scaffold.other_bytes,
                scaffold.unnamed_bytes
            )
        })
        .collect();
    format!("{{\"objects\":[{}],\"scaffolds\":[{}]}}\n", objects.join(","), scaffolds.join(","))
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let (mut words, mut as_json) = (Vec::new(), false);
    for argument in arguments {
        match argument.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            "--json" => as_json = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}\n{USAGE}")),
            word => words.push(word.to_string()),
        }
    }
    let root = crate::compile::root();
    let chosen = if words.is_empty() {
        let unfinished: Vec<String> =
            built::games()?.into_iter().filter(|game| root.join("recon").join(game).is_dir()).map(str::to_string).collect();
        if unfinished.is_empty() {
            println!("every game is done: no recon/ left");
            return Ok(());
        }
        built::editions(&unfinished)?
    } else {
        built::editions(&words)?
    };
    let (mut objects, mut scaffolds) = (Vec::new(), Vec::new());
    for game in built::games()? {
        let editions = of_game(&chosen, game);
        if editions.is_empty() {
            continue;
        }
        let (found, held) = survey(root, game, &editions)?;
        objects.extend(found);
        scaffolds.extend(held);
    }
    print!("{}", if as_json { json(&objects, &scaffolds) } else { text(&objects, &scaffolds) });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> DecompTargetId {
        crate::alchemy::targets::parse_decomp_target(name).unwrap()
    }

    #[test]
    fn json_quotes_and_lists() {
        assert_eq!(quote("a\"b\\c\n"), "\"a\\\"b\\\\c\\u000a\"");
        let object = Object {
            object: "games/tla/src/window/f039864".into(),
            file: "games/tla/src/window/f039864.c".into(),
            linked: vec![id("tla-en")],
            missing: vec![id("tla-ja"), id("tla-de")],
            names: vec![
                ("Message_StepScript".into(), vec![Held { edition: id("tla-ja"), section: ".rom.00039858".into(), size: 1548 }]),
                ("Message_Other".into(), vec![]),
            ],
        };
        let scaffold = Scaffold {
            edition: id("tla-ja"),
            source: "recon/tla/ja/rom.s".into(),
            unlinked: 1,
            linked_sections: 1,
            linked_bytes: 1548,
            other_bytes: 0,
            unnamed_bytes: 0,
        };
        assert_eq!(
            json(std::slice::from_ref(&object), std::slice::from_ref(&scaffold)),
            "{\"objects\":[{\"file\":\"games/tla/src/window/f039864.c\",\"object\":\"games/tla/src/window/f039864\",\
             \"linked\":[\"tla-en\"],\"missing\":[\"tla-ja\",\"tla-de\"],\"names\":[{\"name\":\"Message_StepScript\",\
             \"scaffold\":[{\"edition\":\"tla-ja\",\"section\":\".rom.00039858\",\"size\":1548}]},\
             {\"name\":\"Message_Other\",\"scaffold\":[]}]}],\"scaffolds\":[{\"edition\":\"tla-ja\",\
             \"source\":\"recon/tla/ja/rom.s\",\"unlinked\":1,\"linkedSections\":1,\"linkedBytes\":1548,\"otherBytes\":0,\"unnamedBytes\":0}]}\n"
        );
        let shown = text(&[object], &[scaffold]);
        assert!(shown.starts_with(
            "games/tla/src/window/f039864.c: linked by en; not by ja de\n  Message_StepScript: ja .rom.00039858 (1,548 bytes)\n  Message_Other: not in the scaffold of ja\n"
        ), "{shown}");
        assert!(shown.ends_with("tla-ja: 1 objects not linked; recon/tla/ja/rom.s holds 1,548 bytes in 1 sections with a name English links as C, 0 bytes with names English does not, 0 bytes with no name\n"), "{shown}");
    }

    /// A built scaffold's sections and names, read from its symbol table,
    /// agree with its source: one section per `.section .rom.<offset>`, sized
    /// by its `.incbin`s, holding the names its `.global`s declare.
    #[test]
    fn a_scaffold_object_reads_as_its_source() {
        let root = crate::compile::root();
        for id in [id("tla-ja"), id("tla-de")] {
            let Some((source, object)) = built::scaffold(root, id) else { continue };
            let Ok(read) = sections(&object) else { continue };
            let text = std::fs::read_to_string(root.join(source)).unwrap();
            let mut written: Vec<Section> = Vec::new();
            for line in text.lines().map(str::trim) {
                if let Some(name) = line.strip_prefix(".section ") {
                    let name = name.split(',').next().unwrap().trim().to_string();
                    written.push(Section { name, size: 0, names: Vec::new() });
                } else if let Some(name) = line.strip_prefix(".global ") {
                    written.last_mut().unwrap().names.push(name.trim().to_string());
                } else if let Some(arguments) = line.strip_prefix(".incbin ") {
                    let size = arguments.rsplit(',').next().unwrap().trim();
                    written.last_mut().unwrap().size += u64::from_str_radix(size.trim_start_matches("0x"), 16).unwrap();
                }
            }
            written.sort_by(|a, b| a.name.cmp(&b.name));
            let mut read = read;
            read.sort_by(|a, b| a.name.cmp(&b.name));
            for section in &mut read {
                section.names.sort();
            }
            for section in &mut written {
                section.names.sort();
            }
            assert_eq!(read, written, "{id}");
        }
    }
}
