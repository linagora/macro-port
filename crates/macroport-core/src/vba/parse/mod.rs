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
//!
//! Conditional compilation is not evaluated: the lines of every branch are
//! read. A procedure header that follows an `#Else` or an `#ElseIf` of the
//! `#If` that holds the latest header of the procedure being read is another
//! form of that procedure, whatever its name, as in `#If VBA7 Then`,
//! `Public Sub S(h As LongPtr)`, `#Else`, `Public Sub S(h As Long)`, `#End If`,
//! and then the body, which the two forms share. The `#If` of a form may stand
//! in a branch of another. The name, the kind and the parameters are the ones
//! of the first form; a form that names nothing is recovered all the same.
//!
//! The statements outside any procedure that declare or set something are
//! listed in [`ParsedModule::declarations`], in source order: `Option`,
//! `Declare`, `Const`, variables, `Type`, `Enum`, `Implements`, `Event`,
//! `WithEvents` and the conditional-compilation directives. A `Type` or an
//! `Enum` is one declaration up to its `End`, and its members declare nothing.
//! Any other statement outside a procedure, such as the `Set` of a sample cut
//! from a longer macro, is neither a declaration nor an error.

mod declaration;
mod shape;

use std::ops::Range;

use self::declaration::{block_end, declaration, ends};
use self::shape::{EndOf, Header, Shape, shape};
use super::{Keyword, LogicalKind, LogicalLine, SourceLines, Token, TokenKind, tokenize};
use crate::report::{ProcedureKind, Visibility};

pub use self::declaration::{Declaration, DeclarationKind, OptionKind};
pub use self::shape::Directive;

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
    /// A block that has no end. So far: a `Type` or an `Enum` with no
    /// `End Type` or `End Enum` before the header of a procedure or the end of
    /// the module. Found at its opener.
    UnclosedBlock,
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
    /// The physical line of the `Sub`, `Function` or `Property` keyword, of the
    /// first header when conditional compilation gives the procedure several.
    pub start_line: u32,
    /// The physical line of the `End` that closes it; of the last line of code
    /// when the procedure has no end.
    pub end_line: u32,
    /// Its logical lines, as indices into [`SourceLines::logical`]: the header
    /// to the `End`, comments and directives between them included. The lines
    /// of a procedure with no end stop at its last line of code.
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
    /// The declarations outside any procedure, in source order. A directive
    /// between the forms of a procedure is a line of the procedure.
    pub declarations: Vec<Declaration>,
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
        body: None,
        open_ifs: Vec::new(),
    };
    parser.run();
    parser.module
}

/// A procedure that is being read.
struct Open {
    header: Header,
    /// The logical line of the header.
    first: usize,
    /// The logical line of the latest header: the first one, or the last of the
    /// other forms read.
    latest: usize,
    /// The physical line of the keyword of the header.
    start_line: u32,
    /// The last logical line of code read, and the physical line it ends on:
    /// where a procedure that has no end stops.
    last: usize,
    last_line: u32,
    recoveries: Vec<Recovery>,
    /// Another form of the header is expected: an `#Else` or an `#ElseIf` came
    /// after the latest header, in the `#If` that holds it, and no header has
    /// followed yet.
    form_expected: bool,
}

impl Open {
    /// The procedure that `header` opens, on the logical line `index`. A header
    /// that names nothing opens one all the same, so that its `End` has
    /// something to close, and it is recovered.
    fn new(header: Header, index: usize, line: &LogicalLine) -> Self {
        let start_line = line.line_at(header.keyword_offset);
        let mut open = Self {
            start_line,
            header,
            first: index,
            latest: index,
            last: index,
            last_line: line.end_line,
            recoveries: Vec::new(),
            form_expected: false,
        };
        if open.header.name.is_empty() {
            open.recover(RecoveryKind::Unparseable, start_line);
        }
        open
    }

    /// Notes that `line`, the logical line `index`, is code of the procedure.
    fn touch(&mut self, index: usize, line: &LogicalLine) {
        self.last = index;
        self.last_line = line.end_line;
    }

    /// Reads `header`, on the logical line `index`, as another form of the
    /// procedure. A form that names nothing is recovered like a first one.
    fn form(&mut self, index: usize, line: &LogicalLine, header: &Header) {
        self.form_expected = false;
        self.latest = index;
        self.touch(index, line);
        if header.name.is_empty() {
            self.recover(
                RecoveryKind::Unparseable,
                line.line_at(header.keyword_offset),
            );
        }
    }

    /// An `#Else` or an `#ElseIf` of the `#If` on the logical line `frame`.
    /// When the latest header lies inside that `#If`, the header that follows
    /// is another form of the procedure.
    fn else_of(&mut self, frame: usize) {
        self.form_expected |= self.latest > frame;
    }

