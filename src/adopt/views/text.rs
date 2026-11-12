//! The C a view edits, read by tokens: a part's top-level statements, a
//! name renamed in one of C's name spaces, and a declaration as one line.
//! Nothing here expands a macro; the compiler judges what the edits mean.

use crate::gba::clex::{lex, Kind, Token};

/// Which of C's name spaces a rename reaches: an ordinary identifier (a
/// routine, variable, typedef, enumerator or macro) or a tag (`struct X`,
/// `union X`, `enum X`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Space {
    Ordinary,
    Tag,
}

/// Where a token starts in the text it was read from.
fn offset(text: &str, token: &Token) -> usize {
    token.text.as_ptr() as usize - text.as_ptr() as usize
}

/// The text with every `from` in `space` written `to`. A member name (after
/// `.` or `->`) and an `#include` line are left alone; a tag is the name
/// after `struct`, `union` or `enum`, and an ordinary name is any other.
pub(crate) fn rename(text: &str, from: &str, to: &str, space: Space) -> String {
    let tokens = lex(text);
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    let mut previous: Option<&Token> = None;
    let mut include = false;
    // A header view's own wrapper (`#define FROM TO` ... `#undef FROM`) is
    // what makes the rename; it stays as it is.
    let directive = |index: usize| -> Vec<&str> {
        tokens[index + 1..].iter().take_while(|t| t.kind != Kind::PpEnd && t.kind != Kind::Comment).map(|t| t.text).collect()
    };
    let wrapped = tokens.iter().enumerate().any(|(index, t)| t.kind == Kind::PpStart && directive(index) == ["define", from, to]);
    for (index, token) in tokens.iter().enumerate() {
        if token.kind == Kind::PpStart {
            let words = directive(index);
            include = words.first() == Some(&"include") || (wrapped && (words == ["define", from, to] || words == ["undef", from]));
        } else if token.kind == Kind::PpEnd {
            include = false;
        }
        if token.kind == Kind::Comment {
            continue;
        }
        if token.kind == Kind::Ident && token.text == from && !include {
            let after = previous.map_or("", |previous| previous.text);
            let tagged = matches!(after, "struct" | "union" | "enum");
            let member = matches!(after, "." | "->");
            let reaches = match space {
                Space::Tag => tagged,
                Space::Ordinary => !tagged && !member,
            };
            if reaches {
                let start = offset(text, token);
                out.push_str(&text[at..start]);
                out.push_str(to);
                at = start + token.text.len();
            }
        }
        if token.kind != Kind::PpEnd {
            previous = Some(token);
        }
    }
    out.push_str(&text[at..]);
    out
}

/// A token of a statement: its text, where it starts and the
/// braces open around it inside the statement.
#[derive(Clone, Debug)]
pub(crate) struct Word<'a> {
    pub text: &'a str,
    pub at: usize,
    pub depth: usize,
}

/// One declaration or definition at file scope: from its first token to its
/// `;`, or to the `}` that closes a function's body.
#[derive(Clone, Debug)]
pub(crate) struct Statement<'a> {
    pub start: usize,
    pub end: usize,
    pub first_line: usize,
    pub last_line: usize,
    /// A function body's braces, as offsets.
    pub body: Option<(usize, usize)>,
    pub words: Vec<Word<'a>>,
}

