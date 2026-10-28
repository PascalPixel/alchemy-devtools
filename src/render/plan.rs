//! Which abilities `render psynergy` casts, by whom and at whom, from the
//! game's own tables as the built edition holds them (tables.rs):
//! gAbilityDefinitions (each ability's targeting, flags, element, range and
//! cost), gClass_DefinitionTable and gItem_DefinitionTable (what the party can
//! learn), and the Djinn, summon, item and enemy tables for the rest.

use super::tables::{Game, Table};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The characters, by owner id (games/tbs/include/game/character_ids.h).
pub const ISAAC: u8 = 0;
pub const GARET: u8 = 1;
pub const IVAN: u8 = 2;
pub const MIA: u8 = 3;
pub const JENNA: u8 = 5;

/// The four who travel together, in owner order.
const FOUR: [u8; 4] = [ISAAC, GARET, IVAN, MIA];

/// Jenna's fixed class (Class_Determine): the Psynergy she casts.
const JENNA_CLASS: usize = 202;

/// Debug party preset 0 (gDebug_ParameterSets): the four at level 35, each with their
/// grown-up battle sprite (BATTLE_CHARACTER, 300 on). Preset 10, the level-1
/// rows, equips Isaac and Garet with a Machete, which BattleObject_Make draws with
/// sprites 480 and 482 instead. Jenna, in no preset but 10 and 11, keeps the
/// record the debug menu's defaults give her.
const PRESET_FOUR: u8 = 0;

/// AbilityDefinition.flags (struct AbilityDefinition, battle/battle_unit.h).
const USABLE_IN_BATTLE: u8 = 0x80;
const KIND_MASK: u8 = 15;
/// Kind 9: field Psynergy, Defend, Flee and the enemies' commands.
const KIND_NO_POWER: u8 = 9;

/// enum AbilityFormula and enum AbilityEffect values the setup answers.
const KIND_HEAL_HP: u8 = 1;
const EFFECT_CURE_POISON: u8 = 3;
const EFFECT_RESTORE: u8 = 4;
const EFFECT_REVIVE: u8 = 5;
/// Quartz and Dew.
const EFFECT_REVIVE_HALF: u8 = 56;
const EFFECT_REVIVE_80: u8 = 57;
const EFFECT_INSTANT_DEATH: u8 = 27;

/// AbilityDefinition.targetMode.
pub const TARGET_FOES: u8 = 1;

/// enum Element.
const ELEMENT_NAMES: [&str; 5] = ["Venus", "Mercury", "Mars", "Jupiter", "none"];

pub fn character_name(owner: u8) -> &'static str {
    match owner {
        ISAAC => "Isaac",
        GARET => "Garet",
        IVAN => "Ivan",
        MIA => "Mia",
        JENNA => "Jenna",
        _ => "?",
    }
}

/// One row of gAbilityDefinitions, with its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ability {
    pub id: u16,
    pub name: String,
    pub target_mode: u8,
    pub flags: u8,
    pub element: u8,
    pub effect: u8,
    pub range: u8,
    pub pp_cost: u8,
}

impl Ability {
    pub fn kind(&self) -> u8 {
        self.flags & KIND_MASK
    }

    fn usable_in_battle(&self) -> bool {
        self.flags & USABLE_IN_BATTLE != 0 && self.kind() != 0 && !matches!(self.name.as_str(), "?" | "=")
    }
}

/// Why an ability usable in battle is not rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Skip {
    /// Attack and Defend: commands, not Psynergy.
    Command,
    /// A Djinni's: needs a Djinni set and on the command menu's Djinn list.
    Djinni,
    /// A summon: needs Djinn on standby.
    Summon,
    /// An item's: needs the item in the inventory.
    Item,
    /// A weapon's unleash: comes from Attack at a chance.
    Unleash,
    /// Cast only by enemies.
    Enemy,
    /// A field Psynergy with no power in battle, or no list reaches it.
    Other,
}

impl Skip {
    pub fn reason(self) -> &'static str {
        match self {
            Skip::Command => "a command, not Psynergy",
            Skip::Djinni => "a Djinni's: needs Djinn set",
            Skip::Summon => "a summon: needs Djinn on standby",
            Skip::Item => "an item's",
            Skip::Unleash => "a weapon unleash",
            Skip::Enemy => "enemies only",
            Skip::Other => "no party list teaches it",
        }
    }
}

/// The ally's state at the battle's start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ally {
    Healthy,
    /// At half HP, for healing.
    Hurt,
    /// For Revive.
    Downed,
    /// For Cure Poison.
    Poisoned,
    /// Deluded, for Restore.
    Deluded,
}

