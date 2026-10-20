// SPDX-License-Identifier: Apache-2.0
//! Physical lines, skipped lines and logical lines (SPEC M2 item 1).
//!
//! [`split_lines`] reads the text of one module as the file presents it. A
//! physical line keeps its number whatever happens to it: lines that are not
//! code (corpus header, export header, `Attribute` lines) are listed in
//! [`SourceLines::skipped`] and the parser never sees them, but they count.
//! The statements become logical lines, each carrying the physical line where
//! it starts: `_` continuations are joined, `:` separators cut a physical line
//! into statements, labels are told apart, and comments are cut off and kept
//! apart. Nothing here evaluates anything: the text is only cut.

use std::borrow::Cow;

use super::VbaError;
use super::scan::{
    bracket_len, date_literal_len, is_ident_char, is_ident_start, is_reserved, is_space,
    line_number_len, string_len,
};
use crate::report::Status;

/// The most lines that [`split_lines`] accepts, counted twice: the physical
/// lines of a text, and the logical lines it is cut into, since `:` makes many
/// of one physical line. A single statement chain with more `:` than that is
/// refused as well.
///
/// Splitting costs memory per line, not per byte: a text made of one-character
/// lines takes about 90 times its size (measured), which against the 64 MB
/// SPEC M1 lets through for the VBA part would break its 256 MB per file. At
/// this limit the lines of a module take about 45 MB. The longest module of
/// the corpus has under 4,000 lines. The limit is per module: a caller that
/// splits several modules bounds their sum itself.
pub const MAX_LINES: u32 = 250_000;

/// The error of a text of more than [`MAX_LINES`] physical or logical lines:
/// the file is too large to analyse, not corrupted.
fn too_many_lines() -> VbaError {
    VbaError::new(
        Status::TooLarge,
        "vba.too_many_lines",
        format!("more than {MAX_LINES} lines"),
    )
}

/// Why physical lines are not code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipKind {
    /// The provenance block at the top of a corpus sample.
    CorpusHeader,
    /// The `VERSION` line, the `Object` lines that name control libraries
    /// and the `Begin` ... `End` block (class settings or form designer) of
    /// an exported module.
    ExportHeader,
    /// `Attribute` lines: module-level, procedure-level and variable-level.
    Attribute,
}

/// A run of consecutive physical lines the parser skips.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedLines {
    /// First line of the run, 1-based.
    pub first: u32,
    /// Last line of the run, included.
    pub last: u32,
    /// Why they are skipped.
    pub kind: SkipKind,
}

/// An `Attribute` line, such as `Attribute VB_Name = "Module1"` or
/// `Attribute Foo.VB_Description = "..."`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    /// Physical line, 1-based.
    pub line: u32,
    /// The dotted name left of the `=`, as written: `VB_Name`,
    /// `Foo.VB_Description`.
    pub name: String,
    /// The literals that follow the `=`, as written and not interpreted, a
    /// trailing comment left out: `"Module1"`.
    pub value: String,
}

/// What a logical line holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalKind {
    /// Code: one statement, or the start of one that the parser reads on.
    Code,
    /// A statement label definition: [`LogicalLine::text`] is the label,
    /// without its colon (`ErrHandler`, or the line number `10`).
    Label,
    /// A comment alone: [`LogicalLine::text`] is empty.
    CommentOnly,
}

/// The comment of a logical line, kept for the `comment` matcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    /// Physical line where the comment starts.
    pub line: u32,
    /// The text after `'` or `Rem`, trimmed. A comment continued over
    /// several lines is joined with single spaces.
    pub text: String,
}

/// One statement of the module, with its continuations joined and its comment
/// cut off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalLine {
    /// Physical line where the text starts, 1-based. A line holding nothing
    /// but a continuation mark has no text, so the statement starts below it.
    pub line: u32,
    /// Last physical line the logical line occupies, its comment included.
    pub end_line: u32,
    /// What the line holds.
    pub kind: LogicalKind,
    /// The code, trimmed: continuations are joined with a single space and
    /// the comment is cut off. Strings are untouched.
    pub text: String,
    /// The comment that ends the physical line chain, if any. On a line cut
    /// at `:`, it belongs to the last statement.
    pub comment: Option<Comment>,
    /// True when another logical line of the same chain precedes this one: a
    /// statement before a `:`, or a label. A `:` that leads the chain has
    /// nothing to follow. After `If x Then` it tells a single-line `If`
    /// (`If x Then: y`) from a block.
    pub after_separator: bool,
    /// Offset in `text` and physical line of each line after the first.
    breaks: Vec<(usize, u32)>,
}

