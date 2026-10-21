// SPDX-License-Identifier: Apache-2.0
//! The metrics of a procedure (SPEC M2 item 7): its size, its cyclomatic
//! complexity and its `GoTo`s.
//!
//! [`measure_procedure`] reads the logical lines of one procedure in a single
//! pass, one line at a time as the lexer asks of its callers, so it holds the
//! tokens of one line at once. Everything is counted on tokens: a keyword in a
//! string or in a comment counts for nothing. Nothing in the lines is
//! evaluated.

use super::lexer::TYPE_CHARACTERS;
use super::{Keyword, LogicalKind, LogicalLine, Token, TokenKind, tokenize};
use crate::report::Metrics;

/// Measures a procedure and gives its `loc`, `logical_loc`, `cyclomatic`,
/// `parameters` and `goto_count`, as the report has them for a procedure.
///
/// `lines` are the logical lines of the procedure, from its header to its
/// `End`, comments included: `source.logical[procedure.lines]` for the
/// `Procedure` that the parser gives. `start_line` and `end_line` are the
/// physical lines of the header and of the `End`, and `loc` counts the physical
/// lines from one to the other, those that are not code included. `parameters`
/// is the parser's count.
///
/// The measure is best effort and never fails: a statement that is not VBA
/// only gives odd numbers.
pub fn measure_procedure(
    start_line: u32,
    end_line: u32,
    parameters: usize,
    lines: &[LogicalLine],
) -> Metrics {
    let mut walk = Walk::default();
    for line in lines.iter().filter(|line| line.kind == LogicalKind::Code) {
        walk.line(&Line::new(&line.text));
    }
    Metrics {
        loc: end_line.saturating_sub(start_line).saturating_add(1),
        logical_loc: logical_lines(lines),
        cyclomatic: Some(walk.decisions.saturating_add(1)),
        parameters: Some(u32::try_from(parameters).unwrap_or(u32::MAX)),
        goto_count: Some(walk.gotos),
        ..Metrics::default()
    }
}

/// The number of logical lines that are neither blank nor comment-only (SPEC
/// M2 item 7). A statement label counts as a line, unless a statement follows
/// it on its own line (`Done: x = 1`, `10 x = 1`): the label and its statement
/// are one line of the file.
fn logical_lines(lines: &[LogicalLine]) -> u32 {
    let mut count = 0_u32;
    for (position, line) in lines.iter().enumerate() {
        let counts = match line.kind {
            LogicalKind::Code => true,
            LogicalKind::CommentOnly => false,
            LogicalKind::Label => !lines
                .get(position.saturating_add(1))
                .is_some_and(|next| next.after_separator && next.kind == LogicalKind::Code),
        };
        if counts {
            count = count.saturating_add(1);
        }
    }
    count
}

/// A logical line cut into tokens, with the text they point into. The comments
/// are left out: nothing in them is code. Cutting a line gives at most
/// `MAX_TOKENS` tokens.
struct Line<'a> {
    text: &'a str,
    tokens: Vec<Token>,
}

impl<'a> Line<'a> {
    fn new(text: &'a str) -> Self {
        let mut tokens = tokenize(text);
        tokens.retain(|token| token.kind != TokenKind::Comment);
        Self { text, tokens }
    }

    fn token(&self, i: usize) -> Option<&Token> {
        self.tokens.get(i)
    }

    /// The kind of the token at `i`, if the line has one.
    fn kind(&self, i: usize) -> Option<TokenKind> {
        self.token(i).map(|token| token.kind)
    }

    /// The kind of the token `back` places before the one at `i`.
    fn kind_before(&self, i: usize, back: usize) -> Option<TokenKind> {
        self.kind(i.checked_sub(back)?)
    }

    /// Whether the token at `i` is an identifier called `name`, in any letter
    /// case: the way to read the words that are not keywords, such as the
    /// `Error` of `On Error`.
    fn is_name(&self, i: usize, name: &str) -> bool {
        self.token(i)
            .is_some_and(|token| token.is_name(self.text, name))
    }

    /// Whether the `GoTo` at `i` is the one of an `On Error GoTo`.
    fn follows_on_error(&self, i: usize) -> bool {
        i.checked_sub(1).is_some_and(|at| self.is_name(at, "Error"))
            && self.kind_before(i, 2) == Some(TokenKind::Keyword(Keyword::On))
    }

    /// Whether the token at `i` is a label that an `On Error GoTo` can jump to:
    /// a name or a line number, but not `0`, which switches the handler off.
    fn is_label(&self, i: usize) -> bool {
        let Some(token) = self.token(i) else {
            return false;
        };
        match token.kind {
            TokenKind::Identifier => true,
            TokenKind::Number => {
                let digits = token.text(self.text).trim_end_matches(TYPE_CHARACTERS);
                !digits.bytes().all(|byte| byte == b'0')
            }
            _ => false,
        }
    }
}

