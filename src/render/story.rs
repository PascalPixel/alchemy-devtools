//! The place and story tables `render place` and `render event` read: the
//! scenes (places.tsv: each scene's number, symbol, overlay file, area names
//! and map), the events (events.tsv: each event's scenes, trigger, the flags
//! it requires or tests and its messages) and the flag names (flags.tsv).
//! They are TSV files built from the tree by the story tools, passed by
//! path.

use std::collections::HashMap;
use std::path::Path;

/// One scene, a row of places.tsv's first section.
#[derive(Debug, Clone)]
pub struct Scene {
    pub number: u16,
    /// SceneId_<name> without the prefix; none for an unnamed row.
    pub symbol: Option<String>,
    /// The overlay's source, `games/tbs/src/scene/vale.c`.
    pub file: String,
    /// The area names, `19=Babi's Palace; *=Vale` where they depend on the
    /// entrance.
    pub area: String,
    /// The map folder, or `world` for the perspective world map.
    pub map: String,
    /// The overlay resource that holds its code (places.tsv's overlayId),
    /// when the table gives it.
    pub overlay: Option<u16>,
}

impl Scene {
    /// The scene's folder name: its symbol in lowercase words joined by
    /// hyphens (`kolima-forest-bridge`, `bilibin1`), or `scene-N`.
    pub fn slug(&self) -> String {
        match &self.symbol {
            None => format!("scene-{}", self.number),
            Some(name) => {
                let mut slug = String::new();
                for (i, c) in name.chars().enumerate() {
                    if c.is_ascii_uppercase() && i > 0 {
                        slug.push('-');
                    }
                    slug.push(c.to_ascii_lowercase());
                }
                slug
            }
        }
    }

    pub fn name(&self) -> String {
        self.symbol.clone().unwrap_or_else(|| format!("#{}", self.number))
    }
}

/// A TSV file: its header's column names (before any `:type`) and rows.
struct Tsv {
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Tsv {
    /// Rows up to the first empty line.
    fn read(path: &Path) -> Result<Tsv, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let mut lines = text.lines();
        let columns = lines
            .next()
            .unwrap_or("")
            .split('\t')
            .map(|c| c.split(':').next().unwrap_or(c).to_string())
            .collect();
        let rows = lines.take_while(|l| !l.is_empty()).map(|l| l.split('\t').map(str::to_string).collect()).collect();
        Ok(Tsv { columns, rows })
    }

    fn column(&self, name: &str) -> Result<usize, String> {
        self.columns.iter().position(|c| c == name).ok_or_else(|| format!("no column {name}"))
    }

    fn get<'a>(&self, row: &'a [String], column: usize) -> &'a str {
        row.get(column).map_or("", |s| s.as_str())
    }
}

pub fn read_scenes(path: &Path) -> Result<Vec<Scene>, String> {
    let tsv = Tsv::read(path)?;
    let [number, symbol, file, area, map] =
        ["sceneNumber", "sceneSymbol", "overlayFile", "areaNames", "map"].map(|c| tsv.column(c));
    let (number, symbol, file, area, map) = (number?, symbol?, file?, area?, map?);
    let overlay = tsv.column("overlayId").ok();
    tsv.rows
        .iter()
        .map(|row| {
            let n = tsv.get(row, number);
            Ok(Scene {
                number: n.parse().map_err(|_| format!("{}: scene number {n:?}", path.display()))?,
                symbol: tsv.get(row, symbol).strip_prefix("SceneId_").map(str::to_string),
                file: tsv.get(row, file).to_string(),
                area: tsv.get(row, area).to_string(),
                map: tsv.get(row, map).to_string(),
                overlay: overlay.and_then(|c| u16::from_str_radix(tsv.get(row, c).trim_start_matches("0x"), 16).ok()),
            })
        })
        .collect()
}

/// Flag names to numbers: flags.tsv's names, global and by scene.
pub struct Flags {
    global: HashMap<String, u16>,
    scoped: HashMap<(String, String), u16>,
}

