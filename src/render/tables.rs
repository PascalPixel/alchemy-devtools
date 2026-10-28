//! The Broken Seal's data tables as the game holds them: each table's bytes
//! from the built English edition (out/tbs-en/tbs-en.elf, by its symbol),
//! read field by field through the struct the source declares it with
//! (games/tbs/include), and each row's name from the English text
//! (text/en.po). Nothing here keeps a copy of a table or of a struct's
//! layout: the layout is worked out from the declaration, as alchemy-gcc lays
//! it out, and checked against the header's own STATIC_ASSERTs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A field's type, as far as reading its bytes needs.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Type {
    Int { size: usize, signed: bool },
    Array(Box<Type>, usize),
    Struct(Layout),
}

/// A struct: its fields in order, at their offsets, and its size.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Layout {
    fields: Vec<(String, usize, Type)>,
    size: usize,
    align: usize,
}

impl Type {
    fn size(&self) -> usize {
        match self {
            Type::Int { size, .. } => *size,
            Type::Array(element, count) => element.size() * count,
            Type::Struct(layout) => layout.size,
        }
    }

    fn align(&self) -> usize {
        match self {
            Type::Int { size, .. } => *size,
            Type::Array(element, _) => element.align(),
            Type::Struct(layout) => layout.align,
        }
    }

    /// Every integer of a value of this type at `at` in `bytes`, arrays and
    /// structs flattened in order.
    fn read(&self, bytes: &[u8], at: usize, out: &mut Vec<i64>) {
        match self {
            Type::Int { size, signed } => {
                let mut value = 0u64;
                for i in (0..*size).rev() {
                    value = value << 8 | u64::from(bytes.get(at + i).copied().unwrap_or(0));
                }
                let bits = 8 * *size as u32;
                let value = if *signed && bits < 64 && value >> (bits - 1) & 1 != 0 { value as i64 - (1i64 << bits) } else { value as i64 };
                out.push(value);
            }
            Type::Array(element, count) => {
                for i in 0..*count {
                    element.read(bytes, at + i * element.size(), out);
                }
            }
            Type::Struct(layout) => {
                for (_, offset, field) in &layout.fields {
                    field.read(bytes, at + offset, out);
                }
            }
        }
    }
}

fn scalar(name: &str) -> Option<Type> {
    let (size, signed) = match name {
        "u8" | "vu8" | "bool8" | "unsigned char" => (1, false),
        "s8" | "vs8" | "char" | "signed char" => (1, true),
        "u16" | "vu16" | "bool16" | "unsigned short" => (2, false),
        "s16" | "vs16" | "short" => (2, true),
        "u32" | "vu32" | "bool32" | "unsigned" | "unsigned int" | "unsigned long" => (4, false),
        "s32" | "vs32" | "int" | "long" => (4, true),
        _ => return None,
    };
    Some(Type::Int { size, signed })
}

/// The headers of games/tbs/include: their text with comments removed.
struct Headers {
    texts: Vec<(PathBuf, String)>,
    /// Each object-like `#define`'s replacement text, and each enumerator's
    /// expression (its value counted on from the one before where it has
    /// none, as `PREVIOUS + 1`).
    defines: HashMap<String, String>,
}

impl Headers {
    fn read(root: &Path) -> Result<Headers, String> {
        let mut texts = Vec::new();
        for entry in walkdir::WalkDir::new(root.join("games/tbs/include")).sort_by_file_name() {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.path().extension().and_then(|e| e.to_str()) == Some("h") {
                let text = std::fs::read_to_string(entry.path()).map_err(|e| format!("cannot read {}: {e}", entry.path().display()))?;
                texts.push((entry.path().to_path_buf(), text));
            }
        }
        Headers::from_texts(texts)
    }

    fn from_texts(texts: Vec<(PathBuf, String)>) -> Result<Headers, String> {
        let comments = regex::Regex::new(r"(?s)/\*.*?\*/|//[^\n]*").map_err(|e| e.to_string())?;
        let define = regex::Regex::new(r"(?m)^\s*#\s*define\s+(\w+)[ \t]+(.+?)\s*$").map_err(|e| e.to_string())?;
        let enumeration = regex::Regex::new(r"(?s)\benum\s*\w*\s*\{([^}]*)\}").map_err(|e| e.to_string())?;
        let texts: Vec<(PathBuf, String)> = texts.into_iter().map(|(p, t)| (p, comments.replace_all(&t, "").into_owned())).collect();
        let mut defines = HashMap::new();
        for (_, text) in &texts {
            for found in define.captures_iter(text) {
                defines.entry(found[1].to_string()).or_insert_with(|| found[2].to_string());
            }
            for found in enumeration.captures_iter(text) {
                let mut previous: Option<String> = None;
                for item in found[1].split(',').map(str::trim).filter(|i| !i.is_empty()) {
                    let (name, value) = match item.split_once('=') {
                        Some((name, value)) => (name.trim().to_string(), value.trim().to_string()),
                        None => (item.to_string(), previous.as_ref().map_or("0".to_string(), |p| format!("{p} + 1"))),
                    };
                    defines.entry(name.clone()).or_insert(value);
                    previous = Some(name);
                }
            }
        }
        Ok(Headers { texts, defines })
    }

