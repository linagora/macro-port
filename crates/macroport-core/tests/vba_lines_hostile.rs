// SPDX-License-Identifier: Apache-2.0
//! Hostile texts for the line splitter. Reading one in time linear in its size
//! shows on the clock only, not in the result: the limit is set far above what
//! a linear reading takes and far below what a quadratic one would take.

// clippy.toml keeps the clock out of the core, not out of its tests.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::time::{Duration, Instant};

use macroport_core::vba::split_lines;

/// Splits `text` and fails if that takes longer than `limit`.
fn splits_within(text: &str, limit: Duration) {
    let started = Instant::now();
    let lines = split_lines(text);
    let elapsed = started.elapsed();
    assert!(lines.is_ok(), "{:?}", lines.err());
    assert!(elapsed < limit, "took {elapsed:?}");
}

/// Each `[` looks for a `]` in the rest of its line: without the rule that a
/// search that fails is not made again, 2 MB of them take about a minute.
#[test]
fn a_line_of_opening_brackets_is_read_in_linear_time() {
    splits_within(&"[".repeat(2 << 20), Duration::from_secs(5));
}