impl Flags {
    pub fn read(path: &Path) -> Result<Flags, String> {
        let tsv = Tsv::read(path)?;
        let (flag, number, names) = (tsv.column("flag")?, tsv.column("number")?, tsv.column("names")?);
        let mut global = HashMap::new();
        let mut scoped = HashMap::new();
        for row in &tsv.rows {
            let Some(value) = parse_number(tsv.get(row, number)) else {
                continue;
            };
            let scene = tsv.get(row, flag).split_once('@').map(|(_, s)| s.to_string());
            for name in tsv.get(row, names).split(',').map(str::trim).filter(|n| !n.is_empty()) {
                match &scene {
                    Some(scene) => {
                        scoped.insert((scene.clone(), name.to_string()), value as u16);
                    }
                    None => {
                        global.entry(name.to_string()).or_insert(value as u16);
                    }
                }
            }
        }
        Ok(Flags { global, scoped })
    }

    /// A flag by name (as in scene `scene` when it names one there), or a
    /// number written in hex.
    pub fn resolve(&self, name: &str, scene: Option<&str>) -> Option<u16> {
        if let Some(n) = name.strip_prefix("0x").and_then(|h| u16::from_str_radix(h, 16).ok()) {
            return Some(n);
        }
        if let Some(scene) = scene {
            if let Some(&n) = self.scoped.get(&(scene.to_string(), name.to_string())) {
                return Some(n);
            }
        }
        self.global
            .get(name)
            .copied()
            .or_else(|| self.scoped.iter().find(|((_, n), _)| n == name).map(|(_, &v)| v))
    }
}

fn parse_number(text: &str) -> Option<u32> {
    let text = text.trim();
    match text.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

/// What starts an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Talking to actor N.
    Talk(u16),
    /// Searching (A facing) a layer-0 cell with collision code N.
    Search(u16),
    /// Stepping on a cell with collision code N.
    Touch(u16),
    /// A field Psynergy (its ability id) on actor N.
    Psynergy(u16, u16),
}

/// One row of events.tsv.
#[derive(Debug, Clone)]
pub struct Event {
    pub scenes: Vec<u16>,
    pub name: String,
    /// The row's trigger column as written.
    pub trigger_text: String,
    /// The ways the event starts, in the row's order: each trigger with the
    /// conditions on it and the event's own (flag names and whether each
    /// must be set), or why it cannot be played. A trigger the condition
    /// column lists more than once (`search 70: ... | search 70: ...`) is
    /// one alternative for each.
    pub alternatives: Vec<Alternative>,
    /// The flags the event tests, by name.
    pub tests: Vec<String>,
    /// The flags the event sets, by name.
    pub sets: Vec<String>,
    pub messages: Vec<u32>,
}

/// One way an event starts.
#[derive(Debug, Clone)]
pub struct Alternative {
    pub trigger: Result<Trigger, String>,
    pub conditions: Vec<(String, bool)>,
}

impl Event {
    /// The first alternative's trigger, or why the row has none.
    pub fn trigger(&self) -> Result<Trigger, String> {
        self.alternatives.first().map_or_else(|| Err("the row has no trigger".to_string()), |a| a.trigger.clone())
    }
}

/// Move's ability id: the one field Psynergy steered with the d-pad once it
/// is cast (BattleFx_SteerPushedTarget).
pub const MOVE: u16 = 0x8c;

/// Field Psynergy by the name a trigger gives it, and its ability id
/// (actions.tsv).
const PSYNERGY: [(&str, u16); 13] = [
    ("move", MOVE),
    ("mind read", 0x8d),
    ("force", 0x8e),
    ("frost", 0x18),
    ("growth", 0x0c),
    ("whirlwind", 0x4e),
    ("catch", 0x94),
    ("douse", 0x21),
    ("lift", 0x8f),
    ("halt", 0x91),
    ("carry", 0x93),
    ("cloak", 0x92),
    ("ply", 0x5d),
];

