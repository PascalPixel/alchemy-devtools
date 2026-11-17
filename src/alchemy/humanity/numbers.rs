//! Numbers in code that S3 lets stay numbers. S3's "Numbers" keeps a number
//! where it is the quantity itself: a position or size in plain pixels, a
//! duration in frames, a random range, a fraction written by its own composed
//! forms (`FIXED_ONE * 7 / 10`, `INTtoFIXED(3) / 10`, `TRIG_TURN * 9 / 128`), tenths and
//! percentages. `progress --humanity` still counts every such number among
//! all the numbers in code (`magic_code_all`), and here tells apart the ones
//! the source itself shows to be honest, by kind, so that what is left
//! (`magic_code`) is the numbers that want a name.
//!
//! Each kind is read from the tokens around the number, never guessed from
//! its value: the operand it stands in, up to the nearest `,`, `;`, `&&`,
//! `||`, `?`, `:` or assignment, and the names in that operand at expression
//! level (outside calls' arguments and array indices). A number that only
//! looks like one of these (a depth range, a designer's curve, a message
//! offset) stays counted.

use crate::gba::clex::{Kind, Token};

/// The units S3 composes a fraction from (`FIXED_ONE * n / d`), and the calls
/// that compose one (`INTtoFIXED(n) / d`).
const UNITS: &[&str] = &["FIXED_ONE", "TRIG_TURN", "FACING_STEP", "DAMPING_SCALE"];
const UNIT_CALLS: &[&str] = &["toFIXED", "INTtoFIXED", "DEGtoANG"];

/// Words of a name that hold a position or a size in pixels (or cells).
const POSITION_WORDS: &[&str] = &["x", "y", "z", "dx", "dy", "dz", "row", "column", "width", "height", "radius", "distance"];
/// Words of a name that hold a duration or a moment in frames.
const DURATION_WORDS: &[&str] = &["frame", "frames", "timer", "delay", "duration", "countdown", "wait", "ticks", "vblank", "vblanks"];
/// Words of a name that hold a fade's or a shake's level (`fade = -16`,
/// `shake = -128`): the brightness or the sway it is set to.
const LEVEL_WORDS: &[&str] = &["fade", "shake"];
/// Words that make a name something else whatever its other words say: a
/// rate (`speedX`), a scale (`scaleX`) or an angle (`angleY`).
const OTHER_WORDS: &[&str] =
    &["velocity", "speed", "step", "freq", "frequency", "acceleration", "scale", "angle", "rotation", "zoom", "shift", "flip"];

/// Arithmetic helpers that compute with a random roll they are passed:
/// `Math_RemainderUnsigned(Random_Next(), 90)`.
const ARITHMETIC: &[&str] = &["Remainder", "Divide", "Modulo", "Mul", "Div", "Mod"];

/// A name's words, lowercase, its digits dropped: `transitionFrames` is
/// `transition` and `frames`, `gAltinPeak_FlickerTimer` ends in `timer`.
pub fn words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut previous = '_';
    for c in name.chars() {
        if c == '_' || c.is_ascii_digit() || (c.is_ascii_uppercase() && previous.is_ascii_lowercase()) {
            words.push(std::mem::take(&mut word));
        }
        if c.is_ascii_alphabetic() {
            word.push(c.to_ascii_lowercase());
        }
        previous = c;
    }
    words.push(word);
    words.retain(|word| !word.is_empty());
    words
}

/// What a name (or a member chain's names) says its value is: a `position`,
/// a `duration` or a fade's or shake's `level`, none where any word makes it a
/// rate, scale or angle (`fadeStep`).
pub fn named_kind<'n>(names: impl IntoIterator<Item = &'n str>) -> Option<&'static str> {
    let (mut position, mut duration, mut level) = (false, false, false);
    for name in names {
        let words = words(name);
        if words.iter().any(|word| OTHER_WORDS.contains(&word.as_str())) {
            return None;
        }
        position |= words.iter().any(|word| POSITION_WORDS.contains(&word.as_str()));
        duration |= words.iter().any(|word| DURATION_WORDS.contains(&word.as_str()));
        level |= words.iter().any(|word| LEVEL_WORDS.contains(&word.as_str()));
    }
    if position {
        Some("position")
    } else if duration {
        Some("duration")
    } else if level {
        Some("level")
    } else {
        None
    }
}

/// Whether a name holds a rate, scale or angle, whatever else it says.
pub fn is_other(name: &str) -> bool {
    words(name).iter().any(|word| OTHER_WORDS.contains(&word.as_str()))
}

/// A decimal literal (not hex, not octal): S3 writes positions and durations
/// in decimal, so a hex one (`0x80000`) still wants `INTtoFIXED()` or a name.
pub fn is_decimal(text: &str) -> bool {
    let digits = text.trim_end_matches(['u', 'U', 'l', 'L']);
    digits.bytes().all(|b| b.is_ascii_digit()) && (digits == "0" || !digits.starts_with('0'))
}

