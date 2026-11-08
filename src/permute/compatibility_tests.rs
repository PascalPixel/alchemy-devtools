//! Original search and declaration scenarios through both game toolchains.

use super::tests::{test_options, test_problem};
use super::*;

#[test]
fn initial_exact_expansion_is_saved_under_output_filters() {

    let work = tempfile::tempdir().unwrap();
    let draft = work.path().join("08000000.c");
    let source = "int target(void) { return PERM_GENERAL(3, 4); }\n";
    std::fs::write(&draft, source).unwrap();
    let mut reference_problem = test_problem(&draft, source, vec![], Some("target"));
    reference_problem.symbol = Some("target".into());
    let reference = Job::new(&reference_problem, &work.path().join("reference")).unwrap();
    reference.compile("int target(void) { return 3; }\n").unwrap();
    let listing = crate::diff::disassemble(&reference.object, Some("target")).unwrap();
    let rows = score::disassemble(&reference.object, Some("target")).unwrap();
    for (index, flags) in [vec![], vec!["--better-only", "--stop-on-zero"]].into_iter().enumerate() {
        let mut problem = test_problem(&draft, source, listing.clone(), Some("target"));
        problem.symbol = Some("target".into());
        problem.listing_rows = Some(rows.clone());
        let mut checker = test_problem(&draft, source, listing.clone(), Some("target"));
        checker.symbol = Some("target".into());
        checker.listing_rows = Some(rows.clone());
        let mut arguments = vec![draft.to_str().unwrap(), "--function", "target", "--jobs", "1", "--iterations", "2"];
        arguments.extend(flags);
        let options = parse_options(&arguments.into_iter().map(str::to_string).collect::<Vec<_>>()).unwrap();
        let output = work.path().join(format!("output-{index}"));
        execute(problem, &options, None, &output).unwrap();
        let archive = output.join("output-0-1/source.c");
        let best = output.join("best-1.c");
        assert!(archive.is_file(), "case {index}: an exact initial expansion must be retained");
        assert!(best.is_file(), "case {index}");
        for path in [archive, best] {
            let text = std::fs::read_to_string(path).unwrap();
            assert!(!perm::has_macros(&text));
            assert!(checker.evaluate(&reference, &text).unwrap().is_zero());
        }
    }
}

#[test]
fn saved_best_preserves_line_sensitive_compiler_output() {

    let work = tempfile::tempdir().unwrap();
    let draft = work.path().join("08000000.c");
    let source = "#define VALUE __LINE__\nint target(void)\n{\n    return VALUE;\n}\n";
    let mut problem = test_problem(&draft, source, vec![], Some("target"));
    problem.symbol = Some("target".into());
    let reference = Job::new(&problem, &work.path().join("reference")).unwrap();
    reference.compile(source).unwrap();
    let expected = score::disassemble(&reference.object, Some("target")).unwrap();
    problem.listing = crate::diff::disassemble(&reference.object, Some("target")).unwrap();
    problem.listing_rows = Some(expected.clone());
    let found = Found {
        score: problem.evaluate(&reference, source).unwrap(),
        text: source.into(),
        steps: vec![],
        expanded: Arc::new(perm::expand(&problem.nodes).unwrap()),
        job: 0,
        iteration: 0,
    };
    assert!(found.score.is_zero());
    let best = Best { found: found.clone(), searched: vec![], text: source.into(), respelled: true };
    let output = work.path().join("output");
    std::fs::create_dir_all(&output).unwrap();
    write_best(&problem, &found, &[best], &output).unwrap();
    let saved = std::fs::read_to_string(output.join("best-1.c")).unwrap();
    reference.compile(&saved).unwrap();
    assert_eq!(score::disassemble(&reference.object, Some("target")).unwrap(), expected);
    assert_eq!(saved, source);
    assert!(std::fs::read_to_string(output.join("best-1.txt")).unwrap().contains("Job 0, iteration 0"));
}

#[test]
fn exact_stack_operand_candidate_survives_zero_weighted_baseline() {

    let work = tempfile::tempdir().unwrap();
    let draft = work.path().join("draft/08000000.c");
    std::fs::create_dir_all(draft.parent().unwrap()).unwrap();
    let source = "int target(int x) { volatile int values[PERM_GENERAL(2,3,4)]; values[0] = x; return values[0]; }";
    let target = "int target(int x) { volatile int values[4]; values[0] = x; return values[0]; }";
    std::fs::write(&draft, source).unwrap();
    let mut problem = test_problem(&draft, source, vec![], Some("target"));
    problem.symbol = Some("target".into());
    let reference = Job::new(&problem, &work.path().join("reference")).unwrap();
    reference.compile(target).unwrap();
    problem.listing = crate::diff::disassemble(&reference.object, Some("target")).unwrap();
    problem.listing_rows = Some(score::disassemble(&reference.object, Some("target")).unwrap());
    let base = perm::expand(&problem.nodes).unwrap();
    let score = problem.evaluate(&reference, &base.restore(&base.text)).unwrap();
    assert_eq!(score.weighted, 0, "reference: {:?}; candidate: {:?}", problem.listing_rows, score::disassemble(&reference.object, Some("target")).unwrap());
    assert!(score.differ > 0);
    let arguments = [draft.to_str().unwrap(), "--function", "target", "--jobs", "1", "--iterations", "3", "--better-only", "--stop-on-zero"].map(str::to_string);
    let options = parse_options(&arguments).unwrap();
    let checker = test_problem(&draft, source, problem.listing.clone(), Some("target"));
    let output = work.path().join("output");
    execute(problem, &options, None, &output).unwrap();
    let best = std::fs::read_to_string(output.join("best-1.c")).unwrap();
    assert!(checker.evaluate(&reference, &best).unwrap().is_zero());
    let archived = std::fs::read_to_string(output.join("output-0-1/source.c")).unwrap();
    assert!(checker.evaluate(&reference, &archived).unwrap().is_zero());
}

