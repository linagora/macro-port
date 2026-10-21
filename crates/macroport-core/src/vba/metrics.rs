// SPDX-License-Identifier: Apache-2.0
//! The metrics of a procedure (SPEC M2 item 7): its size, its cyclomatic
//! complexity, how deep its blocks nest and its `GoTo`s, and the comparisons
//! that it makes in loops nested in loops (`nested_loop_compare`, SPEC
//! section 7), and whether its blocks match (SPEC M2 item 6).
//!
//! [`measure_procedure`] reads the logical lines of one procedure in a single
//! pass, one line at a time as the lexer asks of its callers, so it holds the
//! tokens of one line at once. Everything is counted on tokens: a keyword in a
//! string or in a comment counts for nothing. Nothing in the lines is
//! evaluated.

use std::collections::BTreeMap;

use super::lexer::TYPE_CHARACTERS;
use super::{Keyword, LogicalKind, LogicalLine, Token, TokenKind, tokenize};
use crate::report::Metrics;

/// What reading a procedure gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcedureMeasure {
    /// `loc`, `logical_loc`, `cyclomatic`, `max_nesting`, `parameters` and
    /// `goto_count`, as the report has them for a procedure. The other metrics
    /// are left out.
    pub metrics: Metrics,
    /// The `nested_loop_compare` predicate: the position, among the lines read,
    /// of each line that holds an `If` or an `ElseIf` that compares values with
    /// `=`, `<>` or `StrComp` in a loop that is itself in a loop, in order.
    pub nested_loop_compare: Vec<usize>,
    /// Whether the blocks do not match: a closer found no block of its kind or
    /// closed blocks left open inside its own, or a block is still open at the
    /// end. VBA refuses such a procedure, which is what SPEC M2 item 6 calls
    /// unbalanced blocks, and the parser (`parse_recovered`) does not see it.
    pub unbalanced_blocks: bool,
}

