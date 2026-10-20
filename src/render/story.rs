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
    /// The first trigger, or why the row has none to play.
    pub trigger: Result<Trigger, String>,
    /// The conditions on the first trigger and the event's own: flag names
    /// and whether each must be set.
    pub conditions: Vec<(String, bool)>,
    /// The flags the event tests, by name.
    pub tests: Vec<String>,
    pub messages: Vec<u32>,
}

/// Field Psynergy by the name a trigger gives it, and its ability id
/// (actions.tsv). Move needs a direction held after the cast.
const PSYNERGY: [(&str, u16); 11] = [
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
    if first.starts_with("move on ") {
        return Err("Move needs a direction held after the cast".to_string());
    }
    for (name, ability) in PSYNERGY {
        if let Some(rest) = first.strip_prefix(&format!("{name} on ")) {
            return target(rest).map(|actor| Trigger::Psynergy(ability, actor)).ok_or_else(bad);
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

pub fn read_events(path: &Path) -> Result<Vec<Event>, String> {
    let tsv = Tsv::read(path)?;
    let [scenes, event, trigger, condition, requires, tests, messages] =
        ["scenes", "event", "trigger", "condition", "requires", "tests", "messages"].map(|c| tsv.column(c));
    let (scenes, event, trigger, condition, requires, tests, messages) =
        (scenes?, event?, trigger?, condition?, requires?, tests?, messages?);
    Ok(tsv
        .rows
        .iter()
        .map(|row| {
            let trigger_text = tsv.get(row, trigger).to_string();
            // The condition column holds the event table's condition, per
            // trigger when there are several (`touch 20: ... | touch 21: ...`).
            let first_condition = tsv.get(row, condition).split(" | ").next().unwrap_or("");
            let first_condition = first_condition.split_once(": ").map_or(first_condition, |(_, c)| c);
            let mut conditions = parse_conditions(tsv.get(row, requires));
            conditions.extend(parse_conditions(first_condition));
            Event {
                scenes: tsv
                    .get(row, scenes)
                    .split(',')
                    .filter_map(|s| s.split_whitespace().next()?.parse().ok())
                    .collect(),
                name: tsv.get(row, event).to_string(),
                trigger: parse_trigger(&trigger_text),
                trigger_text,
                conditions,
                tests: tsv
                    .get(row, tests)
                    .split(',')
                    .filter_map(|t| t.trim().split(':').next())
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
                    .collect(),
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
        assert!(parse_trigger("move on ACTOR_BLOCK=19").is_err());
        assert!(parse_trigger("entry initialize").is_err());
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
        };
        assert_eq!(scene(Some("KolimaForestBridge")).slug(), "kolima-forest-bridge");
        assert_eq!(scene(Some("Bilibin1")).slug(), "bilibin1");
        assert_eq!(scene(None).slug(), "scene-31");
    }
}
