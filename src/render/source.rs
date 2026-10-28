//! Game facts the renderer reads from the Broken Seal's source rather than
//! keeping copies of: map numbers (gScenes), area names
//! (gArea_NameRules and text/en.po), ability numbers and names
//! (ability_ids.h, MsgAbilityName), actors' ids (each map's enum),
//! characters (character_ids.h) and the debug party presets
//! (gDebug_ParameterSets, read from the build by tables.rs).

use std::collections::HashMap;
use std::path::PathBuf;

fn tbs(path: &str) -> PathBuf {
    crate::compile::root().join("games/tbs").join(path)
}

fn read(path: &str) -> Result<String, String> {
    let path = tbs(path);
    std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

/// A decimal or `0x` number, signed.
fn number(text: &str) -> Option<i64> {
    let text = text.trim();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest.trim()),
        None => (false, text),
    };
    let value = match digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        Some(hex) => i64::from_str_radix(hex, 16).ok()?,
        None => digits.parse().ok()?,
    };
    Some(if negative { -value } else { value })
}

/// Each use of the assembler macros `names`, with its arguments, after `label:` up to
/// the end of that table, with `.if EDITION_INTERNATIONAL` taken as the
/// English edition the renderer runs takes it.
fn macro_rows(
    text: &str,
    label: &str,
    names: &[&str],
) -> Result<Vec<(String, Vec<String>)>, String> {
    let start = text
        .find(&format!("\n{label}:"))
        .ok_or_else(|| format!("no {label}"))?
        + 1;
    let mut rows = Vec::new();
    let mut taken = vec![true];
    for line in text[start..].lines().skip(1) {
        let line = line.split('@').next().unwrap_or("").trim();
        let (word, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        match word {
            ".if" if rest.trim() == "EDITION_INTERNATIONAL" => taken.push(true),
            ".if" => return Err(format!("{label}: unknown condition {rest}")),
            ".else" => {
                let last = taken.pop().ok_or(format!("{label}: .else without .if"))?;
                taken.push(!last);
            }
            ".endif" => {
                taken.pop();
            }
            _ if names.contains(&word) => {
                if taken.iter().all(|&t| t) {
                    rows.push((
                        word.to_string(),
                        rest.split(',').map(|a| a.trim().to_string()).collect(),
                    ));
                }
            }
            _ if word.is_empty() => {}
            ".global" | ".balign" | ".section" => break,
            _ if word.ends_with(':') => break,
            _ => {}
        }
    }
    Ok(rows)
}

/// The scenes in gScenes' order (field/scenes.s): each one's name
/// without `SceneId_` (empty for an unnamed row) and its area.
fn maps() -> Result<Vec<(String, i64)>, String> {
    let text = read("src/field/scenes.s")?;
    macro_rows(&text, "gScenes", &["scene"])?
        .into_iter()
        .map(|(_, row)| {
            let area = row
                .get(2)
                .and_then(|m| number(m))
                .ok_or_else(|| format!("scene {row:?}: no area"))?;
            Ok((row[0].clone(), area))
        })
        .collect()
}

/// The number of map `SceneId_<name>`, its row of gScenes.
pub fn map_id(name: &str) -> Result<u16, String> {
    maps()?
        .iter()
        .position(|(n, _)| n == name)
        .map(|i| i as u16)
        .ok_or_else(|| format!("scenes.s has no SceneId_{name}"))
}

/// The message key of the PO entry named `name` in text/en.po.
fn message_number(catalog: &crate::po::Catalog, name: &str) -> Result<usize, String> {
    let entry = catalog
        .entries
        .iter()
        .find(|e| e.context.as_deref() == Some(name))
        .ok_or_else(|| format!("en.po has no {name}"))?;
    entry
        .id
        .parse()
        .map_err(|_| format!("en.po: {name} has key {:?}", entry.id))
}

/// The area name the game gives map `map` entered by entrance
/// `entrance`, as Area_GetNameIndex finds it: the first rule of
/// gArea_NameRules naming the map, or else its area, with that
/// entrance or any; the name is that many messages after
/// MsgDebugAreaName.
pub fn area_name(map: u16, entrance: u16) -> Result<String, String> {
    let maps = maps()?;
    let area = maps
        .get(usize::from(map))
        .ok_or_else(|| format!("no map {map}"))?
        .1;
    let rules = read("src/overworld/scene_rules.s")?;
    // Before The Broken Seal's rename the table was gMapSec_NameRules and
    // its rows map_name and group_name.
    let kinds = ["scene_name", "area_name", "map_name", "group_name"];
    let rows = macro_rows(&rules, "gArea_NameRules", &kinds)
        .or_else(|_| macro_rows(&rules, "gMapSec_NameRules", &kinds))?;
    let mut index = 0;
    for (kind, args) in rows {
        let [id, dest, name] = &args[..] else {
            return Err(format!("scene_rules.s: {kind} {args:?}"));
        };
        let matches = if kind == "scene_name" || kind == "map_name" {
            maps.iter().position(|(n, _)| n == id) == Some(usize::from(map))
        } else {
            number(id) == Some(area)
        };
        if matches && (number(dest) == Some(-1) || number(dest) == Some(i64::from(entrance))) {
            index = number(name).ok_or_else(|| format!("scene_rules.s: {kind} {args:?}"))? as usize;
            break;
        }
    }
    let catalog = crate::po::read_catalog(&tbs("text/en.po"))?;
    let key = message_number(&catalog, "MsgDebugAreaName").or_else(|_| message_number(&catalog, "MsgDebugEntryName"))? + index;
    catalog
        .entries
        .iter()
        .find(|e| e.id.parse::<usize>().ok() == Some(key))
        .map(|e| e.value.clone())
        .ok_or_else(|| format!("en.po has no message {key}"))
}

/// The enumerators of every `enum` in C source `text`: each enum's tag
/// (none for an anonymous one) and its names and values, which may count on
/// from `known` names; a value that counts on a name not known here is none.
fn enums(
    text: &str,
    known: &HashMap<String, i64>,
) -> Result<Vec<(Option<String>, Vec<(String, Option<i64>)>)>, String> {
    let plain = regex::Regex::new(r"(?s)/\*.*?\*/|//[^\n]*")
        .map_err(|e| e.to_string())?
        .replace_all(text, "");
    let pattern =
        regex::Regex::new(r"(?s)\benum\s*(\w*)\s*\{([^}]*)\}").map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for found in pattern.captures_iter(&plain) {
        let tag = (!found[1].is_empty()).then(|| found[1].to_string());
        let mut values: Vec<(String, Option<i64>)> = Vec::new();
        let mut next = Some(0);
        for item in found[2].split(',').map(str::trim).filter(|i| !i.is_empty()) {
            let (name, value) = match item.split_once('=') {
                Some((name, expr)) => {
                    let lookup = |term: &str| {
                        number(term)
                            .or_else(|| values.iter().find(|(n, _)| n == term).and_then(|v| v.1))
                            .or_else(|| known.get(term).copied())
                    };
                    let sum = expr
                        .split('+')
                        .map(|part| {
                            lookup(part.trim().trim_matches(|c| c == '(' || c == ')').trim())
                        })
                        .sum::<Option<i64>>();
                    (name.trim(), sum)
                }
                None => (item, next),
            };
            values.push((name.to_string(), value));
            next = value.map(|v| v + 1);
        }
        out.push((tag, values));
    }
    Ok(out)
}

/// The value of enumerator `name` in header `path`.
fn enumerator(path: &str, name: &str) -> Result<i64, String> {
    enums(&read(path)?, &HashMap::new())?
        .into_iter()
        .flat_map(|(_, values)| values)
        .find(|(n, _)| n == name)
        .and_then(|(_, v)| v)
        .ok_or_else(|| format!("{path} gives no value to {name}"))
}

/// The number of ability `name` (`ABILITY_MOVE`), from ability_ids.h.
pub fn ability_id(name: &str) -> Result<u16, String> {
    enumerator("include/ability_ids.h", name).map(|v| v as u16)
}

/// The game's name for ability `id`: message MsgAbilityName plus `id`.
pub fn ability_name(id: u16) -> Result<String, String> {
    super::tables::Game::here()?
        .ability_names()?
        .into_iter()
        .nth(usize::from(id))
        .ok_or_else(|| format!("gAbilityDefinitions has no row {id}"))
}

/// The actor id of actor `name` on map `SceneId_<map>`, from the map
/// overlays' enums: the map's own `enum <map>ActorId` where it has one,
/// else the one value every map overlay gives the name. `name` may list its
/// spellings, the new first (`ACTOR_PILLAR|LOCALID_PILLAR`): before The
/// Broken Seal's rename the ids were `LOCALID_`, a map's enum
/// `<map>LocalId` and the shared header map_tables.h.
pub fn actor_id(map: &str, name: &str) -> Result<u16, String> {
    let names: Vec<&str> = name.split('|').collect();
    let shared = read("include/scene_tables.h").or_else(|_| read("include/map_tables.h"))?;
    let mut known = HashMap::new();
    for (_, values) in enums(&shared, &HashMap::new())? {
        known.extend(values.into_iter().filter_map(|(n, v)| Some((n, v?))));
    }
    let dir = tbs("src/scene");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    let own = [format!("{map}ActorId"), format!("{map}LocalId")];
    let mut found = Vec::new();
    for path in files
        .iter()
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("c" | "h")))
    {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if !names.iter().any(|n| text.contains(n)) {
            continue;
        }
        for (tag, values) in enums(&text, &known)? {
            if let Some((_, value)) = values.iter().find(|(n, _)| names.contains(&n.as_str())) {
                let value = value.ok_or_else(|| format!("cannot evaluate {name}"))?;
                if tag.as_ref().is_some_and(|tag| own.contains(tag)) {
                    return Ok(value as u16);
                }
                found.push(value);
            }
        }
    }
    found.dedup();
    match found[..] {
        [value] => Ok(value as u16),
        [] => Err(format!("no scene overlay names {name}")),
        _ => Err(format!(
            "{name} has several values and SceneId_{map} has no enum {}",
            own[0]
        )),
    }
}

