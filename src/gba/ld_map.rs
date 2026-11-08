//! A GNU ld map: its output sections, and the input sections each places with
//! the object that placed it and the symbols the map lists inside it.

pub const ROM: u32 = 0x0800_0000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub name: String,
    pub path: String,
    pub address: u32,
    pub size: u32,
    pub load: u32,
    pub loaded: bool,
    pub symbols: Vec<(u32, String)>,
    /// The output section it is placed in, an index into `Map::outputs`.
    pub output: Option<usize>,
}

impl Placed {
    pub fn is_fill(&self) -> bool {
        self.name == "*fill*"
    }

    pub fn end(&self) -> u32 {
        self.address + self.size
    }
}

pub fn preferred(symbols: &[(u32, String)], address: u32) -> Option<&str> {
    let mut names = symbols.iter().filter(|(a, _)| *a == address).map(|(_, n)| n.as_str());
    let first = names.next()?;
    Some(std::iter::once(first).chain(names).find(|n| !is_placeholder(n)).unwrap_or(first))
}

/// A name given to what is not named yet: a word, `_` and its address in hex,
/// eight digits (`Func_08000010`, `gUnknown_0203A0F4`) or, as pret writes a
/// function's, seven with the leading 0 dropped (`sub_8000040`).
pub fn is_placeholder(name: &str) -> bool {
    name.rsplit_once('_').is_some_and(|(_, tail)| {
        let address = (tail.chars().all(|c| c.is_ascii_hexdigit())).then(|| u32::from_str_radix(tail, 16).ok()).flatten();
        match tail.len() {
            8 => address.is_some(),
            7 => address.is_some_and(|address| address >= 0x0200_0000),
            _ => false,
        }
    })
}

#[derive(Default)]
pub struct Map {
    pub placed: Vec<Placed>,
    pub outputs: Vec<Output>,
}

pub struct Output {
    pub name: String,
    pub address: u32,
    pub size: u32,
    pub load: u32,
}

fn hex(word: &str) -> Option<u32> {
    u64::from_str_radix(word.strip_prefix("0x")?, 16).ok().and_then(|v| u32::try_from(v).ok())
}

fn word(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    (&text[..end], &text[end..])
}

fn placement(text: &str) -> Option<(u32, u32, &str)> {
    let (address, rest) = word(text);
    let (size, rest) = word(rest);
    Some((hex(address)?, hex(size)?, rest.trim()))
}

impl Map {
    pub fn parse(text: &str) -> Map {
        let mut placed: Vec<Placed> = Vec::new();
        let mut outputs: Vec<Output> = Vec::new();
        // The output section being read; a line at the margin that is not
        // one (`LOAD`, `OUTPUT(...)`, `/DISCARD/`) ends it.
        let mut current: Option<usize> = None;
        let mut started = false;
        let mut pending: Option<(bool, String)> = None;
        for line in text.lines() {
            if !started {
                started = line.starts_with("Linker script and memory map");
                continue;
            }
            if line.trim().is_empty() {
                pending = None;
                continue;
            }
            let indented = line.starts_with(' ');
            let column_one = indented && !line[1..].starts_with(' ');
            if let Some((output, name)) = pending.take() {
                if let Some((address, size, rest)) = placement(line).filter(|_| indented && !column_one) {
                    if output {
                        outputs.push(output_section(name, address, size, rest));
                        current = Some(outputs.len() - 1);
                    } else {
                        push(&mut placed, name, address, size, rest, &outputs, current);
                    }
                    continue;
                }
            }
            if !indented {
                let (name, rest) = word(line);
                current = None;
                if rest.trim().is_empty() {
                    pending = Some((true, name.to_string()));
                } else if let Some((address, size, rest)) = placement(rest) {
                    outputs.push(output_section(name.to_string(), address, size, rest));
                    current = Some(outputs.len() - 1);
                }
                continue;
            }
            if column_one {
                let (name, rest) = word(line);
                if name.starts_with('*') && name != "*fill*" {
                    continue;
                }
                if rest.trim().is_empty() {
                    pending = Some((false, name.to_string()));
                } else if let Some((address, size, rest)) = placement(rest) {
                    push(&mut placed, name.to_string(), address, size, rest, &outputs, current);
                }
                continue;
            }
            let (address, rest) = word(line);
            let (name, tail) = word(rest);
            if let (Some(address), true, true) = (hex(address), tail.trim().is_empty(), is_identifier(name)) {
                if let Some(last) = placed.last_mut().filter(|p| !p.is_fill()) {
                    if address >= last.address && address <= last.end() {
                        last.symbols.push((address, name.to_string()));
                    }
                }
            }
        }
        let reserved: Vec<bool> = outputs
            .iter()
            .map(|o| {
                o.load != o.address
                    && outputs.iter().any(|q| q.size > 0 && q.load >= o.load && q.load - o.load < o.size && !std::ptr::eq(q, o))
            })
            .collect();
        for p in &mut placed {
            p.loaded = !is_unloaded(&p.name) && !p.output.is_some_and(|o| reserved[o]);
        }
        Map { placed, outputs }
    }

