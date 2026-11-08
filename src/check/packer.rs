//! The PACKER rows against the ROM. Camelot's packer wrote each stream into
//! a buffer an earlier, longer stream had filled, so the bytes past a
//! stream's end up to its rounding are that earlier stream's; a row that
//! says so, `.byte ... @ PACKER: left in the buffer by NAME+OFF`, holds the
//! bytes NAME's stream has at OFF in the same edition's ROM. Each edition
//! reads only its own rows, as its assembler does: the conditionals around
//! them are followed with the edition's `--defsym` and the `.set`s the file
//! and its includes make.

use super::rom::{checked_all, reference};
use crate::alchemy::built;
use crate::alchemy::targets::DecompTargetId;
use crate::gba::ld_map::{Map, ROM};
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// One row an edition assembles that names the stream it came from.
#[derive(Debug, PartialEq, Eq)]
struct Row {
    line: usize,
    bytes: Vec<u8>,
    provider: String,
    offset: u32,
}

/// The rows of one file an edition assembles, and how many rows it could not
/// read: under a conditional it cannot follow, or with bytes it cannot read.
#[derive(Debug, Default, PartialEq, Eq)]
struct Rows {
    rows: Vec<Row>,
    unread: usize,
}

/// A conditional being read: whether the code around it is assembled, whether
/// a branch of it has been taken, whether this one is, and whether its test
/// could be followed.
struct Branch {
    outer: bool,
    taken: bool,
    active: bool,
    known: bool,
}

/// The assembler's conditionals, followed with the symbols one edition has.
struct Assembly<'a> {
    root: &'a Path,
    symbols: HashMap<String, i64>,
    stack: Vec<Branch>,
}

fn number(text: &str) -> Option<i64> {
    let text = text.trim();
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => i64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

fn row_pattern() -> &'static Regex {
    static ROW: OnceLock<Regex> = OnceLock::new();
    ROW.get_or_init(|| Regex::new(r"^\.byte\s+([^@]*?)\s*@\s*PACKER: left in the buffer by ([A-Za-z_][A-Za-z0-9_]*)\+(0x[0-9a-fA-F]+|\d+)\s*$").unwrap())
}