#[test]
fn independent_shared_declarations_match_printing() {
    for row in include_str!("c/fixtures/shared-declarations.tsv").lines() {
        let c: Vec<_> = row.split('\t').collect();
        let source = c[1].replace("\\n", "\n");
        let expected = c[2].replace("\\n", "\n");
        let tree = c::parse(&source).unwrap_or_else(|e| panic!("{}: {e}", c[0]));
        assert_eq!(tree.print(), expected, "{}", c[0]);
        assert_eq!(
            c::parse(&tree.print()).unwrap(),
            tree,
            "{}: round trip",
            c[0]
        );
    }
}

#[test]
fn independent_search_scenarios_reach_targets_and_isolate_workers() {

    // Each game owns private compiler jobs and output directories. Run
    // both together instead of serializing their subprocesses.
    std::thread::scope(|scope| {
        for (game, edition) in [("tbs", "tbs-en"), ("tla", "tla-en")] {
            scope.spawn(move || check_search_scenarios(game, edition));
        }
    });
}

fn check_search_scenarios(game: &'static str, edition: &str) {
    for row in include_str!("c/fixtures/search-scenarios.tsv").lines() {
        let c: Vec<_> = row.split('\t').collect();
        let source = c[4].replace("\\n", "\n");
        let target = c[5].replace("\\n", "\n");
        let work = tempfile::tempdir().unwrap();
        let draft = work.path().join("draft/08000000.c");
        std::fs::create_dir_all(draft.parent().unwrap()).unwrap();
        std::fs::write(&draft, &source).unwrap();
        let mut problem = test_problem(&draft, &source, vec![], Some(c[1]));
        problem.target = decomp_target(Some(edition)).unwrap();
        problem.symbol = Some(c[1].into());
        let reference = Job::new(&problem, &work.path().join("reference")).unwrap();
        reference
            .compile(&target)
            .unwrap_or_else(|e| panic!("{}: reference: {e}", c[0]));
        problem.listing = crate::diff::disassemble(&reference.object, Some(c[1])).unwrap();
        problem.listing_rows = Some(score::disassemble(&reference.object, Some(c[1])).unwrap());
        let options = Options {
            game,
            target: problem.target,
            jobs: c[2].parse().unwrap(),
            function: Some(c[1].into()),
            iterations: Some(1000),
            stop_on_zero: true,
            ..test_options(&draft)
        };
        let output = work.path().join("output");
        let mut checker =
            test_problem(&options.draft, &source, problem.listing.clone(), Some(c[1]));
        checker.target = problem.target;
        checker.score_options = problem.score_options;
        checker.symbol = problem.symbol.clone();
        checker.listing_rows = problem.listing_rows.clone();
        execute(problem, &options, None, &output)
            .unwrap_or_else(|e| panic!("{}: search: {e}", c[0]));
        let best = match std::fs::read_to_string(output.join("best-1.c")) {
            Ok(best) => best,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let draft = perm::expand(&checker.nodes).unwrap();
                draft.restore(&draft.text)
            }
            Err(e) => panic!("{}: result: {e}", c[0]),
        };
        let checked = checker
            .evaluate(&reference, &best)
            .unwrap_or_else(|e| panic!("{}: saved result: {e}", c[0]));
        assert_eq!(
            checked.weighted == 0,
            c[3] == "1",
            "{}: {}\n{best}",
            c[0],
            checked.summary()
        );
        if c[3] == "1" {
            assert_eq!(
                checked.differ, 0,
                "{}: saved result must match the compiled target",
                c[0]
            );
        }
        if output.exists() {
            assert!(
                std::fs::read_dir(&output).unwrap().all(|entry| {
                    let name = entry.unwrap().file_name();
                    let name = name.to_string_lossy();
                    name.starts_with("best-") || name.starts_with("output-")
                }),
                "{}: scratch survived",
                c[0]
            );
        }
        println!("independent case {} ({edition}) passed", c[0]);
    }
}
