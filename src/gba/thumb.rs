//! Thumb code as the ARM7TDMI reads it, as much as a comparison needs: where
//! each instruction starts and how long it is, which words are literal pools
//! and jump tables, where a lone call suffix stands, and which halfwords the
//! ARMv4T cannot run.

use std::collections::BTreeSet;

/// The displacement of a two-halfword `bl`, or None when `pair` is not one.
pub fn bl_displacement(pair: &[u8]) -> Option<i32> {
    let high = u16::from_le_bytes(pair.get(..2)?.try_into().ok()?);
    let low = u16::from_le_bytes(pair.get(2..4)?.try_into().ok()?);
    if high & 0xf800 != 0xf000 || low & 0xf800 != 0xf800 {
        return None;
    }
    let value = (i32::from(high & 0x7ff) << 12) | (i32::from(low & 0x7ff) << 1);
    Some((value << 9) >> 9)
}

fn half_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*bytes.get(offset)?, *bytes.get(offset + 1)?]))
}

fn word_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
}

/// What one row of a function's bytes is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    /// An instruction: two bytes, or four for a `bl` pair.
    Instruction,
    /// A literal pool word or a jump table entry.
    Data,
    /// A `bl` suffix with no prefix before it: a call to the address in lr.
    CallSuffix,
    /// A halfword the ARMv4T cannot run, or an instruction the end cuts off.
    Undecodable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Line {
    pub address: u32,
    pub size: u32,
    pub part: Part,
}

/// The pool address a pc-relative load at `pc` reads.
fn pool_address(pc: u32, half: u16) -> u32 {
    (pc & !3) + 4 + u32::from(half & 0xff) * 4
}

/// A halfword's part and size; for a pc-relative load, the word it reads.
fn classify(image: &[u8], base: u32, pc: u32, end: u32, half: u16) -> (Part, u32, Option<u32>) {
    let offset = (pc - base) as usize;
    match half >> 11 {
        // ldr rN, [pc, #imm]: its word must be in the image.
        0x09 => match word_at(image, (pool_address(pc, half) - base) as usize) {
            Some(word) => (Part::Instruction, 2, Some(word)),
            None => (Part::Undecodable, 2, None),
        },
        // A bl prefix runs only as a whole pair before the end.
        0x1e if pc + 4 <= end && image.get(offset..offset + 4).and_then(bl_displacement).is_some() => {
            (Part::Instruction, 4, None)
        }
        0x1e | 0x1d => (Part::Undecodable, 2, None),
        0x1f => (Part::CallSuffix, 2, None),
        _ => match half >> 8 {
            // Undefined, and the 0xb000 group's later forms; swi (0xdf),
            // bkpt (0xbe) and nop (0xbf00) are the ARMv4T's.
            0xde | 0xb1 | 0xb2 | 0xb3 | 0xb6 | 0xb7 | 0xb8 | 0xb9 | 0xba | 0xbb => (Part::Undecodable, 2, None),
            0xbf if half != 0xbf00 => (Part::Undecodable, 2, None),
            _ => (Part::Instruction, 2, None),
        },
    }
}

