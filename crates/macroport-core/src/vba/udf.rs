// SPDX-License-Identifier: Apache-2.0
//! The structural predicate `udf_candidate` (SPEC section 7): a function of a
//! standard module that a worksheet formula could call, because it does none
//! of the things that the SPEC lists.
//!
//! The list is closed, and the predicate looks for those things and for nothing
//! else. It reads tokens, a logical line at a time, so a name in a string or in
//! a comment counts for nothing. The function must not:
//!
//! - assign to a `.Value`, `.Formula`, `.Interior` or `.Font` member. The target
//!   of an assignment is the reference before its `=`, and a member of it that
//!   is not in call arguments, such as `Font` in `Cells(1, 1).Font.Bold = True`,
//!   has one of the four names. In a `With` block whose target reaches one of
//!   the four, as `With r.Font` does, an assignment to a member of the block
//!   (`.Bold = True`) is one too;
//! - call `Select`, `Activate`, `Copy`, `Delete`, `Insert` or `ClearContents` on
//!   an object: the name follows a `.`;
//! - call `MsgBox`, wherever the name stands.
//!
//! What the list leaves out is not looked for. A function that writes `Value2`
//! or `FormulaR1C1`, assigns through the default property of a range
//! (`Cells(1, 1) = 1`), calls a procedure that does any of this, or keeps a
//! `Font` in a variable (`Set f = r.Font`, then `f.Bold = True`) is still a
//! candidate. A name after a `!` is the item of a collection, such as a field
//! of a recordset or a key of a dictionary, and no member.

use super::lexer::code_tokens;
use super::{Keyword, LogicalKind, LogicalLine, Procedure, SourceLines, Token, TokenKind};
use crate::report::ProcedureKind;
use crate::rules::ModuleType;

/// The members that an assignment must not reach (SPEC section 7).
const ASSIGNED_MEMBERS: [&str; 4] = ["Value", "Formula", "Interior", "Font"];

/// The methods that a function must not call on an object (SPEC section 7).
const METHODS: [&str; 6] = [
    "Select",
    "Activate",
    "Copy",
    "Delete",
    "Insert",
    "ClearContents",
];

/// The function that a function must not call (SPEC section 7).
const MESSAGE_BOX: &str = "MsgBox";

/// The structural predicate `udf_candidate` for `procedure`, a procedure of the
/// module of type `module_type` that `source` holds: a `Public` or
/// default-visibility `Function` of a standard module, whose body does none of
/// the things that the SPEC lists (see the module documentation).
pub fn is_udf_candidate(
    module_type: ModuleType,
    procedure: &Procedure,
    source: &SourceLines,
) -> bool {
    module_type == ModuleType::Standard
        && procedure.kind == ProcedureKind::Function
        && procedure.is_public()
        && (source.logical.get(procedure.lines.clone())).is_some_and(|lines| !has_effect(lines))
}

/// Whether one of the logical lines of a function does one of the things that
/// the SPEC lists: an effect.
fn has_effect(lines: &[LogicalLine]) -> bool {
    let mut walk = Walk::default();
    (lines.iter())
        .filter(|line| line.kind == LogicalKind::Code)
        .any(|line| walk.line_has_effect(line))
}

/// A run of tokens of a logical line, with the line that they were cut from:
/// the text that names them and the physical lines that they stand on.
#[derive(Debug, Clone, Copy)]
struct Run<'a> {
    line: &'a LogicalLine,
    tokens: &'a [Token],
}

impl<'a> Run<'a> {
    /// The kind of the token at `at`, if the run has one.
    fn kind(self, at: usize) -> Option<TokenKind> {
        self.tokens.get(at).map(|token| token.kind)
    }

    /// Whether the token at `at` is an identifier called `name`, in any letter
    /// case.
    fn is_name(self, at: usize, name: &str) -> bool {
        (self.tokens.get(at)).is_some_and(|token| token.is_name(&self.line.text, name))
    }

    /// The run without its first `count` tokens.
    fn skip(self, count: usize) -> Self {
        Self {
            tokens: self.tokens.get(count..).unwrap_or_default(),
            ..self
        }
    }

    /// The statements of the run, cut at the tokens that end one and start the
    /// next.
    fn statements(self) -> impl Iterator<Item = Run<'a>> {
        (self.tokens.split(starts_a_statement)).map(move |tokens| Run { tokens, ..self })
    }

    /// Whether the `.` or `!` at `at` goes on with the reference before it, the
    /// way the chains of the facts read it: it touches the token before it, or
    /// it opens a physical line of the logical line. After a space it starts
    /// another reference, as the `.Name` of `list.Add .Name` does.
    fn goes_on_before(self, at: usize) -> bool {
        let Some(token) = self.tokens.get(at) else {
            return false;
        };
        let Some(before) = at.checked_sub(1).and_then(|before| self.tokens.get(before)) else {
            return true;
        };
        before.end == token.start
            || self.line.line_at(before.start) != self.line.line_at(token.start)
    }
}

