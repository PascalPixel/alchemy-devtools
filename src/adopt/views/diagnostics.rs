//! What the compiler says about a joined file, read back: its messages with
//! their places, where the preprocessor first entered each header, and the
//! declaration of each routine `-aux-info` saw.

use std::collections::BTreeMap;

/// One compiler message: `FILE:LINE[:COLUMN]: [warning: ]TEXT`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Message {
    pub file: String,
    pub line: usize,
    pub warning: bool,
    pub text: String,
}

impl Message {
    /// The name a message quotes, as gcc 2 quotes it (`` `NAME' ``) or cpp
    /// does (`"NAME"`).
    pub fn name(&self) -> Option<&str> {
        let quoted = |open: char, close: char| -> Option<&str> {
            let start = self.text.find(open)? + 1;
            let end = self.text[start..].find(close)? + start;
            Some(&self.text[start..end])
        };
        quoted('`', '\'').or_else(|| quoted('"', '"'))
    }
}

/// The messages of a compiler's stderr, in order; the lines that only say
/// where (`In function`, `In file included from`, `At top level`) are dropped.
pub(crate) fn messages(stderr: &str) -> Vec<Message> {
    let mut found = Vec::new();
    for line in stderr.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("In file included from") || trimmed.starts_with("from ") {
            continue;
        }
        let Some((file, rest)) = line.split_once(':') else { continue };
        let mut parts = rest.splitn(3, ':');
        let Some(number) = parts.next().and_then(|n| n.trim().parse::<usize>().ok()) else { continue };
        let mut text = parts.next().unwrap_or("").to_string();
        if let Some(column_rest) = parts.next() {
            match text.trim().parse::<usize>() {
                Ok(_) => text = column_rest.to_string(),
                Err(_) => text = format!("{text}:{column_rest}"),
            }
        }
        let text = text.trim();
        if text.starts_with("In function") || text.starts_with("At top level") {
            continue;
        }
        let (warning, text) = match text.strip_prefix("warning: ") {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        found.push(Message { file: file.to_string(), line: number, warning, text: text.to_string() });
    }
    found
}

/// What a preprocessed file says of where things came from.
#[derive(Debug, Default)]
pub(crate) struct Preprocessed {
    /// Each header, by the name the compiler writes, and the line of the
    /// joined file whose `#include` first entered it.
    pub entered: BTreeMap<String, usize>,
    /// Each line of the output: the file and line it came from.
    pub origin: Vec<(String, usize)>,
    pub text: String,
}

/// Reads `cpp`'s line markers (`# LINE "FILE" FLAGS`): `top` is the joined
/// file's name as the compiler writes it.
pub(crate) fn preprocessed(output: &str, top: &str) -> Preprocessed {
    let mut read = Preprocessed { text: output.to_string(), ..Preprocessed::default() };
    let mut file = top.to_string();
    let mut line = 1usize;
    // The headers entered since the joined file was last read: the marker
    // that returns to it names the line after their #include.
    let mut entered: Vec<String> = Vec::new();
    for text in output.split('\n') {
        if let Some((number, name, flags)) = marker(text) {
            if name == top {
                if flags.contains(&2) {
                    for header in entered.drain(..) {
                        read.entry(header, number.saturating_sub(1));
                    }
                }
            } else if !entered.iter().any(|known| known == name) {
                entered.push(name.to_string());
            }
            file = name.to_string();
            line = number;
            read.origin.push((file.clone(), 0));
            continue;
        }
        read.origin.push((file.clone(), line));
        line += 1;
    }
    read
}

impl Preprocessed {
    fn entry(&mut self, header: String, include: usize) {
        self.entered.entry(header).or_insert(include);
    }
}

/// A line marker's line, file and flags.
fn marker(text: &str) -> Option<(usize, &str, Vec<u8>)> {
    let rest = text.strip_prefix("# ")?;
    let (number, rest) = rest.split_once(' ')?;
    let number = number.parse().ok()?;
    let rest = rest.strip_prefix('"')?;
    let (name, flags) = rest.split_once('"')?;
    Some((number, name, flags.split_whitespace().filter_map(|flag| flag.parse().ok()).collect()))
}

