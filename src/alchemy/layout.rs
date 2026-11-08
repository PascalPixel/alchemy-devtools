//! Alchemy's layout: where a game keeps its object list, its English
//! listings, each edition's scaffold, its overlays' link scripts and
//! scaffolds and its drafts, and how an adopted module is named. The object
//! list and the scaffolds are read and written as text, line for line, so an
//! edit changes only the lines it means to.

/// The editions' languages, in the order the object list writes them.
pub const LANGUAGES: [&str; 6] = ["en", "de", "es", "fr", "it", "ja"];

/// Where every overlay links. An overlay draft named `resource_ID_c_ADDRESS.c`
/// names its function's address less 0x8000, the form its listing's labels
/// take; one named `resource_ID/ADDRESS.c` names the address itself. An
/// adopted overlay module is named by its offset from here.
pub const OVERLAY_BASE: u32 = 0x0200_8000;
const OVERLAY_LABEL_OFFSET: u32 = 0x8000;

/// The sections a row places when it names none.
const DEFAULT_SECTIONS: &str = ".text .rodata .data .padding";

pub struct Layout {
    pub game: String,
}

impl Layout {
    pub fn new(game: &str) -> Layout {
        Layout { game: game.to_string() }
    }

    /// The game a path under games/ or recon/ belongs to.
    pub fn of_path(path: &str) -> Option<Layout> {
        let path = path.trim_start_matches("./");
        let rest = path.strip_prefix("games/").or_else(|| path.strip_prefix("recon/"))?;
        let game = rest.split('/').next().filter(|game| !game.is_empty())?;
        Some(Layout::new(game))
    }

    pub fn objects(&self) -> String {
        format!("games/{}/objects.tsv", self.game)
    }

    /// The folder adopted C goes under.
    pub fn sources(&self) -> String {
        format!("games/{}/src/", self.game)
    }

    /// An English listing's object (its source adds `.s`).
    pub fn listing(&self, address: &str) -> String {
        format!("recon/{}/raw/{address}", self.game)
    }

    /// The address an English listing's object is named after.
    pub fn listing_address<'a>(&self, object: &'a str) -> Option<&'a str> {
        object.strip_prefix(&format!("recon/{}/raw/", self.game)).filter(|rest| !rest.contains('/'))
    }

    pub fn main_draft(&self, address: &str) -> String {
        format!("recon/{}/en/main/{address}.c", self.game)
    }

    /// An edition's scaffold object (its source adds `.s`), whose `.rom.OFFSET`
    /// sections each place that edition's bytes from OFFSET of its ROM.
    pub fn scaffold(&self, language: &str) -> String {
        format!("recon/{}/{language}/rom", self.game)
    }

    pub fn overlay_script(&self, id: &str) -> String {
        format!("recon/{}/raw/overlays/resource_{id}.ld", self.game)
    }

    pub fn overlay_scaffold(&self, id: &str) -> String {
        format!("recon/{}/raw/overlays/resource_{id}_overlay.s", self.game)
    }

    /// What an overlay's link script says to place one scaffold section.
    pub fn overlay_scaffold_line(&self, id: &str, section: &str) -> String {
        format!("\"*/resource_{id}_overlay.o\"({section})")
    }

    /// The module a main-image draft becomes: `f` and its address without the
    /// cartridge's 08.
    pub fn main_module(&self, folder: &str, address: &str) -> String {
        format!("{folder}/f{}", address.strip_prefix("08").unwrap_or(address))
    }

    /// The module an overlay draft becomes: `s` and its offset in the overlay.
    pub fn overlay_module(&self, folder: &str, address: u32) -> String {
        format!("{folder}/s{:04x}", address.wrapping_sub(OVERLAY_BASE))
    }

    /// The scene or debug folder an overlay's link script names first.
    pub fn overlay_folder(&self, script: &str) -> Option<String> {
        let prefix = format!("\"*/games/{}/src/", self.game);
        script.lines().find_map(|line| {
            let path = line.trim().strip_prefix(&prefix)?;
            let (kind, rest) = path.split_once('/')?;
            let (folder, _) = rest.split_once('/')?;
            ((kind == "scene" || kind == "debug") && !folder.is_empty()).then(|| format!("games/{}/src/{kind}/{folder}", self.game))
        })
    }
}

/// An overlay draft's resource id and function address, from either naming.
pub fn overlay_draft(path: &str) -> Option<(String, u32)> {
    let path = path.trim_end_matches('/');
    let (folder, file) = path.rsplit_once('/').unwrap_or(("", path));
    let stem = file.strip_suffix(".c")?;
    if let Some((id, address)) = stem.strip_prefix("resource_").and_then(|rest| rest.split_once("_c_")) {
        let address = u32::from_str_radix(address, 16).ok().filter(|_| address.len() == 8)?;
        return Some((id.to_string(), address + OVERLAY_LABEL_OFFSET));
    }
    let id = folder.rsplit('/').next()?.strip_prefix("resource_")?;
    let address = u32::from_str_radix(stem, 16).ok().filter(|_| stem.len() == 8)?;
    Some((id.to_string(), address))
}

