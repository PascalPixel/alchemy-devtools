//! The join with views against the build's own compiler: small runs of two
//! and three files that disagree in each way, each joined and every function
//! held against the files it came from. They need a bootstrapped checkout
//! (ALCHEMY_ROOT); without its compiler they say so and pass.

use super::*;

/// The checkout's alchemy-gcc, run on fixtures as The Lost Age's overlay C.
struct Fixture {
    root: tempfile::TempDir,
    compilers: PathBuf,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Option<Fixture> {
        let checkout = PathBuf::from(std::env::var_os("ALCHEMY_ROOT")?);
        let compilers = checkout.join("tools/out/compilers");
        if !compilers.join("xgcc").is_file() {
            eprintln!("views: no {}: skipped", compilers.join("xgcc").display());
            return None;
        }
        let root = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = root.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        Some(Fixture { root, compilers })
    }

    fn command(&self, source: &Path, mode: Mode, output: &Path) -> Vec<String> {
        let xgcc = self.compilers.join("xgcc").to_string_lossy().into_owned();
        let mut words = vec![xgcc, format!("-B{}/", self.compilers.display())];
        words.extend(["-Os", "-mthumb", "-nostdinc", "-fcall-used-r4", "-mtune=gba"].map(String::from));
        words.push(format!("-I{}", self.root.path().join("include").display()));
        match mode {
            Mode::Assembly => words.push("-S".into()),
            Mode::Declarations => words.extend(["-S".into(), "-aux-info".into(), output.with_extension("aux").to_string_lossy().into_owned()]),
            Mode::Preprocess => words.push("-E".into()),
            Mode::Macros => words.extend(["-E".into(), "-dM".into()]),
        }
        words.extend(["-o".into(), output.to_string_lossy().into_owned(), source.to_string_lossy().into_owned()]);
        words
    }

    fn settle(&self, stems: &[&str]) -> Result<Settled, (usize, String)> {
        let parts = stems.iter().map(|stem| Part::new(stem, std::fs::read_to_string(self.root.path().join(format!("{stem}.c"))).unwrap())).collect();
        settle(parts, &format!("{}.c", stems[0]), self)
    }
}

impl Compiler for Fixture {
    fn editions(&self) -> Vec<String> {
        vec!["tla-en".into()]
    }