/// 10, 100, 1000 and so on.
fn is_power_of_ten(text: &str) -> bool {
    let digits = text.trim_end_matches(['u', 'U', 'l', 'L']);
    digits.len() > 1 && digits.starts_with('1') && digits[1..].bytes().all(|b| b == b'0')
}

const ASSIGNMENTS: &[&str] = &["=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>="];

/// Tokens that end the operand a number stands in.
fn is_stop(text: &str) -> bool {
    matches!(text, "," | ";" | "{" | "}" | "?" | ":" | "&&" | "||" | "return" | "case" | "if" | "while" | "for" | "switch")
        || ASSIGNMENTS.contains(&text)
}

/// The bounds of the operand around `at` (both ends inclusive), and the
/// token that stopped it on the left, if any: `frame == 140` in
/// `if (frame == 140 && x)`, `(Random16() & 127) + 127` after `speed =`.
fn span(tokens: &[Token], at: usize, floor: usize) -> (usize, usize, Option<usize>) {
    let pp = tokens[at].pp;
    let (mut start, mut left, mut depth) = (at, None, 0);
    while start > floor {
        let token = &tokens[start - 1];
        if token.pp != pp || matches!(token.kind, Kind::PpStart | Kind::PpEnd) {
            break;
        }
        match token.text {
            ")" | "]" => depth += 1,
            "(" | "[" if depth == 0 => {
                left = Some(start - 1);
                break;
            }
            "(" | "[" => depth -= 1,
            "{" | "}" | ";" => {
                left = Some(start - 1);
                break;
            }
            text if depth == 0 && is_stop(text) => {
                left = Some(start - 1);
                break;
            }
            _ => {}
        }
        start -= 1;
    }
    let (mut end, mut depth) = (at, 0);
    while end + 1 < tokens.len() {
        let token = &tokens[end + 1];
        if token.pp != pp || matches!(token.kind, Kind::PpStart | Kind::PpEnd) {
            break;
        }
        match token.text {
            "(" | "[" => depth += 1,
            ")" | "]" if depth == 0 => break,
            ")" | "]" => depth -= 1,
            "{" | "}" | ";" => break,
            text if depth == 0 && is_stop(text) => break,
            _ => {}
        }
        end += 1;
    }
    (start, end, left)
}

/// The member chains (`leader->x.part.pixel`, `shaken[2].y`,
/// `gFieldWork->transitionFrames`) between `start` and `end` at expression
/// level: outside a call's arguments and an array's index, so that
/// `Map_GetBehavior(x, z) == 13` says nothing of 13. A call's own name is not
/// one. Also whether a name holding `random` is there, at any level but an
/// index (`gTable[Random16() & 7] + 40` picks a row at random).
fn chains<'a>(tokens: &[Token<'a>], start: usize, end: usize) -> (Vec<Vec<&'a str>>, bool) {
    #[derive(PartialEq)]
    enum Open {
        Call,
        Index,
        Group,
    }
    let mut chains: Vec<Vec<&'a str>> = Vec::new();
    let mut random = false;
    let mut open: Vec<Open> = Vec::new();
    // The last chain may go on (`.`, `->` or an index next), and the next
    // name goes on it.
    let (mut chainable, mut joined) = (false, false);
    for at in start..=end {
        let token = &tokens[at];
        let opaque = open.iter().any(|open| *open != Open::Group);
        let in_index = open.contains(&Open::Index);
        match token.text {
            "(" => open.push(if at > start && tokens[at - 1].kind == Kind::Ident { Open::Call } else { Open::Group }),
            "[" => open.push(Open::Index),
            ")" | "]" => {
                open.pop();
            }
            _ => {}
        }
        if token.kind == Kind::Ident && !in_index && words(token.text).iter().any(|word| word == "random") {
            random = true;
        }
        if opaque {
            continue;
        }
        let call = tokens.get(at + 1).is_some_and(|t| t.text == "(");
        match token.text {
            // An index keeps the chain going: `shaken[2].y`.
            "[" if chainable => continue,
            "]" => continue,
            "." | "->" if chainable => {
                joined = true;
                continue;
            }
            _ => {}
        }
        if token.kind == Kind::Ident && !call {
            match chains.last_mut() {
                Some(chain) if joined => chain.push(token.text),
                _ => chains.push(vec![token.text]),
            }
            chainable = true;
        } else {
            chainable = false;
        }
        joined = false;
    }
    (chains, random)
}

