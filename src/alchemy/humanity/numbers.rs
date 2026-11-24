//! Numbers in code that S3 lets stay numbers. S3's "Numbers" keeps a number
//! where it is the quantity itself: a position or size in plain pixels, a
//! duration in frames, a random range, a fraction written by its own composed
//! forms (`FIXED_ONE * 7 / 10`, `INTtoFIXED(3) / 10`, `TRIG_TURN * 9 / 128`), tenths,
//! percentages and decimal digits; and, among the hex S3 keeps, a mask of
//! whole fields and the sign bit. `progress --humanity` still counts every such number among
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
use std::collections::BTreeSet;

/// The units S3 composes a fraction from (`FIXED_ONE * n / d`), and the calls
/// that compose one (`INTtoFIXED(n) / d`).
const UNITS: &[&str] = &["FIXED_ONE", "TRIG_TURN", "FACING_STEP", "DAMPING_SCALE"];
const UNIT_CALLS: &[&str] = &["toFIXED", "INTtoFIXED", "DEGtoANG", "Q_8_8"];

/// S3's conversions between plain pixels and 16.16, which keep a quantity
/// what it is: a position stays a position through `INTtoFIXED(i * 2 + 96)`.
pub const CONVERSIONS: &[&str] = &["toFIXED", "INTtoFIXED", "FIXEDtoINT"];

/// A constant that is a scale's one, as `FIXED_ONE` is a 16.16 number's
/// (`SOUND_TEMPO_SCALE_NORMAL`, `AFFINE_SCALE_ONE`, `STAT_SCALE_ONE`): it
/// composes a fraction only with a divisor, `SOUND_TEMPO_SCALE_NORMAL * 15 /
/// 16`. A scale's other constants (`BGM_UNITY_SCALE`, `BOUNCE_SCALE_STEPS`)
/// are not its one.
fn is_scale(name: &str) -> bool {
    let words = words(name);
    name.bytes().all(|b| !b.is_ascii_lowercase())
        && words.iter().any(|word| word == "scale")
        && words.last().is_some_and(|word| matches!(word.as_str(), "one" | "normal" | "unity"))
}

/// Words of a name that hold a position or a size in pixels (or cells): a
/// place (`position[0] == 16`) and a slant's start too.
const POSITION_WORDS: &[&str] =
    &["x", "y", "z", "dx", "dy", "dz", "row", "rows", "column", "columns", "width", "height", "radius", "distance", "position", "slant"];
/// Words of a name that hold a duration or a moment in frames, or in beats
/// of a few frames each (`beat == 32`).
const DURATION_WORDS: &[&str] = &["frame", "frames", "timer", "delay", "duration", "countdown", "wait", "ticks", "vblank", "vblanks", "beat", "beats"];
/// Words of a name that hold a fade's or a shake's level (`fade = -16`,
/// `shake = -128`): the brightness or the sway it is set to, and the
/// brightness register's (`REG_BLDY`, GBATEK's fade coefficient).
const LEVEL_WORDS: &[&str] = &["fade", "shake", "bldy"];
/// Words of a variable that holds a tone: a model's shade on the canvas, a
/// brightness, or a colour channel's level, 0 to 31 in RGB555, as `RGB(r, g,
/// b)` takes it. Read only where the number is the level itself
/// (`tone_level`).
const TONE_WORDS: &[&str] = &["shade", "brightness", "red", "green", "blue"];
/// Words that make a tone's name something other than its level:
/// `redPhase`, `shadeScale`, `blueIndex`.
const NOT_TONE_WORDS: &[&str] = &["scale", "phase", "mode", "state", "count", "index"];
/// Words of a name that holds a percentage (`maxHpPercent = 12`).
const PERCENT_WORDS: &[&str] = &["percent"];
/// Words that make a name something else whatever its other words say: a
/// rate (`speedX`), a scale (`scaleX`) or an angle (`angleY`).
const OTHER_WORDS: &[&str] =
    &["velocity", "speed", "step", "freq", "frequency", "acceleration", "scale", "angle", "rotation", "zoom", "shift", "flip"];

/// Arithmetic helpers that compute with a random roll they are passed:
/// `Math_RemainderUnsigned(Random_Next(), 90)`.
const ARITHMETIC: &[&str] = &["Remainder", "Divide", "Modulo", "Mul", "Div", "Mod"];

/// A name's words, lowercase, its digits dropped: `transitionFrames` is
/// `transition` and `frames`, `gAltinPeak_FlickerTimer` ends in `timer`, and
/// a word in mixed case is one (`gVBlankCount` holds `vblank`).
pub fn words(name: &str) -> Vec<String> {
    split(name, false)
}

/// A name's words as `words` gives them, and also each capital that may be a
/// word of one letter before a word in mixed case taken apart from it:
/// `logEColumn` holds `ecolumn`, and also `e` and `column`.
fn all_words(name: &str) -> Vec<String> {
    let mut all = split(name, false);
    all.extend(split(name, true));
    all
}

