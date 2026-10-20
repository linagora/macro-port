// SPDX-License-Identifier: Apache-2.0
//! Macro-Port analysis core: file bytes in, report out.
//!
//! The core is pure. It touches no file, network or console, reads no
//! environment, clock or randomness, and starts no thread or process;
//! `clippy.toml` bans the standard library APIs that would. Everything impure
//! lives in the CLI.

#![forbid(unsafe_code)]

pub mod container;
pub mod engine;
pub mod ovba;
pub mod report;
pub mod rules;