/// The scaffold section that holds an overlay function.
pub fn overlay_section(address: u32) -> String {
    format!(".text.x{address:08x}")
}

/// One row of an object list: image, place, object, sections and editions,
/// kept as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub fields: Vec<String>,
}

impl Row {
    /// A row placing `sections` of `object` in `editions`, in the list's order;
    /// every edition writes no editions column.
    pub fn new(image: &str, place: &str, object: &str, sections: &str, editions: &[&str]) -> Row {
        let mut fields: Vec<String> = [image, place, object, sections].map(str::to_string).into();
        let editions = ordered(editions);
        if !editions.is_empty() && !LANGUAGES.iter().all(|language| editions.contains(language)) {
            fields.push(editions.join(" "));
        }
        Row { fields }
    }

    fn field(&self, index: usize) -> &str {
        self.fields.get(index).map_or("", String::as_str)
    }

    pub fn image(&self) -> &str {
        self.field(0)
    }

    pub fn place(&self) -> &str {
        self.field(1)
    }

    pub fn object(&self) -> &str {
        self.field(2)
    }

    pub fn sections(&self) -> &str {
        match self.field(3) {
            "" => DEFAULT_SECTIONS,
            sections => sections,
        }
    }

    /// The editions the row names; None when it places in every edition.
    pub fn editions(&self) -> Option<Vec<&str>> {
        let editions = self.field(4);
        (!editions.is_empty()).then(|| editions.split(' ').filter(|e| !e.is_empty()).collect())
    }

    pub fn links(&self, language: &str) -> bool {
        self.editions().is_none_or(|editions| editions.contains(&language))
    }

    /// The same row placing in `language` too.
    pub fn with_edition(&self, language: &str) -> Row {
        let mut editions = self.editions().unwrap_or(LANGUAGES.to_vec());
        editions.push(language);
        Row::new(self.image(), self.place(), self.object(), self.field(3), &editions)
    }
}

fn ordered<'a>(editions: &[&'a str]) -> Vec<&'a str> {
    let mut known: Vec<&str> = LANGUAGES.iter().copied().filter(|language| editions.contains(language)).collect();
    for edition in editions {
        if !known.contains(edition) {
            known.push(edition);
        }
    }
    known
}

/// An object list: a header line, then one row a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectList {
    header: Option<String>,
    pub rows: Vec<Row>,
}

impl ObjectList {
    pub fn parse(text: &str) -> ObjectList {
        let mut lines = text.lines().peekable();
        let header = lines.next_if(|line| line.starts_with("image\t")).map(str::to_string);
        let rows = lines.map(|line| Row { fields: line.split('\t').map(str::to_string).collect() }).collect();
        ObjectList { header, rows }
    }

    pub fn render(&self) -> String {
        let mut text = String::new();
        for line in self.header.iter().cloned().chain(self.rows.iter().map(|row| row.fields.join("\t"))) {
            text.push_str(&line);
            text.push('\n');
        }
        text
    }

    /// The first row placing `sections` of `object` in `language`.
    pub fn find(&self, object: &str, sections: &str, language: &str) -> Option<usize> {
        self.rows.iter().position(|row| row.object() == object && row.sections() == sections && row.links(language))
    }

    /// The first row after `index` that `language` links.
    pub fn next_linked(&self, index: usize, language: &str) -> Option<usize> {
        (index + 1..self.rows.len()).find(|&i| self.rows[i].links(language))
    }
}

/// A section of an assembly file: its name and its lines, from its
/// `.section` line up to the next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

fn section_name(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix(".section")?;
    let rest = rest.strip_prefix(|c: char| c.is_whitespace())?.trim_start();
    let end = rest.find(|c: char| c == ',' || c.is_whitespace()).unwrap_or(rest.len());
    Some(&rest[..end])
}

pub fn sections(lines: &[&str]) -> Vec<Section> {
    let starts: Vec<(usize, &str)> = lines.iter().enumerate().filter_map(|(i, line)| Some((i, section_name(line)?))).collect();
    starts
        .iter()
        .enumerate()
        .map(|(n, &(start, name))| Section {
            name: name.to_string(),
            start,
            end: starts.get(n + 1).map_or(lines.len(), |&(next, _)| next),
        })
        .collect()
}

/// The names lines make global.
pub fn globals(lines: &[&str]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix(".global").or_else(|| line.strip_prefix(".globl"))?;
            let name = rest.strip_prefix(|c: char| c.is_whitespace())?.trim();
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect()
}

