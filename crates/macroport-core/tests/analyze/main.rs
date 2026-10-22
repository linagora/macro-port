// SPDX-License-Identifier: Apache-2.0
//! `analyze()`: what a file is, whether it can be read, the modules of its VBA
//! project (the extraction stage) and what the front end makes of them.
//!
//! Every report these tests produce is validated against
//! `schema/report.schema.json` (`support::extraction_with`,
//! `support::report_problems_of`).

// The helpers are test code: a panic in one is a failed test. `clippy.toml`
// only exempts the bodies of `#[test]` functions, and not their arithmetic: a
// counter in a test adds one.
#![allow(clippy::arithmetic_side_effects, clippy::expect_used)]

mod apis;
mod calls;
mod corpus;
mod failures;
mod file_report;
mod fixtures;
mod indicators;
mod judgments;
mod modules;
mod robustness;
mod sample_snapshots;
mod statements;
mod support;
mod text_files;
