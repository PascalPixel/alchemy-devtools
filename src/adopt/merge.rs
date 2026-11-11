//! A three-way merge by lines, so a journal undoes its own changes to a file
//! that later adoptions changed elsewhere (a game's linker script takes every
//! merge's edits).

/// The lines of a text, each with its newline.
fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// The pairs of lines `a` and `b` share, in order: a longest common
/// subsequence, by Myers' O((N+M)D) search.
fn common(a: &[&str], b: &[&str]) -> Vec<(usize, usize)> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = n + m;
    let offset = max + 1;
    let mut v = vec![0isize; 2 * max as usize + 3];
    // Each step's furthest x for k in -d..=d.
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut found = None;
    'search: for d in 0..=max {
        for k in (-d..=d).step_by(2) {
            let down = k == -d || (k != d && v[(offset + k - 1) as usize] < v[(offset + k + 1) as usize]);
            let mut x = if down { v[(offset + k + 1) as usize] } else { v[(offset + k - 1) as usize] + 1 };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[(offset + k) as usize] = x;
            if x >= n && y >= m {
                trace.push(v[(offset - d) as usize..=(offset + d) as usize].to_vec());
                found = Some(d);
                break 'search;
            }
        }
        trace.push(v[(offset - d) as usize..=(offset + d) as usize].to_vec());
    }
    let mut pairs = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (0..=found.unwrap_or(0)).rev() {
        if d == 0 {
            while x > 0 && y > 0 {
                x -= 1;
                y -= 1;
                pairs.push((x as usize, y as usize));
            }
            break;
        }
        let previous = &trace[d as usize - 1];
        let at = |k: isize| previous[(k + d - 1) as usize];
        let k = x - y;
        let down = k == -d || (k != d && at(k - 1) < at(k + 1));
        let previous_k = if down { k + 1 } else { k - 1 };
        let previous_x = at(previous_k);
        let previous_y = previous_x - previous_k;
        let (middle_x, middle_y) = if down { (previous_x, previous_y + 1) } else { (previous_x + 1, previous_y) };
        while x > middle_x && y > middle_y {
            x -= 1;
            y -= 1;
            pairs.push((x as usize, y as usize));
        }
        x = previous_x;
        y = previous_y;
    }
    pairs.reverse();
    pairs
}

/// One change from the base: base lines `start..end` become `lines`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hunk<'a> {
    start: usize,
    end: usize,
    lines: Vec<&'a str>,
}

fn hunks<'a>(base: &[&str], other: &[&'a str]) -> Vec<Hunk<'a>> {
    let mut hunks = Vec::new();
    let (mut i, mut j) = (0, 0);
    for (bi, oj) in common(base, other).into_iter().chain([(base.len(), other.len())]) {
        if bi > i || oj > j {
            hunks.push(Hunk { start: i, end: bi, lines: other[j..oj].to_vec() });
        }
        i = bi + 1;
        j = oj + 1;
    }
    hunks
}

/// `ours` with the change from `base` to `theirs` applied too, or None when
/// the two change the same lines differently (or both insert at one place).
/// Changes that only touch at a boundary both apply, in base order.
pub fn merge(base: &str, ours: &str, theirs: &str) -> Option<String> {
    let (base, ours, theirs) = (lines(base), lines(ours), lines(theirs));
    let mut all: Vec<Hunk> = hunks(&base, &ours);
    for hunk in hunks(&base, &theirs) {
        if all.contains(&hunk) {
            continue;
        }
        let clash = all.iter().any(|other| {
            let overlap = hunk.start.max(other.start) < hunk.end.min(other.end);
            let insertion_inside = |a: &Hunk, b: &Hunk| a.start == a.end && b.start < a.start && a.start < b.end;
            let same_point = hunk.start == hunk.end && other.start == other.end && hunk.start == other.start;
            overlap || insertion_inside(&hunk, other) || insertion_inside(other, &hunk) || same_point
        });
        if clash {
            return None;
        }
        all.push(hunk);
    }
    all.sort_by_key(|hunk| (hunk.start, hunk.end));
    let mut merged = String::new();
    let mut at = 0;
    for hunk in &all {
        merged.extend(base[at..hunk.start].iter().copied());
        merged.extend(hunk.lines.iter().copied());
        at = hunk.end;
    }
    merged.extend(base[at..].iter().copied());
    Some(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_common_lines_are_a_longest_subsequence() {
        let a = lines("a\nb\nc\nd\n");
        let b = lines("a\nx\nc\nd\ny\n");
        assert_eq!(common(&a, &b), [(0, 0), (2, 2), (3, 3)]);
        assert!(common(&[], &b).is_empty());
        assert_eq!(common(&a, &a).len(), 4);
    }

    #[test]
    fn changes_apart_both_apply_and_changes_that_clash_refuse() {
        let base = "1\n2\n3\n4\n5\n6\n";
        // Later work changed 5; the undo puts 2 back.
        assert_eq!(merge(base, "1\n2\n3\n4\nfive\n6\n", "1\ntwo\n3\n4\n5\n6\n").as_deref(), Some("1\ntwo\n3\n4\nfive\n6\n"));
        // Touching changes: a row inserted after the one the undo replaces.
        assert_eq!(merge(base, "1\n2\nnew\n3\n4\n5\n6\n", "1\ntwo\n3\n4\n5\n6\n").as_deref(), Some("1\ntwo\nnew\n3\n4\n5\n6\n"));
        assert_eq!(merge(base, "1\nB\n3\n4\n5\n6\n", "1\nC\n3\n4\n5\n6\n"), None);
        assert_eq!(merge(base, "1\n2\nx\n3\n4\n5\n6\n", "1\n2\ny\n3\n4\n5\n6\n"), None);
        // The same change on both sides is one change.
        assert_eq!(merge(base, "1\n2\n3\n", "1\n2\n3\n").as_deref(), Some("1\n2\n3\n"));
        assert_eq!(merge("a", "a\nb", "c").as_deref(), None);
    }
}