/// The names lines define: their globals and their labels.
fn names(lines: &[&str]) -> Vec<String> {
    let mut names = globals(lines);
    for line in lines {
        if let Some(label) = line.strip_suffix(':').filter(|l| !l.starts_with('.') && is_identifier(l)) {
            if !names.iter().any(|name| name == label) {
                names.push(label.to_string());
            }
        }
    }
    names
}

fn is_identifier(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with(|c: char| c.is_ascii_digit())
        && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether lines assemble differently by edition (`.ifdef GAME_EDITION_...`).
pub fn has_edition_forms(lines: &[&str]) -> bool {
    lines.iter().any(|line| {
        let line = line.trim_start();
        let rest = line.strip_prefix(".ifdef").or_else(|| line.strip_prefix(".ifndef"));
        rest.is_some_and(|rest| rest.contains("_EDITION_"))
    })
}

/// Whether a section assembles only in some editions: everything after its
/// `.section` line and alignment is one `.ifdef GAME_EDITION_...` block with
/// no `.else`, as a Japanese form left on the scaffold is.
pub fn edition_only(lines: &[&str]) -> bool {
    let content: Vec<&str> = lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && section_name(line).is_none() && !line.starts_with(".balign") && !line.starts_with(".align"))
        .collect();
    let (Some(first), Some(last)) = (content.first(), content.last()) else { return false };
    if !has_edition_forms(&[first]) || *last != ".endif" {
        return false;
    }
    let mut depth = 0;
    for (index, line) in content.iter().enumerate() {
        if line.starts_with(".if") {
            depth += 1;
        } else if *line == ".endif" {
            depth -= 1;
            if depth == 0 && index + 1 != content.len() {
                return false;
            }
        } else if depth == 1 && (line.starts_with(".else") || line.starts_with(".elseif")) {
            return false;
        }
    }
    true
}

/// Whether code ends on a halfword and its listing pads it to a word: its
/// last line is `.2byte 0x0000`. Compiled C has no such pad, so whatever
/// follows has to align itself.
pub fn ends_with_padding(lines: &[&str]) -> bool {
    lines.iter().rev().map(|line| line.trim()).find(|line| !line.is_empty()) == Some(".2byte 0x0000")
}

/// Whether a section's or a listing's lines align it before its first byte.
pub fn aligns(lines: &[&str]) -> bool {
    lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && section_name(line).is_none())
        .take_while(|line| line.starts_with('.') && !line.starts_with(".incbin") && !line.starts_with(".2byte") && !line.starts_with(".4byte"))
        .any(|line| line.starts_with(".balign") || line.starts_with(".align") || line.starts_with(".p2align"))
}

/// One `.incbin "baserom.gba", OFFSET, LENGTH` line's offset and length.
pub fn incbin(line: &str) -> Option<(u32, u32)> {
    let rest = line.trim().strip_prefix(".incbin")?.trim_start();
    let mut parts = rest.split(',').map(str::trim);
    let _file = parts.next()?;
    let offset = u32::from_str_radix(parts.next()?.strip_prefix("0x")?, 16).ok()?;
    let length = u32::from_str_radix(parts.next()?.strip_prefix("0x")?, 16).ok()?;
    Some((offset, length))
}

/// The offset a `.rom.OFFSET` section places from.
pub fn rom_offset(section: &str) -> Option<u32> {
    u32::from_str_radix(section.strip_prefix(".rom.")?, 16).ok()
}

pub fn rom_section(offset: u32) -> String {
    format!(".rom.{offset:08x}")
}

/// A stretch of a scaffold section: the label lines that open it (none for
/// the bytes at a section's start) and the bytes they name, up to the next
/// label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub start: usize,
    pub end: usize,
    pub names: Vec<String>,
    pub offset: Option<u32>,
}

