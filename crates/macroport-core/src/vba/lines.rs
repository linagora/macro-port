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

use std::fmt;

use super::scan::{
    bracket_len, date_literal_len, is_ident_char, is_ident_start, is_reserved, is_space, string_len,
};

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

/// The text has more than [`MAX_LINES`] physical or logical lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooManyLines;

impl fmt::Display for TooManyLines {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "more than {MAX_LINES} lines")
    }
}

impl std::error::Error for TooManyLines {}

/// Why physical lines are not code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipKind {
    /// The provenance block at the top of a corpus sample.
    CorpusHeader,
    /// The `VERSION` line and the `Begin` ... `End` block (class settings or
    /// form designer) of an exported module.
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
    /// What follows the `=`, trimmed and not interpreted: `"Module1"`.
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
    /// First physical line, 1-based.
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
    /// True when a `:` separator, or a label, precedes this line on the same
    /// physical line chain. After `If x Then` it tells a single-line `If`
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
    /// The runs of physical lines that are not code, in order. Blank lines
    /// are neither skipped nor logical.
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
/// the first line. Fails only on a text of more than [`MAX_LINES`] lines.
pub fn split_lines(text: &str) -> Result<SourceLines, TooManyLines> {
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
        return Err(TooManyLines);
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

/// The physical lines of one statement, read as its code and, at the end, a
/// comment.
struct Chain {
    /// The code of the physical lines, comment and continuation marks
    /// removed, joined with single spaces.
    code: String,
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
    /// of the file, closed by a line holding only `'`. Without that closing
    /// line they are plain comments.
    fn corpus_header(&mut self) {
        let mut ahead = self.rest.clone();
        let mut provenance = 0u32;
        let closed = loop {
            match ahead.next() {
                Some(line) if is_provenance(line) => provenance = provenance.saturating_add(1),
                Some(line) => break provenance > 0 && line.trim_end_matches(is_space) == "'",
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
            if !code_seen && is_version(text) {
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
    /// it, nested `Begin` lines included.
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
            }
        }
        None
    }

    /// Reads the physical lines of the statement that starts at `first` and
    /// turns them into logical lines. True when one holds code.
    fn chain(&mut self, first: u32, line: &'a str) -> bool {
        let mut chain = Chain {
            code: String::new(),
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
    fn read(&mut self, chain: &mut Chain, number: u32, line: &str, at_start: &mut bool) -> bool {
        let scan = scan(line, at_start, &mut self.line_seps);
        let code = line.get(..scan.code_end).unwrap_or_default();
        let body = code.trim_matches(is_space);
        if !body.is_empty() {
            if !chain.code.is_empty() {
                chain.code.push(' ');
            }
            let base = chain.code.len();
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
            chain.code.push_str(body);
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
    fn emit(&mut self, chain: Chain) -> bool {
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
                    // it moves, so that a huge line is not copied again.
                    let text = if seps.is_empty() {
                        std::mem::take(&mut code)
                    } else {
                        statement.to_owned()
                    };
                    self.push(piece(LogicalKind::Code, text, offset, index > 0));
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

/// Where the continuation mark of `line` is: a final `_` after whitespace,
/// or alone. MS-VBAL forbids whitespace after the `_`; it is accepted.
fn marker(line: &str) -> Option<usize> {
    let before = line.trim_end_matches(is_space).strip_suffix('_')?;
    (before.is_empty() || before.ends_with(is_space)).then_some(before.len())
}

/// Reads `line` left to right: the statement separators go to `seps` (byte
/// offsets in `line`, at most [`MAX_LINES`] and one more), a comment ends the
/// code, and a `_` ends the line as a continuation unless it is inside a
/// string.
///
/// `at_start` tells whether the statement being read has no token yet, which
/// is where `Rem` starts a comment; it carries over to a continuation line.
fn scan(line: &str, at_start: &mut bool, seps: &mut Vec<usize>) -> Scan {
    seps.clear();
    let mut at = 0usize;
    let mut brackets = true;
    let mut open_string = false;
    let mut comment = None;
    while let Some(rest) = line.get(at..) {
        let Some(c) = rest.chars().next() else { break };
        let mut step = c.len_utf8();
        let mut start = is_space(c) && *at_start;
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
    let marker = marker(line).filter(|_| !open_string);
    Scan {
        code_end: comment.map_or(marker.unwrap_or(line.len()), |(start, _)| start),
        comment_text: comment.map(|(_, text)| text),
        marker,
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
    let digits = statement.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 {
        let rest = statement.get(digits..)?;
        return (rest.is_empty() || rest.starts_with(is_space)).then_some(digits);
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

/// A `Begin` line that opens a block of the export header: `Begin {GUID}
/// Name`, `Begin MSForms.CommandButton Name` or `BEGIN`, but not an
/// assignment to a variable called `Begin`.
fn is_begin(text: &str) -> bool {
    text.get(..5)
        .is_some_and(|word| word.eq_ignore_ascii_case("begin"))
        && text.get(5..).is_some_and(|rest| {
            rest.is_empty()
                || rest.starts_with(is_space) && !rest.trim_start_matches(is_space).starts_with('=')
        })
}

/// `Attribute Name = value`, where `Name` is a dotted name. A line with a
/// statement after the value is not an attribute line: it stays code, so that
/// the statement is read.
fn parse_attribute(text: &str, line: u32) -> Option<Attribute> {
    if !text.get(..9)?.eq_ignore_ascii_case("attribute") {
        return None;
    }
    let rest = text
        .get(9..)?
        .strip_prefix(is_space)?
        .trim_start_matches(is_space);
    let end = rest
        .find(|c: char| !(is_ident_char(c) || c == '.'))
        .unwrap_or(rest.len());
    let (name, after) = rest.split_at_checked(end)?;
    let value = after.trim_start_matches(is_space).strip_prefix('=')?;
    (!name.is_empty() && !has_separator(value)).then(|| Attribute {
        line,
        name: name.to_owned(),
        value: value.trim_matches(is_space).to_owned(),
    })
}

/// Whether `text` has a `:` outside strings and before any comment.
fn has_separator(text: &str) -> bool {
    let mut at = 0usize;
    while let Some(rest) = text.get(at..) {
        let Some(c) = rest.chars().next() else {
            return false;
        };
        let step = match c {
            ':' => return true,
            '\'' => return false,
            '"' => string_len(rest).unwrap_or(rest.len()),
            _ => c.len_utf8(),
        };
        at = at.saturating_add(step);
    }
    false
}