fn split(name: &str, letters: bool) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut previous = '_';
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        let next = chars.peek().copied().unwrap_or('_');
        let letter = letters && c.is_ascii_uppercase() && previous.is_ascii_uppercase() && next.is_ascii_lowercase();
        if c == '_' || c.is_ascii_digit() || (c.is_ascii_uppercase() && previous.is_ascii_lowercase()) || letter {
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
/// a `duration`, a fade's or shake's `level` or a percentage (`decimal`), none
/// where any word makes it a rate, scale or angle (`fadeStep`).
pub fn named_kind<'n>(names: impl IntoIterator<Item = &'n str>) -> Option<&'static str> {
    let (mut position, mut duration, mut level, mut percent) = (false, false, false, false);
    for name in names {
        let words = all_words(name);
        let has = |list: &[&str]| words.iter().any(|word| list.contains(&word.as_str()));
        if has(OTHER_WORDS) {
            return None;
        }
        position |= has(POSITION_WORDS);
        duration |= has(DURATION_WORDS);
        level |= has(LEVEL_WORDS);
        percent |= has(PERCENT_WORDS);
    }
    if position {
        Some("position")
    } else if duration {
        Some("duration")
    } else if level {
        Some("level")
    } else if percent {
        Some("decimal")
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
/// From a closing bracket it takes the whole bracket: `menu->memberX[0]`
/// from its `]`.
fn span(tokens: &[Token], at: usize, floor: usize) -> (usize, usize, Option<usize>) {
    let pp = tokens[at].pp;
    let closer = usize::from(matches!(tokens[at].text, ")" | "]"));
    let (mut start, mut left, mut depth) = (at, None, closer);
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
/// `gFieldState->transitionFrames`) between `start` and `end` at expression
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
            // S3's conversions keep their value's kind, as a group does:
            // `FIXEDtoINT(cos * RADIUS_Y) + 40` is reckoned with a radius.
            "(" if at > start && CONVERSIONS.contains(&tokens[at - 1].text) => open.push(Open::Group),
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
/// call or group is one operand, named by its call (`px`) or `()`. Also
/// which of them is the number's own.
fn factors<'a>(tokens: &[Token<'a>], at: usize, floor: usize) -> (Vec<(&'a str, &'a str)>, usize) {
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
    let mut own = 0;
    // Leftwards: `operand op` pairs.
    let mut first = at;
    while first >= floor + 2 && same(first - 1) && matches!(tokens[first - 1].text, "*" | "/" | "%") {
        let op = tokens[first - 1].text;
        let Some((begin, name)) = operand_before(first - 2) else { break };
        out[0].0 = op;
        out.insert(0, ("", name));
        own += 1;
        first = begin;
    }
    let mut last = at;
    while tokens.get(last + 1).is_some_and(|t| matches!(t.text, "*" | "/" | "%")) && same(last + 1) {
        let op = tokens[last + 1].text;
        let Some((end, name)) = operand_after(last + 2) else { break };
        out.push((op, name));
        last = end;
    }
    (out, own)
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
/// routine it is in): a composed form, a decimal fraction or percentage, a
/// random range, a decimal position, duration or level by the names it is
/// reckoned with, or the frames a loop counts. The parameter a number fills
/// is judged by the caller, who knows every declaration.
pub fn local(tokens: &[Token], at: usize, floor: usize, limit: usize) -> Option<&'static str> {
    let text = tokens[at].text;
    let (factors, own) = factors(tokens, at, floor);
    let divided = factors.iter().any(|(op, _)| *op == "/");
    if factors.iter().any(|(_, operand)| UNITS.contains(operand) || UNIT_CALLS.contains(operand) || (divided && is_scale(operand))) {
        return Some("composed");
    }
    let (start, end, left) = span(tokens, at, floor);
    // A fraction a unit's own call composes, its whole argument two numbers:
    // `Q_8_8(11.0 / 16)`.
    let fraction = |open: usize| {
        let texts: Vec<&Token> = tokens.iter().skip(open + 1).take(4).collect();
        matches!(texts[..], [a, b, c, d] if a.kind == Kind::Number && b.text == "/" && c.kind == Kind::Number && d.text == ")")
    };
    // A product of numbers alone, as S3 composes `240 * 160`, and a fraction
    // of a named quantity, a constant's own (`ATAN_ENTRIES / 16`).
    let numeral = |operand: &str| operand.bytes().next().is_some_and(|b| b.is_ascii_digit());
    if factors.len() > 1 && factors.iter().all(|(op, operand)| matches!(*op, "" | "*") && numeral(operand)) {
        return Some("composed");
    }
    let constant = |index: usize| {
        let token = &tokens[index];
        token.kind == Kind::Ident
            && token.text.bytes().any(|b| b.is_ascii_uppercase())
            && token.text.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            && (index == floor || !matches!(tokens[index - 1].text, "." | "->"))
    };
    if factors.len() == 2 && own == 1 && factors[1].0 == "/" && is_decimal(text) && at >= floor + 2 && constant(at - 2) {
        return Some("composed");
    }
    let unit_call = left.is_some_and(|left| tokens[left].text == "(" && left > floor && UNIT_CALLS.contains(&tokens[left - 1].text) && fraction(left));
    if unit_call {
        return Some("composed");
    }
    let (statement_start, statement_end) = statement(tokens, at, floor);
    let char_in_statement = |c: &str| tokens[statement_start..=statement_end].iter().any(|t| t.kind == Kind::Char && t.text == c);
    let digits = char_in_statement("'0'");
    let tenths = factors.iter().any(|(op, operand)| matches!(*op, "/" | "%") && is_power_of_ten(operand));
    let base = digits && factors.iter().any(|(op, operand)| *op == "*" && is_power_of_ten(operand));
    // A percentage: `hazardSteps * 100 / hazardLimit`.
    let multiplied = factors[own].0 == "*" || factors.get(own + 1).is_some_and(|(op, _)| *op == "*");
    let percent = is_power_of_ten(text) && multiplied && factors[own + 1..].iter().any(|(op, _)| *op == "/");
    // The value of a hex digit's letter: `digit + 'A' - 10`.
    let letter = text == "10" && factors.len() == 1 && (char_in_statement("'A'") || char_in_statement("'a'"));
    // A cap of decimal digits a count is held to: `magnitude > 99`.
    let nines = text.len() > 1 && text.bytes().all(|b| b == b'9') && at > floor && matches!(tokens[at - 1].text, "<" | "<=" | ">" | ">=");
    if tenths || base || percent || letter || nines || (digits && is_power_of_ten(text) && factors.len() == 1) || decimal_digit(tokens, at, floor, limit) {
        return Some("decimal");
    }
    // Its own operand's names first, then, out through each group or S3
    // conversion the operand fills, which keeps a quantity's kind, what it is
    // reckoned with or given to: `y = -(i * spacing + 16)`, `rock->y =
    // INTtoFIXED(i * 2 + 96)`. A group that holds more than the operand
    // (`(z >= 36 && column >= 22)`) or is a shift's amount (`1 << (16 -
    // variant)`, bits) is not passed, and a roll counts only in the number's
    // own operand.
    let (mut found, random) = chains(tokens, start, end);
    let (mut inner, mut left) = ((start, end), left);
    let shifted = |open: usize| open > floor && matches!(tokens[open - 1].text, "<<" | ">>");
    while let Some(open) = left.filter(|&open| (is_group(tokens, open, floor) && !shifted(open)) || is_conversion(tokens, open, floor)) {
        let Some(close) = close_of(tokens, open, limit).filter(|&close| inner == (open + 1, close - 1)) else { break };
        let (outer_start, outer_end, outer_left) = span(tokens, close, floor);
        found.extend(chains(tokens, outer_start, outer_end).0);
        (inner, left) = ((outer_start, outer_end), outer_left);
    }
    // A conditional's arm is the value its target is given: `y = i == 0 ? 8
    // : 40`.
    if let Some(question) = left.and_then(|arm| conditional(tokens, arm, floor)).filter(|&question| question > floor) {
        left = span(tokens, question - 1, floor).2;
    }
    if random || random_local(tokens, at, floor, limit) {
        return Some("random");
    }
    if is_mask(tokens, at, floor, limit) {
        return Some("mask");
    }
    if is_sign_bit(tokens, at, floor) {
        return Some("sign");
    }
    if packing(tokens, at, floor, limit) {
        return Some("packing");
    }
    // A shift's amount is bits, never a position or a duration.
    let shift = at > floor && matches!(tokens[at - 1].text, "<<" | ">>");
    // A packed value's low byte or the bytes above it: `crc % 256`, and
    // `(s32)control / 256` of a value cast to be cut (a plain `velocityX /
    // 256` is a fraction).
    let cast = at >= floor + 2 && tokens[at - 2].kind == Kind::Ident && {
        let mut first = at - 2;
        while first >= floor + 2 && matches!(tokens[first - 1].text, "." | "->") && tokens[first - 2].kind == Kind::Ident {
            first -= 2;
        }
        first >= floor + 3 && tokens[first - 1].text == ")" && tokens[first - 3].text == "(" && matches!(tokens[first - 2].text, "u8" | "s8" | "u16" | "s16" | "u32" | "s32")
    };
    if text == "256" && (factors[own].0 == "%" || (factors[own].0 == "/" && cast)) {
        return Some("packing");
    }
    // A 16.16 product or a sine scaled to its unit: `Iwram_MulFX(a, b) >>
    // 11`, `Trig_Sin(angle) >> 14`.
    if shift && shifts_product(tokens, at, floor) {
        return Some("scaling");
    }
    if !is_decimal(text) || shift {
        return None;
    }
    if filled_in_order(tokens, at, floor, limit) {
        return Some("index");
    }
    if weight(tokens, at, floor, limit) {
        return Some("random");
    }
    if left.is_some_and(|left| tokens[left].text == "case") {
        let (subject_start, subject_end) = switch_subject(tokens, at, floor)?;
        let (subject, _) = chains(tokens, subject_start, subject_end);
        let named = subject.iter().find_map(|chain| named_kind(chain.iter().copied()));
        return named.or_else(|| counted_each_pass(tokens, (subject_start, subject_end), limit).then_some("duration"));
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
    named
        .or_else(|| tone_level(tokens, at, floor).then_some("level"))
        .or_else(|| frame_count(tokens, at, floor, limit).then_some("duration"))
        .or_else(|| cadence(tokens, at, floor, limit).then_some("duration"))
}

/// The operand a shift at `at - 1` moves, when it ends right before the
/// shift: its first token and its last.
fn shifted(tokens: &[Token], at: usize, floor: usize) -> Option<(usize, usize)> {
    let last = at.checked_sub(2).filter(|&last| last >= floor)?;
    let mut first = last;
    if matches!(tokens[last].text, ")" | "]") {
        first = open_of(tokens, last, floor)?;
        if first > floor && tokens[first - 1].kind == Kind::Ident {
            first -= 1;
        }
    } else if tokens[last].kind != Kind::Ident {
        return None;
    }
    while first >= floor + 2 && matches!(tokens[first - 1].text, "." | "->") && tokens[first - 2].kind == Kind::Ident {
        first -= 2;
    }
    Some((first, last))
}

/// Whether the shift whose amount is at `at` scales a 16.16 product or a
/// sine to its unit: what `Iwram_MulFX`, `Trig_Sin` or `Trig_Cos` returns.
fn shifts_product(tokens: &[Token], at: usize, floor: usize) -> bool {
    let Some((first, last)) = shifted(tokens, at, floor) else { return false };
    tokens[last].text == ")" && matches!(tokens[first].text, "Iwram_MulFX" | "Trig_Sin" | "Trig_Cos")
}

/// Whether the number at `at` is the period of a cue on the counter of a
/// loop that counts frames by its bound's name, up to `limit`: `if (i % 64
/// == 0)` or `if ((i & 31) == 0)` inside `for (i = 0; i != PUFF_SHOWER_FRAMES;
/// i++)`, a loop that need not wait every pass. A mask is one less than a
/// power of two.
pub fn cadence(tokens: &[Token], at: usize, floor: usize, limit: usize) -> bool {
    let Some(value) = decimal_value(tokens[at].text) else { return false };
    if at < floor + 2 || tokens[at - 2].kind != Kind::Ident || (at >= floor + 3 && matches!(tokens[at - 3].text, "." | "->")) {
        return false;
    }
    let counter = tokens[at - 2].text;
    match tokens[at - 1].text {
        "%" => {}
        "&" if (value + 1).is_power_of_two() => {}
        _ => return false,
    }
    if !words(counter).iter().all(|word| COUNTER_WORDS.contains(&word.as_str())) {
        return false;
    }
    // Outwards through the braces around it to a `for` whose condition
    // reads the counter and a name of frames.
    let mut depth = 0;
    let mut index = at;
    while index > floor {
        index -= 1;
        match tokens[index].text {
            "}" => depth += 1,
            "{" if depth > 0 => depth -= 1,
            "{" if index > floor && tokens[index - 1].text == ")" => {
                let Some(open) = open_of(tokens, index - 1, floor) else { continue };
                if open == floor || tokens[open - 1].text != "for" || close_of(tokens, index, limit).is_none() {
                    continue;
                }
                let header = &tokens[open + 1..index - 1];
                let clauses: Vec<&[Token]> = header.split(|t| t.text == ";").collect();
                let [_, condition, _] = clauses[..] else { continue };
                let reads = condition.iter().any(|t| t.text == counter);
                let frames = condition.iter().any(|t| t.kind == Kind::Ident && t.text != counter && named_kind([t.text]) == Some("duration"));
                if reads && frames {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Whether the number at `at` is an index or a stride continuing the ones
/// before it in the routine whose body opens at `open`, up to `limit`: the
/// 10 of `src[10]` where `src[0]` to `src[9]` are read there too, and the 12
/// of `src += 12` past `src[11]`, as a code generator copies a template's
/// words in order.
pub fn continues_indices(tokens: &[Token], at: usize, open: usize, limit: usize) -> bool {
    let Some(value) = decimal_value(tokens[at].text) else { return false };
    if at < open + 2 || tokens[at - 2].kind != Kind::Ident || value == 0 {
        return false;
    }
    let array = tokens[at - 2].text;
    let index = tokens[at - 1].text == "[" && tokens.get(at + 1).is_some_and(|t| t.text == "]");
    let stride = tokens[at - 1].text == "+=" && tokens.get(at + 1).is_some_and(|t| t.text == ";");
    if !index && !stride {
        return false;
    }
    let end = limit.min(tokens.len() - 1);
    let mut seen = BTreeSet::new();
    for at in open..end.saturating_sub(2) {
        if tokens[at].text == array && tokens[at + 1].text == "[" && tokens[at + 3].text == "]" {
            if let Some(value) = decimal_value(tokens[at + 2].text) {
                seen.insert(value);
            }
        }
    }
    (0..value).all(|before| seen.contains(&before)) && (index || seen.contains(&(value - 1)))
}

/// Whether the `(` at `open` only groups (`-(i * k + 16)`, the cast's operand
/// in `(u32)(x + 15)`): no call's, and no statement's (`if (`, `while (`).
pub fn is_group(tokens: &[Token], open: usize, floor: usize) -> bool {
    const STATEMENTS: &[&str] = &["if", "while", "for", "switch", "sizeof", "return"];
    tokens[open].text == "("
        && (open == floor || !(tokens[open - 1].kind == Kind::Ident || STATEMENTS.contains(&tokens[open - 1].text)))
}

/// Whether the number at `at` is reckoned in a group of its own
/// (`(frame - 486) * STEP`), which its operand ends.
pub fn in_group(tokens: &[Token], at: usize, floor: usize) -> bool {
    let (_, _, left) = span(tokens, at, floor);
    left.is_some_and(|left| is_group(tokens, left, floor))
}

/// The hardware register whose address another argument of the call whose
/// `(` is at `open` takes: `REG_BLDY` of `Io_Write16(31, &REG_BLDY)` and of
/// `IO_WRITE_QUEUE_PUSH((u32)&REG_BLDY, 12, ...)`.
pub fn written_register<'a>(tokens: &[Token<'a>], open: usize) -> Option<&'a str> {
    let mut depth = 0;
    for at in open + 1..tokens.len() {
        match tokens[at].text {
            "(" | "[" => depth += 1,
            ")" | "]" if depth == 0 => return None,
            ")" | "]" => depth -= 1,
            ";" | "{" | "}" => return None,
            "&" if tokens.get(at + 1).is_some_and(|t| t.kind == Kind::Ident && t.text.starts_with("REG_")) => return Some(tokens[at + 1].text),
            _ => {}
        }
    }
    None
}

/// The `?` of the conditional whose arm the `?` or `:` at `arm` opens: none
/// for a `case`'s or a label's colon.
fn conditional(tokens: &[Token], arm: usize, floor: usize) -> Option<usize> {
    match tokens[arm].text {
        "?" => return Some(arm),
        ":" => {}
        _ => return None,
    }
    let (mut depth, mut index) = (0, arm);
    while index > floor {
        index -= 1;
        match tokens[index].text {
            ")" | "]" => depth += 1,
            "(" | "[" if depth > 0 => depth -= 1,
            "(" | "[" => return None,
            "?" if depth == 0 => return Some(index),
            text if depth == 0 && (matches!(text, ";" | "{" | "}" | "," | ":" | "case") || ASSIGNMENTS.contains(&text)) => return None,
            _ => {}
        }
    }
    None
}

/// Whether the `(` at `open` is the argument list of one of S3's conversions
/// (`INTtoFIXED(`).
fn is_conversion(tokens: &[Token], open: usize, floor: usize) -> bool {
    tokens[open].text == "(" && open > floor && CONVERSIONS.contains(&tokens[open - 1].text)
}

/// A hex literal's value.
fn hex_value(text: &str) -> Option<u64> {
    let digits = text.trim_end_matches(['u', 'U', 'l', 'L']);
    let hex = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X"))?;
    u64::from_str_radix(hex, 16).ok()
}

/// Whether the hex number at `at` is a mask of whole fields, which S3 keeps
/// in hex: an operand of `&` or `&=` (`(step & 0xff)`, `*cell &= 0xfff0`,
/// `x &= ~0xff`), or the whole value given to a variable named for a mask
/// (`u32 byteMask = 0xff;`), whose every hex digit is 0 or f (`0xff00`,
/// `0x0ff00ff0`) or which keeps a value's low bits (`0x1ff`, `0x7f`). A lone
/// bit (`& 0x8000`), two or three bits (`& 0xc000`), and a field cleared by
/// its complement (`&= 0xfcff`) are things with names.
pub fn is_mask(tokens: &[Token], at: usize, floor: usize, limit: usize) -> bool {
    let Some(value) = hex_value(tokens[at].text) else { return false };
    let whole_digits = (0..16).all(|digit| matches!((value >> (4 * digit)) & 0xf, 0 | 0xf));
    let low_bits = value & value.wrapping_add(1) == 0;
    if value == 0 || !(whole_digits || low_bits) {
        return false;
    }
    let text = |index: Option<usize>| index.filter(|&index| index >= floor && index <= limit).map_or("", |index| tokens[index].text);
    let before = if text(at.checked_sub(1)) == "~" { text(at.checked_sub(2)) } else { text(at.checked_sub(1)) };
    if matches!(before, "&" | "&=") || text(Some(at + 1)) == "&" {
        return true;
    }
    // `u32 byteMask = 0xff;`.
    let (start, end, left) = span(tokens, at, floor);
    let Some(op) = left.filter(|&left| start == at && end == at && tokens[left].text == "=" && left > floor) else { return false };
    let (target_start, target_end, _) = span(tokens, op - 1, floor);
    let (target, _) = chains(tokens, target_start, target_end);
    target.last().and_then(|chain| chain.last()).is_some_and(|name| words(name).last().is_some_and(|word| matches!(word.as_str(), "mask" | "masks")))
}

/// Whether a hex literal is an address in the console's memory map, which
/// S3 keeps hex: from EWRAM at `0x02000000` to the Game Pak's save memory
/// below `0x10000000` (`(const void *)0x06000290`, in VRAM). The BIOS's
/// low addresses are a number like any other, and values from
/// `0x80000000` up are no address.
pub fn is_hardware_address(text: &str) -> bool {
    hex_value(text).is_some_and(|value| (0x0200_0000..0x1000_0000).contains(&value))
}

/// Whether the number at `at` is the amount of a right shift by 31, which
/// takes the sign bit of a word (`(u32)drift >> 31`, `isActive >>= 31`): the
/// bit S3 keeps as `0x80000000`. Any other amount is a shift's own.
pub fn is_sign_bit(tokens: &[Token], at: usize, floor: usize) -> bool {
    at > floor && tokens[at].text.trim_end_matches(['u', 'U']) == "31" && matches!(tokens[at - 1].text, ">>" | ">>=")
}

const SHIFTS: &[&str] = &["<<", ">>", "<<=", ">>="];

/// A decimal literal's value.
fn decimal_value(text: &str) -> Option<u64> {
    is_decimal(text).then(|| text.trim_end_matches(['u', 'U', 'l', 'L']).parse().ok()).flatten()
}

/// Tokens that end a shift's operand: what binds less tightly than a shift.
fn ends_shift_operand(text: &str) -> bool {
    matches!(
        text,
        "|" | "^" | "&" | "&&" | "||" | "?" | ":" | "," | ";" | "{" | "}" | "<" | ">" | "<=" | ">=" | "==" | "!=" | "<<" | ">>" | "return" | "case"
    ) || ASSIGNMENTS.contains(&text)
}

/// Whether the shift whose operator is at `op` and whose amount ends at
/// `last` is an operand of `|`, out through the groups that hold it alone:
/// `in[1] << 8 | in[2] << 16`, `RGB(21, 10, 21) << 16 | RGB(21, 10, 21)`,
/// `((u32)count >> 16) | KEY_B`.
fn joined_by_or(tokens: &[Token], op: usize, last: usize, floor: usize, limit: usize) -> bool {
    let limit = limit.min(tokens.len() - 1);
    let (mut left, mut right) = (op, last);
    loop {
        let (mut depth, mut index) = (0, right + 1);
        let right_stop = loop {
            if index > limit {
                break None;
            }
            match tokens[index].text {
                "(" | "[" => depth += 1,
                ")" | "]" if depth == 0 => break Some(index),
                ")" | "]" => depth -= 1,
                text if depth == 0 && ends_shift_operand(text) => break Some(index),
                _ => {}
            }
            index += 1;
        };
        let (mut depth, mut index) = (0, left);
        let left_stop = loop {
            if index <= floor {
                break None;
            }
            index -= 1;
            match tokens[index].text {
                ")" | "]" => depth += 1,
                "(" | "[" if depth == 0 => break Some(index),
                "(" | "[" => depth -= 1,
                text if depth == 0 && ends_shift_operand(text) => break Some(index),
                _ => {}
            }
        };
        let or = |stop: Option<usize>| stop.is_some_and(|stop| matches!(tokens[stop].text, "|" | "|="));
        if or(left_stop) || or(right_stop) {
            return true;
        }
        match (left_stop, right_stop) {
            (Some(open), Some(close)) if tokens[open].text == "(" && tokens[close].text == ")" && is_group(tokens, open, floor) => {
                (left, right) = (open, close);
            }
            _ => return false,
        }
    }
}

/// Whether the shift amount at `at` is one of two shifts, left and then
/// right, that cut a bit field out of a word: `(seed << 8) >> 16`, `x << 23 >> 7`, `((u32)grey << 16) >> 16`.
fn double_shift(tokens: &[Token], at: usize, floor: usize, limit: usize) -> bool {
    let limit = limit.min(tokens.len() - 1);
    let number = |index: usize| index <= limit && tokens[index].kind == Kind::Number;
    // A shift after this one: past the groups this amount closes.
    let mut next = at + 1;
    while next <= limit && tokens[next].text == ")" && open_of(tokens, next, floor).is_some_and(|open| is_group(tokens, open, floor)) {
        next += 1;
    }
    let left = |text: &str| matches!(text, "<<" | "<<=");
    if left(tokens[at - 1].text) && next < limit && tokens[next].text == ">>" && number(next + 1) {
        return true;
    }
    // A left shift before it: its operand is one, or a group ending in one.
    if at < floor + 3 || !matches!(tokens[at - 1].text, ">>" | ">>=") {
        return false;
    }
    let operand = at - 2;
    if tokens[operand].kind == Kind::Number && left(tokens[operand - 1].text) {
        return true;
    }
    // `return (seed << 8) >> 16` groups as any other bracket does.
    let group = |open: usize| is_group(tokens, open, floor) || (open > floor && tokens[open - 1].text == "return");
    tokens[operand].text == ")"
        && operand >= floor + 3
        && open_of(tokens, operand, floor).is_some_and(group)
        && tokens[operand - 1].kind == Kind::Number
        && left(tokens[operand - 2].text)
}

/// The array an assignment in the statement around `at` stores to, when its
/// target is an element (`out[statsOffset + 4] = ...`), and where its
/// operator is.
fn stored_element<'a>(tokens: &[Token<'a>], at: usize, floor: usize) -> Option<(&'a str, usize)> {
    let (start, end) = statement(tokens, at, floor);
    let mut depth = 0;
    for index in start..=end {
        match tokens[index].text {
            "(" | "[" => depth += 1,
            ")" | "]" => depth -= 1,
            text if depth == 0 && ASSIGNMENTS.contains(&text) => {
                let close = index.checked_sub(1).filter(|&close| close > start && tokens[close].text == "]")?;
                let open = open_of(tokens, close, start)?;
                let name = open.checked_sub(1).filter(|&name| name >= start && tokens[name].kind == Kind::Ident)?;
                return Some((tokens[name].text, index));
            }
            _ => {}
        }
    }
    None
}

/// The shift amounts the routine (up to `limit`) stores into elements of
/// `array`: `out[k] = word >> 24; out[k + 1] = word >> 16;` gives 24 and 16.
fn stored_amounts(tokens: &[Token], array: &str, floor: usize, limit: usize) -> BTreeSet<u64> {
    let limit = limit.min(tokens.len() - 1);
    let mut amounts = BTreeSet::new();
    for index in floor..=limit {
        if tokens[index].text != array || tokens[index].kind != Kind::Ident || tokens.get(index + 1).is_none_or(|t| t.text != "[") {
            continue;
        }
        let Some((name, op)) = stored_element(tokens, index + 1, floor) else { continue };
        if name != array {
            continue;
        }
        let (_, end) = statement(tokens, op, floor);
        for shift in op + 1..end.min(limit) {
            if SHIFTS.contains(&tokens[shift].text) {
                amounts.extend(decimal_value(tokens[shift + 1].text));
            }
        }
    }
    amounts
}

/// Whether the number at `at` packs a word into bytes or unpacks it, or cuts
/// a bit field out of one: a shift's amount, at a nibble's boundary, of an
/// element joined to others by `|` (`in[statsOffset + 1] << 16 | ...`), of
/// halves or bytes joined by `|` (`RGB(...) << 16 | RGB(...)`), or stored into
/// an array the routine fills with two or more such amounts (`out[k] = stats
/// >> 24`); one of two shifts that cut a field (`(seed << 8) >> 16`); or a
/// byte's index in such an element (`out[statsOffset + 11]`). A shift stored
/// alone (`rows[1] = reciprocal << 16`, `x = (u32)x >> 16`) may be a 16.16
/// conversion, and stays.
pub fn packing(tokens: &[Token], at: usize, floor: usize, limit: usize) -> bool {
    let Some(value) = decimal_value(tokens[at].text) else { return false };
    let previous = if at > floor { tokens[at - 1].text } else { "" };
    if SHIFTS.contains(&previous) {
        if !(1..32).contains(&value) {
            return false;
        }
        if double_shift(tokens, at, floor, limit) {
            return true;
        }
        if value % 4 != 0 {
            return false;
        }
        let joined = joined_by_or(tokens, at - 1, at, floor, limit);
        // An element, or a field of one: `in[3] << 8`, `(in[3] & 0xf0) << 20`.
        let element = at >= floor + 2
            && (tokens[at - 2].text == "]"
                || (tokens[at - 2].text == ")"
                    && open_of(tokens, at - 2, floor).is_some_and(|open| is_group(tokens, open, floor) && tokens[open..at - 2].iter().any(|t| t.text == "["))));
        if joined && (element || value % 8 == 0) {
            return true;
        }
        return stored_element(tokens, at, floor)
            .is_some_and(|(array, op)| op < at && stored_amounts(tokens, array, floor, limit).len() >= 2);
    }
    // A byte's index: `out[statsOffset + 11] = ...`, `in[statsOffset + 13] << 12`.
    let mut open = at;
    while open > floor && (matches!(tokens[open - 1].kind, Kind::Ident | Kind::Number) || matches!(tokens[open - 1].text, "+" | "-" | "*")) {
        open -= 1;
    }
    if open <= floor || tokens[open - 1].text != "[" || tokens[open - 2].kind != Kind::Ident {
        return false;
    }
    let (open, array) = (open - 1, tokens[open - 2].text);
    let Some(close) = close_of(tokens, open, limit) else { return false };
    let after = tokens.get(close + 1).map_or("", |t| t.text);
    if ASSIGNMENTS.contains(&after) {
        return stored_amounts(tokens, array, floor, limit).len() >= 2;
    }
    SHIFTS.contains(&after)
        && tokens.get(close + 2).and_then(|t| decimal_value(t.text)).is_some_and(|amount| amount % 4 == 0)
        && joined_by_or(tokens, close + 1, close + 2, floor, limit)
}

/// The array and index of the element assigned at `at`'s index, as in
/// `name[at] = ...`: the tokens of its name (`gState.weights`), from the start
/// of its statement.
fn element_target<'a>(tokens: &[Token<'a>], at: usize, floor: usize) -> Option<Vec<&'a str>> {
    if at < floor + 2 || tokens[at - 1].text != "[" || tokens.get(at + 1).is_none_or(|t| t.text != "]") || tokens.get(at + 2).is_none_or(|t| t.text != "=") {
        return None;
    }
    let mut start = at - 1;
    while start > floor && (tokens[start - 1].kind == Kind::Ident || matches!(tokens[start - 1].text, "." | "->")) {
        start -= 1;
    }
    let statement = start == floor || matches!(tokens[start - 1].text, ";" | "{" | "}") || tokens[start - 1].kind == Kind::PpEnd;
    (statement && start < at - 1 && tokens[start].kind == Kind::Ident).then(|| tokens[start..at - 1].iter().map(|t| t.text).collect())
}