impl Statement<'_> {
    /// The statement as one line: its tokens apart from comments, a space
    /// between two words that need one.
    pub fn squashed(&self) -> String {
        squash(self.words.iter().map(|word| word.text))
    }

    /// The statement up to its body or `;`, as one line ending in `;`: a
    /// definition's declaration.
    pub fn declaration(&self) -> String {
        let head: Vec<&str> = match self.body {
            Some((open, _)) => self.words.iter().take_while(|word| word.at < open).map(|word| word.text).collect(),
            None => self.words.iter().map(|word| word.text).take_while(|text| *text != ";").collect(),
        };
        format!("{};", squash(head.into_iter()))
    }

    fn top(&self) -> impl Iterator<Item = &Word<'_>> {
        self.words.iter().filter(|word| word.depth == 0)
    }

    /// Whether a word stands at file scope in it: `typedef`, `static`, `extern`.
    pub fn has(&self, word: &str) -> bool {
        self.top().any(|found| found.text == word)
    }

    pub fn is_inline(&self) -> bool {
        self.top().any(|found| matches!(found.text, "inline" | "__inline__" | "__inline"))
    }

    /// The first word that is `name`, at any depth.
    pub fn find(&self, name: &str) -> Option<usize> {
        self.words.iter().position(|word| word.text == name)
    }

    /// Whether `name` is an enumerator here: inside the braces of an `enum`.
    pub fn enumerates(&self, name: &str) -> bool {
        let Some(at) = self.words.iter().position(|word| word.text == name && word.depth > 0) else { return false };
        let open = self.words[..at].iter().rposition(|word| word.text == "{" && word.depth == self.words[at].depth - 1);
        open.is_some_and(|open| {
            let before: Vec<&str> = self.words[..open].iter().rev().take(2).map(|word| word.text).collect();
            before.first() == Some(&"enum") || before.get(1) == Some(&"enum")
        })
    }

    /// Whether `name` is declared here as a routine: its declarator is
    /// followed by a parameter list.
    pub fn routine(&self, name: &str) -> bool {
        self.words.iter().enumerate().any(|(i, word)| word.depth == 0 && word.text == name && self.words.get(i + 1).is_some_and(|next| next.text == "("))
    }

    /// Whether the statement defines `name` rather than only declaring it: a
    /// routine's body, or a variable that is not `extern`.
    pub fn defines(&self, name: &str) -> bool {
        if self.routine(name) {
            return self.body.is_some();
        }
        !self.has("extern")
    }

    /// Whether it declares more than one name at file scope (`int a, b;`).
    pub fn several(&self) -> bool {
        let mut parens = 0usize;
        for word in self.top() {
            match word.text {
                "(" | "[" => parens += 1,
                ")" | "]" => parens = parens.saturating_sub(1),
                "," if parens == 0 => return true,
                _ => {}
            }
        }
        false
    }

    /// Where the statement's last `;` is, for a declaration that has one.
    pub fn semicolon(&self) -> Option<usize> {
        self.words.last().filter(|word| word.text == ";").map(|word| word.at)
    }
}

/// Tokens as one line, a space between two words that need one.
pub(crate) fn squash<'a>(words: impl Iterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for word in words {
        let glue = out.is_empty()
            || matches!(word, ")" | "]" | "," | ";" | "[" | "->" | ".")
            || out.ends_with(['(', '[', '*', '.'])
            || out.ends_with("->")
            || (word == "(" && out.ends_with(|c: char| c.is_alphanumeric() || c == '_'));
        if !glue {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// Every file-scope statement of a text, in order. Directives and comments
/// are no part of one.
pub(crate) fn statements(text: &str) -> Vec<Statement<'_>> {
    let mut found = Vec::new();
    let mut current: Option<Statement> = None;
    let mut depth = 0usize;
    let mut parens = 0usize;
    for token in lex(text).iter().filter(|token| !token.pp && token.kind != Kind::Comment) {
        let at = offset(text, token);
        let statement = current.get_or_insert_with(|| Statement { start: at, end: at, first_line: token.line, last_line: token.line, body: None, words: Vec::new() });
        let mut ends = false;
        let mut word_depth = depth;
        match token.text {
            "(" | "[" => parens += 1,
            ")" | "]" => parens = parens.saturating_sub(1),
            "{" => {
                let after_parameters = statement.words.last().is_some_and(|last| last.text == ")");
                let assigns = statement.words.iter().any(|word| word.depth == 0 && word.text == "=");
                if depth == 0 && parens == 0 && after_parameters && !assigns {
                    statement.body = Some((at, at));
                }
                depth += 1;
            }
            "}" => {
                depth = depth.saturating_sub(1);
                word_depth = depth;
                if depth == 0 {
                    if let Some((open, _)) = statement.body {
                        statement.body = Some((open, at));
                        ends = true;
                    }
                }
            }
            ";" if depth == 0 && parens == 0 => ends = true,
            _ => {}
        }
        statement.words.push(Word { text: token.text, at, depth: word_depth });
        statement.end = at + token.text.len();
        statement.last_line = token.line;
        if ends {
            found.extend(current.take());
        }
    }
    found.extend(current);
    found
}

