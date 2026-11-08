//! Each pseudo register's allocation, read from GCC 2.9x's per-pass dumps
//! (`-da`). global.c ranks the pseudos that live in more than one block by
//! floor(log2 refs) * refs / live length and gives them hard registers in
//! that order (the greg dump's "regs to allocate"); the lreg dump has each
//! pseudo's references, live length and the calls it lives across, and the
//! greg dump's dispositions the hard register each one got.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Pseudo {
    pub number: u32,
    pub refs: u32,
    pub live: u32,
    pub calls: u32,
    /// A C variable rather than a temporary.
    pub user: bool,
    /// None when the pseudo lives on the stack.
    pub hard: Option<u32>,
}

impl Pseudo {
    pub fn priority(&self) -> f64 {
        let log = if self.refs > 0 { 31 - self.refs.leading_zeros() } else { 0 };
        f64::from(log * self.refs) / f64::from(self.live.max(1))
    }
}

/// Each function's dump: the text from its `;; Function NAME` line to the next.
fn functions(dump: &str) -> Vec<(&str, &str)> {
    let mut found: Vec<(&str, &str)> = Vec::new();
    let mut starts: Vec<(usize, &str)> = Vec::new();
    let mut at = 0;
    for line in dump.split_inclusive('\n') {
        if let Some(name) = line.strip_prefix(";; Function ") {
            starts.push((at, name.split_whitespace().next().unwrap_or("")));
        }
        at += line.len();
    }
    for (index, &(start, name)) in starts.iter().enumerate() {
        let end = starts.get(index + 1).map_or(dump.len(), |&(next, _)| next);
        found.push((name, &dump[start..end]));
    }
    found
}

/// `Register N used X times across Y insns; ...; crosses C calls; ...`.
fn lreg_rows(text: &str) -> BTreeMap<u32, (u32, u32, u32, bool)> {
    let mut rows = BTreeMap::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("Register ") else { continue };
        let words: Vec<&str> = rest.split([' ', ';']).filter(|word| !word.is_empty()).collect();
        let number = |at: usize| words.get(at).and_then(|word| word.parse::<u32>().ok());
        let (Some(pseudo), Some(refs), Some(live)) = (number(0), number(2), number(5)) else { continue };
        if words.get(1) != Some(&"used") {
            continue;
        }
        let calls = words.iter().position(|word| *word == "crosses").and_then(|at| number(at + 1)).unwrap_or(0);
        rows.insert(pseudo, (refs, live, calls, line.contains("user var")));
    }
    rows
}

/// The pseudos `;; N regs to allocate:` names, in order, and the hard
/// register each disposition gives (`N in H`).
fn greg_rows(text: &str) -> (Vec<u32>, BTreeMap<u32, u32>) {
    let order = text
        .lines()
        .find_map(|line| line.split_once("regs to allocate:").map(|(_, list)| list))
        .map(|list| list.split_whitespace().filter_map(|word| word.split('+').next()?.parse().ok()).collect())
        .unwrap_or_default();
    let mut hard = BTreeMap::new();
    let mut lines = text.lines().skip_while(|line| !line.starts_with(";; Register dispositions:")).skip(1);
    for line in lines.by_ref().take_while(|line| !line.starts_with(";;") && !line.trim().is_empty()) {
        let words: Vec<&str> = line.split_whitespace().collect();
        for triple in words.windows(3).filter(|triple| triple[1] == "in") {
            if let (Ok(pseudo), Ok(register)) = (triple[0].parse(), triple[2].parse()) {
                hard.insert(pseudo, register);
            }
        }
    }
    (order, hard)
}

/// Each function's pseudos in global.c's order, or only `only`'s.
pub(crate) fn read(lreg: &str, greg: &str, only: Option<&str>) -> Vec<(String, Vec<Pseudo>)> {
    let lregs: BTreeMap<&str, &str> = functions(lreg).into_iter().collect();
    functions(greg)
        .into_iter()
        .filter(|(name, _)| only.is_none_or(|only| only == *name))
        .map(|(name, text)| {
            let (order, hard) = greg_rows(text);
            let rows = lreg_rows(lregs.get(name).copied().unwrap_or(""));
            let pseudos = order
                .into_iter()
                .map(|number| {
                    let (refs, live, calls, user) = rows.get(&number).copied().unwrap_or_default();
                    Pseudo { number, refs, live, calls, user, hard: hard.get(&number).copied() }
                })
                .collect();
            (name.to_string(), pseudos)
        })
        .collect()
}

/// A hard register as objdump names it.
pub(crate) fn register(number: u32) -> String {
    match number {
        10 => "sl".into(),
        11 => "fp".into(),
        12 => "ip".into(),
        13 => "sp".into(),
        14 => "lr".into(),
        15 => "pc".into(),
        n => format!("r{n}"),
    }
}

pub(crate) fn print(functions: &[(String, Vec<Pseudo>)]) {
    for (name, pseudos) in functions {
        println!("allocation: {name}, {} pseudos in global.c's order", pseudos.len());
        println!("{:>7} {:>5} {:>6} {:>9} {:>6}  hard", "pseudo", "refs", "live", "priority", "calls");
        for pseudo in pseudos {
            let hard = pseudo.hard.map_or("stack".to_string(), register);
            let user = if pseudo.user { "  user variable" } else { "" };
            println!("{:>7} {:>5} {:>6} {:>9.3} {:>6}  {hard}{user}", pseudo.number, pseudo.refs, pseudo.live, pseudo.priority(), pseudo.calls);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LREG: &str = "\n;; Function First\n\n  Register 32 costs: NO_REGS:0 LO_REGS:0\nRegister 32 used 136 times across 5992 insns; set 1 time; user var; dies in 5 places; crosses 536 calls; pref LO_REGS; pointer.\n\nRegister 40 used 5 times across 10 insns; set 1 time; dies in 2 places; pref LO_REGS.\n\nRegister 41 used 8 times across 5 insns in block 2; set 1 time; pref LO_REGS.\n\n;; Function Second\n\nRegister 32 used 2 times across 4 insns; set 1 time; crosses 1 call; pref LO_REGS.\n";
    const GREG: &str = "\n;; Function First\n\n;; 2 regs to allocate: 40 32\n;; 32 conflicts: 32 40\n\nSpilling for insn 17.\n;; Register dispositions:\n32 in 4  41 in 3  \n\n;; Hard regs used:  0 1 2 3 4 13 14\n\n;; Function Second\n\n;; 1 regs to allocate: 32+33\n;; Register dispositions:\n32 in 11  \n";

    #[test]
    fn pseudos_come_in_allocation_order_with_their_registers() {
        let found = read(LREG, GREG, None);
        assert_eq!(found.len(), 2);
        let (name, first) = &found[0];
        assert_eq!(name, "First");
        assert_eq!(first, &[
            Pseudo { number: 40, refs: 5, live: 10, calls: 0, user: false, hard: None },
            Pseudo { number: 32, refs: 136, live: 5992, calls: 536, user: true, hard: Some(4) },
        ]);
        assert!((first[0].priority() - 1.0).abs() < 1e-9);
        assert!((first[1].priority() - 7.0 * 136.0 / 5992.0).abs() < 1e-9);
        assert_eq!(read(LREG, GREG, Some("Second"))[0].1, [Pseudo { number: 32, refs: 2, live: 4, calls: 1, user: false, hard: Some(11) }]);
        assert_eq!(register(11), "fp");
    }
}