/// The command the caster gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Psynergy,
    /// A Djinni Set on the caster: its element and its index in it.
    Djinni { element: u8, index: u8 },
    /// A summon, by its row of gSummon_DefinitionTable, with `count` Djinn of
    /// `element` on standby.
    Summon { row: u8, element: u8, count: u8 },
}

/// Where the cast is aimed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aim {
    /// At enemy unit 128, wherever the formation put it.
    Foe,
    /// At the caster's ally, the second party member.
    Ally,
}

/// One cast to record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cast {
    pub ability: Ability,
    pub command: Command,
    pub preset: u8,
    /// The party, the caster first.
    pub party: Vec<u8>,
    /// Characters copied onto enemy units 128 on; empty keeps the
    /// encounter's own enemy.
    pub foes: Vec<u8>,
    pub aim: Aim,
    /// How the ally starts the battle, so the cast has something to do.
    pub ally: Ally,
    /// Instant death: the foes are lucky enough that it never lands.
    pub lucky_foes: bool,
    pub slug: String,
}

impl Cast {
    pub fn caster(&self) -> u8 {
        self.party[0]
    }

    /// The aimed foe's index in `foes`: the middle one, so that a range
    /// reaches the foes beside it.
    pub fn target_foe(&self) -> usize {
        self.foes.len() / 2
    }

    pub fn target_name(&self) -> &'static str {
        match self.aim {
            Aim::Foe if self.foes.is_empty() => "the enemy",
            Aim::Foe => character_name(self.foes[self.target_foe()]),
            Aim::Ally => character_name(self.party[1]),
        }
    }
}

/// The battle abilities in id order: those the party casts, and why the
/// others are left out.
pub struct Plan {
    pub casts: Vec<Cast>,
    pub skipped: Vec<(Ability, Skip)>,
}

/// Every nonzero value of `path` in every row of `table`.
fn nonzero(table: &Table, path: &str) -> Result<BTreeSet<u16>, String> {
    let mut values = BTreeSet::new();
    for row in 0..table.len() {
        values.extend(table.values(row, path)?.into_iter().filter(|&v| v > 0).map(|v| v as u16));
    }
    Ok(values)
}

/// Every ability by id (gAbilityDefinitions), named from MsgAbilityName.
pub fn abilities(game: &Game) -> Result<Vec<Ability>, String> {
    let table = game.table("gAbilityDefinitions", "AbilityDefinition")?;
    let names = game.names("MsgAbilityName", table.len())?;
    (0..table.len())
        .map(|row| {
            let byte = |field| table.value(row, field).map(|v| v as u8);
            Ok(Ability {
                id: row as u16,
                name: names[row].clone(),
                target_mode: byte("targetMode")?,
                flags: byte("flags")?,
                element: byte("element")?,
                effect: byte("effect")?,
                range: byte("range")?,
                pp_cost: byte("ppCost")?,
            })
        })
        .collect()
}

/// What the tables say about the abilities, read from the built edition.
pub struct Tables {
    pub abilities: Vec<Ability>,
    /// Every class's Psynergy (gClass_DefinitionTable), and Jenna's.
    learned: BTreeSet<u16>,
    jenna: BTreeSet<u16>,
    /// The abilities items teach while equipped (ITEM_USE_TEACHES), those they
    /// are used for, and those weapons unleash (gItem_DefinitionTable).
    taught: BTreeSet<u16>,
    used: BTreeSet<u16>,
    unleashed: BTreeSet<u16>,
    /// The Djinn's abilities (gDjinn_DefinitionTable).
    djinn: BTreeSet<u16>,
    /// Each summon's ability and the standby Djinn it needs by element
    /// (gSummon_DefinitionTable).
    summons: Vec<(u16, Vec<i64>)>,
    /// What the enemies choose from (gEnemy_Definitions).
    enemy: BTreeSet<u16>,
}

impl Tables {
    pub fn read(root: &Path) -> Result<Tables, String> {
        let game = Game::read(root)?;
        let classes = game.table("gClass_DefinitionTable", "ClassDefinition")?;
        let items = game.table("gItem_DefinitionTable", "ItemDefinition")?;
        let summons = game.table("gSummon_DefinitionTable", "SummonDefinition")?;
        let teaches = game.constant("ITEM_USE_TEACHES")?;
        let (mut taught, mut used) = (BTreeSet::new(), BTreeSet::new());
        for row in 0..items.len() {
            let id = items.value(row, "abilityId")?;
            if id > 0 {
                if items.value(row, "useType")? == teaches { &mut taught } else { &mut used }.insert(id as u16);
            }
        }
        let jenna = if JENNA_CLASS < classes.len() {
            classes.values(JENNA_CLASS, "abilities.ability")?.into_iter().filter(|&v| v > 0).map(|v| v as u16).collect()
        } else {
            BTreeSet::new()
        };
        Ok(Tables {
            abilities: abilities(&game)?,
            learned: nonzero(&classes, "abilities.ability")?,
            jenna,
            taught,
            used,
            unleashed: nonzero(&items, "unleashAbility")?,
            djinn: nonzero(&game.table("gDjinn_DefinitionTable", "DjinnDefinition")?, "abilityId")?,
            summons: (0..summons.len()).map(|row| Ok((summons.value(row, "abilityId")? as u16, summons.values(row, "djinnRequired")?))).collect::<Result<_, String>>()?,
            enemy: nonzero(&game.table("gEnemy_Definitions", "EnemyDefinition")?, "abilities")?,
        })
    }

