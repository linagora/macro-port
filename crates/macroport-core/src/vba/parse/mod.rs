// SPDX-License-Identifier: Apache-2.0
//! The structural parser (SPEC M2 items 3 and 6): the logical lines of a
//! module become procedures with their spans, and the places where the code
//! does not parse are recorded. It reads one logical line at a time, through
//! [`tokenize`], and keeps no token of a line once the line is read.
//!
//! A module is never refused. A procedure whose end is missing is closed where
//! its code stops, and the parser goes on. A procedure with a recovery is the
//! one that gets the trait `compile_error_suspected` (predicate
//! `parse_recovered`, [`Procedure::is_recovered`]); the recoveries outside any
//! procedure are on the [`ParsedModule`].

mod shape;

use std::ops::Range;

use crate::report::{ProcedureKind, Visibility};

use self::shape::{EndOf, Header, Shape, shape};
use crate::vba::{LogicalKind, LogicalLine, SourceLines, Token, TokenKind, tokenize};

/// The most recoveries kept for the module and for each procedure. The first
/// ones tell where the code stops parsing, which is what a reader looks for,
/// and a hostile text holds as many as it has lines.
pub const MAX_RECOVERIES: usize = 32;

/// What the parser had to step over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryKind {
    /// A procedure that has no `End Sub`, `End Function` or `End Property`:
    /// the header of another one, or the end of the module, closed it. Found at
    /// its header.
    MissingEnd,
    /// An `End Sub`, `End Function` or `End Property` that names another kind
    /// of procedure than the one it closes, such as an `End Function` after a
    /// `Sub`. Found at the `End`.
    WrongEnd,
    /// A terminator with nothing to close: an `End Sub`, `End Function` or
    /// `End Property` outside a procedure.
    StrayTerminator,
}

/// A place where the parser went on past what it could not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recovery {
    /// The physical line, from 1.
    pub line: u32,
    /// What was wrong.
    pub kind: RecoveryKind,
}

/// A procedure: `Sub`, `Function` or `Property`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Procedure {
    /// Its name as the source spells it, without brackets or type character.
    /// Empty when the header names nothing.
    pub name: String,
    /// `Sub`, `Function` or one of the three `Property`.
    pub kind: ProcedureKind,
    /// `Public`, `Private` or `Friend`; [`Visibility::Default`] when the header
    /// says none.
    pub visibility: Visibility,
    /// Whether the header says `Static`.
    pub is_static: bool,
    /// How many parameters the header declares. `ParamArray` counts for one.
    pub parameter_count: usize,
    /// The physical line of the `Sub`, `Function` or `Property` keyword.
    pub start_line: u32,
    /// The physical line of the `End` that closes it; of the last line of code
    /// when the procedure has no end.
    pub end_line: u32,
    /// Its logical lines, as indices into [`SourceLines::logical`]: the header
    /// to the `End`, comments between them included. The lines of a procedure
    /// with no end stop at its last line of code.
    pub lines: Range<usize>,
    /// What the parser stepped over inside it, the first [`MAX_RECOVERIES`].
    pub recoveries: Vec<Recovery>,
}

impl Procedure {
    /// The structural predicate `parse_recovered`: the parser stepped over
    /// something in the procedure.
    pub fn is_recovered(&self) -> bool {
        !self.recoveries.is_empty()
    }
}

/// What the parser makes of a module.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedModule {
    /// The procedures in source order, their lines never overlapping. A
    /// logical line that is in none of them stands outside any procedure.
    pub procedures: Vec<Procedure>,
    /// What the parser stepped over outside any procedure, the first
    /// [`MAX_RECOVERIES`].
    pub recoveries: Vec<Recovery>,
}

/// Reads the structure of a module from its logical lines. Never fails.
pub fn parse(source: &SourceLines) -> ParsedModule {
    let mut parser = Parser {
        source,
        module: ParsedModule::default(),
        open: None,
    };
    parser.run();
    parser.module
}