/// Whether the number at `at` is the index of an array the routine fills
/// element by element, every index from 0 to this one set by a statement of
/// its own (`pageHeights[0] = ...; ... pageHeights[15] = ...;`): its place in
/// the array, as a row's in a table.
pub fn filled_in_order(tokens: &[Token], at: usize, floor: usize, limit: usize) -> bool {
    let Some(name) = element_target(tokens, at, floor) else { return false };
    let Some(value) = decimal_value(tokens[at].text) else { return false };
    let limit = limit.min(tokens.len() - 1);
    let mut found = BTreeSet::new();
    for index in floor..=limit {
        if index > floor && tokens[index].kind == Kind::Number && tokens[index - 1].text == "[" && element_target(tokens, index, floor).is_some_and(|other| other == name) {
            found.extend(decimal_value(tokens[index].text));
        }
    }
    (0..=value).all(|index| found.contains(&index))
}

/// Whether the number at `at` is a random weight: the whole value given to
/// an element of an array named for its weights, filled in order
/// (`prizeWeights[9] = 32;`).
pub fn weight(tokens: &[Token], at: usize, floor: usize, limit: usize) -> bool {
    if at < floor + 4 || tokens[at - 1].text != "=" || tokens.get(at + 1).is_none_or(|t| t.text != ";") || tokens[at - 2].text != "]" {
        return false;
    }
    let index = at - 3;
    let Some(name) = element_target(tokens, index, floor) else { return false };
    let weights = name.last().is_some_and(|last| words(last).iter().any(|word| matches!(word.as_str(), "weight" | "weights")));
    weights && filled_in_order(tokens, index, floor, limit)
}