/// Whether the token ends a statement and starts the next: the `Then` and the
/// `Else` of a single-line `If` and the `:` that the line splitter left.
fn starts_a_statement(token: &Token) -> bool {
    matches!(
        token.kind,
        TokenKind::Colon | TokenKind::Keyword(Keyword::Then | Keyword::Else)
    )
}

/// What the lines read so far say about the code that follows.
#[derive(Debug, Default)]
struct Walk {
    /// The `With` blocks that are open, the innermost last: whether the target of
    /// each reaches one of the assigned members.
    withs: Vec<bool>,
}

impl Walk {
    /// Whether the logical line, which is a statement or the start of one, does
    /// one of the things that the SPEC lists. Reading it opens and closes the
    /// `With` blocks that it holds, for the lines that follow.
    fn line_has_effect(&mut self, line: &LogicalLine) -> bool {
        let tokens = code_tokens(&line.text);
        let whole = Run {
            line,
            tokens: &tokens,
        };
        calls_an_effect(whole)
            || whole
                .statements()
                .any(|statement| self.statement_has_effect(statement))
    }

    /// Reads a statement: a `With` opens a block, its `End With` closes one, and
    /// anything else may assign.
    fn statement_has_effect(&mut self, statement: Run) -> bool {
        match (statement.kind(0), statement.kind(1)) {
            (Some(TokenKind::Keyword(Keyword::With)), _) => {
                let reaches = self.reaches_assigned(&Target::read(statement.skip(1)));
                self.withs.push(reaches);
                false
            }
            (Some(TokenKind::Keyword(Keyword::End)), Some(TokenKind::Keyword(Keyword::With))) => {
                self.withs.pop();
                false
            }
            _ => self.assigns(statement),
        }
    }

    /// Whether the statement assigns to one of the assigned members.
    fn assigns(&self, statement: Run) -> bool {
        let prefixed = matches!(
            statement.kind(0),
            Some(TokenKind::Keyword(Keyword::Let | Keyword::Set))
        );
        let statement = statement.skip(usize::from(prefixed));
        let target = Target::read(statement);
        statement.kind(target.len) == Some(TokenKind::Equals) && self.reaches_assigned(&target)
    }

    /// Whether the target reaches one of the assigned members, itself or
    /// through the `With` block that it is relative to.
    fn reaches_assigned(&self, target: &Target) -> bool {
        target.reaches_assigned
            || (target.relative && self.withs.last().copied().unwrap_or_default())
    }
}

/// The target of an assignment or of a `With`: a name, or a leading `.` that is
/// relative to a `With` block, then members and call arguments in any order, as
/// in `Worksheets(1).Range("A1").Font`.
#[derive(Debug, Default)]
struct Target {
    /// How many tokens it covers.
    len: usize,
    /// It starts with a `.`.
    relative: bool,
    /// A member of it that follows a `.` and is not in call arguments is one of
    /// the assigned members.
    reaches_assigned: bool,
}

impl Target {
    /// The target that the run starts with: nothing if it starts with anything
    /// but a name, a `.` or a `!`. It stops at the first token that does not
    /// go on with it, and before a call argument list that is not closed.
    fn read(run: Run) -> Self {
        let mut target = Self::default();
        let mut at = match run.kind(0) {
            Some(TokenKind::Identifier) => 1,
            Some(TokenKind::Dot) => {
                target.relative = true;
                0
            }
            Some(TokenKind::Bang) => 0,
            _ => return target,
        };
        while let Some(token) = run.tokens.get(at) {
            match token.kind {
                TokenKind::Dot | TokenKind::Bang if run.goes_on_before(at) => {
                    let member = at.saturating_add(1);
                    if run.kind(member) != Some(TokenKind::Identifier) {
                        break;
                    }
                    let assigned = (ASSIGNED_MEMBERS.iter()).any(|name| run.is_name(member, name));
                    target.reaches_assigned |= assigned && token.kind == TokenKind::Dot;
                    at = member.saturating_add(1);
                }
                TokenKind::LeftParen => match after_arguments(run.tokens, at) {
                    Some(next) => at = next,
                    None => break,
                },
                _ => break,
            }
        }
        target.len = at;
        target
    }
}

/// The position after the `)` that closes the `(` at `open`, if there is one.
fn after_arguments(tokens: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (at, token) in tokens.iter().enumerate().skip(open) {
        match token.kind {
            TokenKind::LeftParen => depth = depth.saturating_add(1),
            TokenKind::RightParen => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(at.saturating_add(1));
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether the run calls one of the methods, on an object (the name follows a
/// `.`), or `MsgBox`, wherever the name stands. A name followed by `:=` is the
/// name of an argument, not a call.
fn calls_an_effect(run: Run) -> bool {
    (0..run.tokens.len()).any(|at| {
        let after_dot =
            at.checked_sub(1).and_then(|before| run.kind(before)) == Some(TokenKind::Dot);
        let argument = run.kind(at.saturating_add(1)) == Some(TokenKind::ColonEquals);
        (after_dot && METHODS.iter().any(|method| run.is_name(at, method)))
            || (!argument && run.is_name(at, MESSAGE_BOX))
    })
}