/// Measures a procedure.
///
/// `lines` are the logical lines of the procedure, from its header (position 0)
/// to its `End`, comments included: `source.logical[procedure.lines]` for the
/// `Procedure` that the parser gives. `start_line` and `end_line` are the
/// physical lines of the header and of the `End`, and `loc` counts the physical
/// lines from one to the other, those that are not code included. `parameters`
/// is the parser's count.
///
/// The measure is best effort and never fails: a block that is not closed, a
/// closer that closes nothing or a statement that is not VBA only give odd
/// numbers.
pub fn measure_procedure(
    start_line: u32,
    end_line: u32,
    parameters: usize,
    lines: &[LogicalLine],
) -> ProcedureMeasure {
    let mut walk = Walk::default();
    for (position, line) in lines.iter().enumerate() {
        if line.kind == LogicalKind::Code {
            let continues = lines
                .get(position.saturating_add(1))
                .is_some_and(|next| next.after_separator);
            walk.line(&Line::new(&line.text, position, continues));
        }
    }
    ProcedureMeasure {
        metrics: Metrics {
            loc: end_line.saturating_sub(start_line).saturating_add(1),
            logical_loc: logical_lines(lines),
            cyclomatic: Some(walk.decisions.saturating_add(1)),
            max_nesting: Some(walk.blocks.deepest),
            parameters: Some(u32::try_from(parameters).unwrap_or(u32::MAX)),
            goto_count: Some(walk.gotos),
            ..Metrics::default()
        },
        nested_loop_compare: walk.nested_compares,
        unbalanced_blocks: walk.blocks.is_unbalanced(),
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
    /// Where the line is among the lines of the procedure.
    position: usize,
    /// Whether the next logical line follows this one on the same chain of
    /// statements (`If a Then: b`).
    continues: bool,
}

impl<'a> Line<'a> {
    fn new(text: &'a str, position: usize, continues: bool) -> Self {
        let mut tokens = tokenize(text);
        tokens.retain(|token| token.kind != TokenKind::Comment);
        Self {
            text,
            tokens,
            position,
            continues,
        }
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

    /// Whether the token at `i` is the identifier `name` and no member of an
    /// object: a name after `.` or `!` is one (`Application.GoTo`, `r.StrComp`).
    fn is_bare_name(&self, i: usize, name: &str) -> bool {
        !matches!(
            self.kind_before(i, 1),
            Some(TokenKind::Dot | TokenKind::Bang)
        ) && self.is_name(i, name)
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

    /// How many loops the `Next` at `i` closes: one, and one more for each
    /// comma of `Next j, i`, up to the end of the statement. A `Then` and an
    /// `Else` end it too: past them, each `Next` would read the line again.
    fn next_closes(&self, i: usize) -> u32 {
        let names = self
            .tokens
            .iter()
            .skip(i.saturating_add(1))
            .take_while(|token| {
                !matches!(
                    token.kind,
                    TokenKind::Colon | TokenKind::Keyword(Keyword::Then | Keyword::Else)
                )
            });
        let commas = names.filter(|token| token.kind == TokenKind::Comma).count();
        u32::try_from(commas).unwrap_or(u32::MAX).saturating_add(1)
    }
}

/// The blocks that nest (SPEC M2 item 7, `max_nesting`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Block {
    If,
    Select,
    For,
    Do,
    While,
    With,
}

impl Block {
    const LOOPS: [Self; 3] = [Self::For, Self::Do, Self::While];
}

/// The blocks that are open at a point of the procedure.
///
/// A closer closes the innermost block of its kind and the blocks left open
/// inside it. One with no block of its kind open is ignored, which the count of
/// each kind tells at once and keeps a hostile run of closers linear. VBA
/// refuses both, and so they are noted.
#[derive(Debug, Default)]
struct Blocks {
    stack: Vec<Block>,
    /// How many blocks of each kind are open.
    open: BTreeMap<Block, u32>,
    /// The most blocks that were open at once.
    deepest: u32,
    /// Whether a closer found no block of its kind, or closed blocks left open
    /// inside its own.
    mismatched: bool,
}

impl Blocks {
    /// Opens and closes the blocks that the statement starting at
    /// `line.tokens[i]` opens and closes by its first word. The `If` is not
    /// one of them: only its `Then` tells whether it is a block.
    fn head(&mut self, line: &Line, i: usize) {
        let Some(TokenKind::Keyword(word)) = line.kind(i) else {
            return;
        };
        let next = line.kind(i.saturating_add(1));
        match word {
            Keyword::Select if next == Some(TokenKind::Keyword(Keyword::Case)) => {
                self.push(Block::Select);
            }
            Keyword::For => self.push(Block::For),
            Keyword::Do => self.push(Block::Do),
            Keyword::While => self.push(Block::While),
            Keyword::With => self.push(Block::With),
            Keyword::Loop => self.close(Block::Do),
            Keyword::Wend => self.close(Block::While),
            Keyword::Next => {
                for _ in 0..line.next_closes(i) {
                    self.close(Block::For);
                }
            }
            Keyword::End => match next {
                Some(TokenKind::Keyword(Keyword::If)) => self.close(Block::If),
                Some(TokenKind::Keyword(Keyword::Select)) => self.close(Block::Select),
                Some(TokenKind::Keyword(Keyword::With)) => self.close(Block::With),
                _ => {}
            },
            _ => {}
        }
    }

    /// How many blocks of this kind are open.
    fn count(&self, block: Block) -> u32 {
        self.open.get(&block).copied().unwrap_or_default()
    }

    /// How many loops are open.
    fn loops(&self) -> u32 {
        Block::LOOPS
            .into_iter()
            .fold(0, |loops, block| loops.saturating_add(self.count(block)))
    }

    fn push(&mut self, block: Block) {
        self.stack.push(block);
        let count = self.open.entry(block).or_default();
        *count = count.saturating_add(1);
        let depth = u32::try_from(self.stack.len()).unwrap_or(u32::MAX);
        self.deepest = self.deepest.max(depth);
    }

    fn close(&mut self, block: Block) {
        if self.count(block) == 0 {
            self.mismatched = true;
            return;
        }
        while let Some(top) = self.stack.pop() {
            if let Some(count) = self.open.get_mut(&top) {
                *count = count.saturating_sub(1);
            }
            if top == block {
                break;
            }
            self.mismatched = true;
        }
    }

    /// Whether the blocks read so far do not match: a closer did not find its
    /// block, or a block is still open.
    fn is_unbalanced(&self) -> bool {
        self.mismatched || !self.stack.is_empty()
    }
}

/// What a condition belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owner {
    If,
    ElseIf,
    /// `Do While`, `Loop Until`, `While`.
    Loop,
}

/// A condition being read: an `If` or `ElseIf` up to its `Then`, a loop test
/// up to the end of its statement. `And`, `Or` and `Xor` add decisions to it.
#[derive(Debug)]
struct Condition {
    owner: Owner,
    /// Whether it compares values with `=`, `<>` or `StrComp`.
    compares: bool,
}

impl Condition {
    /// The condition that the statement starting at `line.tokens[i]` opens, if
    /// it opens one: an `If`, an `ElseIf` or a `While` has one, and so has a
    /// `Do` or a `Loop` that has its `While` or `Until` after it.
    fn opened_by(line: &Line, i: usize) -> Option<Self> {
        let owner = match line.kind(i)? {
            TokenKind::Keyword(Keyword::If) => Owner::If,
            TokenKind::Keyword(Keyword::ElseIf) => Owner::ElseIf,
            TokenKind::Keyword(Keyword::While) => Owner::Loop,
            TokenKind::Keyword(Keyword::Do | Keyword::Loop)
                if matches!(
                    line.kind(i.saturating_add(1)),
                    Some(TokenKind::Keyword(Keyword::While | Keyword::Until))
                ) =>
            {
                Owner::Loop
            }
            _ => return None,
        };
        Some(Self {
            owner,
            compares: false,
        })
    }
}

/// Notes that the condition being read, if there is one, compares values.
fn note_comparison(condition: &mut Option<Condition>) {
    if let Some(condition) = condition {
        condition.compares = true;
    }
}

/// What the pass over a procedure has counted so far.
#[derive(Debug, Default)]
struct Walk {
    /// The decision points: cyclomatic complexity is one more.
    decisions: u32,
    /// The `GoTo` and `GoSub` statements, those of `On Error GoTo` apart.
    gotos: u32,
    blocks: Blocks,
    /// The positions of the lines that compare values in nested loops.
    nested_compares: Vec<usize>,
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
        let last = line.tokens.len().saturating_sub(1);
        let mut starts = true;
        let mut condition: Option<Condition> = None;
        for (i, token) in line.tokens.iter().enumerate() {
            if starts {
                starts = false;
                self.count_head(line, i);
                self.blocks.head(line, i);
                condition = Condition::opened_by(line, i);
            }
            match token.kind {
                // The `Else` of `Case Else` starts nothing.
                TokenKind::Keyword(Keyword::Else)
                    if line.kind_before(i, 1) == Some(TokenKind::Keyword(Keyword::Case)) => {}
                TokenKind::Keyword(Keyword::Then) => {
                    let owner = self.end_condition(&mut condition, line.position);
                    // An `If` that ends in its `Then` is a block, unless a
                    // statement of the same chain follows (`If a Then: b`).
                    if owner == Some(Owner::If) && i == last && !line.continues {
                        self.blocks.push(Block::If);
                    }
                    starts = true;
                }
                TokenKind::Keyword(Keyword::Else) | TokenKind::Colon => {
                    self.end_condition(&mut condition, line.position);
                    starts = true;
                }
                TokenKind::Keyword(Keyword::And | Keyword::Or | Keyword::Xor)
                    if condition.is_some() =>
                {
                    bump(&mut self.decisions);
                }
                TokenKind::Equals | TokenKind::NotEqual => note_comparison(&mut condition),
                TokenKind::Keyword(Keyword::GoTo) => self.go_to(line, i),
                TokenKind::Keyword(Keyword::GoSub) => bump(&mut self.gotos),
                TokenKind::Identifier if is_iif_call(line, i) => bump(&mut self.decisions),
                TokenKind::Identifier if line.is_bare_name(i, "StrComp") => {
                    note_comparison(&mut condition);
                }
                _ => {}
            }
        }
        self.end_condition(&mut condition, line.position);
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

    /// Ends the condition being read, if there is one, and tells whose it was.
    /// The `If` or `ElseIf` that compares in a loop nested in a loop is noted
    /// at the position of its line, once however many it has.
    fn end_condition(
        &mut self,
        condition: &mut Option<Condition>,
        position: usize,
    ) -> Option<Owner> {
        let ended = condition.take()?;
        if ended.compares
            && ended.owner != Owner::Loop
            && self.blocks.loops() >= 2
            && self.nested_compares.last() != Some(&position)
        {
            self.nested_compares.push(position);
        }
        Some(ended.owner)
    }
}

/// Adds one to a count, which stops at the largest value it can hold.
fn bump(count: &mut u32) {
    *count = count.saturating_add(1);
}

/// Whether the identifier at `line.tokens[i]` calls `IIf`: the bare name, with
/// its arguments in parentheses.
fn is_iif_call(line: &Line, i: usize) -> bool {
    line.is_bare_name(i, "IIf") && line.kind(i.saturating_add(1)) == Some(TokenKind::LeftParen)
}
