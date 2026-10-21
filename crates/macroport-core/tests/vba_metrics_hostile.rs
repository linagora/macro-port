// SPDX-License-Identifier: Apache-2.0
//! Hostile procedures for the measure. Reading one in time linear in its size
//! shows on the clock only, not in the result: the limit is set far above what
//! a linear reading takes and far below what a quadratic one would take.

// clippy.toml keeps the clock out of the core, not out of its tests, and
// excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::expect_used
)]

use std::time::{Duration, Instant};

use macroport_core::vba::{MAX_TOKENS, ProcedureMeasure, measure_procedure, split_lines};

/// Measures the procedure that `text` is and fails if that takes longer than
/// ten seconds, about ten times what a debug build takes for a million tokens.
/// Splitting the lines is not part of it.
fn measure_within_limit(text: &str) -> ProcedureMeasure {
    let lines = split_lines(text).expect("a text within the limits");
    let started = Instant::now();
    let measure = measure_procedure(1, lines.physical_lines, 0, &lines.logical);
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(10), "took {elapsed:?}");
    measure
}

/// The text of a `Sub` that holds `body`.
fn procedure(body: &str) -> String {
    format!("Sub Test()\n{body}End Sub\n")
}

#[test]
fn a_procedure_of_many_lines_is_measured_in_linear_time() {
    // One decision point for the If and one for each of the two operators.
    let decisions =
        measure_within_limit(&procedure(&"If a And b Or c Then x = 1\n".repeat(50_000)));
    assert_eq!(decisions.metrics.cyclomatic, Some(150_001));
    assert_eq!(decisions.metrics.logical_loc, 50_002);
    let jumps = measure_within_limit(&procedure(&"GoTo Done\nGoSub Work\n".repeat(25_000)));
    assert_eq!(jumps.metrics.goto_count, Some(50_000));
    let handlers = measure_within_limit(&procedure(&"On Error GoTo Handler\n".repeat(50_000)));
    assert_eq!(handlers.metrics.cyclomatic, Some(50_001));
}

#[test]
fn deep_and_unbalanced_blocks_are_measured_in_linear_time() {
    let deep = measure_within_limit(&procedure(&"If a Then\n".repeat(50_000)));
    assert_eq!(deep.metrics.max_nesting, Some(50_000));
    assert!(deep.unbalanced_blocks);
    let closed = format!(
        "{}{}",
        "If a Then\n".repeat(50_000),
        "End If\n".repeat(50_000)
    );
    let closed = measure_within_limit(&procedure(&closed));
    assert_eq!(closed.metrics.max_nesting, Some(50_000));
    assert!(!closed.unbalanced_blocks);
    // Closers that find no block of their kind do not look for one.
    let strays = "End If\nNext\nLoop\nWend\nEnd With\n".repeat(40_000);
    let stray = measure_within_limit(&procedure(&strays));
    assert_eq!(stray.metrics.max_nesting, Some(0));
    assert!(stray.unbalanced_blocks);
    let withs = format!("{}{}", "With r\n".repeat(50_000), "End If\n".repeat(50_000));
    assert_eq!(
        measure_within_limit(&procedure(&withs)).metrics.max_nesting,
        Some(50_000)
    );
    // A `Next` with 100 000 commas tries to close as many loops, none is open.
    let commas = format!("Next {}\n", ",".repeat(100_000)).repeat(10);
    let measure = measure_within_limit(&procedure(&commas));
    assert_eq!(measure.metrics.max_nesting, Some(0));
    // The names of a `Next` end at a `Then`, which starts a statement.
    let thens = format!("{}\n", "Next Then ".repeat(100_000));
    let measure = measure_within_limit(&procedure(&thens));
    assert_eq!(measure.metrics.max_nesting, Some(0));
}

#[test]
fn many_directives_and_stray_branches_are_measured_in_linear_time() {
    // The first side leaves 100 000 blocks open, and the `#Else` drops them at once.
    let dropped = format!(
        "#If A Then\n{}{}",
        "With r\n".repeat(100_000),
        "#Else\n".repeat(100_000)
    );
    let dropped = measure_within_limit(&procedure(&dropped));
    assert_eq!(dropped.metrics.max_nesting, Some(100_000));
    assert!(!dropped.unbalanced_blocks);
    // Each side opens a block that the next drops, and the last one stays open.
    let sides = "#If A Then\nIf a Then\n#Else\nIf b Then\n#End If\n".repeat(30_000);
    let sides = measure_within_limit(&procedure(&sides));
    assert_eq!(sides.metrics.max_nesting, Some(30_000));
    assert!(sides.unbalanced_blocks);
    // Switching sides with 50 000 blocks open before the `#If` drops none.
    let switched = format!(
        "{}#If A Then\n{}",
        "With r\n".repeat(50_000),
        "#Else\n".repeat(50_000)
    );
    let switched = measure_within_limit(&procedure(&switched));
    assert_eq!(switched.metrics.max_nesting, Some(50_000));
    assert!(switched.unbalanced_blocks);
    // Directives with nothing to end or to switch drop nothing.
    let strays = "#End If\n#Else\n#ElseIf A Then\n".repeat(60_000);
    let stray = measure_within_limit(&procedure(&strays));
    assert_eq!(stray.metrics.max_nesting, Some(0));
    assert!(!stray.unbalanced_blocks);
    // Branches with no block to belong to do not look for one.
    let branches = "Else\nElseIf a Then\nCase 1\nCase Else\n".repeat(50_000);
    let branch = measure_within_limit(&procedure(&branches));
    assert!(branch.unbalanced_blocks);
}

#[test]
fn many_comparisons_in_nested_loops_are_measured_in_linear_time() {
    let ifs = "If a = b Then x = 1\n".repeat(50_000);
    let body = format!("For i = 1 To n\nFor j = 1 To n\n{ifs}Next j\nNext i\n");
    let measure = measure_within_limit(&procedure(&body));
    assert_eq!(measure.nested_loop_compare.len(), 50_000);
    assert_eq!(measure.nested_loop_compare.first(), Some(&3));
}

#[test]
fn a_line_of_more_tokens_than_the_limit_is_measured_in_linear_time() {
    // The lexer cuts the line at its limit, and what lies beyond it is one
    // unknown token that counts for nothing.
    let line = format!("If a{} Then x = 1\n", " And b".repeat(MAX_TOKENS));
    let measure = measure_within_limit(&procedure(&line));
    let cyclomatic = measure.metrics.cyclomatic.expect("cyclomatic");
    assert!(
        (2..=u32::try_from(MAX_TOKENS).expect("a small limit")).contains(&cyclomatic),
        "{cyclomatic}"
    );
    let operands = format!("x = a{}\n", " And b".repeat(MAX_TOKENS));
    let measure = measure_within_limit(&procedure(&operands));
    assert_eq!(measure.metrics.cyclomatic, Some(1));
}
