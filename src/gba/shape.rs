//! A function's shape, and the functions of another build most like it. Each
//! Thumb instruction is read as its mnemonic with what of its operands says
//! most about the code around it: an immediate, or that it addresses the
//! stack, the literal pool or pc. Registers and branch targets are set aside,
//! so two compilations of the same C that allocate or place it differently
//! read alike. Two functions are as alike as the Dice coefficient of their
//! sets of five-instruction runs: twice the runs they share over the runs
//! each has.

use super::bounds::function_end;
use super::elf::{Elf, STT_ARM_TFUNC};
use super::ld_map::{is_placeholder, Map, Placed};
use super::thumb::{thumb_lines, Part};
use std::collections::{HashMap, HashSet};

/// The instructions in one run.
pub(crate) const GRAM: usize = 5;

/// What of an instruction's operands a shape keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Operand {
    /// Nothing kept: registers, a register list or a branch target.
    Plain,
    Immediate(u32),
    /// sp, as a base, a destination or a source.
    Stack,
    /// The literal pool, through `ldr rN, [pc, #imm]`.
    Pool,
    /// pc as an operand: `add rN, pc, #imm`, `mov pc, rN`.
    Pc,
}

/// One instruction as a shape reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Instruction {
    pub mnemonic: &'static str,
    pub operand: Operand,
}

impl std::fmt::Display for Instruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.mnemonic)?;
        match self.operand {
            Operand::Plain => Ok(()),
            Operand::Immediate(value) => write!(f, " #{value}"),
            Operand::Stack => f.write_str(" sp"),
            Operand::Pool => f.write_str(" pool"),
            Operand::Pc => f.write_str(" pc"),
        }
    }
}

const ALU: [&str; 16] = ["and", "eor", "lsl", "lsr", "asr", "adc", "sbc", "ror", "tst", "neg", "cmp", "cmn", "orr", "mul", "bic", "mvn"];
const CONDITIONS: [&str; 14] = ["beq", "bne", "bcs", "bcc", "bmi", "bpl", "bvs", "bvc", "bhi", "bls", "bge", "blt", "bgt", "ble"];
const UNDEFINED: Instruction = Instruction { mnemonic: "undefined", operand: Operand::Plain };

/// A Thumb halfword as the ARM7TDMI runs it (GBATEK's THUMB opcode formats
/// 1 to 19), without the flag-setting `s` unified syntax writes; a `bl`
/// half stands for the call. None for padding: `movs r0, r0` (0x0000) and
/// `nop` (0x46c0), which only align what follows.
pub(crate) fn instruction(half: u16) -> Option<Instruction> {
    use Operand::{Immediate, Pc, Plain, Pool, Stack};
    let op = |mnemonic, operand| Some(Instruction { mnemonic, operand });
    let immediate = |value: u16| Immediate(u32::from(value));
    let (high, low) = ((half >> 3) & 7, half & 7);
    match half >> 11 {
        _ if half == 0 || half == 0x46c0 => None,
        // `lsls rD, rS, #0` is unified syntax's `movs rD, rS`.
        0 if half >> 6 == 0 => op("mov", Plain),
        kind @ 0..=2 => {
            let shift = (half >> 6) & 31;
            op(["lsl", "lsr", "asr"][usize::from(kind)], immediate(if shift == 0 { 32 } else { shift }))
        }
        3 => {
            let mnemonic = if half & 0x200 != 0 { "sub" } else { "add" };
            op(mnemonic, if half & 0x400 != 0 { immediate((half >> 6) & 7) } else { Plain })
        }
        kind @ 4..=7 => op(["mov", "cmp", "add", "sub"][usize::from(kind & 3)], immediate(half & 0xff)),
        8 if half & 0x400 == 0 => op(ALU[usize::from((half >> 6) & 15)], Plain),
        8 => {
            let destination = low | ((half >> 4) & 8);
            let source = high | ((half >> 3) & 8);
            let operand = match (destination, source) {
                (13, _) | (_, 13) => Stack,
                (15, _) | (_, 15) => Pc,
                _ => Plain,
            };
            match (half >> 8) & 3 {
                3 => op("bx", operand),
                kind => op(["add", "cmp", "mov"][usize::from(kind)], operand),
            }
        }
        9 => op("ldr", Pool),
        10 | 11 => {
            let kind = usize::from((half >> 10) & 3);
            op(if half & 0x200 == 0 { ["str", "strb", "ldr", "ldrb"][kind] } else { ["strh", "ldrsb", "ldrh", "ldrsh"][kind] }, Plain)
        }
        kind @ 12..=15 => {
            let offset = (half >> 6) & 31;
            match kind {
                12 => op("str", immediate(offset * 4)),
                13 => op("ldr", immediate(offset * 4)),
                14 => op("strb", immediate(offset)),
                _ => op("ldrb", immediate(offset)),
            }
        }
        16 | 17 => op(if half & 0x800 == 0 { "strh" } else { "ldrh" }, immediate(((half >> 6) & 31) * 2)),
        18 | 19 => op(if half & 0x800 == 0 { "str" } else { "ldr" }, Stack),
        20 => op("add", Pc),
        21 => op("add", Stack),
        22 | 23 => match (half >> 8) & 15 {
            0 => op(if half & 0x80 == 0 { "add" } else { "sub" }, Stack),
            4 | 5 => op("push", Plain),
            12 | 13 => op("pop", Plain),
            14 => op("bkpt", Plain),
            _ => Some(UNDEFINED),
        },
        24 => op("stmia", Plain),
        25 => op("ldmia", Plain),
        26 | 27 => match (half >> 8) & 15 {
            15 => op("swi", immediate(half & 0xff)),
            14 => Some(UNDEFINED),
            condition => op(CONDITIONS[usize::from(condition)], Plain),
        },
        28 => op("b", Plain),
        30 | 31 => op("bl", Plain),
        _ => Some(UNDEFINED),
    }
}

