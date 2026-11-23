//! The duplicate search: function bodies and table initializers a person
//! would have written once (H5). Two copies group only when their tokens are
//! the same, preprocessor lines aside (whitespace and comments are no
//! tokens) and their parameters and locals renamed in order (a bound name
//! carries nothing of what the routine does), apart from their own name's
//! prefix (`Daila_` in
//! `Daila_ShowShop`, `gDaila_` in `gDaila_Shops`) and the names that follow
//! from it: a name with that prefix that is itself a function or table of
//! the game, and the same as the other copy's by this same rule. Every other
//! name, number and string must be the same, so routines of one shape that
//! show other text, test other flags or call through other import rows stay
//! apart. A copy written once in a shared file that scenes include is read
//! once, so it is one copy however many scenes include it.

use super::{is_keyword, Group};
use std::collections::HashMap;

/// One function body or table initializer.
pub(super) struct Copy {
    pub function: bool,
    pub file: String,
    pub name: String,
    /// Its tokens, preprocessor lines left out.
    pub tokens: Vec<String>,
}

/// Keywords a local's type may begin with.
const TYPE_WORDS: &[&str] =
    &["const", "volatile", "register", "static", "unsigned", "signed", "int", "char", "short", "long", "void", "float", "double", "_Bool"];

/// A function body's tokens with its parameters and the locals it declares
/// (at a statement's start: a type, then `*`s, then the name and `=`, `;`,
/// `,` or `[`) written `$1`, `$2` and on, in order: parameters first, then
/// each local as it is first declared. A member after `.` or `->` keeps its
/// name.
pub(super) fn bind(parameters: &[String], tokens: &[&str]) -> Vec<String> {
    let bound = bound(parameters, tokens);
    tokens
        .iter()
        .enumerate()
        .map(|(at, text)| match bound.get(text) {
            Some(number) if !(at > 0 && matches!(tokens[at - 1], "." | "->")) => format!("${number}"),
            _ => text.to_string(),
        })
        .collect()
}

/// The names a function body gives its parameters and locals, as `bind`
/// finds them, each with its number.
pub(super) fn bound<'a>(parameters: &'a [String], tokens: &[&'a str]) -> HashMap<&'a str, usize> {
    let ident = |text: &str| text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && !is_keyword(text);
    let mut bound: HashMap<&str, usize> = HashMap::new();
    for name in parameters {
        let next = bound.len() + 1;
        bound.entry(name.as_str()).or_insert(next);
    }
    let mut depth = 0usize;
    let mut start = true;
    let mut declaring = None;
    let mut at = 0;
    while at < tokens.len() {
        let text = tokens[at];
        match text {
            "(" | "[" => depth += 1,
            ")" | "]" => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0 && matches!(text, ";" | "{" | "}") {
            start = true;
            declaring = None;
            at += 1;
            continue;
        }
        // `int a = 1, *b;`: each name after a comma of the declaration.
        let mut name = None;
        if start && depth == 0 {
            let mut next = at;
            while tokens.get(next).is_some_and(|t| TYPE_WORDS.contains(t)) {
                next += 1;
            }
            let typed = next > at;
            let named = |at: usize| tokens.get(at).is_some_and(|t| ident(t));
            if matches!(tokens.get(next).copied(), Some("struct" | "union" | "enum")) && named(next + 1) {
                next += 2;
            } else if named(next) && (named(next + 1) || matches!(tokens.get(next + 1).copied(), Some("*" | "const"))) {
                // A typedef's name (`s32 i`, `struct Actor *` aside).
                next += 1;
            } else if !typed {
                next = usize::MAX;
            }
            if next != usize::MAX {
                while tokens.get(next).is_some_and(|t| matches!(*t, "*" | "const" | "volatile")) {
                    next += 1;
                }
                if tokens.get(next).is_some_and(|t| ident(t)) && tokens.get(next + 1).is_some_and(|t| matches!(*t, "=" | ";" | "," | "[")) {
                    name = Some(next);
                    declaring = Some(depth);
                }
            }
        } else if declaring == Some(depth) && text == "," {
            let mut next = at + 1;
            while tokens.get(next).is_some_and(|t| *t == "*") {
                next += 1;
            }
            if tokens.get(next).is_some_and(|t| ident(t)) && tokens.get(next + 1).is_some_and(|t| matches!(*t, "=" | ";" | "," | "[")) {
                name = Some(next);
            }
        }
        if let Some(name) = name {
            let next = bound.len() + 1;
            bound.entry(tokens[name]).or_insert(next);
        }
        start = false;
        at += 1;
    }
    bound
}

/// A name's `g` or `s` storage letter, if it has one, and the rest.
fn storage(name: &str) -> (&str, &str) {
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some('g' | 's'), Some(second)) if second.is_ascii_uppercase() => name.split_at(1),
        _ => ("", name),
    }
}

/// The prefix a name's copies may differ by: what comes before its first
/// `_`, its storage letter aside (`Daila` for `gDaila_Shops`).
fn prefix(name: &str) -> Option<&str> {
    let (_, bare) = storage(name);
    let (prefix, rest) = bare.split_once('_')?;
    (!prefix.is_empty() && !rest.is_empty() && prefix.starts_with(|c: char| c.is_ascii_uppercase())).then_some(prefix)
}