/// Whether a `switch`'s subject is a counter moved by one every pass: a
/// statement after the switch, before `limit`, moves it by one (`step++`,
/// `perspective->reloadSteps--`), and nothing in the file sets it to a
/// number but 0, so that each `case` is a moment of its count
/// (`sBeaconRiseStep`, `reloadSteps` counting down from
/// `WORLD_MAP_RELOAD_STEPS`). A step machine that jumps to a numbered step
/// (`tutorialStep = 16`) is no count.
fn counted_each_pass(tokens: &[Token], subject: (usize, usize), limit: usize) -> bool {
    let (start, end) = subject;
    let name = &tokens[end];
    let plain = (start..=end).all(|index| tokens[index].kind == Kind::Ident || matches!(tokens[index].text, "." | "->" | "*"));
    if name.kind != Kind::Ident || !plain || tokens.get(end + 2).is_none_or(|t| t.text != "{") {
        return false;
    }
    let limit = limit.min(tokens.len() - 1);
    let Some(close) = close_of(tokens, end + 2, limit) else { return false };
    let named = |index: usize| tokens[index].kind == Kind::Ident && tokens[index].text == name.text;
    let text = |index: usize| tokens.get(index).map_or("", |t| t.text);
    let moved = (close + 1..=limit).any(|index| {
        named(index)
            && (matches!(text(index + 1), "++" | "--")
                || matches!(text(index - 1), "++" | "--")
                || (matches!(text(index + 1), "+=" | "-=") && text(index + 2) == "1" && text(index + 3) == ";"))
    });
    let numbered = (1..tokens.len().saturating_sub(1)).any(|index| {
        named(index)
            && ASSIGNMENTS.contains(&text(index + 1))
            && tokens[index + 2].kind == Kind::Number
            && !(text(index + 1) == "=" && text(index + 2) == "0")
            && !(matches!(text(index + 1), "+=" | "-=") && text(index + 2) == "1")
    });
    moved && !numbered
}

/// Whether the number at `at` is the whole value a routine returns
/// (`return 70;`, `return -100;`), or the whole value given to a local it
/// returns (`fixedChance = 60; ... return -fixedChance;`), between `open`
/// and `close`, its braces.
pub fn returned_whole(tokens: &[Token], at: usize, open: usize, close: usize) -> bool {
    let text = |index: usize| tokens.get(index).map_or("", |t| t.text);
    if text(at + 1) != ";" || at < open + 2 {
        return false;
    }
    let before = if text(at - 1) == "-" { at - 2 } else { at - 1 };
    if text(before) == "return" {
        return true;
    }
    let local = &tokens[before - 1];
    if text(before) != "=" || local.kind != Kind::Ident || !matches!(text(before - 2), ";" | "{" | "}" | ":") {
        return false;
    }
    (open..close).any(|index| {
        text(index) == "return"
            && (text(index + 1) == local.text && text(index + 2) == ";" || text(index + 1) == "-" && text(index + 2) == local.text && text(index + 3) == ";")
    })
}

