// SPDX-License-Identifier: Apache-2.0
//! `analyze()`, the extraction stage: what a file is, whether it can be read,
//! and the modules of its VBA project.
//!
//! Every report these tests produce is validated against
//! `schema/report.schema.json` (`support::extraction_with`).

// The helpers are test code: a panic in one is a failed test. `clippy.toml`
// only exempts the bodies of `#[test]` functions, and not their arithmetic: a
// counter in a test adds one.
#![allow(clippy::arithmetic_side_effects, clippy::expect_used)]

mod failures;
mod file_report;
mod fixtures;
mod indicators;
mod robustness;
mod support;