/// The number after a trigger word: `12` or `ACTOR_NAME=12`.
fn target(text: &str) -> Option<u16> {
    let value = text.trim().rsplit('=').next()?;
    value.trim().parse().ok()
}

pub fn parse_trigger(text: &str) -> Result<Trigger, String> {
    let first = text.split(';').next().unwrap_or("").trim();
    let bad = || format!("no target in {first:?}");
    if let Some(rest) = first.strip_prefix("talk ") {
        return target(rest).map(Trigger::Talk).ok_or_else(bad);
    }
    if let Some(rest) = first.strip_prefix("search ") {
        return target(rest).map(Trigger::Search).ok_or_else(bad);
    }
    if let Some(rest) = first.strip_prefix("touch ") {
        return target(rest).map(Trigger::Touch).ok_or_else(bad);
    }
    for (name, ability) in PSYNERGY {
        if let Some(rest) = first.strip_prefix(&format!("{name} on ")) {
            // The row's trigger keeps its actor in bits 7..0; bit 10 has the
            // event play the effect itself (`mind read on 1027`, Mia).
            return target(rest).map(|actor| Trigger::Psynergy(ability, actor & 0xff)).ok_or_else(bad);
        }
    }
    Err(format!("{first:?} is not a talk, search, touch or Psynergy-on-actor trigger"))
}

/// `FLAG_A set and FLAG_B clear` as (name, set) pairs.
fn parse_conditions(text: &str) -> Vec<(String, bool)> {
    text.split(" and ")
        .filter_map(|c| {
            let (name, state) = c.trim().rsplit_once(' ')?;
            match state {
                "set" => Some((name.trim().to_string(), true)),
                "clear" => Some((name.trim().to_string(), false)),
                _ => None,
            }
        })
        .collect()
}

/// The ways a row's event starts: each trigger of the trigger column (`touch
/// 20; touch 21`), with the conditions the condition column gives it
/// (`touch 20: FLAG_A set | touch 21: FLAG_A clear`; a condition with no
/// trigger before it is the first trigger's) and the row's own.
fn alternatives(triggers: &str, condition: &str, requires: &str) -> Vec<Alternative> {
    let own = parse_conditions(requires);
    let segments: Vec<(Option<&str>, &str)> = condition
        .split(" | ")
        .filter(|s| !s.trim().is_empty())
        .map(|s| match s.split_once(": ") {
            Some((t, c)) => (Some(t.trim()), c),
            None => (None, s),
        })
        .collect();
    let mut out = Vec::new();
    for (i, text) in triggers.split(';').map(str::trim).filter(|t| !t.is_empty()).enumerate() {
        let trigger = parse_trigger(text);
        let mine: Vec<&str> =
            segments.iter().filter(|(t, _)| *t == Some(text) || (t.is_none() && i == 0)).map(|(_, c)| *c).collect();
        let with = |extra: &str| {
            let mut conditions = own.clone();
            conditions.extend(parse_conditions(extra));
            Alternative { trigger: trigger.clone(), conditions }
        };
        if mine.is_empty() {
            out.push(with(""));
        }
        out.extend(mine.into_iter().map(with));
    }
    if out.is_empty() {
        out.push(Alternative { trigger: parse_trigger(triggers), conditions: own });
    }
    out
}

/// A list of flags, `FLAG_A:set, FLAG_B` or `FLAG_A, FLAG_B`, as names.
fn flag_names(text: &str) -> Vec<String> {
    text.split(',').filter_map(|t| t.trim().split(':').next()).filter(|t| !t.is_empty()).map(str::to_string).collect()
}