    /// The value of a number, a `#define` or an enumerator, or of an
    /// expression of them in `| & + - * << >> ~ ( )`.
    fn constant(&self, text: &str) -> Option<i64> {
        let tokens = regex::Regex::new(r"0[xX][0-9a-fA-F]+|\d+|\w+|<<|>>|[-+*|&~()]").ok()?;
        let tokens: Vec<&str> = tokens.find_iter(text).map(|m| m.as_str()).collect();
        let mut at = 0;
        let value = self.expression(&tokens, &mut at, 0, 0)?;
        (at == tokens.len()).then_some(value)
    }

    /// Binary operators by precedence, loosest first.
    fn expression(&self, tokens: &[&str], at: &mut usize, level: usize, depth: usize) -> Option<i64> {
        const LEVELS: [&[&str]; 5] = [&["|"], &["&"], &["<<", ">>"], &["+", "-"], &["*"]];
        if depth > 64 {
            return None;
        }
        let Some(operators) = LEVELS.get(level) else {
            return self.unary(tokens, at, depth);
        };
        let mut value = self.expression(tokens, at, level + 1, depth)?;
        while let Some(&op) = tokens.get(*at).filter(|t| operators.contains(t)) {
            *at += 1;
            let right = self.expression(tokens, at, level + 1, depth)?;
            value = match op {
                "|" => value | right,
                "&" => value & right,
                "<<" => value << right,
                ">>" => value >> right,
                "+" => value + right,
                "-" => value - right,
                _ => value * right,
            };
        }
        Some(value)
    }

    fn unary(&self, tokens: &[&str], at: &mut usize, depth: usize) -> Option<i64> {
        let token = *tokens.get(*at)?;
        *at += 1;
        match token {
            "-" => Some(-self.unary(tokens, at, depth)?),
            "~" => Some(!self.unary(tokens, at, depth)?),
            "(" => {
                let value = self.expression(tokens, at, 0, depth + 1)?;
                (tokens.get(*at) == Some(&")")).then(|| *at += 1)?;
                Some(value)
            }
            _ => number(token).or_else(|| {
                let text = self.defines.get(token)?;
                let tokens = regex::Regex::new(r"0[xX][0-9a-fA-F]+|\d+|\w+|<<|>>|[-+*|&~()]").ok()?;
                let inner: Vec<&str> = tokens.find_iter(text).map(|m| m.as_str()).collect();
                let mut inner_at = 0;
                let value = self.expression(&inner, &mut inner_at, 0, depth + 1)?;
                (inner_at == inner.len()).then_some(value)
            }),
        }
    }

