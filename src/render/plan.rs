//! Which abilities `render psynergy` casts, by whom and at whom, from the
//! game's own tables: actions.tsv (each ability's targeting, flags, element,
//! range and cost), classes.tsv and items.tsv (what the party can learn), and
//! djinn.tsv, summons.tsv, items.tsv and enemies.tsv for the rest.

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

/// Debug party preset 0 (presets.tsv): the four at level 35, each with their
/// grown-up battle sprite (BATTLE_CHARACTER, 300 on). The low-level presets
/// equip Isaac and Garet with a Machete, which BattleObjMake draws with
/// sprites 480 and 482 instead. Jenna, in no preset but 10 and 11, keeps the
/// record the debug menu's defaults give her.
const PRESET_FOUR: u8 = 0;

/// BattleAct.flags (struct BattleAct, battle/battle_unit.h).
const USABLE_IN_BATTLE: u8 = 0x80;
const KIND_MASK: u8 = 15;
/// Kind 9: field Psynergy, Defend, Flee and the enemies' commands.
const KIND_NO_POWER: u8 = 9;

/// enum AbilityKind and enum AbilityEffect values the setup answers.
const KIND_HEAL_HP: u8 = 1;
const EFFECT_CURE_POISON: u8 = 3;
const EFFECT_RESTORE: u8 = 4;
const EFFECT_REVIVE: u8 = 5;
const EFFECT_INSTANT_DEATH: u8 = 27;

/// BattleAct.targetMode.
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

/// One row of actions.tsv.
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

    pub fn target_name(&self) -> &'static str {
        match self.aim {
            Aim::Foe if self.foes.is_empty() => "the enemy",
            Aim::Foe => character_name(self.foes[0]),
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

/// A table the build encodes: a header of `name:type` columns and rows.
struct Table {
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    fn parse(text: &str) -> Table {
        let mut lines = text.lines().filter(|l| !l.is_empty());
        let columns = lines
            .next()
            .unwrap_or("")
            .split('\t')
            .map(|c| c.split(':').next().unwrap_or(c).to_string())
            .collect();
        let rows = lines.map(|l| l.split('\t').map(str::to_string).collect()).collect();
        Table { columns, rows }
    }

    fn read(path: &Path) -> Result<Table, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        Ok(Table::parse(&text))
    }

    fn column(&self, name: &str) -> Result<usize, String> {
        self.columns.iter().position(|c| c == name).ok_or_else(|| format!("no column {name}"))
    }

    fn text(&self, row: usize, column: usize) -> &str {
        self.rows[row].get(column).map_or("", String::as_str)
    }

    fn number(&self, row: usize, column: usize) -> Result<i64, String> {
        let text = self.text(row, column).trim();
        let value = match text.strip_prefix("0x") {
            Some(hex) => i64::from_str_radix(hex, 16).ok(),
            None => text.parse().ok(),
        };
        value.ok_or_else(|| format!("row {row}: {:?} is not a number", text))
    }

    /// Every nonzero value of the named columns.
    fn values(&self, names: &[String]) -> Result<BTreeSet<u16>, String> {
        let mut values = BTreeSet::new();
        for name in names {
            let column = self.column(name)?;
            for row in 0..self.rows.len() {
                let value = self.number(row, column)?;
                if value > 0 {
                    values.insert(value as u16);
                }
            }
        }
        Ok(values)
    }
}

pub fn abilities(actions: &str) -> Result<Vec<Ability>, String> {
    let table = Table::parse(actions);
    let [target_mode, flags, element, effect, range, pp_cost, name] =
        ["target_mode", "flags", "element", "effect", "range", "pp_cost", "ability"].map(|c| table.column(c));
    let (target_mode, flags, element, effect, range, pp_cost, name) =
        (target_mode?, flags?, element?, effect?, range?, pp_cost?, name?);
    (0..table.rows.len())
        .map(|row| {
            let byte = |column| table.number(row, column).map(|v| v as u8);
            Ok(Ability {
                id: row as u16,
                name: table.text(row, name).to_string(),
                target_mode: byte(target_mode)?,
                flags: byte(flags)?,
                element: byte(element)?,
                effect: byte(effect)?,
                range: byte(range)?,
                pp_cost: byte(pp_cost)?,
            })
        })
        .collect()
}

/// What the party learns: each class's Psynergy, and the abilities items
/// teach while equipped (use type 3).
fn class_abilities(classes: &Table) -> Result<(BTreeSet<u16>, BTreeSet<u16>), String> {
    let names: Vec<String> = (0..16).map(|k| format!("ability{k}")).collect();
    let all = classes.values(&names)?;
    let mut jenna = BTreeSet::new();
    if JENNA_CLASS < classes.rows.len() {
        for name in &names {
            let value = classes.number(JENNA_CLASS, classes.column(name)?)?;
            if value > 0 {
                jenna.insert(value as u16);
            }
        }
    }
    Ok((all, jenna))
}