    pub fn plan(&self) -> Result<Plan, String> {
        let (learned, jenna, taught, used, unleashed, djinn, enemy) =
            (&self.learned, &self.jenna, &self.taught, &self.used, &self.unleashed, &self.djinn, &self.enemy);
        let summons: BTreeSet<u16> = self.summons.iter().map(|s| s.0).filter(|&a| a > 0).collect();
        let mut casts = Vec::new();
        let mut specials = Vec::new();
        let mut skipped = Vec::new();
        for ability in self.abilities.iter().cloned().filter(Ability::usable_in_battle) {
            let id = ability.id;
            let skip = if id <= 2 {
                Some(Skip::Command)
            } else if djinn.contains(&id) {
                Some(Skip::Djinni)
            } else if summons.contains(&id) {
                Some(Skip::Summon)
            } else if (learned.contains(&id) || taught.contains(&id)) && ability.kind() != KIND_NO_POWER {
                None
            } else if used.contains(&id) {
                Some(Skip::Item)
            } else if unleashed.contains(&id) {
                Some(Skip::Unleash)
            } else if enemy.contains(&id) {
                Some(Skip::Enemy)
            } else {
                Some(Skip::Other)
            };
            // A Djinni Set on its element's Adept, or a summon with its Djinn
            // on standby.
            let special = match skip {
                Some(Skip::Djinni) => id.checked_sub(DJINN_FIRST).and_then(|n| {
                    let mut c = cast(ability.clone(), false)?;
                    c.command =
                        Command::Djinni { element: (n / DJINN_PER_ELEMENT) as u8, index: (n % DJINN_PER_ELEMENT) as u8 };
                    Some(c)
                }),
                Some(Skip::Summon) => self.summon_row(id).and_then(|(row, element, count)| {
                    let mut c = cast(ability.clone(), false)?;
                    c.command = Command::Summon { row, element, count };
                    Some(c)
                }),
                _ => None,
            };
            if let Some(c) = special {
                specials.push(c);
                continue;
            }
            match skip {
                Some(skip) => skipped.push((ability, skip)),
                None => match cast(ability.clone(), jenna.contains(&id)) {
                    Some(c) => casts.push(c),
                    None => skipped.push((ability, Skip::Other)),
                },
            }
        }
        name_files(&mut casts);
        // Djinn and summons keep their names unless a Psynergy has them.
        for c in &mut specials {
            if casts.iter().any(|p| p.slug == c.slug) {
                let kind = if matches!(c.command, Command::Summon { .. }) { "summon" } else { "djinni" };
                c.slug = format!("{}-{kind}", c.slug);
            }
        }
        casts.extend(specials);
        Ok(Plan { casts, skipped })
    }

    /// A summon's row of gSummon_DefinitionTable, and the one element and
    /// count of Djinn it needs.
    fn summon_row(&self, id: u16) -> Option<(u8, u8, u8)> {
        let row = self.summons.iter().position(|s| s.0 == id)?;
        self.summons[row].1.iter().enumerate().find_map(|(e, &n)| (n > 0).then_some((row as u8, e as u8, n as u8)))
    }
}

/// A Djinni's ability id: 300 + 20 x element + index (gDjinn_DefinitionTable).
const DJINN_FIRST: u16 = 300;
const DJINN_PER_ELEMENT: u16 = 20;

