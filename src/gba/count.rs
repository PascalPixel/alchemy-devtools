//! pret's calcrom over a GNU ld map: every byte an image places, as the input
//! sections that placed it, each with its size and object. Which of them are
//! done is the project's to say, by the object's folder; this module only
//! reads the map.

use super::ld_map::{Map, Output, Placed};

/// The cartridge's window in the GBA's address space (GBATEK): an output
/// section loaded inside it is part of the ROM image.
pub(crate) const CARTRIDGE: std::ops::Range<u32> = 0x0800_0000..0x0a00_0000;

/// Which output sections make up an image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Image {
    /// The cartridge: every output section loaded inside its window.
    Rom,
    /// An image linked on its own, such as an overlay: the output sections
    /// its link script loads into its file, as the project names them.
    Linked(&'static [&'static str]),
}

impl Image {
    fn holds(self, output: &Output) -> bool {
        output.size > 0
            && match self {
                Image::Rom => CARTRIDGE.contains(&output.load),
                Image::Linked(sections) => sections.contains(&output.name.as_str()),
            }
    }
}

/// Every input section the image places that holds bytes, in the map's
/// order. Fill and unnamed input (`*fill*`, `COMMON`) are left to the span.
pub(crate) fn placed(map: &Map, image: Image) -> impl Iterator<Item = &Placed> {
    map.placed.iter().filter(move |placed| {
        placed.size > 0 && placed.name.starts_with('.') && placed.output.is_some_and(|output| image.holds(&map.outputs[output]))
    })
}

/// The image's span, from its first byte to its last.
pub(crate) fn span(map: &Map, image: Image) -> i64 {
    let held = map.outputs.iter().filter(|output| image.holds(output));
    let (first, last) = held.fold((i64::MAX, 0), |(first, last), output| {
        (first.min(i64::from(output.load)), last.max(i64::from(output.load) + i64::from(output.size)))
    });
    (last - first).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: &str = "\
Linker script and memory map

ewram           0x02000000    0x38000
 .bss           0x02000000    0x38000 build/src/ram.o
.rom_start      0x08000000      0x200
 .text          0x08000000      0x100 build/src/main.o
 .rodata        0x08000100       0x30 build/src/main.o
 *fill*         0x08000130       0x10
 .rom.00000140
                0x08000140       0x40 build/asm/rom.o
 .text          0x08000180        0x0 build/src/empty.o
.iwram_code
                0x03000000       0x20 load address 0x08000200
 .text          0x03000000       0x20 build/src/fast.o
";

    #[test]
    fn a_map_places_its_image_by_input_section() {
        let map = Map::parse(MAP);
        // RAM is outside the cartridge, and a wrapped heading still places
        // the code it loads there.
        assert_eq!(span(&map, Image::Rom), 0x220);
        let sections: Vec<_> = placed(&map, Image::Rom).map(|p| (p.name.as_str(), p.size, p.path.as_str())).collect();
        assert_eq!(
            sections,
            [
                (".text", 0x100, "build/src/main.o"),
                (".rodata", 0x30, "build/src/main.o"),
                (".rom.00000140", 0x40, "build/asm/rom.o"),
                (".text", 0x20, "build/src/fast.o"),
            ]
        );
        // An overlay's image is the sections its script loads: here `.text` and `.data`.
        let overlay = Map::parse(
            "\
Linker script and memory map

.text           0x02000000       0x30
 .text          0x02000000       0x30 build/src/scene/vale.o
.data           0x02000030       0x10
 .data          0x02000030       0x10 build/src/scene/vale.o
.bss            0x02000040       0x20
 .bss           0x02000040       0x20 build/src/scene/vale.o
",
        );
        assert_eq!(span(&overlay, Image::Linked(&[".text", ".data"])), 0x40);
        assert_eq!(placed(&overlay, Image::Linked(&[".text", ".data"])).map(|p| p.size).sum::<u32>(), 0x40);
        let nothing = Map::parse("no map");
        assert_eq!((span(&nothing, Image::Rom), placed(&nothing, Image::Rom).count()), (0, 0));
    }
}
