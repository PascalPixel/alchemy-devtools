//! The party each scene is played with: who has joined by that point of the
//! story, and how strong they are.
//!
//! A run starts from a new game's defaults (Isaac alone, level 1), so a
//! scene of Altin Peak would show him fighting its water beast alone. Each
//! scene instead takes a chapter of the story: one of Camelot's own debug
//! parameter sets (DebugParameterSet_Load, gDebug_ParameterSets in games/tbs/src/battle/debug_parameter_sets.c),
//! which raises each member to a level and gives them Djinn, Psynergy and
//! equipment, and the preset's members. Presets 10 to 70 follow the story:
//! Isaac, Garet and Jenna at level 1 or 2, Isaac and Garet at 3, Ivan
//! joining at 4 and 5, three members from level 6 to 9, Mia joining at 10,
//! four members from 11 to 17, Isaac alone at 18 and 19 (the Colosso), and
//! four again from 20 to 33. Each area takes the preset whose members are
//! the party there and whose level fits where the area falls between those
//! joins. Which preset fits which maps is this renderer's choice: nothing in
//! the game ties a preset to a map. The run's own flags then decide the
//! party flags an event needs (FLAG_IN_PARTY), and the party is listed again
//! from them.
//!
//! The stormy night keeps a new game's level-1 children (FLAG_CHILDHOOD is
//! set by the run's flags there).

use super::source;
use std::sync::OnceLock;

/// One stretch of the story: the maps it covers, as runs of gScenes
/// from one SceneId_ to another, the debug preset that sets the party's
/// strength (none for a new game's level 1), and the members it names
/// (character_ids.h) where the party is not the preset's.
struct Chapter {
    maps: &'static [(&'static str, &'static str)],
    preset: Option<u32>,
    members: Option<&'static [&'static str]>,
}

