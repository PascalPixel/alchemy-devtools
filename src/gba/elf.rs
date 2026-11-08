
pub struct Section {
    pub address: u32,
    pub bytes: Vec<u8>,
}

pub struct Symbol {
    pub name: String,
    pub value: u32,
    pub size: u32,
    pub kind: u8,
    /// STB_LOCAL 0, STB_GLOBAL 1, STB_WEAK 2.
    pub bind: u8,
    pub section: u16,
}

/// STT_FUNC.
pub const STT_FUNC: u8 = 2;
/// The ARM ELF's Thumb function type (STT_LOPROC), which old assemblers
/// give a `.thumb_func`. 15 (STT_ARM_16BIT) is a marker, not a function:
/// GAS 2.10 gives it to `.gcc2_compiled.`.
pub const STT_ARM_TFUNC: u8 = 13;

impl Symbol {
    /// Whether the symbol names a function: FUNC or STT_ARM_TFUNC, and not a
    /// mapping symbol (`$t`) or a compiler's marker (`.gcc2_compiled.`).
    pub fn is_function(&self) -> bool {
        matches!(self.kind, STT_FUNC | STT_ARM_TFUNC) && !self.name.is_empty() && !self.name.starts_with(['$', '.'])
    }
}

pub struct Elf {
    pub sections: Vec<Section>,
    pub symbols: Vec<Symbol>,
}

fn half(data: &[u8], at: usize) -> Result<u16, String> {
    data.get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| format!("ELF truncated at {at:#x}"))
}

fn word(data: &[u8], at: usize) -> Result<u32, String> {
    data.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| format!("ELF truncated at {at:#x}"))
}

fn string(table: &[u8], at: usize) -> String {
    let tail = table.get(at..).unwrap_or(&[]);
    let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
    String::from_utf8_lossy(&tail[..end]).into_owned()
}

const SHT_PROGBITS: u32 = 1;
const SHT_SYMTAB: u32 = 2;
const SHF_ALLOC: u32 = 2;

/// One section header; a symbol's `section` is its index in the table.
pub struct Header {
    pub name: String,
    pub kind: u32,
    pub flags: u32,
    pub address: u32,
    pub offset: usize,
    pub size: usize,
    pub link: u32,
    pub info: u32,
}

/// Every section header, in the file's order, named from its string table.
pub fn headers(data: &[u8]) -> Result<Vec<Header>, String> {
    if data.get(..4) != Some(b"\x7fELF") || data.get(4) != Some(&1) || data.get(5) != Some(&1) {
        return Err("not a little-endian ELF32 file".into());
    }
    let shoff = word(data, 0x20)? as usize;
    let shentsize = half(data, 0x2e)? as usize;
    let shnum = half(data, 0x30)? as usize;
    let mut headers = Vec::with_capacity(shnum);
    let mut names = Vec::with_capacity(shnum);
    for i in 0..shnum {
        let at = shoff + i * shentsize;
        names.push(word(data, at)? as usize);
        headers.push(Header {
            name: String::new(),
            kind: word(data, at + 4)?,
            flags: word(data, at + 8)?,
            address: word(data, at + 12)?,
            offset: word(data, at + 16)? as usize,
            size: word(data, at + 20)? as usize,
            link: word(data, at + 24)?,
            info: word(data, at + 28)?,
        });
    }
    let strings = headers.get(half(data, 0x32)? as usize).and_then(|h| data.get(h.offset..h.offset + h.size));
    for (header, name) in headers.iter_mut().zip(names) {
        header.name = strings.map(|table| string(table, name)).unwrap_or_default();
    }
    Ok(headers)
}