/// What is left of `name` once `prefix` is taken out, if `name` has it:
/// `g_Shops` for `gDaila_Shops` under `Daila`.
fn follows(name: &str, prefix: &str) -> Option<String> {
    let (letter, bare) = storage(name);
    let rest = bare.strip_prefix(prefix)?.strip_prefix('_')?;
    (!rest.is_empty()).then(|| format!("{letter}_{rest}"))
}

/// The groups of copies, functions and tables apart, each at least as long
/// as its kind's minimum (shorter copies still settle whether a longer one's
/// callees and tables are the same), largest saving first.
pub(super) fn groups(copies: &[Copy], function_min: usize, table_min: usize) -> (Vec<Group>, Vec<Group>) {
    let mut defined: HashMap<&str, Vec<usize>> = HashMap::new();
    for (at, copy) in copies.iter().enumerate() {
        defined.entry(copy.name.as_str()).or_default().push(at);
    }
    // A name means the definition in its own file, else the only one.
    let resolve = |name: &str, file: &str| {
        let found = defined.get(name)?;
        let mut own = found.iter().filter(|&&at| copies[at].file == file);
        match (own.next(), own.next()) {
            (Some(&at), None) => Some(at),
            (None, _) if found.len() == 1 => Some(found[0]),
            _ => None,
        }
    };
    let mut keys: HashMap<String, usize> = HashMap::new();
    let mut class = Vec::with_capacity(copies.len());
    let mut refs: Vec<Vec<usize>> = Vec::with_capacity(copies.len());
    for copy in copies {
        let own = prefix(&copy.name);
        let mut key = String::from(if copy.function { "F" } else { "T" });
        let mut targets = Vec::new();
        for token in &copy.tokens {
            let target = own.and_then(|own| follows(token, own)).and_then(|rest| Some((rest, resolve(token, &copy.file)?)));
            match target {
                Some((rest, target)) => {
                    key.push('\u{1e}');
                    key.push_str(&rest);
                    targets.push(target);
                }
                None => key.push_str(token),
            }
            key.push('\u{1f}');
        }
        let next = keys.len();
        class.push(*keys.entry(key).or_insert(next));
        refs.push(targets);
    }
    // Split each class by the classes of what its copies name, until no
    // class splits: copies stay together only while their callees and
    // tables do.
    let mut count = keys.len();
    loop {
        let mut signatures: HashMap<(usize, Vec<usize>), usize> = HashMap::new();
        let next: Vec<usize> = (0..copies.len())
            .map(|at| {
                let signature = (class[at], refs[at].iter().map(|&target| class[target]).collect());
                let fresh = signatures.len();
                *signatures.entry(signature).or_insert(fresh)
            })
            .collect();
        class = next;
        if signatures.len() == count {
            break;
        }
        count = signatures.len();
    }
    let mut members: HashMap<usize, Vec<usize>> = HashMap::new();
    for (at, &id) in class.iter().enumerate() {
        members.entry(id).or_default().push(at);
    }
    let (mut functions, mut tables) = (Vec::new(), Vec::new());
    for copies_of in members.into_values().filter(|members| members.len() > 1) {
        let first = &copies[copies_of[0]];
        let tokens = first.tokens.len();
        if tokens < if first.function { function_min } else { table_min } {
            continue;
        }
        let mut names: Vec<String> = copies_of.iter().map(|&at| format!("{}:{}", copies[at].file, copies[at].name)).collect();
        names.sort();
        let group = Group { tokens, members: names };
        if first.function { functions.push(group) } else { tables.push(group) }
    }
    for groups in [&mut functions, &mut tables] {
        groups.sort_by(|a, b| {
            (b.tokens * (b.members.len() - 1)).cmp(&(a.tokens * (a.members.len() - 1))).then(a.members.cmp(&b.members))
        });
    }
    (functions, tables)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copy(function: bool, file: &str, name: &str, body: &str) -> Copy {
        let tokens = body.split_whitespace().map(str::to_string).collect();
        Copy { function, file: file.to_string(), name: name.to_string(), tokens }
    }

    fn members(groups: &[Group]) -> Vec<Vec<&str>> {
        groups.iter().map(|group| group.members.iter().map(String::as_str).collect()).collect()
    }

    #[test]
    fn parameters_and_locals_are_bound() {
        let body = "struct Actor * actor = Actor_Get ( id ) ; s32 i , * p ; u8 rows [ 4 ] ; \
                    for ( i = 0 ; i < n ; i ++ ) { int k = actor -> i + gTable [ i ] * SCALE ; k = rows [ k ] ; } \
                    x = y * SCALE ; return actor -> id ;";
        let tokens: Vec<&str> = body.split_whitespace().collect();
        let bound = bind(&["id".to_string(), "n".to_string()], &tokens).join(" ");
        assert_eq!(
            bound,
            "struct Actor * $3 = Actor_Get ( $1 ) ; s32 $4 , * $5 ; u8 $6 [ 4 ] ; \
             for ( $4 = 0 ; $4 < $2 ; $4 ++ ) { int $7 = $3 -> i + gTable [ $4 ] * SCALE ; $7 = $6 [ $7 ] ; } \
             x = y * SCALE ; return $3 -> id ;"
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        );
    }

    #[test]
    fn prefixes_and_what_follows_them() {
        assert_eq!(prefix("Daila_ShowShop"), Some("Daila"));
        assert_eq!(prefix("gDaila_Shops"), Some("Daila"));
        assert_eq!(prefix("sMagmaRock_Rocks"), Some("MagmaRock"));
        assert_eq!(prefix("gate_Open"), None);
        assert_eq!(prefix("Scene"), None);
        assert_eq!(follows("gDaila_Shops", "Daila"), Some("g_Shops".to_string()));
        assert_eq!(follows("Daila_Run", "Daila"), Some("_Run".to_string()));
        assert_eq!(follows("DailaInside_Run", "Daila"), None);
        assert_eq!(follows("FLAG_DAILA_OPEN", "Daila"), None);
    }

    #[test]
    fn copies_differ_only_by_their_prefix_and_what_follows() {
        let copies = [
            // Same body, each scene's own callee and table, themselves copies.
            copy(true, "a.c", "Daila_Show", "Daila_Open ( gDaila_Rows , 3 ) ;"),
            copy(true, "b.c", "Contigo_Show", "Contigo_Open ( gContigo_Rows , 3 ) ;"),
            copy(true, "a.c", "Daila_Open", "Box_Open ( 1 ) ;"),
            copy(true, "b.c", "Contigo_Open", "Box_Open ( 1 ) ;"),
            copy(false, "a.c", "gDaila_Rows", "{ 1 , 2 }"),
            copy(false, "b.c", "gContigo_Rows", "{ 1 , 2 }"),
            // The same shape with another message: different substance.
            copy(true, "c.c", "Kibombo_Show", "Kibombo_Open ( gKibombo_Rows , 4 ) ;"),
            copy(true, "c.c", "Kibombo_Open", "Box_Open ( 1 ) ;"),
            copy(false, "c.c", "gKibombo_Rows", "{ 1 , 2 }"),
            // The same text but a callee that differs.
            copy(true, "d.c", "Lemuria_Show", "Lemuria_Open ( gLemuria_Rows , 3 ) ;"),
            copy(true, "d.c", "Lemuria_Open", "Box_Open ( 2 ) ;"),
            copy(false, "d.c", "gLemuria_Rows", "{ 1 , 2 }"),
            // Another scene's names, or a callee nothing defines (an import
            // row), must be the same name.
            copy(true, "e.c", "Madra_Show", "Daila_Open ( gDaila_Rows , 3 ) ;"),
            copy(true, "f.c", "Yallam_Show", "Yallam_Import ( gYallam_Rows , 3 ) ;"),
            copy(true, "g.c", "Garoh_Show", "Garoh_Import ( gGaroh_Rows , 3 ) ;"),
            copy(false, "f.c", "gYallam_Rows", "{ 1 , 2 }"),
            copy(false, "g.c", "gGaroh_Rows", "{ 1 , 2 }"),
        ];
        let (functions, tables) = groups(&copies, 1, 1);
        assert_eq!(
            members(&functions),
            [
                vec!["a.c:Daila_Open", "b.c:Contigo_Open", "c.c:Kibombo_Open"],
                vec!["a.c:Daila_Show", "b.c:Contigo_Show"],
            ]
        );
        assert_eq!(
            members(&tables),
            [vec!["a.c:gDaila_Rows", "b.c:gContigo_Rows", "c.c:gKibombo_Rows", "d.c:gLemuria_Rows", "f.c:gYallam_Rows", "g.c:gGaroh_Rows"]]
        );
    }

    #[test]
    fn mutual_references_and_minimums() {
        let copies = [
            // Tables of scene routines that call back into the table.
            copy(false, "a.c", "gDaila_Script", "{ Daila_Step , 0 }"),
            copy(false, "b.c", "gContigo_Script", "{ Contigo_Step , 0 }"),
            copy(true, "a.c", "Daila_Step", "Run ( gDaila_Script ) ;"),
            copy(true, "b.c", "Contigo_Step", "Run ( gContigo_Script ) ;"),
            // Unprefixed names must match outright.
            copy(true, "a.c", "Layer_Track", "gDaila_Line = 1 + 2 ;"),
            copy(true, "b.c", "Layer_Track", "gContigo_Line = 1 + 2 ;"),
        ];
        let (functions, tables) = groups(&copies, 5, 6);
        assert_eq!(members(&functions), [vec!["a.c:Daila_Step", "b.c:Contigo_Step"]]);
        assert_eq!(members(&tables), Vec::<Vec<&str>>::new());
        let (_, tables) = groups(&copies, 5, 5);
        assert_eq!(members(&tables), [vec!["a.c:gDaila_Script", "b.c:gContigo_Script"]]);
    }
}