/// The caster by element, and the opposed element's character as the
/// target: Venus against Jupiter, Mars against Mercury. Jenna casts what her
/// class teaches and Garet the rest of Mars. An offensive cast that reaches
/// past its aim meets three foes, the target in the middle of the line; healing and
/// support go to the target as the caster's ally, hurt for healing, downed
/// for Revive, poisoned for Cure Poison and deluded for Restore. Foes facing
/// instant death are made lucky, so they stand.
pub fn cast(ability: Ability, jenna_casts: bool) -> Option<Cast> {
    let (caster, opposed) = match ability.element {
        0 => (ISAAC, IVAN),
        1 => (MIA, GARET),
        2 if jenna_casts => (JENNA, MIA),
        2 => (GARET, MIA),
        3 => (IVAN, ISAAC),
        _ => return None,
    };
    let preset = PRESET_FOUR;
    let (party, foes, aim) = if ability.target_mode == TARGET_FOES {
        let mut foes = vec![opposed];
        if ability.range != 1 {
            // The target goes in the middle of the line, so a range reaches
            // the foes on both sides of it.
            let mut others = FOUR.iter().copied().filter(|&o| o != caster && o != opposed).take(2);
            foes = [others.next(), Some(opposed), others.next()].into_iter().flatten().collect();
        }
        (vec![caster], foes, Aim::Foe)
    } else {
        (vec![caster, opposed], Vec::new(), Aim::Ally)
    };
    let ally = match (aim, ability.effect) {
        (Aim::Foe, _) => Ally::Healthy,
        (_, EFFECT_REVIVE | EFFECT_REVIVE_HALF | EFFECT_REVIVE_80) => Ally::Downed,
        (_, EFFECT_CURE_POISON) => Ally::Poisoned,
        (_, EFFECT_RESTORE) => Ally::Deluded,
        _ if ability.kind() == KIND_HEAL_HP => Ally::Hurt,
        _ => Ally::Healthy,
    };
    let lucky_foes = ability.effect == EFFECT_INSTANT_DEATH;
    Some(Cast { slug: slug(&ability.name), ability, command: Command::Psynergy, preset, party, foes, aim, ally, lucky_foes })
}

/// The file name of an English ability name: lowercase letters and digits,
/// words joined by hyphens.
pub fn slug(name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars().filter(|&c| c != '\'') {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-').to_string()
}

/// Abilities that share a name (two are called Blast) take their ids too.
fn name_files(casts: &mut [Cast]) {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for c in casts.iter() {
        *counts.entry(c.slug.clone()).or_default() += 1;
    }
    for c in casts.iter_mut() {
        if counts[&c.slug] > 1 {
            c.slug = format!("{}-{}", c.slug, c.ability.id);
        }
    }
}

pub fn element_name(element: u8) -> &'static str {
    ELEMENT_NAMES.get(element as usize).copied().unwrap_or("?")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (target mode, flags, element, effect, range, PP cost, name).
    fn rows() -> Vec<Ability> {
        [(1, 0x05, 4, 0, 1, 0, "Nothing"), (1, 0x83, 4, 0, 1, 0, "Attack"), (4, 0x89, 4, 46, 1, 0, "Defend"), (1, 0x85, 0, 0, 2, 4, "Quake"), (2, 0xc1, 1, 0, 1, 4, "Ply"), (1, 0x85, 2, 0, 1, 4, "Flare")]
            .into_iter()
            .enumerate()
            .map(|(id, (target_mode, flags, element, effect, range, pp_cost, name))| Ability { id: id as u16, name: name.to_string(), target_mode, flags, element, effect, range, pp_cost })
            .collect()
    }

    #[test]
    fn kinds_and_battle_use_come_from_the_flags() {
        let rows = rows();
        assert_eq!((rows[3].flags, rows[3].kind(), rows[3].range, rows[3].pp_cost), (0x85, 5, 2, 4));
        assert!(!rows[0].usable_in_battle());
        assert!(rows[1].usable_in_battle());
    }

    #[test]
    fn casters_and_targets_follow_the_elements() {
        let rows = rows();
        let quake = cast(rows[3].clone(), false).unwrap();
        assert_eq!((quake.party.clone(), quake.foes.clone(), quake.aim), (vec![ISAAC], vec![GARET, IVAN, MIA], Aim::Foe));
        let ply = cast(rows[4].clone(), false).unwrap();
        assert_eq!((ply.party.clone(), ply.foes.clone(), ply.aim), (vec![MIA, GARET], vec![], Aim::Ally));
        assert_eq!(ply.target_name(), "Garet");
        let flare = cast(rows[5].clone(), true).unwrap();
        assert_eq!((flare.party.clone(), flare.foes.clone(), flare.preset), (vec![JENNA], vec![MIA], PRESET_FOUR));
        assert_eq!(cast(rows[5].clone(), false).unwrap().party, vec![GARET]);
    }

    #[test]
    fn slugs_are_lowercase_words() {
        assert_eq!(slug("Quake Sphere"), "quake-sphere");
        assert_eq!(slug("Can't Use"), "cant-use");
        assert_eq!(slug("Ply"), "ply");
    }

    #[test]
    fn shared_names_take_their_ids() {
        let rows = rows();
        let mut casts: Vec<Cast> = [3, 3].iter().map(|&i| cast(rows[i].clone(), false).unwrap()).collect();
        casts[1].ability.id = 9;
        name_files(&mut casts);
        assert_eq!((casts[0].slug.as_str(), casts[1].slug.as_str()), ("quake-3", "quake-9"));
    }
}
