//! The join with views. The tests marked `#[ignore]` run against the build's
//! own compiler: small runs of two and three files that disagree in each
//! way, each joined and every function held against the files it came from.
//! They need a bootstrapped checkout's alchemy-gcc (ALCHEMY_ROOT, or the
//! checkout this crate sits in) and fail without it:
//! `ALCHEMY_ROOT=... cargo test -- --ignored`. The rest need nothing.

use super::*;

/// Why the compiler tests are not run by default.
const NEEDS_GCC: &str = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)";

/// The checkout's alchemy-gcc, run on fixtures as The Lost Age's overlay C.
struct Fixture {
    root: tempfile::TempDir,
    compilers: PathBuf,
    /// Each edition and the macro it defines.
    editions: Vec<(String, String)>,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Fixture {
        let checkout = std::env::var_os("ALCHEMY_ROOT").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join(".."));
        let compilers = checkout.join("tools/out/compilers");
        assert!(compilers.join("xgcc").is_file(), "no {}: {NEEDS_GCC}", compilers.join("xgcc").display());
        let root = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = root.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        Fixture { root, compilers, editions: vec![("tla-en".into(), "EDITION_EN".into())] }
    }

    fn command(&self, edition: &str, source: &Path, mode: Mode, output: &Path) -> Vec<String> {
        let xgcc = self.compilers.join("xgcc").to_string_lossy().into_owned();
        let mut words = vec![xgcc, format!("-B{}/", self.compilers.display())];
        words.extend(["-Os", "-mthumb", "-nostdinc", "-fcall-used-r4", "-mtune=gba"].map(String::from));
        words.push(format!("-I{}", self.root.path().join("include").display()));
        if let Some((_, define)) = self.editions.iter().find(|(e, _)| e == edition) {
            words.push(format!("-D{define}=1"));
        }
        match mode {
            Mode::Assembly => words.push("-S".into()),
            Mode::Declarations => words.extend(["-S".into(), "-aux-info".into(), output.with_extension("aux").to_string_lossy().into_owned()]),
            Mode::Preprocess => words.push("-E".into()),
            Mode::Macros => words.extend(["-E".into(), "-dM".into()]),
        }
        words.extend(["-o".into(), output.to_string_lossy().into_owned(), source.to_string_lossy().into_owned()]);
        words
    }

    fn settle(&self, stems: &[&str]) -> Result<Settled, Failed> {
        let parts = stems.iter().map(|stem| Part::new(stem, std::fs::read_to_string(self.root.path().join(format!("{stem}.c"))).unwrap())).collect();
        settle(parts, &format!("{}.c", stems[0]), self)
    }

    fn sources(&self, stems: &[&str]) -> Vec<(String, String)> {
        stems.iter().map(|s| (s.to_string(), std::fs::read_to_string(self.root.path().join(format!("{s}.c"))).unwrap())).collect()
    }
}

impl Compiler for Fixture {
    fn editions(&self) -> Vec<String> {
        self.editions.iter().map(|(edition, _)| edition.clone()).collect()
    }

    fn compile(&self, edition: &str, source: &Path, mode: Mode) -> Result<Compiled, String> {
        static SERIAL: AtomicUsize = AtomicUsize::new(0);
        let output = self.root.path().join(format!("out{}.s", SERIAL.fetch_add(1, Ordering::Relaxed)));
        let (ok, _, stderr) = crate::gba::process::capture(&self.command(edition, source, mode, &output), self.root.path())?;
        let written = if mode == Mode::Declarations { output.with_extension("aux") } else { output };
        Ok(Compiled { ok, output: std::fs::read_to_string(written).unwrap_or_default(), stderr })
    }

    fn built(&self, edition: &str, stem: &str) -> Result<String, String> {
        let compiled = self.compile(edition, &self.root.path().join(format!("{stem}.c")), Mode::Assembly)?;
        compiled.ok.then_some(compiled.output).ok_or(compiled.stderr)
    }

    fn root(&self) -> &Path {
        self.root.path()
    }
}

