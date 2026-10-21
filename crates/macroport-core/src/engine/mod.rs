// SPDX-License-Identifier: Apache-2.0
//! The rule engine: it evaluates the matchers of the rules (SPEC section 7)
//! over a VBA project and reports where they match.
//!
//! The engine reads a [`ProjectInput`], plain data that the parser's output is
//! converted into, and knows nothing of the lexer or of the parser. An
//! [`Evaluator`] runs a matcher over a procedure, a module or the whole
//! project and returns [`Hit`]s, each with the line to cite and the text to
//! quote. Scoring the hits is left to the callers: family scores, security
//! signals and complexity traits.

mod eval;
pub mod families;
mod glob;
mod hit;
mod input;
mod matchers;
mod names;
#[cfg(test)]
pub(crate) mod testing;
mod unit;

pub use eval::Evaluator;
pub use hit::{Hit, MAX_SNIPPET_CHARS, Place};
pub use input::{
    CallSite, CodeLine, MemberUse, ModuleInput, NameSet, ParserPredicates, ProcedureInput,
    ProjectInput,
};
pub use matchers::{is_auto_exec, is_event_handler};