/// The chapters, in the story's order.
const CHAPTERS: &[Chapter] = &[
    // The stormy night: Isaac and Garet as children, a new game's level 1.
    Chapter {
        maps: &[("ValePrologue", "ValePrologue")],
        preset: None,
        members: Some(&["ISAAC", "GARET"]),
    },
    // Vale three years on, before Sol Sanctum: Jenna, in the preset, has
    // not joined yet.
    Chapter {
        maps: &[("Vale", "ValeEruption")],
        preset: Some(10),
        members: Some(&["ISAAC", "GARET"]),
    },
    // Sol Sanctum and Mt. Aleph's eruption, with Jenna.
    Chapter {
        maps: &[("SolSanctumLunaSolRooms", "SolEntrance")],
        preset: Some(11),
        members: None,
    },
    // The world map and Vault: Isaac and Garet set out; Ivan joins in Vault.
    Chapter {
        maps: &[("WorldMap", "WorldMap"), ("Vault", "StoneRooms")],
        preset: Some(12),
        members: None,
    },
    // Goma Cave, Bilibin and its barricade, Vault revisited.
    Chapter {
        maps: &[("VaultRevisit", "GomaCaveBridge")],
        preset: Some(14),
        members: None,
    },
    Chapter {
        maps: &[("Bilibin", "BilibinBarricade")],
        preset: Some(20),
        members: None,
    },
    // Kolima, its forest and Tret, and the caves by Vale and Vault.
    Chapter {
        maps: &[
            ("Kolima", "TretTreeBranch"),
            ("ValeCaveMouth", "VaultCaveMouth"),
        ],
        preset: Some(21),
        members: None,
    },
    // Bilibin Cave, Imil.
    Chapter {
        maps: &[("BilibinCaveIce", "ImilFallsCave")],
        preset: Some(23),
        members: None,
    },
    // Mercury Lighthouse, where Mia joins.
    Chapter {
        maps: &[(
            "MercuryLighthouseEntrance",
            "UnusedMercuryLighthouseAerieCopy",
        )],
        preset: Some(25),
        members: None,
    },
    // Fuchin Temple and its falls.
    Chapter {
        maps: &[("FuchinTempleGrounds", "FuchinFallsCaveDragonStatue")],
        preset: Some(30),
        members: None,
    },
    // Mogall Forest, Xian, Alpine Crossing.
    Chapter {
        maps: &[("MogallForestSign", "AlpineCrossing")],
        preset: Some(32),
        members: None,
    },
    // Altin and Altin Peak.
    Chapter {
        maps: &[("Altin", "AltinPeakWestExit")],
        preset: Some(34),
        members: None,
    },
    // Lama Temple and the Lamakan Desert.
    Chapter {
        maps: &[("LamaTemple", "LamakanDesertCave")],
        preset: Some(40),
        members: None,
    },
    // Kalay, its docks and Hammet's palace.
    Chapter {
        maps: &[("Kalay", "HammetPalace"), ("KalayDocks", "CottageRoomsE")],
        preset: Some(42),
        members: None,
    },
    // The Tolbi-bound Ship and Tolbi Docks.
    Chapter {
        maps: &[("TolbiShipDeck", "TolbiDocks")],
        preset: Some(43),
        members: None,
    },
    // Tolbi and Babi's palace.
    Chapter {
        maps: &[
            ("Tolbi", "BabiPalaceEntrance"),
            ("TolbiSpring", "TolbiSpring"),
        ],
        preset: Some(51),
        members: None,
    },
    // The Colosso: Isaac alone.
    Chapter {
        maps: &[("Colosseum", "ColosseumNavampaMatch")],
        preset: Some(53),
        members: None,
    },
    // Altmiller Cave, Gondowan Passage, Kalay Tunnel, Gondowan Cave.
    Chapter {
        maps: &[("AltmillerCaveEntrance", "GondowanPassage")],
        preset: Some(60),
        members: None,
    },
    Chapter {
        maps: &[("KalayTunnelEntrance", "GondowanCave")],
        preset: Some(61),
        members: None,
    },
    // Lunpa, its fortress and West Lunpa Cave.
    Chapter {
        maps: &[
            ("Lunpa", "WestLunpaCave"),
            ("LunpaFortressGates", "LunpaFortressToadonpa"),
        ],
        preset: Some(63),
        members: None,
    },
    // The Suhalla Desert, Suhalla, its gate, Gateway Cave, the Tunnel Ruins.
    Chapter {
        maps: &[("SuhallaDesertNorth", "TunnelRuinsLadyStatues")],
        preset: Some(64),
        members: None,
    },
    // Babi Lighthouse, Lalivero, the Lemurian Ship.
    Chapter {
        maps: &[
            ("BabiLighthouseStoneDoor", "TimberRoomsC"),
            ("LemurianShip", "LemurianShip"),
        ],
        preset: Some(65),
        members: None,
    },
    // Crossbone Isle, open to the end.
    Chapter {
        maps: &[("CrossboneIsle", "CrossboneIsleNinthFloor")],
        preset: Some(67),
        members: None,
    },
    // Venus Lighthouse.
    Chapter {
        maps: &[("VenusLighthouseEntry", "VenusLighthouseAerie")],
        preset: Some(69),
        members: None,
    },
];

/// A chapter read through the source: its maps by number, its members by
/// character number in owner order, and its preset.
pub struct Party {
    maps: Vec<(u16, u16)>,
    pub members: Vec<u8>,
    pub preset: Option<u32>,
}

fn resolve(chapter: &Chapter) -> Result<Party, String> {
    let maps = chapter
        .maps
        .iter()
        .map(|&(first, last)| Ok((source::map_id(first)?, source::map_id(last)?)))
        .collect::<Result<_, String>>()?;
    let mut members = match (chapter.members, chapter.preset) {
        (Some(names), _) => names
            .iter()
            .map(|name| source::character(name))
            .collect::<Result<Vec<u8>, String>>()?,
        (None, Some(preset)) => source::preset_members(preset)?,
        (None, None) => return Err("a chapter without a preset names its members".into()),
    };
    members.sort_unstable();
    Ok(Party {
        maps,
        members,
        preset: chapter.preset,
    })
}

/// The chapter scene `scene` falls in, if any.
pub fn chapter(scene: u16) -> Result<Option<&'static Party>, String> {
    static PARTIES: OnceLock<Result<Vec<Party>, String>> = OnceLock::new();
    let parties = PARTIES
        .get_or_init(|| CHAPTERS.iter().map(resolve).collect())
        .as_ref()
        .map_err(Clone::clone)?;
    Ok(parties
        .iter()
        .find(|p| p.maps.iter().any(|&(a, b)| (a..=b).contains(&scene))))
}