    fn compile(&self, _: &str, source: &Path, mode: Mode) -> Result<Compiled, String> {
        static SERIAL: AtomicUsize = AtomicUsize::new(0);
        let output = self.root.path().join(format!("out{}.s", SERIAL.fetch_add(1, Ordering::Relaxed)));
        let (ok, _, stderr) = crate::gba::process::capture(&self.command(source, mode, &output), self.root.path())?;
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

#[test]
fn a_routine_seen_two_ways_keeps_a_private_name_for_the_same_symbol() {
    // The seed: two files' prototypes of one helper, defined in neither.
    let Some(fixture) = Fixture::new(&[
        ("a.c", "/// @file\n/// One.\n\nvoid Helper(int actor, int value);\nint One(int x) { Helper(x, 3); return x + 1; }\n"),
        ("b.c", "/// @file\n/// Two.\n\nstruct Thing { int a; short b; };\nint Helper(struct Thing *thing);\nint Two(struct Thing *t) { return Helper(t) + t->b; }\n"),
    ]) else {
        return;
    };
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
fn the_definition_s_part_keeps_its_declaration_and_the_other_takes_the_view() {
    let Some(fixture) = Fixture::new(&[
        ("a.c", "short Get(int which);\nint Use(void) { return Get(2) + 1; }\n"),
        ("b.c", "int Get(int which) { return which * 3; }\n"),
    ]) else {
        return;
    };
    let settled = fixture.settle(&["a", "b"]).unwrap();
    assert!(settled.text.contains("extern short Get__a(int which) __asm__(\"Get\");"), "{}", settled.text);
    assert!(settled.text.contains("int Get(int which) { return which * 3; }"));
    assert!(settled.text.contains("the definition has int Get(int which)."), "{}", settled.text);
}

#[test]
fn tags_typedefs_enumerators_statics_and_macros_each_keep_their_own() {
    let Some(fixture) = Fixture::new(&[
        ("a.c", "struct Box { int w; };\ntypedef int Count;\nenum { MODE_A, MODE_B };\nstatic int sLast = 1;\n#define SCALE 2\nint A(struct Box *b, Count c) { return b->w * SCALE + c + MODE_B + sLast; }\n"),
        ("b.c", "struct Box { short w; short h; };\ntypedef short Count;\nenum { MODE_B = 5 };\nstatic int sLast = 7;\n#define SCALE 3\nint B(struct Box *b, Count c) { return b->h * SCALE + c + MODE_B + sLast; }\n"),
        ("c.c", "#define SCALE_DEFAULT 4\nint C(int x) { return x * SCALE_DEFAULT; }\n"),
    ]) else {
        return;
    };
    let settled = fixture.settle(&["a", "b", "c"]).unwrap();
    let mut kinds: Vec<String> = marked(&settled.text).into_iter().map(|(part, kind, name)| format!("{part} {kind} {name}")).collect();
    kinds.sort();
    assert_eq!(kinds, ["b enumerator MODE_B", "b static sLast", "b tag struct Box", "b typedef Count"], "{}", settled.text);
    // The macros need no view: each part's are undefined where it ends.
    assert!(settled.text.contains("#undef SCALE\n// ---- b.c ----") || settled.text.contains("#undef SCALE\n\n// ---- b.c ----"), "{}", settled.text);
    assert!(settled.text.contains("static int sLast__b = 7;"));
}

#[test]
fn identical_repeats_are_deleted() {
    let shared = "/// A point.\nstruct Point { int x; int y; };\ntypedef struct Point Point;\nstatic __inline__ int Twice(int v) { return v * 2; }\nint Far(int);\n";
    let Some(fixture) = Fixture::new(&[
        ("a.c", &format!("{shared}int A(Point *p) {{ return Twice(p->x) + Far(1); }}\n")),
        ("b.c", &format!("{shared}int B(Point *p) {{ return Twice(p->y) + Far(2); }}\n")),
    ]) else {
        return;
    };
    let settled = fixture.settle(&["a", "b"]).unwrap();
    assert!(marked(&settled.text).is_empty(), "{}", settled.text);
    assert_eq!(settled.text.matches("struct Point {").count(), 1, "{}", settled.text);
    assert_eq!(settled.text.matches("Twice(int v)").count(), 1);
    assert_eq!(settled.text.matches("/// A point.").count(), 1);
}

#[test]
fn a_call_that_had_no_prototype_keeps_none() {
    // c.c calls Wide unprototyped, so its argument is passed as an int; the
    // prototype a.c brings would narrow it. The compiler stays silent: only
    // the assembly shows it, and the view is found from -aux-info.
    let Some(fixture) = Fixture::new(&[
        ("a.c", "void Wide(signed char value);\nvoid A(int x) { Wide(x); }\n"),
        ("c.c", "void C(int x) { Wide(x); }\n"),
    ]) else {
        return;
    };
    let settled = fixture.settle(&["a", "c"]).unwrap();
    assert!(settled.text.contains("extern int Wide__c() __asm__(\"Wide\");"), "{}", settled.text);
    assert_eq!(marked(&settled.text), [("c".into(), "routine".into(), "Wide".into())]);
}

#[test]
fn a_header_s_declaration_is_viewed_around_its_include() {
    let Some(fixture) = Fixture::new(&[
        ("include/actor.h", "#ifndef ACTOR_H\n#define ACTOR_H\nstruct Actor { int x; };\nvoid Actor_Move(struct Actor *actor, int x);\nstatic __inline__ void Actor_Nudge(struct Actor *actor) { Actor_Move(actor, 1); }\n#endif\n"),
        ("include/local.h", "#ifndef LOCAL_H\n#define LOCAL_H\nvoid Actor_Move(void *actor, short x);\n#endif\n"),
        ("a.c", "#include \"local.h\"\nvoid A(void *p) { Actor_Move(p, 3); }\n"),
        ("b.c", "#include \"actor.h\"\nvoid B(struct Actor *p) { Actor_Move(p, p->x); Actor_Nudge(p); }\n"),
    ]) else {
        return;
    };
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
fn a_part_no_view_reconciles_is_left_out_and_its_neighbours_still_join() {
    let Some(fixture) = Fixture::new(&[
        ("a.c", "int A(void) { return 1; }\n"),
        ("b.c", "int A(void) { return 2; }\n"),
        ("c.c", "int C(void) { return 3; }\n"),
    ]) else {
        return;
    };
    let sources: Vec<(String, String)> = ["a", "b", "c"].iter().map(|s| (s.to_string(), std::fs::read_to_string(fixture.root.path().join(format!("{s}.c"))).unwrap())).collect();
    let prepare = |group: &[(String, String)]| (format!("{}.c", group[0].0), group.iter().map(|(stem, text)| Part::new(stem, text.clone())).collect());
    let reconciled = reconcile(&sources, &prepare, &fixture);
    assert_eq!(reconciled.left_out.len(), 1, "{:?}", reconciled.left_out);
    assert_eq!(reconciled.left_out[0].0, "b");
    assert!(reconciled.left_out[0].1.contains("redefinition"), "{}", reconciled.left_out[0].1);
    // a alone, then b out, then c alone: nothing left to join.
    assert!(reconciled.joined.is_empty());
    assert_eq!(reconciled.alone, ["a", "b", "c"]);
}

#[test]
fn a_header_view_whose_types_the_header_defines_is_bound_after_its_include() {
    let Some(fixture) = Fixture::new(&[
        ("include/task.h", "#ifndef TASK_H\n#define TASK_H\ntypedef void (*TaskFunc)(void);\nint Task_Add(TaskFunc func, int order);\n#endif\n"),
        ("a.c", "void Task_Add(void (*func)(void), short order);\nvoid A(void) { Task_Add(A, 3); }\n"),
        ("b.c", "#include \"task.h\"\nint B(void) { return Task_Add(0, 2); }\n"),
    ]) else {
        return;
    };
    let settled = fixture.settle(&["a", "b"]).unwrap();
    assert!(
        settled.text.contains("#define Task_Add Task_Add__b\n#include \"task.h\"\n#undef Task_Add\nextern int Task_Add__b(TaskFunc func, int order) __asm__(\"Task_Add\");\n"),
        "{}",
        settled.text
    );
}

#[test]
fn a_name_two_of_a_part_s_headers_declare_is_viewed_around_each_include() {
    let Some(fixture) = Fixture::new(&[
        ("include/actor.h", "#ifndef ACTOR_H\n#define ACTOR_H\nvoid Actor_Hide(unsigned actor, int how);\n#endif\n"),
        ("include/event.h", "#ifndef EVENT_H\n#define EVENT_H\nvoid Actor_Hide(unsigned actor, int how);\n#endif\n"),
        ("a.c", "void Actor_Hide(int actor, short how);\nvoid A(void) { Actor_Hide(1, 2); }\n"),
        ("b.c", "#include \"actor.h\"\n#include \"event.h\"\nvoid B(void) { Actor_Hide(3, 4); }\n"),
    ]) else {
        return;
    };
    let settled = fixture.settle(&["a", "b"]).unwrap();
    assert_eq!(settled.text.matches("#define Actor_Hide Actor_Hide__b\n").count(), 2, "{}", settled.text);
    // Each wrapper undefines its own macro; the part's end adds nothing.
    assert_eq!(settled.text.matches("#undef Actor_Hide\n").count(), 2, "{}", settled.text);
}

#[test]
fn a_definition_a_macro_names_is_read_from_the_expansion() {
    let Some(fixture) = Fixture::new(&[
        ("include/spawn.inc", "void EFFECT_SPAWN(int x) { extern int gSpawned; gSpawned = x; }\n"),
        ("a.c", "#define EFFECT_SPAWN Spawn\n#include \"spawn.inc\"\n"),
        ("b.c", "void Spawn(short x);\nvoid B(void) { Spawn(3); }\n"),
    ]) else {
        return;
    };
    let settled = fixture.settle(&["a", "b"]).unwrap();
    assert!(settled.text.contains("extern void Spawn__b(short x) __asm__(\"Spawn\");"), "{}", settled.text);
    assert_eq!(marked(&settled.text), [("b".into(), "routine".into(), "Spawn".into())]);
}

#[test]
fn a_repeat_of_an_inline_renamed_in_the_same_round_is_kept_for_the_next() {
    let Some(fixture) = Fixture::new(&[
        ("include/pick.h", "static __inline__ int Pick(int a) { return a; }\n"),
        ("a.c", "#include \"pick.h\"\nint A(void) { return Pick(1); }\n"),
        ("b.c", "static __inline__ int Pick(int a, int b) { return a + b; }\nint B(void) { return Pick(1, 2); }\n"),
        ("c.c", "static __inline__ int Pick(int a, int b) { return a + b; }\nint C(void) { return Pick(3, 4); }\n"),
    ]) else {
        return;
    };
    let settled = fixture.settle(&["a", "b", "c"]).unwrap();
    assert!(settled.text.contains("Pick__b(1, 2)") && settled.text.contains("Pick__c(3, 4)"), "{}", settled.text);
}

#[test]
fn a_tag_s_repeat_is_judged_by_the_definition_the_message_names() {
    let Some(fixture) = Fixture::new(&[
        ("a.c", "struct T { int a; };\nint A(struct T *t) { return t->a; }\n"),
        ("b.c", "struct T { int a; };\nint B(struct T *t) { return t->a + 1; }\n"),
        ("c.c", "struct T { short c; };\nint C(struct T *t) { return t->c; }\n"),
    ]) else {
        return;
    };
    let settled = fixture.settle(&["a", "b", "c"]).unwrap();
    // b's is a's again and goes; c's is its own.
    assert_eq!(settled.text.matches("struct T {").count(), 1, "{}", settled.text);
    assert!(settled.text.contains("struct T__c { short c; };"), "{}", settled.text);
}
