//! The skeleton gap: the instructions that still differ once registers,
//! stack offsets, pool offsets, branch distances and register lists are set
//! aside. A draft whose skeleton matches has its control flow, types and
//! frame right; what is left is allocation and scheduling.

use regex::Regex;
use std::sync::OnceLock;

/// An instruction row as `normalize` writes it, in its skeleton: registers
/// become `R`, `[sp, #n]` `[sp,#S]`, `[pc, #n]` `[pool]`, a branch distance
/// `L` and a register list `{list}`.
pub(crate) fn skeleton(row: &str) -> String {
    static PATTERNS: OnceLock<[Regex; 6]> = OnceLock::new();
    let [register, stack_slot, stack_offset, pool, branch, list] = PATTERNS.get_or_init(|| {
        [r"\b(r\d+|sl|fp|ip|lr)\b", r"\[sp, #\d+\]", r"sp, #\d+", r"\[pc, #\d+\]", r"([bB][a-z.]*) [+-]\d+", r"\{[^}]*\}"]
            .map(|pattern| Regex::new(pattern).unwrap())
    });
    let row = register.replace_all(row, "R");
    let row = stack_slot.replace_all(&row, "[sp,#S]");
    let row = stack_offset.replace_all(&row, "sp,#S");
    let row = pool.replace_all(&row, "[pool]");
    let row = branch.replace(&row, "${1} L");
    list.replace(&row, "{list}").trim().to_string()
}

pub(crate) fn skeletons(rows: &[String]) -> Vec<String> {
    rows.iter().map(|row| skeleton(row)).collect()
}

/// The listing's skeleton rows the draft lacks, and the draft's the listing
/// lacks.
pub(crate) fn gap(listing: &[String], draft: &[String]) -> (usize, usize) {
    let (listing, draft) = (skeletons(listing), skeletons(draft));
    let (common, _) = super::compare_counts(&listing, &draft);
    (listing.len() - common, draft.len() - common)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_offsets_distances_and_lists_drop_out() {
        for (row, expected) in [
            ("push {r4, r5, r6, lr}", "push {list}"),
            ("ldr r3, [sp, #12]", "ldr R, [sp,#S]"),
            ("add r0, sp, #4", "add R, sp,#S"),
            ("sub sp, #8", "sub sp,#S"),
            ("ldr r2, [pc, #40]", "ldr R, [pool]"),
            ("bne.n -12", "bne.n L"),
            ("b.n +1688", "b.n L"),
            ("mov r8, sl", "mov R, R"),
            ("ldr r3, [r3, #0]", "ldr R, [R, #0]"),
            ("bl", "bl"),
            ("ldr r0, [r1, #4] <t>", "ldr R, [R, #4] <t>"),
        ] {
            assert_eq!(skeleton(row), expected, "{row}");
        }
    }

    #[test]
    fn the_gap_counts_what_differs_in_shape_alone() {
        let rows = |text: &str| text.split(';').map(str::to_string).collect::<Vec<_>>();
        let listing = rows("push {r4, lr};movs r4, r0;ldr r0, [sp, #4];bl;pop {r4, pc}");
        let renamed = rows("push {r5, lr};movs r5, r0;ldr r1, [sp, #8];bl;pop {r5, pc}");
        assert_eq!(gap(&listing, &renamed), (0, 0));
        let reshaped = rows("push {r5, lr};adds r5, r0, #0;ldr r1, [sp, #8];bl;bl;pop {r5, pc}");
        assert_eq!(gap(&listing, &reshaped), (1, 2));
    }
}