/// One routine declaration `-aux-info` wrote: where, how, and as what.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Declared {
    pub file: String,
    pub line: usize,
    /// `I` implicit, `N` a prototype, `O` old style; then `C` a declaration
    /// or `F` a definition.
    pub how: String,
    pub name: String,
    /// The declaration, its comments taken out.
    pub text: String,
}

/// An `-aux-info` file's declarations, in the order the compiler read them.
pub(crate) fn declarations(aux: &str) -> Vec<Declared> {
    let mut found = Vec::new();
    for line in aux.lines() {
        let Some(rest) = line.strip_prefix("/* ") else { continue };
        let Some((place, declaration)) = rest.split_once(" */ ") else { continue };
        let mut fields = place.rsplitn(3, ':');
        let (Some(how), Some(number), Some(file)) = (fields.next(), fields.next(), fields.next()) else { continue };
        let Ok(number) = number.parse() else { continue };
        let text = strip_comments(declaration);
        let Some(name) = declared_name(&text) else { continue };
        found.push(Declared { file: file.to_string(), line: number, how: how.to_string(), name, text });
    }
    found
}

fn strip_comments(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start..].find("*/").map_or("", |end| &rest[start + end + 2..]);
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The routine an aux-info declaration names: the first identifier before a
/// parameter list that is not a word of a type.
fn declared_name(text: &str) -> Option<String> {
    const TYPE_WORDS: [&str; 12] = ["void", "int", "char", "short", "long", "unsigned", "signed", "float", "double", "const", "volatile", "extern"];
    let words = crate::gba::clex::lex(text);
    words
        .windows(2)
        .find(|pair| pair[0].kind == crate::gba::clex::Kind::Ident && pair[1].text == "(" && !TYPE_WORDS.contains(&pair[0].text))
        .map(|pair| pair[0].text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_keep_their_places_and_names() {
        let stderr = "In file included from games/tla/include/field/field.h:7,\n                 from j.c:61:\ngames/tla/include/actor.h:80: conflicting types for `Actor_Move'\nk.h:73: previous declaration of `Actor_Move'\nt.c:16:11: warning: \"M\" redefined\nj.c: In function `Two':\nj.c:20: warning: type mismatch with previous implicit declaration\n";
        let found = messages(stderr);
        assert_eq!(found.len(), 4);
        assert_eq!((found[0].file.as_str(), found[0].line, found[0].name()), ("games/tla/include/actor.h", 80, Some("Actor_Move")));
        assert_eq!((found[2].line, found[2].warning, found[2].name()), (16, true, Some("M")));
        assert_eq!(found[3].text, "type mismatch with previous implicit declaration");
    }

    #[test]
    fn line_markers_say_which_include_entered_a_header() {
        let output = "# 1 \"j.c\"\n\n# 1 \"a.h\" 1\nint a;\n# 1 \"b.h\" 1\nint b;\n# 2 \"a.h\" 2\n# 2 \"j.c\" 2\nint x;\n\n# 1 \"c.h\" 1\nint c;\n# 5 \"j.c\" 2\nint y;\n";
        let read = preprocessed(output, "j.c");
        assert_eq!(read.entered.get("a.h"), Some(&1));
        assert_eq!(read.entered.get("b.h"), Some(&1));
        assert_eq!(read.entered.get("c.h"), Some(&4));
        assert_eq!(read.origin[8], ("j.c".to_string(), 2));
        assert_eq!(read.origin[13], ("j.c".to_string(), 5));
    }

    #[test]
    fn aux_info_names_each_declaration() {
        let aux = "/* compiled from: . */\n/* u.c:2:NC */ extern int f (void);\n/* u.c:4:OC */ extern int g (/* ??? */);\n/* u.c:5:IC */ extern int h (/* ??? */);\n/* inc/x.h:9:NC */ extern void (*Pick (int)) (void);\n";
        let found = declarations(aux);
        let names: Vec<(&str, &str, &str)> = found.iter().map(|d| (d.name.as_str(), d.how.as_str(), d.text.as_str())).collect();
        assert_eq!(names, [("f", "NC", "extern int f (void);"), ("g", "OC", "extern int g ();"), ("h", "IC", "extern int h ();"), ("Pick", "NC", "extern void (*Pick (int)) (void);")]);
        assert_eq!(found[3].file, "inc/x.h");
    }
}