impl LogicalLine {
    /// The physical line of byte `offset` of [`text`](Self::text), for the
    /// lexer: a continuation moves it to the line the text came from.
    pub fn line_at(&self, offset: usize) -> u32 {
        line_before(&self.breaks, offset, self.line)
    }
}

/// The lines of one module.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceLines {
    /// Number of physical lines of the text.
    pub physical_lines: u32,
    /// The runs of physical lines that are not code, in order. A blank line
    /// is never a run of its own: it is skipped only inside a `Begin` block,
    /// and otherwise neither skipped nor logical.
    pub skipped: Vec<SkippedLines>,
    /// The `Attribute` lines, in order.
    pub attributes: Vec<Attribute>,
    /// The statements and the comments between them, in order.
    pub logical: Vec<LogicalLine>,
}

impl SourceLines {
    /// The first attribute called `name`, in any letter case. The name is the
    /// dotted name as written, such as `VB_Base` or `Foo.VB_Description`.
    pub fn attribute(&self, name: &str) -> Option<&Attribute> {
        self.attributes
            .iter()
            .find(|attribute| attribute.name.eq_ignore_ascii_case(name))
    }
}

/// Splits the text of a module into lines.
///
/// Physical lines end at CR LF, CR, LF, U+2028 or U+2029, and a final
/// terminator opens no line. A byte order mark at the start is not part of
/// the first line. Fails only on a text of more than [`MAX_LINES`] lines, with
/// the code `vba.too_many_lines`.
pub fn split_lines(text: &str) -> Result<SourceLines, VbaError> {
    let mut builder = Builder {
        rest: Physical::new(text),
        consumed: 0,
        too_many: false,
        out: SourceLines::default(),
        line_seps: Vec::new(),
        seps: Vec::new(),
    };
    builder.corpus_header();
    builder.body();
    if builder.too_many {
        return Err(too_many_lines());
    }
    builder.out.physical_lines = builder.consumed;
    Ok(builder.out)
}

/// The physical lines of a text, without their terminators.
#[derive(Clone)]
struct Physical<'a>(&'a str);

impl<'a> Physical<'a> {
    fn new(text: &'a str) -> Self {
        Self(text.strip_prefix('\u{feff}').unwrap_or(text))
    }
}

impl<'a> Iterator for Physical<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let text = self.0;
        if text.is_empty() {
            return None;
        }
        let (line, rest) = text
            .split_once(['\n', '\r', '\u{2028}', '\u{2029}'])
            .unwrap_or((text, ""));
        // CR LF is one terminator: the byte after the line tells which it was.
        let crlf = text.as_bytes().get(line.len()) == Some(&b'\r');
        self.0 = if crlf {
            rest.strip_prefix('\n').unwrap_or(rest)
        } else {
            rest
        };
        Some(line)
    }
}

/// The physical lines that continuation marks join, read as their code, which
/// `:` may cut into several statements, and, at the end, a comment.
struct Chain<'a> {
    /// The code of the physical lines, comment and continuation marks
    /// removed, joined with single spaces. A chain of one physical line
    /// borrows its code from the line instead of copying it.
    code: Cow<'a, str>,
    /// Offset in `code` of the code of each physical line, and its number.
    starts: Vec<(usize, u32)>,
    comment: Option<Comment>,
    last: u32,
}

/// What `scan` found in one physical line.
struct Scan {
    /// Where the code ends: at the comment, or at the continuation mark.
    code_end: usize,
    /// Where the text of the comment starts.
    comment_text: Option<usize>,
    /// Where the continuation mark `_` is, when the line is continued.
    marker: Option<usize>,
}

struct Builder<'a> {
    rest: Physical<'a>,
    consumed: u32,
    /// Whether the text has more than [`MAX_LINES`] lines.
    too_many: bool,
    out: SourceLines,
    /// Separators of the current physical line, scratch space.
    line_seps: Vec<usize>,
    /// Separators of the current chain, as offsets in its code.
    seps: Vec<usize>,
}

