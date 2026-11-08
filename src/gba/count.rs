//! pret's calcrom over a GNU ld map: every byte an image places, as the input
//! sections that placed it, each with its size and object. Which of them are
//! done is the project's to say, by the object's folder; this module only
//! reads the map.

/// The cartridge's window in the GBA's address space (GBATEK): an output
/// section loaded inside it is part of the ROM image.
const CARTRIDGE: std::ops::Range<i64> = 0x0800_0000..0x0a00_0000;

/// Which output sections make up an image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Image {
    /// The cartridge: every output section loaded inside its window.
    Rom,
    /// An overlay linked on its own: its `.text` and `.data`.
    Overlay,
}

fn number(text: &str) -> Option<i64> {
    i64::from_str_radix(text.strip_prefix("0x")?, 16).ok()
}

/// Reads one linker map: calls `each` with every input section the image
/// places that holds bytes (its name, size and object, empty for none), and
/// returns the image's span, from its first byte to its last. Fill and
/// unnamed input (`*fill*`) are left to the span.
pub(crate) fn image(map: &str, image: Image, mut each: impl FnMut(&str, i64, &str)) -> i64 {
    let Some(body) = map.split("Linker script and memory map").nth(1) else { return 0 };
    let (mut first, mut last) = (i64::MAX, 0);
    let (mut placed, mut wrapped) = (false, None::<String>);
    // An output section whose name is too long for its column (ld wraps
    // past 14 characters: `.iwram_resident`) has its address, size and load
    // address on the next line.
    let mut heading: Option<String> = None;
    for line in body.lines() {
        let joined;
        let line = match heading.take() {
            Some(name) if line.trim_start().starts_with("0x") => {
                joined = format!("{name} {}", line.trim_start());
                joined.as_str()
            }
            _ => line,
        };
        if !line.starts_with(' ') && !line.is_empty() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() == 1 {
                heading = Some(line.to_owned());
                placed = false;
                continue;
            }
            let (address, size) = (fields.get(1).and_then(|f| number(f)), fields.get(2).and_then(|f| number(f)));
            let load = line.split("load address ").nth(1).and_then(|rest| number(rest.trim()));
            placed = match (address, size) {
                (Some(address), Some(size)) if size > 0 => {
                    let at = load.unwrap_or(address);
                    let inside = match image {
                        Image::Rom => CARTRIDGE.contains(&at),
                        Image::Overlay => matches!(fields[0], ".text" | ".data"),
                    };
                    if inside {
                        first = first.min(at);
                        last = last.max(at + size);
                    }
                    inside
                }
                _ => false,
            };
            continue;
        }
        if !placed {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if let [name] = fields.as_slice() {
            if name.starts_with('.') {
                wrapped = Some((*name).to_owned());
            }
            continue;
        }
        let (name, rest) = match wrapped.take() {
            Some(name) => (name, &fields[..]),
            None if fields.first().is_some_and(|name| name.starts_with('.')) => (fields[0].to_owned(), &fields[1..]),
            None => continue,
        };
        let (Some(_), Some(size)) = (rest.first().and_then(|f| number(f)), rest.get(1).and_then(|f| number(f))) else {
            continue;
        };
        if size > 0 {
            each(&name, size, rest.get(2).copied().unwrap_or(""));
        }
    }
    if last > first {
        last - first
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: &str = "\
Linker script and memory map

ewram           0x02000000    0x38000
 .bss           0x02000000    0x38000 build/obj/src/ram.o
.rom_start      0x08000000      0x200
 .text          0x08000000      0x100 build/obj/src/main.o
 .rodata        0x08000100       0x30 build/obj/src/main.o
 *fill*         0x08000130       0x10
 .rom.00000140
                0x08000140       0x40 build/obj/asm/rom.o
 .text          0x08000180        0x0 build/obj/src/empty.o
.iwram_code
                0x03000000       0x20 load address 0x08000200
 .text          0x03000000       0x20 build/obj/src/fast.o
";

    #[test]
    fn a_map_places_its_image_by_input_section() {
        let mut sections = Vec::new();
        let span = image(MAP, Image::Rom, |name, size, object| sections.push((name.to_owned(), size, object.to_owned())));
        // RAM is outside the cartridge, and a wrapped heading still places
        // the code it loads there.
        assert_eq!(span, 0x220);
        let named = |name: &str, size, object: &str| (name.to_owned(), size, object.to_owned());
        assert_eq!(
            sections,
            [
                named(".text", 0x100, "build/obj/src/main.o"),
                named(".rodata", 0x30, "build/obj/src/main.o"),
                named(".rom.00000140", 0x40, "build/obj/asm/rom.o"),
                named(".text", 0x20, "build/obj/src/fast.o"),
            ]
        );
        // An overlay's image is its own `.text` and `.data`.
        let overlay = "\
Linker script and memory map

.text           0x02000000       0x30
 .text          0x02000000       0x30 build/obj/src/scene/vale.o
.data           0x02000030       0x10
 .data          0x02000030       0x10 build/obj/src/scene/vale.o
.bss            0x02000040       0x20
 .bss           0x02000040       0x20 build/obj/src/scene/vale.o
";
        let mut placed = 0;
        assert_eq!(image(overlay, Image::Overlay, |_, size, _| placed += size), 0x40);
        assert_eq!(placed, 0x40);
        assert_eq!(image("no map", Image::Rom, |_, _, _| unreachable!()), 0);
    }
}
