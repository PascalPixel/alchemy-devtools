//! The party each scene is played with: who has joined by that point of the
//! story, and how strong they are.
//!
//! A run starts from a new game's defaults (Isaac alone, level 1), so a
//! scene of Altin Peak would show him fighting its water beast alone. Each
//! scene instead takes a chapter of the story: one of Camelot's own debug
//! party presets (DebugParty_LoadPreset, games/tbs/src/battle/presets.tsv),
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

/// One stretch of the story: the maps it covers, as runs of gMapHeaders
/// from one MapId_ to another, the debug preset that sets the party's
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
    Chapter { maps: &[("ValeStorm", "ValeStorm")], preset: None, members: Some(&["ISAAC", "GARET"]) },
    // Vale three years on, before Sol Sanctum: Jenna, in the preset, has
    // not joined yet.
    Chapter { maps: &[("Vale", "ValeEruption")], preset: Some(10), members: Some(&["ISAAC", "GARET"]) },
    // Sol Sanctum and Mt. Aleph's eruption, with Jenna.
    Chapter { maps: &[("SolSanctumLunaSolRooms", "SolEntrance")], preset: Some(11), members: None },
    // The world map and Vault: Isaac and Garet set out; Ivan joins in Vault.
    Chapter { maps: &[("WorldMap", "WorldMap"), ("Vault", "VaultJail")], preset: Some(12), members: None },
    // Goma Cave, Bilibin and its barricade, Vault revisited.
    Chapter { maps: &[("VaultRevisit", "GomaCaveBridge")], preset: Some(14), members: None },
    Chapter { maps: &[("Bilibin", "BilibinBarricade")], preset: Some(20), members: None },
    // Kolima, its forest and Tret, and the caves by Vale and Vault.
    Chapter { maps: &[("Kolima", "TretTreeBranch"), ("ValeCaveMouth", "VaultCaveMouth")], preset: Some(21), members: None },
    // Bilibin Cave, Imil.
    Chapter { maps: &[("BilibinCaveIce", "ImilFallsCave")], preset: Some(23), members: None },
    // Mercury Lighthouse, where Mia joins.
    Chapter { maps: &[("MercuryLighthouseEntrance", "UnusedMercuryLighthouseAerieCopy")], preset: Some(25), members: None },
    // Fuchin Temple and its falls.
    Chapter { maps: &[("FuchinTempleGrounds", "FuchinFallsCaveDragonStatue")], preset: Some(30), members: None },
    // Mogall Forest, Xian, Alpine Crossing.
    Chapter { maps: &[("MogallForestSign", "AlpineCrossing")], preset: Some(32), members: None },
    // Altin and Altin Peak.
    Chapter { maps: &[("Altin", "AltinPeakWestExit")], preset: Some(34), members: None },
    // Lama Temple and the Lamakan Desert.
    Chapter { maps: &[("LamaTemple", "LamakanDesertCave")], preset: Some(40), members: None },
    // Kalay, its docks and Hammet's palace.
    Chapter { maps: &[("Kalay", "HammetPalace"), ("KalayDocks", "KalayTolbiDocksInteriors")], preset: Some(42), members: None },
    // The Tolbi-bound Ship and Tolbi Docks.
    Chapter { maps: &[("TolbiShipDeck", "TolbiDocks")], preset: Some(43), members: None },
    // Tolbi and Babi's palace.
    Chapter { maps: &[("Tolbi", "BabiPalaceEntrance"), ("TolbiSpring", "TolbiSpring")], preset: Some(51), members: None },
    // The Colosso: Isaac alone.
    Chapter { maps: &[("Colosseum", "ColosseumNavampaMatch")], preset: Some(53), members: None },
    // Altmiller Cave, Gondowan Passage, Kalay Tunnel, Gondowan Cave.
    Chapter { maps: &[("AltmillerCaveEntrance", "GondowanPassage")], preset: Some(60), members: None },
    Chapter { maps: &[("KalayTunnelEntrance", "GondowanCave")], preset: Some(61), members: None },
    // Lunpa, its fortress and West Lunpa Cave.
    Chapter { maps: &[("Lunpa", "WestLunpaCave"), ("LunpaFortressGates", "LunpaFortressToadonpa")], preset: Some(63), members: None },
    // The Suhalla Desert, Suhalla, its gate, Gateway Cave, the Tunnel Ruins.
    Chapter { maps: &[("SuhallaDesertNorth", "TunnelRuinsLadyStatues")], preset: Some(64), members: None },
    // Babi Lighthouse, Lalivero, the Lemurian Ship.
    Chapter { maps: &[("BabiLighthouseStoneDoor", "LaliveroFaransHouse"), ("LemurianShip", "LemurianShip")], preset: Some(65), members: None },
    // Crossbone Isle, open to the end.
    Chapter { maps: &[("CrossboneIsle", "CrossboneIsleNinthFloor")], preset: Some(67), members: None },
    // Venus Lighthouse.
    Chapter { maps: &[("VenusLighthouseEntry", "VenusLighthouseAerie")], preset: Some(69), members: None },
];