/// Character `name`'s number (`ISAAC`), from character_ids.h.
pub fn character(name: &str) -> Result<u8, String> {
    let text = read("include/game/character_ids.h")?;
    text.lines()
        .find_map(|line| {
            let mut words = line.strip_prefix("#define ")?.split_whitespace();
            (words.next()? == name).then(|| number(words.next()?))?
        })
        .map(|v| v as u8)
        .ok_or_else(|| format!("character_ids.h has no {name}"))
}

/// The characters' names in character_ids.h (`ISAAC` to `PIERS`), by number:
/// its first run of definitions numbered from 0.
pub fn character_names() -> Result<Vec<String>, String> {
    let text = read("include/game/character_ids.h")?;
    let mut names = Vec::new();
    for line in text.lines() {
        let mut words = line
            .strip_prefix("#define ")
            .unwrap_or("")
            .split_whitespace();
        let (Some(name), Some(value)) = (words.next(), words.next().and_then(number)) else {
            continue;
        };
        if value != names.len() as i64 {
            break;
        }
        names.push(name.to_string());
    }
    if names.is_empty() {
        Err("character_ids.h numbers no characters".into())
    } else {
        Ok(names)
    }
}

/// The members debug party preset `preset` raises (gDebug_ParameterSets, read
/// by DebugParty_LoadPreset: runs of rows, each ended by DEBUG_PRESET_END).
pub fn preset_members(preset: u32) -> Result<Vec<u8>, String> {
    let game = super::tables::Game::here()?;
    let end = game.constant("DEBUG_PRESET_END")?;
    let mut current = 0;
    let mut members = Vec::new();
    for unit in game.table("gDebug_ParameterSets", "DebugPartyEntry")?.column("unitId")? {
        if unit == end {
            if current == preset {
                return Ok(members);
            }
            current += 1;
        } else if current == preset {
            members.push(unit as u8);
        }
    }
    Err(format!("gDebug_ParameterSets has no preset {preset}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerators_count_on_from_names() {
        let known = HashMap::from([("ACTOR_FIRST_PLACED".to_string(), 8)]);
        let text = "enum ValeActorId {\n  A = ACTOR_FIRST_PLACED, /* x, y */\n  B, // z\n  C = B + 3,\n  D ///< w\n};\nenum { E = 0x10 };";
        let found = enums(text, &known).unwrap();
        assert_eq!(found[0].0.as_deref(), Some("ValeActorId"));
        assert_eq!(
            found[0].1,
            vec![
                ("A".into(), Some(8)),
                ("B".into(), Some(9)),
                ("C".into(), Some(12)),
                ("D".into(), Some(13))
            ]
        );
        assert_eq!(found[1], (None, vec![("E".into(), Some(16))]));
        assert_eq!(
            enums("enum { F = UNKNOWN, G };", &known).unwrap()[0].1,
            vec![("F".into(), None), ("G".into(), None)]
        );
    }

    #[test]
    fn the_checkout_answers() {
        assert_eq!(map_id("Vale").unwrap(), map_id("ValePrologue").unwrap() + 1);
        assert_eq!(area_name(map_id("Vale").unwrap(), 1).unwrap(), "Vale");
        assert!(actor_id("Vale", "ACTOR_PILLAR|LOCALID_PILLAR").unwrap() >= 8);
        assert!(actor_id("CottageRoomsC", "ACTOR_BILIBIN_INNKEEPER|LOCALID_BILIBIN_INNKEEPER").is_ok());
        assert_eq!(
            character_names().unwrap()[usize::from(character("JENNA").unwrap())],
            "JENNA"
        );
        assert_eq!(character_names().unwrap().len(), 8);
        assert_eq!(
            ability_name(ability_id("ABILITY_MOVE").unwrap()).unwrap(),
            "Move"
        );
        assert!(preset_members(0).unwrap().len() == 4);
    }

    #[test]
    fn macro_rows_take_the_english_edition() {
        let text = "\nTable:\n\tm a, 1\n\t.if EDITION_INTERNATIONAL\n\tm b, 2\n\t.else\n\tm c, 3\n\t.endif\n\tm d, 4 @ note\n\t.global Next\nNext:\n\tm e, 5\n";
        let rows = macro_rows(text, "Table", &["m"]).unwrap();
        let names: Vec<&str> = rows.iter().map(|(_, r)| r[0].as_str()).collect();
        assert_eq!(names, ["a", "b", "d"]);
    }
}