/// The multiplicative chain the number at `at` is a factor of, as
/// `(operator before, operand)` pairs, the number's own included:
/// `hpMax * 60 / 100` is `[("", "hpMax"), ("*", "60"), ("/", "100")]`. A
/// call or group is one operand, named by its call (`px`) or `()`.
fn factors<'a>(tokens: &[Token<'a>], at: usize, floor: usize) -> Vec<(&'a str, &'a str)> {
    let pp = tokens[at].pp;
    let same = |index: usize| tokens[index].pp == pp && !matches!(tokens[index].kind, Kind::PpStart | Kind::PpEnd);
    // The operand ending at `end`: its first token and its name.
    let operand_before = |end: usize| -> Option<(usize, &'a str)> {
        let mut first = end;
        let mut name = tokens[end].text;
        if matches!(tokens[end].text, ")" | "]") {
            let mut depth = 0;
            loop {
                if !same(first) {
                    return None;
                }
                match tokens[first].text {
                    ")" | "]" => depth += 1,
                    "(" | "[" => depth -= 1,
                    _ => {}
                }
                if depth == 0 {
                    break;
                }
                first = first.checked_sub(1).filter(|&f| f >= floor)?;
            }
            name = "()";
            if first > floor && tokens[first - 1].kind == Kind::Ident {
                first -= 1;
                name = tokens[first].text;
            }
        } else if !matches!(tokens[end].kind, Kind::Ident | Kind::Number) {
            return None;
        }
        // A member chain: `unit->hpMax`.
        while first >= floor + 2 && matches!(tokens[first - 1].text, "." | "->") && tokens[first - 2].kind == Kind::Ident {
            first -= 2;
        }
        Some((first, name))
    };
    let operand_after = |begin: usize| -> Option<(usize, &'a str)> {
        let token = tokens.get(begin)?;
        if !same(begin) {
            return None;
        }
        let mut last = begin;
        let mut name = token.text;
        if token.text == "(" || (token.kind == Kind::Ident && tokens.get(begin + 1).is_some_and(|t| t.text == "(")) {
            let open = if token.text == "(" { begin } else { begin + 1 };
            name = if token.text == "(" { "()" } else { token.text };
            let mut depth = 0;
            last = open;
            loop {
                match tokens.get(last)?.text {
                    "(" | "[" => depth += 1,
                    ")" | "]" => depth -= 1,
                    _ => {}
                }
                if depth == 0 {
                    break;
                }
                last += 1;
            }
        } else if !matches!(token.kind, Kind::Ident | Kind::Number) {
            return None;
        }
        while tokens.get(last + 1).is_some_and(|t| matches!(t.text, "." | "->"))
            && tokens.get(last + 2).is_some_and(|t| t.kind == Kind::Ident)
        {
            last += 2;
        }
        Some((last, name))
    };
    let mut out = vec![("", tokens[at].text)];
    // Leftwards: `operand op` pairs.
    let mut first = at;
    while first >= floor + 2 && same(first - 1) && matches!(tokens[first - 1].text, "*" | "/" | "%") {
        let op = tokens[first - 1].text;
        let Some((begin, name)) = operand_before(first - 2) else { break };
        out[0].0 = op;
        out.insert(0, ("", name));
        first = begin;
    }
    let mut last = at;
    while tokens.get(last + 1).is_some_and(|t| matches!(t.text, "*" | "/" | "%")) && same(last + 1) {
        let op = tokens[last + 1].text;
        let Some((end, name)) = operand_after(last + 2) else { break };
        out.push((op, name));
        last = end;
    }
    out
}

/// The statement around `at`: from after the last `;`, `{` or `}` to the next.
fn statement(tokens: &[Token], at: usize, floor: usize) -> (usize, usize) {
    let pp = tokens[at].pp;
    let mut start = at;
    while start > floor && tokens[start - 1].pp == pp && !matches!(tokens[start - 1].text, ";" | "{" | "}") {
        start -= 1;
    }
    let mut end = at;
    while end + 1 < tokens.len() && tokens[end + 1].pp == pp && !matches!(tokens[end + 1].text, ";" | "{" | "}") {
        end += 1;
    }
    (start, end)
}

/// The subject of the `switch` a `case` at `at` belongs to: its bounds.
fn switch_subject(tokens: &[Token], at: usize, floor: usize) -> Option<(usize, usize)> {
    let mut depth = 0;
    let mut index = at;
    while index > floor {
        index -= 1;
        match tokens[index].text {
            "}" => depth += 1,
            "{" if depth > 0 => depth -= 1,
            "{" => {
                // `switch (subject) {`.
                let close = index.checked_sub(1).filter(|&c| tokens[c].text == ")")?;
                let mut open = close;
                let mut parens = 0;
                loop {
                    match tokens[open].text {
                        ")" => parens += 1,
                        "(" => parens -= 1,
                        _ => {}
                    }
                    if parens == 0 {
                        break;
                    }
                    open = open.checked_sub(1).filter(|&o| o >= floor)?;
                }
                let keyword = open.checked_sub(1).map(|k| tokens[k].text);
                return (keyword == Some("switch") && open + 1 < close).then_some((open + 1, close - 1));
            }
            _ => {}
        }
    }
    None
}