impl Elf {
    pub fn parse(data: &[u8]) -> Result<Elf, String> {
        let headers = headers(data)?;
        let bytes_of = |h: &Header| -> Result<&[u8], String> {
            data.get(h.offset..h.offset + h.size)
                .ok_or_else(|| "ELF section outside the file".to_string())
        };
        let mut sections = Vec::new();
        let mut symbols = Vec::new();
        for h in &headers {
            if h.kind == SHT_PROGBITS && h.flags & SHF_ALLOC != 0 && h.size > 0 {
                sections.push(Section {
                    address: h.address,
                    bytes: bytes_of(h)?.to_vec(),
                });
            }
            if h.kind == SHT_SYMTAB {
                let table = bytes_of(h)?;
                let strings = match headers.get(h.link as usize) {
                    Some(s) => bytes_of(s)?,
                    None => &[],
                };
                for entry in table.as_chunks::<16>().0 {
                    let info = entry[12];
                    symbols.push(Symbol {
                        name: string(strings, word(entry, 0)? as usize),
                        value: word(entry, 4)?,
                        size: word(entry, 8)?,
                        kind: info & 0xf,
                        bind: info >> 4,
                        section: half(entry, 14)?,
                    });
                }
            }
        }
        Ok(Elf { sections, symbols })
    }
}

/// R_ARM_ABS32: a word holding a symbol's address plus the word there.
pub const R_ARM_ABS32: u8 = 2;
/// R_ARM_THM_CALL (R_ARM_THM_PC22): a Thumb BL pair.
pub const R_ARM_THM_CALL: u8 = 10;
const SHT_REL: u32 = 9;

/// One relocation of a relocatable object's section: where, of which kind,
/// against which symbol, and whether the object defines that symbol.
pub struct Relocation {
    pub offset: u32,
    pub kind: u8,
    pub symbol: String,
    pub defined: bool,
}

/// A relocatable object's section named `name`: its bytes and the
/// relocations its `SHT_REL` table applies to it. None when it has none.
pub fn relocated_section(data: &[u8], name: &str) -> Result<Option<(Vec<u8>, Vec<Relocation>)>, String> {
    let headers = headers(data)?;
    let Some(index) = headers.iter().position(|h| h.name == name) else { return Ok(None) };
    let bytes_of = |h: &Header| data.get(h.offset..h.offset + h.size).ok_or_else(|| "ELF section outside the file".to_string());
    let bytes = bytes_of(&headers[index])?.to_vec();
    let mut relocations = Vec::new();
    for table in headers.iter().filter(|h| h.kind == SHT_REL && h.info as usize == index) {
        let symbols = headers.get(table.link as usize).ok_or("a relocation table with no symbol table")?;
        let symbol_bytes = bytes_of(symbols)?;
        let strings = headers.get(symbols.link as usize).map(bytes_of).transpose()?.unwrap_or(&[]);
        for entry in bytes_of(table)?.as_chunks::<8>().0 {
            let info = word(entry, 4)?;
            let symbol = symbol_bytes.get((info >> 8) as usize * 16..).ok_or("a relocation past its symbol table")?;
            relocations.push(Relocation {
                offset: word(entry, 0)?,
                kind: (info & 0xff) as u8,
                symbol: string(strings, word(symbol, 0)? as usize),
                defined: half(symbol, 14)? != 0,
            });
        }
    }
    Ok(Some((bytes, relocations)))
}

/// The Thumb BL pair at `at` in `bytes`: where it branches to, as an offset
/// from `at`'s base (`at + 4 + displacement`), or None when it is no BL.
pub fn thumb_bl_target(bytes: &[u8], at: usize) -> Option<u32> {
    let high = u16::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]);
    let low = u16::from_le_bytes([*bytes.get(at + 2)?, *bytes.get(at + 3)?]);
    if high & 0xf800 != 0xf000 || low & 0xf800 != 0xf800 {
        return None;
    }
    let mut displacement = (i32::from(high & 0x7ff) << 12) | (i32::from(low & 0x7ff) << 1);
    if displacement & 0x40_0000 != 0 {
        displacement -= 0x80_0000;
    }
    u32::try_from(at as i64 + 4 + i64::from(displacement)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thumb_bl_pair_decodes_forward_and_back() {
        // bl +0x100 from 0x10, and bl -0x10 from 0x20.
        let mut bytes = vec![0u8; 0x40];
        bytes[0x10..0x14].copy_from_slice(&[0x00, 0xf0, 0x80, 0xf8]);
        bytes[0x20..0x24].copy_from_slice(&[0xff, 0xf7, 0xf8, 0xff]);
        assert_eq!(thumb_bl_target(&bytes, 0x10), Some(0x114));
        assert_eq!(thumb_bl_target(&bytes, 0x20), Some(0x14));
        assert_eq!(thumb_bl_target(&bytes, 0), None);
    }
}