    /// The `#End If` of the `#If` on the logical line `frame`. When the latest
    /// header lies inside it, the forms of the header are all read.
    fn end_if_of(&mut self, frame: usize) {
        self.form_expected &= self.latest <= frame;
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

/// A `Type` or an `Enum` whose members are being read, outside any procedure.
struct Body {
    /// `DeclarationKind::Type` or `DeclarationKind::Enum`.
    kind: DeclarationKind,
    /// The word after the `End` that closes it.
    closer: Keyword,
    /// The logical line of its opener, and the physical line it starts on.
    first: usize,
    line: u32,
    /// The last logical line of code read in it.
    last: usize,
}

impl Body {
    /// The declaration that the block makes, up to the line read last.
    fn into_declaration(self) -> Declaration {
        Declaration {
            kind: self.kind,
            line: self.line,
            lines: self.first..self.last.saturating_add(1),
        }
    }
}

struct Parser<'a> {
    source: &'a SourceLines,
    module: ParsedModule,
    open: Option<Open>,
    /// The `Type` or the `Enum` being read, if there is one. Never together
    /// with a procedure: the header of a procedure ends it.
    body: Option<Body>,
    /// The logical lines of the `#If` directives that are not closed yet.
    open_ifs: Vec<usize>,
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
        self.close_unended_body();
    }

    /// Reads a line of code: a member of the `Type` or the `Enum` being read, a
    /// procedure header, an end, a directive, or a statement.
    fn code(&mut self, index: usize, line: &LogicalLine, tokens: &[Token]) {
        let found = shape(tokens, &line.text);
        if self.member(index, &found, tokens) {
            return;
        }
        match found {
            Shape::Header(header) => self.header(index, line, header),
            Shape::End(end) => self.end(index, line, end),
            Shape::Directive(directive) => {
                self.directive(index, directive);
                self.touch(index, line);
                self.declare(DeclarationKind::Directive(directive), index, line, tokens);
            }
            Shape::Statement => {
                self.touch(index, line);
                if let Some(kind) = declaration(tokens, &line.text) {
                    self.declare(kind, index, line, tokens);
                }
            }
        }
    }

    /// Takes the line as part of the `Type` or the `Enum` being read, if there
    /// is one: a member, or the `End` that closes the block. Whether it did.
    /// The header of a procedure is neither: the block has no end, and the
    /// header is read as what it is.
    fn member(&mut self, index: usize, found: &Shape, tokens: &[Token]) -> bool {
        if self.body.is_none() {
            return false;
        }
        if matches!(found, Shape::Header(_)) {
            self.close_unended_body();
            return false;
        }
        if let Some(body) = self.body.as_mut() {
            body.last = index;
            if ends(tokens, body.closer) {
                self.close_body();
            }
        }
        true
    }

    /// Lists the declaration `kind` that the statement makes, if it stands
    /// outside any procedure. A `Type` or an `Enum` only opens its block.
    fn declare(
        &mut self,
        kind: DeclarationKind,
        index: usize,
        line: &LogicalLine,
        tokens: &[Token],
    ) {
        if self.open.is_some() {
            return;
        }
        let start = tokens
            .first()
            .map_or(line.line, |token| line.line_at(token.start));
        match block_end(&kind) {
            Some(closer) => {
                self.body = Some(Body {
                    kind,
                    closer,
                    first: index,
                    line: start,
                    last: index,
                });
            }
            None => self.module.declarations.push(Declaration {
                kind,
                line: start,
                lines: index..index.saturating_add(1),
            }),
        }
    }

    /// Lists the block being read, up to the line read last.
    fn close_body(&mut self) {
        if let Some(body) = self.body.take() {
            self.module.declarations.push(body.into_declaration());
        }
    }

    /// Closes the block being read, if there is one, where its members stop.
    fn close_unended_body(&mut self) {
        if let Some(line) = self.body.as_ref().map(|body| body.line) {
            self.recover_in_module(RecoveryKind::UnclosedBlock, line);
            self.close_body();
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

    /// A procedure header. After the `#Else` of the `#If` that holds the latest
    /// header of the procedure being read, it is another form of that
    /// procedure, and the body is shared. Elsewhere the procedure still open
    /// has no end, and this header opens another one.
    fn header(&mut self, index: usize, line: &LogicalLine, header: Header) {
        if let Some(open) = self.open.as_mut().filter(|open| open.form_expected) {
            open.form(index, line, &header);
            return;
        }
        self.close_unended();
        self.open = Some(Open::new(header, index, line));
    }

    /// A conditional-compilation directive, on the logical line `index`. The
    /// `#If` blocks that hold the header of the procedure being read tell
    /// whether the next header is another form of it.
    fn directive(&mut self, index: usize, directive: Directive) {
        match directive {
            Directive::If => self.open_ifs.push(index),
            Directive::ElseIf | Directive::Else => {
                if let (Some(&frame), Some(open)) = (self.open_ifs.last(), self.open.as_mut()) {
                    open.else_of(frame);
                }
            }
            Directive::EndIf => {
                if let (Some(frame), Some(open)) = (self.open_ifs.pop(), self.open.as_mut()) {
                    open.end_if_of(frame);
                }
            }
            Directive::Const => {}
        }
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
