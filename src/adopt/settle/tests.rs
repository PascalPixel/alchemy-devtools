//! Views taken away from joined texts as adopt merge --keep-views writes
//! them, judged by a stand-in that says which texts move code.

use super::*;

const JOINED: &str = "/// @file Two parts.\n\
// ---- a.c ----\n\
#include \"thing.h\"\n\
void Helper(int actor, int value);\n\
struct Box { int a; };\n\
\n\
// ---- b.c ----\n\
/* VIEW(b): routine Helper as int Helper(struct Thing *thing); a has void Helper(int actor, int value). */\n\
extern int Helper__b(struct Thing *thing) __asm__(\"Helper\");\n\
/* VIEW(b): tag struct Box as struct Box__b; an earlier part has another definition. */\n\
struct Box__b { int a; };\n\
/* VIEW(b): static Twice as Twice__b; a has static int Twice(int x) { return x + x; }. */\n\
static int Twice__b(int x) { return 2 * x; }\n\
int Use(struct Box__b *box) { return Helper__b(0) + Twice__b(box->a); }\n";

fn views_of(text: &str) -> Vec<View> {
    text.lines().filter_map(marker).collect()
}

#[test]
fn a_marker_names_its_part_kind_and_name() {
    let views = views_of(JOINED);
    let read: Vec<(&str, &str, &str)> = views.iter().map(|v| (v.suffix.as_str(), v.kind, v.name.as_str())).collect();
    assert_eq!(read, [("b", "routine", "Helper"), ("b", "tag", "Box"), ("b", "static", "Twice")]);
    assert!(marker("// a comment").is_none() && marker("/* VIEW(b): nonsense */").is_none());
}

#[test]
fn a_routine_s_view_goes_as_a_declaration_or_as_nothing() {
    let views = views_of(JOINED);
    let declared = apply(JOINED, &views[0], &Choice::Declare { declaration: "void Helper(int actor, int value);".into(), whose: "the definition's".into() }).unwrap();
    assert!(!declared.contains("VIEW(b): routine") && !declared.contains("Helper__b"), "{declared}");
    assert!(declared.contains("// ---- b.c ----\nvoid Helper(int actor, int value);\n/* VIEW(b): tag"), "{declared}");
    assert!(declared.contains("return Helper(0)"));
    let removed = apply(JOINED, &views[0], &Choice::Remove).unwrap();
    assert!(removed.contains("// ---- b.c ----\n/* VIEW(b): tag"), "{removed}");
    // The part before is not touched.
    assert!(removed.starts_with("/// @file Two parts.\n// ---- a.c ----\n#include \"thing.h\"\nvoid Helper(int actor, int value);\n"));
}

#[test]
fn a_tag_s_and_a_static_s_view_go_with_their_own_definition() {
    let views = views_of(JOINED);
    let text = apply(JOINED, &views[1], &Choice::Remove).unwrap();
    assert!(!text.contains("Box__b") && text.contains("int Use(struct Box *box)"), "{text}");
    let text = apply(&text, &views[2], &Choice::Remove).unwrap();
    assert!(!text.contains("Twice__b") && !text.contains("2 * x") && text.contains("+ Twice(box->a)"), "{text}");
    let typedef = "// ---- a.c ----\nstruct T { int a; };\n// ---- b.c ----\n/* VIEW(b): tag struct T as struct T__b; an earlier part has another definition. */\ntypedef struct T__b { int a; } TT;\n";
    let text = apply(typedef, &views_of(typedef)[0], &Choice::Remove).unwrap();
    assert!(text.ends_with("// ---- b.c ----\ntypedef struct T TT;\n"), "{text}");
}

#[test]
fn a_view_whose_definition_holds_a_fakematch_tag_is_not_written() {
    let tagged = "// ---- a.c ----\nstatic int F(int x) { return x; }\n// ---- b.c ----\n/* VIEW(b): static F as F__b; a has static int F(int x). */\nstatic int F__b(int x) {\n  /* FAKEMATCH(dead): the form kept; as x, 1 of 2 instructions differ in tbs-en. */\n  return x + 0;\n}\n";
    assert!(apply(tagged, &views_of(tagged)[0], &Choice::Remove).unwrap_err().contains("FAKEMATCH"));
}