impl<'a> Assembly<'a> {
    fn new(root: &'a Path, defined: &str) -> Assembly<'a> {
        Assembly { root, symbols: HashMap::from([(defined.to_string(), 1)]), stack: Vec::new() }
    }

    fn active(&self) -> bool {
        self.stack.last().is_none_or(|branch| branch.active)
    }

    /// Whether a line here may be assembled: inside a test it could not
    /// follow, though nothing it could follow is false.
    fn possible(&self) -> bool {
        self.stack.iter().all(|branch| branch.active || (!branch.known && branch.outer))
    }

    fn value(&self, expression: &str) -> Option<i64> {
        number(expression).or_else(|| self.symbols.get(expression.trim()).copied())
    }

    fn open(&mut self, test: Option<bool>) {
        let outer = self.active();
        let active = outer && test.unwrap_or(false);
        self.stack.push(Branch { outer, taken: active, active, known: test.is_some() });
    }

    /// Reads one file's lines into `found`, following its includes.
    fn read(&mut self, path: &Path, found: &mut Rows) -> Result<(), String> {
        let text = std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
        for (index, line) in text.lines().enumerate() {
            let code = line.trim();
            let (directive, rest) = code.split_once(char::is_whitespace).unwrap_or((code, ""));
            let rest = rest.trim();
            match directive {
                ".ifdef" => self.open(Some(self.symbols.contains_key(rest))),
                ".ifndef" | ".ifnotdef" => self.open(Some(!self.symbols.contains_key(rest))),
                ".if" | ".ifne" => self.open(self.value(rest).map(|value| value != 0)),
                ".ifeq" => self.open(self.value(rest).map(|value| value == 0)),
                ".else" => {
                    if let Some(branch) = self.stack.last_mut().filter(|branch| branch.known) {
                        branch.active = branch.outer && !branch.taken;
                        branch.taken = true;
                    }
                }
                ".elseif" => {
                    if let Some(branch) = self.stack.last_mut() {
                        branch.active = false;
                        branch.known = false;
                    }
                }
                ".endif" => {
                    self.stack.pop();
                }
                ".byte" if !self.active() && self.possible() && code.contains("PACKER: left in the buffer by") => found.unread += 1,
                _ if !self.active() => {}
                ".set" | ".equ" => {
                    if let Some((name, value)) = rest.split_once(',') {
                        match self.value(value) {
                            Some(value) => self.symbols.insert(name.trim().to_string(), value),
                            None => self.symbols.remove(name.trim()),
                        };
                    }
                }
                ".include" => {
                    let included = rest.trim_matches('"');
                    let path = [path.parent().map(|folder| folder.join(included)), Some(self.root.join(included))]
                        .into_iter()
                        .flatten()
                        .find(|path| path.is_file())
                        .ok_or_else(|| format!("{}:{}: cannot find {included}", path.display(), index + 1))?;
                    self.read(&path, found)?;
                }
                ".byte" if code.contains("PACKER: left in the buffer by") => {
                    let row = row_pattern().captures(code).and_then(|captures| {
                        let bytes = captures[1].split(',').map(|byte| number(byte).and_then(|byte| u8::try_from(byte).ok())).collect::<Option<Vec<u8>>>()?;
                        let offset = u32::try_from(number(&captures[3])?).ok()?;
                        Some(Row { line: index + 1, bytes, provider: captures[2].to_string(), offset })
                    });
                    match row {
                        Some(row) => found.rows.push(row),
                        None => found.unread += 1,
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// The rows `id` assembles from `path`.
fn rows(root: &Path, id: DecompTargetId, path: &Path) -> Result<Rows, String> {
    let defined = format!("{}_EDITION_{}", id.game(), id.language()).to_ascii_uppercase();
    let mut assembly = Assembly::new(root, &defined);
    let mut found = Rows::default();
    assembly.read(path, &mut found)?;
    Ok(found)
}

/// Every assembly file under a game's sources with a PACKER row, sorted.
fn files(root: &Path, game: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(root.join("games").join(game))
        .into_iter()
        .flatten()
        .map(|entry| entry.into_path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "s"))
        .filter(|path| std::fs::read_to_string(path).is_ok_and(|text| text.contains("PACKER: left in the buffer by")))
        .collect();
    files.sort();
    files
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    if let Some(flag) = arguments.iter().find(|argument| argument.starts_with('-')) {
        return Err(format!("unknown option {flag}"));
    }
    let root = crate::compile::root();
    let editions = built::editions(arguments)?;
    let mut by_game: HashMap<&str, Vec<PathBuf>> = HashMap::new();
    let (mut wrong, mut not_built) = (0, Vec::new());
    for &id in &editions {
        let Ok(map) = std::fs::read_to_string(built::output(root, id).join(format!("{id}.map"))) else {
            println!("{id}: not built; make compare-{id}");
            not_built.push(id);
            continue;
        };
        let map = Map::parse(&map);
        let rom = reference(root, id)?;
        let files = by_game.entry(id.game()).or_insert_with(|| files(root, id.game()));
        let (mut checked, mut unread) = (0, 0);
        for path in files.iter() {
            let found = rows(root, id, path)?;
            unread += found.unread;
            let shown = path.strip_prefix(root).unwrap_or(path).display();
            for row in found.rows {
                checked += 1;
                let Some((_, address)) = map.symbol(&row.provider) else {
                    println!("  {shown}:{}: {id} places no {}", row.line, row.provider);
                    wrong += 1;
                    continue;
                };
                let at = address.wrapping_sub(ROM).wrapping_add(row.offset) as usize;
                let held = rom.get(at..at + row.bytes.len());
                if held != Some(&row.bytes[..]) {
                    let hex = |bytes: &[u8]| bytes.iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join(" ");
                    let held = held.map_or("nothing (past the ROM's end)".to_string(), hex);
                    println!("  {shown}:{}: {id} has {}, but {}+{:#x} holds {held}", row.line, hex(&row.bytes), row.provider, row.offset);
                    wrong += 1;
                }
            }
        }
        let more = if unread > 0 { format!("; {unread} rows under conditionals it cannot follow, or with bytes it cannot read, left out") } else { String::new() };
        println!("{id}: {checked} rows in {} files, each its stream's bytes in {}{more}", files.len(), crate::alchemy::targets::target_for(id).rom);
    }
    if wrong > 0 {
        return Err(format!("{wrong} PACKER rows do not hold the bytes they name"));
    }
    checked_all(!arguments.is_empty(), editions.len(), &not_built)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> DecompTargetId {
        crate::alchemy::targets::parse_decomp_target(name).unwrap()
    }

    /// Each edition reads its own rows: its --defsym, the include's .set and
    /// the .else between them.
    #[test]
    fn an_edition_reads_the_rows_its_assembler_would() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir_all(root.join("games/tla/include")).unwrap();
        std::fs::write(root.join("games/tla/include/ed_asm.h"), "\t.set EDITION_INTERNATIONAL, 1\n\t.ifdef TLA_EDITION_JA\n\t.set EDITION_INTERNATIONAL, 0\n\t.endif\n").unwrap();
        let source = "\t.include \"games/tla/include/ed_asm.h\"\nResource_A:\n\t.incbin \"a.lz\"\n\t.ifdef TLA_EDITION_JA\n\t.byte 0x00, 0x01, 0xff\t@ PACKER: left in the buffer by Field_Map299+0x48d\n\t.else\n\t.byte 0x02\t@ PACKER: left in the buffer by Resource_B+16\n\t.endif\n\t.ifeq EDITION_INTERNATIONAL\n\t.byte 0x03\t@ PACKER: left in the buffer by Resource_C+0x1\n\t.endif\n\t.if UNKNOWN_SYMBOL\n\t.byte 0x04\t@ PACKER: left in the buffer by Resource_D+0x1\n\t.endif\n\t.byte 0xcc\t@ PACKER: unused post-stream residue; writer unknown\n";
        let path = root.join("games/tla/src/overlays.s");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, source).unwrap();
        let ja = rows(root, id("tla-ja"), &path).unwrap();
        let row = |line, bytes: &[u8], provider: &str, offset| Row { line, bytes: bytes.to_vec(), provider: provider.into(), offset };
        assert_eq!(ja.rows, [row(5, &[0, 1, 0xff], "Field_Map299", 0x48d), row(10, &[3], "Resource_C", 1)]);
        assert_eq!(ja.unread, 1);
        let en = rows(root, id("tla-en"), &path).unwrap();
        assert_eq!(en.rows, [row(7, &[2], "Resource_B", 16)]);
        assert_eq!(files(root, "tla"), [path]);
    }
}
