//! A small C lexer for counting what a decompilation's source holds:
//! identifiers, numbers, strings and punctuation with their lines, comments
//! kept with their text, and each preprocessor directive bracketed by its own
//! start and end tokens. It never expands a macro or follows an include.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ident,
    Number,
    Str,
    Char,
    Punct,
    Comment,
    /// The `#` that opens a directive at the start of a line.
    PpStart,
    /// The end of a directive's logical line.
    PpEnd,
}

#[derive(Clone, Debug)]
pub struct Token<'a> {
    pub kind: Kind,
    pub text: &'a str,
    pub line: usize,
    /// Inside a directive.
    pub pp: bool,
    /// Written right after the previous token, with no space between.
    pub glued: bool,
}

/// `///`, `//!`, `/**` or `/*!`, and not a rule of slashes or stars.
pub fn is_doc(comment: &str) -> bool {
    (comment.starts_with("///") && !comment.starts_with("////"))
        || comment.starts_with("//!")
        || (comment.starts_with("/**") && !comment.starts_with("/***") && comment != "/**/")
        || comment.starts_with("/*!")
}

/// A trailing documentation note, `///<` or `/**<`.
pub fn is_trailing_doc(comment: &str) -> bool {
    comment.starts_with("///<") || comment.starts_with("/**<") || comment.starts_with("//!<")
}

pub fn lex(text: &str) -> Vec<Token<'_>> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let (mut at, mut line) = (0, 1);
    let mut in_pp = false;
    // Only whitespace since the last newline: a `#` here opens a directive.
    let mut line_start = true;
    let mut last_end = usize::MAX;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte == b'\\' && bytes.get(at + 1) == Some(&b'\n') {
            at += 2;
            line += 1;
            continue;
        }
        if byte == b'\\' && bytes.get(at + 1) == Some(&b'\r') && bytes.get(at + 2) == Some(&b'\n') {
            at += 3;
            line += 1;
            continue;
        }
        if byte == b'\n' {
            if in_pp {
                tokens.push(Token {
                    kind: Kind::PpEnd,
                    text: "",
                    line,
                    pp: true,
                    glued: false,
                });
                in_pp = false;
            }
            line += 1;
            at += 1;
            line_start = true;
            continue;
        }
        if byte.is_ascii_whitespace() {
            at += 1;
            continue;
        }
        let start = at;
        let first_line = line;
        let kind = if byte == b'/' && bytes.get(at + 1) == Some(&b'/') {
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            Kind::Comment
        } else if byte == b'/' && bytes.get(at + 1) == Some(&b'*') {
            at += 2;
            while at < bytes.len() && !(bytes[at] == b'*' && bytes.get(at + 1) == Some(&b'/')) {
                if bytes[at] == b'\n' {
                    line += 1;
                }
                at += 1;
            }
            at = (at + 2).min(bytes.len());
            Kind::Comment
        } else if byte == b'#' && line_start && !in_pp {
            in_pp = true;
            at += 1;
            Kind::PpStart
        } else if byte == b'"' || byte == b'\'' {
            at += 1;
            while at < bytes.len() && bytes[at] != byte && bytes[at] != b'\n' {
                at += if bytes[at] == b'\\' { 2 } else { 1 };
            }
            // The closing quote; an unterminated literal ends at its line's end.
            if bytes.get(at) == Some(&byte) {
                at += 1;
            }
            at = at.min(bytes.len());
            if byte == b'"' {
                Kind::Str
            } else {
                Kind::Char
            }
        } else if byte.is_ascii_digit()
            || (byte == b'.' && bytes.get(at + 1).is_some_and(u8::is_ascii_digit))
        {
            at += 1;
            // An exponent's sign: after e in a decimal number, after p in a hex one.
            let hex = matches!(bytes.get(start + 1), Some(b'x' | b'X')) && byte == b'0';
            let exponent: &[u8] = if hex { b"pP" } else { b"eE" };
            while at < bytes.len() {
                let next = bytes[at];
                let sign = (next == b'+' || next == b'-') && exponent.contains(&bytes[at - 1]);
                if !(sign || next.is_ascii_alphanumeric() || next == b'_' || next == b'.') {
                    break;
                }
                at += 1;
            }
            Kind::Number
        } else if byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$' {
            while at < bytes.len()
                && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_' || bytes[at] == b'$')
            {
                at += 1;
            }
            Kind::Ident
        } else {
            const THREE: [&[u8]; 3] = [b"...", b"<<=", b">>="];
            const TWO: [&[u8]; 20] = [
                b"->", b"++", b"--", b"<<", b">>", b"<=", b">=", b"==", b"!=", b"&&", b"||", b"+=",
                b"-=", b"*=", b"/=", b"%=", b"&=", b"|=", b"^=", b"##",
            ];
            let rest = &bytes[at..];
            at += if THREE.iter().any(|op| rest.starts_with(op)) {
                3
            } else if TWO.iter().any(|op| rest.starts_with(op)) {
                2
            } else {
                // One whole character, even outside ASCII.
                text[at..].chars().next().map_or(1, char::len_utf8)
            };
            Kind::Punct
        };
        line_start = false;
        tokens.push(Token {
            kind,
            text: &text[start..at],
            line: first_line,
            pp: in_pp,
            glued: last_end == start,
        });
        last_end = at;
    }
    if in_pp {
        tokens.push(Token {
            kind: Kind::PpEnd,
            text: "",
            line,
            pp: true,
            glued: false,
        });
    }
    tokens
}

