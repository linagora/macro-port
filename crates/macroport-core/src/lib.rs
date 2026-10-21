// SPDX-License-Identifier: Apache-2.0
//! Macro-Port analysis core: file bytes in, report out.
//!
//! The core is pure. It touches no file, network or console, reads no
//! environment, clock or randomness, and starts no thread or process;
//! `clippy.toml` bans the standard library APIs that would. Everything impure
//! lives in the CLI.

#![forbid(unsafe_code)]

pub mod analyze;
pub mod container;
pub mod ovba;
pub mod redact;
pub mod report;
pub mod rules;

// The entry point of the core, as the SPEC names it: `macroport_core::analyze`
// is the function, and the module of that name holds its options and results.
pub use analyze::analyze;