/// A procedure that is being read.
struct Open {
    header: Header,
    /// The logical line of the header.
    first: usize,
    start_line: u32,
    /// The last logical line of code read, and the physical line it ends on:
    /// where a procedure that has no end stops.
    last: usize,
    last_line: u32,
    recoveries: Vec<Recovery>,
}

impl Open {
    fn recover(&mut self, kind: RecoveryKind, line: u32) {
        if self.recoveries.len() < MAX_RECOVERIES {
            self.recoveries.push(Recovery { line, kind });
        }
    }
}

struct Parser<'a> {
    source: &'a SourceLines,
    module: ParsedModule,
    open: Option<Open>,
}

impl Parser<'_> {
    fn run(&mut self) {
        let source = self.source;
        for (index, line) in source.logical.iter().enumerate() {
            match line.kind {
                LogicalKind::CommentOnly => {}
                LogicalKind::Label => self.touch(index, line),
                LogicalKind::Code => {
                    let tokens = code_tokens(&line.text);
                    self.code(index, line, &tokens);
                }
            }
        }
        if self.open.is_some() {
            self.close_unended();
        }
    }

    fn code(&mut self, index: usize, line: &LogicalLine, tokens: &[Token]) {
        match shape(tokens, &line.text) {
            Shape::Header(header) => self.header(index, line, header),
            Shape::End(end) => self.end(index, line, end),
            Shape::Statement => self.touch(index, line),
        }
    }

    /// Notes that `line` is code of the procedure being read.
    fn touch(&mut self, index: usize, line: &LogicalLine) {
        if let Some(open) = self.open.as_mut() {
            open.last = index;
            open.last_line = line.end_line;
        }
    }

    /// Records a recovery in the procedure being read, or in the module.
    fn recover(&mut self, kind: RecoveryKind, line: u32) {
        match self.open.as_mut() {
            Some(open) => open.recover(kind, line),
            None if self.module.recoveries.len() < MAX_RECOVERIES => {
                self.module.recoveries.push(Recovery { line, kind });
            }
            None => {}
        }
    }

    fn header(&mut self, index: usize, line: &LogicalLine, header: Header) {
        if self.open.is_some() {
            self.close_unended();
        }
        let start_line = line.line_at(header.keyword);
        self.open = Some(Open {
            header,
            first: index,
            start_line,
            last: index,
            last_line: line.end_line,
            recoveries: Vec::new(),
        });
    }

    fn end(&mut self, index: usize, line: &LogicalLine, end: EndOf) {
        let Some(mut open) = self.open.take() else {
            self.recover(RecoveryKind::StrayTerminator, line.line);
            return;
        };
        if !end.closes(open.header.kind) {
            open.recover(RecoveryKind::WrongEnd, line.line);
        }
        self.finish(open, index, line.line);
    }

    /// Closes the procedure being read where its code stops.
    fn close_unended(&mut self) {
        if let Some(mut open) = self.open.take() {
            open.recover(RecoveryKind::MissingEnd, open.start_line);
            let (last, last_line) = (open.last, open.last_line);
            self.finish(open, last, last_line);
        }
    }

    fn finish(&mut self, open: Open, last: usize, end_line: u32) {
        self.module.procedures.push(Procedure {
            name: open.header.name,
            kind: open.header.kind,
            visibility: open.header.visibility,
            is_static: open.header.is_static,
            parameter_count: open.header.parameter_count,
            start_line: open.start_line,
            end_line,
            lines: open.first..last.saturating_add(1),
            recoveries: open.recoveries,
        });
    }
}

/// The tokens of the code of a logical line, without a trailing comment.
fn code_tokens(text: &str) -> Vec<Token> {
    let mut tokens = tokenize(text);
    if tokens
        .last()
        .is_some_and(|token| token.kind == TokenKind::Comment)
    {
        tokens.pop();
    }
    tokens
}