impl<'a> Builder<'a> {
    /// The next physical line and its number. Nothing once the text is over
    /// [`MAX_LINES`] lines.
    fn next_line(&mut self) -> Option<(u32, &'a str)> {
        if self.too_many {
            return None;
        }
        let line = self.rest.next()?;
        self.consumed = self.consumed.saturating_add(1);
        if self.consumed > MAX_LINES {
            self.too_many = true;
            return None;
        }
        Some((self.consumed, line))
    }

    /// Reads `count` lines without looking at them.
    fn discard(&mut self, count: u32) {
        for _ in 0..count {
            self.next_line();
        }
    }

    /// Records lines `first` to `last` as skipped, extending the previous run
    /// of the same kind when they touch it.
    fn mark_skipped(&mut self, first: u32, last: u32, kind: SkipKind) {
        match self.out.skipped.last_mut() {
            Some(run) if run.kind == kind && run.last.saturating_add(1) == first => run.last = last,
            _ => self.out.skipped.push(SkippedLines { first, last, kind }),
        }
    }

    /// The corpus header block (SPEC section 5): provenance lines at the top
    /// of the file, closed by a line holding a single `'` and nothing else.
    /// Without that closing line they are plain comments.
    fn corpus_header(&mut self) {
        let mut ahead = self.rest.clone();
        let mut provenance = 0u32;
        let closed = loop {
            match ahead.next() {
                Some(line) if is_provenance(line) => provenance = provenance.saturating_add(1),
                Some(line) => break provenance > 0 && line == "'",
                None => break false,
            }
        };
        if closed {
            let last = provenance.saturating_add(1);
            self.discard(last);
            self.mark_skipped(1, last, SkipKind::CorpusHeader);
        }
    }

    /// Classifies every line after the corpus header. The export header only
    /// exists before the first line of code, blank lines and comments aside;
    /// `Attribute` lines may come at any statement start.
    fn body(&mut self) {
        let mut code_seen = false;
        let mut may_open_block = true;
        while let Some((number, line)) = self.next_line() {
            let text = line.trim_matches(is_space);
            if text.is_empty() {
                continue;
            }
            if !code_seen && (is_version(text) || is_library_reference(text)) {
                self.mark_skipped(number, number, SkipKind::ExportHeader);
            } else if !code_seen && may_open_block && is_begin(text) {
                // A block that never closes hides nothing but its first line:
                // the rest is read as code, and no other block opens.
                let size = self.block_size();
                may_open_block = size.is_some();
                let size = size.unwrap_or(0);
                self.discard(size);
                self.mark_skipped(number, number.saturating_add(size), SkipKind::ExportHeader);
            } else if let Some(attribute) = parse_attribute(text, number) {
                self.mark_skipped(number, number, SkipKind::Attribute);
                self.out.attributes.push(attribute);
            } else {
                code_seen |= self.chain(number, line);
            }
        }
    }