const CALL: Instruction = Instruction { mnemonic: "bl", operand: Operand::Plain };
const POP: Instruction = Instruction { mnemonic: "pop", operand: Operand::Plain };
const POOL: Instruction = Instruction { mnemonic: "ldr", operand: Operand::Pool };

/// The instructions of the Thumb function at `start..end` of `image`, which
/// begins at `base`, in order: its literal pools and jump tables left out.
/// What one compiler writes one way and another another reads alike:
/// - a call through a register is one `bl`, through a `_call_via_rN` veneer
///   or as `mov lr, rN` and a lone `bl` suffix;
/// - a return through interworking, `pop {rN}; bx rN`, is the `pop {pc}`
///   that returns without it, and after a pop of saved registers the two
///   are one pop, as `pop {r4, pc}` is;
/// - a constant built in its register, `mov rD, #imm` then `lsl rD, rD, #imm`
///   or `add rD, #imm`, is the pool load that reads it from the ROM instead.
pub(crate) fn instructions(image: &[u8], base: u32, start: u32, end: u32) -> Vec<Instruction> {
    let mut out: Vec<Instruction> = Vec::new();
    // The halfword before, when it was a whole instruction, and the register
    // a constant is being built in.
    let mut previous: Option<u16> = None;
    let mut building: Option<u16> = None;
    for line in thumb_lines(image, base, start, end) {
        let at = (line.address - base) as usize;
        let half = u16::from_le_bytes([image[at], image[at + 1]]);
        let single = line.part == Part::Instruction && line.size == 2;
        let extends = single && building.is_some_and(|register| builds_on(register, half));
        match line.part {
            Part::Data => continue,
            Part::Undecodable => out.push(UNDEFINED),
            Part::CallSuffix => {
                // mov lr, rN: GBATEK's format 5 with lr (r14) as its destination.
                if previous.is_some_and(|mov| mov & 0xff87 == 0x4686) {
                    out.pop();
                }
                out.push(CALL);
            }
            _ if !single => out.push(CALL),
            _ if previous.is_some_and(|pop| returns_through(pop, half)) => {
                out.pop();
                if out.last() != Some(&POP) {
                    out.push(POP);
                }
            }
            _ if extends => *out.last_mut().expect("a constant's mov was read") = POOL,
            _ => out.extend(instruction(half)),
        }
        previous = single.then_some(half);
        // mov rD, #imm: GBATEK's format 3, operation 0.
        building = if extends { building } else { (single && half >> 11 == 4).then_some((half >> 8) & 7) };
    }
    out
}

/// Whether `pop` is `pop {rN}` and `half` is `bx rN`.
fn returns_through(pop: u16, half: u16) -> bool {
    let registers = pop & 0xff;
    pop >> 8 == 0xbc && registers.count_ones() == 1 && half == 0x4700 | (registers.trailing_zeros() as u16) << 3
}

/// Whether `half` goes on building a constant in `register`: `lsl rD, rD,
/// #imm` (format 1) or `add rD, #imm` (format 3, operation 2).
fn builds_on(register: u16, half: u16) -> bool {
    let shifts = half >> 11 == 0 && half >> 6 != 0 && (half >> 3) & 7 == register && half & 7 == register;
    shifts || (half >> 11 == 6 && (half >> 8) & 7 == register)
}

