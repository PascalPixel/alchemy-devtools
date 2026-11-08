//! C tokens for the permuter, read through `gba::clex`. Comments are kept as
//! tokens so statements can carry them; preprocessor lines are skipped,
//! because the draft is read as written and the compiler preprocesses every
//! candidate. Unlike the counting lexer, the permuter refuses what is not C:
//! an unterminated comment or literal, or a stray character.

use crate::gba::clex::{self, Kind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tok {
    Ident(String),
    Number(String),
    Char(String),
    Str(String),
    Punct(&'static str),
    Comment(String),
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub start: usize,
    pub end: usize,
    pub line: usize,
}

const PUNCTUATORS: &[&str] = &[
    "...", "<<=", ">>=", "->", "++", "--", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "*=",
    "/=", "%=", "+=", "-=", "&=", "^=", "|=", "##", "[", "]", "(", ")", "{", "}", ".", "&", "*",
    "+", "-", "~", "!", "/", "%", "<", ">", "^", "|", "?", ":", ";", "=", ",", "#",
];

/// Whether a literal ends with its own quote, unescaped.
fn closed(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut at = 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            byte if byte == bytes[0] => return at == bytes.len() - 1,
            _ => at += 1,
        }
    }
    false
}

pub fn lex(source: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    for token in clex::lex(source).into_iter().filter(|token| !token.pp) {
        let (text, line) = (token.text, token.line);
        let unexpected = |c: char| format!("line {line}: unexpected character {c:?}");
        let tok = match token.kind {
            Kind::Comment if text.starts_with("/*") && (text.len() < 4 || !text.ends_with("*/")) => {
                return Err(format!("line {line}: unterminated comment"));
            }
            Kind::Comment => Tok::Comment(text.to_string()),
            Kind::Ident if text.contains('$') => return Err(unexpected('$')),
            Kind::Ident => Tok::Ident(text.to_string()),
            Kind::Number => Tok::Number(text.to_string()),
            Kind::Char | Kind::Str if !closed(text) => return Err(format!("line {line}: unterminated literal")),
            Kind::Char => Tok::Char(text.to_string()),
            Kind::Str => Tok::Str(text.to_string()),
            _ => Tok::Punct(
                PUNCTUATORS.iter().find(|&&punct| punct == text).ok_or_else(|| unexpected(text.chars().next().unwrap_or(' ')))?,
            ),
        };
        let start = text.as_ptr() as usize - source.as_ptr() as usize;
        tokens.push(Token { tok, start, end: start + text.len(), line });
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_skip_directives_and_keep_comments() {
        let tokens =
            lex("#include \"a.h\"\n#define X \\\n  1\nint a = 0x1FFu; /* c */ a <<= 2;\n").unwrap();
        let kinds: Vec<Tok> = tokens.into_iter().map(|token| token.tok).collect();
        assert_eq!(
            kinds,
            vec![
                Tok::Ident("int".into()),
                Tok::Ident("a".into()),
                Tok::Punct("="),
                Tok::Number("0x1FFu".into()),
                Tok::Punct(";"),
                Tok::Comment("/* c */".into()),
                Tok::Ident("a".into()),
                Tok::Punct("<<="),
                Tok::Number("2".into()),
                Tok::Punct(";"),
            ]
        );
        let token = &lex("x = \"a\\\"b\";").unwrap()[2];
        assert_eq!((&token.tok, token.start, token.end), (&Tok::Str("\"a\\\"b\"".into()), 4, 10));
        for text in ["/* open", "\"open\n\";", "'a", "a@b", "a$b", "x = \"a\\\";"] {
            assert!(lex(text).is_err(), "{text:?}");
        }
    }
}