/// The honest kind the tokens around the number at `at` show, reading back
/// no further than `floor` and on no further than `limit` (the end of the
/// routine it is in): a composed form, a decimal fraction, a random range,
/// a decimal position or duration by the names it is reckoned with, or the
/// frames a loop counts. The parameter a number fills is judged by the
/// caller, who knows every declaration.
pub fn local(tokens: &[Token], at: usize, floor: usize, limit: usize) -> Option<&'static str> {
    let text = tokens[at].text;
    let factors = factors(tokens, at, floor);
    if factors.iter().any(|(_, operand)| UNITS.contains(operand) || UNIT_CALLS.contains(operand)) {
        return Some("composed");
    }
    let (statement_start, statement_end) = statement(tokens, at, floor);
    let digits = tokens[statement_start..=statement_end].iter().any(|t| t.kind == Kind::Char && t.text == "'0'");
    let tenths = factors.iter().any(|(op, operand)| matches!(*op, "/" | "%") && is_power_of_ten(operand));
    let base = digits && factors.iter().any(|(op, operand)| *op == "*" && is_power_of_ten(operand));
    if tenths || base || (digits && is_power_of_ten(text) && factors.len() == 1) {
        return Some("decimal");
    }
    let (start, end, left) = span(tokens, at, floor);
    let (mut found, random) = chains(tokens, start, end);
    if random {
        return Some("random");
    }
    // A shift's amount is bits, never a position or a duration.
    let shift = at > floor && matches!(tokens[at - 1].text, "<<" | ">>");
    if !is_decimal(text) || shift {
        return None;
    }
    if left.is_some_and(|left| tokens[left].text == "case") {
        let (subject_start, subject_end) = switch_subject(tokens, at, floor)?;
        let (subject, _) = chains(tokens, subject_start, subject_end);
        return subject.iter().find_map(|chain| named_kind(chain.iter().copied()));
    }
    // An assignment's target: `actor->timer = 60`, `s32 frames = 180`.
    if let Some(op) = left.filter(|&left| ASSIGNMENTS.contains(&tokens[left].text)) {
        let (target_start, target_end, _) = span(tokens, op.checked_sub(1)?, floor);
        if target_end + 1 == op {
            let (target, _) = chains(tokens, target_start, target_end);
            found.extend(target.into_iter().last());
        }
    }
    let kinds: Vec<&str> = found.iter().filter_map(|chain| named_kind(chain.iter().copied())).collect();
    let named = kinds.iter().find(|&&kind| kind == "position").or(kinds.first()).copied();
    named.or_else(|| frame_count(tokens, at, floor, limit).then_some("duration"))
}

