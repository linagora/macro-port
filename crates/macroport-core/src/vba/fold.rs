// SPDX-License-Identifier: Apache-2.0
//! Constant folding (SPEC M2 item 4), the only evaluation the core makes.
//!
//! The set is closed. An expression has a known value when it is a string
//! literal, or known values joined with `&` or `+`. Everything else is
//! unknown, and nothing is guessed about it: arithmetic, `Mid`, `Replace`,
//! `vbCrLf`, a variable, a number joined to a string. Folding never runs
//! anything and reads no state but the text of the statement.
//!
//! The expression reader ([`super::expr`]) walks the expressions; this module
//! holds the values it folds.

/// The value an expression is known to have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Value {
    /// Anything the closed set does not cover.
    Unknown,
    /// A string.
    Text(String),
}