fn item_abilities(items: &Table) -> Result<(BTreeSet<u16>, BTreeSet<u16>, BTreeSet<u16>), String> {
    let (use_type, action, unleash) = (items.column("use_type")?, items.column("action_id")?, items.column("unleash_action")?);
    let (mut taught, mut used, mut unleashed) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for row in 0..items.rows.len() {
        let id = items.number(row, action)?;
        if id > 0 {
            if items.number(row, use_type)? == 3 { &mut taught } else { &mut used }.insert(id as u16);
        }
        let id = items.number(row, unleash)?;
        if id > 0 {
            unleashed.insert(id as u16);
        }
    }
    Ok((taught, used, unleashed))
}

/// The tables, from games/tbs/src under `root`.
pub struct Tables {
    pub actions: String,
    classes: Table,
    items: Table,
    djinn: Table,
    summons: Table,
    enemies: Table,
}

impl Tables {
    pub fn read(root: &Path) -> Result<Tables, String> {
        let game = root.join("games/tbs/src/game");
        let actions = game.join("actions.tsv");
        Ok(Tables {
            actions: std::fs::read_to_string(&actions).map_err(|e| format!("cannot read {}: {e}", actions.display()))?,
            classes: Table::read(&game.join("classes.tsv"))?,
            items: Table::read(&game.join("items.tsv"))?,
            djinn: Table::read(&game.join("djinn.tsv"))?,
            summons: Table::read(&game.join("summons.tsv"))?,
            enemies: Table::read(&game.join("enemies.tsv"))?,
        })
    }

    pub fn plan(&self) -> Result<Plan, String> {
        let abilities = abilities(&self.actions)?;
        let (learned, jenna) = class_abilities(&self.classes)?;
        let (taught, used, unleashed) = item_abilities(&self.items)?;
        let djinn = self.djinn.values(&["ability".to_string()])?;
        let summons = self.summons.values(&["ability".to_string()])?;
        let enemy_columns: Vec<String> = (0..8).map(|k| format!("action{k}")).collect();
        let enemy = self.enemies.values(&enemy_columns)?;
        let mut casts = Vec::new();
        let mut skipped = Vec::new();
        for ability in abilities.into_iter().filter(Ability::usable_in_battle) {
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
            match skip {
                Some(skip) => skipped.push((ability, skip)),
                None => match cast(ability.clone(), jenna.contains(&id)) {
                    Some(c) => casts.push(c),
                    None => skipped.push((ability, Skip::Other)),
                },
            }
        }
        name_files(&mut casts);
        Ok(Plan { casts, skipped })
    }
}

/// The caster by element, and the opposed element's character as the
/// target: Venus against Jupiter, Mars against Mercury. Jenna casts what her
/// class teaches and Garet the rest of Mars. An offensive cast that reaches
/// past its aim meets three foes, the target in the middle; healing and
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
            foes.extend(FOUR.iter().copied().filter(|&o| o != caster && o != opposed).take(2));
        }
        (vec![caster], foes, Aim::Foe)
    } else {
        (vec![caster, opposed], Vec::new(), Aim::Ally)
    };
    let ally = match (aim, ability.effect) {
        (Aim::Foe, _) => Ally::Healthy,
        (_, EFFECT_REVIVE) => Ally::Downed,
        (_, EFFECT_CURE_POISON) => Ally::Poisoned,
        (_, EFFECT_RESTORE) => Ally::Deluded,
        _ if ability.kind() == KIND_HEAL_HP => Ally::Hurt,
        _ => Ally::Healthy,
    };
    let lucky_foes = ability.effect == EFFECT_INSTANT_DEATH;
    Some(Cast { slug: slug(&ability.name), ability, preset, party, foes, aim, ally, lucky_foes })
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

    const ACTIONS: &str = "target_mode:u8\tflags:u8\telement:u8\teffect:u8\trange:u8\tpp_cost:u8\tability
1\t0x05\t4\t0\t1\t0\tNothing
1\t0x83\t4\t0\t1\t0\tAttack
4\t0x89\t4\t46\t1\t0\tDefend
1\t0x85\t0\t0\t2\t4\tQuake
2\t0xc1\t1\t0\t1\t4\tPly
1\t0x85\t2\t0\t1\t4\tFlare
";

    #[test]
    fn rows_are_read_by_column_name() {
        let rows = abilities(ACTIONS).unwrap();
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[3].name, "Quake");
        assert_eq!((rows[3].flags, rows[3].kind(), rows[3].range, rows[3].pp_cost), (0x85, 5, 2, 4));
        assert!(!rows[0].usable_in_battle());
        assert!(rows[1].usable_in_battle());
    }

    #[test]
    fn casters_and_targets_follow_the_elements() {
        let rows = abilities(ACTIONS).unwrap();
        let quake = cast(rows[3].clone(), false).unwrap();
        assert_eq!((quake.party.clone(), quake.foes.clone(), quake.aim), (vec![ISAAC], vec![IVAN, GARET, MIA], Aim::Foe));
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
        let rows = abilities(ACTIONS).unwrap();
        let mut casts: Vec<Cast> = [3, 3].iter().map(|&i| cast(rows[i].clone(), false).unwrap()).collect();
        casts[1].ability.id = 9;
        name_files(&mut casts);
        assert_eq!((casts[0].slug.as_str(), casts[1].slug.as_str()), ("quake-3", "quake-9"));
    }
}