/// The innermost `switch` around `at`: its subject's bounds and its body's
/// braces.
fn enclosing_switch(tokens: &[Token], at: usize, floor: usize, limit: usize) -> Option<((usize, usize), usize, usize)> {
    let mut depth = 0;
    let mut index = at;
    while index > floor {
        index -= 1;
        match tokens[index].text {
            "}" => depth += 1,
            "{" if depth > 0 => depth -= 1,
            "{" => {
                if let Some(subject) = switch_subject(tokens, index + 1, floor).filter(|&(_, end)| end + 2 == index) {
                    return Some((subject, index, close_of(tokens, index, limit)?));
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether the number at `at` is a step of a `switch` whose steps the file's
/// own macros write (`WALK(16, 232, 744)`, `ARRIVE(17, 18)`, which `steps`
/// tells): a `case` of it or a value given to its subject (`case 17: ...
/// *state = 18;`), as a step of the same route.
pub fn route_step(tokens: &[Token], at: usize, floor: usize, limit: usize, steps: &dyn Fn(&str) -> bool) -> bool {
    let Some(((start, end), open, close)) = enclosing_switch(tokens, at, floor, limit) else { return false };
    let written = (open + 1..close).any(|index| tokens[index].kind == Kind::Ident && steps(tokens[index].text) && tokens[index + 1].text == "(");
    if !written {
        return false;
    }
    let text = |index: usize| tokens.get(index).map_or("", |t| t.text);
    if text(at - 1) == "case" && text(at + 1) == ":" {
        return true;
    }
    let length = end + 1 - start;
    text(at + 1) == ";"
        && text(at - 1) == "="
        && at >= length + 2
        && matches!(text(at - 2 - length), ";" | "{" | "}" | ")" | "else" | ":")
        && (0..length).all(|offset| tokens[at - 1 - length + offset].text == tokens[start + offset].text)
}

/// Whether the call whose `(` is at `open` takes a reciprocal
/// (`RECIPROCAL_OF(10)`), which divides by its argument.
fn is_reciprocal(tokens: &[Token], open: usize, floor: usize) -> bool {
    open > floor && tokens[open].text == "(" && words(tokens[open - 1].text).iter().any(|word| word == "reciprocal")
}

/// Whether the number at `at` is a power of ten in decimal digit work: the
/// divisor of `/=` or `%=` (`digit /= 10`, the plain `/ 10` being tenths
/// already), the whole argument of a reciprocal (`RECIPROCAL_OF(10)`), a
/// factor in a routine that writes digits as text and divides by it
/// (`value -= quotient * 10` beside `RECIPROCAL_OF(10)` and `value + '0'`),
/// or a place value compared with a name the routine divides by ten or
/// compares with another power of ten (`if (winnings < 10) ... else if
/// (winnings < 100)`). A lone bound (`if (hp < 100)`) is a threshold, and
/// stays to name.
fn decimal_digit(tokens: &[Token], at: usize, floor: usize, limit: usize) -> bool {
    let text = tokens[at].text;
    if !is_power_of_ten(text) {
        return false;
    }
    let limit = limit.min(tokens.len() - 1);
    let (start, end, left) = span(tokens, at, floor);
    let alone = start == at && end == at;
    let previous = (at > floor).then(|| tokens[at - 1].text).unwrap_or("");
    if alone && matches!(previous, "/=" | "%=") {
        return true;
    }
    let reciprocal_argument = |index: usize| {
        index > floor + 1 && tokens[index - 1].text == "(" && is_reciprocal(tokens, index - 1, floor) && tokens.get(index + 1).is_some_and(|t| t.text == ")")
    };
    if reciprocal_argument(at) {
        return true;
    }
    // Whether the routine writes digits as text (`value + '0'`) and divides
    // by this power of ten.
    let digit_text = (floor..=limit).any(|index| tokens[index].kind == Kind::Char && tokens[index].text == "'0'");
    let divides = (floor + 1..=limit)
        .any(|index| tokens[index].text == text && (matches!(tokens[index - 1].text, "/" | "%" | "/=" | "%=") || reciprocal_argument(index)));
    let (factors, own) = factors(tokens, at, floor);
    let multiplied = factors[own].0 == "*" || factors.get(own + 1).is_some_and(|(op, _)| *op == "*");
    if multiplied && digit_text && divides {
        return true;
    }
    // A place value: `name < 100`, the name alone on the other side.
    let compared = matches!(previous, "<" | "<=" | ">" | ">=") && at >= floor + 2 && tokens[at - 2].kind == Kind::Ident;
    if !compared || left.is_none() || end != at {
        return false;
    }
    let name = tokens[at - 2].text;
    (floor..=limit.saturating_sub(2)).any(|index| {
        let (next, after) = (tokens[index + 1].text, tokens[index + 2].text);
        tokens[index].text == name
            && index != at - 2
            && ((matches!(next, "<" | "<=" | ">" | ">=") && is_power_of_ten(after) && after != text)
                || (matches!(next, "/" | "%" | "/=" | "%=") && after == "10"))
    })
}

/// Whether a member chain names a tone's variable (`blue`, `model->shade`,
/// `greenLift`): a variable's own words, never a constant's
/// (`ACTOR_RED_COGS` names a thing).
fn is_tone(chain: &[&str]) -> bool {
    let Some(name) = chain.last() else { return false };
    let words = all_words(name);
    let has = |list: &[&str]| words.iter().any(|word| list.contains(&word.as_str()));
    name.bytes().any(|b| b.is_ascii_lowercase()) && has(TONE_WORDS) && !has(NOT_TONE_WORDS) && !has(OTHER_WORDS)
}

/// Whether the number at `at` is a tone's level: the whole value set to a
/// tone's variable (`blue = 26`, `shade = -16`, `red += 22`), an addend of it
/// in its own new value (`shade = shade - i * 4 + 40`), or a bound compared
/// with tones alone (`if (blue > 26)`, `if (red + green + blue > 60)`). Never
/// a factor or divisor (`* index / 64`), and never a number in a value given
/// to something else (`*pixel = 63 - red`).
fn tone_level(tokens: &[Token], at: usize, floor: usize) -> bool {
    if factors(tokens, at, floor).0.len() > 1 {
        return false;
    }
    let (start, end, left) = span(tokens, at, floor);
    let (found, _) = chains(tokens, start, end);
    if let Some(op) = left.filter(|&left| ASSIGNMENTS.contains(&tokens[left].text) && left > floor) {
        let (target_start, target_end, _) = span(tokens, op - 1, floor);
        let (target, _) = chains(tokens, target_start, target_end);
        let Some(target) = target.last().filter(|target| target_end + 1 == op && is_tone(target)) else { return false };
        let whole = (start..=end).all(|index| index == at || (index + 1 == at && tokens[index].text == "-"));
        return match tokens[op].text {
            "+=" | "-=" => whole,
            "=" => whole || found.iter().any(|chain| chain == target),
            _ => false,
        };
    }
    let comparison = (start..=end).any(|index| matches!(tokens[index].text, "<" | "<=" | ">" | ">=" | "==" | "!="));
    let plain = (start..=end).all(|index| {
        matches!(tokens[index].kind, Kind::Ident | Kind::Number) || matches!(tokens[index].text, "+" | "-" | "<" | "<=" | ">" | ">=" | "==" | "!=" | "." | "->")
    });
    comparison && plain && !found.is_empty() && found.iter().all(|chain| is_tone(chain))
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

/// The words a loop's own counter is named with: a letter (`i`, `j`), or the
/// frames themselves. A counter named for what it steps through
/// (`mosaicSize`, `pose`, `step`) or counts (`count`, `index`) is that,
/// whatever the loop waits.
const COUNTER_WORDS: &[&str] = &["i", "j", "k", "t", "counter", "remaining", "elapsed", "frame", "frames", "time"];

/// Whether the body between `open` and `close` uses `counter` as more than
/// a count of passes: an array's index (`gPartyActors[i]`), a call's
/// argument (`Spark_Spawn(i)`) or an addend of a name (`ACTOR_HERB + count`),
/// where it numbers things, not frames.
fn numbers_things(tokens: &[Token], counter: &str, open: usize, close: usize) -> bool {
    // Whether each open bracket is an index or a call's arguments.
    let mut enclosing: Vec<bool> = Vec::new();
    for index in open + 1..close {
        let token = &tokens[index];
        match token.text {
            "[" => enclosing.push(true),
            "(" => {
                let before = &tokens[index - 1];
                let call = before.kind == Kind::Ident && !matches!(before.text, "if" | "while" | "for" | "switch" | "return" | "sizeof");
                enclosing.push(call);
            }
            ")" | "]" => {
                enclosing.pop();
            }
            _ => {}
        }
        if token.kind != Kind::Ident || token.text != counter || matches!(tokens[index - 1].text, "." | "->") {
            continue;
        }
        let name = |at: Option<usize>| at.and_then(|at| tokens.get(at)).is_some_and(|t| t.kind == Kind::Ident);
        let added = (matches!(tokens[index - 1].text, "+" | "-") && name(index.checked_sub(2)))
            || (tokens.get(index + 1).is_some_and(|t| matches!(t.text, "+" | "-")) && name(Some(index + 2)));
        if enclosing.contains(&true) || added {
            return true;
        }
    }
    false
}

/// Whether the loop with its header between `header_open` and `header_close`
/// and its body between `body_open` and `body_close` counts frames on
/// `counter`: the loop's condition reads the counter (a `for`'s middle
/// clause), the loop moves it on (`i++`, `++i`, `i += 2`), the body waits a
/// frame each pass and does not number things with it. A body that is one
/// statement has `body_open` just before it.
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
    reads && moves && waits_a_frame(tokens, body.0, body.1) && !numbers_things(tokens, counter, body.0, body.1)
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
    // `i = 60; do { ...; wait(1); i--; } while (i != 0);`: the counter's
    // start, given in the statement just before its loop.
    let before_loop = start_of.filter(|&left| {
        tokens[left - 1].kind == Kind::Ident && matches!(tokens[left - 2].text, ";" | "{" | "}") && tokens.get(at + 1).is_some_and(|t| t.text == ";")
    });
    if let Some(left) = before_loop {
        let counter = tokens[left - 1].text;
        let limit = limit.min(tokens.len() - 1);
        let text = |index: usize| tokens.get(index).filter(|_| index <= limit).map_or("", |t| t.text);
        let counted = words(counter).iter().all(|word| COUNTER_WORDS.contains(&word.as_str()));
        let (header, body) = match text(at + 2) {
            "do" if text(at + 3) == "{" => {
                let Some(close) = close_of(tokens, at + 3, limit) else { return false };
                let Some(header_close) = (text(close + 1) == "while" && text(close + 2) == "(").then(|| close_of(tokens, close + 2, limit)).flatten() else {
                    return false;
                };
                ((close + 2, header_close), (at + 3, close))
            }
            "while" if text(at + 3) == "(" => {
                let Some(header_close) = close_of(tokens, at + 3, limit) else { return false };
                let Some(close) = (text(header_close + 1) == "{").then(|| close_of(tokens, header_close + 1, limit)).flatten() else { return false };
                ((at + 3, header_close), (header_close + 1, close))
            }
            _ => return false,
        };
        return counted && counts_frames(tokens, counter, header, body);
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

/// Whether the number at `at` is the range of a random roll held in a local
/// first, up to `limit`: `s32 mask = 15; ... dx = mask & Random16();`, `speed
/// = 255; speed &= Random16();`, `mask = 64 - 1; ... (Battle_RandomFar() &
/// mask) == 0`. It is the whole value given to a plain local, and the local's
/// next read is the range of a roll: an operand of `&` or `%` whose other
/// operand rolls, or the local masked by `&=` or `%=` with a roll. A
/// threshold compared with a roll (`(Battle_RandomFar() & 255) < chance`) and
/// an addend of one (`id + Random16() % 3`) are not ranges. A read that only
/// sets it again (`mask = 31;`, a sibling `case`'s) is passed over.
pub fn random_local(tokens: &[Token], at: usize, floor: usize, limit: usize) -> bool {
    let (start, end, left) = span(tokens, at, floor);
    let Some(op) = left.filter(|&left| tokens[left].text == "=" && left >= floor + 2) else { return false };
    let value: Vec<&str> = tokens[start..=end].iter().map(|t| t.text).collect();
    if start != at || !(value.len() == 1 || value[1..] == ["-", "1"]) {
        return false;
    }
    let variable = &tokens[op - 1];
    if variable.kind != Kind::Ident || matches!(tokens[op - 2].text, "." | "->" | "]") {
        return false;
    }
    for index in end + 1..=limit.min(tokens.len() - 1) {
        let token = &tokens[index];
        if token.kind != Kind::Ident || token.text != variable.text || matches!(tokens[index - 1].text, "." | "->") {
            continue;
        }
        let next = tokens.get(index + 1).map_or("", |t| t.text);
        if next == "=" {
            continue;
        }
        let rolls = |first: usize, last: usize| {
            (first..=last).any(|at| tokens[at].kind == Kind::Ident && words(tokens[at].text).iter().any(|word| word == "random"))
        };
        if matches!(next, "&=" | "%=") {
            // `speed &= Random16()`.
            let (_, read_end) = statement(tokens, index, floor);
            return rolls(index + 2, read_end);
        }
        if matches!(next, "&" | "%") {
            // `mask & Random16()`: the operand after it.
            let first = index + 2;
            let Some(token) = tokens.get(first) else { return false };
            let open = if token.text == "(" { Some(first) } else { tokens.get(first + 1).filter(|t| t.text == "(").map(|_| first + 1) };
            let last = match open {
                Some(open) => close_of(tokens, open, limit),
                None => (token.kind == Kind::Ident).then_some(first),
            };
            return last.is_some_and(|last| rolls(first, last));
        }
        let previous = tokens[index - 1].text;
        let binary = index >= floor + 2 && (matches!(tokens[index - 2].kind, Kind::Ident | Kind::Number) || matches!(tokens[index - 2].text, ")" | "]"));
        if matches!(previous, "&" | "%") && binary {
            // `Battle_RandomFar() & mask`: the operand before it.
            let last = index - 2;
            let first = if tokens[last].text == ")" {
                open_of(tokens, last, floor).map(|open| if open > floor && tokens[open - 1].kind == Kind::Ident { open - 1 } else { open })
            } else {
                Some(last)
            };
            return first.is_some_and(|first| rolls(first, last));
        }
        return false;
    }
    false
}

/// The calls a decimal number is reckoned with at expression level, when it
/// is compared with them or offsets them, not a factor or a shift's amount:
/// `FIELD_CELL` of `FIELD_CELL(actor->x.fixed) == 15` and of
/// `FIELD_CELL(x) - 42`, none of `FIELD_CELL(x) + FIELD_CELL(z) * 128`.
pub fn operand_calls(tokens: &[Token], at: usize, floor: usize) -> Vec<String> {
    let shift = at > floor && matches!(tokens[at - 1].text, "<<" | ">>");
    if !is_decimal(tokens[at].text) || shift || factors(tokens, at, floor).0.len() > 1 {
        return Vec::new();
    }
    let (start, end, _) = span(tokens, at, floor);
    // Whether each open bracket hides what it holds: a call's arguments and
    // an index do, a group or a cast does not (`(u32)(FIELD_CELL(x) - 15) > 6`).
    let mut opaque: Vec<bool> = Vec::new();
    let mut calls = Vec::new();
    for index in start..=end {
        match tokens[index].text {
            "(" | "[" => {
                let call = tokens[index].text == "(" && index > start && tokens[index - 1].kind == Kind::Ident;
                if call && !opaque.contains(&true) {
                    calls.push(tokens[index - 1].text.to_string());
                }
                opaque.push(call || tokens[index].text == "[");
            }
            ")" | "]" => {
                opaque.pop();
            }
            _ => {}
        }
    }
    calls
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
/// `draw[0](canvas, picture, 48, 24, 104, 120)` gives `draw` and 2, and
/// whether the name is the type the pointer is cast to:
/// `((DrawRectangle)draw[0])(canvas, sheet, px + 101, ...)` gives
/// `DrawRectangle`, 2 and true.
pub fn pointer_call(tokens: &[Token], at: usize, floor: usize) -> Option<(String, usize, bool)> {
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
                // `name[...](`, `(*name)(` or `((Type)name...)(`.
                let before = index.checked_sub(1)?;
                if tokens[before].text == ")" && before >= floor + 3 && tokens[before - 2].text == "*" && tokens[before - 3].text == "(" {
                    let name = &tokens[before - 1];
                    return (name.kind == Kind::Ident).then(|| (name.text.to_string(), position, false));
                }
                if tokens[before].text == ")" {
                    let open = open_of(tokens, before, floor)?;
                    // `EFFECT_BLITTER(slot)(canvas, ...)`: a macro's result,
                    // called as the type it may cast to.
                    if open > floor && tokens[open - 1].kind == Kind::Ident {
                        return Some((tokens[open - 1].text.to_string(), position, true));
                    }
                    let cast = |offset: usize| tokens.get(open + offset).filter(|_| open + offset < before);
                    if cast(1).is_some_and(|t| t.text == "(") && cast(3).is_some_and(|t| t.text == ")") {
                        let ty = cast(2).filter(|t| t.kind == Kind::Ident)?;
                        return Some((ty.text.to_string(), position, true));
                    }
                    return None;
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
                return Some((name.text.to_string(), position, false));
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
        assert_eq!(kind("gFieldState->transitionFrames = 16;", 0), Some("duration"));
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
        assert_eq!(kind("state->speedX = 24;", 0), None, "a rate");
        assert_eq!(kind("object->scaleX = 12;", 0), None, "a scale");
        assert_eq!(kind("if (GameFlag_Get(0x95a)) {}", 0), None);
        assert_eq!(kind("Event_ShowMessage(27, 0);", 0), None);
        assert_eq!(kind("if (rangePattern == 255) {}", 0), None);
        assert_eq!(kind("CLAMP(screen.depth, 160, 799);", 0), None, "a tuned depth range");
        assert_eq!(kind("fade = -16;", 0), Some("level"));
        assert_eq!(kind("state->shake = i * 2 - 128;", 1), Some("level"));
        assert_eq!(kind("state->shakeFrames = 12;", 0), Some("duration"));
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
        // A counter that numbers things: an index, an argument, an addend of
        // a name; and counters named for a count or an index.
        assert_eq!(kind_of("void F(void) { for (i = 0; i < 20; i++) { Draw(gPartyActors[i]->x); wait(1); } }", "20", 0), None);
        assert_eq!(kind_of("void F(void) { for (i = 0; i < 30; i++) { Spark_Spawn(i); wait(1); } }", "30", 0), None);
        assert_eq!(kind_of("void F(void) { for (k = 0; k < 30; k++) { id = ACTOR_HERB + k; wait(1); } }", "30", 0), None);
        assert_eq!(kind_of("void F(void) { for (count = 0; count < 20; count++) { wait(1); } }", "20", 0), None);
        assert_eq!(kind_of("void F(void) { for (index = 0; index < 20; index++) { wait(1); } }", "20", 0), None);
        assert_eq!(kind_of("void F(void) { for (n = 0; n < 20; n++) wait(1); }", "20", 0), None);
        assert_eq!(kind_of("void F(void) { for (i = 0; i < 20; i++) { x = i - 12; wait(1); } }", "20", 0), Some("duration"), "less a number is still a count of frames");
    }

    #[test]
    fn random_ranges_held_in_a_local() {
        assert_eq!(kind_of("void F(void) { { s32 mask = 15; dx = mask & Random16(); } }", "15", 0), Some("random"));
        assert_eq!(kind_of("void F(void) { s32 speed; speed = 255; speed &= Random16(); speed += 64; }", "255", 0), Some("random"));
        assert_eq!(kind_of("void F(void) { s32 speed; speed = 255; speed &= Random16(); speed += 64; }", "64", 0), None, "an offset after the roll");
        let critical = "void F(void) { s32 mask; switch (e) { case A: mask = 64 - 1; break; default: mask = 4 - 1; } if ((Battle_RandomFar() & mask) == 0) Hit(); }";
        assert_eq!(kind_of(critical, "64", 0), Some("random"), "a sibling case's setting passed over");
        assert_eq!(kind_of("void F(void) { s32 spread = 15; x = INTtoFIXED((Random16() & spread) - 8); }", "15", 0), Some("random"));
        assert_eq!(kind_of("void F(void) { s32 size = 10; Draw(size); x = Random16(); }", "10", 0), None, "read without a roll");
        assert_eq!(kind_of("void F(void) { a->mask = 15; x = a->mask & Random16(); }", "15", 0), None, "no plain local");
        assert_eq!(kind_of("void F(void) { s32 m = 15 + n; x = m & Random16(); }", "15", 0), None, "not its whole value");
        // A threshold compared with a roll, which The Lost Age names
        // (CHANCE_BLOW), and an addend of a roll.
        assert_eq!(kind_of("void F(void) { s32 chance; chance = 32; if ((Battle_RandomFar() & 255) < chance) Hit(); }", "32", 0), None);
        assert_eq!(kind_of("void F(void) { s32 id = 112; Message_Show(id + Random16() % 3); }", "112", 0), None);
        assert_eq!(kind_of("void F(void) { s32 m = 12; x = (m & y) + Random16(); }", "12", 0), None, "masking something else");
        assert_eq!(kind_of("void F(void) { s32 n = 90; x = Random16() % n; }", "90", 0), Some("random"));
    }

    #[test]
    fn percentages_and_composed_fractions() {
        // A drag's divisor is to name (PARTICLE_DRAG_X): S3 keeps a decay only
        // as the composed factor `FIXED_ONE * n / d`.
        assert_eq!(kind_of("void F(void) { effect->velocity.x -= effect->velocity.x / 18; }", "18", 0), None);
        assert_eq!(kind_of("void F(void) { p->velocityX = velocityX - velocityX / 256; }", "256", 0), None);
        assert_eq!(kind_of("void F(void) { share = gState.hazardSteps * 100 / gState.hazardLimit; }", "100", 0), Some("decimal"));
        assert_eq!(kind_of("void F(void) { share = steps * 100; }", "100", 0), None, "no division");
        assert_eq!(kind_of("void F(void) { maxHpPercent = 12; }", "12", 0), Some("decimal"));
        assert_eq!(kind_of("void F(void) { c = digit + 'A' - 10; }", "10", 0), Some("decimal"));
        assert_eq!(kind_of("void F(void) { e.scaleX = Q_8_8(11.0 / 16); }", "16", 0), Some("composed"));
        assert_eq!(kind_of("void F(void) { scale = Q_8_8(1) / 16; }", "16", 0), Some("composed"));
        assert_eq!(kind_of("void F(void) { Sound_SetBgmTempo(SOUND_TEMPO_SCALE_NORMAL * 15 / 16); }", "15", 0), Some("composed"));
        assert_eq!(kind_of("void F(void) { v = INTtoFIXED(i / 3 * 32) + w; }", "32", 0), None, "a factor, not the fraction's divisor");
        assert_eq!(kind_of("void F(void) { v = INTtoFIXED(i / 3 * 32) + w; }", "3", 0), None, "no fraction of two numbers");
        assert_eq!(kind_of("void F(void) { pos.y = INTtoFIXED(i / 3 * 32) + z; }", "32", 0), Some("position"), "the place it is given to");
        assert_eq!(kind_of("void F(void) { x = AFFINE_SCALE_ONE * 15 / 16; }", "15", 0), Some("composed"));
        // Semitones in an octave, and a scale's constant that is not its one.
        let tempo = "void F(void) { pitch = (s16)gBgmTempoPitch * 12 - 12 * BGM_UNITY_SCALE; }";
        assert_eq!(kind_of(tempo, "12", 0), None);
        assert_eq!(kind_of(tempo, "12", 1), None, "no divisor");
        assert_eq!(kind_of("void F(void) { v = BOUNCE_SCALE_STEPS * 12 / 16; }", "12", 0), None, "not a scale's one");
    }

    #[test]
    fn levels_and_one_letter_words() {
        assert_eq!(kind_of("void F(void) { if (blue > 26) blue = 26; }", "26", 0), Some("level"));
        assert_eq!(kind_of("void F(void) { model->shade = -16; }", "16", 0), Some("level"));
        assert_eq!(kind_of("void F(void) { brightnessDelta = -12; }", "12", 0), Some("level"));
        assert_eq!(kind_of("void F(void) { red += 22; }", "22", 0), Some("level"));
        assert_eq!(kind_of("void F(void) { shade = shade - i * 4 + 40; }", "40", 0), Some("level"), "an addend of it");
        assert_eq!(kind_of("void F(void) { if (red + green + blue > 60) {} }", "60", 0), Some("level"));
        assert_eq!(kind_of("void F(void) { if (id == ACTOR_RED_COGS + 12) {} }", "12", 0), None, "a constant's word names a thing");
        // The ramp's last palette index, its 64 entries, an RGB555 field's
        // factor, a shade's floor reckoned from a frame, a phase, and a
        // number given to something else.
        assert_eq!(kind_of("void F(void) { *pixel = 63 - red; }", "63", 0), None);
        assert_eq!(kind_of("void F(void) { red = (redOffset + 16) * index / 64; }", "64", 0), None);
        assert_eq!(kind_of("void F(void) { s32 green = grey * 32; }", "32", 0), None);
        assert_eq!(kind_of("void F(void) { shade = t * 4 - 64; }", "64", 0), None);
        assert_eq!(kind_of("void F(void) { redPhase = 40; }", "40", 0), None);
        assert_eq!(kind_of("void F(void) { x2 = blue + 12; }", "12", 0), Some("position"), "the target's own words");
        assert_eq!(kind_of("void F(void) { v = blue + 12; }", "12", 0), None);
        assert_eq!(kind_of("void F(void) { if (Get(blue) > 26) {} }", "26", 0), None, "an argument says nothing");
        assert_eq!(kind_of("void F(void) { if (logEColumn == 24) {} }", "24", 0), Some("position"));
        assert_eq!(kind_of("void F(void) { if (logJRow == 15) {} }", "15", 0), Some("position"));
        assert_eq!(kind_of("void F(void) { if (gVBlankCount % 80 == 0) {} }", "80", 0), Some("duration"), "VBlank is one word");
        assert_eq!(words("logEColumn"), ["log", "ecolumn"], "words() keeps a word in mixed case whole");
    }

    #[test]
    fn masks_of_whole_fields() {
        let mask = |code: &str| {
            let hex = code.find("0x").expect("a hex number");
            kind_of(&format!("void F(void) {{ {code} }}"), code[hex..].split(|c: char| !c.is_ascii_alphanumeric()).next().unwrap(), 0)
        };
        assert_eq!(mask("v = (step & 0xff) * GUST_LAP_STEP;"), Some("mask"), "a value's low bits");
        assert_eq!(mask("v = Trig_Sin((gFrameCount & 0x1ff) * FACING_STEP);"), Some("mask"));
        assert_eq!(mask("*cell &= 0xfff0;"), Some("mask"), "whole hex digits");
        assert_eq!(mask("bits &= 0x00FF00FF;"), Some("mask"));
        assert_eq!(mask("x = 0x0ffff000 & draw;"), Some("mask"), "on the left of the &");
        assert_eq!(mask("x &= ~0xff;"), Some("mask"), "its complement cleared");
        assert_eq!(mask("u32 byteMask = 0xff;"), Some("mask"), "held in a variable named for a mask");
        // A bit, two bits, a field cleared by a complement written out, a bit
        // set, and a hex value given to something else.
        assert_eq!(mask("if (flags & 0x8000) {}"), None);
        assert_eq!(mask("angle = (facing + DEGtoANG(45)) & 0xc000;"), None);
        assert_eq!(mask("*cell &= 0xfcff;"), None);
        assert_eq!(mask("x = y | 0xff;"), None);
        assert_eq!(mask("u32 glyphMask = 0x80;"), None, "one bit, named or not");
        assert_eq!(mask("x = 0xff;"), None);
    }

    #[test]
    fn the_sign_bit() {
        assert_eq!(kind("blit[(u32)drift >> 31](canvas);", 0), Some("sign"));
        assert_eq!(kind("isActive >>= 31;", 0), Some("sign"));
        assert_eq!(kind("half = (b + (s32)((u32)b >> 31)) >> 1;", 0), Some("sign"));
        // FIXEDtoINT's shift, another bit, and the bit set by a left shift.
        assert_eq!(kind("x = y >> 16;", 0), None);
        assert_eq!(kind("x = y >> 30;", 0), None);
        assert_eq!(kind("x = y << 31;", 0), None);
    }

    #[test]
    fn decimal_digits() {
        assert_eq!(kind_of("void F(void) { digit /= 10; }", "10", 0), Some("decimal"), "the radix");
        assert_eq!(kind_of("void F(void) { digit %= 10; }", "10", 0), Some("decimal"));
        assert_eq!(kind_of("void F(void) { v = Mul(v, RECIPROCAL_OF(10)); }", "10", 0), Some("decimal"));
        let text = "void F(void) { q = Mul(v, RECIPROCAL_OF(10)); v -= q * 10; s[i] = v + '0'; }";
        assert_eq!(kind_of(text, "10", 1), Some("decimal"), "a quotient multiplied back where digits are written");
        let ladder = "void F(void) { if (winnings < 10) d = 1; else if (winnings < 100) d = 2; }";
        assert_eq!(kind_of(ladder, "10", 0), Some("decimal"), "place values");
        assert_eq!(kind_of(ladder, "100", 0), Some("decimal"));
        assert_eq!(kind_of("void F(void) { for (;;) { n /= 10; if (n < 10) break; } }", "10", 1), Some("decimal"), "a name divided by ten");
        // A lone threshold, a product where no digit is written, and a
        // divisor that is no power of ten.
        assert_eq!(kind_of("void F(void) { if (hp < 100) Heal(); }", "100", 0), None);
        assert_eq!(kind_of("void F(void) { v = n * 10; w = n / 10; }", "10", 0), None);
        assert_eq!(kind_of("void F(void) { digit /= 16; }", "16", 0), None);
    }

    #[test]
    fn through_groups_and_conversions_to_what_a_number_is_given() {
        assert_eq!(kind_of("void F(void) { y = -(i * spacing + 16); }", "16", 0), Some("position"));
        assert_eq!(kind_of("void F(void) { rock->y = INTtoFIXED(i * 2 + 96); }", "96", 0), Some("position"));
        assert_eq!(kind_of("void F(void) { v = FIXEDtoINT(c * PUFF_RADIUS) + 40; }", "40", 0), Some("position"), "a conversion's own names");
        assert_eq!(kind_of("void F(void) { v = INTtoFIXED(frame - 16); }", "16", 0), Some("duration"), "its own operand's names first");
        assert_eq!(kind_of("void F(void) { menu->memberX[0] = 16; }", "16", 0), Some("position"), "a target ending in an index");
        assert_eq!(kind_of("void F(void) { y = i == 0 ? 8 : 40; }", "40", 0), Some("position"), "a conditional's arm");
        assert_eq!(kind_of("void F(void) { y = i == 0 ? 40 : 8; }", "40", 0), Some("position"));
        assert_eq!(kind_of("void F(void) { v = i == 0 ? 8 : 40; }", "40", 0), None, "given to nothing that says");
        assert_eq!(kind_of("void F(void) { scroll.rows = 11; }", "11", 0), Some("position"), "a size in rows");
        assert_eq!(kind_of("void F(void) { scroll.limit = 176; }", "176", 0), None);
        // A group holding more than the number's operand, a shift's amount,
        // a roll outside the number's own operand, and an index's name.
        assert_eq!(kind_of("void F(void) { if ((cell >= 36 && column >= 22)) {} }", "36", 0), None);
        assert_eq!(kind_of("void F(void) { y = (Random16() & 63) - (1 << (16 - n)); }", "16", 0), None);
        assert_eq!(kind_of("void F(void) { cues[row] += 10; }", "10", 0), None);
    }

    #[test]
    fn a_frame_loops_counter_started_before_it() {
        assert_eq!(kind_of("void F(void) { i = 60; do { wait(1); i--; } while (i != 0); }", "60", 0), Some("duration"));
        assert_eq!(kind_of("void F(void) { i = 60; while (i != 0) { i--; wait(1); } }", "60", 0), Some("duration"));
        assert_eq!(kind_of("void F(void) { i = 60; do { x++; i--; } while (i != 0); }", "60", 0), None, "no wait");
        assert_eq!(kind_of("void F(void) { i = 60; Foo(); do { wait(1); i--; } while (i != 0); }", "60", 0), None, "not just before its loop");
        assert_eq!(kind_of("void F(void) { count = 60; do { wait(1); count--; } while (count != 0); }", "60", 0), None, "a counter named for a count");
    }

    #[test]
    fn packed_bytes_and_bit_fields() {
        let unpack = "void F(void) { levels = in[0] | in[1] << 8 | in[2] << 16 | (in[3] & 0xf0) << 20; stats = in[k] << 24 | in[k + 13] << 12; }";
        assert_eq!(kind_of(unpack, "16", 0), Some("packing"), "an element joined to others by |");
        assert_eq!(kind_of(unpack, "20", 0), Some("packing"), "a field of an element");
        assert_eq!(kind_of(unpack, "24", 0), Some("packing"));
        assert_eq!(kind_of(unpack, "13", 0), Some("packing"), "a byte's index in a loaded element");
        let pack = "void F(void) { out[k] = stats >> 24; out[k + 1] = stats >> 16; out[k + 11] = stats << 4; }";
        assert_eq!(kind_of(pack, "24", 0), Some("packing"), "stored into an array filled with two or more amounts");
        assert_eq!(kind_of(pack, "16", 0), Some("packing"));
        assert_eq!(kind_of(pack, "11", 0), Some("packing"), "a byte's index in a stored element");
        assert_eq!(kind_of("void F(void) { v = RGB(21, 10, 21) << 16 | RGB(21, 10, 21); }", "16", 0), Some("packing"), "two halves joined");
        assert_eq!(kind_of("void F(void) { key = ((u32)(u16)count >> 16) | KEY_B; }", "16", 0), Some("packing"), "out through a group");
        assert_eq!(kind_of("void F(void) { return (seed << 8) >> 16; }", "16", 0), Some("packing"), "a field cut by two shifts");
        assert_eq!(kind_of("void F(void) { x = (u32)(a - b) << 23 >> 7; }", "23", 0), Some("packing"));
        // A shift stored alone may be a 16.16 conversion; a right shift then
        // a left one is a scale; an amount off a nibble; a lone byte array.
        assert_eq!(kind_of("void F(void) { matrix->rows[1] = (u32)reciprocal << 16; }", "16", 0), None);
        assert_eq!(kind_of("void F(void) { x = (u32)x >> 16; }", "16", 0), None);
        assert_eq!(kind_of("void F(void) { c[o + ((y >> 3) << 10)] = 0; }", "10", 0), None);
        assert_eq!(kind_of("void F(void) { v = a << 10 | b; }", "10", 0), None);
        assert_eq!(kind_of("void F(void) { out[12] = stats >> 16; }", "12", 0), None, "one amount");
    }

    #[test]
    fn places_digit_caps_bytes_and_scaled_products() {
        // A place, a slant's start and a scroll's limit, in pixels or cells.
        assert_eq!(kind_of("void F(void) { if (position[0] == 16 && position[2] == 36) {} }", "36", 0), Some("position"));
        assert_eq!(kind_of("void F(void) { for (i = 0, y = -128, slant = -16; i != N; i++) {} }", "16", 0), Some("position"));
        // A cap of decimal digits; any other bound stays.
        assert_eq!(kind_of("void F(void) { if (magnitude > 99) {} }", "99", 0), Some("decimal"));
        assert_eq!(kind_of("void F(void) { if (magnitude > 98) {} }", "98", 0), None);
        assert_eq!(kind_of("void F(void) { magnitude = 99; }", "99", 0), None, "only a bound");
        // A packed value's byte by division or remainder.
        assert_eq!(kind_of("void F(void) { s32 item = (u16)((s32)trigger->control / 256); }", "256", 0), Some("packing"));
        assert_eq!(kind_of("void F(void) { ok = out[k] == (u16)crc % 256; }", "256", 0), Some("packing"));
        assert_eq!(kind_of("void F(void) { v = a * 256; }", "256", 0), None);
        assert_eq!(kind_of("void F(void) { v = a / 256; }", "256", 0), None, "a fraction, no cast byte");
        // A 16.16 product or a sine scaled to its unit.
        assert_eq!(kind_of("void F(void) { *line = (Iwram_MulFX(Trig_Sin(a), b) >> 11) - 4; }", "11", 0), Some("scaling"));
        assert_eq!(kind_of("void F(void) { y = (Trig_Cos(angle + TRIG_TURN * 13 / 32) >> 14) - 2; }", "14", 0), Some("scaling"));
        assert_eq!(kind_of("void F(void) { y = (Other(angle) >> 14) - 2; }", "14", 0), None);
        assert_eq!(kind_of("void F(void) { y = (angle >> 14) - 2; }", "14", 0), None);
    }

    #[test]
    fn the_cadence_of_a_loop_of_frames() {
        let shower = "void F(void) { for (i = 0; i != PUFF_SHOWER_FRAMES; i++) { if (i % 64 == 0) Sound(); if (i % 16) continue; { Spawn(); wait(1); } } }";
        assert_eq!(kind_of(shower, "64", 0), Some("duration"), "a loop that need not wait every pass, its bound frames");
        assert_eq!(kind_of(shower, "16", 0), Some("duration"));
        let blast = "void F(void) { for (i = 0; i < LOHO_BLAST_FRAMES; i++) { if ((i & 31) == 0) { Crash(); } Draw(); } }";
        assert_eq!(kind_of(blast, "31", 0), Some("duration"), "a mask one less than a power of two");
        assert_eq!(kind_of(&blast.replace("31", "30"), "30", 0), None);
        assert_eq!(kind_of(&blast.replace("LOHO_BLAST_FRAMES", "SPARK_COUNT"), "31", 0), None, "a loop over things");
        assert_eq!(kind_of(&shower.replace("i % 64", "slot % 64"), "64", 0), None, "another name's");
    }

    #[test]
    fn indices_and_strides_continuing_a_template() {
        let code = "void F(void) { PUT(src[0]); PUT(src[1]); PUT(src[2]); PUT(src[3]); PUT(src[4]); PUT(src[5]); PUT(src[6]); PUT(src[7]); PUT(src[8]); if (a) { PUT(src[9]); } else { PUT(src[10]); } PUT(src[11] + b); src += 12; x = gTable[12]; }";
        let tokens: Vec<Token> = clex::lex(code).into_iter().filter(|t| t.kind != Kind::Comment).collect();
        let open = tokens.iter().position(|t| t.text == "{").unwrap();
        let at = |text: &str, n: usize| tokens.iter().enumerate().filter(|(_, t)| t.text == text).nth(n).unwrap().0;
        let limit = tokens.len() - 1;
        assert!(continues_indices(&tokens, at("10", 0), open, limit) && continues_indices(&tokens, at("11", 0), open, limit));
        assert!(continues_indices(&tokens, at("12", 0), open, limit), "the stride past src[11]");
        assert!(!continues_indices(&tokens, at("12", 1), open, limit), "another array's index");
        let gap = code.replace("PUT(src[5]); ", "");
        let tokens: Vec<Token> = clex::lex(&gap).into_iter().filter(|t| t.kind != Kind::Comment).collect();
        let ten = tokens.iter().position(|t| t.text == "10").unwrap();
        assert!(!continues_indices(&tokens, ten, open, tokens.len() - 1), "src[5] is missing");
    }

    #[test]
    fn arrays_filled_in_order_and_their_weights() {
        let fill = "void F(void) { w.prizeWeights[0] = 4; w.prizeWeights[1] = 4; x = 0; w.prizeWeights[2] = 16; w.prizeWeights[3] = 16; w.prizeWeights[4] = 16; w.prizeWeights[5] = 16; w.prizeWeights[6] = 16; w.prizeWeights[7] = 32; w.prizeWeights[8] = 32; w.prizeWeights[9] = 32; w.prizeWeights[10] = 64; }";
        assert_eq!(kind_of(fill, "10", 0), Some("index"), "every index from 0 to it set");
        assert_eq!(kind_of(fill, "64", 0), Some("random"), "a weight");
        assert_eq!(kind_of(fill, "16", 0), Some("random"));
        let heights = "void F(void) { h[0] = L; h[1] = L; h[2] = L; h[3] = L; h[4] = L; h[5] = L; h[6] = L; h[7] = L; h[8] = L; h[9] = L; h[10] = L; h[11] = 12; }";
        assert_eq!(kind_of(heights, "11", 0), Some("index"));
        assert_eq!(kind_of(heights, "12", 0), None, "not an array of weights");
        assert_eq!(kind_of("void F(void) { h[0] = L; h[10] = L; }", "10", 0), None, "indices skipped");
        assert_eq!(kind_of("void F(void) { x = h[10]; }", "10", 0), None, "read, not filled");
    }

    #[test]
    fn a_counter_moved_each_pass_and_its_cases() {
        let rise = "static s32 sStep; void F(void) { switch (sStep) { case 16: A(); break; case 24: sStep--; break; } sStep++; }";
        assert_eq!(kind_of(rise, "16", 0), Some("duration"));
        let reload = "void F(P *p) { if (p->steps == 0) return; switch (p->steps) { case RELOAD_STEPS: A(); break; case 11: B(); break; } p->steps--; }";
        assert_eq!(kind_of(reload, "11", 0), Some("duration"), "a countdown");
        let tutorial = "void F(W *w) { switch (w->step) { case 16: w->step = 21; break; case 21: break; } w->step++; }";
        assert_eq!(kind_of(tutorial, "16", 0), None, "a step machine that jumps to a numbered step");
        let machine = "void F(W *w) { switch (w->step) { case 16: w->step++; break; } }";
        assert_eq!(kind_of(machine, "16", 0), None, "moved inside its own cases only");
    }

    #[test]
    fn products_of_numbers_degrees_and_fractions_of_a_named_quantity() {
        assert_eq!(kind_of("void F(void) { p = (Buffer + 32 * 1024)[i]; }", "1024", 0), Some("composed"));
        assert_eq!(kind_of("void F(void) { table -= ATAN_ENTRIES / 16; }", "16", 0), Some("composed"));
        assert_eq!(kind_of("void F(void) { v = value / 16; }", "16", 0), None, "a variable's");
        assert_eq!(kind_of("void F(void) { v = w->LIMIT / 16; }", "16", 0), None, "a member");
        assert_eq!(kind_of("void F(void) { v = COUNT(x) / 16; }", "16", 0), None, "a call's");
        assert_eq!(kind_of("void F(void) { if (beat == 32) {} }", "32", 0), Some("duration"), "a beat of a few frames");
    }

    #[test]
    fn whole_values_returned_and_steps_of_a_route() {
        let returned = |code: &str, number: &str| {
            let tokens: Vec<Token> = clex::lex(code).into_iter().filter(|t| t.kind != Kind::Comment).collect();
            let at = tokens.iter().position(|t| t.text == number).expect("the number");
            let open = tokens.iter().position(|t| t.text == "{").unwrap();
            returned_whole(&tokens, at, open, tokens.len() - 1)
        };
        let rate = "s32 F(s32 e) { s32 fixed; switch (e) { case A: return 70; case B: fixed = 60; break; } return -fixed; }";
        assert!(returned(rate, "70"));
        assert!(returned(rate, "60"), "a local returned");
        assert!(!returned("s32 F(void) { s32 x; x = 60; return x + 1; }", "60"));
        assert!(!returned("s32 F(void) { return 60 + x; }", "60"));
        let step = |code: &str, number: &str| {
            let tokens: Vec<Token> = clex::lex(code).into_iter().filter(|t| t.kind != Kind::Comment).collect();
            let at = tokens.iter().position(|t| t.text == number).expect("the number");
            route_step(&tokens, at, 0, tokens.len() - 1, &|name| name == "WALK")
        };
        let route = "void F(u8 *state) { switch (*state) { WALK(16, 232, 744) case 17: if (A()) *state = 18; break; } }";
        assert!(step(route, "17"));
        assert!(step(route, "18"), "a step given to the switch's subject");
        assert!(!step("void F(u8 *state) { switch (*state) { case 17: *state = 18; break; } }", "17"), "no step macro writes this switch");
        assert!(!step("void F(u8 *state) { switch (*state) { WALK(1, 2, 3) case 17: x = 18; break; } }", "18"), "given to something else");
    }

    #[test]
    fn the_brightness_registers_level() {
        assert_eq!(kind_of("void F(void) { REG_BLDY = 12; }", "12", 0), Some("level"));
        assert_eq!(kind_of("void F(void) { REG_BLDALPHA = 12; }", "12", 0), None, "a blend's two coefficients");
    }

    #[test]
    fn calls_a_number_is_reckoned_with() {
        let calls = |code: &str, number: &str| {
            let tokens: Vec<Token> = clex::lex(code).into_iter().filter(|t| t.kind != Kind::Comment).collect();
            let at = tokens.iter().position(|t| t.text == number).expect("the number");
            operand_calls(&tokens, at, 0)
        };
        assert_eq!(calls("if (FIELD_CELL(a->x.fixed) == 15) {}", "15"), ["FIELD_CELL"]);
        assert_eq!(calls("Set(FIELD_CELL(a->x.fixed) - 42, 0);", "42"), ["FIELD_CELL"]);
        assert_eq!(calls("if ((u32)(FIELD_CELL(x) - 15) > 16) {}", "16"), ["FIELD_CELL"], "through a cast and a group");
        assert!(calls("i = FIELD_CELL(x) + FIELD_CELL(z) * 128;", "128").is_empty(), "a factor");
        assert!(calls("if (F(G(x) == 15)) {}", "15") == ["G"], "the call holding it is no operand");
        assert!(calls("if (FIELD_CELL(x) << 12) {}", "12").is_empty(), "a shift's amount");
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
        assert_eq!(pointer_call(&tokens, at, 0), Some(("draw".to_string(), 2, false)));
        let tokens: Vec<Token> = clex::lex("(*draw)(canvas, cells, sway, 64, 96, 52);").into_iter().collect();
        let at = tokens.iter().position(|t| t.text == "96").unwrap();
        assert_eq!(pointer_call(&tokens, at, 0), Some(("draw".to_string(), 4, false)));
        let tokens: Vec<Token> = clex::lex("((DrawRectangle)draw[0])(canvas, sheet, px + 101, py + 26);").into_iter().collect();
        let at = tokens.iter().position(|t| t.text == "26").unwrap();
        assert_eq!(pointer_call(&tokens, at, 0), Some(("DrawRectangle".to_string(), 3, true)), "the type it is cast to");
        let tokens: Vec<Token> = clex::lex("Get(x)(canvas, 26);").into_iter().collect();
        let at = tokens.iter().position(|t| t.text == "26").unwrap();
        assert_eq!(pointer_call(&tokens, at, 0), Some(("Get".to_string(), 1, true)), "a macro's result, called as the type it may cast to");
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
