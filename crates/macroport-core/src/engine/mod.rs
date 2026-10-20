// SPDX-License-Identifier: Apache-2.0
//! The rule engine: it matches the rules (SPEC section 7) against a VBA
//! project and reports where they match. For now this module holds the data
//! the engine reads and the data it returns; the matchers follow.
//!
//! The engine reads a [`ProjectInput`], plain data that the parser's output is
//! converted into, and knows nothing of the lexer or of the parser. A match is
//! a [`Hit`], with the line to cite and the text to quote. Scoring the hits is
//! left to the callers: family scores, security signals and complexity traits.

mod hit;
mod input;

pub use hit::{Hit, MAX_SNIPPET_CHARS, Place};
pub use input::{
    CallSite, CodeLine, MemberUse, ModuleInput, ParserPredicates, ProcedureInput, ProjectInput,
};
