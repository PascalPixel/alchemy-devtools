# Permuter fixtures

These fixtures use independently authored synthetic inputs. No game source, ROM or imported test corpus is included.

- parser-cases.tsv: 47 translation units combining types, declarators, initializers, operators, statements, extensions and typedef shadowing. Columns: case name, C, formatted C, preorder node kinds with constant types.
- parser-invalid.tsv: 12 syntax and namespace failures.
- parser-unsupported.tsv: unnamed scalar members and pragma expressions. Rejection tables store case name, input C and a short description.
- shared-declarations.tsv: 12 cases covering shared structures, unions, enums, pointers, arrays, callbacks and nested declarations.
- search-scenarios.tsv: 18 original draft/target pairs, including an unreachable target, finite choices, ranges, shared fragments, once-only writes, line swaps, ignored regions and randomized call ordering. The harness uses both games, both score algorithms and multiple workers. Saved results are recompiled and checked for instruction equality.
- .c, .expected and .nodes: complete translation-unit formatting and traversal checks. Trailing spaces use `\x20`; the final blank line uses a `\x0a` marker, decoded before comparison.
- mutations.tsv and mutations-passes.tsv: exact results of single passes, each pass once on its shortest result and each mutations-*.c program at least once (columns: program, pass, seed, changed, next random number, printed C). mutations-integer-seeds.tsv: successive walks from signed and large seeds, with retry counts. mutations-effects.tsv and mutations-access.tsv: side effects and variable-access analysis. The tests also check, without recorded results, that every pass on every program is whole and decided by its seed, and that walks agree with the mutation driver and replay.
- rng*.tsv: signed and large integer seeds, sampling and independent control/mutation streams.
- output-*.tsv: filtering, deduplication, source diffs and assembly normalization, a few cases each.
- levenshtein-alignments.tsv: 264 pairs of short strings, five letters in all at most, which settle how ties are broken; longer random pairs are checked to be optimal alignments against the edit distance. scoring-algorithms.tsv: four listings scored by both algorithms with each normalization option.

TSV cells escape backslashes, tabs, newlines and carriage returns. Expected results check the recorded scenarios; they do not establish exhaustive grammar support, complete command-line compatibility or a measured improvement in matching productivity.