/// A Thumb function a linked image defines.
pub(crate) struct Function<'m> {
    pub name: String,
    /// The input section that placed it.
    pub placed: &'m Placed,
    pub instructions: Vec<Instruction>,
}

/// Every Thumb function `elf` defines inside an input section of `map` that
/// `keep` admits, in address order: each `FUNC` symbol with the Thumb bit,
/// or the old assembler's `STT_ARM_TFUNC`. It ends where the ELF's size says,
/// or else at the next symbol. Names sharing an address are one function,
/// under the first name that is not a placeholder.
pub(crate) fn functions<'m>(elf: &Elf, map: &'m Map, keep: impl Fn(&Placed) -> bool) -> Vec<Function<'m>> {
    // A function defined here: neither SHN_UNDEF nor SHN_ABS.
    let defined: Vec<_> = elf.symbols.iter().filter(|symbol| symbol.is_function() && !matches!(symbol.section, 0 | 0xfff1)).collect();
    let mut symbols: Vec<(u32, &str, u32)> = defined
        .iter()
        .filter(|symbol| symbol.kind == STT_ARM_TFUNC || symbol.value & 1 != 0)
        .map(|symbol| (symbol.value & !1, symbol.name.as_str(), symbol.size))
        .collect();
    symbols.sort_by_key(|&(start, name, _)| (start, is_placeholder(name)));
    symbols.dedup_by_key(|&mut (start, _, _)| start);
    // Every function, ARM's too, ends the one before it.
    let starts: Vec<u32> = defined.iter().map(|symbol| symbol.value & !1).collect();
    let mut found = Vec::new();
    for (start, name, size) in symbols {
        let Some(placed) = map.at_address(start).filter(|placed| keep(placed)) else { continue };
        let end = function_end(placed, start, &starts, Some(size));
        let Some(section) = elf.sections.iter().find(|s| s.address <= start && end as usize <= s.address as usize + s.bytes.len()) else {
            continue;
        };
        found.push(Function {
            name: name.to_string(),
            placed,
            instructions: instructions(&section.bytes, section.address, start, end),
        });
    }
    found
}

/// The distinct runs of `instructions`.
fn runs(instructions: &[Instruction]) -> HashSet<&[Instruction]> {
    instructions.windows(GRAM).collect()
}

/// Functions by their runs, for finding the one most like another.
pub(crate) struct Index {
    vocabulary: HashMap<Instruction, u32>,
    grams: HashMap<[u32; GRAM], u32>,
    /// By run: the functions that have it.
    postings: Vec<Vec<u32>>,
    /// By function: how many distinct runs it has.
    sizes: Vec<u32>,
}