    pub fn symbol(&self, name: &str) -> Option<(&Placed, u32)> {
        self.placed.iter().find_map(|p| {
            p.symbols.iter().find(|(_, n)| n == name).map(|(a, _)| (p, *a))
        })
    }

    pub fn at_address(&self, address: u32) -> Option<&Placed> {
        self.placed
            .iter()
            .find(|p| !p.is_fill() && p.size > 0 && address >= p.address && address < p.end())
    }
}

fn output_section(name: String, address: u32, size: u32, rest: &str) -> Output {
    let load = rest.split_once("load address").and_then(|(_, load)| hex(load.trim()));
    Output { name, address, size, load: load.unwrap_or(address) }
}

fn push(placed: &mut Vec<Placed>, name: String, address: u32, size: u32, path: &str, outputs: &[Output], output: Option<usize>) {
    let relocation = output.map_or(0, |o| i64::from(outputs[o].load) - i64::from(outputs[o].address));
    placed.push(Placed {
        name,
        path: path.to_string(),
        address,
        size,
        load: (i64::from(address) + relocation) as u32,
        loaded: true,
        symbols: Vec::new(),
        output,
    });
}

fn is_unloaded(name: &str) -> bool {
    name.starts_with(".bss") || name.starts_with(".sbss") || name == "COMMON"
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '.')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

#[cfg(test)]
pub(crate) mod tests {
    #[test]
    fn placeholders_are_named_by_their_address() {
        for name in ["Func_08000010", "gData_02004695", "sub_8000040", "gUnknown_0203A0F4", "sub_80A1B2C"] {
            assert!(super::is_placeholder(name), "{name}");
        }
        for name in ["Input_Poll", "nullsub_1", "unk_14", "gText_1234567", "Func_0800001"] {
            assert!(!super::is_placeholder(name), "{name}");
        }
        let symbols = [(0x0800_0040, "sub_8000040".to_string()), (0x0800_0040, "AddOne".to_string())];
        assert_eq!(super::preferred(&symbols, 0x0800_0040), Some("AddOne"));
    }

    pub(crate) const MAP: &str = "\
Discarded input sections

 .ARM.attributes
                0x0000000000000000       0x20 /x/build/asm/unidentified.o

Linker script and memory map

                0x00000000000004ec                Decompress_MoveToFrontLzCommandsCodeSize = (Decompress_End - Decompress_MoveToFrontLzCommands)

.rom_start      0x0000000008000000       0x30
 */asm/rom.o(.rom.00000000)
 .rom.00000000  0x0000000008000000       0x10 /x/build/asm/rom.o
                0x0000000008000000                Rom_Start
 */src/system/io.o(.text .rodata .data .padding)
 .text          0x0000000008000010        0xe /x/build/src/system/io.o
                0x0000000008000010                Func_08000010
                0x0000000008000010                Input_Handle
                0x0000000008000018                Input_Poll
 */asm/rom.o(.rom.00000020)
 *fill*         0x000000000800001e        0x2
 .rom.00000020  0x0000000008000020       0x10 /x/build/asm/rom.o
                0x0000000008000020                Func_08000020
                0x0000000008000030                IwramCodeEnd = .

.iwram_resident 0x0000000003000000        0x8 load address 0x0000000008000030
 */src/system/iwram_routines.o(.text)
 .text          0x0000000003000000        0x8 /x/build/src/system/iwram_routines.o
                0x0000000003000000                IwramIrqMain
 .bss           0x0000000003000008        0x4 /x/build/src/system/iwram_routines.o

iwram           0x0000000003000100       0x40 load address 0x0000000008000038
 */asm/sym_iwram.o(.sym_iwram)
 .sym_iwram     0x0000000003000100       0x40 /x/build/asm/sym_iwram.o
                0x0000000003000100                gVBlankCount

.rom_main       0x0000000008000038       0x20
 */asm/unidentified.o(.unidentified.08000030)
 .unidentified.08000030
                0x0000000008000038       0x10 /x/build/asm/unidentified.o
                0x0000000008000038                Math_Table
 */asm/raw/08000048.o(.text)
 .text          0x0000000008000048       0x10 /x/build/asm/raw/08000048.o
                0x0000000008000048                Func_08000048
LOAD /x/build/asm/rom.o
";

}
