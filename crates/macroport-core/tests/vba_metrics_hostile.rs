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

use macroport_core::report::Metrics;
use macroport_core::vba::{MAX_TOKENS, measure_procedure, split_lines};

/// Measures the procedure that `text` is and fails if that takes longer than
/// ten seconds, about ten times what a debug build takes for a million tokens.
/// Splitting the lines is not part of it.
fn measure_within_limit(text: &str) -> Metrics {
    let lines = split_lines(text).expect("a text within the limits");
    let started = Instant::now();
    let metrics = measure_procedure(1, lines.physical_lines, 0, &lines.logical);
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(10), "took {elapsed:?}");
    metrics
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
    assert_eq!(decisions.cyclomatic, Some(150_001));
    assert_eq!(decisions.logical_loc, 50_002);
    let jumps = measure_within_limit(&procedure(&"GoTo Done\nGoSub Work\n".repeat(25_000)));
    assert_eq!(jumps.goto_count, Some(50_000));
    let handlers = measure_within_limit(&procedure(&"On Error GoTo Handler\n".repeat(50_000)));
    assert_eq!(handlers.cyclomatic, Some(50_001));
}

#[test]
fn a_line_of_more_tokens_than_the_limit_is_measured_in_linear_time() {
    // The lexer cuts the line at its limit, and what lies beyond it is one
    // unknown token that counts for nothing.
    let line = format!("If a{} Then x = 1\n", " And b".repeat(MAX_TOKENS));
    let metrics = measure_within_limit(&procedure(&line));
    let cyclomatic = metrics.cyclomatic.expect("cyclomatic");
    assert!(
        (2..=u32::try_from(MAX_TOKENS).expect("a small limit")).contains(&cyclomatic),
        "{cyclomatic}"
    );
    let operands = format!("x = a{}\n", " And b".repeat(MAX_TOKENS));
    let metrics = measure_within_limit(&procedure(&operands));
    assert_eq!(metrics.cyclomatic, Some(1));
}