pub fn read_events(path: &Path) -> Result<Vec<Event>, String> {
    let tsv = Tsv::read(path)?;
    let [scenes, event, trigger, condition, requires, tests, messages] =
        ["scenes", "event", "trigger", "condition", "requires", "tests", "messages"].map(|c| tsv.column(c));
    let (scenes, event, trigger, condition, requires, tests, messages) =
        (scenes?, event?, trigger?, condition?, requires?, tests?, messages?);
    let sets = tsv.column("sets").ok();
    Ok(tsv
        .rows
        .iter()
        .map(|row| {
            let trigger_text = tsv.get(row, trigger).to_string();
            Event {
                scenes: tsv
                    .get(row, scenes)
                    .split(',')
                    .filter_map(|s| s.split_whitespace().next()?.parse().ok())
                    .collect(),
                name: tsv.get(row, event).to_string(),
                alternatives: alternatives(&trigger_text, tsv.get(row, condition), tsv.get(row, requires)),
                trigger_text,
                tests: flag_names(tsv.get(row, tests)),
                sets: sets.map(|c| flag_names(tsv.get(row, c))).unwrap_or_default(),
                messages: tsv.get(row, messages).split(',').filter_map(|m| m.trim().parse().ok()).collect(),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggers_name_their_target() {
        assert_eq!(parse_trigger("talk ACTOR_EASTWARD_MONK=10"), Ok(Trigger::Talk(10)));
        assert_eq!(parse_trigger("touch 20; touch 21"), Ok(Trigger::Touch(20)));
        assert_eq!(parse_trigger("search 208"), Ok(Trigger::Search(208)));
        assert_eq!(parse_trigger("mind read on 8"), Ok(Trigger::Psynergy(0x8d, 8)));
        assert_eq!(parse_trigger("force on ACTOR_PILLAR=12"), Ok(Trigger::Psynergy(0x8e, 12)));
        assert_eq!(parse_trigger("move on ACTOR_BLOCK=19"), Ok(Trigger::Psynergy(MOVE, 19)));
        assert_eq!(parse_trigger("mind read on 1027"), Ok(Trigger::Psynergy(0x8d, 3)));
        assert_eq!(parse_trigger("ply on ACTOR_WATERFALL_STATUE=12"), Ok(Trigger::Psynergy(0x5d, 12)));
        assert!(parse_trigger("entry initialize").is_err());
    }

    #[test]
    fn each_trigger_is_an_alternative_with_its_conditions() {
        let alts = alternatives("touch 20; touch 21", "touch 20: FLAG_A set | touch 21: FLAG_A clear", "FLAG_B clear");
        assert_eq!(alts.len(), 2);
        assert_eq!(alts[0].trigger, Ok(Trigger::Touch(20)));
        assert_eq!(alts[0].conditions, vec![("FLAG_B".to_string(), false), ("FLAG_A".to_string(), true)]);
        assert_eq!(alts[1].trigger, Ok(Trigger::Touch(21)));
        assert_eq!(alts[1].conditions, vec![("FLAG_B".to_string(), false), ("FLAG_A".to_string(), false)]);
        // A condition with no trigger before it is the first trigger's.
        let alts = alternatives("talk 9; mind read on 9", "FLAG_C set", "");
        assert_eq!(alts[0].conditions, vec![("FLAG_C".to_string(), true)]);
        assert!(alts[1].conditions.is_empty());
        // One trigger listed under several conditions is one alternative each.
        assert_eq!(alternatives("search 70", "search 70: FLAG_D set | search 70: FLAG_D clear", "").len(), 2);
    }

    #[test]
    fn conditions_are_flag_states() {
        assert_eq!(
            parse_conditions("FLAG_A set and FLAG_B clear"),
            vec![("FLAG_A".to_string(), true), ("FLAG_B".to_string(), false)]
        );
        assert!(parse_conditions("").is_empty());
    }

    #[test]
    fn scene_folders_are_lowercase_words() {
        let scene = |symbol: Option<&str>| Scene {
            number: 31,
            symbol: symbol.map(str::to_string),
            file: String::new(),
            area: String::new(),
            map: String::new(),
            overlay: None,
        };
        assert_eq!(scene(Some("KolimaForestBridge")).slug(), "kolima-forest-bridge");
        assert_eq!(scene(Some("Bilibin1")).slug(), "bilibin1");
        assert_eq!(scene(None).slug(), "scene-31");
    }
}