    /// The layout of `struct name`, as alchemy-gcc lays it out: each field at
    /// its type's alignment, every struct aligned and padded to 4 bytes.
    fn layout(&self, name: &str) -> Result<Layout, String> {
        let start = regex::Regex::new(&format!(r"\bstruct\s+{}\s*\{{", regex::escape(name))).map_err(|e| e.to_string())?;
        let (path, text, body) = self
            .texts
            .iter()
            .find_map(|(path, text)| {
                let found = start.find(text)?;
                let end = text[found.end()..].find("};")? + found.end();
                Some((path, text, &text[found.end()..end]))
            })
            .ok_or_else(|| format!("no header declares struct {name}"))?;
        let mut fields = Vec::new();
        let (mut offset, mut align) = (0usize, 4usize);
        for member in body.split(';').map(str::trim).filter(|m| !m.is_empty()) {
            let member = member.split_whitespace().collect::<Vec<_>>().join(" ");
            if member.contains(':') || member.contains('(') {
                return Err(format!("struct {name}: cannot lay out `{member}`"));
            }
            let words: Vec<&str> = member.split(' ').filter(|w| !matches!(*w, "const" | "volatile")).collect();
            let (base, declarators) = match words.first() {
                Some(&"struct") => {
                    let tag = words.get(1).ok_or_else(|| format!("struct {name}: `{member}`"))?;
                    (Type::Struct(self.layout(tag)?), words[2..].join(" "))
                }
                Some(&"unsigned" | &"signed") if words.len() > 2 && scalar(&words[..2].join(" ")).is_some() => {
                    (scalar(&words[..2].join(" ")).unwrap(), words[2..].join(" "))
                }
                Some(word) => (scalar(word).ok_or_else(|| format!("struct {name}: unknown type in `{member}`"))?, words[1..].join(" ")),
                None => continue,
            };
            for declarator in declarators.split(',').map(str::trim) {
                let (field, dims) = declarator.split_once('[').map_or((declarator, ""), |(f, d)| (f.trim(), d));
                let mut kind = if field.starts_with('*') { Type::Int { size: 4, signed: false } } else { base.clone() };
                let mut lengths = Vec::new();
                for dim in dims.split('[').map(|d| d.trim().trim_end_matches(']').trim()).filter(|d| !d.is_empty()) {
                    lengths.push(self.constant(dim).filter(|&n| n >= 0).map(|n| n as usize).ok_or_else(|| format!("struct {name}: cannot size {field}[{dim}]"))?);
                }
                for length in lengths.into_iter().rev() {
                    kind = Type::Array(Box::new(kind), length);
                }
                offset = offset.next_multiple_of(kind.align());
                align = align.max(kind.align());
                let size = kind.size();
                fields.push((field.trim_start_matches('*').to_string(), offset, kind));
                offset += size;
            }
        }
        let layout = Layout { fields, size: offset.next_multiple_of(align), align };
        self.check(name, &layout, text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(layout)
    }

    /// The header's STATIC_ASSERTs on `struct name` hold for `layout`.
    fn check(&self, name: &str, layout: &Layout, text: &str) -> Result<(), String> {
        let tag = regex::escape(name);
        let size = regex::Regex::new(&format!(r"STATIC_ASSERT\(\s*sizeof\(struct {tag}\)\s*==\s*(\w+)\s*\)")).map_err(|e| e.to_string())?;
        for found in size.captures_iter(text) {
            if let Some(want) = self.constant(&found[1]).map(|n| n as usize) {
                if want != layout.size {
                    return Err(format!("struct {name} lays out as {} bytes, its STATIC_ASSERT says {want}", layout.size));
                }
            }
        }
        let offset = regex::Regex::new(&format!(r"STATIC_ASSERT\(\s*offsetof\(struct {tag},\s*(\w+)\)\s*==\s*(\w+)\s*\)")).map_err(|e| e.to_string())?;
        for found in offset.captures_iter(text) {
            let (Some(at), Some(want)) = (layout.fields.iter().find(|f| f.0 == found[1]).map(|f| f.1), self.constant(&found[2]).map(|n| n as usize)) else {
                continue;
            };
            if at != want {
                return Err(format!("struct {name}: {} lays out at {at:#x}, its STATIC_ASSERT says {want:#x}", &found[1]));
            }
        }
        Ok(())
    }
}

fn number(text: &str) -> Option<i64> {
    let text = text.trim();
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => i64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

/// One table: an array of `struct` rows, read from the ELF.
pub struct Table {
    layout: Layout,
    bytes: Vec<u8>,
    symbol: String,
}

impl Table {
    pub fn len(&self) -> usize {
        self.bytes.len() / self.layout.size
    }

    /// The integers of field `path` (`price`, `abilities`,
    /// `abilities.ability`) in row `row`, arrays flattened.
    pub fn values(&self, row: usize, path: &str) -> Result<Vec<i64>, String> {
        if row >= self.len() {
            return Err(format!("{} has no row {row}", self.symbol));
        }
        let mut found = vec![(row * self.layout.size, Type::Struct(self.layout.clone()))];
        for name in path.split('.') {
            let mut next = Vec::new();
            for (at, kind) in found {
                let mut kind = kind;
                let mut at = vec![at];
                while let Type::Array(element, count) = kind {
                    let step = element.size();
                    at = at.iter().flat_map(|&a| (0..count).map(move |i| a + i * step)).collect();
                    kind = *element;
                }
                let Type::Struct(layout) = kind else {
                    return Err(format!("{}: {path} reads into a number", self.symbol));
                };
                let (_, offset, field) = layout.fields.iter().find(|f| f.0 == name).ok_or_else(|| format!("{} has no field {path}", self.symbol))?;
                next.extend(at.into_iter().map(|a| (a + offset, field.clone())));
            }
            found = next;
        }
        let mut out = Vec::new();
        for (at, kind) in found {
            kind.read(&self.bytes, at, &mut out);
        }
        Ok(out)
    }

    /// The one integer of field `path` in row `row`.
    pub fn value(&self, row: usize, path: &str) -> Result<i64, String> {
        match self.values(row, path)?[..] {
            [value] => Ok(value),
            _ => Err(format!("{}: {path} is not one number", self.symbol)),
        }
    }

    /// Field `path` of every row.
    pub fn column(&self, path: &str) -> Result<Vec<i64>, String> {
        (0..self.len()).map(|row| self.value(row, path)).collect()
    }
}

/// The built English edition and the source that describes it.
pub struct Game {
    root: PathBuf,
    elf: crate::elf::Elf,
    headers: Headers,
    catalog: crate::po::Catalog,
}

impl Game {
    pub fn read(root: &Path) -> Result<Game, String> {
        let path = root.join("out/tbs-en/tbs-en.elf");
        let data = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e} (make tbs-en)", path.display()))?;
        Ok(Game {
            root: root.to_path_buf(),
            elf: crate::elf::Elf::parse(&data)?,
            headers: Headers::read(root)?,
            catalog: crate::po::read_catalog(&root.join("games/tbs/text/en.po"))?,
        })
    }

    /// The game of the checkout the program runs in, read once.
    pub fn here() -> Result<&'static Game, String> {
        static GAME: std::sync::OnceLock<Result<Game, String>> = std::sync::OnceLock::new();
        GAME.get_or_init(|| Game::read(crate::compile::root())).as_ref().map_err(Clone::clone)
    }