impl Index {
    pub fn new<'a>(functions: impl IntoIterator<Item = &'a [Instruction]>) -> Index {
        let mut index = Index { vocabulary: HashMap::new(), grams: HashMap::new(), postings: Vec::new(), sizes: Vec::new() };
        for (number, instructions) in functions.into_iter().enumerate() {
            let mut ids = Vec::with_capacity(instructions.len());
            for instruction in instructions {
                let next = index.vocabulary.len() as u32;
                ids.push(*index.vocabulary.entry(*instruction).or_insert(next));
            }
            let mut size = 0;
            let mut seen = HashSet::new();
            for window in ids.windows(GRAM) {
                let gram: [u32; GRAM] = window.try_into().unwrap();
                if !seen.insert(gram) {
                    continue;
                }
                let next = index.grams.len() as u32;
                let id = *index.grams.entry(gram).or_insert(next);
                if id == next {
                    index.postings.push(Vec::new());
                }
                index.postings[id as usize].push(number as u32);
                size += 1;
            }
            index.sizes.push(size);
        }
        index
    }

    /// The indexed function most like `instructions` among those `admits`
    /// lets through, by its number in the index, and their Dice coefficient;
    /// among equals, the nearest in runs, then the first indexed. None when
    /// no admitted function shares a run with it.
    pub fn nearest(&self, instructions: &[Instruction], admits: impl Fn(usize) -> bool) -> Option<(usize, f64)> {
        let runs = runs(instructions);
        let mut shared: HashMap<u32, u32> = HashMap::new();
        for run in &runs {
            let ids: Option<Vec<u32>> = run.iter().map(|instruction| self.vocabulary.get(instruction).copied()).collect();
            let Some(&gram) = ids.and_then(|ids| self.grams.get(&<[u32; GRAM]>::try_from(ids).unwrap())) else { continue };
            for &function in &self.postings[gram as usize] {
                *shared.entry(function).or_default() += 1;
            }
        }
        let size = runs.len() as u32;
        shared
            .into_iter()
            .filter(|&(function, _)| admits(function as usize))
            .map(|(function, count)| {
                let other = self.sizes[function as usize];
                (function, 2.0 * f64::from(count) / f64::from(size + other), size.abs_diff(other))
            })
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.2.cmp(&a.2)).then(b.0.cmp(&a.0)))
            .map(|(function, dice, _)| (function as usize, dice))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gba::elf::{Section, Symbol};

    fn read(half: u16) -> String {
        instruction(half).map_or("-".into(), |instruction| instruction.to_string())
    }

    #[test]
    fn each_format_reads_as_its_mnemonic_and_kept_operand() {
        for (half, expected) in [
            (0x0000, "-"),
            (0x46c0, "-"),
            (0x0008, "mov"),
            (0x04db, "lsl #19"),
            (0x0fdb, "lsr #31"),
            (0x1001, "asr #32"),
            (0x1888, "add"),
            (0x1c06, "add #0"),
            (0x1f9b, "sub #6"),
            (0x2180, "mov #128"),
            (0x2c04, "cmp #4"),
            (0x33d4, "add #212"),
            (0x3b0c, "sub #12"),
            (0x4008, "and"),
            (0x4248, "neg"),
            (0x4359, "mul"),
            (0x4683, "mov"),
            (0x4685, "mov sp"),
            (0x449f, "add pc"),
            (0x4560, "cmp"),
            (0x4700, "bx"),
            (0x4c0e, "ldr pool"),
            (0x5050, "str"),
            (0x5c50, "ldrb"),
            (0x5e50, "ldrsh"),
            (0x6058, "str #4"),
            (0x6818, "ldr #0"),
            (0x7158, "strb #5"),
            (0x7958, "ldrb #5"),
            (0x8098, "strh #4"),
            (0x8858, "ldrh #2"),
            (0x9200, "str sp"),
            (0x9b03, "ldr sp"),
            (0xa002, "add pc"),
            (0xa801, "add sp"),
            (0xb082, "sub sp"),
            (0xb002, "add sp"),
            (0xb530, "push"),
            (0xbd30, "pop"),
            (0xb100, "undefined"),
            (0xc307, "stmia"),
            (0xc807, "ldmia"),
            (0xd0fe, "beq"),
            (0xdd02, "ble"),
            (0xde00, "undefined"),
            (0xdf05, "swi #5"),
            (0xe7fe, "b"),
            (0xe800, "undefined"),
            (0xf000, "bl"),
            (0xf800, "bl"),
        ] {
            assert_eq!(read(half), expected, "{half:#06x}");
        }
    }

    fn halves(words: &[u16]) -> Vec<u8> {
        words.iter().flat_map(|half| half.to_le_bytes()).collect()
    }

    fn text(instructions: &[Instruction]) -> Vec<String> {
        instructions.iter().map(Instruction::to_string).collect()
    }

    #[test]
    fn pools_and_padding_drop_out_and_a_call_through_a_register_is_one_call() {
        const BASE: u32 = 0x0800_0000;
        // push; ldr r3, pool; mov lr, r3; lone bl suffix; bl pair; pop; pad; pool word.
        let mut image = halves(&[0xb500, 0x4b03, 0x469e, 0xf800, 0xf000, 0xf801, 0xbd00, 0x0000]);
        image.extend(0x0800_1234u32.to_le_bytes());
        let read = instructions(&image, BASE, BASE, BASE + image.len() as u32);
        assert_eq!(text(&read), ["push", "ldr pool", "bl", "bl", "pop"]);
        // The veneer's form: ldr r3, pool; bl _call_via_r3.
        let veneer = halves(&[0x4b01, 0xf000, 0xf801, 0x0000, 0x1234, 0x0800]);
        assert_eq!(text(&instructions(&veneer, BASE, BASE, BASE + 12)), ["ldr pool", "bl"]);
    }

    fn read_all(words: &[u16]) -> Vec<String> {
        let image = halves(words);
        text(&instructions(&image, 0x0800_0000, 0x0800_0000, 0x0800_0000 + image.len() as u32))
    }

    #[test]
    fn a_return_through_interworking_reads_as_pop_pc() {
        // pop {r4}; pop {r0}; bx r0, and pop {r4, pc}.
        assert_eq!(read_all(&[0xb510, 0xbc10, 0xbc01, 0x4700]), ["push", "pop"]);
        assert_eq!(read_all(&[0xb510, 0xbd10]), ["push", "pop"]);
        // pop {r1}; bx r1 alone, and a bx through another register.
        assert_eq!(read_all(&[0x2001, 0xbc02, 0x4708]), ["mov #1", "pop"]);
        assert_eq!(read_all(&[0xbc01, 0x4708]), ["pop", "bx"]);
    }

    #[test]
    fn a_constant_built_in_its_register_reads_as_a_pool_load() {
        // mov r0, #18; lsl r0, r0, #8; add r0, #52, and mov r1, #255; add r1, #10.
        assert_eq!(read_all(&[0x2012, 0x0200, 0x3034]), ["ldr pool"]);
        assert_eq!(read_all(&[0x21ff, 0x310a, 0x2203]), ["ldr pool", "mov #3"]);
        // Another register's add, and a shift of another register.
        assert_eq!(read_all(&[0x2001, 0x3102]), ["mov #1", "add #2"]);
        assert_eq!(read_all(&[0x2001, 0x0208]), ["mov #1", "lsl #8"]);
    }

    fn line(text: &str) -> Vec<Instruction> {
        let known = ALU.iter().chain(&CONDITIONS).chain(&["mov", "add", "sub", "ldr", "str", "push", "pop", "bl", "b"]);
        let known: Vec<&'static str> = known.copied().collect();
        text.split(';')
            .map(|word| {
                let (mnemonic, operand) = word.split_once(' ').unwrap_or((word, ""));
                let mnemonic = *known.iter().find(|known| **known == mnemonic).unwrap();
                let operand = operand.strip_prefix('#').map_or(Operand::Plain, |value| Operand::Immediate(value.parse().unwrap()));
                Instruction { mnemonic, operand }
            })
            .collect()
    }

    #[test]
    fn the_nearest_function_shares_the_most_runs_for_its_size() {
        let functions = [
            line("push;mov #1;ldr #4;add;str #8;bl;pop"),
            line("push;mov #1;ldr #4;add;str #8;bl;cmp;beq;pop"),
            line("push;sub;sub;sub;sub;sub;pop"),
        ];
        let index = Index::new(functions.iter().map(Vec::as_slice));
        let all = |_| true;
        let (found, dice) = index.nearest(&functions[0], all).unwrap();
        assert_eq!((found, dice), (0, 1.0));
        // Three runs, two shared with the first function's three and with the
        // second's five.
        let query = line("push;mov #1;ldr #4;add;str #8;bl;mul");
        assert_eq!(index.nearest(&query, all), Some((0, 4.0 / 6.0)));
        assert_eq!(index.nearest(&query, |function| function != 0), Some((1, 4.0 / 8.0)));
        assert_eq!(index.nearest(&line("pop;pop;pop;pop;pop"), all), None);
        assert_eq!(index.nearest(&line("push;pop"), all), None);
    }

    #[test]
    fn an_images_thumb_functions_come_from_its_symbols_in_the_sections_kept() {
        const MAP: &str = "\
Linker script and memory map

.text           0x02008000       0x30
 .text          0x02008000       0x10 build/src/kept.o
                0x02008000                Kept
 .text          0x02008010       0x20 build/asm/other.o
                0x02008010                Other
";
        let map = Map::parse(MAP);
        let code = halves(&[0xb500, 0x2001, 0x2002, 0xbd00, 0x2003, 0x4770, 0x46c0, 0x0000, 0xb500, 0xbd00]);
        let mut bytes = code.clone();
        bytes.resize(0x30, 0);
        let symbol = |name: &str, value: u32, size: u32, kind: u8, section: u16| Symbol { name: name.into(), value, size, kind, bind: 1, section };
        let elf = Elf {
            sections: vec![Section { address: 0x0200_8000, bytes }],
            symbols: vec![
                symbol("Func_02000000", 0x0200_8001, 0, 2, 1),
                symbol("Kept", 0x0200_8001, 8, 2, 1),
                symbol("sStatic", 0x0200_8009, 0, 2, 1),
                symbol("$d", 0x0200_800c, 0, 0, 1),
                symbol("Arm", 0x0200_800c, 0, 2, 1),
                symbol("Other", 0x0200_8011, 0, 2, 1),
                symbol("Imported", 0x0800_0001, 0, 2, 0xfff1),
            ],
        };
        let found = functions(&elf, &map, |placed| placed.path == "build/src/kept.o");
        let rows: Vec<_> = found.iter().map(|f| (f.name.as_str(), text(&f.instructions))).collect();
        assert_eq!(
            rows,
            [("Kept", vec!["push".to_string(), "mov #1".into(), "mov #2".into(), "pop".into()]), ("sStatic", vec!["mov #3".into(), "bx".into()])]
        );
        assert_eq!(functions(&elf, &map, |_| true).len(), 3);
    }
}