/// The statement that declares `name` nearest `line`: one whose lines hold
/// it, else the closest that names it.
pub(crate) fn statement_at<'a>(statements: &'a [Statement<'a>], line: usize, name: &str) -> Option<&'a Statement<'a>> {
    let naming = || statements.iter().filter(|statement| statement.find(name).is_some());
    naming()
        .find(|statement| (statement.first_line..=statement.last_line).contains(&line))
        .or_else(|| naming().min_by_key(|statement| statement.first_line.abs_diff(line).min(statement.last_line.abs_diff(line))))
}

/// The offset a 1-based line starts at, or the text's end.
pub(crate) fn line_start(text: &str, line: usize) -> usize {
    if line <= 1 {
        return 0;
    }
    text.match_indices('\n').nth(line - 2).map_or(text.len(), |(at, _)| at + 1)
}

/// The text with `lines` put before 1-based line `line`.
pub(crate) fn insert_before(text: &str, line: usize, lines: &[String]) -> String {
    let at = line_start(text, line);
    let mut out = String::with_capacity(text.len() + 80 * lines.len());
    out.push_str(&text[..at]);
    if at == text.len() && !text.is_empty() && !text.ends_with('\n') {
        out.push('\n');
    }
    for added in lines {
        out.push_str(added);
        out.push('\n');
    }
    out.push_str(&text[at..]);
    out
}

/// Each macro a text defines, with its definition as one line: the name,
/// its parameters glued to it, and the replacement's tokens.
pub(crate) fn macros(text: &str) -> Vec<(String, String, usize)> {
    let tokens = lex(text);
    let mut found = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != Kind::PpStart || tokens.get(index + 1).map(|t| t.text) != Some("define") {
            continue;
        }
        let rest: Vec<&Token> = tokens[index + 2..].iter().take_while(|t| t.kind != Kind::PpEnd).filter(|t| t.kind != Kind::Comment).collect();
        let Some(name) = rest.first() else { continue };
        found.push((name.text.to_string(), definition(&rest), token.line));
    }
    found
}

/// A `#define`'s words after `define`, as `macros` writes them.
fn definition(rest: &[&Token]) -> String {
    let mut out = rest[0].text.to_string();
    let mut words = rest[1..].iter().peekable();
    if words.peek().is_some_and(|t| t.text == "(" && t.glued) {
        let mut parameters = Vec::new();
        for word in words.by_ref() {
            parameters.push(word.text);
            if word.text == ")" {
                break;
            }
        }
        out.push_str(&squash(parameters.into_iter()));
    }
    let body = squash(words.map(|t| t.text));
    if !body.is_empty() {
        out.push(' ');
        out.push_str(&body);
    }
    out
}

/// The macros a `-dM` listing defines, by name, each as `macros` writes it.
pub(crate) fn listed_macros(listing: &str) -> std::collections::BTreeMap<String, String> {
    macros(listing).into_iter().map(|(name, text, _)| (name, text)).collect()
}

/// A text's words (identifiers and keywords) in order.
pub(crate) fn identifiers_in_order(text: &str) -> Vec<&str> {
    lex(text).into_iter().filter(|token| token.kind == Kind::Ident).map(|token| token.text).collect()
}