    /// Number of lines after the `Begin` just read up to the `End` that closes
    /// it, nested `Begin` lines included. `None` when the block never closes,
    /// or when a line that a designer does not write comes first: a statement
    /// is not part of a header, and a VBA `End` among statements is not its
    /// end.
    fn block_size(&self) -> Option<u32> {
        let mut depth = 1u32;
        let mut size = 0u32;
        for line in self.rest.clone() {
            size = size.saturating_add(1);
            let text = line.trim_matches(is_space);
            if is_begin(text) {
                depth = depth.saturating_add(1);
            } else if text.eq_ignore_ascii_case("end") {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(size);
                }
            } else if !text.is_empty() && !is_designer_line(text) {
                return None;
            }
        }
        None
    }

    /// Reads the physical lines of the statement that starts at `first` and
    /// turns them into logical lines. True when one holds code.
    fn chain(&mut self, first: u32, line: &'a str) -> bool {
        let mut chain = Chain {
            code: Cow::default(),
            starts: Vec::new(),
            comment: None,
            last: first,
        };
        self.seps.clear();
        let mut at_start = true;
        let (mut number, mut line) = (first, line);
        loop {
            let continues = match chain.comment.as_mut() {
                // A comment continues over the line after a continuation mark.
                Some(comment) => {
                    let marker = marker(line);
                    let text = line.get(..marker.unwrap_or(line.len())).unwrap_or_default();
                    append(&mut comment.text, text.trim_matches(is_space));
                    marker.is_some()
                }
                None => self.read(&mut chain, number, line, &mut at_start),
            };
            chain.last = number;
            if !continues {
                break;
            }
            let Some((next, text)) = self.next_line() else {
                break;
            };
            (number, line) = (next, text);
        }
        self.emit(chain)
    }

    /// Adds the code and the comment of a physical line to `chain`, and its
    /// separators to the chain's. True when the line is continued.
    fn read(
        &mut self,
        chain: &mut Chain<'a>,
        number: u32,
        line: &'a str,
        at_start: &mut bool,
    ) -> bool {
        let numbered = chain.code.is_empty();
        let scan = scan(line, numbered, at_start, &mut self.line_seps);
        let code = line.get(..scan.code_end).unwrap_or_default();
        let body = code.trim_matches(is_space);
        if !body.is_empty() {
            let base = if chain.code.is_empty() {
                chain.code = Cow::Borrowed(body);
                0
            } else {
                let joined = chain.code.to_mut();
                joined.push(' ');
                let base = joined.len();
                joined.push_str(body);
                base
            };
            // The code is trimmed on its way into the chain: the separators
            // move with it.
            let indent = code
                .len()
                .saturating_sub(code.trim_start_matches(is_space).len());
            let moved = self
                .line_seps
                .iter()
                .map(|&sep| base.saturating_add(sep.saturating_sub(indent)));
            self.seps.extend(moved);
            self.too_many |= self.seps.len() > MAX_LINES as usize;
            chain.starts.push((base, number));
        }
        if let Some(start) = scan.comment_text {
            let text = line.get(start..scan.marker.unwrap_or(line.len()));
            chain.comment = Some(Comment {
                line: number,
                text: text.unwrap_or_default().trim_matches(is_space).to_owned(),
            });
        }
        scan.marker.is_some()
    }

    /// Adds a logical line, unless the text has [`MAX_LINES`] of them already.
    fn push(&mut self, line: LogicalLine) {
        if self.out.logical.len() >= MAX_LINES as usize {
            self.too_many = true;
        } else {
            self.out.logical.push(line);
        }
    }

    /// Cuts the code of `chain` at its separators and pushes one logical line
    /// per statement, a label and the statement after it counting as two.
    /// The comment goes to the last of them, or stands alone. True when one
    /// holds code.
    fn emit(&mut self, chain: Chain<'_>) -> bool {
        if self.too_many {
            return false;
        }
        let Chain {
            mut code,
            starts,
            comment,
            last,
        } = chain;
        let first = starts.first().map_or(last, |&(_, line)| line);
        let before = self.out.logical.len();
        let seps = std::mem::take(&mut self.seps);
        let ends = seps.iter().copied().chain(std::iter::once(code.len()));
        let mut begin = 0usize;
        for (index, end) in ends.enumerate() {
            let raw = code.get(begin..end).unwrap_or_default();
            let body = raw.trim_start_matches(is_space);
            let offset = begin.saturating_add(raw.len().saturating_sub(body.len()));
            begin = end.saturating_add(1);
            let statement = body.trim_end_matches(is_space);
            if statement.is_empty() {
                continue;
            }
            // A colon that leads the chain separates nothing: the statement it
            // introduces follows only what the chain already gave.
            let follows = self.out.logical.len() > before;
            let piece = |kind, text: String, offset, after_separator| {
                logical(kind, text, offset, after_separator, &starts, first)
            };
            // Only the first statement of a chain can be a label, and its
            // colon comes right after its name.
            let attached = index < seps.len() && !body.ends_with(is_space);
            let label = if index == 0 {
                label_len(statement, attached)
            } else {
                None
            };
            match label {
                Some(len) => {
                    let (name, rest) = statement.split_at_checked(len).unwrap_or_default();
                    self.push(piece(LogicalKind::Label, name.to_owned(), offset, false));
                    let remainder = rest.trim_start_matches(is_space);
                    if !remainder.is_empty() {
                        let gap = rest.len().saturating_sub(remainder.len());
                        let at = offset.saturating_add(len).saturating_add(gap);
                        let text = remainder.to_owned();
                        self.push(piece(LogicalKind::Code, text, at, true));
                    }
                }
                None => {
                    // Without a separator the whole code is the statement:
                    // an owned code moves, so that a huge line is not copied
                    // again.
                    let text = if seps.is_empty() {
                        std::mem::take(&mut code).into_owned()
                    } else {
                        statement.to_owned()
                    };
                    self.push(piece(LogicalKind::Code, text, offset, follows));
                }
            }
            if self.too_many {
                break;
            }
        }
        self.seps = seps;
        let code_found = self.out.logical.len() > before;
        if code_found {
            if let Some(line) = self.out.logical.last_mut() {
                line.end_line = last;
                line.comment = comment;
            }
        } else if let Some(comment) = comment {
            self.push(LogicalLine {
                line: comment.line,
                end_line: last,
                kind: LogicalKind::CommentOnly,
                text: String::new(),
                comment: Some(comment),
                after_separator: false,
                breaks: Vec::new(),
            });
        }
        code_found
    }
}

