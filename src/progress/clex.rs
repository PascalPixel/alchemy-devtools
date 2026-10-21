//! A small C lexer for the humanization report: identifiers, numbers,
//! strings and punctuation with their lines, comments kept with their text,
//! and each preprocessor directive bracketed by its own start and end tokens.
//! It never expands a macro or follows an include.

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
                tokens.push(Token { kind: Kind::PpEnd, text: "", line, pp: true, glued: false });
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
            at = (at + 1).min(bytes.len());
            if byte == b'"' {
                Kind::Str
            } else {
                Kind::Char
            }
        } else if byte.is_ascii_digit() || (byte == b'.' && bytes.get(at + 1).is_some_and(u8::is_ascii_digit)) {
            at += 1;
            while at < bytes.len() {
                let next = bytes[at];
                if (next == b'+' || next == b'-') && matches!(bytes[at - 1], b'e' | b'E' | b'p' | b'P') {
                    at += 1;
                } else if next.is_ascii_alphanumeric() || next == b'_' || next == b'.' {
                    at += 1;
                } else {
                    break;
                }
            }
            Kind::Number
        } else if byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$' {
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_' || bytes[at] == b'$') {
                at += 1;
            }
            Kind::Ident
        } else {
            const THREE: [&[u8]; 3] = [b"...", b"<<=", b">>="];
            const TWO: [&[u8]; 20] = [
                b"->", b"++", b"--", b"<<", b">>", b"<=", b">=", b"==", b"!=", b"&&", b"||", b"+=", b"-=", b"*=",
                b"/=", b"%=", b"&=", b"|=", b"^=", b"##",
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
        tokens.push(Token { kind, text: &text[start..at], line: first_line, pp: in_pp, glued: last_end == start });
        last_end = at;
    }
    if in_pp {
        tokens.push(Token { kind: Kind::PpEnd, text: "", line, pp: true, glued: false });
    }
    tokens
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
        assert!(tokens[3].glued, "a function-like macro's parenthesis is glued to its name");
        assert!(kinds.contains(&(Kind::Number, "0x10u", true)));
        assert!(kinds.contains(&(Kind::Comment, "/// note", true)));
        assert!(kinds.contains(&(Kind::PpEnd, "", true)));
        assert!(kinds.contains(&(Kind::Number, "1.5e+3", false)));
        assert_eq!(tokens.last().unwrap().line, 3);
        assert!(is_doc("/// x") && !is_doc("//// x") && !is_doc("// x") && is_trailing_doc("///< x"));
    }
}