/// A section's pieces, its `.section` line left out.
pub fn pieces(lines: &[&str], section: &Section) -> Vec<Piece> {
    let mut pieces: Vec<Piece> = Vec::new();
    let mut bytes_seen = false;
    for i in section.start + 1..section.end {
        let bytes = incbin(lines[i]);
        if pieces.is_empty() || (bytes.is_none() && bytes_seen && !lines[i].trim().is_empty()) {
            pieces.push(Piece { start: i, end: i, names: Vec::new(), offset: None });
            bytes_seen = false;
        }
        let piece = pieces.last_mut().unwrap();
        piece.end = i + 1;
        if let Some((offset, _)) = bytes {
            piece.offset.get_or_insert(offset);
            bytes_seen = true;
        }
    }
    for piece in &mut pieces {
        piece.names = names(&lines[piece.start..piece.end]);
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_round_trip_and_canonicalize_only_what_changes() {
        let text = "image\tplace\tobject\tsections\teditions\nmain\trom\tgames/tla/src/a\nmain\trom\trecon/tla/raw/08000100\t.text\ten\nmain\trom\tgames/tla/src/b\t.text\tja en\n";
        let list = ObjectList::parse(text);
        assert_eq!(list.render(), text);
        assert!(list.rows[0].links("de"));
        assert_eq!(list.rows[0].sections(), DEFAULT_SECTIONS);
        assert!(!list.rows[1].links("de"));
        assert_eq!(list.rows[2].with_edition("de").fields.join("\t"), "main\trom\tgames/tla/src/b\t.text\ten de ja");
        let all = Row::new("main", "rom", "games/tla/src/c", ".text", &["it", "ja", "en", "de", "es", "fr"]);
        assert_eq!(all.fields.len(), 4);
        assert_eq!(list.find("recon/tla/raw/08000100", ".text", "en"), Some(1));
        assert_eq!(list.next_linked(1, "ja"), Some(2));
    }

    #[test]
    fn overlay_drafts_are_named_two_ways() {
        assert_eq!(overlay_draft("recon/tla/en/overlays/resource_674_c_020006f4.c"), Some(("674".into(), 0x020086f4)));
        assert_eq!(overlay_draft("recon/tla/en/overlays/resource_67c/020084bc.c"), Some(("67c".into(), 0x020084bc)));
        assert_eq!(overlay_draft("recon/tla/en/main/080e2264.c"), None);
        let layout = Layout::new("tla");
        assert_eq!(layout.overlay_module("games/tla/src/scene/islets", 0x020086f4), "games/tla/src/scene/islets/s06f4");
        assert_eq!(layout.main_module("games/tla/src/battle_effect", "0816f080"), "games/tla/src/battle_effect/f16f080");
        let script = "        \"*/games/tla/src/field/object/x.o\"(.text)\n        \"*/games/tla/src/scene/islets/entry.o\"(.text)\n";
        assert_eq!(layout.overlay_folder(script).as_deref(), Some("games/tla/src/scene/islets"));
        assert_eq!(Layout::of_path("recon/tla/en/main/080e2264.c").map(|l| l.game).as_deref(), Some("tla"));
        assert_eq!(layout.listing_address("recon/tla/raw/0802be4c"), Some("0802be4c"));
        assert_eq!(layout.listing_address("recon/tla/raw/overlays/resource_674"), None);
    }

    #[test]
    fn a_scaffold_section_divides_at_its_labels() {
        let text = ".syntax unified\n\t.section .rom.000e877c, \"ax\"\n\t.global F\n\t.type F, %function\n\t.thumb_func\nF:\n\t.incbin \"baserom.gba\", 0x000e877c, 0x00000220\n\t.global G\n\t.thumb_func\nG:\n\t.incbin \"baserom.gba\", 0x000e899c, 0x00000148\n\t.section .rom.000f0000, \"ax\"\n\t.balign 4\n\t.incbin \"baserom.gba\", 0x000f0000, 0x00000010\n";
        let lines: Vec<&str> = text.lines().collect();
        let sections = sections(&lines);
        assert_eq!(sections.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), [".rom.000e877c", ".rom.000f0000"]);
        let pieces = pieces(&lines, &sections[0]);
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].names, ["F"]);
        assert_eq!(pieces[1].names, ["G"]);
        assert_eq!(pieces[1].offset, Some(0xe899c));
        assert!(!aligns(&lines[sections[0].start..sections[0].end]));
        assert!(aligns(&lines[sections[1].start..sections[1].end]));
        assert_eq!(pieces_of(&lines, &sections[1]), vec![Vec::<String>::new()]);
        assert!(ends_with_padding(&["\tpop {pc}", "\t.2byte 0x0000", ""]));
        assert!(!ends_with_padding(&["\t.2byte 0x0000", "\t.4byte 0x1"]));
        assert!(has_edition_forms(&["\t.ifdef TLA_EDITION_JA"]));
        let japanese = ["\t.section .text.x0200af30,\"ax\",%progbits", "\t.balign 4", "\t.ifdef TLA_EDITION_JA", "F:", "\t.4byte 0x1", "\t.endif"];
        assert!(edition_only(&japanese));
        let both = ["\t.section .text.x0200ba14", "\t.ifdef TLA_EDITION_JA", "F:", "\t.else", "F:", "\t.endif"];
        assert!(!edition_only(&both));
        assert!(!edition_only(&["\t.section .text.x1", "\tpush {lr}", "\t.ifdef TLA_EDITION_JA", "\t.endif"]));
    }

    fn pieces_of(lines: &[&str], section: &Section) -> Vec<Vec<String>> {
        pieces(lines, section).into_iter().map(|piece| piece.names).collect()
    }
}