/// What the pass over a procedure has counted so far.
#[derive(Debug, Default)]
struct Walk {
    /// The decision points: cyclomatic complexity is one more.
    decisions: u32,
    /// The `GoTo` and `GoSub` statements, those of `On Error GoTo` apart.
    gotos: u32,
}

impl Walk {
    /// Reads one logical line, which is a statement, or the start of one. The
    /// statements that a single-line `If` holds start after its `Then` and its
    /// `Else`, and a `:` that the splitter left starts one as well.
    fn line(&mut self, line: &Line) {
        // A conditional-compilation directive is no statement of the
        // procedure: the code on each side of it is read as written.
        if line.kind(0) == Some(TokenKind::Hash) {
            return;
        }
        let mut starts = true;
        // Whether the tokens being read are a condition, in which `And`, `Or`
        // and `Xor` are decision points: an `If` or `ElseIf` up to its `Then`,
        // a loop test up to the end of its statement.
        let mut in_condition = false;
        for (i, token) in line.tokens.iter().enumerate() {
            if starts {
                starts = false;
                self.count_head(line, i);
                in_condition = opens_condition(line, i);
            }
            match token.kind {
                // The `Else` of `Case Else` starts nothing.
                TokenKind::Keyword(Keyword::Else)
                    if line.kind_before(i, 1) == Some(TokenKind::Keyword(Keyword::Case)) => {}
                TokenKind::Keyword(Keyword::Then | Keyword::Else) | TokenKind::Colon => {
                    in_condition = false;
                    starts = true;
                }
                TokenKind::Keyword(Keyword::And | Keyword::Or | Keyword::Xor) if in_condition => {
                    bump(&mut self.decisions);
                }
                TokenKind::Keyword(Keyword::GoTo) => self.go_to(line, i),
                TokenKind::Keyword(Keyword::GoSub) => bump(&mut self.gotos),
                TokenKind::Identifier if is_iif_call(line, i) => bump(&mut self.decisions),
                _ => {}
            }
        }
    }

    /// Counts the decision point that the statement starting at `line.tokens[i]`
    /// makes by its first word, if it makes one.
    fn count_head(&mut self, line: &Line, i: usize) {
        let Some(TokenKind::Keyword(word)) = line.kind(i) else {
            return;
        };
        match word {
            // The `Do` counts the loop, whether the `Do` or the `Loop` holds
            // its test.
            Keyword::If | Keyword::ElseIf | Keyword::While | Keyword::For | Keyword::Do => {
                bump(&mut self.decisions);
            }
            // `Select Case` has its `Case` second; `Case Else` is no test.
            Keyword::Case
                if line.kind(i.saturating_add(1)) != Some(TokenKind::Keyword(Keyword::Else)) =>
            {
                bump(&mut self.decisions);
            }
            _ => {}
        }
    }

    /// Reads the `GoTo` at `line.tokens[i]`. The one of an `On Error GoTo` is a
    /// decision point, unless it switches the handler off, and no jump of the
    /// procedure. Any other is a jump.
    fn go_to(&mut self, line: &Line, i: usize) {
        if line.follows_on_error(i) {
            if line.is_label(i.saturating_add(1)) {
                bump(&mut self.decisions);
            }
        } else {
            bump(&mut self.gotos);
        }
    }
}

/// Adds one to a count, which stops at the largest value it can hold.
fn bump(count: &mut u32) {
    *count = count.saturating_add(1);
}

/// Whether the statement starting at `line.tokens[i]` has a condition, in which
/// `And`, `Or` and `Xor` are decision points: an `If`, an `ElseIf` or a
/// `While`, and a `Do` or a `Loop` that has its `While` or `Until` after it.
fn opens_condition(line: &Line, i: usize) -> bool {
    match line.kind(i) {
        Some(TokenKind::Keyword(Keyword::If | Keyword::ElseIf | Keyword::While)) => true,
        Some(TokenKind::Keyword(Keyword::Do | Keyword::Loop)) => matches!(
            line.kind(i.saturating_add(1)),
            Some(TokenKind::Keyword(Keyword::While | Keyword::Until))
        ),
        _ => false,
    }
}

/// Whether the identifier at `line.tokens[i]` calls `IIf`: the bare name, with
/// its arguments in parentheses. A name after `.` or `!` is a member of some
/// object, not the function.
fn is_iif_call(line: &Line, i: usize) -> bool {
    !matches!(
        line.kind_before(i, 1),
        Some(TokenKind::Dot | TokenKind::Bang)
    ) && line.is_name(i, "IIf")
        && line.kind(i.saturating_add(1)) == Some(TokenKind::LeftParen)
}