/// The bracket that closes the one opening at `open`, no further than `limit`.
fn close_of(tokens: &[Token], open: usize, limit: usize) -> Option<usize> {
    let mut depth = 0;
    for (index, token) in tokens.iter().enumerate().take(limit.min(tokens.len() - 1) + 1).skip(open) {
        match token.text {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// The bracket that opens the one closing at `close`, no further back than `floor`.
fn open_of(tokens: &[Token], close: usize, floor: usize) -> Option<usize> {
    let mut depth = 0;
    let mut index = close;
    loop {
        match tokens[index].text {
            ")" | "]" | "}" => depth += 1,
            "(" | "[" | "{" => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        if index <= floor {
            return None;
        }
        index -= 1;
    }
}

/// Whether the body between `open` and `close` (exclusive) waits one frame,
/// every pass: a statement of its own, at its own level, that calls a wait
/// with 1 (`wait(1);`, `System_Wait(1);`).
fn waits_a_frame(tokens: &[Token], open: usize, close: usize) -> bool {
    let mut depth = 0;
    for index in open + 1..close {
        match tokens[index].text {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth -= 1,
            _ => {}
        }
        let texts = |offset: usize| tokens.get(index + offset).map_or("", |t| t.text);
        let statement = index == open + 1 || matches!(tokens[index - 1].text, ";" | "{" | "}");
        if depth == 0
            && statement
            && tokens[index].kind == Kind::Ident
            && words(tokens[index].text).last().is_some_and(|word| word == "wait")
            && (texts(1), texts(2), texts(3), texts(4)) == ("(", "1", ")", ";")
        {
            return true;
        }
    }
    false
}

/// The words a loop's own counter is named with: a letter (`i`, `j`), an
/// index or count, or the frames themselves. A counter named for what it
/// steps through (`mosaicSize`, `pose`, `step`) is that, whatever the loop
/// waits.
const COUNTER_WORDS: &[&str] = &["i", "j", "k", "n", "t", "index", "count", "counter", "remaining", "elapsed", "frame", "frames", "time"];

/// Whether the loop with its header between `header_open` and `header_close`
/// and its body between `body_open` and `body_close` counts frames on
/// `counter`: the loop's condition reads the counter (a `for`'s middle
/// clause), the loop moves it on (`i++`, `++i`, `i += 2`), and the body waits
/// a frame each pass. A body that is one statement has `body_open` just
/// before it.
fn counts_frames(tokens: &[Token], counter: &str, header: (usize, usize), body: (usize, usize)) -> bool {
    // The condition: a `for`'s clause between its semicolons.
    let mut condition = (header.0, header.1);
    if header.0 > 0 && tokens[header.0 - 1].text == "for" {
        let mut depth = 0;
        let mut semicolons = Vec::new();
        for index in header.0 + 1..header.1 {
            match tokens[index].text {
                "(" | "[" => depth += 1,
                ")" | "]" => depth -= 1,
                ";" if depth == 0 => semicolons.push(index),
                _ => {}
            }
        }
        let [first, second] = semicolons[..] else { return false };
        condition = (first, second);
    }
    let reads = (condition.0..condition.1).any(|index| tokens[index].text == counter);
    let moves = (header.0..header.1).chain(body.0..body.1).any(|index| {
        let token = &tokens[index];
        token.text == counter
            && token.kind == Kind::Ident
            && (matches!(tokens[index - 1].text, "++" | "--")
                || tokens.get(index + 1).is_some_and(|next| matches!(next.text, "++" | "--" | "+=" | "-=")))
    });
    reads && moves && waits_a_frame(tokens, body.0, body.1)
}

/// Whether the number at `at` counts the frames of a loop that waits one
/// frame each pass, up to `limit`: its bound (`for (i = 0; i < 20; i++) {
/// ...; System_Wait(1); }`, `} while (++i < 45);`), its start (`for (i =
/// 40; i != 0; i--)`), a cue on its counter inside it (`if (i == 10)`) or a
/// period of it (`if (i % 16 == 0)`). The counter is the one name the number
/// is compared with or divides; nothing else may be reckoned with them.
pub fn frame_count(tokens: &[Token], at: usize, floor: usize, limit: usize) -> bool {
    let (start, end, left) = span(tokens, at, floor);
    let (mut counter, mut compared) = (None, false);
    for (index, token) in tokens.iter().enumerate().take(end + 1).skip(start) {
        match token.text {
            _ if index == at => {}
            "<" | "<=" | ">" | ">=" | "==" | "!=" => compared = true,
            "%" if index + 1 == at => compared = true,
            "++" | "--" | "(" | ")" | "0" => {}
            _ if token.kind == Kind::Ident && counter.is_none_or(|counter| counter == token.text) => counter = Some(token.text),
            _ => return false,
        }
    }
    // `for (i = 40; ...)`: the counter's start.
    let start_of = left.filter(|&left| start == at && end == at && tokens[left].text == "=" && left >= floor + 3);
    if let Some(left) = start_of.filter(|&left| tokens[left - 2].text == "(" && tokens[left - 3].text == "for") {
        counter = (tokens[left - 1].kind == Kind::Ident).then_some(tokens[left - 1].text);
        compared = true;
    }
    let (Some(counter), true) = (counter, compared) else { return false };
    if !words(counter).iter().all(|word| COUNTER_WORDS.contains(&word.as_str())) {
        return false;
    }
    // Outwards through the brackets around the number, each loop in turn.
    let mut depth = 0;
    let mut index = start;
    while index > floor {
        index -= 1;
        match tokens[index].text {
            ")" | "]" | "}" => depth += 1,
            "(" | "[" | "{" if depth > 0 => depth -= 1,
            "(" if index > floor && matches!(tokens[index - 1].text, "for" | "while") => {
                let Some(header_close) = close_of(tokens, index, limit) else { return false };
                let body = if index >= floor + 2 && tokens[index - 1].text == "while" && tokens[index - 2].text == "}" {
                    // `do { ... } while (...)`.
                    let Some(open) = open_of(tokens, index - 2, floor) else { return false };
                    (open, index - 2)
                } else if tokens.get(header_close + 1).is_some_and(|t| t.text == "{") {
                    let Some(close) = close_of(tokens, header_close + 1, limit) else { return false };
                    (header_close + 1, close)
                } else {
                    let Some(semicolon) = (header_close + 1..=limit.min(tokens.len() - 1)).find(|&i| tokens[i].text == ";") else { return false };
                    (header_close, semicolon + 1)
                };
                if counts_frames(tokens, counter, (index, header_close), body) {
                    return true;
                }
            }
            "{" if index > floor => {
                let Some(close) = close_of(tokens, index, limit) else { return false };
                let header = if tokens[index - 1].text == "do" {
                    // `do { ... } while (...)`.
                    let open = close + 2;
                    (tokens.get(close + 1).is_some_and(|t| t.text == "while") && tokens.get(open).is_some_and(|t| t.text == "("))
                        .then(|| close_of(tokens, open, limit).map(|header_close| (open, header_close)))
                        .flatten()
                } else if tokens[index - 1].text == ")" {
                    open_of(tokens, index - 1, floor)
                        .filter(|&open| open > floor && matches!(tokens[open - 1].text, "for" | "while"))
                        .map(|open| (open, index - 1))
                } else {
                    None
                };
                if header.is_some_and(|header| counts_frames(tokens, counter, header, (index, close))) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Whether the number at `at` is the size in a local array's declarator,
/// `u8 elements[10];` or `struct Actor *shards[20];`: a count of its own
/// elements. An index into an array (`x = table[10]`) is not one.
pub fn is_array_size(tokens: &[Token], at: usize, floor: usize) -> bool {
    let text = |index: usize| (index >= floor && index < tokens.len()).then(|| tokens[index].text);
    if text(at.wrapping_sub(1)) != Some("[") || text(at + 1) != Some("]") {
        return false;
    }
    // Back over the dimensions before this one: `counts[4][16]`.
    let mut open = at - 1;
    while open >= floor + 3 && tokens[open - 1].text == "]" && tokens[open - 2].kind == Kind::Number && tokens[open - 3].text == "[" {
        open -= 3;
    }
    let Some(name) = open.checked_sub(1).filter(|&name| name >= floor && tokens[name].kind == Kind::Ident) else { return false };
    // Before the name: a type (`u8`, `Actor` of `struct Actor`), perhaps
    // through pointers.
    let mut before = name;
    while before > floor && tokens[before - 1].text == "*" {
        before -= 1;
    }
    let Some(ty) = before.checked_sub(1).filter(|&ty| ty >= floor) else { return false };
    const NOT_TYPES: &[&str] = &["return", "case", "sizeof", "else", "goto", "do"];
    let declared = tokens[ty].kind == Kind::Ident && !NOT_TYPES.contains(&tokens[ty].text);
    // `*p[3]` after an operator is an expression, so a `*` needs a type.
    declared && (before == name || tokens[ty].kind == Kind::Ident)
}

/// Whether a lone argument's callee, or a helper it computes with, rolls a
/// random number: `Random_Below(120)`, `Math_RemainderUnsigned(Random_Next(), 90)`.
pub fn random_call(callee: &str, random_arguments: bool) -> bool {
    words(callee).iter().any(|word| word == "random") || (random_arguments && ARITHMETIC.iter().any(|helper| callee.contains(helper)))
}

/// The arguments of the call whose `(` is at `open`: for each, the name it
/// passes when it is one name, perhaps cast (`(void (*)())Event_Wait`), and
/// whether a random roll is among them.
pub fn arguments(tokens: &[Token], open: usize) -> (Vec<Option<String>>, bool) {
    let mut out = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut depth = 0;
    let mut random = false;
    let name = |current: &[usize]| -> Option<String> {
        let (&last, rest) = current.split_last()?;
        let cast = rest.is_empty() || (tokens[rest[0]].text == "(" && tokens[*rest.last().unwrap()].text == ")");
        (tokens[last].kind == Kind::Ident && cast).then(|| tokens[last].text.to_string())
    };
    for at in open + 1..tokens.len() {
        let text = tokens[at].text;
        match text {
            "(" | "[" => depth += 1,
            ")" | "]" if depth == 0 => {
                out.push(name(&current));
                return (out, random);
            }
            ")" | "]" => depth -= 1,
            "," if depth == 0 => {
                out.push(name(&current));
                current.clear();
                continue;
            }
            ";" | "{" | "}" => break,
            _ => {}
        }
        if tokens[at].kind == Kind::Ident && words(text).iter().any(|word| word == "random") {
            random = true;
        }
        current.push(at);
    }
    (out, random)
}

/// A call through a pointer that the number at `at` is an argument of:
/// `draw[0](canvas, picture, 48, 24, 104, 120)` gives `draw` and 2.
pub fn pointer_call(tokens: &[Token], at: usize, floor: usize) -> Option<(String, usize)> {
    let pp = tokens[at].pp;
    let (mut depth, mut position) = (0, 0);
    let mut index = at;
    while index > floor {
        index -= 1;
        let token = &tokens[index];
        if token.pp != pp {
            return None;
        }
        match token.text {
            ")" | "]" => depth += 1,
            "(" | "[" if depth > 0 => depth -= 1,
            "(" => {
                // `name[...](` or `(*name)(`.
                let before = index.checked_sub(1)?;
                if tokens[before].text == ")" && before >= floor + 3 && tokens[before - 2].text == "*" && tokens[before - 3].text == "(" {
                    let name = &tokens[before - 1];
                    return (name.kind == Kind::Ident).then(|| (name.text.to_string(), position));
                }
                if tokens[before].text != "]" {
                    return None;
                }
                let mut open = before;
                let mut brackets = 0;
                loop {
                    match tokens[open].text {
                        "]" => brackets += 1,
                        "[" => brackets -= 1,
                        _ => {}
                    }
                    if brackets == 0 {
                        break;
                    }
                    open = open.checked_sub(1).filter(|&o| o >= floor)?;
                }
                let name = open.checked_sub(1).map(|n| &tokens[n]).filter(|t| t.kind == Kind::Ident)?;
                return Some((name.text.to_string(), position));
            }
            "," if depth == 0 => position += 1,
            "{" | "}" | ";" | "[" => return None,
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gba::clex;

    /// The kind `local` gives the `n`th number of `code`.
    fn kind(code: &str, n: usize) -> Option<&'static str> {
        let tokens: Vec<Token> = clex::lex(code).into_iter().filter(|t| t.kind != Kind::Comment).collect();
        let at = tokens.iter().enumerate().filter(|(_, t)| t.kind == Kind::Number).nth(n).expect("a number").0;
        local(&tokens, at, 0, tokens.len() - 1)
    }

    #[test]
    fn honest_numbers_by_what_surrounds_them() {
        assert_eq!(kind("x = FIXED_ONE * 31 / 20;", 0), Some("composed"));
        assert_eq!(kind("x = FIXED_ONE * 31 / 20;", 1), Some("composed"));
        assert_eq!(kind("s = 7 * FIXED_ONE / 10;", 0), Some("composed"));
        assert_eq!(kind("F(TRIG_TURN * 9 / 128);", 1), Some("composed"));
        assert_eq!(kind("a = INTtoFIXED(3) / 10;", 1), Some("composed"));
        assert_eq!(kind("hp += unit->hpMax * 60 / 100;", 0), Some("decimal"));
        assert_eq!(kind("hp += unit->hpMax * 60 / 100;", 1), Some("decimal"));
        assert_eq!(kind("v = levels * 97 / 10;", 0), Some("decimal"));
        assert_eq!(kind("hours = (*digits++ - '0') * 10;", 0), Some("decimal"));
        assert_eq!(kind("v = (Random16() & 127) + 127;", 0), Some("random"));
        assert_eq!(kind("v = ((Random16() & 255) - 127) << 12;", 2), Some("random"));
        assert_eq!(kind("if ((Battle_RandomFar() & 255) <= 152 && x == 16) {}", 1), Some("random"));
        assert_eq!(kind("if ((Battle_RandomFar() & 255) <= 152 && x == 16) {}", 2), Some("position"), "the && ends the roll");
        assert_eq!(kind("if (frame == 140) {}", 0), Some("duration"));
        assert_eq!(kind("for (frame = 0; frame < 24; frame++) {}", 1), Some("duration"));
        assert_eq!(kind("actor->timer = 60;", 0), Some("duration"));
        assert_eq!(kind("s32 framesRemaining = 40;", 0), Some("duration"));
        assert_eq!(kind("gFieldWork->transitionFrames = 16;", 0), Some("duration"));
        assert_eq!(kind("while (++gAltinPeak_FlickerTimer <= 100) {}", 0), Some("duration"));
        assert_eq!(kind("x = centerX - 18;", 0), Some("position"));
        assert_eq!(kind("shaken[2].y += 176;", 1), Some("position"));
        assert_eq!(kind("if (column >= 94 && row > 23) {}", 0), Some("position"));
        assert_eq!(kind("u32 *src = buffer + (srcY * 128 + srcX);", 0), Some("position"));
        assert_eq!(kind("switch (frame) { case 25: break; }", 0), Some("duration"));
        // Hex wants INTtoFIXED() or a name; a call's arguments and an index say nothing.
        assert_eq!(kind("leader->x.fixed = 0x80000;", 0), None);
        assert_eq!(kind("if (Map_GetBehavior(x, z) == 13) {}", 0), None);
        assert_eq!(kind("v = gTable[x] + 40;", 0), None);
        assert_eq!(kind("v = gTable[Random16() & 7] + 40;", 1), None);
        assert_eq!(kind("work->speedX = 24;", 0), None, "a rate");
        assert_eq!(kind("object->scaleX = 12;", 0), None, "a scale");
        assert_eq!(kind("if (GameFlag_Get(0x95a)) {}", 0), None);
        assert_eq!(kind("Event_ShowMessage(27, 0);", 0), None);
        assert_eq!(kind("if (rangePattern == 255) {}", 0), None);
        assert_eq!(kind("CLAMP(screen.depth, 160, 799);", 0), None, "a tuned depth range");
        assert_eq!(kind("fade = -16;", 0), Some("level"));
        assert_eq!(kind("work->shake = i * 2 - 128;", 1), Some("level"));
        assert_eq!(kind("work->shakeFrames = 12;", 0), Some("duration"));
        assert_eq!(kind("perspective->fadeStep = 128;", 0), None, "a rate");
    }

    /// The kind `local` gives the number `number` (its `n`th writing) in a
    /// routine's `code`.
    fn kind_of(code: &str, number: &str, n: usize) -> Option<&'static str> {
        let tokens: Vec<Token> = clex::lex(code).into_iter().filter(|t| t.kind != Kind::Comment).collect();
        let at = tokens.iter().enumerate().filter(|(_, t)| t.text == number).nth(n).expect("the number").0;
        local(&tokens, at, 0, tokens.len() - 1)
    }

    #[test]
    fn frames_a_loop_counts() {
        let drop = "void F(void) { for (i = 0; i < 20; i++) { a->y -= 3; System_Wait(1); if (i == 10) Sound(); if (i % 16 == 0) Puff(); } }";
        assert_eq!(kind_of(drop, "20", 0), Some("duration"), "the loop's bound");
        assert_eq!(kind_of(drop, "10", 0), Some("duration"), "a cue on its counter");
        assert_eq!(kind_of(drop, "16", 0), Some("duration"), "a period of it");
        assert_eq!(kind_of("void F(void) { do { wait(1); } while (++i < 45); }", "45", 0), Some("duration"));
        assert_eq!(kind_of("void F(void) { for (i = 40; i != 0; i--) { x++; Event_Wait(1); } }", "40", 0), Some("duration"));
        assert_eq!(kind_of("void F(void) { while (i != 68) { i++; wait(1); } }", "68", 0), Some("duration"));
        assert_eq!(kind_of("void F(void) { for (i = 0; i < 30; i++) wait(1); }", "30", 0), Some("duration"));
        // A cue on the frame loop's counter, inside a loop over particles.
        let nested = "void F(void) { for (i = 0; i < 60; i++) { for (k = 0; k != 64; k++) { if (i == 12) p[k] = 0; } wait(1); } }";
        assert_eq!(kind_of(nested, "12", 0), Some("duration"));
        assert_eq!(kind_of(nested, "64", 0), None, "the particles are counted, not frames");
        assert_eq!(kind_of("void F(void) { for (i = 0; i != 64; i++) { p[i] = 0; } }", "64", 0), None, "no wait");
        assert_eq!(kind_of("void F(void) { for (i = 0; i < 30; i++) { if (x) wait(1); } }", "30", 0), None, "a wait that may not come");
        assert_eq!(kind_of("void F(void) { for (i = 0; i < 30; i++) { wait(2); } }", "30", 0), None, "two frames a pass");
        assert_eq!(kind_of("void F(void) { for (mosaicSize = 0; mosaicSize <= 15; mosaicSize++) { wait(1); } }", "15", 0), None, "a counter named for what it steps through");
        assert_eq!(kind_of("void F(void) { for (angle = 360, i = 21; i >= 0; i--) { angle -= 8; wait(1); } }", "360", 0), None, "not the loop's condition");
        assert_eq!(kind_of("void F(void) { for (i = 0; i < 20; i++) { v = i * 16 + 32; wait(1); } }", "32", 0), None, "reckoned with more than the counter");
        assert_eq!(kind_of("void F(void) { for (i = 0; i < 20; i++) { wait(1); } if (i == 20) G(); }", "20", 1), None, "after the loop");
    }

    #[test]
    fn sizes_of_local_arrays() {
        let size = |code: &str, n: usize| {
            let tokens: Vec<Token> = clex::lex(code).into_iter().filter(|t| t.kind != Kind::Comment).collect();
            let at = tokens.iter().enumerate().filter(|(_, t)| t.kind == Kind::Number).nth(n).expect("a number").0;
            is_array_size(&tokens, at, 0)
        };
        assert!(size("u8 elements[10];", 0));
        assert!(size("struct Actor *shards[20];", 0));
        assert!(size("s8 counts[4][16];", 1));
        assert!(!size("x = table[10];", 0));
        assert!(!size("return list[3];", 0));
        assert!(!size("y = *p[3];", 0));
        assert!(!size("F(a, b[2]);", 0));
    }

    #[test]
    fn calls_through_pointers_and_their_arguments() {
        let tokens: Vec<Token> = clex::lex("draw[0](canvas, picture + 4, 48, 24);").into_iter().collect();
        let at = tokens.iter().position(|t| t.text == "48").unwrap();
        assert_eq!(pointer_call(&tokens, at, 0), Some(("draw".to_string(), 2)));
        let tokens: Vec<Token> = clex::lex("(*draw)(canvas, cells, sway, 64, 96, 52);").into_iter().collect();
        let at = tokens.iter().position(|t| t.text == "96").unwrap();
        assert_eq!(pointer_call(&tokens, at, 0), Some(("draw".to_string(), 4)));
        let tokens: Vec<Token> = clex::lex("Call1((void (*)())Event_Wait, 15); F(Random16(), 9, x + 1)").into_iter().collect();
        assert_eq!(arguments(&tokens, 1), (vec![Some("Event_Wait".to_string()), None], false));
        let open = tokens.iter().position(|t| t.text == "F").unwrap() + 1;
        assert_eq!(arguments(&tokens, open), (vec![None, None, None], true));
        assert!(random_call("Random_Below", false));
        assert!(random_call("Math_RemainderUnsigned", true));
        assert!(!random_call("Actor_SetPosition", true));
        assert_eq!(words("gAltinPeak_FlickerTimer"), ["g", "altin", "peak", "flicker", "timer"]);
    }
}
