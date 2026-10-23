//! The party each scene is played with: who has joined by that point of the
//! story, and how strong they are.
//!
//! A run starts from a new game's defaults (Isaac alone, level 1), so a
//! scene of Altin Peak would show him fighting its water beast alone. Each
//! scene instead takes a chapter of the story: its members, in the order
//! they joined, and one of Camelot's own debug party presets
//! (DebugParty_LoadPreset, games/tbs/src/battle/presets.tsv), which raises
//! each member to a level and gives them Djinn, Psynergy and equipment.
//! Presets 10 to 70 follow the story: Isaac, Garet and Jenna at level 1 or
//! 2, Isaac and Garet at 3, Ivan joining at 4 and 5, three members from
//! level 6 to 9, Mia joining at 10, four members from 11 to 17, Isaac alone
//! at 18 and 19 (the Colosso), and four again from 20 to 33. Each area takes
//! the preset whose members are the party there and whose level fits where
//! the area falls between those joins; the preset's members the chapter does
//! not name are left out of the party. The run's own flags then decide the
//! party flags an event needs (FLAG_IN_PARTY), and the party is listed again
//! from them.
//!
//! The stormy night keeps a new game's level-1 children (FLAG_CHILDHOOD is
//! set by the run's flags there).

/// The characters by number (FLAG_IN_PARTY, gGameState.party).
pub const ISAAC: u8 = 0;
pub const GARET: u8 = 1;
pub const IVAN: u8 = 2;
pub const MIA: u8 = 3;
pub const JENNA: u8 = 5;

/// One stretch of the story: the scenes it covers (by number), its members
/// in joining order, and the debug preset that sets their strength (none for
/// a new game's level 1).
pub struct Chapter {
    pub scenes: &'static [(u16, u16)],
    pub members: &'static [u8],
    pub preset: Option<u32>,
}

const TWO: &[u8] = &[ISAAC, GARET];
const THREE: &[u8] = &[ISAAC, GARET, IVAN];
const FOUR: &[u8] = &[ISAAC, GARET, IVAN, MIA];

/// The chapters, by scene number (MapId_*): ranges of scenes in the
/// story's order.
pub const CHAPTERS: &[Chapter] = &[
    // The stormy night: Isaac and Garet as children, a new game's level 1.
    Chapter { scenes: &[(3, 3)], members: TWO, preset: None },
    // Vale three years on, before Sol Sanctum.
    Chapter { scenes: &[(4, 10)], members: TWO, preset: Some(10) },
    // Sol Sanctum and Mt. Aleph's eruption, with Jenna.
    Chapter { scenes: &[(11, 19)], members: &[ISAAC, GARET, JENNA], preset: Some(11) },
    // The world map and Vault: Isaac and Garet set out; Ivan joins in Vault.
    Chapter { scenes: &[(2, 2), (20, 22)], members: TWO, preset: Some(12) },
    // Goma Cave, Bilibin and its barricade, Vault revisited.
    Chapter { scenes: &[(23, 29)], members: THREE, preset: Some(14) },
    Chapter { scenes: &[(30, 35)], members: THREE, preset: Some(20) },
    // Kolima, its forest and Tret, and the caves by Vale and Vault.
    Chapter { scenes: &[(36, 46), (93, 98)], members: THREE, preset: Some(21) },
    // Bilibin Cave, Imil.
    Chapter { scenes: &[(47, 52)], members: THREE, preset: Some(23) },
    // Mercury Lighthouse, where Mia joins.
    Chapter { scenes: &[(53, 59)], members: FOUR, preset: Some(25) },
    // Fuchin Temple and its falls.
    Chapter { scenes: &[(60, 67)], members: FOUR, preset: Some(30) },
    // Mogall Forest, Xian, Alpine Crossing.
    Chapter { scenes: &[(68, 74)], members: FOUR, preset: Some(32) },
    // Altin and Altin Peak.
    Chapter { scenes: &[(75, 87)], members: FOUR, preset: Some(34) },
    // Lama Temple and the Lamakan Desert.
    Chapter { scenes: &[(88, 92)], members: FOUR, preset: Some(40) },
    // Kalay, its docks and Hammet's palace.
    Chapter { scenes: &[(99, 103), (107, 108)], members: FOUR, preset: Some(42) },
    // The Tolbi-bound Ship and Tolbi Docks.
    Chapter { scenes: &[(109, 112)], members: FOUR, preset: Some(43) },
    // Tolbi and Babi's palace.
    Chapter { scenes: &[(135, 139), (189, 189)], members: FOUR, preset: Some(51) },
    // The Colosso: Isaac alone.
    Chapter { scenes: &[(140, 145)], members: &[ISAAC], preset: Some(53) },
    // Altmiller Cave, Gondowan Passage, Kalay Tunnel, Gondowan Cave.
    Chapter { scenes: &[(146, 152)], members: FOUR, preset: Some(60) },
    Chapter { scenes: &[(153, 158)], members: FOUR, preset: Some(61) },
    // Lunpa, its fortress and West Lunpa Cave.
    Chapter { scenes: &[(104, 106), (159, 163)], members: FOUR, preset: Some(63) },
    // The Suhalla Desert, Suhalla, its gate, Gateway Cave, the Tunnel Ruins.
    Chapter { scenes: &[(164, 173)], members: FOUR, preset: Some(64) },
    // Babi Lighthouse, Lalivero, the Lemurian Ship.
    Chapter { scenes: &[(174, 180), (188, 188)], members: FOUR, preset: Some(65) },
    // Crossbone Isle, open to the end.
    Chapter { scenes: &[(113, 134)], members: FOUR, preset: Some(67) },
    // Venus Lighthouse.
    Chapter { scenes: &[(181, 187)], members: FOUR, preset: Some(69) },
];

/// The chapter scene `scene` falls in, if any.
pub fn chapter(scene: u16) -> Option<&'static Chapter> {
    CHAPTERS.iter().find(|c| c.scenes.iter().any(|&(a, b)| (a..=b).contains(&scene)))
}

/// The party's order once the run's flags have decided who is in it: the
/// chapter's members first, as they joined, then any other member the flags
/// add, by number.
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
        for scene in 0..201u16 {
            assert!(CHAPTERS.iter().filter(|c| c.scenes.iter().any(|&(a, b)| (a..=b).contains(&scene))).count() <= 1, "scene {scene}");
        }
        assert_eq!(chapter(77).map(|c| c.members), Some(FOUR));
        assert_eq!(chapter(3).and_then(|c| c.preset), None);
    }

    #[test]
    fn the_flags_decide_the_party() {
        assert_eq!(order(THREE, 0b0011), vec![ISAAC, GARET]);
        assert_eq!(order(TWO, 0b0111), vec![ISAAC, GARET, IVAN]);
    }
}