/// The logical line for the statement `text`, found at `offset` in the joined
/// code of a chain whose physical lines start at `starts`.
fn logical(
    kind: LogicalKind,
    text: String,
    offset: usize,
    after_separator: bool,
    starts: &[(usize, u32)],
    first: u32,
) -> LogicalLine {
    let end = offset.saturating_add(text.len());
    // The starts of the physical lines after the first one of the statement.
    let inside = starts.partition_point(|&(start, _)| start <= offset)
        ..starts.partition_point(|&(start, _)| start < end);
    LogicalLine {
        line: line_before(starts, offset, first),
        end_line: line_before(starts, end.saturating_sub(1), first),
        kind,
        text,
        comment: None,
        after_separator,
        breaks: starts
            .get(inside)
            .unwrap_or_default()
            .iter()
            .map(|&(start, line)| (start.saturating_sub(offset), line))
            .collect(),
    }
}

/// The line of the last entry of `marks`, sorted by offset, that starts at or
/// before `offset`, or `default` when none does.
fn line_before(marks: &[(usize, u32)], offset: usize, default: u32) -> u32 {
    let passed = marks.partition_point(|&(start, _)| start <= offset);
    passed
        .checked_sub(1)
        .and_then(|i| marks.get(i))
        .map_or(default, |&(_, line)| line)
}

/// Appends `text` to `joined`, after a space when there is already text.
fn append(joined: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    if !joined.is_empty() {
        joined.push(' ');
    }
    joined.push_str(text);
}

/// Where the continuation mark of `line` is: a `_` that ends the line, right
/// after whitespace (MS-VBAL `line-continuation`). A `_` that something
/// follows, or that touches a name, is not one.
fn marker(line: &str) -> Option<usize> {
    let before = line.strip_suffix('_')?;
    before.ends_with(is_space).then_some(before.len())
}

/// Reads `line` left to right: the statement separators go to `seps` (byte
/// offsets in `line`, at most [`MAX_LINES`] and one more), a comment ends the
/// code, and a `_` ends the line as a continuation unless it is inside a
/// string. In a string that never closes, the `_` is text and the next line a
/// statement of its own: joined to the unclosed string, its code would be
/// read as text.
///
/// `at_start` tells whether the statement being read has no token yet, which
/// is where `Rem` starts a comment; it carries over to a continuation line,
/// the mark of which is no token. `numbered` tells that the chain has no code
/// yet: a line number that opens `line` is a label, no token either, so `Rem`
/// may follow it.
fn scan(line: &str, numbered: bool, at_start: &mut bool, seps: &mut Vec<usize>) -> Scan {
    seps.clear();
    let mark = marker(line);
    let number = if numbered {
        let code = line.trim_start_matches(is_space);
        let indent = line.len().saturating_sub(code.len());
        indent..indent.saturating_add(line_number_len(code).unwrap_or(0))
    } else {
        0..0
    };
    let mut at = 0usize;
    let mut brackets = true;
    let mut open_string = false;
    let mut comment = None;
    while let Some(rest) = line.get(at..) {
        if mark == Some(at) {
            break;
        }
        let Some(c) = rest.chars().next() else { break };
        let mut step = c.len_utf8();
        let mut start = (is_space(c) || number.contains(&at)) && *at_start;
        match c {
            '"' => match string_len(rest) {
                Some(len) => step = len,
                None => {
                    step = rest.len();
                    open_string = true;
                }
            },
            '[' if brackets => match bracket_len(rest) {
                Some(len) => step = len,
                None => brackets = false,
            },
            // After a name or a closing bracket, `#` is a type character.
            '#' if !follows_name(line, at) => {
                step = date_literal_len(rest).unwrap_or(step);
            }
            '\'' => {
                comment = Some((at, at.saturating_add(1)));
                break;
            }
            'R' | 'r' if *at_start && is_rem(rest) => {
                comment = Some((at, at.saturating_add(3)));
                break;
            }
            // `:=` names an argument and separates nothing.
            ':' if !rest.starts_with(":=") => {
                seps.push(at);
                start = true;
                if seps.len() > MAX_LINES as usize {
                    break;
                }
            }
            _ => {}
        }
        *at_start = start;
        at = at.saturating_add(step);
    }
    let continued = mark.filter(|_| !open_string);
    Scan {
        code_end: comment.map_or(continued.unwrap_or(line.len()), |(start, _)| start),
        comment_text: comment.map(|(_, text)| text),
        marker: continued,
    }
}