/// The party's order once the run's flags have decided who is in it: the
/// chapter's members first, then any other member the flags add, by number.
pub fn order(members: &[u8], flags: u8) -> Vec<u8> {
    let mut out: Vec<u8> = members
        .iter()
        .copied()
        .filter(|&m| flags & (1 << m) != 0)
        .collect();
    for m in 0..8u8 {
        if flags & (1 << m) != 0 && !out.contains(&m) {
            out.push(m);
        }
    }
    out
}

/// `party` with `leader` moved to its head (added where it lacks him).
pub fn lead(mut party: Vec<u8>, leader: Option<u8>) -> Vec<u8> {
    if let Some(leader) = leader {
        party.retain(|&m| m != leader);
        party.insert(0, leader);
    }
    party
}

/// The characters by number, as both games' character_ids.h number them.
const CHARACTERS: [&str; 8] = [
    "ISAAC", "GARET", "IVAN", "MIA", "FELIX", "JENNA", "SHEBA", "PIERS",
];

/// A character's number by name, with or without capitals (felix, FELIX).
pub fn character_number(name: &str) -> Result<u8, String> {
    let upper = name.trim().to_ascii_uppercase();
    CHARACTERS
        .iter()
        .position(|&c| c == upper)
        .map(|i| i as u8)
        .ok_or_else(|| {
            format!(
                "{name:?} is no character ({})",
                CHARACTERS.join(", ").to_ascii_lowercase()
            )
        })
}

/// The debug set of The Lost Age that gives a staged party its strength:
/// the first, past the test sets at level 50 and 90, that holds every
/// member (`sets` as tla_sets reads them).
fn staged_set(sets: &[Vec<u8>], members: &[u8]) -> Option<u32> {
    sets.iter()
        .position(|set| {
            !set.is_empty() && members.iter().all(|m| set.contains(m)) && set.len() >= members.len()
        })
        .map(|i| i as u32)
}

/// The party `--party` and `--leader` stage (field::Lineup): the members
/// named, in order, the leader first; in The Lost Age at the strength of
/// the first story debug set that holds them all.
pub fn lineup(
    party: &[String],
    leader: Option<&str>,
    tla: bool,
) -> Result<Option<super::field::Lineup>, String> {
    if party.is_empty() && leader.is_none() {
        return Ok(None);
    }
    let leader = leader.map(character_number).transpose()?;
    let members = lead(
        party
            .iter()
            .map(|n| character_number(n))
            .collect::<Result<Vec<u8>, String>>()?,
        if party.is_empty() { None } else { leader },
    );
    let preset = if tla && !members.is_empty() {
        let path = crate::compile::root().join("games/tla/src/battle/debug_parameter_sets.c");
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let sets = tla_sets(&text);
        // Past the test sets (level 50 and 90): the story's own run from
        // the first set of level 5.
        let story = sets.iter().position(|s| s.len() == 3).unwrap_or(0);
        staged_set(&sets[story..], &members).map(|i| i + story as u32)
    } else {
        None
    };
    Ok(Some(super::field::Lineup {
        members,
        leader,
        preset,
    }))
}

/// The Lost Age's debug parameter sets (gDebug_ParameterSets in
/// games/tla/src/battle/debug_parameter_sets.c): each set's members, in its
/// rows' order, the sets parted by a row whose owner is
/// DEBUG_PARAMETER_SET_END, as DebugParty_LoadPreset counts them.
fn tla_sets(text: &str) -> Vec<Vec<u8>> {
    let mut sets = vec![Vec::new()];
    let rows = text
        .split_once("gDebug_ParameterSets[]")
        .map_or("", |(_, rows)| rows);
    for line in rows.lines() {
        let Some(owner) = line.trim().strip_prefix(".owner = ") else {
            continue;
        };
        match CHARACTERS
            .iter()
            .position(|&name| name == owner.trim_end_matches(','))
        {
            Some(character) => sets.last_mut().expect("a set").push(character as u8),
            None => sets.push(Vec::new()),
        }
    }
    sets
}

