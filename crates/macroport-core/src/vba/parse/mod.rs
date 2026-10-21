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
//! procedure are on the [`ParsedModule`], which is recovered as a whole
//! ([`ParsedModule::is_recovered`]) when a procedure or the module has any.

mod shape;

use std::ops::Range;

use self::shape::{EndOf, Header, Shape, shape};
use super::{LogicalKind, LogicalLine, SourceLines, Token, TokenKind, tokenize};
use crate::report::{ProcedureKind, Visibility};

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
    /// A statement that does not parse as what it starts like. So far: the
    /// header of a procedure that names nothing, such as `Sub (a, b)` or
    /// `Sub 123()`. Found at the statement.
    Unparseable,
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
    /// Empty when the header names nothing, and the procedure is then
    /// recovered.
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

impl ParsedModule {
    /// The structural predicate `parse_recovered` for the file: the parser
    /// stepped over something in a procedure or outside any, such as an `End`
    /// with nothing to close.
    pub fn is_recovered(&self) -> bool {
        !self.recoveries.is_empty() || self.procedures.iter().any(Procedure::is_recovered)
    }
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
    /// The physical line of the keyword of the header.
    start_line: u32,
    /// The last logical line of code read, and the physical line it ends on:
    /// where a procedure that has no end stops.
    last: usize,
    last_line: u32,
    recoveries: Vec<Recovery>,
}

impl Open {
    /// The procedure that `header` opens, on the logical line `index`.
    fn new(header: Header, index: usize, line: &LogicalLine) -> Self {
        Self {
            start_line: line.line_at(header.keyword_offset),
            header,
            first: index,
            last: index,
            last_line: line.end_line,
            recoveries: Vec::new(),
        }
    }

    /// Notes that `line`, the logical line `index`, is code of the procedure.
    fn touch(&mut self, index: usize, line: &LogicalLine) {
        self.last = index;
        self.last_line = line.end_line;
    }

    /// Records a recovery, unless the procedure has [`MAX_RECOVERIES`] already.
    fn recover(&mut self, kind: RecoveryKind, line: u32) {
        if self.recoveries.len() < MAX_RECOVERIES {
            self.recoveries.push(Recovery { line, kind });
        }
    }

    /// The procedure, ending with the logical line `last` on the physical line
    /// `end_line`.
    fn into_procedure(self, last: usize, end_line: u32) -> Procedure {
        Procedure {
            name: self.header.name,
            kind: self.header.kind,
            visibility: self.header.visibility,
            is_static: self.header.is_static,
            parameter_count: self.header.parameter_count,
            start_line: self.start_line,
            end_line,
            lines: self.first..last.saturating_add(1),
            recoveries: self.recoveries,
        }
    }
}

struct Parser<'a> {
    source: &'a SourceLines,
    module: ParsedModule,
    open: Option<Open>,
}

impl Parser<'_> {
    /// Reads every logical line of the module, one at a time.
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
        self.close_unended();
    }

    /// Reads a line of code: a procedure header, an end, or a statement.
    fn code(&mut self, index: usize, line: &LogicalLine, tokens: &[Token]) {
        match shape(tokens, &line.text) {
            Shape::Header(header) => self.header(index, line, header),
            Shape::End(end) => self.end(index, line, end),
            Shape::Statement => self.touch(index, line),
        }
    }

    /// Notes that `line` is code of the procedure being read, if there is one.
    fn touch(&mut self, index: usize, line: &LogicalLine) {
        if let Some(open) = self.open.as_mut() {
            open.touch(index, line);
        }
    }

    /// Records a recovery outside any procedure.
    fn recover_in_module(&mut self, kind: RecoveryKind, line: u32) {
        if self.module.recoveries.len() < MAX_RECOVERIES {
            self.module.recoveries.push(Recovery { line, kind });
        }
    }

    /// A procedure header: the procedure still open has no end, and this
    /// header opens another one. A header that names nothing opens one all the
    /// same, so that its `End` has something to close, and it is recovered.
    fn header(&mut self, index: usize, line: &LogicalLine, header: Header) {
        self.close_unended();
        let mut open = Open::new(header, index, line);
        if open.header.name.is_empty() {
            open.recover(RecoveryKind::Unparseable, open.start_line);
        }
        self.open = Some(open);
    }

    /// An `End Sub`, `End Function` or `End Property`: it closes the procedure
    /// being read, whatever the kind of the procedure.
    fn end(&mut self, index: usize, line: &LogicalLine, end: EndOf) {
        let Some(mut open) = self.open.take() else {
            self.recover_in_module(RecoveryKind::StrayTerminator, line.line);
            return;
        };
        if !end.closes(open.header.kind) {
            open.recover(RecoveryKind::WrongEnd, line.line);
        }
        let procedure = open.into_procedure(index, line.line);
        self.module.procedures.push(procedure);
    }

    /// Closes the procedure being read, if there is one, where its code stops.
    fn close_unended(&mut self) {
        if let Some(mut open) = self.open.take() {
            open.recover(RecoveryKind::MissingEnd, open.start_line);
            let (last, last_line) = (open.last, open.last_line);
            let procedure = open.into_procedure(last, last_line);
            self.module.procedures.push(procedure);
        }
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
