//! Where a function starts and ends in a linked image: found by name or by an
//! address inside it, through the GNU ld map and the ELF's symbols.

use crate::gba::elf::Elf;
use crate::gba::ld_map::{preferred, Map, Placed};

/// Where the function at `start` in `section` ends: at the next symbol after
/// it in the map or among `statics`, the section's end, or `size` bytes on
/// when the ELF gives it a size.
pub(crate) fn function_end(section: &Placed, start: u32, statics: &[u32], size: Option<u32>) -> u32 {
    let mut end = section.end();
    for &address in section.symbols.iter().map(|(a, _)| a).chain(statics) {
        if address > start && address < end {
            end = address;
        }
    }
    match size.filter(|&s| s > 0) {
        Some(size) => end.min(start + size),
        None => end,
    }
}

/// The section, start and name of `wanted`: a name the map or the ELF
/// defines, or an address (eight hex digits, `0x` allowed) that stands for
/// the function holding it. `functions` are the ELF's own function symbols,
/// which the map leaves out when they are static.
pub(crate) fn resolve<'m>(
    map: &'m Map,
    elf: Option<&Elf>,
    functions: &[(u32, String)],
    wanted: &str,
) -> Result<(&'m Placed, u32, String), String> {
    if let Some((section, start)) = map.symbol(wanted) {
        return Ok((section, start, wanted.to_string()));
    }
    let in_section = |address: u32| map.at_address(address).ok_or_else(|| format!("{address:#010x} is in no linked section"));
    let from_elf = functions.iter().find(|(_, n)| n == wanted).map(|(a, _)| *a).or_else(|| {
        elf.and_then(|e| e.symbols.iter().find(|s| s.name == wanted && !s.name.starts_with('$')).map(|s| s.value & !1))
    });
    if let Some(start) = from_elf {
        return Ok((in_section(start)?, start, wanted.to_string()));
    }
    let digits = wanted.trim_start_matches("0x");
    let address = (digits.len() == 8)
        .then(|| u32::from_str_radix(digits, 16).ok())
        .flatten()
        .ok_or_else(|| format!("no symbol {wanted} in the map or the ELF"))?;
    let section = in_section(address)?;
    let start = section
        .symbols
        .iter()
        .map(|(a, _)| *a)
        .chain(functions.iter().map(|(a, _)| *a).filter(|&a| a >= section.address))
        .filter(|&a| a <= address)
        .max()
        .unwrap_or(address);
    let name = preferred(&section.symbols, start)
        .map(str::to_string)
        .or_else(|| functions.iter().find(|(a, _)| *a == start).map(|(_, n)| n.clone()))
        .unwrap_or_else(|| format!("{start:#010x}"));
    Ok((section, start, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_stands_for_the_function_that_holds_it() {
        let map = Map::parse(crate::gba::ld_map::tests::MAP);
        let functions = vec![(0x0800_0014, "Input_Static".to_string())];
        let found = |wanted: &str| resolve(&map, None, &functions, wanted).map(|(p, a, n)| (p.path.clone(), a, n));
        let io = "/x/build/src/system/io.o".to_string();
        assert_eq!(found("Input_Poll"), Ok((io.clone(), 0x0800_0018, "Input_Poll".to_string())));
        assert_eq!(found("08000012"), Ok((io.clone(), 0x0800_0010, "Input_Handle".to_string())));
        assert_eq!(found("0x08000016"), Ok((io.clone(), 0x0800_0014, "Input_Static".to_string())));
        assert_eq!(found("Input_Static"), Ok((io, 0x0800_0014, "Input_Static".to_string())));
        assert!(found("Missing").is_err());
    }

    #[test]
    fn a_function_ends_at_the_next_symbol_in_its_own_section() {
        let section = Placed {
            name: ".text".into(),
            path: "/x/build/src/A.o".into(),
            address: 0x0800_0100,
            size: 0x40,
            load: 0x0800_0100,
            loaded: true,
            symbols: vec![(0x0800_0100, "First".into()), (0x0800_0120, "Second".into())],
            output: None,
        };
        assert_eq!(function_end(&section, 0x0800_0100, &[], None), 0x0800_0120);
        assert_eq!(function_end(&section, 0x0800_0120, &[0x0800_0200], None), 0x0800_0140);
        assert_eq!(function_end(&section, 0x0800_0100, &[0x0800_0110], None), 0x0800_0110);
        assert_eq!(function_end(&section, 0x0800_0100, &[], Some(0x18)), 0x0800_0118);
    }
}