    /// Table `symbol`, rows of `struct row`.
    pub fn table(&self, symbol: &str, row: &str) -> Result<Table, String> {
        let layout = self.headers.layout(row)?;
        let found = self.elf.symbols.iter().find(|s| s.name == symbol && s.size > 0).ok_or_else(|| format!("{} has no {symbol}", self.root.join("out/tbs-en/tbs-en.elf").display()))?;
        let (start, size) = (found.value as usize, found.size as usize);
        let bytes = self
            .elf
            .sections
            .iter()
            .find_map(|s| s.bytes.get(start.checked_sub(s.address as usize)?..start - s.address as usize + size))
            .ok_or_else(|| format!("{symbol} lies in no section"))?;
        if size % layout.size != 0 {
            return Err(format!("{symbol}: {size} bytes is no whole number of struct {row} ({} bytes)", layout.size));
        }
        Ok(Table { layout, bytes: bytes.to_vec(), symbol: symbol.to_string() })
    }

    /// The `count` names that follow message `first` (`MsgAbilityName`): the
    /// name of row n is that message plus n, its control codes left out.
    pub fn names(&self, first: &str, count: usize) -> Result<Vec<String>, String> {
        let key = |e: &crate::po::Entry| e.id.parse::<usize>().ok();
        let base = self.catalog.entries.iter().find(|e| e.context.as_deref() == Some(first)).and_then(key).ok_or_else(|| format!("en.po has no {first}"))?;
        let by_key: HashMap<usize, &str> = self.catalog.entries.iter().filter_map(|e| Some((key(e)?, e.value.as_str()))).collect();
        let codes = regex::Regex::new(r"\{[^}]*\}").map_err(|e| e.to_string())?;
        Ok((0..count).map(|n| by_key.get(&(base + n)).map(|v| codes.replace_all(v, "").into_owned()).unwrap_or_default()).collect())
    }

    /// The names of table `symbol`'s rows (`struct row`), message `first` and
    /// those after it.
    pub fn row_names(&self, symbol: &str, row: &str, first: &str) -> Result<Vec<String>, String> {
        self.names(first, self.table(symbol, row)?.len())
    }

    /// The items' names by item id.
    pub fn item_names(&self) -> Result<Vec<String>, String> {
        self.row_names("gItem_DefinitionTable", "ItemDefinition", "MsgItemName")
    }

    /// The abilities' names by ability id.
    pub fn ability_names(&self) -> Result<Vec<String>, String> {
        self.row_names("gAbilityDefinitions", "AbilityDefinition", "MsgAbilityName")
    }