/// The routine or definition each token is in, by its place: a function's
/// name for its body, a `#define`'s name for its directive (a function-like
/// macro's body included), an enumerator's name for its value, and a
/// global's name for its initializer (`gTable` of `const u8 gTable[] = {...}`).
/// Anything else, a function's parameter list and a struct's members
/// among them, is in none.
pub fn owners<'a>(tokens: &[Token<'a>]) -> Vec<Option<&'a str>> {
    /// Where the reading stands: braces deep and what the outermost pair
    /// belongs to; the top-level statement's declarator, whether `=` gave
    /// it a value, whether it is an enum's, and how deep its parentheses
    /// and brackets are; and the last token that is no comment.
    #[derive(Clone, Default)]
    struct State<'a> {
        depth: usize,
        owner: Option<&'a str>,
        in_enum: bool,
        enumerator: Option<&'a str>,
        expect: bool,
        declarator: Option<&'a str>,
        assigned: bool,
        is_enum: bool,
        nested: usize,
        previous: Option<usize>,
    }
    let mut out = vec![None; tokens.len()];
    let mut state = State::default();
    // Each `#if`'s state, and the state its first branch ended in: the
    // branches after it are read from where it started, and the reading
    // goes on from where the first ended.
    let mut conditions: Vec<(State, Option<State>)> = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        let token = &tokens[at];
        if token.kind == Kind::PpStart {
            let end = (at..tokens.len())
                .find(|&end| tokens[end].kind == Kind::PpEnd)
                .unwrap_or(tokens.len() - 1);
            let words: Vec<&Token> = tokens[at + 1..end]
                .iter()
                .filter(|t| t.kind != Kind::Comment)
                .collect();
            let name = match words[..] {
                [define, name, ..] if define.text == "define" && name.kind == Kind::Ident => {
                    Some(name.text)
                }
                _ => None,
            };
            match words.first().map(|word| word.text) {
                Some("if" | "ifdef" | "ifndef") => conditions.push((state.clone(), None)),
                Some("elif" | "else") => {
                    if let Some((start, first)) = conditions.last_mut() {
                        first.get_or_insert_with(|| state.clone());
                        state = start.clone();
                    }
                }
                Some("endif") => {
                    if let Some((_, Some(first))) = conditions.pop() {
                        state = first;
                    }
                }
                _ => {}
            }
            out[at..=end].fill(name);
            at = end + 1;
            continue;
        }
        if token.kind == Kind::Comment {
            at += 1;
            continue;
        }
        let s = &mut state;
        if s.depth == 0 {
            match token.text {
                "{" => {
                    s.owner = None;
                    if let Some(previous) = s.previous.filter(|&p| tokens[p].text == ")") {
                        // A function's body: the name before its parameters.
                        let (mut parens, mut index) = (0, previous);
                        loop {
                            match tokens[index].text {
                                ")" => parens += 1,
                                "(" => parens -= 1,
                                _ => {}
                            }
                            if parens == 0 || index == 0 {
                                break;
                            }
                            index -= 1;
                        }
                        s.owner = index
                            .checked_sub(1)
                            .map(|i| &tokens[i])
                            .filter(|t| t.kind == Kind::Ident)
                            .map(|t| t.text);
                    } else if s.assigned {
                        s.owner = s.declarator;
                    } else if s.is_enum {
                        (s.in_enum, s.expect) = (true, true);
                    }
                    s.depth = 1;
                }
                ";" => (s.declarator, s.assigned, s.is_enum, s.nested) = (None, false, false, 0),
                "(" | "[" => s.nested += 1,
                ")" | "]" => s.nested = s.nested.saturating_sub(1),
                "=" if s.nested == 0 => s.assigned = true,
                "enum" => s.is_enum = true,
                _ if token.kind == Kind::Ident && s.nested == 0 && !s.assigned => {
                    s.declarator = Some(token.text)
                }
                _ => {}
            }
            if s.assigned && token.text != "=" && token.text != ";" {
                out[at] = s.declarator;
            }
        } else {
            match token.text {
                "{" => s.depth += 1,
                "}" => s.depth -= 1,
                "," if s.in_enum && s.depth == 1 => s.expect = true,
                _ if s.in_enum && s.expect && token.kind == Kind::Ident => {
                    (s.enumerator, s.expect) = (Some(token.text), false)
                }
                _ => {}
            }
            if s.depth == 0 {
                // A function's body ends its statement; an initializer's or
                // an enum's goes on to its `;`.
                if !s.assigned && !s.is_enum {
                    (s.declarator, s.nested) = (None, 0);
                }
                (s.owner, s.in_enum, s.enumerator, s.expect) = (None, false, None, false);
            } else {
                out[at] = if s.in_enum { s.enumerator } else { s.owner };
            }
        }
        s.previous = Some(at);
        at += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directives_comments_and_numbers() {
        let tokens = lex("#define A(x) \\\n  ((x) + 0x10u) /// note\nint b = 1.5e+3; // c\n");
        let kinds: Vec<_> = tokens.iter().map(|t| (t.kind, t.text, t.pp)).collect();
        assert_eq!(kinds[0], (Kind::PpStart, "#", true));
        assert_eq!(kinds[1], (Kind::Ident, "define", true));
        assert!(
            tokens[3].glued,
            "a function-like macro's parenthesis is glued to its name"
        );
        assert!(kinds.contains(&(Kind::Number, "0x10u", true)));
        assert!(kinds.contains(&(Kind::Comment, "/// note", true)));
        assert!(kinds.contains(&(Kind::PpEnd, "", true)));
        assert!(kinds.contains(&(Kind::Number, "1.5e+3", false)));
        // In a hex number e is a digit: the sign after it is an operator.
        let texts: Vec<_> = lex("0x1e+1 'a\n").iter().map(|t| t.text).collect();
        assert_eq!(texts, ["0x1e", "+", "1", "'a"]);
        assert_eq!(tokens.last().unwrap().line, 3);
        assert!(
            is_doc("/// x") && !is_doc("//// x") && !is_doc("// x") && is_trailing_doc("///< x")
        );
    }

    #[test]
    fn what_each_token_is_in() {
        let text = concat!(
            "#define HALF(v) ((v) + 0x600)\n",
            "enum { TAG = 0x212, OTHER = 3 };\n",
            "static const struct Info gInfo = {.id = {.joined = 0x3d1f}};\n",
            "struct S { int a; };\n",
            "s32 F(s32 x) { if (x) { return x / 16; } return 0x80; }\n",
            "u32 gWord = 0x10;\n",
            "#if EDITION\nvoid G(void) {\n#else\nvoid G(s32 b) {\n#endif\n  return 0x20; }\n",
            "void H(void) { 0x30; }\n",
        );
        let tokens = lex(text);
        let owners = owners(&tokens);
        let of = |text: &str| {
            tokens
                .iter()
                .zip(&owners)
                .filter(|(t, _)| t.text == text)
                .map(|(_, owner)| *owner)
                .collect::<Vec<_>>()
        };
        assert_eq!(of("0x600"), [Some("HALF")]);
        assert_eq!(
            (of("0x212"), of("3")),
            (vec![Some("TAG")], vec![Some("OTHER")])
        );
        assert_eq!(of("0x3d1f"), [Some("gInfo")]);
        assert_eq!(
            (of("16"), of("0x80"), of("a")),
            (vec![Some("F")], vec![Some("F")], vec![None])
        );
        assert_eq!(of("0x10"), [Some("gWord")]);
        // Each branch of an `#if` opens the same body.
        assert_eq!((of("0x20"), of("0x30")), (vec![Some("G")], vec![Some("H")]));
    }
}