/// Whether `rest` starts with the keyword `Rem` and not a longer name.
fn is_rem(rest: &str) -> bool {
    rest.get(..3)
        .is_some_and(|word| word.eq_ignore_ascii_case("rem"))
        && rest
            .get(3..)
            .and_then(|after| after.chars().next())
            .is_none_or(|c| !is_ident_char(c))
}

/// Whether the character before byte `at` of `line` ends a name or a closing
/// bracket, which makes a `#` right after it a type character, not the start
/// of a date.
fn follows_name(line: &str, at: usize) -> bool {
    line.get(..at)
        .and_then(|read| read.chars().next_back())
        .is_some_and(|c| is_ident_char(c) || c == ')' || c == ']')
}

/// Length of the label that opens `statement`, the first of a chain: a name
/// that is not a reserved word and whose colon follows it (`attached`), or a
/// line number, which needs no colon. What follows a line number is code.
fn label_len(statement: &str, attached: bool) -> Option<usize> {
    if let Some(digits) = line_number_len(statement) {
        return Some(digits);
    }
    let name = statement.starts_with(is_ident_start) && statement.chars().all(is_ident_char);
    (attached && name && !is_reserved(statement)).then_some(statement.len())
}

/// A provenance line of the corpus header: `' Source:`, `' Author:`,
/// `' License:`, `' Retrieved:`, `' Family:` or `' Synthetic:`.
fn is_provenance(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("' ") else {
        return false;
    };
    [
        "Source",
        "Author",
        "License",
        "Retrieved",
        "Family",
        "Synthetic",
    ]
    .iter()
    .any(|key| {
        rest.strip_prefix(key)
            .is_some_and(|after| after.starts_with(':'))
    })
}

/// `VERSION 5.00` or `VERSION 1.0 CLASS`, the first line of an export.
fn is_version(text: &str) -> bool {
    let digits = |part: Option<&str>| {
        part.is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
    };
    let mut words = text.split_ascii_whitespace();
    let keyword = words
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("version"));
    let number = words.next().is_some_and(|word| {
        let mut parts = word.split('.');
        digits(parts.next()) && digits(parts.next()) && parts.next().is_none()
    });
    let class = words
        .next()
        .is_none_or(|word| word.eq_ignore_ascii_case("class"));
    keyword && number && class && words.next().is_none()
}

/// The text after `keyword` when `text` starts with it, in any letter case.
/// What follows decides whether it is the keyword or the start of a longer
/// name.
fn after_keyword<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let head = text.get(..keyword.len())?;
    head.eq_ignore_ascii_case(keyword)
        .then(|| text.get(keyword.len()..))
        .flatten()
}

/// The text after the string literal that `text` starts with.
fn after_string(text: &str) -> Option<&str> {
    if !text.starts_with('"') {
        return None;
    }
    text.get(string_len(text)?..)
}

/// A name of the designer: letters, digits and `_`.
fn is_name(word: &str) -> bool {
    !word.is_empty() && word.chars().all(is_ident_char)
}

/// A `{GUID}`: hexadecimal digits and hyphens between braces.
fn is_guid(word: &str) -> bool {
    word.strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .is_some_and(|guid| {
            !guid.is_empty() && guid.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        })
}

/// The type of a designer block: a `{GUID}` or a dotted name.
fn is_designer_type(word: &str) -> bool {
    is_guid(word) || !word.is_empty() && word.chars().all(|c| is_ident_char(c) || c == '.')
}