#[test]
fn a_header_s_view_goes_with_its_wrapper() {
    let text = "// ---- a.c ----\nint x;\n// ---- b.c ----\n/* VIEW(b): routine Go as void Go(struct S *s); go.h has void Go(int). */\nstruct S;\nextern void Go__b(struct S *s) __asm__(\"Go\");\n#define Go Go__b\n#include \"go.h\"\n#undef Go\nvoid Run(void) { Go__b(0); }\n";
    let removed = apply(text, &views_of(text)[0], &Choice::Declare { declaration: "void Go(int);".into(), whose: "the definition's".into() }).unwrap();
    assert_eq!(removed, "// ---- a.c ----\nint x;\n// ---- b.c ----\n#include \"go.h\"\nvoid Run(void) { Go(0); }\n");
}

#[test]
fn a_macro_s_view_goes_with_its_definition_and_undefinition() {
    let text = "// ---- a.c ----\n#define ABS(x) ((x) < 0 ? -(x) : (x))\n// ---- b.c ----\n/* VIEW(b): macro ABS as ABS__b; another part or header has another definition. */\n#define ABS__b(x) ((x) >= 0 ? (x) : -(x))\nint F(int x) { return ABS__b(x); }\n#undef ABS__b\n";
    let removed = apply(text, &views_of(text)[0], &Choice::Remove).unwrap();
    assert_eq!(removed, "// ---- a.c ----\n#define ABS(x) ((x) < 0 ? -(x) : (x))\n// ---- b.c ----\nint F(int x) { return ABS(x); }\n");
}

