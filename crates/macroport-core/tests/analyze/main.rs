// SPDX-License-Identifier: Apache-2.0
//! `analyze()`, the extraction stage: what a file is, whether it can be read,
//! and the modules of its VBA project.
//!
//! Every report these tests produce is validated against
//! `schema/report.schema.json` (`support::extraction_with`).

// The helpers are test code: a panic in one is a failed test. `clippy.toml`
// only exempts the bodies of `#[test]` functions.
#![allow(clippy::expect_used)]

mod file_report;
mod support;