/// The party of The Lost Age's debug set `preset`, as a guide recording
/// enters a scene with it: the set loaded as a new game is set up, its
/// members the party in owner order.
pub fn tla(preset: u32) -> Result<&'static Party, String> {
    static SETS: OnceLock<Result<Vec<Party>, String>> = OnceLock::new();
    let sets = SETS
        .get_or_init(|| {
            let path = crate::compile::root().join("games/tla/src/battle/debug_parameter_sets.c");
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            Ok(tla_sets(&text)
                .into_iter()
                .enumerate()
                .map(|(set, mut members)| {
                    members.sort_unstable();
                    Party {
                        maps: Vec::new(),
                        members,
                        preset: Some(set as u32),
                    }
                })
                .collect())
        })
        .as_ref()
        .map_err(Clone::clone)?;
    sets.get(preset as usize)
        .filter(|p| !p.members.is_empty())
        .ok_or_else(|| format!("The Lost Age has no debug set {preset}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenes_fall_in_one_chapter_each() {
        let parties: Vec<Party> = CHAPTERS
            .iter()
            .map(resolve)
            .collect::<Result<_, _>>()
            .unwrap();
        for scene in 0..201u16 {
            assert!(
                parties
                    .iter()
                    .filter(|p| p.maps.iter().any(|&(a, b)| (a..=b).contains(&scene)))
                    .count()
                    <= 1,
                "scene {scene}"
            );
        }
        for p in &parties {
            assert!(p.maps.iter().all(|&(a, b)| a <= b));
        }
        let altin = source::map_id("AltinPeakWestExit").unwrap();
        assert_eq!(chapter(altin).unwrap().map(|p| p.members.len()), Some(4));
        let storm = source::map_id("ValePrologue").unwrap();
        assert_eq!(chapter(storm).unwrap().and_then(|p| p.preset), None);
    }

    #[test]
    fn the_lost_age_s_debug_sets_part_at_owner_minus_one() {
        let text = "struct DebugPartyEntry gDebug_ParameterSets[] ALIGNED(4) = {\n    {\n        .owner = FELIX,\n        .level = 90,\n    },\n    {\n        .owner = DEBUG_PARAMETER_SET_END,\n    },\n    {\n        .owner = JENNA,\n    },\n    {\n        .owner = FELIX,\n    },\n};\n";
        assert_eq!(tla_sets(text), vec![vec![4], vec![5, 4]]);
        // The checkout's: set 7 is Felix, Jenna and Sheba, set 19 the four
        // once Piers has joined.
        assert_eq!(tla(7).unwrap().members, vec![4, 5, 6]);
        assert_eq!(tla(19).unwrap().members, vec![4, 5, 6, 7]);
    }

    #[test]
    fn a_staged_party_has_its_leader_first_and_a_story_set_s_strength() {
        assert_eq!(lead(vec![4, 5, 6], Some(5)), vec![5, 4, 6]);
        assert_eq!(lead(vec![4], Some(5)), vec![5, 4]);
        assert_eq!(character_number("jenna"), Ok(5));
        assert!(character_number("kraden").is_err());
        let sets = vec![vec![4], vec![4, 5, 6], vec![4, 5, 6, 7]];
        assert_eq!(staged_set(&sets, &[5, 4]), Some(1));
        assert_eq!(staged_set(&sets, &[7, 5]), Some(2));
        let staged = lineup(
            &["jenna".into(), "felix".into(), "piers".into()],
            None,
            true,
        )
        .unwrap()
        .unwrap();
        assert_eq!(staged.members, vec![5, 4, 7]);
        // Set 19 is the first of the story's sets with Piers in it.
        assert_eq!(staged.preset, Some(19));
        let leader = lineup(&[], Some("jenna"), true).unwrap().unwrap();
        assert_eq!(
            (leader.members.len(), leader.leader, leader.preset),
            (0, Some(5), None)
        );
    }

    #[test]
    fn the_flags_decide_the_party() {
        assert_eq!(order(&[0, 1, 2], 0b0011), vec![0, 1]);
        assert_eq!(order(&[0, 1], 0b0111), vec![0, 1, 2]);
    }
}