/// The identifiers a text uses, each once.
pub(crate) fn identifiers(text: &str) -> std::collections::BTreeSet<&str> {
    lex(text).into_iter().filter(|token| token.kind == Kind::Ident).map(|token| token.text).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rename_reaches_only_its_own_name_space() {
        let text = "#include \"helper.h\"\nstruct Helper { int Helper; };\nint Helper(struct Helper *h) { return h->Helper + Helper(h); } // Helper\n";
        assert_eq!(
            rename(text, "Helper", "Helper__b", Space::Ordinary),
            "#include \"helper.h\"\nstruct Helper { int Helper__b; };\nint Helper__b(struct Helper *h) { return h->Helper + Helper__b(h); } // Helper\n"
        );
        assert_eq!(
            rename(text, "Helper", "Helper__b", Space::Tag),
            "#include \"helper.h\"\nstruct Helper__b { int Helper; };\nint Helper(struct Helper__b *h) { return h->Helper + Helper(h); } // Helper\n"
        );
        assert_eq!(rename("#define CALL(x) Go(x)\n", "Go", "Go__a", Space::Ordinary), "#define CALL(x) Go__a(x)\n");
        let wrapper = "#define Go Go__a\n#include \"go.h\"\n#undef Go\nvoid Go(int);\n";
        assert_eq!(rename(wrapper, "Go", "Go__a", Space::Ordinary), "#define Go Go__a\n#include \"go.h\"\n#undef Go\nvoid Go__a(int);\n");
        assert_eq!(rename("#undef Go\n", "Go", "Go__a", Space::Ordinary), "#undef Go__a\n");
    }

    #[test]
    fn statements_end_at_their_semicolon_or_body() {
        let text = "/// A thing.\nstruct Thing { int a; short b; };\n#define X 1\nint Helper(struct Thing *thing);\nint Two(struct Thing *t) {\n  return Helper(t) + t->b;\n}\nenum { ONE, TWO = 2 };\ntypedef int T;\nstatic const int sTable[] = { 1, 2 };\n";
        let found = statements(text);
        let squashed: Vec<String> = found.iter().map(Statement::squashed).collect();
        assert_eq!(
            squashed,
            [
                "struct Thing { int a; short b; };",
                "int Helper(struct Thing *thing);",
                "int Two(struct Thing *t) { return Helper(t) + t->b; }",
                "enum { ONE, TWO = 2 };",
                "typedef int T;",
                "static const int sTable[] = { 1, 2 };"
            ]
        );
        assert!(found[1].routine("Helper") && !found[1].defines("Helper"));
        assert!(found[2].defines("Two") && found[2].body.is_some());
        assert_eq!(found[2].declaration(), "int Two(struct Thing *t);");
        assert!(found[3].enumerates("TWO") && !found[0].enumerates("a"));
        assert!(found[4].has("typedef") && found[5].has("static") && found[5].defines("sTable"));
        assert_eq!((found[2].first_line, found[2].last_line), (5, 7));
        assert_eq!(statement_at(&found, 6, "Helper").map(|s| s.first_line), Some(5));
        assert_eq!(statement_at(&found, 4, "Helper").map(|s| s.first_line), Some(4));
        assert!(statements("int a, b;")[0].several() && !statements("int f(int a, int b);")[0].several());
    }

    #[test]
    fn macros_read_as_one_line_and_lines_insert() {
        let text = "#define A 1\n#define F(x, y)  ((x) +\\\n (y))\n#define G (x)\n";
        let found: Vec<(String, String)> = macros(text).into_iter().map(|(n, d, _)| (n, d)).collect();
        assert_eq!(found, [("A".into(), "A 1".into()), ("F".into(), "F(x, y) ((x) + (y))".into()), ("G".into(), "G (x)".into())]);
        assert_eq!(listed_macros("#define F(x,y) ((x) + (y))\n")["F"], "F(x, y) ((x) + (y))");
        assert_eq!(insert_before("a\nb\n", 2, &["x".into()]), "a\nx\nb\n");
        assert_eq!(insert_before("a\nb", 3, &["x".into()]), "a\nb\nx\n");
    }
}