/// A `Begin` line that opens a block of the export header: `BEGIN` alone
/// (class settings), or `Begin` with a type and at most a name, as in
/// `Begin {GUID} UserForm1` and `Begin MSForms.CommandButton Button1`. A
/// comment may follow. Anything else stays code: `Begin = 1` is an
/// assignment, and a statement is no header.
fn is_begin(text: &str) -> bool {
    let code = text.split_once('\'').map_or(text, |(code, _)| code);
    let mut words = code.split(is_space).filter(|word| !word.is_empty());
    words
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("begin"))
        && words.next().is_none_or(is_designer_type)
        && words.next().is_none_or(is_name)
        && words.next().is_none()
}

/// A `BeginProperty Name` line of a VB6 designer, with the `{GUID}` some of
/// them carry.
fn is_begin_property(text: &str) -> bool {
    let mut words = text.split(is_space).filter(|word| !word.is_empty());
    words
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("beginproperty"))
        && words.next().is_some_and(is_name)
        && words.next().is_none_or(is_guid)
        && words.next().is_none()
}

/// A property of a designer block: `Name = value`, whatever the value is.
fn is_property(text: &str) -> bool {
    text.split_once('=')
        .is_some_and(|(name, _)| is_name(name.trim_matches(is_space)))
}

/// A line a designer writes inside a block, `Begin` and `End` aside: a
/// property, the two lines around the properties of a VB6 designer, or a
/// comment.
fn is_designer_line(text: &str) -> bool {
    text.starts_with('\'')
        || text.eq_ignore_ascii_case("endproperty")
        || is_begin_property(text)
        || is_property(text)
}

/// `Object = "{GUID}#2.2#0"; "MSCOMCTL.OCX"`: the reference to a control
/// library that the export of a form carries before its `Begin` line.
fn is_library_reference(text: &str) -> bool {
    after_keyword(text, "object")
        .and_then(|rest| rest.trim_start_matches(is_space).strip_prefix('='))
        .and_then(|rest| after_string(rest.trim_start_matches(is_space)))
        .and_then(|rest| rest.trim_start_matches(is_space).strip_prefix(';'))
        .and_then(|rest| after_string(rest.trim_start_matches(is_space)))
        .is_some_and(|rest| rest.trim_matches(is_space).is_empty())
}

/// `Attribute Name = value`, where `Name` is a dotted name and the value a
/// list of literals, as VBA requires. A line that holds anything else after
/// the `=` is not an attribute line: it stays code, so that what it holds is
/// read.
fn parse_attribute(text: &str, line: u32) -> Option<Attribute> {
    let rest = after_keyword(text, "attribute")?
        .strip_prefix(is_space)?
        .trim_start_matches(is_space);
    let end = rest
        .find(|c: char| !(is_ident_char(c) || c == '.'))
        .unwrap_or(rest.len());
    let (name, after) = rest.split_at_checked(end)?;
    let value = literal_list(after.trim_start_matches(is_space).strip_prefix('=')?)?;
    (!name.is_empty()).then(|| Attribute {
        line,
        name: name.to_owned(),
        value: value.to_owned(),
    })
}

/// The literals of `text` as written, from the first to the last: strings,
/// numbers, `True` and `False`, separated by commas and optionally followed
/// by a comment. `None` for anything else, an empty list included.
fn literal_list(text: &str) -> Option<&str> {
    let text = text.trim_start_matches(is_space);
    let mut next = 0usize;
    loop {
        let rest = text.get(next..)?.trim_start_matches(is_space);
        let start = text.len().checked_sub(rest.len())?;
        let end = start.checked_add(literal_len(rest)?)?;
        let tail = text.get(end..)?.trim_start_matches(is_space);
        match tail.chars().next() {
            Some(',') => next = text.len().checked_sub(tail.len())?.checked_add(1)?,
            None | Some('\'') => return text.get(..end),
            Some(_) => return None,
        }
    }
}

/// Bytes of the literal that `rest` starts with.
fn literal_len(rest: &str) -> Option<usize> {
    if rest.starts_with('"') {
        return string_len(rest);
    }
    let len = rest
        .find(|c: char| is_space(c) || c == ',' || c == '\'')
        .unwrap_or(rest.len());
    let word = rest.get(..len)?;
    let number = word.starts_with(|c: char| c.is_ascii_digit() || "+-.&".contains(c))
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.&#%!@^".contains(c));
    (number || word.eq_ignore_ascii_case("true") || word.eq_ignore_ascii_case("false"))
        .then_some(len)
}
