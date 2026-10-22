// SPDX-License-Identifier: Apache-2.0
//! Hostile functions for the predicate `udf_candidate`. Reading one in time
//! linear in its size shows on the clock only, not in the result: the limit is
//! set far above what a linear reading takes and far below what a quadratic one
//! would take.

// clippy.toml keeps the clock out of the core, not out of its tests, and
// excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::expect_used
)]

use std::time::{Duration, Instant};

use macroport_core::rules::ModuleType;
use macroport_core::vba::{MAX_TOKENS, is_udf_candidate, parse, split_lines};

/// Whether the public function of a standard module that holds `body` is a UDF
/// candidate, and fails if that takes longer than ten seconds, about ten times
/// what a debug build takes for a million tokens. Splitting the lines and
/// parsing are not part of it.
fn is_candidate(body: &str) -> bool {
    let text = format!("Public Function F(r As Range)\n{body}End Function\n");
    let lines = split_lines(&text).expect("a text within the limits");
    let parsed = parse(&lines);
    let procedure = parsed.procedures.first().expect("a procedure");
    let started = Instant::now();
    let candidate = is_udf_candidate(ModuleType::Standard, procedure, &lines);
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(10), "took {elapsed:?}");
    candidate
}

#[test]
fn a_function_of_many_lines_is_read_in_linear_time() {
    assert!(is_candidate(&"x = r.Value\n".repeat(100_000)));
    assert!(is_candidate(&"If a Then x = 1 Else y = 2\n".repeat(50_000)));
    // The side effect on the last line is found after all the others.
    let body = format!("{}r.Value = 1\n", "x = r.Value\n".repeat(100_000));
    assert!(!is_candidate(&body));
    // Lists that are never closed stop at the end of their line.
    assert!(is_candidate(&"a(\n".repeat(100_000)));
}

#[test]
fn many_with_blocks_are_read_in_linear_time() {
    let opened = "With r\n".repeat(100_000);
    let closed = "End With\n".repeat(100_000);
    assert!(is_candidate(&format!("{opened}x = .Name\n{closed}")));
    // Blocks that are never closed, and closers that find no block.
    let fonts = "With r.Font\n".repeat(100_000);
    assert!(is_candidate(&format!("{fonts}x = .Bold\n")));
    assert!(!is_candidate(&format!("{fonts}.Bold = True\n")));
    assert!(is_candidate(&"End With\n".repeat(100_000)));
}

#[test]
fn a_line_of_many_members_is_read_in_linear_time() {
    let members = ".a".repeat(400_000);
    assert!(is_candidate(&format!("x = r{members}\n")));
    assert!(is_candidate(&format!("r{members} = 1\n")));
    assert!(!is_candidate(&format!("r{members}.Value = 1\n")));
}

#[test]
fn a_line_of_many_dots_after_spaces_or_continuations_is_read_in_linear_time() {
    // No dot goes on with the target, and the reading stops at the first.
    let spaced = " .a".repeat(300_000);
    assert!(is_candidate(&format!("r{spaced}.Value = 1\n")));
    // Every dot opens a physical line of the same logical line, and goes on.
    let continued = " _\n.a".repeat(100_000);
    assert!(!is_candidate(&format!("r{continued} _\n.Value = 1\n")));
}

#[test]
fn a_line_of_many_arguments_is_read_in_linear_time() {
    // Lists that are never closed, and that are nested without end.
    let opened = "(".repeat(500_000);
    assert!(is_candidate(&format!("x{opened} = 1\n")));
    assert!(is_candidate(&format!("r{opened}.Value = 1\n")));
    // Lists that are closed one after the other.
    let lists = "(1)".repeat(100_000);
    assert!(is_candidate(&format!("a{lists} = 1\n")));
    assert!(!is_candidate(&format!("a{lists}.Value = 1\n")));
}

#[test]
fn a_line_of_many_statements_is_read_in_linear_time() {
    assert!(is_candidate(&format!(
        "{}x = 1\n",
        "If a Then ".repeat(100_000)
    )));
    assert!(is_candidate(&format!(
        "If a Then{}\n",
        " Else x = r.Value".repeat(100_000)
    )));
}

#[test]
fn a_line_of_more_tokens_than_the_limit_is_read_up_to_the_limit() {
    // The lexer cuts the line at its limit, and what lies beyond it is one
    // unknown token: the `MsgBox` after the limit is not seen, and the one
    // before it is.
    let filler = " + b".repeat(MAX_TOKENS);
    assert!(is_candidate(&format!(
        "If a Then x = a{filler} Else MsgBox x\n"
    )));
    assert!(!is_candidate(&format!(
        "If a Then MsgBox x Else x = a{filler}\n"
    )));
}