/// A chapter read through the source: its maps by number, its members by
/// character number in owner order, and its preset.
pub struct Party {
    maps: Vec<(u16, u16)>,
    pub members: Vec<u8>,
    pub preset: Option<u32>,
}

fn resolve(chapter: &Chapter) -> Result<Party, String> {
    let maps = chapter.maps.iter().map(|&(first, last)| Ok((source::map_id(first)?, source::map_id(last)?))).collect::<Result<_, String>>()?;
    let mut members = match (chapter.members, chapter.preset) {
        (Some(names), _) => names.iter().map(|name| source::character(name)).collect::<Result<Vec<u8>, String>>()?,
        (None, Some(preset)) => source::preset_members(preset)?,
        (None, None) => return Err("a chapter without a preset names its members".into()),
    };
    members.sort_unstable();
    Ok(Party { maps, members, preset: chapter.preset })
}

/// The chapter scene `scene` falls in, if any.
pub fn chapter(scene: u16) -> Result<Option<&'static Party>, String> {
    static PARTIES: OnceLock<Result<Vec<Party>, String>> = OnceLock::new();
    let parties = PARTIES.get_or_init(|| CHAPTERS.iter().map(resolve).collect()).as_ref().map_err(Clone::clone)?;
    Ok(parties.iter().find(|p| p.maps.iter().any(|&(a, b)| (a..=b).contains(&scene))))
}

/// The party's order once the run's flags have decided who is in it: the
/// chapter's members first, then any other member the flags add, by number.
pub fn order(members: &[u8], flags: u8) -> Vec<u8> {
    let mut out: Vec<u8> = members.iter().copied().filter(|&m| flags & (1 << m) != 0).collect();
    for m in 0..8u8 {
        if flags & (1 << m) != 0 && !out.contains(&m) {
            out.push(m);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenes_fall_in_one_chapter_each() {
        let parties: Vec<Party> = CHAPTERS.iter().map(resolve).collect::<Result<_, _>>().unwrap();
        for scene in 0..201u16 {
            assert!(parties.iter().filter(|p| p.maps.iter().any(|&(a, b)| (a..=b).contains(&scene))).count() <= 1, "scene {scene}");
        }
        for p in &parties {
            assert!(p.maps.iter().all(|&(a, b)| a <= b));
        }
        let altin = source::map_id("AltinPeakWestExit").unwrap();
        assert_eq!(chapter(altin).unwrap().map(|p| p.members.len()), Some(4));
        let storm = source::map_id("ValeStorm").unwrap();
        assert_eq!(chapter(storm).unwrap().and_then(|p| p.preset), None);
    }

    #[test]
    fn the_flags_decide_the_party() {
        assert_eq!(order(&[0, 1, 2], 0b0011), vec![0, 1]);
        assert_eq!(order(&[0, 1], 0b0111), vec![0, 1, 2]);
    }
}