/// The views a settled text marks, as (part, kind, name).
fn marked(text: &str) -> Vec<(String, String, String)> {
    let pattern = regex::Regex::new(r"/\* VIEW\(([^)]+)\): (\w+) ((?:struct )?\w+) as").unwrap();
    pattern.captures_iter(text).map(|c| (c[1].to_string(), c[2].to_string(), c[3].to_string())).collect()
}

/// A compiler that compiles nothing: every compile succeeds with no
/// message and no code, and the build's assembly of a stem is what `built`
/// says (an error where it says one), so a join is judged without gcc.
struct Silent {
    root: tempfile::TempDir,
    built: Vec<(&'static str, Result<&'static str, &'static str>)>,
    /// Every group a compiler was asked for, by its stems.
    asked: std::sync::Mutex<Vec<Vec<String>>>,
}

impl Silent {
    fn new(stems: &[&str], built: Vec<(&'static str, Result<&'static str, &'static str>)>) -> Silent {
        let root = tempfile::tempdir().unwrap();
        for stem in stems {
            std::fs::write(root.path().join(format!("{stem}.c")), "int x;\n").unwrap();
        }
        Silent { root, built, asked: std::sync::Mutex::new(Vec::new()) }
    }
}

impl Compiler for Silent {
    fn editions(&self) -> Vec<String> {
        vec!["tla-en".into()]
    }
    fn compile(&self, _: &str, _: &Path, _: Mode) -> Result<Compiled, String> {
        Ok(Compiled { ok: true, output: String::new(), stderr: String::new() })
    }
    fn built(&self, _: &str, stem: &str) -> Result<String, String> {
        match self.built.iter().find(|(s, _)| *s == stem).map(|(_, b)| *b) {
            Some(Ok(asm)) => Ok(asm.to_string()),
            Some(Err(why)) => Err(why.to_string()),
            None => Ok(String::new()),
        }
    }
    fn root(&self) -> &Path {
        self.root.path()
    }
}

fn texts(stems: &[&str]) -> Vec<(String, String)> {
    stems.iter().map(|s| (s.to_string(), "int x;\n".to_string())).collect()
}

/// A group's compiler is its own first object's: after c is left out of
/// a..d, a and b are judged by a's, and d stands alone (S7).
#[test]
fn each_group_takes_its_own_first_object_s_compiler() {
    let silent = Silent::new(&["a", "b", "c", "d"], vec![("c", Ok(".text\nC:\n\tbx\tlr\n"))]);
    let prepare = |group: &[(String, String)]| (format!("{}.c", group[0].0), group.iter().map(|(stem, text)| Part::new(stem, text.clone())).collect());
    let compilers = |stems: &[String]| -> Result<Box<dyn Compiler + '_>, String> {
        silent.asked.lock().unwrap().push(stems.to_vec());
        Ok(Box::new(&silent))
    };
    let reconciled = reconcile(&texts(&["a", "b", "c", "d"]), &prepare, &compilers);
    assert_eq!(reconciled.left_out.len(), 1, "{reconciled:?}");
    assert_eq!(reconciled.left_out[0].0, "c");
    assert_eq!(*silent.asked.lock().unwrap(), [vec!["a", "b", "c", "d"], vec!["a", "b"]]);
    assert_eq!(reconciled.joined.len(), 1);
    assert_eq!(reconciled.joined[0].stems, ["a", "b"]);
}

/// A stale build of the third object is the group's failure, not the first
/// part's: nothing is left out one by one, nothing joins, and the reason is
/// kept (S5). Before, part 0 went with the wrong reason, then part 1...
#[test]
fn an_error_no_part_owns_ends_the_group_without_blaming_a_part() {
    let silent = Silent::new(&["a", "b", "c"], vec![("c", Err("out/tla-en/obj/c.s is missing or older than c.c: make tla-en first"))]);
    let prepare = |group: &[(String, String)]| (format!("{}.c", group[0].0), group.iter().map(|(stem, text)| Part::new(stem, text.clone())).collect());
    let compilers = |_: &[String]| -> Result<Box<dyn Compiler + '_>, String> { Ok(Box::new(&silent)) };
    let reconciled = reconcile(&texts(&["a", "b", "c"]), &prepare, &compilers);
    assert!(reconciled.left_out.is_empty(), "{reconciled:?}");
    assert!(reconciled.joined.is_empty());
    assert_eq!(reconciled.alone, ["a", "b", "c"]);
    assert_eq!(reconciled.failed.len(), 1);
    assert!(reconciled.failed[0].1.contains("make tla-en first"), "{reconciled:?}");
    // A group whose compiler cannot be made fails the same way.
    let compilers = |_: &[String]| -> Result<Box<dyn Compiler + '_>, String> { Err("tla-ja links a but not b, so they cannot be one object".into()) };
    let reconciled = reconcile(&texts(&["a", "b"]), &prepare, &compilers);
    assert!(reconciled.left_out.is_empty() && reconciled.failed[0].1.contains("cannot be one object"), "{reconciled:?}");
}

/// part 1: static int gX = 2; part 2: int gX = 1. alchemy-gcc says
/// "redefinition of `gX'". The static's side takes the private name, and
/// only it is read as a static; the object keeps exporting gX (M1). Before,
/// part 2's global became gX__b and the check mapped it back to gX.
#[test]
fn a_static_against_an_external_definition_renames_the_static() {
    let silent = Silent::new(&[], Vec::new());
    let parts = vec![Part::new("a", "static int gX = 2;\nint A(void) { return gX; }\n".into()), Part::new("b", "int gX = 1;\nint B(void) { return gX; }\n".into())];
    let mut join = Join { parts, compiler: &silent, shared: BTreeMap::new(), before: Vec::new(), views: BTreeMap::new() };
    let message = Message { file: "join.c".into(), line: 4, warning: false, text: "redefinition of `gX'".into() };
    let segments: Segments = vec![(1, 0, 1, 2), (4, 1, 1, 2)];
    let places = Places { later: &message, earlier: None, pre: None, segments: &segments, scratch: "join.c" };
    let Answer::Fix(fix) = join.redefined("gX", &Site::Part { part: 1, line: 1 }, &Site::Part { part: 0, line: 1 }, &places) else { panic!("no fix") };
    assert!(matches!(&fix, Fix::Rename { site: Site::Part { part: 0, .. }, kind: "static", .. }), "{fix:?}");
    join.apply(&fix).unwrap();
    assert!(join.parts[0].text.contains("static int gX__a = 2;") && join.parts[0].text.contains("return gX__a;"), "{}", join.parts[0].text);
    assert_eq!(join.parts[1].text, "int gX = 1;\nint B(void) { return gX; }\n");
    assert_eq!(join.parts[0].statics, [("gX".to_string(), "gX__a".to_string())]);
    assert!(join.parts[1].statics.is_empty());
    // Renaming a definition that is not static as a static is refused.
    let wrong = Fix::Rename { name: "gX".into(), site: Site::Part { part: 1, line: 1 }, space: Space::Ordinary, kind: "static", other: Other { owner: "a".into(), text: "x".into() } };
    assert!(join.apply(&wrong).unwrap_err().contains("not static"));
}

/// A part that redefines a header's macro gives the header's back where it
/// ends, so the parts after it still see it (S4). Before, its #undef took
/// the header's away for every later part.
#[test]
fn a_part_s_end_gives_back_a_macro_in_force_before_it() {
    let silent = Silent::new(&[], Vec::new());
    let parts = vec![
        Part::new("a", "#include \"macros.h\"\nint A(int v) { return ABS(v); }\n".into()),
        Part::new("b", "#undef ABS\n#define ABS(x) ((x) >= 0 ? (x) : -(x))\nint B(int v) { return ABS(v); }\n".into()),
        Part::new("c", "#define LOCAL 3\nint C(int v) { return ABS(v) + LOCAL; }\n".into()),
    ];
    let before = vec![BTreeMap::new(), BTreeMap::from([("ABS".to_string(), "ABS(x) ((x) < 0 ? -(x) : (x))".to_string())]), BTreeMap::new()];
    let join = Join { parts, compiler: &silent, shared: BTreeMap::new(), before, views: BTreeMap::new() };
    let (text, _) = join.render();
    assert!(text.contains("int B(int v) { return ABS(v); }\n#undef ABS\n#define ABS(x) ((x) < 0 ? -(x) : (x))\n"), "{text}");
    assert!(text.ends_with("#undef LOCAL\n"), "{text}");
}

/// A join is compiled outside the checkout, as a file of the object's
/// folder: its quoted includes beside the object reach them by their whole
/// path, others stay for the search path (S13).
#[test]
fn a_scratch_file_stands_outside_the_checkout_for_a_file_in_it() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("games/tla/src/scene/cave")).unwrap();
    std::fs::write(root.path().join("games/tla/src/scene/cave/local.h"), "").unwrap();
    std::fs::write(root.path().join("games/tla/src/scene/shared.h"), "").unwrap();
    let scratch = Scratch::new(root.path(), "games/tla/src/scene/cave.c").unwrap();
    assert!(!scratch.path.starts_with(root.path()), "{}", scratch.path.display());
    scratch.write("#include \"cave/local.h\"\n#include \"shared.h\"\n#include \"global.h\"\n#include <x.h>\nint x;\n").unwrap();
    let written = std::fs::read_to_string(&scratch.path).unwrap();
    let folder = root.path().join("games/tla/src/scene");
    assert_eq!(written.lines().count(), 5);
    assert!(written.contains(&format!("#include \"{}\"", folder.join("cave/local.h").display())), "{written}");
    assert!(written.contains(&format!("#include \"{}\"", folder.join("shared.h").display())), "{written}");
    assert!(written.contains("#include \"global.h\"\n#include <x.h>\n"), "{written}");
    let path = scratch.path.clone();
    drop(scratch);
    assert!(!path.exists());
    assert!(std::fs::read_dir(&folder).unwrap().all(|entry| !entry.unwrap().file_name().to_string_lossy().starts_with('.')));
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn a_static_and_a_global_of_one_name_keep_the_global_exported() {
    let fixture = Fixture::new(&[("a.c", "static int gX = 2;\nint A(void) { return gX; }\n"), ("b.c", "int gX = 1;\nint B(void) { return gX; }\n")]);
    let settled = fixture.settle(&["a", "b"]).unwrap();
    assert!(settled.text.contains("static int gX__a = 2;") && settled.text.contains("\nint gX = 1;"), "{}", settled.text);
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn a_header_macro_a_part_redefines_reaches_the_parts_after_it() {
    let fixture = Fixture::new(&[
        ("include/macros.h", "#ifndef MACROS_H\n#define MACROS_H\n#define ABS(x) ((x) < 0 ? -(x) : (x))\n#endif\n"),
        ("a.c", "#include \"macros.h\"\nint A(int v) { return ABS(v); }\n"),
        ("b.c", "#include \"macros.h\"\n#undef ABS\n#define ABS(x) ((x) >= 0 ? (x) : -(x))\nint B(int v) { return ABS(v); }\n"),
        ("c.c", "#include \"macros.h\"\nint C(int v) { return ABS(v) + 1; }\n"),
    ]);
    let settled = fixture.settle(&["a", "b", "c"]).unwrap();
    assert!(settled.text.contains("int B(int v) { return ABS(v); }\n#undef ABS\n#define ABS(x) ((x) < 0 ? - (x) : (x))\n"), "{}", settled.text);
}

/// b declares Helper its own way only in one edition and calls it in both:
/// the view edition TWO asks for leaves edition EN calling an undeclared
/// private name. Every edition is cleaned again until none adds a view, so
/// the part is left out by its own error, never part a (S6, S5).
#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn a_view_one_edition_needs_is_judged_in_every_edition() {
    let mut fixture = Fixture::new(&[
        ("a.c", "void Helper(int a) { extern int g; g = a; }\n"),
        ("b.c", "#ifdef EDITION_TWO\nint Helper(int *p);\n#endif\nint B(void) { return Helper(0); }\n"),
    ]);
    fixture.editions.push(("tla-de".into(), "EDITION_TWO".into()));
    let error = fixture.settle(&["a", "b"]).unwrap_err();
    assert!(matches!(&error, Failed::Part(1, _)), "{error:?}");
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn a_routine_seen_two_ways_keeps_a_private_name_for_the_same_symbol() {
    // The seed: two files' prototypes of one helper, defined in neither.
    let fixture = Fixture::new(&[
        ("a.c", "/// @file\n/// One.\n\nvoid Helper(int actor, int value);\nint One(int x) { Helper(x, 3); return x + 1; }\n"),
        ("b.c", "/// @file\n/// Two.\n\nstruct Thing { int a; short b; };\nint Helper(struct Thing *thing);\nint Two(struct Thing *t) { return Helper(t) + t->b; }\n"),
    ]);
    let settled = fixture.settle(&["a", "b"]).unwrap();
    assert!(settled.text.starts_with("/// @file\n/// One.\n"), "{}", settled.text);
    assert!(settled.text.contains("// ---- a.c ----\n") && settled.text.contains("// ---- b.c ----\n"));
    assert!(settled.text.contains("extern int Helper__b(struct Thing *thing) __asm__(\"Helper\");"), "{}", settled.text);
    assert!(settled.text.contains("return Helper__b(t) + t->b;"));
    assert_eq!(marked(&settled.text), [("b".into(), "routine".into(), "Helper".into())]);
    assert!(settled.text.contains("/* VIEW(b): routine Helper as int Helper(struct Thing *thing); a has void Helper(int actor, int value). */"), "{}", settled.text);
    assert_eq!(settled.views.get("routine"), Some(&1));
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn the_definition_s_part_keeps_its_declaration_and_the_other_takes_the_view() {
    let fixture = Fixture::new(&[
        ("a.c", "short Get(int which);\nint Use(void) { return Get(2) + 1; }\n"),
        ("b.c", "int Get(int which) { return which * 3; }\n"),
    ]);
    let settled = fixture.settle(&["a", "b"]).unwrap();
    assert!(settled.text.contains("extern short Get__a(int which) __asm__(\"Get\");"), "{}", settled.text);
    assert!(settled.text.contains("int Get(int which) { return which * 3; }"));
    assert!(settled.text.contains("the definition has int Get(int which)."), "{}", settled.text);
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn tags_typedefs_enumerators_statics_and_macros_each_keep_their_own() {
    let fixture = Fixture::new(&[
        ("a.c", "struct Box { int w; };\ntypedef int Count;\nenum { MODE_A, MODE_B };\nstatic int sLast = 1;\n#define SCALE 2\nint A(struct Box *b, Count c) { return b->w * SCALE + c + MODE_B + sLast; }\n"),
        ("b.c", "struct Box { short w; short h; };\ntypedef short Count;\nenum { MODE_B = 5 };\nstatic int sLast = 7;\n#define SCALE 3\nint B(struct Box *b, Count c) { return b->h * SCALE + c + MODE_B + sLast; }\n"),
        ("c.c", "#define SCALE_DEFAULT 4\nint C(int x) { return x * SCALE_DEFAULT; }\n"),
    ]);
    let settled = fixture.settle(&["a", "b", "c"]).unwrap();
    let mut kinds: Vec<String> = marked(&settled.text).into_iter().map(|(part, kind, name)| format!("{part} {kind} {name}")).collect();
    kinds.sort();
    assert_eq!(kinds, ["b enumerator MODE_B", "b static sLast", "b tag struct Box", "b typedef Count"], "{}", settled.text);
    // The macros need no view: each part's are undefined where it ends.
    assert!(settled.text.contains("#undef SCALE\n// ---- b.c ----") || settled.text.contains("#undef SCALE\n\n// ---- b.c ----"), "{}", settled.text);
    assert!(settled.text.contains("static int sLast__b = 7;"));
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn identical_repeats_are_deleted() {
    let shared = "/// A point.\nstruct Point { int x; int y; };\ntypedef struct Point Point;\nstatic __inline__ int Twice(int v) { return v * 2; }\nint Far(int);\n";
    let fixture = Fixture::new(&[
        ("a.c", &format!("{shared}int A(Point *p) {{ return Twice(p->x) + Far(1); }}\n")),
        ("b.c", &format!("{shared}int B(Point *p) {{ return Twice(p->y) + Far(2); }}\n")),
    ]);
    let settled = fixture.settle(&["a", "b"]).unwrap();
    assert!(marked(&settled.text).is_empty(), "{}", settled.text);
    assert_eq!(settled.text.matches("struct Point {").count(), 1, "{}", settled.text);
    assert_eq!(settled.text.matches("Twice(int v)").count(), 1);
    assert_eq!(settled.text.matches("/// A point.").count(), 1);
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn a_call_that_had_no_prototype_keeps_none() {
    // c.c calls Wide unprototyped, so its argument is passed as an int; the
    // prototype a.c brings would narrow it. The compiler stays silent: only
    // the assembly shows it, and the view is found from -aux-info.
    let fixture = Fixture::new(&[
        ("a.c", "void Wide(signed char value);\nvoid A(int x) { Wide(x); }\n"),
        ("c.c", "void C(int x) { Wide(x); }\n"),
    ]);
    let settled = fixture.settle(&["a", "c"]).unwrap();
    assert!(settled.text.contains("extern int Wide__c() __asm__(\"Wide\");"), "{}", settled.text);
    assert_eq!(marked(&settled.text), [("c".into(), "routine".into(), "Wide".into())]);
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn a_header_s_declaration_is_viewed_around_its_include() {
    let fixture = Fixture::new(&[
        ("include/actor.h", "#ifndef ACTOR_H\n#define ACTOR_H\nstruct Actor { int x; };\nvoid Actor_Move(struct Actor *actor, int x);\nstatic __inline__ void Actor_Nudge(struct Actor *actor) { Actor_Move(actor, 1); }\n#endif\n"),
        ("include/local.h", "#ifndef LOCAL_H\n#define LOCAL_H\nvoid Actor_Move(void *actor, short x);\n#endif\n"),
        ("a.c", "#include \"local.h\"\nvoid A(void *p) { Actor_Move(p, 3); }\n"),
        ("b.c", "#include \"actor.h\"\nvoid B(struct Actor *p) { Actor_Move(p, p->x); Actor_Nudge(p); }\n"),
    ]);
    let settled = fixture.settle(&["a", "b"]).unwrap();
    // Bound before the include, its tag declared at file scope first.
    assert!(
        settled.text.contains("struct Actor;\nextern void Actor_Move__b(struct Actor *actor, int x) __asm__(\"Actor_Move\");\n#define Actor_Move Actor_Move__b\n#include \"actor.h\"\n#undef Actor_Move\n"),
        "{}",
        settled.text
    );
    assert!(settled.text.contains("Actor_Move__b(p, p->x);"));
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn a_part_no_view_reconciles_is_left_out_and_its_neighbours_still_join() {
    let fixture = Fixture::new(&[
        ("a.c", "int A(void) { return 1; }\n"),
        ("b.c", "int A(void) { return 2; }\n"),
        ("c.c", "int C(void) { return 3; }\n"),
    ]);
    let sources = fixture.sources(&["a", "b", "c"]);
    let prepare = |group: &[(String, String)]| (format!("{}.c", group[0].0), group.iter().map(|(stem, text)| Part::new(stem, text.clone())).collect());
    let compilers = |_: &[String]| -> Result<Box<dyn Compiler + '_>, String> { Ok(Box::new(&fixture)) };
    let reconciled = reconcile(&sources, &prepare, &compilers);
    assert_eq!(reconciled.left_out.len(), 1, "{:?}", reconciled.left_out);
    assert_eq!(reconciled.left_out[0].0, "b");
    assert!(reconciled.left_out[0].1.contains("redefinition"), "{}", reconciled.left_out[0].1);
    // a alone, then b out, then c alone: nothing left to join.
    assert!(reconciled.joined.is_empty());
    assert_eq!(reconciled.alone, ["a", "b", "c"]);
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn a_header_view_whose_types_the_header_defines_is_bound_after_its_include() {
    let fixture = Fixture::new(&[
        ("include/task.h", "#ifndef TASK_H\n#define TASK_H\ntypedef void (*TaskFunc)(void);\nint Task_Add(TaskFunc func, int order);\n#endif\n"),
        ("a.c", "void Task_Add(void (*func)(void), short order) { func(); }\nvoid A(void) { Task_Add(A, 3); }\n"),
        ("b.c", "#include \"task.h\"\nint B(void) { return Task_Add(0, 2); }\n"),
    ]);
    let settled = fixture.settle(&["a", "b"]).unwrap();
    assert!(
        settled.text.contains("#define Task_Add Task_Add__b\n#include \"task.h\"\n#undef Task_Add\nextern int Task_Add__b(TaskFunc func, int order) __asm__(\"Task_Add\");\n"),
        "{}",
        settled.text
    );
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn a_name_two_of_a_part_s_headers_declare_is_viewed_around_each_include() {
    let fixture = Fixture::new(&[
        ("include/actor.h", "#ifndef ACTOR_H\n#define ACTOR_H\nvoid Actor_Hide(unsigned actor, int how);\n#endif\n"),
        ("include/event.h", "#ifndef EVENT_H\n#define EVENT_H\nvoid Actor_Hide(unsigned actor, int how);\n#endif\n"),
        ("a.c", "void Actor_Hide(int actor, short how) { extern int gHidden; gHidden = actor + how; }\nvoid A(void) { Actor_Hide(1, 2); }\n"),
        ("b.c", "#include \"actor.h\"\n#include \"event.h\"\nvoid B(void) { Actor_Hide(3, 4); }\n"),
    ]);
    let settled = fixture.settle(&["a", "b"]).unwrap();
    assert_eq!(settled.text.matches("#define Actor_Hide Actor_Hide__b\n").count(), 2, "{}", settled.text);
    // Each wrapper undefines its own macro; the part's end adds nothing.
    assert_eq!(settled.text.matches("#undef Actor_Hide\n").count(), 2, "{}", settled.text);
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn a_definition_a_macro_names_is_read_from_the_expansion() {
    let fixture = Fixture::new(&[
        ("include/spawn.inc", "void EFFECT_SPAWN(int x) { extern int gSpawned; gSpawned = x; }\n"),
        ("a.c", "#define EFFECT_SPAWN Spawn\n#include \"spawn.inc\"\n"),
        ("b.c", "void Spawn(short x);\nvoid B(void) { Spawn(3); }\n"),
    ]);
    let settled = fixture.settle(&["a", "b"]).unwrap();
    assert!(settled.text.contains("extern void Spawn__b(short x) __asm__(\"Spawn\");"), "{}", settled.text);
    assert_eq!(marked(&settled.text), [("b".into(), "routine".into(), "Spawn".into())]);
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn a_repeat_of_an_inline_renamed_in_the_same_round_is_kept_for_the_next() {
    let fixture = Fixture::new(&[
        ("include/pick.h", "static __inline__ int Pick(int a) { return a; }\n"),
        ("a.c", "#include \"pick.h\"\nint A(void) { return Pick(1); }\n"),
        ("b.c", "static __inline__ int Pick(int a, int b) { return a + b; }\nint B(void) { return Pick(1, 2); }\n"),
        ("c.c", "static __inline__ int Pick(int a, int b) { return a + b; }\nint C(void) { return Pick(3, 4); }\n"),
    ]);
    let settled = fixture.settle(&["a", "b", "c"]).unwrap();
    assert!(settled.text.contains("Pick__b(1, 2)") && settled.text.contains("Pick__c(3, 4)"), "{}", settled.text);
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn a_tag_s_repeat_is_judged_by_the_definition_the_message_names() {
    let fixture = Fixture::new(&[
        ("a.c", "struct T { int a; };\nint A(struct T *t) { return t->a; }\n"),
        ("b.c", "struct T { int a; };\nint B(struct T *t) { return t->a + 1; }\n"),
        ("c.c", "struct T { short c; };\nint C(struct T *t) { return t->c; }\n"),
    ]);
    let settled = fixture.settle(&["a", "b", "c"]).unwrap();
    // b's is a's again and goes; c's is its own.
    assert_eq!(settled.text.matches("struct T {").count(), 1, "{}", settled.text);
    assert!(settled.text.contains("struct T__c { short c; };"), "{}", settled.text);
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn a_macro_two_headers_define_is_undefined_before_the_later_include_and_given_back() {
    let fixture = Fixture::new(&[
        ("include/one.h", "#ifndef ONE_H\n#define ONE_H\nint One(void);\n#define TWICE(x) ((x) * 2)\n#endif\n"),
        ("include/two.h", "#ifndef TWO_H\n#define TWO_H\nint Two(void);\n#define TWICE(x) ((x) + (x))\n#endif\n"),
        ("a.c", "#include \"one.h\"\nint A(int v) { return TWICE(v); }\n"),
        ("b.c", "#include \"two.h\"\nint B(int v) { return TWICE(v); }\n"),
        ("c.c", "#include \"one.h\"\nint C(int v) { return TWICE(v); }\n"),
    ]);
    let settled = fixture.settle(&["a", "b", "c"]).unwrap();
    assert!(settled.text.contains("#undef TWICE\n#include \"two.h\"\n"), "{}", settled.text);
    assert!(settled.text.contains("#undef TWICE\n#define TWICE(x) ((x) *2)\n"), "{}", settled.text);
    assert_eq!(marked(&settled.text), [("b".into(), "macro".into(), "TWICE".into())]);
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn a_declaration_a_macro_names_in_a_shared_file_takes_its_view_around_the_include() {
    let fixture = Fixture::new(&[
        ("include/table.inc", "extern const short EFFECT_TABLE[];\nint EFFECT_READ(void) { return EFFECT_TABLE[1]; }\n"),
        ("a.c", "#define EFFECT_TABLE gTable\n#define EFFECT_READ A\n#include \"table.inc\"\n"),
        ("b.c", "const int gTable[2] = { 1, 2 };\n"),
    ]);
    let settled = fixture.settle(&["a", "b"]).unwrap();
    assert!(settled.text.contains("extern const short gTable__a[] __asm__(\"gTable\");\n#define gTable gTable__a\n#include \"table.inc\"\n#undef gTable\n"), "{}", settled.text);
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn between_a_part_s_declaration_and_a_header_s_the_part_keeps_the_view() {
    let fixture = Fixture::new(&[
        ("include/input.h", "#ifndef INPUT_H\n#define INPUT_H\nstruct Input { unsigned short held; };\nextern struct Input gInput;\n#endif\n"),
        ("a.c", "extern unsigned gInput;\nint A(void) { return gInput >> 4; }\n"),
        ("b.c", "#include \"input.h\"\nint B(void) { return gInput.held; }\n"),
        ("c.c", "#include \"input.h\"\nint C(void) { return gInput.held >> 4; }\n"),
    ]);
    let settled = fixture.settle(&["a", "b", "c"]).unwrap();
    // The header keeps its own name for b and c, whose second include it guards.
    assert!(settled.text.contains("extern unsigned gInput__a __asm__(\"gInput\");"), "{}", settled.text);
    assert!(!settled.text.contains("#define gInput"), "{}", settled.text);
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn a_part_that_declared_a_routine_without_its_parameters_calls_it_as_it_did() {
    let fixture = Fixture::new(&[
        ("a.c", "void Move(int *position) { position[0] += 1; }\n"),
        ("b.c", "void Move();\nvoid B(int *p) { Move(p, 2, 3); }\n"),
    ]);
    let settled = fixture.settle(&["a", "b"]).unwrap();
    assert!(settled.text.contains("extern void Move__b() __asm__(\"Move\");\nvoid B(int *p) { Move__b(p, 2, 3); }"), "{}", settled.text);
}

#[test]
#[ignore = "needs a bootstrapped Alchemy checkout's alchemy-gcc (ALCHEMY_ROOT)"]
fn an_inline_repeat_that_holds_a_fakematch_tag_is_kept() {
    let inline = "static __inline__ int Twice(int v) {\n  /* FAKEMATCH(wrapper): kept; as v * 2, 1 of 2 instructions differ in tbs-en. */\n  return v + v;\n}\n";
    let a = format!("{inline}int A(int v) {{ return Twice(v); }}\n");
    let b = format!("{inline}int B(int v) {{ return Twice(v) + 1; }}\n");
    let fixture = Fixture::new(&[("a.c", &a), ("b.c", &b)]);
    let settled = fixture.settle(&["a", "b"]).unwrap();
    assert_eq!(settled.text.matches("FAKEMATCH(wrapper)").count(), 2, "{}", settled.text);
    assert!(settled.text.contains("Twice__b(v) + 1"), "{}", settled.text);
}
