// SPDX-License-Identifier: Apache-2.0
//! The reader of Compound File Binary containers: paths and malformed files.
//!
//! The containers come from three places. The `cfb` writer builds well-formed
//! ones in memory (`support::container`, the only caller of the writer, which
//! the core's `clippy.toml` bans); bytes patched by hand make the structures
//! that no writer produces; the corpus holds what Excel wrote and the files
//! that `make_malformed.py` broke.

// The helpers build and patch bytes by hand. The lints against unwrapping,
// indexing and unchecked arithmetic protect the code that reads hostile
// input, and `clippy.toml` only exempts the bodies of `#[test]` functions.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

mod fixtures;
mod malformed;
mod reading;
mod support;