    /// The value of `#define name` or enumerator `name` in the headers.
    pub fn constant(&self, name: &str) -> Result<i64, String> {
        self.headers.constant(name).ok_or_else(|| format!("the headers give no value to {name}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(text: &str) -> Headers {
        Headers::from_texts(vec![(PathBuf::from("t.h"), text.to_string())]).unwrap()
    }

    const HEADER: &str = "#define COUNT 4
struct Pair {
  u8 kind;   ///< one
  s8 amount; // two
};
struct Row {
  s16 price;
  u8 type, flags;
  u32 word;
  struct Pair pairs[COUNT];
  u16 ids[2][3];
  u8 last;
};
STATIC_ASSERT(sizeof(struct Row) == 0x28);
STATIC_ASSERT(offsetof(struct Row, pairs) == 0x08);
";

    #[test]
    fn constants_count_on_and_evaluate() {
        let h = headers("enum Element { VENUS, MERCURY = 3, MARS, /* x */ NONE, COUNT = NONE };\n#define END -1 ///< y\n#define SHIFTED (1 << 4 | MARS)\n");
        assert_eq!([h.constant("MARS"), h.constant("COUNT"), h.constant("END"), h.constant("SHIFTED")], [Some(4), Some(5), Some(-1), Some(20)]);
        assert_eq!(h.constant("MISSING"), None);
    }

    #[test]
    fn layouts_follow_alignment_and_pad_structs_to_four() {
        let h = headers(HEADER);
        let pair = h.layout("Pair").unwrap();
        assert_eq!(pair.size, 4);
        let row = h.layout("Row").unwrap();
        let at: Vec<(&str, usize)> = row.fields.iter().map(|f| (f.0.as_str(), f.1)).collect();
        assert_eq!(at, [("price", 0), ("type", 2), ("flags", 3), ("word", 4), ("pairs", 8), ("ids", 24), ("last", 36)]);
        assert_eq!(row.size, 40);
    }

    #[test]
    fn a_wrong_static_assert_is_an_error() {
        let h = headers(&HEADER.replace("== 0x08", "== 0x0c"));
        assert!(h.layout("Row").unwrap_err().contains("pairs"));
    }

    #[test]
    fn fields_read_signed_and_through_arrays() {
        let h = headers(HEADER);
        let layout = h.layout("Row").unwrap();
        let mut bytes = vec![0u8; 80];
        bytes[0..2].copy_from_slice(&(-5i16).to_le_bytes());
        bytes[9] = 0xff;
        bytes[12] = 7;
        bytes[40 + 36] = 9;
        let table = Table { layout, bytes, symbol: "gRows".into() };
        assert_eq!(table.len(), 2);
        assert_eq!(table.value(0, "price").unwrap(), -5);
        assert_eq!(table.values(0, "pairs.amount").unwrap(), [-1, 0, 0, 0]);
        assert_eq!(table.values(0, "pairs.kind").unwrap(), [0, 7, 0, 0]);
        assert_eq!(table.values(0, "ids").unwrap().len(), 6);
        assert_eq!(table.column("last").unwrap(), [0, 9]);
        assert!(table.value(2, "last").is_err());
    }

    /// The checkout's tables, where tbs-en is built: each lays out as a
    /// whole number of rows and reads as the game shows it.
    #[test]
    fn the_checkout_tables_read() {
        let root = crate::compile::root();
        if !root.join("out/tbs-en/tbs-en.elf").exists() {
            eprintln!("no out/tbs-en/tbs-en.elf: make tbs-en to read the tables");
            return;
        }
        let game = Game::read(root).unwrap();
        let items = game.table("gItem_DefinitionTable", "ItemDefinition").unwrap();
        let long_sword = game.item_names().unwrap().iter().position(|n| n == "Long Sword").unwrap();
        assert_eq!(items.value(long_sword, "type").unwrap(), game.constant("ITEM_TYPE_WEAPON").unwrap());
        let abilities = game.ability_names().unwrap();
        assert_eq!(abilities.len(), game.table("gAbilityDefinitions", "AbilityDefinition").unwrap().len());
        assert_eq!(abilities[game.constant("ABILITY_MOVE").unwrap() as usize], "Move");
        for (symbol, row) in [
            ("gClass_DefinitionTable", "ClassDefinition"),
            ("gDjinn_DefinitionTable", "DjinnDefinition"),
            ("gSummon_DefinitionTable", "SummonDefinition"),
            ("gEnemy_Definitions", "EnemyDefinition"),
            ("gDebug_ParameterSets", "DebugParameterSetRow"),
        ] {
            assert!(game.table(symbol, row).unwrap().len() > 0, "{symbol}");
        }
        assert_eq!(game.constant("DEBUG_PARAMETER_SET_END").unwrap(), -1);
    }
}