/// Moves code wherever a text holds one of its words.
struct Moves(Vec<&'static str>);

impl Judge for Moves {
    fn judge(&self, text: &str) -> Result<Option<Failure>, String> {
        Ok(self.0.iter().find(|word| text.contains(*word)).map(|_| Failure { edition: "tla-en".into(), error: None, moved: vec![("Use".into(), 4, 61)] }))
    }
}

#[test]
fn views_that_move_nothing_go_and_the_rest_say_what_moved() {
    let views = views_of(JOINED);
    let lines: Vec<&str> = JOINED.lines().collect();
    let choices = |view: &View| match view.kind {
        "routine" => vec![Choice::Declare { declaration: "void Helper(int actor, int value);".into(), whose: "the definition's".into() }, Choice::Remove],
        _ => vec![Choice::Remove],
    };
    let units = units(&views, &lines, &choices);
    assert_eq!(units.len(), 3);
    // Twice's own definition gone moves Use.
    let solved = solve(JOINED, &views, &units, &Moves(vec!["+ Twice(box"])).unwrap();
    assert_eq!(solved.deleted, [0, 1]);
    assert_eq!(solved.kept.len(), 1);
    assert_eq!(solved.kept[0].2, ["Use"]);
    assert!(solved.text.contains("/* VIEW(b): static Twice as Twice__b; a has static int Twice(int x) { return x + x; }; tried with the definition the rest of the object has, Use moves: 4 of 61 instructions in tla-en. */"), "{}", solved.text);
    assert!(views_of(&solved.text).len() == 1 && !solved.text.contains("Helper__b") && solved.text.contains("int Use(struct Box *box)"));
    // A second settle replaces the note rather than adding one.
    let again = annotate(&solved.text, &views_of(&solved.text)[0], "tried as before, Use moves");
    assert!(again.contains("{ return x + x; }; tried as before, Use moves. */"), "{again}");
    // Where the definition's declaration moves code, the object's own is tried.
    let solved = solve(JOINED, &views, &units, &Moves(vec!["b.c ----\nvoid Helper"])).unwrap();
    assert_eq!(solved.deleted, [0, 1, 2]);
    assert!(solved.text.contains("// ---- b.c ----\nint Use"), "{}", solved.text);
}

#[test]
fn a_definition_gives_the_first_choice_and_a_repeat_is_not_written() {
    let files = vec![
        ("games/tla/include/thing.h".to_string(), "void Helper(int actor, int value);\nextern u8 gTable[];\n".to_string()),
        ("games/tla/src/a.c".to_string(), "void Helper(int actor, int value) { }\nconst u16 gData[4] = { 1 };\nextern char gTable;\n".to_string()),
        ("games/tla/src/b.c".to_string(), "/* VIEW(b): ... */\nextern int Helper__b(struct Thing *thing) __asm__(\"Helper\");\nextern u8 gTable[];\n".to_string()),
    ];
    let names: BTreeSet<String> = ["Helper", "gData", "gTable"].map(String::from).into();
    let found = Declarations::read(&files, &names);
    assert_eq!(found.definition["Helper"], "void Helper(int actor, int value);");
    assert_eq!(found.definition["gData"], "extern const u16 gData[4];");
    assert_eq!(found.declared["Helper"]["int Helper(struct Thing *thing);"], 1);
    assert_eq!(found.declared["gTable"]["extern u8 gTable[];"], 2);
    // The definition's head is in a header: nothing new is written first.
    let choices = found.choices("Helper", "");
    assert_eq!(choices[0], Choice::Remove);
    assert!(matches!(&choices[1], Choice::Declare { whose, .. } if whose == "the definition's"));
    let choices = found.choices("gData", "");
    assert!(matches!(&choices[0], Choice::Declare { declaration, .. } if declaration == "extern const u16 gData[4];"));
    assert_eq!(choices[1], Choice::Remove);
    let choices = found.choices("gTable", "");
    assert_eq!(choices[0], Choice::Remove);
    assert!(matches!(&choices[1], Choice::Declare { declaration, whose } if declaration == "extern u8 gTable[];" && whose == "the 2 declarations'"));
}

#[test]
fn instructions_differ_by_their_longest_common_run() {
    assert_eq!(differing(&["a", "b", "c", "d"], &["a", "c", "d"]), 1);
    assert_eq!(differing(&["a", "b"], &["c", "d", "e"]), 3);
    assert_eq!(differing(&[], &[]), 0);
}

#[test]
fn settle_takes_a_game_and_objects() {
    let words = |list: &[&str]| list.iter().map(|w| w.to_string()).collect::<Vec<_>>();
    let request = parse(&words(&["--game", "tla", "--dry-run", "games/tla/src/a.c"])).unwrap();
    assert_eq!((request.game.as_str(), request.dry_run, request.objects), ("tla", true, vec!["games/tla/src/a".to_string()]));
    assert!(parse(&words(&["--dry-run"])).is_err());
    assert!(parse(&words(&["--game", "tla", "--force"])).is_err());
}

#[test]
fn a_tag_with_the_other_s_layout_reads_its_members_by_the_other_s_names() {
    let text = "// ---- a.c ----\nstruct P { s32 x; s16 y[2]; void (*run)(int); };\nstruct Q { s32 z; };\n// ---- b.c ----\n/* VIEW(b): tag struct P as struct P__b; an earlier part has another definition. */\nstruct P__b { s32 posX; s16 y[2]; void (*go)(int); };\nint F(struct P__b *p) { return p->posX + p->y[0] + p . go; }\n";
    let view = &views_of(text)[0];
    let renames = member_renames(text, view, text).unwrap();
    assert_eq!(renames, [("posX".to_string(), "x".to_string()), ("go".to_string(), "run".to_string())]);
    let settled = apply(text, view, &Choice::Members { renames }).unwrap();
    assert!(settled.ends_with("// ---- b.c ----\nint F(struct P *p) { return p->x + p->y[0] + p . run; }\n"), "{settled}");
    // A layout that differs, or an old name another tag also has, is not renamed.
    let other = "struct P { s32 x; s32 y[2]; void (*run)(int); };\n";
    assert!(member_renames(text, view, other).is_none());
    let shared = text.replace("struct Q { s32 z; }", "struct Q { s32 posX; }");
    assert!(member_renames(&shared, view, &shared).is_none());
}