/// The rows of the Thumb function at `start..end` of `image`, which begins at
/// `base`. A pc-relative load's word is data where it falls inside the
/// function, and so is a word-aligned table a loaded word points at, up to
/// the first entry the table names.
pub(crate) fn thumb_lines(image: &[u8], base: u32, start: u32, end: u32) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut pool = BTreeSet::new();
    let mut tables = BTreeSet::new();
    let at = |address: u32| address.wrapping_sub(base) as usize;
    let mut pc = start;
    while pc < end {
        if tables.remove(&pc) {
            let mut lowest = end;
            while pc + 4 <= end && pc < lowest {
                let Some(word) = word_at(image, at(pc)) else {
                    break;
                };
                if word <= pc || word >= end {
                    break;
                }
                lowest = lowest.min(word & !1);
                lines.push(Line { address: pc, size: 4, part: Part::Data });
                pc += 4;
            }
            continue;
        }
        if pool.contains(&pc) && pc + 4 <= end && word_at(image, at(pc)).is_some() {
            lines.push(Line { address: pc, size: 4, part: Part::Data });
            pc += 4;
            continue;
        }
        let Some(half) = half_at(image, at(pc)) else {
            break;
        };
        let (part, size, loaded) = classify(image, base, pc, end, half);
        if let Some(word) = loaded {
            let target = pool_address(pc, half);
            if target < end {
                pool.insert(target);
                if word > pc && word < end && word & 3 == 0 {
                    tables.insert(word);
                }
            }
        }
        lines.push(Line { address: pc, size, part });
        pc += size;
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u32 = 0x0800_0000;

    fn halves(words: &[u16]) -> Vec<u8> {
        words.iter().flat_map(|h| h.to_le_bytes()).collect()
    }

    fn rows(lines: &[Line]) -> Vec<(u32, u32, Part)> {
        lines.iter().map(|line| (line.address - BASE, line.size, line.part)).collect()
    }

    #[test]
    fn a_jump_table_and_its_pool_read_as_words() {
        use Part::*;
        let mut image = halves(&[0x4a05, 0x00bb, 0x589b, 0x469f]);
        image.extend((BASE + 0x10).to_le_bytes());
        image.extend((BASE + 0x14).to_le_bytes());
        image.extend(halves(&[0x2001, 0x4770, 0x2002, 0x4770]));
        image.extend((BASE + 8).to_le_bytes());
        assert_eq!(rows(&thumb_lines(&image, BASE, BASE, BASE + 0x1c)), [
            (0, 2, Instruction), (2, 2, Instruction), (4, 2, Instruction), (6, 2, Instruction),
            (8, 4, Data), (0xc, 4, Data),
            (0x10, 2, Instruction), (0x12, 2, Instruction), (0x14, 2, Instruction), (0x16, 2, Instruction),
            (0x18, 4, Data),
        ]);
    }

    #[test]
    fn calls_suffixes_and_what_the_cpu_cannot_run() {
        use Part::*;
        // A bl pair, mov lr then a lone suffix, swi and bkpt.
        let image = halves(&[0xf000, 0xf801, 0x469e, 0xf800, 0xdf01, 0xbe00]);
        assert_eq!(rows(&thumb_lines(&image, BASE, BASE, BASE + 12)),
            [(0, 4, Instruction), (4, 2, Instruction), (6, 2, CallSuffix), (8, 2, Instruction), (10, 2, Instruction)]);
        // A blx suffix, an undefined halfword, a later form and a hint.
        for half in [0xe800, 0xde00, 0xb100, 0xbf10] {
            assert_eq!(rows(&thumb_lines(&halves(&[half]), BASE, BASE, BASE + 2)), [(0, 2, Undecodable)], "{half:#06x}");
        }
        assert_eq!(rows(&thumb_lines(&halves(&[0xbf00]), BASE, BASE, BASE + 2)), [(0, 2, Instruction)]);
        // A prefix the end cuts off, and a load whose word lies past the image.
        assert_eq!(rows(&thumb_lines(&halves(&[0xf000, 0xf801]), BASE, BASE, BASE + 2)), [(0, 2, Undecodable)]);
        assert_eq!(rows(&thumb_lines(&halves(&[0x4840]), BASE, BASE, BASE + 2)), [(0, 2, Undecodable)]);
    }

    #[test]
    fn bl_displacements_reach_both_ways() {
        assert_eq!(bl_displacement(&halves(&[0xf000, 0xf801])), Some(2));
        assert_eq!(bl_displacement(&halves(&[0xf7ff, 0xfffe])), Some(-4));
        assert_eq!(bl_displacement(&halves(&[0xf000, 0xe801])), None);
        assert_eq!(bl_displacement(&[0, 0xf0]), None);
    }
}
